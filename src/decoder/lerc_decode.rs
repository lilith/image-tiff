//! LERC (Compression 34887), following libtiff's tif_lerc.c.
//!
//! A chunk is one LERC blob, optionally wrapped in zlib or Zstandard (LercParameters, tag 50674:
//! version, then 0 none / 1 deflate / 2 zstd). The blob holds the chunk at its stored size with
//! one band whose depth is the samples per chunk, except for two layouts libtiff also accepts:
//! 8-bit samples whose last extra sample is unassociated alpha, carried as the LERC mask
//! (alpha = 255 * mask), and float samples stored as one band per sample (liblerc 3+). Invalid
//! float pixels decode as NaN, as libtiff fills them. Samples come out in native byte order.

use crate::tags::SampleFormat;
use crate::{TiffError, TiffFormatError, TiffResult, TiffUnsupportedError};
use ::lerc::bitmask::BitMask;
use ::lerc::DataType;

/// What the image says about the chunk.
pub(crate) struct LercChunk<'a> {
    /// LercParameters, empty when absent.
    pub params: &'a [u32],
    pub sample_format: SampleFormat,
    pub bits_per_sample: u8,
    /// Samples per chunk: SamplesPerPixel, or 1 for planar images.
    pub samples: u16,
    /// Chunky image whose last extra sample is unassociated alpha.
    pub alpha_last: bool,
    pub width: u32,
    pub height: u32,
    pub limit: usize,
}

fn corrupt(message: &str) -> TiffError {
    TiffError::FormatError(TiffFormatError::CompressedDataCorrupt(format!(
        "LERC: {message}"
    )))
}

/// Native-endian bytes of the LERC sample types.
trait NeBytes: ::lerc::Sample {
    fn extend(values: &[Self], out: &mut Vec<u8>);
}

macro_rules! ne_bytes {
    ($($t:ty),*) => {$(
        impl NeBytes for $t {
            fn extend(values: &[Self], out: &mut Vec<u8>) {
                for v in values {
                    out.extend_from_slice(&v.to_ne_bytes());
                }
            }
        }
    )*};
}
ne_bytes!(i8, u8, i16, u16, i32, u32, f32, f64);

fn expected_type(format: SampleFormat, bits: u8) -> Option<DataType> {
    Some(match (format, bits) {
        (SampleFormat::Int, 8) => DataType::Char,
        (SampleFormat::Uint, 8) => DataType::Byte,
        (SampleFormat::Int, 16) => DataType::Short,
        (SampleFormat::Uint, 16) => DataType::UShort,
        (SampleFormat::Int, 32) => DataType::Int,
        (SampleFormat::Uint, 32) => DataType::UInt,
        (SampleFormat::IEEEFP, 32) => DataType::Float,
        (SampleFormat::IEEEFP, 64) => DataType::Double,
        _ => return None,
    })
}

/// Reads a whole decompression stream, failing once it exceeds `limit` bytes.
#[cfg(any(feature = "deflate", feature = "zstd", feature = "zstd-safe-rust"))]
fn read_bounded(stream: impl std::io::Read, limit: usize) -> TiffResult<Vec<u8>> {
    use std::io::Read;
    let mut out = Vec::new();
    stream.take(limit as u64 + 1).read_to_end(&mut out)?;
    if out.len() > limit {
        return Err(TiffError::LimitsExceeded);
    }
    Ok(out)
}

/// Undo the optional zlib / Zstandard wrapper.
#[cfg_attr(
    not(any(feature = "deflate", feature = "zstd", feature = "zstd-safe-rust")),
    allow(unused_variables)
)]
fn unwrap_blob(compressed: Vec<u8>, method: u32, limit: usize) -> TiffResult<Vec<u8>> {
    match method {
        0 => Ok(compressed),
        #[cfg(feature = "deflate")]
        1 => read_bounded(flate2::read::ZlibDecoder::new(&compressed[..]), limit),
        #[cfg(feature = "zstd")]
        2 => read_bounded(zstd::Decoder::new(&compressed[..])?, limit),
        #[cfg(all(feature = "zstd-safe-rust", not(feature = "zstd")))]
        2 => read_bounded(
            zrip_decode::streaming::FrameDecoder::new(&compressed[..]),
            limit,
        ),
        _ => Err(TiffError::UnsupportedError(
            TiffUnsupportedError::UnsupportedCompressionMethod(
                crate::tags::CompressionMethod::Unknown(34887),
            ),
        )),
    }
}

