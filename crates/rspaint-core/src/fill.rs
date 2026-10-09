//! Flood fill operations (the Fill With Color tool).

use crate::buffer::PixelBuffer;
use crate::color::Color;

/// Replaces every pixel equal to `from` with `to`.
///
/// Returns the number of pixels changed.
pub fn replace_color(buffer: &mut PixelBuffer, from: Color, to: Color) -> usize {
    if from == to {
        return 0;
    }
    let from = from.to_array();
    let to = to.to_array();
    let mut changed = 0;
    for pixel in buffer.data_mut().chunks_exact_mut(4) {
        if pixel == from {
            pixel.copy_from_slice(&to);
            changed += 1;
        }
    }
    changed
}

/// Fills the region connected to `(x, y)` with `new_color` using 4-connectivity.
///
/// When `contiguous` is false, every matching pixel in the image is replaced
/// instead (JS Paint's Shift+fill behavior).
pub fn flood_fill(buffer: &mut PixelBuffer, x: i32, y: i32, new_color: Color, contiguous: bool) {
    if x < 0 || y < 0 {
        return;
    }
    let (width, height) = (buffer.width() as i32, buffer.height() as i32);
    if x >= width || y >= height {
        return;
    }
    let target = match buffer.get(x as u32, y as u32) {
        Some(color) => color,
        None => return,
    };
    if target == new_color {
        return;
    }
    if !contiguous {
        replace_color(buffer, target, new_color);
        return;
    }

    let mut stack = vec![(x, y)];
    while let Some((px, py)) = stack.pop() {
        if px < 0 || py < 0 || px >= width || py >= height {
            continue;
        }
        if buffer.get(px as u32, py as u32) != Some(target) {
            continue;
        }
        buffer.set(px as u32, py as u32, new_color);
        stack.push((px + 1, py));
        stack.push((px - 1, py));
        stack.push((px, py + 1));
        stack.push((px, py - 1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contiguous_fill_respects_barriers() {
        let mut buffer = PixelBuffer::new(5, 5, Color::WHITE);
        // Vertical black wall at x=2, splitting the canvas.
        for y in 0..5 {
            buffer.set(2, y, Color::BLACK);
        }
        flood_fill(&mut buffer, 0, 0, Color::rgb(255, 0, 0), true);
        assert_eq!(buffer.get(0, 0), Some(Color::rgb(255, 0, 0)));
        assert_eq!(buffer.get(1, 4), Some(Color::rgb(255, 0, 0)));
        assert_eq!(buffer.get(3, 0), Some(Color::WHITE), "other side untouched");
        assert_eq!(buffer.get(2, 0), Some(Color::BLACK), "wall untouched");
    }

    #[test]
    fn non_contiguous_fill_replaces_globally() {
        let mut buffer = PixelBuffer::new(5, 5, Color::WHITE);
        for y in 0..5 {
            buffer.set(2, y, Color::BLACK);
        }
        flood_fill(&mut buffer, 0, 0, Color::rgb(0, 0, 255), false);
        assert_eq!(buffer.get(3, 0), Some(Color::rgb(0, 0, 255)));
        assert_eq!(buffer.get(4, 4), Some(Color::rgb(0, 0, 255)));
        assert_eq!(buffer.get(2, 0), Some(Color::BLACK));
    }

    #[test]
    fn filling_with_same_color_is_noop() {
        let mut buffer = PixelBuffer::new(3, 3, Color::WHITE);
        flood_fill(&mut buffer, 0, 0, Color::WHITE, true);
        assert_eq!(buffer.get(0, 0), Some(Color::WHITE));
    }
}
