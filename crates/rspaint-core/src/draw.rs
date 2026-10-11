//! Raster drawing primitives.
//!
//! All shapes are defined by bounding box corners in image coordinates and are
//! drawn aliased (no antialiasing), matching MS Paint's default behavior.

use crate::brush::Brush;
use crate::buffer::PixelBuffer;
use crate::color::{BlendMode, Color};

/// A point in image coordinates.
pub type Point = (i32, i32);

fn normalize(a: Point, b: Point) -> (i32, i32, i32, i32) {
    let (x0, x1) = if a.0 <= b.0 { (a.0, b.0) } else { (b.0, a.0) };
    let (y0, y1) = if a.1 <= b.1 { (a.1, b.1) } else { (b.1, a.1) };
    (x0, y0, x1, y1)
}

/// Calls `f` for each integer point on the Bresenham line from `a` to `b`.
pub fn walk_line(a: Point, b: Point, mut f: impl FnMut(i32, i32)) {
    let (mut x, mut y) = a;
    let (x1, y1) = b;
    let dx = (x1 - x).abs();
    let dy = -(y1 - y).abs();
    let sx = if x < x1 { 1 } else { -1 };
    let sy = if y < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        f(x, y);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Draws a line from `a` to `b` using Bresenham's algorithm, stamping `brush`.
/// Like [`walk_line`], but also emits the intermediate pixel of each diagonal
/// step so brush strokes stay continuous (JS Paint's `bresenham_dense_line`).
pub fn walk_dense_line(a: Point, b: Point, mut f: impl FnMut(i32, i32)) {
    let (mut x, mut y) = a;
    let (x1, y1) = b;
    let dx = (x1 - x).abs();
    let dy = -(y1 - y).abs();
    let sx = if x < x1 { 1 } else { -1 };
    let sy = if y < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        f(x, y);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        f(x, y);
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Draws a line from `a` to `b` with Bresenham's algorithm, stamping `brush`.
///
/// Multi-pixel brushes use the dense variant to avoid gaps.
/// Draws a line stamping `brush`, painting each pixel at most once per
/// `painted` mask (for incremental freehand strokes).
pub fn draw_line_masked(
    buffer: &mut PixelBuffer,
    a: Point,
    b: Point,
    brush: &Brush,
    color: Color,
    painted: &mut [bool],
    width: u32,
) {
    let mut stamp = |x, y| brush.stamp_masked(buffer, painted, width, x, y, color);
    if brush.size > 1 {
        walk_dense_line(a, b, &mut stamp);
    } else {
        walk_line(a, b, &mut stamp);
    }
}

pub fn draw_line(buffer: &mut PixelBuffer, a: Point, b: Point, brush: &Brush, color: Color) {
    if brush.size > 1 {
        walk_dense_line(a, b, |x, y| brush.stamp(buffer, x, y, color));
    } else {
        walk_line(a, b, |x, y| brush.stamp(buffer, x, y, color));
    }
}

/// Replaces `from` with `to` along a line (the color eraser).
pub fn erase_line(
    buffer: &mut PixelBuffer,
    a: Point,
    b: Point,
    brush: &Brush,
    from: Color,
    to: Color,
) {
    if brush.size > 1 {
        walk_dense_line(a, b, |x, y| brush.replace_in_stamp(buffer, x, y, from, to));
    } else {
        walk_line(a, b, |x, y| brush.replace_in_stamp(buffer, x, y, from, to));
    }
}

fn draw_outline(
    buffer: &mut PixelBuffer,
    corners: (Point, Point),
    contains: impl Fn(i32, i32) -> bool,
    brush: &Brush,
    color: Color,
) {
    let (x0, y0, x1, y1) = normalize(corners.0, corners.1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if !contains(x, y) {
                continue;
            }
            let edge = !contains(x - 1, y)
                || !contains(x + 1, y)
                || !contains(x, y - 1)
                || !contains(x, y + 1);
            if edge {
                brush.stamp(buffer, x, y, color);
            }
        }
    }
}

fn fill_region(
    buffer: &mut PixelBuffer,
    corners: (Point, Point),
    contains: impl Fn(i32, i32) -> bool,
    color: Color,
    blend: BlendMode,
) {
    let (x0, y0, x1, y1) = normalize(corners.0, corners.1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            if contains(x, y) && x >= 0 && y >= 0 {
                buffer.paint(x as u32, y as u32, color, blend);
            }
        }
    }
}

fn rect_contains(x0: i32, y0: i32, x1: i32, y1: i32) -> impl Fn(i32, i32) -> bool {
    move |x, y| x >= x0 && x <= x1 && y >= y0 && y <= y1
}

