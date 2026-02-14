use crate::color::{Color as AppColor, ColorDepth};
use crate::theme::ResolvedTheme;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

// ---------------------------------------------------------------------------
// Highlight name index → theme color mapping
// ---------------------------------------------------------------------------

/// Recognized highlight names in priority order. The index into this array
/// matches the `Highlight` index returned by tree-sitter-highlight.
const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "function",
    "function.builtin",
    "keyword",
    "number",
    "operator",
    "property",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Map a highlight name index to a theme color.
fn highlight_color(index: usize, theme: &ResolvedTheme) -> AppColor {
    match HIGHLIGHT_NAMES.get(index) {
        Some(&"comment") => theme.syn_comment,
        Some(&"keyword") => theme.syn_keyword,
        Some(&"string" | &"string.special") => theme.syn_string,
        Some(&"number" | &"constant" | &"constant.builtin") => theme.syn_number,
        Some(&"function" | &"function.builtin" | &"constructor") => theme.syn_function,
        Some(&"type" | &"type.builtin") => theme.syn_type,
        Some(&"operator") => theme.syn_operator,
        Some(&"punctuation" | &"punctuation.bracket" | &"punctuation.delimiter") => {
            theme.syn_punctuation
        }
        Some(&"attribute" | &"tag") => theme.syn_name_builtin,
        Some(&"property") => theme.syn_name,
        _ => theme.text_primary,
    }
}

// ---------------------------------------------------------------------------
// Language configuration
// ---------------------------------------------------------------------------

/// Get a `HighlightConfiguration` for a language tag, or `None` if unsupported.
#[allow(clippy::too_many_lines)]
fn config_for_language(lang: &str) -> Option<HighlightConfiguration> {
    let (language, highlights_query, injections_query, locals_query) = match lang {
        "rust" | "rs" => (
            tree_sitter_rust::LANGUAGE.into(),
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        ),
        "go" | "golang" => (
            tree_sitter_go::LANGUAGE.into(),
            tree_sitter_go::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "python" | "py" => (
            tree_sitter_python::LANGUAGE.into(),
            tree_sitter_python::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "javascript" | "js" => (
            tree_sitter_javascript::LANGUAGE.into(),
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::INJECTIONS_QUERY,
            tree_sitter_javascript::LOCALS_QUERY,
        ),
        "typescript" | "ts" => (
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "tsx" => (
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            tree_sitter_typescript::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_typescript::LOCALS_QUERY,
        ),
        "ruby" | "rb" => (
            tree_sitter_ruby::LANGUAGE.into(),
            tree_sitter_ruby::HIGHLIGHTS_QUERY,
            "",
            tree_sitter_ruby::LOCALS_QUERY,
        ),
        "bash" | "sh" | "shell" | "zsh" => (
            tree_sitter_bash::LANGUAGE.into(),
            tree_sitter_bash::HIGHLIGHT_QUERY,
            "",
            "",
        ),
        "json" => (
            tree_sitter_json::LANGUAGE.into(),
            tree_sitter_json::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "toml" => (
            tree_sitter_toml_ng::LANGUAGE.into(),
            tree_sitter_toml_ng::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "html" => (
            tree_sitter_html::LANGUAGE.into(),
            tree_sitter_html::HIGHLIGHTS_QUERY,
            tree_sitter_html::INJECTIONS_QUERY,
            "",
        ),
        "css" => (
            tree_sitter_css::LANGUAGE.into(),
            tree_sitter_css::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        "c" => (
            tree_sitter_c::LANGUAGE.into(),
            tree_sitter_c::HIGHLIGHT_QUERY,
            "",
            "",
        ),
        "cpp" | "c++" | "cxx" => (
            tree_sitter_cpp::LANGUAGE.into(),
            tree_sitter_cpp::HIGHLIGHT_QUERY,
            "",
            "",
        ),
        "java" => (
            tree_sitter_java::LANGUAGE.into(),
            tree_sitter_java::HIGHLIGHTS_QUERY,
            "",
            "",
        ),
        _ => return None,
    };

    let mut config = HighlightConfiguration::new(
        language,
        lang,
        highlights_query,
        injections_query,
        locals_query,
    )
    .ok()?;

    config.configure(HIGHLIGHT_NAMES);
    Some(config)
}

// ---------------------------------------------------------------------------
// Public API: highlight code → styled spans
// ---------------------------------------------------------------------------

/// A styled span of source code.
#[derive(Debug, Clone)]
pub struct SyntaxSpan {
    pub text: String,
    pub color: AppColor,
}

/// Highlight source code with tree-sitter and return colored spans.
///
/// If the language is unsupported or highlighting fails, returns the plain
/// text with the default code block color.
pub fn highlight_code(
    source: &str,
    lang: &str,
    theme: &ResolvedTheme,
    _depth: ColorDepth,
) -> Vec<SyntaxSpan> {
    let Some(config) = config_for_language(lang) else {
        return vec![SyntaxSpan {
            text: source.to_owned(),
            color: theme.md_code_block,
        }];
    };

    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(&config, source.as_bytes(), None, |_| None) else {
        return vec![SyntaxSpan {
            text: source.to_owned(),
            color: theme.md_code_block,
        }];
    };

    let mut spans = Vec::new();
    let mut color_stack: Vec<AppColor> = vec![theme.md_code_block];

    for event in events {
        let Ok(event) = event else { break };
        match event {
            HighlightEvent::Source { start, end } => {
                let text = source.get(start..end).unwrap_or("").to_owned();
                if !text.is_empty() {
                    let color = color_stack.last().copied().unwrap_or(theme.md_code_block);
                    spans.push(SyntaxSpan { text, color });
                }
            }
            HighlightEvent::HighlightStart(h) => {
                color_stack.push(highlight_color(h.0, theme));
            }
            HighlightEvent::HighlightEnd => {
                color_stack.pop();
            }
        }
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::Theme;
    use crate::theme::Background;

    fn test_theme() -> ResolvedTheme {
        ResolvedTheme::resolve(&Theme::default(), Background::Dark)
    }

    #[test]
    fn highlight_rust_code() {
        let theme = test_theme();
        let code = "fn main() {\n    println!(\"hello\");\n}\n";
        let spans = highlight_code(code, "rust", &theme, ColorDepth::TrueColor);
        assert!(!spans.is_empty(), "should produce spans for Rust code");

        let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(full_text, code, "spans should reconstruct original code");
    }

    #[test]
    fn highlight_python_code() {
        let theme = test_theme();
        let code = "def hello():\n    print(\"world\")\n";
        let spans = highlight_code(code, "python", &theme, ColorDepth::TrueColor);
        assert!(!spans.is_empty());

        let full_text: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(full_text, code);
    }

    #[test]
    fn unsupported_language_returns_plain() {
        let theme = test_theme();
        let code = "some exotic code";
        let spans = highlight_code(code, "brainfuck", &theme, ColorDepth::TrueColor);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].text, code);
    }

    #[test]
    fn config_for_all_supported_languages() {
        let langs = [
            "rust",
            "go",
            "python",
            "javascript",
            "typescript",
            "tsx",
            "ruby",
            "bash",
            "json",
            "toml",
            "html",
            "css",
            "c",
            "cpp",
            "java",
        ];
        for lang in &langs {
            assert!(
                config_for_language(lang).is_some(),
                "should support language: {lang}"
            );
        }
    }
}
