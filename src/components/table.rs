use std::collections::{HashMap, HashSet};

use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};

// ---------------------------------------------------------------------------
// Column definition
// ---------------------------------------------------------------------------

/// Defines a column in the table.
#[derive(Debug, Clone)]
pub struct Column {
    /// Unique identifier (e.g., "title", "author", "state").
    pub id: String,
    /// Display header text.
    pub header: String,
    /// Default width as a fraction of total width (0.0..1.0).
    pub default_width_pct: f32,
    /// Text alignment for this column.
    pub align: TextAlign,
}

/// A single styled fragment within a cell.
#[derive(Debug, Clone)]
pub struct Span {
    pub text: String,
    pub color: Option<AppColor>,
    pub bold: bool,
}

/// A cell value composed of one or more styled spans.
#[derive(Debug, Clone)]
pub struct Cell {
    pub spans: Vec<Span>,
}

impl Cell {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            spans: vec![Span {
                text: text.into(),
                color: None,
                bold: false,
            }],
        }
    }

    pub fn colored(text: impl Into<String>, color: AppColor) -> Self {
        Self {
            spans: vec![Span {
                text: text.into(),
                color: Some(color),
                bold: false,
            }],
        }
    }

    pub fn bold(text: impl Into<String>) -> Self {
        Self {
            spans: vec![Span {
                text: text.into(),
                color: None,
                bold: true,
            }],
        }
    }

    pub fn from_spans(spans: Vec<Span>) -> Self {
        Self { spans }
    }

    /// Concatenate all span texts into a single string (for filtering).
    pub fn text(&self) -> String {
        let mut s = String::new();
        for span in &self.spans {
            s.push_str(&span.text);
        }
        s
    }
}

/// A complete row of cells indexed by column id.
pub type Row = HashMap<String, Cell>;

// ---------------------------------------------------------------------------
// Pre-rendered table data (all owned)
// ---------------------------------------------------------------------------

/// Pre-render table data into fully owned structures that can be passed
/// into the `element!` macro without lifetime issues.
pub struct RenderedTable {
    pub header_cells: Vec<HeaderCell>,
    pub body_rows: Vec<RenderedRow>,
    pub total_width: u32,
    pub show_separator: bool,
    pub header_fg: Color,
    pub border_fg: Color,
    /// Message to display when there are no rows.
    pub empty_message: Option<String>,
}

pub struct HeaderCell {
    pub text: String,
    pub width: u32,
    pub align: TextAlign,
}

pub struct RenderedRow {
    pub key: usize,
    pub bg: Option<Color>,
    pub cells: Vec<RenderedCell>,
    /// Optional subtitle line rendered below the cells (full row width).
    pub subtitle: Option<RenderedCell>,
}

pub struct RenderedSpan {
    pub text: String,
    pub fg: Color,
    pub weight: Weight,
}

pub struct RenderedCell {
    pub spans: Vec<RenderedSpan>,
    pub width: u32,
    pub align: TextAlign,
}

/// Configuration for building a `RenderedTable`.
pub struct TableBuildConfig<'a> {
    pub columns: &'a [Column],
    pub rows: &'a [Row],
    pub cursor: usize,
    pub scroll_offset: usize,
    pub visible_rows: usize,
    pub hidden_columns: Option<&'a HashSet<String>>,
    pub width_overrides: Option<&'a HashMap<String, u16>>,
    pub total_width: u16,
    pub depth: ColorDepth,
    pub selected_bg: Option<AppColor>,
    pub header_color: Option<AppColor>,
    pub border_color: Option<AppColor>,
    pub show_separator: bool,
    /// Message to show when rows are empty.
    pub empty_message: Option<&'a str>,
    /// Column ID whose cell is extracted as a subtitle line below the row.
    pub subtitle_column: Option<&'a str>,
}

