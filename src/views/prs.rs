use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_compat::Compat;
use iocraft::prelude::*;
use octocrab::Octocrab;

use crate::actions::{clipboard, pr_actions};
use crate::app::ViewKind;
use crate::color::{Color as AppColor, ColorDepth};
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::sidebar::{RenderedSidebar, Sidebar, SidebarTab};
use crate::components::sidebar_tabs;
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, Span, TableBuildConfig,
};
use crate::components::text_input::{RenderedTextInput, TextInput};
use crate::config::keybindings::{MergedBindings, ViewContext};
use crate::config::types::PrSection;
use crate::filter;
use crate::github::graphql::{self, PrDetail};
use crate::github::rate_limit;
use crate::github::types::PullRequest;
use crate::icons::ResolvedIcons;
use crate::markdown::renderer::{self, StyledLine};
use crate::theme::ResolvedTheme;

// ---------------------------------------------------------------------------
// PR-specific column definitions (FR-011)
// ---------------------------------------------------------------------------

fn pr_columns(icons: &ResolvedIcons) -> Vec<Column> {
    vec![
        Column {
            id: "state".to_owned(),
            header: icons.header_state.clone(),
            default_width_pct: 0.03,
            align: TextAlign::Center,
            fixed_width: Some(3),
        },
        Column {
            id: "info".to_owned(),
            header: "Title".to_owned(),
            default_width_pct: 0.35,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "comments".to_owned(),
            header: icons.header_comments.clone(),
            default_width_pct: 0.04,
            align: TextAlign::Right,
            fixed_width: Some(6),
        },
        Column {
            id: "review".to_owned(),
            header: icons.header_review.clone(),
            default_width_pct: 0.04,
            align: TextAlign::Center,
            fixed_width: Some(6),
        },
        Column {
            id: "ci".to_owned(),
            header: icons.header_ci.clone(),
            default_width_pct: 0.04,
            align: TextAlign::Center,
            fixed_width: Some(6),
        },
        Column {
            id: "lines".to_owned(),
            header: icons.header_lines.clone(),
            default_width_pct: 0.10,
            align: TextAlign::Right,
            fixed_width: None,
        },
        Column {
            id: "updated".to_owned(),
            header: icons.header_time.clone(),
            default_width_pct: 0.06,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
        Column {
            id: "created".to_owned(),
            header: icons.header_time.clone(),
            default_width_pct: 0.06,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
    ]
}

/// Convert a `PullRequest` into a table `Row`.
#[allow(clippy::too_many_lines)]
fn pr_to_row(pr: &PullRequest, theme: &ResolvedTheme, date_format: &str) -> Row {
    let mut row = HashMap::new();

    // State indicator
    let icons = &theme.icons;
    let (state_icon, state_color) = if pr.is_draft {
        (&icons.pr_draft, theme.text_faint)
    } else {
        match pr.state {
            crate::github::types::PrState::Open => (&icons.pr_open, theme.text_success),
            crate::github::types::PrState::Closed => (&icons.pr_closed, theme.text_error),
            crate::github::types::PrState::Merged => (&icons.pr_merged, theme.text_actor),
        }
    };
    row.insert(
        "state".to_owned(),
        Cell::colored(state_icon.clone(), state_color),
    );

    // Info line: repo/name #N by @author
    let repo_name = pr
        .repo
        .as_ref()
        .map_or_else(String::new, crate::github::types::RepoRef::full_name);
    let author = pr.author.as_ref().map_or("unknown", |a| a.login.as_str());
    row.insert(
        "info".to_owned(),
        Cell::from_spans(vec![
            Span {
                text: repo_name,
                color: Some(theme.text_secondary),
                bold: false,
            },
            Span {
                text: format!(" #{}", pr.number),
                color: Some(theme.text_primary),
                bold: false,
            },
            Span {
                text: " by ".to_owned(),
                color: Some(theme.text_faint),
                bold: false,
            },
            Span {
                text: format!("@{author}"),
                color: Some(theme.text_actor),
                bold: false,
            },
        ]),
    );

    // Subtitle: PR title (extracted by subtitle_column)
    row.insert(
        "subtitle".to_owned(),
        Cell::colored(&pr.title, theme.text_primary),
    );

    // Comments
    let comments = if pr.comment_count > 0 {
        pr.comment_count.to_string()
    } else {
        String::new()
    };
    row.insert(
        "comments".to_owned(),
        Cell::colored(comments, theme.text_secondary),
    );

    // Review status
    let (review_text, review_color) = match pr.review_decision {
        Some(crate::github::types::ReviewDecision::Approved) => {
            (&icons.review_approved, theme.text_success)
        }
        Some(crate::github::types::ReviewDecision::ChangesRequested) => {
            (&icons.review_changes, theme.text_warning)
        }
        Some(crate::github::types::ReviewDecision::ReviewRequired) => {
            (&icons.review_required, theme.text_faint)
        }
        None => (&icons.review_none, theme.text_faint),
    };
    row.insert(
        "review".to_owned(),
        Cell::colored(review_text.clone(), review_color),
    );

    // CI status (aggregate from check runs)
    let (ci_text, ci_color) = aggregate_ci_status(&pr.check_runs, theme);
    row.insert("ci".to_owned(), Cell::colored(ci_text, ci_color));

    // Lines changed: green/red like gh-dash
    row.insert(
        "lines".to_owned(),
        Cell::from_spans(vec![
            Span {
                text: format!("+{}", pr.additions),
                color: Some(theme.text_success),
                bold: false,
            },
            Span {
                text: " ".to_owned(),
                color: None,
                bold: false,
            },
            Span {
                text: format!("-{}", pr.deletions),
                color: Some(theme.text_error),
                bold: false,
            },
        ]),
    );

    // Updated
    let updated = crate::util::format_date(&pr.updated_at, date_format);
    row.insert(
        "updated".to_owned(),
        Cell::colored(updated, theme.text_faint),
    );

    // Created
    let created = crate::util::format_date(&pr.created_at, date_format);
    row.insert(
        "created".to_owned(),
        Cell::colored(created, theme.text_faint),
    );

    row
}

/// Aggregate CI check runs into a single status icon.
fn aggregate_ci_status(
    checks: &[crate::github::types::CheckRun],
    theme: &ResolvedTheme,
) -> (String, AppColor) {
    use crate::github::types::{CheckConclusion, CheckStatus};

    let icons = &theme.icons;

    if checks.is_empty() {
        return (icons.ci_none.clone(), theme.text_faint);
    }

    let any_failing = checks.iter().any(|c| {
        matches!(
            c.conclusion,
            Some(CheckConclusion::Failure | CheckConclusion::TimedOut | CheckConclusion::Cancelled)
        )
    });
    if any_failing {
        return (icons.ci_failure.clone(), theme.text_error);
    }

    let any_pending = checks.iter().any(|c| {
        matches!(
            c.status,
            Some(CheckStatus::InProgress | CheckStatus::Queued)
        ) || (matches!(c.status, Some(CheckStatus::Completed)) && c.conclusion.is_none())
    });
    if any_pending {
        return (icons.ci_pending.clone(), theme.text_warning);
    }

    (icons.ci_success.clone(), theme.text_success)
}

// ---------------------------------------------------------------------------
// Input mode / action state (T058, T061)
// ---------------------------------------------------------------------------

/// What mode the input handler is in.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    /// Normal navigation mode.
    Normal,
    /// Typing a comment; buffer accumulates chars.
    Comment,
    /// Confirmation prompt for a destructive action (y/n).
    Confirm(PendingAction),
    /// Search/filter mode (T087).
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingAction {
    Close,
    Reopen,
    Merge,
}

// ---------------------------------------------------------------------------
// Section state
// ---------------------------------------------------------------------------

/// State for a single PR section.
#[derive(Debug, Clone)]
struct SectionData {
    rows: Vec<Row>,
    /// PR bodies for preview (indexed same as rows).
    bodies: Vec<String>,
    /// PR titles for sidebar header.
    titles: Vec<String>,
    /// Full PR data for actions.
    prs: Vec<PullRequest>,
    pr_count: usize,
    loading: bool,
    error: Option<String>,
}

impl Default for SectionData {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            bodies: Vec::new(),
            titles: Vec::new(),
            prs: Vec::new(),
            pr_count: 0,
            loading: true,
            error: None,
        }
    }
}

