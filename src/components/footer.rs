use std::time::Instant;

use iocraft::prelude::*;

use crate::app::ViewKind;
use crate::color::{Color as AppColor, ColorDepth};
use crate::icons::ResolvedIcons;
use crate::types::RateLimitInfo;

// ---------------------------------------------------------------------------
// Footer component — structured status bar
// ---------------------------------------------------------------------------

pub struct FooterView {
    pub label: String,
    pub is_active: bool,
    pub color: Color,
}

pub struct RenderedFooter {
    pub views: Vec<FooterView>,
    pub inactive_fg: Color,
    pub scope_label: String,
    pub context_text: String,
    pub updated_text: String,
    pub rate_limit_text: String,
    pub help_hint: String,
    pub text_fg: Color,
    pub border_fg: Color,
    pub separator_fg: Color,
}

impl RenderedFooter {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        active_view: ViewKind,
        icons: &ResolvedIcons,
        scope_label: String,
        context_text: String,
        updated_text: String,
        rate_limit_text: String,
        depth: ColorDepth,
        view_colors: [Option<AppColor>; 4],
        inactive_color: Option<AppColor>,
        text_color: Option<AppColor>,
        border_color: Option<AppColor>,
    ) -> Self {
        let inactive_fg = inactive_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let text_fg = text_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let separator_fg = text_fg;

        let views = ViewKind::ALL
            .iter()
            .zip(view_colors.iter())
            .map(|(v, color)| FooterView {
                label: v.icon_label(icons),
                is_active: *v == active_view,
                color: color.map_or(Color::White, |c| c.to_crossterm_color(depth)),
            })
            .collect();

        Self {
            views,
            inactive_fg,
            scope_label,
            context_text,
            updated_text,
            rate_limit_text,
            help_hint: "? help".to_owned(),
            text_fg,
            border_fg,
            separator_fg,
        }
    }
}

/// Format rate limit info as "API remaining/limit".
pub fn format_rate_limit(info: Option<&RateLimitInfo>) -> String {
    match info {
        Some(rl) => format!("API {}/{}", rl.remaining, rl.limit),
        None => String::new(),
    }
}

/// Format a last-fetch instant as a human-readable "Updated ~Xs ago" string.
pub fn format_updated_ago(last_fetch: Option<Instant>) -> String {
    let Some(t) = last_fetch else {
        return String::new();
    };
    let elapsed = t.elapsed().as_secs();
    if elapsed < 60 {
        format!("Updated ~{elapsed}s ago")
    } else if elapsed < 3600 {
        format!("Updated ~{}m ago", elapsed / 60)
    } else {
        format!("Updated ~{}h ago", elapsed / 3600)
    }
}

#[derive(Default, Props)]
pub struct FooterProps {
    pub footer: Option<RenderedFooter>,
}

#[component]
pub fn Footer(props: &mut FooterProps) -> impl Into<AnyElement<'static>> {
    let Some(f) = props.footer.take() else {
        return element! { View }.into_any();
    };

    let has_scope = !f.scope_label.is_empty();
    let has_context = !f.context_text.is_empty();
    let has_updated = !f.updated_text.is_empty();
    let has_rate_limit = !f.rate_limit_text.is_empty();

    // Pre-build context area contents for MixedText.
    let mut context_contents = Vec::new();
    if has_scope {
        context_contents.push(MixedTextContent::new(&f.scope_label).color(f.text_fg));
    }
    if has_scope && (has_context || has_updated || has_rate_limit) {
        context_contents.push(MixedTextContent::new("  \u{2022}  ").color(f.separator_fg));
    }
    if has_context {
        context_contents.push(MixedTextContent::new(&f.context_text).color(f.text_fg));
    }
    if has_context && has_updated {
        context_contents.push(MixedTextContent::new("  \u{2022}  ").color(f.separator_fg));
    }
    if has_updated {
        context_contents.push(MixedTextContent::new(&f.updated_text).color(f.text_fg));
    }
    if (has_context || has_updated) && has_rate_limit {
        context_contents.push(MixedTextContent::new("  \u{2022}  ").color(f.separator_fg));
    }
    if has_rate_limit {
        context_contents.push(MixedTextContent::new(&f.rate_limit_text).color(f.text_fg));
    }

    element! {
        View(
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: f.border_fg,
            padding_left: 1,
            padding_right: 1,
        ) {
            // Left: view indicators
            #(f.views.iter().map(|s| {
                let (fg, bg, weight) = if s.is_active {
                    (Color::White, Some(s.color), Weight::Bold)
                } else {
                    (f.inactive_fg, None, Weight::Normal)
                };
                element! {
                    View(background_color: bg.unwrap_or(Color::Reset)) {
                        Text(content: format!(" {} ", s.label), color: fg, weight, wrap: TextWrap::NoWrap)
                    }
                }
            }))
            // Pipe separator
            Text(content: " \u{2502} ", color: f.separator_fg, wrap: TextWrap::NoWrap)
            // Middle: context + updated (flex_grow to fill space)
            View(flex_grow: 1.0) {
                MixedText(contents: context_contents, wrap: TextWrap::NoWrap)
            }
            // Pipe separator
            Text(content: " \u{2502} ", color: f.separator_fg, wrap: TextWrap::NoWrap)
            // Right: help hint
            Text(content: f.help_hint.clone(), color: f.text_fg, wrap: TextWrap::NoWrap)
        }
    }
    .into_any()
}
