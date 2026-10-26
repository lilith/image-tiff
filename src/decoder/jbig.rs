//! JBIG (Compression 34661), the bi-level coding of ITU-T T.82 | ISO/IEC 11544, as libtiff
//! stores it: one bi-level image entity per strip or tile.
//!
//! One bit plane, decoded to the highest resolution through any differential layers, in the
//! layer-by-layer stripe orders jbigkit decodes. The decoder follows the recommendation: the QM
//! arithmetic decoder (6.8.3), typical prediction for the lowest and differential layers (6.4,
//! 6.5), deterministic prediction with the default or a private table (6.6), the lowest-layer
//! and differential templates with the adaptive pixel (6.7) and the stripe, edge and marker
//! rules (6.1.2, 6.2).

use crate::{TiffError, TiffFormatError, TiffResult, TiffUnsupportedError};

/// T.82 Table 24: LSZ, NLPS, NMPS and SWITCH for each probability estimation state.
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

const ESC: u8 = 0xFF;
const STUFF: u8 = 0x00;
const RESERVE: u8 = 0x01;
const SDNORM: u8 = 0x02;
const SDRST: u8 = 0x03;
const ABORT: u8 = 0x04;
const NEWLEN: u8 = 0x05;
const ATMOVE: u8 = 0x06;
const COMMENT: u8 = 0x07;

/// Order byte (T.82 Table 7).
const HITOLO: u8 = 0x08;
const SEQ: u8 = 0x04;

/// Options byte (T.82 Table 8).
const LRLTWO: u8 = 0x40;
const TPDON: u8 = 0x10;
const TPBON: u8 = 0x08;
const DPON: u8 = 0x04;
const DPPRIV: u8 = 0x02;
const DPLAST: u8 = 0x01;

/// The context of the lowest-resolution typical-prediction pseudo-pixel (T.82 Figures 11
/// and 12) in this decoder's bit order, see [`Template::context`].
const TPB_CONTEXT_THREE_LINE: usize = 0x0e5;
const TPB_CONTEXT_TWO_LINE: usize = 0x195;

fn corrupt(message: &str) -> TiffError {
    TiffError::FormatError(TiffFormatError::CompressedDataCorrupt(format!(
        "JBIG: {message}"
    )))
}

fn unsupported() -> TiffError {
    TiffError::UnsupportedError(TiffUnsupportedError::UnsupportedCompressionMethod(
        crate::tags::CompressionMethod::Unknown(34661),
    ))
}

fn be32(data: &[u8]) -> u32 {
    u32::from_be_bytes([data[0], data[1], data[2], data[3]])
}

/// The adaptive probability estimators: a state and a most probable symbol per context.
struct Model {
    state: Vec<u8>,
    mps: Vec<u8>,
}

impl Model {
    fn new(contexts: usize) -> Self {
        Model {
            state: vec![0; contexts],
            mps: vec![0; contexts],
        }
    }

    fn reset(&mut self) {
        self.state.fill(0);
        self.mps.fill(0);
    }
}

/// The arithmetic decoder of T.82 6.8.3 over one stripe's coded data, with the ESC STUFF pairs
/// already replaced by ESC.
struct ArithmeticDecoder<'a> {
    data: &'a [u8],
    pos: usize,
    c: u32,
    a: u32,
    ct: u32,
}

impl<'a> ArithmeticDecoder<'a> {
    /// INITDEC (Figure 37), without the estimator reset, which the caller owns.
    fn new(data: &'a [u8]) -> Self {
        let mut d = ArithmeticDecoder {
            data,
            pos: 0,
            c: 0,
            a: 0x10000,
            ct: 0,
        };
        d.byte_in();
        d.c <<= 8;
        d.byte_in();
        d.c <<= 8;
        d.byte_in();
        d
    }

    /// BYTEIN (Figure 36): past the end of the data, zero bytes are read.
    fn byte_in(&mut self) {
        let byte = self.data.get(self.pos).copied().unwrap_or(0);
        self.pos += 1;
        self.c = self.c.wrapping_add(u32::from(byte) << 8);
        self.ct = 8;
    }

    /// RENORMD (Figure 35).
    fn renormalize(&mut self) {
        loop {
            if self.ct == 0 {
                self.byte_in();
            }
            self.a <<= 1;
            self.c <<= 1;
            self.ct -= 1;
            if self.a >= 0x8000 {
                break;
            }
        }
        if self.ct == 0 {
            self.byte_in();
        }
    }

    /// DECODE (Figure 32) with LPS_EXCHANGE and MPS_EXCHANGE (Figures 33 and 34).
    fn decode(&mut self, state: &mut u8, mps: &mut u8) -> u8 {
        let (lsz, nlps, nmps, switch) = STATES[usize::from(*state)];
        let lsz = u32::from(lsz);
        self.a -= lsz;
        if (self.c >> 16) < self.a {
            if self.a >= 0x8000 {
                return *mps;
            }
            // MPS_EXCHANGE
            let pix = if self.a < lsz {
                let pix = 1 - *mps;
                if switch == 1 {
                    *mps = 1 - *mps;
                }
                *state = nlps;
                pix
            } else {
                *state = nmps;
                *mps
            };
            self.renormalize();
            pix
        } else {
            // LPS_EXCHANGE
            self.c -= self.a << 16;
            let pix = if self.a < lsz {
                *state = nmps;
                *mps
            } else {
                let pix = 1 - *mps;
                if switch == 1 {
                    *mps = 1 - *mps;
                }
                *state = nlps;
                pix
            };
            self.a = lsz;
            self.renormalize();
            pix
        }
    }

