//! Best-effort system clipboard image read/write.

use rspaint_core::buffer::PixelBuffer;
use rspaint_core::color::Color;

/// Writes `pixels` to the system clipboard as an RGBA image.
pub fn write_image(pixels: &PixelBuffer) {
    let (width, height) = (pixels.width(), pixels.height());
    if width == 0 || height == 0 {
        return;
    }
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let color = pixels.get(x, y).unwrap_or(Color::TRANSPARENT);
            bytes.extend_from_slice(&[color.r, color.g, color.b, color.a]);
        }
    }
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        let _ = clipboard.set_image(arboard::ImageData {
            width: width as usize,
            height: height as usize,
            bytes: std::borrow::Cow::Owned(bytes),
        });
    }
}

/// Reads an RGBA image from the system clipboard, if any.
pub fn read_image() -> Option<PixelBuffer> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    let image = clipboard.get_image().ok()?;
    let (width, height) = (image.width, image.height);
    if width == 0 || height == 0 {
        return None;
    }
    let mut buffer = PixelBuffer::transparent(width as u32, height as u32);
    for y in 0..height {
        for x in 0..width {
            let index = (y * width + x) * 4;
            if index + 3 < image.bytes.len() {
                let b = &image.bytes;
                buffer.set(
                    x as u32,
                    y as u32,
                    Color::rgba(b[index], b[index + 1], b[index + 2], b[index + 3]),
                );
            }
        }
    }
    Some(buffer)
}
#[cfg(test)]
mod clip_tests {
    use super::*;
    use rspaint_core::color::Color;

    #[test]
    fn clipboard_round_trip() {
        let mut b = PixelBuffer::transparent(4, 3);
        b.set(0, 0, Color::rgb(255, 0, 0));
        b.set(1, 0, Color::rgb(0, 255, 0));
        b.set(2, 1, Color::rgb(0, 0, 255));
        write_image(&b);
        let read = read_image();
        assert!(read.is_some(), "expected an image back");
        let read = read.unwrap();
        assert_eq!((read.width(), read.height()), (4, 3), "size mismatch");
        assert_eq!(read.get(0, 0), Some(Color::rgb(255, 0, 0)), "pixel (0,0)");
        assert_eq!(read.get(1, 0), Some(Color::rgb(0, 255, 0)), "pixel (1,0)");
        assert_eq!(read.get(2, 1), Some(Color::rgb(0, 0, 255)), "pixel (2,1)");
    }
}
