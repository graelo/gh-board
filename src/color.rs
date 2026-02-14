use std::fmt;
use std::str::FromStr;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Ansi256(u8),
    Hex { r: u8, g: u8, b: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColorDepth {
    #[default]
    TrueColor,
    Color256,
    Color16,
}

#[derive(Debug, Error)]
#[error("invalid color value for field \"{field}\": \"{value}\"")]
pub struct ColorParseError {
    pub field: String,
    pub value: String,
}

impl Color {
    /// Parse a color string with a field name for error reporting.
    ///
    /// Accepts:
    /// - `"0"` through `"255"` (with optional leading zeros) → `Ansi256`
    /// - `"#RRGGBB"` or `"#RGB"` → `Hex`
    ///
    /// # Errors
    ///
    /// Returns `ColorParseError` if the string is not a valid color.
    pub fn parse(s: &str, field: &str) -> Result<Self, ColorParseError> {
        let make_err = || ColorParseError {
            field: field.to_owned(),
            value: s.to_owned(),
        };

        if let Some(hex) = s.strip_prefix('#') {
            match hex.len() {
                6 => {
                    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| make_err())?;
                    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| make_err())?;
                    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| make_err())?;
                    Ok(Color::Hex { r, g, b })
                }
                3 => {
                    let r = u8::from_str_radix(&hex[0..1], 16).map_err(|_| make_err())?;
                    let g = u8::from_str_radix(&hex[1..2], 16).map_err(|_| make_err())?;
                    let b = u8::from_str_radix(&hex[2..3], 16).map_err(|_| make_err())?;
                    Ok(Color::Hex {
                        r: r * 17,
                        g: g * 17,
                        b: b * 17,
                    })
                }
                _ => Err(make_err()),
            }
        } else {
            // Try ANSI-256 index: must be a decimal integer 0-255.
            let n: u16 = s.parse().map_err(|_| make_err())?;
            let Ok(byte) = u8::try_from(n) else {
                return Err(make_err());
            };
            Ok(Color::Ansi256(byte))
        }
    }

    /// Convert to a `crossterm::style::Color` at the given terminal color depth.
    ///
    /// For ANSI indices 0–15, we use crossterm's named color variants so that
    /// the terminal renders them through its user-configured 16-color palette
    /// (SGR 30–37 / 90–97) rather than the 256-color palette (`38;5;N`), which
    /// some terminals do **not** map to the customised palette.
    pub fn to_crossterm_color(self, depth: ColorDepth) -> crossterm::style::Color {
        match depth {
            ColorDepth::TrueColor => match self {
                Color::Hex { r, g, b } => crossterm::style::Color::Rgb { r, g, b },
                Color::Ansi256(n) => ansi_to_crossterm(n),
            },
            ColorDepth::Color256 => match self {
                Color::Hex { r, g, b } => {
                    let idx = approximate_ansi256(r, g, b);
                    crossterm::style::Color::AnsiValue(idx)
                }
                Color::Ansi256(n) => ansi_to_crossterm(n),
            },
            ColorDepth::Color16 => {
                let (r, g, b) = self.to_rgb();
                let idx = approximate_ansi16(r, g, b);
                ansi_to_crossterm(idx)
            }
        }
    }

    /// Return the RGB representation of this color.
    fn to_rgb(self) -> (u8, u8, u8) {
        match self {
            Color::Hex { r, g, b } => (r, g, b),
            Color::Ansi256(n) => ansi256_to_rgb(n),
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Color::Ansi256(n) => write!(f, "{n}"),
            Color::Hex { r, g, b } => write!(f, "#{r:02x}{g:02x}{b:02x}"),
        }
    }
}

/// Custom serde: deserialize a Color from a string, using a generic field name.
/// For config deserialization, we use `Option<Color>` with a custom deserializer
/// in the theme types.
impl FromStr for Color {
    type Err = ColorParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Color::parse(s, "<unknown>")
    }
}

// ---------------------------------------------------------------------------
// Color depth detection
// ---------------------------------------------------------------------------

