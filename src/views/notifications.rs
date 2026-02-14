use std::collections::HashMap;
use std::sync::Arc;

use async_compat::Compat;
use iocraft::prelude::*;
use octocrab::Octocrab;

use crate::actions::clipboard;
use crate::color::ColorDepth;
use crate::components::footer::{Footer, RenderedFooter};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::components::text_input::{RenderedTextInput, TextInput};
use crate::config::types::NotificationSection;
use crate::filter;
use crate::github::notifications::{self, NotificationFilter};
use crate::github::rate_limit;
use crate::github::types::{Notification, SubjectType};
use crate::theme::ResolvedTheme;

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
        },
        Column {
            id: "type".to_owned(),
            header: "Type".to_owned(),
            default_width_pct: 0.05,
            align: TextAlign::Center,
        },
        Column {
            id: "title".to_owned(),
            header: "Title".to_owned(),
            default_width_pct: 0.38,
            align: TextAlign::Left,
        },
        Column {
            id: "repo".to_owned(),
            header: "Repo".to_owned(),
            default_width_pct: 0.20,
            align: TextAlign::Left,
        },
        Column {
            id: "reason".to_owned(),
            header: "Reason".to_owned(),
            default_width_pct: 0.14,
            align: TextAlign::Left,
        },
        Column {
            id: "updated".to_owned(),
            header: "Updated".to_owned(),
            default_width_pct: 0.12,
            align: TextAlign::Right,
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

/// Input modes for the notifications view.
#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    Search,
}

// ---------------------------------------------------------------------------
// Section state (T053)
// ---------------------------------------------------------------------------

/// State for a single notification section.
#[derive(Debug, Clone)]
struct SectionData {
    rows: Vec<Row>,
    /// Notification IDs indexed same as rows (for future action keybindings).
    #[allow(dead_code)]
    ids: Vec<String>,
    /// Original notification objects for structured filtering (T089).
    notifications: Vec<Notification>,
    notification_count: usize,
    loading: bool,
    error: Option<String>,
}

impl Default for SectionData {
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

/// Shared state across all notification sections.
#[derive(Debug, Clone)]
struct NotificationsState {
    sections: Vec<SectionData>,
}

// ---------------------------------------------------------------------------
// NotificationsView component (T053-T054)
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct NotificationsViewProps<'a> {
    pub sections: Option<&'a [NotificationSection]>,
    pub octocrab: Option<&'a Arc<Octocrab>>,
    pub theme: Option<&'a ResolvedTheme>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
    pub show_section_count: bool,
    pub show_separator: bool,
    pub should_exit: Option<State<bool>>,
    pub switch_view: Option<State<bool>>,
    /// Date format string (from `config.defaults.date_format`).
    pub date_format: Option<&'a str>,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn NotificationsView<'a>(
    props: &NotificationsViewProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let sections_cfg = props.sections.unwrap_or(&[]);
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let section_count = sections_cfg.len();

    let mut active_section = hooks.use_state(|| 0usize);
    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);

    // State: input mode and search (T087, T089).
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut search_query = hooks.use_state(String::new);

    let initial_sections = vec![SectionData::default(); section_count];
    let mut notif_state = hooks.use_state(move || NotificationsState {
        sections: initial_sections,
    });
    let mut fetch_triggered = hooks.use_state(|| false);

