/* Write uncompressed 12-bit gray and RGB TIFFs (packed MSB-first, as TIFF stores 12-bit
 * samples) with gradients, edges and noise, for tiffcp to re-encode as 12-bit JPEG.
 * Usage: gen_base12 <out-dir> */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <tiffio.h>

static uint16_t sample(uint32_t x, uint32_t y, int c, uint32_t w, uint32_t h) {
    uint32_t v = (x * 4095 / (w > 1 ? w - 1 : 1) + y * 1365 / (h > 1 ? h - 1 : 1) + (uint32_t)c * 700) % 4096;
    if ((x / 7 + y / 5) % 3 == 0) v = 4095 - v;            /* edges */
    uint32_t n = (x * 2654435761u) ^ (y * 40503u) ^ ((uint32_t)c * 977u);
    n ^= n >> 13;
    return (uint16_t)((v + (n % 97)) % 4096);              /* noise */
}

static int write(const char *path, uint32_t w, uint32_t h, int spp) {
    TIFF *t = TIFFOpen(path, "w");
    if (!t) return 1;
    TIFFSetField(t, TIFFTAG_IMAGEWIDTH, w);
    TIFFSetField(t, TIFFTAG_IMAGELENGTH, h);
    TIFFSetField(t, TIFFTAG_BITSPERSAMPLE, 12);
    TIFFSetField(t, TIFFTAG_SAMPLESPERPIXEL, spp);
    TIFFSetField(t, TIFFTAG_PHOTOMETRIC, spp == 1 ? PHOTOMETRIC_MINISBLACK : PHOTOMETRIC_RGB);
    TIFFSetField(t, TIFFTAG_PLANARCONFIG, PLANARCONFIG_CONTIG);
    TIFFSetField(t, TIFFTAG_ROWSPERSTRIP, h);
    tmsize_t line = TIFFScanlineSize(t);
    uint8_t *row = calloc(1, (size_t)line);
    for (uint32_t y = 0; y < h; y++) {
        memset(row, 0, (size_t)line);
        uint64_t bit = 0;
        for (uint32_t x = 0; x < w; x++)
            for (int c = 0; c < spp; c++, bit += 12) {
                uint16_t v = sample(x, y, c, w, h);
                for (int i = 11; i >= 0; i--)
                    if ((v >> i) & 1) row[(bit + 11 - i) / 8] |= (uint8_t)(0x80 >> ((bit + 11 - i) % 8));
            }
        if (TIFFWriteScanline(t, row, y, 0) < 0) return 1;
    }
    free(row);
    TIFFClose(t);
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    char p[4096];
    uint32_t sizes[][2] = {{37, 23}, {129, 67}};
    for (int s = 0; s < 2; s++)
        for (int spp = 1; spp <= 3; spp += 2) {
            snprintf(p, sizeof p, "%s/%s-u12-%ux%u.tif", argv[1], spp == 1 ? "gray" : "rgb", sizes[s][0], sizes[s][1]);
            if (write(p, sizes[s][0], sizes[s][1], spp)) { fprintf(stderr, "failed %s\n", p); return 1; }
        }
    return 0;
}
