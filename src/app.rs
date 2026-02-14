use std::path::Path;
use std::sync::Arc;

use iocraft::prelude::*;
use octocrab::Octocrab;

use crate::color::ColorDepth;
use crate::config::types::AppConfig;
use crate::theme::ResolvedTheme;
use crate::views::issues::IssuesView;
use crate::views::notifications::NotificationsView;
use crate::views::prs::PrsView;
use crate::views::repo::RepoView;

// ---------------------------------------------------------------------------
// Active view enum
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveView {
    Prs,
    Issues,
    Notifications,
    Repo,
}

// ---------------------------------------------------------------------------
// Root App component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct AppProps<'a> {
    pub config: Option<&'a AppConfig>,
    pub octocrab: Option<&'a Arc<Octocrab>>,
    pub theme: Option<&'a ResolvedTheme>,
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
    let depth = props.color_depth;

    // View switching state.
    let initial_view = config.map_or(ActiveView::Prs, |c| match c.defaults.view {
        crate::config::types::View::Issues => ActiveView::Issues,
        crate::config::types::View::Notifications => ActiveView::Notifications,
        crate::config::types::View::Repo => ActiveView::Repo,
        crate::config::types::View::Prs => ActiveView::Prs,
    });
    let mut active_view = hooks.use_state(move || initial_view);

    // Switch-view signal: when a child view sets this to true, we cycle.
    let mut switch_signal = hooks.use_state(|| false);
    if switch_signal.get() {
        switch_signal.set(false);
        let next = match active_view.get() {
            ActiveView::Prs => ActiveView::Issues,
            ActiveView::Issues => ActiveView::Notifications,
            ActiveView::Notifications => ActiveView::Repo,
            ActiveView::Repo => ActiveView::Prs,
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
        ActiveView::Prs => {
            let sections = config.map(|c| c.pr_sections.as_slice());
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    PrsView(
                        sections,
                        octocrab: props.octocrab,
                        theme,
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
        ActiveView::Issues => {
            let sections = config.map(|c| c.issues_sections.as_slice());
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    IssuesView(
                        sections,
                        octocrab: props.octocrab,
                        theme,
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
        ActiveView::Notifications => {
            let sections = config.map(|c| c.notifications_sections.as_slice());
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    NotificationsView(
                        sections,
                        octocrab: props.octocrab,
                        theme,
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
        ActiveView::Repo => {
            let repo_path = props.repo_path;
            element! {
                View(width: u32::from(width), height: u32::from(height), flex_direction: FlexDirection::Column) {
                    RepoView(
                        theme,
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
