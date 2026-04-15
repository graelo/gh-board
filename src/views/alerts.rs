// Alerts view — security alerts dashboard.
//
// Displays GitHub security alerts (Dependabot, Code Scanning, Secret Scanning)
// in a filterable, navigable table with sidebar detail.
//
// This view is modeled on `ActionsView` and reuses the same component primitives
// (tab bar, scrollable table, sidebar, footer, help overlay, text input).

use std::collections::{HashMap, HashSet};

use iocraft::prelude::*;

use crate::actions::clipboard;
use crate::app::ViewKind;
use crate::color::{Color as AppColor, ColorDepth};
use crate::components::footer::{self, ActionFeedback, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::sidebar::{RenderedSidebar, Sidebar, SidebarMeta, SidebarTab};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::components::text_input::{RenderedTextInput, TextInput};
use crate::config::keybindings::{
    BuiltinAction, MergedBindings, ResolvedBinding, TemplateVars, ViewContext,
    execute_shell_command, expand_template, key_event_to_string,
};
use crate::config::types::AlertsFilter;
use crate::engine::{EngineHandle, Event, FilterConfig, Request};
use crate::markdown::renderer::{StyledLine, StyledSpan};
use crate::theme::ResolvedTheme;
use crate::types::{
    AlertCategory, AlertDetail, AlertSeverity, AlertState, RateLimitInfo, SecretLocation,
    SecurityAlert,
};

// ---------------------------------------------------------------------------
// Column definitions
// ---------------------------------------------------------------------------

fn alerts_columns() -> Vec<Column> {
    vec![
        Column {
            id: "number".to_owned(),
            header: "#".to_owned(),
            default_width_pct: 0.05,
            align: TextAlign::Right,
            fixed_width: Some(6),
        },
        Column {
            id: "severity".to_owned(),
            header: "Severity".to_owned(),
            default_width_pct: 0.08,
            align: TextAlign::Left,
            fixed_width: Some(10),
        },
        Column {
            id: "category".to_owned(),
            header: "Category".to_owned(),
            default_width_pct: 0.10,
            align: TextAlign::Left,
            fixed_width: Some(14),
        },
        Column {
            id: "pkg_rule".to_owned(),
            header: "Package/Rule".to_owned(),
            default_width_pct: 0.30,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "state".to_owned(),
            header: "State".to_owned(),
            default_width_pct: 0.08,
            align: TextAlign::Left,
            fixed_width: Some(10),
        },
        Column {
            id: "repo".to_owned(),
            header: "Repo".to_owned(),
            default_width_pct: 0.20,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "age".to_owned(),
            header: "Age".to_owned(),
            default_width_pct: 0.08,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
    ]
}

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

fn severity_color(severity: AlertSeverity, theme: &ResolvedTheme) -> AppColor {
    match severity {
        AlertSeverity::Critical => theme.text_error,
        AlertSeverity::High => AppColor::Ansi256(208), // orange
        AlertSeverity::Medium => theme.text_warning,
        AlertSeverity::Low => theme.text_secondary,
        AlertSeverity::Unknown => theme.text_faint,
    }
}

fn state_color(state: AlertState, theme: &ResolvedTheme) -> AppColor {
    match state {
        AlertState::Open => theme.text_success,
        AlertState::Fixed => theme.text_secondary,
        AlertState::Dismissed | AlertState::AutoDismissed | AlertState::Unknown => theme.text_faint,
    }
}

// ---------------------------------------------------------------------------
// Row building
// ---------------------------------------------------------------------------

fn alert_to_row(alert: &SecurityAlert, theme: &ResolvedTheme) -> Row {
    let mut row = HashMap::new();
    row.insert(
        "number".to_owned(),
        Cell::colored(format!("#{}", alert.number), theme.text_faint),
    );
    row.insert(
        "severity".to_owned(),
        Cell::colored(
            alert.severity.to_string(),
            severity_color(alert.severity, theme),
        ),
    );
    row.insert(
        "category".to_owned(),
        Cell::colored(alert.category.to_string(), theme.text_secondary),
    );
    row.insert(
        "pkg_rule".to_owned(),
        Cell::plain(alert.package_or_rule.clone()),
    );
    row.insert(
        "state".to_owned(),
        Cell::colored(alert.state.to_string(), state_color(alert.state, theme)),
    );
    row.insert(
        "repo".to_owned(),
        Cell::colored(alert.repo.clone(), theme.text_secondary),
    );
    let age = crate::util::format_date(&alert.created_at, "relative");
    row.insert("age".to_owned(), Cell::colored(age, theme.text_faint));
    row
}

// ---------------------------------------------------------------------------
// Filter data per tab
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct FilterData {
    rows: Vec<Row>,
    alerts: Vec<SecurityAlert>,
    alert_count: usize,
    loading: bool,
    error: Option<String>,
}

impl Default for FilterData {
    fn default() -> Self {
        Self {
            rows: Vec::new(),
            alerts: Vec::new(),
            alert_count: 0,
            loading: true,
            error: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Input mode
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    Search,
}

// ---------------------------------------------------------------------------
// Navigator
// ---------------------------------------------------------------------------

const NAV_W: u16 = 28;

#[derive(Clone)]
enum NavItem {
    All,
    Category(AlertCategory),
    Tool { name: String },
}

impl NavItem {
    fn label(&self) -> String {
        match self {
            Self::All => "All".to_owned(),
            Self::Category(c) => format!("{c}"),
            Self::Tool { name } => format!("  {name}"),
        }
    }
}

/// Build navigator items dynamically from the current filter's alerts.
fn build_nav_items(alerts: &[SecurityAlert]) -> Vec<NavItem> {
    let mut items = vec![NavItem::All];
    items.push(NavItem::Category(AlertCategory::Dependabot));
    items.push(NavItem::Category(AlertCategory::CodeScanning));
    // Under CodeScanning: collect unique tool names.
    let tool_names: Vec<String> = alerts
        .iter()
        .filter_map(|a| {
            if let AlertDetail::CodeScanning { ref tool_name, .. } = a.detail {
                Some(tool_name.clone())
            } else {
                None
            }
        })
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    // BTreeSet iteration is already sorted — no explicit sort needed.
    for name in tool_names {
        items.push(NavItem::Tool { name });
    }
    items.push(NavItem::Category(AlertCategory::SecretScanning));
    items
}

/// Filter alerts by the selected nav item.
fn nav_filter(alert: &SecurityAlert, item: &NavItem) -> bool {
    match item {
        NavItem::All => true,
        NavItem::Category(cat) => alert.category == *cat,
        NavItem::Tool { name } => {
            alert.category == AlertCategory::CodeScanning
                && matches!(&alert.detail, AlertDetail::CodeScanning { tool_name, .. } if tool_name == name)
        }
    }
}

// ---------------------------------------------------------------------------
// Sidebar tab
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlertSidebarTab {
    Overview,
    Detail,
}

/// The two `SidebarTab` variants we reuse for the alerts sidebar.
const ALERTS_SIDEBAR_TABS: &[SidebarTab] = &[SidebarTab::Overview, SidebarTab::Activity];

impl AlertSidebarTab {
    fn label(self, category: Option<&AlertCategory>) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Detail => match category {
                Some(AlertCategory::Dependabot) => "Remediation",
                Some(AlertCategory::CodeScanning) => "Instances",
                Some(AlertCategory::SecretScanning) => "Locations",
                None => "Detail",
            },
        }
    }

    /// Map to the `SidebarTab` enum used by `build_tabbed()`.
    fn to_sidebar_tab(self) -> SidebarTab {
        match self {
            Self::Overview => SidebarTab::Overview,
            Self::Detail => SidebarTab::Activity, // repurposed, label overridden
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Overview => Self::Detail,
            Self::Detail => Self::Overview,
        }
    }

    fn prev(self) -> Self {
        self.next()
    }
}

// ---------------------------------------------------------------------------
// Sidebar content builders
// ---------------------------------------------------------------------------

fn build_alert_sidebar_meta(
    alert: &SecurityAlert,
    theme: &ResolvedTheme,
    depth: ColorDepth,
) -> SidebarMeta {
    let icons = &theme.icons;

    // Pill: severity badge
    let (pill_text, pill_bg) = match alert.severity {
        AlertSeverity::Critical => ("Critical".to_owned(), theme.text_error),
        AlertSeverity::High => ("High".to_owned(), theme.text_warning),
        AlertSeverity::Medium => ("Medium".to_owned(), theme.text_warning),
        AlertSeverity::Low => ("Low".to_owned(), theme.text_secondary),
        AlertSeverity::Unknown => ("Unknown".to_owned(), theme.text_faint),
    };

    // "Branch" line: category (+ tool if code scanning)
    let branch_text = match &alert.detail {
        AlertDetail::CodeScanning { tool_name, .. } => {
            format!("{} · {tool_name}", alert.category)
        }
        _ => alert.category.to_string(),
    };

    // State as the "update" text
    let (update_text, update_fg) = match alert.state {
        AlertState::Open => (Some("Open".to_owned()), theme.text_success),
        AlertState::Fixed => (Some("Fixed".to_owned()), theme.text_secondary),
        AlertState::Dismissed | AlertState::AutoDismissed => {
            (Some("Dismissed".to_owned()), theme.text_faint)
        }
        AlertState::Unknown => (None, theme.text_faint),
    };

    let fmt = "%Y-%m-%d %H:%M:%S";
    let created_text = alert
        .created_at
        .with_timezone(&chrono::Local)
        .format(fmt)
        .to_string();
    let created_age = crate::util::format_date(&alert.created_at, "relative");

    SidebarMeta {
        pill_icon: icons.view_alerts.clone(),
        pill_text,
        pill_bg: pill_bg.to_crossterm_color(depth),
        pill_fg: theme.pill_fg.to_crossterm_color(depth),
        pill_left: icons.pill_left.clone(),
        pill_right: icons.pill_right.clone(),
        branch_text,
        branch_fg: theme.pill_branch.to_crossterm_color(depth),
        update_text,
        update_fg: update_fg.to_crossterm_color(depth),
        author_login: alert.package_or_rule.clone(),
        role_icon: String::new(),
        role_text: alert.repo.clone(),
        role_fg: theme.text_role.to_crossterm_color(depth),
        label_fg: theme.text_secondary.to_crossterm_color(depth),
        participants: vec![],
        participants_fg: theme.text_actor.to_crossterm_color(depth),
        labels_text: None,
        assignees_text: None,
        created_text,
        created_age,
        // Alerts have no separate updated_at — reuse created
        updated_text: String::new(),
        updated_age: String::new(),
        lines_added: None,
        lines_deleted: None,
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

fn build_overview_lines(
    alert: &SecurityAlert,
    theme: &ResolvedTheme,
    depth: ColorDepth,
) -> Vec<StyledLine> {
    // The summary is the main body content — render as styled text.
    if alert.summary.is_empty() {
        return vec![StyledLine::from_span(StyledSpan::text(
            "No description available.",
            theme.text_faint,
        ))];
    }

    // Render the summary via the markdown renderer for proper formatting
    crate::markdown::renderer::render_markdown(&alert.summary, theme, depth)
}

#[allow(clippy::too_many_lines)]
fn build_detail_lines(
    alert: &SecurityAlert,
    locations_cache: &HashMap<u64, Vec<SecretLocation>>,
    theme: &ResolvedTheme,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    match &alert.detail {
        AlertDetail::Dependabot {
            ecosystem,
            ghsa_id,
            cve_id,
            vulnerable_version_range,
            patched_version,
        } => {
            lines.push(StyledLine::from_spans(vec![
                StyledSpan::text("Ecosystem: ", theme.text_faint),
                StyledSpan::text(ecosystem.clone(), theme.text_primary),
            ]));
            if !ghsa_id.is_empty() {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("GHSA ID: ", theme.text_faint),
                    StyledSpan::text(ghsa_id.clone(), theme.text_primary),
                ]));
            }
            if let Some(cve) = cve_id {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("CVE ID: ", theme.text_faint),
                    StyledSpan::text(cve.clone(), theme.text_primary),
                ]));
            }
            if let Some(range) = vulnerable_version_range {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("Vulnerable range: ", theme.text_faint),
                    StyledSpan::text(range.clone(), theme.text_error),
                ]));
            }
            if let Some(patched) = patched_version {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("Patched version: ", theme.text_faint),
                    StyledSpan::text(patched.clone(), theme.text_success),
                ]));
            }
        }
        AlertDetail::CodeScanning {
            tool_name,
            tool_version,
            rule_id,
            rule_description,
            instances,
        } => {
            lines.push(StyledLine::from_spans(vec![
                StyledSpan::text("Tool: ", theme.text_faint),
                StyledSpan::text(tool_name.clone(), theme.text_primary),
            ]));
            if let Some(ver) = tool_version {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("Version: ", theme.text_faint),
                    StyledSpan::text(ver.clone(), theme.text_faint),
                ]));
            }
            lines.push(StyledLine::from_spans(vec![
                StyledSpan::text("Rule: ", theme.text_faint),
                StyledSpan::text(rule_id.clone(), theme.text_primary),
            ]));
            if !rule_description.is_empty() {
                lines.push(StyledLine::from_spans(vec![]));
                lines.push(StyledLine::from_span(StyledSpan::text(
                    rule_description.clone(),
                    theme.text_secondary,
                )));
            }
            if !instances.is_empty() {
                lines.push(StyledLine::from_spans(vec![]));
                lines.push(StyledLine::from_span(StyledSpan::text(
                    format!("Instances ({}):", instances.len()),
                    theme.text_faint,
                )));
                for inst in instances {
                    let path = inst.path.as_deref().unwrap_or("<unknown>");
                    let start = inst.start_line.unwrap_or(0);
                    let end = inst.end_line.unwrap_or(start);
                    lines.push(StyledLine::from_spans(vec![
                        StyledSpan::text("  ", theme.text_faint),
                        StyledSpan::text(format!("{path}:{start}-{end}"), theme.text_secondary),
                    ]));
                }
            }
        }
        AlertDetail::SecretScanning {
            secret_type,
            secret_type_display_name,
            validity,
            resolution,
        } => {
            lines.push(StyledLine::from_spans(vec![
                StyledSpan::text("Secret type: ", theme.text_faint),
                StyledSpan::text(secret_type.clone(), theme.text_primary),
            ]));
            if !secret_type_display_name.is_empty() {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("Display name: ", theme.text_faint),
                    StyledSpan::text(secret_type_display_name.clone(), theme.text_primary),
                ]));
            }
            if let Some(val) = validity {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("Validity: ", theme.text_faint),
                    StyledSpan::text(val.clone(), theme.text_warning),
                ]));
            }
            if let Some(res) = resolution {
                lines.push(StyledLine::from_spans(vec![
                    StyledSpan::text("Resolution: ", theme.text_faint),
                    StyledSpan::text(res.clone(), theme.text_secondary),
                ]));
            }

            // Show cached locations if available.
            if let Some(locs) = locations_cache.get(&alert.number) {
                lines.push(StyledLine::from_spans(vec![]));
                lines.push(StyledLine::from_span(StyledSpan::text(
                    format!("Locations ({}):", locs.len()),
                    theme.text_faint,
                )));
                for loc in locs {
                    let path = loc.path.as_deref().unwrap_or("<unknown>");
                    let start = loc.start_line.unwrap_or(0);
                    let end = loc.end_line.unwrap_or(start);
                    lines.push(StyledLine::from_spans(vec![
                        StyledSpan::text("  ", theme.text_faint),
                        StyledSpan::text(format!("{path}:{start}-{end}"), theme.text_secondary),
                    ]));
                }
            } else {
                lines.push(StyledLine::from_spans(vec![]));
                lines.push(StyledLine::from_span(StyledSpan::text(
                    "No locations loaded yet",
                    theme.text_faint,
                )));
            }
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve `@current` to the active scope repo, or return the literal value.
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

