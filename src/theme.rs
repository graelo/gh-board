use crate::color::Color;
use crate::config::types::Theme;
use crate::icons::ResolvedIcons;

/// Detected terminal background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    Dark,
    Light,
}

impl Background {
    /// Detect whether the terminal has a dark or light background.
    ///
    /// Heuristic: check `COLORFGBG` (format "fg;bg"), fall back to dark.
    pub fn detect() -> Self {
        if let Ok(val) = std::env::var("COLORFGBG")
            && let Some(bg) = val.rsplit(';').next()
            && let Ok(n) = bg.parse::<u8>()
        {
            // ANSI colors 0-6 and 8 are typically dark backgrounds.
            if n > 6 && n != 8 {
                return Background::Light;
            }
        }
        Background::Dark
    }
}

/// A fully resolved theme: every color slot has a concrete `Color` value
/// (either from user config or from defaults for the detected background).
#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    // Text
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_inverted: Color,
    pub text_faint: Color,
    pub text_warning: Color,
    pub text_success: Color,
    pub text_error: Color,
    pub text_actor: Color,
    // Background
    pub bg_selected: Color,
    // Border
    pub border_primary: Color,
    pub border_secondary: Color,
    pub border_faint: Color,
    // Markdown
    pub md_text: Color,
    pub md_heading: Color,
    pub md_h1: Color,
    pub md_h2: Color,
    pub md_h3: Color,
    pub md_code: Color,
    pub md_code_block: Color,
    pub md_link: Color,
    pub md_link_text: Color,
    pub md_emphasis: Color,
    pub md_strong: Color,
    pub md_strikethrough: Color,
    pub md_horizontal_rule: Color,
    pub md_blockquote: Color,
    // Syntax highlighting
    pub syn_keyword: Color,
    pub syn_string: Color,
    pub syn_comment: Color,
    pub syn_number: Color,
    pub syn_function: Color,
    pub syn_type: Color,
    pub syn_operator: Color,
    pub syn_punctuation: Color,
    pub syn_name: Color,
    pub syn_name_builtin: Color,
    // Sidebar pill / meta
    pub pill_draft_bg: Color,
    pub pill_open_bg: Color,
    pub pill_closed_bg: Color,
    pub pill_merged_bg: Color,
    pub pill_fg: Color,
    pub pill_branch: Color,
    pub pill_author: Color,
    pub pill_age: Color,
    pub pill_role: Color,
    pub pill_separator: Color,
    // Footer
    pub footer_prs: Color,
    pub footer_issues: Color,
    pub footer_actions: Color,
    pub footer_notifications: Color,
    pub footer_repo: Color,
    // Icons
    pub icons: ResolvedIcons,
}

