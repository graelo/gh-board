use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

use crate::color::Color;
use crate::config::keybindings::KeybindingsConfig;

// ---------------------------------------------------------------------------
// Custom Color deserialization
// ---------------------------------------------------------------------------

/// Deserialize an `Option<Color>` from a TOML string value.
pub(crate) mod color_de {
    use serde::{self, Deserialize, Deserializer};

    use crate::color::Color;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Color>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: Option<String> = Option::deserialize(deserializer)?;
        match s {
            None => Ok(None),
            Some(s) => Color::parse(&s, "<theme>")
                .map(Some)
                .map_err(serde::de::Error::custom),
        }
    }
}

// ---------------------------------------------------------------------------
// Top-level config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    #[serde(default, rename = "pr_filters")]
    pub pr_filters: Vec<PrFilter>,
    #[serde(default, rename = "issues_filters")]
    pub issues_filters: Vec<IssueFilter>,
    #[serde(default, rename = "actions_filters")]
    pub actions_filters: Vec<ActionsFilter>,
    #[serde(default, rename = "notifications_filters")]
    pub notifications_filters: Vec<NotificationFilter>,
    pub github: GitHubConfig,
    pub defaults: Defaults,
    pub theme: Theme,
    pub keybindings: KeybindingsConfig,
    #[serde(default)]
    pub repo_paths: HashMap<String, PathBuf>,
    /// Path to a theme-only TOML file. Accepts:
    ///   - `"builtin:<name>"` (e.g. `"builtin:dracula"`)
    ///   - A filesystem path (e.g. `"~/.config/gh-board/themes/monokai.toml"`)
    pub theme_file: Option<String>,
}

// ---------------------------------------------------------------------------
// GitHub backend settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GitHubConfig {
    pub scope: Scope,
    pub refetch_interval_minutes: u32,
    /// Number of PR details to prefetch in the background after the list loads.
    /// `0` = on-demand only (default).
    pub prefetch_pr_details: u32,
    /// When `true`, automatically clone a repo via `gh repo clone` if the
    /// configured `repo_paths` target doesn't exist yet (checkout / worktree).
    pub auto_clone: bool,
}

