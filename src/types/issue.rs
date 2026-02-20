use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::common::{Actor, Comment, Label, ReactionGroups, RepoRef, TimelineEvent};

// ---------------------------------------------------------------------------
// Issue-specific enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IssueState {
    Open,
    Closed,
    #[serde(other)]
    Unknown,
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

// ---------------------------------------------------------------------------
// Issue domain types
// ---------------------------------------------------------------------------

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

/// Detailed Issue data fetched for the sidebar tabs.
#[derive(Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    pub body: String,
    pub timeline_events: Vec<TimelineEvent>,
}