// ---------------------------------------------------------------------------
// AlertsView component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct AlertsViewProps<'a> {
    pub filters: Option<&'a [AlertsFilter]>,
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
    pub date_format: Option<&'a str>,
    /// Shared rate-limit state (owned by App).
    pub rate_limit: Option<State<Option<RateLimitInfo>>>,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn AlertsView<'a>(props: &AlertsViewProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
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

    // -----------------------------------------------------------------------
    // State hooks
    // -----------------------------------------------------------------------

    let mut active_filter = hooks.use_state(|| 0usize);
    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut search_query = hooks.use_state(String::new);
    let mut help_visible = hooks.use_state(|| false);

    // Navigator
    let mut nav_open = hooks.use_state(|| false);
    let mut nav_cursor = hooks.use_state(|| 0usize);
    let mut nav_focused = hooks.use_state(|| false);

    // Sidebar
    let mut preview_open = hooks.use_state(|| false);
    let mut preview_scroll = hooks.use_state(|| 0usize);
    let mut sidebar_tab = hooks.use_state(|| AlertSidebarTab::Overview);

    // Alerts data: one FilterData per filter tab
    let initial_filters = vec![FilterData::default(); filter_count];
    let mut alerts_state = hooks.use_state(move || initial_filters);

    let mut filter_in_flight = hooks.use_state(move || vec![false; filter_count]);
    let mut filter_fetch_times =
        hooks.use_state(move || vec![Option::<std::time::Instant>::None; filter_count]);

    // Secret locations cache for sidebar Locations tab
    let mut locations_cache = hooks.use_state(HashMap::<u64, Vec<SecretLocation>>::new);
    let mut locations_in_flight = hooks.use_state(HashSet::<u64>::new);

    // Action feedback
    let mut action_status = hooks.use_state(|| Option::<ActionFeedback>::None);
    let mut status_set_at = hooks.use_state(|| Option::<std::time::Instant>::None);

    // Rate limit
    let fallback_rl = hooks.use_state(|| None);
    let mut rate_limit_state = props.rate_limit.unwrap_or(fallback_rl);

    let mut refresh_registered = hooks.use_state(|| false);
    let mut refresh_all = hooks.use_state(|| false);

    // Event channel
    let event_channel = hooks.use_state(super::common::new_event_channel);
    let (event_tx, event_rx_arc) = event_channel.read().clone();
    let engine: Option<EngineHandle> = props.engine.cloned();

    // Track scope changes: when scope_repo changes, invalidate all filters.
    let mut last_scope = hooks.use_state(|| scope_repo.clone());
    if *last_scope.read() != scope_repo {
        last_scope.set(scope_repo.clone());
        alerts_state.set(vec![FilterData::default(); filter_count]);
        filter_fetch_times.set(vec![None; filter_count]);
        filter_in_flight.set(vec![false; filter_count]);
        refresh_registered.set(false);
    }

    let current_filter_idx = active_filter.get().min(filter_count.saturating_sub(1));

    let active_needs_fetch = alerts_state
        .read()
        .get(current_filter_idx)
        .is_some_and(|d| d.loading);
    let active_in_flight = filter_in_flight
        .read()
        .get(current_filter_idx)
        .copied()
        .unwrap_or(false);

    // Register for background refresh (once per mount / scope change).
    if !refresh_registered.get()
        && let Some(ref eng) = engine
    {
        let resolved_for_refresh: Vec<AlertsFilter> = filters_cfg
            .iter()
            .map(|f| {
                if let Some(repo) =
                    resolve_filter_repo(&f.repo, scope_repo.as_deref(), detected_repo.as_deref())
                    && repo != f.repo.as_str()
                {
                    return AlertsFilter {
                        repo: repo.to_owned(),
                        ..f.clone()
                    };
                }
                f.clone()
            })
            .collect();
        eng.send(Request::RegisterRefresh {
            configs: resolved_for_refresh
                .into_iter()
                .map(FilterConfig::Alert)
                .collect(),
            notify_tx: event_tx.clone(),
        });
        refresh_registered.set(true);
    }

    // Refresh-all: re-fetch every filter tab.
    if refresh_all.get()
        && is_active
        && let Some(ref eng) = engine
    {
        refresh_all.set(false);
        for (filter_idx, cfg) in filters_cfg.iter().enumerate() {
            let Some(resolved_repo) =
                resolve_filter_repo(&cfg.repo, scope_repo.as_deref(), detected_repo.as_deref())
            else {
                tracing::debug!("alerts: skipping @current filter[{filter_idx}] — no scope repo");
                continue;
            };
            let filter = if resolved_repo == cfg.repo.as_str() {
                cfg.clone()
            } else {
                AlertsFilter {
                    repo: resolved_repo.to_owned(),
                    ..cfg.clone()
                }
            };
            super::common::set_in_flight(&mut filter_in_flight, filter_idx, true);
            eng.send(Request::FetchAlerts {
                filter_idx,
                filter,
                reply_tx: event_tx.clone(),
            });
        }
    } else if active_needs_fetch
        && !active_in_flight
        && is_active
        && let Some(ref eng) = engine
        && let Some(cfg) = filters_cfg.get(current_filter_idx)
    {
        if let Some(resolved_repo) =
            resolve_filter_repo(&cfg.repo, scope_repo.as_deref(), detected_repo.as_deref())
        {
            let filter = if resolved_repo == cfg.repo.as_str() {
                cfg.clone()
            } else {
                AlertsFilter {
                    repo: resolved_repo.to_owned(),
                    ..cfg.clone()
                }
            };
            super::common::set_in_flight(&mut filter_in_flight, current_filter_idx, true);
            eng.send(Request::FetchAlerts {
                filter_idx: current_filter_idx,
                filter,
                reply_tx: event_tx.clone(),
            });
        } else {
            // `@current` filter with no scope resolved — clear loading to show empty tab.
            tracing::debug!(
                "alerts: @current filter[{current_filter_idx}] — no scope, clearing loading state"
            );
            let mut data = alerts_state.read().clone();
            if current_filter_idx < data.len() {
                data[current_filter_idx].loading = false;
            }
            alerts_state.set(data);
        }
    }

    // Poll engine events.
    {
        let rx_for_poll = event_rx_arc.clone();
        let theme_for_poll = theme.clone();
        hooks.use_future(async move {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(100)).await;
                // Auto-clear status after 60 seconds.
                if let Some(t) = status_set_at.get()
                    && t.elapsed().as_secs() >= 60
                {
                    action_status.set(None);
                    status_set_at.set(None);
                }

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
                        Event::AlertsFetched {
                            filter_idx,
                            alerts,
                            rate_limit,
                        } => {
                            let rows: Vec<Row> = alerts
                                .iter()
                                .map(|a| alert_to_row(a, &theme_for_poll))
                                .collect();
                            let alert_count = alerts.len();
                            let filter_data = FilterData {
                                rows,
                                alerts,
                                alert_count,
                                loading: false,
                                error: None,
                            };
                            let mut data = alerts_state.read().clone();
                            if filter_idx < data.len() {
                                data[filter_idx] = filter_data;
                            }
                            alerts_state.set(data);
                            let mut times = filter_fetch_times.read().clone();
                            if filter_idx < times.len() {
                                times[filter_idx] = Some(std::time::Instant::now());
                            }
                            filter_fetch_times.set(times);
                            super::common::set_in_flight(&mut filter_in_flight, filter_idx, false);
                            if let Some(rl) = rate_limit {
                                rate_limit_state.set(Some(rl));
                            }
                        }
                        Event::SecretLocationsFetched {
                            alert_number,
                            locations,
                            ..
                        } => {
                            let mut cache = locations_cache.read().clone();
                            cache.insert(alert_number, locations);
                            locations_cache.set(cache);

                            let mut inflight = locations_in_flight.read().clone();
                            inflight.remove(&alert_number);
                            locations_in_flight.set(inflight);
                        }
                        Event::FetchError { message, .. } => {
                            action_status.set(Some(ActionFeedback::Error(format!(
                                "Fetch error: {message}"
                            ))));
                            status_set_at.set(Some(std::time::Instant::now()));
                        }
                        _ => {}
                    }
                }
            }
        });
    }

    // -----------------------------------------------------------------------
    // Pre-computed owned data for keyboard handler
    // -----------------------------------------------------------------------

    let visible_rows = (props.height.saturating_sub(5) / 2).max(1) as usize;
    let current_filter_for_kb: Option<AlertsFilter> = filters_cfg.get(current_filter_idx).cloned();

    // -----------------------------------------------------------------------
    // Keyboard handling (before render data to avoid borrow conflicts)
    // -----------------------------------------------------------------------

    let keybindings = props.keybindings.cloned();
    hooks.use_terminal_events({
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
                    InputMode::Normal => {
                        // Nav panel focused: route j/k/Enter/Esc to navigator.
                        if nav_focused.get() {
                            match code {
                                KeyCode::Char('j') | KeyCode::Down => {
                                    // Compute nav items count at runtime from state.
                                    let nil = {
                                        let st = alerts_state.read();
                                        let alerts = st
                                            .get(current_filter_idx)
                                            .map_or(&[] as &[SecurityAlert], |d| {
                                                d.alerts.as_slice()
                                            });
                                        build_nav_items(alerts).len()
                                    };
                                    nav_cursor
                                        .set((nav_cursor.get() + 1).min(nil.saturating_sub(1)));
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
                            let current_alert: Option<SecurityAlert> = get_alert_at_cursor(
                                &alerts_state,
                                current_filter_idx,
                                cursor.get(),
                            );
                            let total_rows_kb = alerts_state
                                .read()
                                .get(current_filter_idx)
                                .map_or(0, |d| d.alerts.len());
                            let vars = TemplateVars {
                                url: current_alert
                                    .as_ref()
                                    .map_or_else(String::new, |a| a.html_url.clone()),
                                number: current_alert
                                    .as_ref()
                                    .map_or_else(String::new, |a| a.number.to_string()),
                                repo_name: current_filter_for_kb
                                    .as_ref()
                                    .map_or_else(String::new, |f| f.repo.clone()),
                                ..Default::default()
                            };
                            match keybindings
                                .as_ref()
                                .and_then(|kb| kb.resolve(&key_str, ViewContext::Alerts))
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
                                        if let Some(ref alert) = current_alert
                                            && !alert.html_url.is_empty()
                                        {
                                            let _ = clipboard::open_in_browser(&alert.html_url);
                                        }
                                    }
                                    BuiltinAction::CopyNumber => {
                                        if let Some(ref alert) = current_alert {
                                            let text = alert.number.to_string();
                                            match clipboard::copy_to_clipboard(&text) {
                                                Ok(()) => {
                                                    action_status.set(Some(
                                                        ActionFeedback::Success(format!(
                                                            "Copied #{}",
                                                            alert.number
                                                        )),
                                                    ));
                                                    status_set_at
                                                        .set(Some(std::time::Instant::now()));
                                                }
                                                Err(e) => {
                                                    action_status.set(Some(ActionFeedback::Error(
                                                        format!("Copy failed: {e}"),
                                                    )));
                                                    status_set_at
                                                        .set(Some(std::time::Instant::now()));
                                                }
                                            }
                                        }
                                    }
                                    BuiltinAction::CopyUrl => {
                                        if let Some(ref alert) = current_alert
                                            && !alert.html_url.is_empty()
                                        {
                                            let _ = clipboard::copy_to_clipboard(&alert.html_url);
                                        }
                                    }
                                    BuiltinAction::Refresh => {
                                        let idx = current_filter_idx;
                                        let mut data = alerts_state.read().clone();
                                        if idx < data.len() {
                                            data[idx] = FilterData::default();
                                        }
                                        alerts_state.set(data);
                                        let mut times = filter_fetch_times.read().clone();
                                        if idx < times.len() {
                                            times[idx] = None;
                                        }
                                        filter_fetch_times.set(times);
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    BuiltinAction::RefreshAll => {
                                        alerts_state.set(vec![FilterData::default(); filter_count]);
                                        filter_fetch_times.set(vec![None; filter_count]);
                                        filter_in_flight.set(vec![false; filter_count]);
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
                                        let new_open = !preview_open.get();
                                        preview_open.set(new_open);
                                        if new_open {
                                            preview_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::Search => {
                                        input_mode.set(InputMode::Search);
                                        search_query.set(String::new());
                                    }
                                    BuiltinAction::MoveDown if total_rows_kb > 0 => {
                                        let new_cursor =
                                            (cursor.get() + 1).min(total_rows_kb.saturating_sub(1));
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
                                    BuiltinAction::Last if total_rows_kb > 0 => {
                                        cursor.set(total_rows_kb.saturating_sub(1));
                                        scroll_offset
                                            .set(total_rows_kb.saturating_sub(visible_rows));
                                    }
                                    BuiltinAction::PageDown if total_rows_kb > 0 => {
                                        let new_cursor = (cursor.get() + visible_rows)
                                            .min(total_rows_kb.saturating_sub(1));
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
                                        if preview_open.get() {
                                            preview_scroll.set(preview_scroll.get() + half);
                                        } else if total_rows_kb > 0 {
                                            let new_cursor = (cursor.get() + half)
                                                .min(total_rows_kb.saturating_sub(1));
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
                                        if preview_open.get() {
                                            preview_scroll
                                                .set(preview_scroll.get().saturating_sub(half));
                                        } else {
                                            let new_cursor = cursor.get().saturating_sub(half);
                                            cursor.set(new_cursor);
                                            if new_cursor < scroll_offset.get() {
                                                scroll_offset.set(new_cursor);
                                            }
                                        }
                                    }
                                    BuiltinAction::PrevFilter if filter_count > 0 => {
                                        let current = active_filter.get();
                                        active_filter.set(if current == 0 {
                                            filter_count.saturating_sub(1)
                                        } else {
                                            current - 1
                                        });
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    BuiltinAction::NextFilter if filter_count > 0 => {
                                        active_filter.set((active_filter.get() + 1) % filter_count);
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                    }
                                    _ => {}
                                },
                                Some(ResolvedBinding::ShellCommand(cmd)) => {
                                    let expanded = expand_template(&cmd, &vars);
                                    let _ = execute_shell_command(&expanded);
                                }
                                None => {
                                    // Esc: close nav -> close sidebar
                                    if key_str == "esc" {
                                        if nav_open.get() {
                                            nav_focused.set(false);
                                            nav_open.set(false);
                                        } else if preview_open.get() {
                                            preview_open.set(false);
                                        }
                                    } else if key_str == "]" {
                                        if preview_open.get() {
                                            sidebar_tab.set(sidebar_tab.get().next());
                                            preview_scroll.set(0);
                                        }
                                    } else if key_str == "[" && preview_open.get() {
                                        sidebar_tab.set(sidebar_tab.get().prev());
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

    // -----------------------------------------------------------------------
    // Compute render data (after keyboard handler to avoid borrow conflicts)
    // -----------------------------------------------------------------------

    let state_ref = alerts_state.read();
    let current_data = state_ref.get(current_filter_idx);

    let all_alerts: &[SecurityAlert] = current_data.map_or(&[], |d| d.alerts.as_slice());
    let all_rows: &[Row] = current_data.map_or(&[], |d| d.rows.as_slice());

    // Build navigator items from current alerts.
    let nav_items = build_nav_items(all_alerts);
    let nav_items_len = nav_items.len();
    let nav_cur = nav_cursor.get().min(nav_items_len.saturating_sub(1));

    // Apply search filter.
    let search_q = search_query.read().clone();
    let after_search_idx: Vec<usize> = if search_q.is_empty() {
        (0..all_rows.len()).collect()
    } else {
        let q_lower = search_q.to_lowercase();
        (0..all_alerts.len())
            .filter(|&i| {
                all_alerts[i]
                    .package_or_rule
                    .to_lowercase()
                    .contains(&q_lower)
                    || all_alerts[i].summary.to_lowercase().contains(&q_lower)
                    || all_alerts[i].repo.to_lowercase().contains(&q_lower)
                    || all_alerts[i].number.to_string().contains(&q_lower)
            })
            .collect()
    };

    // Apply navigator filter.
    let filtered_indices: Vec<usize> = if nav_cur == 0 || nav_items.is_empty() {
        after_search_idx
    } else {
        let nav_item = &nav_items[nav_cur];
        after_search_idx
            .into_iter()
            .filter(|&i| nav_filter(&all_alerts[i], nav_item))
            .collect()
    };

    let filtered_rows: Vec<Row> = filtered_indices
        .iter()
        .filter_map(|&i| all_rows.get(i))
        .cloned()
        .collect();

    let total_rows = filtered_rows.len();

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
    let (table_w, sidebar_w) = if preview_open.get() {
        let sb = (f64::from(props.width) * preview_pct).round() as u16;
        let tb = props.width.saturating_sub(nav_w).saturating_sub(sb);
        (tb, sb)
    } else {
        (props.width.saturating_sub(nav_w), 0u16)
    };

    // -----------------------------------------------------------------------
    // Build rendered components
    // -----------------------------------------------------------------------

    let tabs: Vec<Tab> = filters_cfg
        .iter()
        .enumerate()
        .map(|(i, f)| Tab {
            title: f.title.clone(),
            count: state_ref.get(i).map(|d| d.alert_count),
            is_ephemeral: false,
        })
        .collect();

    let columns = alerts_columns();
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
            Some("No security alerts found")
        } else {
            Some("No alerts match this filter")
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
        Some(theme.footer_alerts),
        Some(theme.footer_alerts),
        Some(theme.border_faint),
        &theme.icons.tab_filter,
        "", // no ephemeral icon for alerts
    );

    let current_mode = input_mode.read().clone();
    let rendered_text_input = match current_mode {
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

    // Context text.
    let context_text = if current_data.is_some_and(|d| d.loading) {
        "Fetching security alerts\u{2026}".to_owned()
    } else if let Some(err) = current_data.and_then(|d| d.error.as_ref()) {
        format!("Error: {err}")
    } else {
        let total = current_data.map_or(0, |d| d.alert_count);
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        format!("Alert {cursor_pos}/{total_rows} (of {total})")
    };

    let active_fetch_time = filter_fetch_times
        .read()
        .get(current_filter_idx)
        .copied()
        .flatten();
    let updated_text = footer::format_updated_ago(active_fetch_time);
    let rate_limit_text = footer::format_rate_limit(rate_limit_state.read().as_ref());
    let scope_label = match &scope_repo {
        Some(_) => filters_cfg
            .get(current_filter_idx)
            .map_or_else(String::new, |f| {
                resolve_filter_repo(&f.repo, scope_repo.as_deref(), detected_repo.as_deref())
                    .unwrap_or("all repos")
                    .to_owned()
            }),
        None => "all repos".to_owned(),
    };

    let rendered_footer = RenderedFooter::build(
        ViewKind::Alerts,
        &theme.icons,
        scope_label,
        context_text,
        updated_text,
        rate_limit_text,
        action_status.read().as_ref(),
        &theme,
        depth,
        [
            Some(theme.footer_prs),
            Some(theme.footer_issues),
            Some(theme.footer_actions),
            Some(theme.footer_alerts),
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
                context: ViewContext::Alerts,
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

    // Right sidebar.
    let current_alert_for_sidebar: Option<&SecurityAlert> = filtered_indices
        .get(cursor.get())
        .and_then(|&idx| all_alerts.get(idx));

    // Trigger secret locations fetch when the sidebar is open on a secret alert
    // and the Detail tab is active.
    if preview_open.get()
        && let Some(selected_alert) = current_alert_for_sidebar
        && let AlertDetail::SecretScanning { .. } = &selected_alert.detail
        && sidebar_tab.get() == AlertSidebarTab::Detail
        && !locations_cache.read().contains_key(&selected_alert.number)
        && !locations_in_flight.read().contains(&selected_alert.number)
        && let Some((owner, repo)) = selected_alert.repo.split_once('/')
        && let Some(ref eng) = engine
    {
        eng.send(Request::FetchSecretLocations {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            alert_number: selected_alert.number,
            reply_tx: event_tx.clone(),
        });
        let mut inflight = locations_in_flight.read().clone();
        inflight.insert(selected_alert.number);
        locations_in_flight.set(inflight);
    }
    let rendered_sidebar = if preview_open.get() && sidebar_w > 0 {
        let (sidebar_lines, sidebar_meta) = if let Some(alert) = current_alert_for_sidebar {
            let meta = build_alert_sidebar_meta(alert, &theme, depth);
            let lines = match sidebar_tab.get() {
                AlertSidebarTab::Overview => build_overview_lines(alert, &theme, depth),
                AlertSidebarTab::Detail => {
                    build_detail_lines(alert, &locations_cache.read(), &theme)
                }
            };
            (lines, Some(meta))
        } else {
            (
                vec![StyledLine::from_span(StyledSpan::text(
                    "No alert selected",
                    theme.text_faint,
                ))],
                None,
            )
        };
        let sidebar_title = current_alert_for_sidebar
            .map_or_else(|| "Alert".to_owned(), |a| format!("Alert #{}", a.number));

        // Build tab label overrides: Activity → category-specific detail name
        let category_for_label = current_alert_for_sidebar.map(|a| &a.category);
        let detail_label = AlertSidebarTab::Detail.label(category_for_label).to_owned();
        let tab_overrides: HashMap<SidebarTab, String> =
            [(SidebarTab::Activity, detail_label)].into_iter().collect();

        let meta_lines = sidebar_meta.as_ref().map_or(0, SidebarMeta::line_count) as usize;
        // Subtract: border(2) + title(1) + tab_bar(1) + separator(1) + meta + margin
        let sidebar_visible_lines = (props.height as usize).saturating_sub(5 + meta_lines);

        let sidebar = RenderedSidebar::build_tabbed(
            &sidebar_title,
            &sidebar_lines,
            preview_scroll.get(),
            sidebar_visible_lines,
            sidebar_w,
            depth,
            Some(theme.text_primary),
            Some(theme.border_faint),
            Some(theme.text_faint),
            Some(theme.border_primary),
            Some(sidebar_tab.get().to_sidebar_tab()),
            Some(&theme.icons),
            sidebar_meta,
            Some(ALERTS_SIDEBAR_TABS),
            Some(&tab_overrides),
        );
        if preview_scroll.get() != sidebar.clamped_scroll {
            preview_scroll.set(sidebar.clamped_scroll);
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
                // Left navigator (optional)
                #(nav_is_open.then(|| {
                    let items = nav_items.clone();
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
                                    content: "Categories",
                                    color: theme_nav.text_primary.to_crossterm_color(depth),
                                    weight: Weight::Bold,
                                    wrap: TextWrap::NoWrap,
                                )
                            }
                            #(items.into_iter().enumerate().map(|(i, item)| {
                                let is_selected = i == cur;
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
                                let label = item.label();
                                let max_len = (NAV_W as usize).saturating_sub(4);
                                let display = if label.chars().count() > max_len {
                                    let end = label
                                        .char_indices()
                                        .nth(max_len.saturating_sub(1))
                                        .map_or(label.len(), |(idx, _)| idx);
                                    format!("{}\u{2026}", &label[..end])
                                } else {
                                    label
                                };
                                element! {
                                    View(key: i, flex_direction: FlexDirection::Row, background_color: bg) {
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

fn get_alert_at_cursor(
    alerts_state: &State<Vec<FilterData>>,
    filter_idx: usize,
    cursor: usize,
) -> Option<SecurityAlert> {
    let state = alerts_state.read();
    state.get(filter_idx)?.alerts.get(cursor).cloned()
}

// ---------------------------------------------------------------------------
// Fallback theme
// ---------------------------------------------------------------------------

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}
