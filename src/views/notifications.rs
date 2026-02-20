use std::collections::HashMap;

use iocraft::prelude::*;

use crate::actions::clipboard;
use crate::app::ViewKind;
use crate::color::ColorDepth;
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::components::text_input::{RenderedTextInput, TextInput};
use crate::config::keybindings::{MergedBindings, ViewContext};
use crate::config::types::NotificationFilter;
use crate::engine::{EngineHandle, Event, Request};
use crate::filter;
use crate::theme::ResolvedTheme;
use crate::types::{Notification, RateLimitInfo, SubjectType};

// ---------------------------------------------------------------------------
// Notification-specific column definitions (FR-031)
// ---------------------------------------------------------------------------

fn notification_columns() -> Vec<Column> {
    vec![
        Column {
            id: "unread".to_owned(),
            header: " ".to_owned(),
            default_width_pct: 0.03,
            align: TextAlign::Center,
            fixed_width: Some(3),
        },
        Column {
            id: "type".to_owned(),
            header: "Type".to_owned(),
            default_width_pct: 0.05,
            align: TextAlign::Center,
            fixed_width: Some(6),
        },
        Column {
            id: "title".to_owned(),
            header: "Title".to_owned(),
            default_width_pct: 0.38,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "repo".to_owned(),
            header: "Repo".to_owned(),
            default_width_pct: 0.20,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "reason".to_owned(),
            header: "Reason".to_owned(),
            default_width_pct: 0.14,
            align: TextAlign::Left,
            fixed_width: None,
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

/// Convert a `Notification` into a table `Row`.
fn notification_to_row(
    notification: &Notification,
    theme: &ResolvedTheme,
    date_format: &str,
) -> Row {
    let mut row = HashMap::new();

    // Unread indicator.
    let icons = &theme.icons;
    let (unread_icon, unread_color) = if notification.unread {
        (icons.notif_unread.clone(), theme.text_success)
    } else {
        (" ".to_owned(), theme.text_faint)
    };
    row.insert(
        "unread".to_owned(),
        Cell::colored(unread_icon, unread_color),
    );

    // Subject type icon.
    let (type_icon, type_color) = match notification.subject_type {
        Some(SubjectType::PullRequest) => (icons.notif_type_pr.clone(), theme.text_success),
        Some(SubjectType::Issue) => (icons.notif_type_issue.clone(), theme.text_warning),
        Some(SubjectType::Release) => (icons.notif_type_release.clone(), theme.text_actor),
        Some(SubjectType::Discussion) => {
            (icons.notif_type_discussion.clone(), theme.text_secondary)
        }
        _ => ("?".to_owned(), theme.text_faint),
    };
    row.insert("type".to_owned(), Cell::colored(type_icon, type_color));

    // Title.
    if notification.unread {
        row.insert("title".to_owned(), Cell::bold(&notification.subject_title));
    } else {
        row.insert("title".to_owned(), Cell::plain(&notification.subject_title));
    }

    // Repo.
    let repo_name = notification
        .repository
        .as_ref()
        .map_or_else(String::new, crate::github::types::RepoRef::full_name);
    row.insert(
        "repo".to_owned(),
        Cell::colored(repo_name, theme.text_secondary),
    );

    // Reason.
    let reason_text = notification.reason.as_str();
    row.insert(
        "reason".to_owned(),
        Cell::colored(reason_text, theme.text_faint),
    );

    // Updated.
    let updated = crate::util::format_date(&notification.updated_at, date_format);
    row.insert(
        "updated".to_owned(),
        Cell::colored(updated, theme.text_faint),
    );

    row
}

/// Destructive or bulk actions that require confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingAction {
    MarkAllDone,
    MarkAllRead,
    Unsubscribe,
    MarkDone,
}

/// Input modes for the notifications view.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    /// Confirmation prompt for a destructive/bulk action (y/n).
    Confirm(PendingAction),
    Search,
}

// ---------------------------------------------------------------------------
// Filter state (T053)
// ---------------------------------------------------------------------------

/// State for a single filter.
#[derive(Debug, Clone)]
struct FilterData {
    rows: Vec<Row>,
    /// Notification IDs indexed same as rows (used by action keybindings).
    ids: Vec<String>,
    /// Original notification objects for structured filtering (T089).
    notifications: Vec<Notification>,
    notification_count: usize,
    loading: bool,
    error: Option<String>,
}

impl Default for FilterData {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            ids: Vec::new(),
            notifications: Vec::new(),
            notification_count: 0,
            loading: true,
            error: None,
        }
    }
}

