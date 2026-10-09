//! Selection extraction and compositing.

use crate::buffer::PixelBuffer;
use crate::color::Color;

/// Copies a rectangular region out of `source`.
///
/// The result is always `width` x `height`; areas outside the source bounds
/// become transparent.
pub fn extract_rect(source: &PixelBuffer, x: i32, y: i32, width: u32, height: u32) -> PixelBuffer {
    let width = width.max(1);
    let height = height.max(1);
    let mut out = PixelBuffer::transparent(width, height);
    for oy in 0..height as i32 {
        for ox in 0..width as i32 {
            let sx = x + ox;
            let sy = y + oy;
            if sx >= 0 && sy >= 0 {
                if let Some(color) = source.get(sx as u32, sy as u32) {
                    out.set(ox as u32, oy as u32, color);
                }
            }
        }
    }
    out
}

/// Fills a rectangular region of `source` with `fill`.
pub fn clear_rect(source: &mut PixelBuffer, x: i32, y: i32, width: u32, height: u32, fill: Color) {
    for oy in 0..height as i32 {
        for ox in 0..width as i32 {
            let dx = x + ox;
            let dy = y + oy;
            if dx >= 0 && dy >= 0 {
                source.set(dx as u32, dy as u32, fill);
            }
        }
    }
}

/// Draws `pixels` onto `source` at `(x, y)`.
///
/// Fully transparent pixels are skipped. When `transparent_bg` is set, pixels
/// equal to it are also skipped (JS Paint's transparent selection mode).
pub fn stamp(
    source: &mut PixelBuffer,
    pixels: &PixelBuffer,
    x: i32,
    y: i32,
    transparent_bg: Option<Color>,
) {
    for oy in 0..pixels.height() as i32 {
        for ox in 0..pixels.width() as i32 {
            let Some(color) = pixels.get(ox as u32, oy as u32) else {
                continue;
            };
            if color.a == 0 || Some(color) == transparent_bg {
                continue;
            }
            let dx = x + ox;
            let dy = y + oy;
            if dx >= 0 && dy >= 0 {
                source.set(dx as u32, dy as u32, color);
            }
        }
    }
}

/// Fills only the pixels that [`stamp`] would draw at `(x, y)`.
///
/// This mirrors `stamp`: fully transparent pixels and (when `transparent_bg`
/// is set) background-colored pixels are left untouched. Used to cut a
/// selection out of its original location without erasing the content around
/// it (important for free-form/lasso selections).
pub fn clear_masked(
    source: &mut PixelBuffer,
    pixels: &PixelBuffer,
    x: i32,
    y: i32,
    transparent_bg: Option<Color>,
    fill: Color,
) {
    for oy in 0..pixels.height() as i32 {
        for ox in 0..pixels.width() as i32 {
            let Some(color) = pixels.get(ox as u32, oy as u32) else {
                continue;
            };
            if color.a == 0 || Some(color) == transparent_bg {
                continue;
            }
            let dx = x + ox;
            let dy = y + oy;
            if dx >= 0 && dy >= 0 {
                source.set(dx as u32, dy as u32, fill);
            }
        }
    }
}

/// Whether two colors differ by at most `tolerance` on every channel.
pub fn within_tolerance(a: crate::color::Color, b: crate::color::Color, tolerance: u8) -> bool {
    let t = i32::from(tolerance);
    (i32::from(a.r) - i32::from(b.r)).abs() <= t
        && (i32::from(a.g) - i32::from(b.g)).abs() <= t
        && (i32::from(a.b) - i32::from(b.b)).abs() <= t
        && (i32::from(a.a) - i32::from(b.a)).abs() <= t
}

/// Selects the region of matching pixels connected to `(x, y)` (the magic
/// wand). Returns the selected content (sized to its bounding box, with
/// unselected pixels transparent) and the bounding box origin.
///
/// `tolerance` (0–255) allows near-colors to match; 0 means exact match.
/// When `contiguous` is false, every matching pixel in the image is selected.
pub fn magic_wand(
    source: &PixelBuffer,
    x: i32,
    y: i32,
    contiguous: bool,
    tolerance: u8,
) -> Option<(PixelBuffer, (i32, i32), Vec<bool>)> {
    if x < 0 || y < 0 {
        return None;
    }
    let (width, height) = (source.width(), source.height());
    if x >= width as i32 || y >= height as i32 {
        return None;
    }
    let target = source.get(x as u32, y as u32)?;
    let mut matched = vec![false; (width * height) as usize];

    if contiguous {
        let mut stack = vec![(x, y)];
        while let Some((px, py)) = stack.pop() {
            if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                continue;
            }
            let index = (py as u32 * width + px as u32) as usize;
            let matches = source
                .get(px as u32, py as u32)
                .is_some_and(|c| within_tolerance(c, target, tolerance));
            if matched[index] || !matches {
                continue;
            }
            matched[index] = true;
            stack.push((px + 1, py));
            stack.push((px - 1, py));
            stack.push((px, py + 1));
            stack.push((px, py - 1));
        }
    } else {
        for py in 0..height {
            for px in 0..width {
                if source
                    .get(px, py)
                    .is_some_and(|c| within_tolerance(c, target, tolerance))
                {
                    matched[(py * width + px) as usize] = true;
                }
            }
        }
    }

    let mut min_x = width as i32;
    let mut min_y = height as i32;
    let mut max_x = -1;
    let mut max_y = -1;
    for py in 0..height as i32 {
        for px in 0..width as i32 {
            if matched[(py as u32 * width + px as u32) as usize] {
                min_x = min_x.min(px);
                min_y = min_y.min(py);
                max_x = max_x.max(px);
                max_y = max_y.max(py);
            }
        }
    }
    if max_x < min_x {
        return None;
    }

    let box_w = (max_x - min_x + 1) as u32;
    let box_h = (max_y - min_y + 1) as u32;
    let mut content = PixelBuffer::transparent(box_w, box_h);
    let mut box_mask = vec![false; (box_w * box_h) as usize];
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            if matched[(py as u32 * width + px as u32) as usize] {
                box_mask[((py - min_y) as u32 * box_w + (px - min_x) as u32) as usize] = true;
                if let Some(color) = source.get(px as u32, py as u32) {
                    content.set((px - min_x) as u32, (py - min_y) as u32, color);
                }
            }
        }
    }
    Some((content, (min_x, min_y), box_mask))
}

