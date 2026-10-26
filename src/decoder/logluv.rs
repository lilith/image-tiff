use std::io::{Read, Take};

use crate::TiffError;

pub struct LogLuv24<R> {
    reader: Take<R>,
    /// The required buffer size. We only decompressed all samples at once.
    chunk_sz: usize,
}

impl<R> LogLuv24<R> {
    pub fn new(reader: R, n: u64, npixels: u32) -> Result<Self, TiffError>
    where
        R: Read,
    {
        let chunk_sz = usize::try_from(
            u64::from(npixels)
                .checked_mul(4 * 3)
                .ok_or(TiffError::LimitsExceeded)?,
        )?;

        let reader = reader.take(n);
        Ok(LogLuv24 { reader, chunk_sz })
    }

    fn expand_buf(buf: &mut [u8]) {
        let data_len = buf.len() / 4;

        for i in (0..data_len / 3).rev() {
            let ch = buf.as_chunks_mut::<3>().0[i];
            let [a, b, c] = ch.map(u32::from);

            let raw = a << 16 | b << 8 | c;
            let [bx, by, bz] = luv24_to_xyz(raw);

            let [r, g, b] = xyz_to_rgb([bx, by, bz]);

            let into = &mut buf.as_chunks_mut::<12>().0[i];
            into[0..4].copy_from_slice(&f32::to_ne_bytes(r));
            into[4..8].copy_from_slice(&f32::to_ne_bytes(g));
            into[8..12].copy_from_slice(&f32::to_ne_bytes(b));
        }
    }
}

impl<R: Read> Read for LogLuv24<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.read_exact(buf)?;
        Ok(buf.len())
    }

    // FIXME: this is the interface we have for compressed chunks but it is not optimal for what we
    // want. Note `fill_buf` is not super helpful since it does not guarantee any size of the
    // filled buffer but we need 24-bits at a time. Two code paths is even more awkward.
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        if !buf.len().is_multiple_of(self.chunk_sz) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "buffer size {0} must match the chunk size for compression method SGILog24 {1}",
                    buf.len(),
                    self.chunk_sz,
                ),
            ));
        }

        let data_len = buf.len() / 4;
        self.reader.read_exact(&mut buf[..data_len])?;
        Self::expand_buf(buf);

        Ok(())
    }
}

fn packbits_decode<R: Read, const N: usize>(
    reader: &mut R,
    buf: &mut [[u8; N]],
) -> std::io::Result<()> {
    let expected = buf.len();
    // The planes run from the most significant byte down (libtiff's `shft` loops in
    // LogL16Decode/LogLuvDecode32), matching the big-endian reads in `expand_row`.
    for offset in 0..N {
        let mut i = 0;
        let mut code = [0u8; 1];

        while let Some(pixels) = buf.get_mut(i..) {
            if pixels.is_empty() {
                break;
            }

            let pixels = pixels.iter_mut();
            reader.read_exact(&mut code)?;
            let [repeat] = code;

            if repeat >= 128 {
                reader.read_exact(&mut code)?;
                let [value] = code;
                let rc = (repeat - 128) + 2;

                for (_, px) in (0..rc).zip(pixels) {
                    px[offset] = value;
                }

                i += usize::from(rc);
            } else {
                for (_, px) in (0..repeat).zip(pixels) {
                    reader.read_exact(&mut code)?;
                    let [value] = code;
                    px[offset] = value;
                }

                i += usize::from(repeat);
            }
        }

        if i < expected {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "PackBits RLE decoded fewer pixels than expected",
            ));
        }
    }

    Ok(())
}

pub struct LogLuv32<R> {
    reader: Take<R>,
    /// The required buffer size. We only decompressed all samples at once.
    chunk_sz: usize,
}

impl<R: Read> LogLuv32<R> {
    pub fn new(reader: R, n: u64, npixels: u32) -> Result<Self, TiffError> {
        let chunk_sz = usize::try_from(
            u64::from(npixels)
                .checked_mul(4 * 3)
                .ok_or(TiffError::LimitsExceeded)?,
        )?;

        let reader = reader.take(n);
        Ok(LogLuv32 { reader, chunk_sz })
    }