    fn decode_in(&mut self, model: &mut Model, context: usize) -> u8 {
        let (state, mps) = (&mut model.state[context], &mut model.mps[context]);
        self.decode(state, mps)
    }
}

/// The lowest-resolution-layer model template (T.82 6.7.1) with its adaptive pixel.
struct Template {
    two_line: bool,
    /// AT offsets: the pixel at (x - tx, y - ty); (0, 0) is the default location.
    tx: i64,
    ty: i64,
}

/// The decoded bitmap: rows of `row_bytes`, 1 bit per pixel, most significant bit first.
struct Bitmap {
    width: i64,
    row_bytes: usize,
    data: Vec<u8>,
    /// Rows above this one read as background: the top of the image, or the first row of the
    /// stripe after a reset (SDRST).
    top: i64,
}

impl Bitmap {
    fn get(&self, x: i64, y: i64) -> usize {
        if x < 0 || x >= self.width || y < self.top {
            return 0;
        }
        // Rows below the image are only read for deterministic-prediction phases that do not
        // use them.
        self.data
            .get(y as usize * self.row_bytes + (x as usize >> 3))
            .map_or(0, |&byte| usize::from((byte >> (7 - (x & 7))) & 1))
    }

    fn set(&mut self, x: i64, y: i64) {
        self.data[y as usize * self.row_bytes + (x as usize >> 3)] |= 0x80 >> (x & 7);
    }
}

impl Template {
    /// The context of pixel (x, y). Bits run left to right, top row first; the AT pixel takes
    /// the bit of its default location.
    fn context(&self, b: &Bitmap, x: i64, y: i64) -> usize {
        let at = |default_x: i64, default_y: i64| {
            if self.tx == 0 && self.ty == 0 {
                b.get(default_x, default_y)
            } else {
                b.get(x - self.tx, y - self.ty)
            }
        };
        if self.two_line {
            // Row y-1: x-3..x+1 and A at x+2; row y: x-4..x-1.
            (b.get(x - 3, y - 1) << 9)
                | (b.get(x - 2, y - 1) << 8)
                | (b.get(x - 1, y - 1) << 7)
                | (b.get(x, y - 1) << 6)
                | (b.get(x + 1, y - 1) << 5)
                | (at(x + 2, y - 1) << 4)
                | (b.get(x - 4, y) << 3)
                | (b.get(x - 3, y) << 2)
                | (b.get(x - 2, y) << 1)
                | b.get(x - 1, y)
        } else {
            // Row y-2: x-1..x+1; row y-1: x-2..x+1 and A at x+2; row y: x-2, x-1.
            (b.get(x - 1, y - 2) << 9)
                | (b.get(x, y - 2) << 8)
                | (b.get(x + 1, y - 2) << 7)
                | (b.get(x - 2, y - 1) << 6)
                | (b.get(x - 1, y - 1) << 5)
                | (b.get(x, y - 1) << 4)
                | (b.get(x + 1, y - 1) << 3)
                | (at(x + 2, y - 1) << 2)
                | (b.get(x - 2, y) << 1)
                | b.get(x - 1, y)
        }
    }
}

/// Scans the image data for a NEWLEN marker segment, as libtiff does with `jbg_newlen` before
/// decoding, and returns the new height if there is one.
fn new_length(data: &[u8]) -> Option<u32> {
    let mut pos = 0;
    while pos + 1 < data.len() {
        if data[pos] != ESC {
            pos += 1;
            continue;
        }
        match data[pos + 1] {
            NEWLEN => return data.get(pos + 2..pos + 6).map(be32),
            COMMENT => {
                let len = data.get(pos + 2..pos + 6).map(be32)? as usize;
                pos = pos.checked_add(6 + len)?;
            }
            ATMOVE => pos += 8,
            _ => pos += 2,
        }
    }
    None
}

