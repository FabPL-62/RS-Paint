//! Original pixels captured lazily before a preview touches each block.
use super::{Color, PixelBuffer, RectI};
use std::{collections::HashMap, sync::Arc};

const BLOCK: u32 = 128;

pub(super) trait OriginalPixels {
    fn get(&self, x: u32, y: u32) -> Option<Color>;
}

impl OriginalPixels for PixelBuffer {
    fn get(&self, x: u32, y: u32) -> Option<Color> {
        PixelBuffer::get(self, x, y)
    }
}

#[derive(Clone)]
pub(super) struct PreviewBackup {
    width: u32,
    height: u32,
    tiles: Arc<HashMap<(u32, u32), PixelBuffer>>,
    pub(super) bounds: Option<RectI>,
}

impl PreviewBackup {
    pub(super) fn new(pixels: &PixelBuffer) -> Self {
        Self {
            width: pixels.width(),
            height: pixels.height(),
            tiles: Arc::default(),
            bounds: None,
        }
    }

    pub(super) fn capture(&mut self, pixels: &PixelBuffer, rect: RectI) {
        assert_eq!((pixels.width(), pixels.height()), (self.width, self.height));
        let Some((x, y, w, h)) = rect.clamped(self.width, self.height) else {
            return;
        };
        self.bounds = Some(self.bounds.map_or(rect, |old| old.union(rect)));
        let tiles = Arc::make_mut(&mut self.tiles);
        for row in y / BLOCK..=(y + h - 1) / BLOCK {
            for col in x / BLOCK..=(x + w - 1) / BLOCK {
                tiles.entry((col, row)).or_insert_with(|| {
                    let tx = col * BLOCK;
                    let ty = row * BLOCK;
                    pixels.cropped(
                        tx,
                        ty,
                        BLOCK.min(self.width - tx),
                        BLOCK.min(self.height - ty),
                    )
                });
            }
        }
    }

    pub(super) fn restore(&self, pixels: &mut PixelBuffer, rect: RectI) {
        assert_eq!((pixels.width(), pixels.height()), (self.width, self.height));
        let Some((x, y, w, h)) = rect.clamped(self.width, self.height) else {
            return;
        };
        for row in y / BLOCK..=(y + h - 1) / BLOCK {
            for col in x / BLOCK..=(x + w - 1) / BLOCK {
                let Some(tile) = self.tiles.get(&(col, row)) else {
                    continue;
                };
                let tx = col * BLOCK;
                let ty = row * BLOCK;
                let left = x.max(tx);
                let top = y.max(ty);
                let right = (x + w).min(tx + tile.width());
                let bottom = (y + h).min(ty + tile.height());
                for dy in top..bottom {
                    let start = ((dy - ty) * tile.width() + left - tx) as usize * 4;
                    let bytes = &tile.data()[start..start + (right - left) as usize * 4];
                    pixels
                        .row_mut(dy, left, right - left)
                        .copy_from_slice(bytes);
                }
            }
        }
    }

    pub(super) fn restore_all(&self, pixels: &mut PixelBuffer) {
        for (&(col, row), tile) in self.tiles.iter() {
            pixels.blit(tile, col * BLOCK, row * BLOCK);
        }
    }
}

impl OriginalPixels for PreviewBackup {
    fn get(&self, x: u32, y: u32) -> Option<Color> {
        self.tiles
            .get(&(x / BLOCK, y / BLOCK))?
            .get(x % BLOCK, y % BLOCK)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_keeps_original_blocks_across_overlapping_previews_and_edges() {
        let mut pixels = PixelBuffer::new(259, 131, Color::WHITE);
        pixels.set(258, 130, Color::rgba(10, 20, 30, 40));
        let original = pixels.clone();
        let mut backup = PreviewBackup::new(&pixels);
        let first = RectI::between((127, 127), (129, 129));
        backup.capture(&pixels, first);
        assert_eq!(backup.tiles.len(), 4);
        pixels.set(128, 128, Color::BLACK);
        let copied = backup.clone();
        let next = RectI::between((128, 128), (300, 180));
        backup.capture(&pixels, next);
        assert_eq!(backup.get(128, 128), Some(Color::WHITE));
        assert_eq!(copied.tiles.len(), 4);
        pixels.set(258, 130, Color::BLACK);
        backup.restore(&mut pixels, first);
        assert_eq!(pixels.get(128, 128), Some(Color::WHITE));
        assert_eq!(pixels.get(258, 130), Some(Color::BLACK));
        backup.restore_all(&mut pixels);
        assert_eq!(pixels, original);
        let bytes: usize = backup.tiles.values().map(|tile| tile.data().len()).sum();
        assert!(bytes < original.data().len());
    }
}