    fn decode_rle_row(&mut self, row: &mut [u8]) -> std::io::Result<()>
    where
        R: Read,
    {
        // 32-bit per pixel format.
        let n_pixels = self.chunk_sz / 12;
        // buffer for reading (using only 32-bit per pixel)
        let raw = &mut row.as_chunks_mut::<4>().0[..n_pixels];
        packbits_decode::<_, 4>(&mut self.reader, raw)?;

        Ok(())
    }

    fn expand_row(row: &mut [u8]) {
        let n_pixels = row.len() / 12;

        // Work backwards, the 32-bit compressed raw data is at the front of the row buffer and
        // this avoids overwriting data before we read it.
        for i in (0..n_pixels).rev() {
            let raw = row.as_chunks_mut::<4>().0[i];

            let [bx, by, bz] = luv32_to_xyz(u32::from_be_bytes(raw));

            let [r, g, b] = xyz_to_rgb([bx, by, bz]);
            let into = &mut row.as_chunks_mut::<12>().0[i];
            into[0..4].copy_from_slice(&f32::to_ne_bytes(r));
            into[4..8].copy_from_slice(&f32::to_ne_bytes(g));
            into[8..12].copy_from_slice(&f32::to_ne_bytes(b));
        }
    }
}

impl<R: Read> Read for LogLuv32<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.read_exact(buf)?;
        Ok(buf.len())
    }

    // FIXME: See `LogLuv24::read_exact`
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        for row in buf.chunks_exact_mut(self.chunk_sz) {
            self.decode_rle_row(row)?;
            Self::expand_row(row);
        }

        Ok(())
    }
}

/// Pack-Bits like encoding for a single 16-bit log-L sample.
pub struct LogLuv16<R> {
    reader: Take<R>,
    /// The required buffer size. We only decompressed all samples at once.
    chunk_sz: usize,
}

impl<R: Read> LogLuv16<R> {
    pub fn new(reader: R, n: u64, npixels: u32) -> Result<Self, TiffError> {
        let chunk_sz = usize::try_from(
            u64::from(npixels)
                .checked_mul(4)
                .ok_or(TiffError::LimitsExceeded)?,
        )?;

        let reader = reader.take(n);
        Ok(LogLuv16 { reader, chunk_sz })
    }

    fn decode_rle_row(&mut self, row: &mut [u8]) -> std::io::Result<()>
    where
        R: Read,
    {
        // 32-bit per pixel format.
        let n_pixels = self.chunk_sz / 4;
        // buffer for reading (16-bit per pixel)
        let raw = &mut row.as_chunks_mut::<2>().0[..n_pixels];
        packbits_decode::<_, 2>(&mut self.reader, raw)?;

        Ok(())
    }

    fn expand_row(row: &mut [u8]) {
        let n_pixels = row.len() / 4;

        // Work backwards, the 32-bit compressed raw data is at the front of the row buffer and
        // this avoids overwriting data before we read it.
        for i in (0..n_pixels).rev() {
            let raw = row.as_chunks_mut::<2>().0[i];

            let l = u16::from(raw[0]) << 8 | u16::from(raw[1]);
            let by = log16_to_y(l) as f32;

            let into = &mut row.as_chunks_mut::<4>().0[i];
            into[0..4].copy_from_slice(&f32::to_ne_bytes(by));
        }
    }
}

impl<R: Read> Read for LogLuv16<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.read_exact(buf)?;
        Ok(buf.len())
    }

    // FIXME: See `LogLuv24::read_exact`
    fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()> {
        for row in buf.chunks_exact_mut(self.chunk_sz) {
            self.decode_rle_row(row)?;
            Self::expand_row(row);
        }

        Ok(())
    }
}

// Table information from: `libtiff`, 1997 by Greg Ward Larson, SGI
#[doc(alias = "UV_SQSIZ")]
const UV_QUANTIZATION_WIDTH: f32 = 0.003500;

// The `v` value corresonding to the bottom most row (quantized v' = 0).
const UV_VSTART: f32 = 0.016940;

