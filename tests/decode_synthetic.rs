//! Regression tests on TIFF files generated in the test, one per decoder defect. Each test
//! states the layout that triggered the defect and checks every decoded sample.

mod synth;

use std::io::Cursor;
use tiff::decoder::Decoder;

fn decode_all_planes(file: Vec<u8>) -> Vec<u8> {
    let mut decoder = Decoder::open(Cursor::new(file)).unwrap();
    decoder.next_image().unwrap();
    let layout = decoder.image_buffer_layout().unwrap();
    let mut buffer = vec![0u8; layout.complete_len];
    decoder.read_image_bytes(&mut buffer).unwrap();
    buffer
}

/// Tiled images with PlanarConfiguration = 2 store one tile grid per plane. Border tiles of
/// the second and later planes were sized as interior tiles (upstream issue 403), which
/// panicked while reading the image.
#[test]
fn tiled_planar_border_tiles() {
    let (width, height, tile) = (100usize, 72usize, 32usize);
    let planes: Vec<Vec<u8>> = (0..3)
        .map(|p| {
            (0..width * height)
                .map(|i| (i * 7 + p * 91) as u8)
                .collect()
        })
        .collect();
    let mut chunks = Vec::new();
    for plane in &planes {
        chunks.extend(synth::tile_chunky(plane, width, height, 1, tile, tile));
    }
    let file = synth::image(width as u32, height as u32, 8, 3, 1)
        .short(262, &[2])
        .short(284, &[2])
        .short(322, &[tile as u16])
        .short(323, &[tile as u16])
        .tiles(chunks)
        .build();

    let decoded = decode_all_planes(file);
    assert_eq!(decoded, planes.concat());
}

/// Floating-point predictor rows span the full tile width. Border tiles were de-interleaved
/// at the clipped data width, which corrupted the right-most tile column.
#[cfg(feature = "deflate")]
fn float_predictor_border_tiles(bytes_per_value: usize, samples: usize) {
    let (width, height, tile_w, tile_h) = (100usize, 40usize, 64usize, 32usize);
    let bpp = bytes_per_value * samples;
    // Distinct, non-trivial byte patterns in every byte plane.
    let pixels: Vec<u8> = (0..width * height * bpp)
        .map(|i| (i.wrapping_mul(2654435761) >> 7) as u8)
        .collect();
    let tiles: Vec<Vec<u8>> = synth::tile_chunky(&pixels, width, height, bpp, tile_w, tile_h)
        .into_iter()
        .map(|t| {
            let predicted: Vec<u8> = t
                .chunks(tile_w * bpp)
                .flat_map(|row| synth::float_predict_row(row, bytes_per_value, samples))
                .collect();
            synth::deflate(&predicted)
        })
        .collect();
    let photometric = if samples == 3 { 2 } else { 1 };
    let file = synth::image(
        width as u32,
        height as u32,
        (bytes_per_value * 8) as u16,
        samples as u16,
        8,
    )
    .short(262, &[photometric])
    .short(317, &[3])
    .short(339, &vec![3; samples])
    .short(322, &[tile_w as u16])
    .short(323, &[tile_h as u16])
    .tiles(tiles)
    .build();

    let decoded = decode_all_planes(file);
    // The decoder returns native-endian samples; the file is little-endian.
    let expected: Vec<u8> = pixels
        .chunks(bytes_per_value)
        .flat_map(|v| {
            let mut v = v.to_vec();
            if cfg!(target_endian = "big") {
                v.reverse();
            }
            v
        })
        .collect();
    assert_eq!(decoded, expected);
}

#[cfg(feature = "deflate")]
#[test]
fn float_predictor_border_tiles_f16_gray() {
    float_predictor_border_tiles(2, 1);
}

#[cfg(feature = "deflate")]
#[test]
fn float_predictor_border_tiles_f32_gray() {
    float_predictor_border_tiles(4, 1);
}

#[cfg(feature = "deflate")]
#[test]
fn float_predictor_border_tiles_f32_rgb() {
    float_predictor_border_tiles(4, 3);
}

#[cfg(feature = "deflate")]
#[test]
fn float_predictor_border_tiles_f64_gray() {
    float_predictor_border_tiles(8, 1);
}

/// PhotometricInterpretation is required, but GIS and scientific writers omit it (upstream
/// issue 253). One color sample decodes as BlackIsZero and three as RGB, as libtiff's RGBA
/// reader and GDAL do; other sample counts stay unsupported.
#[test]
fn missing_photometric_interpretation() {
    use tiff::ColorType;

    let gray: Vec<u8> = (0..20u32 * 6).map(|i| (i * 3) as u8).collect();
    let file = synth::image(20, 6, 8, 1, 1)
        .strips(vec![gray.clone()])
        .build();
    let mut decoder = Decoder::open(Cursor::new(file.clone())).unwrap();
    decoder.next_image().unwrap();
    assert_eq!(decoder.colortype().unwrap(), ColorType::Gray(8));
    assert_eq!(decode_all_planes(file), gray);

    let rgb: Vec<u8> = (0..20u32 * 6 * 3).map(|i| (i * 5) as u8).collect();
    let file = synth::image(20, 6, 8, 3, 1)
        .strips(vec![rgb.clone()])
        .build();
    let mut decoder = Decoder::open(Cursor::new(file.clone())).unwrap();
    decoder.next_image().unwrap();
    assert_eq!(decoder.colortype().unwrap(), ColorType::RGB(8));
    assert_eq!(decode_all_planes(file), rgb);

    // Two color samples have no conventional interpretation.
    let two: Vec<u8> = vec![0; 20 * 6 * 2];
    let file = synth::image(20, 6, 8, 2, 1).strips(vec![two]).build();
    let mut decoder = Decoder::open(Cursor::new(file)).unwrap();
    assert!(decoder.next_image().is_err());

    // A present but unknown value is still rejected.
    let file = synth::image(20, 6, 8, 1, 1)
        .short(262, &[999])
        .strips(vec![gray])
        .build();
    let mut decoder = Decoder::open(Cursor::new(file)).unwrap();
    assert!(decoder.next_image().is_err());
}

