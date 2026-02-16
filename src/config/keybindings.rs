use anyhow::{Context as _, Result};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Config types (T063)
// ---------------------------------------------------------------------------

/// A single key binding: maps a key chord to either a built-in action or a
/// shell command template.
#[derive(Debug, Clone, Deserialize)]
pub struct Keybinding {
    pub key: String,
    pub builtin: Option<String>,
    pub command: Option<String>,
    pub name: Option<String>,
}

/// All keybinding overrides from the config file.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct KeybindingsConfig {
    pub universal: Vec<Keybinding>,
    pub prs: Vec<Keybinding>,
    pub issues: Vec<Keybinding>,
    pub branches: Vec<Keybinding>,
}

/// View-independent action identifier used for dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltinAction {
    // Universal
    MoveDown,
    MoveUp,
    First,
    Last,
    PageDown,
    PageUp,
    PrevSection,
    NextSection,
    TogglePreview,
    OpenBrowser,
    Refresh,
    RefreshAll,
    Search,
    CopyNumber,
    CopyUrl,
    ToggleHelp,
    Quit,
    // PR
    Approve,
    Assign,
    Unassign,
    CommentAction,
    ViewDiff,
    Checkout,
    Close,
    Reopen,
    MarkReady,
    Merge,
    UpdateFromBase,
    // Issues
    LabelAction,
    // Notifications
    MarkDone,
    MarkAllDone,
    MarkRead,
    MarkAllRead,
    Unsubscribe,
    // Branches
    DeleteBranch,
    NewBranch,
    CreatePrFromBranch,
    ViewPrsForBranch,
    // View switching
    SwitchView,
    SwitchViewBack,
    // Scope
    ToggleScope,
}

impl BuiltinAction {
    /// Parse a builtin action name from the config string.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "move_down" => Self::MoveDown,
            "move_up" => Self::MoveUp,
            "first" => Self::First,
            "last" => Self::Last,
            "page_down" => Self::PageDown,
            "page_up" => Self::PageUp,
            "prev_section" => Self::PrevSection,
            "next_section" => Self::NextSection,
            "toggle_preview" => Self::TogglePreview,
            "open_browser" => Self::OpenBrowser,
            "refresh" => Self::Refresh,
            "refresh_all" => Self::RefreshAll,
            "search" => Self::Search,
            "copy_number" => Self::CopyNumber,
            "copy_url" => Self::CopyUrl,
            "toggle_help" => Self::ToggleHelp,
            "quit" => Self::Quit,
            "approve" => Self::Approve,
            "assign" => Self::Assign,
            "unassign" => Self::Unassign,
            "comment" => Self::CommentAction,
            "view_diff" => Self::ViewDiff,
            "checkout" => Self::Checkout,
            "close" => Self::Close,
            "reopen" => Self::Reopen,
            "mark_ready" => Self::MarkReady,
            "merge" => Self::Merge,
            "update_from_base" => Self::UpdateFromBase,
            "label" => Self::LabelAction,
            "mark_done" => Self::MarkDone,
            "mark_all_done" => Self::MarkAllDone,
            "mark_read" => Self::MarkRead,
            "mark_all_read" => Self::MarkAllRead,
            "unsubscribe" => Self::Unsubscribe,
            "delete_branch" => Self::DeleteBranch,
            "new_branch" => Self::NewBranch,
            "create_pr_from_branch" => Self::CreatePrFromBranch,
            "view_prs_for_branch" => Self::ViewPrsForBranch,
            "switch_view" => Self::SwitchView,
            "switch_view_back" => Self::SwitchViewBack,
            "toggle_scope" => Self::ToggleScope,
            _ => return None,
        })
    }

    /// Human-readable description of this action (for help overlay).
    pub fn description(self) -> &'static str {
        match self {
            Self::MoveDown => "Move cursor down",
            Self::MoveUp => "Move cursor up",
            Self::First => "Jump to first item",
            Self::Last => "Jump to last item",
            Self::PageDown => "Page down",
            Self::PageUp => "Page up",
            Self::PrevSection => "Previous section",
            Self::NextSection => "Next section",
            Self::TogglePreview => "Toggle preview pane",
            Self::OpenBrowser => "Open in browser",
            Self::Refresh => "Refresh current section",
            Self::RefreshAll => "Refresh all sections",
            Self::Search => "Search / filter",
            Self::CopyNumber => "Copy number to clipboard",
            Self::CopyUrl => "Copy URL to clipboard",
            Self::ToggleHelp => "Toggle help overlay",
            Self::Quit => "Quit",
            Self::Approve => "Approve",
            Self::Assign => "Assign",
            Self::Unassign => "Unassign",
            Self::CommentAction => "Comment",
            Self::ViewDiff => "View diff in pager",
            Self::Checkout => "Checkout branch",
            Self::Close => "Close",
            Self::Reopen => "Reopen",
            Self::MarkReady => "Mark as ready for review",
            Self::Merge => "Merge",
            Self::UpdateFromBase => "Update from base branch",
            Self::LabelAction => "Label (autocomplete)",
            Self::MarkDone => "Mark as done",
            Self::MarkAllDone => "Mark all as done",
            Self::MarkRead => "Mark as read",
            Self::MarkAllRead => "Mark all as read",
            Self::Unsubscribe => "Unsubscribe",
            Self::DeleteBranch => "Delete branch",
            Self::NewBranch => "Create new branch",
            Self::CreatePrFromBranch => "Create PR from branch",
            Self::ViewPrsForBranch => "View PRs for branch",
            Self::SwitchView => "Switch view",
            Self::SwitchViewBack => "Switch view back",
            Self::ToggleScope => "Toggle repo scope",
        }
    }
}

