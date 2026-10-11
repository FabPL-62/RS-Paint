//! Whole-image transformations: flips, rotations, inversion and cropping.

use crate::buffer::PixelBuffer;

/// Mirrors the image left-to-right.
pub fn flip_horizontal(buffer: &PixelBuffer) -> PixelBuffer {
    let (width, height) = (buffer.width(), buffer.height());
    let mut out = PixelBuffer::transparent(width, height);
    for y in 0..height {
        for x in 0..width {
            if let Some(color) = buffer.get(x, y) {
                out.set(width - 1 - x, y, color);
            }
        }
    }
    out
}

/// Mirrors the image top-to-bottom.
pub fn flip_vertical(buffer: &PixelBuffer) -> PixelBuffer {
    let (width, height) = (buffer.width(), buffer.height());
    let mut out = PixelBuffer::transparent(width, height);
    for y in 0..height {
        for x in 0..width {
            if let Some(color) = buffer.get(x, y) {
                out.set(x, height - 1 - y, color);
            }
        }
    }
    out
}

/// Rotates the image 90° clockwise.
pub fn rotate_90(buffer: &PixelBuffer) -> PixelBuffer {
    let (width, height) = (buffer.width(), buffer.height());
    let mut out = PixelBuffer::transparent(height, width);
    for y in 0..height {
        for x in 0..width {
            if let Some(color) = buffer.get(x, y) {
                out.set(height - 1 - y, x, color);
            }
        }
    }
    out
}

/// Rotates the image 180°.
pub fn rotate_180(buffer: &PixelBuffer) -> PixelBuffer {
    let (width, height) = (buffer.width(), buffer.height());
    let mut out = PixelBuffer::transparent(width, height);
    for y in 0..height {
        for x in 0..width {
            if let Some(color) = buffer.get(x, y) {
                out.set(width - 1 - x, height - 1 - y, color);
            }
        }
    }
    out
}

/// Rotates the image 270° clockwise.
pub fn rotate_270(buffer: &PixelBuffer) -> PixelBuffer {
    let (width, height) = (buffer.width(), buffer.height());
    let mut out = PixelBuffer::transparent(height, width);
    for y in 0..height {
        for x in 0..width {
            if let Some(color) = buffer.get(x, y) {
                out.set(y, width - 1 - x, color);
            }
        }
    }
    out
}

/// Rotates the image by an arbitrary angle (degrees, clockwise), expanding the
/// canvas to fit and sampling the nearest neighbor.
pub fn rotate(buffer: &PixelBuffer, degrees: f64) -> PixelBuffer {
    let radians = degrees.to_radians();
    let (cos, sin) = (radians.cos(), radians.sin());
    let width = buffer.width() as f64;
    let height = buffer.height() as f64;

    let new_width = (width * cos.abs() + height * sin.abs()).ceil().max(1.0) as u32;
    let new_height = (width * sin.abs() + height * cos.abs()).ceil().max(1.0) as u32;
    let mut out = PixelBuffer::transparent(new_width, new_height);

    let center_in_x = width / 2.0;
    let center_in_y = height / 2.0;
    let center_out_x = new_width as f64 / 2.0;
    let center_out_y = new_height as f64 / 2.0;

    for oy in 0..new_height {
        for ox in 0..new_width {
            let dx = ox as f64 + 0.5 - center_out_x;
            let dy = oy as f64 + 0.5 - center_out_y;
            let src_x = dx * cos + dy * sin + center_in_x;
            let src_y = -dx * sin + dy * cos + center_in_y;
            let sx = src_x.floor();
            let sy = src_y.floor();
            if sx >= 0.0 && sy >= 0.0 && sx < width && sy < height {
                if let Some(color) = buffer.get(sx as u32, sy as u32) {
                    out.set(ox, oy, color);
                }
            }
        }
    }
    out
}