fn decode_values<T: NeBytes>(blob: &[u8], count: usize) -> TiffResult<(Vec<T>, Vec<BitMask>)> {
    let mut values = vec![T::default(); count];
    let result = ::lerc::decode_into(blob, &mut values).map_err(|e| corrupt(&e.to_string()))?;
    Ok((values, result.valid_masks))
}

fn valid(masks: &[BitMask], band: usize, pixel: usize) -> bool {
    masks
        .get(band)
        .or_else(|| masks.first())
        .is_none_or(|m| m.is_valid(pixel))
}

/// Decode one chunk into native-endian samples, `width * height * samples` of them.
pub(crate) fn decode(compressed: Vec<u8>, chunk: &LercChunk<'_>) -> TiffResult<Vec<u8>> {
    let method = chunk.params.get(1).copied().unwrap_or(0);
    let blob = unwrap_blob(compressed, method, chunk.limit)?;
    let info = ::lerc::decode_info(&blob).map_err(|e| corrupt(&e.to_string()))?;
    let data_type = expected_type(chunk.sample_format, chunk.bits_per_sample).ok_or(
        TiffError::UnsupportedError(TiffUnsupportedError::UnsupportedCompressionMethod(
            crate::tags::CompressionMethod::Unknown(34887),
        )),
    )?;
    if info.data_type != data_type {
        return Err(corrupt(
            "data type does not match SampleFormat/BitsPerSample",
        ));
    }
    if (info.width, info.height) != (chunk.width, chunk.height) {
        return Err(corrupt("blob size does not match the chunk"));
    }
    let pixels = chunk.width as usize * chunk.height as usize;
    let samples = usize::from(chunk.samples);
    let (depth, bands) = (info.depth as usize, info.bands as usize);
    let out_len = pixels
        .checked_mul(samples)
        .and_then(|n| n.checked_mul(usize::from(chunk.bits_per_sample / 8)))
        .ok_or(TiffError::LimitsExceeded)?;
    if out_len > chunk.limit {
        return Err(TiffError::LimitsExceeded);
    }
    let mut out = Vec::with_capacity(out_len);

    macro_rules! typed {
        ($t:ty, $nan:expr) => {{
            if bands == 1 && depth == samples {
                let (mut v, masks) = decode_values::<$t>(&blob, pixels * depth)?;
                if let Some(nan) = $nan {
                    for p in 0..pixels {
                        if !valid(&masks, 0, p) {
                            v[p * depth..(p + 1) * depth].fill(nan);
                        }
                    }
                }
                <$t>::extend(&v, &mut out);
            } else if bands == samples && depth == 1 && samples > 1 && $nan.is_some() {
                // One band per sample: interleave, NaN where that band's mask is invalid.
                let (v, masks) = decode_values::<$t>(&blob, pixels * bands)?;
                let mut interleaved = vec![<$t>::default(); pixels * bands];
                for b in 0..bands {
                    for p in 0..pixels {
                        let x = v[b * pixels + p];
                        interleaved[p * bands + b] = if valid(&masks, b, p) {
                            x
                        } else {
                            $nan.unwrap_or(x)
                        };
                    }
                }
                <$t>::extend(&interleaved, &mut out);
            } else {
                return Err(corrupt("band or depth layout does not match the image"));
            }
        }};
    }

    match data_type {
        DataType::Byte if chunk.alpha_last && bands == 1 && depth + 1 == samples => {
            // Alpha carried as the LERC mask.
            let (v, masks) = decode_values::<u8>(&blob, pixels * depth)?;
            for p in 0..pixels {
                out.extend_from_slice(&v[p * depth..(p + 1) * depth]);
                out.push(if valid(&masks, 0, p) { 255 } else { 0 });
            }
        }
        DataType::Char => typed!(i8, None::<i8>),
        DataType::Byte => typed!(u8, None::<u8>),
        DataType::Short => typed!(i16, None::<i16>),
        DataType::UShort => typed!(u16, None::<u16>),
        DataType::Int => typed!(i32, None::<i32>),
        DataType::UInt => typed!(u32, None::<u32>),
        DataType::Float => typed!(f32, Some(f32::from_bits(0x7fc0_0000))),
        DataType::Double => typed!(f64, Some(f64::from(f32::from_bits(0x7fc0_0000)))),
    }
    Ok(out)
}