#[cfg(feature = "fax")]
fn ccitt_rows(width: usize, height: usize, seed: u32) -> Vec<Vec<bool>> {
    let mut state = seed.wrapping_mul(2654435761) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    (0..height)
        .map(|y| match y % 5 {
            0 => vec![false; width],
            1 => vec![true; width],
            _ => {
                let mut row = Vec::with_capacity(width);
                let max_run = [3, 40, 700][y % 3];
                let mut color = next() % 2 == 0;
                while row.len() < width {
                    let n = (next() % max_run + 1) as usize;
                    row.extend(std::iter::repeat_n(color, n.min(width - row.len())));
                    color = !color;
                }
                row
            }
        })
        .collect()
}

/// Encodes `rows` in strips of `rows_per_strip` with `encode`, decodes the file and checks
/// every pixel. Photometric is WhiteIsZero; the decoder normalizes it so a decoded 1 bit is
/// white. Bits past the row width are padding and are not compared.
#[cfg(feature = "fax")]
fn check_ccitt(
    compression: u16,
    t4_options: Option<u32>,
    fill_order: u16,
    rows: &[Vec<bool>],
    rows_per_strip: usize,
    encode: impl Fn(&[Vec<bool>]) -> Vec<u8>,
) {
    let width = rows[0].len();
    let strips: Vec<Vec<u8>> = rows
        .chunks(rows_per_strip)
        .map(|strip| {
            let mut data = encode(strip);
            if fill_order == 2 {
                data.iter_mut().for_each(|b| *b = b.reverse_bits());
            }
            data
        })
        .collect();
    let mut file = synth::image(width as u32, rows.len() as u32, 1, 1, compression)
        .short(262, &[0])
        .short(266, &[fill_order])
        .long(278, &[rows_per_strip as u32]);
    if let Some(options) = t4_options {
        file = file.long(292, &[options]);
    }
    let mut decoded = decode_all_planes(file.strips(strips).build());
    let row_bytes = width.div_ceil(8);
    if !width.is_multiple_of(8) {
        let keep = 0xffu8 << (8 - width % 8);
        for row in decoded.chunks_mut(row_bytes) {
            row[row_bytes - 1] &= keep;
        }
    }
    let white: Vec<Vec<bool>> = rows
        .iter()
        .map(|r| r.iter().map(|&b| !b).collect())
        .collect();
    assert_eq!(
        decoded,
        synth::ccitt::pack(&white),
        "compression {compression}, T4Options {t4_options:?}, FillOrder {fill_order}, width {width}"
    );
}

/// Group 3 rows coded two-dimensionally (T4Options bit 0) were decoded as one-dimensional
/// rows, producing wrong pixels; Modified Huffman (compression 2, upstream issue 405) was
/// unsupported.
#[cfg(feature = "fax")]
#[test]
fn ccitt_group3_and_modified_huffman() {
    for (i, &width) in [1usize, 13, 64, 300, 1728, 2600].iter().enumerate() {
        let rows = ccitt_rows(width, 11, i as u32 + 1);
        for fill_order in [1, 2] {
            for rows_per_strip in [11, 4] {
                check_ccitt(
                    2,
                    None,
                    fill_order,
                    &rows,
                    rows_per_strip,
                    synth::ccitt::modified_huffman,
                );
                for options in [0, 4] {
                    check_ccitt(3, Some(options), fill_order, &rows, rows_per_strip, |r| {
                        synth::ccitt::group3(r, options, 1)
                    });
                }
                for (options, k) in [(1, 2), (1, 4), (5, 4), (1, usize::MAX)] {
                    check_ccitt(3, Some(options), fill_order, &rows, rows_per_strip, |r| {
                        synth::ccitt::group3(r, options, k)
                    });
                }
            }
        }
    }
}

/// CCITT rows wider than 65,535 pixels (fax 0.3.0 widened its positions to u32).
#[cfg(feature = "fax")]
#[test]
fn ccitt_wider_than_u16() {
    let rows = ccitt_rows(70_001, 6, 77);
    check_ccitt(2, None, 1, &rows, 6, synth::ccitt::modified_huffman);
    check_ccitt(3, Some(0), 1, &rows, 6, |r| synth::ccitt::group3(r, 0, 1));
    check_ccitt(3, Some(1), 1, &rows, 3, |r| synth::ccitt::group3(r, 1, 2));
    // Group 4 is Group 3 two-dimensional coding without EOLs or 1D rows.
    check_ccitt(4, None, 1, &rows, 6, |r| {
        let mut encoder = fax34::encoder::Encoder::new(fax34::VecWriter::new());
        for row in r {
            let pels = row.iter().map(|&b| {
                if b {
                    fax34::Color::Black
                } else {
                    fax34::Color::White
                }
            });
            encoder.encode_line(pels, row.len() as u32).unwrap();
        }
        encoder.finish().unwrap().finish()
    });
}