/// Resolved binding: what to do when a key is pressed.
#[derive(Debug, Clone)]
pub enum ResolvedBinding {
    Builtin(BuiltinAction),
    ShellCommand(String),
}

// ---------------------------------------------------------------------------
// Key string conversion (T063)
// ---------------------------------------------------------------------------

use iocraft::prelude::{KeyCode, KeyEventKind, KeyModifiers};

/// Convert a crossterm `KeyEvent` to our canonical key string format.
///
/// Examples: `"j"`, `"G"`, `"ctrl+c"`, `"alt+d"`, `"enter"`, `"space"`,
/// `"delete"`, `"pagedown"`, `"up"`, `"?"`.
pub fn key_event_to_string(
    code: KeyCode,
    modifiers: KeyModifiers,
    kind: KeyEventKind,
) -> Option<String> {
    if kind == KeyEventKind::Release {
        return None;
    }

    let base = match code {
        KeyCode::Char(c) => {
            // For ctrl+<char>, use lowercase in the key string.
            if modifiers.contains(KeyModifiers::CONTROL) {
                Some(c.to_ascii_lowercase().to_string())
            } else {
                Some(c.to_string())
            }
        }
        KeyCode::Enter => Some("enter".to_owned()),
        KeyCode::Esc => Some("esc".to_owned()),
        KeyCode::Backspace => Some("backspace".to_owned()),
        KeyCode::Tab => Some("tab".to_owned()),
        KeyCode::Delete => Some("delete".to_owned()),
        KeyCode::Up => Some("up".to_owned()),
        KeyCode::Down => Some("down".to_owned()),
        KeyCode::Left => Some("left".to_owned()),
        KeyCode::Right => Some("right".to_owned()),
        KeyCode::PageUp => Some("pageup".to_owned()),
        KeyCode::PageDown => Some("pagedown".to_owned()),
        KeyCode::Home => Some("home".to_owned()),
        KeyCode::End => Some("end".to_owned()),
        KeyCode::F(n) => Some(format!("f{n}")),
        _ => None,
    }?;

    // Build modifier prefix (don't add shift prefix for regular chars, since
    // the char itself encodes the case, e.g. 'G' vs 'g').
    let mut prefix = String::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        prefix.push_str("ctrl+");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        prefix.push_str("alt+");
    }

    Some(format!("{prefix}{base}"))
}

