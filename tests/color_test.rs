use gh_board::color::{Color, ColorDepth};
use gh_board::config::types::AppConfig;
use gh_board::theme::{Background, ResolvedTheme};

/// Extract the 16-color index from a crossterm color.  Works for both named
/// color variants (e.g. `Color::Red` → 9) and `AnsiValue(0..15)`.
fn to_ansi16_index(c: crossterm::style::Color) -> Option<u8> {
    use crossterm::style::Color as C;
    match c {
        C::Black => Some(0),
        C::DarkRed => Some(1),
        C::DarkGreen => Some(2),
        C::DarkYellow => Some(3),
        C::DarkBlue => Some(4),
        C::DarkMagenta => Some(5),
        C::DarkCyan => Some(6),
        C::Grey => Some(7),
        C::DarkGrey => Some(8),
        C::Red => Some(9),
        C::Green => Some(10),
        C::Yellow => Some(11),
        C::Blue => Some(12),
        C::Magenta => Some(13),
        C::Cyan => Some(14),
        C::White => Some(15),
        C::AnsiValue(n) if n < 16 => Some(n),
        _ => None,
    }
}

#[test]
fn parse_hex_6_digit() {
    let c = Color::parse("#c0caf5", "test_field").unwrap();
    assert_eq!(
        c,
        Color::Hex {
            r: 0xc0,
            g: 0xca,
            b: 0xf5
        }
    );
}

#[test]
fn parse_hex_3_digit() {
    let c = Color::parse("#f0a", "test_field").unwrap();
    // #f0a → r=0xff, g=0x00, b=0xaa
    assert_eq!(
        c,
        Color::Hex {
            r: 0xff,
            g: 0x00,
            b: 0xaa
        }
    );
}

#[test]
fn parse_hex_uppercase() {
    let c = Color::parse("#FF00AA", "test_field").unwrap();
    assert_eq!(
        c,
        Color::Hex {
            r: 0xff,
            g: 0x00,
            b: 0xaa
        }
    );
}

#[test]
fn parse_ansi256_zero() {
    let c = Color::parse("0", "test_field").unwrap();
    assert_eq!(c, Color::Ansi256(0));
}

#[test]
fn parse_ansi256_max() {
    let c = Color::parse("255", "test_field").unwrap();
    assert_eq!(c, Color::Ansi256(255));
}

#[test]
fn parse_ansi256_leading_zeros() {
    let c = Color::parse("007", "test_field").unwrap();
    assert_eq!(c, Color::Ansi256(7));
}

#[test]
fn parse_ansi256_out_of_range() {
    let err = Color::parse("256", "bg.selected").unwrap_err();
    assert!(err.to_string().contains("bg.selected"));
    assert!(err.to_string().contains("256"));
}

#[test]
fn parse_invalid_string() {
    let err = Color::parse("foobar", "text.primary").unwrap_err();
    assert!(err.to_string().contains("text.primary"));
    assert!(err.to_string().contains("foobar"));
}

#[test]
fn parse_invalid_hex_too_short() {
    let err = Color::parse("#ab", "border.faint").unwrap_err();
    assert!(err.to_string().contains("border.faint"));
}

#[test]
fn parse_invalid_hex_bad_chars() {
    let err = Color::parse("#gggggg", "text.error").unwrap_err();
    assert!(err.to_string().contains("text.error"));
}

#[test]
fn to_crossterm_truecolor_hex() {
    let c = Color::Hex {
        r: 0xc0,
        g: 0xca,
        b: 0xf5,
    };
    let ct = c.to_crossterm_color(ColorDepth::TrueColor);
    assert_eq!(
        ct,
        crossterm::style::Color::Rgb {
            r: 0xc0,
            g: 0xca,
            b: 0xf5
        }
    );
}

#[test]
fn to_crossterm_truecolor_ansi() {
    let c = Color::Ansi256(42);
    let ct = c.to_crossterm_color(ColorDepth::TrueColor);
    assert_eq!(ct, crossterm::style::Color::AnsiValue(42));
}

