//! NeXT 2-bit gray (Compression 32766), following libtiff's tif_next.c.
//!
//! Each row starts with an opcode: 0x00 is a literal row, 0x40 a literal span at an offset
//! (both 16-bit big-endian) within a white row, and any other byte begins a run of
//! `<gray:2><count:6>` codes that fill the row. Rows the data does not reach stay white
//! (0xff bytes), as libtiff leaves them.

use crate::{TiffError, TiffFormatError, TiffResult};

const LITERAL_ROW: u8 = 0x00;
const LITERAL_SPAN: u8 = 0x40;

fn short(row: usize) -> TiffError {
    TiffError::FormatError(TiffFormatError::CompressedDataCorrupt(format!(
        "NeXT: not enough data for row {row}"
    )))
}

/// Decodes `rows` rows of `width` 2-bit pixels, `row_bytes` bytes each.
pub(crate) fn decode(
    data: &[u8],
    width: usize,
    row_bytes: usize,
    rows: usize,
) -> TiffResult<Vec<u8>> {
    let mut out = vec![0xffu8; row_bytes * rows];
    let mut pos = 0;
    for (y, row) in out.chunks_exact_mut(row_bytes.max(1)).enumerate() {
        let Some(&opcode) = data.get(pos) else {
            break;
        };
        pos += 1;
        match opcode {
            LITERAL_ROW => {
                let literal = data.get(pos..pos + row_bytes).ok_or_else(|| short(y))?;
                row.copy_from_slice(literal);
                pos += row_bytes;
            }
            LITERAL_SPAN => {
                let header = data.get(pos..pos + 4).ok_or_else(|| short(y))?;
                let offset = usize::from(u16::from_be_bytes([header[0], header[1]]));
                let len = usize::from(u16::from_be_bytes([header[2], header[3]]));
                let span = data.get(pos + 4..pos + 4 + len).ok_or_else(|| short(y))?;
                let target = row.get_mut(offset..offset + len).ok_or_else(|| short(y))?;
                target.copy_from_slice(span);
                pos += 4 + len;
            }
            mut code => {
                // A pixel's byte is assigned when its first pixel is set, so the pixels after
                // the last one set in a byte read as 0, as in libtiff.
                let mut pixels = 0usize;
                loop {
                    let gray = code >> 6;
                    let mut count = code & 0x3f;
                    while count > 0 && pixels < width && pixels / 4 < row_bytes {
                        let byte = &mut row[pixels / 4];
                        let shift = 6 - 2 * (pixels % 4);
                        if pixels % 4 == 0 {
                            *byte = gray << shift;
                        } else {
                            *byte |= gray << shift;
                        }
                        pixels += 1;
                        count -= 1;
                    }
                    if pixels >= width {
                        break;
                    }
                    if pixels / 4 >= row_bytes {
                        return Err(TiffError::FormatError(
                            TiffFormatError::CompressedDataCorrupt(format!(
                                "NeXT: invalid data for row {y}"
                            )),
                        ));
                    }
                    code = *data.get(pos).ok_or_else(|| short(y))?;
                    pos += 1;
                }
            }
        }
    }
    Ok(out)
}
