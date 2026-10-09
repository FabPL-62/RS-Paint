//! An owned RGBA8 pixel buffer.

use crate::color::Color;

/// An owned, top-left-origin RGBA8 image buffer with 4 bytes per pixel.
///
/// The raw bytes are laid out row-major and can be uploaded directly to a GPU
/// texture. Alpha is stored non-premultiplied.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PixelBuffer {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl PixelBuffer {
    /// Creates a buffer of the given size filled with `fill`.
    ///
    /// Dimensions are clamped to a minimum of 1 so the buffer is never empty.
    pub fn new(width: u32, height: u32, fill: Color) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let bytes = width as usize * height as usize * 4;
        let mut data = vec![0u8; bytes];
        let fill = fill.to_array();
        for pixel in data.chunks_exact_mut(4) {
            pixel.copy_from_slice(&fill);
        }
        Self {
            width,
            height,
            data,
        }
    }

    /// Creates a fully transparent buffer of the given size.
    pub fn transparent(width: u32, height: u32) -> Self {
        Self::new(width, height, Color::TRANSPARENT)
    }

    /// The buffer width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The buffer height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The buffer size as `[width, height]`.
    pub fn size(&self) -> [usize; 2] {
        [self.width as usize, self.height as usize]
    }

    /// The raw RGBA8 bytes, row-major, 4 bytes per pixel.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Mutable access to the raw RGBA8 bytes.
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    fn index(&self, x: u32, y: u32) -> usize {
        (y as usize * self.width as usize + x as usize) * 4
    }

    /// Whether the given coordinates are within bounds.
    pub fn contains(&self, x: u32, y: u32) -> bool {
        x < self.width && y < self.height
    }

    /// Returns the color at `(x, y)`, or `None` if out of bounds.
    pub fn get(&self, x: u32, y: u32) -> Option<Color> {
        if !self.contains(x, y) {
            return None;
        }
        let i = self.index(x, y);
        Some(Color::rgba(
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ))
    }

    /// Sets the color at `(x, y)`, ignoring out-of-bounds writes.
    pub fn set(&mut self, x: u32, y: u32, color: Color) {
        if !self.contains(x, y) {
            return;
        }
        let i = self.index(x, y);
        self.data[i..i + 4].copy_from_slice(&color.to_array());
    }

    /// Extracts a rectangular region as contiguous RGBA8 bytes.
    ///
    /// The region is clamped to the buffer bounds.
    pub fn region_rgba(&self, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let w = w.min(self.width - x);
        let h = h.min(self.height - y);
        let mut out = Vec::with_capacity(w as usize * h as usize * 4);
        for row in 0..h {
            let start = self.index(x, y + row);
            out.extend_from_slice(&self.data[start..start + (w as usize * 4)]);
        }
        out
    }

    /// Copies a rectangular region from another buffer of the same size.
    ///
    /// The region is clamped to the buffer bounds. Does nothing if the sizes differ.
    pub fn copy_region_from(&mut self, src: &Self, x: u32, y: u32, w: u32, h: u32) {
        if self.width != src.width || self.height != src.height {
            return;
        }
        let x = x.min(self.width);
        let y = y.min(self.height);
        let w = w.min(self.width - x);
        let h = h.min(self.height - y);
        let len = w as usize * 4;
        for row in 0..h {
            let start = self.index(x, y + row);
            self.data[start..start + len].copy_from_slice(&src.data[start..start + len]);
        }
    }

    /// Returns a copy resized to `width` x `height`, keeping the top-left
    /// overlap and filling any new area with `fill`.
    pub fn resized(&self, width: u32, height: u32, fill: Color) -> Self {
        let mut out = Self::new(width, height, fill);
        let copy_w = width.min(self.width);
        let copy_h = height.min(self.height);
        let len = copy_w as usize * 4;
        for y in 0..copy_h {
            let src = self.index(0, y);
            let dst = out.index(0, y);
            out.data[dst..dst + len].copy_from_slice(&self.data[src..src + len]);
        }
        out
    }

    /// Fills the whole buffer with a single color.
    pub fn fill(&mut self, color: Color) {
        let color = color.to_array();
        for pixel in self.data.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    /// Clears the buffer to fully transparent.
    pub fn clear(&mut self) {
        self.fill(Color::TRANSPARENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fills_and_exposes_size() {
        let buffer = PixelBuffer::new(3, 2, Color::WHITE);
        assert_eq!(buffer.width(), 3);
        assert_eq!(buffer.height(), 2);
        assert_eq!(buffer.size(), [3, 2]);
        assert_eq!(buffer.data().len(), 3 * 2 * 4);
        assert_eq!(buffer.get(2, 1), Some(Color::WHITE));
    }

    #[test]
    fn dimensions_are_clamped_to_one() {
        let buffer = PixelBuffer::new(0, 0, Color::BLACK);
        assert_eq!(buffer.size(), [1, 1]);
    }

    #[test]
    fn set_and_get_roundtrip() {
        let mut buffer = PixelBuffer::transparent(4, 4);
        buffer.set(1, 2, Color::rgb(10, 20, 30));
        assert_eq!(buffer.get(1, 2), Some(Color::rgb(10, 20, 30)));
        assert_eq!(buffer.get(0, 0), Some(Color::TRANSPARENT));
    }

    #[test]
    fn out_of_bounds_is_safe() {
        let mut buffer = PixelBuffer::transparent(2, 2);
        assert_eq!(buffer.get(5, 5), None);
        buffer.set(5, 5, Color::BLACK);
        assert_eq!(buffer.get(5, 5), None);
    }

    #[test]
    fn region_rgba_extracts_subrect() {
        let mut buffer = PixelBuffer::transparent(4, 4);
        buffer.set(1, 1, Color::rgb(1, 2, 3));
        buffer.set(2, 1, Color::rgb(4, 5, 6));
        let region = buffer.region_rgba(1, 1, 2, 1);
        assert_eq!(region, vec![1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn region_rgba_clamps_out_of_bounds() {
        let buffer = PixelBuffer::new(3, 3, Color::BLACK);
        let region = buffer.region_rgba(2, 2, 10, 10);
        assert_eq!(region, vec![0, 0, 0, 255]);
    }

    #[test]
    fn copy_region_from_restores_pixels() {
        let base = PixelBuffer::new(3, 3, Color::WHITE);
        let mut buffer = PixelBuffer::new(3, 3, Color::BLACK);
        buffer.copy_region_from(&base, 1, 1, 2, 2);
        assert_eq!(buffer.get(0, 0), Some(Color::BLACK));
        assert_eq!(buffer.get(1, 1), Some(Color::WHITE));
        assert_eq!(buffer.get(2, 2), Some(Color::WHITE));
    }

    #[test]
    fn resized_keeps_overlap_and_fills_rest() {
        let mut buffer = PixelBuffer::new(2, 2, Color::BLACK);
        buffer.set(0, 0, Color::WHITE);
        let bigger = buffer.resized(4, 4, Color::rgb(0, 0, 255));
        assert_eq!(bigger.get(0, 0), Some(Color::WHITE));
        assert_eq!(bigger.get(1, 1), Some(Color::BLACK));
        assert_eq!(bigger.get(3, 3), Some(Color::rgb(0, 0, 255)));
        let smaller = buffer.resized(1, 1, Color::WHITE);
        assert_eq!(smaller.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn clear_makes_transparent() {
        let mut buffer = PixelBuffer::new(2, 2, Color::BLACK);
        buffer.clear();
        assert_eq!(buffer.get(0, 0), Some(Color::TRANSPARENT));
        assert_eq!(buffer.get(1, 1), Some(Color::TRANSPARENT));
    }
}