/// Group 4 strips that end without EOFB, so the last row's codes end in the last bits of the
/// strip. The fax crate's bit reader could not match a code there and the last row was lost.
#[cfg(feature = "fax")]
#[test]
fn ccitt_group4_without_eofb() {
    for (i, &width) in [1usize, 5, 13, 40, 203].iter().enumerate() {
        for seed in 0..20 {
            let rows = ccitt_rows(width, 4, (i * 100 + seed) as u32);
            check_ccitt(4, None, 1, &rows, 4, |r| {
                synth::ccitt::group4_without_eofb(r)
            });
        }
    }
}

/// Group 3 data without EOL codes (libtiff's testfax3_bug54_1dnoEOL.tif is such a file):
/// rows are decoded back to back, as libtiff does when it finds no EOL in a strip.
#[cfg(feature = "fax")]
#[test]
fn ccitt_group3_without_eol() {
    for (i, &width) in [13usize, 300, 1728].iter().enumerate() {
        let rows = ccitt_rows(width, 11, 40 + i as u32);
        for rows_per_strip in [11, 4] {
            check_ccitt(3, Some(0), 1, &rows, rows_per_strip, |r| {
                synth::ccitt::group3_without_eol(r, 0, 1)
            });
            check_ccitt(3, Some(1), 1, &rows, rows_per_strip, |r| {
                synth::ccitt::group3_without_eol(r, 1, 2)
            });
        }
    }
}

/// Uncompressed mode (T4Options bit 1) is not implemented and must not decode as data.
#[cfg(feature = "fax")]
#[test]
fn ccitt_group3_uncompressed_mode_is_unsupported() {
    let rows = ccitt_rows(16, 4, 9);
    let file = synth::image(16, 4, 1, 1, 3)
        .short(262, &[0])
        .long(292, &[2])
        .strips(vec![synth::ccitt::group3(&rows, 0, 1)])
        .build();
    let mut decoder = Decoder::open(Cursor::new(file)).unwrap();
    decoder.next_image().unwrap();
    let layout = decoder.image_buffer_layout().unwrap();
    let mut buffer = vec![0u8; layout.complete_len];
    assert!(decoder.read_image_bytes(&mut buffer).is_err());
}

/// A BigTIFF directory claiming more than 65,535 entries is rejected, as libtiff does, instead
/// of being read entry by entry.
#[test]
fn bigtiff_directory_entry_count_is_bounded() {
    let mut file = b"II\x2b\x00\x08\x00\x00\x00".to_vec();
    file.extend_from_slice(&16u64.to_le_bytes());
    file.extend_from_slice(&70_000u64.to_le_bytes());
    file.resize(file.len() + 70_000 * 20 + 8, 0);
    let err = Decoder::open(Cursor::new(file)).and_then(|mut d| d.next_image().map(|_| d));
    assert!(
        matches!(
            err,
            Err(tiff::TiffError::FormatError(
                tiff::TiffFormatError::ImageFileDirectoryNotFound
            ))
        ),
        "{:?}",
        err.err()
    );
}

/// `Decoder::new` reads the first directory before `with_limits` can apply, so it keeps tiff
/// 0.11.3's behaviour: a first image with more than `ifd_value_size` (1 MiB) of strip offsets
/// opens. Here 262,145 one-row strips need 1 MiB + 4 bytes of offsets. `Decoder::open`
/// followed by `next_image` applies the default limit, and `with_limits` can raise it.
#[test]
fn first_directory_with_over_one_mib_of_offsets() {
    let rows = 262_145u32;
    let strips: Vec<Vec<u8>> = (0..rows).map(|i| vec![i as u8]).collect();
    let file = synth::image(1, rows, 8, 1, 1)
        .short(262, &[1])
        .long(278, &[1])
        .strips(strips)
        .build();

    #[allow(deprecated)]
    let mut decoder = Decoder::new(Cursor::new(file.clone())).unwrap();
    let mut buffer = vec![0u8; rows as usize];
    decoder.read_image_bytes(&mut buffer).unwrap();
    assert!(buffer.iter().enumerate().all(|(i, &b)| b == i as u8));

    let mut decoder = Decoder::open(Cursor::new(file.clone())).unwrap();
    assert!(matches!(
        decoder.next_image(),
        Err(tiff::TiffError::LimitsExceeded)
    ));

    let mut limits = tiff::decoder::Limits::default();
    limits.ifd_value_size = 2 << 20;
    let mut decoder = Decoder::open(Cursor::new(file))
        .unwrap()
        .with_limits(limits);
    decoder.next_image().unwrap();
}

