"""Write uncompressed base images for the parity matrix (run by gen_matrix.sh).

Usage: gen_base.py <out-dir>. Odd sizes so tiles and strips have partial borders; content mixes
gradients, steps and noise so predictors and run-length codecs see real structure.
"""
import os
import sys

import numpy as np
import tifffile

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
rng = np.random.default_rng(20261026)
H, W = 23, 37


def field(maxv, channels=1, dtype=np.uint16):
    y, x = np.mgrid[0:H, 0:W]
    planes = []
    for c in range(channels):
        base = (x * (c + 3) + y * (2 * c + 1)) % (maxv + 1)
        noise = rng.integers(0, max(2, (maxv + 1) // 8), size=(H, W))
        steps = ((x // 5 + y // 4 + c) % 2) * (maxv // 3)
        planes.append(np.clip(base + noise + steps, 0, maxv))
    a = np.stack(planes, -1) if channels > 1 else planes[0]
    return a.astype(dtype)


def save(name, data, **kw):
    tifffile.imwrite(os.path.join(out, name), data, **kw)


# Unsigned gray at every common depth (sub-byte via bitspersample, packed by tifffile).
for bits in (1, 2, 4, 8, 16, 32):
    dt = np.uint8 if bits <= 8 else np.uint16 if bits == 16 else np.uint32
    save(f"gray-u{bits}.tif", field((1 << bits) - 1, 1, dt), photometric="minisblack",
         bitspersample=bits if bits < 8 else None)
save("gray-u8-miniswhite.tif", field(255, 1, np.uint8), photometric="miniswhite")
save("gray-u1-miniswhite.tif", field(1, 1, np.uint8), photometric="miniswhite", bitspersample=1)
for bits, dt in ((8, np.int8), (16, np.int16), (32, np.int32)):
    save(f"gray-i{bits}.tif", (field((1 << bits) - 1, 1, np.int64) - (1 << (bits - 1))).astype(dt),
         photometric="minisblack")
for dt in (np.float16, np.float32, np.float64):
    save(f"gray-f{np.dtype(dt).itemsize * 8}.tif",
         (field(1000, 1, np.float64) / 7.0 - 50).astype(dt), photometric="minisblack")
for bits, dt in ((8, np.uint8), (16, np.uint16)):
    save(f"rgb-u{bits}.tif", field((1 << bits) - 1, 3, dt), photometric="rgb")
    save(f"rgba-u{bits}-unassoc.tif", field((1 << bits) - 1, 4, dt), photometric="rgb",
         extrasamples=[2])
    save(f"rgba-u{bits}-assoc.tif", field((1 << bits) - 1, 4, dt), photometric="rgb",
         extrasamples=[1])
    save(f"cmyk-u{bits}.tif", field((1 << bits) - 1, 4, dt), photometric="separated")
    save(f"graya-u{bits}.tif", field((1 << bits) - 1, 2, dt), photometric="minisblack",
         extrasamples=[2])
save("rgb-f32.tif", (field(1000, 3, np.float64) / 9.0).astype(np.float32), photometric="rgb")
save("rgb-u8-extra2.tif", field(255, 5, np.uint8), photometric="rgb", extrasamples=[0, 0])
# 8-bit only: tifffile writes a 256-entry colormap for any depth (the corpus has 4-bit palettes).
for bits in (8,):
    cmap = (np.arange(3 * (1 << bits)).reshape(3, -1) * 997 % 65536).astype(np.uint16)
    save(f"palette-u{bits}.tif", field((1 << bits) - 1, 1, np.uint8), photometric="palette",
         colormap=cmap, bitspersample=bits if bits < 8 else None)
print(len(os.listdir(out)), "base images")