impl Default for GitHubConfig {
    fn default() -> Self {
        Self {
            scope: Scope::Auto,
            refetch_interval_minutes: 10,
            prefetch_pr_details: 0,
            auto_clone: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Filters
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PrFilter {
    pub title: String,
    pub filters: String,
    pub limit: Option<u32>,
    pub host: Option<String>,
    pub layout: Option<LayoutConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssueFilter {
    pub title: String,
    pub filters: String,
    pub limit: Option<u32>,
    pub host: Option<String>,
    pub layout: Option<LayoutConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ActionsFilter {
    pub title: String,
    /// `"owner/repo"` — the repository to fetch workflow runs for.
    /// Use `"@current"` to resolve to the repository detected from the
    /// current working directory (requires running gh-board inside a git repo).
    /// The fetch is skipped when `@current` is used but no repo is in context.
    pub repo: String,
    pub host: Option<String>,
    pub limit: Option<u32>,
    /// GitHub API `status` query param: `"queued"`, `"in_progress"`, `"completed"`,
    /// `"waiting"`, `"requested"`, `"pending"`, or a conclusion value like
    /// `"failure"`, `"success"`, `"cancelled"`, …
    pub status: Option<String>,
    /// GitHub API `event` query param: `"push"`, `"pull_request"`, `"schedule"`,
    /// `"workflow_dispatch"`, …
    pub event: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationFilter {
    pub title: String,
    pub filters: String,
    pub limit: Option<u32>,
    pub host: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LayoutConfig {
    // Per-column width/hidden overrides. Specific fields TBD.
    pub hidden: Vec<String>,
    pub widths: HashMap<String, u16>,
}

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Auto,
    Repo,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum View {
    #[default]
    Prs,
    Issues,
    Actions,
    Notifications,
    Repo,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub view: View,
    pub preview: PreviewDefaults,
    pub date_format: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            view: View::Prs,
            preview: PreviewDefaults::default(),
            date_format: "relative".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PreviewDefaults {
    pub width: f64,
}

impl Default for PreviewDefaults {
    fn default() -> Self {
        Self { width: 0.45 }
    }
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub ui: UiTheme,
    pub colors: ColorsTheme,
    pub icons: IconConfig,
}

impl Theme {
    /// Merge two themes: `overlay` fields win over `base` when present (`Some`).
    ///
    /// This is used to apply a theme loaded from `theme_file` as the base,
    /// with any inline `[theme.*]` fields in the main config as the overlay.
    pub fn merge(base: Theme, overlay: Theme) -> Theme {
        Theme {
            ui: UiTheme {
                filters_show_count: overlay.ui.filters_show_count.or(base.ui.filters_show_count),
                table: TableTheme {
                    show_separator: overlay
                        .ui
                        .table
                        .show_separator
                        .or(base.ui.table.show_separator),
                    compact: overlay.ui.table.compact.or(base.ui.table.compact),
                },
            },
            colors: merge_colors(&base.colors, &overlay.colors),
            icons: merge_icons(base.icons, overlay.icons),
        }
    }
}

fn merge_colors(base: &ColorsTheme, overlay: &ColorsTheme) -> ColorsTheme {
    ColorsTheme {
        text: TextColors {
            primary: overlay.text.primary.or(base.text.primary),
            secondary: overlay.text.secondary.or(base.text.secondary),
            inverted: overlay.text.inverted.or(base.text.inverted),
            faint: overlay.text.faint.or(base.text.faint),
            warning: overlay.text.warning.or(base.text.warning),
            success: overlay.text.success.or(base.text.success),
            error: overlay.text.error.or(base.text.error),
            actor: overlay.text.actor.or(base.text.actor),
            role: overlay.text.role.or(base.text.role),
        },
        background: BgColors {
            selected: overlay.background.selected.or(base.background.selected),
        },
        border: BorderColors {
            primary: overlay.border.primary.or(base.border.primary),
            secondary: overlay.border.secondary.or(base.border.secondary),
            faint: overlay.border.faint.or(base.border.faint),
        },
        icon: IconColors {
            newcontributor: overlay.icon.newcontributor.or(base.icon.newcontributor),
            contributor: overlay.icon.contributor.or(base.icon.contributor),
            collaborator: overlay.icon.collaborator.or(base.icon.collaborator),
            member: overlay.icon.member.or(base.icon.member),
            owner: overlay.icon.owner.or(base.icon.owner),
            unknownrole: overlay.icon.unknownrole.or(base.icon.unknownrole),
        },
        pill: PillColors {
            draft_bg: overlay.pill.draft_bg.or(base.pill.draft_bg),
            open_bg: overlay.pill.open_bg.or(base.pill.open_bg),
            closed_bg: overlay.pill.closed_bg.or(base.pill.closed_bg),
            merged_bg: overlay.pill.merged_bg.or(base.pill.merged_bg),
            fg: overlay.pill.fg.or(base.pill.fg),
            branch: overlay.pill.branch.or(base.pill.branch),
            author: overlay.pill.author.or(base.pill.author),
            age: overlay.pill.age.or(base.pill.age),
            separator: overlay.pill.separator.or(base.pill.separator),
        },
        markdown: merge_markdown(&base.markdown, &overlay.markdown),
        footer: FooterColors {
            prs: overlay.footer.prs.or(base.footer.prs),
            issues: overlay.footer.issues.or(base.footer.issues),
            notifications: overlay.footer.notifications.or(base.footer.notifications),
            repo: overlay.footer.repo.or(base.footer.repo),
            actions: overlay.footer.actions.or(base.footer.actions),
        },
    }
}

fn merge_markdown(base: &MarkdownColors, overlay: &MarkdownColors) -> MarkdownColors {
    MarkdownColors {
        text: overlay.text.or(base.text),
        heading: overlay.heading.or(base.heading),
        h1: overlay.h1.or(base.h1),
        h2: overlay.h2.or(base.h2),
        h3: overlay.h3.or(base.h3),
        h4: overlay.h4.or(base.h4),
        h5: overlay.h5.or(base.h5),
        h6: overlay.h6.or(base.h6),
        code: overlay.code.or(base.code),
        code_block: overlay.code_block.or(base.code_block),
        link: overlay.link.or(base.link),
        link_text: overlay.link_text.or(base.link_text),
        image: overlay.image.or(base.image),
        image_text: overlay.image_text.or(base.image_text),
        horizontal_rule: overlay.horizontal_rule.or(base.horizontal_rule),
        strikethrough: overlay.strikethrough.or(base.strikethrough),
        emph: overlay.emph.or(base.emph),
        strong: overlay.strong.or(base.strong),
        syntax: SyntaxColors {
            text: overlay.syntax.text.or(base.syntax.text),
            background: overlay.syntax.background.or(base.syntax.background),
            error: overlay.syntax.error.or(base.syntax.error),
            error_background: overlay
                .syntax
                .error_background
                .or(base.syntax.error_background),
            comment: overlay.syntax.comment.or(base.syntax.comment),
            comment_preproc: overlay
                .syntax
                .comment_preproc
                .or(base.syntax.comment_preproc),
            keyword: overlay.syntax.keyword.or(base.syntax.keyword),
            keyword_reserved: overlay
                .syntax
                .keyword_reserved
                .or(base.syntax.keyword_reserved),
            keyword_namespace: overlay
                .syntax
                .keyword_namespace
                .or(base.syntax.keyword_namespace),
            keyword_type: overlay.syntax.keyword_type.or(base.syntax.keyword_type),
            operator: overlay.syntax.operator.or(base.syntax.operator),
            punctuation: overlay.syntax.punctuation.or(base.syntax.punctuation),
            name: overlay.syntax.name.or(base.syntax.name),
            name_builtin: overlay.syntax.name_builtin.or(base.syntax.name_builtin),
            name_tag: overlay.syntax.name_tag.or(base.syntax.name_tag),
            name_attribute: overlay.syntax.name_attribute.or(base.syntax.name_attribute),
            name_class: overlay.syntax.name_class.or(base.syntax.name_class),
            name_decorator: overlay.syntax.name_decorator.or(base.syntax.name_decorator),
            name_function: overlay.syntax.name_function.or(base.syntax.name_function),
            number: overlay.syntax.number.or(base.syntax.number),
            string: overlay.syntax.string.or(base.syntax.string),
            string_escape: overlay.syntax.string_escape.or(base.syntax.string_escape),
            deleted: overlay.syntax.deleted.or(base.syntax.deleted),
            inserted: overlay.syntax.inserted.or(base.syntax.inserted),
            subheading: overlay.syntax.subheading.or(base.syntax.subheading),
        },
    }
}

fn merge_icons(base: IconConfig, overlay: IconConfig) -> IconConfig {
    IconConfig {
        preset: overlay.preset.or(base.preset),
        pr_open: overlay.pr_open.or(base.pr_open),
        pr_closed: overlay.pr_closed.or(base.pr_closed),
        pr_merged: overlay.pr_merged.or(base.pr_merged),
        pr_draft: overlay.pr_draft.or(base.pr_draft),
        header_state: overlay.header_state.or(base.header_state),
        header_comments: overlay.header_comments.or(base.header_comments),
        header_review: overlay.header_review.or(base.header_review),
        header_ci: overlay.header_ci.or(base.header_ci),
        header_lines: overlay.header_lines.or(base.header_lines),
        header_time: overlay.header_time.or(base.header_time),
        review_approved: overlay.review_approved.or(base.review_approved),
        review_changes: overlay.review_changes.or(base.review_changes),
        review_required: overlay.review_required.or(base.review_required),
        review_none: overlay.review_none.or(base.review_none),
        review_commented: overlay.review_commented.or(base.review_commented),
        ci_success: overlay.ci_success.or(base.ci_success),
        ci_failure: overlay.ci_failure.or(base.ci_failure),
        ci_pending: overlay.ci_pending.or(base.ci_pending),
        ci_none: overlay.ci_none.or(base.ci_none),
        issue_open: overlay.issue_open.or(base.issue_open),
        issue_closed: overlay.issue_closed.or(base.issue_closed),
        notif_unread: overlay.notif_unread.or(base.notif_unread),
        notif_type_pr: overlay.notif_type_pr.or(base.notif_type_pr),
        notif_type_issue: overlay.notif_type_issue.or(base.notif_type_issue),
        notif_type_release: overlay.notif_type_release.or(base.notif_type_release),
        notif_type_discussion: overlay.notif_type_discussion.or(base.notif_type_discussion),
        branch_ahead: overlay.branch_ahead.or(base.branch_ahead),
        branch_behind: overlay.branch_behind.or(base.branch_behind),
        check_success: overlay.check_success.or(base.check_success),
        check_failure: overlay.check_failure.or(base.check_failure),
        check_pending: overlay.check_pending.or(base.check_pending),
        branch_arrow: overlay.branch_arrow.or(base.branch_arrow),
        tab_overview: overlay.tab_overview.or(base.tab_overview),
        tab_activity: overlay.tab_activity.or(base.tab_activity),
        tab_commits: overlay.tab_commits.or(base.tab_commits),
        tab_checks: overlay.tab_checks.or(base.tab_checks),
        tab_files: overlay.tab_files.or(base.tab_files),
        role_newcontributor: overlay.role_newcontributor.or(base.role_newcontributor),
        role_contributor: overlay.role_contributor.or(base.role_contributor),
        role_collaborator: overlay.role_collaborator.or(base.role_collaborator),
        role_member: overlay.role_member.or(base.role_member),
        role_owner: overlay.role_owner.or(base.role_owner),
        role_unknown: overlay.role_unknown.or(base.role_unknown),
        view_prs: overlay.view_prs.or(base.view_prs),
        view_issues: overlay.view_issues.or(base.view_issues),
        view_notifications: overlay.view_notifications.or(base.view_notifications),
        view_repo: overlay.view_repo.or(base.view_repo),
        view_actions: overlay.view_actions.or(base.view_actions),
        tab_filter: overlay.tab_filter.or(base.tab_filter),
        pill_left: overlay.pill_left.or(base.pill_left),
        pill_right: overlay.pill_right.or(base.pill_right),
        header_update: overlay.header_update.or(base.header_update),
        update_needed: overlay.update_needed.or(base.update_needed),
        update_conflict: overlay.update_conflict.or(base.update_conflict),
        update_ok: overlay.update_ok.or(base.update_ok),
        feedback_ok: overlay.feedback_ok.or(base.feedback_ok),
        feedback_error: overlay.feedback_error.or(base.feedback_error),
        tab_ephemeral: overlay.tab_ephemeral.or(base.tab_ephemeral),
        select_cursor: overlay.select_cursor.or(base.select_cursor),
        action_success: overlay.action_success.or(base.action_success),
        action_failure: overlay.action_failure.or(base.action_failure),
        action_cancelled: overlay.action_cancelled.or(base.action_cancelled),
        action_skipped: overlay.action_skipped.or(base.action_skipped),
        action_running: overlay.action_running.or(base.action_running),
        action_queued: overlay.action_queued.or(base.action_queued),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct UiTheme {
    pub filters_show_count: Option<bool>,
    pub table: TableTheme,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TableTheme {
    pub show_separator: Option<bool>,
    pub compact: Option<bool>,
}

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ColorsTheme {
    pub text: TextColors,
    pub background: BgColors,
    pub border: BorderColors,
    pub icon: IconColors,
    pub pill: PillColors,
    pub markdown: MarkdownColors,
    pub footer: FooterColors,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct FooterColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub prs: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub issues: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub notifications: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub repo: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub actions: Option<Color>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TextColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub primary: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub secondary: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub inverted: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub faint: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub warning: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub success: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub error: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub actor: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub role: Option<Color>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BgColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub selected: Option<Color>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct BorderColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub primary: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub secondary: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub faint: Option<Color>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct IconColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub newcontributor: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub contributor: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub collaborator: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub member: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub owner: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub unknownrole: Option<Color>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PillColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub draft_bg: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub open_bg: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub closed_bg: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub merged_bg: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub fg: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub branch: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub author: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub age: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub separator: Option<Color>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct MarkdownColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub text: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub heading: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub h1: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub h2: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub h3: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub h4: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub h5: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub h6: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub code: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub code_block: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub link: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub link_text: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub image: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub image_text: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub horizontal_rule: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub strikethrough: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub emph: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub strong: Option<Color>,
    #[serde(default)]
    pub syntax: SyntaxColors,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SyntaxColors {
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub text: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub background: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub error: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub error_background: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub comment: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub comment_preproc: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub keyword: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub keyword_reserved: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub keyword_namespace: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub keyword_type: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub operator: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub punctuation: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub name: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub name_builtin: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub name_tag: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub name_attribute: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub name_class: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub name_decorator: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub name_function: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub number: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub string: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub string_escape: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub deleted: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub inserted: Option<Color>,
    #[serde(default, deserialize_with = "color_de::deserialize")]
    pub subheading: Option<Color>,
}

// ---------------------------------------------------------------------------
// Icons
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct IconConfig {
    pub preset: Option<String>,
    // PR state
    pub pr_open: Option<String>,
    pub pr_closed: Option<String>,
    pub pr_merged: Option<String>,
    pub pr_draft: Option<String>,
    // PR column headers
    pub header_state: Option<String>,
    pub header_comments: Option<String>,
    pub header_review: Option<String>,
    pub header_ci: Option<String>,
    pub header_lines: Option<String>,
    pub header_time: Option<String>,
    // Review decision
    pub review_approved: Option<String>,
    pub review_changes: Option<String>,
    pub review_required: Option<String>,
    pub review_none: Option<String>,
    pub review_commented: Option<String>,
    // CI status
    pub ci_success: Option<String>,
    pub ci_failure: Option<String>,
    pub ci_pending: Option<String>,
    pub ci_none: Option<String>,
    // Issue state
    pub issue_open: Option<String>,
    pub issue_closed: Option<String>,
    // Notifications
    pub notif_unread: Option<String>,
    pub notif_type_pr: Option<String>,
    pub notif_type_issue: Option<String>,
    pub notif_type_release: Option<String>,
    pub notif_type_discussion: Option<String>,
    // Branch
    pub branch_ahead: Option<String>,
    pub branch_behind: Option<String>,
    // Sidebar checks
    pub check_success: Option<String>,
    pub check_failure: Option<String>,
    pub check_pending: Option<String>,
    // Sidebar decorative
    pub branch_arrow: Option<String>,
    // Sidebar tabs
    pub tab_overview: Option<String>,
    pub tab_activity: Option<String>,
    pub tab_commits: Option<String>,
    pub tab_checks: Option<String>,
    pub tab_files: Option<String>,
    // Author roles
    pub role_newcontributor: Option<String>,
    pub role_contributor: Option<String>,
    pub role_collaborator: Option<String>,
    pub role_member: Option<String>,
    pub role_owner: Option<String>,
    pub role_unknown: Option<String>,
    // Status bar views
    pub view_prs: Option<String>,
    pub view_issues: Option<String>,
    pub view_notifications: Option<String>,
    pub view_repo: Option<String>,
    pub view_actions: Option<String>,
    // Tab filter marker
    pub tab_filter: Option<String>,
    // Pill caps (rounded edges)
    pub pill_left: Option<String>,
    pub pill_right: Option<String>,
    // Branch update status
    pub header_update: Option<String>,
    pub update_needed: Option<String>,
    pub update_conflict: Option<String>,
    pub update_ok: Option<String>,
    // Feedback indicators
    pub feedback_ok: Option<String>,
    pub feedback_error: Option<String>,
    // UI chrome
    pub tab_ephemeral: Option<String>,
    pub select_cursor: Option<String>,
    // Actions run status
    pub action_success: Option<String>,
    pub action_failure: Option<String>,
    pub action_cancelled: Option<String>,
    pub action_skipped: Option<String>,
    pub action_running: Option<String>,
    pub action_queued: Option<String>,
}
