use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::common::Actor;

// ---------------------------------------------------------------------------
// WorkflowRun-specific enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    InProgress,
    Completed,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunConclusion {
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

// ---------------------------------------------------------------------------
// WorkflowJob and JobStep domain types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStep {
    pub name: String,
    pub status: RunStatus,
    pub conclusion: Option<RunConclusion>,
    pub number: u32,
    #[serde(default)]
    pub started_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowJob {
    pub id: u64,
    pub name: String,
    pub status: RunStatus,
    pub conclusion: Option<RunConclusion>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub html_url: String,
    pub steps: Vec<JobStep>,
}

// ---------------------------------------------------------------------------
// WorkflowRun domain type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    /// Workflow file name / workflow display name.
    pub name: String,
    /// Commit message head / trigger title shown in the GitHub UI.
    pub display_title: String,
    pub status: RunStatus,
    pub conclusion: Option<RunConclusion>,
    /// Trigger event: `"push"`, `"pull_request"`, `"schedule"`, …
    pub event: String,
    pub head_branch: Option<String>,
    pub actor: Option<Actor>,
    pub run_number: u64,
    /// URL used for the `o` keybinding (open in browser).
    pub html_url: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
