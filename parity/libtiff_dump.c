/* Parity oracle: decode every directory of a TIFF with libtiff and write the samples as libtiff
 * returns them from TIFFReadEncodedStrip/TIFFReadEncodedTile: predictor undone, native byte order,
 * no photometric conversion. Strips and tiles are assembled into whole-image rows (each row
 * byte-aligned, planes one after another for PlanarConfiguration 2).
 *
 * Usage: libtiff_dump <file.tif> <out-prefix>
 * Writes <out-prefix>.<n>.txt (key=value lines) and, when decoding succeeds, <out-prefix>.<n>.raw
 * for directory n = 0, 1, ...
 */
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <tiffio.h>

static char messages[8192];
static size_t messages_len;

static void collect(const char *kind, const char *module, const char *fmt, va_list ap) {
    if (messages_len >= sizeof(messages) - 2) return;
    int n = snprintf(messages + messages_len, sizeof(messages) - messages_len, "%s:%s: ", kind,
                     module ? module : "");
    if (n > 0) messages_len += (size_t)n;
    if (messages_len >= sizeof(messages) - 2) return;
    n = vsnprintf(messages + messages_len, sizeof(messages) - messages_len, fmt, ap);
    if (n > 0) messages_len += (size_t)n;
    if (messages_len < sizeof(messages) - 2) messages[messages_len++] = '|';
    messages[messages_len] = 0;
}
static void on_error(const char *module, const char *fmt, va_list ap) { collect("E", module, fmt, ap); }
static void on_warning(const char *module, const char *fmt, va_list ap) { collect("W", module, fmt, ap); }

static uint16_t tag16(TIFF *t, uint32_t tag, uint16_t dflt) {
    uint16_t v = dflt;
    if (!TIFFGetFieldDefaulted(t, tag, &v)) v = dflt;
    return v;
}

/* Copy `bits` bits starting at bit `src_bit` of `src` to bit `dst_bit` of `dst` (MSB first). */
static void copy_bits(uint8_t *dst, uint64_t dst_bit, const uint8_t *src, uint64_t src_bit, uint64_t bits) {
    if ((dst_bit & 7) == 0 && (src_bit & 7) == 0 && (bits & 7) == 0) {
        memcpy(dst + dst_bit / 8, src + src_bit / 8, bits / 8);
        return;
    }
    for (uint64_t i = 0; i < bits; i++) {
        uint64_t s = src_bit + i, d = dst_bit + i;
        int bit = (src[s >> 3] >> (7 - (s & 7))) & 1;
        if (bit) dst[d >> 3] |= (uint8_t)(0x80 >> (d & 7));
        else dst[d >> 3] &= (uint8_t) ~(0x80 >> (d & 7));
    }
}

