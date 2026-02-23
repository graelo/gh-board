use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Enums supporting common structs
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Common supporting types
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
    /// Actions workflow run ID; `None` for non-Actions checks.
    #[serde(default)]
    pub workflow_run_id: Option<u64>,
    /// Workflow display name (e.g., "Essentials"); `None` for non-Actions checks.
    #[serde(default)]
    pub workflow_name: Option<String>,
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

/// A review on a pull request (from `reviews` connection).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub author: Option<Actor>,
    pub state: ReviewState,
    pub body: String,
    pub submitted_at: Option<DateTime<Utc>>,
}

/// A timeline event on a pull request or issue.
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

/// Public rate limit info extracted from GraphQL responses.
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    pub limit: u32,
    pub remaining: u32,
    pub cost: u32,
}
