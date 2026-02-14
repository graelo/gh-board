use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};
use crate::config::keybindings::{Keybinding, MergedBindings, ViewContext};

// ---------------------------------------------------------------------------
// Help overlay (T066 — FR-104)
// ---------------------------------------------------------------------------

/// A single row in the help overlay.
#[allow(dead_code)]
struct HelpRow {
    key: String,
    description: String,
}

/// A group of keybindings under a section header.
#[allow(dead_code)]
struct HelpGroup {
    title: String,
    rows: Vec<HelpRow>,
}

/// Pre-rendered help overlay data (owned, 'static-safe).
pub struct RenderedHelpOverlay {
    pub groups: Vec<RenderedHelpGroup>,
    pub title_fg: Color,
    pub key_fg: Color,
    pub desc_fg: Color,
    pub border_fg: Color,
    pub bg: Color,
}

pub struct RenderedHelpGroup {
    pub title: String,
    pub rows: Vec<RenderedHelpRow>,
}

pub struct RenderedHelpRow {
    pub key: String,
    pub description: String,
}

/// Configuration for building a help overlay.
pub struct HelpOverlayBuildConfig<'a> {
    pub bindings: &'a MergedBindings,
    pub context: ViewContext,
    pub depth: ColorDepth,
    pub title_color: Option<AppColor>,
    pub key_color: Option<AppColor>,
    pub desc_color: Option<AppColor>,
    pub border_color: Option<AppColor>,
    pub bg_color: Option<AppColor>,
}

#[allow(dead_code)]
impl RenderedHelpOverlay {
    /// Build the help overlay for a given view context.
    pub fn build(cfg: &HelpOverlayBuildConfig<'_>) -> Self {
        let groups = build_help_groups(cfg.bindings, cfg.context);

        let title_fg = cfg
            .title_color
            .map_or(Color::White, |c| c.to_crossterm_color(cfg.depth));
        let key_fg = cfg
            .key_color
            .map_or(Color::Cyan, |c| c.to_crossterm_color(cfg.depth));
        let desc_fg = cfg
            .desc_color
            .map_or(Color::Grey, |c| c.to_crossterm_color(cfg.depth));
        let border_fg = cfg
            .border_color
            .map_or(Color::DarkGrey, |c| c.to_crossterm_color(cfg.depth));
        let bg = cfg
            .bg_color
            .map_or(Color::Black, |c| c.to_crossterm_color(cfg.depth));

        let rendered_groups = groups
            .into_iter()
            .map(|g| RenderedHelpGroup {
                title: g.title,
                rows: g
                    .rows
                    .into_iter()
                    .map(|r| RenderedHelpRow {
                        key: r.key,
                        description: r.description,
                    })
                    .collect(),
            })
            .collect();

        Self {
            groups: rendered_groups,
            title_fg,
            key_fg,
            desc_fg,
            border_fg,
            bg,
        }
    }
}

#[allow(dead_code)]
fn build_help_groups(bindings: &MergedBindings, context: ViewContext) -> Vec<HelpGroup> {
    let sections = bindings.all_for_context(context);
    let mut groups = Vec::new();

    for (label, keybindings) in sections {
        let mut rows: Vec<HelpRow> = Vec::new();

        // Group bindings by action to combine keys (e.g., "j / Down" → Move down).
        let mut seen_actions: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for kb in keybindings {
            let desc = kb
                .name
                .clone()
                .unwrap_or_else(|| description_for_keybinding(kb));

            let action_key = desc.clone();
            if let Some(&idx) = seen_actions.get(&action_key) {
                // Combine keys: "j" + "down" → "j / down"
                rows[idx].key = format!("{} / {}", rows[idx].key, format_key_display(&kb.key));
            } else {
                seen_actions.insert(action_key, rows.len());
                rows.push(HelpRow {
                    key: format_key_display(&kb.key),
                    description: desc,
                });
            }
        }

        if !rows.is_empty() {
            groups.push(HelpGroup {
                title: label.to_owned(),
                rows,
            });
        }
    }

    groups
}

