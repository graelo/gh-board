use std::collections::{HashMap, HashSet};

use iocraft::prelude::*;

use crate::actions::clipboard;
use crate::app::{NavigationTarget, ViewKind};
use crate::color::{Color as AppColor, ColorDepth};
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::selection_overlay::{
    RenderedSelectionOverlay, SelectionOverlay, SelectionOverlayBuildConfig, SelectionOverlayItem,
};
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
use crate::config::types::PrFilter;
use crate::engine::{EngineHandle, Event, PrRef, Request};
use crate::filter::{self, apply_scope};
use crate::icons::ResolvedIcons;
use crate::markdown::renderer::{self, StyledLine};
use crate::theme::ResolvedTheme;
use crate::types::{
    AuthorAssociation, BranchUpdateStatus, MergeStateStatus, MergeableState, PrDetail, PullRequest,
    RateLimitInfo,
};
use crate::views::MAX_EPHEMERAL_TABS;

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
            fixed_width: Some(4),
        },
        Column {
            id: "review".to_owned(),
            header: icons.header_review.clone(),
            default_width_pct: 0.04,
            align: TextAlign::Center,
            fixed_width: Some(4),
        },
        Column {
            id: "ci".to_owned(),
            header: icons.header_ci.clone(),
            default_width_pct: 0.04,
            align: TextAlign::Center,
            fixed_width: Some(4),
        },
        Column {
            id: "update".to_owned(),
            header: icons.header_update.clone(),
            default_width_pct: 0.04,
            align: TextAlign::Center,
            fixed_width: Some(4),
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
///
/// When `detail` is provided the "update" cell is derived from the refined detail
/// data; otherwise the coarse `merge_state_status` from the PR itself is used.
#[allow(clippy::too_many_lines)]
fn pr_to_row(
    pr: &PullRequest,
    theme: &ResolvedTheme,
    date_format: &str,
    detail: Option<&PrDetail>,
) -> Row {
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
        Cell::colored(crate::util::expand_emoji(&pr.title), theme.text_primary),
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

    // Review status: prefer reviewDecision, fall back to latestReviews.
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
        None => {
            // Infer from latestReviews when reviewDecision is null
            // (repos without required review branch protection).
            use crate::types::ReviewState;
            if pr
                .reviews
                .iter()
                .any(|r| r.state == ReviewState::ChangesRequested)
            {
                (&icons.review_changes, theme.text_warning)
            } else if pr.reviews.iter().any(|r| r.state == ReviewState::Approved) {
                (&icons.review_approved, theme.text_success)
            } else if pr.reviews.iter().any(|r| r.state == ReviewState::Commented) {
                (&icons.review_commented, theme.text_secondary)
            } else {
                (&icons.review_none, theme.text_faint)
            }
        }
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

    // Update status (refined from detail if available, coarse from PR otherwise).
    // Skip for closed/merged PRs — branch status is irrelevant once the PR is done.
    let update = if matches!(
        pr.state,
        crate::github::types::PrState::Closed | crate::github::types::PrState::Merged
    ) {
        Cell::colored("-".to_owned(), theme.text_faint)
    } else if let Some(d) = detail {
        update_cell_from_detail(d, theme)
    } else {
        update_cell(branch_update_status(pr), theme)
    };
    row.insert("update".to_owned(), update);

    row
}

/// Aggregate CI check runs into a single status icon.
fn aggregate_ci_status(
    checks: &[crate::github::types::CheckRun],
    theme: &ResolvedTheme,
) -> (String, AppColor) {
    use crate::types::{CheckConclusion, CheckStatus};

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

/// Derive a coarse `BranchUpdateStatus` from the PR's `merge_state_status`.
///
/// Only definitive *negative* signals (BEHIND, DIRTY) are surfaced from the
/// search query. Everything else stays Unknown/blank until the detail fetch
/// provides `behind_by` / `mergeable`.  In particular, CLEAN must not be
/// mapped to `UpToDate` here: GitHub returns CLEAN for branches that are hundreds
/// of commits behind when the repo does not require up-to-date branches before
/// merging.  The authoritative ✓ comes only from `effective_update_status`.
fn branch_update_status(pr: &PullRequest) -> BranchUpdateStatus {
    match pr.merge_state_status {
        Some(MergeStateStatus::Behind) => BranchUpdateStatus::NeedsUpdate,
        Some(MergeStateStatus::Dirty) => BranchUpdateStatus::HasConflicts,
        _ => BranchUpdateStatus::Unknown,
    }
}

/// Derive a refined `MergeStateStatus` from full detail data.
///
/// Returns `None` when no definitive status is available (e.g. `behind_by` is
/// unknown and `mergeable` is not `Conflicting`).
fn effective_update_status(detail: &PrDetail) -> Option<MergeStateStatus> {
    if matches!(detail.mergeable, Some(MergeableState::Conflicting)) {
        return Some(MergeStateStatus::Dirty);
    }
    match detail.behind_by {
        Some(0) => Some(MergeStateStatus::Clean),
        Some(_) => Some(MergeStateStatus::Behind),
        None => None,
    }
}

/// Build the "update" table cell from a coarse `BranchUpdateStatus`.
fn update_cell(status: BranchUpdateStatus, theme: &ResolvedTheme) -> Cell {
    let icons = &theme.icons;
    match status {
        BranchUpdateStatus::NeedsUpdate => {
            Cell::colored(icons.update_needed.clone(), theme.text_warning)
        }
        BranchUpdateStatus::HasConflicts => {
            Cell::colored(icons.update_conflict.clone(), theme.text_error)
        }
        BranchUpdateStatus::Unknown => Cell::colored(String::new(), theme.text_faint),
    }
}

/// Build the "update" table cell from full detail data.
fn update_cell_from_detail(detail: &PrDetail, theme: &ResolvedTheme) -> Cell {
    let icons = &theme.icons;
    match effective_update_status(detail) {
        Some(MergeStateStatus::Dirty) => {
            Cell::colored(icons.update_conflict.clone(), theme.text_error)
        }
        Some(MergeStateStatus::Behind) => {
            Cell::colored(icons.update_needed.clone(), theme.text_warning)
        }
        Some(MergeStateStatus::Clean) => Cell::colored(icons.update_ok.clone(), theme.text_success),
        _ => Cell::colored(String::new(), theme.text_faint),
    }
}

// ---------------------------------------------------------------------------
// Detail request (debounce)
// ---------------------------------------------------------------------------

/// Parameters needed to fetch sidebar detail for a PR (including the compare call).
#[derive(Clone)]
struct DetailRequest {
    owner: String,
    repo: String,
    pr_number: u64,
    base_ref: String,
    head_repo_owner: Option<String>,
    head_ref: String,
}

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
    Confirm(BuiltinAction),
    /// Search/filter mode (T087).
    Search,
    /// Text input mode for assigning to any user.
    Assign,
    /// Text input mode for adding a label.
    Label,
    /// Prompt for which branch-update method to use (merge or rebase).
    UpdateBranchMethod,
}

// ---------------------------------------------------------------------------
// Filter state
// ---------------------------------------------------------------------------

/// State for a single filter.
#[derive(Debug, Clone)]
struct FilterData {
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

impl Default for FilterData {
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

/// Shared state across all filters (stored in a single State handle).
#[derive(Debug, Clone)]
struct PrsState {
    filters: Vec<FilterData>,
}

/// Build a merged list of (filter, `is_ephemeral`) from config + ephemeral filters.
fn merged_pr_filters<'a>(
    config: &'a [PrFilter],
    ephemeral: &'a [(PrFilter, Option<u64>)],
) -> Vec<(&'a PrFilter, bool)> {
    let mut out: Vec<_> = config.iter().map(|f| (f, false)).collect();
    out.extend(ephemeral.iter().map(|(f, _)| (f, true)));
    out
}

// ---------------------------------------------------------------------------
// PrsView component (T029-T033 + T040 preview pane + T061-T062 actions)
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct PrsViewProps<'a> {
    /// PR filter configs.
    pub filters: Option<&'a [PrFilter]>,
    /// Engine handle.
    pub engine: Option<&'a EngineHandle>,
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
    /// Whether filter counts are shown in tabs.
    pub show_filter_count: bool,
    /// Whether table separators are shown.
    pub show_separator: bool,
    /// Signal to exit the app.
    pub should_exit: Option<State<bool>>,
    /// Signal to switch to another view.
    pub switch_view: Option<State<bool>>,
    /// Signal to switch to the previous view.
    pub switch_view_back: Option<State<bool>>,
    /// Signal to toggle repo scope.
    pub scope_toggle: Option<State<bool>>,
    /// Active scope repo (e.g. `"owner/repo"`), or `None` for global.
    pub scope_repo: Option<String>,
    /// Repo paths for checkout (from `config.repo_paths`).
    pub repo_paths: Option<&'a HashMap<String, std::path::PathBuf>>,
    /// Date format string (from `config.defaults.date_format`).
    pub date_format: Option<&'a str>,
    /// Whether this view is the currently active (visible) one.
    pub is_active: bool,
    /// Auto-refetch interval in minutes (0 = disabled).
    pub refetch_interval_minutes: u32,
    /// Number of PR details to prefetch after list load. 0 = on-demand only.
    pub prefetch_pr_details: u32,
    /// Navigation target state — set by `JumpToRun` to trigger cross-view navigation.
    pub nav_target: Option<State<Option<NavigationTarget>>>,
    /// Go-back signal — set to true to return to previous view.
    pub go_back: Option<State<bool>>,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn PrsView<'a>(props: &PrsViewProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
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

    // State: active filter index, cursor, scroll offset.
    let mut active_filter = hooks.use_state(|| 0usize);
    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);

    // State: preview pane.
    let mut preview_open = hooks.use_state(|| false);
    let mut preview_scroll = hooks.use_state(|| 0usize);

    // State: sidebar tab.
    let mut sidebar_tab = hooks.use_state(|| SidebarTab::Overview);

    // State: cached PR detail data for sidebar tabs (HashMap cache + debounce).
    let mut detail_cache = hooks.use_state(HashMap::<u64, PrDetail>::new);
    // Pending detail request: parameters for the next debounced fetch.
    let mut pending_detail = hooks.use_state(|| Option::<DetailRequest>::None);
    let mut debounce_gen = hooks.use_state(|| 0u64);

    // State: input mode for actions.
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut input_buffer = hooks.use_state(String::new);
    let mut action_status = hooks.use_state(|| Option::<String>::None);

    // State: search query.
    let mut search_query = hooks.use_state(String::new);

    // State: assignee autocomplete.
    let mut assignee_candidates = hooks.use_state(Vec::<String>::new);
    let mut assignee_selection = hooks.use_state(|| 0usize);
    let mut assignee_selected = hooks.use_state(Vec::<String>::new);

    // State: label autocomplete.
    let mut label_candidates = hooks.use_state(Vec::<String>::new);
    let mut label_selection = hooks.use_state(|| 0usize);
    let mut label_selected = hooks.use_state(Vec::<String>::new);

    let mut help_visible = hooks.use_state(|| false);

    // State: run selector overlay for JumpToRun disambiguation.
    let mut run_selector_items =
        hooks.use_state(|| Option::<Vec<(NavigationTarget, String)>>::None);
    let mut run_selector_cursor = hooks.use_state(|| 0usize);
    let nav_target = props.nav_target;
    let go_back_prop = props.go_back;

    // State: ephemeral tabs created by deep-linking to repos without config tabs.
    // Each entry is (filter, optional pending PR number to highlight after fetch).
    let mut ephemeral_filters = hooks.use_state(Vec::<(PrFilter, Option<u64>)>::new);

    // State: rate limit from last GraphQL response.
    let mut rate_limit_state = hooks.use_state(|| Option::<RateLimitInfo>::None);

    // State: per-filter fetch tracking (lazy: only fetch the active filter).
    let mut filter_fetch_times =
        hooks.use_state(move || vec![Option::<std::time::Instant>::None; filter_count]);
    let mut filter_in_flight = hooks.use_state(move || vec![false; filter_count]);
    // Set by 'R' keypress; consumed by render body to fetch all filters eagerly.
    let mut refresh_all = hooks.use_state(|| false);

    // When true, the next lazy fetch bypasses the moka cache (set by `r` key and MutationOk).
    let mut force_refresh = hooks.use_state(|| false);

    // Whether RegisterPrsRefresh has been sent to the engine yet.
    let mut refresh_registered = hooks.use_state(|| false);

    // State: loaded filter data (non-Copy, use .read()/.set()).
    let initial_filters = vec![FilterData::default(); filter_count];
    let mut prs_state = hooks.use_state(move || PrsState {
        filters: initial_filters,
    });

    // Track scope changes: when scope_repo changes, invalidate all filters.
    let mut last_scope = hooks.use_state(|| scope_repo.clone());
    if *last_scope.read() != *scope_repo {
        last_scope.set(scope_repo.clone());
        // Reset all filters to trigger refetch with new scope.
        prs_state.set(PrsState {
            filters: vec![FilterData::default(); filter_count],
        });
        filter_fetch_times.set(vec![None; filter_count]);
        filter_in_flight.set(vec![false; filter_count]);
        refresh_registered.set(false);
    }

    // Per-view event channel: engine sends results here, polling future processes them.
    let event_channel = hooks.use_state(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        (tx, std::sync::Arc::new(std::sync::Mutex::new(rx)))
    });
    let (event_tx, event_rx_arc) = event_channel.read().clone();
    // Clone the EngineHandle so it can be captured in 'static use_future closures.
    let engine: Option<EngineHandle> = props.engine.cloned();
    // Pre-clone for each consumer: debounce future, polling future, fetch trigger, keyboard handler.
    let engine_for_poll = engine.clone();
    let engine_for_keyboard = engine.clone();

    // Debounce future: waits for cursor to settle, then sends FetchPrDetail to engine.
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
                if let Some(DetailRequest {
                    owner,
                    repo,
                    pr_number,
                    base_ref,
                    head_repo_owner,
                    head_ref,
                }) = req
                {
                    spawned_gen = current_gen;
                    if let Some(ref eng) = engine_for_debounce {
                        eng.send(Request::FetchPrDetail {
                            owner: owner.clone(),
                            repo: repo.clone(),
                            number: pr_number,
                            base_ref,
                            head_repo_owner,
                            head_ref,
                            reply_tx: event_tx_for_debounce.clone(),
                        });
                    }
                }
            }
        }
    });

    // Compute active filter index early (needed by fetch logic below).
    let eph_snapshot = ephemeral_filters.read().clone();
    let ephemeral_count = eph_snapshot.len();
    let total_tab_count = filter_count + ephemeral_count;
    let current_filter_idx = active_filter.get().min(total_tab_count.saturating_sub(1));
    let all_filters = merged_pr_filters(filters_cfg, &eph_snapshot);

    // Lazy fetch: only fetch the active filter when it needs data.
    let active_needs_fetch = prs_state
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
        eng.send(Request::RegisterPrsRefresh {
            filter_configs: scoped_configs,
            notify_tx: event_tx.clone(),
        });
        refresh_registered.set(true);
    }

    if refresh_all.get()
        && is_active
        && let Some(ref engine) = engine
    {
        // 'R' was pressed: reset the flag and eagerly fetch every filter.
        refresh_all.set(false);
        let mut in_flight = filter_in_flight.read().clone();
        for (filter_idx, (cfg, _is_eph)) in all_filters.iter().enumerate() {
            if filter_idx < in_flight.len() {
                in_flight[filter_idx] = true;
            }
            let mut modified_filter = (*cfg).clone();
            modified_filter.filters = apply_scope(&cfg.filters, scope_repo.as_deref());
            engine.send(Request::FetchPrs {
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
        && let Some(ref engine) = engine
    {
        // Look up the active filter from the merged list (config + ephemeral).
        if let Some((cfg, _is_eph)) = all_filters.get(current_filter_idx) {
            // Mark this filter as in-flight.
            let mut in_flight = filter_in_flight.read().clone();
            if current_filter_idx < in_flight.len() {
                in_flight[current_filter_idx] = true;
            }
            filter_in_flight.set(in_flight);

            let filter_idx = current_filter_idx;
            let mut modified_filter = (*cfg).clone();
            modified_filter.filters = apply_scope(&cfg.filters, scope_repo.as_deref());

            // Consume the force flag: bypass cache for `r`-key and post-mutation fetches.
            let force = force_refresh.get();
            if force {
                force_refresh.set(false);
            }

            engine.send(Request::FetchPrs {
                filter_idx,
                filter: modified_filter,
                force,
                reply_tx: event_tx.clone(),
            });
        }
    }

    // Polling future: receive engine events every 100ms and update state.
    {
        let rx_for_poll = event_rx_arc.clone();
        let theme_for_poll = theme.clone();
        let date_format_for_poll = props.date_format.unwrap_or("relative").to_owned();
        let prefetch_limit = props.prefetch_pr_details as usize;
        let current_filter_for_poll = current_filter_idx;
        let engine = engine_for_poll;
        let event_tx = event_tx.clone();
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
                        Event::PrsFetched {
                            filter_idx,
                            prs,
                            rate_limit,
                        } => {
                            if rate_limit.is_some() {
                                rate_limit_state.set(rate_limit);
                            }
                            let detail_snap = detail_cache.read().clone();
                            let rows: Vec<Row> = prs
                                .iter()
                                .map(|pr| {
                                    let detail = detail_snap.get(&pr.number);
                                    pr_to_row(pr, &theme_for_poll, &date_format_for_poll, detail)
                                })
                                .collect();
                            let bodies: Vec<String> =
                                prs.iter().map(|pr| pr.body.clone()).collect();
                            let titles: Vec<String> =
                                prs.iter().map(|pr| pr.title.clone()).collect();
                            let pr_count = prs.len();
                            let prs_for_prefetch: Vec<PrRef> = prs
                                .iter()
                                .take(prefetch_limit)
                                .filter(|pr| {
                                    !detail_snap.contains_key(&pr.number)
                                        && pr.state == crate::github::types::PrState::Open
                                })
                                .filter_map(|pr| {
                                    pr.repo.as_ref().map(|r| PrRef {
                                        owner: r.owner.clone(),
                                        repo: r.name.clone(),
                                        number: pr.number,
                                        base_ref: pr.base_ref.clone(),
                                        head_repo_owner: pr.head_repo_owner.clone(),
                                        head_ref: pr.head_ref.clone(),
                                    })
                                })
                                .collect();
                            let filter_data = FilterData {
                                rows,
                                bodies,
                                titles,
                                prs,
                                pr_count,
                                loading: false,
                                error: None,
                            };
                            let mut state = prs_state.read().clone();
                            if filter_idx < state.filters.len() {
                                state.filters[filter_idx] = filter_data;
                            }
                            prs_state.set(state);
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
                            // Trigger prefetch via engine.
                            if !prs_for_prefetch.is_empty()
                                && let Some(ref eng) = engine
                            {
                                eng.send(Request::PrefetchPrDetails {
                                    prs: prs_for_prefetch,
                                    reply_tx: event_tx.clone(),
                                });
                            }
                        }
                        Event::PrDetailFetched {
                            number,
                            detail,
                            rate_limit,
                        } => {
                            if rate_limit.is_some() {
                                rate_limit_state.set(rate_limit);
                            }
                            // Update the "update" cell in the table row.
                            // Skip for closed/merged PRs — branch status is irrelevant.
                            let mut state = prs_state.read().clone();
                            'update: for fd in &mut state.filters {
                                if let Some(idx) = fd.prs.iter().position(|p| p.number == number) {
                                    let update = if matches!(
                                        fd.prs[idx].state,
                                        crate::github::types::PrState::Closed
                                            | crate::github::types::PrState::Merged
                                    ) {
                                        Cell::colored("-".to_owned(), theme_for_poll.text_faint)
                                    } else {
                                        update_cell_from_detail(&detail, &theme_for_poll)
                                    };
                                    fd.rows[idx].insert("update".to_owned(), update);
                                    break 'update;
                                }
                            }
                            prs_state.set(state);
                            let mut cache = detail_cache.read().clone();
                            cache.insert(number, detail);
                            detail_cache.set(cache);
                        }
                        Event::FetchError {
                            context: _,
                            message,
                        } => {
                            // Find the in-flight filter and mark it as error.
                            let in_flight = filter_in_flight.read().clone();
                            let error_filter_idx = in_flight.iter().position(|&f| f);
                            if let Some(fi) = error_filter_idx {
                                let mut state = prs_state.read().clone();
                                if fi < state.filters.len() {
                                    state.filters[fi] = FilterData {
                                        loading: false,
                                        error: Some(message.clone()),
                                        ..FilterData::default()
                                    };
                                }
                                prs_state.set(state);
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
                            let mut state = prs_state.read().clone();
                            if current_filter_for_poll < state.filters.len() {
                                state.filters[current_filter_for_poll] = FilterData::default();
                            }
                            prs_state.set(state);
                            let mut times = filter_fetch_times.read().clone();
                            if current_filter_for_poll < times.len() {
                                times[current_filter_for_poll] = None;
                            }
                            filter_fetch_times.set(times);
                            // Must come LAST: force_refresh is consumed by the
                            // lazy-fetch trigger only when active_needs_fetch
                            // (loading=true) is already visible in the same
                            // render. Setting it before prs_state.set() risks
                            // a render where loading is still false and the
                            // flag is silently dropped, causing a non-forced
                            // (cached) refetch.
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

    // -----------------------------------------------------------------------
    // Process cross-view navigation target (deep-link from CLI or another view)
    // -----------------------------------------------------------------------

    let nav_target_prop = props.nav_target;

    if is_active && let Some(ref nt_state) = nav_target_prop {
        let target = nt_state.read().clone();
        if let Some(NavigationTarget::PullRequest {
            ref owner,
            ref repo,
            number,
            ref host,
        }) = target
        {
            // 1. Search all loaded filter data (config + ephemeral) for this PR.
            let found = {
                let state = prs_state.read();
                state.filters.iter().enumerate().find_map(|(fi, fd)| {
                    if fd.loading {
                        return None;
                    }
                    fd.prs
                        .iter()
                        .position(|p| {
                            p.number == number
                                && p.repo
                                    .as_ref()
                                    .is_some_and(|r| r.owner == *owner && r.name == *repo)
                        })
                        .map(|pos| (fi, pos))
                })
            };

            if let Some((filter_idx, pr_pos)) = found {
                // PR found in an existing tab — switch to it.
                active_filter.set(filter_idx);
                cursor.set(pr_pos);
                scroll_offset.set(pr_pos.saturating_sub(5));
                preview_open.set(true);
                preview_scroll.set(0);
                search_query.set(String::new());
                if let Some(mut nt) = nav_target_prop {
                    nt.set(None);
                }
            } else {
                // 2. Check if any filter is still loading — wait.
                let any_in_flight = filter_in_flight.read().iter().any(|&f| f);
                if !any_in_flight {
                    // 3. All loaded, PR not found.
                    let full_repo = format!("{owner}/{repo}");

                    // Check if an ephemeral tab for this repo already exists.
                    let existing_eph = {
                        let eph = ephemeral_filters.read();
                        eph.iter()
                            .position(|(f, _)| f.filters.contains(&format!("repo:{full_repo}")))
                            .map(|ei| filter_count + ei)
                    };

                    if let Some(tab_idx) = existing_eph {
                        // Ephemeral tab exists — switch to it and wait for data.
                        active_filter.set(tab_idx);
                        cursor.set(0);
                        scroll_offset.set(0);
                        preview_scroll.set(0);
                        search_query.set(String::new());

                        // Check if the PR is in this tab's data.
                        let pr_in_tab = {
                            let state = prs_state.read();
                            state.filters.get(tab_idx).and_then(|fd| {
                                if fd.loading {
                                    return None;
                                }
                                fd.prs.iter().position(|p| p.number == number)
                            })
                        };
                        if let Some(pos) = pr_in_tab {
                            cursor.set(pos);
                            scroll_offset.set(pos.saturating_sub(5));
                            preview_open.set(true);
                            preview_scroll.set(0);
                            if let Some(mut nt) = nav_target_prop {
                                nt.set(None);
                            }
                        } else {
                            // Mark pending number on this ephemeral entry so we
                            // position the cursor when data arrives.
                            let ei = tab_idx - filter_count;
                            let mut eph = ephemeral_filters.read().clone();
                            if ei < eph.len() {
                                eph[ei].1 = Some(number);
                                ephemeral_filters.set(eph);
                            }
                            if let Some(mut nt) = nav_target_prop {
                                nt.set(None);
                            }
                        }
                    } else if ephemeral_filters.read().len() < MAX_EPHEMERAL_TABS {
                        // Create new ephemeral tab.
                        tracing::debug!(
                            "deep-link: creating ephemeral PR tab for {full_repo}, number={number}"
                        );
                        let new_filter = PrFilter {
                            title: full_repo.clone(),
                            filters: format!("repo:{full_repo}"),
                            host: host.clone(),
                            limit: None,
                            layout: None,
                        };
                        let mut eph = ephemeral_filters.read().clone();
                        eph.push((new_filter, Some(number)));
                        let new_tab_idx = filter_count + eph.len() - 1;
                        ephemeral_filters.set(eph);

                        // Grow state vectors for the new tab.
                        let mut state = prs_state.read().clone();
                        state.filters.push(FilterData::default());
                        prs_state.set(state);
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
                        preview_scroll.set(0);
                        search_query.set(String::new());

                        // FetchPrs will be triggered by the active_needs_fetch
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

    // Handle pending PR number for ephemeral tabs after data loads.
    {
        let eph = ephemeral_filters.read().clone();
        for (ei, (eph_filter, pending)) in eph.iter().enumerate() {
            if let Some(target_number) = pending {
                let tab_idx = filter_count + ei;
                let state = prs_state.read();
                if let Some(fd) = state.filters.get(tab_idx)
                    && !fd.loading
                {
                    let pos = fd.prs.iter().position(|p| p.number == *target_number);
                    if let Some(pos) = pos {
                        if active_filter.get() == tab_idx {
                            cursor.set(pos);
                            scroll_offset.set(pos.saturating_sub(5));
                            preview_open.set(true);
                            preview_scroll.set(0);
                        }
                    } else {
                        action_status.set(Some(format!(
                            "PR #{target_number} not found in {}",
                            eph_filter.title
                        )));
                    }
                    // Clear pending number.
                    let mut eph_mut = ephemeral_filters.read().clone();
                    eph_mut[ei].1 = None;
                    ephemeral_filters.set(eph_mut);
                }
            }
        }
    }

    // Read current state for rendering.
    let state_ref = prs_state.read();
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

    // Reserve space for tab bar (2 lines), footer (2 lines), header (1 line).
    // Each PR row occupies 2 terminal lines (info + subtitle).
    let visible_rows = (props.height.saturating_sub(5) / 3).max(1) as usize;

    let repo_paths = props.repo_paths.cloned().unwrap_or_default();
    let filter_host_for_kb = all_filters
        .get(current_filter_idx)
        .and_then(|(f, _)| f.host.clone());
    // Engine handle for the keyboard handler closure.
    let engine = engine_for_keyboard;

    let keybindings = props.keybindings.cloned();
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

                // Run selector overlay: intercept keys when showing.
                if run_selector_items.read().is_some() {
                    match code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            let len = run_selector_items.read().as_ref().map_or(0, Vec::len);
                            run_selector_cursor
                                .set((run_selector_cursor.get() + 1).min(len.saturating_sub(1)));
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            run_selector_cursor.set(run_selector_cursor.get().saturating_sub(1));
                        }
                        KeyCode::Enter => {
                            let selected = {
                                let items = run_selector_items.read();
                                items
                                    .as_ref()
                                    .and_then(|v| v.get(run_selector_cursor.get()))
                                    .map(|(nav, _)| nav.clone())
                            };
                            run_selector_items.set(None);
                            run_selector_cursor.set(0);
                            if let Some(target) = selected
                                && let Some(mut nt) = nav_target
                            {
                                nt.set(Some(target));
                            }
                        }
                        KeyCode::Esc => {
                            run_selector_items.set(None);
                            run_selector_cursor.set(0);
                        }
                        _ => {}
                    }
                    return;
                }

                // Read input mode into a local to avoid borrow conflict.
                let current_mode = input_mode.read().clone();
                match current_mode {
                    InputMode::Assign => {
                        handle_assign_input(
                            code,
                            modifiers,
                            input_mode,
                            input_buffer,
                            &prs_state,
                            current_filter_idx,
                            cursor.get(),
                            engine.as_ref(),
                            &event_tx,
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
                            &prs_state,
                            current_filter_idx,
                            cursor.get(),
                            engine.as_ref(),
                            &event_tx,
                        );
                    }
                    InputMode::Comment => match code {
                        // Submit comment with Ctrl+D.
                        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                            let comment_text = input_buffer.read().clone();
                            if !comment_text.is_empty() {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_filter_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info
                                    && let Some(ref eng) = engine
                                {
                                    eng.send(Request::AddPrComment {
                                        owner: owner.clone(),
                                        repo: repo.clone(),
                                        number,
                                        body: comment_text.clone(),
                                        reply_tx: event_tx.clone(),
                                    });
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
                            let pr_info =
                                get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
                            if let Some((owner, repo, number)) = pr_info
                                && let Some(ref eng) = engine
                            {
                                match pending {
                                    BuiltinAction::Close => {
                                        eng.send(Request::ClosePr {
                                            owner: owner.clone(),
                                            repo: repo.clone(),
                                            number,
                                            reply_tx: event_tx.clone(),
                                        });
                                    }
                                    BuiltinAction::Reopen => {
                                        eng.send(Request::ReopenPr {
                                            owner: owner.clone(),
                                            repo: repo.clone(),
                                            number,
                                            reply_tx: event_tx.clone(),
                                        });
                                    }
                                    BuiltinAction::Merge => {
                                        eng.send(Request::MergePr {
                                            owner: owner.clone(),
                                            repo: repo.clone(),
                                            number,
                                            reply_tx: event_tx.clone(),
                                        });
                                    }
                                    BuiltinAction::Approve => {
                                        eng.send(Request::ApprovePr {
                                            owner: owner.clone(),
                                            repo: repo.clone(),
                                            number,
                                            body: None,
                                            reply_tx: event_tx.clone(),
                                        });
                                    }
                                    BuiltinAction::UpdateFromBase => {
                                        eng.send(Request::UpdateBranch {
                                            owner: owner.clone(),
                                            repo: repo.clone(),
                                            number,
                                            reply_tx: event_tx.clone(),
                                        });
                                    }
                                    BuiltinAction::MarkReady => {
                                        eng.send(Request::ReadyForReview {
                                            owner: owner.clone(),
                                            repo: repo.clone(),
                                            number,
                                            reply_tx: event_tx.clone(),
                                        });
                                    }
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
                    InputMode::Normal => {
                        if let Some(key_str) = key_event_to_string(code, modifiers, kind) {
                            let pr_info =
                                get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
                            let (pr_owner, pr_repo, pr_number) =
                                pr_info.unwrap_or_else(|| (String::new(), String::new(), 0));
                            let pr_url = if pr_number > 0 {
                                format!("https://github.com/{pr_owner}/{pr_repo}/pull/{pr_number}")
                            } else {
                                String::new()
                            };
                            let head_branch = {
                                let state = prs_state.read();
                                state
                                    .filters
                                    .get(current_filter_idx)
                                    .and_then(|f| f.prs.get(cursor.get()))
                                    .map_or_else(String::new, |p| p.head_ref.clone())
                            };
                            let base_branch = {
                                let state = prs_state.read();
                                state
                                    .filters
                                    .get(current_filter_idx)
                                    .and_then(|f| f.prs.get(cursor.get()))
                                    .map_or_else(String::new, |p| p.base_ref.clone())
                            };
                            let vars = TemplateVars {
                                url: pr_url.clone(),
                                number: if pr_number > 0 {
                                    pr_number.to_string()
                                } else {
                                    String::new()
                                },
                                repo_name: format!("{pr_owner}/{pr_repo}"),
                                head_branch,
                                base_branch,
                            };
                            match keybindings
                                .as_ref()
                                .and_then(|kb| kb.resolve(&key_str, ViewContext::Prs))
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
                                    BuiltinAction::Approve => {
                                        input_mode.set(InputMode::Confirm(BuiltinAction::Approve));
                                        action_status.set(None);
                                    }
                                    BuiltinAction::CommentAction => {
                                        input_mode.set(InputMode::Comment);
                                        input_buffer.set(String::new());
                                        action_status.set(None);
                                    }
                                    BuiltinAction::Close => {
                                        input_mode.set(InputMode::Confirm(BuiltinAction::Close));
                                        action_status.set(None);
                                    }
                                    BuiltinAction::Reopen => {
                                        input_mode.set(InputMode::Confirm(BuiltinAction::Reopen));
                                        action_status.set(None);
                                    }
                                    BuiltinAction::Merge => {
                                        input_mode.set(InputMode::Confirm(BuiltinAction::Merge));
                                        action_status.set(None);
                                    }
                                    BuiltinAction::UpdateFromBase => {
                                        let (pn, coarse) = {
                                            let state = prs_state.read();
                                            let pr = state
                                                .filters
                                                .get(current_filter_idx)
                                                .and_then(|f| f.prs.get(cursor.get()));
                                            (pr.map(|p| p.number), pr.map(branch_update_status))
                                        };
                                        let effective = pn.and_then(|num| {
                                            let cache = detail_cache.read();
                                            cache.get(&num).and_then(effective_update_status)
                                        });
                                        match effective {
                                            Some(MergeStateStatus::Clean) => {
                                                action_status.set(Some(
                                                    "Branch is already up-to-date".into(),
                                                ));
                                            }
                                            Some(MergeStateStatus::Dirty) => {
                                                action_status.set(Some(
                                                    "Cannot auto-update: branch has conflicts"
                                                        .into(),
                                                ));
                                            }
                                            Some(MergeStateStatus::Behind) => {
                                                input_mode.set(InputMode::UpdateBranchMethod);
                                                action_status.set(None);
                                            }
                                            _ => match coarse {
                                                Some(BranchUpdateStatus::HasConflicts) => {
                                                    action_status.set(Some(
                                                        "Cannot auto-update: branch has conflicts"
                                                            .into(),
                                                    ));
                                                }
                                                Some(BranchUpdateStatus::NeedsUpdate) => {
                                                    input_mode.set(InputMode::UpdateBranchMethod);
                                                    action_status.set(None);
                                                }
                                                _ => {
                                                    input_mode.set(InputMode::Confirm(
                                                        BuiltinAction::UpdateFromBase,
                                                    ));
                                                    action_status.set(None);
                                                }
                                            },
                                        }
                                    }
                                    BuiltinAction::MarkReady => {
                                        input_mode
                                            .set(InputMode::Confirm(BuiltinAction::MarkReady));
                                        action_status.set(None);
                                    }
                                    BuiltinAction::ViewDiff if pr_number > 0 => {
                                        match crate::actions::local::open_diff(
                                            &pr_owner, &pr_repo, pr_number,
                                        ) {
                                            Ok(msg) => action_status.set(Some(msg)),
                                            Err(e) => {
                                                action_status.set(Some(format!("Diff error: {e}")));
                                            }
                                        }
                                    }
                                    BuiltinAction::Checkout => {
                                        let current_data = prs_state
                                            .read()
                                            .filters
                                            .get(current_filter_idx)
                                            .cloned();
                                        if let Some(data) = current_data
                                            && let Some(pr) = data.prs.get(cursor.get())
                                        {
                                            let repo_name = pr
                                                .repo
                                                .as_ref()
                                                .map(crate::github::types::RepoRef::full_name)
                                                .unwrap_or_default();
                                            match crate::actions::local::checkout_branch(
                                                &pr.head_ref,
                                                &repo_name,
                                                &repo_paths,
                                            ) {
                                                Ok(msg) => action_status.set(Some(msg)),
                                                Err(e) => action_status
                                                    .set(Some(format!("Checkout error: {e}"))),
                                            }
                                        }
                                    }
                                    BuiltinAction::Worktree => {
                                        let current_data = prs_state
                                            .read()
                                            .filters
                                            .get(current_filter_idx)
                                            .cloned();
                                        if let Some(data) = current_data
                                            && let Some(pr) = data.prs.get(cursor.get())
                                        {
                                            let repo_name = pr
                                                .repo
                                                .as_ref()
                                                .map(crate::github::types::RepoRef::full_name)
                                                .unwrap_or_default();
                                            match crate::actions::local::create_or_open_worktree(
                                                &pr.head_ref,
                                                &repo_name,
                                                &repo_paths,
                                            ) {
                                                Ok(path) => {
                                                    match clipboard::copy_to_clipboard(&path) {
                                                        Ok(()) => action_status.set(Some(
                                                            format!("Worktree ready (copied): {path}"),
                                                        )),
                                                        Err(e) => action_status.set(Some(
                                                            format!("Worktree ready: {path} (clipboard: {e})"),
                                                        )),
                                                    }
                                                }
                                                Err(e) => action_status
                                                    .set(Some(format!("Worktree error: {e}"))),
                                            }
                                        }
                                    }
                                    BuiltinAction::Assign | BuiltinAction::Unassign => {
                                        input_mode.set(InputMode::Assign);
                                        input_buffer.set(String::new());
                                        assignee_selection.set(0);
                                        let current = get_current_pr_assignees(
                                            &prs_state,
                                            current_filter_idx,
                                            cursor.get(),
                                        );
                                        assignee_selected.set(current);
                                        let initial = {
                                            let state = prs_state.read();
                                            state
                                                .filters
                                                .get(current_filter_idx)
                                                .and_then(|f| f.prs.get(cursor.get()))
                                                .map(build_pr_assignee_candidates)
                                                .unwrap_or_default()
                                        };
                                        assignee_candidates.set(initial);
                                        action_status.set(None);
                                        if let Some(ref eng) = engine
                                            && let Some((owner, repo, _)) = get_current_pr_info(
                                                &prs_state,
                                                current_filter_idx,
                                                cursor.get(),
                                            )
                                        {
                                            eng.send(Request::FetchRepoCollaborators {
                                                owner,
                                                repo,
                                                reply_tx: event_tx.clone(),
                                            });
                                        }
                                    }
                                    BuiltinAction::LabelAction => {
                                        input_mode.set(InputMode::Label);
                                        input_buffer.set(String::new());
                                        label_selection.set(0);
                                        label_candidates.set(Vec::new());
                                        let current_labels = get_current_pr_labels(
                                            &prs_state,
                                            current_filter_idx,
                                            cursor.get(),
                                        );
                                        label_selected.set(current_labels);
                                        action_status.set(None);
                                        if let Some(ref eng) = engine
                                            && let Some((owner, repo, _)) = get_current_pr_info(
                                                &prs_state,
                                                current_filter_idx,
                                                cursor.get(),
                                            )
                                        {
                                            eng.send(Request::FetchRepoLabels {
                                                owner,
                                                repo,
                                                reply_tx: event_tx.clone(),
                                            });
                                        }
                                    }
                                    BuiltinAction::CopyNumber if pr_number > 0 => {
                                        let text = pr_number.to_string();
                                        match clipboard::copy_to_clipboard(&text) {
                                            Ok(()) => action_status
                                                .set(Some(format!("Copied #{pr_number}"))),
                                            Err(e) => {
                                                action_status
                                                    .set(Some(format!("Copy failed: {e}")));
                                            }
                                        }
                                    }
                                    BuiltinAction::CopyUrl if !pr_url.is_empty() => {
                                        match clipboard::copy_to_clipboard(&pr_url) {
                                            Ok(()) => action_status
                                                .set(Some(format!("Copied URL for #{pr_number}"))),
                                            Err(e) => {
                                                action_status
                                                    .set(Some(format!("Copy failed: {e}")));
                                            }
                                        }
                                    }
                                    BuiltinAction::OpenBrowser if !pr_url.is_empty() => {
                                        match clipboard::open_in_browser(&pr_url) {
                                            Ok(()) => action_status
                                                .set(Some(format!("Opened #{pr_number}"))),
                                            Err(e) => {
                                                action_status
                                                    .set(Some(format!("Open failed: {e}")));
                                            }
                                        }
                                    }
                                    BuiltinAction::Refresh => {
                                        force_refresh.set(true);
                                        let idx = active_filter.get();
                                        let mut state = prs_state.read().clone();
                                        if idx < state.filters.len() {
                                            state.filters[idx] = FilterData::default();
                                        }
                                        prs_state.set(state);
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
                                        let mut state = prs_state.read().clone();
                                        for filter in &mut state.filters {
                                            *filter = FilterData::default();
                                        }
                                        prs_state.set(state);
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
                                        preview_scroll.set(0);
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
                                    BuiltinAction::Last if total_rows > 0 => {
                                        cursor.set(total_rows.saturating_sub(1));
                                        scroll_offset.set(total_rows.saturating_sub(visible_rows));
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::PageDown if total_rows > 0 => {
                                        let new_cursor = (cursor.get() + visible_rows)
                                            .min(total_rows.saturating_sub(1));
                                        cursor.set(new_cursor);
                                        scroll_offset.set(
                                            new_cursor
                                                .saturating_sub(visible_rows.saturating_sub(1)),
                                        );
                                        preview_scroll.set(0);
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
                                    BuiltinAction::ToggleHelp => {
                                        help_visible.set(true);
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
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::NextFilter if total_tab_count > 0 => {
                                        active_filter
                                            .set((active_filter.get() + 1) % total_tab_count);
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                        preview_scroll.set(0);
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
                                            let mut state = prs_state.read().clone();
                                            if current_filter_idx < state.filters.len() {
                                                state.filters.remove(current_filter_idx);
                                            }
                                            prs_state.set(state);
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
                                    BuiltinAction::GoBack => {
                                        preview_open.set(false);
                                        if let Some(mut gb) = go_back_prop {
                                            gb.set(true);
                                        }
                                    }
                                    BuiltinAction::JumpToRun => {
                                        // Collect distinct workflow runs from current PR's checks.
                                        let entries = {
                                            let state = prs_state.read();
                                            let pr = state
                                                .filters
                                                .get(current_filter_idx)
                                                .and_then(|f| f.prs.get(cursor.get()));
                                            let repo_ref = pr.and_then(|p| p.repo.as_ref());
                                            if let Some(pr) = pr
                                                && let Some(rr) = repo_ref
                                            {
                                                let mut seen = HashSet::new();
                                                pr.check_runs
                                                    .iter()
                                                    .filter_map(|cr| {
                                                        let rid = cr.workflow_run_id?;
                                                        if !seen.insert(rid) {
                                                            return None;
                                                        }
                                                        let label = cr
                                                            .workflow_name
                                                            .as_deref()
                                                            .unwrap_or(&cr.name);
                                                        Some((
                                                            NavigationTarget::ActionsRun {
                                                                owner: rr.owner.clone(),
                                                                repo: rr.name.clone(),
                                                                run_id: rid,
                                                                host: filter_host_for_kb.clone(),
                                                            },
                                                            label.to_owned(),
                                                        ))
                                                    })
                                                    .collect()
                                            } else {
                                                Vec::new()
                                            }
                                        };
                                        match entries.len() {
                                            0 => {
                                                action_status.set(Some(
                                                    "No Actions run linked to this PR's checks"
                                                        .to_owned(),
                                                ));
                                            }
                                            1 => {
                                                if let (Some(mut nt), Some((target, _))) =
                                                    (nav_target, entries.into_iter().next())
                                                {
                                                    nt.set(Some(target));
                                                }
                                            }
                                            _ => {
                                                run_selector_cursor.set(0);
                                                run_selector_items.set(Some(entries));
                                            }
                                        }
                                    }
                                    _ => {}
                                },
                                Some(ResolvedBinding::ShellCommand(cmd)) => {
                                    let expanded = expand_template(&cmd, &vars);
                                    let _ = execute_shell_command(&expanded);
                                }
                                None => {
                                    if key_str == "]" {
                                        sidebar_tab.set(sidebar_tab.get().next());
                                        preview_scroll.set(0);
                                    } else if key_str == "[" {
                                        sidebar_tab.set(sidebar_tab.get().prev());
                                        preview_scroll.set(0);
                                    }
                                }
                            }
                        }
                    }
                    InputMode::UpdateBranchMethod => match code {
                        // Merge-update (only merge strategy supported).
                        KeyCode::Char('m' | 'M') => {
                            let pr_info =
                                get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
                            if let Some((owner, repo, number)) = pr_info
                                && let Some(ref eng) = engine
                            {
                                eng.send(Request::UpdateBranch {
                                    owner: owner.clone(),
                                    repo: repo.clone(),
                                    number,
                                    reply_tx: event_tx.clone(),
                                });
                            }
                            input_mode.set(InputMode::Normal);
                        }
                        KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            action_status.set(Some("Cancelled".to_owned()));
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
    let tabs: Vec<Tab> = all_filters
        .iter()
        .enumerate()
        .map(|(i, (f, is_eph))| Tab {
            title: f.title.clone(),
            count: state_ref.filters.get(i).map(|d| d.pr_count),
            is_ephemeral: *is_eph,
        })
        .collect();

    // Current filter data.
    let current_data = state_ref.filters.get(current_filter_idx);
    let columns = pr_columns(&theme.icons);

    // Layout config for hidden/width overrides.
    let layout = filters_cfg
        .get(current_filter_idx)
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
        scrollbar_thumb_color: Some(theme.border_primary),
    });

    // Request detail when sidebar is open and current PR is not cached.
    if is_preview_open {
        let cursor_idx = cursor.get();
        let current_pr = current_data.and_then(|d| d.prs.get(cursor_idx));
        if let Some(pr) = current_pr {
            let pr_number = pr.number;
            let already_cached = detail_cache.read().contains_key(&pr_number);
            let already_pending = {
                let guard = pending_detail.read();
                match *guard {
                    Some(ref r) => r.pr_number == pr_number,
                    None => false,
                }
            };

            if !already_cached && !already_pending {
                // Store fetch params; debounce future will spawn fetch when stable.
                if let Some(repo_ref) = &pr.repo {
                    pending_detail.set(Some(DetailRequest {
                        owner: repo_ref.owner.clone(),
                        repo: repo_ref.name.clone(),
                        pr_number,
                        base_ref: pr.base_ref.clone(),
                        head_repo_owner: pr.head_repo_owner.clone(),
                        head_ref: pr.head_ref.clone(),
                    }));
                    debounce_gen.set(debounce_gen.get() + 1);
                }
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
        let cache_ref = detail_cache.read();
        let detail_for_pr = current_pr.and_then(|pr| cache_ref.get(&pr.number));

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
                if let Some(detail) = detail_for_pr {
                    sidebar_tabs::render_activity(detail, &theme, depth)
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
                    sidebar_tabs::render_checks(pr, &theme, sidebar_width)
                } else {
                    Vec::new()
                }
            }
            SidebarTab::Files => {
                if let Some(detail) = detail_for_pr {
                    sidebar_tabs::render_files(detail, &theme, sidebar_width)
                } else {
                    vec![StyledLine::from_span(
                        crate::markdown::renderer::StyledSpan::text("Loading...", theme.text_faint),
                    )]
                }
            }
        };

        // Build meta header for Overview tab.
        let sidebar_meta = if current_tab == SidebarTab::Overview {
            current_pr.map(|pr| build_sidebar_meta(pr, detail_for_pr, &theme, depth))
        } else {
            None
        };

        // Account for tab bar (2 extra lines) + meta in sidebar height.
        #[allow(clippy::cast_possible_truncation)]
        let meta_lines = sidebar_meta.as_ref().map_or(0, SidebarMeta::line_count) as u16;
        let sidebar_visible_lines = props.height.saturating_sub(8 + meta_lines) as usize;

        let sidebar = RenderedSidebar::build_tabbed(
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
            None, // Show all tabs for PRs
        );
        // Store the clamped offset so ctrl+u works immediately.
        if preview_scroll.get() != sidebar.clamped_scroll {
            preview_scroll.set(sidebar.clamped_scroll);
        }
        Some(sidebar)
    } else {
        None
    };

    let rendered_tab_bar = RenderedTabBar::build(
        &tabs,
        current_filter_idx,
        props.show_filter_count,
        depth,
        Some(theme.footer_prs),
        Some(theme.footer_prs),
        Some(theme.border_faint),
        &theme.icons.tab_filter,
        &theme.icons.tab_ephemeral,
    );

    // Build footer or input area based on mode.
    let current_mode = input_mode.read().clone();

    let rendered_text_input = match &current_mode {
        InputMode::Assign => {
            let buf = input_buffer.read().clone();
            let candidates = assignee_candidates.read();
            let filtered = crate::components::text_input::filter_suggestions(&candidates, &buf);
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
            let filtered = crate::components::text_input::filter_suggestions(&candidates, &buf);
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
                BuiltinAction::Close => "Close this PR? (y/n)",
                BuiltinAction::Reopen => "Reopen this PR? (y/n)",
                BuiltinAction::Merge => "Merge this PR? (y/n)",
                BuiltinAction::Approve => "Approve this PR? (y/n)",
                BuiltinAction::UpdateFromBase => "Update branch from base? (y/n)",
                BuiltinAction::MarkReady => "Mark this draft PR ready for review? (y/n)",
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
        InputMode::UpdateBranchMethod => Some(RenderedTextInput::build(
            "[m]erge  Esc cancel",
            "",
            depth,
            Some(theme.text_primary),
            Some(theme.text_warning),
            Some(theme.border_faint),
        )),
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
        ViewKind::Prs,
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

    let rendered_run_selector = {
        let items_opt = run_selector_items.read();
        items_opt.as_ref().map(|items| {
            let overlay_items: Vec<SelectionOverlayItem> = items
                .iter()
                .map(|(_, label)| SelectionOverlayItem {
                    label: label.clone(),
                })
                .collect();
            RenderedSelectionOverlay::build(SelectionOverlayBuildConfig {
                title: "Select workflow run".to_owned(),
                items: overlay_items,
                cursor: run_selector_cursor.get(),
                depth,
                title_color: Some(theme.text_primary),
                item_color: Some(theme.text_secondary),
                cursor_color: Some(theme.text_primary),
                selected_bg: Some(theme.bg_selected),
                border_color: Some(theme.border_primary),
                hint_color: Some(theme.text_faint),
                cursor_marker: theme.icons.select_cursor.clone(),
            })
        })
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
            SelectionOverlay(overlay: rendered_run_selector, width: props.width, height: props.height)
        }
    }
    .into_any()
}

/// Build autocomplete candidates from PR data (participants, reviewers, assignees).
/// Returns a deduplicated, sorted list of usernames.
fn build_pr_assignee_candidates(pr: &PullRequest) -> Vec<String> {
    let mut pool = Vec::new();

    // Add participants (already deduplicated by GitHub)
    pool.extend(pr.participants.iter().cloned());

    // Add review requests
    pool.extend(pr.review_requests.iter().map(|a| a.login.clone()));

    // Add current assignees
    pool.extend(pr.assignees.iter().map(|a| a.login.clone()));

    // Add author
    if let Some(author) = &pr.author {
        pool.push(author.login.clone());
    }

    // Deduplicate and sort
    pool.sort();
    pool.dedup();

    pool
}

/// Handle text input for assigning PRs to users.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_assign_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    mut input_mode: State<InputMode>,
    mut input_buffer: State<String>,
    prs_state: &State<PrsState>,
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
            let info = get_current_pr_info(prs_state, filter_idx, cursor);
            if let Some((owner, repo, number)) = info
                && let Some(eng) = engine
            {
                eng.send(Request::SetPrAssignees {
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

/// Handle text input for adding a label to a PR.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn handle_label_input(
    code: KeyCode,
    modifiers: KeyModifiers,
    mut input_mode: State<InputMode>,
    mut input_buffer: State<String>,
    label_candidates: State<Vec<String>>,
    mut label_selection: State<usize>,
    mut label_selected: State<Vec<String>>,
    prs_state: &State<PrsState>,
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
            let info = get_current_pr_info(prs_state, filter_idx, cursor);
            if let Some((owner, repo, number)) = info
                && let Some(eng) = engine
            {
                eng.send(Request::SetPrLabels {
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

/// Extract (owner, repo, number) from the current PR at cursor position.
fn get_current_pr_info(
    prs_state: &State<PrsState>,
    filter_idx: usize,
    cursor: usize,
) -> Option<(String, String, u64)> {
    let state = prs_state.read();
    let filter = state.filters.get(filter_idx)?;
    let pr = filter.prs.get(cursor)?;
    let repo_ref = pr.repo.as_ref()?;
    Some((repo_ref.owner.clone(), repo_ref.name.clone(), pr.number))
}

fn get_current_pr_labels(
    prs_state: &State<PrsState>,
    filter_idx: usize,
    cursor: usize,
) -> Vec<String> {
    let state = prs_state.read();
    let Some(filter) = state.filters.get(filter_idx) else {
        return vec![];
    };
    let Some(pr) = filter.prs.get(cursor) else {
        return vec![];
    };
    pr.labels.iter().map(|l| l.name.clone()).collect()
}

fn get_current_pr_assignees(
    prs_state: &State<PrsState>,
    filter_idx: usize,
    cursor: usize,
) -> Vec<String> {
    let state = prs_state.read();
    let Some(filter) = state.filters.get(filter_idx) else {
        return vec![];
    };
    let Some(pr) = filter.prs.get(cursor) else {
        return vec![];
    };
    pr.assignees.iter().map(|a| a.login.clone()).collect()
}

/// Compute update-status text and color for the sidebar.
///
/// Returns `(None, text_faint)` for closed/merged PRs or when no data is available.
fn sidebar_update_status(
    pr: &PullRequest,
    detail: Option<&PrDetail>,
    theme: &ResolvedTheme,
) -> (Option<String>, AppColor) {
    let icons = &theme.icons;

    if matches!(
        pr.state,
        crate::github::types::PrState::Closed | crate::github::types::PrState::Merged
    ) {
        return (None, theme.text_faint);
    }

    if let Some(d) = detail {
        match effective_update_status(d) {
            Some(MergeStateStatus::Behind) => {
                let suffix = d.behind_by.map_or(String::new(), |b| format!(" by {b}"));
                (
                    Some(format!("{} Behind{suffix}", icons.update_needed)),
                    theme.text_warning,
                )
            }
            Some(MergeStateStatus::Dirty) => (
                Some(format!("{} Conflicts", icons.update_conflict)),
                theme.text_error,
            ),
            Some(_) => (
                Some(format!("{} Up to date", icons.update_ok)),
                theme.text_success,
            ),
            None => (None, theme.text_faint),
        }
    } else {
        match branch_update_status(pr) {
            BranchUpdateStatus::NeedsUpdate => (
                Some(format!("{} Behind", icons.update_needed)),
                theme.text_warning,
            ),
            BranchUpdateStatus::HasConflicts => (
                Some(format!("{} Conflicts", icons.update_conflict)),
                theme.text_error,
            ),
            BranchUpdateStatus::Unknown => (None, theme.text_faint),
        }
    }
}

/// Build the `SidebarMeta` header from a pull request.
#[allow(clippy::too_many_lines, clippy::similar_names)]
fn build_sidebar_meta(
    pr: &PullRequest,
    detail: Option<&PrDetail>,
    theme: &ResolvedTheme,
    depth: ColorDepth,
) -> SidebarMeta {
    let icons = &theme.icons;

    // Pill: state + draft
    let (pill_icon, pill_text, pill_bg_app) = if pr.is_draft {
        (
            icons.pr_draft.clone(),
            "Draft".to_owned(),
            theme.pill_draft_bg,
        )
    } else {
        match pr.state {
            crate::github::types::PrState::Open => {
                (icons.pr_open.clone(), "Open".to_owned(), theme.pill_open_bg)
            }
            crate::github::types::PrState::Closed => (
                icons.pr_closed.clone(),
                "Closed".to_owned(),
                theme.pill_closed_bg,
            ),
            crate::github::types::PrState::Merged => (
                icons.pr_merged.clone(),
                "Merged".to_owned(),
                theme.pill_merged_bg,
            ),
        }
    };

    // Branch: base ← head
    let branch_text = format!("{} {} {}", pr.base_ref, icons.branch_arrow, pr.head_ref);

    // Role
    let (role_icon, role_text) = match pr.author_association {
        Some(AuthorAssociation::Owner) => (icons.role_owner.clone(), "owner".to_owned()),
        Some(AuthorAssociation::Member) => (icons.role_member.clone(), "member".to_owned()),
        Some(AuthorAssociation::Collaborator) => {
            (icons.role_collaborator.clone(), "collaborator".to_owned())
        }
        Some(AuthorAssociation::Contributor) => {
            (icons.role_contributor.clone(), "contributor".to_owned())
        }
        Some(AuthorAssociation::FirstTimer | AuthorAssociation::FirstTimeContributor) => (
            icons.role_newcontributor.clone(),
            "new contributor".to_owned(),
        ),
        Some(AuthorAssociation::None | AuthorAssociation::Mannequin) | None => {
            (icons.role_unknown.clone(), "none".to_owned())
        }
    };

    // Participants: from GitHub's native `participants` connection (includes
    // commenters, reviewers, label editors, etc.)
    let participants: Vec<String> = pr.participants.iter().map(|l| format!("@{l}")).collect();

    let (update_text, update_fg_app) = sidebar_update_status(pr, detail, theme);

    let author_login = pr
        .author
        .as_ref()
        .map_or_else(|| "unknown".to_owned(), |a| a.login.clone());

    // Overview metadata (pinned in fixed section)
    let labels_text = if pr.labels.is_empty() {
        None
    } else {
        Some(
            pr.labels
                .iter()
                .map(|l| crate::util::expand_emoji(&l.name))
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    let assignees_text = if pr.assignees.is_empty() {
        None
    } else {
        Some(
            pr.assignees
                .iter()
                .map(|a| a.login.as_str())
                .collect::<Vec<_>>()
                .join(", "),
        )
    };

    let fmt = "%Y-%m-%d %H:%M:%S";
    let created_text = pr
        .created_at
        .with_timezone(&chrono::Local)
        .format(fmt)
        .to_string();
    let created_age = crate::util::format_date(&pr.created_at, "relative");
    let updated_text = pr
        .updated_at
        .with_timezone(&chrono::Local)
        .format(fmt)
        .to_string();
    let updated_age = crate::util::format_date(&pr.updated_at, "relative");

    let lines_added = Some(format!("+{}", pr.additions));
    let lines_deleted = Some(format!("-{}", pr.deletions));

    SidebarMeta {
        pill_icon,
        pill_text,
        pill_bg: pill_bg_app.to_crossterm_color(depth),
        pill_fg: theme.pill_fg.to_crossterm_color(depth),
        pill_left: icons.pill_left.clone(),
        pill_right: icons.pill_right.clone(),
        branch_text,
        branch_fg: theme.pill_branch.to_crossterm_color(depth),
        update_text,
        update_fg: update_fg_app.to_crossterm_color(depth),
        author_login,
        role_icon,
        role_text,
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
        lines_added,
        lines_deleted,
        reactions_text: None,
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

/// Fallback theme when none is provided.
fn default_theme() -> ResolvedTheme {
    super::default_theme()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{MergeableState, PrDetail, PullRequest};

    fn pr_with_status(status: Option<MergeStateStatus>) -> PullRequest {
        PullRequest {
            number: 1,
            title: String::new(),
            body: String::new(),
            author: None,
            state: crate::github::types::PrState::Open,
            is_draft: false,
            mergeable: None,
            review_decision: None,
            additions: 0,
            deletions: 0,
            head_ref: String::new(),
            base_ref: String::new(),
            labels: vec![],
            assignees: vec![],
            commits: vec![],
            comments: vec![],
            review_threads: vec![],
            review_requests: vec![],
            reviews: vec![],
            timeline_events: vec![],
            files: vec![],
            check_runs: vec![],
            updated_at: chrono::Utc::now(),
            created_at: chrono::Utc::now(),
            url: String::new(),
            repo: None,
            comment_count: 0,
            author_association: None,
            participants: vec![],
            merge_state_status: status,
            head_repo_owner: None,
            head_repo_name: None,
        }
    }

    fn detail_with(mergeable: Option<MergeableState>, behind_by: Option<u32>) -> PrDetail {
        PrDetail {
            body: String::new(),
            reviews: vec![],
            review_threads: vec![],
            timeline_events: vec![],
            commits: vec![],
            files: vec![],
            mergeable,
            behind_by,
        }
    }

    // --- branch_update_status ---

    #[test]
    fn branch_status_behind_is_needs_update() {
        let pr = pr_with_status(Some(MergeStateStatus::Behind));
        assert_eq!(branch_update_status(&pr), BranchUpdateStatus::NeedsUpdate);
    }

    #[test]
    fn branch_status_dirty_is_conflicts() {
        let pr = pr_with_status(Some(MergeStateStatus::Dirty));
        assert_eq!(branch_update_status(&pr), BranchUpdateStatus::HasConflicts);
    }

    #[test]
    fn branch_status_clean_is_unknown() {
        // CLEAN from the search query is not sufficient to show ✓: a repo that
        // does not require up-to-date branches returns CLEAN even when behind.
        let pr = pr_with_status(Some(MergeStateStatus::Clean));
        assert_eq!(branch_update_status(&pr), BranchUpdateStatus::Unknown);
    }

    #[test]
    fn branch_status_unknown_is_unknown() {
        let pr = pr_with_status(Some(MergeStateStatus::Unknown));
        assert_eq!(branch_update_status(&pr), BranchUpdateStatus::Unknown);
    }

    #[test]
    fn branch_status_none_is_unknown() {
        let pr = pr_with_status(None);
        assert_eq!(branch_update_status(&pr), BranchUpdateStatus::Unknown);
    }

    // --- effective_update_status ---

    #[test]
    fn effective_status_conflicting_mergeable_is_dirty() {
        let d = detail_with(Some(MergeableState::Conflicting), None);
        assert_eq!(effective_update_status(&d), Some(MergeStateStatus::Dirty));
    }

    #[test]
    fn effective_status_behind_by_positive_is_behind() {
        let d = detail_with(None, Some(3));
        assert_eq!(effective_update_status(&d), Some(MergeStateStatus::Behind));
    }

    #[test]
    fn effective_status_behind_by_zero_is_clean() {
        let d = detail_with(None, Some(0));
        assert_eq!(effective_update_status(&d), Some(MergeStateStatus::Clean));
    }

    #[test]
    fn effective_status_no_data_is_none() {
        let d = detail_with(None, None);
        assert_eq!(effective_update_status(&d), None);
    }

    #[test]
    fn effective_status_conflicting_takes_priority_over_behind() {
        let d = detail_with(Some(MergeableState::Conflicting), Some(5));
        assert_eq!(effective_update_status(&d), Some(MergeStateStatus::Dirty));
    }
}