#[test]
fn to_crossterm_256_ansi_passthrough() {
    let c = Color::Ansi256(100);
    let ct = c.to_crossterm_color(ColorDepth::Color256);
    assert_eq!(ct, crossterm::style::Color::AnsiValue(100));
}

#[test]
fn to_crossterm_256_hex_approximated() {
    let c = Color::Hex { r: 255, g: 0, b: 0 };
    let ct = c.to_crossterm_color(ColorDepth::Color256);
    // Should be approximated to an ANSI 256 value (not RGB).
    assert!(matches!(ct, crossterm::style::Color::AnsiValue(_)));
}

#[test]
fn to_crossterm_16_color() {
    let c = Color::Ansi256(196); // bright red in 256 palette
    let ct = c.to_crossterm_color(ColorDepth::Color16);
    // Should map to one of the 16 standard colors (named or AnsiValue).
    let n = to_ansi16_index(ct).expect("expected a 16-color value");
    assert!(n < 16, "expected 16-color index, got {n}");
}

#[test]
fn display_ansi() {
    assert_eq!(Color::Ansi256(42).to_string(), "42");
}

#[test]
fn display_hex() {
    assert_eq!(
        Color::Hex {
            r: 0xc0,
            g: 0xca,
            b: 0xf5
        }
        .to_string(),
        "#c0caf5"
    );
}

// ---------------------------------------------------------------------------
// T043: ANSI-only config loads and resolves at all color depths
// ---------------------------------------------------------------------------

fn load_fixture(name: &str) -> AppConfig {
    let path = format!("tests/fixtures/{name}");
    let contents = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"));
    toml::from_str(&contents).unwrap_or_else(|e| panic!("parsing {path}: {e}"))
}

#[test]
fn ansi_only_config_loads_successfully() {
    let config = load_fixture("ansi_only_theme.toml");
    assert!(!config.pr_filters.is_empty(), "should have PR filters");
    assert!((config.defaults.preview.width - 0.45).abs() < f64::EPSILON);
}

#[test]
fn ansi_only_theme_resolves_at_all_depths() {
    let config = load_fixture("ansi_only_theme.toml");

    for bg in [Background::Dark, Background::Light] {
        let theme = ResolvedTheme::resolve(&config.theme, bg);

        for depth in [
            ColorDepth::TrueColor,
            ColorDepth::Color256,
            ColorDepth::Color16,
        ] {
            // Every resolved color should convert to a crossterm color without panic.
            let colors = [
                theme.text_primary,
                theme.text_secondary,
                theme.text_inverted,
                theme.text_faint,
                theme.text_warning,
                theme.text_success,
                theme.text_error,
                theme.text_actor,
                theme.bg_selected,
                theme.border_primary,
                theme.border_secondary,
                theme.border_faint,
                theme.md_text,
                theme.md_heading,
                theme.md_h1,
                theme.md_h2,
                theme.md_h3,
                theme.md_code,
                theme.md_code_block,
                theme.md_link,
                theme.md_link_text,
                theme.md_emphasis,
                theme.md_strong,
                theme.md_strikethrough,
                theme.md_horizontal_rule,
                theme.md_blockquote,
                theme.syn_keyword,
                theme.syn_string,
                theme.syn_comment,
                theme.syn_number,
                theme.syn_function,
                theme.syn_type,
                theme.syn_operator,
                theme.syn_punctuation,
                theme.syn_name,
                theme.syn_name_builtin,
            ];
            for color in colors {
                let _ct = color.to_crossterm_color(depth);
            }
        }
    }
}

