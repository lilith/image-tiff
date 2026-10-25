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

/// TIFF LZW (MSB-first, early change) using only literal codes, with a Clear code every 250
/// codes so all codes stay 9 bits wide. `with_eoi = false` leaves out the trailing
/// EndOfInformation code, as some writers do.
pub fn lzw_literal(data: &[u8], with_eoi: bool) -> Vec<u8> {
    let mut codes = Vec::new();
    for (i, &b) in data.iter().enumerate() {
        if i % 250 == 0 {
            codes.push(256u16);
        }
        codes.push(u16::from(b));
    }
    if data.is_empty() {
        codes.push(256);
    }
    if with_eoi {
        codes.push(257);
    }
    let mut out = Vec::new();
    let (mut acc, mut nbits) = (0u32, 0u32);
    for code in codes {
        acc = (acc << 9) | u32::from(code);
        nbits += 9;
        while nbits >= 8 {
            out.push((acc >> (nbits - 8)) as u8);
            nbits -= 8;
        }
        acc &= (1 << nbits) - 1;
    }
    if nbits > 0 {
        out.push((acc << (8 - nbits)) as u8);
    }
    out
}

/// CCITT test encoders built on the fax crate's code tables and two-dimensional line
/// encoder. Rows are `true` for black pixels.
#[cfg(feature = "fax")]
pub mod ccitt {
    use fax34::maps::{black, white};
    use fax34::{BitWriter, Bits, Color};
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct Buffer {
        bits: Vec<bool>,
        muted: bool,
    }

    /// A bit sink shared between the fax crate's line encoder and the EOL / tag writer.
    #[derive(Clone, Default)]
    struct Sink(Rc<RefCell<Buffer>>);

    impl BitWriter for Sink {
        type Error = std::convert::Infallible;
        fn write(&mut self, bits: Bits) -> Result<(), Self::Error> {
            let mut buf = self.0.borrow_mut();
            if !buf.muted {
                for i in (0..bits.len).rev() {
                    buf.bits.push((bits.data >> i) & 1 == 1);
                }
            }
            Ok(())
        }
    }

    impl Sink {
        fn put(&self, data: u16, len: u8) {
            self.clone().write(Bits { data, len }).unwrap();
        }
        fn len(&self) -> usize {
            self.0.borrow().bits.len()
        }
        fn mute(&self, muted: bool) {
            self.0.borrow_mut().muted = muted;
        }
        fn bytes(&self) -> Vec<u8> {
            let bits = &self.0.borrow().bits;
            bits.chunks(8)
                .map(|c| {
                    c.iter()
                        .enumerate()
                        .fold(0u8, |b, (i, &v)| b | (u8::from(v) << (7 - i)))
                })
                .collect()
        }
    }

    fn run(sink: &Sink, black_run: bool, mut n: u32) {
        let code = |n: u32| {
            let n = n as u16;
            if black_run {
                black::encode(n)
            } else {
                white::encode(n)
            }
            .unwrap()
        };
        let mut w = sink.clone();
        while n >= 2560 {
            w.write(code(2560)).unwrap();
            n -= 2560;
        }
        if n >= 64 {
            w.write(code(n & !63)).unwrap();
            n &= 63;
        }
        w.write(code(n)).unwrap();
    }

    /// One-dimensional row: alternating runs starting with white.
    fn row_1d(sink: &Sink, row: &[bool]) {
        let mut color = false;
        let mut x = 0;
        while x < row.len() {
            let n = row[x..].iter().take_while(|&&p| p == color).count();
            run(sink, color, n as u32);
            x += n;
            color = !color;
        }
        // A row that ends on a black run needs no closing white run, but a row that starts
        // black opens with a zero-length white run, which the loop above already wrote.
    }

    fn pels(row: &[bool]) -> impl Iterator<Item = Color> + '_ {
        row.iter()
            .map(|&b| if b { Color::Black } else { Color::White })
    }

    /// Modified Huffman (compression 2): 1D rows, each padded to a byte boundary.
    pub fn modified_huffman(rows: &[Vec<bool>]) -> Vec<u8> {
        let sink = Sink::default();
        for row in rows {
            row_1d(&sink, row);
            while sink.len() % 8 != 0 {
                sink.put(0, 1);
            }
        }
        sink.bytes()
    }

    /// Group 3 with the given T4Options. With two-dimensional coding, every `k`-th row is
    /// one-dimensional and the rest are coded against the previous row.
    pub fn group3(rows: &[Vec<bool>], options: u32, k: usize) -> Vec<u8> {
        group3_rows(rows, options, k, true)
    }

    /// Group 3 without EOL codes, as some writers produce: rows back to back, with the 1D/2D
    /// tag bit still in front of each row when two-dimensional coding is on.
    pub fn group3_without_eol(rows: &[Vec<bool>], options: u32, k: usize) -> Vec<u8> {
        group3_rows(rows, options, k, false)
    }

    fn group3_rows(rows: &[Vec<bool>], options: u32, k: usize, eol: bool) -> Vec<u8> {
        let sink = Sink::default();
        let width = rows.first().map_or(0, |r| r.len()) as u32;
        let mut encoder = fax34::encoder::Encoder::new(sink.clone());
        for (y, row) in rows.iter().enumerate() {
            if eol {
                if options & 4 != 0 {
                    while (sink.len() + 12) % 8 != 0 {
                        sink.put(0, 1);
                    }
                }
                sink.put(1, 12);
            }
            let one_dimensional = options & 1 == 0 || y % k == 0;
            if options & 1 != 0 {
                sink.put(u16::from(one_dimensional), 1);
            }
            // The line encoder always runs so its reference row stays current; its output
            // is kept only for two-dimensional rows.
            sink.mute(one_dimensional);
            encoder.encode_line(pels(row), width).unwrap();
            sink.mute(false);
            if one_dimensional {
                row_1d(&sink, row);
            }
        }
        sink.bytes()
    }

    /// Group 4 rows without the end-of-facsimile-block marker.
    pub fn group4_without_eofb(rows: &[Vec<bool>]) -> Vec<u8> {
        let sink = Sink::default();
        let width = rows.first().map_or(0, |r| r.len()) as u32;
        let mut encoder = fax34::encoder::Encoder::new(sink.clone());
        for row in rows {
            encoder.encode_line(pels(row), width).unwrap();
        }
        sink.bytes()
    }

    /// Packs rows MSB-first, one bit per pixel, each row padded to a byte.
    pub fn pack(rows: &[Vec<bool>]) -> Vec<u8> {
        rows.iter()
            .flat_map(|row| {
                row.chunks(8)
                    .map(|c| {
                        c.iter()
                            .enumerate()
                            .fold(0u8, |b, (i, &v)| b | (u8::from(v) << (7 - i)))
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}
