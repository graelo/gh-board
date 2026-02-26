use std::collections::{HashMap, HashSet};

use iocraft::prelude::*;

use crate::actions::clipboard;
use crate::app::ViewKind;
use crate::color::ColorDepth;
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::sidebar::{RenderedSidebar, Sidebar, SidebarMeta, SidebarTab};
use crate::components::sidebar_tabs;
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, Span, TableBuildConfig,
};
use crate::components::text_input::{self, RenderedTextInput, TextInput};
use crate::config::keybindings::{
    BuiltinAction, MergedBindings, ResolvedBinding, TemplateVars, ViewContext,
    execute_shell_command, expand_template, key_event_to_string,
};
use crate::config::types::IssueFilter;
use crate::engine::{EngineHandle, Event, Request};
use crate::filter::{self, apply_scope};
use crate::icons::ResolvedIcons;
use crate::markdown::renderer::{self, StyledLine, StyledSpan};
use crate::theme::ResolvedTheme;
use crate::types::RateLimitInfo;
use crate::types::{Issue, IssueDetail};

/// Issue sidebar only shows Overview and Activity tabs.
const ISSUE_TABS: &[SidebarTab] = &[SidebarTab::Overview, SidebarTab::Activity];

/// Pending detail fetch request: (owner, repo, number).
type DetailRequest = Option<(String, String, u64)>;

// ---------------------------------------------------------------------------
// Issue-specific column definitions (FR-021)
// ---------------------------------------------------------------------------

