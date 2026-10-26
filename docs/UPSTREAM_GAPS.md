# Upstream gap inventory

Open issues and pull requests in image-rs/image-tiff, checked against this fork on
2026-10-24 (fork main after `2a7d124f`). "Affects zenextras" means zentiff decodes or
encodes through the path. Every "fixed" row has a test in this repository; the oracle column
says what the decoded output was compared against.

## Issues

| # | Topic | Fork status | Affects zenextras | Evidence |
|---|---|---|---|---|
| 403 | Tiled + planar panics on border tiles | Fixed (`42583e9c`) | Yes: zentiff reads all planes | `tests/decode_synthetic.rs` `tiled_planar_border_tiles`; tifffile-written files match |
| 395 | Tiled LZW fails via `read_image` | Fixed: the reported file is band-interleaved (planar), same path as 403 | Yes | tifffile tiled LZW, chunky and planar, match |
| — | Floating-point predictor on tiled images with border tiles decoded wrong samples (found while checking 395; also in upstream) | Fixed (`42583e9c`) | Yes | `float_predictor_border_tiles_*`; tifffile f16/f32/f64/RGB files match |
| 253 | Missing `PhotometricInterpretation` rejected | Fixed (`a528ec13`): 1 color sample is BlackIsZero, 3 are RGB (libtiff RGBA reader, GDAL) | Yes: GeoTIFFs without the tag | `missing_photometric_interpretation` |
| 405a | CCITT Modified Huffman (compression 2) unsupported | Fixed (`19ab3adb`) | Yes | 91 libtiff-written CCITT files match; `ccitt_group3_and_modified_huffman` |
| — | Group 3 2D (`T4Options` bit 0) decoded as 1D, wrong pixels | Fixed (`19ab3adb`) | Yes | same |
| — | Group 3 without EOL codes (libtiff's testfax3_bug54_1dnoEOL.tif) decoded 5.7% of pixels wrong | Fixed: rows decode back to back when a strip holds no EOL, as libtiff does | Yes | `ccitt_group3_without_eol`; the file now matches libtiff |
| — | CCITT wider than 65,535 pixels; G4 strips without EOFB lost their last row | Fixed (`3862a009`, `45c4dd80`) | Yes | `ccitt_wider_than_u16`, `ccitt_group4_without_eofb` |
| — | Sparse chunks (byte count 0, written by GDAL for COGs) decoded the bytes at their offset as pixels | Fixed: decoded as stored samples of 0, as GDAL and tifffile do (libtiff errors) | Yes: COG | `sparse_chunks_decode_as_zero` |
| — | Tiled YCbCr with chroma subsampling failed on border tiles (the upsampler got the clipped width) | Fixed | Yes | `ycbcr_subsampled_border_tiles` |
| 405b | Pure-Rust zstd for wasm | Already in the fork: `zstd-safe-rust` (zrip-decode) | Yes (wasm builds) | — |
| 405c | Access to extra samples beyond the photometric ones | Open: needs API | Not today: zentiff keeps photometric samples and alpha | — |
| 315 | zune-jpeg panic in scalar upsampler | Not reproducible: all 9 attached inputs return errors with zune-jpeg 0.5.15 | — | checked 2026-10-24 |
| 331 | Specific image fails in oculante | Not a decoder bug: the 45181×6520 1-bit LZW sample decodes identical to tifffile | — | checked 2026-10-24 |
| 410 | LERC compression | Open: decodable through `Decoder::with_decompressor` once a LERC decoder exists | GeoTIFF (COG) | — |
| 264, 222 | Private interfaces | Mostly addressed upstream: `Directory`, `current_ifd`, `read_directory`, `read_directory_tags` | zentiff's inventory walks IFDs itself | — |
| 217 | Gray(1) | Supported | — | — |
| 98, 85 | GeoTIFF, cloud (COG) | Planned in zentiff: `zentiff/docs/GEOTIFF_PYRAMIDS.md` (zenextras) | Yes | — |
| 166, 205 | Tiled / multipage tiled encoding | Open: needed for a COG writer | Planned | — |
| 195, 258, 265 | CMYKA, GrayA, multiband encoding | Open (encoder API) | zentiff writes GrayA as Gray + ExtraSamples | — |
| 232, 251, 273, 299, 47, 61, 40 | Encoder conflicts, threading, EXIF, tags, tests, API | Not decoder correctness; not taken | — | — |

## Pull requests

| # | Topic | Fork status | Notes |
|---|---|---|---|
| 388 | YCbCr subsampling without JPEG | Taken (`93eff0dd`) | Independent block-unpacking oracle matches `dscf0013.tif` and `ycbcr-cat.tif` byte for byte |
| 391 | LZW without EndOfInformation | Not needed: the fork already decodes complete strips without EOI and errors on truncated ones | `lzw_without_end_of_information` |
| 389 | RowsPerStrip larger than the image | Not needed: uncompressed and Group 4 strips with RowsPerStrip 95318 decode | — |
| 417 | Encoder predictor stride with extra samples | Not taken: changes `ColorType` trait methods, which breaks the 0.11.3 API this fork keeps | zentiff disables the predictor for GrayAlpha (`effective_predictor`) |
| 418 | ICC profile as UNDEFINED | Not taken (API); zentiff writes UNDEFINED itself through `write_entry_bytes` | — |
| 414 | CCITT 1D Huffman encoding, Gray1 encoding | Decoding done separately; encoding not taken (API) | — |
| 419 | Zstd encoding | Not taken (API, large) | — |
| 411 | Reuse decompression engines | Not taken: performance; would interact with `with_decompressor` | — |
| 284 | Seek to independent IFD sequences (SubIFDs) | Partly upstream already (`read_directory`); rest pending for pyramid work | — |
| 330 | Copy EXIF helper | Not taken | zentiff handles EXIF |
| 420, 421 | Docs, clippy | Not needed | fork clippy is clean |

## Fork-only additions under review

- `Decoder::with_decompressor` / `decoder::pluggable::ChunkDecompressor`: caller-supplied
  strip/tile decompressors (branch `feat/chunk-decompressors`, PR in lilith/image-tiff assigned to lilith).

## Not yet handled

- DNG predictors 34892-34895 (horizontal X2/X4, floating point X2/X4): rejected at parse as
  unknown predictors; `tags::Predictor` is an exhaustive public enum, so supporting them needs an
  API change or an internal numeric predictor.

- Missing `StripByteCounts`: libtiff estimates them for one strip per plane; this fork rejects
  the file.
