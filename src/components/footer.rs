use iocraft::prelude::*;

use crate::color::{Color as AppColor, ColorDepth};

// ---------------------------------------------------------------------------
// Footer component (owned data, no lifetime issues)
// ---------------------------------------------------------------------------

pub struct RenderedFooter {
    pub help_text: String,
    pub status: String,
    pub text_fg: Color,
    pub border_fg: Color,
}

impl RenderedFooter {
    pub fn build(
        help_text: String,
        status: String,
        depth: ColorDepth,
        text_color: Option<AppColor>,
        border_color: Option<AppColor>,
    ) -> Self {
        let text_fg = text_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        let border_fg = border_color.map_or(Color::DarkGrey, |c| c.to_crossterm_color(depth));
        Self {
            help_text,
            status,
            text_fg,
            border_fg,
        }
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

    let has_status = !f.status.is_empty();

    element! {
        View(
            border_style: BorderStyle::Single,
            border_edges: Edges::Top,
            border_color: f.border_fg,
            padding_left: 1,
            padding_right: 1,
        ) {
            View(flex_grow: 1.0) {
                Text(content: f.help_text, color: f.text_fg, wrap: TextWrap::NoWrap)
            }
            #(if has_status {
                Some(element! {
                    Text(content: f.status, color: f.text_fg)
                })
            } else {
                None
            })
        }
    }
    .into_any()
}
