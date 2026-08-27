//! What fraction of a TIFF decode is the horizontal predictor?
//!
//! The predictor is a per-channel prefix sum — serial by construction. A NEON
//! log-shift prototype measured 1.35-1.38x over the scalar loop, which is only
//! worth a SIMD dependency in this fork if the predictor is a meaningful share
//! of decode. Predictor-using TIFFs are LZW/deflate compressed and entropy
//! decoding is far more expensive per byte, so this measures the whole decode
//! to put that 1.38x in context rather than assuming it.
//!
//! Run: `cargo bench --bench predictor_share`

use std::fs;
use std::io::Cursor;
use tiff::decoder::Decoder;
use zenbench::prelude::*;

/// Encode a realistic 1920x1080 RGB8 image twice — with and without the
/// horizontal predictor, same compression — so decode times can be compared.
///
/// The predictor's whole purpose is to make the entropy coder's job easier, so
/// the two ALSO differ in compressed size. Both sizes are printed, because the
/// decode-time delta is predictor cost PLUS whatever the size change does to
/// decompression — it is not predictor cost alone, and reading it that way
/// would overstate the predictor.
fn encoded_pair(noise_shift: u32) -> (Vec<u8>, Vec<u8>) {
    use tiff::encoder::{colortype, Compression, TiffEncoder};
    use tiff::tags::Predictor;

    let (w, h) = (1920u32, 1080u32);
    // Smooth gradients plus light noise. A flat image would make the predictor
    // look free (everything compresses away); pure noise would make it look
    // useless. Real photographic content sits between.
    let mut px = vec![0u8; (w * h * 3) as usize];
    let mut s = 0x9e37_79b9u32;
    for y in 0..h {
        for x in 0..w {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let n = (s >> noise_shift) as u8;
            let i = ((y * w + x) * 3) as usize;
            px[i] = ((x * 255 / w) as u8).wrapping_add(n);
            px[i + 1] = ((y * 255 / h) as u8).wrapping_add(n);
            px[i + 2] = (((x + y) * 255 / (w + h)) as u8).wrapping_add(n);
        }
    }

    let mut out = Vec::new();
    for pred in [Predictor::None, Predictor::Horizontal] {
        let mut buf = Cursor::new(Vec::new());
        {
            let enc = TiffEncoder::new(&mut buf)
                .expect("encoder")
                .with_compression(Compression::Deflate(6))
                .with_predictor(pred);
            let mut enc = enc;
            enc.write_image::<colortype::RGB8>(w, h, &px)
                .expect("write_image");
        }
        out.push(buf.into_inner());
    }
    let b = out.pop().unwrap();
    let a = out.pop().unwrap();
    (a, b)
}

fn bench_decode(suite: &mut Suite) {
    let files = [
        "tests/images/quad-lzw-compat.tiff",
        "tests/images/issue_69_lzw.tiff",
        "tests/images/predictor-3-rgb-f32.tif",
        "tests/images/predictor-3-gray-f32.tif",
        "tests/images/hpredict-1c-12b.tiff",
    ];
    // The predictor's SHARE of decode depends entirely on how much work
    // deflate has to do, so bracket it: `noisy` is incompressible (deflate
    // emits stored blocks, so decompression is nearly free and the predictor
    // dominates), `smooth` compresses well (decompression dominates). Real
    // content sits between these two.
    for (content, shift) in [("noisy", 27u32), ("smooth", 31)] {
        let (none_tif, pred_tif) = encoded_pair(shift);
        eprintln!(
            "[predictor_share] {content}: 1920x1080 RGB8 deflate, raw 6220800 B -> \
             Predictor::None {} B, Predictor::Horizontal {} B",
            none_tif.len(),
            pred_tif.len()
        );
        for (label, data) in [("pred_none", none_tif), ("pred_horizontal", pred_tif)] {
            let data: &'static [u8] = Box::leak(data.into_boxed_slice());
            suite.compare(format!("decode_1920x1080_{content}/{label}"), |g| {
                g.throughput(Throughput::Bytes((1920 * 1080 * 3) as u64));
                g.bench("full", move |b| {
                    b.iter(move || {
                        let mut d = Decoder::new(Cursor::new(data)).expect("decoder");
                        d.read_image().expect("decode")
                    })
                });
            });
        }
    }

    for path in files {
        let Ok(bytes) = fs::read(path) else {
            eprintln!("[predictor_share] missing {path}, skipping");
            continue;
        };
        let bytes: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        // Some fixtures are deliberately malformed compat cases. Probe once
        // and report loudly rather than panicking mid-run or silently
        // measuring nothing.
        match Decoder::new(Cursor::new(bytes)).and_then(|mut d| d.read_image()) {
            Ok(_) => {}
            Err(e) => {
                eprintln!("[predictor_share] SKIPPING {path}: does not decode ({e:?})");
                continue;
            }
        }
        let name = path.rsplit('/').next().unwrap_or(path).to_string();
        suite.compare(format!("decode/{name}"), |g| {
            g.bench("full", move |b| {
                b.iter(move || {
                    let mut d = Decoder::new(Cursor::new(bytes)).expect("decoder");
                    d.read_image().expect("decode")
                })
            });
        });
    }
}

zenbench::main!(bench_decode);
