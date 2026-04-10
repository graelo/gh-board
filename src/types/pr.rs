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

impl PullRequest {
    /// Returns a [`ForkSource`] when this is a cross-fork PR (head repo owner
    /// differs from the base repo owner).
    pub fn fork_source(&self) -> Option<crate::actions::local::ForkSource> {
        let head_owner = self.head_repo_owner.as_deref()?;
        let base_owner = self.repo.as_ref()?.owner.as_str();
        if head_owner == base_owner {
            return None;
        }
        Some(crate::actions::local::ForkSource {
            owner: head_owner.to_owned(),
            repo_name: self.head_repo_name.clone()?,
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal `PullRequest` for testing `fork_source()`.
    fn pr_stub(
        head_repo_owner: Option<&str>,
        head_repo_name: Option<&str>,
        base_owner: &str,
    ) -> PullRequest {
        PullRequest {
            number: 1,
            title: String::new(),
            body: String::new(),
            author: None,
            state: PrState::Open,
            is_draft: false,
            mergeable: None,
            review_decision: None,
            additions: 0,
            deletions: 0,
            head_ref: "feat/x".into(),
            base_ref: "main".into(),
            labels: vec![],
            assignees: vec![],
            commits: vec![],
            comments: vec![],
            review_threads: vec![],
            review_requests: vec![],
            reviews: vec![],
            timeline_events: vec![],
            files: vec![],
            check_runs: vec![],
            updated_at: Utc::now(),
            created_at: Utc::now(),
            url: String::new(),
            repo: Some(RepoRef {
                owner: base_owner.into(),
                name: "repo".into(),
            }),
            comment_count: 0,
            author_association: None,
            participants: vec![],
            merge_state_status: None,
            head_repo_owner: head_repo_owner.map(Into::into),
            head_repo_name: head_repo_name.map(Into::into),
        }
    }

    #[test]
    fn fork_source_same_repo_returns_none() {
        let pr = pr_stub(Some("graelo"), Some("repo"), "graelo");
        assert!(pr.fork_source().is_none());
    }

    #[test]
    fn fork_source_cross_fork_returns_some() {
        let pr = pr_stub(Some("alice"), Some("repo-fork"), "graelo");
        let fs = pr.fork_source().unwrap();
        assert_eq!(fs.owner, "alice");
        assert_eq!(fs.repo_name, "repo-fork");
    }

    #[test]
    fn fork_source_missing_head_owner_returns_none() {
        let pr = pr_stub(None, Some("repo"), "graelo");
        assert!(pr.fork_source().is_none());
    }

    #[test]
    fn fork_source_missing_head_name_returns_none() {
        let pr = pr_stub(Some("alice"), None, "graelo");
        assert!(pr.fork_source().is_none());
    }
}
