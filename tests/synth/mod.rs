//! Builds small classic little-endian TIFF files in memory, so regression tests can describe
//! the exact tag layout they need without committing binary fixtures.

#![allow(dead_code)]

pub const BYTE: u16 = 1;
pub const ASCII: u16 = 2;
pub const SHORT: u16 = 3;
pub const LONG: u16 = 4;

/// One IFD entry: tag, field type, count and the little-endian value bytes.
pub struct Entry {
    tag: u16,
    type_: u16,
    count: u32,
    bytes: Vec<u8>,
}

/// A single-image TIFF: tags plus the strip or tile payloads, in file order.
#[derive(Default)]
pub struct Tiff {
    entries: Vec<Entry>,
    chunks: Vec<Vec<u8>>,
    tiled: bool,
    omit_counts: bool,
}

impl Tiff {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn short(mut self, tag: u16, values: &[u16]) -> Self {
        let bytes = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.entries.push(Entry {
            tag,
            type_: SHORT,
            count: values.len() as u32,
            bytes,
        });
        self
    }

    pub fn long(mut self, tag: u16, values: &[u32]) -> Self {
        let bytes = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        self.entries.push(Entry {
            tag,
            type_: LONG,
            count: values.len() as u32,
            bytes,
        });
        self
    }

    pub fn ascii(mut self, tag: u16, value: &str) -> Self {
        let mut bytes = value.as_bytes().to_vec();
        bytes.push(0);
        self.entries.push(Entry {
            tag,
            type_: ASCII,
            count: bytes.len() as u32,
            bytes,
        });
        self
    }

    /// Strip payloads; StripOffsets and StripByteCounts are filled in by `build`.
    pub fn strips(mut self, chunks: Vec<Vec<u8>>) -> Self {
        self.chunks = chunks;
        self.tiled = false;
        self
    }

    /// Tile payloads; TileOffsets and TileByteCounts are filled in by `build`.
    pub fn tiles(mut self, chunks: Vec<Vec<u8>>) -> Self {
        self.chunks = chunks;
        self.tiled = true;
        self
    }

    /// Leave out StripByteCounts / TileByteCounts.
    pub fn without_byte_counts(mut self) -> Self {
        self.omit_counts = true;
        self
    }

    pub fn build(mut self) -> Vec<u8> {
        let mut out = b"II\x2a\x00\0\0\0\0".to_vec();
        let mut offsets = Vec::new();
        for chunk in &self.chunks {
            offsets.push(out.len() as u32);
            out.extend_from_slice(chunk);
            if out.len() % 2 == 1 {
                out.push(0);
            }
        }
        let counts: Vec<u32> = self.chunks.iter().map(|c| c.len() as u32).collect();
        let (offset_tag, count_tag) = if self.tiled { (324, 325) } else { (273, 279) };
        if !self.chunks.is_empty() {
            self = self.long(offset_tag, &offsets);
            if !self.omit_counts {
                self = self.long(count_tag, &counts);
            }
        }
        self.entries.sort_by_key(|e| e.tag);

        let ifd_offset = out.len() as u32;
        out[4..8].copy_from_slice(&ifd_offset.to_le_bytes());
        let ifd_len = 2 + 12 * self.entries.len() + 4;
        let mut extra = Vec::new();
        let extra_base = ifd_offset as usize + ifd_len;

        out.extend_from_slice(&(self.entries.len() as u16).to_le_bytes());
        for e in &self.entries {
            out.extend_from_slice(&e.tag.to_le_bytes());
            out.extend_from_slice(&e.type_.to_le_bytes());
            out.extend_from_slice(&e.count.to_le_bytes());
            if e.bytes.len() <= 4 {
                let mut inline = [0u8; 4];
                inline[..e.bytes.len()].copy_from_slice(&e.bytes);
                out.extend_from_slice(&inline);
            } else {
                let at = (extra_base + extra.len()) as u32;
                out.extend_from_slice(&at.to_le_bytes());
                extra.extend_from_slice(&e.bytes);
                if extra.len() % 2 == 1 {
                    extra.push(0);
                }
            }
        }
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&extra);
        out
    }
}

/// Baseline tags for an image: width, height, bits per sample (one per sample), samples per
/// pixel and compression. Photometric interpretation is left to the caller.
pub fn image(width: u32, height: u32, bits: u16, samples: u16, compression: u16) -> Tiff {
    Tiff::new()
        .long(256, &[width])
        .long(257, &[height])
        .short(258, &vec![bits; usize::from(samples)])
        .short(259, &[compression])
        .short(277, &[samples])
}

/// Splits a chunky (pixel-interleaved) image into tiles stored at the full tile size, with the
/// border tiles zero-padded as writers commonly do. `bpp` is bytes per pixel.
pub fn tile_chunky(
    pixels: &[u8],
    width: usize,
    height: usize,
    bpp: usize,
    tile_w: usize,
    tile_h: usize,
) -> Vec<Vec<u8>> {
    let mut tiles = Vec::new();
    for ty in (0..height).step_by(tile_h) {
        for tx in (0..width).step_by(tile_w) {
            let mut tile = vec![0u8; tile_w * tile_h * bpp];
            for y in 0..tile_h.min(height - ty) {
                let cols = tile_w.min(width - tx);
                let src = &pixels[((ty + y) * width + tx) * bpp..][..cols * bpp];
                tile[y * tile_w * bpp..][..cols * bpp].copy_from_slice(src);
            }
            tiles.push(tile);
        }
    }
    tiles
}

/// TIFF floating-point predictor (Predictor = 3) applied to one row of `samples`-sample
/// pixels: split each value into big-endian byte planes, then difference each byte against
/// the byte `samples` positions earlier.
pub fn float_predict_row(row: &[u8], bytes_per_value: usize, samples: usize) -> Vec<u8> {
    let n = row.len() / bytes_per_value;
    let mut planes = vec![0u8; row.len()];
    for i in 0..n {
        for b in 0..bytes_per_value {
            // Little-endian input value, big-endian plane order.
            planes[b * n + i] = row[i * bytes_per_value + (bytes_per_value - 1 - b)];
        }
    }
    let mut out = planes.clone();
    for i in samples..out.len() {
        out[i] = planes[i].wrapping_sub(planes[i - samples]);
    }
    out
}

#[cfg(feature = "deflate")]
pub fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).unwrap();
    enc.finish().unwrap()
}
