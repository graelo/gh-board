use std::sync::mpsc::Sender;
use std::time::{Duration, SystemTime};

use super::interface::Event;

struct WatchEntry {
    owner: String,
    repo: String,
    run_id: u64,
    host: Option<String>,
    reply_tx: Sender<Event>,
    last_poll: Option<SystemTime>,
}

/// Tracks workflow runs being watched for completion.
///
/// Sibling of `RefreshScheduler` — lives in the engine thread and polls
/// individual runs at a short interval via the existing `fetch_run_by_id`
/// REST endpoint.
pub struct WatchScheduler {
    entries: Vec<WatchEntry>,
    interval: Duration,
}

impl WatchScheduler {
    pub fn new(interval: Duration) -> Self {
        Self {
            entries: Vec::new(),
            interval,
        }
    }

    /// Add a run to watch. De-duplicates by `run_id`.
    pub fn add(
        &mut self,
        owner: String,
        repo: String,
        run_id: u64,
        host: Option<String>,
        reply_tx: Sender<Event>,
    ) {
        if self.entries.iter().any(|e| e.run_id == run_id) {
            return;
        }
        self.entries.push(WatchEntry {
            owner,
            repo,
            run_id,
            host,
            reply_tx,
            last_poll: None,
        });
    }

    /// Remove a run from the watch list.
    pub fn remove(&mut self, run_id: u64) {
        self.entries.retain(|e| e.run_id != run_id);
    }

    /// Check whether a run is currently being watched.
    #[allow(dead_code)]
    pub fn is_watched(&self, run_id: u64) -> bool {
        self.entries.iter().any(|e| e.run_id == run_id)
    }

    /// Return entries whose poll interval has elapsed (or never been polled).
    pub fn due_entries(&self) -> Vec<DueWatchEntry> {
        let now = SystemTime::now();
        self.entries
            .iter()
            .filter(|e| {
                e.last_poll.is_none_or(|t| {
                    now.duration_since(t).unwrap_or(Duration::ZERO) >= self.interval
                })
            })
            .map(|e| DueWatchEntry {
                owner: e.owner.clone(),
                repo: e.repo.clone(),
                run_id: e.run_id,
                host: e.host.clone(),
                reply_tx: e.reply_tx.clone(),
            })
            .collect()
    }

    /// Record that a run was just polled.
    pub fn mark_polled(&mut self, run_id: u64) {
        let now = SystemTime::now();
        for entry in &mut self.entries {
            if entry.run_id == run_id {
                entry.last_poll = Some(now);
            }
        }
    }

    /// Remove a completed entry.
    pub fn complete(&mut self, run_id: u64) {
        self.remove(run_id);
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A watch entry that is due for polling.
pub struct DueWatchEntry {
    pub owner: String,
    pub repo: String,
    pub run_id: u64,
    pub host: Option<String>,
    pub reply_tx: Sender<Event>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn make_scheduler(interval: Duration) -> (WatchScheduler, Sender<Event>) {
        let (tx, _rx) = mpsc::channel::<Event>();
        (WatchScheduler::new(interval), tx)
    }

    #[test]
    fn add_deduplicates_by_run_id() {
        let (mut sched, tx) = make_scheduler(Duration::from_secs(60));
        sched.add("owner1".into(), "repo1".into(), 42, None, tx.clone());
        sched.add("owner2".into(), "repo2".into(), 42, None, tx);
        assert!(sched.is_watched(42));
        assert_eq!(sched.due_entries().len(), 1);
    }

    #[test]
    fn remove_selectively() {
        let (mut sched, tx) = make_scheduler(Duration::from_secs(60));
        sched.add("o".into(), "r".into(), 1, None, tx.clone());
        sched.add("o".into(), "r".into(), 2, None, tx);
        sched.remove(1);
        assert!(!sched.is_watched(1));
        assert!(sched.is_watched(2));
    }

    #[test]
    fn due_entries_never_polled() {
        let (mut sched, tx) = make_scheduler(Duration::from_secs(60));
        sched.add(
            "acme".into(),
            "widget".into(),
            99,
            Some("ghes.example.com".into()),
            tx,
        );
        let due = sched.due_entries();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].owner, "acme");
        assert_eq!(due[0].repo, "widget");
        assert_eq!(due[0].run_id, 99);
        assert_eq!(due[0].host.as_deref(), Some("ghes.example.com"));
    }

    #[test]
    fn due_entries_excludes_recently_polled() {
        let (mut sched, tx) = make_scheduler(Duration::from_secs(60));
        sched.add("o".into(), "r".into(), 1, None, tx);
        sched.mark_polled(1);
        assert!(sched.due_entries().is_empty());
    }

    #[test]
    fn due_entries_includes_stale_polled() {
        let (mut sched, tx) = make_scheduler(Duration::ZERO);
        sched.add("o".into(), "r".into(), 1, None, tx);
        sched.mark_polled(1);
        std::thread::sleep(Duration::from_millis(1));
        assert_eq!(sched.due_entries().len(), 1);
    }
}
