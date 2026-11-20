//! lx1: the picture on the system clipboard, for a chat paste whose web view hands over no file. WebKitGTK on Linux
//! gives the chat box nothing for a copied screen capture; WebView2 on Windows gives it as a file, so this is only the
//! fallback when a paste carried neither a file nor text.

use base64::Engine;

/// The clipboard's picture as PNG in base64, or None when it holds no picture.
pub fn image_png() -> Result<Option<String>, String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| format!("The clipboard could not be read: {e}"))?;
    let image = match clipboard.get_image() {
        Ok(i) => i,
        Err(arboard::Error::ContentNotAvailable) => return Ok(None),
        Err(e) => return Err(format!("The clipboard's picture could not be read: {e}")),
    };
    let png = encode_png(image.width as u32, image.height as u32, &image.bytes)?;
    Ok(Some(base64::engine::general_purpose::STANDARD.encode(png)))
}

/// RGBA pixels as a PNG file.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    if width == 0 || height == 0 || rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err("The clipboard's picture has no usable size.".into());
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_become_a_png_that_reads_back_the_same() {
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| (i * 10) as u8).collect();
        let png = encode_png(2, 3, &rgba).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buf).unwrap();
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(&buf[..info.buffer_size()], &rgba[..]);
        assert!(encode_png(2, 3, &rgba[..5]).is_err());
        assert!(encode_png(0, 3, &[]).is_err());
    }

    /// Run by hand on a machine with a desktop session (it replaces what the clipboard holds):
    /// `cargo test --lib clipboard -- --ignored`.
    #[test]
    #[ignore]
    fn reads_a_picture_put_on_the_real_clipboard() {
        let mut c = arboard::Clipboard::new().unwrap();
        let bytes: Vec<u8> = [255u8, 0, 0, 255].repeat(4 * 4);
        c.set_image(arboard::ImageData {
            width: 4,
            height: 4,
            bytes: bytes.into(),
        })
        .unwrap();
        let png = image_png().unwrap().expect("a picture");
        assert!(!png.is_empty());
        c.set_text("no picture").unwrap();
        assert_eq!(image_png().unwrap(), None);
    }
}
