//! Brush shapes and pixel stamping.

use crate::buffer::PixelBuffer;
use crate::color::{BlendMode, Color};

/// The shape of a brush tip.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BrushShape {
    /// Round brush.
    Circle,
    /// Square brush.
    Square,
    /// A 1px line from the top-left to the bottom-right (`\`).
    Diagonal,
    /// A 1px line from the bottom-left to the top-right (`/`).
    ReverseDiagonal,
}

/// A brush tip: a shape, a pixel size and a blend mode.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Brush {
    pub shape: BrushShape,
    pub size: u32,
    pub blend: BlendMode,
}

impl Brush {
    /// A single-pixel square brush (the Pencil).
    pub const PENCIL: Self = Self {
        shape: BrushShape::Square,
        size: 1,
        blend: BlendMode::Blend,
    };

    /// Creates a brush with the given shape and size (clamped to at least 1).
    pub fn new(shape: BrushShape, size: u32) -> Self {
        Self {
            shape,
            size: size.max(1),
            blend: BlendMode::Blend,
        }
    }

    /// Sets the brush's blend mode.
    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        self.blend = blend;
        self
    }

    /// Calls `f` for every pixel of the brush footprint centered at `(cx, cy)`.
    pub fn visit(&self, cx: i32, cy: i32, f: impl FnMut(i32, i32)) {
        self.for_each_pixel(cx, cy, f);
    }

    /// Calls `f` for every pixel of the brush tip centered at `(cx, cy)`.
    fn for_each_pixel(&self, cx: i32, cy: i32, mut f: impl FnMut(i32, i32)) {
        let size = self.size.max(1) as i32;
        let offset = (size - 1) / 2;
        match self.shape {
            BrushShape::Diagonal => {
                crate::draw::walk_line((0, 0), (size - 1, size - 1), |lx, ly| {
                    f(cx + lx - offset, cy + ly - offset);
                });
            }
            BrushShape::ReverseDiagonal => {
                crate::draw::walk_line((0, size - 1), (size - 1, 0), |lx, ly| {
                    f(cx + lx - offset, cy + ly - offset);
                });
            }
            shape => {
                if size == 1 {
                    f(cx, cy);
                    return;
                }
                // Radius of size/2 (like JS Paint); the size-3 circle is
                // special-cased to a plus so it is not a full 3x3 square.
                let radius = if size == 3 { 1.0 } else { size as f32 / 2.0 };
                let center = (size - 1) as f32 / 2.0;
                let circle = shape == BrushShape::Circle;
                for dy in 0..size {
                    for dx in 0..size {
                        let inside = if circle {
                            let ox = dx as f32 - center;
                            let oy = dy as f32 - center;
                            ox * ox + oy * oy <= radius * radius
                        } else {
                            true
                        };
                        if inside {
                            f(cx + dx - offset, cy + dy - offset);
                        }
                    }
                }
            }
        }
    }

    /// Stamps the brush tip centered at `(cx, cy)` using the brush's blend mode.
    pub fn stamp(&self, buffer: &mut PixelBuffer, cx: i32, cy: i32, color: Color) {
        self.for_each_pixel(cx, cy, |px, py| {
            if px >= 0 && py >= 0 {
                buffer.paint(px as u32, py as u32, color, self.blend);
            }
        });
    }

    /// Stamps the brush tip, painting each pixel at most once per `painted`
    /// mask (used to keep a single freehand stroke from accumulating on itself).
    pub fn stamp_masked(
        &self,
        buffer: &mut PixelBuffer,
        painted: &mut [bool],
        width: u32,
        cx: i32,
        cy: i32,
        color: Color,
    ) {
        self.for_each_pixel(cx, cy, |px, py| {
            if px >= 0 && py >= 0 && (px as u32) < width {
                let index = py as usize * width as usize + px as usize;
                if index < painted.len() && !painted[index] {
                    painted[index] = true;
                    buffer.paint(px as u32, py as u32, color, self.blend);
                }
            }
        });
    }

    /// Replaces `from` with `to` within the brush tip (the color eraser).
    pub fn replace_in_stamp(
        &self,
        buffer: &mut PixelBuffer,
        cx: i32,
        cy: i32,
        from: Color,
        to: Color,
    ) {
        self.for_each_pixel(cx, cy, |px, py| {
            if px >= 0 && py >= 0 && buffer.get(px as u32, py as u32) == Some(from) {
                buffer.set(px as u32, py as u32, to);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pencil_stamps_single_pixel() {
        let mut buffer = PixelBuffer::transparent(8, 8);
        Brush::PENCIL.stamp(&mut buffer, 3, 4, Color::BLACK);
        assert_eq!(buffer.get(3, 4), Some(Color::BLACK));
        assert_eq!(buffer.get(4, 4), Some(Color::TRANSPARENT));
    }

    #[test]
    fn square_brush_area_grows_quadratically() {
        let mut buffer = PixelBuffer::transparent(16, 16);
        Brush::new(BrushShape::Square, 3).stamp(&mut buffer, 8, 8, Color::BLACK);
        let count = buffer.data().chunks_exact(4).filter(|p| p[3] != 0).count();
        assert_eq!(count, 9);
    }

    #[test]
    fn diagonal_brush_is_a_thin_line() {
        let mut buffer = PixelBuffer::transparent(8, 8);
        Brush::new(BrushShape::Diagonal, 3).stamp(&mut buffer, 4, 4, Color::BLACK);
        assert_eq!(buffer.get(3, 3), Some(Color::BLACK));
        assert_eq!(buffer.get(4, 4), Some(Color::BLACK));
        assert_eq!(buffer.get(5, 5), Some(Color::BLACK));
        assert_eq!(buffer.get(5, 3), Some(Color::TRANSPARENT));
        let count = buffer.data().chunks_exact(4).filter(|p| p[3] != 0).count();
        assert_eq!(count, 3);
    }

    #[test]
    fn reverse_diagonal_brush_is_a_thin_line() {
        let mut buffer = PixelBuffer::transparent(8, 8);
        Brush::new(BrushShape::ReverseDiagonal, 3).stamp(&mut buffer, 4, 4, Color::BLACK);
        assert_eq!(buffer.get(5, 3), Some(Color::BLACK));
        assert_eq!(buffer.get(4, 4), Some(Color::BLACK));
        assert_eq!(buffer.get(3, 5), Some(Color::BLACK));
        assert_eq!(buffer.get(3, 3), Some(Color::TRANSPARENT));
    }

    #[test]
    fn circle_brush_size_three_is_a_cross() {
        let mut buffer = PixelBuffer::transparent(8, 8);
        Brush::new(BrushShape::Circle, 3).stamp(&mut buffer, 4, 4, Color::BLACK);
        assert_eq!(buffer.get(4, 4), Some(Color::BLACK), "center");
        assert_eq!(buffer.get(4, 3), Some(Color::BLACK), "orthogonal");
        assert_eq!(buffer.get(3, 4), Some(Color::BLACK), "orthogonal");
        assert_eq!(
            buffer.get(3, 3),
            Some(Color::TRANSPARENT),
            "corner excluded"
        );
        assert_eq!(
            buffer.get(5, 5),
            Some(Color::TRANSPARENT),
            "corner excluded"
        );
        let count = buffer.data().chunks_exact(4).filter(|p| p[3] != 0).count();
        assert_eq!(count, 5);
    }

    #[test]
    fn circle_brush_size_five_is_not_spiky() {
        let mut buffer = PixelBuffer::transparent(16, 16);
        Brush::new(BrushShape::Circle, 5).stamp(&mut buffer, 8, 8, Color::BLACK);
        // The top row spans three pixels instead of a single spike.
        assert_eq!(buffer.get(7, 6), Some(Color::BLACK));
        assert_eq!(buffer.get(8, 6), Some(Color::BLACK));
        assert_eq!(buffer.get(9, 6), Some(Color::BLACK));
        assert_eq!(buffer.get(6, 6), Some(Color::TRANSPARENT));
    }

    #[test]
    fn circle_brush_is_round() {
        let mut buffer = PixelBuffer::transparent(16, 16);
        Brush::new(BrushShape::Circle, 5).stamp(&mut buffer, 8, 8, Color::BLACK);
        // A 5px circle covers the center and 4 orthogonal neighbors, but not
        // the diagonal corners of its bounding box.
        assert_eq!(buffer.get(8, 8), Some(Color::BLACK));
        assert_eq!(buffer.get(8, 6), Some(Color::BLACK));
        assert_eq!(buffer.get(6, 6), Some(Color::TRANSPARENT));
    }

    #[test]
    fn stamping_off_canvas_is_safe() {
        let mut buffer = PixelBuffer::transparent(4, 4);
        Brush::new(BrushShape::Circle, 8).stamp(&mut buffer, 0, 0, Color::BLACK);
    }
}