fn ellipse_contains(x0: i32, y0: i32, x1: i32, y1: i32) -> impl Fn(i32, i32) -> bool {
    let cx = (x0 + x1) as f64 / 2.0;
    let cy = (y0 + y1) as f64 / 2.0;
    let rx = (x1 - x0) as f64 / 2.0;
    let ry = (y1 - y0) as f64 / 2.0;
    move |x, y| {
        if rx <= 0.0 || ry <= 0.0 {
            return x == cx as i32 && y == cy as i32;
        }
        let nx = (x as f64 - cx) / (rx + 0.5);
        let ny = (y as f64 - cy) / (ry + 0.5);
        nx * nx + ny * ny <= 1.0
    }
}

fn rounded_rect_contains(
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    radius: i32,
) -> impl Fn(i32, i32) -> bool {
    let max_radius = ((x1 - x0).min(y1 - y0)) / 2;
    let r = radius.clamp(0, max_radius.max(0));
    move |x, y| {
        if x < x0 || x > x1 || y < y0 || y > y1 {
            return false;
        }
        if r <= 0 {
            return true;
        }
        let nx = x.clamp(x0 + r, x1 - r);
        let ny = y.clamp(y0 + r, y1 - r);
        let dx = x - nx;
        let dy = y - ny;
        dx * dx + dy * dy <= r * r
    }
}

/// Draws a rectangle outline.
pub fn draw_rectangle(buffer: &mut PixelBuffer, a: Point, b: Point, brush: &Brush, color: Color) {
    let (x0, y0, x1, y1) = normalize(a, b);
    draw_outline(buffer, (a, b), rect_contains(x0, y0, x1, y1), brush, color);
}

/// Draws a filled rectangle.
pub fn fill_rectangle(
    buffer: &mut PixelBuffer,
    a: Point,
    b: Point,
    color: Color,
    blend: BlendMode,
) {
    let (x0, y0, x1, y1) = normalize(a, b);
    fill_region(buffer, (a, b), rect_contains(x0, y0, x1, y1), color, blend);
}

/// Draws an ellipse outline within the given bounding box.
pub fn draw_ellipse(buffer: &mut PixelBuffer, a: Point, b: Point, brush: &Brush, color: Color) {
    let (x0, y0, x1, y1) = normalize(a, b);
    draw_outline(
        buffer,
        (a, b),
        ellipse_contains(x0, y0, x1, y1),
        brush,
        color,
    );
}

/// Draws a filled ellipse within the given bounding box.
pub fn fill_ellipse(buffer: &mut PixelBuffer, a: Point, b: Point, color: Color, blend: BlendMode) {
    let (x0, y0, x1, y1) = normalize(a, b);
    fill_region(
        buffer,
        (a, b),
        ellipse_contains(x0, y0, x1, y1),
        color,
        blend,
    );
}

/// Draws a rounded-rectangle outline.
pub fn draw_rounded_rectangle(
    buffer: &mut PixelBuffer,
    a: Point,
    b: Point,
    radius: i32,
    brush: &Brush,
    color: Color,
) {
    let (x0, y0, x1, y1) = normalize(a, b);
    draw_outline(
        buffer,
        (a, b),
        rounded_rect_contains(x0, y0, x1, y1, radius),
        brush,
        color,
    );
}

/// Draws a filled rounded rectangle.
pub fn fill_rounded_rectangle(
    buffer: &mut PixelBuffer,
    a: Point,
    b: Point,
    radius: i32,
    color: Color,
    blend: BlendMode,
) {
    let (x0, y0, x1, y1) = normalize(a, b);
    fill_region(
        buffer,
        (a, b),
        rounded_rect_contains(x0, y0, x1, y1, radius),
        color,
        blend,
    );
}

/// Draws a polyline through `points`, optionally closing it back to the start.
pub fn draw_polyline(
    buffer: &mut PixelBuffer,
    points: &[(i32, i32)],
    brush: &Brush,
    color: Color,
    close: bool,
) {
    match points.len() {
        0 => {}
        1 => brush.stamp(buffer, points[0].0, points[0].1, color),
        _ => {
            for pair in points.windows(2) {
                draw_line(buffer, pair[0], pair[1], brush, color);
            }
            if close {
                draw_line(buffer, points[points.len() - 1], points[0], brush, color);
            }
        }
    }
}