/// The default deterministic-prediction tables (T.82 Tables 19 to 22, phases 0 to 3), packed
/// as a private table is (6.6.3): two bits per entry, four entries per byte, first entry in
/// the high bits.
const DEFAULT_DP_TABLE: [u8; 1728] = [
    0x2a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0x2a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x0a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0x2a, 0xaa, 0xaa, 0xaa, 0x0a, 0xaa, 0xaa, 0xaa, 0x22, 0x2a, 0xaa, 0xaa, 0x22, 0xaa, 0xaa, 0xaa,
    0x0a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa9, 0x22, 0x0a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa0, 0x00, 0x2a, 0xaa, 0xaa, 0xaa, 0x0a, 0xaa, 0xa5, 0x55,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x95, 0x55, 0x2a, 0xaa, 0xa5, 0x55, 0xaa, 0xaa, 0xa5, 0xa9,
    0x2a, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0xaa, 0xaa, 0x0a, 0xaa, 0xaa, 0xa0, 0x8a, 0xaa, 0xaa, 0xaa,
    0x2a, 0xaa, 0xa5, 0x55, 0xaa, 0xaa, 0xaa, 0x92, 0x5a, 0xaa, 0xaa, 0xa6, 0xaa, 0x0a, 0xaa, 0xa6,
    0x8a, 0xaa, 0xaa, 0xaa, 0x0a, 0x2a, 0xaa, 0xaa, 0x80, 0x2a, 0xaa, 0xaa, 0x00, 0x0a, 0xaa, 0xa9,
    0x8a, 0xaa, 0xaa, 0xaa, 0x5a, 0xaa, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xa9, 0xaa, 0x5a, 0xaa, 0xa9,
    0x82, 0x0a, 0xaa, 0xaa, 0xa0, 0x0a, 0xaa, 0xaa, 0x88, 0x82, 0xaa, 0xaa, 0x8a, 0x02, 0xaa, 0xaa,
    0xa0, 0x0a, 0xaa, 0xaa, 0x0a, 0x0a, 0xaa, 0xa9, 0x99, 0xa2, 0xaa, 0xaa, 0xaa, 0x02, 0xaa, 0xaa,
    0xaa, 0xaa, 0x6a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0x2a, 0xaa, 0x5a, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0x2a, 0xaa, 0x5a, 0xaa, 0xaa, 0x5a, 0xaa, 0xaa,
    0x08, 0xaa, 0x55, 0x55, 0x08, 0x2a, 0x55, 0x55, 0x0a, 0xaa, 0x96, 0xaa, 0x4a, 0xaa, 0xa5, 0x6a,
    0x2a, 0xaa, 0x5a, 0xaa, 0x0a, 0xaa, 0x9a, 0xaa, 0xaa, 0xaa, 0xa8, 0xa8, 0xaa, 0x0a, 0xa5, 0xaa,
    0x8a, 0xaa, 0x9a, 0xaa, 0x82, 0x0a, 0xaa, 0xaa, 0x80, 0x2a, 0xaa, 0xa8, 0xa0, 0x0a, 0xaa, 0xa6,
    0x8a, 0x2a, 0xa9, 0x55, 0xa2, 0x2a, 0xa5, 0x9a, 0xa0, 0x0a, 0xa6, 0x9a, 0xa0, 0xaa, 0xaa, 0xaa,
    0x82, 0x0a, 0xaa, 0xa0, 0xa0, 0x0a, 0xaa, 0xa6, 0xa8, 0x8a, 0xaa, 0xaa, 0x88, 0xa2, 0xaa, 0xa6,
    0xa8, 0x8a, 0xaa, 0xa0, 0x00, 0x8a, 0xaa, 0xa6, 0xaa, 0xa2, 0xaa, 0xa9, 0xa0, 0xa8, 0xa9, 0xa9,
    0x2a, 0xaa, 0xa5, 0x5a, 0x2a, 0xaa, 0x5a, 0xaa, 0xa9, 0x9a, 0xaa, 0x00, 0xa6, 0x9a, 0xa8, 0x80,
    0x00, 0x00, 0xaa, 0xaa, 0x2a, 0xaa, 0xa0, 0x00, 0xa0, 0xaa, 0xaa, 0xaa, 0xa0, 0x96, 0xaa, 0xaa,
    0x8a, 0xaa, 0x9a, 0x5a, 0x8a, 0xaa, 0xa6, 0x6a, 0xa0, 0x0a, 0xa5, 0x9a, 0x2a, 0xaa, 0xa9, 0xaa,
    0x8a, 0xaa, 0xaa, 0x8a, 0x02, 0xaa, 0x95, 0x6a, 0x20, 0x0a, 0xaa, 0xa0, 0xa0, 0xa6, 0xaa, 0xaa,
    0xa2, 0x2a, 0xa5, 0x5a, 0xaa, 0x8a, 0xaa, 0xa0, 0xaa, 0x8a, 0xaa, 0xa6, 0xaa, 0xa2, 0xaa, 0xa9,
    0xa0, 0x0a, 0xaa, 0xaa, 0x08, 0x6a, 0xaa, 0xa8, 0xa2, 0xa2, 0xaa, 0xaa, 0xa0, 0xa0, 0xaa, 0xaa,
    0xa8, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xa9, 0xa8, 0xa8, 0xaa, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0xa0, 0xa2, 0xaa, 0xa9, 0x88, 0xa8, 0xaa, 0xaa, 0xa0, 0xa8, 0xaa, 0xaa,
    0xaa, 0xaa, 0x55, 0x55, 0xaa, 0xaa, 0x55, 0x55, 0x2a, 0xaa, 0x95, 0x55, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xaa, 0x55, 0x55, 0x2a, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa, 0x82,
    0x0a, 0xaa, 0x55, 0x55, 0xaa, 0xa2, 0xaa, 0x59, 0x6a, 0xaa, 0xaa, 0xa6, 0xa0, 0xa2, 0xaa, 0x55,
    0x2a, 0xaa, 0x9a, 0xaa, 0xaa, 0xaa, 0xa8, 0x62, 0xaa, 0x2a, 0xa4, 0x6a, 0xa0, 0x0a, 0xaa, 0xa9,
    0x8a, 0xaa, 0xa5, 0x95, 0x52, 0x2a, 0xa9, 0x55, 0xa8, 0x82, 0xaa, 0xaa, 0x08, 0xaa, 0xaa, 0xa6,
    0xa2, 0x0a, 0xaa, 0xa5, 0xa0, 0x0a, 0xaa, 0xa6, 0xaa, 0x8a, 0xaa, 0xa6, 0xaa, 0xa2, 0xaa, 0xa6,
    0xa0, 0x8a, 0xa9, 0x65, 0xaa, 0xa6, 0xaa, 0xa9, 0x9a, 0x92, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0x99,
    0xa6, 0x5a, 0xaa, 0xaa, 0xa5, 0x5a, 0xaa, 0x2a, 0xaa, 0xa0, 0xaa, 0xa9, 0xa0, 0x02, 0xaa, 0xa9,
    0x00, 0x00, 0x55, 0x55, 0x2a, 0xaa, 0x9a, 0xaa, 0x8a, 0xaa, 0xa6, 0x55, 0xaa, 0x2a, 0xa6, 0x5a,
    0x0a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x96, 0x6a, 0x82, 0x2a, 0xa5, 0x55, 0xaa, 0x0a, 0xa9, 0x9a,
    0x8a, 0x0a, 0xa5, 0x55, 0xa2, 0x0a, 0xaa, 0x59, 0xa0, 0x0a, 0xaa, 0x9a, 0xaa, 0x0a, 0xaa, 0xa5,
    0x82, 0x2a, 0xa5, 0x55, 0xa0, 0xaa, 0xa9, 0x5a, 0x56, 0xaa, 0xaa, 0x95, 0xaa, 0xa2, 0xaa, 0xa4,
    0xa8, 0x0a, 0xaa, 0xaa, 0xa6, 0x8a, 0xaa, 0xa6, 0xaa, 0xa2, 0xaa, 0xa5, 0xaa, 0xa0, 0xaa, 0xa9,
    0xaa, 0x8a, 0xaa, 0xaa, 0xa6, 0x6a, 0xaa, 0xaa, 0xa0, 0x00, 0xaa, 0xa5, 0xa0, 0x00, 0xaa, 0xa5,
    0xaa, 0xa2, 0xaa, 0xa5, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xa0, 0xaa, 0xa9, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa8, 0xaa, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0xaa, 0xa6,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0xa5, 0x59, 0xa0, 0x08, 0xaa, 0x5a, 0xaa, 0x0a,
    0xaa, 0xaa, 0xaa, 0xaa, 0x80, 0x00, 0x95, 0x55, 0x80, 0x00, 0x95, 0x55, 0xa0, 0x0a, 0xa5, 0x5a,
    0x82, 0xaa, 0x96, 0xaa, 0xaa, 0x6a, 0xaa, 0x2a, 0xa8, 0x2a, 0xa9, 0x6a, 0xa0, 0xaa, 0xa5, 0xaa,
    0x80, 0x00, 0x95, 0x55, 0xa0, 0x0a, 0xa5, 0x5a, 0xa8, 0x8a, 0xa9, 0x9a, 0x88, 0x88, 0x99, 0x99,
    0xa9, 0x95, 0xa8, 0x80, 0x00, 0x8a, 0x55, 0x9a, 0xaa, 0xa6, 0xaa, 0xa2, 0xa2, 0xaa, 0xa6, 0xaa,
    0x2a, 0xaa, 0x6a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0x20, 0xa6, 0x65, 0xa9, 0x65, 0xa8, 0x20,
    0x00, 0x00, 0x55, 0x55, 0x00, 0x00, 0x55, 0x55, 0xa0, 0x00, 0xa5, 0x55, 0xa0, 0xa8, 0xa5, 0xa9,
    0xaa, 0xaa, 0xaa, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa, 0xaa, 0x6a, 0xaa, 0x2a, 0x22, 0x1a, 0x66, 0x4a,
    0x80, 0x00, 0x95, 0x55, 0x2a, 0xaa, 0x6a, 0xaa, 0x20, 0x10, 0x65, 0x45, 0xa0, 0xa8, 0xa5, 0xa9,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x95, 0xaa, 0x80, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa, 0xa2, 0xaa, 0xa6,
    0xa0, 0x00, 0xa5, 0x55, 0x08, 0x80, 0x59, 0x95, 0xa2, 0xa0, 0xa6, 0xa5, 0xa0, 0xa4, 0xa5, 0xa1,
    0xa8, 0xa6, 0xa9, 0xa2, 0xaa, 0xa6, 0xaa, 0xa2, 0xaa, 0xaa, 0xaa, 0xaa, 0xa8, 0xa8, 0xa9, 0xa9,
    0xaa, 0xa8, 0xaa, 0xa9, 0xa0, 0xa2, 0xa5, 0xa6, 0x88, 0xa8, 0x99, 0xa9, 0xa0, 0xa8, 0xa5, 0xa9,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0xaa, 0x56, 0xaa, 0x02,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0xaa, 0xa6, 0xa5, 0x59, 0xa0, 0x08, 0xa5, 0xaa, 0xa0, 0xaa,
    0x2a, 0xaa, 0x6a, 0xaa, 0xa5, 0xa6, 0xa0, 0xa2, 0xa9, 0x9a, 0xa8, 0x8a, 0xa1, 0x1a, 0xa4, 0x4a,
    0x8a, 0x2a, 0x9a, 0x6a, 0xa6, 0xaa, 0xa2, 0xaa, 0xa9, 0x95, 0xa8, 0x80, 0x08, 0x0a, 0x59, 0x5a,
    0xa5, 0x5a, 0xa0, 0x0a, 0xa0, 0x0a, 0xa5, 0x5a, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa,
    0xa2, 0x8a, 0xa6, 0x9a, 0xaa, 0x0a, 0xaa, 0x5a, 0xaa, 0xa6, 0xaa, 0xa2, 0xaa, 0x2a, 0xaa, 0x6a,
    0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0xaa, 0xa6, 0xa5, 0x56, 0xa0, 0x02,
    0xaa, 0xaa, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0x00, 0x00, 0x55, 0x55, 0xaa, 0xaa, 0xaa, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa, 0xa2, 0x2a, 0xa6, 0x6a,
    0x8a, 0xaa, 0x9a, 0xaa, 0xa5, 0xa6, 0xa0, 0xa2, 0xa1, 0x2a, 0xa4, 0x6a, 0xaa, 0x5a, 0xaa, 0x0a,
    0x9a, 0xaa, 0x8a, 0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xa8, 0x6a, 0xa9, 0x2a, 0xaa, 0xa8, 0xaa, 0xa9,
    0xa9, 0x55, 0xa8, 0x00, 0xa8, 0x8a, 0xa9, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0xaa, 0xa6,
    0xaa, 0x80, 0xaa, 0x95, 0xaa, 0xa2, 0xaa, 0xa6, 0xa5, 0x5a, 0xa0, 0x0a, 0xa0, 0x5a, 0xa5, 0x0a,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa8, 0xaa, 0xa9,
    0x2a, 0xaa, 0x6a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x8a, 0xaa, 0x9a, 0xaa, 0xaa, 0x56, 0xaa, 0x02,
    0xa2, 0x00, 0xa6, 0x55, 0xa6, 0x95, 0xa2, 0x80, 0xa6, 0xaa, 0xa2, 0xaa, 0x21, 0x2a, 0x64, 0x6a,
    0x8a, 0xaa, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0x1a, 0xa6, 0x4a, 0xa5, 0xaa, 0xa0, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0xa9, 0xa5, 0xa8, 0xa0, 0xaa, 0x86, 0xaa, 0x92, 0xaa, 0xa6, 0xaa, 0xa2,
    0xa5, 0x95, 0xa0, 0x80, 0xa8, 0x8a, 0xa9, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0x88, 0xaa, 0x99,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa,
    0xa5, 0x5a, 0xa0, 0x0a, 0x8a, 0xaa, 0x9a, 0xaa, 0xa5, 0xaa, 0xa0, 0xaa, 0xa2, 0x2a, 0xa6, 0x6a,
    0xaa, 0xaa, 0xaa, 0xaa, 0x9a, 0xaa, 0x8a, 0xaa, 0xaa, 0x00, 0xaa, 0x55, 0xaa, 0x80, 0xaa, 0x95,
    0xaa, 0x5a, 0xaa, 0x0a, 0xa2, 0x2a, 0xa6, 0x6a, 0xaa, 0xaa, 0xaa, 0xaa, 0xa5, 0x5a, 0xa0, 0x0a,
    0xa0, 0x20, 0xa5, 0x65, 0xa8, 0x6a, 0xa9, 0x2a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa0, 0xaa, 0xa5,
    0xa8, 0x8a, 0xa9, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xa8, 0xaa, 0xa9,
    0xaa, 0xa0, 0xaa, 0xa5, 0xaa, 0x52, 0xaa, 0x06, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0x00, 0x00, 0x55, 0x55, 0x00, 0x00, 0x55, 0x55, 0x80, 0x00, 0x95, 0x55, 0x2a, 0xaa, 0x6a, 0xaa,
    0x00, 0x00, 0x55, 0x55, 0xaa, 0xaa, 0xaa, 0xaa, 0x9a, 0xaa, 0x8a, 0xaa, 0xaa, 0x2a, 0xaa, 0x6a,
    0xa0, 0x00, 0xa5, 0x55, 0xaa, 0x08, 0xaa, 0x59, 0x21, 0x00, 0x64, 0x55, 0xaa, 0x08, 0xaa, 0x59,
    0x8a, 0xaa, 0x9a, 0xaa, 0xa2, 0x2a, 0xa6, 0x6a, 0xa6, 0x8a, 0xa2, 0x9a, 0xaa, 0xa8, 0xaa, 0xa9,
    0xa0, 0x00, 0xa5, 0x55, 0x08, 0x80, 0x59, 0x95, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0x02, 0xaa, 0x56,
    0xa8, 0xa0, 0xa9, 0xa5, 0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xa2, 0xaa, 0xa6, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0x20, 0xaa, 0x65, 0xaa, 0x04, 0xaa, 0x51, 0x8a, 0x88, 0x9a, 0x99, 0xaa, 0x08, 0xaa, 0x59,
    0xa5, 0x5a, 0xa0, 0x0a, 0xa5, 0x8a, 0xa0, 0x9a, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa8, 0xaa, 0xa9,
    0xaa, 0xaa, 0xaa, 0xaa, 0x9a, 0xaa, 0x8a, 0xaa, 0xa2, 0x00, 0xa6, 0x55, 0xa6, 0x8a, 0xa2, 0x9a,
    0xa0, 0x00, 0xa5, 0x55, 0x92, 0x2a, 0x86, 0x6a, 0xa8, 0x80, 0xa9, 0x95, 0xa0, 0xaa, 0xa5, 0xaa,
    0xa0, 0xa0, 0xa5, 0xa5, 0xa8, 0xa0, 0xa9, 0xa5, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xa0, 0xaa, 0xa5,
    0xa8, 0x80, 0xa9, 0x95, 0xaa, 0x0a, 0xaa, 0x5a, 0x00, 0x00, 0x55, 0x55, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xa0, 0xaa, 0xa5, 0xa5, 0xa2, 0xa0, 0xa6, 0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xa0, 0xaa, 0xa5, 0xa6, 0x66, 0xa2, 0x22, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xa8, 0xaa, 0xa9, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
    0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa,
];

