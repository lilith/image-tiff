//! Old-style JPEG (Compression 6), following libtiff's tif_ojpeg.c.
//!
//! libtiff reads an image as one JPEG stream per plane. The header comes from the stream at
//! `JPEGInterchangeFormat` (513, length 514), which may run on into the first strip or tile,
//! or, when that stream holds no frame header, is built from the `JPEGQTables`,
//! `JPEGDCTables` and `JPEGACTables` tags (519-521) and the YCbCr subsampling. Every strip or
//! tile follows with a restart marker between them, and libtiff sets the restart interval to
//! one strip or tile when there is more than one. Each chunk therefore starts at a restart
//! boundary, and [`Header::chunk_stream`] turns it into a self-contained JPEG stream: the same
//! tables and scan header, with the frame height of the chunk.

use crate::{TiffError, TiffFormatError, TiffResult};
use std::io::{Read, Seek, SeekFrom};

const SOI: u8 = 0xD8;
const EOI: u8 = 0xD9;
const SOS: u8 = 0xDA;
const DQT: u8 = 0xDB;
const DHT: u8 = 0xC4;
const DRI: u8 = 0xDD;
const SOF0: u8 = 0xC0;

/// The old-style JPEG tags of an image. Offsets of 0 mean absent, as in libtiff.
#[derive(Debug, Default)]
pub(crate) struct Tags {
    /// `JPEGInterchangeFormat` and `JPEGInterchangeFormatLength`.
    pub interchange_format: u64,
    pub interchange_format_length: u64,
    /// `JPEGRestartInterval` (515).
    pub restart_interval: u16,
    pub q_tables: Vec<u64>,
    pub dc_tables: Vec<u64>,
    pub ac_tables: Vec<u64>,
}

/// What the image says about its chunks.
pub(crate) struct Geometry {
    pub samples: u16,
    pub ycbcr: bool,
    /// `YCbCrSubsampling`, (2, 2) when absent.
    pub subsampling: (u16, u16),
    pub image_width: u32,
    pub image_length: u32,
    /// Strip: the image width and RowsPerStrip. Tile: the tile size.
    pub chunk_width: u32,
    pub chunk_length: u32,
    pub tiled: bool,
}

/// The JPEG header libtiff assembles for an image.
#[derive(Debug)]
pub(crate) struct Header {
    /// DQT and DHT segments, emitted in libtiff's order (quantization tables 0-3, DC 0-3,
    /// AC 0-3).
    tables: Vec<u8>,
    restart_interval: u16,
    sof_marker: u8,
    width: u16,
    /// Per component: identifier, sampling factors, quantization table.
    frame: Vec<[u8; 3]>,
    /// Per component: identifier, DC and AC table selectors.
    scan: Vec<[u8; 2]>,
    /// Entropy-coded bytes after the header in the interchange-format stream; libtiff feeds
    /// them ahead of the first chunk.
    leading: Vec<u8>,
    /// Bytes at the start of the first chunk that belong to the header.
    first_chunk_header: usize,
}

fn corrupt(message: &str) -> TiffError {
    TiffError::FormatError(TiffFormatError::CompressedDataCorrupt(format!(
        "old-style JPEG: {message}"
    )))
}

/// libtiff's input stream: the interchange-format bytes, then the first chunk.
struct Input<'a> {
    data: [&'a [u8]; 2],
    pos: usize,
}

impl Input<'_> {
    fn len(&self) -> usize {
        self.data[0].len() + self.data[1].len()
    }

    fn peek(&self) -> TiffResult<u8> {
        let (a, b) = (self.data[0], self.data[1]);
        let byte = if self.pos < a.len() {
            a.get(self.pos)
        } else {
            b.get(self.pos - a.len())
        };
        byte.copied().ok_or_else(|| corrupt("header truncated"))
    }

    fn byte(&mut self) -> TiffResult<u8> {
        let byte = self.peek()?;
        self.pos += 1;
        Ok(byte)
    }

    fn word(&mut self) -> TiffResult<u16> {
        Ok(u16::from_be_bytes([self.byte()?, self.byte()?]))
    }

    fn block(&mut self, len: usize) -> TiffResult<Vec<u8>> {
        (0..len).map(|_| self.byte()).collect()
    }

    fn skip(&mut self, len: usize) -> TiffResult<()> {
        if self.pos + len > self.len() {
            return Err(corrupt("header truncated"));
        }
        self.pos += len;
        Ok(())
    }
}