impl RenderedTable {
    /// Build a `RenderedTable` from a configuration.
    pub fn build(cfg: &TableBuildConfig<'_>) -> Self {
        let columns = cfg.columns;
        let rows = cfg.rows;
        let cursor = cfg.cursor;
        let scroll_offset = cfg.scroll_offset;
        let visible_rows = cfg.visible_rows;
        let hidden_columns = cfg.hidden_columns;
        let width_overrides = cfg.width_overrides;
        let total_width = cfg.total_width;
        let depth = cfg.depth;
        let selected_bg = cfg.selected_bg;
        let header_color = cfg.header_color;
        let border_color = cfg.border_color;
        let show_separator = cfg.show_separator;
        // Filter out hidden columns.
        let visible_columns: Vec<&Column> = columns
            .iter()
            .filter(|c| hidden_columns.is_none_or(|h| !h.contains(&c.id)))
            .collect();

        // Compute column widths.
        let col_widths = compute_column_widths(&visible_columns, width_overrides, total_width);

        let header_fg = header_color.map_or(Color::White, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let selected_bg_color = selected_bg.map(|c| c.to_crossterm_color(depth));

        // Build header cells.
        let header_cells: Vec<HeaderCell> = visible_columns
            .iter()
            .zip(col_widths.iter())
            .map(|(col, &w)| HeaderCell {
                text: col.header.clone(),
                width: u32::from(w),
                align: col.align,
            })
            .collect();

        // Build body rows.
        let end = (scroll_offset + visible_rows).min(rows.len());
        let visible_slice = if scroll_offset < rows.len() {
            &rows[scroll_offset..end]
        } else {
            &[]
        };

        let subtitle_column = cfg.subtitle_column;

        let body_rows: Vec<RenderedRow> = visible_slice
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let absolute_idx = scroll_offset + i;
                let is_selected = absolute_idx == cursor;
                let bg = if is_selected { selected_bg_color } else { None };

                let cells: Vec<RenderedCell> = visible_columns
                    .iter()
                    .zip(col_widths.iter())
                    .map(|(col, &w)| {
                        let cell = row.get(&col.id);
                        let spans = cell.map_or_else(
                            || {
                                vec![RenderedSpan {
                                    text: String::new(),
                                    fg: Color::Reset,
                                    weight: Weight::Normal,
                                }]
                            },
                            |c| render_spans(&c.spans, depth),
                        );
                        RenderedCell {
                            spans,
                            width: u32::from(w),
                            align: col.align,
                        }
                    })
                    .collect();

                // Extract subtitle cell if configured.
                let subtitle = subtitle_column.and_then(|col_id| {
                    row.get(col_id).map(|cell| {
                        let spans = render_spans(&cell.spans, depth);
                        RenderedCell {
                            spans,
                            width: u32::from(total_width),
                            align: TextAlign::Left,
                        }
                    })
                });

                RenderedRow {
                    key: absolute_idx,
                    bg,
                    cells,
                    subtitle,
                }
            })
            .collect();

        let empty_message = if rows.is_empty() {
            cfg.empty_message.map(String::from)
        } else {
            None
        };

        Self {
            header_cells,
            body_rows,
            total_width: u32::from(total_width),
            show_separator,
            header_fg,
            border_fg,
            empty_message,
        }
    }
}

// ---------------------------------------------------------------------------
// ScrollableTable component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct ScrollableTableProps {
    /// Pre-rendered table data.
    pub table: Option<RenderedTable>,
}

#[component]
pub fn ScrollableTable(props: &mut ScrollableTableProps) -> impl Into<AnyElement<'static>> {
    let Some(table) = props.table.take() else {
        return element! { View }.into_any();
    };

    element! {
        View(flex_direction: FlexDirection::Column, width: table.total_width) {
            // Header row
            View(
                border_style: if table.show_separator { BorderStyle::Single } else { BorderStyle::None },
                border_edges: Edges::Bottom,
                border_color: table.border_fg,
            ) {
                #(table.header_cells.into_iter().enumerate().map(|(i, hc)| {
                    element! {
                        View(key: i, width: hc.width) {
                            Text(
                                content: hc.text,
                                weight: Weight::Bold,
                                color: table.header_fg,
                                wrap: TextWrap::NoWrap,
                                align: hc.align,
                            )
                        }
                    }
                }))
            }

            // Empty-state message or body rows
            #(table.empty_message.into_iter().map(|msg| {
                element! {
                    View(padding_top: 1, padding_left: 2) {
                        Text(
                            content: msg,
                            color: Color::DarkGrey,
                        )
                    }
                }
            }))
            #(table.body_rows.into_iter().map(|row| {
                let subtitle_elem = row.subtitle.map(|sub| {
                    let content: String = sub.spans.iter().map(|s| s.text.as_str()).collect();
                    let fg = sub.spans.first().map_or(Color::Reset, |s| s.fg);
                    let weight = sub.spans.first().map_or(Weight::Normal, |s| s.weight);
                    (content, fg, weight, sub.width)
                });
                element! {
                    View(key: row.key, flex_direction: FlexDirection::Column, background_color: row.bg) {
                        // Main cells line
                        View(flex_direction: FlexDirection::Row) {
                            #(row.cells.into_iter().enumerate().map(|(ci, cell)| {
                                // For single-span cells, render directly; for multi-span, concatenate.
                                let content: String = cell.spans.iter().map(|s| s.text.as_str()).collect();
                                let fg = cell.spans.first().map_or(Color::Reset, |s| s.fg);
                                let weight = cell.spans.first().map_or(Weight::Normal, |s| s.weight);
                                element! {
                                    View(key: ci, width: cell.width) {
                                        Text(
                                            content,
                                            color: fg,
                                            weight,
                                            wrap: TextWrap::NoWrap,
                                            align: cell.align,
                                        )
                                    }
                                }
                            }))
                        }
                        // Subtitle line (if present)
                        #(subtitle_elem.into_iter().map(|(content, fg, weight, width)| {
                            element! {
                                View(width, padding_left: 4u32) {
                                    Text(
                                        content,
                                        color: fg,
                                        weight,
                                        wrap: TextWrap::NoWrap,
                                    )
                                }
                            }
                        }))
                    }
                }
            }))
        }
    }
    .into_any()
}