/// Fills a polygon using the even-odd rule at pixel centers.
pub fn fill_polygon(
    buffer: &mut PixelBuffer,
    points: &[(i32, i32)],
    color: Color,
    blend: BlendMode,
) {
    if points.len() < 3 {
        return;
    }
    let min_y = points.iter().map(|p| p.1).min().unwrap_or(0);
    let max_y = points.iter().map(|p| p.1).max().unwrap_or(0);
    let count = points.len();

    for y in min_y..=max_y {
        let scan_y = y as f64 + 0.5;
        let mut crossings: Vec<f64> = Vec::new();
        let mut j = count - 1;
        for i in 0..count {
            let (xi, yi) = (points[i].0 as f64, points[i].1 as f64);
            let (xj, yj) = (points[j].0 as f64, points[j].1 as f64);
            if (yi > scan_y) != (yj > scan_y) {
                crossings.push((xj - xi) * (scan_y - yi) / (yj - yi) + xi);
            }
            j = i;
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut k = 0;
        while k + 1 < crossings.len() {
            let start = (crossings[k] - 0.5).ceil() as i32;
            let end = (crossings[k + 1] - 0.5).floor() as i32;
            for x in start..=end {
                if x >= 0 && y >= 0 {
                    buffer.paint(x as u32, y as u32, color, blend);
                }
            }
            k += 2;
        }
    }
}

fn bezier_steps(points: &[Point]) -> i32 {
    let mut length = 0.0;
    for pair in points.windows(2) {
        let dx = (pair[1].0 - pair[0].0) as f64;
        let dy = (pair[1].1 - pair[0].1) as f64;
        length += (dx * dx + dy * dy).sqrt();
    }
    (length.ceil() as i32).clamp(8, 2048)
}

fn quad_point(t: f64, p0: Point, control: Point, p1: Point) -> Point {
    let mt = 1.0 - t;
    let x = mt * mt * p0.0 as f64 + 2.0 * mt * t * control.0 as f64 + t * t * p1.0 as f64;
    let y = mt * mt * p0.1 as f64 + 2.0 * mt * t * control.1 as f64 + t * t * p1.1 as f64;
    (x.round() as i32, y.round() as i32)
}

fn cubic_point(t: f64, p0: Point, c1: Point, c2: Point, p1: Point) -> Point {
    let mt = 1.0 - t;
    let x = mt * mt * mt * p0.0 as f64
        + 3.0 * mt * mt * t * c1.0 as f64
        + 3.0 * mt * t * t * c2.0 as f64
        + t * t * t * p1.0 as f64;
    let y = mt * mt * mt * p0.1 as f64
        + 3.0 * mt * mt * t * c1.1 as f64
        + 3.0 * mt * t * t * c2.1 as f64
        + t * t * t * p1.1 as f64;
    (x.round() as i32, y.round() as i32)
}

/// Draws a quadratic Bézier curve from `start` to `end` with one control point.
pub fn draw_quadratic_curve(
    buffer: &mut PixelBuffer,
    start: Point,
    control: Point,
    end: Point,
    brush: &Brush,
    color: Color,
) {
    let steps = bezier_steps(&[start, control, end]);
    let mut previous = start;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let point = quad_point(t, start, control, end);
        draw_line(buffer, previous, point, brush, color);
        previous = point;
    }
}

