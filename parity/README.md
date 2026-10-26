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

Results land in `<out>/results.tsv`; `header.txt` records the libtiff version, the image-tiff
commit and the host.
