#!/usr/bin/env python3
"""Compare image-tiff (examples/parity_dump.rs) with libtiff (parity/libtiff_dump.c).

Usage: compare.py <dump-dir> <results.tsv>

<dump-dir> holds, per input file `<name>`, `<name>.libtiff.<n>.{txt,raw}` and
`<name>.fork.<n>.{txt,raw}` for directory/image n. Both sides are turned into integer sample
arrays (planes, rows, samples per row) in libtiff's stored-sample domain, undoing the fork's
documented normalizations, then compared.

Result classes:
  exact            every sample identical (LogLuv: libtiff's XYZ through the fork's
                   f32 XYZ-to-sRGB matrix)
  differs          both decoded, samples differ (count and max difference reported)
  fork-fails       libtiff decodes, image-tiff errors
  fork-panics      image-tiff panicked
  libtiff-fails    image-tiff decodes, libtiff errors (image-tiff is more lenient)
  both-fail        neither decodes
  fork-missing     libtiff has this directory, image-tiff stopped before it
  jpeg             JPEG: libtiff converts YCbCr to RGB, image-tiff returns YCbCr; reports the
                   difference after converting image-tiff's samples with the JFIF formula.
                   Old-style JPEG: both sides YCbCr (libtiff's raw components with nearest
                   chroma, image-tiff's upsampled by its JPEG decoder); luma and chroma
                   differences reported separately
  skipped          not comparable yet (reason given)
"""
import os
import re
import sys

import numpy as np


def read_meta(path):
    meta = {}
    with open(path, errors="replace") as f:
        for line in f:
            if "=" in line:
                k, v = line.rstrip("\n").split("=", 1)
                meta[k] = v
    return meta


def unpack(rows_bytes, n_samples, bits, little=True):
    """rows_bytes: uint8 (rows, row_bytes). Returns uint64 (rows, n_samples)."""
    rows = rows_bytes.shape[0]
    if bits in (8, 16, 32, 64) and little:
        dt = {8: "<u1", 16: "<u2", 32: "<u4", 64: "<u8"}[bits]
        nbytes = n_samples * bits // 8
        return np.ascontiguousarray(rows_bytes[:, :nbytes]).view(dt).astype(np.uint64)
    if bits == 24 and little:
        b = rows_bytes[:, : n_samples * 3].reshape(rows, n_samples, 3).astype(np.uint64)
        return b[..., 0] | (b[..., 1] << 8) | (b[..., 2] << 16)
    # MSB-first packed bit fields.
    bitarr = np.unpackbits(rows_bytes, axis=1)[:, : n_samples * bits]
    bitarr = bitarr.reshape(rows, n_samples, bits).astype(np.uint64)
    weights = (np.uint64(1) << np.arange(bits - 1, -1, -1, dtype=np.uint64))
    return (bitarr * weights).sum(axis=2, dtype=np.uint64)


COLOR = re.compile(r"Ok\((\w+)(?:\((\d+)\)| \{ bit_depth: (\d+), num_samples: (\d+) \})\)")
SAMPLES = {"Gray": 1, "GrayA": 2, "RGB": 3, "RGBA": 4, "CMYK": 4, "CMYKA": 5, "YCbCr": 3,
           "Palette": 1, "Lab": 3}


def fork_samples(meta, raw):
    """image-tiff output as uint64 (planes, rows, samples per row), plus its kind and depth."""
    m = COLOR.match(meta.get("colortype", ""))
    if not m:
        return None, None, None, "unparsed colortype " + meta.get("colortype", "")
    kind = m.group(1)
    if m.group(2):
        depth, spp = int(m.group(2)), SAMPLES.get(kind)
    else:
        depth, spp = int(m.group(3)), int(m.group(4))
    if spp is None:
        return None, None, None, "unknown kind " + kind
    w, h = int(meta["width"]), int(meta["height"])
    planes = int(meta["planes"])
    row_stride = int(meta["row_stride"])
    plane_stride = int(meta["plane_stride"]) or len(raw)
    per_plane = spp // planes if planes > 1 else spp
    out_bits = depth if depth < 8 else (8 if depth == 8 else 16 if depth <= 16 else 32 if depth <= 32 else 64)
    arr = np.frombuffer(raw, np.uint8)
    result = []
    for p in range(planes):
        plane = arr[p * plane_stride : p * plane_stride + row_stride * h]
        if plane.size < row_stride * h:
            return None, None, None, "fork buffer short"
        result.append(unpack(plane.reshape(h, row_stride), w * per_plane, out_bits))
    return np.stack(result), kind, depth, None


