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
