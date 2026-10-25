//! Caller-supplied decompressors for strip and tile data.
//!
//! A [`ChunkDecompressor`] registered with [`Decoder::with_decompressor`] decodes the
//! compressed bytes of each strip or tile that uses its compression method, in place of the
//! built-in codec (or where there is none, such as LERC, LZMA, JPEG XL or JPEG 2000). Its output
//! replaces only the decompression step: predictors, byte order, bit unpacking, chroma
//! upsampling, extra-sample handling and photometric post-processing run on it exactly as they
//! do on the output of the built-in codecs.
//!
//! [`Decoder::with_decompressor`]: super::Decoder::with_decompressor

use crate::tags::{CompressionMethod, PhotometricInterpretation, SampleFormat};
use crate::TiffResult;

/// The strip or tile a [`ChunkDecompressor`] is asked to decode.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct ChunkInfo<'a> {
    /// The image's `Compression` value.
    pub compression: CompressionMethod,
    /// Width of the chunk as stored, in pixels: the tile width for tiles (including the padding
    /// of border tiles), the image width for strips.
    pub width: u32,
    /// Rows the output must contain: the tile length for tiles, the rows of this strip for strips
    /// (the last strip may be shorter than `RowsPerStrip`).
    pub height: u32,
    /// Samples per pixel in this chunk: `SamplesPerPixel`, or 1 for planar images.
    pub samples: u16,
    /// Bits per sample.
    pub bits_per_sample: u16,
    /// The image's `SampleFormat`.
    pub sample_format: SampleFormat,
    /// The image's `PhotometricInterpretation`.
    pub photometric: PhotometricInterpretation,
    /// Whether multi-byte samples in the output are little-endian. This is the file's byte order.
    pub little_endian: bool,
    /// The `JPEGTables` value, for JPEG compression with shared tables.
    pub jpeg_tables: Option<&'a [u8]>,
    /// Upper bound for the output length in bytes. Longer output is rejected with
    /// [`TiffError::LimitsExceeded`](crate::TiffError::LimitsExceeded).
    pub max_output_len: usize,
}

/// Decodes the compressed bytes of one strip or tile.
///
/// The output must hold the chunk's samples as an uncompressed TIFF stores them: `height` rows of
/// `width * samples` samples of `bits_per_sample` bits, each row starting on a byte boundary,
/// multi-byte samples in the file's byte order (see [`ChunkInfo::little_endian`]), before any
/// predictor is undone. Image codecs that decode to pixels (JPEG, JPEG XL, JPEG 2000) produce the
/// stored color samples without color conversion; YCbCr from JPEG is upsampled to full
/// resolution, as the built-in JPEG decoder does.
///
/// Output shorter than `height` rows is reported as an I/O error by the decoder; it is never
/// padded.
pub trait ChunkDecompressor: Send + Sync {
    /// Decode `compressed` into `output`, which is empty when called.
    fn decompress(
        &self,
        compressed: &[u8],
        info: &ChunkInfo<'_>,
        output: &mut Vec<u8>,
    ) -> TiffResult<()>;
}

/// Decompressors registered on a decoder, keyed by compression number.
#[derive(Clone, Default)]
pub(crate) struct Registry(Vec<(u16, std::sync::Arc<dyn ChunkDecompressor>)>);

impl Registry {
    pub(crate) fn set(
        &mut self,
        method: CompressionMethod,
        decompressor: std::sync::Arc<dyn ChunkDecompressor>,
    ) {
        let key = method.to_u16();
        self.0.retain(|(k, _)| *k != key);
        self.0.push((key, decompressor));
    }

    pub(crate) fn get(&self, method: CompressionMethod) -> Option<&dyn ChunkDecompressor> {
        let key = method.to_u16();
        self.0.iter().find(|(k, _)| *k == key).map(|(_, d)| &**d)
    }
}

impl core::fmt::Debug for Registry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(k, _)| k))
            .finish()
    }
}