/// Draws a cubic Bézier curve from `start` to `end` with two control points.
pub fn draw_bezier(
    buffer: &mut PixelBuffer,
    start: Point,
    control_1: Point,
    control_2: Point,
    end: Point,
    brush: &Brush,
    color: Color,
) {
    let steps = bezier_steps(&[start, control_1, control_2, end]);
    let mut previous = start;
    for i in 1..=steps {
        let t = i as f64 / steps as f64;
        let point = cubic_point(t, start, control_1, control_2, end);
        draw_line(buffer, previous, point, brush, color);
        previous = point;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count_non_transparent(buffer: &PixelBuffer) -> usize {
        buffer.data().chunks_exact(4).filter(|p| p[3] != 0).count()
    }

    #[test]
    fn bezier_covers_endpoints() {
        let mut buffer = PixelBuffer::transparent(32, 32);
        draw_bezier(
            &mut buffer,
            (2, 2),
            (2, 20),
            (28, 20),
            (28, 2),
            &Brush::PENCIL,
            Color::BLACK,
        );
        assert_eq!(buffer.get(2, 2), Some(Color::BLACK), "start");
        assert_eq!(buffer.get(28, 2), Some(Color::BLACK), "end");
        // The curve dips down in the middle (not a straight line).
        assert_eq!(buffer.get(15, 2), Some(Color::TRANSPARENT), "not straight");
    }

    #[test]
    fn walk_dense_line_emits_intermediate_points() {
        let mut sparse = 0;
        walk_line((0, 0), (4, 4), |_, _| sparse += 1);
        let mut dense = 0;
        walk_dense_line((0, 0), (4, 4), |_, _| dense += 1);
        assert_eq!(sparse, 5);
        assert!(dense > sparse, "dense={dense} sparse={sparse}");
    }

    #[test]
    fn horizontal_line_covers_endpoints() {
        let mut buffer = PixelBuffer::transparent(10, 10);
        draw_line(&mut buffer, (1, 2), (8, 2), &Brush::PENCIL, Color::BLACK);
        for x in 1..=8 {
            assert_eq!(buffer.get(x, 2), Some(Color::BLACK), "x={x}");
        }
        assert_eq!(count_non_transparent(&buffer), 8);
    }

    #[test]
    fn fill_polygon_fills_interior_only() {
        let mut buffer = PixelBuffer::new(8, 8, Color::WHITE);
        let triangle = [(1, 1), (6, 1), (1, 6)];
        fill_polygon(&mut buffer, &triangle, Color::BLACK, BlendMode::Blend);
        assert_eq!(buffer.get(2, 2), Some(Color::BLACK), "inside");
        assert_eq!(buffer.get(5, 5), Some(Color::WHITE), "outside");
    }

    #[test]
    fn draw_polyline_closes_back_to_start() {
        let mut buffer = PixelBuffer::transparent(8, 8);
        let points = [(0, 0), (4, 0), (4, 4)];
        draw_polyline(&mut buffer, &points, &Brush::PENCIL, Color::BLACK, true);
        assert_eq!(buffer.get(0, 0), Some(Color::BLACK));
        assert_eq!(buffer.get(4, 0), Some(Color::BLACK));
        assert_eq!(buffer.get(4, 4), Some(Color::BLACK));
        assert_eq!(buffer.get(2, 2), Some(Color::BLACK), "closing edge");
    }

    #[test]
    fn erase_line_replaces_only_matching_color() {
        let mut buffer = PixelBuffer::new(8, 2, Color::WHITE);
        buffer.set(2, 0, Color::BLACK);
        buffer.set(3, 0, Color::BLACK);
        let brush = Brush::new(crate::brush::BrushShape::Square, 1);
        erase_line(
            &mut buffer,
            (0, 0),
            (7, 0),
            &brush,
            Color::BLACK,
            Color::rgb(255, 0, 0),
        );
        assert_eq!(buffer.get(2, 0), Some(Color::rgb(255, 0, 0)));
        assert_eq!(buffer.get(3, 0), Some(Color::rgb(255, 0, 0)));
        assert_eq!(buffer.get(0, 0), Some(Color::WHITE));
    }

    #[test]
    fn rectangle_outline_is_hollow() {
        let mut buffer = PixelBuffer::transparent(10, 10);
        draw_rectangle(&mut buffer, (1, 1), (5, 4), &Brush::PENCIL, Color::BLACK);
        assert_eq!(buffer.get(1, 1), Some(Color::BLACK));
        assert_eq!(buffer.get(5, 4), Some(Color::BLACK));
        assert_eq!(buffer.get(3, 3), Some(Color::TRANSPARENT));
    }

    #[test]
    fn filled_rectangle_is_solid() {
        let mut buffer = PixelBuffer::transparent(10, 10);
        fill_rectangle(&mut buffer, (2, 2), (4, 4), Color::BLACK, BlendMode::Blend);
        assert_eq!(count_non_transparent(&buffer), 9);
    }

    #[test]
    fn ellipse_is_symmetric_and_hollow() {
        let mut buffer = PixelBuffer::transparent(20, 20);
        draw_ellipse(&mut buffer, (2, 4), (16, 14), &Brush::PENCIL, Color::BLACK);
        assert_eq!(buffer.get(9, 4), Some(Color::BLACK), "top of ellipse");
        assert_eq!(buffer.get(9, 14), Some(Color::BLACK), "bottom of ellipse");
        assert_eq!(
            buffer.get(9, 9),
            Some(Color::TRANSPARENT),
            "interior hollow"
        );
    }

    #[test]
    fn filled_ellipse_is_solid_inside() {
        let mut buffer = PixelBuffer::transparent(20, 20);
        fill_ellipse(
            &mut buffer,
            (2, 4),
            (16, 14),
            Color::BLACK,
            BlendMode::Blend,
        );
        assert_eq!(buffer.get(9, 9), Some(Color::BLACK));
        assert_eq!(buffer.get(0, 0), Some(Color::TRANSPARENT));
    }

    #[test]
    fn rounded_rectangle_clips_corners() {
        let mut buffer = PixelBuffer::transparent(20, 20);
        fill_rounded_rectangle(
            &mut buffer,
            (0, 0),
            (19, 19),
            4,
            Color::BLACK,
            BlendMode::Blend,
        );
        assert_eq!(buffer.get(0, 0), Some(Color::TRANSPARENT), "corner clipped");
        assert_eq!(buffer.get(10, 10), Some(Color::BLACK), "center filled");
    }
}
