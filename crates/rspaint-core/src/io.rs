//! Image file decoding and encoding.

use std::io::Cursor;

use image::{ImageFormat, RgbaImage};

use crate::buffer::PixelBuffer;

/// Errors that can occur while reading or writing image files.
#[derive(Debug)]
pub enum IoError {
    Decode(String),
    Encode(String),
}

impl std::fmt::Display for IoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IoError::Decode(message) => write!(f, "failed to decode image: {message}"),
            IoError::Encode(message) => write!(f, "failed to encode image: {message}"),
        }
    }
}

impl std::error::Error for IoError {}

/// The image file formats supported for saving.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaveFormat {
    Png,
    Bmp,
}

impl SaveFormat {
    /// Guesses the format from a file extension, defaulting to PNG.
    pub fn from_extension(extension: &str) -> Self {
        match extension.to_ascii_lowercase().as_str() {
            "bmp" | "dib" => Self::Bmp,
            _ => Self::Png,
        }
    }

    /// The canonical file extension for this format.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Bmp => "bmp",
        }
    }
}

/// Decodes an image from raw bytes (PNG, BMP, JPEG, GIF or TIFF).
pub fn decode_image(bytes: &[u8]) -> Result<PixelBuffer, IoError> {
    let image = image::load_from_memory(bytes).map_err(|e| IoError::Decode(e.to_string()))?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut buffer = PixelBuffer::transparent(width, height);
    buffer.data_mut().copy_from_slice(rgba.as_raw());
    Ok(buffer)
}

fn encode_image(buffer: &PixelBuffer, format: ImageFormat) -> Result<Vec<u8>, IoError> {
    let image = RgbaImage::from_raw(buffer.width(), buffer.height(), buffer.data().to_vec())
        .ok_or_else(|| IoError::Encode("invalid pixel buffer".to_string()))?;
    let mut out = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut out, format)
        .map_err(|e| IoError::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

/// Encodes the buffer in the given format.
pub fn encode(buffer: &PixelBuffer, format: SaveFormat) -> Result<Vec<u8>, IoError> {
    match format {
        SaveFormat::Png => encode_image(buffer, ImageFormat::Png),
        SaveFormat::Bmp => encode_image(buffer, ImageFormat::Bmp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn png_roundtrip_preserves_pixels() {
        let mut buffer = PixelBuffer::new(4, 3, Color::WHITE);
        buffer.set(1, 1, Color::rgba(10, 20, 30, 128));
        buffer.set(3, 2, Color::TRANSPARENT);

        let bytes = encode(&buffer, SaveFormat::Png).unwrap();
        let decoded = decode_image(&bytes).unwrap();

        assert_eq!(decoded.size(), [4, 3]);
        assert_eq!(decoded.get(1, 1), Some(Color::rgba(10, 20, 30, 128)));
        assert_eq!(decoded.get(3, 2), Some(Color::TRANSPARENT));
        assert_eq!(decoded.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn bmp_roundtrip_preserves_size() {
        let buffer = PixelBuffer::new(8, 5, Color::rgb(200, 100, 50));
        let bytes = encode(&buffer, SaveFormat::Bmp).unwrap();
        let decoded = decode_image(&bytes).unwrap();
        assert_eq!(decoded.size(), [8, 5]);
    }

    #[test]
    fn format_is_guessed_from_extension() {
        assert_eq!(SaveFormat::from_extension("BMP"), SaveFormat::Bmp);
        assert_eq!(SaveFormat::from_extension("dib"), SaveFormat::Bmp);
        assert_eq!(SaveFormat::from_extension("png"), SaveFormat::Png);
        assert_eq!(SaveFormat::from_extension("gif"), SaveFormat::Png);
    }

    #[test]
    fn decoding_garbage_errors() {
        assert!(decode_image(b"not an image").is_err());
    }
}
