//! An owned RGBA8 pixel buffer.

use crate::color::Color;
use std::sync::Arc;

/// An owned, top-left-origin RGBA8 image buffer with 4 bytes per pixel.
///
/// The raw bytes are laid out row-major and can be uploaded directly to a GPU
/// texture. Alpha is stored non-premultiplied.
///
/// Cloning is cheap: the pixels are shared until the clone is written to
/// (copy-on-write). This keeps per-stroke snapshots and undo history from
/// copying the whole image.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PixelBuffer {
    width: u32,
    height: u32,
    data: Arc<Vec<u8>>,
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
            data: Arc::new(data),
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

    /// A cheap, shared handle to the raw RGBA8 bytes.
    ///
    /// Used to hand the full-canvas bytes to the renderer without copying them;
    /// the next mutation copies them first (copy-on-write).
    pub fn shared_data(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.data)
    }

    /// Mutable access to the raw RGBA8 bytes.
    pub fn data_mut(&mut self) -> &mut [u8] {
        Arc::make_mut(&mut self.data).as_mut_slice()
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
        let data = Arc::make_mut(&mut self.data);
        data[i..i + 4].copy_from_slice(&color.to_array());
    }

    /// Paints `color` at `(x, y)` using the given blend mode.
    ///
    /// [`BlendMode::Blend`](crate::color::BlendMode::Blend) composites the color
    /// over the existing pixel (source-over); `Replace` overwrites it.
    pub fn paint(&mut self, x: u32, y: u32, color: Color, blend: crate::color::BlendMode) {
        match blend {
            crate::color::BlendMode::Blend => {
                if let Some(dst) = self.get(x, y) {
                    self.set(x, y, color.over(dst));
                }
            }
            crate::color::BlendMode::Replace => self.set(x, y, color),
        }
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

    /// Returns a copy of the rectangular region `(x, y, w, h)`.
    ///
    /// The region is clamped to the buffer bounds (and kept non-empty).
    pub fn cropped(&self, x: u32, y: u32, w: u32, h: u32) -> Self {
        let x = x.min(self.width);
        let y = y.min(self.height);
        let w = w.min(self.width - x).max(1);
        let h = h.min(self.height - y).max(1);
        let mut out = Self::transparent(w, h);
        let len = w as usize * 4;
        let dst = Arc::make_mut(&mut out.data);
        for row in 0..h {
            let src = self.index(x, y + row);
            let start = row as usize * len;
            dst[start..start + len].copy_from_slice(&self.data[src..src + len]);
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
        let width = self.width as usize;
        let x = x as usize;
        let dst = Arc::make_mut(&mut self.data);
        for row in 0..h {
            let start = ((y + row) as usize * width + x) * 4;
            dst[start..start + len].copy_from_slice(&src.data[start..start + len]);
        }
    }

    /// Returns a copy resized to `width` x `height`, keeping the top-left
    /// overlap and filling any new area with `fill`.
    pub fn resized(&self, width: u32, height: u32, fill: Color) -> Self {
        let mut out = Self::new(width, height, fill);
        let copy_w = width.min(self.width);
        let copy_h = height.min(self.height);
        let len = copy_w as usize * 4;
        let out_width = out.width as usize;
        let data = Arc::make_mut(&mut out.data);
        for y in 0..copy_h {
            let src = self.index(0, y);
            let dst = y as usize * out_width * 4;
            data[dst..dst + len].copy_from_slice(&self.data[src..src + len]);
        }
        out
    }

    /// Returns a resized copy with the old content placed at `(offset_x, offset_y)`.
    ///
    /// Used by the canvas resize handles to keep the content anchored to the
    /// edge opposite the dragged one.
    pub fn resized_with_offset(
        &self,
        width: u32,
        height: u32,
        offset_x: i32,
        offset_y: i32,
        fill: Color,
    ) -> Self {
        let mut out = Self::new(width, height, fill);
        for y in 0..self.height() {
            for x in 0..self.width() {
                let dx = x as i32 + offset_x;
                let dy = y as i32 + offset_y;
                if dx >= 0 && dy >= 0 && (dx as u32) < width && (dy as u32) < height {
                    if let Some(color) = self.get(x, y) {
                        out.set(dx as u32, dy as u32, color);
                    }
                }
            }
        }
        out
    }

    /// Fills the whole buffer with a single color.
    pub fn fill(&mut self, color: Color) {
        let color = color.to_array();
        for pixel in Arc::make_mut(&mut self.data).chunks_exact_mut(4) {
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
    fn resized_with_offset_anchors_content() {
        let mut buffer = PixelBuffer::new(2, 2, Color::WHITE);
        buffer.set(0, 0, Color::BLACK);
        // Grow to the top-left: the content shifts down-right.
        let grown = buffer.resized_with_offset(4, 4, 2, 2, Color::WHITE);
        assert_eq!(grown.get(2, 2), Some(Color::BLACK));
        assert_eq!(grown.get(0, 0), Some(Color::WHITE));
        // Shrink from the top-left keeps the bottom-right part.
        let mut bottom_right = PixelBuffer::new(2, 2, Color::WHITE);
        bottom_right.set(1, 1, Color::BLACK);
        let shrunk = bottom_right.resized_with_offset(1, 1, -1, -1, Color::WHITE);
        assert_eq!(shrunk.size(), [1, 1]);
        assert_eq!(shrunk.get(0, 0), Some(Color::BLACK));
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

    #[test]
    fn clones_are_independent_after_a_write() {
        let mut a = PixelBuffer::new(2, 2, Color::BLACK);
        let mut b = a.clone();
        // A clone shares the pixels until one side is written to.
        a.set(0, 0, Color::WHITE);
        b.set(1, 1, Color::rgb(200, 10, 20));
        assert_eq!(a.get(0, 0), Some(Color::WHITE));
        assert_eq!(a.get(1, 1), Some(Color::BLACK));
        assert_eq!(b.get(0, 0), Some(Color::BLACK));
        assert_eq!(b.get(1, 1), Some(Color::rgb(200, 10, 20)));
    }
}