/// A boundary segment of a pixel mask, as the two grid-line endpoints.
pub type BoundarySegment = ((i32, i32), (i32, i32));

/// Extracts the outline of a boolean pixel mask as merged horizontal/vertical
/// grid-line segments. Coordinates are relative to the mask's top-left corner.
pub fn boundary_segments(mask: &[bool], width: u32, height: u32) -> Vec<BoundarySegment> {
    let width = width as i32;
    let height = height as i32;
    let selected = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && x < width && y < height && mask[(y * width + x) as usize]
    };

    let mut segments = Vec::new();

    // Horizontal edges (on grid line rows).
    for y in 0..=height {
        let mut x = 0;
        while x < width {
            if selected(x, y - 1) != selected(x, y) {
                let start = x;
                while x < width && selected(x, y - 1) != selected(x, y) {
                    x += 1;
                }
                segments.push(((start, y), (x, y)));
            } else {
                x += 1;
            }
        }
    }

    // Vertical edges (on grid line columns).
    for x in 0..=width {
        let mut y = 0;
        while y < height {
            if selected(x - 1, y) != selected(x, y) {
                let start = y;
                while y < height && selected(x - 1, y) != selected(x, y) {
                    y += 1;
                }
                segments.push(((x, start), (x, y)));
            } else {
                y += 1;
            }
        }
    }

    segments
}