/// Number of distinct 'u' rows, each at a different v level. Each is quantized individually with
/// the first quantization level starting at its `ustart` and `UV_QUANTIZATION_WIDTH` wide.
const UV_NVS: usize = 163;

/// Contrary to the original C code we calculate the dependent values (cumulative sum).
struct UvRow {
    ustart: f32,
    num_us: u16,
}

#[rustfmt::skip]
const UV_ROWS: [UvRow; UV_NVS] = [
    UvRow { ustart: 0.247663, num_us: 4, },
    UvRow { ustart: 0.243779, num_us: 6, },
    UvRow { ustart: 0.241684, num_us: 7, },
    UvRow { ustart: 0.237874, num_us: 9, },
    UvRow { ustart: 0.235906, num_us: 10, },
    UvRow { ustart: 0.232153, num_us: 12, },
    UvRow { ustart: 0.228352, num_us: 14, },
    UvRow { ustart: 0.226259, num_us: 15, },
    UvRow { ustart: 0.222371, num_us: 17, },
    UvRow { ustart: 0.220410, num_us: 18, },
    UvRow { ustart: 0.214710, num_us: 21, },
    UvRow { ustart: 0.212714, num_us: 22, },
    UvRow { ustart: 0.210721, num_us: 23, },
    UvRow { ustart: 0.204976, num_us: 26, },
    UvRow { ustart: 0.202986, num_us: 27, },
    UvRow { ustart: 0.199245, num_us: 29, },
    UvRow { ustart: 0.195525, num_us: 31, },
    UvRow { ustart: 0.193560, num_us: 32, },
    UvRow { ustart: 0.189878, num_us: 34, },
    UvRow { ustart: 0.186216, num_us: 36, },
    UvRow { ustart: 0.186216, num_us: 36, },
    UvRow { ustart: 0.182592, num_us: 38, },
    UvRow { ustart: 0.179003, num_us: 40, },
    UvRow { ustart: 0.175466, num_us: 42, },
    UvRow { ustart: 0.172001, num_us: 44, },
    UvRow { ustart: 0.172001, num_us: 44, },
    UvRow { ustart: 0.168612, num_us: 46, },
    UvRow { ustart: 0.168612, num_us: 46, },
    UvRow { ustart: 0.163575, num_us: 49, },
    UvRow { ustart: 0.158642, num_us: 52, },
    UvRow { ustart: 0.158642, num_us: 52, },
    UvRow { ustart: 0.158642, num_us: 52, },
    UvRow { ustart: 0.153815, num_us: 55, },
    UvRow { ustart: 0.153815, num_us: 55, },
    UvRow { ustart: 0.149097, num_us: 58, },
    UvRow { ustart: 0.149097, num_us: 58, },
    UvRow { ustart: 0.142746, num_us: 62, },
    UvRow { ustart: 0.142746, num_us: 62, },
    UvRow { ustart: 0.142746, num_us: 62, },
    UvRow { ustart: 0.138270, num_us: 65, },
    UvRow { ustart: 0.138270, num_us: 65, },
    UvRow { ustart: 0.138270, num_us: 65, },
    UvRow { ustart: 0.132166, num_us: 69, },
    UvRow { ustart: 0.132166, num_us: 69, },
    UvRow { ustart: 0.126204, num_us: 73, },
    UvRow { ustart: 0.126204, num_us: 73, },
    UvRow { ustart: 0.126204, num_us: 73, },
    UvRow { ustart: 0.120381, num_us: 77, },
    UvRow { ustart: 0.120381, num_us: 77, },
    UvRow { ustart: 0.120381, num_us: 77, },
    UvRow { ustart: 0.120381, num_us: 77, },
    UvRow { ustart: 0.112962, num_us: 82, },
    UvRow { ustart: 0.112962, num_us: 82, },
    UvRow { ustart: 0.112962, num_us: 82, },
    UvRow { ustart: 0.107450, num_us: 86, },
    UvRow { ustart: 0.107450, num_us: 86, },
    UvRow { ustart: 0.107450, num_us: 86, },
    UvRow { ustart: 0.107450, num_us: 86, },
    UvRow { ustart: 0.100343, num_us: 91, },
    UvRow { ustart: 0.100343, num_us: 91, },
    UvRow { ustart: 0.100343, num_us: 91, },
    UvRow { ustart: 0.095126, num_us: 95, },
    UvRow { ustart: 0.095126, num_us: 95, },
    UvRow { ustart: 0.095126, num_us: 95, },
    UvRow { ustart: 0.095126, num_us: 95, },
    UvRow { ustart: 0.088276, num_us: 100, },
    UvRow { ustart: 0.088276, num_us: 100, },
    UvRow { ustart: 0.088276, num_us: 100, },
    UvRow { ustart: 0.088276, num_us: 100, },
    UvRow { ustart: 0.081523, num_us: 105, },
    UvRow { ustart: 0.081523, num_us: 105, },
    UvRow { ustart: 0.081523, num_us: 105, },
    UvRow { ustart: 0.081523, num_us: 105, },
    UvRow { ustart: 0.074861, num_us: 110, },
    UvRow { ustart: 0.074861, num_us: 110, },
    UvRow { ustart: 0.074861, num_us: 110, },
    UvRow { ustart: 0.074861, num_us: 110, },
    UvRow { ustart: 0.068290, num_us: 115, },
    UvRow { ustart: 0.068290, num_us: 115, },
    UvRow { ustart: 0.068290, num_us: 115, },
    UvRow { ustart: 0.068290, num_us: 115, },
    UvRow { ustart: 0.063573, num_us: 119, },
    UvRow { ustart: 0.063573, num_us: 119, },
    UvRow { ustart: 0.063573, num_us: 119, },
    UvRow { ustart: 0.063573, num_us: 119, },
    UvRow { ustart: 0.057219, num_us: 124, },
    UvRow { ustart: 0.057219, num_us: 124, },
    UvRow { ustart: 0.057219, num_us: 124, },
    UvRow { ustart: 0.057219, num_us: 124, },
    UvRow { ustart: 0.050985, num_us: 129, },
    UvRow { ustart: 0.050985, num_us: 129, },
    UvRow { ustart: 0.050985, num_us: 129, },
    UvRow { ustart: 0.050985, num_us: 129, },
    UvRow { ustart: 0.050985, num_us: 129, },
    UvRow { ustart: 0.044859, num_us: 134, },
    UvRow { ustart: 0.044859, num_us: 134, },
    UvRow { ustart: 0.044859, num_us: 134, },
    UvRow { ustart: 0.044859, num_us: 134, },
    UvRow { ustart: 0.040571, num_us: 138, },
    UvRow { ustart: 0.040571, num_us: 138, },
    UvRow { ustart: 0.040571, num_us: 138, },
    UvRow { ustart: 0.040571, num_us: 138, },
    UvRow { ustart: 0.036339, num_us: 142, },
    UvRow { ustart: 0.036339, num_us: 142, },
    UvRow { ustart: 0.036339, num_us: 142, },
    UvRow { ustart: 0.036339, num_us: 142, },
    UvRow { ustart: 0.032139, num_us: 146, },
    UvRow { ustart: 0.032139, num_us: 146, },
    UvRow { ustart: 0.032139, num_us: 146, },
    UvRow { ustart: 0.032139, num_us: 146, },
    UvRow { ustart: 0.027947, num_us: 150, },
    UvRow { ustart: 0.027947, num_us: 150, },
    UvRow { ustart: 0.027947, num_us: 150, },
    UvRow { ustart: 0.023739, num_us: 154, },
    UvRow { ustart: 0.023739, num_us: 154, },
    UvRow { ustart: 0.023739, num_us: 154, },
    UvRow { ustart: 0.023739, num_us: 154, },
    UvRow { ustart: 0.019504, num_us: 158, },
    UvRow { ustart: 0.019504, num_us: 158, },
    UvRow { ustart: 0.019504, num_us: 158, },
    UvRow { ustart: 0.016976, num_us: 161, },
    UvRow { ustart: 0.016976, num_us: 161, },
    UvRow { ustart: 0.016976, num_us: 161, },
    UvRow { ustart: 0.016976, num_us: 161, },
    UvRow { ustart: 0.012639, num_us: 165, },
    UvRow { ustart: 0.012639, num_us: 165, },
    UvRow { ustart: 0.012639, num_us: 165, },
    UvRow { ustart: 0.009991, num_us: 168, },
    UvRow { ustart: 0.009991, num_us: 168, },
    UvRow { ustart: 0.009991, num_us: 168, },
    UvRow { ustart: 0.009016, num_us: 170, },
    UvRow { ustart: 0.009016, num_us: 170, },
    UvRow { ustart: 0.009016, num_us: 170, },
    UvRow { ustart: 0.006217, num_us: 173, },
    UvRow { ustart: 0.006217, num_us: 173, },
    UvRow { ustart: 0.005097, num_us: 175, },
    UvRow { ustart: 0.005097, num_us: 175, },
    UvRow { ustart: 0.005097, num_us: 175, },
    UvRow { ustart: 0.003909, num_us: 177, },
    UvRow { ustart: 0.003909, num_us: 177, },
    UvRow { ustart: 0.002340, num_us: 177, },
    UvRow { ustart: 0.002389, num_us: 170, },
    UvRow { ustart: 0.001068, num_us: 164, },
    UvRow { ustart: 0.001653, num_us: 157, },
    UvRow { ustart: 0.000717, num_us: 150, },
    UvRow { ustart: 0.001614, num_us: 143, },
    UvRow { ustart: 0.000270, num_us: 136, },
    UvRow { ustart: 0.000484, num_us: 129, },
    UvRow { ustart: 0.001103, num_us: 123, },
    UvRow { ustart: 0.001242, num_us: 115, },
    UvRow { ustart: 0.001188, num_us: 109, },
    UvRow { ustart: 0.001011, num_us: 103, },
    UvRow { ustart: 0.000709, num_us: 97, },
    UvRow { ustart: 0.000301, num_us: 89, },
    UvRow { ustart: 0.002416, num_us: 82, },
    UvRow { ustart: 0.003251, num_us: 76, },
    UvRow { ustart: 0.003246, num_us: 69, },
    UvRow { ustart: 0.004141, num_us: 62, },
    UvRow { ustart: 0.005963, num_us: 55, },
    UvRow { ustart: 0.008839, num_us: 47, },
    UvRow { ustart: 0.010490, num_us: 40, },
    UvRow { ustart: 0.016994, num_us: 31, },
    UvRow { ustart: 0.023659, num_us: 21, },
];

