//! Color palettes, mirroring JS Paint's `color-data.js`.

use crate::color::Color;

/// The default 28-color palette, in two rows of 14.
pub const DEFAULT_PALETTE: [&str; 28] = [
    "000000", "808080", "800000", "808000", "008000", "008080", "000080", "800080", "808040",
    "004040", "0080FF", "004080", "4000FF", "804000", "FFFFFF", "C0C0C0", "FF0000", "FFFF00",
    "00FF00", "00FFFF", "0000FF", "FF00FF", "FFFF80", "00FF80", "80FFFF", "8080FF", "FF0080",
    "FF8040",
];

/// The 48-color "basic colors" grid shown in the Edit Colors dialog.
pub const BASIC_COLORS: [&str; 48] = [
    "FF8080", "FFFF80", "80FF80", "00FF80", "80FFFF", "0080FF", "FF80C0", "FF80FF", "FF0000",
    "FFFF00", "80FF00", "00FF40", "00FFFF", "0080C0", "8080C0", "FF00FF", "804040", "FF8040",
    "00FF00", "008080", "004080", "8080FF", "800040", "FF0080", "800000", "FF8000", "008000",
    "008040", "0000FF", "0000A0", "800080", "8000FF", "400000", "804000", "004000", "004040",
    "000080", "000040", "400040", "400080", "000000", "808000", "808040", "808080", "408080",
    "C0C0C0", "400040", "FFFFFF",
];

/// Number of customizable colors shown in the palette.
pub const CUSTOM_COLOR_COUNT: usize = 16;

/// Parses a slice of hex strings, skipping any that fail to parse.
pub fn colors_from_hex(hexes: &[&str]) -> Vec<Color> {
    hexes
        .iter()
        .filter_map(|hex| Color::from_hex(hex))
        .collect()
}

/// The default palette as colors.
pub fn default_palette() -> Vec<Color> {
    colors_from_hex(&DEFAULT_PALETTE)
}

/// The monochrome palette (grayscale ramp), used in "black and white" mode.
pub fn monochrome_palette() -> Vec<Color> {
    const MONO: [&str; 28] = [
        "000000", "090909", "121212", "1B1B1B", "252525", "2E2E2E", "373737", "3F3F3F", "494949",
        "525252", "5C5C5C", "656565", "6E6E6E", "777777", "FFFFFF", "FAFAFA", "F2F2F2", "D4D4D4",
        "C9C9C9", "BFBFBF", "B6B6B6", "9F9F9F", "808080", "ADADAD", "A4A4A4", "9B9B9B", "929292",
        "898989",
    ];
    colors_from_hex(&MONO)
}

/// The initial custom color swatches (all white, as in JS Paint).
pub fn default_custom_colors() -> Vec<Color> {
    vec![Color::WHITE; CUSTOM_COLOR_COUNT]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_palette_has_expected_size_and_first_entry() {
        let palette = default_palette();
        assert_eq!(palette.len(), 28);
        assert_eq!(palette[0], Color::BLACK);
        assert_eq!(palette[14], Color::WHITE);
    }

    #[test]
    fn basic_colors_parse_completely() {
        assert_eq!(colors_from_hex(&BASIC_COLORS).len(), 48);
    }

    #[test]
    fn custom_colors_start_white() {
        assert_eq!(default_custom_colors().len(), CUSTOM_COLOR_COUNT);
        assert!(default_custom_colors().iter().all(|c| *c == Color::WHITE));
    }
}
