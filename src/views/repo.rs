use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use chrono::{DateTime, Utc};
use iocraft::prelude::*;

use crate::app::ViewKind;
use crate::color::ColorDepth;
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::config::keybindings::{
    BuiltinAction, MergedBindings, ResolvedBinding, TemplateVars, ViewContext,
    execute_shell_command, expand_template, key_event_to_string,
};
use crate::icons::ResolvedIcons;
use crate::theme::ResolvedTheme;

// ---------------------------------------------------------------------------
// T079: Branch type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct Branch {
    pub(crate) name: String,
    pub(crate) is_current: bool,
    pub(crate) last_commit_message: String,
    pub(crate) last_updated: Option<DateTime<Utc>>,
    pub(crate) ahead: u32,
    pub(crate) behind: u32,
}

// ---------------------------------------------------------------------------
// T078: Local Git operations
// ---------------------------------------------------------------------------

/// List local branches with metadata.
fn list_branches(repo_path: &Path) -> Vec<Branch> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(HEAD)|%(refname:short)|%(subject)|%(committerdate:iso8601)",
            "refs/heads",
        ])
        .current_dir(repo_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let default_branch = detect_default_branch(repo_path);

    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() < 4 {
                return None;
            }
            let is_current = parts[0].trim() == "*";
            let name = parts[1].to_owned();
            let last_commit_message = parts[2].to_owned();
            let last_updated = DateTime::parse_from_str(parts[3].trim(), "%Y-%m-%d %H:%M:%S %z")
                .ok()
                .map(|dt| dt.with_timezone(&Utc));

            let (ahead, behind) = if name == default_branch {
                (0, 0)
            } else {
                get_ahead_behind(repo_path, &name, &default_branch)
            };

            Some(Branch {
                name,
                is_current,
                last_commit_message,
                last_updated,
                ahead,
                behind,
            })
        })
        .collect()
}

fn detect_default_branch(repo_path: &Path) -> String {
    // Try symbolic-ref for HEAD's upstream default
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .current_dir(repo_path)
        .output();
    if let Ok(o) = output
        && o.status.success()
    {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_owned();
        // "origin/main" → "main"
        if let Some(branch) = s.strip_prefix("origin/") {
            return branch.to_owned();
        }
    }
    "main".to_owned()
}

fn get_ahead_behind(repo_path: &Path, branch: &str, base: &str) -> (u32, u32) {
    let output = Command::new("git")
        .args([
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base}...{branch}"),
        ])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let parts: Vec<&str> = s.trim().split('\t').collect();
            if parts.len() == 2 {
                let behind = parts[0].parse().unwrap_or(0);
                let ahead = parts[1].parse().unwrap_or(0);
                (ahead, behind)
            } else {
                (0, 0)
            }
        }
        _ => (0, 0),
    }
}

// ---------------------------------------------------------------------------
// T081: Branch actions
// ---------------------------------------------------------------------------

