use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

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

struct RefreshEntry {
    filter_idx: usize,
    filter: FilterConfig,
    interval: Duration,
    notify_tx: Sender<Event>,
    last_fetch: Option<Instant>,
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

    /// Register PR filters for background refresh (replaces any existing PR entries).
    pub fn register_prs(
        &mut self,
        filter_configs: Vec<PrFilter>,
        interval: Duration,
        notify_tx: &Sender<Event>,
    ) {
        self.entries
            .retain(|e| !matches!(e.filter, FilterConfig::Pr(_)));
        for (filter_idx, filter) in filter_configs.into_iter().enumerate() {
            self.entries.push(RefreshEntry {
                filter_idx,
                filter: FilterConfig::Pr(filter),
                interval,
                notify_tx: notify_tx.clone(),
                last_fetch: None,
            });
        }
    }

    /// Register Issue filters for background refresh.
    pub fn register_issues(
        &mut self,
        filter_configs: Vec<IssueFilter>,
        interval: Duration,
        notify_tx: &Sender<Event>,
    ) {
        self.entries
            .retain(|e| !matches!(e.filter, FilterConfig::Issue(_)));
        for (filter_idx, filter) in filter_configs.into_iter().enumerate() {
            self.entries.push(RefreshEntry {
                filter_idx,
                filter: FilterConfig::Issue(filter),
                interval,
                notify_tx: notify_tx.clone(),
                last_fetch: None,
            });
        }
    }

    /// Register Notification filters for background refresh.
    pub fn register_notifications(
        &mut self,
        filter_configs: Vec<NotificationFilter>,
        interval: Duration,
        notify_tx: &Sender<Event>,
    ) {
        self.entries
            .retain(|e| !matches!(e.filter, FilterConfig::Notification(_)));
        for (filter_idx, filter) in filter_configs.into_iter().enumerate() {
            self.entries.push(RefreshEntry {
                filter_idx,
                filter: FilterConfig::Notification(filter),
                interval,
                notify_tx: notify_tx.clone(),
                last_fetch: None,
            });
        }
    }

    /// Mark the given filter index + view kind as having just been fetched.
    pub fn mark_fetched(&mut self, filter_idx: usize, view_kind: ViewKind) {
        let now = Instant::now();
        for entry in &mut self.entries {
            let kind_matches = matches!(
                (&entry.filter, view_kind),
                (FilterConfig::Pr(_), ViewKind::Prs)
                    | (FilterConfig::Issue(_), ViewKind::Issues)
                    | (FilterConfig::Notification(_), ViewKind::Notifications)
            );
            if kind_matches && entry.filter_idx == filter_idx {
                entry.last_fetch = Some(now);
            }
        }
    }

    /// Return all entries whose refresh interval has elapsed since last fetch.
    ///
    /// Entries that have never been fetched are skipped — the initial load is
    /// done on-demand by the view; background refresh fires only afterwards.
    pub fn due_entries(&self) -> Vec<DueEntry> {
        let now = Instant::now();
        self.entries
            .iter()
            .filter(|e| {
                e.last_fetch
                    .is_some_and(|t| now.duration_since(t) >= e.interval)
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
