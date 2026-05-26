//! Fuzz crash regression suite (DEDUP-J template, ported from zenwebp).
//!
//! Runs every file in `fuzz/regression/` through every decoder entry point that
//! has a fuzz target. Each seed file is a previously-found crash that has been
//! fixed; this test ensures none of them re-introduce a panic.
//!
//! Reproduces what the `decode_comprehensive` and `decode_image` fuzz targets
//! do, but as a regular `cargo test` — no nightly toolchain needed. Failures
//! here mean a regression of a previously-fixed bug.
//!
//! To add a new seed: drop the (preferably minimized) crash file into
//! `fuzz/regression/` with a descriptive name, no other action required.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

fn regression_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fuzz/regression")
}

/// Recursively collect every regular file under `dir`. Skips dotfiles and
/// silently tolerates a missing directory.
fn collect_seeds(dir: &PathBuf, out: &mut Vec<PathBuf>) {
    let read = match fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in read.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        match entry.file_type() {
            Ok(t) if t.is_file() => out.push(path),
            Ok(t) if t.is_dir() => collect_seeds(&path, out),
            _ => {}
        }
    }
}

fn restrictive_limits() -> tiff::decoder::Limits {
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = 512 * 1024;
    limits.ifd_value_size = 64 * 1024;
    limits.intermediate_buffer_size = 512 * 1024;
    limits
}

fn run_decode_comprehensive(data: &[u8]) {
    // Mirrors fuzz_targets/decode_comprehensive.rs.
    // Path 1: probe.
    {
        let _ = tiff::decoder::TiffHeader::parse(Cursor::new(data));
    }
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
}

fn run_decode_image(data: &[u8]) {
    // Mirrors fuzz_targets/decode_image.rs.
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
}

#[test]
fn fuzz_regression_seeds_do_not_panic() {
    let dir = regression_dir();
    let mut seeds = Vec::new();
    collect_seeds(&dir, &mut seeds);

    if seeds.is_empty() {
        eprintln!(
            "note: no regression seeds found under {} — nothing to check",
            dir.display()
        );
        return;
    }

    for path in seeds {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("<unnamed>")
            .to_owned();
        let input = fs::read(&path).unwrap_or_else(|e| panic!("read {name}: {e}"));

        // Each entry point may return Err but must not panic. If any panics,
        // the test fails with the seed name in the unwind message.
        run_decode_comprehensive(&input);
        run_decode_image(&input);

        eprintln!("ok: {name} ({} bytes)", input.len());
    }
}