fn issue_columns(icons: &ResolvedIcons) -> Vec<Column> {
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
            fixed_width: Some(4),
        },
        Column {
            id: "reactions".to_owned(),
            header: "React".to_owned(),
            default_width_pct: 0.05,
            align: TextAlign::Right,
            fixed_width: Some(6),
        },
        Column {
            id: "assignees".to_owned(),
            header: "Assign".to_owned(),
            default_width_pct: 0.12,
            align: TextAlign::Left,
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

    // Info line: repo/name #N by @author
    let repo_name = issue
        .repo
        .as_ref()
        .map_or_else(String::new, crate::github::types::RepoRef::full_name);
    let author = issue
        .author
        .as_ref()
        .map_or("unknown", |a| a.login.as_str());
    row.insert(
        "info".to_owned(),
        Cell::from_spans(vec![
            Span {
                text: repo_name,
                color: Some(theme.text_secondary),
                bold: false,
            },
            Span {
                text: format!(" #{}", issue.number),
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

    // Subtitle: issue title (extracted by subtitle_column)
    row.insert(
        "subtitle".to_owned(),
        Cell::colored(crate::util::expand_emoji(&issue.title), theme.text_primary),
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
    Confirm(BuiltinAction),
    Search,
}

// ---------------------------------------------------------------------------
// Filter state (T047)
// ---------------------------------------------------------------------------

/// State for a single filter.
#[derive(Debug, Clone)]
struct FilterData {
    rows: Vec<Row>,
    bodies: Vec<String>,
    titles: Vec<String>,
    issues: Vec<Issue>,
    issue_count: usize,
    loading: bool,
    error: Option<String>,
}

impl Default for FilterData {
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

/// Shared state across all issue filters.
#[derive(Debug, Clone)]
struct IssuesState {
    filters: Vec<FilterData>,
}

// ---------------------------------------------------------------------------
// IssuesView component (T047-T048, T086)
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct IssuesViewProps<'a> {
    pub filters: Option<&'a [IssueFilter]>,
    /// Engine handle.
    pub engine: Option<&'a EngineHandle>,
    pub theme: Option<&'a ResolvedTheme>,
    /// Merged keybindings for help overlay.
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
    pub preview_width_pct: f64,
    pub show_filter_count: bool,
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
    let filters_cfg = props.filters.unwrap_or(&[]);
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let switch_view_back = props.switch_view_back;
    let scope_toggle = props.scope_toggle;
    let scope_repo = &props.scope_repo;
    let filter_count = filters_cfg.len();
    let is_active = props.is_active;
    let preview_pct = if props.preview_width_pct > 0.0 {
        props.preview_width_pct
    } else {
        0.45
    };

    let mut active_filter = hooks.use_state(|| 0usize);
    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);
    let mut preview_open = hooks.use_state(|| false);
    let mut preview_scroll = hooks.use_state(|| 0usize);

    // State: sidebar tab.
    let mut sidebar_tab = hooks.use_state(|| SidebarTab::Overview);

    // State: cached issue detail data for sidebar tabs (HashMap cache + debounce).
    let mut detail_cache = hooks.use_state(HashMap::<u64, IssueDetail>::new);
    let mut pending_detail = hooks.use_state(|| DetailRequest::None);
    let mut debounce_gen = hooks.use_state(|| 0u64);

    // Action state.
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut input_buffer = hooks.use_state(String::new);
    let mut action_status = hooks.use_state(|| Option::<String>::None);
    let mut label_candidates = hooks.use_state(Vec::<String>::new);
    let mut label_selection = hooks.use_state(|| 0usize);
    let mut label_selected = hooks.use_state(Vec::<String>::new);
    let mut assignee_candidates = hooks.use_state(Vec::<String>::new);
    let mut assignee_selection = hooks.use_state(|| 0usize);
    let mut assignee_selected = hooks.use_state(Vec::<String>::new);

    // When true, the next lazy fetch bypasses the moka cache (set by `r` key and MutationOk).
    let mut force_refresh = hooks.use_state(|| false);

    // Whether RegisterIssuesRefresh has been sent to the engine yet.
    let mut refresh_registered = hooks.use_state(|| false);

    // State: search query (T087).
    let mut search_query = hooks.use_state(String::new);

    let mut help_visible = hooks.use_state(|| false);

    // State: rate limit from last GraphQL response.
    let mut rate_limit_state = hooks.use_state(|| Option::<RateLimitInfo>::None);

    // State: per-filter fetch tracking (lazy: only fetch the active filter).
    let mut filter_fetch_times =
        hooks.use_state(move || vec![Option::<std::time::Instant>::None; filter_count]);
    let mut filter_in_flight = hooks.use_state(move || vec![false; filter_count]);
    // Set by 'R' keypress; consumed by render body to fetch all filters eagerly.
    let mut refresh_all = hooks.use_state(|| false);

    let initial_filters = vec![FilterData::default(); filter_count];
    let mut issues_state = hooks.use_state(move || IssuesState {
        filters: initial_filters,
    });

    // Track scope changes: when scope_repo changes, invalidate all filters.
    let mut last_scope = hooks.use_state(|| scope_repo.clone());
    if *last_scope.read() != *scope_repo {
        last_scope.set(scope_repo.clone());
        issues_state.set(IssuesState {
            filters: vec![FilterData::default(); filter_count],
        });
        filter_fetch_times.set(vec![None; filter_count]);
        filter_in_flight.set(vec![false; filter_count]);
        refresh_registered.set(false);
    }

    // Event channel: engine pushes events back to UI.
    let event_channel = hooks.use_state(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        (tx, std::sync::Arc::new(std::sync::Mutex::new(rx)))
    });
    let (event_tx, event_rx_arc) = event_channel.read().clone();
    // Clone the EngineHandle so it can be captured in 'static use_future closures.
    let engine: Option<EngineHandle> = props.engine.cloned();
    // Pre-clone for each consumer: debounce future, fetch trigger, keyboard handler.
    let engine_for_keyboard = engine.clone();

    // Debounce future: waits for cursor to settle, then sends FetchIssueDetail via engine.
    let engine_for_debounce = engine.clone();
    let event_tx_for_debounce = event_tx.clone();
    hooks.use_future(async move {
        let mut last_gen = 0u64;
        let mut spawned_gen = 0u64;
        loop {
            smol::Timer::after(std::time::Duration::from_millis(300)).await;
            let current_gen = debounce_gen.get();
            if current_gen != last_gen {
                // Generation changed during this cycle — not stable yet.
                last_gen = current_gen;
            } else if current_gen > 0 && current_gen != spawned_gen {
                // Stable for one full cycle and not yet spawned — fetch now.
                let req = pending_detail.read().clone();
                if let Some((owner, repo, issue_number)) = req {
                    spawned_gen = current_gen;
                    if let Some(ref eng) = engine_for_debounce {
                        eng.send(Request::FetchIssueDetail {
                            owner: owner.clone(),
                            repo: repo.clone(),
                            number: issue_number,
                            reply_tx: event_tx_for_debounce.clone(),
                        });
                    }
                }
            }
        }
    });

    // Compute active filter index early (needed by fetch logic below).
    let current_filter_idx = active_filter.get().min(filter_count.saturating_sub(1));

    // Lazy fetch: only fetch the active filter when it needs data.
    let active_needs_fetch = issues_state
        .read()
        .filters
        .get(current_filter_idx)
        .is_some_and(|s| s.loading);
    let active_in_flight = filter_in_flight
        .read()
        .get(current_filter_idx)
        .copied()
        .unwrap_or(false);

    // Register all filters for background refresh once at mount (or after scope change).
    if !refresh_registered.get()
        && let Some(ref eng) = engine
    {
        let scoped_configs: Vec<_> = filters_cfg
            .iter()
            .map(|cfg| {
                let mut modified = cfg.clone();
                modified.filters = apply_scope(&cfg.filters, scope_repo.as_deref());
                modified
            })
            .collect();
        eng.send(Request::RegisterIssuesRefresh {
            filter_configs: scoped_configs,
            notify_tx: event_tx.clone(),
        });
        refresh_registered.set(true);
    }

    if refresh_all.get()
        && is_active
        && let Some(ref engine_ref) = engine
    {
        // 'R' was pressed: reset the flag and eagerly fetch every filter.
        refresh_all.set(false);
        let mut in_flight = filter_in_flight.read().clone();
        for (filter_idx, cfg) in filters_cfg.iter().enumerate() {
            if filter_idx < in_flight.len() {
                in_flight[filter_idx] = true;
            }
            let mut modified_filter = cfg.clone();
            modified_filter.filters = apply_scope(&cfg.filters, scope_repo.as_deref());
            engine_ref.send(Request::FetchIssues {
                filter_idx,
                filter: modified_filter,
                force: true,
                reply_tx: event_tx.clone(),
            });
        }
        filter_in_flight.set(in_flight);
    } else if active_needs_fetch
        && !active_in_flight
        && is_active
        && let Some(cfg) = filters_cfg.get(current_filter_idx)
        && let Some(ref engine_ref) = engine
    {
        let mut in_flight = filter_in_flight.read().clone();
        if current_filter_idx < in_flight.len() {
            in_flight[current_filter_idx] = true;
        }
        filter_in_flight.set(in_flight);

        let filter_idx = current_filter_idx;
        let mut modified_filter = cfg.clone();
        modified_filter.filters = apply_scope(&cfg.filters, scope_repo.as_deref());

        // Consume the force flag: bypass cache for `r`-key and post-mutation fetches.
        let force = force_refresh.get();
        if force {
            force_refresh.set(false);
        }

        engine_ref.send(Request::FetchIssues {
            filter_idx,
            filter: modified_filter,
            force,
            reply_tx: event_tx.clone(),
        });
    }

    // Event polling: drain events from engine reply channel.
    {
        let rx_for_poll = event_rx_arc.clone();
        let theme_for_poll = theme.clone();
        let date_format_for_poll = props.date_format.unwrap_or("relative").to_owned();
        let current_filter_for_poll = current_filter_idx;
        hooks.use_future(async move {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(100)).await;
                let events: Vec<Event> = {
                    let rx = rx_for_poll.lock().unwrap();
                    let mut evts = Vec::new();
                    while let Ok(evt) = rx.try_recv() {
                        evts.push(evt);
                    }
                    evts
                };
                for evt in events {
                    match evt {
                        Event::IssuesFetched {
                            filter_idx,
                            issues,
                            rate_limit,
                        } => {
                            if rate_limit.is_some() {
                                rate_limit_state.set(rate_limit);
                            }
                            let detail_snap = detail_cache.read().clone();
                            let rows: Vec<Row> = issues
                                .iter()
                                .map(|issue| {
                                    issue_to_row(issue, &theme_for_poll, &date_format_for_poll)
                                })
                                .collect();
                            let bodies: Vec<String> =
                                issues.iter().map(|i| i.body.clone()).collect();
                            let titles: Vec<String> =
                                issues.iter().map(|i| i.title.clone()).collect();
                            let issue_count = issues.len();
                            let filter_data = FilterData {
                                rows,
                                bodies,
                                titles,
                                issue_count,
                                issues,
                                loading: false,
                                error: None,
                            };
                            let _ = detail_snap; // suppress unused warning
                            let mut state = issues_state.read().clone();
                            if filter_idx < state.filters.len() {
                                state.filters[filter_idx] = filter_data;
                            }
                            issues_state.set(state);
                            let mut times = filter_fetch_times.read().clone();
                            if filter_idx < times.len() {
                                times[filter_idx] = Some(std::time::Instant::now());
                            }
                            filter_fetch_times.set(times);
                            let mut ifl = filter_in_flight.read().clone();
                            if filter_idx < ifl.len() {
                                ifl[filter_idx] = false;
                            }
                            filter_in_flight.set(ifl);
                        }
                        Event::IssueDetailFetched { number, detail } => {
                            let mut cache = detail_cache.read().clone();
                            cache.insert(number, detail);
                            detail_cache.set(cache);
                        }
                        Event::FetchError {
                            context: _,
                            message,
                        } => {
                            let in_flight_snap = filter_in_flight.read().clone();
                            let error_fi = in_flight_snap.iter().position(|&f| f);
                            if let Some(fi) = error_fi {
                                let mut state = issues_state.read().clone();
                                if fi < state.filters.len() {
                                    state.filters[fi] = FilterData {
                                        loading: false,
                                        error: Some(message.clone()),
                                        ..FilterData::default()
                                    };
                                }
                                issues_state.set(state);
                                let mut times = filter_fetch_times.read().clone();
                                if fi < times.len() {
                                    times[fi] = Some(std::time::Instant::now());
                                }
                                filter_fetch_times.set(times);
                                let mut ifl = filter_in_flight.read().clone();
                                if fi < ifl.len() {
                                    ifl[fi] = false;
                                }
                                filter_in_flight.set(ifl);
                            }
                        }
                        Event::MutationOk { description } => {
                            action_status.set(Some(format!(
                                "{} {description}",
                                theme_for_poll.icons.feedback_ok
                            )));
                            // Trigger a refetch of the active filter.
                            let mut state = issues_state.read().clone();
                            if current_filter_for_poll < state.filters.len() {
                                state.filters[current_filter_for_poll] = FilterData::default();
                            }
                            issues_state.set(state);
                            let mut times = filter_fetch_times.read().clone();
                            if current_filter_for_poll < times.len() {
                                times[current_filter_for_poll] = None;
                            }
                            filter_fetch_times.set(times);
                            // Must come LAST: force_refresh is consumed by the
                            // lazy-fetch trigger only when active_needs_fetch
                            // (loading=true) is already visible in the same
                            // render. Setting it before issues_state.set()
                            // risks a render where loading is still false and
                            // the flag is silently dropped, causing a
                            // non-forced (cached) refetch.
                            force_refresh.set(true);
                        }
                        Event::MutationError {
                            description,
                            message,
                        } => {
                            action_status.set(Some(format!(
                                "{} {description}: {message}",
                                theme_for_poll.icons.feedback_error
                            )));
                        }
                        Event::RateLimitUpdated { info } => {
                            rate_limit_state.set(Some(info));
                        }
                        Event::RepoLabelsFetched { labels, .. } => {
                            label_candidates.set(labels);
                        }
                        Event::RepoCollaboratorsFetched { logins, .. } => {
                            let mut combined = assignee_candidates.read().clone();
                            combined.extend(logins);
                            combined.sort();
                            combined.dedup();
                            assignee_candidates.set(combined);
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    let state_ref = issues_state.read();
    let all_rows_count = state_ref
        .filters
        .get(current_filter_idx)
        .map_or(0, |s| s.rows.len());
    let search_q = search_query.read().clone();
    let total_rows = if search_q.is_empty() {
        all_rows_count
    } else {
        state_ref
            .filters
            .get(current_filter_idx)
            .map_or(0, |s| filter::filter_rows(&s.rows, &search_q).len())
    };

    let visible_rows = (props.height.saturating_sub(5) / 3).max(1) as usize;

    // Engine and event_tx clones for the keyboard handler closure.
    let engine = engine_for_keyboard;
    let event_tx_kb = event_tx.clone();

    // Keyboard handling.
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
                    InputMode::Comment => {
                        handle_text_input(
                            code,
                            modifiers,
                            &current_mode,
                            input_mode,
                            input_buffer,
                            action_status,
                            &issues_state,
                            current_filter_idx,
                            cursor.get(),
                            engine.as_ref(),
                            &event_tx_kb,
                        );
                    }
                    InputMode::Assign => {
                        handle_assign_input(
                            code,
                            modifiers,
                            input_mode,
                            input_buffer,
                            &issues_state,
                            current_filter_idx,
                            cursor.get(),
                            engine.as_ref(),
                            &event_tx_kb,
                            assignee_candidates,
                            assignee_selection,
                            assignee_selected,
                        );
                    }
                    InputMode::Label => {
                        handle_label_input(
                            code,
                            modifiers,
                            input_mode,
                            input_buffer,
                            label_candidates,
                            label_selection,
                            label_selected,
                            &issues_state,
                            current_filter_idx,
                            cursor.get(),
                            engine.as_ref(),
                            &event_tx_kb,
                        );
                    }
                    InputMode::Confirm(ref pending) => match code {
                        KeyCode::Char('y' | 'Y') => {
                            let info = get_current_issue_info(
                                &issues_state,
                                current_filter_idx,
                                cursor.get(),
                            );
                            if let Some((owner, repo, number)) = info
                                && let Some(eng) = engine.as_ref()
                            {
                                match pending {
                                    BuiltinAction::Close => eng.send(Request::CloseIssue {
                                        owner,
                                        repo,
                                        number,
                                        reply_tx: event_tx_kb.clone(),
                                    }),
                                    BuiltinAction::Reopen => eng.send(Request::ReopenIssue {
                                        owner,
                                        repo,
                                        number,
                                        reply_tx: event_tx_kb.clone(),
                                    }),
                                    _ => {}
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
                    InputMode::Normal => {
                        let engine = engine.as_ref();
                        let event_tx = &event_tx_kb;
                        if let Some(key_str) = key_event_to_string(code, modifiers, kind) {
                            let info = get_current_issue_info(
                                &issues_state,
                                current_filter_idx,
                                cursor.get(),
                            );
                            let vars = TemplateVars {
                                url: info.as_ref().map_or_else(String::new, |(o, r, n)| {
                                    format!("https://github.com/{o}/{r}/issues/{n}")
                                }),
                                number: info
                                    .as_ref()
                                    .map_or_else(String::new, |(_, _, n)| n.to_string()),
                                repo_name: info
                                    .as_ref()
                                    .map_or_else(String::new, |(o, r, _)| format!("{o}/{r}")),
                                ..Default::default()
                            };
                            match keybindings
                                .as_ref()
                                .and_then(|kb| kb.resolve(&key_str, ViewContext::Issues))
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
                                    BuiltinAction::TogglePreview => {
                                        preview_open.set(!preview_open.get());
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::CommentAction => {
                                        input_mode.set(InputMode::Comment);
                                        input_buffer.set(String::new());
                                        action_status.set(None);
                                    }
                                    BuiltinAction::LabelAction => {
                                        input_mode.set(InputMode::Label);
                                        input_buffer.set(String::new());
                                        label_selection.set(0);
                                        label_candidates.set(Vec::new());
                                        let current_labels = get_current_issue_labels(
                                            &issues_state,
                                            current_filter_idx,
                                            cursor.get(),
                                        );
                                        label_selected.set(current_labels);
                                        action_status.set(None);
                                        if let Some(engine) = engine
                                            && let Some((owner, repo, _)) = &info
                                        {
                                            engine.send(Request::FetchRepoLabels {
                                                owner: owner.clone(),
                                                repo: repo.clone(),
                                                reply_tx: event_tx.clone(),
                                            });
                                        }
                                    }
                                    BuiltinAction::Assign | BuiltinAction::Unassign => {
                                        input_mode.set(InputMode::Assign);
                                        input_buffer.set(String::new());
                                        assignee_selection.set(0);
                                        let current = get_current_issue_assignees(
                                            &issues_state,
                                            current_filter_idx,
                                            cursor.get(),
                                        );
                                        assignee_selected.set(current);
                                        let initial = {
                                            let state = issues_state.read();
                                            state
                                                .filters
                                                .get(current_filter_idx)
                                                .and_then(|f| f.issues.get(cursor.get()))
                                                .map(build_issue_assignee_candidates)
                                                .unwrap_or_default()
                                        };
                                        assignee_candidates.set(initial);
                                        action_status.set(None);
                                        if let Some(engine) = engine
                                            && let Some((owner, repo, _)) = &info
                                        {
                                            engine.send(Request::FetchRepoCollaborators {
                                                owner: owner.clone(),
                                                repo: repo.clone(),
                                                reply_tx: event_tx.clone(),
                                            });
                                        }
                                    }
                                    BuiltinAction::Close => {
                                        input_mode.set(InputMode::Confirm(BuiltinAction::Close));
                                        action_status.set(None);
                                    }
                                    BuiltinAction::Reopen => {
                                        input_mode.set(InputMode::Confirm(BuiltinAction::Reopen));
                                        action_status.set(None);
                                    }
                                    BuiltinAction::CopyNumber => {
                                        if let Some((_, _, number)) = info {
                                            let text = number.to_string();
                                            match clipboard::copy_to_clipboard(&text) {
                                                Ok(()) => action_status
                                                    .set(Some(format!("Copied #{number}"))),
                                                Err(e) => action_status
                                                    .set(Some(format!("Copy failed: {e}"))),
                                            }
                                        }
                                    }
                                    BuiltinAction::CopyUrl => {
                                        if let Some((owner, repo, number)) = info {
                                            let url = format!(
                                                "https://github.com/{owner}/{repo}/issues/{number}"
                                            );
                                            match clipboard::copy_to_clipboard(&url) {
                                                Ok(()) => action_status
                                                    .set(Some(format!("Copied URL for #{number}"))),
                                                Err(e) => action_status
                                                    .set(Some(format!("Copy failed: {e}"))),
                                            }
                                        }
                                    }
                                    BuiltinAction::OpenBrowser => {
                                        if let Some((owner, repo, number)) = info {
                                            let url = format!(
                                                "https://github.com/{owner}/{repo}/issues/{number}"
                                            );
                                            match clipboard::open_in_browser(&url) {
                                                Ok(()) => action_status
                                                    .set(Some(format!("Opened #{number}"))),
                                                Err(e) => action_status
                                                    .set(Some(format!("Open failed: {e}"))),
                                            }
                                        }
                                    }
                                    BuiltinAction::Refresh => {
                                        force_refresh.set(true);
                                        let idx = active_filter.get();
                                        let mut state = issues_state.read().clone();
                                        if idx < state.filters.len() {
                                            state.filters[idx] = FilterData::default();
                                        }
                                        issues_state.set(state);
                                        let mut times = filter_fetch_times.read().clone();
                                        if idx < times.len() {
                                            times[idx] = None;
                                        }
                                        filter_fetch_times.set(times);
                                        pending_detail.set(None);
                                        detail_cache.set(HashMap::new());
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    BuiltinAction::RefreshAll => {
                                        let mut state = issues_state.read().clone();
                                        for filter in &mut state.filters {
                                            *filter = FilterData::default();
                                        }
                                        issues_state.set(state);
                                        let mut times = filter_fetch_times.read().clone();
                                        times.fill(None);
                                        filter_fetch_times.set(times);
                                        pending_detail.set(None);
                                        detail_cache.set(HashMap::new());
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                        refresh_all.set(true);
                                    }
                                    BuiltinAction::Search => {
                                        input_mode.set(InputMode::Search);
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
                                            preview_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::MoveUp => {
                                        let new_cursor = cursor.get().saturating_sub(1);
                                        cursor.set(new_cursor);
                                        if new_cursor < scroll_offset.get() {
                                            scroll_offset.set(new_cursor);
                                        }
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::First => {
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::Last => {
                                        if total_rows > 0 {
                                            cursor.set(total_rows.saturating_sub(1));
                                            scroll_offset
                                                .set(total_rows.saturating_sub(visible_rows));
                                            preview_scroll.set(0);
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
                                            preview_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::PageUp => {
                                        let new_cursor = cursor.get().saturating_sub(visible_rows);
                                        cursor.set(new_cursor);
                                        scroll_offset
                                            .set(scroll_offset.get().saturating_sub(visible_rows));
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::HalfPageDown => {
                                        let half = visible_rows / 2;
                                        if preview_open.get() {
                                            preview_scroll.set(preview_scroll.get() + half);
                                        } else if total_rows > 0 {
                                            let new_cursor = (cursor.get() + half)
                                                .min(total_rows.saturating_sub(1));
                                            cursor.set(new_cursor);
                                            if new_cursor >= scroll_offset.get() + visible_rows {
                                                scroll_offset.set(
                                                    new_cursor.saturating_sub(visible_rows) + 1,
                                                );
                                            }
                                            preview_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::HalfPageUp => {
                                        let half = visible_rows / 2;
                                        if preview_open.get() {
                                            preview_scroll
                                                .set(preview_scroll.get().saturating_sub(half));
                                        } else {
                                            let new_cursor = cursor.get().saturating_sub(half);
                                            cursor.set(new_cursor);
                                            if new_cursor < scroll_offset.get() {
                                                scroll_offset.set(new_cursor);
                                            }
                                            preview_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::PrevFilter => {
                                        if filter_count > 0 {
                                            let current = active_filter.get();
                                            active_filter.set(if current == 0 {
                                                filter_count.saturating_sub(1)
                                            } else {
                                                current - 1
                                            });
                                            cursor.set(0);
                                            scroll_offset.set(0);
                                            preview_scroll.set(0);
                                            pending_detail.set(None);
                                        }
                                    }
                                    BuiltinAction::NextFilter => {
                                        if filter_count > 0 {
                                            active_filter
                                                .set((active_filter.get() + 1) % filter_count);
                                            cursor.set(0);
                                            scroll_offset.set(0);
                                            preview_scroll.set(0);
                                            pending_detail.set(None);
                                        }
                                    }
                                    BuiltinAction::ToggleHelp => {
                                        help_visible.set(true);
                                    }
                                    _ => {}
                                },
                                Some(ResolvedBinding::ShellCommand(cmd)) => {
                                    let expanded = expand_template(&cmd, &vars);
                                    let _ = execute_shell_command(&expanded);
                                }
                                None => {
                                    if key_str == "]" {
                                        let current = sidebar_tab.get();
                                        let idx = ISSUE_TABS
                                            .iter()
                                            .position(|&t| t == current)
                                            .unwrap_or(0);
                                        sidebar_tab.set(ISSUE_TABS[(idx + 1) % ISSUE_TABS.len()]);
                                        preview_scroll.set(0);
                                    } else if key_str == "[" {
                                        let current = sidebar_tab.get();
                                        let idx = ISSUE_TABS
                                            .iter()
                                            .position(|&t| t == current)
                                            .unwrap_or(0);
                                        sidebar_tab.set(
                                            ISSUE_TABS[if idx == 0 {
                                                ISSUE_TABS.len() - 1
                                            } else {
                                                idx - 1
                                            }],
                                        );
                                        preview_scroll.set(0);
                                    }
                                }
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

    // Build tabs.
    let tabs: Vec<Tab> = filters_cfg
        .iter()
        .enumerate()
        .map(|(i, s)| Tab {
            title: s.title.clone(),
            count: state_ref.filters.get(i).map(|d| d.issue_count),
            is_ephemeral: false,
        })
        .collect();

    let current_data = state_ref.filters.get(current_filter_idx);
    let columns = issue_columns(&theme.icons);

    let layout = filters_cfg
        .get(current_filter_idx)
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
        subtitle_column: Some("subtitle"),
        row_separator: true,
        scrollbar_thumb_color: Some(theme.border_primary),
    });

    // Request issue detail when sidebar is open and current issue is not cached.
    if is_preview_open {
        let cursor_idx = cursor.get();
        let current_issue = current_data.and_then(|d| d.issues.get(cursor_idx));
        if let Some(issue) = current_issue {
            let issue_number = issue.number;
            let already_cached = detail_cache.read().contains_key(&issue_number);
            let already_pending = {
                let guard = pending_detail.read();
                match *guard {
                    Some((_, _, n)) => n == issue_number,
                    None => false,
                }
            };

            if !already_cached
                && !already_pending
                && let Some(repo_ref) = &issue.repo
            {
                pending_detail.set(Some((
                    repo_ref.owner.clone(),
                    repo_ref.name.clone(),
                    issue_number,
                )));
                debounce_gen.set(debounce_gen.get() + 1);
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
        let current_issue = current_data.and_then(|d| d.issues.get(cursor_idx));
        let cache_ref = detail_cache.read();
        let detail_for_issue = current_issue.and_then(|i| cache_ref.get(&i.number));

        let md_lines: Vec<StyledLine> = match current_tab {
            SidebarTab::Overview => {
                let body = current_data
                    .and_then(|d| d.bodies.get(cursor_idx))
                    .map_or("", String::as_str);
                if body.is_empty() {
                    Vec::new()
                } else {
                    renderer::render_markdown(body, &theme, depth)
                }
            }
            SidebarTab::Activity => {
                if let Some(detail) = detail_for_issue {
                    sidebar_tabs::render_issue_activity(detail, &theme, depth)
                } else {
                    vec![StyledLine::from_span(StyledSpan::text(
                        "Loading...",
                        theme.text_faint,
                    ))]
                }
            }
            _ => Vec::new(),
        };

        // Build meta header for Overview tab.
        let sidebar_meta = if current_tab == SidebarTab::Overview {
            current_issue.map(|issue| build_issue_sidebar_meta(issue, &theme, depth))
        } else {
            None
        };

        // Account for tab bar (2 extra lines) + meta in sidebar height.
        #[allow(clippy::cast_possible_truncation)]
        let meta_lines = sidebar_meta.as_ref().map_or(0, SidebarMeta::line_count) as u16;
        let sidebar_visible_lines = props.height.saturating_sub(8 + meta_lines) as usize;

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
            Some(theme.border_primary),
            Some(current_tab),
            Some(&theme.icons),
            sidebar_meta,
            Some(ISSUE_TABS),
        ))
    } else {
        None
    };

    let rendered_tab_bar = RenderedTabBar::build(
        &tabs,
        current_filter_idx,
        props.show_filter_count,
        depth,
        Some(theme.footer_issues),
        Some(theme.footer_issues),
        Some(theme.border_faint),
        &theme.icons.tab_filter,
        &theme.icons.tab_ephemeral,
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
        InputMode::Assign => {
            let buf = input_buffer.read().clone();
            let candidates = assignee_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            let sel = assignee_selection.get();
            let selected_idx = if filtered.is_empty() {
                None
            } else {
                Some(sel.min(filtered.len().saturating_sub(1)))
            };
            let selected = assignee_selected.read();
            let prompt = if selected.is_empty() {
                "Assign:".to_owned()
            } else {
                format!("Assign [{}]:", selected.join(", "))
            };
            Some(RenderedTextInput::build_with_multiselect_suggestions(
                &prompt,
                &buf,
                depth,
                Some(theme.text_primary),
                Some(theme.text_secondary),
                Some(theme.border_faint),
                &filtered,
                selected_idx,
                Some(theme.text_primary),
                Some(theme.bg_selected),
                Some(theme.text_faint),
                &selected,
            ))
        }
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
            let selected = label_selected.read();
            let prompt = if selected.is_empty() {
                "Label:".to_owned()
            } else {
                format!("Label [{}]:", selected.join(", "))
            };
            Some(RenderedTextInput::build_with_multiselect_suggestions(
                &prompt,
                &buf,
                depth,
                Some(theme.text_primary),
                Some(theme.text_secondary),
                Some(theme.border_faint),
                &filtered,
                selected_idx,
                Some(theme.text_primary),
                Some(theme.bg_selected),
                Some(theme.text_faint),
                &selected,
            ))
        }
        InputMode::Confirm(action) => {
            let prompt = match action {
                BuiltinAction::Close => "Close this issue? (y/n)",
                BuiltinAction::Reopen => "Reopen this issue? (y/n)",
                _ => "(y/n)",
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
    let active_fetch_time = filter_fetch_times
        .read()
        .get(current_filter_idx)
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

/// Build autocomplete candidates from Issue data (participants, assignees, author).
/// Returns a deduplicated, sorted list of usernames.
fn build_issue_assignee_candidates(issue: &Issue) -> Vec<String> {
    let mut pool = Vec::new();

    // Add participants (already deduplicated by GitHub)
    pool.extend(issue.participants.iter().cloned());

    // Add current assignees
    pool.extend(issue.assignees.iter().map(|a| a.login.clone()));

    // Add author (in case not in participants)
    if let Some(author) = &issue.author {
        pool.push(author.login.clone());
    }

    // Deduplicate and sort
    pool.sort();
    pool.dedup();

    pool
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_assign_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    mut input_mode: State<InputMode>,
    mut input_buffer: State<String>,
    issues_state: &State<IssuesState>,
    filter_idx: usize,
    cursor: usize,
    engine: Option<&EngineHandle>,
    event_tx: &std::sync::mpsc::Sender<Event>,
    assignee_candidates: State<Vec<String>>,
    mut assignee_selection: State<usize>,
    mut assignee_selected: State<Vec<String>>,
) {
    match code {
        KeyCode::Tab | KeyCode::Down => {
            let buf = input_buffer.read().clone();
            let candidates = assignee_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            if !filtered.is_empty() {
                assignee_selection.set((assignee_selection.get() + 1) % filtered.len());
            }
        }
        KeyCode::Up | KeyCode::BackTab => {
            let buf = input_buffer.read().clone();
            let candidates = assignee_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            if !filtered.is_empty() {
                let sel = assignee_selection.get();
                assignee_selection.set(if sel == 0 {
                    filtered.len() - 1
                } else {
                    sel - 1
                });
            }
        }
        KeyCode::Char(' ') => {
            let buf = input_buffer.read().clone();
            let candidates = assignee_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            if !filtered.is_empty() {
                let sel = assignee_selection
                    .get()
                    .min(filtered.len().saturating_sub(1));
                let item = filtered[sel].clone();
                let mut selected = assignee_selected.read().clone();
                if let Some(pos) = selected.iter().position(|s| s == &item) {
                    selected.remove(pos);
                } else {
                    selected.push(item);
                }
                assignee_selected.set(selected);
            }
            input_buffer.set(String::new());
            assignee_selection.set(0);
        }
        KeyCode::Enter => {
            let buf = input_buffer.read().clone();
            let checked = assignee_selected.read().clone();
            let logins: Vec<String> = if buf.is_empty() {
                // No text typed → submit multiselect state as-is (may be empty = unassign all)
                checked
            } else {
                // Text typed → resolve suggestion, merge into checked set
                let candidates = assignee_candidates.read();
                let filtered = text_input::filter_suggestions(&candidates, &buf);
                let login = if filtered.is_empty() {
                    buf
                } else {
                    let sel = assignee_selection
                        .get()
                        .min(filtered.len().saturating_sub(1));
                    filtered[sel].clone()
                };
                if login.is_empty() {
                    checked
                } else {
                    let mut all = checked;
                    if !all.contains(&login) {
                        all.push(login);
                    }
                    all
                }
            };
            let info = get_current_issue_info(issues_state, filter_idx, cursor);
            if let Some((owner, repo, number)) = info
                && let Some(eng) = engine
            {
                eng.send(Request::SetIssueAssignees {
                    owner,
                    repo,
                    number,
                    logins,
                    reply_tx: event_tx.clone(),
                });
            }
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            assignee_selection.set(0);
            assignee_selected.set(Vec::new());
        }
        KeyCode::Esc => {
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            assignee_selection.set(0);
            assignee_selected.set(Vec::new());
        }
        KeyCode::Backspace => {
            let mut buf = input_buffer.read().clone();
            buf.pop();
            input_buffer.set(buf);
            assignee_selection.set(0);
        }
        KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
            let mut buf = input_buffer.read().clone();
            buf.push(ch);
            input_buffer.set(buf);
            assignee_selection.set(0);
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
    _action_status: State<Option<String>>,
    issues_state: &State<IssuesState>,
    filter_idx: usize,
    cursor: usize,
    engine: Option<&EngineHandle>,
    event_tx: &std::sync::mpsc::Sender<Event>,
) {
    match code {
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            let text = input_buffer.read().clone();
            if !text.is_empty() {
                let info = get_current_issue_info(issues_state, filter_idx, cursor);
                if let Some((owner, repo, number)) = info
                    && let Some(engine) = engine
                    && *current_mode == InputMode::Comment
                {
                    engine.send(Request::AddIssueComment {
                        owner,
                        repo,
                        number,
                        body: text.clone(),
                        reply_tx: event_tx.clone(),
                    });
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

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_label_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    mut input_mode: State<InputMode>,
    mut input_buffer: State<String>,
    label_candidates: State<Vec<String>>,
    mut label_selection: State<usize>,
    mut label_selected: State<Vec<String>>,
    issues_state: &State<IssuesState>,
    filter_idx: usize,
    cursor: usize,
    engine: Option<&EngineHandle>,
    event_tx: &std::sync::mpsc::Sender<Event>,
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
        KeyCode::Char(' ') => {
            let buf = input_buffer.read().clone();
            let candidates = label_candidates.read();
            let filtered = text_input::filter_suggestions(&candidates, &buf);
            if !filtered.is_empty() {
                let sel = label_selection.get().min(filtered.len().saturating_sub(1));
                let item = filtered[sel].clone();
                let mut selected = label_selected.read().clone();
                if let Some(pos) = selected.iter().position(|s| s == &item) {
                    selected.remove(pos);
                } else {
                    selected.push(item);
                }
                label_selected.set(selected);
            }
            input_buffer.set(String::new());
            label_selection.set(0);
        }
        KeyCode::Enter => {
            let buf = input_buffer.read().clone();
            let checked = label_selected.read().clone();
            let labels: Vec<String> = if buf.is_empty() {
                // No text typed → submit multiselect state as-is (may be empty = clear all)
                checked
            } else {
                // Text typed → resolve suggestion, merge into checked set
                let candidates = label_candidates.read();
                let filtered = text_input::filter_suggestions(&candidates, &buf);
                let label = if filtered.is_empty() {
                    buf
                } else {
                    let sel = label_selection.get().min(filtered.len().saturating_sub(1));
                    filtered[sel].clone()
                };
                if label.is_empty() {
                    checked
                } else {
                    let mut all = checked;
                    if !all.contains(&label) {
                        all.push(label);
                    }
                    all
                }
            };
            let info = get_current_issue_info(issues_state, filter_idx, cursor);
            if let Some((owner, repo, number)) = info
                && let Some(engine) = engine
            {
                engine.send(Request::SetIssueLabels {
                    owner,
                    repo,
                    number,
                    labels,
                    reply_tx: event_tx.clone(),
                });
            }
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            label_selection.set(0);
            label_selected.set(Vec::new());
        }
        KeyCode::Esc => {
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            label_selection.set(0);
            label_selected.set(Vec::new());
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_current_issue_info(
    issues_state: &State<IssuesState>,
    filter_idx: usize,
    cursor: usize,
) -> Option<(String, String, u64)> {
    let state = issues_state.read();
    let filter = state.filters.get(filter_idx)?;
    let issue = filter.issues.get(cursor)?;
    let repo_ref = issue.repo.as_ref()?;
    Some((repo_ref.owner.clone(), repo_ref.name.clone(), issue.number))
}

fn get_current_issue_labels(
    issues_state: &State<IssuesState>,
    filter_idx: usize,
    cursor: usize,
) -> Vec<String> {
    let state = issues_state.read();
    let Some(filter) = state.filters.get(filter_idx) else {
        return vec![];
    };
    let Some(issue) = filter.issues.get(cursor) else {
        return vec![];
    };
    issue.labels.iter().map(|l| l.name.clone()).collect()
}

fn get_current_issue_assignees(
    issues_state: &State<IssuesState>,
    filter_idx: usize,
    cursor: usize,
) -> Vec<String> {
    let state = issues_state.read();
    let Some(filter) = state.filters.get(filter_idx) else {
        return vec![];
    };
    let Some(issue) = filter.issues.get(cursor) else {
        return vec![];
    };
    issue.assignees.iter().map(|a| a.login.clone()).collect()
}

#[allow(clippy::too_many_lines)]
fn build_issue_sidebar_meta(
    issue: &Issue,
    theme: &ResolvedTheme,
    depth: ColorDepth,
) -> SidebarMeta {
    let icons = &theme.icons;

    // Pill: Open (green) / Closed (red)
    let (pill_icon, pill_text, pill_bg_app) = match issue.state {
        crate::github::types::IssueState::Open => (
            icons.issue_open.clone(),
            "Open".to_owned(),
            theme.pill_open_bg,
        ),
        crate::github::types::IssueState::Closed | crate::github::types::IssueState::Unknown => (
            icons.issue_closed.clone(),
            "Closed".to_owned(),
            theme.pill_closed_bg,
        ),
    };

    // Author login (issues have no branch — use empty branch_text)
    let author_login = issue
        .author
        .as_ref()
        .map_or_else(|| "unknown".to_owned(), |a| a.login.clone());

    // Participants: assignee logins
    let participants: Vec<String> = issue
        .assignees
        .iter()
        .map(|a| format!("@{}", a.login))
        .collect();

    // Overview metadata (pinned in fixed section)
    let labels_text = if issue.labels.is_empty() {
        None
    } else {
        Some(
            issue
                .labels
                .iter()
                .map(|l| crate::util::expand_emoji(&l.name))
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    let assignees_text = if issue.assignees.is_empty() {
        None
    } else {
        Some(
            issue
                .assignees
                .iter()
                .map(|a| a.login.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    let fmt = "%Y-%m-%d %H:%M:%S";
    let created_text = issue
        .created_at
        .with_timezone(&chrono::Local)
        .format(fmt)
        .to_string();
    let created_age = crate::util::format_date(&issue.created_at, "relative");
    let updated_text = issue
        .updated_at
        .with_timezone(&chrono::Local)
        .format(fmt)
        .to_string();
    let updated_age = crate::util::format_date(&issue.updated_at, "relative");

    // Reactions
    let r = &issue.reactions;
    let reactions_text = if r.total() > 0 {
        let mut parts = Vec::new();
        if r.thumbs_up > 0 {
            parts.push(format!("\u{1f44d} {}", r.thumbs_up));
        }
        if r.thumbs_down > 0 {
            parts.push(format!("\u{1f44e} {}", r.thumbs_down));
        }
        if r.laugh > 0 {
            parts.push(format!("\u{1f604} {}", r.laugh));
        }
        if r.hooray > 0 {
            parts.push(format!("\u{1f389} {}", r.hooray));
        }
        if r.confused > 0 {
            parts.push(format!("\u{1f615} {}", r.confused));
        }
        if r.heart > 0 {
            parts.push(format!("\u{2764}\u{fe0f} {}", r.heart));
        }
        if r.rocket > 0 {
            parts.push(format!("\u{1f680} {}", r.rocket));
        }
        if r.eyes > 0 {
            parts.push(format!("\u{1f440} {}", r.eyes));
        }
        Some(parts.join("  "))
    } else {
        None
    };

    SidebarMeta {
        pill_icon,
        pill_text,
        pill_bg: pill_bg_app.to_crossterm_color(depth),
        pill_fg: theme.pill_fg.to_crossterm_color(depth),
        pill_left: icons.pill_left.clone(),
        pill_right: icons.pill_right.clone(),
        branch_text: String::new(),
        branch_fg: theme.pill_branch.to_crossterm_color(depth),
        update_text: None,
        update_fg: theme.text_faint.to_crossterm_color(depth),
        author_login,
        role_icon: String::new(),
        role_text: String::new(),
        role_fg: theme.text_role.to_crossterm_color(depth),
        label_fg: theme.text_secondary.to_crossterm_color(depth),
        participants,
        participants_fg: theme.text_actor.to_crossterm_color(depth),
        labels_text,
        assignees_text,
        created_text,
        created_age,
        updated_text,
        updated_age,
        lines_added: None,
        lines_deleted: None,
        reactions_text,
        date_fg: theme.text_faint.to_crossterm_color(depth),
        date_age_fg: theme.text_secondary.to_crossterm_color(depth),
        additions_fg: theme.text_success.to_crossterm_color(depth),
        deletions_fg: theme.text_error.to_crossterm_color(depth),
        separator_fg: theme.md_horizontal_rule.to_crossterm_color(depth),
        primary_fg: theme.text_primary.to_crossterm_color(depth),
        actor_fg: theme.text_actor.to_crossterm_color(depth),
        reactions_fg: theme.text_primary.to_crossterm_color(depth),
    }
}

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}