const UV_CUMULATIVE: [u16; UV_NVS] = {
    let mut arr = [0u16; UV_NVS];
    let mut sum = 0;
    let mut i = 0;
    while i < UV_NVS {
        arr[i] = sum;
        sum += UV_ROWS[i].num_us;
        i += 1;
    }
    arr
};

const UV_NDIVS: u16 = UV_CUMULATIVE[UV_NVS - 1] + UV_ROWS[UV_NVS - 1].num_us;

fn decode_quantized_uv(uv: u32) -> (u8, u8) {
    if uv >= UV_NDIVS as u32 {
        return (0, 0);
    }

    // Find the start of the row, or the index after that.
    let v_index = match UV_CUMULATIVE.binary_search(&(uv as u16)) {
        Ok(n) => n,
        Err(n) => n - 1,
    };

    let u_index = uv as u16 - UV_CUMULATIVE[v_index];
    (u_index as u8, v_index as u8)
}

// The conversions below follow libtiff's tif_luv.c (LogL16toY, LogL10toY, uv_decode,
// LogLuv24toXYZ, LogLuv32toXYZ) operation for operation: double precision, results cast to f32.

/// Neutral chroma used for an out-of-range 24-bit color index.
const U_NEU: f64 = 0.210526316;
const V_NEU: f64 = 0.473684211;
/// 32-bit LogLuv quantizes u' and v' in steps of 1/410.
const UVSCALE: f64 = 410.;

