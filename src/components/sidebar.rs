use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};
use crate::components::markdown_view::{MarkdownView, RenderedMarkdown};
use crate::components::scrollbar::{ScrollInfo, Scrollbar};
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
    // Update status (on pill line, after branch)
    pub update_text: Option<String>,
    pub update_fg: Color,
    // Author metadata line
    pub author_login: String,
    pub role_icon: String,
    pub role_text: String,
    pub role_fg: Color,
    // Metadata label color (matches Labels:/Lines: style)
    pub label_fg: Color,
    // Participants line
    pub participants: Vec<String>,
    pub participants_fg: Color,

    // Overview metadata (pinned, non-scrollable)
    pub labels_text: Option<String>,
    pub assignees_text: Option<String>,
    pub created_text: String,
    pub created_age: String,
    pub updated_text: String,
    pub updated_age: String,
    pub lines_added: Option<String>,
    pub lines_deleted: Option<String>,
    pub reactions_text: Option<String>,
    pub date_fg: Color,
    pub date_age_fg: Color,
    pub additions_fg: Color,
    pub deletions_fg: Color,
    pub separator_fg: Color,
    pub primary_fg: Color,
    pub actor_fg: Color,
    pub reactions_fg: Color,
}

impl SidebarMeta {
    /// Number of fixed lines rendered in the meta section.
    ///
    /// Base: pill(1) + author(1) = 2, plus optional participants(1).
    /// Plus overview metadata: created(1) + updated(1) + separator(1) = 3,
    /// plus optional labels(1), assignees(1), lines(1), reactions(1).
    /// We also account for `margin_top: 1` on each sub-group.
    pub fn line_count(&self) -> u32 {
        // outer margin_top(1) + pill(1) + author margin_top(1) + author(1) = 4
        let mut count: u32 = 4;
        if self.participants.len() > 1 {
            count += 1;
        }
        // Overview metadata: created + updated + separator = 3
        count += 3;
        if self.labels_text.is_some() {
            count += 1;
        }
        if self.assignees_text.is_some() {
            count += 1;
        }
        if self.lines_added.is_some() {
            count += 1;
        }
        if self.reactions_text.is_some() {
            count += 1;
        }
        count
    }
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
    /// Scroll metadata for the scrollbar (None when content fits).
    pub scroll_info: Option<ScrollInfo>,
    /// Height of the scrollbar track in rows.
    pub track_height: u32,
    /// Scrollbar track color.
    pub scrollbar_track_fg: Color,
    /// Scrollbar thumb color.
    pub scrollbar_thumb_fg: Color,
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
        visible_tabs: Option<&[SidebarTab]>,
    ) -> Self {
        let title_fg = title_color.map_or(Color::White, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let indicator_fg = indicator_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));

        // Estimate visual row count: each logical line may wrap to multiple
        // terminal rows. Content width = sidebar width minus left border (1) +
        // padding (2) + scrollbar (1).
        let content_width = usize::from(width).saturating_sub(4).max(1);
        let visual_row_count = |line: &StyledLine| -> usize {
            let w = line.display_width();
            if w == 0 { 1 } else { w.div_ceil(content_width) }
        };

        let visual_total: usize = lines.iter().map(&visual_row_count).sum();
        let visual_offset: usize = lines[..scroll_offset.min(lines.len())]
            .iter()
            .map(visual_row_count)
            .sum();

        let scroll_indicator = if visual_total > visible_lines {
            let pos = if visual_total == 0 {
                0
            } else {
                (visual_offset * 100) / visual_total.max(1)
            };
            format!("{pos}%")
        } else {
            String::new()
        };

        let markdown = RenderedMarkdown::build(lines, scroll_offset, visible_lines, depth);

        let tabs_to_show = visible_tabs.unwrap_or(SidebarTab::ALL);
        let tab_labels = if let Some(current) = active_tab {
            tabs_to_show
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

        // Scroll metadata for the scrollbar.
        let scroll_info = ScrollInfo {
            scroll_offset: visual_offset,
            visible_count: visible_lines,
            total_count: visual_total,
        };
        let scroll_info = if scroll_info.needs_scrollbar() {
            Some(scroll_info)
        } else {
            None
        };

        #[allow(clippy::cast_possible_truncation)]
        let track_height = visible_lines as u32;

        Self {
            title: crate::util::expand_emoji(title).into_owned(),
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
            scroll_info,
            track_height,
            scrollbar_track_fg: border_fg,
            scrollbar_thumb_fg: title_fg,
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

    let has_tabs = !sb.tab_labels.is_empty();
    let meta = sb.meta;
    let scroll_info = sb.scroll_info;
    let has_scrollbar = scroll_info.is_some();
    let track_height = sb.track_height;
    let track_color = sb.scrollbar_track_fg;
    let thumb_color = sb.scrollbar_thumb_fg;

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
            }

            // Tab labels (with bottom border separator when tabs are present)
            View(
                border_style: if has_tabs { BorderStyle::Single } else { BorderStyle::None },
                border_edges: Edges::Bottom,
                border_color: sb.border_fg,
            ) {
                    MixedText(contents: tab_contents, wrap: TextWrap::NoWrap)
            }

            // Meta section (pill badge + author + participants + overview metadata, Overview tab only)
            #(meta.map(|m| {
                let pill_label = format!(" {} {} ", m.pill_icon, m.pill_text);
                let branch_label = format!(" {}", m.branch_text);
                let has_caps = !m.pill_left.is_empty();
                let show_participants = m.participants.len() > 1;
                let participants_text = m.participants.join(", ");
                let participants_fg = m.participants_fg;
                let label_fg = m.label_fg;
                let has_update = m.update_text.is_some();
                let update_label = m.update_text.map(|t| format!(" {t}")).unwrap_or_default();
                let update_fg = m.update_fg;
                let author_text = format!("@{}", m.author_login);
                let role_suffix = if m.role_text.is_empty() {
                    String::new()
                } else {
                    format!("  {} {}", m.role_icon, m.role_text)
                };
                let role_fg = m.role_fg;

                // Overview metadata fields
                let has_labels = m.labels_text.is_some();
                let labels_text = m.labels_text.unwrap_or_default();
                let has_assignees = m.assignees_text.is_some();
                let assignees_text = m.assignees_text.unwrap_or_default();
                let created_label = format!(" {} ", m.created_text);
                let created_age_label = format!("({})", m.created_age);
                let updated_label = format!(" {} ", m.updated_text);
                let updated_age_label = format!("({})", m.updated_age);
                let has_lines = m.lines_added.is_some();
                let lines_added = m.lines_added.unwrap_or_default();
                let lines_deleted = m.lines_deleted.unwrap_or_default();
                let has_reactions = m.reactions_text.is_some();
                let reactions_text = m.reactions_text.unwrap_or_default();
                let date_fg = m.date_fg;
                let date_age_fg = m.date_age_fg;
                let additions_fg = m.additions_fg;
                let deletions_fg = m.deletions_fg;
                let separator_fg = m.separator_fg;
                let primary_fg = m.primary_fg;
                let actor_fg = m.actor_fg;
                let reactions_fg = m.reactions_fg;
                let separator = "\u{2500}".repeat(20);

                element! {
                    View(margin_top: 1, flex_direction: FlexDirection::Column) {
                        // Line 1: pill + branch + update status
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
                            #(if has_update {
                                Some(element! {
                                    Text(
                                        content: update_label,
                                        color: update_fg,
                                        wrap: TextWrap::NoWrap,
                                    )
                                })
                            } else {
                                None
                            })
                        }
                        // Line 2: Author + role badge
                        View(margin_top: 1) {
                            MixedText(
                                contents: vec![
                                    MixedTextContent::new("Author:  ")
                                        .color(label_fg)
                                        .weight(Weight::Bold),
                                    MixedTextContent::new(author_text)
                                        .color(participants_fg),
                                    MixedTextContent::new(role_suffix)
                                        .color(role_fg),
                                ],
                                wrap: TextWrap::NoWrap,
                            )
                        }
                        // Line 3: Participants (only if > 1)
                        #(if show_participants {
                            Some(element! {
                                View(margin_top: 0) {
                                    MixedText(
                                        contents: vec![
                                            MixedTextContent::new("Participants: ")
                                                .color(label_fg)
                                                .weight(Weight::Bold),
                                            MixedTextContent::new(participants_text)
                                                .color(participants_fg),
                                        ],
                                        wrap: TextWrap::NoWrap,
                                    )
                                }
                            })
                        } else {
                            None
                        })

                        // Labels (optional)
                        #(if has_labels {
                            Some(element! {
                                View {
                                    MixedText(
                                        contents: vec![
                                            MixedTextContent::new("Labels: ")
                                                .color(label_fg)
                                                .weight(Weight::Bold),
                                            MixedTextContent::new(labels_text)
                                                .color(primary_fg),
                                        ],
                                        wrap: TextWrap::NoWrap,
                                    )
                                }
                            })
                        } else {
                            None
                        })
                        // Assignees (optional)
                        #(if has_assignees {
                            Some(element! {
                                View {
                                    MixedText(
                                        contents: vec![
                                            MixedTextContent::new("Assign: ")
                                                .color(label_fg)
                                                .weight(Weight::Bold),
                                            MixedTextContent::new(assignees_text)
                                                .color(actor_fg),
                                        ],
                                        wrap: TextWrap::NoWrap,
                                    )
                                }
                            })
                        } else {
                            None
                        })
                        // Created
                        View {
                            MixedText(
                                contents: vec![
                                    MixedTextContent::new("Created:")
                                        .color(label_fg)
                                        .weight(Weight::Bold),
                                    MixedTextContent::new(created_label)
                                        .color(date_fg),
                                    MixedTextContent::new(created_age_label)
                                        .color(date_age_fg),
                                ],
                                wrap: TextWrap::NoWrap,
                            )
                        }
                        // Updated
                        View {
                            MixedText(
                                contents: vec![
                                    MixedTextContent::new("Updated:")
                                        .color(label_fg)
                                        .weight(Weight::Bold),
                                    MixedTextContent::new(updated_label)
                                        .color(date_fg),
                                    MixedTextContent::new(updated_age_label)
                                        .color(date_age_fg),
                                ],
                                wrap: TextWrap::NoWrap,
                            )
                        }
                        // Lines changed (optional, PRs only)
                        #(if has_lines {
                            Some(element! {
                                View {
                                    MixedText(
                                        contents: vec![
                                            MixedTextContent::new("Lines:  ")
                                                .color(label_fg)
                                                .weight(Weight::Bold),
                                            MixedTextContent::new(lines_added)
                                                .color(additions_fg),
                                            MixedTextContent::new(" / ")
                                                .color(date_fg),
                                            MixedTextContent::new(lines_deleted)
                                                .color(deletions_fg),
                                        ],
                                        wrap: TextWrap::NoWrap,
                                    )
                                }
                            })
                        } else {
                            None
                        })
                        // Reactions (optional, issues only)
                        #(if has_reactions {
                            Some(element! {
                                View {
                                    MixedText(
                                        contents: vec![
                                            MixedTextContent::new("React:  ")
                                                .color(label_fg)
                                                .weight(Weight::Bold),
                                            MixedTextContent::new(reactions_text)
                                                .color(reactions_fg),
                                        ],
                                        wrap: TextWrap::NoWrap,
                                    )
                                }
                            })
                        } else {
                            None
                        })
                        // Separator
                        View {
                            Text(content: separator, color: separator_fg, wrap: TextWrap::NoWrap)
                        }
                    }
                }
            }))

            // Content area with scrollbar
            View(flex_grow: 1.0, flex_direction: FlexDirection::Row) {
                View(flex_grow: 1.0, flex_direction: FlexDirection::Column,
                     margin_right: u32::from(has_scrollbar)) {
                    MarkdownView(markdown: sb.markdown)
                }
                Scrollbar(
                    scroll_info: scroll_info,
                    track_height: track_height,
                    track_color: track_color,
                    thumb_color: thumb_color,
                )
            }
        }
    }
    .into_any()
}
