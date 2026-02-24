use std::fmt::Write as _;
use std::sync::Arc;

use anyhow::{Context, Result};
use octocrab::Octocrab;
use serde::Deserialize;
use serde_json::Value as JsonValue;

use crate::config::types::ActionsFilter;
use crate::github::client::extract_rest_rate_limit;
use crate::types::{
    Actor, JobStep, RateLimitInfo, RunConclusion, RunStatus, WorkflowJob, WorkflowRun,
};

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
    #[serde(default)]
    started_at: Option<chrono::DateTime<chrono::Utc>>,
    #[serde(default)]
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

// ---------------------------------------------------------------------------
// Fetch workflow runs
// ---------------------------------------------------------------------------

/// Fetch workflow runs for the repository described by `filter`.
pub async fn fetch_workflow_runs(
    octocrab: &Arc<Octocrab>,
    filter: &ActionsFilter,
) -> Result<(Vec<WorkflowRun>, Option<RateLimitInfo>)> {
    let (owner, repo) = filter.repo.split_once('/').with_context(|| {
        format!(
            "invalid repo format {:?} — expected owner/repo",
            filter.repo
        )
    })?;

    let per_page = filter.limit.unwrap_or(30).min(100);

    let mut qs = format!("per_page={per_page}&page=1");
    if let Some(ref status) = filter.status {
        write!(qs, "&status={status}").expect("write to String is infallible");
    }
    if let Some(ref event) = filter.event {
        write!(qs, "&event={event}").expect("write to String is infallible");
    }

    let url = format!("/repos/{owner}/{repo}/actions/runs?{qs}");
    let response = octocrab._get(url).await.context("fetching workflow runs")?;

    let rate_limit = extract_rest_rate_limit(response.headers());
    let body = octocrab
        .body_to_string(response)
        .await
        .context("reading workflow runs body")?;
    let parsed: RawWorkflowRunsResponse =
        serde_json::from_str(&body).context("deserializing workflow runs")?;

    let runs = parsed.workflow_runs.into_iter().map(into_domain).collect();
    Ok((runs, rate_limit))
}

/// Fetch the jobs for a specific workflow run.
pub async fn fetch_run_jobs(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<(Vec<WorkflowJob>, Option<RateLimitInfo>)> {
    let url = format!("/repos/{owner}/{repo}/actions/runs/{run_id}/jobs?per_page=100");
    let response = octocrab._get(url).await.context("fetching run jobs")?;

    let rate_limit = extract_rest_rate_limit(response.headers());
    let body = octocrab
        .body_to_string(response)
        .await
        .context("reading run jobs body")?;
    let parsed: RawJobsResponse = serde_json::from_str(&body).context("deserializing run jobs")?;

    let jobs = parsed
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
                    started_at: s.started_at,
                    completed_at: s.completed_at,
                })
                .collect(),
        })
        .collect();
    Ok((jobs, rate_limit))
}

/// Fetch a single workflow run by ID.
pub async fn fetch_run_by_id(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    run_id: u64,
) -> Result<(WorkflowRun, Option<RateLimitInfo>)> {
    let url = format!("/repos/{owner}/{repo}/actions/runs/{run_id}");
    let response = octocrab
        ._get(url)
        .await
        .context("fetching single workflow run")?;

    let rate_limit = extract_rest_rate_limit(response.headers());
    let body = octocrab
        .body_to_string(response)
        .await
        .context("reading single workflow run body")?;
    let raw: RawWorkflowRun =
        serde_json::from_str(&body).context("deserializing single workflow run")?;

    Ok((into_domain(raw), rate_limit))
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
