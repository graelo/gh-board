use std::path::Path;
use std::sync::Arc;

use iocraft::prelude::*;
use octocrab::Octocrab;

use crate::color::ColorDepth;
use crate::config::keybindings::MergedBindings;
use crate::config::types::AppConfig;
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
            Self::Prs => format!("{} {}", icons.section_prs, self.label()),
            Self::Issues => format!("{} {}", icons.section_issues, self.label()),
            Self::Notifications => format!("{} {}", icons.section_notifications, self.label()),
            Self::Repo => format!("{} {}", icons.section_repo, self.label()),
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
    pub theme: Option<&'a ResolvedTheme>,
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub repo_path: Option<&'a Path>,
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

    // Switch-view signal: when a child view sets this to true, we cycle.
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

    // Exit handling.
    if should_exit.get() {
        system.exit();
    }

    let show_count = config.is_none_or(|c| c.theme.ui.sections_show_count);
    let show_separator = config.is_none_or(|c| c.theme.ui.table.show_separator);
    let preview_width_pct = config.map_or(0.45, |c| c.defaults.preview.width);
    let repo_paths = config.map(|c| &c.repo_paths);
    let date_format = config.map(|c| c.defaults.date_format.as_str());

    match active_view.get() {
        ViewKind::Prs => {
            let sections = config.map(|c| c.pr_sections.as_slice());
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    PrsView(
                        sections,
                        octocrab: props.octocrab,
                        theme,
                        keybindings,
                        color_depth: depth,
                        width,
                        height,
                        preview_width_pct,
                        show_section_count: show_count,
                        show_separator,
                        should_exit,
                        switch_view: switch_signal,
                        repo_paths,
                        date_format,
                    )
                }
            }
        }
        ViewKind::Issues => {
            let sections = config.map(|c| c.issues_sections.as_slice());
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    IssuesView(
                        sections,
                        octocrab: props.octocrab,
                        theme,
                        keybindings,
                        color_depth: depth,
                        width,
                        height,
                        preview_width_pct,
                        show_section_count: show_count,
                        show_separator,
                        should_exit,
                        switch_view: switch_signal,
                        date_format,
                    )
                }
            }
        }
        ViewKind::Notifications => {
            let sections = config.map(|c| c.notifications_sections.as_slice());
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    NotificationsView(
                        sections,
                        octocrab: props.octocrab,
                        theme,
                        keybindings,
                        color_depth: depth,
                        width,
                        height,
                        show_section_count: show_count,
                        show_separator,
                        should_exit,
                        switch_view: switch_signal,
                        date_format,
                    )
                }
            }
        }
        ViewKind::Repo => {
            let repo_path = props.repo_path;
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    RepoView(
                        theme,
                        keybindings,
                        color_depth: depth,
                        width,
                        height,
                        show_separator,
                        should_exit,
                        switch_view: switch_signal,
                        repo_path,
                        date_format,
                    )
                }
            }
        }
    }
}