#[test]
fn ansi_only_config_values_are_ansi256() {
    let config = load_fixture("ansi_only_theme.toml");
    let theme = ResolvedTheme::resolve(&config.theme, Background::Dark);

    // Text primary is "7" → Ansi256(7)
    assert_eq!(theme.text_primary, Color::Ansi256(7));
    // Markdown heading is "12" → Ansi256(12)
    assert_eq!(theme.md_heading, Color::Ansi256(12));
    // Syntax keyword is "5" → Ansi256(5)
    assert_eq!(theme.syn_keyword, Color::Ansi256(5));
    // Background selected is "237" → Ansi256(237)
    assert_eq!(theme.bg_selected, Color::Ansi256(237));
}

// ---------------------------------------------------------------------------
// T043: Mixed ANSI+hex config
// ---------------------------------------------------------------------------

#[test]
fn mixed_config_loads_successfully() {
    let config = load_fixture("mixed_theme.toml");
    assert!(!config.pr_filters.is_empty());
}

#[test]
fn mixed_theme_coexists_ansi_and_hex() {
    let config = load_fixture("mixed_theme.toml");
    let theme = ResolvedTheme::resolve(&config.theme, Background::Dark);

    // text.primary is hex
    assert_eq!(
        theme.text_primary,
        Color::Hex {
            r: 0xc0,
            g: 0xca,
            b: 0xf5
        }
    );
    // text.secondary is ANSI
    assert_eq!(theme.text_secondary, Color::Ansi256(245));
    // text.actor is ANSI
    assert_eq!(theme.text_actor, Color::Ansi256(6));
    // md.heading is hex
    assert_eq!(
        theme.md_heading,
        Color::Hex {
            r: 0x7a,
            g: 0xa2,
            b: 0xf7
        }
    );
    // syn.comment is ANSI
    assert_eq!(theme.syn_comment, Color::Ansi256(243));
    // syn.keyword is hex
    assert_eq!(
        theme.syn_keyword,
        Color::Hex {
            r: 0xbb,
            g: 0x9a,
            b: 0xf7
        }
    );
}