/// Offsets of the four phases' entries in a DP table.
const DP_PHASE_START: [usize; 4] = [0, 256, 768, 2816];

/// The context of the differential-layer typical-prediction pseudo-pixel (T.82 Figure 8):
/// phase 3, the six high-resolution template pixels foreground, the four low-resolution ones
/// background, in the bit order of [`differential_context`].
const TPD_CONTEXT: usize = 0xfc3;

/// Decoding state of one resolution layer.
struct Layer {
    bitmap: Bitmap,
    height: u32,
    /// Lines per stripe.
    lines: u32,
    model: Model,
    template: Template,
    lntp: u8,
    reset: bool,
    /// First low-resolution row the current stripe may read: rows above it are background
    /// after a reset.
    low_top: i64,
}

/// Reads low-resolution pixels for one stripe of the layer above, with the edge rules of
/// T.82 6.1.2: background to the left, right and top (or above the stripe after a reset), and
/// rows below the stripe replicate its last row.
struct LowRes<'a> {
    bitmap: &'a Bitmap,
    top: i64,
    last: i64,
}

impl LowRes<'_> {
    fn get(&self, x: i64, y: i64) -> usize {
        if y < self.top {
            return 0;
        }
        let y = y.min(self.last);
        if x < 0 || x >= self.bitmap.width {
            return 0;
        }
        let byte = self.bitmap.data[y as usize * self.bitmap.row_bytes + (x as usize >> 3)];
        usize::from((byte >> (7 - (x & 7))) & 1)
    }
}

