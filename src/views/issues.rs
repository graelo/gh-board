use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_compat::Compat;
use iocraft::prelude::*;
use moka::future::Cache;
use octocrab::Octocrab;

use crate::actions::{clipboard, issue_actions};
use crate::app::ViewKind;
use crate::color::ColorDepth;
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::sidebar::{RenderedSidebar, Sidebar};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::components::text_input::{self, RenderedTextInput, TextInput};
use crate::config::keybindings::{MergedBindings, ViewContext};
use crate::config::types::IssueSection;
use crate::filter;
use crate::github::graphql::{self, RateLimitInfo};
use crate::github::rate_limit;
use crate::github::types::Issue;
use crate::markdown::renderer::{self, StyledLine};
use crate::theme::ResolvedTheme;

// ---------------------------------------------------------------------------
// Issue-specific column definitions (FR-021)
// ---------------------------------------------------------------------------

fn issue_columns() -> Vec<Column> {
    vec![
        Column {
            id: "state".to_owned(),
            header: " ".to_owned(),
            default_width_pct: 0.03,
            align: TextAlign::Center,
            fixed_width: Some(3),
        },
        Column {
            id: "title".to_owned(),
            header: "Title".to_owned(),
            default_width_pct: 0.30,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "repo".to_owned(),
            header: "Repo".to_owned(),
            default_width_pct: 0.14,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "creator".to_owned(),
            header: "Creator".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "assignees".to_owned(),
            header: "Assignees".to_owned(),
            default_width_pct: 0.12,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "comments".to_owned(),
            header: "Cmt".to_owned(),
            default_width_pct: 0.05,
            align: TextAlign::Right,
            fixed_width: Some(6),
        },
        Column {
            id: "reactions".to_owned(),
            header: "React".to_owned(),
            default_width_pct: 0.06,
            align: TextAlign::Right,
            fixed_width: Some(7),
        },
        Column {
            id: "updated".to_owned(),
            header: "Updated".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
        Column {
            id: "created".to_owned(),
            header: "Created".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
    ]
}

/// Convert an `Issue` into a table `Row`.
fn issue_to_row(issue: &Issue, theme: &ResolvedTheme, date_format: &str) -> Row {
    let mut row = HashMap::new();

    // State indicator
    let icons = &theme.icons;
    let (state_icon, state_color) = match issue.state {
        crate::github::types::IssueState::Open => (&icons.issue_open, theme.text_success),
        crate::github::types::IssueState::Closed | crate::github::types::IssueState::Unknown => {
            (&icons.issue_closed, theme.text_actor)
        }
    };
    row.insert(
        "state".to_owned(),
        Cell::colored(state_icon.clone(), state_color),
    );

    // Title
    row.insert("title".to_owned(), Cell::plain(&issue.title));

    // Repo
    let repo_name = issue
        .repo
        .as_ref()
        .map_or_else(String::new, crate::github::types::RepoRef::full_name);
    row.insert(
        "repo".to_owned(),
        Cell::colored(repo_name, theme.text_secondary),
    );

    // Creator
    let creator = issue
        .author
        .as_ref()
        .map_or("unknown", |a| a.login.as_str());
    row.insert(
        "creator".to_owned(),
        Cell::colored(creator, theme.text_actor),
    );

    // Assignees
    let assignees_text: String = issue
        .assignees
        .iter()
        .map(|a| a.login.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    row.insert(
        "assignees".to_owned(),
        Cell::colored(assignees_text, theme.text_faint),
    );

    // Comments
    let comments = if issue.comment_count > 0 {
        issue.comment_count.to_string()
    } else {
        String::new()
    };
    row.insert(
        "comments".to_owned(),
        Cell::colored(comments, theme.text_secondary),
    );

    // Reactions (total)
    let total_reactions = issue.reactions.total();
    let reactions = if total_reactions > 0 {
        total_reactions.to_string()
    } else {
        String::new()
    };
    row.insert(
        "reactions".to_owned(),
        Cell::colored(reactions, theme.text_secondary),
    );

    // Updated
    let updated = crate::util::format_date(&issue.updated_at, date_format);
    row.insert(
        "updated".to_owned(),
        Cell::colored(updated, theme.text_faint),
    );

    // Created
    let created = crate::util::format_date(&issue.created_at, date_format);
    row.insert(
        "created".to_owned(),
        Cell::colored(created, theme.text_faint),
    );

    row
}

// ---------------------------------------------------------------------------
// Input mode for issue actions (T086)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    Comment,
    Label,
    Assign,
    Confirm(PendingAction),
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingAction {
    Close,
    Reopen,
}

// ---------------------------------------------------------------------------
// Section state (T047)
// ---------------------------------------------------------------------------

/// State for a single Issue section.
#[derive(Debug, Clone)]
struct SectionData {
    rows: Vec<Row>,
    bodies: Vec<String>,
    titles: Vec<String>,
    issues: Vec<Issue>,
    issue_count: usize,
    loading: bool,
    error: Option<String>,
}

impl Default for SectionData {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            bodies: Vec::new(),
            titles: Vec::new(),
            issues: Vec::new(),
            issue_count: 0,
            loading: true,
            error: None,
        }
    }
}

/// Shared state across all issue sections.
#[derive(Debug, Clone)]
struct IssuesState {
    sections: Vec<SectionData>,
}

// ---------------------------------------------------------------------------
// IssuesView component (T047-T048, T086)
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct IssuesViewProps<'a> {
    pub sections: Option<&'a [IssueSection]>,
    pub octocrab: Option<&'a Arc<Octocrab>>,
    pub api_cache: Option<&'a Cache<String, String>>,
    pub theme: Option<&'a ResolvedTheme>,
    /// Merged keybindings for help overlay.
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
    pub preview_width_pct: f64,
    pub show_section_count: bool,
    pub show_separator: bool,
    pub should_exit: Option<State<bool>>,
    pub switch_view: Option<State<bool>>,
    /// Signal to switch to the previous view.
    pub switch_view_back: Option<State<bool>>,
    /// Signal to toggle repo scope.
    pub scope_toggle: Option<State<bool>>,
    /// Active scope repo (e.g. `"owner/repo"`), or `None` for global.
    pub scope_repo: Option<String>,
    pub date_format: Option<&'a str>,
    /// Whether this view is the currently active (visible) one.
    pub is_active: bool,
    /// Auto-refetch interval in minutes (0 = disabled).
    pub refetch_interval_minutes: u32,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn IssuesView<'a>(props: &IssuesViewProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let sections_cfg = props.sections.unwrap_or(&[]);
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let switch_view_back = props.switch_view_back;
    let scope_toggle = props.scope_toggle;
    let scope_repo = &props.scope_repo;
    let section_count = sections_cfg.len();
    let is_active = props.is_active;
    let preview_pct = if props.preview_width_pct > 0.0 {
        props.preview_width_pct
    } else {
        0.45
    };

    let active_section = hooks.use_state(|| 0usize);
    let cursor = hooks.use_state(|| 0usize);
    let scroll_offset = hooks.use_state(|| 0usize);
    let preview_open = hooks.use_state(|| false);
    let preview_scroll = hooks.use_state(|| 0usize);

    // Action state.
    let input_mode = hooks.use_state(|| InputMode::Normal);
    let input_buffer = hooks.use_state(String::new);
    let action_status = hooks.use_state(|| Option::<String>::None);
    let label_candidates = hooks.use_state(Vec::<String>::new);
    let label_selection = hooks.use_state(|| 0usize);

    // State: search query (T087).
    let search_query = hooks.use_state(String::new);

    let mut help_visible = hooks.use_state(|| false);

    // State: rate limit from last GraphQL response.
    let mut rate_limit_state = hooks.use_state(|| Option::<RateLimitInfo>::None);

    // State: per-section fetch tracking (lazy: only fetch the active section).
    let mut section_fetch_times =
        hooks.use_state(move || vec![Option::<std::time::Instant>::None; section_count]);
    let mut section_in_flight = hooks.use_state(move || vec![false; section_count]);

    let initial_sections = vec![SectionData::default(); section_count];
    let mut issues_state = hooks.use_state(move || IssuesState {
        sections: initial_sections,
    });

    // Track scope changes: when scope_repo changes, invalidate all sections.
    let mut last_scope = hooks.use_state(|| scope_repo.clone());
    if *last_scope.read() != *scope_repo {
        last_scope.set(scope_repo.clone());
        issues_state.set(IssuesState {
            sections: vec![SectionData::default(); section_count],
        });
        section_fetch_times.set(vec![None; section_count]);
        section_in_flight.set(vec![false; section_count]);
    }

    // Timer tick for periodic re-renders (supports auto-refetch).
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future(async move {
        loop {
            smol::Timer::after(std::time::Duration::from_secs(60)).await;
            tick.set(tick.get() + 1);
        }
    });

    // Compute active section index early (needed by fetch logic below).
    let current_section_idx = active_section
        .get()
        .min(section_count.saturating_sub(1));

    // Auto-refetch: only reset the active section when its interval has elapsed.
    let refetch_interval = props.refetch_interval_minutes;
    let needs_refetch = is_active
        && refetch_interval > 0
        && !section_in_flight
            .read()
            .get(current_section_idx)
            .copied()
            .unwrap_or(false)
        && section_fetch_times
            .read()
            .get(current_section_idx)
            .copied()
            .flatten()
            .is_some_and(|last| {
                last.elapsed()
                    >= std::time::Duration::from_secs(u64::from(refetch_interval) * 60)
            });
    if needs_refetch {
        let mut state = issues_state.read().clone();
        if current_section_idx < state.sections.len() {
            state.sections[current_section_idx] = SectionData::default();
        }
        issues_state.set(state);
        let mut times = section_fetch_times.read().clone();
        if current_section_idx < times.len() {
            times[current_section_idx] = None;
        }
        section_fetch_times.set(times);
    }

    // Clone octocrab and cache for use in action closures.
    let octocrab_for_actions = props.octocrab.map(Arc::clone);
    let api_cache_for_actions = props.api_cache.cloned();

    // Lazy fetch: only fetch the active section when it needs data.
    let active_needs_fetch = issues_state
        .read()
        .sections
        .get(current_section_idx)
        .is_some_and(|s| s.loading);
    let active_in_flight = section_in_flight
        .read()
        .get(current_section_idx)
        .copied()
        .unwrap_or(false);

    if active_needs_fetch
        && !active_in_flight
        && is_active
        && let Some(cfg) = sections_cfg.get(current_section_idx)
        && let Some(octocrab) = props.octocrab
    {
        let mut in_flight = section_in_flight.read().clone();
        if current_section_idx < in_flight.len() {
            in_flight[current_section_idx] = true;
        }
        section_in_flight.set(in_flight);

        let octocrab = Arc::clone(octocrab);
        let api_cache = props.api_cache.cloned();
        let section_idx = current_section_idx;
        let mut filters = cfg.filters.clone();
        // Inject repo scope if active and not already present.
        if let Some(ref repo) = *scope_repo
            && !filters.split_whitespace().any(|t| t.starts_with("repo:"))
        {
            filters = format!("{filters} repo:{repo}");
        }
        let limit = cfg.limit.unwrap_or(30);
        let theme_clone = theme.clone();
        let date_format_owned = props.date_format.unwrap_or("relative").to_owned();

        smol::spawn(Compat::new(async move {
            let section_data =
                match graphql::search_issues_all(&octocrab, &filters, limit, api_cache.as_ref()).await {
                    Ok((issues, rl)) => {
                        if rl.is_some() {
                            rate_limit_state.set(rl);
                        }
                        let rows: Vec<Row> = issues
                            .iter()
                            .map(|issue| issue_to_row(issue, &theme_clone, &date_format_owned))
                            .collect();
                        let bodies: Vec<String> = issues.iter().map(|i| i.body.clone()).collect();
                        let titles: Vec<String> = issues.iter().map(|i| i.title.clone()).collect();
                        let issue_count = issues.len();
                        SectionData {
                            rows,
                            bodies,
                            titles,
                            issue_count,
                            loading: false,
                            error: None,
                            issues,
                        }
                    }
                    Err(e) => {
                        let error_msg = if rate_limit::is_rate_limited(&e) {
                            rate_limit::format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        };
                        SectionData {
                            loading: false,
                            error: Some(error_msg),
                            ..SectionData::default()
                        }
                    }
                };

            let mut state = issues_state.read().clone();
            if section_idx < state.sections.len() {
                state.sections[section_idx] = section_data;
            }
            issues_state.set(state);

            let mut times = section_fetch_times.read().clone();
            if section_idx < times.len() {
                times[section_idx] = Some(std::time::Instant::now());
            }
            section_fetch_times.set(times);
            let mut in_flight = section_in_flight.read().clone();
            if section_idx < in_flight.len() {
                in_flight[section_idx] = false;
            }
            section_in_flight.set(in_flight);
        }))
        .detach();
    }

    let state_ref = issues_state.read();
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

    let visible_rows = props.height.saturating_sub(5) as usize;

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

                let current_mode = input_mode.read().clone();
                match current_mode {
                    InputMode::Comment | InputMode::Assign => {
                        handle_text_input(
                            code,
                            modifiers,
                            &current_mode,
                            input_mode,
                            input_buffer,
                            action_status,
                            &issues_state,
                            current_section_idx,
                            cursor.get(),
                            octocrab_for_actions.as_ref(),
                        );
                    }
                    InputMode::Label => {
                        handle_label_input(
                            code,
                            modifiers,
                            input_mode,
                            input_buffer,
                            action_status,
                            label_candidates,
                            label_selection,
                            &issues_state,
                            current_section_idx,
                            cursor.get(),
                            octocrab_for_actions.as_ref(),
                        );
                    }
                    InputMode::Confirm(ref pending) => {
                        handle_confirm_input(
                            code,
                            pending,
                            input_mode,
                            action_status,
                            &issues_state,
                            current_section_idx,
                            cursor.get(),
                            octocrab_for_actions.as_ref(),
                        );
                    }
                    InputMode::Search => {
                        handle_search_input(
                            code,
                            modifiers,
                            input_mode,
                            search_query,
                            cursor,
                            scroll_offset,
                        );
                    }
                    InputMode::Normal => {
                        handle_normal_input(
                            code,
                            modifiers,
                            should_exit,
                            switch_view,
                            switch_view_back,
                            scope_toggle,
                            preview_open,
                            preview_scroll,
                            cursor,
                            scroll_offset,
                            active_section,
                            input_mode,
                            input_buffer,
                            action_status,
                            label_candidates,
                            label_selection,
                            total_rows,
                            visible_rows,
                            section_count,
                            issues_state,
                            current_section_idx,
                            octocrab_for_actions.as_ref(),
                            api_cache_for_actions.as_ref(),
                            section_fetch_times,
                            help_visible,
                            rate_limit_state,
                        );
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

    // Build tabs.
    let tabs: Vec<Tab> = sections_cfg
        .iter()
        .enumerate()
        .map(|(i, s)| Tab {
            title: s.title.clone(),
            count: state_ref.sections.get(i).map(|d| d.issue_count),
        })
        .collect();

    let current_data = state_ref.sections.get(current_section_idx);
    let columns = issue_columns();

    let layout = sections_cfg
        .get(current_section_idx)
        .and_then(|s| s.layout.as_ref());
    let hidden_set: HashSet<String> = layout
        .map(|l| l.hidden.iter().cloned().collect())
        .unwrap_or_default();
    let width_map: HashMap<String, u16> = layout.map(|l| l.widths.clone()).unwrap_or_default();

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
    let filtered_indices = filter::filter_rows(all_rows, &search_q);
    let filtered_rows: Vec<Row> = filtered_indices
        .iter()
        .filter_map(|&i| all_rows.get(i).cloned())
        .collect();

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
            Some("No issues found")
        } else {
            Some("No issues match this filter")
        },
        subtitle_column: None,
        row_separator: true,
    });

    let rendered_sidebar = if is_preview_open {
        let cursor_idx = cursor.get();
        let body = current_data
            .and_then(|d| d.bodies.get(cursor_idx))
            .map_or("", String::as_str);
        let title = current_data
            .and_then(|d| d.titles.get(cursor_idx))
            .map_or("Preview", String::as_str);

        let md_lines: Vec<StyledLine> = if body.is_empty() {
            Vec::new()
        } else {
            renderer::render_markdown(body, &theme, depth)
        };

        let sidebar_visible_lines = props.height.saturating_sub(7) as usize;

        Some(RenderedSidebar::build(
            title,
            &md_lines,
            preview_scroll.get(),
            sidebar_visible_lines,
            sidebar_width,
            depth,
            Some(theme.text_primary),
            Some(theme.border_faint),
            Some(theme.text_faint),
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
        &theme.icons.tab_section,
    );

    // Build text input widget.
    let current_mode = input_mode.read().clone();
    let rendered_text_input = match &current_mode {
        InputMode::Comment => Some(RenderedTextInput::build(
            "Comment:",
            &input_buffer.read(),
            depth,
            Some(theme.text_primary),
            Some(theme.text_secondary),
            Some(theme.border_faint),
        )),
        InputMode::Assign => Some(RenderedTextInput::build(
            "Assign user:",
            &input_buffer.read(),
            depth,
            Some(theme.text_primary),
            Some(theme.text_secondary),
            Some(theme.border_faint),
        )),
        InputMode::Label => {
            let buf = input_buffer.read().clone();
            let candidates = label_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            let sel = label_selection.get();
            let selected_idx = if filtered.is_empty() {
                None
            } else {
                Some(sel.min(filtered.len().saturating_sub(1)))
            };
            Some(RenderedTextInput::build_with_suggestions(
                "Label:",
                &buf,
                depth,
                Some(theme.text_primary),
                Some(theme.text_secondary),
                Some(theme.border_faint),
                &filtered,
                selected_idx,
                Some(theme.text_inverted),
                Some(theme.bg_selected),
            ))
        }
        InputMode::Confirm(action) => {
            let prompt = match action {
                PendingAction::Close => "Close this issue? (y/n)",
                PendingAction::Reopen => "Reopen this issue? (y/n)",
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
        "Fetching issues...".to_owned()
    } else if let Some(err) = current_data.and_then(|d| d.error.as_ref()) {
        format!("Error: {err}")
    } else {
        let total = current_data.map_or(0, |d| d.issue_count);
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        if search_q.is_empty() {
            format!("Issue {cursor_pos}/{total}")
        } else {
            format!("Issue {cursor_pos}/{total_rows} (filtered from {total})")
        }
    };
    let active_fetch_time = section_fetch_times
        .read()
        .get(current_section_idx)
        .copied()
        .flatten();
    let updated_text = footer::format_updated_ago(active_fetch_time);

    let rate_limit_text = footer::format_rate_limit(rate_limit_state.read().as_ref());

    let scope_label = match scope_repo {
        Some(repo) => repo.clone(),
        None => "all repos".to_owned(),
    };
    let rendered_footer = RenderedFooter::build(
        ViewKind::Issues,
        &theme.icons,
        scope_label,
        context_text,
        updated_text,
        rate_limit_text,
        depth,
        [
            Some(theme.footer_prs),
            Some(theme.footer_issues),
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
                context: ViewContext::Issues,
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

            View(flex_grow: 1.0, flex_direction: FlexDirection::Row, overflow: Overflow::Hidden) {
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

// ---------------------------------------------------------------------------
// Keyboard input handlers (extracted to avoid too_many_lines)
// ---------------------------------------------------------------------------

fn handle_search_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    mut input_mode: State<InputMode>,
    mut search_query: State<String>,
    mut cursor: State<usize>,
    mut scroll_offset: State<usize>,
) {
    match code {
        KeyCode::Esc => {
            input_mode.set(InputMode::Normal);
            search_query.set(String::new());
        }
        KeyCode::Enter => {
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
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_text_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    current_mode: &InputMode,
    mut input_mode: State<InputMode>,
    mut input_buffer: State<String>,
    mut action_status: State<Option<String>>,
    issues_state: &State<IssuesState>,
    section_idx: usize,
    cursor: usize,
    octocrab_for_actions: Option<&Arc<Octocrab>>,
) {
    match code {
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            let text = input_buffer.read().clone();
            if !text.is_empty()
                && let Some(octocrab) = octocrab_for_actions
            {
                let info = get_current_issue_info(issues_state, section_idx, cursor);
                if let Some((owner, repo, number)) = info {
                    let octocrab = Arc::clone(octocrab);
                    let is_comment = *current_mode == InputMode::Comment;
                    smol::spawn(Compat::new(async move {
                        let result = if is_comment {
                            issue_actions::add_comment(&octocrab, &owner, &repo, number, &text)
                                .await
                        } else {
                            // Assign mode
                            issue_actions::assign(&octocrab, &owner, &repo, number, &text).await
                        };
                        match result {
                            Ok(()) => {
                                let action = if is_comment {
                                    "Comment added"
                                } else {
                                    "Assigned"
                                };
                                action_status.set(Some(format!("{action} on issue #{number}")));
                            }
                            Err(e) => action_status.set(Some(format!("Action failed: {e}"))),
                        }
                    }))
                    .detach();
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
        KeyCode::Enter if *current_mode == InputMode::Comment => {
            let mut buf = input_buffer.read().clone();
            buf.push('\n');
            input_buffer.set(buf);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_label_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    mut input_mode: State<InputMode>,
    mut input_buffer: State<String>,
    mut action_status: State<Option<String>>,
    label_candidates: State<Vec<String>>,
    mut label_selection: State<usize>,
    issues_state: &State<IssuesState>,
    section_idx: usize,
    cursor: usize,
    octocrab_for_actions: Option<&Arc<Octocrab>>,
) {
    match code {
        KeyCode::Tab | KeyCode::Down => {
            let buf = input_buffer.read().clone();
            let candidates = label_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            if !filtered.is_empty() {
                label_selection.set((label_selection.get() + 1) % filtered.len());
            }
        }
        KeyCode::Up | KeyCode::BackTab => {
            let buf = input_buffer.read().clone();
            let candidates = label_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            if !filtered.is_empty() {
                let sel = label_selection.get();
                label_selection.set(if sel == 0 {
                    filtered.len() - 1
                } else {
                    sel - 1
                });
            }
        }
        KeyCode::Enter => {
            // Submit the selected label.
            let buf = input_buffer.read().clone();
            let candidates = label_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            let label = if filtered.is_empty() {
                buf.clone()
            } else {
                let sel = label_selection.get().min(filtered.len().saturating_sub(1));
                filtered[sel].clone()
            };

            if !label.is_empty()
                && let Some(octocrab) = octocrab_for_actions
            {
                let info = get_current_issue_info(issues_state, section_idx, cursor);
                if let Some((owner, repo, number)) = info {
                    let octocrab = Arc::clone(octocrab);
                    let labels = vec![label.clone()];
                    smol::spawn(Compat::new(async move {
                        match issue_actions::add_labels(&octocrab, &owner, &repo, number, &labels)
                            .await
                        {
                            Ok(()) => action_status
                                .set(Some(format!("Label '{label}' added to issue #{number}"))),
                            Err(e) => action_status.set(Some(format!("Label failed: {e}"))),
                        }
                    }))
                    .detach();
                }
            }
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            label_selection.set(0);
        }
        KeyCode::Esc => {
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            label_selection.set(0);
        }
        KeyCode::Backspace => {
            let mut buf = input_buffer.read().clone();
            buf.pop();
            input_buffer.set(buf);
            label_selection.set(0);
        }
        KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let mut buf = input_buffer.read().clone();
            buf.push(ch);
            input_buffer.set(buf);
            label_selection.set(0);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_confirm_input(
    code: KeyCode,
    pending: &PendingAction,
    mut input_mode: State<InputMode>,
    mut action_status: State<Option<String>>,
    issues_state: &State<IssuesState>,
    section_idx: usize,
    cursor: usize,
    octocrab_for_actions: Option<&Arc<Octocrab>>,
) {
    match code {
        KeyCode::Char('y' | 'Y') => {
            if let Some(octocrab) = octocrab_for_actions {
                let info = get_current_issue_info(issues_state, section_idx, cursor);
                if let Some((owner, repo, number)) = info {
                    let octocrab = Arc::clone(octocrab);
                    let action = pending.clone();
                    let action_name = match pending {
                        PendingAction::Close => "Closed",
                        PendingAction::Reopen => "Reopened",
                    };
                    let action_label = action_name.to_owned();
                    smol::spawn(Compat::new(async move {
                        let result = match action {
                            PendingAction::Close => {
                                issue_actions::close(&octocrab, &owner, &repo, number).await
                            }
                            PendingAction::Reopen => {
                                issue_actions::reopen(&octocrab, &owner, &repo, number).await
                            }
                        };
                        match result {
                            Ok(()) => {
                                action_status.set(Some(format!("{action_label} issue #{number}")));
                            }
                            Err(e) => {
                                action_status.set(Some(format!("{action_label} failed: {e}")));
                            }
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
    }
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_normal_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    should_exit: Option<State<bool>>,
    switch_view: Option<State<bool>>,
    switch_view_back: Option<State<bool>>,
    scope_toggle: Option<State<bool>>,
    mut preview_open: State<bool>,
    mut preview_scroll: State<usize>,
    mut cursor: State<usize>,
    mut scroll_offset: State<usize>,
    mut active_section: State<usize>,
    mut input_mode: State<InputMode>,
    mut input_buffer: State<String>,
    mut action_status: State<Option<String>>,
    mut label_candidates: State<Vec<String>>,
    mut label_selection: State<usize>,
    total_rows: usize,
    visible_rows: usize,
    section_count: usize,
    mut issues_state: State<IssuesState>,
    current_section_idx: usize,
    octocrab_for_actions: Option<&Arc<Octocrab>>,
    api_cache: Option<&Cache<String, String>>,
    mut section_fetch_times: State<Vec<Option<std::time::Instant>>>,
    mut help_visible: State<bool>,
    mut rate_limit_state: State<Option<RateLimitInfo>>,
) {
    match code {
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
        KeyCode::Char('n') => {
            if let Some(mut sv) = switch_view {
                sv.set(true);
            }
        }
        KeyCode::Char('N') => {
            if let Some(mut sv) = switch_view_back {
                sv.set(true);
            }
        }
        KeyCode::Char('S') => {
            if let Some(mut st) = scope_toggle {
                st.set(true);
            }
        }
        KeyCode::Char('p') => {
            preview_open.set(!preview_open.get());
            preview_scroll.set(0);
        }
        // --- Issue Actions (T086) ---
        KeyCode::Char('c') => {
            input_mode.set(InputMode::Comment);
            input_buffer.set(String::new());
            action_status.set(None);
        }
        KeyCode::Char('L') => {
            // Fetch labels for autocomplete.
            input_mode.set(InputMode::Label);
            input_buffer.set(String::new());
            label_selection.set(0);
            action_status.set(None);

            if let Some(octocrab) = octocrab_for_actions {
                let info = get_current_issue_info(&issues_state, current_section_idx, cursor.get());
                if let Some((owner, repo, _)) = info {
                    let octocrab = Arc::clone(octocrab);
                    let api_cache = api_cache.cloned();
                    smol::spawn(Compat::new(async move {
                        if let Ok((labels, rl)) =
                            graphql::fetch_repo_labels(&octocrab, &owner, &repo, api_cache.as_ref()).await
                        {
                            if rl.is_some() {
                                rate_limit_state.set(rl);
                            }
                            let names: Vec<String> = labels.into_iter().map(|l| l.name).collect();
                            label_candidates.set(names);
                        }
                    }))
                    .detach();
                }
            }
        }
        KeyCode::Char('a') => {
            input_mode.set(InputMode::Assign);
            input_buffer.set(String::new());
            action_status.set(None);
        }
        KeyCode::Char('A') => {
            // Unassign: fire immediately for the first assignee.
            if let Some(octocrab) = octocrab_for_actions {
                let state = issues_state.read();
                let issue = state
                    .sections
                    .get(current_section_idx)
                    .and_then(|s| s.issues.get(cursor.get()));
                if let Some(issue) = issue
                    && let Some(assignee) = issue.assignees.first()
                {
                    let login = assignee.login.clone();
                    if let Some(repo_ref) = &issue.repo {
                        let owner = repo_ref.owner.clone();
                        let repo = repo_ref.name.clone();
                        let number = issue.number;
                        let octocrab = Arc::clone(octocrab);
                        smol::spawn(Compat::new(async move {
                            match issue_actions::unassign(&octocrab, &owner, &repo, number, &login)
                                .await
                            {
                                Ok(()) => action_status
                                    .set(Some(format!("Unassigned {login} from #{number}"))),
                                Err(e) => action_status.set(Some(format!("Unassign failed: {e}"))),
                            }
                        }))
                        .detach();
                    }
                }
            }
        }
        KeyCode::Char('x') => {
            input_mode.set(InputMode::Confirm(PendingAction::Close));
            action_status.set(None);
        }
        KeyCode::Char('X') => {
            input_mode.set(InputMode::Confirm(PendingAction::Reopen));
            action_status.set(None);
        }
        // --- Clipboard & Browser (T091, T092) ---
        KeyCode::Char('y') => {
            let info = get_current_issue_info(&issues_state, current_section_idx, cursor.get());
            if let Some((_, _, number)) = info {
                let text = number.to_string();
                match clipboard::copy_to_clipboard(&text) {
                    Ok(()) => action_status.set(Some(format!("Copied #{number}"))),
                    Err(e) => action_status.set(Some(format!("Copy failed: {e}"))),
                }
            }
        }
        KeyCode::Char('Y') => {
            let info = get_current_issue_info(&issues_state, current_section_idx, cursor.get());
            if let Some((owner, repo, number)) = info {
                let url = format!("https://github.com/{owner}/{repo}/issues/{number}");
                match clipboard::copy_to_clipboard(&url) {
                    Ok(()) => action_status.set(Some(format!("Copied URL for #{number}"))),
                    Err(e) => action_status.set(Some(format!("Copy failed: {e}"))),
                }
            }
        }
        KeyCode::Char('o') => {
            let info = get_current_issue_info(&issues_state, current_section_idx, cursor.get());
            if let Some((owner, repo, number)) = info {
                let url = format!("https://github.com/{owner}/{repo}/issues/{number}");
                match clipboard::open_in_browser(&url) {
                    Ok(()) => action_status.set(Some(format!("Opened #{number}"))),
                    Err(e) => action_status.set(Some(format!("Open failed: {e}"))),
                }
            }
        }
        // Retry / refresh
        KeyCode::Char('r') => {
            if let Some(c) = api_cache {
                c.invalidate_all();
            }
            let idx = active_section.get();
            let mut state = issues_state.read().clone();
            if idx < state.sections.len() {
                state.sections[idx] = SectionData::default();
            }
            issues_state.set(state);
            let mut times = section_fetch_times.read().clone();
            if idx < times.len() {
                times[idx] = None;
            }
            section_fetch_times.set(times);
            cursor.set(0);
            scroll_offset.set(0);
        }
        // --- Search (T087) ---
        KeyCode::Char('/') => {
            input_mode.set(InputMode::Search);
            action_status.set(None);
        }
        // --- Navigation ---
        KeyCode::Down | KeyCode::Char('j') => {
            if total_rows > 0 {
                let new_cursor = (cursor.get() + 1).min(total_rows.saturating_sub(1));
                cursor.set(new_cursor);
                if new_cursor >= scroll_offset.get() + visible_rows {
                    scroll_offset.set(new_cursor.saturating_sub(visible_rows) + 1);
                }
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
        KeyCode::PageDown => {
            if total_rows > 0 {
                let new_cursor = (cursor.get() + visible_rows).min(total_rows.saturating_sub(1));
                cursor.set(new_cursor);
                scroll_offset.set(new_cursor.saturating_sub(visible_rows.saturating_sub(1)));
                preview_scroll.set(0);
            }
        }
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            if preview_open.get() {
                let half = visible_rows / 2;
                preview_scroll.set(preview_scroll.get() + half);
            } else if total_rows > 0 {
                let half = visible_rows / 2;
                let new_cursor = (cursor.get() + half).min(total_rows.saturating_sub(1));
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
        KeyCode::Char('?') => {
            help_visible.set(true);
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_current_issue_info(
    issues_state: &State<IssuesState>,
    section_idx: usize,
    cursor: usize,
) -> Option<(String, String, u64)> {
    let state = issues_state.read();
    let section = state.sections.get(section_idx)?;
    let issue = section.issues.get(cursor)?;
    let repo_ref = issue.repo.as_ref()?;
    Some((repo_ref.owner.clone(), repo_ref.name.clone(), issue.number))
}

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}
