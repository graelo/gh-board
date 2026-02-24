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
    pub title_fg: Color,
    pub item_fg: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
    pub border_fg: Color,
    pub hint_fg: Color,
}

/// Configuration for building a selection overlay.
pub struct SelectionOverlayBuildConfig {
    pub title: String,
    pub items: Vec<SelectionOverlayItem>,
    pub cursor: usize,
    pub depth: ColorDepth,
    pub title_color: Option<AppColor>,
    pub item_color: Option<AppColor>,
    pub cursor_color: Option<AppColor>,
    pub selected_bg: Option<AppColor>,
    pub border_color: Option<AppColor>,
    pub hint_color: Option<AppColor>,
}

impl RenderedSelectionOverlay {
    /// Build a selection overlay with themed colors.
    pub fn build(cfg: SelectionOverlayBuildConfig) -> Self {
        Self {
            title: cfg.title,
            items: cfg.items,
            cursor: cfg.cursor,
            title_fg: cfg
                .title_color
                .map_or(Color::White, |c| c.to_crossterm_color(cfg.depth)),
            item_fg: cfg
                .item_color
                .map_or(Color::Grey, |c| c.to_crossterm_color(cfg.depth)),
            cursor_fg: cfg
                .cursor_color
                .map_or(Color::White, |c| c.to_crossterm_color(cfg.depth)),
            cursor_bg: cfg
                .selected_bg
                .map_or(Color::DarkGrey, |c| c.to_crossterm_color(cfg.depth)),
            border_fg: cfg
                .border_color
                .map_or(Color::DarkGrey, |c| c.to_crossterm_color(cfg.depth)),
            hint_fg: cfg
                .hint_color
                .map_or(Color::Grey, |c| c.to_crossterm_color(cfg.depth)),
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
    #[allow(clippy::cast_possible_truncation)]
    let content_height = (overlay.items.len() as u32) + 4; // border + title + hint + items
    let overlay_width = (width * 2 / 5).max(30).min(width.saturating_sub(4));
    let overlay_height = content_height.min(height.saturating_sub(2));
    let pad_left = (width.saturating_sub(overlay_width)) / 2;
    let pad_top = (height.saturating_sub(overlay_height)) / 2;

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
                // Title row
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
                    View(flex_grow: 1.0)
                    Text(
                        content: "j/k Enter Esc",
                        color: overlay.hint_fg,
                        wrap: TextWrap::NoWrap,
                    )
                }

                // Items
                View(
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    padding_left: 1,
                    padding_right: 1,
                    overflow: Overflow::Hidden,
                ) {
                    #(overlay.items.into_iter().enumerate().map(|(i, item)| {
                        let is_selected = i == overlay.cursor;
                        let fg = if is_selected { overlay.cursor_fg } else { overlay.item_fg };
                        let bg = if is_selected { overlay.cursor_bg } else { Color::Reset };
                        let marker = if is_selected { "\u{25b6} " } else { "  " };
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
            }
        }
    }
    .into_any()
}
