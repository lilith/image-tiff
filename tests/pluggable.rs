//! Caller-supplied strip/tile decompressors (`Decoder::with_decompressor`).

mod synth;

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use tiff::decoder::pluggable::{ChunkDecompressor, ChunkInfo};
use tiff::decoder::Decoder;
use tiff::tags::CompressionMethod;
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
    fn decompress(&self, data: &[u8], info: &ChunkInfo<'_>, out: &mut Vec<u8>) -> TiffResult<()> {
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
        Ok(())
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