impl ResolvedTheme {
    /// Build a resolved theme from user config and detected background.
    pub fn resolve(theme: &Theme, bg: Background) -> Self {
        let d = Defaults::for_background(bg);
        let md = &theme.colors.markdown;
        let syn = &md.syntax;
        let pill = &theme.colors.pill;

        // Resolve base text colors first so pills can fall back to them.
        let text_primary = theme.colors.text.primary.unwrap_or(d.text_primary);
        let text_secondary = theme.colors.text.secondary.unwrap_or(d.text_secondary);
        let text_faint = theme.colors.text.faint.unwrap_or(d.text_faint);
        let text_success = theme.colors.text.success.unwrap_or(d.text_success);
        let text_error = theme.colors.text.error.unwrap_or(d.text_error);
        let text_actor = theme.colors.text.actor.unwrap_or(d.text_actor);

        Self {
            text_primary,
            text_secondary,
            text_inverted: theme.colors.text.inverted.unwrap_or(d.text_inverted),
            text_faint,
            text_warning: theme.colors.text.warning.unwrap_or(d.text_warning),
            text_success,
            text_error,
            text_actor,
            bg_selected: theme.colors.background.selected.unwrap_or(d.bg_selected),
            border_primary: theme.colors.border.primary.unwrap_or(d.border_primary),
            border_secondary: theme.colors.border.secondary.unwrap_or(d.border_secondary),
            border_faint: theme.colors.border.faint.unwrap_or(d.border_faint),
            // Markdown
            md_text: md.text.unwrap_or(d.text_primary),
            md_heading: md.heading.unwrap_or(d.md_heading),
            md_h1: md.h1.or(md.heading).unwrap_or(d.md_heading),
            md_h2: md.h2.or(md.heading).unwrap_or(d.md_heading),
            md_h3: md.h3.or(md.heading).unwrap_or(d.md_heading),
            md_code: md.code.unwrap_or(d.md_code),
            md_code_block: md.code_block.unwrap_or(d.md_code_block),
            md_link: md.link.unwrap_or(d.md_link),
            md_link_text: md.link_text.unwrap_or(d.md_link_text),
            md_emphasis: md.emph.unwrap_or(d.text_primary),
            md_strong: md.strong.unwrap_or(d.text_primary),
            md_strikethrough: md.strikethrough.unwrap_or(d.text_faint),
            md_horizontal_rule: md.horizontal_rule.unwrap_or(d.border_faint),
            md_blockquote: md.text.unwrap_or(d.text_faint),
            // Syntax
            syn_keyword: syn.keyword.unwrap_or(d.syn_keyword),
            syn_string: syn.string.unwrap_or(d.syn_string),
            syn_comment: syn.comment.unwrap_or(d.syn_comment),
            syn_number: syn.number.unwrap_or(d.syn_number),
            syn_function: syn.name_function.unwrap_or(d.syn_function),
            syn_type: syn.keyword_type.or(syn.name_class).unwrap_or(d.syn_type),
            syn_operator: syn.operator.unwrap_or(d.syn_operator),
            syn_punctuation: syn.punctuation.unwrap_or(d.syn_punctuation),
            syn_name: syn.name.unwrap_or(d.text_primary),
            syn_name_builtin: syn.name_builtin.unwrap_or(d.syn_name_builtin),
            // Sidebar pill / meta (fall back to base text colors)
            pill_draft_bg: pill.draft_bg.unwrap_or(text_faint),
            pill_open_bg: pill.open_bg.unwrap_or(text_success),
            pill_closed_bg: pill.closed_bg.unwrap_or(text_error),
            pill_merged_bg: pill.merged_bg.unwrap_or(text_actor),
            pill_fg: pill.fg.unwrap_or(d.pill_fg),
            pill_branch: pill.branch.unwrap_or(text_primary),
            pill_author: pill.author.unwrap_or(text_actor),
            pill_age: pill.age.unwrap_or(text_faint),
            pill_role: pill.role.unwrap_or(text_secondary),
            pill_separator: pill.separator.unwrap_or(text_faint),
            // Footer
            footer_prs: theme.colors.footer.prs.unwrap_or(d.footer_prs),
            footer_issues: theme.colors.footer.issues.unwrap_or(d.footer_issues),
            footer_actions: theme.colors.footer.actions.unwrap_or(d.footer_actions),
            footer_notifications: theme
                .colors
                .footer
                .notifications
                .unwrap_or(d.footer_notifications),
            footer_repo: theme.colors.footer.repo.unwrap_or(d.footer_repo),
            icons: ResolvedIcons::resolve(&theme.icons),
        }
    }
}

/// Default color values for a given terminal background.
struct Defaults {
    text_primary: Color,
    text_secondary: Color,
    text_inverted: Color,
    text_faint: Color,
    text_warning: Color,
    text_success: Color,
    text_error: Color,
    text_actor: Color,
    bg_selected: Color,
    border_primary: Color,
    border_secondary: Color,
    border_faint: Color,
    // Markdown
    md_heading: Color,
    md_code: Color,
    md_code_block: Color,
    md_link: Color,
    md_link_text: Color,
    // Pill
    pill_fg: Color,
    // Syntax
    syn_keyword: Color,
    syn_string: Color,
    syn_comment: Color,
    syn_number: Color,
    syn_function: Color,
    syn_type: Color,
    syn_operator: Color,
    syn_punctuation: Color,
    syn_name_builtin: Color,
    // Footer
    footer_prs: Color,
    footer_issues: Color,
    footer_actions: Color,
    footer_notifications: Color,
    footer_repo: Color,
}

