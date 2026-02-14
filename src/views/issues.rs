use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_compat::Compat;
use iocraft::prelude::*;
use octocrab::Octocrab;

use crate::actions::{clipboard, issue_actions};
use crate::color::ColorDepth;
use crate::components::footer::{Footer, RenderedFooter};
use crate::components::sidebar::{RenderedSidebar, Sidebar};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::components::text_input::{self, RenderedTextInput, TextInput};
use crate::config::types::IssueSection;
use crate::filter;
use crate::github::graphql;
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
        },
        Column {
            id: "title".to_owned(),
            header: "Title".to_owned(),
            default_width_pct: 0.30,
            align: TextAlign::Left,
        },
        Column {
            id: "repo".to_owned(),
            header: "Repo".to_owned(),
            default_width_pct: 0.14,
            align: TextAlign::Left,
        },
        Column {
            id: "creator".to_owned(),
            header: "Creator".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Left,
        },
        Column {
            id: "assignees".to_owned(),
            header: "Assignees".to_owned(),
            default_width_pct: 0.12,
            align: TextAlign::Left,
        },
        Column {
            id: "comments".to_owned(),
            header: "Cmt".to_owned(),
            default_width_pct: 0.05,
            align: TextAlign::Right,
        },
        Column {
            id: "reactions".to_owned(),
            header: "React".to_owned(),
            default_width_pct: 0.06,
            align: TextAlign::Right,
        },
        Column {
            id: "updated".to_owned(),
            header: "Updated".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Right,
        },
        Column {
            id: "created".to_owned(),
            header: "Created".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Right,
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
    pub theme: Option<&'a ResolvedTheme>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
    pub preview_width_pct: f64,
    pub show_section_count: bool,
    pub show_separator: bool,
    pub should_exit: Option<State<bool>>,
    pub switch_view: Option<State<bool>>,
    pub date_format: Option<&'a str>,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn IssuesView<'a>(props: &IssuesViewProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let sections_cfg = props.sections.unwrap_or(&[]);
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let section_count = sections_cfg.len();
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

    let initial_sections = vec![SectionData::default(); section_count];
    let mut issues_state = hooks.use_state(move || IssuesState {
        sections: initial_sections,
    });
    let mut fetch_triggered = hooks.use_state(|| false);

    // Clone octocrab for use in action closures.
    let octocrab_for_actions = props.octocrab.map(Arc::clone);

    // Trigger data fetch on first render.
    if !fetch_triggered.get()
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
                match graphql::search_issues_all(&octocrab, filters, *limit).await {
                    Ok(issues) => {
                        let rows: Vec<Row> = issues
                            .iter()
                            .map(|issue| issue_to_row(issue, &theme_clone, &date_format_owned))
                            .collect();
                        let bodies: Vec<String> = issues.iter().map(|i| i.body.clone()).collect();
                        let titles: Vec<String> = issues.iter().map(|i| i.title.clone()).collect();
                        let issue_count = issues.len();
                        new_sections.push(SectionData {
                            rows,
                            bodies,
                            titles,
                            issue_count,
                            loading: false,
                            error: None,
                            issues,
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
            issues_state.set(IssuesState {
                sections: new_sections,
            });
        }))
        .detach();
    }

    let state_ref = issues_state.read();
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
                            &issues_state,
                            current_section_idx,
                            octocrab_for_actions.as_ref(),
                            fetch_triggered,
                        );
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

    let status = if let Some(msg) = action_status.read().as_ref() {
        msg.clone()
    } else if current_data.is_some_and(|d| d.loading) {
        "Fetching issues...".to_owned()
    } else if let Some(err) = current_data.and_then(|d| d.error.as_ref()) {
        format!("Error: {err}")
    } else {
        let total = current_data.map_or(0, |d| d.issue_count);
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        if search_q.is_empty() {
            format!("{cursor_pos}/{total}")
        } else {
            format!("{cursor_pos}/{total_rows} (filtered from {total})")
        }
    };

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

    let help_text = match &current_mode {
        InputMode::Normal => {
            "[j/k] Nav  [h/l] Sections  [/] Search  [s] Switch  [p] Preview  [r] Refresh  [c] Comment  [L] Label  [a/A] Assign  [x/X] Close/Reopen  [y] Copy#  [o] Open  [q] Quit"
        }
        InputMode::Comment | InputMode::Assign => "[Ctrl+D] Submit  [Esc] Cancel",
        InputMode::Label => "[Tab/Enter] Select  [Esc] Cancel",
        InputMode::Confirm(_) => "[y] Confirm  [n/Esc] Cancel",
        InputMode::Search => "[Enter] Confirm  [Esc] Clear & Cancel",
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

            View(flex_grow: 1.0, flex_direction: FlexDirection::Row) {
                View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                    ScrollableTable(table: rendered_table)
                }
                Sidebar(sidebar: rendered_sidebar)
            }

            TextInput(input: rendered_text_input)
            Footer(footer: rendered_footer)
        }
    }
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
    issues_state: &State<IssuesState>,
    current_section_idx: usize,
    octocrab_for_actions: Option<&Arc<Octocrab>>,
    mut fetch_triggered: State<bool>,
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
        KeyCode::Char('s') => {
            if let Some(mut sv) = switch_view {
                sv.set(true);
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
                let info = get_current_issue_info(issues_state, current_section_idx, cursor.get());
                if let Some((owner, repo, _)) = info {
                    let octocrab = Arc::clone(octocrab);
                    smol::spawn(Compat::new(async move {
                        if let Ok(labels) =
                            graphql::fetch_repo_labels(&octocrab, &owner, &repo).await
                        {
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
            let info = get_current_issue_info(issues_state, current_section_idx, cursor.get());
            if let Some((_, _, number)) = info {
                let text = number.to_string();
                match clipboard::copy_to_clipboard(&text) {
                    Ok(()) => action_status.set(Some(format!("Copied #{number}"))),
                    Err(e) => action_status.set(Some(format!("Copy failed: {e}"))),
                }
            }
        }
        KeyCode::Char('Y') => {
            let info = get_current_issue_info(issues_state, current_section_idx, cursor.get());
            if let Some((owner, repo, number)) = info {
                let url = format!("https://github.com/{owner}/{repo}/issues/{number}");
                match clipboard::copy_to_clipboard(&url) {
                    Ok(()) => action_status.set(Some(format!("Copied URL for #{number}"))),
                    Err(e) => action_status.set(Some(format!("Copy failed: {e}"))),
                }
            }
        }
        KeyCode::Char('o') => {
            let info = get_current_issue_info(issues_state, current_section_idx, cursor.get());
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
            fetch_triggered.set(false);
            let mut st = *issues_state;
            st.set(IssuesState {
                sections: vec![SectionData::default(); section_count],
            });
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
