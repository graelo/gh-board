// Root application component and view management.
//
// This module provides the main `App` TUI component that orchestrates the
// five views (PRs, Issues, Actions, Notifications, Repo) and handles global
// key events.
//
// ## View Kind Enum
//
// `ViewKind` is the public-facing enum for view identification, used by
// the status bar and other components to determine which view is active.
use std::path::Path;

use iocraft::prelude::*;

use crate::color::ColorDepth;
use crate::components::selection_overlay::{
    RenderedSelectionOverlay, SelectionOverlay, SelectionOverlayBuildConfig, SelectionOverlayItem,
};
use crate::components::text_input::filter_suggestions;
use crate::config::keybindings::MergedBindings;
use crate::config::types::{AppConfig, Scope};
use crate::engine::EngineHandle;
use crate::icons::ResolvedIcons;
use crate::theme::ResolvedTheme;
use crate::types::{RateLimitInfo, RepoRef};
use crate::views::actions::ActionsView;
use crate::views::alerts::AlertsView;
use crate::views::issues::IssuesView;
use crate::views::notifications::NotificationsView;
use crate::views::prs::PrsView;
use crate::views::repo::RepoView;

// ---------------------------------------------------------------------------
// Navigation target (cross-view deep-link context)
// ---------------------------------------------------------------------------

/// Carries cross-view navigation context (e.g., "jump to this Actions run").
#[derive(Clone, Debug)]
pub enum NavigationTarget {
    ActionsRun {
        owner: String,
        repo: String,
        run_id: u64,
        host: Option<String>,
    },
    PullRequest {
        owner: String,
        repo: String,
        number: u64,
        host: Option<String>,
    },
    Issue {
        owner: String,
        repo: String,
        number: u64,
        host: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// View kind enum (public for status bar)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Prs,
    Issues,
    Actions,
    Alerts,
    Notifications,
    Repo,
}

impl ViewKind {
    pub const ALL: [ViewKind; 6] = [
        ViewKind::Prs,
        ViewKind::Issues,
        ViewKind::Actions,
        ViewKind::Alerts,
        ViewKind::Notifications,
        ViewKind::Repo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Prs => "PRs",
            Self::Issues => "Issues",
            Self::Actions => "Actions",
            Self::Alerts => "Alerts",
            Self::Notifications => "Notifs",
            Self::Repo => "Repo",
        }
    }

    pub fn icon_label(self, icons: &ResolvedIcons) -> String {
        match self {
            Self::Prs => format!("{} {}", icons.view_prs, self.label()),
            Self::Issues => format!("{} {}", icons.view_issues, self.label()),
            Self::Actions => format!("{} {}", icons.view_actions, self.label()),
            Self::Alerts => format!("{} {}", icons.view_alerts, self.label()),
            Self::Notifications => format!("{} {}", icons.view_notifications, self.label()),
            Self::Repo => format!("{} {}", icons.view_repo, self.label()),
        }
    }
}

// ---------------------------------------------------------------------------
// Root App component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct AppProps<'a> {
    pub config: Option<&'a AppConfig>,
    pub engine: Option<&'a EngineHandle>,
    pub theme: Option<&'a ResolvedTheme>,
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub repo_path: Option<&'a Path>,
    pub detected_repo: Option<&'a RepoRef>,
    pub initial_nav_target: Option<NavigationTarget>,
}

