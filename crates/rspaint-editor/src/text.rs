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
    pub underline: bool,
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
            "sans-serif",
            &["segoeui.ttf", "arial.ttf", "calibri.ttf", "tahoma.ttf"],
            epaint_default_fonts::UBUNTU_LIGHT,
        );
        let serif = pick_font(
            "serif",
            &["times.ttf", "georgia.ttf", "constan.ttf", "cambria.ttc"],
            epaint_default_fonts::UBUNTU_LIGHT,
        );
        let mono = pick_font(
            "monospace",
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
fn pick_font(family: &str, names: &[&str], fallback: &[u8]) -> FontVec {
    #[cfg(target_os = "windows")]
    {
        if let Some(font) = load_windows_font(names) {
            return font;
        }
        let _ = family;
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(font) = load_linux_font(family) {
            return font;
        }
        let _ = names;
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = (family, names);
    }
    FontVec::try_from_vec(fallback.to_vec()).expect("bundled font is valid")
}

/// Reads a font file, returning `None` if it cannot be read or parsed.
fn read_font(path: &std::path::Path) -> Option<FontVec> {
    FontVec::try_from_vec(std::fs::read(path).ok()?).ok()
}

/// Loads a system font from the Windows fonts directory.
#[cfg(target_os = "windows")]
fn load_windows_font(names: &[&str]) -> Option<FontVec> {
    let fonts = std::path::Path::new(&std::env::var("WINDIR").ok()?).join("Fonts");
    names.iter().find_map(|name| read_font(&fonts.join(name)))
}

/// Loads a system font on Linux, using fontconfig when available and common
/// font locations as a fallback (so minimal installs still get a real serif or
/// monospace instead of a sans-serif bundled font).
#[cfg(target_os = "linux")]
fn load_linux_font(family: &str) -> Option<FontVec> {
    if let Some(font) = fontconfig_match(family).and_then(|path| read_font(&path)) {
        return Some(font);
    }
    linux_font_candidates(family)
        .into_iter()
        .find_map(|path| read_font(&path))
}

/// Asks fontconfig for the file backing a generic family.
#[cfg(target_os = "linux")]
fn fontconfig_match(family: &str) -> Option<std::path::PathBuf> {
    let output = std::process::Command::new("fc-match")
        .args(["--format", "%{file}", family])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = std::path::PathBuf::from(String::from_utf8(output.stdout).ok()?.trim());
    path.is_file().then_some(path)
}

/// Well-known font files for a generic family, searched under common Linux
/// font directories.
#[cfg(target_os = "linux")]
fn linux_font_candidates(family: &str) -> Vec<std::path::PathBuf> {
    const SANS: &[&str] = &[
        "DejaVuSans.ttf",
        "LiberationSans-Regular.ttf",
        "NotoSans-Regular.ttf",
        "FreeSans.ttf",
    ];
    const SERIF: &[&str] = &[
        "DejaVuSerif.ttf",
        "LiberationSerif-Regular.ttf",
        "NotoSerif-Regular.ttf",
        "FreeSerif.ttf",
    ];
    const MONO: &[&str] = &[
        "DejaVuSansMono.ttf",
        "LiberationMono-Regular.ttf",
        "NotoSansMono-Regular.ttf",
        "FreeMono.ttf",
    ];
    let names = match family {
        "serif" => SERIF,
        "monospace" => MONO,
        _ => SANS,
    };

    let mut roots = vec![
        std::path::PathBuf::from("/usr/share/fonts"),
        std::path::PathBuf::from("/usr/local/share/fonts"),
        std::path::PathBuf::from("/usr/share/fonts/truetype"),
        std::path::PathBuf::from("/usr/share/fonts/TTF"),
        std::path::PathBuf::from("/usr/share/fonts/opentype"),
    ];
    if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
        roots.push(std::path::PathBuf::from(data).join("fonts"));
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = std::path::PathBuf::from(home);
        roots.push(home.join(".local/share/fonts"));
        roots.push(home.join(".fonts"));
    }

    let mut found = Vec::new();
    for root in roots {
        find_font_files(&root, names, 4, &mut found);
    }
    found
}

/// Appends matching font files found under `dir` (recursively, up to `depth`).
#[cfg(target_os = "linux")]
fn find_font_files(
    dir: &std::path::Path,
    names: &[&str],
    depth: usize,
    found: &mut Vec<std::path::PathBuf>,
) {
    if depth == 0 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            subdirs.push(path);
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| names.iter().any(|wanted| name.eq_ignore_ascii_case(wanted)))
        {
            found.push(path);
        }
    }
    for subdir in subdirs {
        find_font_files(&subdir, names, depth - 1, found);
    }
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
        if style.underline {
            draw_underline(
                buffer,
                x as f32,
                pen_y,
                measure(line),
                style,
                scaled.descent(),
                color,
            );
        }
    }
    (lines.len() as f32 * line_height).ceil() as i32
}

/// Draws a horizontal underline beneath a line of text.
fn draw_underline(
    buffer: &mut PixelBuffer,
    start_x: f32,
    baseline: f32,
    width: f32,
    style: TextStyle,
    descent: f32,
    color: Color,
) {
    // Sit the line a little below the baseline, thicker for larger sizes.
    let y = baseline - descent * 0.5;
    let thickness = (style.size / 14.0).round().max(1.0) as i32;
    let end = start_x + width;
    let y0 = y.round() as i32;
    for dy in 0..thickness {
        let py = y0 + dy;
        let shear = if style.italic {
            ((y - baseline) * -0.25).round() as i32
        } else {
            0
        };
        let mut px = start_x.round() as i32;
        while (px as f32) < end {
            blend(buffer, px + shear, py, color, 1.0);
            px += 1;
        }
    }
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
            underline: false,
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
    fn underline_adds_pixels() {
        let fonts = FontSet::load();
        let make = |underline: bool| TextStyle {
            family: FontFamily::Sans,
            size: 24.0,
            bold: false,
            italic: false,
            underline,
        };
        let count = |buffer: &PixelBuffer| {
            (0..buffer.height())
                .flat_map(|y| (0..buffer.width()).map(move |x| (x, y)))
                .filter(|&(x, y)| buffer.get(x, y).is_some_and(|c| c.a > 0))
                .count()
        };
        let mut plain = PixelBuffer::transparent(80, 40);
        let mut underlined = PixelBuffer::transparent(80, 40);
        draw_text(
            &fonts,
            &mut plain,
            (2, 2),
            76,
            "Hi",
            make(false),
            Color::BLACK,
        );
        draw_text(
            &fonts,
            &mut underlined,
            (2, 2),
            76,
            "Hi",
            make(true),
            Color::BLACK,
        );
        assert!(count(&underlined) > count(&plain));
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
