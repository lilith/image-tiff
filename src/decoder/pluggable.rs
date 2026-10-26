//! Caller-supplied decompressors for strip and tile data.
//!
//! A [`ChunkDecompressor`] registered with [`Decoder::with_decompressor`] decodes the
//! compressed bytes of each strip or tile that uses its compression method, in place of the
//! built-in codec or where there is none (LERC, LZMA, JPEG XL, JPEG 2000, old-style JPEG, JBIG,
//! PixarLog, ...). Its output replaces only the decompression step: predictors, byte order, bit
//! unpacking, chroma upsampling, extra-sample handling and photometric post-processing run on it
//! exactly as they do on the output of the built-in codecs.
//!
//! What a decompressor can use beyond the chunk's bytes:
//! - the image parameters in [`ChunkInfo`], including the chunk's index, plane and position;
//! - the values of any tags it lists in [`ChunkDecompressor::tags`] (LercParameters,
//!   T4Options/T6Options, JPEGInterchangeFormat and the old-style JPEG tables, ...);
//! - reads at arbitrary file offsets through [`FileRead`], for codecs whose data lives outside
//!   the chunk (old-style JPEG headers and tables).
//!
//! Chunks with no stored bytes are absent and decode as zero samples without calling the
//! decompressor.
//!
//! [`Decoder::with_decompressor`]: super::Decoder::with_decompressor

use crate::decoder::ifd::Value;
use crate::tags::{ByteOrder, CompressionMethod, PhotometricInterpretation, SampleFormat, Tag};
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
    /// The file's byte order.
    pub byte_order: ByteOrder,
    /// The `FillOrder` value: 1 for most significant bit first, 2 for least significant first.
    pub fill_order: u16,
    /// The `JPEGTables` value, for JPEG compression with shared tables.
    pub jpeg_tables: Option<&'a [u8]>,
    /// Values of the tags listed by [`ChunkDecompressor::tags`] that the directory holds.
    pub tags: &'a [(Tag, Value)],
    /// Index of the chunk in the `StripOffsets`/`TileOffsets` array.
    pub chunk_index: u32,
    /// Plane of the chunk: 0 for chunky images, the sample index for planar images.
    pub plane: u16,
    /// Column of the chunk's first pixel in the image.
    pub x: u32,
    /// Row of the chunk's first pixel in the image.
    pub y: u32,
    /// Upper bound for the output length in bytes. Longer output is rejected with
    /// [`TiffError::LimitsExceeded`](crate::TiffError::LimitsExceeded).
    pub max_output_len: usize,
}

impl ChunkInfo<'_> {
    /// The value of `tag`, if it was requested through [`ChunkDecompressor::tags`] and present.
    pub fn tag(&self, tag: Tag) -> Option<&Value> {
        self.tags.iter().find(|(t, _)| *t == tag).map(|(_, v)| v)
    }
}

/// Reads bytes of the TIFF file at absolute offsets.
pub trait FileRead {
    /// Read exactly `len` bytes starting at `offset`. Requests longer than the decoder's
    /// `intermediate_buffer_size` limit fail with `LimitsExceeded`.
    fn read_at(&mut self, offset: u64, len: usize) -> TiffResult<Vec<u8>>;
}

/// Decodes the compressed bytes of one strip or tile.
///
/// The output must hold the chunk's samples as an uncompressed TIFF stores them: `height` rows of
/// `width * samples` samples of `bits_per_sample` bits, each row starting on a byte boundary,
/// before any predictor is undone. Multi-byte samples are in the byte order `decompress` returns:
/// [`ChunkInfo::byte_order`] for byte-stream codecs (LZMA, ...), or [`ByteOrder::native`] for
/// codecs that decode to samples in memory (JPEG XL, JPEG 2000, LERC, ...). Image codecs produce
/// samples in the color space `PhotometricInterpretation` names: YCbCr stays YCbCr, and a JPEG
/// 2000 tile coded as YCbCr under an RGB photometric (Aperio's 33003) is converted to RGB.
/// Subsampled YCbCr is upsampled to full resolution by the JPEG-based codecs (old-style JPEG 6,
/// JPEG 7, DNG lossy JPEG 34892), as the built-in JPEG decoder does; every other codec produces
/// the stored block layout, which the decoder upsamples.
///
/// Output shorter than `height` rows is reported as an I/O error by the decoder; it is never
/// padded.
pub trait ChunkDecompressor: Send + Sync {
    /// Tags whose values this decompressor needs, read once per image and passed in
    /// [`ChunkInfo::tags`]. Tags the directory lacks are left out.
    fn tags(&self) -> &[Tag] {
        &[]
    }

    /// Decode `compressed` into `output`, which is empty when called, and return the byte order
    /// of the multi-byte samples written.
    fn decompress(
        &self,
        compressed: &[u8],
        info: &ChunkInfo<'_>,
        file: &mut dyn FileRead,
        output: &mut Vec<u8>,
    ) -> TiffResult<ByteOrder>;
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

/// [`FileRead`] over the decoder's reader, bounded by `intermediate_buffer_size`.
pub(crate) struct BoundedFile<'r, R> {
    pub(crate) reader: &'r mut R,
    pub(crate) max_len: usize,
}

impl<R: std::io::Read + std::io::Seek> FileRead for BoundedFile<'_, R> {
    fn read_at(&mut self, offset: u64, len: usize) -> TiffResult<Vec<u8>> {
        if len > self.max_len {
            return Err(crate::TiffError::LimitsExceeded);
        }
        self.reader.seek(std::io::SeekFrom::Start(offset))?;
        let mut data = vec![0; len];
        self.reader.read_exact(&mut data)?;
        Ok(data)
    }
}
