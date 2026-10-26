/* Write a JBIG (34661) TIFF whose strips are encoded by jbigkit with chosen options, for
 * parity tests beyond libtiff's own JBIG encoder (which always writes one strip with jbigkit's
 * defaults). Strip data follows libtiff's convention: bit-reversed when FillOrder is 1.
 *
 * Usage: jbig_tiff <out.tif> <width> <height> <rows-per-strip> <l0> <mx> <layers> <flags>
 *   flags: any of lrltwo tpbon tpdon dpon sdrst delayat fill2 newlen, comma separated, or -.
 * newlen appends a NEWLEN marker segment giving the strip three rows fewer than encoded.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <jbig.h>
#include <tiffio.h>

struct sink {
    unsigned char *data;
    size_t len, cap;
};

static void out(unsigned char *start, size_t len, void *file) {
    struct sink *s = file;
    if (s->len + len > s->cap) {
        s->cap = (s->len + len) * 2;
        s->data = realloc(s->data, s->cap);
    }
    memcpy(s->data + s->len, start, len);
    s->len += len;
}

static unsigned char reverse(unsigned char b) {
    unsigned char r = 0;
    for (int i = 0; i < 8; i++) r |= ((b >> i) & 1) << (7 - i);
    return r;
}

/* Text-like strokes, a period-5 halftone band (which moves the AT pixel), noise and runs. */
static int pixel(uint32_t x, uint32_t y, uint32_t h) {
    if (y < h / 4) return ((x / 3 + y / 2) % 7 == 0) || (x % 11 == 3 && y % 9 < 6);
    if (y < h / 2) return (x % 5 == y % 5) || (x % 5 == (y + 2) % 5);
    if (y < 3 * h / 4) {
        uint32_t v = (x * 2654435761u) ^ (y * 40503u);
        v ^= v >> 13;
        return (v & 7) == 0;
    }
    return (x / 17 + y / 13) % 2;
}

int main(int argc, char **argv) {
    if (argc != 9) {
        fprintf(stderr, "usage: %s <out.tif> <width> <height> <rows-per-strip> <l0> <mx> <layers> <flags>\n", argv[0]);
        return 2;
    }
    uint32_t w = (uint32_t)atoi(argv[2]), h = (uint32_t)atoi(argv[3]), rps = (uint32_t)atoi(argv[4]);
    unsigned long l0 = strtoul(argv[5], NULL, 10);
    int mx = atoi(argv[6]), layers = atoi(argv[7]);
    const char *flags = argv[8];
    int options = 0, fill2 = strstr(flags, "fill2") != NULL, newlen = strstr(flags, "newlen") != NULL;
    if (strstr(flags, "lrltwo")) options |= JBG_LRLTWO;
    if (strstr(flags, "tpbon")) options |= JBG_TPBON;
    if (strstr(flags, "tpdon")) options |= JBG_TPDON;
    if (strstr(flags, "dpon")) options |= JBG_DPON;
    if (strstr(flags, "sdrst")) options |= JBG_SDRST;
    if (strstr(flags, "delayat")) options |= JBG_DELAY_AT;
    if (newlen) options |= JBG_VLENGTH;
    if (rps == 0 || rps > h) rps = h;

    TIFF *tif = TIFFOpen(argv[1], "w");
    if (!tif) return 1;
    TIFFSetField(tif, TIFFTAG_IMAGEWIDTH, w);
    TIFFSetField(tif, TIFFTAG_IMAGELENGTH, h);
    TIFFSetField(tif, TIFFTAG_BITSPERSAMPLE, 1);
    TIFFSetField(tif, TIFFTAG_SAMPLESPERPIXEL, 1);
    TIFFSetField(tif, TIFFTAG_PHOTOMETRIC, PHOTOMETRIC_MINISBLACK);
    TIFFSetField(tif, TIFFTAG_COMPRESSION, 34661);
    TIFFSetField(tif, TIFFTAG_FILLORDER, fill2 ? FILLORDER_LSB2MSB : FILLORDER_MSB2LSB);
    TIFFSetField(tif, TIFFTAG_ROWSPERSTRIP, rps);

    size_t row_bytes = (w + 7) / 8;
    for (uint32_t s = 0; s * rps < h; s++) {
        uint32_t y0 = s * rps, rows = h - y0 < rps ? h - y0 : rps;
        unsigned char *bitmap = calloc(row_bytes * rows, 1);
        for (uint32_t y = 0; y < rows; y++)
            for (uint32_t x = 0; x < w; x++)
                if (pixel(x, y0 + y, h)) bitmap[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
        struct sink sink = {0};
        struct jbg_enc_state enc;
        unsigned char *planes[1] = {bitmap};
        jbg_enc_init(&enc, w, rows, 1, planes, out, &sink);
        if (layers > 0) jbg_enc_layers(&enc, layers);
        jbg_enc_options(&enc, JBG_ILEAVE | JBG_SMID, options, l0, mx, 0);
        jbg_enc_out(&enc);
        jbg_enc_free(&enc);
        if (newlen && rows > 3) {
            unsigned char marker[6] = {0xff, 0x05, 0, 0, 0, 0};
            uint32_t len = rows - 3;
            marker[2] = len >> 24, marker[3] = len >> 16, marker[4] = len >> 8, marker[5] = len;
            out(marker, 6, &sink);
        }
        if (!fill2)
            for (size_t i = 0; i < sink.len; i++) sink.data[i] = reverse(sink.data[i]);
        if (TIFFWriteRawStrip(tif, s, sink.data, (tmsize_t)sink.len) < 0) return 1;
        free(sink.data);
        free(bitmap);
    }
    TIFFWriteDirectory(tif);
    TIFFClose(tif);
    return 0;
}