    // Trigger data fetch on first render.
    if !fetch_triggered.get()
        && !sections_cfg.is_empty()
        && let Some(octocrab) = props.octocrab
    {
        fetch_triggered.set(true);
        let octocrab = Arc::clone(octocrab);
        let configs: Vec<NotificationFilter> = sections_cfg
            .iter()
            .map(|s| notifications::parse_filters(&s.filters, s.limit.unwrap_or(30)))
            .collect();
        let theme_clone = theme.clone();
        let date_format_owned = props.date_format.unwrap_or("relative").to_owned();

        smol::spawn(Compat::new(async move {
            let mut new_sections = Vec::new();
            for filter in &configs {
                match notifications::fetch_notifications(&octocrab, filter).await {
                    Ok(notifs) => {
                        let rows: Vec<Row> = notifs
                            .iter()
                            .map(|n| notification_to_row(n, &theme_clone, &date_format_owned))
                            .collect();
                        let ids: Vec<String> = notifs.iter().map(|n| n.id.clone()).collect();
                        let notification_count = notifs.len();
                        new_sections.push(SectionData {
                            rows,
                            ids,
                            notifications: notifs,
                            notification_count,
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
            notif_state.set(NotificationsState {
                sections: new_sections,
            });
        }))
        .detach();
    }

    let state_ref = notif_state.read();
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
        state_ref.sections.get(current_section_idx).map_or(0, |s| {
            filter::filter_notifications(&s.notifications, &s.rows, &search_q).len()
        })
    };

    // Reserve space for tab bar (2 lines), footer (2 lines), header (1 line).
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
                if *input_mode.read() == InputMode::Search {
                    // Search mode handling.
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
                } else {
                    match code {
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
                        // Clipboard & Browser (T091, T092)
                        KeyCode::Char('y') => {
                            let notif = get_current_notification(
                                &notif_state,
                                current_section_idx,
                                cursor.get(),
                            );
                            if let Some(n) = notif {
                                let _ = clipboard::copy_to_clipboard(&n.subject_title);
                            }
                        }
                        KeyCode::Char('Y') => {
                            let notif = get_current_notification(
                                &notif_state,
                                current_section_idx,
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
                                current_section_idx,
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
                            fetch_triggered.set(false);
                            notif_state.set(NotificationsState {
                                sections: vec![SectionData::default(); section_count],
                            });
                            cursor.set(0);
                            scroll_offset.set(0);
                        }
                        // Search (T087)
                        KeyCode::Char('/') => {
                            input_mode.set(InputMode::Search);
                            search_query.set(String::new());
                        }
                        // Cursor movement
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
                            if section_count > 0 {
                                let current = active_section.get();
                                active_section.set(if current == 0 {
                                    section_count.saturating_sub(1)
                                } else {
                                    current - 1
                                });
                                cursor.set(0);
                                scroll_offset.set(0);
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            if section_count > 0 {
                                active_section.set((active_section.get() + 1) % section_count);
                                cursor.set(0);
                                scroll_offset.set(0);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    });

    // Build tabs.
    let tabs: Vec<Tab> = sections_cfg
        .iter()
        .enumerate()
        .map(|(i, s)| Tab {
            title: s.title.clone(),
            count: state_ref.sections.get(i).map(|d| d.notification_count),
        })
        .collect();

    let current_data = state_ref.sections.get(current_section_idx);
    let columns = notification_columns();

    let status = if current_data.is_some_and(|d| d.loading) {
        "Fetching notifications...".to_owned()
    } else if let Some(err) = current_data.and_then(|d| d.error.as_ref()) {
        format!("Error: {err}")
    } else {
        let total = current_data.map_or(0, |d| d.notification_count);
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        if search_q.is_empty() {
            format!("{cursor_pos}/{total}")
        } else {
            format!("{cursor_pos}/{total_rows} (filtered from {total})")
        }
    };

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
        current_section_idx,
        props.show_section_count,
        depth,
        Some(theme.border_primary),
        Some(theme.text_faint),
        Some(theme.border_faint),
    );

    let is_searching = *input_mode.read() == InputMode::Search;

    let rendered_text_input = if is_searching {
        Some(RenderedTextInput::build(
            "/",
            &search_query.read(),
            depth,
            Some(theme.text_primary),
            Some(theme.text_secondary),
            Some(theme.border_faint),
        ))
    } else {
        None
    };

    let help_text = if is_searching {
        "[Enter] Confirm  [Esc] Clear & Cancel  (repo:, reason:, is:unread/read/all)"
    } else {
        "[j/k] Navigate  [h/l] Sections  [/] Search  [s] Switch  [r] Refresh  [y] Copy  [o] Open  [q] Quit"
    };

    let rendered_footer = RenderedFooter::build(
        help_text.to_owned(),
        status,
        depth,
        Some(theme.text_faint),
        Some(theme.border_faint),
    );

    let width = u32::from(props.width);
    let height = u32::from(props.height);

    element! {
        View(flex_direction: FlexDirection::Column, width, height) {
            TabBar(tab_bar: rendered_tab_bar)

            View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                ScrollableTable(table: rendered_table)
            }

            TextInput(input: rendered_text_input)
            Footer(footer: rendered_footer)
        }
    }
}

fn get_current_notification(
    notif_state: &State<NotificationsState>,
    section_idx: usize,
    cursor: usize,
) -> Option<Notification> {
    let state = notif_state.read();
    let section = state.sections.get(section_idx)?;
    section.notifications.get(cursor).cloned()
}

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}
