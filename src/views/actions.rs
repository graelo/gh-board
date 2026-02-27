use std::collections::{BTreeSet, HashMap, HashSet};

use iocraft::prelude::*;

use crate::actions::clipboard;
use crate::app::{NavigationTarget, ViewKind};
use crate::color::{Color as AppColor, ColorDepth};
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::sidebar::{RenderedSidebar, Sidebar};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::components::text_input::{RenderedTextInput, TextInput};
use crate::config::keybindings::{
    BuiltinAction, MergedBindings, ResolvedBinding, TemplateVars, ViewContext,
    execute_shell_command, expand_template, key_event_to_string,
};
use crate::config::types::ActionsFilter;
use crate::engine::{EngineHandle, Event, Request};
use crate::markdown::renderer::{StyledLine, StyledSpan};
use crate::theme::ResolvedTheme;
use crate::types::{RateLimitInfo, RunConclusion, RunStatus, WorkflowJob, WorkflowRun};
use crate::url::owner_repo_from_url;
use crate::views::MAX_EPHEMERAL_TABS;
use unicode_width::UnicodeWidthStr;

// ---------------------------------------------------------------------------
// Column definitions
// ---------------------------------------------------------------------------

fn actions_columns() -> Vec<Column> {
    vec![
        Column {
            id: "status".to_owned(),
            header: " ".to_owned(),
            default_width_pct: 0.04,
            align: TextAlign::Center,
            fixed_width: Some(3),
        },
        Column {
            id: "run".to_owned(),
            header: "Run".to_owned(),
            default_width_pct: 0.07,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
        Column {
            id: "workflow".to_owned(),
            header: "Workflow".to_owned(),
            default_width_pct: 0.15,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "title".to_owned(),
            header: "Title".to_owned(),
            default_width_pct: 0.30,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "event".to_owned(),
            header: "Event".to_owned(),
            default_width_pct: 0.12,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "branch".to_owned(),
            header: "Branch".to_owned(),
            default_width_pct: 0.16,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "actor".to_owned(),
            header: "Actor".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "age".to_owned(),
            header: "Age".to_owned(),
            default_width_pct: 0.06,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
    ]
}

/// Map run status/conclusion to (icon, color).
fn run_status_icon_color(
    status: RunStatus,
    conclusion: Option<RunConclusion>,
    theme: &ResolvedTheme,
) -> (String, AppColor) {
    let icons = &theme.icons;
    match status {
        RunStatus::Completed => match conclusion {
            Some(RunConclusion::Success) => (icons.action_success.clone(), theme.text_success),
            Some(RunConclusion::Failure | RunConclusion::TimedOut) => {
                (icons.action_failure.clone(), theme.text_error)
            }
            Some(RunConclusion::Cancelled) => (icons.action_cancelled.clone(), theme.text_faint),
            Some(RunConclusion::Skipped | RunConclusion::Neutral) => {
                (icons.action_skipped.clone(), theme.text_faint)
            }
            _ => ("?".to_owned(), theme.text_faint),
        },
        RunStatus::InProgress => (icons.action_running.clone(), theme.text_warning),
        RunStatus::Queued => (icons.action_queued.clone(), theme.text_secondary),
        RunStatus::Unknown => ("?".to_owned(), theme.text_faint),
    }
}

/// Convert a `WorkflowRun` into a table `Row`.
fn run_to_row(run: &WorkflowRun, theme: &ResolvedTheme) -> Row {
    let mut row = HashMap::new();
    let (status_icon, status_color) = run_status_icon_color(run.status, run.conclusion, theme);
    row.insert(
        "status".to_owned(),
        Cell::colored(status_icon, status_color),
    );
    row.insert(
        "run".to_owned(),
        Cell::colored(format!("#{}", run.run_number), theme.text_faint),
    );
    row.insert(
        "workflow".to_owned(),
        Cell::plain(crate::util::expand_emoji(&run.name)),
    );
    row.insert(
        "title".to_owned(),
        Cell::plain(crate::util::expand_emoji(&run.display_title)),
    );
    row.insert(
        "event".to_owned(),
        Cell::colored(run.event.clone(), theme.text_secondary),
    );
    let branch = run.head_branch.as_deref().unwrap_or("-");
    row.insert(
        "branch".to_owned(),
        Cell::colored(branch, theme.text_secondary),
    );
    let actor = run
        .actor
        .as_ref()
        .map_or("-", |a| a.login.as_str())
        .to_owned();
    row.insert("actor".to_owned(), Cell::colored(actor, theme.text_actor));
    let age = crate::util::format_date(&run.created_at, "relative");
    row.insert("age".to_owned(), Cell::colored(age, theme.text_faint));
    row
}

// ---------------------------------------------------------------------------
// Filter state
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FilterData {
    rows: Vec<Row>,
    runs: Vec<WorkflowRun>,
    run_count: usize,
    loading: bool,
    error: Option<String>,
}

impl Default for FilterData {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            runs: Vec::new(),
            run_count: 0,
            loading: true,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ActionsState {
    filters: Vec<FilterData>,
}

// ---------------------------------------------------------------------------
// Input mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    Search,
    Confirm(BuiltinAction),
}

// ---------------------------------------------------------------------------
// Nav panel width
// ---------------------------------------------------------------------------

const NAV_W: u16 = 28;

/// Deferred request for a single workflow run, set by the polling future
/// when an ephemeral tab's bulk fetch completes without the target run.
#[derive(Clone)]
struct PendingRunFetch {
    owner: String,
    repo: String,
    run_id: u64,
    host: Option<String>,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve `@current` to the active scope repo, or return the literal value.
/// Returns `None` only when the repo is `@current` and no scope is available.
fn resolve_filter_repo<'a>(
    repo: &'a str,
    scope_repo: Option<&'a str>,
    detected_repo: Option<&'a str>,
) -> Option<&'a str> {
    if repo == "@current" {
        scope_repo.or(detected_repo)
    } else {
        Some(repo)
    }
}

fn owner_repo_for_run(
    run: &WorkflowRun,
    filter: Option<&ActionsFilter>,
) -> Option<(String, String)> {
    owner_repo_from_url(&run.html_url).or_else(|| {
        filter.and_then(|f| {
            f.repo
                .split_once('/')
                .map(|(o, r)| (o.to_owned(), r.to_owned()))
        })
    })
}

/// Build a merged list of (filter, `is_ephemeral`) from config + ephemeral filters.
fn merged_filters<'a>(
    config: &'a [ActionsFilter],
    ephemeral: &'a [(ActionsFilter, Option<u64>)],
) -> Vec<(&'a ActionsFilter, bool)> {
    let mut out: Vec<_> = config.iter().map(|f| (f, false)).collect();
    out.extend(ephemeral.iter().map(|(f, _)| (f, true)));
    out
}

