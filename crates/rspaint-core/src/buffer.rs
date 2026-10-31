//! An owned RGBA8 pixel buffer.

use crate::color::Color;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

pub(crate) const HISTORY_BLOCK_SIZE: u32 = 128;

fn next_version() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let version = NEXT.fetch_add(1, Ordering::Relaxed);
    assert_ne!(version, 0, "pixel version counter exhausted");
    version
}

/// An owned, top-left-origin RGBA8 image buffer with 4 bytes per pixel.
///
/// The raw bytes are laid out row-major and can be uploaded directly to a GPU
/// texture. Alpha is stored non-premultiplied.
///
/// Cloning is cheap: the pixels are shared until the clone is written to
/// (copy-on-write). The first write to a shared buffer copies the whole image.
/// Long-lived editor history uses separate block-shared snapshots instead.
#[derive(Clone)]
pub struct PixelBuffer {
    width: u32,
    height: u32,
    data: Arc<Vec<u8>>,
    versions: Arc<Vec<u64>>,
    write_version: u64,
    /// Blocks still bearing this generation have the alpha of the last known
    /// uniform initialization/fill. Later mutations use a distinct generation.
    uniform_version: u64,
    uniform_alpha: u8,
}

// Tracking is derived state, not part of image equality or debug output.
impl PartialEq for PixelBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width && self.height == other.height && self.data == other.data
    }
}
impl Eq for PixelBuffer {}
impl std::fmt::Debug for PixelBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PixelBuffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("data", &self.data)
            .finish()
    }
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
        let version = next_version();
        let blocks = width.div_ceil(HISTORY_BLOCK_SIZE) as usize
            * height.div_ceil(HISTORY_BLOCK_SIZE) as usize;
        Self {
            width,
            height,
            data: Arc::new(data),
            versions: Arc::new(vec![version; blocks]),
            write_version: next_version(),
            uniform_version: version,
            uniform_alpha: fill[3],
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

    /// Weak storage identity for derived caches. Every mutation uses make_mut,
    /// which invalidates weak identities even when there is only one owner.
    /// Unlike a strong snapshot, this does not retain or copy the pixel Vec.
    pub(crate) fn weak_data(&self) -> std::sync::Weak<Vec<u8>> {
        Arc::downgrade(&self.data)
    }

    /// Mutable access to the raw RGBA8 bytes.
    pub fn data_mut(&mut self) -> &mut [u8] {
        self.mark_region(0, 0, self.width, self.height);
        Arc::make_mut(&mut self.data).as_mut_slice()
    }

    /// Mutable RGBA bytes of one clipped row span. Only the blocks intersecting
    /// the returned slice are marked as possibly changed. Raw data_mut instead
    /// marks every block, so unrestricted byte writes remain safe for history.
    pub fn row_mut(&mut self, y: u32, x: u32, width: u32) -> &mut [u8] {
        if y >= self.height || x >= self.width || width == 0 {
            return &mut [];
        }
        let width = width.min(self.width - x);
        self.mark_region(x, y, width, 1);
        let start = self.index(x, y);
        &mut Arc::make_mut(&mut self.data)[start..start + width as usize * 4]
    }

    pub(crate) fn history_versions(&self) -> Arc<Vec<u64>> {
        self.versions.clone()
    }

    pub(crate) fn known_block_alpha(&self, version: u64) -> Option<u8> {
        (version == self.uniform_version).then_some(self.uniform_alpha)
    }

    pub(crate) fn known_empty_alpha(&self) -> bool {
        self.uniform_alpha == 0 && self.versions.iter().all(|v| *v == self.uniform_version)
    }

    pub(crate) fn mostly_known_transparent(&self) -> bool {
        self.uniform_alpha == 0
            && self
                .versions
                .iter()
                .filter(|v| **v == self.uniform_version)
                .count()
                .saturating_mul(4)
                >= self.versions.len().saturating_mul(3)
    }

    /// Used only after reconstruction from these exact immutable blocks.
    pub(crate) fn adopt_history_versions(&mut self, versions: Arc<Vec<u64>>) {
        assert_eq!(versions.len(), self.versions.len());
        self.versions = versions;
        self.write_version = next_version();
    }

    fn mark_region(&mut self, x: u32, y: u32, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        // A shared table belongs to a snapshot or another editable clone.
        // Allocate a unique generation once for this mutation batch, never
        // once per pixel, and detach only the small version table.
        if Arc::strong_count(&self.versions) > 1 {
            self.write_version = next_version();
        } else if width == 1 && height == 1 {
            // Most stamped pixels revisit a block already touched in this
            // batch. Avoid another make_mut ownership check in that hot path.
            let cols = self.width.div_ceil(HISTORY_BLOCK_SIZE) as usize;
            let index =
                (y / HISTORY_BLOCK_SIZE) as usize * cols + (x / HISTORY_BLOCK_SIZE) as usize;
            if self.versions[index] == self.write_version {
                return;
            }
        }
        let version = self.write_version;
        let versions = Arc::make_mut(&mut self.versions);
        if x == 0 && y == 0 && width == self.width && height == self.height {
            versions.fill(version);
            return;
        }
        let cols = self.width.div_ceil(HISTORY_BLOCK_SIZE) as usize;
        let left = x / HISTORY_BLOCK_SIZE;
        let right = (x + width - 1) / HISTORY_BLOCK_SIZE;
        for row in y / HISTORY_BLOCK_SIZE..=(y + height - 1) / HISTORY_BLOCK_SIZE {
            let start = row as usize * cols + left as usize;
            versions[start..start + (right - left + 1) as usize].fill(version);
        }
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
        self.mark_region(x, y, 1, 1);
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
        out.mark_region(0, 0, w, h);
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
        if w == 0 || h == 0 {
            return;
        }
        self.mark_region(x, y, w, h);
        let len = w as usize * 4;
        let width = self.width as usize;
        let x = x as usize;
        let dst = Arc::make_mut(&mut self.data);
        for row in 0..h {
            let start = ((y + row) as usize * width + x) * 4;
            dst[start..start + len].copy_from_slice(&src.data[start..start + len]);
        }
    }

    /// Copies a whole buffer into this one at `(x, y)`, clipping to the edges.
    pub fn blit(&mut self, src: &Self, x: u32, y: u32) {
        if x >= self.width || y >= self.height {
            return;
        }
        let w = src.width.min(self.width - x);
        let h = src.height.min(self.height - y);
        self.mark_region(x, y, w, h);
        let len = w as usize * 4;
        let width = self.width as usize;
        let x = x as usize;
        let dst = Arc::make_mut(&mut self.data);
        for row in 0..h {
            let source = src.index(0, row);
            let start = ((y + row) as usize * width + x) * 4;
            dst[start..start + len].copy_from_slice(&src.data[source..source + len]);
        }
    }

    /// Returns a copy resized to `width` x `height`, keeping the top-left
    /// overlap and filling any new area with `fill`.
    pub fn resized(&self, width: u32, height: u32, fill: Color) -> Self {
        let mut out = Self::new(width, height, fill);
        let copy_w = width.min(self.width);
        let copy_h = height.min(self.height);
        out.mark_region(0, 0, copy_w, copy_h);
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
        self.mark_region(0, 0, self.width, self.height);
        self.uniform_version = self.write_version;
        self.uniform_alpha = color.a;
        let color = color.to_array();
        for pixel in Arc::make_mut(&mut self.data).chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
        self.write_version = next_version();
    }

    /// Clears the buffer to fully transparent.
    pub fn clear(&mut self) {
        self.fill(Color::TRANSPARENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_known_alpha(buffer: &PixelBuffer) {
        let versions = buffer.history_versions();
        let cols = buffer.width().div_ceil(HISTORY_BLOCK_SIZE);
        for (index, version) in versions.iter().enumerate() {
            if let Some(alpha) = buffer.known_block_alpha(*version) {
                let x = index as u32 % cols * HISTORY_BLOCK_SIZE;
                let y = index as u32 / cols * HISTORY_BLOCK_SIZE;
                for yy in y..(y + HISTORY_BLOCK_SIZE).min(buffer.height()) {
                    for xx in x..(x + HISTORY_BLOCK_SIZE).min(buffer.width()) {
                        assert_eq!(buffer.get(xx, yy).unwrap().a, alpha);
                    }
                }
            }
        }
    }

    #[test]
    fn uniform_alpha_seeds_are_revoked_by_first_writes_and_reset_by_fill() {
        let mut buffer = PixelBuffer::transparent(259, 259);
        assert!(buffer.known_empty_alpha());
        buffer.set(1, 1, Color::BLACK);
        assert!(!buffer.known_empty_alpha());
        check_known_alpha(&buffer);
        let mut left = buffer.clone();
        let mut right = buffer.clone();
        left.row_mut(128, 128, 1)
            .copy_from_slice(&[10, 20, 30, 127]);
        right.set(258, 258, Color::BLACK);
        check_known_alpha(&left);
        check_known_alpha(&right);
        buffer.fill(Color::rgba(30, 60, 90, 73));
        check_known_alpha(&buffer);
        assert!(buffer
            .history_versions()
            .iter()
            .all(|v| buffer.known_block_alpha(*v) == Some(73)));
        buffer.set(258, 258, Color::TRANSPARENT);
        check_known_alpha(&buffer);
        buffer.clear();
        assert!(buffer.known_empty_alpha());
        buffer.data_mut()[3] = 255;
        assert!(!buffer.known_empty_alpha());
        assert!(buffer
            .history_versions()
            .iter()
            .all(|v| buffer.known_block_alpha(*v).is_none()));
    }

    #[test]
    fn copying_constructors_do_not_retain_fill_proofs_for_copied_pixels() {
        let mut source = PixelBuffer::transparent(259, 259);
        source.set(1, 1, Color::BLACK);
        let cropped = source.cropped(0, 0, 130, 130);
        let resized = source.resized(390, 390, Color::TRANSPARENT);
        let offset = source.resized_with_offset(390, 390, 50, 50, Color::rgba(1, 2, 3, 73));
        for buffer in [&cropped, &resized, &offset] {
            assert!(!buffer.known_empty_alpha());
            check_known_alpha(buffer);
        }
    }

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

    #[test]
    fn block_versions_track_clipped_mutations_and_forked_clones() {
        let base = PixelBuffer::new(257, 259, Color::WHITE);
        let initial = base.history_versions();
        let mut left = base.clone();
        let mut right = base.clone();
        left.set(127, 127, Color::BLACK);
        right.set(127, 127, Color::rgb(10, 20, 30));
        let l = left.history_versions();
        let r = right.history_versions();
        assert_ne!(l[0], r[0], "sibling writes need distinct versions");
        for index in 1..9 {
            assert_eq!(l[index], initial[index]);
        }
        assert!(left.row_mut(999, 0, 10).is_empty());
        assert!(left.row_mut(0, 999, 10).is_empty());
        assert_eq!(left.history_versions(), l);
        left.row_mut(128, 127, 999).fill(30);
        let row = left.history_versions();
        for index in 0..9 {
            assert_eq!(row[index] != l[index], [3, 4, 5].contains(&index));
        }
        let patch = PixelBuffer::new(3, 3, Color::BLACK);
        left.blit(&patch, 127, 127);
        let blit = left.history_versions();
        for index in 0..9 {
            assert_eq!(blit[index] != row[index], [0, 1, 3, 4].contains(&index));
        }
        left.copy_region_from(&base, 256, 258, u32::MAX, u32::MAX);
        let copied = left.history_versions();
        for index in 0..9 {
            assert_eq!(copied[index] != blit[index], index == 8);
        }
        left.data_mut()[0] = 10;
        let raw = left.history_versions();
        assert!(raw.iter().zip(copied.iter()).all(|(a, b)| a != b));
        left.fill(Color::WHITE);
        assert!(left
            .history_versions()
            .iter()
            .zip(raw.iter())
            .all(|(a, b)| a != b));
        assert_eq!(left, base, "tracking must not affect image equality");
    }
}