fn delete_branch(repo_path: &Path, branch: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["branch", "-d", branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!("Deleted branch {branch}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn create_branch(repo_path: &Path, name: &str, from: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["branch", name, from])
        .current_dir(repo_path)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!("Created branch {name} from {from}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn checkout_branch(repo_path: &Path, branch: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["checkout", branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!("Switched to {branch}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

// ---------------------------------------------------------------------------
// T080: Table columns and row conversion
// ---------------------------------------------------------------------------

fn branch_columns(icons: &ResolvedIcons) -> Vec<Column> {
    vec![
        Column {
            id: "current".to_owned(),
            header: " ".to_owned(),
            default_width_pct: 0.03,
            align: TextAlign::Center,
            fixed_width: Some(3),
        },
        Column {
            id: "name".to_owned(),
            header: "Branch".to_owned(),
            default_width_pct: 0.25,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "message".to_owned(),
            header: "Last Commit".to_owned(),
            default_width_pct: 0.35,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "ahead_behind".to_owned(),
            header: format!("{}/{}", icons.branch_ahead, icons.branch_behind),
            default_width_pct: 0.10,
            align: TextAlign::Center,
            fixed_width: Some(10),
        },
        Column {
            id: "updated".to_owned(),
            header: "Updated".to_owned(),
            default_width_pct: 0.12,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
    ]
}

fn branch_to_row(branch: &Branch, theme: &ResolvedTheme, date_format: &str) -> Row {
    let mut row = HashMap::new();

    let marker = if branch.is_current { "*" } else { " " };
    let marker_color = if branch.is_current {
        theme.text_success
    } else {
        theme.text_faint
    };
    row.insert("current".to_owned(), Cell::colored(marker, marker_color));

    let name_color = if branch.is_current {
        theme.text_success
    } else {
        theme.text_primary
    };
    row.insert("name".to_owned(), Cell::colored(&branch.name, name_color));

    row.insert(
        "message".to_owned(),
        Cell::colored(&branch.last_commit_message, theme.text_secondary),
    );

    let icons = &theme.icons;
    let ab_text = if branch.ahead == 0 && branch.behind == 0 {
        String::new()
    } else {
        format!(
            "{}{} {}{}",
            icons.branch_ahead, branch.ahead, icons.branch_behind, branch.behind
        )
    };
    row.insert(
        "ahead_behind".to_owned(),
        Cell::colored(ab_text, theme.text_faint),
    );

    let updated = branch
        .last_updated
        .as_ref()
        .map(|dt| crate::util::format_date(dt, date_format))
        .unwrap_or_default();
    row.insert(
        "updated".to_owned(),
        Cell::colored(updated, theme.text_faint),
    );

    row
}

// ---------------------------------------------------------------------------
// Input mode for branch actions (T081)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    ConfirmDelete,
    CreateBranch,
}

// ---------------------------------------------------------------------------
// T080/T082: RepoView component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct RepoViewProps<'a> {
    pub theme: Option<&'a ResolvedTheme>,
    /// Merged keybindings for help overlay.
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
    pub show_separator: bool,
    pub should_exit: Option<State<bool>>,
    pub switch_view: Option<State<bool>>,
    /// Signal to switch to the previous view.
    pub switch_view_back: Option<State<bool>>,
    /// Signal to toggle repo scope.
    pub scope_toggle: Option<State<bool>>,
    /// Active scope repo (e.g. `"owner/repo"`), or `None` for global.
    pub scope_repo: Option<String>,
    pub repo_path: Option<&'a std::path::Path>,
    pub date_format: Option<&'a str>,
    /// Whether this view is the currently active (visible) one.
    pub is_active: bool,
    /// Auto-refetch interval in minutes (0 = disabled).
    pub refetch_interval_minutes: u32,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn RepoView<'a>(props: &RepoViewProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let switch_view_back = props.switch_view_back;
    let scope_toggle = props.scope_toggle;
    let scope_repo = &props.scope_repo;
    let date_format = props.date_format.unwrap_or("relative");
    let is_active = props.is_active;

    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut input_buffer = hooks.use_state(String::new);
    let mut action_status = hooks.use_state(|| Option::<String>::None);
    let mut help_visible = hooks.use_state(|| false);

    // State: last fetch time (for status bar).
    let mut last_fetch_time = hooks.use_state(|| Option::<std::time::Instant>::None);

    // Load branches.
    let mut branches_state = hooks.use_state(Vec::<Branch>::new);
    let mut loaded = hooks.use_state(|| false);

    // Timer tick for periodic re-renders (supports auto-refetch).
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future(async move {
        loop {
            smol::Timer::after(std::time::Duration::from_secs(60)).await;
            tick.set(tick.get() + 1);
        }
    });

    // Auto-refetch if interval has elapsed (only for already-visited views).
    let refetch_interval = props.refetch_interval_minutes;
    if loaded.get()
        && is_active
        && refetch_interval > 0
        && let Some(last) = last_fetch_time.get()
        && last.elapsed() >= std::time::Duration::from_secs(u64::from(refetch_interval) * 60)
    {
        loaded.set(false);
    }

    if !loaded.get() && is_active {
        loaded.set(true);
        if let Some(repo_path) = props.repo_path {
            branches_state.set(list_branches(repo_path));
            last_fetch_time.set(Some(std::time::Instant::now()));
        }
    }

    let branches = branches_state.read();
    let total_rows = branches.len();
    let visible_rows = (props.height.saturating_sub(5) / 2).max(1) as usize;

    // Keyboard handling.
    let repo_path_owned = props.repo_path.map(std::borrow::ToOwned::to_owned);
    let keybindings = props.keybindings.cloned();
    hooks.use_terminal_events({
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code,
                kind,
                modifiers,
                ..
            }) if kind != KeyEventKind::Release => {
                // Only process events when this view is active.
                if !is_active {
                    return;
                }
                // Help overlay: intercept all keys when visible.
                if help_visible.get() {
                    if matches!(code, KeyCode::Char('?') | KeyCode::Esc) {
                        help_visible.set(false);
                    }
                    return;
                }
                let current_mode = input_mode.read().clone();
                match current_mode {
                    InputMode::ConfirmDelete => match code {
                        KeyCode::Char('y' | 'Y') => {
                            if let Some(ref repo_path) = repo_path_owned {
                                let branch_name = branches_state
                                    .read()
                                    .get(cursor.get())
                                    .map(|b| b.name.clone());
                                if let Some(name) = branch_name {
                                    match delete_branch(repo_path, &name) {
                                        Ok(msg) => {
                                            action_status.set(Some(msg));
                                            branches_state.set(list_branches(repo_path));
                                            if cursor.get() >= branches_state.read().len() {
                                                cursor.set(
                                                    branches_state.read().len().saturating_sub(1),
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            action_status.set(Some(format!("Delete failed: {e}")));
                                        }
                                    }
                                }
                            }
                            input_mode.set(InputMode::Normal);
                        }
                        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            action_status.set(Some("Cancelled".to_owned()));
                        }
                        _ => {}
                    },
                    InputMode::CreateBranch => match code {
                        KeyCode::Enter => {
                            let name = input_buffer.read().clone();
                            if !name.is_empty()
                                && let Some(ref repo_path) = repo_path_owned
                            {
                                let from = branches_state
                                    .read()
                                    .get(cursor.get())
                                    .map_or_else(|| "HEAD".to_owned(), |b| b.name.clone());
                                match create_branch(repo_path, &name, &from) {
                                    Ok(msg) => {
                                        action_status.set(Some(msg));
                                        branches_state.set(list_branches(repo_path));
                                    }
                                    Err(e) => {
                                        action_status.set(Some(format!("Create failed: {e}")));
                                    }
                                }
                            }
                            input_mode.set(InputMode::Normal);
                            input_buffer.set(String::new());
                        }
                        KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            input_buffer.set(String::new());
                        }
                        KeyCode::Backspace => {
                            let mut buf = input_buffer.read().clone();
                            buf.pop();
                            input_buffer.set(buf);
                        }
                        KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
                            let mut buf = input_buffer.read().clone();
                            buf.push(ch);
                            input_buffer.set(buf);
                        }
                        _ => {}
                    },
                    InputMode::Normal => {
                        if let Some(key_str) = key_event_to_string(code, modifiers, kind) {
                            let current_branch = branches_state
                                .read()
                                .get(cursor.get())
                                .map(|b| b.name.clone())
                                .unwrap_or_default();
                            let vars = TemplateVars {
                                head_branch: current_branch.clone(),
                                ..Default::default()
                            };
                            match keybindings
                                .as_ref()
                                .and_then(|kb| kb.resolve(&key_str, ViewContext::Branches))
                            {
                                Some(ResolvedBinding::Builtin(action)) => match action {
                                    BuiltinAction::Quit => {
                                        if let Some(mut exit) = should_exit {
                                            exit.set(true);
                                        }
                                    }
                                    BuiltinAction::SwitchView => {
                                        if let Some(mut sv) = switch_view {
                                            sv.set(true);
                                        }
                                    }
                                    BuiltinAction::SwitchViewBack => {
                                        if let Some(mut sv) = switch_view_back {
                                            sv.set(true);
                                        }
                                    }
                                    BuiltinAction::ToggleScope => {
                                        if let Some(mut st) = scope_toggle {
                                            st.set(true);
                                        }
                                    }
                                    BuiltinAction::Checkout => {
                                        if let Some(ref repo_path) = repo_path_owned {
                                            let branch_name = branches_state
                                                .read()
                                                .get(cursor.get())
                                                .map(|b| b.name.clone());
                                            if let Some(name) = branch_name {
                                                match checkout_branch(repo_path, &name) {
                                                    Ok(msg) => {
                                                        action_status.set(Some(msg));
                                                        branches_state
                                                            .set(list_branches(repo_path));
                                                    }
                                                    Err(e) => {
                                                        action_status.set(Some(format!(
                                                            "Checkout failed: {e}"
                                                        )));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    BuiltinAction::DeleteBranch => {
                                        input_mode.set(InputMode::ConfirmDelete);
                                        action_status.set(None);
                                    }
                                    BuiltinAction::NewBranch => {
                                        input_mode.set(InputMode::CreateBranch);
                                        input_buffer.set(String::new());
                                        action_status.set(None);
                                    }
                                    BuiltinAction::MoveDown => {
                                        if total_rows > 0 {
                                            let new_cursor = (cursor.get() + 1)
                                                .min(total_rows.saturating_sub(1));
                                            cursor.set(new_cursor);
                                            if new_cursor >= scroll_offset.get() + visible_rows {
                                                scroll_offset.set(
                                                    new_cursor.saturating_sub(visible_rows) + 1,
                                                );
                                            }
                                        }
                                    }
                                    BuiltinAction::MoveUp => {
                                        let new_cursor = cursor.get().saturating_sub(1);
                                        cursor.set(new_cursor);
                                        if new_cursor < scroll_offset.get() {
                                            scroll_offset.set(new_cursor);
                                        }
                                    }
                                    BuiltinAction::First => {
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    BuiltinAction::Last => {
                                        if total_rows > 0 {
                                            cursor.set(total_rows.saturating_sub(1));
                                            scroll_offset
                                                .set(total_rows.saturating_sub(visible_rows));
                                        }
                                    }
                                    BuiltinAction::PageDown => {
                                        if total_rows > 0 {
                                            let new_cursor = (cursor.get() + visible_rows)
                                                .min(total_rows.saturating_sub(1));
                                            cursor.set(new_cursor);
                                            scroll_offset
                                                .set(new_cursor.saturating_sub(
                                                    visible_rows.saturating_sub(1),
                                                ));
                                        }
                                    }
                                    BuiltinAction::PageUp => {
                                        let new_cursor = cursor.get().saturating_sub(visible_rows);
                                        cursor.set(new_cursor);
                                        scroll_offset
                                            .set(scroll_offset.get().saturating_sub(visible_rows));
                                    }
                                    BuiltinAction::HalfPageDown => {
                                        let half = visible_rows / 2;
                                        if total_rows > 0 {
                                            let new_cursor = (cursor.get() + half)
                                                .min(total_rows.saturating_sub(1));
                                            cursor.set(new_cursor);
                                            if new_cursor >= scroll_offset.get() + visible_rows {
                                                scroll_offset.set(
                                                    new_cursor.saturating_sub(visible_rows) + 1,
                                                );
                                            }
                                        }
                                    }
                                    BuiltinAction::HalfPageUp => {
                                        let half = visible_rows / 2;
                                        let new_cursor = cursor.get().saturating_sub(half);
                                        cursor.set(new_cursor);
                                        if new_cursor < scroll_offset.get() {
                                            scroll_offset.set(new_cursor);
                                        }
                                    }
                                    BuiltinAction::Refresh | BuiltinAction::RefreshAll => {
                                        loaded.set(false);
                                        action_status.set(None);
                                    }
                                    BuiltinAction::ToggleHelp => {
                                        help_visible.set(true);
                                    }
                                    BuiltinAction::CopyNumber | BuiltinAction::CopyUrl => {
                                        let _ = crate::actions::clipboard::copy_to_clipboard(
                                            &current_branch,
                                        );
                                    }
                                    _ => {}
                                },
                                Some(ResolvedBinding::ShellCommand(cmd)) => {
                                    let expanded = expand_template(&cmd, &vars);
                                    let _ = execute_shell_command(&expanded);
                                }
                                None => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    });

    // Skip heavy rendering for inactive views (all hooks above are unconditional).
    if !is_active {
        return element! {
            View(flex_direction: FlexDirection::Column)
        }
        .into_any();
    }

    // Build table.
    let columns = branch_columns(&theme.icons);
    let rows: Vec<Row> = branches
        .iter()
        .map(|b| branch_to_row(b, &theme, date_format))
        .collect();

    let rendered_table = RenderedTable::build(&TableBuildConfig {
        columns: &columns,
        rows: &rows,
        cursor: cursor.get(),
        scroll_offset: scroll_offset.get(),
        visible_rows,
        hidden_columns: None,
        width_overrides: None,
        total_width: props.width,
        depth,
        selected_bg: Some(theme.bg_selected),
        header_color: Some(theme.text_secondary),
        border_color: Some(theme.border_faint),
        show_separator: props.show_separator,
        empty_message: Some("No branches found"),
        subtitle_column: None,
        row_separator: true,
        scrollbar_thumb_color: Some(theme.footer_repo),
    });

    let tabs = vec![Tab {
        title: "Branches".to_owned(),
        count: Some(total_rows),
        is_ephemeral: false,
    }];
    let rendered_tab_bar = RenderedTabBar::build(
        &tabs,
        0,
        true,
        depth,
        Some(theme.footer_repo),
        Some(theme.footer_repo),
        Some(theme.border_faint),
        &theme.icons.tab_filter,
        &theme.icons.tab_ephemeral,
    );

    let current_mode = input_mode.read().clone();

    let rendered_text_input = match &current_mode {
        InputMode::CreateBranch => Some(crate::components::text_input::RenderedTextInput::build(
            "New branch name:",
            &input_buffer.read(),
            depth,
            Some(theme.text_primary),
            Some(theme.text_secondary),
            Some(theme.border_faint),
        )),
        InputMode::ConfirmDelete => {
            let branch_name = branches.get(cursor.get()).map_or("?", |b| b.name.as_str());
            let prompt = format!("Delete branch '{branch_name}'? (y/n)");
            Some(crate::components::text_input::RenderedTextInput::build(
                &prompt,
                "",
                depth,
                Some(theme.text_primary),
                Some(theme.text_warning),
                Some(theme.border_faint),
            ))
        }
        InputMode::Normal => None,
    };

    let context_text = if let Some(msg) = action_status.read().as_ref() {
        msg.clone()
    } else {
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        format!("Branch {cursor_pos}/{total_rows}")
    };
    let updated_text = footer::format_updated_ago(last_fetch_time.get());

    let scope_label = match scope_repo {
        Some(repo) => repo.clone(),
        None => "all repos".to_owned(),
    };
    let rendered_footer = RenderedFooter::build(
        ViewKind::Repo,
        &theme.icons,
        scope_label,
        context_text,
        updated_text,
        String::new(),
        depth,
        [
            Some(theme.footer_prs),
            Some(theme.footer_issues),
            Some(theme.footer_actions),
            Some(theme.footer_notifications),
            Some(theme.footer_repo),
        ],
        Some(theme.text_faint),
        Some(theme.text_faint),
        Some(theme.border_faint),
    );

    let rendered_help = if help_visible.get() {
        props.keybindings.map(|kb| {
            RenderedHelpOverlay::build(&HelpOverlayBuildConfig {
                bindings: kb,
                context: ViewContext::Branches,
                depth,
                title_color: Some(theme.text_primary),
                key_color: Some(theme.text_success),
                desc_color: Some(theme.text_secondary),
                border_color: Some(theme.border_primary),
            })
        })
    } else {
        None
    };

    let width = u32::from(props.width);
    let height = u32::from(props.height);

    element! {
        View(flex_direction: FlexDirection::Column, width, height) {
            TabBar(tab_bar: rendered_tab_bar)
            View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                ScrollableTable(table: rendered_table)
            }
            crate::components::text_input::TextInput(input: rendered_text_input)
            Footer(footer: rendered_footer)
            HelpOverlay(overlay: rendered_help, width: props.width, height: props.height)
        }
    }
    .into_any()
}

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> ResolvedTheme {
        default_theme()
    }

    fn sample_branch(name: &str, is_current: bool) -> Branch {
        Branch {
            name: name.to_owned(),
            is_current,
            last_commit_message: "test commit".to_owned(),
            last_updated: Some(chrono::Utc::now()),
            ahead: 2,
            behind: 1,
        }
    }

    #[test]
    fn branch_columns_has_five_entries() {
        let theme = test_theme();
        let cols = branch_columns(&theme.icons);
        assert_eq!(cols.len(), 5);
        assert_eq!(cols[0].id, "current");
        assert_eq!(cols[1].id, "name");
        assert_eq!(cols[2].id, "message");
        assert_eq!(cols[3].id, "ahead_behind");
        assert_eq!(cols[4].id, "updated");
    }

    #[test]
    fn branch_to_row_current_marker() {
        let theme = test_theme();
        let branch = sample_branch("main", true);
        let row = branch_to_row(&branch, &theme, "relative");
        assert_eq!(row.get("current").unwrap().text(), "*");
    }

    #[test]
    fn branch_to_row_non_current_marker() {
        let theme = test_theme();
        let branch = sample_branch("feature", false);
        let row = branch_to_row(&branch, &theme, "relative");
        assert_eq!(row.get("current").unwrap().text(), " ");
    }

    #[test]
    fn branch_to_row_name() {
        let theme = test_theme();
        let branch = sample_branch("feature-xyz", false);
        let row = branch_to_row(&branch, &theme, "relative");
        assert_eq!(row.get("name").unwrap().text(), "feature-xyz");
    }

    #[test]
    fn branch_to_row_ahead_behind_nonzero() {
        let theme = test_theme();
        let branch = sample_branch("dev", false);
        let row = branch_to_row(&branch, &theme, "relative");
        let ab = &row.get("ahead_behind").unwrap().text();
        assert!(ab.contains('2'), "should contain ahead count");
        assert!(ab.contains('1'), "should contain behind count");
    }

    #[test]
    fn branch_to_row_ahead_behind_zero() {
        let theme = test_theme();
        let mut branch = sample_branch("main", true);
        branch.ahead = 0;
        branch.behind = 0;
        let row = branch_to_row(&branch, &theme, "relative");
        assert_eq!(row.get("ahead_behind").unwrap().text(), "");
    }

    #[test]
    fn branch_to_row_commit_message() {
        let theme = test_theme();
        let mut branch = sample_branch("fix", false);
        branch.last_commit_message = "fix: resolve bug".to_owned();
        let row = branch_to_row(&branch, &theme, "relative");
        assert_eq!(row.get("message").unwrap().text(), "fix: resolve bug");
    }

    #[test]
    fn list_branches_nonexistent_path_returns_empty() {
        let branches = list_branches(Path::new("/nonexistent/path"));
        assert!(branches.is_empty());
    }

    #[test]
    fn detect_default_branch_nonexistent_path_returns_main() {
        let default = detect_default_branch(Path::new("/nonexistent/path"));
        assert_eq!(default, "main");
    }

    #[test]
    fn get_ahead_behind_nonexistent_returns_zero() {
        let (ahead, behind) = get_ahead_behind(Path::new("/nonexistent/path"), "foo", "bar");
        assert_eq!(ahead, 0);
        assert_eq!(behind, 0);
    }

    #[test]
    fn delete_branch_nonexistent_path_returns_err() {
        let result = delete_branch(Path::new("/nonexistent/path"), "foo");
        assert!(result.is_err());
    }

    #[test]
    fn create_branch_nonexistent_path_returns_err() {
        let result = create_branch(Path::new("/nonexistent/path"), "foo", "HEAD");
        assert!(result.is_err());
    }

    #[test]
    fn checkout_branch_nonexistent_path_returns_err() {
        let result = checkout_branch(Path::new("/nonexistent/path"), "foo");
        assert!(result.is_err());
    }
}
