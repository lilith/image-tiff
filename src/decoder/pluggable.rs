//! Pluggable tile/strip decompressor trait for TIFF embedded images.
//!
//! Allows callers to provide custom decoders for JPEG, WebP, and other
//! image compression methods used in TIFF tiles/strips.

use crate::tags::CompressionMethod;
use crate::TiffResult;

/// Color space of decompressed image data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecompressedColorSpace {
    Gray,
    Rgb,
    Rgba,
    Cmyk,
    YCbCr,
}

/// Output from a tile/strip image decompressor.
pub struct TileDecompressOutput {
    /// Decoded pixel bytes.
    pub pixels: Vec<u8>,
    /// Actual width of decoded tile.
    pub width: u32,
    /// Actual height of decoded tile.
    pub height: u32,
    /// Color space of decoded pixels.
    pub color_space: DecompressedColorSpace,
    /// Samples per pixel in the output.
    pub samples_per_pixel: u16,
}

/// Context from the TIFF container for the decompressor.
pub struct TiffDecompressContext<'a> {
    /// Expected tile/strip width.
    pub width: u32,
    /// Expected tile/strip height.
    pub height: u32,
    /// Samples per pixel from TIFF tags.
    pub samples_per_pixel: u16,
    /// JPEG quantization/Huffman tables from TIFF tag 347.
    /// Must be prepended to JPEG tile data (minus the SOI marker from tables,
    /// minus the EOI marker from tables).
    pub jpeg_tables: Option<&'a [u8]>,
    /// Optional cancellation check. Returns true to cancel.
    pub check_cancelled: Option<&'a dyn Fn() -> bool>,
}

/// Trait for pluggable tile/strip image decompressors.
///
/// Register with [`super::Decoder::set_decompressor`] to override the built-in
/// JPEG or WebP decompressor for a specific compression method.
///
/// # Example
///
/// ```ignore
/// use tiff::decoder::pluggable::{TileDecompressor, TiffDecompressContext, TileDecompressOutput};
/// use tiff::TiffResult;
///
/// struct MyJpegDecompressor;
///
/// impl TileDecompressor for MyJpegDecompressor {
///     fn decompress(
///         &self,
///         data: &[u8],
///         ctx: &TiffDecompressContext<'_>,
///     ) -> TiffResult<TileDecompressOutput> {
///         // ... decode JPEG data ...
///         # todo!()
///     }
/// }
/// ```
pub trait TileDecompressor: Send + Sync {
    /// Decompress a TIFF tile or strip.
    fn decompress(
        &self,
        data: &[u8],
        ctx: &TiffDecompressContext<'_>,
    ) -> TiffResult<TileDecompressOutput>;
}

/// Registry of pluggable decompressors keyed by compression method.
///
/// Uses a `Vec` rather than `HashMap` for no_std compatibility and because
/// the number of registered decompressors is typically very small (1-3).
pub(crate) struct DecompressorRegistry {
    entries: Vec<(CompressionMethod, Box<dyn TileDecompressor>)>,
}

impl DecompressorRegistry {
    pub(crate) fn new() -> Self {
        DecompressorRegistry {
            entries: Vec::new(),
        }
    }

    pub(crate) fn set(
        &mut self,
        method: CompressionMethod,
        decompressor: Box<dyn TileDecompressor>,
    ) {
        // Remove existing entry for this method if any.
        self.entries.retain(|(m, _)| *m != method);
        self.entries.push((method, decompressor));
    }

    pub(crate) fn get(&self, method: &CompressionMethod) -> Option<&dyn TileDecompressor> {
        self.entries
            .iter()
            .find(|(m, _)| m == method)
            .map(|(_, d)| d.as_ref())
    }
}

impl std::fmt::Debug for DecompressorRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let methods: Vec<_> = self.entries.iter().map(|(m, _)| m).collect();
        f.debug_struct("DecompressorRegistry")
            .field("count", &self.entries.len())
            .field("methods", &methods)
            .finish()
    }
}
