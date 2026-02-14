use iocraft::prelude::*;

use crate::color::ColorDepth;
use crate::markdown::renderer::StyledLine;

// ---------------------------------------------------------------------------
// Pre-rendered markdown data (all owned)
// ---------------------------------------------------------------------------

pub struct RenderedMarkdown {
    pub lines: Vec<RenderedMdLine>,
    pub total_lines: usize,
}

pub struct RenderedMdLine {
    pub key: usize,
    pub spans: Vec<RenderedMdSpan>,
}

pub struct RenderedMdSpan {
    pub text: String,
    pub fg: Color,
    pub weight: Weight,
    pub italic: bool,
    pub decoration: TextDecoration,
}

impl RenderedMarkdown {
    /// Build pre-rendered markdown from styled lines, applying scroll and color
    /// depth conversion.
    pub fn build(
        lines: &[StyledLine],
        scroll_offset: usize,
        visible_lines: usize,
        depth: ColorDepth,
    ) -> Self {
        let total_lines = lines.len();
        let end = (scroll_offset + visible_lines).min(total_lines);
        let visible = if scroll_offset < total_lines {
            &lines[scroll_offset..end]
        } else {
            &[]
        };

        let rendered_lines: Vec<RenderedMdLine> = visible
            .iter()
            .enumerate()
            .map(|(i, line)| {
                let spans = line
                    .spans
                    .iter()
                    .map(|span| {
                        let fg = span.color.to_crossterm_color(depth);
                        let weight = if span.bold {
                            Weight::Bold
                        } else {
                            Weight::Normal
                        };
                        let decoration = if span.underline {
                            TextDecoration::Underline
                        } else {
                            TextDecoration::None
                        };
                        RenderedMdSpan {
                            text: span.text.clone(),
                            fg,
                            weight,
                            italic: span.italic,
                            decoration,
                        }
                    })
                    .collect();
                RenderedMdLine {
                    key: scroll_offset + i,
                    spans,
                }
            })
            .collect();

        Self {
            lines: rendered_lines,
            total_lines,
        }
    }
}

// ---------------------------------------------------------------------------
// MarkdownView component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct MarkdownViewProps {
    pub markdown: Option<RenderedMarkdown>,
}

#[component]
pub fn MarkdownView(props: &mut MarkdownViewProps) -> impl Into<AnyElement<'static>> {
    let Some(md) = props.markdown.take() else {
        return element! { View }.into_any();
    };

    element! {
        View(flex_direction: FlexDirection::Column) {
            #(md.lines.into_iter().map(|line| {
                element! {
                    View(key: line.key) {
                        #(line.spans.into_iter().enumerate().map(|(si, span)| {
                            element! {
                                Text(
                                    key: si,
                                    content: span.text,
                                    color: span.fg,
                                    weight: span.weight,
                                    italic: span.italic,
                                    decoration: span.decoration,
                                    wrap: TextWrap::Wrap,
                                )
                            }
                        }))
                    }
                }
            }))
        }
    }
    .into_any()
}