/// Stretches and skews the image (JS Paint's `stretch_and_skew`).
///
/// `x_scale` / `y_scale` are multipliers (1.0 = unchanged); `h_skew` and
/// `v_skew` are shear angles in radians. The result is expanded to fit and
/// sampled with the nearest neighbor; empty areas are filled with `fill`.
pub fn stretch_and_skew(
    buffer: &PixelBuffer,
    x_scale: f64,
    y_scale: f64,
    h_skew: f64,
    v_skew: f64,
    fill: crate::color::Color,
) -> PixelBuffer {
    let width = buffer.width() as f64 * x_scale;
    let height = buffer.height() as f64 * y_scale;
    let tan_h = h_skew.tan();
    let tan_v = v_skew.tan();

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (x01, y01) in [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)] {
        let x = tan_h * height * x01 + width * y01;
        let y = tan_v * width * y01 + height * x01;
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let bb_w = (max_x - min_x).max(1.0);
    let bb_h = (max_y - min_y).max(1.0);
    let mut out = PixelBuffer::new(bb_w as u32, bb_h as u32, fill);

    let det = 1.0 - tan_h * tan_v;
    if det.abs() < 1e-9 {
        return out;
    }
    let source_w = buffer.width() as f64;
    let source_h = buffer.height() as f64;

    for oy in 0..out.height() {
        for ox in 0..out.width() {
            let big_x = ox as f64 + 0.5 + min_x;
            let big_y = oy as f64 + 0.5 + min_y;
            let sx = (big_x - tan_h * big_y) / det;
            let sy = (big_y - tan_v * big_x) / det;
            let px = (sx / x_scale).floor();
            let py = (sy / y_scale).floor();
            if px >= 0.0 && py >= 0.0 && px < source_w && py < source_h {
                if let Some(color) = buffer.get(px as u32, py as u32) {
                    out.set(ox, oy, color);
                }
            }
        }
    }
    out
}

/// Inverts the RGB channels, keeping alpha.
pub fn invert(buffer: &PixelBuffer) -> PixelBuffer {
    let mut out = buffer.clone();
    for pixel in out.data_mut().chunks_exact_mut(4) {
        pixel[0] = 255 - pixel[0];
        pixel[1] = 255 - pixel[1];
        pixel[2] = 255 - pixel[2];
    }
    out
}

/// Rescales the image to `new_width` x `new_height` with nearest-neighbor.
pub fn scale(buffer: &PixelBuffer, new_width: u32, new_height: u32) -> PixelBuffer {
    let new_width = new_width.max(1);
    let new_height = new_height.max(1);
    let (width, height) = (buffer.width(), buffer.height());
    let mut out = PixelBuffer::transparent(new_width, new_height);
    if width == 0 || height == 0 {
        return out;
    }
    for y in 0..new_height {
        for x in 0..new_width {
            let sx = ((x as u64 * width as u64) / new_width as u64) as u32;
            let sy = ((y as u64 * height as u64) / new_height as u64) as u32;
            if let Some(color) = buffer.get(sx.min(width - 1), sy.min(height - 1)) {
                out.set(x, y, color);
            }
        }
    }
    out
}