impl Header {
    /// Reads the header as libtiff does. `first_chunk` holds the stored bytes of the image's
    /// first strip or tile.
    pub(crate) fn read<R: Read + Seek>(
        reader: &mut R,
        tags: &Tags,
        geometry: &Geometry,
        first_chunk: &[u8],
        limit: usize,
    ) -> TiffResult<Header> {
        let samples = match geometry.samples {
            1 | 3 => usize::from(geometry.samples),
            n => return Err(corrupt(&format!("{n} samples per pixel are not supported"))),
        };
        let file_size = reader.seek(SeekFrom::End(0))?;

        // The interchange-format stream, clamped to the file as libtiff clamps it.
        let mut jif = Vec::new();
        if tags.interchange_format != 0 && tags.interchange_format < file_size {
            let available = file_size - tags.interchange_format;
            let len = match tags.interchange_format_length {
                0 => available,
                n => n.min(available),
            };
            if len > limit as u64 {
                return Err(TiffError::LimitsExceeded);
            }
            reader.seek(SeekFrom::Start(tags.interchange_format))?;
            reader.take(len).read_to_end(&mut jif)?;
        }

        // Subsampling: the frame header's when the stream has one, otherwise the tag's.
        let (mut sub_h, mut sub_v) = if samples == 3 && geometry.ycbcr {
            (geometry.subsampling.0, geometry.subsampling.1)
        } else {
            (1, 1)
        };

        let mut q = [const { None::<Vec<u8>> }; 4];
        let mut dc = [const { None::<Vec<u8>> }; 4];
        let mut ac = [const { None::<Vec<u8>> }; 4];
        let mut stream_restart_interval = None;
        let mut frame: Option<(u8, u16, Vec<[u8; 3]>)> = None;
        let mut scan = vec![[0u8; 2]; samples];

        let mut input = Input {
            data: [&jif, first_chunk],
            pos: 0,
        };
        loop {
            if input.peek()? != 0xFF {
                break;
            }
            input.byte()?;
            let mut marker = input.byte()?;
            while marker == 0xFF {
                marker = input.byte()?;
            }
            match marker {
                SOI => {}
                0xFE | 0xE0..=0xEF => {
                    let len = input.word()?;
                    if len < 2 {
                        return Err(corrupt("corrupt marker segment"));
                    }
                    input.skip(usize::from(len - 2))?;
                }
                DRI => {
                    if input.word()? != 4 {
                        return Err(corrupt("corrupt DRI marker"));
                    }
                    stream_restart_interval = Some(input.word()?);
                }
                DQT => {
                    let len = input.word()?;
                    if len <= 2 {
                        return Err(corrupt("corrupt DQT marker"));
                    }
                    let mut rest = usize::from(len - 2);
                    while rest > 0 {
                        if rest < 65 {
                            return Err(corrupt("corrupt DQT marker"));
                        }
                        let table = input.block(65)?;
                        let id = usize::from(table[0] & 15);
                        if id > 3 {
                            return Err(corrupt("corrupt DQT marker"));
                        }
                        let mut segment = vec![0xFF, DQT, 0, 67];
                        segment.extend_from_slice(&table);
                        q[id] = Some(segment);
                        rest -= 65;
                    }
                }
                DHT => {
                    // One table per marker, as libtiff assumes.
                    let len = input.word()?;
                    if len <= 2 {
                        return Err(corrupt("corrupt DHT marker"));
                    }
                    let body = input.block(usize::from(len - 2))?;
                    let class = body[0];
                    let mut segment = vec![0xFF, DHT, (len >> 8) as u8, len as u8];
                    segment.extend_from_slice(&body);
                    match class & 0xF0 {
                        0 if class <= 3 => dc[usize::from(class)] = Some(segment),
                        16 if class & 15 <= 3 => ac[usize::from(class & 15)] = Some(segment),
                        _ => return Err(corrupt("corrupt DHT marker")),
                    }
                }
                0xC0 | 0xC1 | 0xC3 => {
                    if frame.is_some() {
                        return Err(corrupt("second frame header"));
                    }
                    let len = input.word()?;
                    if len < 11 || (len - 8) % 3 != 0 {
                        return Err(corrupt("corrupt SOF marker"));
                    }
                    let n = usize::from((len - 8) / 3);
                    if n != samples {
                        return Err(corrupt("unexpected number of components"));
                    }
                    if input.byte()? != 8 {
                        return Err(corrupt("unexpected sample precision"));
                    }
                    let height = u32::from(input.word()?);
                    let total_length = chunk_length_total(geometry);
                    if height < geometry.image_length && height < total_length {
                        return Err(corrupt("unexpected height"));
                    }
                    let width = input.word()?;
                    let chunk_width = geometry.chunk_width;
                    if u32::from(width) < geometry.image_width && u32::from(width) < chunk_width {
                        return Err(corrupt("unexpected width"));
                    }
                    if u32::from(width) > chunk_width {
                        return Err(corrupt("width exceeds the image width"));
                    }
                    if usize::from(input.byte()?) != n {
                        return Err(corrupt("corrupt SOF marker"));
                    }
                    let components: Vec<[u8; 3]> = (0..n)
                        .map(|_| Ok([input.byte()?, input.byte()?, input.byte()?]))
                        .collect::<TiffResult<_>>()?;
                    if samples == 3 && geometry.ycbcr {
                        let hv = components[0][1];
                        (sub_h, sub_v) = (u16::from(hv >> 4), u16::from(hv & 15));
                    }
                    frame = Some((marker, width, components));
                }
                SOS => {
                    if frame.is_none() {
                        return Err(corrupt("SOS before SOF"));
                    }
                    let len = input.word()?;
                    if usize::from(len) != 6 + samples * 2 || usize::from(input.byte()?) != samples
                    {
                        return Err(corrupt("corrupt SOS marker"));
                    }
                    for component in scan.iter_mut() {
                        *component = [input.byte()?, input.byte()?];
                    }
                    // Spectral selection and approximation are not checked, as in libjpeg.
                    input.skip(3)?;
                    break;
                }
                other => {
                    return Err(corrupt(&format!("unknown marker {other:#04x}")));
                }
            }
        }
        let header_end = input.pos;

        // libtiff sets the restart interval to one chunk when there is more than one; a DRI in
        // the stream overrides that, and the tag applies otherwise.
        let mut restart_interval = tags.restart_interval;
        if geometry.chunk_length < geometry.image_length {
            let (h, v) = (u32::from(sub_h), u32::from(sub_v));
            if !matches!(h, 1 | 2 | 4) || !matches!(v, 1 | 2 | 4) {
                return Err(corrupt("invalid subsampling values"));
            }
            if !geometry.chunk_length.is_multiple_of(v * 8) {
                return Err(corrupt(
                    "incompatible vertical subsampling and strip or tile length",
                ));
            }
            restart_interval =
                (geometry.chunk_width.div_ceil(h * 8) * (geometry.chunk_length / (v * 8))) as u16;
        }
        if let Some(interval) = stream_restart_interval {
            restart_interval = interval;
        }

        let (sof_marker, width, frame) = match frame {
            Some(frame) => frame,
            None => {
                // No frame header in the stream: build the header from the table tags.
                let mut frame = vec![[0u8; 3]; samples];
                for (o, component) in frame.iter_mut().enumerate() {
                    component[0] = o as u8;
                    component[1] = if o == 0 {
                        ((sub_h as u8) << 4) | (sub_v as u8 & 15)
                    } else {
                        0x11
                    };
                }
                read_q_tables(reader, tags, &mut q, &mut frame)?;
                read_huffman_tables(reader, &tags.dc_tables, 0, &mut dc, &mut scan)?;
                read_huffman_tables(reader, &tags.ac_tables, 16, &mut ac, &mut scan)?;
                for (o, component) in scan.iter_mut().enumerate().skip(1) {
                    component[0] = o as u8;
                }
                let width = u16::try_from(geometry.chunk_width)
                    .map_err(|_| corrupt("width does not fit a JPEG frame"))?;
                (SOF0, width, frame)
            }
        };
        if u32::from(width) != geometry.chunk_width {
            return Err(corrupt("frame width differs from the strip or tile width"));
        }

        let mut tables = Vec::new();
        for segment in q.iter().chain(&dc).chain(&ac).flatten() {
            tables.extend_from_slice(segment);
        }
        let (leading, first_chunk_header) = if header_end <= jif.len() {
            (jif[header_end..].to_vec(), 0)
        } else {
            (Vec::new(), header_end - jif.len())
        };
        Ok(Header {
            tables,
            restart_interval,
            sof_marker,
            width,
            frame,
            scan,
            leading,
            first_chunk_header,
        })
    }