// ---------------------------------------------------------------------------
// Default keybindings (T063 — FR-100)
// ---------------------------------------------------------------------------

fn kb(key: &str, builtin: &str, name: &str) -> Keybinding {
    Keybinding {
        key: key.to_owned(),
        builtin: Some(builtin.to_owned()),
        command: None,
        name: Some(name.to_owned()),
    }
}

/// Default universal keybindings (all views).
pub fn default_universal() -> Vec<Keybinding> {
    vec![
        kb("j", "move_down", "Move cursor down"),
        kb("down", "move_down", "Move cursor down"),
        kb("k", "move_up", "Move cursor up"),
        kb("up", "move_up", "Move cursor up"),
        kb("g", "first", "Jump to first item"),
        kb("home", "first", "Jump to first item"),
        kb("G", "last", "Jump to last item"),
        kb("end", "last", "Jump to last item"),
        kb("ctrl+d", "page_down", "Page down"),
        kb("pagedown", "page_down", "Page down"),
        kb("ctrl+u", "page_up", "Page up"),
        kb("pageup", "page_up", "Page up"),
        kb("h", "prev_section", "Previous section"),
        kb("left", "prev_section", "Previous section"),
        kb("l", "next_section", "Next section"),
        kb("right", "next_section", "Next section"),
        kb("p", "toggle_preview", "Toggle preview pane"),
        kb("o", "open_browser", "Open in browser"),
        kb("r", "refresh", "Refresh current section"),
        kb("R", "refresh_all", "Refresh all sections"),
        kb("/", "search", "Search / filter"),
        kb("y", "copy_number", "Copy number"),
        kb("Y", "copy_url", "Copy URL"),
        kb("?", "toggle_help", "Toggle help"),
        kb("q", "quit", "Quit"),
        kb("ctrl+c", "quit", "Quit"),
    ]
}

/// Default PR view keybindings.
pub fn default_prs() -> Vec<Keybinding> {
    vec![
        kb("v", "approve", "Approve"),
        kb("a", "assign", "Assign (multi, autocomplete)"),
        kb("ctrl+a", "assign_self", "Assign to me"),
        kb("A", "unassign", "Unassign"),
        kb("c", "comment", "Comment"),
        kb("d", "view_diff", "View diff"),
        kb("C", "checkout", "Checkout branch"),
        kb("space", "checkout", "Checkout branch"),
        kb("x", "close", "Close PR"),
        kb("X", "reopen", "Reopen PR"),
        kb("W", "mark_ready", "Mark ready for review"),
        kb("m", "merge", "Merge PR"),
        kb("u", "update_from_base", "Update from base"),
        kb("n", "switch_view", "Switch view"),
        kb("N", "switch_view_back", "Switch view back"),
        kb("S", "toggle_scope", "Toggle repo scope"),
    ]
}

/// Default Issue view keybindings.
pub(crate) fn default_issues() -> Vec<Keybinding> {
    vec![
        kb("L", "label", "Label (autocomplete)"),
        kb("a", "assign", "Assign (multi, autocomplete)"),
        kb("ctrl+a", "assign_self", "Assign to me"),
        kb("A", "unassign", "Unassign"),
        kb("c", "comment", "Comment"),
        kb("x", "close", "Close issue"),
        kb("X", "reopen", "Reopen issue"),
        kb("n", "switch_view", "Switch view"),
        kb("N", "switch_view_back", "Switch view back"),
        kb("S", "toggle_scope", "Toggle repo scope"),
    ]
}

/// Default Notification view keybindings.
pub(crate) fn default_notifications() -> Vec<Keybinding> {
    vec![
        kb("d", "mark_done", "Mark as done"),
        kb("D", "mark_all_done", "Mark all as done"),
        kb("m", "mark_read", "Mark as read"),
        kb("M", "mark_all_read", "Mark all as read"),
        kb("u", "unsubscribe", "Unsubscribe"),
        kb("n", "switch_view", "Switch view"),
        kb("N", "switch_view_back", "Switch view back"),
        kb("S", "toggle_scope", "Toggle repo scope"),
    ]
}

