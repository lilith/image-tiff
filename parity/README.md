# libtiff parity harness

Decodes TIFF files with libtiff and with image-tiff and compares every sample.

```
just parity ~/tmp/parity/run tests/images tests/libtiffpic <more files or dirs>
```

- `build-libtiff.sh` builds libtiff 4.7.2 (hash-checked) with the system's codec libraries
  (libjpeg-turbo, zlib, libdeflate, LZMA, ZSTD, WebP, JBIG, LERC) into
  `~/.cache/image-tiff-parity/`. It is a test oracle only; nothing in the crate links it.
- `libtiff_dump.c` writes, per directory, libtiff's `TIFFReadEncodedStrip`/`Tile` output
  assembled into whole-image rows: predictor undone, native byte order, no photometric
  conversion. Subsampled YCbCr (uncompressed or old-style JPEG) keeps libtiff's block layout;
  JPEG (7) YCbCr is read with `JPEGCOLORMODE_RGB`; SGILog with `SGILOGDATAFMT_FLOAT`.
- `examples/parity_dump.rs` writes image-tiff's `read_image_bytes` output per image.
- `compare.py` brings both to stored samples (undoing image-tiff's WhiteIsZero inversion,
  unpacking sub-byte rows, expanding YCbCr blocks with nearest chroma) and classifies each
  directory: `exact`, `differs`, `fork-fails`, `fork-panics`, `libtiff-fails`, `both-fail`,
  `fork-missing`, `jpeg` (libtiff's RGB against image-tiff's YCbCr through the JFIF formula),
  `skipped`.

`gen_matrix.sh` writes the codec matrix (tiffcp, plus `libtiff_recode.c` for PixarLog);
`gen_jbig.sh` writes jbigkit-encoded JBIG files beyond libtiff's own encoder (`jbig_tiff.c`:
templates, typical and deterministic prediction, SDRST, AT, NEWLEN, differential layers).

Results land in `<out>/results.tsv`; `header.txt` records the libtiff version, the image-tiff
commit and the host.

## Where image-tiff and libtiff 4.7.2 differ on purpose

- Chunks with no stored bytes (GDAL's sparse tiles and strips): image-tiff decodes zeros, as
  GDAL and tifffile do; libtiff reports an error.
- CCITT RLEW (32771): image-tiff starts every row on a 16-bit boundary from the start of the
  chunk. libtiff clears `BitsAvail % 16` buffered bits at the end of a row, so when 16 or more
  bits are buffered (its 16-bit lookups read up to 19 ahead) the padding byte stays in the
  buffer and the next row is misread; its FAX3_DEBUG trace shows the row after such an end
  decoding from the padding.
- Old-style JPEG (6): libtiff returns the raw subsampled components; image-tiff returns
  full-resolution YCbCr, upsampled by its JPEG decoder (zune-jpeg), so chroma differs and
  luma is within 1 (zune-jpeg's IDCT).
- JBIG (34661): libtiff refuses multi-strip JBIG; image-tiff decodes each strip's
  image entity (138 multi-strip files match libjbig called strip by strip). Three-line JBIG
  with one-line stripes and no typical prediction, as jbigkit 2.1 writes it for wide images,
  is rejected by both decoders (coded data left after a stripe's lines).