/// LZW strips without the EndOfInformation code decode when their data is complete (upstream
/// PR 391); a truncated strip is an error, not zero-filled.
#[cfg(feature = "lzw")]
#[test]
fn lzw_without_end_of_information() {
    let pixels: Vec<u8> = (0..40u32 * 30).map(|i| (i * 7) as u8).collect();
    let half = pixels.len() / 2;
    for eoi in [true, false] {
        let file = synth::image(40, 30, 8, 1, 5)
            .short(262, &[1])
            .long(278, &[15])
            .strips(vec![
                synth::lzw_literal(&pixels[..half], eoi),
                synth::lzw_literal(&pixels[half..], eoi),
            ])
            .build();
        assert_eq!(decode_all_planes(file), pixels, "eoi {eoi}");
    }

    let mut truncated = synth::lzw_literal(&pixels, false);
    truncated.truncate(truncated.len() / 2);
    let file = synth::image(40, 30, 8, 1, 5)
        .short(262, &[1])
        .strips(vec![truncated])
        .build();
    let mut decoder = Decoder::open(Cursor::new(file)).unwrap();
    decoder.next_image().unwrap();
    let mut buffer = vec![0u8; pixels.len()];
    assert!(decoder.read_image_bytes(&mut buffer).is_err());
}

/// YCbCr with 2x2 chroma subsampling, uncompressed, in tiles whose right column is a border
/// tile: each tile stores whole blocks over its full stored width.
#[test]
fn ycbcr_subsampled_border_tiles() {
    let (width, height, tile) = (6usize, 4usize, 4usize);
    // Pixel (x, y): Y = 10 * y + x, Cb/Cr per 2x2 block.
    let y_at = |x: usize, y: usize| (10 * y + x) as u8;
    let cb_at = |bx: usize, by: usize| (100 + 10 * by + bx) as u8;
    let cr_at = |bx: usize, by: usize| (200 + 10 * by + bx) as u8;
    let mut tiles = Vec::new();
    for ty in (0..height).step_by(tile) {
        for tx in (0..width).step_by(tile) {
            let mut data = Vec::new();
            for by in (ty..ty + tile).step_by(2) {
                for bx in (tx..tx + tile).step_by(2) {
                    for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        data.push(y_at(bx + dx, by + dy));
                    }
                    data.push(cb_at(bx / 2, by / 2));
                    data.push(cr_at(bx / 2, by / 2));
                }
            }
            tiles.push(data);
        }
    }
    let file = synth::image(width as u32, height as u32, 8, 3, 1)
        .short(262, &[6])
        .short(530, &[2, 2])
        .short(322, &[tile as u16])
        .short(323, &[tile as u16])
        .tiles(tiles)
        .build();
    let expected: Vec<u8> = (0..height)
        .flat_map(|y| {
            (0..width).flat_map(move |x| [y_at(x, y), cb_at(x / 2, y / 2), cr_at(x / 2, y / 2)])
        })
        .collect();
    assert_eq!(decode_all_planes(file), expected);
}

/// Chunks with no stored bytes (GDAL's sparse tiles and strips) decode as stored samples of 0,
/// as in GDAL and tifffile, instead of reading whatever follows their offset.
#[test]
fn sparse_chunks_decode_as_zero() {
    // Two 16x16 tiles; the first is absent, so its offset points at the second tile's bytes.
    let file = synth::image(32, 16, 8, 1, 1)
        .short(262, &[1])
        .short(322, &[16])
        .short(323, &[16])
        .tiles(vec![vec![], vec![7u8; 256]])
        .build();
    let decoded = decode_all_planes(file);
    for (i, row) in decoded.chunks(32).enumerate() {
        assert_eq!(&row[..16], &[0u8; 16], "row {i}");
        assert_eq!(&row[16..], &[7u8; 16], "row {i}");
    }

    // Three strips of two rows, the middle one absent.
    let file = synth::image(4, 6, 8, 1, 1)
        .short(262, &[1])
        .long(278, &[2])
        .strips(vec![vec![1; 8], vec![], vec![3; 8]])
        .build();
    assert_eq!(
        decode_all_planes(file),
        [vec![1u8; 8], vec![0; 8], vec![3; 8]].concat()
    );
}

/// RGB with two unspecified extra samples and the horizontal predictor: the predictor runs over
/// all five samples, so it must be undone before the extra samples are dropped.
#[cfg(feature = "deflate")]
#[test]
fn predictor_with_dropped_extra_samples() {
    let (width, height, samples) = (9usize, 4usize, 5usize);
    let pixels: Vec<u8> = (0..width * height * samples)
        .map(|i| (i * 37 % 251) as u8)
        .collect();
    let predicted: Vec<u8> = pixels
        .chunks(width * samples)
        .flat_map(|row| {
            (0..row.len()).map(move |i| {
                if i < samples {
                    row[i]
                } else {
                    row[i].wrapping_sub(row[i - samples])
                }
            })
        })
        .collect();
    let file = synth::image(width as u32, height as u32, 8, samples as u16, 8)
        .short(262, &[2])
        .short(317, &[2])
        .short(338, &[0, 0])
        .strips(vec![synth::deflate(&predicted)])
        .build();
    let expected: Vec<u8> = pixels
        .chunks(samples)
        .flat_map(|p| p[..3].to_vec())
        .collect();
    assert_eq!(decode_all_planes(file), expected);
}

