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
    pub contents: Vec<MixedTextContent>,
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
                // Empty lines (blank separators) need a single space so the
                // element has non-zero height in the layout.
                let contents = if line.spans.is_empty() {
                    vec![MixedTextContent::new(" ")]
                } else {
                    line.spans
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
                            let mut c = MixedTextContent::new(&span.text)
                                .color(fg)
                                .weight(weight)
                                .decoration(decoration);
                            if span.italic {
                                c = c.italic();
                            }
                            c
                        })
                        .collect()
                };
                RenderedMdLine {
                    key: scroll_offset + i,
                    contents,
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
                    MixedText(
                        key: line.key,
                        contents: line.contents,
                        wrap: TextWrap::Wrap,
                    )
                }
            }))
        }
    }
    .into_any()
}