/// The differential-layer context (T.82 Figure 16): the high-resolution pixels at (x, y-2),
/// the AT pixel (default (x-1, y-1)), (x, y-1), (x+1, y-1), (x-2, y) and (x-1, y); the four
/// low-resolution pixels at and right of (or left of, for even x) the pixel's own, in its row
/// and the next; and the spatial phase.
fn differential_context(t: &Template, h: &Bitmap, l: &LowRes, x: i64, y: i64) -> usize {
    let at = if t.tx == 0 && t.ty == 0 {
        h.get(x - 1, y - 1)
    } else {
        h.get(x - t.tx, y - t.ty)
    };
    let (lx, ly) = (x >> 1, y >> 1);
    let left = if x & 1 == 0 { lx - 1 } else { lx };
    (h.get(x, y - 2) << 11)
        | (at << 10)
        | (h.get(x, y - 1) << 9)
        | (h.get(x + 1, y - 1) << 8)
        | (h.get(x - 2, y) << 7)
        | (h.get(x - 1, y) << 6)
        | (l.get(left, ly) << 5)
        | (l.get(left + 1, ly) << 4)
        | (l.get(left, ly + 1) << 3)
        | (l.get(left + 1, ly + 1) << 2)
        | (((y & 1) << 1) | (x & 1)) as usize
}