/// The bounding box of a polygon as `(x, y, width, height)`.
pub fn lasso_bounds(points: &[(i32, i32)]) -> Option<(i32, i32, u32, u32)> {
    let first = *points.first()?;
    let (mut min_x, mut min_y) = first;
    let (mut max_x, mut max_y) = first;
    for &(x, y) in points {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    Some((
        min_x,
        min_y,
        (max_x - min_x + 1) as u32,
        (max_y - min_y + 1) as u32,
    ))
}

fn point_in_polygon(px: f32, py: f32, points: &[(i32, i32)]) -> bool {
    let mut inside = false;
    let count = points.len();
    if count < 3 {
        return false;
    }
    let mut j = count - 1;
    for i in 0..count {
        let (xi, yi) = (points[i].0 as f32, points[i].1 as f32);
        let (xj, yj) = (points[j].0 as f32, points[j].1 as f32);
        if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Extracts the pixels inside a lasso polygon, returning the content (sized to
/// the polygon's bounding box), the bounding box origin, and the pixel mask.
pub fn extract_lasso(
    source: &PixelBuffer,
    points: &[(i32, i32)],
) -> Option<(PixelBuffer, (i32, i32), Vec<bool>)> {
    let (min_x, min_y, width, height) = lasso_bounds(points)?;
    if width == 0 || height == 0 {
        return None;
    }
    let mut out = PixelBuffer::transparent(width, height);
    let mut mask = vec![false; (width * height) as usize];
    let mut any = false;
    for oy in 0..height as i32 {
        for ox in 0..width as i32 {
            let sx = min_x + ox;
            let sy = min_y + oy;
            if point_in_polygon(sx as f32 + 0.5, sy as f32 + 0.5, points) {
                mask[(oy as u32 * width + ox as u32) as usize] = true;
                if sx >= 0 && sy >= 0 {
                    if let Some(color) = source.get(sx as u32, sy as u32) {
                        out.set(ox as u32, oy as u32, color);
                        any = true;
                    }
                }
            }
        }
    }
    any.then_some((out, (min_x, min_y), mask))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_rect_copies_region() {
        let mut source = PixelBuffer::new(4, 4, Color::WHITE);
        source.set(1, 1, Color::BLACK);
        let region = extract_rect(&source, 1, 1, 2, 2);
        assert_eq!(region.size(), [2, 2]);
        assert_eq!(region.get(0, 0), Some(Color::BLACK));
        assert_eq!(region.get(1, 1), Some(Color::WHITE));
    }

    #[test]
    fn clear_rect_sets_fill() {
        let mut source = PixelBuffer::new(4, 4, Color::WHITE);
        clear_rect(&mut source, 1, 1, 2, 2, Color::TRANSPARENT);
        assert_eq!(source.get(1, 1), Some(Color::TRANSPARENT));
        assert_eq!(source.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn stamp_skips_transparent_and_background() {
        let mut target = PixelBuffer::new(4, 4, Color::WHITE);
        let mut pixels = PixelBuffer::transparent(2, 2);
        pixels.set(0, 0, Color::BLACK);
        pixels.set(1, 0, Color::rgb(255, 0, 0));
        stamp(&mut target, &pixels, 1, 1, Some(Color::rgb(255, 0, 0)));
        assert_eq!(target.get(1, 1), Some(Color::BLACK));
        // red skipped due to transparent background
        assert_eq!(target.get(2, 1), Some(Color::WHITE));
        // transparent pixel left untouched
        assert_eq!(target.get(1, 2), Some(Color::WHITE));
    }

    #[test]
    fn clear_masked_only_clears_stamped_pixels() {
        let mut target = PixelBuffer::new(3, 3, Color::BLACK);
        let mut pixels = PixelBuffer::transparent(2, 2);
        pixels.set(0, 0, Color::BLACK);
        // (1,0) and the bottom row stay transparent (outside the lasso)
        clear_masked(&mut target, &pixels, 0, 0, None, Color::WHITE);
        assert_eq!(target.get(0, 0), Some(Color::WHITE));
        assert_eq!(target.get(1, 0), Some(Color::BLACK));
        assert_eq!(target.get(0, 1), Some(Color::BLACK));
    }

    #[test]
    fn lasso_extracts_triangle() {
        let source = PixelBuffer::new(8, 8, Color::BLACK);
        let points = [(1, 1), (6, 1), (1, 6)];
        let (pixels, origin, _mask) = extract_lasso(&source, &points).unwrap();
        assert_eq!(origin, (1, 1));
        assert_eq!(pixels.size(), [6, 6]);
        // Inside the triangle
        assert_eq!(pixels.get(0, 0), Some(Color::BLACK));
        // Outside (bottom-right corner of the bbox)
        assert_eq!(pixels.get(5, 5), Some(Color::TRANSPARENT));
    }

    #[test]
    fn boundary_segments_of_solid_rect_is_the_perimeter() {
        let mask = vec![true; 3 * 2];
        let segments = boundary_segments(&mask, 3, 2);
        assert_eq!(segments.len(), 4);
        assert!(segments.contains(&((0, 0), (3, 0))), "top");
        assert!(segments.contains(&((0, 2), (3, 2))), "bottom");
        assert!(segments.contains(&((0, 0), (0, 2))), "left");
        assert!(segments.contains(&((3, 0), (3, 2))), "right");
    }

    #[test]
    fn magic_wand_selects_connected_region() {
        let mut source = PixelBuffer::new(5, 5, Color::WHITE);
        source.set(1, 1, Color::BLACK);

        let (content, origin, _mask) = magic_wand(&source, 1, 1, true, 0).unwrap();
        assert_eq!(origin, (1, 1));
        assert_eq!(content.size(), [1, 1]);

        let (background, origin, _mask) = magic_wand(&source, 0, 0, true, 0).unwrap();
        assert_eq!(origin, (0, 0));
        assert_eq!(background.size(), [5, 5]);
        assert_eq!(
            background.get(1, 1),
            Some(Color::TRANSPARENT),
            "black excluded"
        );
        assert_eq!(background.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn magic_wand_tolerance_matches_near_colors() {
        let mut source = PixelBuffer::new(3, 1, Color::rgb(100, 100, 100));
        source.set(1, 0, Color::rgb(110, 100, 100));

        // Tolerance 0: only the exact-color seed pixel matches.
        let (exact, origin, _mask) = magic_wand(&source, 0, 0, true, 0).unwrap();
        assert_eq!(origin, (0, 0));
        assert_eq!(exact.size(), [1, 1]);

        // Tolerance 20: the near-color middle pixel connects to the rest.
        let (loose, origin, _mask) = magic_wand(&source, 0, 0, true, 20).unwrap();
        assert_eq!(origin, (0, 0));
        assert_eq!(loose.size(), [3, 1]);
    }

    #[test]
    fn magic_wand_reports_disconnected_islands_separately() {
        let mut source = PixelBuffer::new(6, 3, Color::WHITE);
        source.set(1, 1, Color::BLACK);
        source.set(4, 1, Color::BLACK);
        let (left, origin, _mask) = magic_wand(&source, 1, 1, true, 0).unwrap();
        assert_eq!(origin, (1, 1));
        assert_eq!(left.size(), [1, 1]);
    }

    #[test]
    fn lasso_bounds_matches_points() {
        assert_eq!(lasso_bounds(&[(3, 5), (1, 2), (7, 9)]), Some((1, 2, 7, 8)));
        assert_eq!(lasso_bounds(&[]), None);
    }
}
