//! Text rasterization for the Text tool.
//!
//! Uses `ab_glyph` to rasterize glyphs into a [`PixelBuffer`]. Fonts are loaded
//! from the operating system when available, falling back to the fonts bundled
//! with `epaint` so the tool always works.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};
use rspaint_core::buffer::PixelBuffer;
use rspaint_core::color::Color;

/// The available font families for the Text tool.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontFamily {
    Sans,
    Serif,
    Mono,
}

impl FontFamily {
    /// Every family, in menu order.
    pub const ALL: [FontFamily; 3] = [FontFamily::Sans, FontFamily::Serif, FontFamily::Mono];

    /// A human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            FontFamily::Sans => "Sans Serif",
            FontFamily::Serif => "Serif",
            FontFamily::Mono => "Monospace",
        }
    }
}

/// The styling of a text box.
#[derive(Clone, Copy)]
pub struct TextStyle {
    pub family: FontFamily,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
}

/// The loaded fonts, one per family.
pub struct FontSet {
    sans: FontVec,
    serif: FontVec,
    mono: FontVec,
}

impl FontSet {
    /// Loads the fonts, preferring system fonts and falling back to bundled ones.
    pub fn load() -> Self {
        let sans = pick_font(
            &["segoeui.ttf", "arial.ttf", "calibri.ttf", "tahoma.ttf"],
            epaint_default_fonts::UBUNTU_LIGHT,
        );
        let serif = pick_font(
            &["times.ttf", "georgia.ttf", "constan.ttf", "cambria.ttc"],
            epaint_default_fonts::UBUNTU_LIGHT,
        );
        let mono = pick_font(
            &["consola.ttf", "cour.ttf", "lucon.ttf"],
            epaint_default_fonts::HACK_REGULAR,
        );
        Self { sans, serif, mono }
    }

    fn font(&self, family: FontFamily) -> &FontVec {
        match family {
            FontFamily::Sans => &self.sans,
            FontFamily::Serif => &self.serif,
            FontFamily::Mono => &self.mono,
        }
    }
}

/// Loads the first readable system font, else the bundled fallback.
fn pick_font(names: &[&str], fallback: &[u8]) -> FontVec {
    if let Ok(windir) = std::env::var("WINDIR") {
        for name in names {
            let path = format!("{windir}\\Fonts\\{name}");
            if let Ok(bytes) = std::fs::read(&path) {
                if let Ok(font) = FontVec::try_from_vec(bytes) {
                    return font;
                }
            }
        }
    }
    FontVec::try_from_vec(fallback.to_vec()).expect("bundled font is valid")
}

/// The height of a single line of text, in pixels.
pub fn line_height(fonts: &FontSet, style: TextStyle) -> i32 {
    let scaled = fonts
        .font(style.family)
        .as_scaled(PxScale::from(style.size));
    (scaled.height() + scaled.line_gap()).ceil().max(1.0) as i32
}

/// Draws `text` into `buffer` with the box's top-left at `origin`.
///
/// `wrap_width` (in pixels) wraps words; use `0` for no wrapping. The text is
/// not clipped; callers render into a box-sized buffer for clipping.
/// Returns the height used.
pub fn draw_text(
    fonts: &FontSet,
    buffer: &mut PixelBuffer,
    origin: (i32, i32),
    wrap_width: i32,
    text: &str,
    style: TextStyle,
    color: Color,
) -> i32 {
    let (x, y) = origin;
    let font = fonts.font(style.family);
    let scale = PxScale::from(style.size);
    let scaled = font.as_scaled(scale);
    let line_height = (scaled.height() + scaled.line_gap()).ceil().max(1.0);
    let baseline = scaled.ascent();

    let measure = |s: &str| -> f32 {
        let mut width = 0.0;
        let mut prev = None;
        for c in s.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                width += scaled.kern(p, id);
            }
            width += scaled.h_advance(id);
            prev = Some(id);
        }
        width
    };

    let lines = wrap_text(text, wrap_width, &measure);

    for (i, line) in lines.iter().enumerate() {
        let pen_y = y as f32 + baseline + i as f32 * line_height;
        let mut pen_x = x as f32;
        let mut prev = None;
        for c in line.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                pen_x += scaled.kern(p, id);
            }
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(pen_x, pen_y));
            if let Some(outlined) = scaled.outline_glyph(glyph) {
                let bounds = outlined.px_bounds();
                outlined.draw(|gx, gy, coverage| {
                    let px = bounds.min.x as i32 + gx as i32;
                    let py = bounds.min.y as i32 + gy as i32;
                    let shear = if style.italic {
                        (((py as f32) - pen_y) * -0.25).round() as i32
                    } else {
                        0
                    };
                    blend(buffer, px + shear, py, color, coverage);
                    if style.bold {
                        blend(buffer, px + shear + 1, py, color, coverage);
                    }
                });
            }
            pen_x += scaled.h_advance(id);
            prev = Some(id);
        }
    }
    (lines.len() as f32 * line_height).ceil() as i32
}