/// Deterministic prediction (T.82 6.6): 0 or 1 when the pixel is determined, 2 otherwise.
fn deterministic_prediction(table: &[u8], h: &Bitmap, l: &LowRes, x: i64, y: i64) -> u8 {
    let (lx, ly) = (x >> 1, y >> 1);
    let (hx, hy) = (lx * 2, ly * 2);
    // Figure 13: low-resolution 0-3, high-resolution 4-12 around the 2x2 block.
    let pixels = [
        l.get(lx - 1, ly - 1),
        l.get(lx, ly - 1),
        l.get(lx - 1, ly),
        l.get(lx, ly),
        h.get(hx - 1, hy - 1),
        h.get(hx, hy - 1),
        h.get(hx + 1, hy - 1),
        h.get(hx - 1, hy),
        h.get(hx, hy),
        h.get(hx + 1, hy),
        h.get(hx - 1, hy + 1),
        h.get(hx, hy + 1),
    ];
    let phase = (((y & 1) << 1) | (x & 1)) as usize;
    let used = [8, 9, 11, 12][phase];
    let index = pixels[..used]
        .iter()
        .enumerate()
        .fold(0, |acc, (bit, &p)| acc | (p << bit));
    let entry = DP_PHASE_START[phase] + index;
    (table[entry / 4] >> (6 - 2 * (entry % 4))) & 3
}

/// Decodes one stripe of the lowest resolution layer.
fn decode_lowest_stripe(
    layer: &mut Layer,
    decoder: &mut ArithmeticDecoder,
    tpbon: bool,
    first: u32,
    end: u32,
    moves: &[(u32, i64, i64)],
) {
    let tpb_context = if layer.template.two_line {
        TPB_CONTEXT_TWO_LINE
    } else {
        TPB_CONTEXT_THREE_LINE
    };
    let row_bytes = layer.bitmap.row_bytes;
    for y in first..end {
        apply_moves(&mut layer.template, moves, first, y);
        let yy = i64::from(y);
        if tpbon {
            let slntp = decoder.decode_in(&mut layer.model, tpb_context);
            layer.lntp = u8::from(slntp == layer.lntp);
            if layer.lntp == 0 {
                // A typical line repeats the line above.
                if yy > layer.bitmap.top {
                    let (above, row) =
                        layer.bitmap.data[(y as usize - 1) * row_bytes..].split_at_mut(row_bytes);
                    row[..row_bytes].copy_from_slice(above);
                }
                continue;
            }
        }
        for x in 0..layer.bitmap.width {
            let context = layer.template.context(&layer.bitmap, x, yy);
            if decoder.decode_in(&mut layer.model, context) == 1 {
                layer.bitmap.set(x, yy);
            }
        }
    }
}

