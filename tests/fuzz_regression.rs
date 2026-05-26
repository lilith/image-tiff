//! Replay seed inputs from `fuzz/regression/` through every fuzz target
//! entry point. Shared scaffolding lives in `zen-fuzz-regress`.

use std::io::Cursor;
use zen_fuzz_regress::RegressionSuite;

fn restrictive_limits() -> tiff::decoder::Limits {
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = 512 * 1024;
    limits.ifd_value_size = 64 * 1024;
    limits.intermediate_buffer_size = 512 * 1024;
    limits
}

#[test]
fn fuzz_regression() {
    RegressionSuite::new("fuzz/regression")
        .target("decode_comprehensive", |data| {
            // Path 1: probe.
            let _ = tiff::decoder::TiffHeader::parse(Cursor::new(data));
            // Path 2: bounded full decode.
            let cursor = Cursor::new(data);
            let Ok(decoder) = tiff::decoder::Decoder::open(cursor) else {
                return;
            };
            let mut decoder = decoder.with_limits(restrictive_limits());
            let dims_ok = decoder.dimensions().is_ok();
            let _ = decoder.colortype();
            if dims_ok {
                let _ = decoder.get_chunk_type();
            }
        })
        .target("decode_image", |data| {
            let Ok(decoder) = tiff::decoder::Decoder::open(Cursor::new(data)) else {
                return;
            };
            let mut limits = tiff::decoder::Limits::default();
            limits.decoding_buffer_size = 1_000_000;
            limits.ifd_value_size = 1_000_000;
            limits.intermediate_buffer_size = 1_000_000;
            let mut decoder = decoder.with_limits(limits);
            loop {
                if !decoder.more_images() {
                    break;
                }
                if decoder.next_directory().is_err() {
                    break;
                }
                let _ = decoder.read_image();
            }
        })
        .run();
}
