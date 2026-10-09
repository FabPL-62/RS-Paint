//! Color primitives and compositing.

/// An 8-bit RGBA color, stored non-premultiplied.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Fully transparent black, the default canvas clear color.
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    /// Opaque black.
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    /// Opaque white, the default canvas background.
    pub const WHITE: Self = Self::rgb(255, 255, 255);

    /// Creates a color from its four 8-bit channels.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Creates an opaque color from its three 8-bit channels.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// Returns the channels as `[r, g, b, a]`.
    pub const fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    /// Whether the color is fully transparent.
    pub const fn is_transparent(self) -> bool {
        self.a == 0
    }

    /// Whether the color is fully opaque.
    pub const fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// Parses `#RGB`, `#RRGGBB` or `#RRGGBBAA` (the leading `#` is optional).
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        let parse = |s: &str| u8::from_str_radix(s, 16).ok();
        match hex.len() {
            3 => {
                let b = hex.as_bytes();
                let dup = |c: u8| -> Option<u8> {
                    let n = (c as char).to_digit(16)? as u8;
                    Some(n * 17)
                };
                Some(Self::rgb(dup(b[0])?, dup(b[1])?, dup(b[2])?))
            }
            6 => Some(Self::rgb(
                parse(&hex[0..2])?,
                parse(&hex[2..4])?,
                parse(&hex[4..6])?,
            )),
            8 => Some(Self::rgba(
                parse(&hex[0..2])?,
                parse(&hex[2..4])?,
                parse(&hex[4..6])?,
                parse(&hex[6..8])?,
            )),
            _ => None,
        }
    }

    /// Composites `self` (the source) over `dst` using source-over alpha blending.
    pub fn over(self, dst: Self) -> Self {
        if self.is_opaque() {
            return self;
        }
        if self.is_transparent() {
            return dst;
        }
        let sa = f32::from(self.a) / 255.0;
        let da = f32::from(dst.a) / 255.0;
        let out_a = sa + da * (1.0 - sa);
        if out_a <= 0.0 {
            return Self::TRANSPARENT;
        }
        let channel = |src: u8, dst: u8| -> u8 {
            let value = (f32::from(src) * sa + f32::from(dst) * da * (1.0 - sa)) / out_a;
            value.round().clamp(0.0, 255.0) as u8
        };
        Self {
            r: channel(self.r, dst.r),
            g: channel(self.g, dst.g),
            b: channel(self.b, dst.b),
            a: (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self::TRANSPARENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_sets_full_alpha() {
        assert_eq!(Color::rgb(1, 2, 3).to_array(), [1, 2, 3, 255]);
    }

    #[test]
    fn transparent_detection() {
        assert!(Color::TRANSPARENT.is_transparent());
        assert!(!Color::BLACK.is_transparent());
    }

    #[test]
    fn default_is_transparent() {
        assert_eq!(Color::default(), Color::TRANSPARENT);
    }

    #[test]
    fn parses_hex_variants() {
        assert_eq!(Color::from_hex("#ff0000"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(Color::from_hex("00FF00"), Some(Color::rgb(0, 255, 0)));
        assert_eq!(Color::from_hex("#0f0"), Some(Color::rgb(0, 255, 0)));
        assert_eq!(
            Color::from_hex("#0000FF80"),
            Some(Color::rgba(0, 0, 255, 128))
        );
        assert_eq!(Color::from_hex("nope"), None);
    }

    #[test]
    fn opaque_source_over_replaces() {
        assert_eq!(Color::BLACK.over(Color::WHITE), Color::BLACK);
    }

    #[test]
    fn transparent_source_over_is_identity() {
        assert_eq!(Color::TRANSPARENT.over(Color::WHITE), Color::WHITE);
    }

    #[test]
    fn half_transparent_black_over_white_is_gray() {
        let result = Color::rgba(0, 0, 0, 128).over(Color::WHITE);
        assert!(result.r > 120 && result.r < 136, "got {}", result.r);
        assert_eq!(result.a, 255);
    }
}
