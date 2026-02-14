use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::color::{Color as AppColor, ColorDepth};
use crate::markdown::syntax;
use crate::theme::ResolvedTheme;

// ---------------------------------------------------------------------------
// Styled output types
// ---------------------------------------------------------------------------

/// A single styled span of text.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct StyledSpan {
    pub text: String,
    pub color: AppColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl StyledSpan {
    fn plain(text: impl Into<String>, color: AppColor) -> Self {
        Self {
            text: text.into(),
            color,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }

    /// Create a plain span (public, for sidebar tab rendering).
    pub fn text(text: impl Into<String>, color: AppColor) -> Self {
        Self::plain(text, color)
    }

    /// Create a bold span.
    pub fn bold(text: impl Into<String>, color: AppColor) -> Self {
        Self {
            text: text.into(),
            color,
            bold: true,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

/// A line of styled spans.
#[derive(Debug, Clone)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
}

impl StyledLine {
    fn new() -> Self {
        Self { spans: Vec::new() }
    }

    fn push(&mut self, span: StyledSpan) {
        self.spans.push(span);
    }

    fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Create a line from a single span (public, for sidebar tab rendering).
    pub fn from_span(span: StyledSpan) -> Self {
        Self { spans: vec![span] }
    }

    /// Create a line from multiple spans.
    pub fn from_spans(spans: Vec<StyledSpan>) -> Self {
        Self { spans }
    }

    /// Create an empty line.
    pub fn blank() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Rendering context
// ---------------------------------------------------------------------------

/// Render markdown text into styled lines for display.
pub fn render_markdown(
    markdown: &str,
    theme: &ResolvedTheme,
    depth: ColorDepth,
) -> Vec<StyledLine> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);

    let parser = Parser::new_ext(markdown, opts);

    let mut ctx = RenderContext::new(theme, depth);
    ctx.process(parser);
    ctx.lines
}

struct RenderContext<'t> {
    theme: &'t ResolvedTheme,
    depth: ColorDepth,
    lines: Vec<StyledLine>,
    current_line: StyledLine,
    // Style state stack
    bold: u32,
    italic: u32,
    strikethrough: u32,
    // Block context
    in_heading: Option<u8>,
    in_code_block: Option<String>, // language tag
    code_block_content: String,
    in_blockquote: bool,
    list_depth: u32,
    ordered_index: Vec<Option<u64>>, // Some(n) = ordered at n, None = unordered
    in_link: bool,
    link_url: String,
}

impl<'t> RenderContext<'t> {
    fn new(theme: &'t ResolvedTheme, depth: ColorDepth) -> Self {
        Self {
            theme,
            depth,
            lines: Vec::new(),
            current_line: StyledLine::new(),
            bold: 0,
            italic: 0,
            strikethrough: 0,
            in_heading: None,
            in_code_block: None,
            code_block_content: String::new(),
            in_blockquote: false,
            list_depth: 0,
            ordered_index: Vec::new(),
            in_link: false,
            link_url: String::new(),
        }
    }

    fn flush_line(&mut self) {
        let line = std::mem::replace(&mut self.current_line, StyledLine::new());
        self.lines.push(line);
    }

    fn current_color(&self) -> AppColor {
        if self.in_heading.is_some() {
            match self.in_heading {
                Some(1) => self.theme.md_h1,
                Some(2) => self.theme.md_h2,
                Some(3) => self.theme.md_h3,
                _ => self.theme.md_heading,
            }
        } else if self.strikethrough > 0 {
            self.theme.md_strikethrough
        } else if self.bold > 0 {
            self.theme.md_strong
        } else if self.italic > 0 {
            self.theme.md_emphasis
        } else if self.in_blockquote {
            self.theme.md_blockquote
        } else {
            self.theme.md_text
        }
    }

    fn push_text(&mut self, text: &str) {
        let color = self.current_color();
        let span = StyledSpan {
            text: text.to_owned(),
            color,
            bold: self.bold > 0 || self.in_heading.is_some(),
            italic: self.italic > 0,
            underline: false,
            strikethrough: self.strikethrough > 0,
        };
        self.current_line.push(span);
    }

    fn push_prefix(&mut self) {
        if self.in_blockquote {
            let span = StyledSpan::plain(
                "\u{2502} ", // │
                self.theme.md_blockquote,
            );
            self.current_line.push(span);
        }
        if self.list_depth > 0 {
            let indent = "  ".repeat(self.list_depth.saturating_sub(1) as usize);
            if !indent.is_empty() {
                self.current_line
                    .push(StyledSpan::plain(indent, self.theme.md_text));
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    fn process<'a>(&mut self, parser: impl Iterator<Item = Event<'a>>) {
        for event in parser {
            match event {
                // ---- Block start tags ----
                Event::Start(Tag::Heading { level, .. }) => {
                    let lvl = level as u8;
                    self.in_heading = Some(lvl);
                    let prefix = "#".repeat(lvl as usize);
                    let color = match lvl {
                        1 => self.theme.md_h1,
                        2 => self.theme.md_h2,
                        3 => self.theme.md_h3,
                        _ => self.theme.md_heading,
                    };
                    self.current_line.push(StyledSpan {
                        text: format!("{prefix} "),
                        color,
                        bold: true,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                    });
                }
                Event::Start(Tag::Paragraph) => {
                    self.push_prefix();
                }
                Event::Start(Tag::BlockQuote(_)) => {
                    self.in_blockquote = true;
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(lang) => {
                            let l = lang.split_whitespace().next().unwrap_or("").to_owned();
                            if l.is_empty() { None } else { Some(l) }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    self.in_code_block = lang.or_else(|| Some(String::new()));
                    self.code_block_content.clear();
                }
                Event::Start(Tag::List(ordered)) => {
                    self.list_depth += 1;
                    self.ordered_index.push(ordered);
                }
                Event::Start(Tag::Item) => {
                    self.push_prefix();
                    // Emit bullet or number
                    let marker = if let Some(Some(n)) = self.ordered_index.last_mut() {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    } else {
                        "\u{2022} ".to_owned() // •
                    };
                    self.current_line
                        .push(StyledSpan::plain(marker, self.theme.md_text));
                }
                Event::Start(Tag::Emphasis) => {
                    self.italic += 1;
                }
                Event::Start(Tag::Strong) => {
                    self.bold += 1;
                }
                Event::Start(Tag::Strikethrough) => {
                    self.strikethrough += 1;
                }
                Event::Start(Tag::Link { dest_url, .. }) => {
                    self.in_link = true;
                    self.link_url = dest_url.to_string();
                }
                Event::Start(Tag::Image { dest_url, .. }) => {
                    self.current_line.push(StyledSpan::plain(
                        format!("[image: {dest_url}]"),
                        self.theme.md_link,
                    ));
                }
                // ---- Block end tags ----
                Event::End(TagEnd::Heading(_)) => {
                    self.in_heading = None;
                    self.flush_line();
                    // Add blank line after heading.
                    self.lines.push(StyledLine::new());
                }
                Event::End(TagEnd::Paragraph) => {
                    self.flush_line();
                    self.lines.push(StyledLine::new());
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    self.in_blockquote = false;
                }
                Event::End(TagEnd::CodeBlock) => {
                    let lang = self.in_code_block.take().unwrap_or_default();
                    let content = std::mem::take(&mut self.code_block_content);

                    if lang.is_empty() {
                        // Plain code block (no language).
                        for line_text in content.lines() {
                            self.current_line
                                .push(StyledSpan::plain(line_text, self.theme.md_code_block));
                            self.flush_line();
                        }
                    } else {
                        // Syntax-highlighted code block.
                        let spans = syntax::highlight_code(&content, &lang, self.theme, self.depth);
                        // Split spans into lines.
                        for span in spans {
                            for (i, segment) in span.text.split('\n').enumerate() {
                                if i > 0 {
                                    self.flush_line();
                                }
                                if !segment.is_empty() {
                                    self.current_line
                                        .push(StyledSpan::plain(segment, span.color));
                                }
                            }
                        }
                    }
                    if !self.current_line.is_empty() {
                        self.flush_line();
                    }
                    self.lines.push(StyledLine::new());
                }
                Event::End(TagEnd::List(_)) => {
                    self.list_depth = self.list_depth.saturating_sub(1);
                    self.ordered_index.pop();
                    if self.list_depth == 0 {
                        self.lines.push(StyledLine::new());
                    }
                }
                Event::End(TagEnd::Item | TagEnd::TableRow | TagEnd::TableHead) => {
                    self.flush_line();
                }
                Event::End(TagEnd::Emphasis) => {
                    self.italic = self.italic.saturating_sub(1);
                }
                Event::End(TagEnd::Strong) => {
                    self.bold = self.bold.saturating_sub(1);
                }
                Event::End(TagEnd::Strikethrough) => {
                    self.strikethrough = self.strikethrough.saturating_sub(1);
                }
                Event::End(TagEnd::Link) => {
                    if !self.link_url.is_empty() {
                        let url_text = format!(" ({})", self.link_url);
                        self.current_line.push(StyledSpan {
                            text: url_text,
                            color: self.theme.md_link,
                            bold: false,
                            italic: false,
                            underline: true,
                            strikethrough: false,
                        });
                    }
                    self.in_link = false;
                    self.link_url.clear();
                }
                Event::End(TagEnd::Table) => {
                    self.lines.push(StyledLine::new());
                }
                Event::End(TagEnd::TableCell) => {
                    self.current_line
                        .push(StyledSpan::plain(" | ", self.theme.md_text));
                }

                // ---- Inline content ----
                Event::Text(text) => {
                    if self.in_code_block.is_some() {
                        self.code_block_content.push_str(&text);
                    } else if self.in_link {
                        self.current_line.push(StyledSpan {
                            text: text.to_string(),
                            color: self.theme.md_link_text,
                            bold: self.bold > 0,
                            italic: self.italic > 0,
                            underline: true,
                            strikethrough: false,
                        });
                    } else {
                        self.push_text(&text);
                    }
                }
                Event::Code(code) => {
                    self.current_line.push(StyledSpan {
                        text: format!("`{code}`"),
                        color: self.theme.md_code,
                        bold: false,
                        italic: false,
                        underline: false,
                        strikethrough: false,
                    });
                }
                Event::SoftBreak => {
                    self.push_text(" ");
                }
                Event::HardBreak => {
                    self.flush_line();
                    self.push_prefix();
                }
                Event::Rule => {
                    self.current_line.push(StyledSpan::plain(
                        "\u{2500}".repeat(40), // ─ repeated
                        self.theme.md_horizontal_rule,
                    ));
                    self.flush_line();
                    self.lines.push(StyledLine::new());
                }
                Event::TaskListMarker(checked) => {
                    let marker = if checked { "[\u{2714}] " } else { "[ ] " };
                    self.current_line
                        .push(StyledSpan::plain(marker, self.theme.md_text));
                }
                Event::Html(html) | Event::InlineHtml(html) => {
                    // Strip HTML comments, render others as-is.
                    let text = html.trim();
                    if !text.starts_with("<!--") {
                        self.push_text(text);
                    }
                }
                _ => {}
            }
        }

        // Flush remaining content.
        if !self.current_line.is_empty() {
            self.flush_line();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::types::Theme;
    use crate::theme::Background;

    fn test_theme() -> ResolvedTheme {
        ResolvedTheme::resolve(&Theme::default(), Background::Dark)
    }

    fn render(markdown: &str) -> Vec<StyledLine> {
        let theme = test_theme();
        render_markdown(markdown, &theme, ColorDepth::TrueColor)
    }

    fn line_text(line: &StyledLine) -> String {
        line.spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn heading_levels() {
        let lines = render("# H1\n\n## H2\n\n### H3\n");
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        assert!(texts[0].starts_with("# "), "H1: {}", texts[0]);
        // After H1, blank line, then H2
        assert!(
            texts.iter().any(|t| t.starts_with("## ")),
            "should contain H2"
        );
        assert!(
            texts.iter().any(|t| t.starts_with("### ")),
            "should contain H3"
        );
    }

    #[test]
    fn bold_and_italic() {
        let lines = render("Hello **bold** and *italic* text\n");
        let all_text: String = lines.iter().map(line_text).collect::<String>();
        assert!(all_text.contains("bold"), "should contain bold text");
        assert!(all_text.contains("italic"), "should contain italic text");

        // Check that bold spans have bold=true
        let bold_span = lines
            .iter()
            .flat_map(|l| &l.spans)
            .find(|s| s.text == "bold");
        assert!(bold_span.is_some_and(|s| s.bold));
    }

    #[test]
    fn inline_code() {
        let lines = render("Use `foo()` here\n");
        let all_text: String = lines.iter().map(line_text).collect::<String>();
        assert!(
            all_text.contains("`foo()`"),
            "inline code should be wrapped in backticks: {all_text}"
        );
    }

    #[test]
    fn fenced_code_block_with_language() {
        let md = "```rust\nfn main() {}\n```\n";
        let lines = render(md);
        let all_text: String = lines.iter().map(line_text).collect::<Vec<_>>().join("\n");
        assert!(
            all_text.contains("fn"),
            "code block should contain source: {all_text}"
        );
    }

    #[test]
    fn unordered_list() {
        let md = "- item one\n- item two\n- item three\n";
        let lines = render(md);
        let texts: Vec<String> = lines
            .iter()
            .map(line_text)
            .filter(|t| !t.is_empty())
            .collect();
        assert!(texts.len() >= 3, "should have 3 list items: {texts:?}");
        assert!(
            texts[0].contains("\u{2022}"),
            "should use bullet char: {}",
            texts[0]
        );
    }

    #[test]
    fn ordered_list() {
        let md = "1. first\n2. second\n3. third\n";
        let lines = render(md);
        let texts: Vec<String> = lines
            .iter()
            .map(line_text)
            .filter(|t| !t.is_empty())
            .collect();
        assert!(texts[0].contains("1."), "first item: {}", texts[0]);
    }

    #[test]
    fn links() {
        let md = "[click here](https://example.com)\n";
        let lines = render(md);
        let all_text: String = lines.iter().map(line_text).collect::<String>();
        assert!(all_text.contains("click here"), "link text: {all_text}");
        assert!(
            all_text.contains("https://example.com"),
            "link url: {all_text}"
        );
    }

    #[test]
    fn horizontal_rule() {
        let md = "above\n\n---\n\nbelow\n";
        let lines = render(md);
        let has_rule = lines.iter().any(|l| line_text(l).contains('\u{2500}'));
        assert!(has_rule, "should contain horizontal rule character");
    }

    #[test]
    fn html_comments_stripped() {
        let md = "before <!-- comment --> after\n";
        let lines = render(md);
        let all_text: String = lines.iter().map(line_text).collect::<String>();
        assert!(
            !all_text.contains("comment"),
            "HTML comments should be stripped: {all_text}"
        );
    }

    #[test]
    fn blockquote() {
        let md = "> quoted text\n";
        let lines = render(md);
        let has_quote = lines.iter().any(|l| line_text(l).contains('\u{2502}'));
        assert!(has_quote, "blockquote should use │ prefix");
    }

    #[test]
    fn task_list() {
        let md = "- [x] done\n- [ ] todo\n";
        let lines = render(md);
        let texts: Vec<String> = lines
            .iter()
            .map(line_text)
            .filter(|t| !t.is_empty())
            .collect();
        assert!(
            texts.iter().any(|t| t.contains('\u{2714}')),
            "checked item should show ✔: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("[ ]")),
            "unchecked item should show [ ]: {texts:?}"
        );
    }
}
