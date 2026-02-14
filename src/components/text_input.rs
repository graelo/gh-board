use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};

// ---------------------------------------------------------------------------
// Pre-rendered text input (T056)
// ---------------------------------------------------------------------------

pub struct RenderedTextInput {
    pub prompt: String,
    pub text: String,
    pub text_fg: Color,
    pub prompt_fg: Color,
    pub border_fg: Color,
    pub suggestions: Vec<RenderedSuggestion>,
    pub selected_index: Option<usize>,
}

pub struct RenderedSuggestion {
    pub text: String,
    pub is_selected: bool,
    pub fg: Color,
    pub selected_fg: Color,
    #[allow(dead_code)]
    pub selected_bg: Color,
}

impl RenderedTextInput {
    pub fn build(
        prompt: &str,
        text: &str,
        depth: ColorDepth,
        text_color: Option<AppColor>,
        prompt_color: Option<AppColor>,
        border_color: Option<AppColor>,
    ) -> Self {
        Self::build_with_suggestions(
            prompt,
            text,
            depth,
            text_color,
            prompt_color,
            border_color,
            &[],
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn build_with_suggestions(
        prompt: &str,
        text: &str,
        depth: ColorDepth,
        text_color: Option<AppColor>,
        prompt_color: Option<AppColor>,
        border_color: Option<AppColor>,
        suggestions: &[String],
        selected_index: Option<usize>,
        highlight_color: Option<AppColor>,
        _highlight_bg_color: Option<AppColor>,
    ) -> Self {
        let text_fg = text_color.map_or(Color::White, |c| c.to_crossterm_color(depth));
        let prompt_fg = prompt_color.map_or(Color::Cyan, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let highlight_fg = highlight_color.map_or(Color::Cyan, |c| c.to_crossterm_color(depth));

        let rendered_suggestions: Vec<RenderedSuggestion> = suggestions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let is_selected = selected_index == Some(i);
                RenderedSuggestion {
                    text: s.clone(),
                    is_selected,
                    fg: text_fg,
                    selected_fg: highlight_fg,
                    selected_bg: Color::Reset,
                }
            })
            .collect();

        Self {
            prompt: prompt.to_owned(),
            text: format!("{text}\u{2588}"), // append full block cursor █
            text_fg,
            prompt_fg,
            border_fg,
            suggestions: rendered_suggestions,
            selected_index,
        }
    }
}

// ---------------------------------------------------------------------------
// Autocomplete filtering helper (T085)
// ---------------------------------------------------------------------------

/// Filter a list of candidates by a query string (case-insensitive prefix match).
pub(crate) fn filter_suggestions(candidates: &[String], query: &str) -> Vec<String> {
    if query.is_empty() {
        return candidates.to_vec();
    }
    let lower = query.to_lowercase();
    candidates
        .iter()
        .filter(|c| c.to_lowercase().contains(&lower))
        .cloned()
        .collect()
}

// ---------------------------------------------------------------------------
// TextInput component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct TextInputProps {
    pub input: Option<RenderedTextInput>,
}

#[component]
pub fn TextInput(props: &mut TextInputProps) -> impl Into<AnyElement<'static>> {
    let Some(input) = props.input.take() else {
        return element! { View }.into_any();
    };

    // Pre-render suggestion elements.
    let suggestion_elements: Vec<_> = input
        .suggestions
        .iter()
        .map(|s| {
            let color = if s.is_selected { s.selected_fg } else { s.fg };
            let prefix = if s.is_selected { "> " } else { "  " };
            (format!("{prefix}{}", s.text), color)
        })
        .collect();

    let has_suggestions = !suggestion_elements.is_empty();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: input.border_fg,
            padding_left: 1,
            padding_right: 1,
        ) {
            View(flex_direction: FlexDirection::Row) {
                Text(content: input.prompt, color: input.prompt_fg, wrap: TextWrap::NoWrap)
                Text(content: " ", color: input.text_fg)
                Text(content: input.text, color: input.text_fg, wrap: TextWrap::NoWrap)
            }
            #(if has_suggestions {
                Some(element! {
                    View(flex_direction: FlexDirection::Column) {
                        #(suggestion_elements.into_iter().map(|(text, fg)| {
                            element! {
                                Text(content: text, color: fg, wrap: TextWrap::NoWrap)
                            }.into_any()
                        }))
                    }
                })
            } else {
                None
            })
        }
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_empty_query_returns_all() {
        let candidates = vec!["bug".to_owned(), "feature".to_owned(), "docs".to_owned()];
        let result = filter_suggestions(&candidates, "");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn filter_prefix_match() {
        let candidates = vec!["bug".to_owned(), "build".to_owned(), "docs".to_owned()];
        let result = filter_suggestions(&candidates, "bu");
        assert_eq!(result.len(), 2);
        assert!(result.contains(&"bug".to_owned()));
        assert!(result.contains(&"build".to_owned()));
    }

    #[test]
    fn filter_case_insensitive() {
        let candidates = vec!["Bug".to_owned(), "BUILD".to_owned(), "docs".to_owned()];
        let result = filter_suggestions(&candidates, "bu");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_no_match() {
        let candidates = vec!["bug".to_owned(), "feature".to_owned()];
        let result = filter_suggestions(&candidates, "xyz");
        assert!(result.is_empty());
    }

    #[test]
    fn filter_substring_match() {
        let candidates = vec!["enhancement".to_owned(), "bug".to_owned()];
        let result = filter_suggestions(&candidates, "ance");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "enhancement");
    }
}