fn build_jobs_lines(jobs: &[WorkflowJob], loading: bool, theme: &ResolvedTheme) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    if loading {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "Loading jobs\u{2026}",
            theme.text_faint,
        )));
        return lines;
    }
    if jobs.is_empty() {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "No jobs found",
            theme.text_faint,
        )));
        return lines;
    }
    let mut sorted_jobs: Vec<&WorkflowJob> = jobs.iter().collect();
    sorted_jobs.sort_by(|a, b| a.name.cmp(&b.name));

    for (i, job) in sorted_jobs.iter().enumerate() {
        if i > 0 {
            lines.push(StyledLine::from_spans(vec![]));
        }
        let (icon, color) = run_status_icon_color(job.status, job.conclusion, theme);
        let duration = crate::util::format_duration(job.started_at, job.completed_at);
        let dur_text = if duration.is_empty() {
            String::new()
        } else {
            format!("  ({duration})")
        };
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::text(icon, color),
            StyledSpan::text(format!("  {}", job.name), theme.text_primary),
            StyledSpan::text(dur_text, theme.text_faint),
        ]));
        let max_step_name_width = job
            .steps
            .iter()
            .map(|s| UnicodeWidthStr::width(s.name.as_str()))
            .max()
            .unwrap_or(0);
        for step in &job.steps {
            let (step_icon, step_color) =
                run_status_icon_color(step.status, step.conclusion, theme);
            let step_dur = crate::util::format_duration(step.started_at, step.completed_at);
            let mut spans = vec![
                StyledSpan::text("   ", theme.text_faint),
                StyledSpan::text(step_icon, step_color),
                StyledSpan::text(format!("  {}", step.name), theme.text_secondary),
            ];
            if !step_dur.is_empty() {
                let name_w = UnicodeWidthStr::width(step.name.as_str());
                let pad = max_step_name_width - name_w + 2;
                spans.push(StyledSpan::text(
                    format!("{:pad$}{step_dur}", "", pad = pad),
                    theme.text_faint,
                ));
            }
            lines.push(StyledLine::from_spans(spans));
        }
    }
    lines
}

