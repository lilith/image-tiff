# image-tiff (lilith fork) — Project Notes

## Investigation Notes

- **`gh` resolves to UPSTREAM here — always pass `-R lilith/image-tiff`.**
  This clone has `remote.upstream.gh-resolved base` set, so bare `gh run
  list`, `gh run view`, `gh pr create`, `gh issue` etc. silently target
  `image-rs/image-tiff` (the parent), NOT the fork. Diagnosed 2026-06-12:
  `gh run list --commit <fork-sha>` returned nothing and the visible "daily
  failing scheduled Fuzz" runs were upstream's (their matrix still asks for a
  `decode_image` bin their fuzz crate no longer declares). The fork's own CI
  on 793c7370 (api-doc migration) was green for both workflows — verified via
  explicit `gh api repos/lilith/image-tiff/actions/runs?event=push`.
  Extra caution: bare `gh pr/issue comment` here would post to a third-party
  org — that requires explicit user approval per global rules.
- Fork CI: `rust.yml` (Rust CI) + `fuzz.yml` both trigger on push to main and
  are active. Upstream divergence: fork main diverged long before upstream's
  2026-06-02 PR #394 merge; don't compare run histories across the two.

## libtiff parity (2026-10-26)

- Harness: `parity/` (README there). `just parity <out> <inputs...>` builds libtiff 4.7.2 and
  compares every directory; `parity/gen_matrix.sh` and `parity/gen_jbig.sh` make the corpora;
  `parity/summarize.py <out>` groups the results. Deliberate divergences are listed at the end
  of `parity/README.md`.
- `run.sh` calls `target/release/examples/parity_dump` once per file for the whole sweep (10+
  minutes): rebuilding during a sweep swaps the binary under it. Build experiments with
  `CARGO_TARGET_DIR=~/tmp/<name>` while a sweep runs.
- libtiff stores and reads JBIG (34661) strips bit-reversed under FillOrder 1 (the codec sets
  TIFF_NOBITREV, then reverses when the fill order matches the handle's MSB2LSB default).
- jbigkit 2.1 writes three-line JBIG with one-line stripes (L0 = 1) and no TPBON that its own
  decoder rejects ("PSCD was longer than expected"); ours rejects it too. Not a parity gap.
- libtiff returns old-style JPEG as raw subsampled components; zune-jpeg has no raw output, so
  OJPEG and JPEG (7) are compared with tolerance (`jpeg` class). Byte parity needs a decoder
  that matches libjpeg-turbo (zenjpeg in zentiff).
- LogLuv: the fork returns linear sRGB (f32 matrix in logluv.rs), libtiff XYZ; compare.py
  converts libtiff's XYZ the same way before comparing bits.
- CI's oldest-toolchain job uses `-Zdirect-minimal-versions`; full minimal versions pick
  wyz 0.5.0 + tap 1.0.0 through lerc-rs/bitvec, which do not build.
