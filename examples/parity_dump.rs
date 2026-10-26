//! Parity harness side of image-tiff: decode every image of a TIFF and write what the decoder
//! returns, for `parity/compare.py` to check against libtiff (`parity/libtiff_dump.c`).
//!
//! Usage: parity_dump <file.tif> <out-prefix>
//! Writes `<out-prefix>.<n>.txt` (key=value lines) and, when decoding succeeds,
//! `<out-prefix>.<n>.raw` (`read_image_bytes` output: native-endian samples, planes one after
//! another) for image n = 0, 1, ...

use std::fs::File;
use std::io::{BufReader, Write};
use tiff::decoder::Decoder;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(input), Some(prefix)) = (args.next(), args.next()) else {
        eprintln!("usage: parity_dump <file.tif> <out-prefix>");
        std::process::exit(2);
    };
    let reader = BufReader::new(File::open(&input).expect("open input"));
    let mut decoder = match Decoder::open(reader) {
        Ok(d) => d,
        Err(e) => {
            let mut f = File::create(format!("{prefix}.open.txt")).unwrap();
            writeln!(f, "open_failed=1\nmessages={e}").unwrap();
            return;
        }
    };
    let mut n = 0;
    while n < 64 && decoder.more_images() {
        let mut meta = File::create(format!("{prefix}.{n}.txt")).unwrap();
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decode_one(&mut decoder, &format!("{prefix}.{n}"), &mut meta)
        }));
        match outcome {
            Ok(Ok(())) => writeln!(meta, "decoded=1").unwrap(),
            Ok(Err(e)) => writeln!(meta, "decoded=0\nmessages={e}").unwrap(),
            Err(_) => {
                writeln!(meta, "decoded=0\npanicked=1").unwrap();
                break;
            }
        }
        n += 1;
    }
}

fn decode_one<R: std::io::Read + std::io::Seek>(
    decoder: &mut Decoder<R>,
    prefix: &str,
    meta: &mut File,
) -> tiff::TiffResult<()> {
    decoder.next_image()?;
    let (w, h) = decoder.dimensions()?;
    writeln!(meta, "width={w}\nheight={h}")?;
    let color = decoder.colortype();
    writeln!(meta, "colortype={color:?}")?;
    let layout = decoder.image_buffer_layout()?;
    writeln!(
        meta,
        "planes={}\nplane_stride={}\nrow_stride={}\ncomplete_len={}\nsample_format={:?}",
        layout.planes,
        layout.plane_stride.map_or(0, |s| s.get()),
        layout.row_stride.map_or(0, |s| s.get()),
        layout.complete_len,
        layout.sample_format,
    )?;
    let mut buffer = vec![0u8; layout.complete_len];
    decoder.read_image_bytes(&mut buffer)?;
    File::create(format!("{prefix}.raw"))?.write_all(&buffer)?;
    Ok(())
}