impl ColorDepth {
    /// Detect terminal color depth from environment variables.
    pub fn detect() -> Self {
        if let Ok(ct) = std::env::var("COLORTERM")
            && (ct == "truecolor" || ct == "24bit")
        {
            return ColorDepth::TrueColor;
        }
        if let Ok(term) = std::env::var("TERM")
            && term.contains("256color")
        {
            return ColorDepth::Color256;
        }
        ColorDepth::Color16
    }
}

// ---------------------------------------------------------------------------
// ANSI index → crossterm Color
// ---------------------------------------------------------------------------

/// Map an ANSI index to a crossterm `Color`.
///
/// Indices 0–15 are mapped to crossterm's named 16-color variants so the
/// terminal uses its configured palette (SGR 30–37 / 90–97).  Indices 16–255
/// pass through as `AnsiValue`.
fn ansi_to_crossterm(n: u8) -> crossterm::style::Color {
    use crossterm::style::Color;
    match n {
        0 => Color::Black,
        1 => Color::DarkRed,
        2 => Color::DarkGreen,
        3 => Color::DarkYellow,
        4 => Color::DarkBlue,
        5 => Color::DarkMagenta,
        6 => Color::DarkCyan,
        7 => Color::Grey,
        8 => Color::DarkGrey,
        9 => Color::Red,
        10 => Color::Green,
        11 => Color::Yellow,
        12 => Color::Blue,
        13 => Color::Magenta,
        14 => Color::Cyan,
        15 => Color::White,
        _ => Color::AnsiValue(n),
    }
}

// ---------------------------------------------------------------------------
// ANSI-256 palette → RGB lookup
// ---------------------------------------------------------------------------

/// Convert an ANSI-256 index to approximate RGB.
fn ansi256_to_rgb(n: u8) -> (u8, u8, u8) {
    match n {
        // Standard 16 colors (approximate).
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        // 6x6x6 color cube (indices 16-231).
        16..=231 => {
            let idx = n - 16;
            let b_idx = idx % 6;
            let g_idx = (idx / 6) % 6;
            let r_idx = idx / 36;
            let to_val = |i: u8| if i == 0 { 0 } else { 55 + 40 * i };
            (to_val(r_idx), to_val(g_idx), to_val(b_idx))
        }
        // Grayscale ramp (indices 232-255).
        232..=255 => {
            let v = 8 + 10 * (n - 232);
            (v, v, v)
        }
    }
}

/// Approximate an RGB color to the nearest ANSI-256 index (16-255 range).
fn approximate_ansi256(r: u8, g: u8, b: u8) -> u8 {
    // Check grayscale first.
    if r == g && g == b {
        if r < 8 {
            return 16; // black in cube
        }
        if r > 248 {
            return 231; // white in cube
        }
        return 232 + ((u16::from(r) - 8) / 10).min(23) as u8;
    }

    // Map to 6x6x6 cube.
    let to_idx = |v: u8| -> u8 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            ((u16::from(v) - 35) / 40).min(5) as u8
        }
    };
    let ri = to_idx(r);
    let gi = to_idx(g);
    let bi = to_idx(b);
    16 + 36 * ri + 6 * gi + bi
}

/// Approximate an RGB color to the nearest ANSI 16-color index.
fn approximate_ansi16(r: u8, g: u8, b: u8) -> u8 {
    // Simple brightness-based mapping to the 16 standard colors.
    let brightness = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let bright = brightness > 128;

    let rb = r > 128;
    let gb = g > 128;
    let bb = b > 128;

    let base: u8 = match (rb, gb, bb) {
        (false, false, false) => 0, // black
        (true, false, false) => 1,  // red
        (false, true, false) => 2,  // green
        (true, true, false) => 3,   // yellow
        (false, false, true) => 4,  // blue
        (true, false, true) => 5,   // magenta
        (false, true, true) => 6,   // cyan
        (true, true, true) => 7,    // white
    };

    if bright { base + 8 } else { base }
}