#[test]
fn mixed_theme_resolves_at_all_depths() {
    let config = load_fixture("mixed_theme.toml");
    let theme = ResolvedTheme::resolve(&config.theme, Background::Dark);

    for depth in [
        ColorDepth::TrueColor,
        ColorDepth::Color256,
        ColorDepth::Color16,
    ] {
        // Hex value converted at each depth
        let ct = theme.text_primary.to_crossterm_color(depth);
        match depth {
            ColorDepth::TrueColor => {
                assert_eq!(
                    ct,
                    crossterm::style::Color::Rgb {
                        r: 0xc0,
                        g: 0xca,
                        b: 0xf5
                    }
                );
            }
            ColorDepth::Color256 => {
                assert!(matches!(ct, crossterm::style::Color::AnsiValue(_)));
            }
            ColorDepth::Color16 => {
                let n = to_ansi16_index(ct).expect("expected 16-color");
                assert!(n < 16, "16-color: got {n}");
            }
        }

        // ANSI value should pass through at TrueColor/256, degrade at 16
        let ct_ansi = theme.text_secondary.to_crossterm_color(depth);
        match depth {
            ColorDepth::TrueColor | ColorDepth::Color256 => {
                assert_eq!(ct_ansi, crossterm::style::Color::AnsiValue(245));
            }
            ColorDepth::Color16 => {
                let n = to_ansi16_index(ct_ansi).expect("expected 16-color");
                assert!(n < 16, "16-color degradation: got {n}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// T044: Invalid color values produce descriptive errors
// ---------------------------------------------------------------------------

#[test]
fn invalid_color_in_config_produces_error() {
    let toml_str = r#"
[[pr_filters]]
title = "test"
filters = "is:open"

[theme.colors.text]
primary = "foobar"
"#;

    let result: Result<AppConfig, _> = toml::from_str(toml_str);
    let err = result.unwrap_err();
    let msg = err.to_string();
    // Error should reference the invalid value.
    assert!(
        msg.contains("foobar"),
        "error should mention the invalid value: {msg}"
    );
}

#[test]
fn out_of_range_ansi_in_config_produces_error() {
    let toml_str = r#"
[theme.colors.text]
primary = "999"
"#;

    let result: Result<AppConfig, _> = toml::from_str(toml_str);
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("999"),
        "error should mention the out-of-range value: {msg}"
    );
}

#[test]
fn invalid_hex_in_config_produces_error() {
    let toml_str = r##"
[theme.colors.border]
primary = "#xyz"
"##;

    let result: Result<AppConfig, _> = toml::from_str(toml_str);
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("#xyz"),
        "error should mention the invalid hex: {msg}"
    );
}

// ---------------------------------------------------------------------------
// T045: 16-color degradation produces reasonable approximations
// ---------------------------------------------------------------------------

#[test]
fn hex_degrades_to_16_color_reasonably() {
    // Pure red hex → should degrade to red (ANSI 1 or 9)
    let red = Color::Hex { r: 255, g: 0, b: 0 };
    let n =
        to_ansi16_index(red.to_crossterm_color(ColorDepth::Color16)).expect("expected 16-color");
    assert!(n == 1 || n == 9, "red should map to ANSI 1 or 9, got {n}");

    // Pure green hex → should degrade to green (ANSI 2 or 10)
    let green = Color::Hex { r: 0, g: 255, b: 0 };
    let n =
        to_ansi16_index(green.to_crossterm_color(ColorDepth::Color16)).expect("expected 16-color");
    assert!(
        n == 2 || n == 10,
        "green should map to ANSI 2 or 10, got {n}"
    );

    // Pure blue hex → should degrade to blue (ANSI 4 or 12)
    let blue = Color::Hex { r: 0, g: 0, b: 255 };
    let n =
        to_ansi16_index(blue.to_crossterm_color(ColorDepth::Color16)).expect("expected 16-color");
    assert!(
        n == 4 || n == 12,
        "blue should map to ANSI 4 or 12, got {n}"
    );

    // White hex → should degrade to white (ANSI 7 or 15)
    let white = Color::Hex {
        r: 255,
        g: 255,
        b: 255,
    };
    let n =
        to_ansi16_index(white.to_crossterm_color(ColorDepth::Color16)).expect("expected 16-color");
    assert!(
        n == 7 || n == 15,
        "white should map to ANSI 7 or 15, got {n}"
    );
}

#[test]
fn ansi256_degrades_to_16_color() {
    // ANSI 196 (bright red) → should degrade to a red-ish 16-color
    let c = Color::Ansi256(196);
    let n = to_ansi16_index(c.to_crossterm_color(ColorDepth::Color16)).expect("expected 16-color");
    assert!(n == 1 || n == 9, "bright red ANSI 196 → got {n}");

    // ANSI 46 (bright green) → should degrade to green-ish
    let c = Color::Ansi256(46);
    let n = to_ansi16_index(c.to_crossterm_color(ColorDepth::Color16)).expect("expected 16-color");
    assert!(n == 2 || n == 10, "bright green ANSI 46 → got {n}");
}

#[test]
fn full_ansi_theme_degrades_to_16_without_panic() {
    let config = load_fixture("ansi_only_theme.toml");
    let theme = ResolvedTheme::resolve(&config.theme, Background::Dark);

    let all_colors = [
        theme.text_primary,
        theme.text_secondary,
        theme.text_faint,
        theme.text_warning,
        theme.text_success,
        theme.text_error,
        theme.text_actor,
        theme.bg_selected,
        theme.border_primary,
        theme.border_faint,
        theme.md_heading,
        theme.md_code,
        theme.md_link,
        theme.syn_keyword,
        theme.syn_string,
        theme.syn_comment,
        theme.syn_number,
        theme.syn_function,
        theme.syn_type,
    ];

    for color in all_colors {
        let ct = color.to_crossterm_color(ColorDepth::Color16);
        let n = to_ansi16_index(ct).expect("expected 16-color for Color16 degradation");
        assert!(n < 16, "all colors should degrade to 16-color: got {n}");
    }
}
