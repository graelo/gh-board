use std::time::Instant;

use iocraft::prelude::*;

use crate::app::ViewKind;
use crate::color::{Color as AppColor, ColorDepth};
use crate::icons::ResolvedIcons;

// ---------------------------------------------------------------------------
// Footer component — structured status bar
// ---------------------------------------------------------------------------

pub struct FooterSection {
    pub label: String,
    pub is_active: bool,
}

pub struct RenderedFooter {
    pub sections: Vec<FooterSection>,
    pub active_fg: Color,
    pub inactive_fg: Color,
    pub context_text: String,
    pub updated_text: String,
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
        context_text: String,
        updated_text: String,
        depth: ColorDepth,
        active_color: Option<AppColor>,
        inactive_color: Option<AppColor>,
        text_color: Option<AppColor>,
        border_color: Option<AppColor>,
    ) -> Self {
        let active_fg = active_color.map_or(Color::White, |c| c.to_crossterm_color(depth));
        let inactive_fg = inactive_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let text_fg = text_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let separator_fg = text_fg;

        let sections = ViewKind::ALL
            .iter()
            .map(|v| FooterSection {
                label: v.icon_label(icons),
                is_active: *v == active_view,
            })
            .collect();

        Self {
            sections,
            active_fg,
            inactive_fg,
            context_text,
            updated_text,
            help_hint: "? help".to_owned(),
            text_fg,
            border_fg,
            separator_fg,
        }
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

    let has_context = !f.context_text.is_empty();
    let has_updated = !f.updated_text.is_empty();

    element! {
        View(
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: f.border_fg,
            padding_left: 1,
            padding_right: 1,
        ) {
            // Left: section indicators
            #(f.sections.iter().map(|s| {
                let (fg, bg, weight) = if s.is_active {
                    (Color::White, Some(f.active_fg), Weight::Bold)
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
                #(if has_context {
                    Some(element! {
                        Text(content: f.context_text.clone(), color: f.text_fg, wrap: TextWrap::NoWrap)
                    })
                } else {
                    None
                })
                #(if has_context && has_updated {
                    Some(element! {
                        Text(content: "  \u{2022}  ", color: f.separator_fg, wrap: TextWrap::NoWrap)
                    })
                } else {
                    None
                })
                #(if has_updated {
                    Some(element! {
                        Text(content: f.updated_text.clone(), color: f.text_fg, wrap: TextWrap::NoWrap)
                    })
                } else {
                    None
                })
            }
            // Pipe separator
            Text(content: " \u{2502} ", color: f.separator_fg, wrap: TextWrap::NoWrap)
            // Right: help hint
            Text(content: f.help_hint.clone(), color: f.text_fg, wrap: TextWrap::NoWrap)
        }
    }
    .into_any()
}
