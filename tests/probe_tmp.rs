mod synth;
use std::io::Cursor;
use tiff::decoder::Decoder;
fn try_decode(file: Vec<u8>) -> Result<Vec<u8>, String> {
    let mut d = Decoder::open(Cursor::new(file)).map_err(|e| format!("open {e}"))?;
    d.next_image().map_err(|e| format!("next_image {e}"))?;
    let layout = d.image_buffer_layout().map_err(|e| format!("layout {e}"))?;
    let mut b = vec![0u8; layout.complete_len];
    d.read_image_bytes(&mut b)
        .map_err(|e| format!("read {e}"))?;
    Ok(b)
}
#[test]
fn probe() {
    let px: Vec<u8> = (0..40u32 * 30).map(|i| (i * 7) as u8).collect();
    for eoi in [true, false] {
        let f = synth::image(40, 30, 8, 1, 5)
            .short(262, &[1])
            .strips(vec![synth::lzw_literal(&px, eoi)])
            .build();
        eprintln!("lzw eoi={eoi}: {:?}", try_decode(f).map(|b| b == px));
        let half = px.len() / 2;
        let f = synth::image(40, 30, 8, 1, 5)
            .short(262, &[1])
            .long(278, &[15])
            .strips(vec![
                synth::lzw_literal(&px[..half], eoi),
                synth::lzw_literal(&px[half..], eoi),
            ])
            .build();
        eprintln!(
            "lzw 2 strips eoi={eoi}: {:?}",
            try_decode(f).map(|b| b == px)
        );
    }
    // truncated data (short strip) must be an error, not silent zeros
    let mut short = synth::lzw_literal(&px, false);
    short.truncate(short.len() / 2);
    let f = synth::image(40, 30, 8, 1, 5)
        .short(262, &[1])
        .strips(vec![short])
        .build();
    eprintln!("lzw truncated: {:?}", try_decode(f).map(|b| b == px));
}
