use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};
use crate::components::markdown_view::{MarkdownView, RenderedMarkdown};
use crate::icons::ResolvedIcons;
use crate::markdown::renderer::StyledLine;

// ---------------------------------------------------------------------------
// Sidebar tab enum (T072 — FR-014)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarTab {
    Overview,
    Activity,
    Commits,
    Checks,
    Files,
}

impl SidebarTab {
    pub const ALL: &'static [SidebarTab] = &[
        SidebarTab::Overview,
        SidebarTab::Activity,
        SidebarTab::Commits,
        SidebarTab::Checks,
        SidebarTab::Files,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Activity => "Activity",
            Self::Commits => "Commits",
            Self::Checks => "Checks",
            Self::Files => "Files",
        }
    }

    /// Label prefixed with the matching icon from the resolved icon set.
    pub fn icon_label(self, icons: &ResolvedIcons) -> String {
        let icon = match self {
            Self::Overview => &icons.tab_overview,
            Self::Activity => &icons.tab_activity,
            Self::Commits => &icons.tab_commits,
            Self::Checks => &icons.tab_checks,
            Self::Files => &icons.tab_files,
        };
        format!("{icon} {}", self.label())
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    #[must_use]
    pub fn next(self) -> Self {
        let idx = self.index();
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    #[must_use]
    pub fn prev(self) -> Self {
        let idx = self.index();
        if idx == 0 {
            Self::ALL[Self::ALL.len() - 1]
        } else {
            Self::ALL[idx - 1]
        }
    }
}

// ---------------------------------------------------------------------------
// Sidebar meta header (gh-dash style pill + author line)
// ---------------------------------------------------------------------------

pub struct SidebarMeta {
    // Pill badge
    pub pill_icon: String,
    pub pill_text: String,
    pub pill_bg: Color,
    pub pill_fg: Color,
    // Pill caps (rounded edges via Powerline glyphs)
    pub pill_left: String,
    pub pill_right: String,
    // Branch (same line, after pill)
    pub branch_text: String,
    pub branch_fg: Color,
    // Author line
    pub author_text: String,
    pub author_fg: Color,
    pub separator_fg: Color,
    pub age_text: String,
    pub age_fg: Color,
    pub role_icon: String,
    pub role_text: String,
    pub role_fg: Color,
}

// ---------------------------------------------------------------------------
// Pre-rendered sidebar (all owned)
// ---------------------------------------------------------------------------

pub struct RenderedSidebar {
    pub title: String,
    pub scroll_indicator: String,
    pub markdown: RenderedMarkdown,
    pub width: u32,
    pub title_fg: Color,
    pub border_fg: Color,
    pub indicator_fg: Color,
    /// Tab labels for the sidebar sub-tab bar.
    pub tab_labels: Vec<(String, bool)>,
    /// Foreground color for the active tab.
    pub tab_active_fg: Color,
    /// Foreground color for inactive tabs.
    pub tab_inactive_fg: Color,
    /// Optional meta header (pill badge + author line) for Overview tab.
    pub meta: Option<SidebarMeta>,
}

impl RenderedSidebar {
    /// Build a pre-rendered sidebar from markdown lines and display parameters.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        title: &str,
        lines: &[StyledLine],
        scroll_offset: usize,
        visible_lines: usize,
        width: u16,
        depth: ColorDepth,
        title_color: Option<AppColor>,
        border_color: Option<AppColor>,
        indicator_color: Option<AppColor>,
    ) -> Self {
        Self::build_tabbed(
            title,
            lines,
            scroll_offset,
            visible_lines,
            width,
            depth,
            title_color,
            border_color,
            indicator_color,
            None,
            None,
            None,
        )
    }

    /// Build a pre-rendered sidebar with optional tab bar.
    #[allow(clippy::too_many_arguments)]
    pub fn build_tabbed(
        title: &str,
        lines: &[StyledLine],
        scroll_offset: usize,
        visible_lines: usize,
        width: u16,
        depth: ColorDepth,
        title_color: Option<AppColor>,
        border_color: Option<AppColor>,
        indicator_color: Option<AppColor>,
        active_tab: Option<SidebarTab>,
        icons: Option<&ResolvedIcons>,
        meta: Option<SidebarMeta>,
    ) -> Self {
        let title_fg = title_color.map_or(Color::White, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let indicator_fg = indicator_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));

        let total = lines.len();
        let scroll_indicator = if total > visible_lines {
            let pos = if total == 0 {
                0
            } else {
                (scroll_offset * 100) / total.max(1)
            };
            format!("{pos}%")
        } else {
            String::new()
        };

        let markdown = RenderedMarkdown::build(lines, scroll_offset, visible_lines, depth);

        let tab_labels = if let Some(current) = active_tab {
            SidebarTab::ALL
                .iter()
                .map(|&t| {
                    let label = if let Some(ic) = icons {
                        t.icon_label(ic)
                    } else {
                        t.label().to_owned()
                    };
                    (label, t == current)
                })
                .collect()
        } else {
            Vec::new()
        };

        Self {
            title: title.to_owned(),
            scroll_indicator,
            markdown,
            width: u32::from(width),
            title_fg,
            border_fg,
            indicator_fg,
            tab_labels,
            tab_active_fg: title_fg,
            tab_inactive_fg: indicator_fg,
            meta,
        }
    }
}