    /// One JPEG stream for a chunk of `rows` stored rows.
    pub(crate) fn chunk_stream(&self, first: bool, chunk: &[u8], rows: u32) -> TiffResult<Vec<u8>> {
        let rows = u16::try_from(rows).map_err(|_| corrupt("height does not fit a JPEG frame"))?;
        let n = self.frame.len() as u8;
        let mut out = Vec::with_capacity(self.tables.len() + chunk.len() + 64);
        out.extend_from_slice(&[0xFF, SOI]);
        out.extend_from_slice(&self.tables);
        if self.restart_interval != 0 {
            out.extend_from_slice(&[0xFF, DRI, 0, 4]);
            out.extend_from_slice(&self.restart_interval.to_be_bytes());
        }
        out.extend_from_slice(&[0xFF, self.sof_marker, 0, 8 + 3 * n, 8]);
        out.extend_from_slice(&rows.to_be_bytes());
        out.extend_from_slice(&self.width.to_be_bytes());
        out.push(n);
        for component in &self.frame {
            out.extend_from_slice(component);
        }
        out.extend_from_slice(&[0xFF, SOS, 0, 6 + 2 * n, n]);
        for component in &self.scan {
            out.extend_from_slice(component);
        }
        out.extend_from_slice(&[0, 63, 0]);
        if first {
            out.extend_from_slice(&self.leading);
            out.extend_from_slice(chunk.get(self.first_chunk_header..).unwrap_or_default());
        } else {
            out.extend_from_slice(chunk);
        }
        out.extend_from_slice(&[0xFF, EOI]);
        Ok(out)
    }
}