/// Default Branch view keybindings.
pub(crate) fn default_branches() -> Vec<Keybinding> {
    vec![
        kb("delete", "delete_branch", "Delete branch"),
        kb("+", "new_branch", "Create new branch"),
        kb("p", "create_pr_from_branch", "Create PR from branch"),
        kb("v", "view_prs_for_branch", "View PRs for branch"),
        kb("n", "switch_view", "Switch view"),
        kb("N", "switch_view_back", "Switch view back"),
        kb("S", "toggle_scope", "Toggle repo scope"),
    ]
}

// ---------------------------------------------------------------------------
// Merged keybinding set (T063)
// ---------------------------------------------------------------------------

/// A fully resolved keybinding map: defaults merged with user overrides.
///
/// User overrides replace defaults for the same key.
#[derive(Debug, Clone)]
pub struct MergedBindings {
    pub universal: Vec<Keybinding>,
    pub prs: Vec<Keybinding>,
    pub issues: Vec<Keybinding>,
    pub notifications: Vec<Keybinding>,
    pub branches: Vec<Keybinding>,
}

impl MergedBindings {
    /// Merge user config overrides on top of defaults.
    ///
    /// For each context, user bindings for a given key replace the default
    /// binding for that key. User bindings for keys not in defaults are appended.
    pub fn from_config(config: &KeybindingsConfig) -> Self {
        Self {
            universal: merge_lists(&default_universal(), &config.universal),
            prs: merge_lists(&default_prs(), &config.prs),
            issues: merge_lists(&default_issues(), &config.issues),
            notifications: merge_lists(&default_notifications(), &[]),
            branches: merge_lists(&default_branches(), &config.branches),
        }
    }

    /// Look up a key string, checking context-specific bindings first, then
    /// universal. Returns the resolved binding if found.
    pub fn resolve(&self, key: &str, context: ViewContext) -> Option<ResolvedBinding> {
        let context_bindings = match context {
            ViewContext::Prs => &self.prs,
            ViewContext::Issues => &self.issues,
            ViewContext::Notifications => &self.notifications,
            ViewContext::Branches => &self.branches,
        };

        if let Some(binding) = find_binding(context_bindings, key) {
            return Some(binding);
        }
        find_binding(&self.universal, key)
    }

    /// Return all bindings for a given context, grouped as
    /// `(context_label, bindings)` pairs. Universal bindings come first.
    pub fn all_for_context(&self, context: ViewContext) -> Vec<(&'static str, &[Keybinding])> {
        let context_bindings = match context {
            ViewContext::Prs => ("PR", self.prs.as_slice()),
            ViewContext::Issues => ("Issue", self.issues.as_slice()),
            ViewContext::Notifications => ("Notification", self.notifications.as_slice()),
            ViewContext::Branches => ("Branch", self.branches.as_slice()),
        };

        vec![("Universal", self.universal.as_slice()), context_bindings]
    }
}

/// View context for keybinding resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewContext {
    Prs,
    Issues,
    Notifications,
    Branches,
}

/// Merge user overrides on top of defaults. User bindings for the same key
/// replace the default; additional user bindings are appended.
fn merge_lists(defaults: &[Keybinding], overrides: &[Keybinding]) -> Vec<Keybinding> {
    let override_keys: std::collections::HashSet<&str> =
        overrides.iter().map(|b| b.key.as_str()).collect();

    let mut result: Vec<Keybinding> = defaults
        .iter()
        .filter(|b| !override_keys.contains(b.key.as_str()))
        .cloned()
        .collect();

    result.extend(overrides.iter().cloned());
    result
}