/// Decodes one stripe of a differential layer from the layer below.
#[allow(clippy::too_many_arguments)]
fn decode_differential_stripe(
    layer: &mut Layer,
    low: &LowRes,
    decoder: &mut ArithmeticDecoder,
    tpdon: bool,
    dp_table: Option<&[u8]>,
    first: u32,
    end: u32,
    moves: &[(u32, i64, i64)],
) {
    for y in first..end {
        apply_moves(&mut layer.template, moves, first, y);
        let yy = i64::from(y);
        // The pseudo-pixel LNTP starts each line pair.
        if tpdon && y & 1 == 0 {
            layer.lntp = decoder.decode_in(&mut layer.model, TPD_CONTEXT);
        }
        for x in 0..layer.bitmap.width {
            let (lx, ly) = (x >> 1, yy >> 1);
            if tpdon && layer.lntp == 0 {
                let color = low.get(lx, ly);
                let uniform =
                    (-1..=1).all(|dy| (-1..=1).all(|dx| low.get(lx + dx, ly + dy) == color));
                if uniform {
                    if color == 1 {
                        layer.bitmap.set(x, yy);
                    }
                    continue;
                }
            }
            if let Some(table) = dp_table {
                match deterministic_prediction(table, &layer.bitmap, low, x, yy) {
                    0 => continue,
                    1 => {
                        layer.bitmap.set(x, yy);
                        continue;
                    }
                    _ => {}
                }
            }
            let context = differential_context(&layer.template, &layer.bitmap, low, x, yy);
            if decoder.decode_in(&mut layer.model, context) == 1 {
                layer.bitmap.set(x, yy);
            }
        }
    }
}

/// Applies the AT movements that take effect at line `y` of the stripe starting at `first`.
fn apply_moves(template: &mut Template, moves: &[(u32, i64, i64)], first: u32, y: u32) {
    for &(line, tx, ty) in moves {
        if first.checked_add(line) == Some(y) {
            template.tx = tx;
            template.ty = ty;
        }
    }
}

