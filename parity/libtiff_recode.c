/* Re-encode the first directory of a TIFF with libtiff under any compression number, for codecs
 * tiffcp has no option for (PixarLog). Strips only; samples are copied as libtiff decodes them.
 *
 * Usage: libtiff_recode <in.tif> <out.tif> <compression> <rows-per-strip> [planar]
 * Exit status 0 when written, 1 when libtiff refuses the combination.
 */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <tiffio.h>

static void quiet(const char *module, const char *fmt, va_list ap) {
    (void)module;
    (void)fmt;
    (void)ap;
}

int main(int argc, char **argv) {
    if (argc < 5) {
        fprintf(stderr, "usage: %s <in.tif> <out.tif> <compression> <rows-per-strip> [planar]\n", argv[0]);
        return 2;
    }
    TIFFSetWarningHandler(quiet);
    TIFFSetErrorHandler(quiet);
    TIFF *in = TIFFOpen(argv[1], "r");
    if (!in) return 1;
    uint32_t w = 0, h = 0;
    uint16_t spp = 1, bps = 1, fmt = 1, photometric = 1, planar = 1, extra_count = 0;
    uint16_t *extra = NULL;
    TIFFGetField(in, TIFFTAG_IMAGEWIDTH, &w);
    TIFFGetField(in, TIFFTAG_IMAGELENGTH, &h);
    TIFFGetFieldDefaulted(in, TIFFTAG_SAMPLESPERPIXEL, &spp);
    TIFFGetFieldDefaulted(in, TIFFTAG_BITSPERSAMPLE, &bps);
    TIFFGetFieldDefaulted(in, TIFFTAG_SAMPLEFORMAT, &fmt);
    TIFFGetField(in, TIFFTAG_PHOTOMETRIC, &photometric);
    TIFFGetFieldDefaulted(in, TIFFTAG_PLANARCONFIG, &planar);
    TIFFGetField(in, TIFFTAG_EXTRASAMPLES, &extra_count, &extra);
    if (planar != PLANARCONFIG_CONTIG || TIFFIsTiled(in)) return 1;

    tmsize_t line = TIFFScanlineSize(in);
    uint8_t *image = malloc((size_t)line * h + 1);
    if (!image) return 1;
    for (uint32_t y = 0; y < h; y++)
        if (TIFFReadScanline(in, image + (size_t)y * line, y, 0) < 0) return 1;

    int out_planar = argc > 5 && strcmp(argv[5], "planar") == 0;
    TIFF *out = TIFFOpen(argv[2], "w");
    if (!out) return 1;
    TIFFSetField(out, TIFFTAG_IMAGEWIDTH, w);
    TIFFSetField(out, TIFFTAG_IMAGELENGTH, h);
    TIFFSetField(out, TIFFTAG_SAMPLESPERPIXEL, spp);
    TIFFSetField(out, TIFFTAG_BITSPERSAMPLE, bps);
    TIFFSetField(out, TIFFTAG_SAMPLEFORMAT, fmt);
    TIFFSetField(out, TIFFTAG_PHOTOMETRIC, photometric);
    if (extra_count) TIFFSetField(out, TIFFTAG_EXTRASAMPLES, extra_count, extra);
    uint16_t *red, *green, *blue;
    if (photometric == PHOTOMETRIC_PALETTE && TIFFGetField(in, TIFFTAG_COLORMAP, &red, &green, &blue))
        TIFFSetField(out, TIFFTAG_COLORMAP, red, green, blue);
    TIFFSetField(out, TIFFTAG_PLANARCONFIG, out_planar ? PLANARCONFIG_SEPARATE : PLANARCONFIG_CONTIG);
    if (!TIFFSetField(out, TIFFTAG_COMPRESSION, atoi(argv[3]))) return 1;
    uint32_t rps = (uint32_t)atoi(argv[4]);
    TIFFSetField(out, TIFFTAG_ROWSPERSTRIP, rps == 0 || rps > h ? h : rps);

    int ok = 1;
    if (!out_planar) {
        for (uint32_t y = 0; y < h && ok; y++)
            ok = TIFFWriteScanline(out, image + (size_t)y * line, y, 0) >= 0;
    } else {
        /* Split each chunky row into one row per plane; byte-aligned samples only. */
        if (bps % 8 != 0) return 1;
        size_t bytes = bps / 8, plane_line = (size_t)w * bytes;
        uint8_t *row = malloc(plane_line);
        for (uint16_t s = 0; s < spp && ok; s++)
            for (uint32_t y = 0; y < h && ok; y++) {
                const uint8_t *src = image + (size_t)y * line;
                for (uint32_t x = 0; x < w; x++)
                    memcpy(row + x * bytes, src + ((size_t)x * spp + s) * bytes, bytes);
                ok = TIFFWriteScanline(out, row, y, s) >= 0;
            }
        free(row);
    }
    if (ok) ok = TIFFWriteDirectory(out);
    TIFFClose(out);
    TIFFClose(in);
    free(image);
    if (!ok) remove(argv[2]);
    return ok ? 0 : 1;
}