// ---------------------------------------------------------------------------
// Sidebar component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct SidebarProps {
    pub sidebar: Option<RenderedSidebar>,
}

#[component]
pub fn Sidebar(props: &mut SidebarProps) -> impl Into<AnyElement<'static>> {
    let Some(sb) = props.sidebar.take() else {
        return element! { View }.into_any();
    };

    let has_indicator = !sb.scroll_indicator.is_empty();
    let has_tabs = !sb.tab_labels.is_empty();
    let meta = sb.meta;

    // Pre-build tab label contents for MixedText.
    let tab_contents: Vec<MixedTextContent> = sb
        .tab_labels
        .into_iter()
        .map(|(label, active)| {
            let color = if active {
                sb.tab_active_fg
            } else {
                sb.tab_inactive_fg
            };
            let weight = if active { Weight::Bold } else { Weight::Normal };
            MixedTextContent::new(format!(" {label} "))
                .color(color)
                .weight(weight)
        })
        .collect();

    element! {
        View(
            flex_direction: FlexDirection::Column,
            width: sb.width,
            border_style: BorderStyle::Single,
            border_edges: Edges::Left,
            border_color: sb.border_fg,
            padding_left: 1,
            padding_right: 1,
        ) {
            // Title bar
            View(margin_bottom: 1) {
                View(flex_grow: 1.0) {
                    Text(
                        content: sb.title,
                        color: sb.title_fg,
                        weight: Weight::Bold,
                        wrap: TextWrap::NoWrap,
                    )
                }
                #(if has_indicator {
                    Some(element! {
                        Text(
                            content: sb.scroll_indicator,
                            color: sb.indicator_fg,
                        )
                    })
                } else {
                    None
                })
            }

            // Tab labels (with bottom border separator when tabs are present)
            View(
                border_style: if has_tabs { BorderStyle::Single } else { BorderStyle::None },
                border_edges: Edges::Bottom,
                border_color: sb.border_fg,
            ) {
                    MixedText(contents: tab_contents, wrap: TextWrap::NoWrap)
            }

            // Meta section (pill badge + author line, Overview tab only)
            #(meta.map(|m| {
                let pill_label = format!(" {} {} ", m.pill_icon, m.pill_text);
                let branch_label = format!(" {}", m.branch_text);
                let author_contents = vec![
                    MixedTextContent::new(&m.author_text).color(m.author_fg),
                    MixedTextContent::new(" \u{b7} ").color(m.separator_fg),
                    MixedTextContent::new(&m.age_text).color(m.age_fg),
                    MixedTextContent::new(" \u{b7} ").color(m.separator_fg),
                    MixedTextContent::new(format!("{} {}", m.role_icon, m.role_text)).color(m.role_fg),
                ];
                let has_caps = !m.pill_left.is_empty();
                element! {
                    View(margin_top: 1, margin_bottom: 1, flex_direction: FlexDirection::Column) {
                        // Line 1: pill + branch
                        View(flex_direction: FlexDirection::Row) {
                            // Left cap (Powerline glyph, fg = pill color, no bg)
                            #(if has_caps {
                                Some(element! {
                                    Text(
                                        content: m.pill_left,
                                        color: m.pill_bg,
                                        wrap: TextWrap::NoWrap,
                                    )
                                })
                            } else {
                                None
                            })
                            View(background_color: m.pill_bg) {
                                Text(
                                    content: pill_label,
                                    color: m.pill_fg,
                                    weight: Weight::Bold,
                                    wrap: TextWrap::NoWrap,
                                )
                            }
                            // Right cap (Powerline glyph, fg = pill color, no bg)
                            #(if has_caps {
                                Some(element! {
                                    Text(
                                        content: m.pill_right,
                                        color: m.pill_bg,
                                        wrap: TextWrap::NoWrap,
                                    )
                                })
                            } else {
                                None
                            })
                            Text(content: branch_label, color: m.branch_fg, wrap: TextWrap::NoWrap)
                        }
                        // Line 2: by @author · age · role
                        View(margin_top: 1) {
                            MixedText(contents: author_contents, wrap: TextWrap::NoWrap)
                        }
                    }
                }
            }))

            // Content area
            View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                MarkdownView(markdown: sb.markdown)
            }
        }
    }
    .into_any()
}