/// Shared state across all notification filters.
#[derive(Debug, Clone)]
struct NotificationsState {
    filters: Vec<FilterData>,
}

// ---------------------------------------------------------------------------
// NotificationsView component (T053-T054)
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct NotificationsViewProps<'a> {
    pub filters: Option<&'a [NotificationFilter]>,
    /// Engine handle (replaces octocrab; used after T022-T023 refactor).
    pub engine: Option<&'a EngineHandle>,
    pub theme: Option<&'a ResolvedTheme>,
    /// Merged keybindings for help overlay.
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
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
    /// Date format string (from `config.defaults.date_format`).
    pub date_format: Option<&'a str>,
    /// Whether this view is the currently active (visible) one.
    pub is_active: bool,
    /// Auto-refetch interval in minutes (0 = disabled).
    pub refetch_interval_minutes: u32,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn NotificationsView<'a>(
    props: &NotificationsViewProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
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

    let mut active_filter = hooks.use_state(|| 0usize);
    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);

    // State: input mode and search (T087, T089).
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut search_query = hooks.use_state(String::new);
    let mut help_visible = hooks.use_state(|| false);
    let mut action_status = hooks.use_state(|| Option::<String>::None);
    let mut rate_limit_state = hooks.use_state(|| Option::<RateLimitInfo>::None);

    // Whether RegisterNotificationsRefresh has been sent to the engine yet.
    let mut refresh_registered = hooks.use_state(|| false);

    // State: per-filter fetch tracking (lazy: only fetch the active filter).
    let mut filter_fetch_times =
        hooks.use_state(move || vec![Option::<std::time::Instant>::None; filter_count]);
    let mut filter_in_flight = hooks.use_state(move || vec![false; filter_count]);

    let initial_filters = vec![FilterData::default(); filter_count];
    let mut notif_state = hooks.use_state(move || NotificationsState {
        filters: initial_filters,
    });

    // Event channel: engine sends events back to this view.
    let event_channel = hooks.use_state(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        (tx, std::sync::Arc::new(std::sync::Mutex::new(rx)))
    });
    let (event_tx, event_rx_arc) = event_channel.read().clone();
    // Clone so it can be captured in 'static futures.
    let engine: Option<crate::engine::EngineHandle> = props.engine.cloned();

    // Track scope changes: when scope_repo changes, invalidate all filters.
    let mut last_scope = hooks.use_state(|| scope_repo.clone());
    if *last_scope.read() != *scope_repo {
        last_scope.set(scope_repo.clone());
        notif_state.set(NotificationsState {
            filters: vec![FilterData::default(); filter_count],
        });
        filter_fetch_times.set(vec![None; filter_count]);
        filter_in_flight.set(vec![false; filter_count]);
    }

    // Compute active filter index early (needed by fetch logic below).
    let current_filter_idx = active_filter.get().min(filter_count.saturating_sub(1));

    // Lazy fetch: only fetch the active filter when it needs data.
    let active_needs_fetch = notif_state
        .read()
        .filters
        .get(current_filter_idx)
        .is_some_and(|s| s.loading);
    let active_in_flight = filter_in_flight
        .read()
        .get(current_filter_idx)
        .copied()
        .unwrap_or(false);

    // Register all filters for background refresh once at mount.
    if !refresh_registered.get()
        && let Some(ref eng) = engine
    {
        eng.send(Request::RegisterNotificationsRefresh {
            filter_configs: filters_cfg.to_vec(),
            notify_tx: event_tx.clone(),
        });
        refresh_registered.set(true);
    }

    if active_needs_fetch
        && !active_in_flight
        && is_active
        && let Some(cfg) = filters_cfg.get(current_filter_idx)
        && let Some(ref eng) = engine
    {
        let mut in_flight = filter_in_flight.read().clone();
        if current_filter_idx < in_flight.len() {
            in_flight[current_filter_idx] = true;
        }
        filter_in_flight.set(in_flight);

        let filter_idx = current_filter_idx;
        let mut modified_filter = cfg.clone();
        // Inject repo scope into the filter string if active and not already present.
        if let Some(ref repo) = *scope_repo {
            let has_repo = modified_filter.filters.contains("repo:");
            if !has_repo {
                format!("{} repo:{repo}", modified_filter.filters)
                    .trim()
                    .clone_into(&mut modified_filter.filters);
            }
        }

        eng.send(Request::FetchNotifications {
            filter_idx,
            filter: modified_filter,
            reply_tx: event_tx.clone(),
        });
    }

    // Poll engine events and update local state.
    {
        let rx_for_poll = event_rx_arc.clone();
        let engine_for_poll = engine.clone();
        let event_tx_for_poll = event_tx.clone();
        let current_filter_for_poll = current_filter_idx;
        let theme_for_poll = theme.clone();
        let date_format_for_poll = props.date_format.unwrap_or("relative").to_owned();
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
                        Event::NotificationsFetched {
                            filter_idx,
                            notifications,
                        } => {
                            let rows: Vec<Row> = notifications
                                .iter()
                                .map(|n| {
                                    notification_to_row(n, &theme_for_poll, &date_format_for_poll)
                                })
                                .collect();
                            let ids: Vec<String> =
                                notifications.iter().map(|n| n.id.clone()).collect();
                            let notification_count = notifications.len();
                            let filter_data = FilterData {
                                rows,
                                ids,
                                notifications,
                                notification_count,
                                loading: false,
                                error: None,
                            };
                            let mut state = notif_state.read().clone();
                            if filter_idx < state.filters.len() {
                                state.filters[filter_idx] = filter_data;
                            }
                            notif_state.set(state);
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
                            // Piggyback a rate-limit check — REST API provides none.
                            if let Some(ref eng) = engine_for_poll {
                                eng.send(Request::FetchRateLimit {
                                    reply_tx: event_tx_for_poll.clone(),
                                });
                            }
                        }
                        Event::FetchError {
                            context: _,
                            message,
                        } => {
                            let ifl = filter_in_flight.read().clone();
                            let fi = ifl.iter().position(|&f| f);
                            if let Some(fi) = fi {
                                let mut state = notif_state.read().clone();
                                if fi < state.filters.len() {
                                    state.filters[fi] = FilterData {
                                        loading: false,
                                        error: Some(message.clone()),
                                        ..FilterData::default()
                                    };
                                }
                                notif_state.set(state);
                                let mut times = filter_fetch_times.read().clone();
                                if fi < times.len() {
                                    times[fi] = Some(std::time::Instant::now());
                                }
                                filter_fetch_times.set(times);
                                let mut ifl2 = filter_in_flight.read().clone();
                                if fi < ifl2.len() {
                                    ifl2[fi] = false;
                                }
                                filter_in_flight.set(ifl2);
                            }
                        }
                        Event::MutationOk { description } => {
                            action_status.set(Some(format!("✓ {description}")));
                            // Trigger refetch of current filter.
                            let mut state = notif_state.read().clone();
                            if current_filter_for_poll < state.filters.len() {
                                state.filters[current_filter_for_poll] = FilterData::default();
                            }
                            notif_state.set(state);
                            let mut times = filter_fetch_times.read().clone();
                            if current_filter_for_poll < times.len() {
                                times[current_filter_for_poll] = None;
                            }
                            filter_fetch_times.set(times);
                        }
                        Event::MutationError {
                            description,
                            message,
                        } => {
                            action_status.set(Some(format!("✗ {description}: {message}")));
                        }
                        Event::RateLimitUpdated { info } => {
                            rate_limit_state.set(Some(info));
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    let state_ref = notif_state.read();
    let all_rows_count = state_ref
        .filters
        .get(current_filter_idx)
        .map_or(0, |s| s.rows.len());
    let search_q = search_query.read().clone();
    let total_rows = if search_q.is_empty() {
        all_rows_count
    } else {
        state_ref.filters.get(current_filter_idx).map_or(0, |s| {
            filter::filter_notifications(&s.notifications, &s.rows, &search_q).len()
        })
    };

    // Reserve space for tab bar (2 lines), footer (2 lines), header (1 line).
    let visible_rows = props.height.saturating_sub(5) as usize;

    // Keyboard handling.
    hooks.use_terminal_events({
        let engine_for_keys = engine.clone();
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
                    InputMode::Confirm(ref pending) => match code {
                        KeyCode::Char('y' | 'Y') => {
                            if let Some(ref eng) = engine_for_keys {
                                match pending {
                                    PendingAction::Unsubscribe => {
                                        let notif = get_current_notification(
                                            &notif_state,
                                            current_filter_idx,
                                            cursor.get(),
                                        );
                                        if let Some(n) = notif {
                                            let id = n.id.clone();
                                            eng.send(Request::UnsubscribeNotification {
                                                id,
                                                reply_tx: event_tx.clone(),
                                            });
                                            remove_notification(
                                                notif_state,
                                                current_filter_idx,
                                                cursor.get(),
                                            );
                                            clamp_cursor(
                                                cursor,
                                                scroll_offset,
                                                total_rows.saturating_sub(1),
                                            );
                                        }
                                    }
                                    PendingAction::MarkAllRead => {
                                        eng.send(Request::MarkAllNotificationsRead {
                                            reply_tx: event_tx.clone(),
                                        });
                                        clear_filter(notif_state, current_filter_idx);
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    PendingAction::MarkAllDone => {
                                        // Mark all as done: mark each notification as read
                                        // (GitHub doesn't have a "mark as done" bulk API via
                                        // our engine; use MarkAllNotificationsRead as best effort).
                                        eng.send(Request::MarkAllNotificationsRead {
                                            reply_tx: event_tx.clone(),
                                        });
                                        clear_filter(notif_state, current_filter_idx);
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    PendingAction::MarkDone => {
                                        let notif = get_current_notification(
                                            &notif_state,
                                            current_filter_idx,
                                            cursor.get(),
                                        );
                                        if let Some(n) = notif {
                                            let id = n.id.clone();
                                            eng.send(Request::MarkNotificationDone {
                                                id,
                                                reply_tx: event_tx.clone(),
                                            });
                                            remove_notification(
                                                notif_state,
                                                current_filter_idx,
                                                cursor.get(),
                                            );
                                            clamp_cursor(
                                                cursor,
                                                scroll_offset,
                                                total_rows.saturating_sub(1),
                                            );
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
                        KeyCode::Char('n') => {
                            if let Some(mut sv) = switch_view {
                                sv.set(true);
                            }
                        }
                        // Switch view back
                        KeyCode::Char('N') => {
                            if let Some(mut sv) = switch_view_back {
                                sv.set(true);
                            }
                        }
                        // Toggle repo scope
                        KeyCode::Char('S') => {
                            if let Some(mut st) = scope_toggle {
                                st.set(true);
                            }
                        }
                        // Clipboard & Browser (T091, T092)
                        KeyCode::Char('y') => {
                            let notif = get_current_notification(
                                &notif_state,
                                current_filter_idx,
                                cursor.get(),
                            );
                            if let Some(n) = notif {
                                let _ = clipboard::copy_to_clipboard(&n.subject_title);
                            }
                        }
                        KeyCode::Char('Y') => {
                            let notif = get_current_notification(
                                &notif_state,
                                current_filter_idx,
                                cursor.get(),
                            );
                            if let Some(n) = notif
                                && !n.url.is_empty()
                            {
                                let _ = clipboard::copy_to_clipboard(&n.url);
                            }
                        }
                        KeyCode::Char('o') => {
                            let notif = get_current_notification(
                                &notif_state,
                                current_filter_idx,
                                cursor.get(),
                            );
                            if let Some(n) = notif
                                && !n.url.is_empty()
                            {
                                let _ = clipboard::open_in_browser(&n.url);
                            }
                        }
                        // Retry / refresh
                        KeyCode::Char('r') => {
                            let idx = active_filter.get();
                            let mut state = notif_state.read().clone();
                            if idx < state.filters.len() {
                                state.filters[idx] = FilterData::default();
                            }
                            notif_state.set(state);
                            let mut times = filter_fetch_times.read().clone();
                            if idx < times.len() {
                                times[idx] = None;
                            }
                            filter_fetch_times.set(times);
                            cursor.set(0);
                            scroll_offset.set(0);
                        }
                        // Search (T087)
                        KeyCode::Char('/') => {
                            input_mode.set(InputMode::Search);
                            search_query.set(String::new());
                        }

                        // -------------------------------------------------------
                        // Notification actions
                        // -------------------------------------------------------

                        // Mark as done (d) — with confirmation
                        KeyCode::Char('d') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            input_mode.set(InputMode::Confirm(PendingAction::MarkDone));
                            action_status.set(None);
                        }
                        // Mark as read (m) — immediate, no confirm
                        KeyCode::Char('m') => {
                            if let Some(ref eng) = engine_for_keys {
                                let notif = get_current_notification(
                                    &notif_state,
                                    current_filter_idx,
                                    cursor.get(),
                                );
                                if let Some(n) = notif {
                                    let id = n.id.clone();
                                    eng.send(Request::MarkNotificationRead {
                                        id,
                                        reply_tx: event_tx.clone(),
                                    });
                                    remove_notification(
                                        notif_state,
                                        current_filter_idx,
                                        cursor.get(),
                                    );
                                    clamp_cursor(
                                        cursor,
                                        scroll_offset,
                                        total_rows.saturating_sub(1),
                                    );
                                }
                            }
                        }
                        // Mark all as read (M) — confirm first
                        KeyCode::Char('M') => {
                            input_mode.set(InputMode::Confirm(PendingAction::MarkAllRead));
                            action_status.set(None);
                        }
                        // Mark all as done (D) — confirm first
                        KeyCode::Char('D') => {
                            input_mode.set(InputMode::Confirm(PendingAction::MarkAllDone));
                            action_status.set(None);
                        }
                        // Unsubscribe (u, plain — not Ctrl+u) — confirm first
                        KeyCode::Char('u') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            input_mode.set(InputMode::Confirm(PendingAction::Unsubscribe));
                            action_status.set(None);
                        }

                        // -------------------------------------------------------
                        // Cursor movement
                        // -------------------------------------------------------
                        KeyCode::Down | KeyCode::Char('j') => {
                            if total_rows > 0 {
                                let new_cursor =
                                    (cursor.get() + 1).min(total_rows.saturating_sub(1));
                                cursor.set(new_cursor);
                                if new_cursor >= scroll_offset.get() + visible_rows {
                                    scroll_offset.set(new_cursor.saturating_sub(visible_rows) + 1);
                                }
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            let new_cursor = cursor.get().saturating_sub(1);
                            cursor.set(new_cursor);
                            if new_cursor < scroll_offset.get() {
                                scroll_offset.set(new_cursor);
                            }
                        }
                        KeyCode::Char('g') => {
                            cursor.set(0);
                            scroll_offset.set(0);
                        }
                        KeyCode::Char('G') => {
                            if total_rows > 0 {
                                cursor.set(total_rows.saturating_sub(1));
                                scroll_offset.set(total_rows.saturating_sub(visible_rows));
                            }
                        }
                        KeyCode::PageDown => {
                            if total_rows > 0 {
                                let new_cursor =
                                    (cursor.get() + visible_rows).min(total_rows.saturating_sub(1));
                                cursor.set(new_cursor);
                                scroll_offset
                                    .set(new_cursor.saturating_sub(visible_rows.saturating_sub(1)));
                            }
                        }
                        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                            if total_rows > 0 {
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
                        }
                        KeyCode::Char('u') if modifiers.contains(KeyModifiers::CONTROL) => {
                            let half = visible_rows / 2;
                            let new_cursor = cursor.get().saturating_sub(half);
                            cursor.set(new_cursor);
                            if new_cursor < scroll_offset.get() {
                                scroll_offset.set(new_cursor);
                            }
                        }
                        // Section switching
                        KeyCode::Char('h') | KeyCode::Left => {
                            if filter_count > 0 {
                                let current = active_filter.get();
                                active_filter.set(if current == 0 {
                                    filter_count.saturating_sub(1)
                                } else {
                                    current - 1
                                });
                                cursor.set(0);
                                scroll_offset.set(0);
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            if filter_count > 0 {
                                active_filter.set((active_filter.get() + 1) % filter_count);
                                cursor.set(0);
                                scroll_offset.set(0);
                            }
                        }
                        KeyCode::Char('?') => {
                            help_visible.set(true);
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
    let tabs: Vec<Tab> = filters_cfg
        .iter()
        .enumerate()
        .map(|(i, s)| Tab {
            title: s.title.clone(),
            count: state_ref.filters.get(i).map(|d| d.notification_count),
        })
        .collect();

    let current_data = state_ref.filters.get(current_filter_idx);
    let columns = notification_columns();

    let all_rows: &[Row] = current_data.map_or(&[], |d| d.rows.as_slice());
    let all_notifs: &[Notification] = current_data.map_or(&[], |d| d.notifications.as_slice());
    let filtered_indices = filter::filter_notifications(all_notifs, all_rows, &search_q);
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
        hidden_columns: None,
        width_overrides: None,
        total_width: props.width,
        depth,
        selected_bg: Some(theme.bg_selected),
        header_color: Some(theme.text_secondary),
        border_color: Some(theme.border_faint),
        show_separator: props.show_separator,
        empty_message: if search_q.is_empty() {
            Some("No notifications found")
        } else {
            Some("No notifications match this filter")
        },
        subtitle_column: None,
        row_separator: true,
    });

    let rendered_tab_bar = RenderedTabBar::build(
        &tabs,
        current_filter_idx,
        props.show_filter_count,
        depth,
        Some(theme.footer_notifications),
        Some(theme.footer_notifications),
        Some(theme.border_faint),
        &theme.icons.tab_filter,
    );

    let current_mode = input_mode.read().clone();

    let rendered_text_input = match &current_mode {
        InputMode::Confirm(action) => {
            let prompt = match action {
                PendingAction::MarkAllDone => "Mark ALL notifications as done? (y/n)",
                PendingAction::MarkAllRead => "Mark ALL notifications as read? (y/n)",
                PendingAction::Unsubscribe => {
                    "Unsubscribe from this thread? This is irreversible. (y/n)"
                }
                PendingAction::MarkDone => "Mark this notification as done? (y/n)",
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
        "Fetching notifications...".to_owned()
    } else if let Some(err) = current_data.and_then(|d| d.error.as_ref()) {
        format!("Error: {err}")
    } else {
        let total = current_data.map_or(0, |d| d.notification_count);
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        if search_q.is_empty() {
            format!("Notif {cursor_pos}/{total}")
        } else {
            format!("Notif {cursor_pos}/{total_rows} (filtered from {total})")
        }
    };
    let active_fetch_time = filter_fetch_times
        .read()
        .get(current_filter_idx)
        .copied()
        .flatten();
    let updated_text = footer::format_updated_ago(active_fetch_time);

    let scope_label = match scope_repo {
        Some(repo) => repo.clone(),
        None => "all repos".to_owned(),
    };
    let rate_limit_text = footer::format_rate_limit(rate_limit_state.read().as_ref());
    let rendered_footer = RenderedFooter::build(
        ViewKind::Notifications,
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
                context: ViewContext::Notifications,
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

            View(flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow::Hidden) {
                ScrollableTable(table: rendered_table)
            }

            TextInput(input: rendered_text_input)
            Footer(footer: rendered_footer)
            HelpOverlay(overlay: rendered_help, width: props.width, height: props.height)
        }
    }
    .into_any()
}

fn get_current_notification(
    notif_state: &State<NotificationsState>,
    filter_idx: usize,
    cursor: usize,
) -> Option<Notification> {
    let state = notif_state.read();
    let filter = state.filters.get(filter_idx)?;
    filter.notifications.get(cursor).cloned()
}

/// Remove a notification at `index` from filter `filter_idx` in local state.
fn remove_notification(
    mut notif_state: State<NotificationsState>,
    filter_idx: usize,
    index: usize,
) {
    let mut state = notif_state.read().clone();
    if let Some(filter) = state.filters.get_mut(filter_idx)
        && index < filter.rows.len()
    {
        filter.rows.remove(index);
        filter.ids.remove(index);
        filter.notifications.remove(index);
        filter.notification_count = filter.notifications.len();
    }
    notif_state.set(state);
}

/// Clear all notifications from a filter in local state.
fn clear_filter(mut notif_state: State<NotificationsState>, filter_idx: usize) {
    let mut state = notif_state.read().clone();
    if let Some(filter) = state.filters.get_mut(filter_idx) {
        filter.rows.clear();
        filter.ids.clear();
        filter.notifications.clear();
        filter.notification_count = 0;
    }
    notif_state.set(state);
}

/// Clamp cursor and scroll offset after removing an item.
fn clamp_cursor(mut cursor: State<usize>, mut scroll_offset: State<usize>, max_index: usize) {
    if cursor.get() > max_index {
        cursor.set(max_index);
    }
    if scroll_offset.get() > max_index {
        scroll_offset.set(max_index);
    }
}

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}
