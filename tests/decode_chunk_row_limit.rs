//! Regression test for the unbounded per-row scratch allocation in
//! `Image::expand_chunk` (fuzz-farm issues #7 / #8).
//!
//! A malformed strip can declare a decoded row width (`chunk_row_bytes`) of
//! several gigabytes while carrying a tiny compressed payload. Before the fix,
//! `expand_chunk` allocated a `vec![0u8; chunk_row_bytes]` scratch row with no
//! bound, so `read_chunk` / `read_image` tried a ~4 GB allocation regardless of
//! the configured decoder `Limits` — an OOM under the fuzzer's RSS cap.
//!
//! With the fix, `chunk_row_bytes` is capped against
//! `limits.intermediate_buffer_size` and the decode fails cleanly with
//! `TiffError::LimitsExceeded` instead of allocating.
//!
//! Mutation check: removing the cap in `expand_chunk` makes `read_chunk`
//! allocate the ~4 GB row and then return `TiffError::IoError(..)` (or abort
//! under a memory-limited allocator) rather than `LimitsExceeded`, so the
//! `LimitsExceeded` assertion below fails.

use std::io::Cursor;
use tiff::decoder::{Decoder, Limits};
use tiff::TiffError;

/// The fuzz farm's original repro (`oom-6b069247…`, ~4 GB row buffer request).
const SEED: &[u8] = include_bytes!("../fuzz/regression/crash-expand-chunk-row-oom");

fn restrictive_limits() -> Limits {
    let mut limits = Limits::default();
    limits.decoding_buffer_size = 512 * 1024;
    limits.ifd_value_size = 64 * 1024;
    limits.intermediate_buffer_size = 512 * 1024;
    limits
}

#[test]
fn expand_chunk_row_is_bounded() {
    let mut decoder = Decoder::open(Cursor::new(SEED))
        .expect("seed parses as a TIFF")
        .with_limits(restrictive_limits());

    // The offending directory (a tile whose declared decoded row is ~4 GB) is
    // not the first IFD; walk the chain to the first one that parses as an
    // image, mirroring the fuzz target's traversal.
    let mut decodable = None;
    for i in 0..8usize {
        if decoder.seek_to_directory(i).is_err() {
            break;
        }
        if decoder.dimensions().is_ok() {
            decodable = Some(i);
            break;
        }
    }
    let idx = decodable.expect("seed has a directory whose dimensions parse");

    // The decoded row width vastly exceeds `intermediate_buffer_size`, so the
    // bounded decode must reject it rather than allocating gigabytes.
    match decoder.read_chunk(0) {
        Err(TiffError::LimitsExceeded) => {}
        other => panic!("directory {idx}: expected LimitsExceeded from read_chunk, got {other:?}"),
    }

    // `read_image` walks every chunk through the same path; it must also stay
    // bounded rather than allocating the multi-gigabyte row.
    decoder
        .seek_to_image(idx)
        .expect("re-select the image directory");
    match decoder.read_image() {
        Err(TiffError::LimitsExceeded) => {}
        other => panic!("directory {idx}: expected LimitsExceeded from read_image, got {other:?}"),
    }
}
