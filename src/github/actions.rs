use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use octocrab::Octocrab;
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::config::types::ActionsFilter;
use crate::types::{Actor, JobStep, RunConclusion, RunStatus, WorkflowJob, WorkflowRun};

// ---------------------------------------------------------------------------
// Raw API response types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawWorkflowRunsResponse {
    workflow_runs: Vec<RawWorkflowRun>,
}

#[derive(Deserialize)]
struct RawWorkflowRun {
    id: u64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    display_title: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    event: String,
    #[serde(default)]
    head_branch: Option<String>,
    #[serde(default)]
    actor: Option<RawActor>,
    run_number: u64,
    #[serde(default)]
    html_url: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct RawActor {
    login: String,
    #[serde(default)]
    avatar_url: String,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn parse_status(s: &str) -> RunStatus {
    match s {
        "queued" => RunStatus::Queued,
        "in_progress" => RunStatus::InProgress,
        "completed" => RunStatus::Completed,
        _ => RunStatus::Unknown,
    }
}

fn parse_conclusion(s: &str) -> RunConclusion {
    match s {
        "success" => RunConclusion::Success,
        "failure" => RunConclusion::Failure,
        "neutral" => RunConclusion::Neutral,
        "cancelled" => RunConclusion::Cancelled,
        "timed_out" => RunConclusion::TimedOut,
        "action_required" => RunConclusion::ActionRequired,
        "skipped" => RunConclusion::Skipped,
        "stale" => RunConclusion::Stale,
        _ => RunConclusion::Unknown,
    }
}

fn into_domain(raw: RawWorkflowRun) -> WorkflowRun {
    WorkflowRun {
        id: raw.id,
        name: raw.name.unwrap_or_default(),
        display_title: raw.display_title,
        status: raw
            .status
            .as_deref()
            .map_or(RunStatus::Unknown, parse_status),
        conclusion: raw.conclusion.as_deref().map(parse_conclusion),
        event: raw.event,
        head_branch: raw.head_branch,
        actor: raw.actor.map(|a| Actor {
            login: a.login,
            avatar_url: a.avatar_url,
        }),
        run_number: raw.run_number,
        html_url: raw.html_url,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Raw types for jobs API
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawJobsResponse {
    jobs: Vec<RawJob>,
}

#[derive(Deserialize)]
struct RawJob {
    id: u64,
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    steps: Vec<RawStep>,
}

#[derive(Deserialize)]
struct RawStep {
    name: String,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    number: u32,
}

// ---------------------------------------------------------------------------
// Fetch workflow runs
// ---------------------------------------------------------------------------

/// Fetch workflow runs for the repository described by `filter`.
pub async fn fetch_workflow_runs(
    octocrab: &Arc<Octocrab>,
    filter: &ActionsFilter,
) -> Result<Vec<WorkflowRun>> {
    let (owner, repo) = filter.repo.split_once('/').with_context(|| {
        format!(
            "invalid repo format {:?} — expected owner/repo",
            filter.repo
        )
    })?;

    let per_page = filter.limit.unwrap_or(30).min(100);

    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("per_page", per_page.to_string());
    params.insert("page", "1".to_owned());
    if let Some(ref status) = filter.status {
        params.insert("status", status.clone());
    }
    if let Some(ref event) = filter.event {
        params.insert("event", event.clone());
    }

    let route = format!("/repos/{owner}/{repo}/actions/runs");
    let response: RawWorkflowRunsResponse = octocrab
        .get(route, Some(&params))
        .await
        .context("fetching workflow runs")?;

    Ok(response
        .workflow_runs
        .into_iter()
        .map(into_domain)
        .collect())
}

/// Fetch the jobs for a specific workflow run.
pub async fn fetch_run_jobs(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<Vec<WorkflowJob>> {
    let mut params: HashMap<&str, String> = HashMap::new();
    params.insert("per_page", "100".to_owned());

    let route = format!("/repos/{owner}/{repo}/actions/runs/{run_id}/jobs");
    let response: RawJobsResponse = octocrab
        .get(route, Some(&params))
        .await
        .context("fetching run jobs")?;

    let jobs = response
        .jobs
        .into_iter()
        .map(|j| WorkflowJob {
            id: j.id,
            name: j.name,
            status: j.status.as_deref().map_or(RunStatus::Unknown, parse_status),
            conclusion: j.conclusion.as_deref().map(parse_conclusion),
            started_at: j.started_at,
            completed_at: j.completed_at,
            html_url: j.html_url,
            steps: j
                .steps
                .into_iter()
                .map(|s| JobStep {
                    name: s.name,
                    status: s.status.as_deref().map_or(RunStatus::Unknown, parse_status),
                    conclusion: s.conclusion.as_deref().map(parse_conclusion),
                    number: s.number,
                })
                .collect(),
        })
        .collect();
    Ok(jobs)
}

/// Re-run a workflow run (all jobs or failed jobs only).
pub async fn rerun_workflow_run(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    run_id: u64,
    failed_only: bool,
) -> Result<()> {
    let suffix = if failed_only {
        "rerun-failed-jobs"
    } else {
        "rerun"
    };
    let route = format!("/repos/{owner}/{repo}/actions/runs/{run_id}/{suffix}");
    let _: JsonValue = octocrab
        .post(route, None::<&()>)
        .await
        .context("rerunning workflow run")?;
    Ok(())
}

/// Cancel a workflow run.
pub async fn cancel_workflow_run(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/actions/runs/{run_id}/cancel");
    let _: JsonValue = octocrab
        .post(route, None::<&()>)
        .await
        .context("cancelling workflow run")?;
    Ok(())
}
