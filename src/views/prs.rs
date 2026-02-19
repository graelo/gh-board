use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_compat::Compat;
use iocraft::prelude::*;
use moka::future::Cache;
use octocrab::Octocrab;

use crate::actions::{clipboard, pr_actions};
use crate::app::ViewKind;
use crate::color::{Color as AppColor, ColorDepth};
use crate::components::footer::{self, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::sidebar::{RenderedSidebar, Sidebar, SidebarMeta, SidebarTab};
use crate::components::sidebar_tabs;
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, Span, TableBuildConfig,
};
use crate::components::text_input::{RenderedTextInput, TextInput};
use crate::config::keybindings::{MergedBindings, ViewContext};
use crate::config::types::PrFilter;
use crate::filter;
use crate::github::graphql::{self, PrDetail, RateLimitInfo};
use crate::github::rate_limit;
use crate::github::types::{
    AuthorAssociation, BranchUpdateStatus, MergeStateStatus, MergeableState, PullRequest,
};
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
            use crate::github::types::ReviewState;
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

    // Update status (refined from detail if available, coarse from PR otherwise)
    let update = if let Some(d) = detail {
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

/// Parameters needed to fetch sidebar detail for a PR.
#[derive(Clone)]
struct DetailRequest {
    octocrab: Arc<Octocrab>,
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
    Confirm(PendingAction),
    /// Search/filter mode (T087).
    Search,
    /// Text input mode for assigning to any user.
    Assign,
    /// Prompt for which branch-update method to use (merge or rebase).
    UpdateBranchMethod,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingAction {
    Close,
    Reopen,
    Merge,
    Approve,
    UpdateBranch,
    ReadyForReview,
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

// ---------------------------------------------------------------------------
// PrsView component (T029-T033 + T040 preview pane + T061-T062 actions)
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct PrsViewProps<'a> {
    /// PR filter configs.
    pub filters: Option<&'a [PrFilter]>,
    /// Octocrab instance for fetching.
    pub octocrab: Option<&'a Arc<Octocrab>>,
    /// Moka API response cache.
    pub api_cache: Option<&'a Cache<String, String>>,
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

    // State: sidebar tab (T072 — FR-014).
    let mut sidebar_tab = hooks.use_state(|| SidebarTab::Overview);

    // State: cached PR detail data for sidebar tabs (HashMap cache + debounce).
    let mut detail_cache = hooks.use_state(HashMap::<u64, PrDetail>::new);
    // Pending detail request: parameters for the next debounced fetch.
    let mut pending_detail = hooks.use_state(|| Option::<DetailRequest>::None);
    let mut debounce_gen = hooks.use_state(|| 0u64);

    // State: input mode for actions (T058, T061).
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut input_buffer = hooks.use_state(String::new);
    let mut action_status = hooks.use_state(|| Option::<String>::None);

    // State: search query (T087).
    let mut search_query = hooks.use_state(String::new);

    // State: assignee autocomplete.
    let mut assignee_candidates = hooks.use_state(Vec::<String>::new);
    let mut assignee_selection = hooks.use_state(|| 0usize);

    let mut help_visible = hooks.use_state(|| false);

    // State: rate limit from last GraphQL response.
    let mut rate_limit_state = hooks.use_state(|| Option::<RateLimitInfo>::None);

    // State: per-filter fetch tracking (lazy: only fetch the active filter).
    let mut filter_fetch_times =
        hooks.use_state(move || vec![Option::<std::time::Instant>::None; filter_count]);
    let mut filter_in_flight = hooks.use_state(move || vec![false; filter_count]);

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
    }

    // Timer tick for periodic re-renders (supports auto-refetch).
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future(async move {
        loop {
            smol::Timer::after(std::time::Duration::from_secs(60)).await;
            tick.set(tick.get() + 1);
        }
    });

    // Debounce future: waits for cursor to settle, then spawns the detail fetch directly.
    let api_cache_for_detail = props.api_cache.cloned();
    let theme_for_debounce = theme.clone();
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
                    octocrab,
                    owner,
                    repo,
                    pr_number,
                    base_ref,
                    head_repo_owner,
                    head_ref: head_ref_name,
                }) = req
                {
                    spawned_gen = current_gen;
                    let api_cache = api_cache_for_detail.clone();
                    let theme_snap = theme_for_debounce.clone();
                    smol::spawn(Compat::new(async move {
                        if let Ok((mut detail, rl)) = graphql::fetch_pr_detail(
                            &octocrab,
                            &owner,
                            &repo,
                            pr_number,
                            api_cache.as_ref(),
                        )
                        .await
                        {
                            // Phase 2: fill behind_by via REST compare when not set.
                            if detail.behind_by.is_none()
                                && let Some(ref head_owner) = head_repo_owner
                            {
                                match graphql::fetch_compare(
                                    &octocrab,
                                    &owner,
                                    &repo,
                                    &base_ref,
                                    head_owner,
                                    &head_ref_name,
                                )
                                .await
                                {
                                    Ok(behind) => detail.behind_by = behind,
                                    Err(e) => tracing::debug!(
                                        "compare API failed for PR #{pr_number}: {e:#}"
                                    ),
                                }
                            }

                            if rl.is_some() {
                                rate_limit_state.set(rl);
                            }

                            // Phase 2: update the "update" cell in the table row.
                            let update = update_cell_from_detail(&detail, &theme_snap);
                            let mut state = prs_state.read().clone();
                            'update: for fd in &mut state.filters {
                                if let Some(idx) = fd.prs.iter().position(|p| p.number == pr_number)
                                {
                                    fd.rows[idx].insert("update".to_owned(), update);
                                    break 'update;
                                }
                            }
                            prs_state.set(state);

                            let mut cache = detail_cache.read().clone();
                            cache.insert(pr_number, detail);
                            detail_cache.set(cache);
                        }
                    }))
                    .detach();
                }
            }
        }
    });

    // Compute active filter index early (needed by fetch logic below).
    let current_filter_idx = active_filter.get().min(filter_count.saturating_sub(1));

    // Auto-refetch: only reset the active filter when its interval has elapsed.
    let refetch_interval = props.refetch_interval_minutes;
    let needs_refetch = is_active
        && refetch_interval > 0
        && !filter_in_flight
            .read()
            .get(current_filter_idx)
            .copied()
            .unwrap_or(false)
        && filter_fetch_times
            .read()
            .get(current_filter_idx)
            .copied()
            .flatten()
            .is_some_and(|last| {
                last.elapsed() >= std::time::Duration::from_secs(u64::from(refetch_interval) * 60)
            });
    if needs_refetch {
        let mut state = prs_state.read().clone();
        if current_filter_idx < state.filters.len() {
            state.filters[current_filter_idx] = FilterData::default();
        }
        prs_state.set(state);
        let mut times = filter_fetch_times.read().clone();
        if current_filter_idx < times.len() {
            times[current_filter_idx] = None;
        }
        filter_fetch_times.set(times);
    }

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

    if active_needs_fetch
        && !active_in_flight
        && is_active
        && let Some(cfg) = filters_cfg.get(current_filter_idx)
        && let Some(octocrab) = props.octocrab
    {
        // Mark this filter as in-flight.
        let mut in_flight = filter_in_flight.read().clone();
        if current_filter_idx < in_flight.len() {
            in_flight[current_filter_idx] = true;
        }
        filter_in_flight.set(in_flight);

        let octocrab = Arc::clone(octocrab);
        let api_cache = props.api_cache.cloned();
        let filter_idx = current_filter_idx;
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
        let prefetch_limit = props.prefetch_pr_details as usize;
        // Phase 3: snapshot detail cache so already-fetched detail backfills rows.
        let detail_cache_snap = detail_cache.read().clone();

        smol::spawn(Compat::new(async move {
            let filter_data = match graphql::search_pull_requests_all(
                &octocrab,
                &filters,
                limit,
                api_cache.as_ref(),
            )
            .await
            {
                Ok((prs, rl)) => {
                    if rl.is_some() {
                        rate_limit_state.set(rl);
                    }
                    let rows: Vec<Row> = prs
                        .iter()
                        .map(|pr| {
                            let detail = detail_cache_snap.get(&pr.number);
                            pr_to_row(pr, &theme_clone, &date_format_owned, detail)
                        })
                        .collect();
                    let bodies: Vec<String> = prs.iter().map(|pr| pr.body.clone()).collect();
                    let titles: Vec<String> = prs.iter().map(|pr| pr.title.clone()).collect();
                    let pr_count = prs.len();
                    FilterData {
                        rows,
                        bodies,
                        titles,
                        prs,
                        pr_count,
                        loading: false,
                        error: None,
                    }
                }
                Err(e) => {
                    let error_msg = if rate_limit::is_rate_limited(&e) {
                        rate_limit::format_rate_limit_message(&e)
                    } else {
                        e.to_string()
                    };
                    FilterData {
                        loading: false,
                        error: Some(error_msg),
                        ..FilterData::default()
                    }
                }
            };

            // Capture PRs for prefetch before filter_data is moved into state.
            let prs_for_prefetch: Vec<PullRequest> = if prefetch_limit > 0 {
                let n = prefetch_limit.min(filter_data.prs.len());
                filter_data.prs[..n].to_vec()
            } else {
                Vec::new()
            };

            // Update only this filter.
            let mut state = prs_state.read().clone();
            if filter_idx < state.filters.len() {
                state.filters[filter_idx] = filter_data;
            }
            prs_state.set(state);

            // Record fetch time and clear in-flight.
            let mut times = filter_fetch_times.read().clone();
            if filter_idx < times.len() {
                times[filter_idx] = Some(std::time::Instant::now());
            }
            filter_fetch_times.set(times);
            let mut in_flight = filter_in_flight.read().clone();
            if filter_idx < in_flight.len() {
                in_flight[filter_idx] = false;
            }
            filter_in_flight.set(in_flight);

            // Background prefetch: sequentially fetch full details for the first N PRs.
            if !prs_for_prefetch.is_empty() {
                let octocrab_pf = Arc::clone(&octocrab);
                let api_cache_pf = api_cache.clone();
                let theme_pf = theme_clone.clone();

                smol::spawn(Compat::new(async move {
                    for pr in prs_for_prefetch {
                        // Skip PRs already in the in-component cache.
                        if detail_cache.read().contains_key(&pr.number) {
                            continue;
                        }
                        let Some(ref repo) = pr.repo else { continue };
                        let (owner, repo_name) = (repo.owner.clone(), repo.name.clone());

                        match graphql::fetch_pr_detail(
                            &octocrab_pf,
                            &owner,
                            &repo_name,
                            pr.number,
                            api_cache_pf.as_ref(),
                        )
                        .await
                        {
                            Ok((mut detail, rl)) => {
                                if detail.behind_by.is_none()
                                    && let Some(ref head_owner) = pr.head_repo_owner
                                    && let Ok(behind) = graphql::fetch_compare(
                                        &octocrab_pf,
                                        &owner,
                                        &repo_name,
                                        &pr.base_ref,
                                        head_owner,
                                        &pr.head_ref,
                                    )
                                    .await
                                {
                                    detail.behind_by = behind;
                                }

                                if rl.is_some() {
                                    rate_limit_state.set(rl);
                                }

                                let update_cell = update_cell_from_detail(&detail, &theme_pf);
                                let mut state = prs_state.read().clone();
                                'pf: for fd in &mut state.filters {
                                    if let Some(idx) =
                                        fd.prs.iter().position(|p| p.number == pr.number)
                                    {
                                        fd.rows[idx].insert("update".to_owned(), update_cell);
                                        break 'pf;
                                    }
                                }
                                prs_state.set(state);

                                let mut cache = detail_cache.read().clone();
                                cache.insert(pr.number, detail);
                                detail_cache.set(cache);
                            }
                            Err(e) => {
                                tracing::debug!("prefetch failed for PR #{}: {e:#}", pr.number);
                                if rate_limit::is_rate_limited(&e) {
                                    tracing::warn!("rate-limited during prefetch, stopping");
                                    break;
                                }
                            }
                        }
                    }
                }))
                .detach();
            }
        }))
        .detach();
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
    let visible_rows = (props.height.saturating_sub(5) / 2) as usize;

    // Clone octocrab and cache for action closures.
    let octocrab_for_actions = props.octocrab.map(Arc::clone);
    let api_cache_for_refresh = props.api_cache.cloned();
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
                    InputMode::Assign => {
                        handle_assign_input(
                            code,
                            modifiers,
                            input_mode,
                            input_buffer,
                            action_status,
                            &prs_state,
                            current_filter_idx,
                            cursor.get(),
                            octocrab_for_actions.as_ref(),
                            assignee_candidates,
                            assignee_selection,
                        );
                    }
                    InputMode::Comment => match code {
                        // Submit comment with Ctrl+D.
                        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                            let comment_text = input_buffer.read().clone();
                            if !comment_text.is_empty()
                                && let Some(ref octocrab) = octocrab_for_actions
                            {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_filter_idx,
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
                                    current_filter_idx,
                                    cursor.get(),
                                );
                                if let Some((owner, repo, number)) = pr_info {
                                    let octocrab = Arc::clone(octocrab);
                                    let action = pending.clone();
                                    let action_name = match pending {
                                        PendingAction::Close => "Closed",
                                        PendingAction::Reopen => "Reopened",
                                        PendingAction::Merge => "Merged",
                                        PendingAction::Approve => "Approved",
                                        PendingAction::UpdateBranch => "Updated branch for",
                                        PendingAction::ReadyForReview => "Marked",
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
                                            PendingAction::Approve => {
                                                pr_actions::approve(&octocrab, &owner, &repo, number, None)
                                                    .await
                                            }
                                            PendingAction::UpdateBranch => {
                                                pr_actions::update_branch(&octocrab, &owner, &repo, number)
                                                    .await
                                            }
                                            PendingAction::ReadyForReview => {
                                                pr_actions::ready_for_review(&octocrab, &owner, &repo, number)
                                                    .await
                                            }
                                        };
                                        match result {
                                            Ok(()) => {
                                                let msg = match action {
                                                    PendingAction::ReadyForReview => {
                                                        format!("{action_label} PR #{number} ready for review")
                                                    }
                                                    _ => format!("{action_label} PR #{number}"),
                                                };
                                                action_status.set(Some(msg));
                                            }
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
                        // Toggle preview pane
                        KeyCode::Char('p') => {
                            preview_open.set(!preview_open.get());
                            preview_scroll.set(0);
                        }
                        // --- PR Actions (T061) ---
                        // Approve (with confirmation)
                        KeyCode::Char('v') => {
                            input_mode.set(InputMode::Confirm(PendingAction::Approve));
                            action_status.set(None);
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
                        // Update branch — prefer refined detail-cache status, fall back to coarse.
                        KeyCode::Char('u') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            let (pr_number, coarse) = {
                                let state = prs_state.read();
                                let pr = state
                                    .filters
                                    .get(current_filter_idx)
                                    .and_then(|f| f.prs.get(cursor.get()));
                                (pr.map(|p| p.number), pr.map(branch_update_status))
                            };
                            let effective = pr_number.and_then(|num| {
                                let cache = detail_cache.read();
                                cache.get(&num).and_then(effective_update_status)
                            });
                            match effective {
                                Some(MergeStateStatus::Clean) => {
                                    action_status
                                        .set(Some("Branch is already up-to-date".into()));
                                }
                                Some(MergeStateStatus::Dirty) => {
                                    action_status.set(Some(
                                        "Cannot auto-update: branch has conflicts".into(),
                                    ));
                                }
                                Some(MergeStateStatus::Behind) => {
                                    input_mode.set(InputMode::UpdateBranchMethod);
                                    action_status.set(None);
                                }
                                _ => {
                                    // Refined status unavailable; use coarse signal.
                                    match coarse {
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
                                                PendingAction::UpdateBranch,
                                            ));
                                            action_status.set(None);
                                        }
                                    }
                                }
                            }
                        }
                        // Ready for review (with confirmation)
                        KeyCode::Char('W') => {
                            input_mode.set(InputMode::Confirm(PendingAction::ReadyForReview));
                            action_status.set(None);
                        }
                        // Diff (plain d, not Ctrl+d)
                        KeyCode::Char('d') if !modifiers.contains(KeyModifiers::CONTROL) => {
                            let pr_info =
                                get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
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
                                prs_state.read().filters.get(current_filter_idx).cloned();
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
                        // Quick self-assign (Ctrl+a) - immediate action without text input
                        KeyCode::Char('a') if modifiers.contains(KeyModifiers::CONTROL) => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_filter_idx,
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
                                                &[user.login],
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
                        // Assign (text input for any user)
                        KeyCode::Char('a') => {
                            input_mode.set(InputMode::Assign);
                            assignee_selection.set(0);
                            action_status.set(None);

                            // Get current PR and build candidates from its data
                            let (current_assignees, candidates) = {
                                let pr_info = get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
                                if let Some((_owner, _repo, number)) = pr_info {
                                    let state = prs_state.read();
                                    let pr = state.filters
                                        .get(current_filter_idx)
                                        .and_then(|s| s.prs.iter().find(|p| p.number == number));

                                    if let Some(pr) = pr {
                                        let assignees = pr.assignees
                                            .iter()
                                            .map(|a| a.login.as_str())
                                            .collect::<Vec<_>>()
                                            .join(", ");

                                        // Add trailing ", " to show all suggestions immediately
                                        let assignees_str = if assignees.is_empty() {
                                            String::new()
                                        } else {
                                            format!("{assignees}, ")
                                        };

                                        // Build candidates from PR data (instant, no API call)
                                        let candidates = build_pr_assignee_candidates(pr);

                                        (assignees_str, candidates)
                                    } else {
                                        (String::new(), Vec::new())
                                    }
                                } else {
                                    (String::new(), Vec::new())
                                }
                            };

                            input_buffer.set(current_assignees);
                            assignee_candidates.set(candidates);  // Set immediately, no async fetch
                        }
                        // Unassign (self)
                        KeyCode::Char('A') => {
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_filter_idx,
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
                                get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
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
                                get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
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
                                get_current_pr_info(&prs_state, current_filter_idx, cursor.get());
                            if let Some((owner, repo, number)) = pr_info {
                                let url =
                                    format!("https://github.com/{owner}/{repo}/pull/{number}");
                                match clipboard::open_in_browser(&url) {
                                    Ok(()) => action_status.set(Some(format!("Opened #{number}"))),
                                    Err(e) => action_status.set(Some(format!("Open failed: {e}"))),
                                }
                            }
                        }
                        // Retry / refresh (active filter only)
                        KeyCode::Char('r') => {
                            if let Some(c) = &api_cache_for_refresh {
                                c.invalidate_all();
                            }
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
                            }
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            if filter_count > 0 {
                                active_filter.set((active_filter.get() + 1) % filter_count);
                                cursor.set(0);
                                scroll_offset.set(0);
                                preview_scroll.set(0);
                            }
                        }
                        _ => {}
                    },
                    InputMode::UpdateBranchMethod => match code {
                        // Merge-update (only merge strategy supported).
                        KeyCode::Char('m' | 'M') => {
                            // Reuse Confirm flow to actually trigger the update.
                            if let Some(ref octocrab) = octocrab_for_actions {
                                let pr_info = get_current_pr_info(
                                    &prs_state,
                                    current_filter_idx,
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
                                            Ok(()) => action_status
                                                .set(Some(format!("Updated branch for PR #{number}"))),
                                            Err(e) => action_status
                                                .set(Some(format!("Update branch failed: {e}"))),
                                        }
                                    }))
                                    .detach();
                                }
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
    let tabs: Vec<Tab> = filters_cfg
        .iter()
        .enumerate()
        .map(|(i, s)| Tab {
            title: s.title.clone(),
            count: state_ref.filters.get(i).map(|d| d.pr_count),
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
                if let Some(repo_ref) = &pr.repo
                    && let Some(octocrab) = props.octocrab
                {
                    pending_detail.set(Some(DetailRequest {
                        octocrab: Arc::clone(octocrab),
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

        // Build meta header for Overview tab.
        let sidebar_meta = if current_tab == SidebarTab::Overview {
            current_pr.map(|pr| build_sidebar_meta(pr, detail_for_pr, &theme, depth))
        } else {
            None
        };

        // Account for tab bar (2 extra lines) + meta (3 lines) in sidebar height.
        let meta_lines = if sidebar_meta.is_some() { 4 } else { 0 };
        let sidebar_visible_lines = props.height.saturating_sub(9 + meta_lines) as usize;

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
            sidebar_meta,
            None, // Show all tabs for PRs
        ))
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
    );

    // Build footer or input area based on mode.
    let current_mode = input_mode.read().clone();

    let rendered_text_input = match &current_mode {
        InputMode::Assign => {
            let buf = input_buffer.read().clone();
            let (_prefix, current_word, _) = extract_current_word(&buf);
            let candidates = assignee_candidates.read();
            let filtered =
                crate::components::text_input::filter_suggestions(&candidates, current_word);
            let sel = assignee_selection.get();
            let selected_idx = if filtered.is_empty() {
                None
            } else {
                Some(sel.min(filtered.len().saturating_sub(1)))
            };
            Some(RenderedTextInput::build_with_suggestions(
                "Assign users (comma-separated):",
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
                PendingAction::Close => "Close this PR? (y/n)",
                PendingAction::Reopen => "Reopen this PR? (y/n)",
                PendingAction::Merge => "Merge this PR? (y/n)",
                PendingAction::Approve => "Approve this PR? (y/n)",
                PendingAction::UpdateBranch => "Update branch from base? (y/n)",
                PendingAction::ReadyForReview => "Mark this draft PR ready for review? (y/n)",
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

/// Parse comma-separated usernames from input string.
/// - Splits on commas
/// - Trims whitespace
/// - Strips @ prefix if present
/// - Handles @me syntax (replaces with current user login)
/// - Deduplicates usernames
/// - Filters empty strings
///
/// Returns: `(parsed_usernames, needs_current_user_fetch)`
/// The bool indicates if @me was found and we need to fetch `current().user()`
fn parse_assignee_input(input: &str) -> (Vec<String>, bool) {
    let mut needs_me = false;
    let usernames: Vec<String> = input
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let cleaned = s.strip_prefix('@').unwrap_or(s);
            if cleaned.eq_ignore_ascii_case("me") {
                needs_me = true;
                "@me".to_string() // Placeholder, will be replaced
            } else {
                cleaned.to_string()
            }
        })
        .collect();

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    let deduped: Vec<String> = usernames
        .into_iter()
        .filter(|u| seen.insert(u.clone()))
        .collect();

    (deduped, needs_me)
}

/// Extract the "current word" being typed for autocomplete.
///
/// Example: `"alice, bob, ch"` → `("alice, bob, ", "ch", 2)`
///          Returns: `(prefix, current_word, word_index)`
///
/// Used for filtering autocomplete suggestions based only on the last username.
fn extract_current_word(input: &str) -> (&str, &str, usize) {
    if let Some(last_comma_pos) = input.rfind(',') {
        let prefix = &input[..=last_comma_pos]; // "alice, bob, "
        let current = input[last_comma_pos + 1..].trim_start(); // "ch"
        let word_count = input[..=last_comma_pos].matches(',').count();
        (prefix, current, word_count)
    } else {
        // No comma yet - entire input is one word
        ("", input.trim(), 0)
    }
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
    mut action_status: State<Option<String>>,
    prs_state: &State<PrsState>,
    filter_idx: usize,
    cursor: usize,
    octocrab_for_actions: Option<&Arc<Octocrab>>,
    assignee_candidates: State<Vec<String>>,
    mut assignee_selection: State<usize>,
) {
    match code {
        // Navigate suggestions: Tab/Down moves down, Up/BackTab moves up
        KeyCode::Tab | KeyCode::Down => {
            let buf = input_buffer.read().clone();
            let (_prefix, current_word, _word_idx) = extract_current_word(&buf);
            let candidates = assignee_candidates.read();
            let filtered =
                crate::components::text_input::filter_suggestions(&candidates, current_word);
            if !filtered.is_empty() {
                assignee_selection.set((assignee_selection.get() + 1) % filtered.len());
            }
        }
        KeyCode::Up | KeyCode::BackTab => {
            let buf = input_buffer.read().clone();
            let (_prefix, current_word, _) = extract_current_word(&buf);
            let candidates = assignee_candidates.read();
            let filtered =
                crate::components::text_input::filter_suggestions(&candidates, current_word);
            if !filtered.is_empty() {
                let sel = assignee_selection.get();
                assignee_selection.set(if sel == 0 {
                    filtered.len() - 1
                } else {
                    sel - 1
                });
            }
        }
        KeyCode::Enter => {
            let buf = input_buffer.read().clone();
            let (prefix, current_word, _) = extract_current_word(&buf);
            let candidates = assignee_candidates.read();
            let filtered =
                crate::components::text_input::filter_suggestions(&candidates, current_word);

            // If user selected from suggestions, replace current word with selection
            // Otherwise, keep the buffer as-is
            let final_input = if filtered.is_empty() {
                buf.clone() // No suggestions, use typed input
            } else {
                let sel = assignee_selection
                    .get()
                    .min(filtered.len().saturating_sub(1));
                format!("{}{}", prefix, filtered[sel]) // Reconstruct: prefix + selected username
            };

            if !final_input.trim().is_empty()
                && let Some(octocrab) = octocrab_for_actions
            {
                let info = get_current_pr_info(prs_state, filter_idx, cursor);
                if let Some((owner, repo, number)) = info {
                    let octocrab = Arc::clone(octocrab);
                    let (mut usernames, needs_me) = parse_assignee_input(&final_input);

                    smol::spawn(Compat::new(async move {
                        let result = async {
                            // Replace @me with current user
                            if needs_me {
                                let user = octocrab.current().user().await?;
                                for username in &mut usernames {
                                    if username == "@me" {
                                        username.clone_from(&user.login);
                                    }
                                }
                            }

                            pr_actions::assign(&octocrab, &owner, &repo, number, &usernames).await
                        }
                        .await;

                        let count = usernames.len();
                        match result {
                            Ok(()) if count == 1 => action_status
                                .set(Some(format!("Assigned {} to PR #{number}", usernames[0]))),
                            Ok(()) => action_status
                                .set(Some(format!("Assigned {count} users to PR #{number}"))),
                            Err(e) => action_status.set(Some(format!("Assign failed: {e}"))),
                        }
                    }))
                    .detach();
                }
            }
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            assignee_selection.set(0);
        }
        KeyCode::Esc => {
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            assignee_selection.set(0);
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
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            // Backward compatibility: Ctrl+D also submits
            let buf = input_buffer.read().clone();
            let (prefix, current_word, _) = extract_current_word(&buf);
            let candidates = assignee_candidates.read();
            let filtered =
                crate::components::text_input::filter_suggestions(&candidates, current_word);

            // If user selected from suggestions, replace current word with selection
            // Otherwise, keep the buffer as-is
            let final_input = if filtered.is_empty() {
                buf.clone() // No suggestions, use typed input
            } else {
                let sel = assignee_selection
                    .get()
                    .min(filtered.len().saturating_sub(1));
                format!("{}{}", prefix, filtered[sel]) // Reconstruct: prefix + selected username
            };

            if !final_input.trim().is_empty()
                && let Some(octocrab) = octocrab_for_actions
            {
                let info = get_current_pr_info(prs_state, filter_idx, cursor);
                if let Some((owner, repo, number)) = info {
                    let octocrab = Arc::clone(octocrab);
                    let (mut usernames, needs_me) = parse_assignee_input(&final_input);

                    smol::spawn(Compat::new(async move {
                        let result = async {
                            // Replace @me with current user
                            if needs_me {
                                let user = octocrab.current().user().await?;
                                for username in &mut usernames {
                                    if username == "@me" {
                                        username.clone_from(&user.login);
                                    }
                                }
                            }

                            pr_actions::assign(&octocrab, &owner, &repo, number, &usernames).await
                        }
                        .await;

                        let count = usernames.len();
                        match result {
                            Ok(()) if count == 1 => action_status
                                .set(Some(format!("Assigned {} to PR #{number}", usernames[0]))),
                            Ok(()) => action_status
                                .set(Some(format!("Assigned {count} users to PR #{number}"))),
                            Err(e) => action_status.set(Some(format!("Assign failed: {e}"))),
                        }
                    }))
                    .detach();
                }
            }
            input_mode.set(InputMode::Normal);
            input_buffer.set(String::new());
            assignee_selection.set(0);
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

/// Build the `SidebarMeta` header from a pull request.
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

    // Update status: prefer confirmed detail info; fall back to coarse status from search.
    let (update_text, update_fg_app) = if let Some(d) = detail {
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
    };

    SidebarMeta {
        pill_icon,
        pill_text,
        pill_bg: pill_bg_app.to_crossterm_color(depth),
        pill_fg: theme.pill_fg.to_crossterm_color(depth),
        pill_left: icons.pill_left.clone(),
        pill_right: icons.pill_right.clone(),
        branch_text,
        branch_fg: theme.pill_branch.to_crossterm_color(depth),
        role_icon,
        role_text,
        role_fg: theme.pill_role.to_crossterm_color(depth),
        update_text,
        update_fg: update_fg_app.to_crossterm_color(depth),
        participants,
        participants_fg: theme.text_actor.to_crossterm_color(depth),
    }
}

/// Fallback theme when none is provided.
fn default_theme() -> ResolvedTheme {
    super::default_theme()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::graphql::PrDetail;
    use crate::github::types::{MergeableState, PullRequest};

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
