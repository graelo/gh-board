use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime};

use crate::config::types::{IssueFilter, NotificationFilter, PrFilter};

use super::interface::Event;

/// Identifies which view type a refresh entry belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum ViewKind {
    Prs,
    Issues,
    Notifications,
}

/// A filter configuration for any view kind.
#[derive(Clone)]
pub enum FilterConfig {
    Pr(PrFilter),
    Issue(IssueFilter),
    Notification(NotificationFilter),
}

impl FilterConfig {
    pub(crate) fn view_kind(&self) -> ViewKind {
        match self {
            Self::Pr(_) => ViewKind::Prs,
            Self::Issue(_) => ViewKind::Issues,
            Self::Notification(_) => ViewKind::Notifications,
        }
    }
}

struct RefreshEntry {
    filter_idx: usize,
    filter: FilterConfig,
    interval: Duration,
    notify_tx: Sender<Event>,
    // SystemTime (wall clock) intentionally — Instant uses CLOCK_MONOTONIC,
    // which freezes during laptop sleep, causing missed refreshes after wake.
    last_fetch: Option<SystemTime>,
}

/// Tracks per-filter background refresh state for the engine.
pub struct RefreshScheduler {
    entries: Vec<RefreshEntry>,
}

impl Default for RefreshScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl RefreshScheduler {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Register filters for background refresh, replacing any existing entries
    /// for the same view kind.
    pub fn register(
        &mut self,
        configs: Vec<FilterConfig>,
        interval: Duration,
        notify_tx: &Sender<Event>,
    ) {
        let Some(kind) = configs.first().map(FilterConfig::view_kind) else {
            return;
        };
        self.entries.retain(|e| e.filter.view_kind() != kind);
        for (filter_idx, filter) in configs.into_iter().enumerate() {
            self.entries.push(RefreshEntry {
                filter_idx,
                filter,
                interval,
                notify_tx: notify_tx.clone(),
                last_fetch: None,
            });
        }
    }

    /// Mark the given filter index + view kind as having just been fetched.
    pub fn mark_fetched(&mut self, filter_idx: usize, view_kind: ViewKind) {
        let now = SystemTime::now();
        for entry in &mut self.entries {
            if entry.filter.view_kind() == view_kind && entry.filter_idx == filter_idx {
                entry.last_fetch = Some(now);
            }
        }
    }

    /// Return all entries whose refresh interval has elapsed since last fetch.
    ///
    /// Entries that have never been fetched are skipped — the initial load is
    /// done on-demand by the view; background refresh fires only afterwards.
    pub fn due_entries(&self) -> Vec<DueEntry> {
        let now = SystemTime::now();
        self.entries
            .iter()
            .filter(|e| {
                e.last_fetch
                    .is_some_and(|t| now.duration_since(t).unwrap_or(Duration::ZERO) >= e.interval)
            })
            .map(|e| DueEntry {
                filter_idx: e.filter_idx,
                filter: e.filter.clone(),
                notify_tx: e.notify_tx.clone(),
            })
            .collect()
    }
}

/// An entry that is due for background refresh.
pub struct DueEntry {
    pub filter_idx: usize,
    pub filter: FilterConfig,
    pub notify_tx: Sender<Event>,
}