/// ThunderScan (32809): raw pixels, 2-bit and 3-bit deltas and runs, including a run that starts
/// on an odd pixel and ends on an odd count; a row without enough data is an error, as in libtiff.
#[test]
fn thunderscan() {
    // Row 0: raw 5; deltas +1, -1, skip; deltas +3, -3; run of 2 -> 5 6 5 8 5 5 5.
    // Row 1: raw 3; run of 4 (from an odd pixel); raw 9; raw 1 -> 3 3 3 3 3 9 1.
    let data = vec![
        0xc5,
        0x40 | (1 << 4) | (3 << 2) | 2,
        0x80 | (3 << 3) | 5,
        0x02, //
        0xc3,
        0x04,
        0xc9,
        0xc1,
    ];
    let file = synth::image(7, 2, 4, 1, 32809)
        .short(262, &[1])
        .strips(vec![data.clone()])
        .build();
    assert_eq!(
        decode_all_planes(file),
        [0x56, 0x58, 0x55, 0x50, 0x33, 0x33, 0x39, 0x10]
    );

    let short = synth::image(7, 2, 4, 1, 32809)
        .short(262, &[1])
        .strips(vec![data[..6].to_vec()])
        .build();
    let mut decoder = Decoder::open(Cursor::new(short)).unwrap();
    decoder.next_image().unwrap();
    let mut buffer = vec![0u8; 8];
    assert!(decoder.read_image_bytes(&mut buffer).is_err());
}

/// One xz stream with libtiff's filter chain: Delta at the sample size, then LZMA2, no check.
fn xz(data: &[u8], delta: u32) -> Vec<u8> {
    use std::io::Write;
    let mut options = lzma_rust2::XzOptions::with_preset(6);
    options.set_check_sum_type(lzma_rust2::CheckType::None);
    options.prepend_pre_filter(lzma_rust2::FilterType::Delta, delta);
    let mut writer = lzma_rust2::XzWriter::new(Vec::new(), options).unwrap();
    writer.write_all(data).unwrap();
    writer.finish().unwrap()
}

/// LZMA (34925) strips and tiles, 8- and 16-bit, with and without the horizontal predictor.
#[test]
fn lzma() {
    let (width, height) = (29usize, 11usize);
    let gray16: Vec<u16> = (0..width * height)
        .map(|i| (i * 2311 % 65521) as u16)
        .collect();
    let bytes16: Vec<u8> = gray16.iter().flat_map(|v| v.to_le_bytes()).collect();
    let file = synth::image(width as u32, height as u32, 16, 1, 34925)
        .short(262, &[1])
        .long(278, &[4])
        .strips(bytes16.chunks(width * 2 * 4).map(|s| xz(s, 2)).collect())
        .build();
    let expected: Vec<u8> = gray16.iter().flat_map(|v| v.to_ne_bytes()).collect();
    assert_eq!(decode_all_planes(file), expected);

    let rgb: Vec<u8> = (0..width * height * 3)
        .map(|i| (i * 7 % 253) as u8)
        .collect();
    let tiles: Vec<Vec<u8>> = synth::tile_chunky(&rgb, width, height, 3, 16, 16)
        .into_iter()
        .map(|t| {
            let predicted: Vec<u8> = t
                .chunks(16 * 3)
                .flat_map(|row| {
                    (0..row.len()).map(move |i| {
                        if i < 3 {
                            row[i]
                        } else {
                            row[i].wrapping_sub(row[i - 3])
                        }
                    })
                })
                .collect();
            xz(&predicted, 1)
        })
        .collect();
    let file = synth::image(width as u32, height as u32, 8, 3, 34925)
        .short(262, &[2])
        .short(317, &[2])
        .short(322, &[16])
        .short(323, &[16])
        .tiles(tiles)
        .build();
    assert_eq!(decode_all_planes(file), rgb);
}

/// One LERC blob holding `depth` interleaved samples per pixel in each of `bands` bands, with
/// one validity mask per band (None: all valid).
fn lerc_blob<T: lerc::Sample>(
    width: usize,
    height: usize,
    depth: u32,
    bands: u32,
    data: &[T],
    masks: Option<Vec<lerc::bitmask::BitMask>>,
) -> Vec<u8> {
    let masks = masks.unwrap_or_else(|| {
        (0..bands)
            .map(|_| lerc::bitmask::BitMask::all_valid(width * height))
            .collect()
    });
    lerc::encode_borrowed(
        width as u32,
        height as u32,
        depth,
        bands,
        data,
        &masks,
        None,
        lerc::Precision::Lossless,
    )
    .unwrap()
}