fn chunk_length_total(geometry: &Geometry) -> u32 {
    if geometry.tiled {
        geometry
            .image_length
            .div_ceil(geometry.chunk_length.max(1))
            .saturating_mul(geometry.chunk_length)
    } else {
        geometry.image_length
    }
}

/// `JPEGQTables`: 64 bytes per table; components without their own offset share the previous
/// component's table.
fn read_q_tables<R: Read + Seek>(
    reader: &mut R,
    tags: &Tags,
    q: &mut [Option<Vec<u8>>; 4],
    frame: &mut [[u8; 3]],
) -> TiffResult<()> {
    let offset = |m: usize| tags.q_tables.get(m).copied().unwrap_or(0);
    if offset(0) == 0 {
        return Err(corrupt("missing JPEG tables"));
    }
    for m in 0..frame.len() {
        if offset(m) != 0 && (m == 0 || offset(m) != offset(m - 1)) {
            if (0..m.saturating_sub(1)).any(|n| offset(n) == offset(m)) {
                return Err(corrupt("corrupt JPEGQTables"));
            }
            let mut segment = vec![0xFF, DQT, 0, 67, m as u8];
            segment.resize(5 + 64, 0);
            reader.seek(SeekFrom::Start(offset(m)))?;
            reader.read_exact(&mut segment[5..])?;
            q[m] = Some(segment);
            frame[m][2] = m as u8;
        } else {
            frame[m][2] = frame[m - 1][2];
        }
    }
    Ok(())
}

/// `JPEGDCTables` (`class` 0) or `JPEGACTables` (`class` 16): 16 code counts, then the values.
fn read_huffman_tables<R: Read + Seek>(
    reader: &mut R,
    offsets: &[u64],
    class: u8,
    tables: &mut [Option<Vec<u8>>; 4],
    scan: &mut [[u8; 2]],
) -> TiffResult<()> {
    let offset = |m: usize| offsets.get(m).copied().unwrap_or(0);
    if offset(0) == 0 {
        return Err(corrupt("missing JPEG tables"));
    }
    for m in 0..scan.len() {
        if offset(m) != 0 && (m == 0 || offset(m) != offset(m - 1)) {
            if (0..m.saturating_sub(1)).any(|n| offset(n) == offset(m)) {
                return Err(corrupt("corrupt JPEG Huffman table tags"));
            }
            let mut counts = [0u8; 16];
            reader.seek(SeekFrom::Start(offset(m)))?;
            reader.read_exact(&mut counts)?;
            let values: usize = counts.iter().map(|&c| usize::from(c)).sum();
            let len = 19 + values;
            let mut segment = vec![0xFF, DHT, (len >> 8) as u8, len as u8, class | m as u8];
            segment.extend_from_slice(&counts);
            let start = segment.len();
            segment.resize(start + values, 0);
            reader.read_exact(&mut segment[start..])?;
            tables[m] = Some(segment);
            scan[m][1] = if class == 0 {
                (m as u8) << 4
            } else {
                scan[m][1] | m as u8
            };
        } else if class == 0 {
            scan[m][1] = scan[m - 1][1];
        } else {
            scan[m][1] |= scan[m - 1][1] & 15;
        }
    }
    Ok(())
}
