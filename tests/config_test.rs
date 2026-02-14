use std::path::Path;

use gh_board::config::loader::load_config;
use gh_board::config::types::AppConfig;

#[test]
fn parse_minimal_config() {
    let toml = r#"
[[pr_sections]]
title = "My PRs"
filters = "author:@me is:open"
"#;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.pr_sections.len(), 1);
    assert_eq!(config.pr_sections[0].title, "My PRs");
    assert_eq!(config.pr_sections[0].filters, "author:@me is:open");
}

#[test]
fn parse_unknown_keys_ignored() {
    let toml = r#"
unknown_top_level = "should be ignored"

[[pr_sections]]
title = "Test"
filters = "is:open"
"#;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.pr_sections.len(), 1);
}

#[test]
fn parse_defaults() {
    let toml = r#"
[defaults]
view = "issues"
refetch_interval_minutes = 5
date_format = "%d/%m/%Y"

[defaults.preview]
width = 0.6
"#;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.defaults.view, gh_board::config::types::View::Issues);
    assert_eq!(config.defaults.refetch_interval_minutes, 5);
    assert!((config.defaults.preview.width - 0.6).abs() < f64::EPSILON);
}

#[test]
fn parse_theme_colors_ansi() {
    let toml = r#"
[theme.colors.text]
primary = "7"
secondary = "245"
"#;
    let config: AppConfig = toml::from_str(toml).unwrap();
    let primary = config.theme.colors.text.primary.unwrap();
    assert_eq!(primary, gh_board::color::Color::Ansi256(7));
}

#[test]
fn parse_theme_colors_hex() {
    let toml = r##"
[theme.colors.text]
primary = "#c0caf5"
"##;
    let config: AppConfig = toml::from_str(toml).unwrap();
    let primary = config.theme.colors.text.primary.unwrap();
    assert_eq!(
        primary,
        gh_board::color::Color::Hex {
            r: 0xc0,
            g: 0xca,
            b: 0xf5
        }
    );
}

#[test]
fn parse_theme_colors_mixed() {
    let toml = r##"
[theme.colors.text]
primary = "#c0caf5"
secondary = "245"
"##;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert!(config.theme.colors.text.primary.is_some());
    assert!(config.theme.colors.text.secondary.is_some());
}

#[test]
fn parse_syntax_colors() {
    let toml = r##"
[theme.colors.markdown.syntax]
keyword = "5"
string = "#00ff00"
"##;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(
        config.theme.colors.markdown.syntax.keyword.unwrap(),
        gh_board::color::Color::Ansi256(5)
    );
}

#[test]
fn parse_invalid_color_fails() {
    let toml = r#"
[theme.colors.text]
primary = "not_a_color"
"#;
    let result: Result<AppConfig, _> = toml::from_str(toml);
    assert!(result.is_err());
}

#[test]
fn default_config_has_sane_defaults() {
    let config = AppConfig::default();
    assert_eq!(config.defaults.view, gh_board::config::types::View::Prs);
    assert_eq!(config.defaults.refetch_interval_minutes, 10);
    assert!((config.defaults.preview.width - 0.45).abs() < f64::EPSILON);
}

#[test]
fn parse_keybindings() {
    let toml = r#"
[[keybindings.universal]]
key = "j"
builtin = "move_down"
name = "Move down"

[[keybindings.prs]]
key = "ctrl+b"
command = "open {{.Url}}"
name = "Open in browser"
"#;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.keybindings.universal.len(), 1);
    assert_eq!(config.keybindings.prs.len(), 1);
    assert_eq!(
        config.keybindings.prs[0].command.as_deref(),
        Some("open {{.Url}}")
    );
}

#[test]
fn parse_repo_paths() {
    let toml = r#"
[repo_paths]
"owner/repo1" = "/Users/user/projects/repo1"
"owner/repo2" = "/Users/user/projects/repo2"
"#;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.repo_paths.len(), 2);
}

// ---------------------------------------------------------------------------
// T068/T069: Config loading integration tests
// ---------------------------------------------------------------------------

#[test]
fn load_global_fixture() {
    let path = Path::new("tests/fixtures/global_config.toml");
    let config = load_config(Some(path)).unwrap();
    assert_eq!(config.pr_sections.len(), 1);
    assert_eq!(config.pr_sections[0].title, "Global PRs");
    assert_eq!(config.defaults.refetch_interval_minutes, 15);
    assert!((config.defaults.preview.width - 0.5).abs() < f64::EPSILON);
    assert!(config.repo_paths.contains_key("org/global-repo"));
}

#[test]
fn load_local_override_fixture() {
    let path = Path::new("tests/fixtures/local_override.toml");
    let config = load_config(Some(path)).unwrap();
    assert_eq!(config.pr_sections.len(), 1);
    assert_eq!(config.pr_sections[0].title, "Local PRs");
    assert_eq!(config.pr_sections[0].limit, Some(50));
    assert_eq!(config.defaults.view, gh_board::config::types::View::Issues);
    assert_eq!(config.defaults.refetch_interval_minutes, 5);
    assert!((config.defaults.preview.width - 0.3).abs() < f64::EPSILON);
}

#[test]
fn invalid_toml_produces_error() {
    let path = Path::new("tests/fixtures/invalid_toml.toml");
    let result = load_config(Some(path));
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    // Error should reference the file path.
    assert!(
        err_msg.contains("invalid_toml.toml"),
        "error should mention file: {err_msg}"
    );
}

#[test]
fn unknown_keys_in_fixture_tolerated() {
    let path = Path::new("tests/fixtures/unknown_keys_config.toml");
    let config = load_config(Some(path)).unwrap();
    assert_eq!(config.pr_sections.len(), 1);
    assert_eq!(config.pr_sections[0].title, "PRs with Unknown");
}

#[test]
fn config_flag_overrides_discovery() {
    // When an explicit path is given, it should be loaded directly.
    let path = Path::new("tests/fixtures/global_config.toml");
    let config = load_config(Some(path)).unwrap();
    assert_eq!(config.pr_sections[0].title, "Global PRs");
}

#[test]
fn missing_config_file_produces_error() {
    let path = Path::new("tests/fixtures/nonexistent.toml");
    let result = load_config(Some(path));
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// T070: date_format config option
// ---------------------------------------------------------------------------

#[test]
fn default_date_format_is_relative() {
    let config = AppConfig::default();
    assert_eq!(config.defaults.date_format, "relative");
}

#[test]
fn parse_custom_date_format() {
    let toml = r#"
[defaults]
date_format = "%Y-%m-%d"
"#;
    let config: AppConfig = toml::from_str(toml).unwrap();
    assert_eq!(config.defaults.date_format, "%Y-%m-%d");
}