#[component]
pub fn App<'a>(props: &AppProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let (width, height) = hooks.use_terminal_size();
    let mut system = hooks.use_context_mut::<SystemContext>();
    let should_exit = hooks.use_state(|| false);

    let config = props.config;
    let theme = props.theme;
    let keybindings = props.keybindings;
    let depth = props.color_depth;

    // View switching state.
    let initial_view = config.map_or(ViewKind::Prs, |c| {
        match c.defaults.view.unwrap_or_default() {
            crate::config::types::View::Prs => ViewKind::Prs,
            crate::config::types::View::Issues => ViewKind::Issues,
            crate::config::types::View::Actions => ViewKind::Actions,
            crate::config::types::View::Notifications => ViewKind::Notifications,
            crate::config::types::View::Alerts => ViewKind::Alerts,
            crate::config::types::View::Repo => ViewKind::Repo,
        }
    });
    let mut active_view = hooks.use_state(move || initial_view);

    // Cross-view navigation target (deep-link).
    let initial_nav = props.initial_nav_target.clone();
    let mut nav_target: State<Option<NavigationTarget>> = hooks.use_state(move || initial_nav);
    let mut previous_view: State<Option<ViewKind>> = hooks.use_state(|| None);

    // Go-back signal: when a child view sets this to true, we return to previous view.
    let mut go_back_signal = hooks.use_state(|| false);
    if go_back_signal.get() {
        go_back_signal.set(false);
        let prev = *previous_view.read();
        nav_target.set(None);
        if let Some(prev) = prev {
            active_view.set(prev);
            previous_view.set(None);
        }
    }

    // When nav_target is set, store current view and switch to target.
    if let Some(target) = &*nav_target.read() {
        let dest = match target {
            NavigationTarget::ActionsRun { .. } => ViewKind::Actions,
            NavigationTarget::PullRequest { .. } => ViewKind::Prs,
            NavigationTarget::Issue { .. } => ViewKind::Issues,
        };
        if active_view.get() != dest {
            previous_view.set(Some(active_view.get()));
            active_view.set(dest);
        }
    }

    // Switch-view signal: when a child view sets this to true, we cycle forward.
    let mut switch_signal = hooks.use_state(|| false);
    if switch_signal.get() {
        switch_signal.set(false);
        let next = match active_view.get() {
            ViewKind::Prs => ViewKind::Issues,
            ViewKind::Issues => ViewKind::Actions,
            ViewKind::Actions => ViewKind::Alerts,
            ViewKind::Alerts => ViewKind::Notifications,
            ViewKind::Notifications => ViewKind::Repo,
            ViewKind::Repo => ViewKind::Prs,
        };
        active_view.set(next);
    }

    // Switch-view-back signal: cycle views in reverse order.
    let mut switch_back_signal = hooks.use_state(|| false);
    if switch_back_signal.get() {
        switch_back_signal.set(false);
        let prev = match active_view.get() {
            ViewKind::Prs => ViewKind::Repo,
            ViewKind::Issues => ViewKind::Prs,
            ViewKind::Actions => ViewKind::Issues,
            ViewKind::Alerts => ViewKind::Actions,
            ViewKind::Notifications => ViewKind::Alerts,
            ViewKind::Repo => ViewKind::Notifications,
        };
        active_view.set(prev);
    }

    // Go-to-view signal: a child view sets this to jump directly to a specific view.
    // Unlike switch_view/switch_view_back (which just cycle), goto updates
    // previous_view so that go_back (ctrl+t) returns to the origin.
    let mut goto_view_signal = hooks.use_state(|| Option::<ViewKind>::None);
    if let Some(dest) = goto_view_signal.get() {
        goto_view_signal.set(None);
        if active_view.get() != dest {
            previous_view.set(Some(active_view.get()));
            active_view.set(dest);
        }
    }

    // Scope state: repo-scoped vs global.
    // When deep-linking to an external repo (different from the detected local
    // repo), start in global scope so config tabs aren't hidden by scope.
    let detected_repo = props.detected_repo;
    let scope_config = config.map_or(Scope::Auto, |c| c.github.scope.unwrap_or_default());
    let nav_targets_external = {
        let detected_full = detected_repo.map(RepoRef::full_name);
        nav_target.read().as_ref().is_some_and(|t| {
            let target_repo = match t {
                NavigationTarget::ActionsRun { owner, repo, .. }
                | NavigationTarget::PullRequest { owner, repo, .. }
                | NavigationTarget::Issue { owner, repo, .. } => format!("{owner}/{repo}"),
            };
            detected_full.as_ref().is_none_or(|d| *d != target_repo)
        })
    };
    let initial_scoped = if nav_targets_external {
        false
    } else {
        match scope_config {
            Scope::Auto | Scope::Repo => detected_repo.is_some(),
            Scope::Global => false,
        }
    };
    let mut repo_scoped = hooks.use_state(move || initial_scoped);

    // Repo picker: user-selected repo override (persists until changed).
    let mut selected_repo: State<Option<String>> = hooks.use_state(|| None);

    // The effective repo name for scope purposes: user selection overrides detection.
    let effective_repo_name: Option<String> = {
        let sel = selected_repo.read();
        if sel.is_some() {
            sel.clone()
        } else {
            detected_repo.map(RepoRef::full_name)
        }
    };

    // Scope toggle signal: when a child view sets this to true, we toggle.
    let mut scope_toggle_signal = hooks.use_state(|| false);
    if scope_toggle_signal.get() {
        scope_toggle_signal.set(false);
        if effective_repo_name.is_some() {
            repo_scoped.set(!repo_scoped.get());
        }
    }

    // Effective scope repo string to pass to views.
    let scope_repo: Option<String> = if repo_scoped.get() {
        effective_repo_name.clone()
    } else {
        None
    };

    // Repo picker overlay state.
    let mut picker_visible = hooks.use_state(|| false);
    let mut picker_cursor = hooks.use_state(|| 0_usize);
    let mut picker_filter = hooks.use_state(String::new);

    // Repo picker signal: child views set this to request the picker overlay.
    let mut picker_signal = hooks.use_state(|| false);
    if picker_signal.get() {
        picker_signal.set(false);
        // Build items list: detected repo first, then repo_paths keys.
        let has_items = detected_repo.is_some() || config.is_some_and(|c| !c.repo_paths.is_empty());
        if has_items {
            picker_cursor.set(0);
            picker_filter.set(String::new());
            picker_visible.set(true);
        }
    }

    // Repo picker: pre-compute item list (repo names) for the closure.
    let picker_items: Vec<String> = {
        let mut items = Vec::new();
        if let Some(d) = detected_repo {
            items.push(d.full_name());
        }
        if let Some(c) = config {
            for key in c.repo_paths.keys() {
                // Skip detected repo if already in the list.
                if detected_repo.is_none_or(|d| d.full_name() != *key) {
                    items.push(key.clone());
                }
            }
        }
        items
    };
    let picker_items_for_closure = picker_items.clone();
    let detected_repo_name = detected_repo.map(RepoRef::full_name);

    // Repo picker keyboard handling.
    hooks.use_terminal_events({
        move |event| {
            if !picker_visible.get() {
                return;
            }
            if let TerminalEvent::Key(KeyEvent { code, kind, .. }) = event {
                if kind == KeyEventKind::Release {
                    return;
                }
                // Compute filtered list for navigation & selection.
                let filter_buf = picker_filter.read().clone();
                let filtered = filter_suggestions(&picker_items_for_closure, &filter_buf);
                let filtered_len = filtered.len();

                match code {
                    KeyCode::Down => {
                        picker_cursor
                            .set((picker_cursor.get() + 1).min(filtered_len.saturating_sub(1)));
                    }
                    KeyCode::Up => {
                        picker_cursor.set(picker_cursor.get().saturating_sub(1));
                    }
                    KeyCode::Enter => {
                        let idx = picker_cursor.get();
                        if let Some(name) = filtered.get(idx) {
                            // If selecting the originally detected repo, clear override.
                            let is_detected =
                                detected_repo_name.as_ref().is_some_and(|d| d == name);
                            if is_detected {
                                selected_repo.set(None);
                            } else {
                                selected_repo.set(Some(name.clone()));
                            }
                            // Activate scope so the selection takes effect immediately.
                            repo_scoped.set(true);
                        }
                        picker_filter.set(String::new());
                        picker_visible.set(false);
                    }
                    KeyCode::Esc => {
                        picker_filter.set(String::new());
                        picker_visible.set(false);
                    }
                    KeyCode::Backspace => {
                        let mut buf = picker_filter.read().clone();
                        buf.pop();
                        picker_filter.set(buf);
                        // Clamp cursor to new filtered length.
                        let new_len =
                            filter_suggestions(&picker_items_for_closure, &picker_filter.read())
                                .len();
                        if picker_cursor.get() >= new_len {
                            picker_cursor.set(new_len.saturating_sub(1));
                        }
                    }
                    KeyCode::Char(ch) => {
                        let mut buf = picker_filter.read().clone();
                        buf.push(ch);
                        picker_filter.set(buf);
                        // Reset cursor and clamp to new filtered length.
                        picker_cursor.set(0);
                    }
                    _ => {}
                }
            }
        }
    });

    // Exit handling.
    if should_exit.get() {
        system.exit();
    }

    // Per-pool rate-limit state — GraphQL and REST have separate GitHub quotas.
    let graphql_rate_limit: State<Option<RateLimitInfo>> = hooks.use_state(|| None);
    let rest_rate_limit: State<Option<RateLimitInfo>> = hooks.use_state(|| None);

    let show_count = config.is_none_or(|c| c.theme.ui.filters_show_count.unwrap_or(true));
    let show_separator = config.is_none_or(|c| c.theme.ui.table.show_separator.unwrap_or(true));
    let default_preview_pct = config.map_or(0.45, |c| c.defaults.preview.width.unwrap_or(0.45));
    let preview_width_pct: State<f64> = hooks.use_state(move || default_preview_pct);
    let repo_paths = config.map(|c| &c.repo_paths);
    let date_format = config.map(|c| c.defaults.date_format.as_deref().unwrap_or("relative"));

    // All filters/paths needed simultaneously (views are always in the tree).
    let active = active_view.get();
    let refetch_minutes = config.map_or(10, |c| c.github.refetch_interval_minutes.unwrap_or(10));
    let prefetch_pr_details = config.map_or(0, |c| c.github.prefetch_pr_details.unwrap_or(0));
    let auto_clone = config.is_some_and(|c| c.github.auto_clone.unwrap_or(false));
    let filters_pr = config.map(|c| c.pr_filters.as_slice());
    let filters_issue = config.map(|c| c.issues_filters.as_slice());
    let filters_actions = config.map(|c| c.actions_filters.as_slice());
    let filters_notif = config.map(|c| c.notifications_filters.as_slice());
    let filters_alerts = config.map(|c| c.alerts_filters.as_slice());
    let repo_path = props.repo_path;

    // Build repo picker overlay when visible.
    let rendered_repo_picker: Option<RenderedSelectionOverlay> = if picker_visible.get() {
        let theme_ref = theme.unwrap();
        let anchor_icon = &theme_ref.icons.repo_anchor;
        let filter_buf = picker_filter.read().clone();
        let filtered = filter_suggestions(&picker_items, &filter_buf);
        let overlay_items: Vec<SelectionOverlayItem> = filtered
            .iter()
            .map(|name| {
                // Detected repo (first in unfiltered list) gets the anchor icon.
                let is_detected = detected_repo.is_some_and(|d| d.full_name() == *name);
                let label = if is_detected {
                    format!("{name} {anchor_icon}")
                } else {
                    name.clone()
                };
                SelectionOverlayItem { label }
            })
            .collect();
        Some(RenderedSelectionOverlay::build(
            SelectionOverlayBuildConfig {
                title: "Select repo".to_owned(),
                items: overlay_items,
                cursor: picker_cursor.get(),
                show_filter: true,
                filter_text: filter_buf,
                depth,
                title_color: Some(theme_ref.text_primary),
                item_color: Some(theme_ref.text_secondary),
                cursor_color: Some(theme_ref.text_primary),
                selected_bg: Some(theme_ref.bg_selected),
                border_color: Some(theme_ref.border_primary),
                hint_color: Some(theme_ref.text_faint),
                filter_prompt_color: Some(theme_ref.text_faint),
                filter_text_color: Some(theme_ref.text_primary),
                cursor_marker: theme_ref.icons.select_cursor.clone(),
            },
        ))
    } else {
        None
    };

    element! {
        View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
            View(
                display: if active == ViewKind::Prs { Display::Flex } else { Display::None },
                flex_grow: 1.0_f32,
            ) {
                PrsView(
                    filters: filters_pr,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct: preview_width_pct,
                    default_preview_pct,
                    show_filter_count: show_count,
                    show_separator,
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    goto_view: goto_view_signal,
                    scope_toggle: scope_toggle_signal,
                    repo_picker: picker_signal,
                    scope_repo: scope_repo.clone(),
                    repo_paths,
                    date_format,
                    is_active: active == ViewKind::Prs && !picker_visible.get(),
                    refetch_interval_minutes: refetch_minutes,
                    prefetch_pr_details,
                    auto_clone,
                    nav_target,
                    go_back: go_back_signal,
                    rate_limit: graphql_rate_limit,
                )
            }
            View(
                display: if active == ViewKind::Issues { Display::Flex } else { Display::None },
                flex_grow: 1.0_f32,
            ) {
                IssuesView(
                    filters: filters_issue,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct: preview_width_pct,
                    default_preview_pct,
                    show_filter_count: show_count,
                    show_separator,
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    goto_view: goto_view_signal,
                    scope_toggle: scope_toggle_signal,
                    repo_picker: picker_signal,
                    scope_repo: scope_repo.clone(),
                    date_format,
                    is_active: active == ViewKind::Issues && !picker_visible.get(),
                    refetch_interval_minutes: refetch_minutes,
                    nav_target,
                    go_back: go_back_signal,
                    rate_limit: graphql_rate_limit,
                )
            }
            View(
                display: if active == ViewKind::Actions { Display::Flex } else { Display::None },
                flex_grow: 1.0_f32,
            ) {
                ActionsView(
                    filters: filters_actions,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct: preview_width_pct,
                    default_preview_pct,
                    show_filter_count: show_count,
                    show_separator,
                    scope_repo: scope_repo.clone(),
                    detected_repo: detected_repo.map(RepoRef::full_name),
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    goto_view: goto_view_signal,
                    scope_toggle: scope_toggle_signal,
                    repo_picker: picker_signal,
                    is_active: active == ViewKind::Actions && !picker_visible.get(),
                    refetch_interval_minutes: refetch_minutes,
                    nav_target,
                    go_back: go_back_signal,
                    rate_limit: rest_rate_limit,
                )
            }
            View(
                display: if active == ViewKind::Alerts { Display::Flex } else { Display::None },
                flex_grow: 1.0_f32,
            ) {
                AlertsView(
                    filters: filters_alerts,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct: preview_width_pct,
                    default_preview_pct,
                    show_filter_count: show_count,
                    show_separator,
                    scope_repo: scope_repo.clone(),
                    detected_repo: detected_repo.map(RepoRef::full_name),
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    goto_view: goto_view_signal,
                    scope_toggle: scope_toggle_signal,
                    repo_picker: picker_signal,
                    is_active: active == ViewKind::Alerts && !picker_visible.get(),
                    refetch_interval_minutes: refetch_minutes,
                    date_format,
                    rate_limit: rest_rate_limit,
                )
            }
            View(
                display: if active == ViewKind::Notifications { Display::Flex } else { Display::None },
                flex_grow: 1.0_f32,
            ) {
                NotificationsView(
                    filters: filters_notif,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    show_filter_count: show_count,
                    show_separator,
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    goto_view: goto_view_signal,
                    scope_toggle: scope_toggle_signal,
                    repo_picker: picker_signal,
                    scope_repo: scope_repo.clone(),
                    date_format,
                    is_active: active == ViewKind::Notifications && !picker_visible.get(),
                    refetch_interval_minutes: refetch_minutes,
                    rate_limit: rest_rate_limit,
                )
            }
            View(
                display: if active == ViewKind::Repo { Display::Flex } else { Display::None },
                flex_grow: 1.0_f32,
            ) {
                RepoView(
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct: preview_width_pct,
                    default_preview_pct,
                    show_separator,
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    goto_view: goto_view_signal,
                    scope_toggle: scope_toggle_signal,
                    repo_picker: picker_signal,
                    scope_repo: scope_repo.clone(),
                    repo_path,
                    detected_repo,
                    repo_paths,
                    engine: props.engine,
                    nav_target,
                    date_format,
                    is_active: active == ViewKind::Repo && !picker_visible.get(),
                    refetch_interval_minutes: refetch_minutes,
                    rate_limit: graphql_rate_limit,
                )
            }
            SelectionOverlay(overlay: rendered_repo_picker, width, height)
        }
    }
}
