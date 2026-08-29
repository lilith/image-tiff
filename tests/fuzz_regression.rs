//! Replay seed inputs from `fuzz/regression/` through every fuzz target
//! entry point. Shared scaffolding lives in `zen-fuzz-regress`.

use std::io::Cursor;
use std::path::Path;
use zenutils_fuzz::RegressionSuite;

/// Lower bound on the replayable seed corpus committed under `fuzz/regression/`.
///
/// `RegressionSuite` treats a missing or empty seed directory as a clean no-op,
/// so an emptied, renamed, or never-checked-out corpus would let this test pass
/// without replaying a single seed. Pinning the floor makes that a loud failure.
/// Raise this when seeds are added; only lower it when deleting seeds on purpose.
const MIN_SEEDS: usize = 5;

/// Count the files `RegressionSuite::run` will actually replay, using its own
/// filters: recurse into subdirectories, skip dotfiles, `*.md` and `*.txt`.
fn replayable_seeds(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut found = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            found += replayable_seeds(&path);
        } else if path.is_file() {
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".md") && !lower.ends_with(".txt") {
                found += 1;
            }
        }
    }
    found
}

/// Fail loudly when the corpus this suite exists to replay is not there.
fn assert_corpus_present() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fuzz/regression");
    let found = replayable_seeds(&dir);
    assert!(
        found >= MIN_SEEDS,
        "{} holds {found} replayable seeds, expected at least {MIN_SEEDS} — \
         the committed regression corpus is missing or was renamed, which would \
         otherwise let this test pass without replaying anything",
        dir.display()
    );
}

fn restrictive_limits() -> tiff::decoder::Limits {
    let mut limits = tiff::decoder::Limits::default();
    limits.decoding_buffer_size = 512 * 1024;
    limits.ifd_value_size = 64 * 1024;
    limits.intermediate_buffer_size = 512 * 1024;
    limits
}

#[test]
fn fuzz_regression() {
    assert_corpus_present();
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
                // Exercise the bounded chunk-decode path (expand_chunk). A
                // malformed strip can declare a multi-gigabyte decoded row
                // width; this must fail on the limit, not allocate it (#7/#8).
                let _ = decoder.read_chunk(0);
                let _ = decoder.read_image();
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