impl Defaults {
    fn for_background(bg: Background) -> Self {
        match bg {
            Background::Dark => Self {
                text_primary: Color::Ansi256(7),
                text_secondary: Color::Ansi256(245),
                text_inverted: Color::Ansi256(0),
                text_faint: Color::Ansi256(243),
                text_warning: Color::Ansi256(11),
                text_success: Color::Ansi256(10),
                text_error: Color::Ansi256(1),
                text_actor: Color::Ansi256(6),
                bg_selected: Color::Ansi256(237),
                border_primary: Color::Ansi256(244),
                border_secondary: Color::Ansi256(243),
                border_faint: Color::Ansi256(241),
                // Pill
                pill_fg: Color::Ansi256(15), // white
                // Markdown
                md_heading: Color::Ansi256(12), // bright blue
                md_code: Color::Ansi256(180),   // sand/gold
                md_code_block: Color::Ansi256(245),
                md_link: Color::Ansi256(4),      // blue
                md_link_text: Color::Ansi256(6), // cyan
                // Syntax
                syn_keyword: Color::Ansi256(5),   // magenta
                syn_string: Color::Ansi256(2),    // green
                syn_comment: Color::Ansi256(243), // gray
                syn_number: Color::Ansi256(3),    // yellow
                syn_function: Color::Ansi256(4),  // blue
                syn_type: Color::Ansi256(6),      // cyan
                syn_operator: Color::Ansi256(7),  // white
                syn_punctuation: Color::Ansi256(245),
                syn_name_builtin: Color::Ansi256(6),
                // Footer
                footer_prs: Color::Ansi256(4),           // blue
                footer_issues: Color::Ansi256(2),        // green
                footer_actions: Color::Ansi256(6),       // cyan
                footer_notifications: Color::Ansi256(5), // magenta
                footer_repo: Color::Ansi256(13),         // bright magenta/violet
            },
            Background::Light => Self {
                text_primary: Color::Ansi256(0),
                text_secondary: Color::Ansi256(240),
                text_inverted: Color::Ansi256(15),
                text_faint: Color::Ansi256(248),
                text_warning: Color::Ansi256(3),
                text_success: Color::Ansi256(2),
                text_error: Color::Ansi256(1),
                text_actor: Color::Ansi256(4),
                bg_selected: Color::Ansi256(254),
                border_primary: Color::Ansi256(240),
                border_secondary: Color::Ansi256(248),
                border_faint: Color::Ansi256(252),
                // Pill
                pill_fg: Color::Ansi256(15), // white
                // Markdown
                md_heading: Color::Ansi256(4), // blue
                md_code: Color::Ansi256(130),  // brown
                md_code_block: Color::Ansi256(240),
                md_link: Color::Ansi256(4),      // blue
                md_link_text: Color::Ansi256(6), // cyan
                // Syntax
                syn_keyword: Color::Ansi256(5),   // magenta
                syn_string: Color::Ansi256(2),    // green
                syn_comment: Color::Ansi256(248), // gray
                syn_number: Color::Ansi256(1),    // red
                syn_function: Color::Ansi256(4),  // blue
                syn_type: Color::Ansi256(6),      // cyan
                syn_operator: Color::Ansi256(0),  // black
                syn_punctuation: Color::Ansi256(240),
                syn_name_builtin: Color::Ansi256(6),
                // Footer
                footer_prs: Color::Ansi256(4),           // blue
                footer_issues: Color::Ansi256(2),        // green
                footer_actions: Color::Ansi256(6),       // cyan
                footer_notifications: Color::Ansi256(5), // magenta
                footer_repo: Color::Ansi256(13),         // bright magenta/violet
            },
        }
    }
}
