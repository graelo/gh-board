use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, Options, Parser, Tag, TagEnd,
};
use unicode_width::UnicodeWidthStr;

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
// Helpers
// ---------------------------------------------------------------------------

/// Compute the display width of a sequence of spans.
fn spans_width(spans: &[StyledSpan]) -> usize {
    spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.text.as_str()))
        .sum()
}

/// Find the next bare URL (`https://` or `http://`) in `text` starting from `start`.
/// Returns `(url_start, url_end)` byte offsets or `None`.
fn find_bare_url(text: &str, start: usize) -> Option<(usize, usize)> {
    let haystack = &text[start..];
    let prefix_pos = haystack
        .find("https://")
        .or_else(|| haystack.find("http://"))?;
    let url_start = start + prefix_pos;
    // Scan forward until whitespace, or end of string.
    let url_end = text[url_start..]
        .find(|c: char| c.is_whitespace())
        .map_or(text.len(), |pos| url_start + pos);
    Some((url_start, url_end))
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
    opts.insert(Options::ENABLE_GFM);

    let parser = Parser::new_ext(markdown, opts);

    let mut ctx = RenderContext::new(theme, depth);
    ctx.process(parser);
    ctx.lines
}

#[allow(clippy::struct_excessive_bools)]
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
    blockquote_kind: Option<BlockQuoteKind>,
    list_depth: u32,
    ordered_index: Vec<Option<u64>>, // Some(n) = ordered at n, None = unordered
    in_link: bool,
    link_url: String,
    // Table buffering state
    in_table: bool,
    table_alignments: Vec<Alignment>,
    table_rows: Vec<Vec<Vec<StyledSpan>>>, // [row][col][spans]
    table_current_row: Vec<Vec<StyledSpan>>,
    table_current_cell: Vec<StyledSpan>,
    table_is_header: bool,
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
            blockquote_kind: None,
            list_depth: 0,
            ordered_index: Vec::new(),
            in_link: false,
            link_url: String::new(),
            in_table: false,
            table_alignments: Vec::new(),
            table_rows: Vec::new(),
            table_current_row: Vec::new(),
            table_current_cell: Vec::new(),
            table_is_header: false,
        }
    }

    fn flush_line(&mut self) {
        let line = std::mem::replace(&mut self.current_line, StyledLine::new());
        self.lines.push(line);
    }

    /// Ensure there is a blank line before the next block element.
    /// Does nothing if we are at the very start or there is already a trailing blank line.
    fn ensure_blank_line(&mut self) {
        if let Some(last) = self.lines.last()
            && !last.is_empty()
        {
            self.lines.push(StyledLine::new());
        }
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
        if self.in_table {
            self.table_current_cell.push(span);
        } else {
            self.current_line.push(span);
        }
    }

    fn push_span(&mut self, span: StyledSpan) {
        if self.in_table {
            self.table_current_cell.push(span);
        } else {
            self.current_line.push(span);
        }
    }

    /// Push text with bare-URL autolink detection.
    /// Bare URLs are replaced with `↗` icon; surrounding text is kept.
    fn push_text_with_autolinks(&mut self, text: &str) {
        let mut pos = 0;
        while pos < text.len() {
            if let Some((url_start, url_end)) = find_bare_url(text, pos) {
                // Emit text before the URL.
                if url_start > pos {
                    self.push_text(&text[pos..url_start]);
                }
                // Emit the link icon instead of the URL.
                self.push_span(StyledSpan::plain("\u{2197}", self.theme.md_link)); // ↗
                pos = url_end;
            } else {
                // No more URLs; emit the rest.
                self.push_text(&text[pos..]);
                break;
            }
        }
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

    fn push_alert_prefix(&mut self) {
        if let Some(kind) = self.blockquote_kind {
            let (label, color) = match kind {
                BlockQuoteKind::Note => ("Note", self.theme.md_link),
                BlockQuoteKind::Tip => ("Tip", self.theme.text_success),
                BlockQuoteKind::Important => ("Important", self.theme.md_link),
                BlockQuoteKind::Warning => ("Warning", self.theme.text_warning),
                BlockQuoteKind::Caution => ("Caution", self.theme.text_error),
            };
            self.current_line
                .push(StyledSpan::bold(format!("{label}: "), color));
            self.blockquote_kind = None;
        }
    }

    /// Flush the buffered table into `self.lines`.
    fn flush_table(&mut self) {
        let alignments = std::mem::take(&mut self.table_alignments);
        let rows = std::mem::take(&mut self.table_rows);

        if rows.is_empty() {
            return;
        }

        let num_cols = alignments.len();

        // Pass 1: compute column widths.
        let mut col_widths = vec![0usize; num_cols];
        for row in &rows {
            for (c, cell) in row.iter().enumerate() {
                if c < num_cols {
                    col_widths[c] = col_widths[c].max(spans_width(cell));
                }
            }
        }

        let border_color = self.theme.border_faint;

        // Pass 2: emit rows.
        for (r, row) in rows.iter().enumerate() {
            let mut line = StyledLine::new();
            for (c, cell) in row.iter().enumerate() {
                if c < num_cols {
                    if c > 0 {
                        line.push(StyledSpan::plain(" \u{2502} ", border_color)); // │
                    } else {
                        line.push(StyledSpan::plain(" ", self.theme.md_text));
                    }
                    // Build the plain text content of this cell for padding.
                    let cell_text: String = cell.iter().map(|s| s.text.as_str()).collect();
                    let cell_w = UnicodeWidthStr::width(cell_text.as_str());
                    let target = col_widths[c];
                    let align = alignments.get(c).copied().unwrap_or(Alignment::None);

                    let (left_pad, right_pad) = if cell_w >= target {
                        (0, 0)
                    } else {
                        let pad = target - cell_w;
                        match align {
                            Alignment::Center => {
                                let l = pad / 2;
                                (l, pad - l)
                            }
                            Alignment::Right => (pad, 0),
                            Alignment::None | Alignment::Left => (0, pad),
                        }
                    };

                    if left_pad > 0 {
                        line.push(StyledSpan::plain(" ".repeat(left_pad), self.theme.md_text));
                    }

                    // Emit cell spans. Header row (r == 0) gets bold.
                    for span in cell {
                        let mut s = span.clone();
                        if r == 0 {
                            s.bold = true;
                        }
                        line.push(s);
                    }

                    if right_pad > 0 {
                        line.push(StyledSpan::plain(" ".repeat(right_pad), self.theme.md_text));
                    }
                }
            }
            self.lines.push(line);

            // After the header row (row 0), emit a separator.
            if r == 0 {
                let mut sep = StyledLine::new();
                for (c, &w) in col_widths.iter().enumerate() {
                    if c > 0 {
                        sep.push(StyledSpan::plain(
                            "\u{2500}\u{253C}\u{2500}", // ─┼─
                            border_color,
                        ));
                    } else {
                        sep.push(StyledSpan::plain("\u{2500}", border_color)); // ─
                    }
                    sep.push(StyledSpan::plain("\u{2500}".repeat(w), border_color));
                }
                self.lines.push(sep);
            }
        }

        self.lines.push(StyledLine::new());
        self.in_table = false;
    }

    #[allow(clippy::too_many_lines)]
    fn process<'a>(&mut self, parser: impl Iterator<Item = Event<'a>>) {
        for event in parser {
            match event {
                // ---- Block start tags ----
                Event::Start(Tag::Heading { level, .. }) => {
                    self.ensure_blank_line();
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
                    self.push_alert_prefix();
                }
                Event::Start(Tag::BlockQuote(kind)) => {
                    self.ensure_blank_line();
                    self.in_blockquote = true;
                    self.blockquote_kind = kind;
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    self.ensure_blank_line();
                    let lang = match kind {
                        CodeBlockKind::Fenced(lang) => {
                            let l = lang.split_whitespace().next().unwrap_or("").to_owned();
                            if l.is_empty() { None } else { Some(l) }
                        }
                        CodeBlockKind::Indented => None,
                    };
                    self.in_code_block = Some(lang.unwrap_or_default());
                    self.code_block_content.clear();
                }
                Event::Start(Tag::List(ordered)) => {
                    if self.list_depth == 0 {
                        self.ensure_blank_line();
                    }
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
                    // Emit link icon before the link text.
                    self.push_span(StyledSpan::plain("\u{2197} ", self.theme.md_link)); // ↗
                }
                Event::Start(Tag::Image { dest_url, .. }) => {
                    self.push_span(StyledSpan::plain(
                        format!("[image: {dest_url}]"),
                        self.theme.md_link,
                    ));
                }
                // Table events
                Event::Start(Tag::Table(alignments)) => {
                    self.ensure_blank_line();
                    self.in_table = true;
                    self.table_alignments = alignments;
                    self.table_rows.clear();
                    self.table_current_row.clear();
                    self.table_current_cell.clear();
                    self.table_is_header = false;
                }
                Event::Start(Tag::TableHead) => {
                    self.table_is_header = true;
                    self.table_current_row.clear();
                }
                Event::Start(Tag::TableRow) => {
                    self.table_current_row.clear();
                }
                Event::Start(Tag::TableCell) => {
                    self.table_current_cell.clear();
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
                    self.blockquote_kind = None;
                    self.ensure_blank_line();
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
                Event::End(TagEnd::Item) => {
                    self.flush_line();
                }
                Event::End(TagEnd::TableCell) => {
                    let cell = std::mem::take(&mut self.table_current_cell);
                    self.table_current_row.push(cell);
                }
                Event::End(TagEnd::TableHead) => {
                    let row = std::mem::take(&mut self.table_current_row);
                    self.table_rows.push(row);
                    self.table_is_header = false;
                }
                Event::End(TagEnd::TableRow) => {
                    let row = std::mem::take(&mut self.table_current_row);
                    self.table_rows.push(row);
                }
                Event::End(TagEnd::Table) => {
                    self.flush_table();
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
                    // URL is hidden — icon was already emitted at Start(Link).
                    self.in_link = false;
                    self.link_url.clear();
                }

                // ---- Inline content ----
                Event::Text(text) => {
                    if self.in_code_block.is_some() {
                        self.code_block_content.push_str(&text);
                    } else {
                        let text = crate::util::expand_emoji(&text);
                        if self.in_link {
                            self.push_span(StyledSpan {
                                text,
                                color: self.theme.md_link_text,
                                bold: self.bold > 0,
                                italic: self.italic > 0,
                                underline: false,
                                strikethrough: false,
                            });
                        } else {
                            self.push_text_with_autolinks(&text);
                        }
                    }
                }
                Event::Code(code) => {
                    self.push_span(StyledSpan {
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
                    self.ensure_blank_line();
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
            all_text.contains('\u{2197}'),
            "should contain ↗ icon: {all_text}"
        );
        // URL should be hidden
        assert!(
            !all_text.contains("https://example.com"),
            "URL should be hidden: {all_text}"
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
        assert!(has_quote, "blockquote should use \u{2502} prefix");
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
            "checked item should show \u{2714}: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("[ ]")),
            "unchecked item should show [ ]: {texts:?}"
        );
    }

    // --- New tests ---

    #[test]
    fn table_rendering() {
        let md = "| Name | Age | City |\n|------|-----|------|\n| Alice | 30 | NYC |\n| Bob | 25 | LA |\n";
        let lines = render(md);
        let texts: Vec<String> = lines.iter().map(line_text).collect();

        // Should have header row, separator, and 2 body rows.
        let non_empty: Vec<&String> = texts.iter().filter(|t| !t.is_empty()).collect();
        assert!(
            non_empty.len() >= 4,
            "table should have header + separator + 2 body rows: {non_empty:?}"
        );

        // Header row should contain all headers.
        assert!(non_empty[0].contains("Name"), "header: {}", non_empty[0]);
        assert!(non_empty[0].contains("Age"), "header: {}", non_empty[0]);
        assert!(non_empty[0].contains("City"), "header: {}", non_empty[0]);

        // Separator should contain ─ and ┼.
        assert!(
            non_empty[1].contains('\u{2500}'),
            "separator should have ─: {}",
            non_empty[1]
        );
        assert!(
            non_empty[1].contains('\u{253C}'),
            "separator should have ┼: {}",
            non_empty[1]
        );

        // Cell separators should use │.
        assert!(
            non_empty[0].contains('\u{2502}'),
            "columns separated by │: {}",
            non_empty[0]
        );

        // Body rows should contain cell data.
        assert!(
            non_empty[2].contains("Alice"),
            "body row 1: {}",
            non_empty[2]
        );
        assert!(non_empty[3].contains("Bob"), "body row 2: {}", non_empty[3]);
    }

    #[test]
    fn table_column_alignment() {
        let md = "| Left | Center | Right |\n|:-----|:------:|------:|\n| a | b | c |\n";
        let lines = render(md);
        let texts: Vec<String> = lines.iter().map(line_text).collect();
        let non_empty: Vec<&String> = texts.iter().filter(|t| !t.is_empty()).collect();

        // Just verify it renders without panic and has expected structure.
        assert!(
            non_empty.len() >= 3,
            "aligned table should render: {non_empty:?}"
        );
        assert!(
            non_empty[0].contains("Left"),
            "header present: {}",
            non_empty[0]
        );
    }

    #[test]
    fn bare_url_autolink() {
        let md = "See https://github.com/foo/bar for details\n";
        let lines = render(md);
        let all_text: String = lines.iter().map(line_text).collect::<String>();

        // URL should be replaced by ↗ icon.
        assert!(
            all_text.contains('\u{2197}'),
            "bare URL should show ↗: {all_text}"
        );
        // The full URL should not appear.
        assert!(
            !all_text.contains("https://github.com"),
            "bare URL text should be hidden: {all_text}"
        );
        // Surrounding text should remain.
        assert!(all_text.contains("See "), "text before URL: {all_text}");
        assert!(
            all_text.contains(" for details"),
            "text after URL: {all_text}"
        );
    }

    #[test]
    fn bare_url_with_trailing_text() {
        let md = "https://github.com/org/repo/pull/25239: initial PR\n";
        let lines = render(md);
        let all_text: String = lines.iter().map(line_text).collect::<String>();

        assert!(
            all_text.contains('\u{2197}'),
            "should have ↗ icon: {all_text}"
        );
        assert!(
            !all_text.contains("https://"),
            "URL should be hidden: {all_text}"
        );
        // URL scanning stops at whitespace; the colon is part of the URL.
        assert!(
            all_text.contains(" initial PR"),
            "text after URL should remain: {all_text}"
        );
    }

    #[test]
    fn link_icon_style() {
        let md = "[click here](https://example.com)\n";
        let lines = render(md);

        // Find the icon span.
        let icon_span = lines
            .iter()
            .flat_map(|l| &l.spans)
            .find(|s| s.text.contains('\u{2197}'));
        assert!(icon_span.is_some(), "should have ↗ icon span");

        // Find the link text span.
        let text_span = lines
            .iter()
            .flat_map(|l| &l.spans)
            .find(|s| s.text == "click here");
        assert!(text_span.is_some(), "should have link text span");
    }

    #[test]
    fn gfm_alert_note() {
        let md = "> [!NOTE]\n> This is a note\n";
        let lines = render(md);
        let all_text: String = lines.iter().map(line_text).collect::<String>();

        assert!(
            all_text.contains("Note:"),
            "should show 'Note:' prefix: {all_text}"
        );
        assert!(
            all_text.contains("This is a note"),
            "should show alert body: {all_text}"
        );
    }

    #[test]
    fn gfm_alert_warning() {
        let md = "> [!WARNING]\n> Be careful here\n";
        let lines = render(md);
        let all_text: String = lines.iter().map(line_text).collect::<String>();

        assert!(
            all_text.contains("Warning:"),
            "should show 'Warning:' prefix: {all_text}"
        );
        assert!(
            all_text.contains("Be careful here"),
            "should show alert body: {all_text}"
        );
    }

    #[test]
    fn gfm_alert_has_bold_prefix() {
        let md = "> [!NOTE]\n> Info here\n";
        let lines = render(md);

        let note_span = lines
            .iter()
            .flat_map(|l| &l.spans)
            .find(|s| s.text.contains("Note:"));
        assert!(
            note_span.is_some_and(|s| s.bold),
            "alert prefix should be bold"
        );
    }
}
