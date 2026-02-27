use std::path::Path;

use iocraft::prelude::*;

use crate::color::ColorDepth;
use crate::config::keybindings::MergedBindings;
use crate::config::types::{AppConfig, Scope};
use crate::engine::EngineHandle;
use crate::icons::ResolvedIcons;
use crate::theme::ResolvedTheme;
use crate::types::RepoRef;
use crate::views::actions::ActionsView;
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
    Notifications,
    Repo,
}

impl ViewKind {
    pub const ALL: [ViewKind; 5] = [
        ViewKind::Prs,
        ViewKind::Issues,
        ViewKind::Actions,
        ViewKind::Notifications,
        ViewKind::Repo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Prs => "PRs",
            Self::Issues => "Issues",
            Self::Actions => "Actions",
            Self::Notifications => "Notifs",
            Self::Repo => "Repo",
        }
    }

    pub fn icon_label(self, icons: &ResolvedIcons) -> String {
        match self {
            Self::Prs => format!("{} {}", icons.view_prs, self.label()),
            Self::Issues => format!("{} {}", icons.view_issues, self.label()),
            Self::Actions => format!("{} {}", icons.view_actions, self.label()),
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
    let initial_view = config.map_or(ViewKind::Prs, |c| match c.defaults.view {
        crate::config::types::View::Prs => ViewKind::Prs,
        crate::config::types::View::Issues => ViewKind::Issues,
        crate::config::types::View::Actions => ViewKind::Actions,
        crate::config::types::View::Notifications => ViewKind::Notifications,
        crate::config::types::View::Repo => ViewKind::Repo,
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
            ViewKind::Actions => ViewKind::Notifications,
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
            ViewKind::Notifications => ViewKind::Actions,
            ViewKind::Repo => ViewKind::Notifications,
        };
        active_view.set(prev);
    }

    // Scope state: repo-scoped vs global.
    // When deep-linking to an external repo (different from the detected local
    // repo), start in global scope so config tabs aren't hidden by scope.
    let detected_repo = props.detected_repo;
    let scope_config = config.map_or(Scope::Auto, |c| c.github.scope);
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

    // Scope toggle signal: when a child view sets this to true, we toggle.
    let mut scope_toggle_signal = hooks.use_state(|| false);
    if scope_toggle_signal.get() {
        scope_toggle_signal.set(false);
        if detected_repo.is_some() {
            repo_scoped.set(!repo_scoped.get());
        }
    }

    // Effective scope repo string to pass to views.
    let scope_repo: Option<String> = if repo_scoped.get() {
        detected_repo.map(RepoRef::full_name)
    } else {
        None
    };

    // Exit handling.
    if should_exit.get() {
        system.exit();
    }

    let show_count = config.is_none_or(|c| c.theme.ui.filters_show_count.unwrap_or(true));
    let show_separator = config.is_none_or(|c| c.theme.ui.table.show_separator.unwrap_or(true));
    let preview_width_pct = config.map_or(0.45, |c| c.defaults.preview.width);
    let repo_paths = config.map(|c| &c.repo_paths);
    let date_format = config.map(|c| c.defaults.date_format.as_str());

    // All filters/paths needed simultaneously (views are always in the tree).
    let active = active_view.get();
    let refetch_minutes = config.map_or(10, |c| c.github.refetch_interval_minutes);
    let prefetch_pr_details = config.map_or(0, |c| c.github.prefetch_pr_details);
    let auto_clone = config.is_some_and(|c| c.github.auto_clone);
    let filters_pr = config.map(|c| c.pr_filters.as_slice());
    let filters_issue = config.map(|c| c.issues_filters.as_slice());
    let filters_actions = config.map(|c| c.actions_filters.as_slice());
    let filters_notif = config.map(|c| c.notifications_filters.as_slice());
    let repo_path = props.repo_path;

    element! {
        View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
            View(
                display: if active == ViewKind::Prs { Display::Flex } else { Display::None },
                flex_grow: 1.0,
            ) {
                PrsView(
                    filters: filters_pr,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct,
                    show_filter_count: show_count,
                    show_separator,
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    scope_toggle: scope_toggle_signal,
                    scope_repo: scope_repo.clone(),
                    repo_paths,
                    date_format,
                    is_active: active == ViewKind::Prs,
                    refetch_interval_minutes: refetch_minutes,
                    prefetch_pr_details,
                    auto_clone,
                    nav_target,
                    go_back: go_back_signal,
                )
            }
            View(
                display: if active == ViewKind::Issues { Display::Flex } else { Display::None },
                flex_grow: 1.0,
            ) {
                IssuesView(
                    filters: filters_issue,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct,
                    show_filter_count: show_count,
                    show_separator,
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    scope_toggle: scope_toggle_signal,
                    scope_repo: scope_repo.clone(),
                    date_format,
                    is_active: active == ViewKind::Issues,
                    refetch_interval_minutes: refetch_minutes,
                    nav_target,
                    go_back: go_back_signal,
                )
            }
            View(
                display: if active == ViewKind::Actions { Display::Flex } else { Display::None },
                flex_grow: 1.0,
            ) {
                ActionsView(
                    filters: filters_actions,
                    engine: props.engine,
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    preview_width_pct,
                    show_filter_count: show_count,
                    show_separator,
                    scope_repo: scope_repo.clone(),
                    detected_repo: detected_repo.map(RepoRef::full_name),
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    scope_toggle: scope_toggle_signal,
                    is_active: active == ViewKind::Actions,
                    refetch_interval_minutes: refetch_minutes,
                    nav_target,
                    go_back: go_back_signal,
                )
            }
            View(
                display: if active == ViewKind::Notifications { Display::Flex } else { Display::None },
                flex_grow: 1.0,
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
                    scope_toggle: scope_toggle_signal,
                    scope_repo: scope_repo.clone(),
                    date_format,
                    is_active: active == ViewKind::Notifications,
                    refetch_interval_minutes: refetch_minutes,
                )
            }
            View(
                display: if active == ViewKind::Repo { Display::Flex } else { Display::None },
                flex_grow: 1.0,
            ) {
                RepoView(
                    theme,
                    keybindings,
                    color_depth: depth,
                    width,
                    height,
                    show_separator,
                    should_exit,
                    switch_view: switch_signal,
                    switch_view_back: switch_back_signal,
                    scope_toggle: scope_toggle_signal,
                    scope_repo: scope_repo.clone(),
                    repo_path,
                    date_format,
                    is_active: active == ViewKind::Repo,
                    refetch_interval_minutes: refetch_minutes,
                )
            }
        }
    }
}
