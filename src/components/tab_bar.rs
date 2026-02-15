use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};

// ---------------------------------------------------------------------------
// TabBar component
// ---------------------------------------------------------------------------

/// A single tab definition.
#[derive(Debug, Clone)]
pub struct Tab {
    pub title: String,
    pub count: Option<usize>,
}

/// Pre-rendered tab data (all owned, no lifetime issues).
pub struct RenderedTabBar {
    pub tabs: Vec<RenderedTab>,
    pub active_fg: Color,
    pub inactive_fg: Color,
    pub border_fg: Color,
}

pub struct RenderedTab {
    pub label: String,
    pub is_active: bool,
}

impl RenderedTabBar {
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        tabs: &[Tab],
        active: usize,
        show_count: bool,
        depth: ColorDepth,
        active_color: Option<AppColor>,
        inactive_color: Option<AppColor>,
        border_color: Option<AppColor>,
        icon: &str,
    ) -> Self {
        let active_fg = active_color.map_or(Color::Cyan, |c| c.to_crossterm_color(depth));
        let inactive_fg = inactive_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));

        let icon_prefix = if icon.is_empty() {
            String::new()
        } else {
            format!("{icon} ")
        };

        let rendered_tabs: Vec<RenderedTab> = tabs
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let label = if show_count {
                    if let Some(count) = tab.count {
                        format!(" {icon_prefix}{} ({}) ", tab.title, count)
                    } else {
                        format!(" {icon_prefix}{} ", tab.title)
                    }
                } else {
                    format!(" {icon_prefix}{} ", tab.title)
                };
                RenderedTab {
                    label,
                    is_active: i == active,
                }
            })
            .collect();

        Self {
            tabs: rendered_tabs,
            active_fg,
            inactive_fg,
            border_fg,
        }
    }
}

#[derive(Default, Props)]
pub struct TabBarProps {
    pub tab_bar: Option<RenderedTabBar>,
}

#[component]
pub fn TabBar(props: &mut TabBarProps) -> impl Into<AnyElement<'static>> {
    let Some(tb) = props.tab_bar.take() else {
        return element! { View }.into_any();
    };

    let active_fg = tb.active_fg;
    let inactive_fg = tb.inactive_fg;

    element! {
        View(
            border_style: BorderStyle::Single,
            border_edges: Edges::Bottom,
            border_color: tb.border_fg,
            padding_left: 1,
        ) {
            #(tb.tabs.into_iter().enumerate().map(|(i, tab)| {
                let (fg, bg, weight) = if tab.is_active {
                    (Color::White, Some(active_fg), Weight::Bold)
                } else {
                    (inactive_fg, None, Weight::Normal)
                };

                element! {
                    View(key: i, padding_right: 1, background_color: bg.unwrap_or(Color::Reset)) {
                        Text(content: tab.label, color: fg, weight, wrap: TextWrap::NoWrap)
                    }
                }
            }))
        }
    }
    .into_any()
}
