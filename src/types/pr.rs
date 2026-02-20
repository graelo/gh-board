use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::common::{
    Actor, CheckRun, Comment, Commit, File, Label, RepoRef, Review, ReviewThread, TimelineEvent,
};

// ---------------------------------------------------------------------------
// PR-specific enums
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

// ---------------------------------------------------------------------------
// PR domain types
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

/// Detailed PR data fetched for the sidebar tabs.
#[derive(Clone, Serialize, Deserialize)]
pub struct PrDetail {
    pub body: String,
    pub reviews: Vec<Review>,
    pub review_threads: Vec<ReviewThread>,
    pub timeline_events: Vec<TimelineEvent>,
    pub commits: Vec<Commit>,
    pub files: Vec<File>,
    /// Mergeability from the detail query (`mergeable` field).
    pub mergeable: Option<MergeableState>,
    /// How many commits behind base this PR is (from REST compare API).
    pub behind_by: Option<u32>,
}
