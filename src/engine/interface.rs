use std::sync::mpsc::Sender;

use crate::config::types::{IssueFilter, NotificationFilter, PrFilter};
use crate::types::{Issue, IssueDetail, Notification, PrDetail, PullRequest, RateLimitInfo};

/// Handle to the backend engine held by the UI layer.
///
/// Cheaply cloneable. When the last handle is dropped the sender channel
/// closes, signalling the engine to shut down.
#[derive(Clone)]
pub struct EngineHandle {
    tx: tokio::sync::mpsc::UnboundedSender<Request>,
}

impl EngineHandle {
    pub(super) fn new(tx: tokio::sync::mpsc::UnboundedSender<Request>) -> Self {
        Self { tx }
    }

    /// Send a request to the engine. Non-blocking — returns immediately.
    pub fn send(&self, req: Request) {
        // Ignore errors: if the receiver is gone the engine has already shut down.
        let _ = self.tx.send(req);
    }
}

/// Trait implemented by both `GitHubEngine` and `StubEngine`.
pub trait Engine: Send + 'static {
    fn start(self) -> EngineHandle;
}

/// All fields needed by the engine to fetch a single PR detail including the compare call.
pub struct PrRef {
    pub owner: String,
    pub repo: String,
    pub number: u64,
    pub base_ref: String,
    pub head_repo_owner: Option<String>,
    pub head_ref: String,
}

/// All operations the UI layer can send to the engine.
pub enum Request {
    // -----------------------------------------------------------------------
    // Fetch operations (UI pulls data on demand)
    // -----------------------------------------------------------------------
    FetchPrs {
        filter_idx: usize,
        filter: PrFilter,
        reply_tx: Sender<Event>,
    },
    FetchIssues {
        filter_idx: usize,
        filter: IssueFilter,
        /// Skip the moka cache and fetch fresh data from the GitHub API.
        force: bool,
        reply_tx: Sender<Event>,
    },
    FetchNotifications {
        filter_idx: usize,
        filter: NotificationFilter,
        reply_tx: Sender<Event>,
    },
    FetchPrDetail {
        owner: String,
        repo: String,
        number: u64,
        base_ref: String,
        head_repo_owner: Option<String>,
        head_ref: String,
        reply_tx: Sender<Event>,
    },
    FetchIssueDetail {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    FetchRepoLabels {
        owner: String,
        repo: String,
        reply_tx: Sender<Event>,
    },
    FetchRepoCollaborators {
        owner: String,
        repo: String,
        reply_tx: Sender<Event>,
    },
    /// Prefetch PR details for a list of PRs (includes branch refs for the compare call).
    PrefetchPrDetails {
        prs: Vec<PrRef>,
        reply_tx: Sender<Event>,
    },

    // -----------------------------------------------------------------------
    // Background refresh registration (UI registers once per view)
    // -----------------------------------------------------------------------
    RegisterPrsRefresh {
        filter_configs: Vec<PrFilter>,
        notify_tx: Sender<Event>,
    },
    RegisterIssuesRefresh {
        filter_configs: Vec<IssueFilter>,
        notify_tx: Sender<Event>,
    },
    RegisterNotificationsRefresh {
        filter_configs: Vec<NotificationFilter>,
        notify_tx: Sender<Event>,
    },

    // -----------------------------------------------------------------------
    // Mutation operations — PR
    // -----------------------------------------------------------------------
    ApprovePr {
        owner: String,
        repo: String,
        number: u64,
        body: Option<String>,
        reply_tx: Sender<Event>,
    },
    MergePr {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    ClosePr {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    ReopenPr {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    AddPrComment {
        owner: String,
        repo: String,
        number: u64,
        body: String,
        reply_tx: Sender<Event>,
    },
    UpdateBranch {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    ReadyForReview {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    AssignPr {
        owner: String,
        repo: String,
        number: u64,
        logins: Vec<String>,
        reply_tx: Sender<Event>,
    },
    UnassignPr {
        owner: String,
        repo: String,
        number: u64,
        login: String,
        reply_tx: Sender<Event>,
    },

    // -----------------------------------------------------------------------
    // Mutation operations — Issue
    // -----------------------------------------------------------------------
    CloseIssue {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    ReopenIssue {
        owner: String,
        repo: String,
        number: u64,
        reply_tx: Sender<Event>,
    },
    AddIssueComment {
        owner: String,
        repo: String,
        number: u64,
        body: String,
        reply_tx: Sender<Event>,
    },
    AddIssueLabels {
        owner: String,
        repo: String,
        number: u64,
        labels: Vec<String>,
        reply_tx: Sender<Event>,
    },
    AssignIssue {
        owner: String,
        repo: String,
        number: u64,
        logins: Vec<String>,
        reply_tx: Sender<Event>,
    },
    UnassignIssue {
        owner: String,
        repo: String,
        number: u64,
        login: String,
        reply_tx: Sender<Event>,
    },

    // -----------------------------------------------------------------------
    // Mutation operations — Notification
    // -----------------------------------------------------------------------
    MarkNotificationRead {
        id: String,
        reply_tx: Sender<Event>,
    },
    MarkNotificationDone {
        id: String,
        reply_tx: Sender<Event>,
    },
    MarkAllNotificationsRead {
        reply_tx: Sender<Event>,
    },
    UnsubscribeNotification {
        id: String,
        reply_tx: Sender<Event>,
    },

    // -----------------------------------------------------------------------
    // Control
    // -----------------------------------------------------------------------
    Shutdown,
}

/// All events the engine can push back to UI views.
pub enum Event {
    // -----------------------------------------------------------------------
    // Fetch results
    // -----------------------------------------------------------------------
    PrsFetched {
        filter_idx: usize,
        prs: Vec<PullRequest>,
        rate_limit: Option<RateLimitInfo>,
    },
    IssuesFetched {
        filter_idx: usize,
        issues: Vec<Issue>,
        rate_limit: Option<RateLimitInfo>,
    },
    NotificationsFetched {
        filter_idx: usize,
        notifications: Vec<Notification>,
    },
    PrDetailFetched {
        number: u64,
        detail: PrDetail,
        rate_limit: Option<RateLimitInfo>,
    },
    IssueDetailFetched {
        number: u64,
        detail: IssueDetail,
    },
    RepoLabelsFetched {
        labels: Vec<String>,
        rate_limit: Option<RateLimitInfo>,
    },
    RepoCollaboratorsFetched {
        logins: Vec<String>,
        rate_limit: Option<RateLimitInfo>,
    },
    /// Unified error event for all fetch or mutation failures.
    FetchError {
        context: String,
        message: String,
    },

    // -----------------------------------------------------------------------
    // Mutation results
    // -----------------------------------------------------------------------
    MutationOk {
        description: String,
    },
    MutationError {
        description: String,
        message: String,
    },

    // -----------------------------------------------------------------------
    // Rate limit
    // -----------------------------------------------------------------------
    RateLimitUpdated {
        info: RateLimitInfo,
    },
}
