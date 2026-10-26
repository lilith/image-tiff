//! PixarLog (Compression 32909), following libtiff's tif_pixarlog.c.
//!
//! A chunk is a zlib stream of 11-bit log-encoded tokens, 16 bits each in the file's byte
//! order, horizontally differenced per row (samples per pixel apart). libtiff maps the tokens
//! through tables it builds in double precision to the format the bit depth implies: 8-bit and
//! 16-bit unsigned integers, or 32-bit floats. Samples come out in native byte order.

use crate::tags::{ByteOrder, SampleFormat};
use crate::{TiffError, TiffFormatError, TiffResult, TiffUnsupportedError};
use std::io::Read;
use std::sync::OnceLock;

/// Table size: one entry per 11-bit token, plus libtiff's slop entry.
const TSIZE: usize = 2048;
/// The token of 1.0.
const ONE: f64 = 1250.0;
/// The ratio between consecutive tokens in the logarithmic part.
const RATIO: f64 = 1.004;
const CODE_MASK: u16 = 0x7ff;

struct Tables {
    float: [f32; TSIZE + 1],
    u16: [u16; TSIZE + 1],
    u8: [u8; TSIZE + 1],
}

/// libtiff's `PixarLogMakeTables`: a linear segment up to about 0.0183, then a constant ratio.
fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let nlin = (1.0 / RATIO.ln()) as i32;
        let c = 1.0 / f64::from(nlin);
        let b = (-c * ONE).exp();
        let linstep = b * c * 1f64.exp();
        let mut float = [0f32; TSIZE + 1];
        for (i, v) in float.iter_mut().enumerate().take(TSIZE) {
            *v = if (i as i32) < nlin {
                (i as f64 * linstep) as f32
            } else {
                (b * (c * i as f64).exp()) as f32
            };
        }
        float[TSIZE] = float[TSIZE - 1];
        let mut u16_table = [0u16; TSIZE + 1];
        let mut u8_table = [0u8; TSIZE + 1];
        for i in 0..=TSIZE {
            let v = f64::from(float[i]) * 65535.0 + 0.5;
            u16_table[i] = if v > 65535.0 { 65535 } else { v as u16 };
            let v = f64::from(float[i]) * 255.0 + 0.5;
            u8_table[i] = if v > 255.0 { 255 } else { v as u8 };
        }
        Tables {
            float,
            u16: u16_table,
            u8: u8_table,
        }
    })
}

/// What a chunk holds.
pub(crate) struct PixarLogChunk {
    pub byte_order: ByteOrder,
    /// Samples per pixel in a row: SamplesPerPixel, or 1 for planar images.
    pub stride: usize,
    pub width: usize,
    pub rows: usize,
    pub bits_per_sample: u8,
    pub sample_format: SampleFormat,
}

enum Format {
    U8,
    U16,
    F32,
}

fn corrupt(message: &str) -> TiffError {
    TiffError::FormatError(TiffFormatError::CompressedDataCorrupt(format!(
        "PixarLog: {message}"
    )))
}

pub(crate) fn decode(
    compressed: &[u8],
    chunk: &PixarLogChunk,
    limit: usize,
) -> TiffResult<Vec<u8>> {
    // libtiff's PixarLogGuessDataFmt. The 11-bit and 12-bit formats come out as 16-bit words,
    // which this crate would read as packed samples; they are not taken.
    let format = match (chunk.bits_per_sample, chunk.sample_format) {
        (8, SampleFormat::Uint | SampleFormat::Void) => Format::U8,
        (16, SampleFormat::Uint | SampleFormat::Void) => Format::U16,
        (32, SampleFormat::IEEEFP) => Format::F32,
        _ => {
            return Err(TiffError::UnsupportedError(
                TiffUnsupportedError::UnsupportedBitsPerChannel(chunk.bits_per_sample),
            ))
        }
    };
    let row_len = chunk.stride * chunk.width;
    let samples = row_len
        .checked_mul(chunk.rows)
        .ok_or(TiffError::LimitsExceeded)?;
    let token_bytes = samples.checked_mul(2).ok_or(TiffError::LimitsExceeded)?;
    if token_bytes > limit {
        return Err(TiffError::LimitsExceeded);
    }
    let mut raw = Vec::with_capacity(token_bytes);
    flate2::read::ZlibDecoder::new(compressed)
        .take(token_bytes as u64)
        .read_to_end(&mut raw)
        .map_err(|e| corrupt(&e.to_string()))?;
    if raw.len() < token_bytes {
        return Err(corrupt("not enough data"));
    }
    let mut tokens: Vec<u16> = raw
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&b| match chunk.byte_order {
            ByteOrder::LittleEndian => u16::from_le_bytes(b),
            ByteOrder::BigEndian => u16::from_be_bytes(b),
        })
        .collect();

    // Undo the differencing: each sample adds the one a pixel to the left, modulo 2^16, and
    // the table lookup keeps the low 11 bits.
    if chunk.stride > 0 {
        for row in tokens.chunks_exact_mut(row_len.max(1)) {
            for i in chunk.stride..row.len() {
                row[i] = row[i].wrapping_add(row[i - chunk.stride]);
            }
        }
    }

    let tables = tables();
    let index = |t: u16| usize::from(t & CODE_MASK);
    let out = match format {
        Format::U8 => tokens.iter().map(|&t| tables.u8[index(t)]).collect(),
        Format::U16 => tokens
            .iter()
            .flat_map(|&t| tables.u16[index(t)].to_ne_bytes())
            .collect(),
        Format::F32 => tokens
            .iter()
            .flat_map(|&t| tables.float[index(t)].to_ne_bytes())
            .collect(),
    };
    Ok(out)
}
