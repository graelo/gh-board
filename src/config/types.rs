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
    #[serde(default, rename = "pr_sections")]
    pub pr_sections: Vec<PrSection>,
    #[serde(default, rename = "issues_sections")]
    pub issues_sections: Vec<IssueSection>,
    #[serde(default, rename = "notifications_sections")]
    pub notifications_sections: Vec<NotificationSection>,
    pub defaults: Defaults,
    pub theme: Theme,
    pub keybindings: KeybindingsConfig,
    #[serde(default)]
    pub repo_paths: HashMap<String, PathBuf>,
}

// ---------------------------------------------------------------------------
// Sections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct PrSection {
    pub title: String,
    pub filters: String,
    pub limit: Option<u32>,
    pub host: Option<String>,
    pub layout: Option<LayoutConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IssueSection {
    pub title: String,
    pub filters: String,
    pub limit: Option<u32>,
    pub host: Option<String>,
    pub layout: Option<LayoutConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NotificationSection {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum View {
    #[default]
    Prs,
    Issues,
    Notifications,
    Repo,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Defaults {
    pub view: View,
    pub preview: PreviewDefaults,
    pub refetch_interval_minutes: u32,
    pub date_format: String,
}

impl Default for Defaults {
    fn default() -> Self {
        Self {
            view: View::Prs,
            preview: PreviewDefaults::default(),
            refetch_interval_minutes: 10,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiTheme {
    pub sections_show_count: bool,
    pub table: TableTheme,
}

impl Default for UiTheme {
    fn default() -> Self {
        Self {
            sections_show_count: true,
            table: TableTheme::default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TableTheme {
    pub show_separator: bool,
    pub compact: bool,
}

impl Default for TableTheme {
    fn default() -> Self {
        Self {
            show_separator: true,
            compact: false,
        }
    }
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
    pub markdown: MarkdownColors,
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
}