impl KeybindingsConfig {
    /// Look up a key string in the given context, returning the resolved binding.
    ///
    /// Resolution order: context-specific bindings first, then universal.
    pub fn resolve(&self, key: &str, context: &[Keybinding]) -> Option<ResolvedBinding> {
        // Context-specific first.
        if let Some(binding) = find_binding(context, key) {
            return Some(binding);
        }
        // Then universal.
        find_binding(&self.universal, key)
    }
}

fn find_binding(bindings: &[Keybinding], key: &str) -> Option<ResolvedBinding> {
    for b in bindings {
        if b.key == key {
            if let Some(ref builtin) = b.builtin
                && let Some(action) = BuiltinAction::from_name(builtin)
            {
                return Some(ResolvedBinding::Builtin(action));
            }
            if let Some(ref cmd) = b.command {
                return Some(ResolvedBinding::ShellCommand(cmd.clone()));
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Template variable expansion (T064 — FR-103)
// ---------------------------------------------------------------------------

/// Context variables available for template expansion in custom commands.
#[derive(Debug, Clone, Default)]
pub struct TemplateVars {
    pub url: String,
    pub number: String,
    pub repo_name: String,
    pub head_branch: String,
    pub base_branch: String,
}

/// Expand `{{.Var}}` template variables in a command string.
pub fn expand_template(template: &str, vars: &TemplateVars) -> String {
    template
        .replace("{{.Url}}", &vars.url)
        .replace("{{.Number}}", &vars.number)
        .replace("{{.RepoName}}", &vars.repo_name)
        .replace("{{.HeadBranch}}", &vars.head_branch)
        .replace("{{.BaseBranch}}", &vars.base_branch)
}

// ---------------------------------------------------------------------------
// Shell command execution (T065)
// ---------------------------------------------------------------------------

/// Execute a shell command (after template expansion) and return its combined
/// stdout/stderr output.
#[allow(dead_code)]
pub(crate) fn execute_shell_command(command: &str) -> Result<String> {
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .context("spawning shell command")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        Ok(stdout.trim().to_owned())
    } else {
        anyhow::bail!(
            "command failed (exit {}): {}",
            output.status.code().unwrap_or(-1),
            stderr.trim()
        )
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_event_to_string_simple_char() {
        let s = key_event_to_string(
            KeyCode::Char('j'),
            KeyModifiers::empty(),
            KeyEventKind::Press,
        );
        assert_eq!(s, Some("j".to_owned()));
    }

    #[test]
    fn key_event_to_string_uppercase() {
        let s = key_event_to_string(KeyCode::Char('G'), KeyModifiers::SHIFT, KeyEventKind::Press);
        assert_eq!(s, Some("G".to_owned()));
    }

    #[test]
    fn key_event_to_string_ctrl() {
        let s = key_event_to_string(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press,
        );
        assert_eq!(s, Some("ctrl+d".to_owned()));
    }

    #[test]
    fn key_event_to_string_alt() {
        let s = key_event_to_string(KeyCode::Char('d'), KeyModifiers::ALT, KeyEventKind::Press);
        assert_eq!(s, Some("alt+d".to_owned()));
    }

    #[test]
    fn key_event_to_string_special_keys() {
        assert_eq!(
            key_event_to_string(KeyCode::Enter, KeyModifiers::empty(), KeyEventKind::Press),
            Some("enter".to_owned())
        );
        assert_eq!(
            key_event_to_string(
                KeyCode::PageDown,
                KeyModifiers::empty(),
                KeyEventKind::Press
            ),
            Some("pagedown".to_owned())
        );
        assert_eq!(
            key_event_to_string(KeyCode::Delete, KeyModifiers::empty(), KeyEventKind::Press),
            Some("delete".to_owned())
        );
    }

    #[test]
    fn key_event_to_string_release_ignored() {
        let s = key_event_to_string(
            KeyCode::Char('j'),
            KeyModifiers::empty(),
            KeyEventKind::Release,
        );
        assert_eq!(s, None);
    }

    #[test]
    fn expand_template_all_vars() {
        let vars = TemplateVars {
            url: "https://github.com/org/repo/pull/42".to_owned(),
            number: "42".to_owned(),
            repo_name: "org/repo".to_owned(),
            head_branch: "feature-x".to_owned(),
            base_branch: "main".to_owned(),
        };
        let result = expand_template(
            "echo {{.Number}} {{.Url}} {{.RepoName}} {{.HeadBranch}} {{.BaseBranch}}",
            &vars,
        );
        assert_eq!(
            result,
            "echo 42 https://github.com/org/repo/pull/42 org/repo feature-x main"
        );
    }

    #[test]
    fn expand_template_no_vars() {
        let vars = TemplateVars::default();
        assert_eq!(expand_template("echo hello", &vars), "echo hello");
    }

    #[test]
    fn merge_override_replaces_default() {
        let defaults = vec![kb("v", "approve", "Approve")];
        let overrides = vec![kb("v", "comment", "Comment instead")];
        let merged = merge_lists(&defaults, &overrides);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].builtin.as_deref(), Some("comment"));
    }

    #[test]
    fn merge_preserves_unoverridden_defaults() {
        let defaults = vec![kb("j", "move_down", "Down"), kb("k", "move_up", "Up")];
        let overrides = vec![kb("j", "first", "First")];
        let merged = merge_lists(&defaults, &overrides);
        assert_eq!(merged.len(), 2);
        // k should be preserved.
        assert!(
            merged
                .iter()
                .any(|b| b.key == "k" && b.builtin.as_deref() == Some("move_up"))
        );
        // j should be overridden.
        assert!(
            merged
                .iter()
                .any(|b| b.key == "j" && b.builtin.as_deref() == Some("first"))
        );
    }

    #[test]
    fn merge_appends_new_user_bindings() {
        let defaults = vec![kb("j", "move_down", "Down")];
        let overrides = vec![kb("z", "quit", "Quick quit")];
        let merged = merge_lists(&defaults, &overrides);
        assert_eq!(merged.len(), 2);
        assert!(merged.iter().any(|b| b.key == "z"));
    }

    #[test]
    fn resolve_context_takes_priority() {
        let config = KeybindingsConfig::default();
        let merged = MergedBindings::from_config(&config);
        // 'n' is bound to switch_view in prs context.
        let binding = merged.resolve("n", ViewContext::Prs);
        assert!(matches!(
            binding,
            Some(ResolvedBinding::Builtin(BuiltinAction::SwitchView))
        ));
    }

    #[test]
    fn resolve_falls_through_to_universal() {
        let config = KeybindingsConfig::default();
        let merged = MergedBindings::from_config(&config);
        // 'q' is in universal only, not in prs.
        let binding = merged.resolve("q", ViewContext::Prs);
        assert!(matches!(
            binding,
            Some(ResolvedBinding::Builtin(BuiltinAction::Quit))
        ));
    }

    #[test]
    fn resolve_shell_command() {
        let config = KeybindingsConfig {
            prs: vec![Keybinding {
                key: "z".to_owned(),
                builtin: None,
                command: Some("echo {{.Number}}".to_owned()),
                name: Some("Custom".to_owned()),
            }],
            ..Default::default()
        };
        let merged = MergedBindings::from_config(&config);
        let binding = merged.resolve("z", ViewContext::Prs);
        assert!(
            matches!(binding, Some(ResolvedBinding::ShellCommand(ref cmd)) if cmd == "echo {{.Number}}")
        );
    }

    #[test]
    fn execute_shell_echo() {
        let result = execute_shell_command("echo hello");
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn default_keybindings_cover_all_universal() {
        let bindings = default_universal();
        // Verify key actions we expect.
        let has = |key: &str| bindings.iter().any(|b| b.key == key);
        assert!(has("j"));
        assert!(has("k"));
        assert!(has("q"));
        assert!(has("ctrl+c"));
        assert!(has("?"));
        assert!(has("/"));
    }
}