/// LERC (34887) as libtiff writes and reads it: one band whose depth is the samples per chunk,
/// optionally zlib-wrapped (LercParameters [4, 1]); float pixels the mask marks invalid read as
/// NaN; float data stored one band per sample; 8-bit unassociated alpha carried as the mask.
/// Checked against libtiff 4.7.2 on the parity matrix (`parity/`).
#[test]
#[cfg(feature = "deflate")]
fn lerc() {
    let (width, height) = (29usize, 11usize);

    // u16 gray strips of 4 rows, plain and zlib-wrapped.
    let gray16: Vec<u16> = (0..width * height)
        .map(|i| (i * 2311 % 65521) as u16)
        .collect();
    let expected: Vec<u8> = gray16.iter().flat_map(|v| v.to_ne_bytes()).collect();
    for wrapper in [0u32, 1] {
        let strips = gray16
            .chunks(width * 4)
            .map(|rows| {
                let blob = lerc_blob(width, rows.len() / width, 1, 1, rows, None);
                if wrapper == 1 {
                    synth::deflate(&blob)
                } else {
                    blob
                }
            })
            .collect();
        let file = synth::image(width as u32, height as u32, 16, 1, 34887)
            .short(262, &[1])
            .long(278, &[4])
            .long(50674, &[4, wrapper])
            .strips(strips)
            .build();
        assert_eq!(decode_all_planes(file), expected, "wrapper {wrapper}");
    }

    // f32 RGB tiles with invalid pixels, depth 3 in one band.
    let tile = 16usize;
    let rgb: Vec<f32> = (0..width * height * 3)
        .map(|i| (i * 37 % 1009) as f32 / 7.0)
        .collect();
    let invalid = |x: usize, y: usize| (x + 2 * y).is_multiple_of(5);
    let mut expected = rgb.clone();
    for y in 0..height {
        for x in 0..width {
            if invalid(x, y) {
                expected[(y * width + x) * 3..][..3].fill(f32::from_bits(0x7fc0_0000));
            }
        }
    }
    let expected: Vec<u8> = expected.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let rgb_bytes: Vec<u8> = rgb.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let tiles_of = |interleaved: bool| -> Vec<Vec<u8>> {
        synth::tile_chunky(&rgb_bytes, width, height, 12, tile, tile)
            .into_iter()
            .enumerate()
            .map(|(t, bytes)| {
                let (tx, ty) = (
                    t % width.div_ceil(tile) * tile,
                    t / width.div_ceil(tile) * tile,
                );
                let values: Vec<f32> = bytes
                    .chunks(4)
                    .map(|b| f32::from_ne_bytes(b.try_into().unwrap()))
                    .collect();
                let mut mask = lerc::bitmask::BitMask::all_valid(tile * tile);
                for k in 0..tile * tile {
                    if invalid(tx + k % tile, ty + k / tile) {
                        mask.set_invalid(k);
                    }
                }
                if interleaved {
                    lerc_blob(tile, tile, 3, 1, &values, Some(vec![mask]))
                } else {
                    let planes: Vec<f32> = (0..3)
                        .flat_map(|s| values.iter().skip(s).step_by(3).copied())
                        .collect();
                    lerc_blob(tile, tile, 1, 3, &planes, Some(vec![mask; 3]))
                }
            })
            .collect()
    };
    for interleaved in [true, false] {
        let file = synth::image(width as u32, height as u32, 32, 3, 34887)
            .short(262, &[2])
            .short(339, &[3, 3, 3])
            .short(322, &[tile as u16])
            .short(323, &[tile as u16])
            .long(50674, &[4, 0])
            .tiles(tiles_of(interleaved))
            .build();
        assert_eq!(
            decode_all_planes(file),
            expected,
            "interleaved {interleaved}"
        );
    }

    // u8 RGBA with unassociated alpha: the blob holds RGB, the mask holds alpha != 0.
    let rgba: Vec<u8> = (0..width * height * 4)
        .map(|i| match i % 4 {
            3 => {
                if (i / 4) % 3 == 0 {
                    0
                } else {
                    255
                }
            }
            _ => (i * 7 % 253) as u8,
        })
        .collect();
    let colors: Vec<u8> = rgba.chunks(4).flat_map(|p| p[..3].to_vec()).collect();
    let mut mask = lerc::bitmask::BitMask::all_valid(width * height);
    for (k, p) in rgba.chunks(4).enumerate() {
        if p[3] == 0 {
            mask.set_invalid(k);
        }
    }
    let file = synth::image(width as u32, height as u32, 8, 4, 34887)
        .short(262, &[2])
        .short(338, &[2])
        .long(278, &[height as u32])
        .long(50674, &[4, 0])
        .strips(vec![lerc_blob(
            width,
            height,
            3,
            1,
            &colors,
            Some(vec![mask]),
        )])
        .build();
    // LERC keeps no samples for masked pixels, so the colour under alpha 0 reads as 0 too.
    let expected: Vec<u8> = rgba
        .chunks(4)
        .flat_map(|p| {
            if p[3] == 0 {
                [0; 4]
            } else {
                [p[0], p[1], p[2], p[3]]
            }
        })
        .collect();
    assert_eq!(decode_all_planes(file), expected);
}

/// A baseline JPEG split the way old-style JPEG TIFFs store it.
#[cfg(feature = "jpeg")]
struct JpegParts {
    /// The whole stream.
    stream: Vec<u8>,
    /// SOI up to the end of the SOS header.
    header: Vec<u8>,
    /// Quantization tables by id, 64 bytes each.
    q: Vec<(u8, Vec<u8>)>,
    /// Huffman tables by class and id: 16 counts, then the values.
    huffman: Vec<(u8, Vec<u8>)>,
    /// The entropy-coded data between restart markers.
    intervals: Vec<Vec<u8>>,
}

#[cfg(feature = "jpeg")]
fn jpeg_parts(stream: Vec<u8>) -> JpegParts {
    let (mut q, mut huffman) = (Vec::new(), Vec::new());
    let mut pos = 2;
    loop {
        assert_eq!(stream[pos], 0xFF);
        let marker = stream[pos + 1];
        let len = usize::from(u16::from_be_bytes([stream[pos + 2], stream[pos + 3]]));
        let body = &stream[pos + 4..pos + 2 + len];
        match marker {
            0xDB => {
                for table in body.chunks(65) {
                    q.push((table[0], table[1..].to_vec()));
                }
            }
            0xC4 => {
                let mut rest = body;
                while !rest.is_empty() {
                    let values: usize = rest[1..17].iter().map(|&c| usize::from(c)).sum();
                    huffman.push((rest[0], rest[1..17 + values].to_vec()));
                    rest = &rest[17 + values..];
                }
            }
            _ => {}
        }
        pos += 2 + len;
        if marker == 0xDA {
            break;
        }
    }
    let header = stream[..pos].to_vec();
    let mut intervals = vec![Vec::new()];
    let mut i = pos;
    while i < stream.len() {
        if stream[i] == 0xFF && (0xD0..=0xD7).contains(&stream[i + 1]) {
            intervals.push(Vec::new());
            i += 2;
        } else if stream[i] == 0xFF && stream[i + 1] == 0xD9 {
            break;
        } else {
            intervals.last_mut().unwrap().push(stream[i]);
            i += 1;
        }
    }
    JpegParts {
        stream,
        header,
        q,
        huffman,
        intervals,
    }
}

