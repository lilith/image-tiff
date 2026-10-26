//! Caller-supplied strip/tile decompressors (`Decoder::with_decompressor`).

mod synth;

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tiff::decoder::pluggable::{ChunkDecompressor, ChunkInfo, FileRead};
use tiff::decoder::Decoder;
use tiff::tags::{ByteOrder, CompressionMethod};
use tiff::{TiffError, TiffResult};

/// `ChunkInfo` fields recorded per call: width, height, samples, bits, max output length.
type Call = (u32, u32, u16, u16, usize);

/// A stand-in codec: bytes XORed with 0x5A. Records the `ChunkInfo` of each call.
#[derive(Default)]
struct Xor {
    calls: Mutex<Vec<Call>>,
    /// Bytes to drop from (negative) or append to (positive) every output.
    adjust: isize,
}

impl ChunkDecompressor for Xor {
    fn decompress(
        &self,
        data: &[u8],
        info: &ChunkInfo<'_>,
        _file: &mut dyn FileRead,
        out: &mut Vec<u8>,
    ) -> TiffResult<ByteOrder> {
        self.calls.lock().unwrap().push((
            info.width,
            info.height,
            info.samples,
            info.bits_per_sample,
            info.max_output_len,
        ));
        out.extend(data.iter().map(|b| b ^ 0x5A));
        if self.adjust < 0 {
            out.truncate(out.len() - self.adjust.unsigned_abs());
        } else {
            out.resize(out.len() + self.adjust as usize, 0);
        }
        Ok(info.byte_order)
    }
}

const XOR: u16 = 34887;

fn xor(data: &[u8]) -> Vec<u8> {
    data.iter().map(|b| b ^ 0x5A).collect()
}

fn decode(file: Vec<u8>, codec: Arc<Xor>) -> TiffResult<Vec<u8>> {
    let mut decoder =
        Decoder::open(Cursor::new(file))?.with_decompressor(CompressionMethod::Unknown(XOR), codec);
    decoder.next_image()?;
    let layout = decoder.image_buffer_layout()?;
    let mut buffer = vec![0u8; layout.complete_len];
    decoder.read_image_bytes(&mut buffer)?;
    Ok(buffer)
}