/// Luminance from 16-bit LogL (sign bit plus 15-bit log).
fn log16_to_y(l: u16) -> f64 {
    let le = f64::from(l & 0x7fff);
    if le == 0.0 {
        return 0.0;
    }
    let y = (std::f64::consts::LN_2 / 256. * (le + 0.5) - std::f64::consts::LN_2 * 64.).exp();
    if l & 0x8000 != 0 {
        -y
    } else {
        y
    }
}

/// Luminance from the 10-bit log of 24-bit LogLuv.
fn log10_to_y(l: u32) -> f64 {
    if l == 0 {
        return 0.0;
    }
    (std::f64::consts::LN_2 / 64. * (f64::from(l) + 0.5) - std::f64::consts::LN_2 * 12.).exp()
}

/// (u', v') at the center of a 24-bit LogLuv color cell, `None` for an index past the table.
fn uv_decode(c: u32) -> Option<(f64, f64)> {
    if c >= u32::from(UV_NDIVS) {
        return None;
    }
    let (ui, vi) = decode_quantized_uv(c);
    let row = &UV_ROWS[usize::from(vi)];
    let width = f64::from(UV_QUANTIZATION_WIDTH);
    let u = f64::from(row.ustart) + (f64::from(ui) + 0.5) * width;
    let v = f64::from(UV_VSTART) + (f64::from(vi) + 0.5) * width;
    Some((u, v))
}