def libtiff_samples(meta, raw):
    w, h = int(meta["width"]), int(meta["height"])
    spp, bps = int(meta["spp"]), int(meta["bps"])
    planes = int(meta["planes"])
    row_bytes = int(meta["row_bytes"])
    elem = 32 if meta["compression"] in ("34676", "34677") else bps
    per_plane = 1 if planes > 1 else spp
    arr = np.frombuffer(raw, np.uint8)
    result = []
    for p in range(planes):
        plane = arr[p * row_bytes * h : (p + 1) * row_bytes * h]
        result.append(unpack(plane.reshape(h, row_bytes), w * per_plane, elem))
    return np.stack(result)


def upsample_blocks(raw, meta, tile_dims):
    """libtiff's stored YCbCr block layout -> full-resolution (1, h, w*3), nearest chroma."""
    w, h = int(meta["width"]), int(meta["height"])
    sh, sv = (int(x) for x in meta["subsampling"].split(","))
    chunk_bytes, chunks = int(meta["chunk_bytes"]), int(meta["chunks"])
    cw, ch = tile_dims
    across = -(-w // cw)
    out = np.zeros((h, w, 3), np.uint64)
    data = np.frombuffer(raw, np.uint8)
    bw = -(-cw // sh)
    block = sh * sv + 2
    for c in range(chunks):
        chunk = data[c * chunk_bytes : (c + 1) * chunk_bytes]
        x0, y0 = (c % across) * cw, (c // across) * ch
        rows = min(ch, h - y0)
        nblock_rows = -(-rows // sv)
        for br in range(nblock_rows):
            for bc in range(bw):
                off = (br * bw + bc) * block
                if off + block > chunk.size:
                    continue
                ys = chunk[off : off + sh * sv].reshape(sv, sh)
                cb, cr = chunk[off + sh * sv], chunk[off + sh * sv + 1]
                for dy in range(sv):
                    for dx in range(sh):
                        y, x = y0 + br * sv + dy, x0 + bc * sh + dx
                        if y < min(h, y0 + ch) and x < min(w, x0 + cw):
                            out[y, x] = (ys[dy, dx], cb, cr)
    return out.reshape(1, h, w * 3)


def compare_one(name, n, d):
    lt_meta_path = os.path.join(d, f"{name}.libtiff.{n}.txt")
    fk_meta_path = os.path.join(d, f"{name}.fork.{n}.txt")
    lt = read_meta(lt_meta_path) if os.path.exists(lt_meta_path) else None
    fk = read_meta(fk_meta_path) if os.path.exists(fk_meta_path) else None
    info = ""
    if lt:
        info = (f"c={lt.get('compression')} ph={lt.get('photometric')} spp={lt.get('spp')} "
                f"bps={lt.get('bps')} fmt={lt.get('sampleformat')} pl={lt.get('planar')} "
                f"tiled={lt.get('tiled')}")
    lt_ok = lt is not None and lt.get("decoded") == "1"
    fk_ok = fk is not None and fk.get("decoded") == "1"
    if fk is not None and fk.get("panicked") == "1":
        return "fork-panics", info, ""
    if lt is not None and fk is None:
        return ("fork-missing" if lt_ok else "both-fail"), info, lt.get("messages", "")[:200]
    if not lt_ok and not fk_ok:
        return "both-fail", info, (fk or {}).get("messages", "")[:120]
    if lt_ok and not fk_ok:
        return "fork-fails", info, fk.get("messages", "")[:200]
    if fk_ok and not lt_ok:
        return "libtiff-fails", info, (lt or {}).get("messages", "")[:200]

    lt_raw = open(os.path.join(d, f"{name}.libtiff.{n}.raw"), "rb").read()
    fk_raw = open(os.path.join(d, f"{name}.fork.{n}.raw"), "rb").read()
    f_arr, kind, depth, err = fork_samples(fk, fk_raw)
    if err:
        return "skipped", info, err
    compression = lt["compression"]
    photometric = lt["photometric"]
    if compression == "7":
        return "jpeg", info, jpeg_diff(lt, lt_raw, f_arr, kind)
    if lt["layout"] == "ycbcr-blocks":
        w, h = int(lt["width"]), int(lt["height"])
        if lt["tiled"] == "1":
            dims = (int(lt["tilewidth"]), int(lt["tilelength"]))
        else:
            dims = (w, max(1, min(int(lt["rowsperstrip"]) or h, h)))
        l_arr = upsample_blocks(lt_raw, lt, dims)
    else:
        l_arr = libtiff_samples(lt, lt_raw)
    if compression == "6":
        return "jpeg", info, ojpeg_diff(l_arr, f_arr, int(lt["spp"]))
    notes = []
    # LogLuv: libtiff returns XYZ (SGILOGDATAFMT_FLOAT), the fork linear sRGB through its f32
    # matrix (src/decoder/logluv.rs xyz_to_rgb); convert libtiff's XYZ the same way.
    if photometric == "32845" and kind == "RGB" and l_arr.shape[-1] % 3 == 0:
        l_arr = xyz_to_rgb_bits(l_arr)
        notes.append("libtiff XYZ to RGB")
    # WhiteIsZero: the fork returns max - v for unsigned gray.
    if photometric == "0" and kind in ("Gray", "GrayA") and fk.get("sample_format") == "Uint":
        maxv = np.uint64((1 << int(lt["bps"])) - 1)
        f_arr = f_arr.copy()
        if kind == "Gray":
            f_arr = maxv - f_arr
        else:
            f_arr[..., 0::2] = maxv - f_arr[..., 0::2]
        notes.append("WhiteIsZero un-inverted")
    if f_arr.shape != l_arr.shape and f_arr.shape[0] < l_arr.shape[0] and f_arr.shape[1:] == l_arr.shape[1:]:
        # Planar: the fork returns the photometric (and alpha) planes, not unspecified extras.
        notes.append(f"fork keeps {f_arr.shape[0]} of {l_arr.shape[0]} planes")
        l_arr = l_arr[: f_arr.shape[0]]
    if f_arr.shape != l_arr.shape:
        lt_spp, f_spp = int(lt["spp"]), l_arr.shape[2]
        if (f_arr.shape[0] == l_arr.shape[0] == 1 and f_arr.shape[1] == l_arr.shape[1]
                and l_arr.shape[2] % int(lt["width"]) == 0 and f_arr.shape[2] % int(lt["width"]) == 0):
            w = int(lt["width"])
            ls, fs = l_arr.shape[2] // w, f_arr.shape[2] // w
            if fs < ls:
                l_arr = l_arr.reshape(1, l_arr.shape[1], w, ls)[..., :fs].reshape(1, l_arr.shape[1], w * fs)
                notes.append(f"fork keeps {fs} of {ls} samples")
        if f_arr.shape != l_arr.shape:
            return "differs", info, f"shape fork {f_arr.shape} libtiff {l_arr.shape}"
    diff = f_arr != l_arr
    if not diff.any():
        return "exact", info, "; ".join(notes)
    count = int(diff.sum())
    delta = np.abs(f_arr.astype(np.float64) - l_arr.astype(np.float64))
    idx = np.argwhere(diff)[0]
    return "differs", info, (f"{count} of {diff.size} samples, max |d| {delta.max():.0f}, first at "
                             f"{tuple(int(i) for i in idx)}: fork {int(f_arr[tuple(idx)])} libtiff "
                             f"{int(l_arr[tuple(idx)])}; " + "; ".join(notes))


def jpeg_diff(lt, lt_raw, f_arr, kind):
    """libtiff RGB (JPEGCOLORMODE_RGB) vs image-tiff YCbCr converted with the JFIF formula."""
    try:
        l_arr = libtiff_samples(lt, lt_raw).astype(np.float64)
    except Exception as e:  # noqa: BLE001
        return f"libtiff layout: {e}"
    f = f_arr.astype(np.float64)
    if kind == "YCbCr":
        y, cb, cr = f[..., 0::3], f[..., 1::3] - 128, f[..., 2::3] - 128
        rgb = np.empty_like(f)
        rgb[..., 0::3] = y + 1.402 * cr
        rgb[..., 1::3] = y - 0.344136 * cb - 0.714136 * cr
        rgb[..., 2::3] = y + 1.772 * cb
        f = np.clip(np.rint(rgb), 0, 255)
    if f.shape != l_arr.shape:
        return f"shape fork {f.shape} libtiff {l_arr.shape}"
    d = np.abs(f - l_arr)
    return f"max |d| {d.max():.0f}, {int((d > 0).sum())} of {d.size} samples differ"


def xyz_to_rgb_bits(arr):
    """XYZ float bit patterns to the fork's linear sRGB, rounding as its f32 code does."""
    f32 = np.float32
    xyz = arr.astype(np.uint32).view(np.float32).reshape(-1, 3)
    x, y, z = xyz[:, 0], xyz[:, 1], xyz[:, 2]
    with np.errstate(all="ignore"):
        r = (f32(3.2404542) * x + f32(-1.5371385) * y) + f32(-0.4985314) * z
        g = (f32(-0.969266) * x + f32(1.8760108) * y) + f32(0.0415560) * z
        b = (f32(0.0556434) * x + f32(-0.2040259) * y) + f32(1.0572252) * z
    rgb = np.stack([r, g, b], 1).astype(np.float32)
    return rgb.view(np.uint32).astype(np.uint64).reshape(arr.shape)


def ojpeg_diff(l_arr, f_arr, spp):
    """Old-style JPEG: libtiff's components against image-tiff's, luma and chroma apart."""
    if f_arr.shape != l_arr.shape:
        return f"shape fork {f_arr.shape} libtiff {l_arr.shape}"
    d = np.abs(f_arr.astype(np.int64) - l_arr.astype(np.int64))
    parts = [("Y", d[..., 0::spp])]
    if spp == 3:
        parts.append(("CbCr", np.concatenate([d[..., 1::3], d[..., 2::3]], axis=-1)))
    return "; ".join(f"{name} max |d| {p.max()}, {int((p > 0).sum())} of {p.size} differ"
                     for name, p in parts)


def main():
    d, out = sys.argv[1], sys.argv[2]
    names = sorted({f.rsplit(".libtiff.", 1)[0] for f in os.listdir(d) if ".libtiff." in f}
                   | {f.rsplit(".fork.", 1)[0] for f in os.listdir(d) if ".fork." in f})
    rows = []
    for name in names:
        ns = set()
        for f in os.listdir(d):
            for side in (".libtiff.", ".fork."):
                if f.startswith(name + side) and f.endswith(".txt") and not f.endswith("open.txt"):
                    ns.add(int(f[len(name + side):-4]))
        if os.path.exists(os.path.join(d, f"{name}.libtiff.open.txt")):
            fork_open = os.path.exists(os.path.join(d, f"{name}.fork.0.txt"))
            rows.append((name, "-", "libtiff-fails" if fork_open else "both-fail", "",
                         read_meta(os.path.join(d, f"{name}.libtiff.open.txt")).get("messages", "")[:200]))
        for n in sorted(ns):
            try:
                cls, info, detail = compare_one(name, n, d)
            except Exception as e:  # noqa: BLE001
                cls, info, detail = "skipped", "", f"comparator error: {e}"
            rows.append((name, str(n), cls, info, detail))
    with open(out, "w") as f:
        f.write("file\tifd\tresult\tparams\tdetail\n")
        for r in rows:
            f.write("\t".join(x.replace("\t", " ").replace("\n", " ") for x in r) + "\n")
    counts = {}
    for r in rows:
        counts[r[2]] = counts.get(r[2], 0) + 1
    print(" ".join(f"{k}={v}" for k, v in sorted(counts.items())), f"(of {len(rows)})")


if __name__ == "__main__":
    main()