// ---------------------------------------------------------------------------
// ActionsView component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct ActionsViewProps<'a> {
    pub filters: Option<&'a [ActionsFilter]>,
    pub engine: Option<&'a EngineHandle>,
    pub theme: Option<&'a ResolvedTheme>,
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
    /// Preview sidebar width as a fraction of total width.
    pub preview_width_pct: f64,
    pub show_filter_count: bool,
    pub show_separator: bool,
    pub scope_repo: Option<String>,
    pub detected_repo: Option<String>,
    pub should_exit: Option<State<bool>>,
    pub switch_view: Option<State<bool>>,
    pub switch_view_back: Option<State<bool>>,
    pub scope_toggle: Option<State<bool>>,
    pub is_active: bool,
    pub refetch_interval_minutes: u32,
    /// Navigation target state — set by `PrsView`, consumed here.
    pub nav_target: Option<State<Option<NavigationTarget>>>,
    /// Go-back signal — set to true to return to previous view.
    pub go_back: Option<State<bool>>,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn ActionsView<'a>(
    props: &ActionsViewProps<'a>,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'a>> {
    let filters_cfg = props.filters.unwrap_or(&[]);
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let switch_view_back = props.switch_view_back;
    let filter_count = filters_cfg.len();
    let is_active = props.is_active;
    let preview_pct = props.preview_width_pct;
    let scope_repo = props.scope_repo.clone();
    let detected_repo = props.detected_repo.clone();
    let scope_toggle = props.scope_toggle;

    let mut active_filter = hooks.use_state(|| 0usize);
    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut search_query = hooks.use_state(String::new);

    let mut help_visible = hooks.use_state(|| false);

    let mut nav_open = hooks.use_state(|| false);
    let mut nav_cursor = hooks.use_state(|| 0usize);
    let mut nav_focused = hooks.use_state(|| false);

    let mut detail_open = hooks.use_state(|| false);
    let mut detail_scroll = hooks.use_state(|| 0usize);
    let mut jobs_cache = hooks.use_state(HashMap::<u64, Vec<WorkflowJob>>::new);
    let mut jobs_in_flight = hooks.use_state(HashSet::<u64>::new);

    // State: ephemeral tabs created by deep-linking to repos without config tabs.
    // Each entry is (filter, optional pending run_id to highlight after fetch).
    let mut ephemeral_filters = hooks.use_state(Vec::<(ActionsFilter, Option<u64>)>::new);

    // Signal: the polling future sets this when an ephemeral tab's data arrives
    // but the pending run_id is not among the results — the synchronous render
    // path picks it up and sends FetchRunById.
    let mut pending_run_fetch = hooks.use_state(|| Option::<PendingRunFetch>::None);

    let mut action_status = hooks.use_state(|| Option::<String>::None);
    let mut rate_limit_state = hooks.use_state(|| Option::<RateLimitInfo>::None);

    let mut refresh_registered = hooks.use_state(|| false);
    let mut filter_fetch_times =
        hooks.use_state(move || vec![Option::<std::time::Instant>::None; filter_count]);
    let mut filter_in_flight = hooks.use_state(move || vec![false; filter_count]);
    let mut refresh_all = hooks.use_state(|| false);

    let initial_filters = vec![FilterData::default(); filter_count];
    let mut actions_state = hooks.use_state(move || ActionsState {
        filters: initial_filters,
    });

    let event_channel = hooks.use_state(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        (tx, std::sync::Arc::new(std::sync::Mutex::new(rx)))
    });
    let (event_tx, event_rx_arc) = event_channel.read().clone();
    let engine: Option<crate::engine::EngineHandle> = props.engine.cloned();

    // Track scope changes: when scope_repo changes, invalidate all filters.
    let mut last_scope = hooks.use_state(|| scope_repo.clone());
    if *last_scope.read() != scope_repo {
        last_scope.set(scope_repo.clone());
        actions_state.set(ActionsState {
            filters: vec![FilterData::default(); filter_count],
        });
        filter_fetch_times.set(vec![None; filter_count]);
        filter_in_flight.set(vec![false; filter_count]);
        refresh_registered.set(false);
    }

    let eph_snapshot = ephemeral_filters.read().clone();
    let ephemeral_count = eph_snapshot.len();
    let total_tab_count = filter_count + ephemeral_count;
    let current_filter_idx = active_filter.get().min(total_tab_count.saturating_sub(1));
    let all_filters = merged_filters(filters_cfg, &eph_snapshot);

    let active_needs_fetch = actions_state
        .read()
        .filters
        .get(current_filter_idx)
        .is_some_and(|s| s.loading);
    let active_in_flight = filter_in_flight
        .read()
        .get(current_filter_idx)
        .copied()
        .unwrap_or(false);

    if !refresh_registered.get()
        && let Some(ref eng) = engine
    {
        // Resolve @current; pass the raw filter for unresolvable ones (engine fails
        // silently on "@current" — same as a typo, background refresh skips it).
        let resolved_for_refresh: Vec<ActionsFilter> = filters_cfg
            .iter()
            .map(|f| {
                if let Some(repo) =
                    resolve_filter_repo(&f.repo, scope_repo.as_deref(), detected_repo.as_deref())
                    && repo != f.repo.as_str()
                {
                    return ActionsFilter {
                        repo: repo.to_owned(),
                        ..f.clone()
                    };
                }
                f.clone()
            })
            .collect();
        eng.send(Request::RegisterActionsRefresh {
            filter_configs: resolved_for_refresh,
            notify_tx: event_tx.clone(),
        });
        refresh_registered.set(true);
    }

    if refresh_all.get()
        && is_active
        && let Some(ref eng) = engine
    {
        refresh_all.set(false);
        let mut in_flight = filter_in_flight.read().clone();
        for (filter_idx, (cfg, is_eph)) in all_filters.iter().enumerate() {
            let Some(resolved_repo) =
                resolve_filter_repo(&cfg.repo, scope_repo.as_deref(), detected_repo.as_deref())
            else {
                if !is_eph {
                    tracing::debug!(
                        "actions: skipping @current filter[{filter_idx}] — no scope repo"
                    );
                }
                continue;
            };
            let filter = if resolved_repo == cfg.repo.as_str() {
                (*cfg).clone()
            } else {
                ActionsFilter {
                    repo: resolved_repo.to_owned(),
                    ..(*cfg).clone()
                }
            };
            if filter_idx < in_flight.len() {
                in_flight[filter_idx] = true;
            }
            eng.send(Request::FetchActions {
                filter_idx,
                filter,
                reply_tx: event_tx.clone(),
            });
        }
        filter_in_flight.set(in_flight);
    } else if active_needs_fetch
        && !active_in_flight
        && is_active
        && let Some(ref eng) = engine
    {
        // Look up the active filter from the merged list (config + ephemeral).
        if let Some((cfg, _is_eph)) = all_filters.get(current_filter_idx) {
            if let Some(resolved_repo) =
                resolve_filter_repo(&cfg.repo, scope_repo.as_deref(), detected_repo.as_deref())
            {
                let filter = if resolved_repo == cfg.repo.as_str() {
                    (*cfg).clone()
                } else {
                    ActionsFilter {
                        repo: resolved_repo.to_owned(),
                        ..(*cfg).clone()
                    }
                };
                let mut in_flight = filter_in_flight.read().clone();
                if current_filter_idx < in_flight.len() {
                    in_flight[current_filter_idx] = true;
                }
                filter_in_flight.set(in_flight);
                eng.send(Request::FetchActions {
                    filter_idx: current_filter_idx,
                    filter,
                    reply_tx: event_tx.clone(),
                });
            } else {
                // `@current` filter with no scope resolved — clear loading to show empty tab.
                tracing::debug!(
                    "actions: @current filter[{current_filter_idx}] — no scope, clearing loading state"
                );
                let mut state = actions_state.read().clone();
                if current_filter_idx < state.filters.len() {
                    state.filters[current_filter_idx].loading = false;
                }
                actions_state.set(state);
            }
        }
    }

    // Poll engine events.
    {
        let rx_for_poll = event_rx_arc.clone();
        let theme_for_poll = theme.clone();
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
                        Event::ActionsFetched {
                            filter_idx,
                            runs,
                            rate_limit,
                        } => {
                            let rows: Vec<Row> = runs
                                .iter()
                                .map(|r| run_to_row(r, &theme_for_poll))
                                .collect();
                            let run_count = runs.len();
                            let filter_data = FilterData {
                                rows,
                                runs,
                                run_count,
                                loading: false,
                                error: None,
                            };
                            let mut state = actions_state.read().clone();
                            if filter_idx < state.filters.len() {
                                state.filters[filter_idx] = filter_data;
                            }
                            actions_state.set(state);
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
                            // Fresh run data arrived — evict job cache so the
                            // sidebar re-fetches updated job status rather than
                            // displaying stale results from the previous poll.
                            jobs_cache.set(HashMap::new());
                            jobs_in_flight.set(HashSet::new());
                            if let Some(rl) = rate_limit {
                                rate_limit_state.set(Some(rl));
                            }

                            // If this is an ephemeral tab with a pending run_id,
                            // check whether the target run is in the results.
                            if filter_idx >= filter_count {
                                let ei = filter_idx - filter_count;
                                let eph = ephemeral_filters.read().clone();
                                if let Some((eph_filter, Some(target_id))) = eph.get(ei) {
                                    let target_id = *target_id;
                                    let state = actions_state.read();
                                    let found_pos = state.filters.get(filter_idx).and_then(|fd| {
                                        fd.runs.iter().position(|r| r.id == target_id)
                                    });
                                    if let Some(pos) = found_pos {
                                        // Run is in the results — position cursor.
                                        cursor.set(pos);
                                        scroll_offset.set(pos.saturating_sub(5));
                                        detail_open.set(true);
                                        detail_scroll.set(0);
                                        // Clear pending.
                                        let mut eph_mut = eph;
                                        eph_mut[ei].1 = None;
                                        ephemeral_filters.set(eph_mut);
                                    } else if let Some((owner, repo)) = eph_filter
                                        .repo
                                        .split_once('/')
                                        .map(|(o, r)| (o.to_owned(), r.to_owned()))
                                    {
                                        // Run not in results — signal for
                                        // FetchRunById (needs engine access).
                                        pending_run_fetch.set(Some(PendingRunFetch {
                                            owner,
                                            repo,
                                            run_id: target_id,
                                            host: eph_filter.host.clone(),
                                        }));
                                    }
                                }
                            }
                        }
                        Event::RunJobsFetched {
                            run_id,
                            jobs,
                            rate_limit,
                        } => {
                            let mut cache = jobs_cache.read().clone();
                            cache.insert(run_id, jobs);
                            jobs_cache.set(cache);
                            let mut ifl = jobs_in_flight.read().clone();
                            ifl.remove(&run_id);
                            jobs_in_flight.set(ifl);
                            if let Some(rl) = rate_limit {
                                rate_limit_state.set(Some(rl));
                            }
                        }
                        Event::MutationOk { description } => {
                            action_status.set(Some(format!(
                                "{} {description}",
                                theme_for_poll.icons.feedback_ok
                            )));
                            // Refetch current filter.
                            let mut state = actions_state.read().clone();
                            if current_filter_for_poll < state.filters.len() {
                                state.filters[current_filter_for_poll] = FilterData::default();
                            }
                            actions_state.set(state);
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
                            action_status.set(Some(format!(
                                "{} {description}: {message}",
                                theme_for_poll.icons.feedback_error
                            )));
                        }
                        Event::RateLimitUpdated { info } => {
                            rate_limit_state.set(Some(info));
                        }
                        Event::SingleRunFetched {
                            run_id,
                            run,
                            rate_limit,
                        } => {
                            if let Some(fetched) = run {
                                // Insert the fetched run into the ephemeral tab that
                                // requested it (matched by pending run_id).
                                let eph = ephemeral_filters.read().clone();
                                let eph_idx = eph.iter().position(|(_, pending)| {
                                    pending.is_some_and(|id| id == run_id)
                                });
                                if let Some(ei) = eph_idx {
                                    let tab_idx = filter_count + ei;
                                    let mut state = actions_state.read().clone();
                                    if let Some(fd) = state.filters.get_mut(tab_idx) {
                                        // Prepend if not already present.
                                        if !fd.runs.iter().any(|r| r.id == fetched.id) {
                                            fd.rows
                                                .insert(0, run_to_row(&fetched, &theme_for_poll));
                                            fd.runs.insert(0, fetched);
                                            fd.run_count = fd.runs.len();
                                        }
                                    }
                                    actions_state.set(state);
                                    // Clear the pending run_id.
                                    let mut eph_mut = eph;
                                    eph_mut[ei].1 = None;
                                    ephemeral_filters.set(eph_mut);
                                    // Position cursor on the run.
                                    cursor.set(0);
                                    scroll_offset.set(0);
                                    detail_open.set(true);
                                    detail_scroll.set(0);
                                }
                            } else {
                                action_status.set(Some(format!(
                                    "Run #{run_id} not found (deleted or inaccessible)"
                                )));
                            }
                            if let Some(rl) = rate_limit {
                                rate_limit_state.set(Some(rl));
                            }
                        }
                        Event::FetchError { message, .. } => {
                            let ifl = filter_in_flight.read().clone();
                            let fi = ifl.iter().position(|&f| f);
                            if let Some(fi) = fi {
                                let mut state = actions_state.read().clone();
                                if fi < state.filters.len() {
                                    state.filters[fi] = FilterData {
                                        loading: false,
                                        error: Some(message.clone()),
                                        ..FilterData::default()
                                    };
                                }
                                actions_state.set(state);
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
                        _ => {}
                    }
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Deferred FetchRunById: the polling future detected that an ephemeral
    // tab's data arrived without the target run — fetch it individually.
    // -----------------------------------------------------------------------
    let pending_fetch = pending_run_fetch.read().clone();
    if let Some(prf) = pending_fetch {
        pending_run_fetch.set(None);
        tracing::debug!(
            "deep-link: target run {} not in bulk fetch for {}/{}, fetching individually",
            prf.run_id,
            prf.owner,
            prf.repo,
        );
        if let Some(ref eng) = engine {
            eng.send(Request::FetchRunById {
                owner: prf.owner,
                repo: prf.repo,
                run_id: prf.run_id,
                host: prf.host,
                reply_tx: event_tx.clone(),
            });
        }
    }

    // -----------------------------------------------------------------------
    // Process cross-view navigation target (deep-link from PR checks)
    // -----------------------------------------------------------------------

    let nav_target_prop = props.nav_target;
    let go_back_prop = props.go_back;

    if is_active && let Some(ref nt_state) = nav_target_prop {
        let target = nt_state.read().clone();
        if let Some(NavigationTarget::ActionsRun {
            ref owner,
            ref repo,
            run_id,
            ref host,
        }) = target
        {
            // 1. Search all loaded filter data (config + ephemeral) for this run.
            let found = {
                let state = actions_state.read();
                state.filters.iter().enumerate().find_map(|(fi, fd)| {
                    if fd.loading {
                        return None;
                    }
                    fd.runs
                        .iter()
                        .position(|r| r.id == run_id)
                        .map(|pos| (fi, pos))
                })
            };

            if let Some((filter_idx, run_pos)) = found {
                // Run found in an existing tab — switch to it.
                active_filter.set(filter_idx);
                cursor.set(run_pos);
                scroll_offset.set(run_pos.saturating_sub(5));
                detail_open.set(true);
                detail_scroll.set(0);
                search_query.set(String::new());
                nav_cursor.set(0);
                nav_focused.set(false);
                if let Some(mut nt) = nav_target_prop {
                    nt.set(None);
                }
            } else {
                // 2. Check if any filter is still loading — wait.
                let any_in_flight = filter_in_flight.read().iter().any(|&f| f);
                if !any_in_flight {
                    // 3. All loaded, run not found.
                    let full_repo = format!("{owner}/{repo}");

                    // Check if an ephemeral tab for this repo already exists.
                    let existing_eph = {
                        let eph = ephemeral_filters.read();
                        eph.iter()
                            .position(|(f, _)| f.repo == full_repo)
                            .map(|ei| filter_count + ei)
                    };

                    if let Some(tab_idx) = existing_eph {
                        // Ephemeral tab exists — switch to it. If the run isn't
                        // there, fetch it by ID into this tab.
                        active_filter.set(tab_idx);
                        cursor.set(0);
                        scroll_offset.set(0);
                        search_query.set(String::new());
                        nav_cursor.set(0);
                        nav_focused.set(false);

                        let run_in_tab = {
                            let state = actions_state.read();
                            state
                                .filters
                                .get(tab_idx)
                                .is_some_and(|fd| fd.runs.iter().any(|r| r.id == run_id))
                        };
                        if run_in_tab {
                            let pos = {
                                let state = actions_state.read();
                                state.filters[tab_idx]
                                    .runs
                                    .iter()
                                    .position(|r| r.id == run_id)
                                    .unwrap_or(0)
                            };
                            cursor.set(pos);
                            scroll_offset.set(pos.saturating_sub(5));
                            detail_open.set(true);
                            detail_scroll.set(0);
                        } else {
                            // Mark pending run_id on this ephemeral entry.
                            let ei = tab_idx - filter_count;
                            let mut eph = ephemeral_filters.read().clone();
                            eph[ei].1 = Some(run_id);
                            ephemeral_filters.set(eph);
                            if let Some(ref eng) = engine {
                                eng.send(Request::FetchRunById {
                                    owner: owner.clone(),
                                    repo: repo.clone(),
                                    run_id,
                                    host: host.clone(),
                                    reply_tx: event_tx.clone(),
                                });
                            }
                        }
                    } else if ephemeral_filters.read().len() < MAX_EPHEMERAL_TABS {
                        // Create new ephemeral tab.
                        tracing::debug!(
                            "deep-link: creating ephemeral actions tab for {full_repo}, run_id={run_id}"
                        );
                        let new_filter = ActionsFilter {
                            title: full_repo.clone(),
                            repo: full_repo,
                            host: host.clone(),
                            limit: None,
                            status: None,
                            event: None,
                        };
                        let mut eph = ephemeral_filters.read().clone();
                        eph.push((new_filter, Some(run_id)));
                        let new_tab_idx = filter_count + eph.len() - 1;
                        ephemeral_filters.set(eph);

                        // Grow state vectors for the new tab.
                        let mut state = actions_state.read().clone();
                        state.filters.push(FilterData::default());
                        actions_state.set(state);
                        let mut in_flight = filter_in_flight.read().clone();
                        in_flight.push(false);
                        filter_in_flight.set(in_flight);
                        let mut times = filter_fetch_times.read().clone();
                        times.push(None);
                        filter_fetch_times.set(times);

                        // Switch to the new tab and trigger fetch.
                        active_filter.set(new_tab_idx);
                        cursor.set(0);
                        scroll_offset.set(0);
                        search_query.set(String::new());
                        nav_cursor.set(0);
                        nav_focused.set(false);

                        // FetchActions will be triggered by the active_needs_fetch
                        // logic on the next render cycle (the new FilterData has
                        // loading: true by default).
                    } else {
                        action_status.set(Some(
                            "Too many ephemeral tabs \u{2014} close one first (d)".to_owned(),
                        ));
                    }

                    if let Some(mut nt) = nav_target_prop {
                        nt.set(None);
                    }
                }
                // If still loading, wait for next render cycle.
            }
        }
    }

    // -----------------------------------------------------------------------
    // Compute render + keyboard-handler data
    // -----------------------------------------------------------------------

    let state_ref = actions_state.read();
    let current_data = state_ref.filters.get(current_filter_idx);

    let workflow_names: Vec<String> = {
        let mut names: Vec<String> = current_data
            .map(|d| {
                d.runs
                    .iter()
                    .map(|r| r.name.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        names.insert(0, "All".to_owned());
        names
    };
    let nav_names_len = workflow_names.len();
    let nav_cur = nav_cursor.get().min(nav_names_len.saturating_sub(1));

    let search_q = search_query.read().clone();
    let all_rows: &[Row] = current_data.map_or(&[], |d| d.rows.as_slice());
    let all_runs: &[WorkflowRun] = current_data.map_or(&[], |d| d.runs.as_slice());

    // Apply search filter.
    let after_search_idx: Vec<usize> = if search_q.is_empty() {
        (0..all_rows.len()).collect()
    } else {
        let q_lower = search_q.to_lowercase();
        (0..all_rows.len())
            .filter(|&i| {
                all_rows[i]
                    .get("workflow")
                    .is_some_and(|c| c.text().to_lowercase().contains(&q_lower))
                    || all_rows[i]
                        .get("title")
                        .is_some_and(|c| c.text().to_lowercase().contains(&q_lower))
            })
            .collect()
    };

    // Apply workflow nav filter.
    let filtered_run_indices: Vec<usize> = if nav_cur == 0 {
        after_search_idx
    } else {
        let nav_name = workflow_names.get(nav_cur).map_or("", String::as_str);
        after_search_idx
            .into_iter()
            .filter(|&i| {
                all_rows[i]
                    .get("workflow")
                    .is_some_and(|c| c.text() == nav_name)
            })
            .collect()
    };

    let filtered_rows: Vec<Row> = filtered_run_indices
        .iter()
        .filter_map(|&i| all_rows.get(i))
        .cloned()
        .collect();

    let total_rows = filtered_rows.len();
    let visible_rows = (props.height.saturating_sub(5) / 2).max(1) as usize;

    // Clone for keyboard handler capture.
    let filtered_run_indices_for_kb = filtered_run_indices.clone();
    // Resolve current filter from the merged list (config + ephemeral).
    let current_filter_cfg_for_kb: Option<ActionsFilter> = all_filters
        .get(current_filter_idx)
        .map(|(f, _)| (*f).clone());

    // Trigger job fetch when detail sidebar is open.
    let cur_run_for_fetch: Option<WorkflowRun> = filtered_run_indices
        .get(cursor.get())
        .and_then(|&orig_idx| current_data.and_then(|d| d.runs.get(orig_idx)))
        .cloned();

    if detail_open.get()
        && is_active
        && let Some(ref cur_run) = cur_run_for_fetch
    {
        let run_id = cur_run.id;
        if !jobs_in_flight.read().contains(&run_id)
            && !jobs_cache.read().contains_key(&run_id)
            && let Some(ref eng) = engine
            && let Some((owner, repo)) =
                owner_repo_for_run(cur_run, current_filter_cfg_for_kb.as_ref())
        {
            let host = current_filter_cfg_for_kb
                .as_ref()
                .and_then(|f| f.host.clone());
            eng.send(Request::FetchRunJobs {
                owner,
                repo,
                run_id,
                host,
                reply_tx: event_tx.clone(),
            });
            let mut ifl = jobs_in_flight.read().clone();
            ifl.insert(run_id);
            jobs_in_flight.set(ifl);
        }
    }

    // -----------------------------------------------------------------------
    // Keyboard handling
    // -----------------------------------------------------------------------

    let keybindings = props.keybindings.cloned();
    hooks.use_terminal_events({
        let engine_for_keys = engine.clone();
        let event_tx_for_keys = event_tx.clone();
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code,
                kind,
                modifiers,
                ..
            }) if kind != KeyEventKind::Release => {
                if !is_active {
                    return;
                }

                // Help overlay intercepts all keys.
                if help_visible.get() {
                    if matches!(code, KeyCode::Char('?') | KeyCode::Esc) {
                        help_visible.set(false);
                    }
                    return;
                }

                let current_mode = input_mode.read().clone();
                match current_mode {
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
                    InputMode::Confirm(ref pending) => match code {
                        KeyCode::Char('y' | 'Y') => {
                            match pending {
                                BuiltinAction::RerunFailed => {
                                    send_rerun(
                                        &actions_state,
                                        current_filter_idx,
                                        cursor.get(),
                                        &filtered_run_indices_for_kb,
                                        current_filter_cfg_for_kb.as_ref(),
                                        true,
                                        engine_for_keys.as_ref(),
                                        &event_tx_for_keys,
                                    );
                                }
                                BuiltinAction::RerunAll => {
                                    send_rerun(
                                        &actions_state,
                                        current_filter_idx,
                                        cursor.get(),
                                        &filtered_run_indices_for_kb,
                                        current_filter_cfg_for_kb.as_ref(),
                                        false,
                                        engine_for_keys.as_ref(),
                                        &event_tx_for_keys,
                                    );
                                }
                                BuiltinAction::CancelRun => {
                                    send_cancel(
                                        &actions_state,
                                        current_filter_idx,
                                        cursor.get(),
                                        &filtered_run_indices_for_kb,
                                        current_filter_cfg_for_kb.as_ref(),
                                        engine_for_keys.as_ref(),
                                        &event_tx_for_keys,
                                    );
                                }
                                _ => {}
                            }
                            input_mode.set(InputMode::Normal);
                        }
                        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            action_status.set(Some("Cancelled".to_owned()));
                        }
                        _ => {}
                    },
                    InputMode::Normal => {
                        // Nav panel focused: route j/k/Enter/Esc to navigator.
                        if nav_focused.get() {
                            match code {
                                KeyCode::Char('j') | KeyCode::Down => {
                                    nav_cursor.set(
                                        (nav_cursor.get() + 1).min(nav_names_len.saturating_sub(1)),
                                    );
                                    return;
                                }
                                KeyCode::Char('k') | KeyCode::Up => {
                                    nav_cursor.set(nav_cursor.get().saturating_sub(1));
                                    return;
                                }
                                KeyCode::Enter => {
                                    nav_focused.set(false);
                                    cursor.set(0);
                                    scroll_offset.set(0);
                                    return;
                                }
                                KeyCode::Esc => {
                                    nav_focused.set(false);
                                    nav_open.set(false);
                                    return;
                                }
                                _ => {} // fall through to normal handling
                            }
                        }

                        if let Some(key_str) = key_event_to_string(code, modifiers, kind) {
                            let current_run = get_run_at_cursor(
                                &actions_state,
                                current_filter_idx,
                                cursor.get(),
                                &filtered_run_indices_for_kb,
                            );
                            let vars = TemplateVars {
                                url: current_run
                                    .as_ref()
                                    .map_or_else(String::new, |r| r.html_url.clone()),
                                number: current_run
                                    .as_ref()
                                    .map_or_else(String::new, |r| r.run_number.to_string()),
                                repo_name: current_filter_cfg_for_kb
                                    .as_ref()
                                    .map_or_else(String::new, |f| f.repo.clone()),
                                head_branch: current_run
                                    .as_ref()
                                    .and_then(|r| r.head_branch.clone())
                                    .unwrap_or_default(),
                                base_branch: String::new(),
                            };
                            match keybindings
                                .as_ref()
                                .and_then(|kb| kb.resolve(&key_str, ViewContext::Actions))
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
                                    BuiltinAction::ToggleHelp => {
                                        help_visible.set(true);
                                    }
                                    BuiltinAction::OpenBrowser => {
                                        if let Some(run) = current_run
                                            && !run.html_url.is_empty()
                                        {
                                            let _ = clipboard::open_in_browser(&run.html_url);
                                        }
                                    }
                                    BuiltinAction::CopyNumber => {
                                        if let Some(run) = current_run {
                                            let text = run.run_number.to_string();
                                            match clipboard::copy_to_clipboard(&text) {
                                                Ok(()) => action_status.set(Some(format!(
                                                    "Copied #{}",
                                                    run.run_number
                                                ))),
                                                Err(e) => action_status
                                                    .set(Some(format!("Copy failed: {e}"))),
                                            }
                                        }
                                    }
                                    BuiltinAction::CopyUrl => {
                                        if let Some(run) = current_run
                                            && !run.html_url.is_empty()
                                        {
                                            let _ = clipboard::copy_to_clipboard(&run.html_url);
                                        }
                                    }
                                    BuiltinAction::Refresh => {
                                        let idx = current_filter_idx;
                                        let mut state = actions_state.read().clone();
                                        if idx < state.filters.len() {
                                            state.filters[idx] = FilterData::default();
                                        }
                                        actions_state.set(state);
                                        let mut times = filter_fetch_times.read().clone();
                                        if idx < times.len() {
                                            times[idx] = None;
                                        }
                                        filter_fetch_times.set(times);
                                        // Clear job cache and in-flight set so the
                                        // sidebar detail re-fetches along with the
                                        // table (otherwise the cache hit prevents
                                        // a new FetchRunJobs from being sent).
                                        jobs_cache.set(HashMap::new());
                                        jobs_in_flight.set(HashSet::new());
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    BuiltinAction::RefreshAll => {
                                        let mut state = actions_state.read().clone();
                                        for filter in &mut state.filters {
                                            *filter = FilterData::default();
                                        }
                                        actions_state.set(state);
                                        let mut times = filter_fetch_times.read().clone();
                                        times.fill(None);
                                        filter_fetch_times.set(times);
                                        jobs_cache.set(HashMap::new());
                                        jobs_in_flight.set(HashSet::new());
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                        refresh_all.set(true);
                                    }
                                    BuiltinAction::ToggleWorkflowNav => {
                                        let new_open = !nav_open.get();
                                        nav_open.set(new_open);
                                        nav_focused.set(new_open);
                                    }
                                    BuiltinAction::TogglePreview => {
                                        let new_open = !detail_open.get();
                                        detail_open.set(new_open);
                                        if new_open {
                                            detail_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::RerunFailed => {
                                        if let Some(run) = get_run_at_cursor(
                                            &actions_state,
                                            current_filter_idx,
                                            cursor.get(),
                                            &filtered_run_indices_for_kb,
                                        ) {
                                            if run.status == RunStatus::Completed {
                                                input_mode.set(InputMode::Confirm(
                                                    BuiltinAction::RerunFailed,
                                                ));
                                                action_status.set(None);
                                            } else {
                                                action_status.set(Some(
                                                    "Cannot re-run: run is still in progress"
                                                        .to_owned(),
                                                ));
                                            }
                                        }
                                    }
                                    BuiltinAction::RerunAll => {
                                        if let Some(run) = get_run_at_cursor(
                                            &actions_state,
                                            current_filter_idx,
                                            cursor.get(),
                                            &filtered_run_indices_for_kb,
                                        ) {
                                            if run.status == RunStatus::Completed {
                                                input_mode.set(InputMode::Confirm(
                                                    BuiltinAction::RerunAll,
                                                ));
                                                action_status.set(None);
                                            } else {
                                                action_status.set(Some(
                                                    "Cannot re-run: run is still in progress"
                                                        .to_owned(),
                                                ));
                                            }
                                        }
                                    }
                                    BuiltinAction::CancelRun => {
                                        if let Some(run) = get_run_at_cursor(
                                            &actions_state,
                                            current_filter_idx,
                                            cursor.get(),
                                            &filtered_run_indices_for_kb,
                                        ) {
                                            if matches!(
                                                run.status,
                                                RunStatus::Queued | RunStatus::InProgress
                                            ) {
                                                input_mode.set(InputMode::Confirm(
                                                    BuiltinAction::CancelRun,
                                                ));
                                                action_status.set(None);
                                            } else {
                                                action_status.set(Some(
                                                    "Cannot cancel: run is not in progress"
                                                        .to_owned(),
                                                ));
                                            }
                                        }
                                    }
                                    BuiltinAction::GoBack => {
                                        detail_open.set(false);
                                        if let Some(mut gb) = go_back_prop {
                                            gb.set(true);
                                        }
                                    }
                                    BuiltinAction::Search => {
                                        input_mode.set(InputMode::Search);
                                        search_query.set(String::new());
                                    }
                                    BuiltinAction::MoveDown if total_rows > 0 => {
                                        let new_cursor =
                                            (cursor.get() + 1).min(total_rows.saturating_sub(1));
                                        cursor.set(new_cursor);
                                        if new_cursor >= scroll_offset.get() + visible_rows {
                                            scroll_offset
                                                .set(new_cursor.saturating_sub(visible_rows) + 1);
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
                                    BuiltinAction::Last if total_rows > 0 => {
                                        cursor.set(total_rows.saturating_sub(1));
                                        scroll_offset.set(total_rows.saturating_sub(visible_rows));
                                    }
                                    BuiltinAction::PageDown if total_rows > 0 => {
                                        let new_cursor = (cursor.get() + visible_rows)
                                            .min(total_rows.saturating_sub(1));
                                        cursor.set(new_cursor);
                                        scroll_offset.set(
                                            new_cursor
                                                .saturating_sub(visible_rows.saturating_sub(1)),
                                        );
                                    }
                                    BuiltinAction::PageUp => {
                                        let new_cursor = cursor.get().saturating_sub(visible_rows);
                                        cursor.set(new_cursor);
                                        scroll_offset
                                            .set(scroll_offset.get().saturating_sub(visible_rows));
                                    }
                                    BuiltinAction::HalfPageDown => {
                                        let half = visible_rows / 2;
                                        if detail_open.get() {
                                            detail_scroll.set(detail_scroll.get() + half);
                                        } else if total_rows > 0 {
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
                                        if detail_open.get() {
                                            detail_scroll
                                                .set(detail_scroll.get().saturating_sub(half));
                                        } else {
                                            let new_cursor = cursor.get().saturating_sub(half);
                                            cursor.set(new_cursor);
                                            if new_cursor < scroll_offset.get() {
                                                scroll_offset.set(new_cursor);
                                            }
                                        }
                                    }
                                    BuiltinAction::PrevFilter if total_tab_count > 0 => {
                                        let current = active_filter.get();
                                        active_filter.set(if current == 0 {
                                            total_tab_count.saturating_sub(1)
                                        } else {
                                            current - 1
                                        });
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    BuiltinAction::NextFilter if total_tab_count > 0 => {
                                        active_filter
                                            .set((active_filter.get() + 1) % total_tab_count);
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    BuiltinAction::CloseTab => {
                                        if current_filter_idx >= filter_count {
                                            // Ephemeral tab — remove it.
                                            let ei = current_filter_idx - filter_count;
                                            let mut eph = ephemeral_filters.read().clone();
                                            debug_assert!(
                                                ei < eph.len(),
                                                "ephemeral index out of range"
                                            );
                                            eph.remove(ei);
                                            let new_total = filter_count + eph.len();
                                            ephemeral_filters.set(eph);

                                            // Remove from state vectors.
                                            let mut state = actions_state.read().clone();
                                            if current_filter_idx < state.filters.len() {
                                                state.filters.remove(current_filter_idx);
                                            }
                                            actions_state.set(state);
                                            let mut in_flight = filter_in_flight.read().clone();
                                            if current_filter_idx < in_flight.len() {
                                                in_flight.remove(current_filter_idx);
                                            }
                                            filter_in_flight.set(in_flight);
                                            let mut times = filter_fetch_times.read().clone();
                                            if current_filter_idx < times.len() {
                                                times.remove(current_filter_idx);
                                            }
                                            filter_fetch_times.set(times);

                                            // Clamp active filter.
                                            if active_filter.get() >= new_total && new_total > 0 {
                                                active_filter.set(new_total - 1);
                                            }
                                            cursor.set(0);
                                            scroll_offset.set(0);
                                        } else {
                                            action_status
                                                .set(Some("Cannot close config tabs".to_owned()));
                                        }
                                    }
                                    _ => {}
                                },
                                Some(ResolvedBinding::ShellCommand(cmd)) => {
                                    let expanded = expand_template(&cmd, &vars);
                                    let _ = execute_shell_command(&expanded);
                                }
                                None => {
                                    // Esc: close nav → close detail
                                    if key_str == "esc" {
                                        if nav_open.get() {
                                            nav_focused.set(false);
                                            nav_open.set(false);
                                        } else if detail_open.get() {
                                            detail_open.set(false);
                                        }
                                    } else if key_str == "[" {
                                        if detail_open.get() {
                                            detail_scroll
                                                .set(detail_scroll.get().saturating_sub(1));
                                        }
                                    } else if key_str == "]" && detail_open.get() {
                                        detail_scroll.set(detail_scroll.get() + 1);
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

    // Skip heavy rendering for inactive views.
    if !is_active {
        return element! {
            View(flex_direction: FlexDirection::Column)
        }
        .into_any();
    }

    // -----------------------------------------------------------------------
    // Width layout (three-pane)
    // -----------------------------------------------------------------------

    let nav_w: u16 = if nav_open.get() { NAV_W } else { 0 };
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (table_w, sidebar_w) = if detail_open.get() {
        let sb = (f64::from(props.width) * preview_pct).round() as u16;
        let tb = props.width.saturating_sub(nav_w).saturating_sub(sb);
        (tb, sb)
    } else {
        (props.width.saturating_sub(nav_w), 0u16)
    };

    // -----------------------------------------------------------------------
    // Build rendered components
    // -----------------------------------------------------------------------

    let tabs: Vec<Tab> = all_filters
        .iter()
        .enumerate()
        .map(|(i, (f, is_eph))| Tab {
            title: f.title.clone(),
            count: state_ref.filters.get(i).map(|d| d.run_count),
            is_ephemeral: *is_eph,
        })
        .collect();

    let columns = actions_columns();
    let rendered_table = RenderedTable::build(&TableBuildConfig {
        columns: &columns,
        rows: &filtered_rows,
        cursor: cursor.get(),
        scroll_offset: scroll_offset.get(),
        visible_rows,
        hidden_columns: None,
        width_overrides: None,
        total_width: table_w,
        depth,
        selected_bg: Some(theme.bg_selected),
        header_color: Some(theme.text_secondary),
        border_color: Some(theme.border_faint),
        show_separator: props.show_separator,
        empty_message: if search_q.is_empty() {
            Some("No workflow runs found")
        } else {
            Some("No runs match this filter")
        },
        subtitle_column: None,
        row_separator: true,
        scrollbar_thumb_color: Some(theme.border_primary),
    });

    let rendered_tab_bar = RenderedTabBar::build(
        &tabs,
        current_filter_idx,
        props.show_filter_count,
        depth,
        Some(theme.footer_actions),
        Some(theme.footer_actions),
        Some(theme.border_faint),
        &theme.icons.tab_filter,
        &theme.icons.tab_ephemeral,
    );

    let current_mode = input_mode.read().clone();
    let rendered_text_input = match current_mode {
        InputMode::Confirm(ref action) => {
            let prompt = match action {
                BuiltinAction::RerunFailed => "Re-run failed jobs? (y/n)",
                BuiltinAction::RerunAll => "Re-run ALL jobs? (y/n)",
                BuiltinAction::CancelRun => "Cancel this run? (y/n)",
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

    // Context text: action status > normal.
    let context_text = if let Some(ref status) = *action_status.read() {
        status.clone()
    } else if current_data.is_some_and(|d| d.loading) {
        "Fetching workflow runs\u{2026}".to_owned()
    } else if let Some(err) = current_data.and_then(|d| d.error.as_ref()) {
        format!("Error: {err}")
    } else {
        let total = current_data.map_or(0, |d| d.run_count);
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        format!("Run {cursor_pos}/{total_rows} (of {total})")
    };

    let active_fetch_time = filter_fetch_times
        .read()
        .get(current_filter_idx)
        .copied()
        .flatten();
    let updated_text = footer::format_updated_ago(active_fetch_time);
    let rate_limit_text = footer::format_rate_limit(rate_limit_state.read().as_ref());
    let scope_label = match &scope_repo {
        Some(_) => all_filters
            .get(current_filter_idx)
            .map_or_else(String::new, |(f, _)| {
                resolve_filter_repo(&f.repo, scope_repo.as_deref(), detected_repo.as_deref())
                    .unwrap_or("all repos")
                    .to_owned()
            }),
        None => "all repos".to_owned(),
    };

    let rendered_footer = RenderedFooter::build(
        ViewKind::Actions,
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
                context: ViewContext::Actions,
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

    // Right sidebar: jobs detail.
    let current_run_for_detail = filtered_run_indices
        .get(cursor.get())
        .and_then(|&idx| all_runs.get(idx));
    let sidebar_run_id = current_run_for_detail.map(|r| r.id);
    let sidebar_loading = sidebar_run_id.is_some_and(|id| jobs_in_flight.read().contains(&id));
    let sidebar_jobs = sidebar_run_id
        .and_then(|id| jobs_cache.read().get(&id).cloned())
        .unwrap_or_default();
    let sidebar_visible_lines = props.height.saturating_sub(6) as usize;
    let rendered_sidebar = if detail_open.get() && sidebar_w > 0 {
        let jobs_lines = build_jobs_lines(&sidebar_jobs, sidebar_loading, &theme);
        let sidebar_title = current_run_for_detail
            .map_or_else(|| "Jobs".to_owned(), |r| format!("Run #{}", r.run_number));
        let sidebar = RenderedSidebar::build(
            &sidebar_title,
            &jobs_lines,
            detail_scroll.get(),
            sidebar_visible_lines,
            sidebar_w,
            depth,
            Some(theme.text_primary),
            Some(theme.border_faint),
            Some(theme.text_faint),
            Some(theme.border_primary),
        );
        if detail_scroll.get() != sidebar.clamped_scroll {
            detail_scroll.set(sidebar.clamped_scroll);
        }
        Some(sidebar)
    } else {
        None
    };

    let nav_is_open = nav_open.get();
    let nav_is_focused = nav_focused.get();
    let nav_border_color = if nav_is_focused {
        theme.border_primary.to_crossterm_color(depth)
    } else {
        theme.border_faint.to_crossterm_color(depth)
    };

    let width = u32::from(props.width);
    let height = u32::from(props.height);

    element! {
        View(flex_direction: FlexDirection::Column, width, height) {
            TabBar(tab_bar: rendered_tab_bar)

            View(flex_grow: 1.0, flex_direction: FlexDirection::Row, overflow: Overflow::Hidden) {
                // Left workflow navigator (optional)
                #(nav_is_open.then(|| {
                    let names = workflow_names.clone();
                    let cur = nav_cur;
                    let theme_nav = theme.clone();
                    element! {
                        View(
                            width: u32::from(NAV_W),
                            flex_direction: FlexDirection::Column,
                            border_style: BorderStyle::Single,
                            border_edges: Edges::Right,
                            border_color: nav_border_color,
                            padding_left: 1u32,
                        ) {
                            View(
                                border_style: BorderStyle::Single,
                                border_edges: Edges::Bottom,
                                border_color: theme_nav.border_faint.to_crossterm_color(depth),
                            ) {
                                Text(
                                    content: "Workflows",
                                    color: theme_nav.text_primary.to_crossterm_color(depth),
                                    weight: Weight::Bold,
                                    wrap: TextWrap::NoWrap,
                                )
                            }
                            #(names.into_iter().enumerate().map(|(i, name)| {
                                let is_selected = i == cur;
                                let (dot, dot_color) = if i == 0 {
                                    ("\u{25a1}".to_owned(), theme_nav.text_faint)
                                } else {
                                    let most_recent = all_runs.iter().find(|r| r.name == name);
                                    most_recent.map_or_else(
                                        || (" ".to_owned(), theme_nav.text_faint),
                                        |r| run_status_icon_color(r.status, r.conclusion, &theme_nav),
                                    )
                                };
                                let text_color = if is_selected {
                                    theme_nav.text_primary
                                } else {
                                    theme_nav.text_secondary
                                };
                                let bg = if is_selected {
                                    theme_nav.bg_selected.to_crossterm_color(depth)
                                } else {
                                    Color::Reset
                                };
                                let max_len = (NAV_W as usize).saturating_sub(4);
                                let display = if name.chars().count() > max_len {
                                    let end =
                                        name.char_indices()
                                            .nth(max_len.saturating_sub(1))
                                            .map_or(name.len(), |(i, _)| i);
                                    format!("{}\u{2026}", &name[..end])
                                } else {
                                    name.clone()
                                };
                                element! {
                                    View(key: i, flex_direction: FlexDirection::Row, background_color: bg) {
                                        Text(content: dot, color: dot_color.to_crossterm_color(depth), wrap: TextWrap::NoWrap)
                                        Text(content: format!(" {display}"), color: text_color.to_crossterm_color(depth), wrap: TextWrap::NoWrap)
                                    }
                                }.into_any()
                            }))
                        }
                    }.into_any()
                }))

                // Main table
                View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                    ScrollableTable(table: rendered_table)
                }

                // Right sidebar
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
// Key lookup helpers (used in keyboard handler)
// ---------------------------------------------------------------------------

fn get_run_at_cursor(
    actions_state: &State<ActionsState>,
    filter_idx: usize,
    cursor: usize,
    run_indices: &[usize],
) -> Option<WorkflowRun> {
    let orig_idx = *run_indices.get(cursor)?;
    let state = actions_state.read();
    state.filters.get(filter_idx)?.runs.get(orig_idx).cloned()
}

#[allow(clippy::too_many_arguments)]
fn send_rerun(
    actions_state: &State<ActionsState>,
    filter_idx: usize,
    cursor: usize,
    run_indices: &[usize],
    filter: Option<&ActionsFilter>,
    failed_only: bool,
    engine: Option<&EngineHandle>,
    reply_tx: &std::sync::mpsc::Sender<Event>,
) {
    let Some(run) = get_run_at_cursor(actions_state, filter_idx, cursor, run_indices) else {
        return;
    };
    let Some((owner, repo)) = owner_repo_for_run(&run, filter) else {
        return;
    };
    let Some(eng) = engine else { return };
    eng.send(Request::RerunWorkflowRun {
        owner,
        repo,
        run_id: run.id,
        failed_only,
        reply_tx: reply_tx.clone(),
    });
}

#[allow(clippy::too_many_arguments)]
fn send_cancel(
    actions_state: &State<ActionsState>,
    filter_idx: usize,
    cursor: usize,
    run_indices: &[usize],
    filter: Option<&ActionsFilter>,
    engine: Option<&EngineHandle>,
    reply_tx: &std::sync::mpsc::Sender<Event>,
) {
    let Some(run) = get_run_at_cursor(actions_state, filter_idx, cursor, run_indices) else {
        return;
    };
    let Some((owner, repo)) = owner_repo_for_run(&run, filter) else {
        return;
    };
    let Some(eng) = engine else { return };
    eng.send(Request::CancelWorkflowRun {
        owner,
        repo,
        run_id: run.id,
        reply_tx: reply_tx.clone(),
    });
}

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}
