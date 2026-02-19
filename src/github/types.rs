use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueState {
    Open,
    Closed,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeableState {
    Mergeable,
    Conflicting,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeStateStatus {
    Behind,
    Blocked,
    Clean,
    Dirty,
    Draft,
    HasHooks,
    Unknown,
    Unstable,
}

/// Coarse branch update status derived from `MergeStateStatus`.
///
/// Only the two definitively-negative states are set from the search query;
/// `UpToDate` is never returned here — the authoritative positive confirmation
/// comes from `effective_update_status` after the detail fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BranchUpdateStatus {
    NeedsUpdate,
    HasConflicts,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationReason {
    Subscribed,
    ReviewRequested,
    Mention,
    Author,
    Comment,
    Assign,
    StateChange,
    CiActivity,
    TeamMention,
    SecurityAlert,
    #[serde(other)]
    Unknown,
}

impl NotificationReason {
    /// Stable display name for filtering and UI rendering.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subscribed => "subscribed",
            Self::ReviewRequested => "review requested",
            Self::Mention => "mention",
            Self::Author => "author",
            Self::Comment => "comment",
            Self::Assign => "assigned",
            Self::StateChange => "state change",
            Self::CiActivity => "ci activity",
            Self::TeamMention => "team mention",
            Self::SecurityAlert => "security alert",
            Self::Unknown => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubjectType {
    PullRequest,
    Issue,
    Release,
    Discussion,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AuthorAssociation {
    Collaborator,
    Contributor,
    FirstTimer,
    FirstTimeContributor,
    Mannequin,
    Member,
    None,
    Owner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckStatus {
    Queued,
    InProgress,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    TimedOut,
    ActionRequired,
    Skipped,
    Stale,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommitCheckState {
    Success,
    Failure,
    Pending,
    Error,
    Expected,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileChangeType {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    #[serde(other)]
    Unknown,
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    pub login: String,
    #[serde(default)]
    pub avatar_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoRef {
    pub owner: String,
    pub name: String,
}

impl RepoRef {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// Parse `"owner/name"` into a `RepoRef`.
    pub fn from_full_name(s: &str) -> Option<Self> {
        let (owner, name) = s.split_once('/')?;
        Some(Self {
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    /// Hex color without `#` prefix, as returned by the GitHub API.
    pub color: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub author: Option<Actor>,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Commit {
    pub sha: String,
    pub message: String,
    pub author: Option<String>,
    pub committed_date: Option<DateTime<Utc>>,
    pub check_state: Option<CommitCheckState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckRun {
    pub name: String,
    pub status: Option<CheckStatus>,
    pub conclusion: Option<CheckConclusion>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct File {
    pub path: String,
    pub additions: u32,
    pub deletions: u32,
    pub status: Option<FileChangeType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewThread {
    pub is_resolved: bool,
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
    Pending,
    #[serde(other)]
    Unknown,
}

/// A review on a pull request (from `reviews` connection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub author: Option<Actor>,
    pub state: ReviewState,
    pub body: String,
    pub submitted_at: Option<DateTime<Utc>>,
}

/// A timeline event on a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TimelineEvent {
    Comment {
        author: Option<String>,
        body: String,
        created_at: DateTime<Utc>,
    },
    Review {
        author: Option<String>,
        state: ReviewState,
        body: String,
        submitted_at: DateTime<Utc>,
    },
    Merged {
        actor: Option<String>,
        created_at: DateTime<Utc>,
    },
    Closed {
        actor: Option<String>,
        created_at: DateTime<Utc>,
    },
    Reopened {
        actor: Option<String>,
        created_at: DateTime<Utc>,
    },
    ForcePushed {
        actor: Option<String>,
        created_at: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReactionGroups {
    #[serde(default)]
    pub thumbs_up: u32,
    #[serde(default)]
    pub thumbs_down: u32,
    #[serde(default)]
    pub laugh: u32,
    #[serde(default)]
    pub hooray: u32,
    #[serde(default)]
    pub confused: u32,
    #[serde(default)]
    pub heart: u32,
    #[serde(default)]
    pub rocket: u32,
    #[serde(default)]
    pub eyes: u32,
}

impl ReactionGroups {
    /// Total reaction count across all types.
    pub fn total(&self) -> u32 {
        self.thumbs_up
            + self.thumbs_down
            + self.laugh
            + self.hooray
            + self.confused
            + self.heart
            + self.rocket
            + self.eyes
    }
}

// ---------------------------------------------------------------------------
// Primary domain entities
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub author: Option<Actor>,
    pub state: PrState,
    #[serde(default)]
    pub is_draft: bool,
    pub mergeable: Option<MergeableState>,
    pub review_decision: Option<ReviewDecision>,
    #[serde(default)]
    pub additions: u32,
    #[serde(default)]
    pub deletions: u32,
    #[serde(default)]
    pub head_ref: String,
    #[serde(default)]
    pub base_ref: String,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub assignees: Vec<Actor>,
    #[serde(default)]
    pub commits: Vec<Commit>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub review_threads: Vec<ReviewThread>,
    #[serde(default)]
    pub review_requests: Vec<Actor>,
    #[serde(default)]
    pub reviews: Vec<Review>,
    #[serde(skip)]
    pub timeline_events: Vec<TimelineEvent>,
    #[serde(default)]
    pub files: Vec<File>,
    #[serde(default)]
    pub check_runs: Vec<CheckRun>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub repo: Option<RepoRef>,
    /// Total comment count (from GraphQL `comments { totalCount }`).
    #[serde(default)]
    pub comment_count: u32,
    pub author_association: Option<AuthorAssociation>,
    /// Deduplicated participant logins (from GitHub's `participants` connection).
    #[serde(default)]
    pub participants: Vec<String>,
    /// Merge state from GitHub's `mergeStateStatus` field.
    pub merge_state_status: Option<MergeStateStatus>,
    /// Owner login of the head repository (for fork PRs).
    pub head_repo_owner: Option<String>,
    /// Name of the head repository (for fork PRs).
    pub head_repo_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub author: Option<Actor>,
    pub state: IssueState,
    #[serde(default)]
    pub assignees: Vec<Actor>,
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub reactions: ReactionGroups,
    #[serde(default)]
    pub labels: Vec<Label>,
    pub updated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub url: String,
    pub repo: Option<RepoRef>,
    #[serde(default)]
    pub comment_count: u32,
    /// Deduplicated participant logins (from GitHub's `participants` connection).
    #[serde(default)]
    pub participants: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub subject_type: Option<SubjectType>,
    pub subject_title: String,
    pub reason: NotificationReason,
    #[serde(default)]
    pub unread: bool,
    pub repository: Option<RepoRef>,
    #[serde(default)]
    pub url: String,
    pub updated_at: DateTime<Utc>,
}