/// 16-bit planar RGB in tiles with border padding and the horizontal predictor: the custom
/// output goes through the predictor, byte order and tile assembly like built-in output.
#[test]
fn custom_codec_runs_through_the_pipeline() {
    let (width, height, tile) = (40usize, 20usize, 16usize);
    let planes: Vec<Vec<u16>> = (0..3)
        .map(|p| {
            (0..width * height)
                .map(|i| (i as u16).wrapping_mul(97).wrapping_add(p * 1000))
                .collect()
        })
        .collect();
    let mut chunks = Vec::new();
    for plane in &planes {
        let bytes: Vec<u8> = plane.iter().flat_map(|v| v.to_le_bytes()).collect();
        for t in synth::tile_chunky(&bytes, width, height, 2, tile, tile) {
            // Horizontal predictor on 16-bit samples, one sample per pixel in a plane.
            let mut predicted = t.clone();
            for row in predicted.chunks_mut(tile * 2) {
                let vals: Vec<u16> = row
                    .chunks(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                for (i, c) in row.chunks_mut(2).enumerate() {
                    let d = if i == 0 {
                        vals[0]
                    } else {
                        vals[i].wrapping_sub(vals[i - 1])
                    };
                    c.copy_from_slice(&d.to_le_bytes());
                }
            }
            chunks.push(xor(&predicted));
        }
    }
    let file = synth::image(width as u32, height as u32, 16, 3, XOR)
        .short(262, &[2])
        .short(284, &[2])
        .short(317, &[2])
        .short(322, &[tile as u16])
        .short(323, &[tile as u16])
        .tiles(chunks)
        .build();

    let codec = Arc::new(Xor::default());
    let decoded = decode(file, codec.clone()).unwrap();
    let expected: Vec<u8> = planes
        .iter()
        .flatten()
        .flat_map(|v| v.to_ne_bytes())
        .collect();
    assert_eq!(decoded, expected);

    let calls = codec.calls.lock().unwrap();
    // 3 tiles across, 2 down, 3 planes; every call sees the full stored tile.
    assert_eq!(calls.len(), 18);
    assert!(calls
        .iter()
        .all(|&c| c == (tile as u32, tile as u32, 1, 16, tile * tile * 2)));
}

fn strips_file(rows_per_strip: u32) -> (Vec<u8>, Vec<u8>) {
    let (width, height) = (10u32, 7u32);
    let pixels: Vec<u8> = (0..width * height).map(|i| (i * 13) as u8).collect();
    let strips = pixels
        .chunks((width * rows_per_strip) as usize)
        .map(xor)
        .collect();
    let file = synth::image(width, height, 8, 1, XOR)
        .short(262, &[1])
        .long(278, &[rows_per_strip])
        .strips(strips)
        .build();
    (file, pixels)
}

/// The last strip is shorter; the decompressor is told how many rows it holds.
#[test]
fn custom_codec_strip_heights() {
    let (file, pixels) = strips_file(3);
    let codec = Arc::new(Xor::default());
    assert_eq!(decode(file, codec.clone()).unwrap(), pixels);
    let heights: Vec<u32> = codec.calls.lock().unwrap().iter().map(|c| c.1).collect();
    assert_eq!(heights, [3, 3, 1]);
}

/// Output shorter than the chunk is an error, never zero-padded.
#[test]
fn short_custom_output_is_an_error() {
    let (file, _) = strips_file(7);
    let codec = Arc::new(Xor {
        adjust: -1,
        ..Default::default()
    });
    assert!(matches!(decode(file, codec), Err(TiffError::IoError(_))));
}

/// Output longer than the chunk can hold is rejected.
#[test]
fn oversized_custom_output_is_rejected() {
    let (file, _) = strips_file(7);
    let codec = Arc::new(Xor {
        adjust: 1,
        ..Default::default()
    });
    assert!(matches!(
        decode(file, codec),
        Err(TiffError::LimitsExceeded)
    ));
}

/// Without a registered decompressor the method stays unsupported; a registered one also
/// replaces a built-in codec.
#[test]
fn registration_is_per_method() {
    let (file, pixels) = strips_file(7);
    let mut decoder = Decoder::open(Cursor::new(file.clone())).unwrap();
    decoder.next_image().unwrap();
    let mut buffer = vec![0u8; pixels.len()];
    assert!(matches!(
        decoder.read_image_bytes(&mut buffer),
        Err(TiffError::UnsupportedError(_))
    ));

    // Register the stand-in codec for PackBits; the strips are XOR data, not PackBits, so only
    // the replacement can decode them.
    let mut bytes = file;
    let pos = bytes
        .windows(4)
        .position(|w| w == [0x03, 0x01, 0x03, 0x00])
        .expect("Compression entry");
    bytes[pos + 8..pos + 10].copy_from_slice(&0x8005u16.to_le_bytes());
    let mut decoder = Decoder::open(Cursor::new(bytes))
        .unwrap()
        .with_decompressor(CompressionMethod::PackBits, Arc::new(Xor::default()));
    decoder.next_image().unwrap();
    decoder.read_image_bytes(&mut buffer).unwrap();
    assert_eq!(buffer, pixels);
}

/// Records each chunk's index, plane and position; reads a key byte from a requested tag's file
/// offset and returns 16-bit samples in the byte order opposite to the host's.
#[derive(Default)]
struct Probe {
    seen: Mutex<Vec<(u32, u16, u32, u32)>>,
}

const KEY_TAG: u16 = 65000;

impl ChunkDecompressor for Probe {
    fn tags(&self) -> &[tiff::tags::Tag] {
        &[tiff::tags::Tag::Unknown(KEY_TAG)]
    }

    fn decompress(
        &self,
        data: &[u8],
        info: &ChunkInfo<'_>,
        file: &mut dyn FileRead,
        out: &mut Vec<u8>,
    ) -> TiffResult<ByteOrder> {
        self.seen
            .lock()
            .unwrap()
            .push((info.chunk_index, info.plane, info.x, info.y));
        let offset = info
            .tag(tiff::tags::Tag::Unknown(KEY_TAG))
            .unwrap()
            .clone()
            .into_u64()?;
        let key = file.read_at(offset, 1)?[0];
        // `data` holds little-endian u16 samples XORed with the key byte.
        let order = if ByteOrder::native() == ByteOrder::LittleEndian {
            ByteOrder::BigEndian
        } else {
            ByteOrder::LittleEndian
        };
        for pair in data.chunks(2) {
            let v = u16::from_le_bytes([pair[0] ^ key, pair[1] ^ key]);
            out.extend_from_slice(&match order {
                ByteOrder::BigEndian => v.to_be_bytes(),
                ByteOrder::LittleEndian => v.to_le_bytes(),
            });
        }
        Ok(order)
    }
}

/// Requested tags, file reads, the returned byte order and chunk positions, on a tiled planar
/// 16-bit image with border tiles.
#[test]
fn tags_file_reads_byte_order_and_positions() {
    let (width, height, tile) = (40usize, 20usize, 16usize);
    let planes: Vec<Vec<u16>> = (0..2)
        .map(|p| {
            (0..width * height)
                .map(|i| (i as u16) * 3 + p * 5000)
                .collect()
        })
        .collect();
    // The key byte is byte 4 of the file (the low byte of the first IFD offset), so build once
    // to learn it, then encode the tiles with it; the IFD offset does not depend on the tile
    // contents' values.
    let build = |key: u8| {
        let mut chunks = Vec::new();
        for plane in &planes {
            let bytes: Vec<u8> = plane.iter().flat_map(|v| v.to_le_bytes()).collect();
            for t in synth::tile_chunky(&bytes, width, height, 2, tile, tile) {
                chunks.push(t.iter().map(|b| b ^ key).collect::<Vec<u8>>());
            }
        }
        synth::image(width as u32, height as u32, 16, 2, XOR)
            .short(262, &[1])
            .short(284, &[2])
            .short(338, &[0])
            .short(322, &[tile as u16])
            .short(323, &[tile as u16])
            .long(KEY_TAG, &[4])
            .tiles(chunks)
            .build()
    };
    let key = build(0)[4];
    let file = build(key);
    assert_eq!(file[4], key);

    let probe = Arc::new(Probe::default());
    let mut decoder = Decoder::open(Cursor::new(file))
        .unwrap()
        .with_decompressor(CompressionMethod::Unknown(XOR), probe.clone());
    decoder.next_image().unwrap();
    let layout = decoder.image_buffer_layout().unwrap();
    let mut buffer = vec![0u8; layout.complete_len];
    decoder.read_image_bytes(&mut buffer).unwrap();
    let expected: Vec<u8> = planes
        .iter()
        .flatten()
        .flat_map(|v| v.to_ne_bytes())
        .collect();
    assert_eq!(buffer, expected);

    let mut seen = probe.seen.lock().unwrap().clone();
    seen.sort();
    let want: Vec<(u32, u16, u32, u32)> = (0..12u32)
        .map(|i| {
            let (plane, j) = ((i / 6) as u16, i % 6);
            (i, plane, (j % 3) * 16, (j / 3) * 16)
        })
        .collect();
    assert_eq!(seen, want);
}

/// Deflate (8 and 32946) through flate2, as a byte-stream codec.
#[cfg(feature = "deflate")]
struct CustomDeflate;

#[cfg(feature = "deflate")]
impl ChunkDecompressor for CustomDeflate {
    fn decompress(
        &self,
        data: &[u8],
        info: &ChunkInfo<'_>,
        _file: &mut dyn FileRead,
        out: &mut Vec<u8>,
    ) -> TiffResult<ByteOrder> {
        use std::io::Read;
        flate2::read::ZlibDecoder::new(data)
            .take(info.max_output_len as u64)
            .read_to_end(out)?;
        Ok(info.byte_order)
    }
}

/// JPEG (7) through zune-jpeg, merging JPEGTables and keeping the stored color samples, as
/// the contract asks.
#[cfg(feature = "jpeg")]
struct CustomJpeg;

#[cfg(feature = "jpeg")]
impl ChunkDecompressor for CustomJpeg {
    fn decompress(
        &self,
        data: &[u8],
        info: &ChunkInfo<'_>,
        _file: &mut dyn FileRead,
        out: &mut Vec<u8>,
    ) -> TiffResult<ByteOrder> {
        use zune_jpeg::zune_core;
        let stream = match info.jpeg_tables {
            Some(tables) if tables.len() >= 2 && data.len() >= 2 => {
                [&tables[..tables.len() - 2], &data[2..]].concat()
            }
            _ => data.to_vec(),
        };
        let mut decoder = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(stream));
        decoder.decode_headers().map_err(|e| {
            TiffError::FormatError(tiff::TiffFormatError::CompressedDataCorrupt(e.to_string()))
        })?;
        let mut options = zune_core::options::DecoderOptions::default();
        if let Some(colorspace) = decoder.input_colorspace() {
            options = options.jpeg_set_out_colorspace(colorspace);
        }
        decoder.set_options(options);
        *out = decoder.decode().map_err(|e| {
            TiffError::FormatError(tiff::TiffFormatError::CompressedDataCorrupt(e.to_string()))
        })?;
        Ok(info.byte_order)
    }
}