// ---------------------------------------------------------------------------
// Column width computation
// ---------------------------------------------------------------------------

/// Convert `Span` values to `RenderedSpan` values.
fn render_spans(spans: &[Span], depth: ColorDepth) -> Vec<RenderedSpan> {
    spans
        .iter()
        .map(|s| RenderedSpan {
            text: s.text.clone(),
            fg: s
                .color
                .map_or(Color::Reset, |c| c.to_crossterm_color(depth)),
            weight: if s.bold { Weight::Bold } else { Weight::Normal },
        })
        .collect()
}

fn compute_column_widths(
    columns: &[&Column],
    overrides: Option<&HashMap<String, u16>>,
    total: u16,
) -> Vec<u16> {
    let mut widths: Vec<Option<u16>> = columns
        .iter()
        .map(|c| overrides.and_then(|o| o.get(&c.id)).copied())
        .collect();

    let fixed_total: u16 = widths.iter().filter_map(|w| *w).sum();
    let remaining = total.saturating_sub(fixed_total);

    let unfixed_pct_sum: f32 = columns
        .iter()
        .zip(widths.iter())
        .filter(|(_, w)| w.is_none())
        .map(|(c, _)| c.default_width_pct)
        .sum();

    for (i, col) in columns.iter().enumerate() {
        if widths[i].is_none() {
            let ratio = if unfixed_pct_sum > 0.0 {
                col.default_width_pct / unfixed_pct_sum
            } else {
                #[allow(clippy::cast_precision_loss)]
                {
                    1.0 / columns.len() as f32
                }
            };
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let w = (f32::from(remaining) * ratio).round() as u16;
            widths[i] = Some(w);
        }
    }

    widths.iter().map(|w| w.unwrap_or(1)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_columns() -> Vec<Column> {
        vec![
            Column {
                id: "state".to_owned(),
                header: "State".to_owned(),
                default_width_pct: 0.08,
                align: TextAlign::Left,
            },
            Column {
                id: "title".to_owned(),
                header: "Title".to_owned(),
                default_width_pct: 0.50,
                align: TextAlign::Left,
            },
            Column {
                id: "author".to_owned(),
                header: "Author".to_owned(),
                default_width_pct: 0.15,
                align: TextAlign::Left,
            },
            Column {
                id: "updated".to_owned(),
                header: "Updated".to_owned(),
                default_width_pct: 0.12,
                align: TextAlign::Right,
            },
        ]
    }

    #[test]
    fn column_widths_without_overrides() {
        let cols = make_columns();
        let col_refs: Vec<&Column> = cols.iter().collect();
        let widths = compute_column_widths(&col_refs, None, 100);

        let total: u16 = widths.iter().sum();
        assert!(
            (99..=101).contains(&total),
            "widths should sum close to 100, got {total}"
        );

        assert!(widths[1] > widths[0], "title should be wider than state");
    }

    #[test]
    fn column_widths_with_override() {
        let cols = make_columns();
        let col_refs: Vec<&Column> = cols.iter().collect();
        let overrides: HashMap<String, u16> = [("state".to_owned(), 10)].into_iter().collect();
        let widths = compute_column_widths(&col_refs, Some(&overrides), 100);

        assert_eq!(widths[0], 10, "state should be fixed at 10");

        let remaining_total: u16 = widths[1..].iter().sum();
        assert_eq!(
            remaining_total, 90,
            "remaining columns should fill 90 chars"
        );
    }

    #[test]
    fn hidden_columns_are_excluded() {
        let cols = make_columns();
        let hidden: HashSet<String> = ["author".to_owned()].into_iter().collect();
        let visible: Vec<&Column> = cols.iter().filter(|c| !hidden.contains(&c.id)).collect();

        assert_eq!(visible.len(), 3);
        assert!(visible.iter().all(|c| c.id != "author"));
    }
}
