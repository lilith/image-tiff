//! Builds small classic little-endian TIFF files in memory, so regression tests can describe
//! the exact tag layout they need without committing binary fixtures.

#![allow(dead_code)]

pub const BYTE: u16 = 1;
pub const ASCII: u16 = 2;
pub const SHORT: u16 = 3;
pub const LONG: u16 = 4;

/// Where [`Tiff::data`] lands in the file.
pub const DATA_OFFSET: u32 = 8;

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
    data: Vec<u8>,
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

    /// Raw bytes stored at file offset [`DATA_OFFSET`], ahead of the chunks, for tags that
    /// point into the file.
    pub fn data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    /// Leave out StripByteCounts / TileByteCounts.
    pub fn without_byte_counts(mut self) -> Self {
        self.omit_counts = true;
        self
    }

    pub fn build(mut self) -> Vec<u8> {
        let mut out = b"II\x2a\x00\0\0\0\0".to_vec();
        out.extend_from_slice(&self.data);
        if out.len() % 2 == 1 {
            out.push(0);
        }
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
        modified_huffman_aligned(rows, 8)
    }

    /// Modified Huffman with each row padded to a multiple of `align` bits: 8 for
    /// compression 2, 16 for CCITT RLEW (32771).
    pub fn modified_huffman_aligned(rows: &[Vec<bool>], align: usize) -> Vec<u8> {
        let sink = Sink::default();
        for row in rows {
            row_1d(&sink, row);
            while sink.len() % align != 0 {
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

/// A T.82 (JBIG) encoder for the lowest resolution layer, written from the recommendation's
/// flow diagrams (6.8.2), to make JBIG test data. Its output for the recommendation's
/// artificial test image reproduces the byte counts of T.82 Table 29.
pub mod jbig {
    /// T.82 Table 24: LSZ, NLPS, NMPS, SWITCH.
    const STATES: [(u16, u8, u8, u8); 113] = [
        (0x5a1d, 1, 1, 1),
        (0x2586, 14, 2, 0),
        (0x1114, 16, 3, 0),
        (0x080b, 18, 4, 0),
        (0x03d8, 20, 5, 0),
        (0x01da, 23, 6, 0),
        (0x00e5, 25, 7, 0),
        (0x006f, 28, 8, 0),
        (0x0036, 30, 9, 0),
        (0x001a, 33, 10, 0),
        (0x000d, 35, 11, 0),
        (0x0006, 9, 12, 0),
        (0x0003, 10, 13, 0),
        (0x0001, 12, 13, 0),
        (0x5a7f, 15, 15, 1),
        (0x3f25, 36, 16, 0),
        (0x2cf2, 38, 17, 0),
        (0x207c, 39, 18, 0),
        (0x17b9, 40, 19, 0),
        (0x1182, 42, 20, 0),
        (0x0cef, 43, 21, 0),
        (0x09a1, 45, 22, 0),
        (0x072f, 46, 23, 0),
        (0x055c, 48, 24, 0),
        (0x0406, 49, 25, 0),
        (0x0303, 51, 26, 0),
        (0x0240, 52, 27, 0),
        (0x01b1, 54, 28, 0),
        (0x0144, 56, 29, 0),
        (0x00f5, 57, 30, 0),
        (0x00b7, 59, 31, 0),
        (0x008a, 60, 32, 0),
        (0x0068, 62, 33, 0),
        (0x004e, 63, 34, 0),
        (0x003b, 32, 35, 0),
        (0x002c, 33, 9, 0),
        (0x5ae1, 37, 37, 1),
        (0x484c, 64, 38, 0),
        (0x3a0d, 65, 39, 0),
        (0x2ef1, 67, 40, 0),
        (0x261f, 68, 41, 0),
        (0x1f33, 69, 42, 0),
        (0x19a8, 70, 43, 0),
        (0x1518, 72, 44, 0),
        (0x1177, 73, 45, 0),
        (0x0e74, 74, 46, 0),
        (0x0bfb, 75, 47, 0),
        (0x09f8, 77, 48, 0),
        (0x0861, 78, 49, 0),
        (0x0706, 79, 50, 0),
        (0x05cd, 48, 51, 0),
        (0x04de, 50, 52, 0),
        (0x040f, 50, 53, 0),
        (0x0363, 51, 54, 0),
        (0x02d4, 52, 55, 0),
        (0x025c, 53, 56, 0),
        (0x01f8, 54, 57, 0),
        (0x01a4, 55, 58, 0),
        (0x0160, 56, 59, 0),
        (0x0125, 57, 60, 0),
        (0x00f6, 58, 61, 0),
        (0x00cb, 59, 62, 0),
        (0x00ab, 61, 63, 0),
        (0x008f, 61, 32, 0),
        (0x5b12, 65, 65, 1),
        (0x4d04, 80, 66, 0),
        (0x412c, 81, 67, 0),
        (0x37d8, 82, 68, 0),
        (0x2fe8, 83, 69, 0),
        (0x293c, 84, 70, 0),
        (0x2379, 86, 71, 0),
        (0x1edf, 87, 72, 0),
        (0x1aa9, 87, 73, 0),
        (0x174e, 72, 74, 0),
        (0x1424, 72, 75, 0),
        (0x119c, 74, 76, 0),
        (0x0f6b, 74, 77, 0),
        (0x0d51, 75, 78, 0),
        (0x0bb6, 77, 79, 0),
        (0x0a40, 77, 48, 0),
        (0x5832, 80, 81, 1),
        (0x4d1c, 88, 82, 0),
        (0x438e, 89, 83, 0),
        (0x3bdd, 90, 84, 0),
        (0x34ee, 91, 85, 0),
        (0x2eae, 92, 86, 0),
        (0x299a, 93, 87, 0),
        (0x2516, 86, 71, 0),
        (0x5570, 88, 89, 1),
        (0x4ca9, 95, 90, 0),
        (0x44d9, 96, 91, 0),
        (0x3e22, 97, 92, 0),
        (0x3824, 99, 93, 0),
        (0x32b4, 99, 94, 0),
        (0x2e17, 93, 86, 0),
        (0x56a8, 95, 96, 1),
        (0x4f46, 101, 97, 0),
        (0x47e5, 102, 98, 0),
        (0x41cf, 103, 99, 0),
        (0x3c3d, 104, 100, 0),
        (0x375e, 99, 93, 0),
        (0x5231, 105, 102, 0),
        (0x4c0f, 106, 103, 0),
        (0x4639, 107, 104, 0),
        (0x415e, 103, 99, 0),
        (0x5627, 105, 106, 1),
        (0x50e7, 108, 107, 0),
        (0x4b85, 109, 103, 0),
        (0x5597, 110, 109, 0),
        (0x504f, 111, 107, 0),
        (0x5a10, 110, 111, 1),
        (0x5522, 112, 109, 0),
        (0x59eb, 112, 111, 1),
    ];

    struct Coder {
        c: u32,
        a: u32,
        ct: u32,
        sc: u32,
        buffer: u32,
        out: Vec<u8>,
    }

    impl Coder {
        /// INITENC (Figure 27), without the estimator reset.
        fn new() -> Self {
            Coder {
                c: 0,
                a: 0x10000,
                ct: 11,
                sc: 0,
                buffer: 0,
                out: Vec::new(),
            }
        }

        /// BYTEOUT (Figure 26).
        fn byte_out(&mut self) {
            let temp = self.c >> 19;
            if temp > 0xff {
                self.out.push((self.buffer + 1) as u8);
                self.out.extend(std::iter::repeat_n(0x00, self.sc as usize));
                self.sc = 0;
                self.buffer = temp & 0xff;
            } else if temp == 0xff {
                self.sc += 1;
            } else {
                self.out.push(self.buffer as u8);
                self.out.extend(std::iter::repeat_n(0xff, self.sc as usize));
                self.sc = 0;
                self.buffer = temp;
            }
            self.c &= 0x7ffff;
            self.ct = 8;
        }

        /// RENORME (Figure 25).
        fn renormalize(&mut self) {
            loop {
                self.a <<= 1;
                self.c <<= 1;
                self.ct -= 1;
                if self.ct == 0 {
                    self.byte_out();
                }
                if self.a >= 0x8000 {
                    break;
                }
            }
        }

        /// ENCODE, CODELPS and CODEMPS (Figures 22-24).
        fn encode(&mut self, pix: u8, state: &mut u8, mps: &mut u8) {
            let (lsz, nlps, nmps, switch) = STATES[usize::from(*state)];
            let lsz = u32::from(lsz);
            self.a -= lsz;
            if pix != *mps {
                if self.a >= lsz {
                    self.c += self.a;
                    self.a = lsz;
                }
                if switch == 1 {
                    *mps = 1 - *mps;
                }
                *state = nlps;
                self.renormalize();
            } else if self.a < 0x8000 {
                if self.a < lsz {
                    self.c += self.a;
                    self.a = lsz;
                }
                *state = nmps;
                self.renormalize();
            }
        }

        /// FLUSH with CLEARBITS and FINALWRITES (Figures 28-30); trailing zero bytes removed.
        fn finish(mut self) -> Vec<u8> {
            let temp = (self.a - 1 + self.c) & 0xffff_0000;
            self.c = if temp < self.c { temp + 0x8000 } else { temp };
            self.c <<= self.ct;
            if self.c > 0x7ff_ffff {
                self.out.push((self.buffer + 1) as u8);
                self.out.extend(std::iter::repeat_n(0x00, self.sc as usize));
            } else {
                self.out.push(self.buffer as u8);
                self.out.extend(std::iter::repeat_n(0xff, self.sc as usize));
            }
            self.out.push((self.c >> 19) as u8);
            self.out.push((self.c >> 11) as u8);
            self.out.remove(0);
            while self.out.last() == Some(&0) {
                self.out.pop();
            }
            self.out
        }
    }

    /// Encoding parameters for the lowest resolution layer.
    #[derive(Clone, Default)]
    pub struct Params {
        pub two_line: bool,
        pub tpbon: bool,
        pub l0: u32,
        pub mx: u8,
        /// End stripes with SDRST instead of SDNORM.
        pub reset: bool,
        /// AT movements: (stripe, line within the stripe, tx), effective from that line.
        pub at_moves: Vec<(u32, u32, i8)>,
        /// Append a NEWLEN marker segment with this height (VLENGTH is set).
        pub new_length: Option<u32>,
        /// Put a comment marker segment before the first stripe.
        pub comment: bool,
    }

    /// Byte counts of an encoding, as T.82 Table 29 lists them.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct Counts {
        pub scd: usize,
        pub pscd: usize,
        pub sde: usize,
        pub bid: usize,
        pub bie: usize,
    }

    /// Encodes `rows` (one bool per pixel, true is foreground) as one bi-level image entity.
    pub fn encode(rows: &[Vec<bool>], params: &Params) -> (Vec<u8>, Counts) {
        let height = rows.len() as u32;
        let width = rows.first().map_or(0, |r| r.len()) as u32;
        let mut options = 0u8;
        if params.two_line {
            options |= 0x40;
        }
        if params.new_length.is_some() {
            options |= 0x20;
        }
        if params.tpbon {
            options |= 0x08;
        }
        let mut bie = vec![0, 0, 1, 0];
        bie.extend_from_slice(&width.to_be_bytes());
        bie.extend_from_slice(&height.to_be_bytes());
        bie.extend_from_slice(&params.l0.to_be_bytes());
        bie.extend_from_slice(&[params.mx, 0, 0, options]);
        let mut counts = Counts::default();
        if params.comment {
            bie.extend_from_slice(&[0xff, 0x07, 0, 0, 0, 3, b'a', 0xff, b'c']);
        }

        let px = |x: i64, y: i64, top: i64| -> usize {
            if x < 0 || x >= i64::from(width) || y < top {
                0
            } else {
                usize::from(rows[y as usize][x as usize])
            }
        };
        let mut state = vec![0u8; 1024];
        let mut mps = vec![0u8; 1024];
        let (mut tx, mut ty) = (0i64, 0i64);
        let mut lntp_prev = 1u8;
        let mut reset = true;
        let stripes = height.div_ceil(params.l0);
        for s in 0..stripes {
            let first = s * params.l0;
            let end = (first + params.l0).min(height);
            for &(stripe, line, new_tx) in &params.at_moves {
                if stripe == s {
                    bie.extend_from_slice(&[0xff, 0x06]);
                    bie.extend_from_slice(&line.to_be_bytes());
                    bie.extend_from_slice(&[new_tx as u8, 0]);
                }
            }
            if reset {
                state.fill(0);
                mps.fill(0);
                lntp_prev = 1;
                tx = 0;
                ty = 0;
            }
            let top = if reset { i64::from(first) } else { 0 };
            let top = if s == 0 { 0 } else { top };
            let mut coder = Coder::new();
            for y in first..end {
                for &(stripe, line, new_tx) in &params.at_moves {
                    if stripe == s && first + line == y {
                        tx = i64::from(new_tx);
                        ty = 0;
                    }
                }
                let yy = i64::from(y);
                if params.tpbon {
                    let above_differs =
                        (0..i64::from(width)).any(|x| px(x, yy, top) != px(x, yy - 1, top));
                    let lntp = u8::from(above_differs);
                    let slntp = u8::from(lntp == lntp_prev);
                    let cx = if params.two_line { 0x195 } else { 0x0e5 };
                    coder.encode(slntp, &mut state[cx], &mut mps[cx]);
                    lntp_prev = lntp;
                    if lntp == 0 {
                        continue;
                    }
                }
                for x in 0..i64::from(width) {
                    let at = |dx: i64, dy: i64| {
                        if tx == 0 && ty == 0 {
                            px(x + dx, yy + dy, top)
                        } else {
                            px(x - tx, yy - ty, top)
                        }
                    };
                    let cx = if params.two_line {
                        (px(x - 3, yy - 1, top) << 9)
                            | (px(x - 2, yy - 1, top) << 8)
                            | (px(x - 1, yy - 1, top) << 7)
                            | (px(x, yy - 1, top) << 6)
                            | (px(x + 1, yy - 1, top) << 5)
                            | (at(2, -1) << 4)
                            | (px(x - 4, yy, top) << 3)
                            | (px(x - 3, yy, top) << 2)
                            | (px(x - 2, yy, top) << 1)
                            | px(x - 1, yy, top)
                    } else {
                        (px(x - 1, yy - 2, top) << 9)
                            | (px(x, yy - 2, top) << 8)
                            | (px(x + 1, yy - 2, top) << 7)
                            | (px(x - 2, yy - 1, top) << 6)
                            | (px(x - 1, yy - 1, top) << 5)
                            | (px(x, yy - 1, top) << 4)
                            | (px(x + 1, yy - 1, top) << 3)
                            | (at(2, -1) << 2)
                            | (px(x - 2, yy, top) << 1)
                            | px(x - 1, yy, top)
                    };
                    let pix = px(x, yy, 0) as u8;
                    coder.encode(pix, &mut state[cx], &mut mps[cx]);
                }
            }
            let scd = coder.finish();
            counts.scd += scd.len();
            let mut pscd = Vec::with_capacity(scd.len());
            for &b in &scd {
                pscd.push(b);
                if b == 0xff {
                    pscd.push(0x00);
                }
            }
            counts.pscd += pscd.len();
            counts.sde += pscd.len() + 2;
            bie.extend_from_slice(&pscd);
            bie.extend_from_slice(&[0xff, if params.reset { 0x03 } else { 0x02 }]);
            reset = params.reset;
        }
        if let Some(new_height) = params.new_length {
            bie.extend_from_slice(&[0xff, 0x05]);
            bie.extend_from_slice(&new_height.to_be_bytes());
        }
        counts.bie = bie.len();
        counts.bid = bie.len() - 20;
        (bie, counts)
    }

    /// The artificial test image of T.82 7.2.1 (Figure 38): 1960 x 1951 pixels.
    pub fn artificial_image() -> Vec<Vec<bool>> {
        let mut prsg: u32 = 1;
        let mut repeat = [false; 8];
        (0..1951)
            .map(|j| {
                (0..1960usize)
                    .map(|i| {
                        if j < 192 {
                            false
                        } else if j < 1023 || (i >> 3) & 3 == 0 {
                            let sum = (prsg & 1)
                                + ((prsg >> 2) & 1)
                                + ((prsg >> 11) & 1)
                                + ((prsg >> 15) & 1);
                            prsg = (prsg << 1).wrapping_add(sum & 1);
                            let on = prsg & 3 == 0;
                            repeat[i & 7] = on;
                            on
                        } else {
                            repeat[i & 7]
                        }
                    })
                    .collect()
            })
            .collect()
    }
}
