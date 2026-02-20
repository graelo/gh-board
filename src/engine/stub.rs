use tokio::sync::mpsc::UnboundedReceiver;

use crate::types::{Issue, Notification, PullRequest};

use super::interface::{Engine, EngineHandle, Event, Request};

/// A stub engine that serves pre-loaded fixture data without any network calls.
///
/// Useful for integration tests and UI demos that must not require a `GITHUB_TOKEN`.
pub struct StubEngine {
    pub prs: Vec<PullRequest>,
    pub issues: Vec<Issue>,
    pub notifications: Vec<Notification>,
}

impl Engine for StubEngine {
    fn start(self) -> EngineHandle {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Request>();
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().expect("stub tokio runtime");
            rt.block_on(self.run_loop(rx));
        });
        EngineHandle::new(tx)
    }
}

impl StubEngine {
    #[allow(clippy::match_same_arms)]
    async fn run_loop(self, mut rx: UnboundedReceiver<Request>) {
        while let Some(req) = rx.recv().await {
            match req {
                Request::FetchPrs {
                    filter_idx,
                    reply_tx,
                    ..
                } => {
                    let _ = reply_tx.send(Event::PrsFetched {
                        filter_idx,
                        prs: self.prs.clone(),
                        rate_limit: None,
                    });
                }
                Request::FetchIssues {
                    filter_idx,
                    reply_tx,
                    ..
                } => {
                    let _ = reply_tx.send(Event::IssuesFetched {
                        filter_idx,
                        issues: self.issues.clone(),
                        rate_limit: None,
                    });
                }
                Request::FetchNotifications {
                    filter_idx,
                    reply_tx,
                    ..
                } => {
                    let _ = reply_tx.send(Event::NotificationsFetched {
                        filter_idx,
                        notifications: self.notifications.clone(),
                    });
                }
                Request::FetchPrDetail { reply_tx, .. }
                | Request::PrefetchPrDetails { reply_tx, .. } => {
                    let _ = reply_tx.send(Event::FetchError {
                        context: "stub".into(),
                        message: "no detail in stub".into(),
                    });
                }
                Request::FetchIssueDetail { reply_tx, .. } => {
                    let _ = reply_tx.send(Event::FetchError {
                        context: "stub".into(),
                        message: "no detail in stub".into(),
                    });
                }

                // Refresh registration — ignored by stub
                Request::RegisterPrsRefresh { .. }
                | Request::RegisterIssuesRefresh { .. }
                | Request::RegisterNotificationsRefresh { .. } => {}

                // All mutations succeed instantly
                Request::ApprovePr { reply_tx, .. }
                | Request::MergePr { reply_tx, .. }
                | Request::ClosePr { reply_tx, .. }
                | Request::ReopenPr { reply_tx, .. }
                | Request::AddPrComment { reply_tx, .. }
                | Request::UpdateBranch { reply_tx, .. }
                | Request::ReadyForReview { reply_tx, .. }
                | Request::AssignPr { reply_tx, .. }
                | Request::UnassignPr { reply_tx, .. }
                | Request::CloseIssue { reply_tx, .. }
                | Request::ReopenIssue { reply_tx, .. }
                | Request::AddIssueComment { reply_tx, .. }
                | Request::AddIssueLabels { reply_tx, .. }
                | Request::AssignIssue { reply_tx, .. }
                | Request::UnassignIssue { reply_tx, .. }
                | Request::MarkNotificationRead { reply_tx, .. }
                | Request::MarkNotificationDone { reply_tx, .. }
                | Request::MarkAllNotificationsRead { reply_tx }
                | Request::UnsubscribeNotification { reply_tx, .. } => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: "stub ok".into(),
                    });
                }

                Request::Shutdown => break,
            }
        }
    }
}
