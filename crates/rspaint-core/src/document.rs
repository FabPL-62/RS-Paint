//! The document being edited.

use crate::buffer::PixelBuffer;
use crate::color::Color;

/// Default canvas width used by JS Paint.
pub const DEFAULT_CANVAS_WIDTH: u32 = 683;
/// Default canvas height used by JS Paint.
pub const DEFAULT_CANVAS_HEIGHT: u32 = 384;

/// The state of a single image being edited.
///
/// This is the minimal starting point for Phase 0; layers, selections and the
/// history graph will be added in later phases.
#[derive(Clone, Debug)]
pub struct Document {
    /// The pixels of the image.
    pub buffer: PixelBuffer,
    /// Whether the image supports transparency (JS Paint's "Transparent" mode).
    pub transparent: bool,
    /// The color used when clearing or starting a new image.
    pub background: Color,
    /// The display name, e.g. `"Untitled"`.
    pub name: String,
    /// Whether the document has unsaved changes.
    pub modified: bool,
}

impl Document {
    /// Creates a new opaque white document of the given size.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            buffer: PixelBuffer::new(width, height, Color::WHITE),
            transparent: false,
            background: Color::WHITE,
            name: String::from("Untitled"),
            modified: false,
        }
    }

    /// The document width in pixels.
    pub fn width(&self) -> u32 {
        self.buffer.width()
    }

    /// The document height in pixels.
    pub fn height(&self) -> u32 {
        self.buffer.height()
    }
}

impl Default for Document {
    fn default() -> Self {
        Self::new(DEFAULT_CANVAS_WIDTH, DEFAULT_CANVAS_HEIGHT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uses_jspaint_dimensions() {
        let document = Document::default();
        assert_eq!(document.width(), DEFAULT_CANVAS_WIDTH);
        assert_eq!(document.height(), DEFAULT_CANVAS_HEIGHT);
        assert!(!document.transparent);
        assert_eq!(document.buffer.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn new_document_is_unmodified() {
        let document = Document::new(16, 16);
        assert!(!document.modified);
        assert_eq!(document.name, "Untitled");
    }
}