/// Crops the image to the given rectangle, clamped to the image bounds.
pub fn crop(buffer: &PixelBuffer, x: i32, y: i32, width: u32, height: u32) -> PixelBuffer {
    let image_w = buffer.width() as i32;
    let image_h = buffer.height() as i32;
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + width as i32).min(image_w);
    let y1 = (y + height as i32).min(image_h);
    if x0 >= x1 || y0 >= y1 {
        return PixelBuffer::transparent(1, 1);
    }
    let mut out = PixelBuffer::transparent((x1 - x0) as u32, (y1 - y0) as u32);
    for oy in 0..(y1 - y0) {
        for ox in 0..(x1 - x0) {
            if let Some(color) = buffer.get((x0 + ox) as u32, (y0 + oy) as u32) {
                out.set(ox as u32, oy as u32, color);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    fn marked(width: u32, height: u32, x: u32, y: u32) -> PixelBuffer {
        let mut buffer = PixelBuffer::new(width, height, Color::WHITE);
        buffer.set(x, y, Color::BLACK);
        buffer
    }

    #[test]
    fn flip_horizontal_mirrors_x() {
        let buffer = marked(3, 1, 0, 0);
        let flipped = flip_horizontal(&buffer);
        assert_eq!(flipped.get(2, 0), Some(Color::BLACK));
        assert_eq!(flipped.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn scale_keeps_corners_and_resizes() {
        let mut buffer = PixelBuffer::new(2, 2, Color::WHITE);
        buffer.set(0, 0, Color::BLACK);
        buffer.set(1, 1, Color::rgb(0, 0, 255));
        let upscaled = scale(&buffer, 4, 4);
        assert_eq!(upscaled.size(), [4, 4]);
        assert_eq!(upscaled.get(0, 0), Some(Color::BLACK));
        assert_eq!(upscaled.get(3, 3), Some(Color::rgb(0, 0, 255)));
        let downscaled = scale(&buffer, 1, 1);
        assert_eq!(downscaled.size(), [1, 1]);
    }

    #[test]
    fn flip_vertical_mirrors_y() {
        let buffer = marked(1, 3, 0, 0);
        let flipped = flip_vertical(&buffer);
        assert_eq!(flipped.get(0, 2), Some(Color::BLACK));
    }

    #[test]
    fn rotate_90_swaps_dimensions_and_maps_corners() {
        let mut buffer = PixelBuffer::new(2, 1, Color::WHITE);
        buffer.set(0, 0, Color::BLACK);
        buffer.set(1, 0, Color::WHITE);
        let rotated = rotate_90(&buffer);
        assert_eq!(rotated.size(), [1, 2]);
        assert_eq!(rotated.get(0, 0), Some(Color::BLACK), "left goes to top");
    }

    #[test]
    fn rotate_180_reverses_order() {
        let mut buffer = PixelBuffer::new(2, 1, Color::WHITE);
        buffer.set(0, 0, Color::BLACK);
        let rotated = rotate_180(&buffer);
        assert_eq!(rotated.get(1, 0), Some(Color::BLACK));
    }

    #[test]
    fn rotate_90_cw_equals_arbitrary_90_on_square() {
        let mut buffer = PixelBuffer::new(2, 2, Color::WHITE);
        buffer.set(0, 0, Color::BLACK); // top-left
        let by_quarter = rotate_90(&buffer);
        let by_angle = rotate(&buffer, 90.0);
        assert_eq!(by_angle.size(), by_quarter.size());
        assert_eq!(
            by_angle.get(1, 0),
            Some(Color::BLACK),
            "top-left -> top-right"
        );
        assert_eq!(by_angle.get(1, 0), by_quarter.get(1, 0));
    }

    #[test]
    fn rotate_zero_is_identity() {
        let buffer = marked(4, 3, 1, 2);
        let rotated = rotate(&buffer, 0.0);
        assert_eq!(rotated.size(), [4, 3]);
        assert_eq!(rotated.get(1, 2), Some(Color::BLACK));
    }

    #[test]
    fn stretch_and_skew_identity_is_unchanged() {
        let buffer = marked(4, 3, 1, 2);
        let out = stretch_and_skew(&buffer, 1.0, 1.0, 0.0, 0.0, Color::TRANSPARENT);
        assert_eq!(out.size(), [4, 3]);
        assert_eq!(out.get(1, 2), Some(Color::BLACK));
    }

    #[test]
    fn stretch_doubles_dimensions() {
        let buffer = marked(2, 2, 0, 0);
        let out = stretch_and_skew(&buffer, 2.0, 2.0, 0.0, 0.0, Color::TRANSPARENT);
        assert_eq!(out.size(), [4, 4]);
        assert_eq!(out.get(0, 0), Some(Color::BLACK));
        assert_eq!(out.get(1, 1), Some(Color::BLACK));
        assert_eq!(out.get(3, 3), Some(Color::WHITE));
    }

    #[test]
    fn horizontal_skew_expands_width() {
        let buffer = marked(2, 1, 0, 0);
        let out = stretch_and_skew(
            &buffer,
            1.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
            0.0,
            Color::TRANSPARENT,
        );
        assert_eq!(out.size(), [3, 1]);
    }

    #[test]
    fn invert_flips_rgb_only() {
        let mut buffer = PixelBuffer::new(1, 1, Color::rgba(10, 20, 30, 128));
        buffer.set(0, 0, Color::rgba(10, 20, 30, 128));
        let out = invert(&buffer);
        assert_eq!(out.get(0, 0), Some(Color::rgba(245, 235, 225, 128)));
    }

    #[test]
    fn crop_extracts_and_clamps() {
        let buffer = marked(5, 5, 3, 3);
        let cropped = crop(&buffer, 2, 2, 2, 2);
        assert_eq!(cropped.size(), [2, 2]);
        assert_eq!(cropped.get(1, 1), Some(Color::BLACK));
        // Out-of-bounds region clamps to the image.
        let clamped = crop(&buffer, 4, 4, 10, 10);
        assert_eq!(clamped.size(), [1, 1]);
    }
}