/// zune-jpeg's decode of a JPEG stream, in the stream's own color space.
#[cfg(feature = "jpeg")]
fn zune_decode(stream: &[u8]) -> Vec<u8> {
    use zune_jpeg::zune_core;
    let mut decoder =
        zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(stream.to_vec()));
    decoder.decode_headers().unwrap();
    let colorspace = decoder.input_colorspace().unwrap();
    decoder.set_options(
        zune_core::options::DecoderOptions::default().jpeg_set_out_colorspace(colorspace),
    );
    decoder.decode().unwrap()
}

/// Old-style JPEG (6) in the three layouts libtiff reads: the tables in JPEGQTables,
/// JPEGDCTables and JPEGACTables with one restart interval per strip; the header in the
/// JPEGInterchangeFormat stream with the strips holding the scan; and the whole JPEG in that
/// stream. Gray, 4:4:4, 4:2:2 and a 4:2:0 tile wider and taller than the image. Each must
/// decode as zune-jpeg decodes the original stream (libtiff 4.7.2 agrees on the luma of its
/// five old-style JPEG test files to within 1).
#[test]
#[cfg(feature = "jpeg")]
fn old_style_jpeg() {
    let (width, height) = (40usize, 32usize);
    let pixel =
        |x: usize, y: usize, c: usize| ((x * 7 + y * 5 + c * 60 + (x * y) % 13) % 256) as u8;

    // (samples, sampling factors, rows per strip, tiled)
    let cases = [
        (1usize, (1u8, 1u8), 8usize, false),
        (3, (1, 1), 8, false),
        (3, (2, 1), 8, false),
        (3, (2, 2), 32, false),
        (3, (2, 2), 48, true),
    ];
    for (samples, (sub_h, sub_v), rows, tiled) in cases {
        // Tiles are 48x48: the JPEG covers the whole padded tile.
        let (enc_w, enc_h) = if tiled { (48, 48) } else { (width, height) };
        let pixels: Vec<u8> = (0..enc_w * enc_h * samples)
            .map(|i| pixel(i / samples % enc_w, i / samples / enc_w, i % samples))
            .collect();
        let mut stream = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut stream, 90);
        let color = if samples == 1 {
            jpeg_encoder::ColorType::Luma
        } else {
            encoder.set_sampling_factor(
                jpeg_encoder::SamplingFactor::from_factors(sub_h, sub_v).unwrap(),
            );
            jpeg_encoder::ColorType::Ycbcr
        };
        let strips = enc_h.div_ceil(rows);
        if strips > 1 {
            let mcus = enc_w.div_ceil(8 * usize::from(sub_h)) * (rows / (8 * usize::from(sub_v)));
            encoder.set_restart_interval(mcus as u16);
        }
        encoder
            .encode(&pixels, enc_w as u16, enc_h as u16, color)
            .unwrap();
        let parts = jpeg_parts(stream);
        assert_eq!(parts.intervals.len(), strips);

        let decoded = zune_decode(&parts.stream);
        let expected: Vec<u8> = decoded
            .chunks(enc_w * samples)
            .take(height)
            .flat_map(|row| row[..width * samples].to_vec())
            .collect();

        let photometric = if samples == 1 { 1 } else { 6 };
        let base = || {
            let file = synth::image(width as u32, height as u32, 8, samples as u16, 6)
                .short(262, &[photometric])
                .short(512, &[1]);
            let file = if samples == 3 {
                file.short(530, &[u16::from(sub_h), u16::from(sub_v)])
            } else {
                file
            };
            if tiled {
                file.short(322, &[enc_w as u16]).short(323, &[enc_h as u16])
            } else {
                file.long(278, &[rows as u32])
            }
        };
        let chunks = |file: synth::Tiff, chunks: Vec<Vec<u8>>| {
            if tiled {
                file.tiles(chunks)
            } else {
                file.strips(chunks)
            }
        };

        // Tables in tags: each component points at its own table or repeats the previous one.
        let mut data = Vec::new();
        let mut table_offsets = |tables: Vec<Vec<u8>>| -> Vec<u32> {
            tables
                .into_iter()
                .map(|t| {
                    let at = synth::DATA_OFFSET + data.len() as u32;
                    data.extend_from_slice(&t);
                    at
                })
                .collect()
        };
        let by_component = |tables: &[(u8, Vec<u8>)], class: u8| -> Vec<Vec<u8>> {
            (0..samples)
                .map(|c| {
                    let id = if c == 0 { 0 } else { 1 };
                    let table = tables.iter().find(|t| t.0 == class | id).unwrap();
                    table.1.clone()
                })
                .collect()
        };
        let dedup = |offsets: Vec<u32>| -> Vec<u32> {
            // Components 1 and 2 share a table: point both at the first copy.
            offsets
                .iter()
                .enumerate()
                .map(|(c, &o)| if c == 2 { offsets[1] } else { o })
                .collect()
        };
        let q = dedup(table_offsets(by_component(&parts.q, 0)));
        let dc = dedup(table_offsets(by_component(&parts.huffman, 0)));
        let ac = dedup(table_offsets(by_component(&parts.huffman, 16)));
        let file = chunks(
            base()
                .data(data)
                .long(519, &q)
                .long(520, &dc)
                .long(521, &ac),
            parts.intervals.clone(),
        );
        assert_eq!(
            decode_all_planes(file.build()),
            expected,
            "tables {samples} {sub_h}x{sub_v}"
        );

        // The header in the interchange-format stream, the scan in the strips.
        let file = chunks(
            base()
                .data(parts.header.clone())
                .long(513, &[synth::DATA_OFFSET])
                .long(514, &[parts.header.len() as u32]),
            parts.intervals.clone(),
        );
        assert_eq!(
            decode_all_planes(file.build()),
            expected,
            "header {samples} {sub_h}x{sub_v}"
        );

        // The whole JPEG in the interchange-format stream (one strip, which repeats the scan).
        if strips == 1 {
            let file = chunks(
                base()
                    .data(parts.stream.clone())
                    .long(513, &[synth::DATA_OFFSET])
                    .long(514, &[parts.stream.len() as u32]),
                parts.intervals.clone(),
            );
            assert_eq!(
                decode_all_planes(file.build()),
                expected,
                "whole {samples} {sub_h}x{sub_v}"
            );
        }
    }
}