static int dump_directory(TIFF *t, const char *prefix, int n) {
    char path[4096];
    snprintf(path, sizeof path, "%s.%d.txt", prefix, n);
    FILE *meta = fopen(path, "w");
    if (!meta) return 1;

    uint32_t w = 0, h = 0;
    TIFFGetField(t, TIFFTAG_IMAGEWIDTH, &w);
    TIFFGetField(t, TIFFTAG_IMAGELENGTH, &h);
    uint16_t spp = tag16(t, TIFFTAG_SAMPLESPERPIXEL, 1);
    uint16_t bps = tag16(t, TIFFTAG_BITSPERSAMPLE, 1);
    uint16_t fmt = tag16(t, TIFFTAG_SAMPLEFORMAT, 1);
    uint16_t planar = tag16(t, TIFFTAG_PLANARCONFIG, 1);
    uint16_t compression = tag16(t, TIFFTAG_COMPRESSION, 1);
    uint16_t photometric = 0;
    int has_photometric = TIFFGetField(t, TIFFTAG_PHOTOMETRIC, &photometric);
    uint16_t sub_h = 1, sub_v = 1;
    /* Old-style JPEG returns the raw subsampled components (libtiff corrects the tag to the
     * JPEG frame's sampling factors); new-style JPEG is read as RGB below. */
    if (has_photometric && photometric == PHOTOMETRIC_YCBCR && compression != COMPRESSION_JPEG)
        TIFFGetFieldDefaulted(t, TIFFTAG_YCBCRSUBSAMPLING, &sub_h, &sub_v);
    uint16_t extra_count = 0;
    uint16_t *extra = NULL;
    TIFFGetField(t, TIFFTAG_EXTRASAMPLES, &extra_count, &extra);
    int tiled = TIFFIsTiled(t);

    fprintf(meta, "width=%u\nheight=%u\nspp=%u\nbps=%u\nsampleformat=%u\nplanar=%u\ncompression=%u\n",
            w, h, spp, bps, fmt, planar, compression);
    fprintf(meta, "photometric=%d\nsubsampling=%u,%u\nextrasamples=%u\ntiled=%d\n",
            has_photometric ? photometric : -1, sub_h, sub_v, extra_count, tiled);
    if (extra_count) {
        fprintf(meta, "extra=");
        for (int i = 0; i < extra_count; i++) fprintf(meta, "%s%u", i ? "," : "", extra[i]);
        fprintf(meta, "\n");
    }
    if (compression == COMPRESSION_SGILOG || compression == COMPRESSION_SGILOG24)
        TIFFSetField(t, TIFFTAG_SGILOGDATAFMT, SGILOGDATAFMT_FLOAT);
    if (compression == COMPRESSION_JPEG && has_photometric && photometric == PHOTOMETRIC_YCBCR)
        TIFFSetField(t, TIFFTAG_JPEGCOLORMODE, JPEGCOLORMODE_RGB);
    {
        uint32_t rps = 0, tw = 0, th = 0;
        TIFFGetFieldDefaulted(t, TIFFTAG_ROWSPERSTRIP, &rps);
        TIFFGetField(t, TIFFTAG_TILEWIDTH, &tw);
        TIFFGetField(t, TIFFTAG_TILELENGTH, &th);
        fprintf(meta, "rowsperstrip=%u\ntilewidth=%u\ntilelength=%u\n", rps, tw, th);
    }

    /* Decoded bits per stored row of one plane (chunky: all samples). */
    uint16_t samples_in_row = planar == PLANARCONFIG_SEPARATE ? 1 : spp;
    uint16_t planes = planar == PLANARCONFIG_SEPARATE ? spp : 1;
    uint64_t elem_bits = (uint64_t)bps;
    if (compression == COMPRESSION_SGILOG || compression == COMPRESSION_SGILOG24) elem_bits = 32;
    int status = 0;
    uint8_t *image = NULL;
    uint64_t row_bytes = 0, plane_bytes = 0;

    if (sub_h > 1 || sub_v > 1) {
        /* Subsampled YCbCr: keep libtiff's stored block layout, chunk by chunk, unassembled. */
        fprintf(meta, "layout=ycbcr-blocks\n");
        tmsize_t chunk_size = tiled ? TIFFTileSize(t) : TIFFStripSize(t);
        uint32_t chunks = tiled ? TIFFNumberOfTiles(t) : TIFFNumberOfStrips(t);
        if (chunk_size <= 0 || chunks == 0) {
            status = 1;
        } else {
            image = calloc(chunks, (size_t)chunk_size);
            for (uint32_t c = 0; c < chunks && image; c++) {
                tmsize_t got = tiled ? TIFFReadEncodedTile(t, c, image + (size_t)c * chunk_size, chunk_size)
                                     : TIFFReadEncodedStrip(t, c, image + (size_t)c * chunk_size, chunk_size);
                if (got < 0) { status = 1; break; }
            }
            plane_bytes = (uint64_t)chunks * (uint64_t)chunk_size;
            fprintf(meta, "chunk_bytes=%lld\nchunks=%u\n", (long long)chunk_size, chunks);
        }
    } else {
        row_bytes = ((uint64_t)w * samples_in_row * elem_bits + 7) / 8;
        plane_bytes = row_bytes * h;
        image = calloc(1, plane_bytes * planes + 1);
        if (!image) status = 1;
        if (!status && tiled) {
            uint32_t tw = 0, th = 0;
            TIFFGetField(t, TIFFTAG_TILEWIDTH, &tw);
            TIFFGetField(t, TIFFTAG_TILELENGTH, &th);
            tmsize_t tsize = TIFFTileSize(t);
            uint8_t *tile = tsize > 0 ? malloc((size_t)tsize) : NULL;
            uint64_t trow_bits = (uint64_t)tw * samples_in_row * elem_bits;
            uint64_t trow_bytes = (trow_bits + 7) / 8;
            for (uint16_t p = 0; p < planes && tile && !status; p++)
                for (uint32_t y = 0; y < h && !status; y += th)
                    for (uint32_t x = 0; x < w; x += tw) {
                        uint32_t tile_index = TIFFComputeTile(t, x, y, 0, p);
                        if (TIFFReadEncodedTile(t, tile_index, tile, tsize) < 0) { status = 1; break; }
                        uint32_t rows = (y + th > h) ? h - y : th;
                        uint32_t cols = (x + tw > w) ? w - x : tw;
                        for (uint32_t r = 0; r < rows; r++)
                            copy_bits(image + p * plane_bytes + (uint64_t)(y + r) * row_bytes,
                                      (uint64_t)x * samples_in_row * elem_bits, tile + r * trow_bytes, 0,
                                      (uint64_t)cols * samples_in_row * elem_bits);
                    }
            if (!tile) status = 1;
            free(tile);
        } else if (!status) {
            uint32_t rps = h;
            TIFFGetFieldDefaulted(t, TIFFTAG_ROWSPERSTRIP, &rps);
            if (rps == 0 || rps > h) rps = h;
            tmsize_t ssize = TIFFStripSize(t);
            uint8_t *strip = ssize > 0 ? malloc((size_t)ssize) : NULL;
            for (uint16_t p = 0; p < planes && strip && !status; p++)
                for (uint32_t y = 0; y < h; y += rps) {
                    uint32_t strip_index = TIFFComputeStrip(t, y, p);
                    if (TIFFReadEncodedStrip(t, strip_index, strip, ssize) < 0) { status = 1; break; }
                    uint32_t rows = (y + rps > h) ? h - y : rps;
                    memcpy(image + p * plane_bytes + (uint64_t)y * row_bytes, strip, rows * row_bytes);
                }
            if (!strip) status = 1;
            free(strip);
        }
        fprintf(meta, "layout=rows\nrow_bytes=%llu\nplanes=%u\n", (unsigned long long)row_bytes, planes);
    }

    fprintf(meta, "decoded=%d\n", status == 0);
    if (status == 0 && image) {
        snprintf(path, sizeof path, "%s.%d.raw", prefix, n);
        FILE *raw = fopen(path, "wb");
        if (raw) {
            fwrite(image, 1, plane_bytes * planes, raw);
            fclose(raw);
        }
    }
    fprintf(meta, "messages=%s\n", messages);
    messages_len = 0;
    messages[0] = 0;
    fclose(meta);
    free(image);
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--version") == 0) {
        puts(TIFFGetVersion());
        return 0;
    }
    if (argc != 3) {
        fprintf(stderr, "usage: %s <file.tif> <out-prefix>\n", argv[0]);
        return 2;
    }
    TIFFSetErrorHandler(on_error);
    TIFFSetWarningHandler(on_warning);
    TIFF *t = TIFFOpen(argv[1], "r");
    char path[4096];
    if (!t) {
        snprintf(path, sizeof path, "%s.open.txt", argv[2]);
        FILE *f = fopen(path, "w");
        if (f) { fprintf(f, "open_failed=1\nmessages=%s\n", messages); fclose(f); }
        return 0;
    }
    int n = 0;
    do {
        dump_directory(t, argv[2], n++);
    } while (n < 64 && TIFFReadDirectory(t));
    TIFFClose(t);
    return 0;
}