/// Decodes one bi-level image entity to rows of `ceil(XD / 8)` bytes, `YD` rows (after a
/// NEWLEN marker segment, if any). `limit` bounds the decoded size.
pub(crate) fn decode(bie: &[u8], limit: usize) -> TiffResult<Vec<u8>> {
    let bih = bie.get(..20).ok_or_else(|| corrupt("header truncated"))?;
    let (dl, d, planes) = (bih[0], bih[1], bih[2]);
    let (mut width, mut height, l0) = (be32(&bih[4..]), be32(&bih[8..]), be32(&bih[12..]));
    let (mx, order, options) = (bih[16], bih[18], bih[19]);
    // Several bit planes, or an entity that continues a previous one, are not taken; neither
    // are the progressive orders jbigkit does not decode (HITOLO, SEQ) with differential layers.
    if dl != 0 || planes != 1 || (d > 0 && order & (HITOLO | SEQ) != 0) {
        return Err(unsupported());
    }
    if width == 0 || height == 0 || l0 == 0 || mx > 127 || d > 31 {
        return Err(corrupt("invalid header"));
    }
    let mut pos = 20;
    let mut dp_table: &[u8] = &DEFAULT_DP_TABLE;
    if options & DPON != 0 && options & DPPRIV != 0 && options & DPLAST == 0 {
        dp_table = bie
            .get(20..20 + 1728)
            .ok_or_else(|| corrupt("header truncated"))?;
        pos += 1728;
    }
    let data = bie.get(pos..).ok_or_else(|| corrupt("header truncated"))?;
    if let Some(new_height) = new_length(data) {
        if new_height > 0 && new_height <= height {
            height = new_height;
        }
    }

    // Layer sizes halve, rounding up, from the highest resolution down (T.82 6.2.3).
    let mut sizes = vec![(width, height)];
    for _ in 0..d {
        (width, height) = (width.div_ceil(2), height.div_ceil(2));
        sizes.push((width, height));
    }
    sizes.reverse();
    let mut total = 0usize;
    let mut layers = Vec::with_capacity(sizes.len());
    for (index, &(w, h)) in sizes.iter().enumerate() {
        let row_bytes = (w as usize).div_ceil(8);
        let size = row_bytes
            .checked_mul(h as usize)
            .ok_or(TiffError::LimitsExceeded)?;
        total = total.checked_add(size).ok_or(TiffError::LimitsExceeded)?;
        if total > limit {
            return Err(TiffError::LimitsExceeded);
        }
        let lines = l0
            .checked_shl(index as u32)
            .filter(|&n| n >> index == l0)
            .ok_or_else(|| corrupt("invalid header"))?;
        layers.push(Layer {
            bitmap: Bitmap {
                width: i64::from(w),
                row_bytes,
                data: vec![0; size],
                top: 0,
            },
            height: h,
            lines,
            model: Model::new(if index == 0 { 1024 } else { 4096 }),
            template: Template {
                two_line: options & LRLTWO != 0,
                tx: 0,
                ty: 0,
            },
            lntp: 1,
            reset: true,
            low_top: 0,
        });
    }
    let stripes = sizes[0].1.div_ceil(l0);

    // AT movements announced for the next stripe: (line within the stripe, tx, ty).
    let mut moves: Vec<(u32, i64, i64)> = Vec::new();
    let mut scd = Vec::new();
    let mut pos = 0;
    'layers: for index in 0..layers.len() {
        for stripe in 0..stripes {
            // Floating marker segments between stripes.
            while data.get(pos) == Some(&ESC) {
                match data.get(pos + 1) {
                    Some(&ATMOVE) => {
                        let m = data
                            .get(pos + 2..pos + 8)
                            .ok_or_else(|| corrupt("truncated"))?;
                        moves.push((be32(m), i64::from(m[4] as i8), i64::from(m[5])));
                        pos += 8;
                    }
                    Some(&NEWLEN) => pos += 6,
                    Some(&COMMENT) => {
                        let len = be32(
                            data.get(pos + 2..pos + 6)
                                .ok_or_else(|| corrupt("truncated"))?,
                        );
                        pos = pos
                            .checked_add(6 + len as usize)
                            .ok_or_else(|| corrupt("truncated"))?;
                    }
                    Some(&ABORT) => return Err(corrupt("aborted")),
                    Some(&RESERVE) => return Err(corrupt("reserved marker")),
                    _ => break,
                }
            }

            // One stripe data entity: protected coded data up to ESC SDNORM or ESC SDRST.
            scd.clear();
            let terminator = loop {
                match data.get(pos) {
                    None => return Err(corrupt("stripe data truncated")),
                    Some(&ESC) => match data.get(pos + 1) {
                        Some(&STUFF) => {
                            scd.push(ESC);
                            pos += 2;
                        }
                        Some(&m @ (SDNORM | SDRST)) => {
                            pos += 2;
                            break m;
                        }
                        _ => return Err(corrupt("unexpected marker in stripe data")),
                    },
                    Some(&byte) => {
                        scd.push(byte);
                        pos += 1;
                    }
                }
            };

            let (below, rest) = layers.split_at_mut(index);
            let layer = &mut rest[0];
            let first = stripe.saturating_mul(layer.lines);
            if first >= layer.height {
                break 'layers;
            }
            let end = first.saturating_add(layer.lines).min(layer.height);
            if layer.reset {
                layer.model.reset();
                layer.lntp = 1;
                layer.template.tx = 0;
                layer.template.ty = 0;
                layer.bitmap.top = i64::from(first);
            }
            let mut decoder = ArithmeticDecoder::new(&scd);
            match below.last() {
                None => decode_lowest_stripe(
                    layer,
                    &mut decoder,
                    options & TPBON != 0,
                    first,
                    end,
                    &moves,
                ),
                Some(low_layer) => {
                    let low_first = stripe.saturating_mul(low_layer.lines);
                    let low_end = low_first
                        .saturating_add(low_layer.lines)
                        .min(low_layer.height);
                    if layer.reset {
                        layer.low_top = i64::from(low_first);
                    }
                    let low = LowRes {
                        bitmap: &low_layer.bitmap,
                        top: layer.low_top,
                        last: i64::from(low_end) - 1,
                    };
                    decode_differential_stripe(
                        layer,
                        &low,
                        &mut decoder,
                        options & TPDON != 0,
                        (options & DPON != 0).then_some(dp_table),
                        first,
                        end,
                        &moves,
                    );
                }
            }
            // Coded data left over once the stripe's lines are decoded means the data does
            // not fit the image; jbigkit rejects it the same way. The stripe that ends the
            // image may hold more lines than a NEWLEN marker segment leaves.
            if end < layer.height && decoder.pos < scd.len() {
                return Err(corrupt("stripe data longer than its lines"));
            }
            moves.clear();
            layer.reset = terminator == SDRST;
        }
    }
    Ok(layers
        .pop()
        .map(|layer| layer.bitmap.data)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bits(words: &str) -> Vec<u8> {
        words
            .split_whitespace()
            .flat_map(|w| {
                let v = u16::from_str_radix(w, 16).unwrap();
                (0..16).rev().map(move |i| ((v >> i) & 1) as u8)
            })
            .collect()
    }

    /// T.82 7.1: the arithmetic decoder test sequence (Table 26).
    #[test]
    fn arithmetic_decoder_test_sequence() {
        let pix =
            bits("05e0 0000 8b00 01c4 1700 0034 7fff 1a3f 951b 05d8 1d17 e770 0000 0000 0656 0e6a");
        let cx =
            bits("0fe0 0000 0f00 00f0 ff00 0000 0000 0000 0000 0000 0000 0000 0000 0000 0000 0000");
        let scd: Vec<u8> = "6989995c32eafaa0d5ff527fffffffc00000003fff2d20829100"
            .as_bytes()
            .chunks(2)
            .map(|h| u8::from_str_radix(std::str::from_utf8(h).unwrap(), 16).unwrap())
            .collect();
        let mut model = Model::new(2);
        let mut decoder = ArithmeticDecoder::new(&scd[..25]);
        let decoded: Vec<u8> = cx
            .iter()
            .map(|&c| decoder.decode_in(&mut model, usize::from(c)))
            .collect();
        assert_eq!(decoded, pix);
    }
}