/// PixarLog's companding curve, re-derived from libtiff's PixarLogMakeTables.
#[cfg(feature = "deflate")]
fn pixarlog_linear(token: u16) -> f32 {
    let nlin = (1.0 / 1.004f64.ln()) as i32;
    let c = 1.0 / f64::from(nlin);
    let b = (-c * 1250.0).exp();
    let linstep = b * c * std::f64::consts::E;
    if i32::from(token) < nlin {
        (f64::from(token) * linstep) as f32
    } else {
        (b * (c * f64::from(token)).exp()) as f32
    }
}

/// PixarLog (32909): zlib-compressed 11-bit log tokens, 16 bits each in the file's byte order,
/// differenced along each row a pixel apart, read as 8-bit, 16-bit or float samples. Checked
/// against libtiff 4.7.2 on the parity matrix.
#[test]
#[cfg(feature = "deflate")]
fn pixarlog() {
    let (width, height) = (13usize, 6usize);
    for samples in [1usize, 3, 4] {
        let tokens: Vec<u16> = (0..width * height * samples)
            .map(|i| ((i * 211 + i / samples * 37) % 2048) as u16)
            .collect();
        let strips: Vec<Vec<u8>> = tokens
            .chunks(width * samples * 4)
            .map(|rows| {
                let differenced: Vec<u8> = rows
                    .chunks(width * samples)
                    .flat_map(|row| {
                        (0..row.len()).map(move |i| {
                            if i < samples {
                                row[i]
                            } else {
                                row[i].wrapping_sub(row[i - samples]).wrapping_add(4096)
                            }
                        })
                    })
                    .flat_map(u16::to_le_bytes)
                    .collect();
                synth::deflate(&differenced)
            })
            .collect();
        let photometric = if samples == 1 { 1 } else { 2 };
        let file = |bits: u16, format: u16| {
            let file = synth::image(width as u32, height as u32, bits, samples as u16, 32909)
                .short(262, &[photometric])
                .short(339, &vec![format; samples])
                .long(278, &[4]);
            let file = if samples == 4 {
                file.short(338, &[2])
            } else {
                file
            };
            file.strips(strips.clone()).build()
        };

        let linear: Vec<f32> = tokens.iter().map(|&t| pixarlog_linear(t)).collect();
        let to_int = |v: f32, max: f64| {
            let v = f64::from(v) * max + 0.5;
            if v > max {
                max
            } else {
                v.trunc()
            }
        };
        let expected8: Vec<u8> = linear.iter().map(|&v| to_int(v, 255.0) as u8).collect();
        assert_eq!(
            decode_all_planes(file(8, 1)),
            expected8,
            "8-bit, {samples} samples"
        );
        let expected16: Vec<u8> = linear
            .iter()
            .flat_map(|&v| (to_int(v, 65535.0) as u16).to_ne_bytes())
            .collect();
        assert_eq!(
            decode_all_planes(file(16, 1)),
            expected16,
            "16-bit, {samples} samples"
        );
        let expected32: Vec<u8> = linear.iter().flat_map(|v| v.to_ne_bytes()).collect();
        assert_eq!(
            decode_all_planes(file(32, 3)),
            expected32,
            "float, {samples} samples"
        );
    }

    // Anchors: token 0 is black, 1250 is 1.0, the top of the curve clamps.
    let tokens = [0u16, 1250, 2047];
    let file = synth::image(3, 1, 8, 1, 32909)
        .short(262, &[1])
        .strips(vec![synth::deflate(
            &[tokens[0], tokens[1] - tokens[0], tokens[2] - tokens[1]]
                .iter()
                .flat_map(|t| t.to_le_bytes())
                .collect::<Vec<u8>>(),
        )])
        .build();
    assert_eq!(decode_all_planes(file), [0, 255, 255]);
}