/// Format a key string for display (capitalize special keys).
#[allow(dead_code)]
fn format_key_display(key: &str) -> String {
    match key {
        "space" => "Space".to_owned(),
        "enter" => "Enter".to_owned(),
        "esc" => "Esc".to_owned(),
        "delete" => "Delete".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "pageup" => "PgUp".to_owned(),
        "pagedown" => "PgDn".to_owned(),
        "up" => "\u{2191}".to_owned(),    // ↑
        "down" => "\u{2193}".to_owned(),  // ↓
        "left" => "\u{2190}".to_owned(),  // ←
        "right" => "\u{2192}".to_owned(), // →
        "home" => "Home".to_owned(),
        "end" => "End".to_owned(),
        "tab" => "Tab".to_owned(),
        s if s.starts_with("ctrl+") => format!("Ctrl+{}", &s[5..]),
        s if s.starts_with("alt+") => format!("Alt+{}", &s[4..]),
        s => s.to_owned(),
    }
}

#[allow(dead_code)]
fn description_for_keybinding(kb: &Keybinding) -> String {
    if let Some(ref builtin) = kb.builtin {
        use crate::config::keybindings::BuiltinAction;
        if let Some(action) = BuiltinAction::from_name(builtin) {
            return action.description().to_owned();
        }
    }
    if let Some(ref cmd) = kb.command {
        return format!("Run: {cmd}");
    }
    "(unbound)".to_owned()
}

// ---------------------------------------------------------------------------
// HelpOverlay component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct HelpOverlayProps {
    pub overlay: Option<RenderedHelpOverlay>,
    pub width: u16,
    pub height: u16,
}

#[component]
pub fn HelpOverlay(props: &mut HelpOverlayProps) -> impl Into<AnyElement<'static>> {
    let Some(overlay) = props.overlay.take() else {
        return element! { View }.into_any();
    };

    let width = u32::from(props.width);
    let height = u32::from(props.height);

    // Calculate overlay dimensions: centered, ~60% width, up to 80% height.
    let overlay_width = (width * 3 / 5).max(40).min(width.saturating_sub(4));
    let overlay_height = (height * 4 / 5).max(10).min(height.saturating_sub(2));
    let pad_left = (width.saturating_sub(overlay_width)) / 2;
    let pad_top = (height.saturating_sub(overlay_height)) / 2;

    // Build content lines as a flat list of elements.
    // We flatten the groups into a single column of Text elements.
    let mut lines: Vec<(String, Color, Weight)> = Vec::new();
    lines.push(("Keybindings".to_owned(), overlay.title_fg, Weight::Bold));
    lines.push((String::new(), overlay.desc_fg, Weight::Normal));

    for group in &overlay.groups {
        lines.push((format!("  {}", group.title), overlay.title_fg, Weight::Bold));
        for row in &group.rows {
            let line = format!("    {:>14}  {}", row.key, row.description);
            lines.push((line, overlay.desc_fg, Weight::Normal));
        }
        lines.push((String::new(), overlay.desc_fg, Weight::Normal));
    }

    lines.push((
        "  Press ? to close".to_owned(),
        overlay.key_fg,
        Weight::Normal,
    ));

    // Truncate if too many lines.
    let max_lines = overlay_height.saturating_sub(2) as usize;
    lines.truncate(max_lines);

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
                border_style: BorderStyle::Double,
                border_color: overlay.border_fg,
                background_color: overlay.bg,
                padding_left: 1,
                padding_right: 1,
                overflow: Overflow::Hidden,
            ) {
                #(lines.into_iter().map(|(text, color, weight)| {
                    element! {
                        Text(content: text, color, weight, wrap: TextWrap::NoWrap)
                    }
                }))
            }
        }
    }
    .into_any()
}