/// XYZ from (u', v') and luminance; zero for non-positive luminance.
fn uv_to_xyz(l: f64, u: f64, v: f64) -> [f32; 3] {
    if l <= 0. {
        return [0.0; 3];
    }
    let s = 1. / (6. * u - 16. * v + 12.);
    let x = 9. * u * s;
    let y = 4. * v * s;
    [(x / y * l) as f32, l as f32, ((1. - x - y) / y * l) as f32]
}

/// XYZ from a 24-bit LogLuv pixel: 10-bit log luminance, 14-bit color index.
fn luv24_to_xyz(p: u32) -> [f32; 3] {
    let l = log10_to_y(p >> 14 & 0x3ff);
    let (u, v) = uv_decode(p & 0x3fff).unwrap_or((U_NEU, V_NEU));
    uv_to_xyz(l, u, v)
}

/// XYZ from a 32-bit LogLuv pixel: 16-bit LogL, then 8-bit u' and v'.
fn luv32_to_xyz(p: u32) -> [f32; 3] {
    let l = log16_to_y((p >> 16) as u16);
    let u = 1. / UVSCALE * (f64::from(p >> 8 & 0xff) + 0.5);
    let v = 1. / UVSCALE * (f64::from(p & 0xff) + 0.5);
    uv_to_xyz(l, u, v)
}

fn xyz_to_rgb([l, u, v]: [f32; 3]) -> [f32; 3] {
    // XYZ to sRGB primaries at D65
    let r = 3.2404542 * l + -1.5371385 * u + -0.4985314 * v;
    let g = -0.969266 * l + 1.8760108 * u + 0.0415560 * v;
    let b = 0.0556434 * l + -0.2040259 * u + 1.0572252 * v;
    [r, g, b]
}

#[test]
fn test_decode_quantized_uv() {
    // Test some known values from the original C code.
    assert_eq!(decode_quantized_uv(0), (0, 0));
    assert_eq!(decode_quantized_uv(3), (3, 0));
    assert_eq!(decode_quantized_uv(4), (0, 1));
    assert_eq!(decode_quantized_uv(9), (5, 1));
    assert_eq!(decode_quantized_uv(16288), (20, 162));
    assert_eq!(decode_quantized_uv(16289), (0, 0)); // Out of range
}

/// Serves reads of any length from a reader that decodes whole rows of `row.len()` bytes:
/// border tiles are read up to the image edge and their padding skipped separately.
pub struct WholeRows<R> {
    inner: R,
    row: Vec<u8>,
    pos: usize,
}

impl<R: Read> WholeRows<R> {
    pub fn new(inner: R, row_len: usize) -> Self {
        WholeRows {
            inner,
            row: vec![0; row_len],
            pos: row_len,
        }
    }
}

impl<R: Read> Read for WholeRows<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() || self.row.is_empty() {
            return Ok(0);
        }
        if self.pos == self.row.len() {
            self.inner.read_exact(&mut self.row)?;
            self.pos = 0;
        }
        let n = buf.len().min(self.row.len() - self.pos);
        buf[..n].copy_from_slice(&self.row[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}
