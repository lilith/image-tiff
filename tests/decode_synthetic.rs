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