/// Shared state across all sections (stored in a single State handle).
#[derive(Debug, Clone)]
struct PrsState {
    sections: Vec<SectionData>,
}

// ---------------------------------------------------------------------------
// PrsView component (T029-T033 + T040 preview pane + T061-T062 actions)
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct PrsViewProps<'a> {
    /// PR section configs.
    pub sections: Option<&'a [PrSection]>,
    /// Octocrab instance for fetching.
    pub octocrab: Option<&'a Arc<Octocrab>>,
    /// Resolved theme.
    pub theme: Option<&'a ResolvedTheme>,
    /// Merged keybindings for help overlay.
    pub keybindings: Option<&'a MergedBindings>,
    /// Color depth.
    pub color_depth: ColorDepth,
    /// Available width.
    pub width: u16,
    /// Available height.
    pub height: u16,
    /// Preview pane width fraction (from config defaults.preview.width).
    pub preview_width_pct: f64,
    /// Whether section counts are shown in tabs.
    pub show_section_count: bool,
    /// Whether table separators are shown.
    pub show_separator: bool,
    /// Signal to exit the app.
    pub should_exit: Option<State<bool>>,
    /// Signal to switch to another view.
    pub switch_view: Option<State<bool>>,
    /// Repo paths for checkout (from `config.repo_paths`).
    pub repo_paths: Option<&'a HashMap<String, std::path::PathBuf>>,
    /// Date format string (from `config.defaults.date_format`).
    pub date_format: Option<&'a str>,
    /// Whether this view is the currently active (visible) one.
    pub is_active: bool,
    /// Auto-refetch interval in minutes (0 = disabled).
    pub refetch_interval_minutes: u32,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn PrsView<'a>(props: &PrsViewProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let sections_cfg = props.sections.unwrap_or(&[]);
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let section_count = sections_cfg.len();
    let is_active = props.is_active;
    let preview_pct = if props.preview_width_pct > 0.0 {
        props.preview_width_pct
    } else {
        0.45
    };

    // State: active section index, cursor, scroll offset.
    let mut active_section = hooks.use_state(|| 0usize);
    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);

    // State: preview pane.
    let mut preview_open = hooks.use_state(|| false);
    let mut preview_scroll = hooks.use_state(|| 0usize);

    // State: sidebar tab (T072 — FR-014).
    let mut sidebar_tab = hooks.use_state(|| SidebarTab::Overview);

    // State: cached PR detail data for sidebar tabs.
    let mut detail_data = hooks.use_state(|| Option::<(u64, PrDetail)>::None);
    let mut detail_fetch_key = hooks.use_state(|| Option::<u64>::None);

    // State: input mode for actions (T058, T061).
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut input_buffer = hooks.use_state(String::new);
    let mut action_status = hooks.use_state(|| Option::<String>::None);

    // State: search query (T087).
    let mut search_query = hooks.use_state(String::new);

    let mut help_visible = hooks.use_state(|| false);

    // State: last fetch time (for status bar).
    let mut last_fetch_time = hooks.use_state(|| Option::<std::time::Instant>::None);

    // State: loaded section data (non-Copy, use .read()/.set()).
    let initial_sections = vec![SectionData::default(); section_count];
    let mut prs_state = hooks.use_state(move || PrsState {
        sections: initial_sections,
    });
    let mut fetch_triggered = hooks.use_state(|| false);

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
    if fetch_triggered.get()
        && is_active
        && refetch_interval > 0
        && let Some(last) = last_fetch_time.get()
        && last.elapsed() >= std::time::Duration::from_secs(u64::from(refetch_interval) * 60)
    {
        fetch_triggered.set(false);
        prs_state.set(PrsState {
            sections: vec![SectionData::default(); section_count],
        });
    }

    // Trigger data fetch on first visit to this view.
    if !fetch_triggered.get()
        && is_active
        && !sections_cfg.is_empty()
        && let Some(octocrab) = props.octocrab
    {
        fetch_triggered.set(true);
        let octocrab = Arc::clone(octocrab);
        let configs: Vec<(String, u32)> = sections_cfg
            .iter()
            .map(|s| (s.filters.clone(), s.limit.unwrap_or(30)))
            .collect();
        let theme_clone = theme.clone();
        let date_format_owned = props.date_format.unwrap_or("relative").to_owned();

        smol::spawn(Compat::new(async move {
            let mut new_sections = Vec::new();
            for (filters, limit) in &configs {
                match graphql::search_pull_requests_all(&octocrab, filters, *limit).await {
                    Ok(prs) => {
                        let rows: Vec<Row> = prs
                            .iter()
                            .map(|pr| pr_to_row(pr, &theme_clone, &date_format_owned))
                            .collect();
                        let bodies: Vec<String> = prs.iter().map(|pr| pr.body.clone()).collect();
                        let titles: Vec<String> = prs.iter().map(|pr| pr.title.clone()).collect();
                        let pr_count = prs.len();
                        new_sections.push(SectionData {
                            rows,
                            bodies,
                            titles,
                            prs,
                            pr_count,
                            loading: false,
                            error: None,
                        });
                    }
                    Err(e) => {
                        let error_msg = if rate_limit::is_rate_limited(&e) {
                            rate_limit::format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        };
                        new_sections.push(SectionData {
                            loading: false,
                            error: Some(error_msg),
                            ..SectionData::default()
                        });
                    }
                }
            }
            prs_state.set(PrsState {
                sections: new_sections,
            });
            last_fetch_time.set(Some(std::time::Instant::now()));
        }))
        .detach();
    }

    // Read current state for rendering.
    let state_ref = prs_state.read();
    let current_section_idx = active_section
        .get()
        .min(state_ref.sections.len().saturating_sub(1));
    let all_rows_count = state_ref
        .sections
        .get(current_section_idx)
        .map_or(0, |s| s.rows.len());
    let search_q = search_query.read().clone();
    let total_rows = if search_q.is_empty() {
        all_rows_count
    } else {
        state_ref
            .sections
            .get(current_section_idx)
            .map_or(0, |s| filter::filter_rows(&s.rows, &search_q).len())
    };

    // Reserve space for tab bar (2 lines), footer (2 lines), header (1 line).
    // Each PR row occupies 2 terminal lines (info + subtitle).
    let visible_rows = (props.height.saturating_sub(5) / 2) as usize;

    // Clone octocrab for action closures.
    let octocrab_for_actions = props.octocrab.map(Arc::clone);
    let repo_paths = props.repo_paths.cloned().unwrap_or_default();

    // Keyboard handling.
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

                // Read input mode into a local to avoid borrow conflict.
                let current_mode = input_mode.read().clone();
                match current_mode {
                    InputMode::Comment => match code {
                        // Submit comment with Ctrl+D.
                        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                            let comment_text = input_buffer.read().clone();
                            if !comment_text.is_empty()
                                && let Some(ref octocrab) = octocrab_for_actions
                            {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_section_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    let text = comment_text.clone();
                                    smol::spawn(Compat::new(async move {
                                        match pr_actions::add_comment(
                                            &octocrab, &owner, &repo, number, &text,
                                        )
                                        .await
                                        {
                                            Ok(()) => action_status.set(Some(format!(
                                                "Comment added to PR #{number}"
                                            ))),
                                            Err(e) => action_status
                                                .set(Some(format!("Comment failed: {e}"))),
                                        }
                                    }))
                                    .detach();
                                }
                            }
                            input_mode.set(InputMode::Normal);
                            input_buffer.set(String::new());
                        }
                        // Cancel with Esc.
                        KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            input_buffer.set(String::new());
                        }
                        // Backspace.
                        KeyCode::Backspace => {
                            let mut buf = input_buffer.read().clone();
                            buf.pop();
                            input_buffer.set(buf);
                        }
                        // Character input.
                        KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
                            let mut buf = input_buffer.read().clone();
                            buf.push(ch);
                            input_buffer.set(buf);
                        }
                        KeyCode::Enter => {
                            let mut buf = input_buffer.read().clone();
                            buf.push('\n');
                            input_buffer.set(buf);
                        }
                        _ => {}
                    },
                    InputMode::Confirm(ref pending) => match code {
                        KeyCode::Char('y' | 'Y') => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_section_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    let action = pending.clone();
                                    let action_name = match pending {
                                        PendingAction::Close => "Closed",
                                        PendingAction::Reopen => "Reopened",
                                        PendingAction::Merge => "Merged",
                                    };
                                    let action_label = action_name.to_owned();
                                    smol::spawn(Compat::new(async move {
                                        let result = match action {
                                            PendingAction::Close => {
                                                pr_actions::close(&octocrab, &owner, &repo, number)
                                                    .await
                                            }
                                            PendingAction::Reopen => {
                                                pr_actions::reopen(&octocrab, &owner, &repo, number)
                                                    .await
                                            }
                                            PendingAction::Merge => {
                                                pr_actions::merge(&octocrab, &owner, &repo, number)
                                                    .await
                                            }
                                        };
                                        match result {
                                            Ok(()) => action_status
                                                .set(Some(format!("{action_label} PR #{number}"))),
                                            Err(e) => action_status
                                                .set(Some(format!("{action_label} failed: {e}"))),
                                        }
                                    }))
                                    .detach();
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
                    InputMode::Search => match code {
                        KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            search_query.set(String::new());
                        }
                        KeyCode::Enter => {
                            // Confirm search: stay filtered but exit search input.
                            input_mode.set(InputMode::Normal);
                        }
                        KeyCode::Backspace => {
                            let mut q = search_query.read().clone();
                            q.pop();
                            search_query.set(q);
                            cursor.set(0);
                            scroll_offset.set(0);
                        }
                        KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
                            let mut q = search_query.read().clone();
                            q.push(ch);
                            search_query.set(q);
                            cursor.set(0);
                            scroll_offset.set(0);
                        }
                        _ => {}
                    },
                    InputMode::Normal => match code {
                        // Quit
                        KeyCode::Char('q') => {
                            if let Some(mut exit) = should_exit {
                                exit.set(true);
                            }
                        }
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            if let Some(mut exit) = should_exit {
                                exit.set(true);
                            }
                        }
                        // Switch view
                        KeyCode::Char('s') => {
                            if let Some(mut sv) = switch_view {
                                sv.set(true);
                            }
                        }
                        // Toggle preview pane
                        KeyCode::Char('p') => {
                            preview_open.set(!preview_open.get());
                            preview_scroll.set(0);
                        }
                        // --- PR Actions (T061) ---
                        // Approve
                        KeyCode::Char('v') => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_section_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    smol::spawn(Compat::new(async move {
                                        match pr_actions::approve(
                                            &octocrab, &owner, &repo, number, None,
                                        )
                                        .await
                                        {
                                            Ok(()) => action_status
                                                .set(Some(format!("Approved PR #{number}"))),
                                            Err(e) => action_status
                                                .set(Some(format!("Approve failed: {e}"))),
                                        }
                                    }))
                                    .detach();
                                }
                            }
                        }
                        // Comment
                        KeyCode::Char('c') => {
                            input_mode.set(InputMode::Comment);
                            input_buffer.set(String::new());
                            action_status.set(None);
                        }
                        // Close (with confirmation)
                        KeyCode::Char('x') => {
                            input_mode.set(InputMode::Confirm(PendingAction::Close));
                            action_status.set(None);
                        }
                        // Reopen (with confirmation)
                        KeyCode::Char('X') => {
                            input_mode.set(InputMode::Confirm(PendingAction::Reopen));
                            action_status.set(None);
                        }
                        // Merge (with confirmation)
                        KeyCode::Char('m') => {
                            input_mode.set(InputMode::Confirm(PendingAction::Merge));
                            action_status.set(None);
                        }
                        // Update branch (plain u, not Ctrl+u)
                        KeyCode::Char('u') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_section_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    smol::spawn(Compat::new(async move {
                                        match pr_actions::update_branch(
                                            &octocrab, &owner, &repo, number,
                                        )
                                        .await
                                        {
                                            Ok(()) => action_status.set(Some(format!(
                                                "Updated PR #{number} from base"
                                            ))),
                                            Err(e) => action_status
                                                .set(Some(format!("Update failed: {e}"))),
                                        }
                                    }))
                                    .detach();
                                }
                            }
                        }
                        // Ready for review
                        KeyCode::Char('W') => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_section_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    smol::spawn(Compat::new(async move {
                                        match pr_actions::ready_for_review(
                                            &octocrab, &owner, &repo, number,
                                        )
                                        .await
                                        {
                                            Ok(()) => action_status.set(Some(format!(
                                                "PR #{number} marked ready for review"
                                            ))),
                                            Err(e) => action_status
                                                .set(Some(format!("Mark ready failed: {e}"))),
                                        }
                                    }))
                                    .detach();
                                }
                            }
                        }
                        // Diff (plain d, not Ctrl+d)
                        KeyCode::Char('d') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            let pr_info =
                                get_current_pr_info(&prs_state, current_section_idx, cursor.get());
                            if let Some((owner, repo, number)) = pr_info {
                                match pr_actions::open_diff(&owner, &repo, number) {
                                    Ok(msg) => action_status.set(Some(msg)),
                                    Err(e) => {
                                        action_status.set(Some(format!("Diff error: {e}")));
                                    }
                                }
                            }
                        }
                        // Checkout
                        KeyCode::Char(' ') => {
                            let current_data =
                                prs_state.read().sections.get(current_section_idx).cloned();
                            if let Some(data) = current_data
                                && let Some(pr) = data.prs.get(cursor.get())
                            {
                                let repo_name = pr
                                    .repo
                                    .as_ref()
                                    .map(crate::github::types::RepoRef::full_name)
                                    .unwrap_or_default();
                                match pr_actions::checkout_branch(
                                    &pr.head_ref,
                                    &repo_name,
                                    &repo_paths,
                                ) {
                                    Ok(msg) => action_status.set(Some(msg)),
                                    Err(e) => {
                                        action_status.set(Some(format!("Checkout error: {e}")));
                                    }
                                }
                            }
                        }
                        // Assign (self)
                        KeyCode::Char('a') => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_section_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    smol::spawn(Compat::new(async move {
                                        let result = async {
                                            let user = octocrab.current().user().await?;
                                            pr_actions::assign(
                                                &octocrab,
                                                &owner,
                                                &repo,
                                                number,
                                                &user.login,
                                            )
                                            .await
                                        }
                                        .await;
                                        match result {
                                            Ok(()) => action_status
                                                .set(Some(format!("Assigned to PR #{number}"))),
                                            Err(e) => action_status
                                                .set(Some(format!("Assign failed: {e}"))),
                                        }
                                    }))
                                    .detach();
                                }
                            }
                        }
                        // Unassign (self)
                        KeyCode::Char('A') => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_section_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    smol::spawn(Compat::new(async move {
                                        let result = async {
                                            let user = octocrab.current().user().await?;
                                            pr_actions::unassign(
                                                &octocrab,
                                                &owner,
                                                &repo,
                                                number,
                                                &user.login,
                                            )
                                            .await
                                        }
                                        .await;
                                        match result {
                                            Ok(()) => action_status
                                                .set(Some(format!("Unassigned from PR #{number}"))),
                                            Err(e) => action_status
                                                .set(Some(format!("Unassign failed: {e}"))),
                                        }
                                    }))
                                    .detach();
                                }
                            }
                        }
                        // --- Clipboard & Browser (T091, T092) ---
                        // Copy issue number
                        KeyCode::Char('y') => {
                            let pr_info =
                                get_current_pr_info(&prs_state, current_section_idx, cursor.get());
                            if let Some((_, _, number)) = pr_info {
                                let text = number.to_string();
                                match clipboard::copy_to_clipboard(&text) {
                                    Ok(()) => action_status.set(Some(format!("Copied #{number}"))),
                                    Err(e) => action_status.set(Some(format!("Copy failed: {e}"))),
                                }
                            }
                        }
                        // Copy PR URL
                        KeyCode::Char('Y') => {
                            let pr_info =
                                get_current_pr_info(&prs_state, current_section_idx, cursor.get());
                            if let Some((owner, repo, number)) = pr_info {
                                let url =
                                    format!("https://github.com/{owner}/{repo}/pull/{number}");
                                match clipboard::copy_to_clipboard(&url) {
                                    Ok(()) => {
                                        action_status
                                            .set(Some(format!("Copied URL for #{number}")));
                                    }
                                    Err(e) => action_status.set(Some(format!("Copy failed: {e}"))),
                                }
                            }
                        }
                        // Open in browser
                        KeyCode::Char('o') => {
                            let pr_info =
                                get_current_pr_info(&prs_state, current_section_idx, cursor.get());
                            if let Some((owner, repo, number)) = pr_info {
                                let url =
                                    format!("https://github.com/{owner}/{repo}/pull/{number}");
                                match clipboard::open_in_browser(&url) {
                                    Ok(()) => action_status.set(Some(format!("Opened #{number}"))),
                                    Err(e) => action_status.set(Some(format!("Open failed: {e}"))),
                                }
                            }
                        }
                        // Retry / refresh
                        KeyCode::Char('r') => {
                            fetch_triggered.set(false);
                            prs_state.set(PrsState {
                                sections: vec![SectionData::default(); section_count],
                            });
                            cursor.set(0);
                            scroll_offset.set(0);
                        }
                        // --- Search (T087) ---
                        KeyCode::Char('/') => {
                            input_mode.set(InputMode::Search);
                            search_query.set(String::new());
                        }
                        // --- Navigation ---
                        KeyCode::Down | KeyCode::Char('j') => {
                            if total_rows > 0 {
                                let new_cursor =
                                    (cursor.get() + 1).min(total_rows.saturating_sub(1));
                                cursor.set(new_cursor);
                                if new_cursor >= scroll_offset.get() + visible_rows {
                                    scroll_offset.set(new_cursor.saturating_sub(visible_rows) + 1);
                                }
                                // Reset preview scroll on cursor change.
                                preview_scroll.set(0);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let new_cursor = cursor.get().saturating_sub(1);
                            cursor.set(new_cursor);
                            if new_cursor < scroll_offset.get() {
                                scroll_offset.set(new_cursor);
                            }
                            preview_scroll.set(0);
                        }
                        // Jump to top/bottom
                        KeyCode::Char('g') => {
                            cursor.set(0);
                            scroll_offset.set(0);
                            preview_scroll.set(0);
                        }
                        KeyCode::Char('G') => {
                            if total_rows > 0 {
                                cursor.set(total_rows.saturating_sub(1));
                                scroll_offset.set(total_rows.saturating_sub(visible_rows));
                                preview_scroll.set(0);
                            }
                        }
                        // Page up/down
                        KeyCode::PageDown => {
                            if total_rows > 0 {
                                let new_cursor =
                                    (cursor.get() + visible_rows).min(total_rows.saturating_sub(1));
                                cursor.set(new_cursor);
                                scroll_offset
                                    .set(new_cursor.saturating_sub(visible_rows.saturating_sub(1)));
                                preview_scroll.set(0);
                            }
                        }
                        // Ctrl+d/u: scroll preview if open, else scroll table
                        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                            if preview_open.get() {
                                let half = visible_rows / 2;
                                preview_scroll.set(preview_scroll.get() + half);
                            } else if total_rows > 0 {
                                let half = visible_rows / 2;
                                let new_cursor =
                                    (cursor.get() + half).min(total_rows.saturating_sub(1));
                                cursor.set(new_cursor);
                                if new_cursor >= scroll_offset.get() + visible_rows {
                                    scroll_offset.set(new_cursor.saturating_sub(visible_rows) + 1);
                                }
                            }
                        }
                        KeyCode::PageUp => {
                            let new_cursor = cursor.get().saturating_sub(visible_rows);
                            cursor.set(new_cursor);
                            scroll_offset.set(scroll_offset.get().saturating_sub(visible_rows));
                            preview_scroll.set(0);
                        }
                        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                            if preview_open.get() {
                                let half = visible_rows / 2;
                                preview_scroll.set(preview_scroll.get().saturating_sub(half));
                            } else {
                                let half = visible_rows / 2;
                                let new_cursor = cursor.get().saturating_sub(half);
                                cursor.set(new_cursor);
                                if new_cursor < scroll_offset.get() {
                                    scroll_offset.set(new_cursor);
                                }
                            }
                        }
                        // Sidebar tab cycling (T072)
                        KeyCode::Char(']') => {
                            sidebar_tab.set(sidebar_tab.get().next());
                            preview_scroll.set(0);
                        }
                        KeyCode::Char('[') => {
                            sidebar_tab.set(sidebar_tab.get().prev());
                            preview_scroll.set(0);
                        }
                        // Help overlay
                        KeyCode::Char('?') => {
                            help_visible.set(true);
                        }
                        // Section switching
                        KeyCode::Char('h') | KeyCode::Left => {
                            if section_count > 0 {
                                let current = active_section.get();
                                active_section.set(if current == 0 {
                                    section_count.saturating_sub(1)
                                } else {
                                    current - 1
                                });
                                cursor.set(0);
                                scroll_offset.set(0);
                                preview_scroll.set(0);
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            if section_count > 0 {
                                active_section.set((active_section.get() + 1) % section_count);
                                cursor.set(0);
                                scroll_offset.set(0);
                                preview_scroll.set(0);
                            }
                        }
                        _ => {}
                    },
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

    // Build tabs.
    let tabs: Vec<Tab> = sections_cfg
        .iter()
        .enumerate()
        .map(|(i, s)| Tab {
            title: s.title.clone(),
            count: state_ref.sections.get(i).map(|d| d.pr_count),
        })
        .collect();

    // Current section data.
    let current_data = state_ref.sections.get(current_section_idx);
    let columns = pr_columns(&theme.icons);

    // Layout config for hidden/width overrides.
    let layout = sections_cfg
        .get(current_section_idx)
        .and_then(|s| s.layout.as_ref());
    let hidden_set: HashSet<String> = layout
        .map(|l| l.hidden.iter().cloned().collect())
        .unwrap_or_default();
    let width_map: HashMap<String, u16> = layout.map(|l| l.widths.clone()).unwrap_or_default();

    // Compute widths for table vs sidebar.
    let is_preview_open = preview_open.get();
    let (table_width, sidebar_width) = if is_preview_open {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let sb_w = (f64::from(props.width) * preview_pct).round() as u16;
        let tb_w = props.width.saturating_sub(sb_w);
        (tb_w, sb_w)
    } else {
        (props.width, 0)
    };

    let all_rows: &[Row] = current_data.map_or(&[], |d| d.rows.as_slice());
    let query = search_query.read().clone();
    let filtered_indices = filter::filter_rows(all_rows, &query);
    let filtered_rows: Vec<Row> = filtered_indices
        .iter()
        .filter_map(|&i| all_rows.get(i).cloned())
        .collect();

    // Pre-render table.
    let rendered_table = RenderedTable::build(&TableBuildConfig {
        columns: &columns,
        rows: &filtered_rows,
        cursor: cursor.get(),
        scroll_offset: scroll_offset.get(),
        visible_rows,
        hidden_columns: Some(&hidden_set),
        width_overrides: Some(&width_map),
        total_width: table_width,
        depth,
        selected_bg: Some(theme.bg_selected),
        header_color: Some(theme.text_secondary),
        border_color: Some(theme.border_faint),
        show_separator: props.show_separator,
        empty_message: if search_q.is_empty() {
            Some("No pull requests found")
        } else {
            Some("No pull requests match this filter")
        },
        subtitle_column: Some("subtitle"),
        row_separator: true,
    });

    // Trigger detail fetch when sidebar is open and cursor changes.
    if is_preview_open {
        let cursor_idx = cursor.get();
        let current_pr = current_data.and_then(|d| d.prs.get(cursor_idx));
        if let Some(pr) = current_pr {
            let pr_number = pr.number;
            // Only fetch if we haven't fetched for this PR yet.
            if !detail_fetch_key.read().is_some_and(|k| k == pr_number)
                && let Some(repo_ref) = &pr.repo
                && let Some(octocrab) = props.octocrab
            {
                detail_fetch_key.set(Some(pr_number));
                let octocrab = Arc::clone(octocrab);
                let owner = repo_ref.owner.clone();
                let repo = repo_ref.name.clone();
                smol::spawn(Compat::new(async move {
                    if let Ok(detail) =
                        graphql::fetch_pr_detail(&octocrab, &owner, &repo, pr_number).await
                    {
                        detail_data.set(Some((pr_number, detail)));
                    }
                }))
                .detach();
            }
        }
    }

    // Pre-render sidebar (preview pane with tabs).
    let rendered_sidebar = if is_preview_open {
        let cursor_idx = cursor.get();
        let title = current_data
            .and_then(|d| d.titles.get(cursor_idx))
            .map_or("Preview", String::as_str);

        let current_tab = sidebar_tab.get();
        let current_pr = current_data.and_then(|d| d.prs.get(cursor_idx));
        let cached_detail = detail_data.read();
        let detail_for_pr = current_pr.and_then(|pr| {
            cached_detail.as_ref().and_then(
                |(num, d)| {
                    if *num == pr.number { Some(d) } else { None }
                },
            )
        });

        let md_lines: Vec<StyledLine> = match current_tab {
            SidebarTab::Overview => {
                // Metadata + markdown body
                let body = current_data
                    .and_then(|d| d.bodies.get(cursor_idx))
                    .map_or("", String::as_str);
                let mut lines = Vec::new();
                if let Some(pr) = current_pr {
                    lines.extend(sidebar_tabs::render_overview_metadata(pr, &theme));
                }
                if !body.is_empty() {
                    lines.extend(renderer::render_markdown(body, &theme, depth));
                }
                lines
            }
            SidebarTab::Activity => {
                if let Some(detail) = detail_for_pr {
                    sidebar_tabs::render_activity(detail, &theme)
                } else {
                    vec![StyledLine::from_span(
                        crate::markdown::renderer::StyledSpan::text("Loading...", theme.text_faint),
                    )]
                }
            }
            SidebarTab::Commits => {
                if let Some(detail) = detail_for_pr {
                    sidebar_tabs::render_commits(detail, &theme)
                } else {
                    vec![StyledLine::from_span(
                        crate::markdown::renderer::StyledSpan::text("Loading...", theme.text_faint),
                    )]
                }
            }
            SidebarTab::Checks => {
                if let Some(pr) = current_pr {
                    sidebar_tabs::render_checks(pr, &theme)
                } else {
                    Vec::new()
                }
            }
            SidebarTab::Files => {
                if let Some(detail) = detail_for_pr {
                    sidebar_tabs::render_files(detail, &theme)
                } else {
                    vec![StyledLine::from_span(
                        crate::markdown::renderer::StyledSpan::text("Loading...", theme.text_faint),
                    )]
                }
            }
        };

        // Account for tab bar (2 extra lines) in sidebar height.
        let sidebar_visible_lines = props.height.saturating_sub(9) as usize;

        Some(RenderedSidebar::build_tabbed(
            title,
            &md_lines,
            preview_scroll.get(),
            sidebar_visible_lines,
            sidebar_width,
            depth,
            Some(theme.text_primary),
            Some(theme.border_faint),
            Some(theme.text_faint),
            Some(current_tab),
            Some(&theme.icons),
        ))
    } else {
        None
    };

    let rendered_tab_bar = RenderedTabBar::build(
        &tabs,
        current_section_idx,
        props.show_section_count,
        depth,
        Some(theme.border_primary),
        Some(theme.text_faint),
        Some(theme.border_faint),
    );

    // Build footer or input area based on mode.
    let current_mode = input_mode.read().clone();

    let rendered_text_input = match &current_mode {
        InputMode::Comment => Some(RenderedTextInput::build(
            "Comment (Ctrl+D to submit, Esc to cancel):",
            &input_buffer.read(),
            depth,
            Some(theme.text_primary),
            Some(theme.text_secondary),
            Some(theme.border_faint),
        )),
        InputMode::Confirm(action) => {
            let prompt = match action {
                PendingAction::Close => "Close this PR? (y/n)",
                PendingAction::Reopen => "Reopen this PR? (y/n)",
                PendingAction::Merge => "Merge this PR? (y/n)",
            };
            Some(RenderedTextInput::build(
                prompt,
                "",
                depth,
                Some(theme.text_primary),
                Some(theme.text_warning),
                Some(theme.border_faint),
            ))
        }
        InputMode::Search => Some(RenderedTextInput::build(
            "/",
            &search_query.read(),
            depth,
            Some(theme.text_primary),
            Some(theme.text_secondary),
            Some(theme.border_faint),
        )),
        InputMode::Normal => None,
    };

    let context_text = if let Some(msg) = action_status.read().as_ref() {
        msg.clone()
    } else if current_data.is_some_and(|d| d.loading) {
        "Fetching PRs...".to_owned()
    } else if let Some(err) = current_data.and_then(|d| d.error.as_ref()) {
        format!("Error: {err}")
    } else {
        let total = current_data.map_or(0, |d| d.pr_count);
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        if search_q.is_empty() {
            format!("PR {cursor_pos}/{total}")
        } else {
            format!("PR {cursor_pos}/{total_rows} (filtered from {total})")
        }
    };
    let updated_text = footer::format_updated_ago(last_fetch_time.get());

    let rendered_footer = RenderedFooter::build(
        ViewKind::Prs,
        &theme.icons,
        context_text,
        updated_text,
        depth,
        Some(theme.border_primary),
        Some(theme.text_faint),
        Some(theme.text_faint),
        Some(theme.border_faint),
    );

    let rendered_help = if help_visible.get() {
        props.keybindings.map(|kb| {
            RenderedHelpOverlay::build(&HelpOverlayBuildConfig {
                bindings: kb,
                context: ViewContext::Prs,
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

            View(flex_grow: 1.0, flex_direction: FlexDirection::Row) {
                View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                    ScrollableTable(table: rendered_table)
                }
                Sidebar(sidebar: rendered_sidebar)
            }

            TextInput(input: rendered_text_input)
            Footer(footer: rendered_footer)
            HelpOverlay(overlay: rendered_help, width: props.width, height: props.height)
        }
    }
    .into_any()
}

/// Extract (owner, repo, number) from the current PR at cursor position.
fn get_current_pr_info(
    prs_state: &State<PrsState>,
    section_idx: usize,
    cursor: usize,
) -> Option<(String, String, u64)> {
    let state = prs_state.read();
    let section = state.sections.get(section_idx)?;
    let pr = section.prs.get(cursor)?;
    let repo_ref = pr.repo.as_ref()?;
    Some((repo_ref.owner.clone(), repo_ref.name.clone(), pr.number))
}

/// Fallback theme when none is provided.
fn default_theme() -> ResolvedTheme {
    super::default_theme()
}