fn decode_file(
    data: &[u8],
    custom: Option<(CompressionMethod, Arc<dyn ChunkDecompressor>)>,
) -> TiffResult<Vec<u8>> {
    let mut decoder = Decoder::open(Cursor::new(data))?;
    if let Some((method, d)) = custom {
        decoder = decoder.with_decompressor(method, d);
    }
    decoder.next_image()?;
    let layout = decoder.image_buffer_layout()?;
    let mut buffer = vec![0u8; layout.complete_len];
    decoder.read_image_bytes(&mut buffer)?;
    Ok(buffer)
}

/// Custom Deflate and JPEG decompressors reproduce the built-in decoders' output on every
/// fixture in tests/ that uses those compressions (predictors, 16-bit and float samples,
/// tiles, YCbCr JPEG with shared tables).
#[cfg(all(feature = "deflate", feature = "jpeg"))]
#[test]
fn custom_codecs_match_built_in_on_fixtures() {
    let mut checked = Vec::new();
    for dir in ["tests/images", "tests/images/bigtiff", "tests/libtiffpic"] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            panic!("fixture directory {dir} missing");
        };
        let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            if !matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("tif" | "tiff")
            ) {
                continue;
            }
            let data = std::fs::read(&path).unwrap();
            let Ok(mut d) = Decoder::open(Cursor::new(&data[..])) else {
                continue;
            };
            if d.next_image().is_err() {
                continue;
            }
            let Ok(Some(c)) = d
                .current_ifd()
                .find_tag_unsigned::<u16>(tiff::tags::Tag::Compression)
            else {
                continue;
            };
            let custom: (CompressionMethod, Arc<dyn ChunkDecompressor>) = match c {
                8 | 32946 => (
                    CompressionMethod::from_u16_exhaustive(c),
                    Arc::new(CustomDeflate),
                ),
                7 => (CompressionMethod::ModernJPEG, Arc::new(CustomJpeg)),
                _ => continue,
            };
            let Ok(built_in) = decode_file(&data, None) else {
                continue;
            };
            let replaced = decode_file(&data, Some(custom)).unwrap();
            assert_eq!(replaced, built_in, "{}", path.display());
            checked.push(path.display().to_string());
        }
    }
    assert!(
        checked.len() >= 5,
        "only {} fixtures checked: {checked:?}",
        checked.len()
    );
    eprintln!("custom decompressors matched built-in on: {checked:?}");
}

/// Old-style JPEG (6) decoders upsample YCbCr themselves, as JPEG (7) decoders do: their full
/// resolution output is not run through the block upsampler.
#[test]
fn jpeg_family_custom_output_is_not_upsampled_again() {
    let (width, height) = (4u32, 4u32);
    let full: Vec<u8> = (0..width * height * 3).map(|i| (i * 11) as u8).collect();
    let file = synth::image(width, height, 8, 3, 6)
        .short(262, &[6])
        .short(530, &[2, 2])
        .long(278, &[height])
        .strips(vec![xor(&full)])
        .build();
    let mut decoder = Decoder::open(Cursor::new(file))
        .unwrap()
        .with_decompressor(CompressionMethod::JPEG, Arc::new(Xor::default()));
    decoder.next_image().unwrap();
    let mut buffer = vec![0u8; full.len()];
    decoder.read_image_bytes(&mut buffer).unwrap();
    assert_eq!(buffer, full);
}
