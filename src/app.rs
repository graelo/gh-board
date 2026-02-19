use std::path::Path;
use std::sync::Arc;

use iocraft::prelude::*;
use moka::future::Cache;
use octocrab::Octocrab;

use crate::color::ColorDepth;
use crate::config::keybindings::MergedBindings;
use crate::config::types::{AppConfig, Scope};
use crate::github::types::RepoRef;
use crate::icons::ResolvedIcons;
use crate::theme::ResolvedTheme;
use crate::views::issues::IssuesView;
use crate::views::notifications::NotificationsView;
use crate::views::prs::PrsView;
use crate::views::repo::RepoView;

// ---------------------------------------------------------------------------
// View kind enum (public for status bar)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewKind {
    Prs,
    Issues,
    Notifications,
    Repo,
}

impl ViewKind {
    pub const ALL: [ViewKind; 4] = [
        ViewKind::Prs,
        ViewKind::Issues,
        ViewKind::Notifications,
        ViewKind::Repo,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Prs => "PRs",
            Self::Issues => "Issues",
            Self::Notifications => "Notifs",
            Self::Repo => "Repo",
        }
    }

    pub fn icon_label(self, icons: &ResolvedIcons) -> String {
        match self {
            Self::Prs => format!("{} {}", icons.view_prs, self.label()),
            Self::Issues => format!("{} {}", icons.view_issues, self.label()),
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
    pub octocrab: Option<&'a Arc<Octocrab>>,
    pub api_cache: Option<&'a Cache<String, String>>,
    pub theme: Option<&'a ResolvedTheme>,
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub repo_path: Option<&'a Path>,
    pub detected_repo: Option<&'a RepoRef>,
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
        crate::config::types::View::Issues => ViewKind::Issues,
        crate::config::types::View::Notifications => ViewKind::Notifications,
        crate::config::types::View::Repo => ViewKind::Repo,
        crate::config::types::View::Prs => ViewKind::Prs,
    });
    let mut active_view = hooks.use_state(move || initial_view);

    // Switch-view signal: when a child view sets this to true, we cycle forward.
    let mut switch_signal = hooks.use_state(|| false);
    if switch_signal.get() {
        switch_signal.set(false);
        let next = match active_view.get() {
            ViewKind::Prs => ViewKind::Issues,
            ViewKind::Issues => ViewKind::Notifications,
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
            ViewKind::Notifications => ViewKind::Issues,
            ViewKind::Repo => ViewKind::Notifications,
        };
        active_view.set(prev);
    }

    // Scope state: repo-scoped vs global.
    let detected_repo = props.detected_repo;
    let scope_config = config.map_or(Scope::Auto, |c| c.github.scope);
    let initial_scoped = match scope_config {
        Scope::Auto | Scope::Repo => detected_repo.is_some(),
        Scope::Global => false,
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
    let filters_pr = config.map(|c| c.pr_filters.as_slice());
    let filters_issue = config.map(|c| c.issues_filters.as_slice());
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
                    octocrab: props.octocrab,
                    api_cache: props.api_cache,
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
                )
            }
            View(
                display: if active == ViewKind::Issues { Display::Flex } else { Display::None },
                flex_grow: 1.0,
            ) {
                IssuesView(
                    filters: filters_issue,
                    octocrab: props.octocrab,
                    api_cache: props.api_cache,
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
                )
            }
            View(
                display: if active == ViewKind::Notifications { Display::Flex } else { Display::None },
                flex_grow: 1.0,
            ) {
                NotificationsView(
                    filters: filters_notif,
                    octocrab: props.octocrab,
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