/// The caret line at the end of `text`, in text space: `(x, top, height)`.
///
/// `x` is the horizontal position after the last character, `top` the top of
/// the last line and `height` the line height, all relative to the text origin.
pub fn caret(fonts: &FontSet, style: TextStyle, wrap_width: i32, text: &str) -> (i32, i32, i32) {
    let font = fonts.font(style.family);
    let scaled = font.as_scaled(PxScale::from(style.size));
    let line_height = (scaled.height() + scaled.line_gap()).ceil().max(1.0) as i32;
    let measure = |s: &str| -> f32 {
        let mut width = 0.0;
        let mut prev = None;
        for c in s.chars() {
            let id = scaled.glyph_id(c);
            if let Some(p) = prev {
                width += scaled.kern(p, id);
            }
            width += scaled.h_advance(id);
            prev = Some(id);
        }
        width
    };
    let lines = wrap_text(text, wrap_width, &measure);
    let last = lines.last().map(String::as_str).unwrap_or("");
    let x = measure(last).ceil() as i32;
    let top = lines.len().saturating_sub(1) as i32 * line_height;
    (x, top, line_height)
}

/// Splits `text` into lines, wrapping words to `wrap_width` when positive.
fn wrap_text(text: &str, wrap_width: i32, measure: &impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        if wrap_width <= 0 {
            lines.push(paragraph.to_string());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_inclusive(' ') {
            let candidate = format!("{current}{word}");
            if !current.is_empty() && measure(candidate.trim_end()) > wrap_width as f32 {
                lines.push(current.trim_end().to_string());
                current = word.to_string();
            } else {
                current = candidate;
            }
        }
        lines.push(current.trim_end().to_string());
    }
    lines
}

/// Alpha-blends `color` into `buffer` at `(x, y)` with the given `coverage`.
fn blend(buffer: &mut PixelBuffer, x: i32, y: i32, color: Color, coverage: f32) {
    if coverage <= 0.0 || x < 0 || y < 0 {
        return;
    }
    let alpha = (color.a as f32 / 255.0) * coverage.min(1.0);
    if alpha <= 0.0 {
        return;
    }
    let src = Color::rgba(color.r, color.g, color.b, (alpha * 255.0).round() as u8);
    if let Some(dst) = buffer.get(x as u32, y as u32) {
        buffer.set(x as u32, y as u32, src.over(dst));
    }
}

/// Composites `src` over `dst` at the given offset, skipping transparent pixels.
pub fn composite(dst: &mut PixelBuffer, src: &PixelBuffer, ox: i32, oy: i32) {
    for y in 0..src.height() {
        for x in 0..src.width() {
            let color = src.get(x, y).unwrap_or(Color::TRANSPARENT);
            if color.a == 0 {
                continue;
            }
            let dx = ox + x as i32;
            let dy = oy + y as i32;
            if dx < 0 || dy < 0 {
                continue;
            }
            if let Some(base) = dst.get(dx as u32, dy as u32) {
                dst.set(dx as u32, dy as u32, color.over(base));
            }
        }
    }
}

/// Rotates a buffer 90 degrees clockwise.
pub fn rotate_90_cw(src: &PixelBuffer) -> PixelBuffer {
    let (w, h) = (src.width(), src.height());
    let mut dst = PixelBuffer::transparent(h, w);
    for y in 0..h {
        for x in 0..w {
            let color = src.get(x, y).unwrap_or(Color::TRANSPARENT);
            dst.set(h - 1 - y, x, color);
        }
    }
    dst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draw_text_paints_inside_the_box() {
        let fonts = FontSet::load();
        let style = TextStyle {
            family: FontFamily::Sans,
            size: 24.0,
            bold: false,
            italic: false,
        };
        let mut buffer = PixelBuffer::transparent(80, 40);
        let used = draw_text(&fonts, &mut buffer, (2, 2), 76, "Hi", style, Color::BLACK);
        assert!(used > 0, "some height is used");
        let painted = (0..buffer.height())
            .flat_map(|y| (0..buffer.width()).map(move |x| (x, y)))
            .filter(|&(x, y)| buffer.get(x, y).is_some_and(|c| c.a > 0))
            .count();
        assert!(painted > 10, "text painted {painted} pixels");
    }

    #[test]
    fn rotate_90_cw_swaps_dimensions() {
        let mut src = PixelBuffer::transparent(3, 2);
        src.set(0, 0, Color::BLACK);
        let rotated = rotate_90_cw(&src);
        assert_eq!((rotated.width(), rotated.height()), (2, 3));
        // Top-left of the source moves to the top-right after a CW rotation.
        assert_eq!(rotated.get(1, 0), Some(Color::BLACK));
    }
}
