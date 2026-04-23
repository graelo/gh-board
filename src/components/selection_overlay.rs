use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};

// ---------------------------------------------------------------------------
// Selection overlay (T011 — deep-link disambiguation)
// ---------------------------------------------------------------------------

/// A single selectable item in the overlay.
pub struct SelectionOverlayItem {
    pub label: String,
}

/// Pre-rendered overlay data (owned, 'static-safe).
pub struct RenderedSelectionOverlay {
    pub title: String,
    pub items: Vec<SelectionOverlayItem>,
    pub cursor: usize,
    pub cursor_marker: String,
    pub show_filter: bool,
    pub filter_text: String,
    pub title_fg: Color,
    pub item_fg: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
    pub border_fg: Color,
    pub hint_fg: Color,
    pub filter_prompt_fg: Color,
    pub filter_text_fg: Color,
}

/// Configuration for building a selection overlay.
pub struct SelectionOverlayBuildConfig {
    pub title: String,
    pub items: Vec<SelectionOverlayItem>,
    pub cursor: usize,
    /// Show the filter input row. When `false`, the filter row is hidden and
    /// the overlay is more compact (useful for small pickers).
    pub show_filter: bool,
    pub filter_text: String,
    pub depth: ColorDepth,
    pub title_color: Option<AppColor>,
    pub item_color: Option<AppColor>,
    pub cursor_color: Option<AppColor>,
    pub selected_bg: Option<AppColor>,
    pub border_color: Option<AppColor>,
    pub hint_color: Option<AppColor>,
    pub filter_prompt_color: Option<AppColor>,
    pub filter_text_color: Option<AppColor>,
    pub cursor_marker: String,
}

impl RenderedSelectionOverlay {
    /// Build a selection overlay with themed colors.
    pub fn build(cfg: SelectionOverlayBuildConfig) -> Self {
        let depth = cfg.depth;
        Self {
            title: cfg.title,
            items: cfg.items,
            cursor: cfg.cursor,
            cursor_marker: cfg.cursor_marker,
            show_filter: cfg.show_filter,
            filter_text: cfg.filter_text,
            title_fg: cfg
                .title_color
                .map_or(Color::White, |c| c.to_crossterm_color(depth)),
            item_fg: cfg
                .item_color
                .map_or(Color::Grey, |c| c.to_crossterm_color(depth)),
            cursor_fg: cfg
                .cursor_color
                .map_or(Color::White, |c| c.to_crossterm_color(depth)),
            cursor_bg: cfg
                .selected_bg
                .map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth)),
            border_fg: cfg
                .border_color
                .map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth)),
            hint_fg: cfg
                .hint_color
                .map_or(Color::Grey, |c| c.to_crossterm_color(depth)),
            filter_prompt_fg: cfg
                .filter_prompt_color
                .map_or(Color::Cyan, |c| c.to_crossterm_color(depth)),
            filter_text_fg: cfg
                .filter_text_color
                .map_or(Color::White, |c| c.to_crossterm_color(depth)),
        }
    }
}

// ---------------------------------------------------------------------------
// SelectionOverlay component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct SelectionOverlayProps {
    pub overlay: Option<RenderedSelectionOverlay>,
    pub width: u16,
    pub height: u16,
}

#[component]
pub fn SelectionOverlay(props: &mut SelectionOverlayProps) -> impl Into<AnyElement<'static>> {
    let Some(overlay) = props.overlay.take() else {
        return element! { View }.into_any();
    };

    let width = u32::from(props.width);
    let height = u32::from(props.height);

    // Overlay dimensions: centered, ~40% width, height fits items.
    // Base overhead: outer border (2) + title text (1) + title ─── (1) = 4
    // With filter:   + filter ─── (1) + filter text (1) = 6
    let filter_overhead: u32 = if overlay.show_filter { 2 } else { 0 };
    #[expect(clippy::cast_possible_truncation)]
    let content_height = (overlay.items.len() as u32) + 4 + filter_overhead;
    let overlay_width = (width * 2 / 5).max(30).min(width.saturating_sub(4));
    let overlay_height = content_height.min(height.saturating_sub(2));
    let pad_left = (width.saturating_sub(overlay_width)) / 2;
    let pad_top = (height.saturating_sub(overlay_height)) / 2;

    let cursor_marker_str = format!("{} ", overlay.cursor_marker);
    let show_filter = overlay.show_filter;
    let filter_display = format!("{}\u{2588}", overlay.filter_text); // append block cursor █
    let has_filter_text = !overlay.filter_text.is_empty();
    let hint = if has_filter_text {
        "Type to filter"
    } else {
        "j/k Enter Esc"
    };

    element! {
        View(
            width,
            height,
            position: Position::Absolute,
        ) {
            View(
                margin_left: pad_left,
                margin_top: pad_top,
                width: overlay_width,
                height: overlay_height,
                flex_direction: FlexDirection::Column,
                border_style: BorderStyle::Round,
                border_color: overlay.border_fg,
                background_color: Color::Reset,
                overflow: Overflow::Hidden,
            ) {
                // Title row (sidebar-style: bold title + spacer + hint)
                View(
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: overlay.border_fg,
                    padding_left: 1,
                    padding_right: 1,
                ) {
                    Text(
                        content: overlay.title,
                        color: overlay.title_fg,
                        weight: Weight::Bold,
                        wrap: TextWrap::NoWrap,
                    )
                    View(flex_grow: 1.0_f32)
                    Text(
                        content: hint,
                        color: overlay.hint_fg,
                        wrap: TextWrap::NoWrap,
                    )
                }

                // Items
                View(
                    flex_grow: 1.0_f32,
                    flex_direction: FlexDirection::Column,
                    padding_left: 1,
                    padding_right: 1,
                    overflow: Overflow::Hidden,
                ) {
                    #(overlay.items.into_iter().enumerate().map(|(i, item)| {
                        let is_selected = i == overlay.cursor;
                        let fg = if is_selected { overlay.cursor_fg } else { overlay.item_fg };
                        let bg = if is_selected { overlay.cursor_bg } else { Color::Reset };
                        let marker = if is_selected { cursor_marker_str.as_str() } else { "  " };
                        element! {
                            View(key: i, background_color: bg) {
                                Text(
                                    content: format!("{marker}{}", item.label),
                                    color: fg,
                                    wrap: TextWrap::NoWrap,
                                )
                            }
                        }.into_any()
                    }))
                }

                // Filter input (sidebar-style: top-border separator + prompt)
                #(if show_filter {
                    Some(element! {
                        View(
                            border_style: BorderStyle::Single,
                            border_edges: Edges::Top,
                            border_color: overlay.border_fg,
                            padding_left: 1,
                            padding_right: 1,
                        ) {
                            MixedText(
                                contents: vec![
                                    MixedTextContent::new("Filter: ").color(overlay.filter_prompt_fg),
                                    MixedTextContent::new(filter_display).color(overlay.filter_text_fg),
                                ],
                                wrap: TextWrap::NoWrap,
                            )
                        }
                    })
                } else {
                    None
                })
            }
        }
    }
    .into_any()
}
