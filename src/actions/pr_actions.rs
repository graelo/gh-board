use std::sync::Arc;

use anyhow::{Context, Result};
use octocrab::Octocrab;

// ---------------------------------------------------------------------------
// PR action API calls (T057)
// ---------------------------------------------------------------------------

/// Approve a pull request.
pub async fn approve(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    body: Option<&str>,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/pulls/{number}/reviews");
    let payload = serde_json::json!({
        "event": "APPROVE",
        "body": body.unwrap_or(""),
    });
    let _: serde_json::Value = octocrab
        .post(route, Some(&payload))
        .await
        .context("approving PR")?;
    Ok(())
}

/// Add a comment to a PR (via issues endpoint).
pub async fn add_comment(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    body: &str,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/issues/{number}/comments");
    let payload = serde_json::json!({ "body": body });
    let _: serde_json::Value = octocrab
        .post(route, Some(&payload))
        .await
        .context("adding comment")?;
    Ok(())
}

/// Merge a pull request.
pub async fn merge(octocrab: &Arc<Octocrab>, owner: &str, repo: &str, number: u64) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/pulls/{number}/merge");
    let payload = serde_json::json!({
        "merge_method": "merge",
    });
    let _: serde_json::Value = octocrab
        .put(route, Some(&payload))
        .await
        .context("merging PR")?;
    Ok(())
}

/// Close a pull request.
pub async fn close(octocrab: &Arc<Octocrab>, owner: &str, repo: &str, number: u64) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/pulls/{number}");
    let payload = serde_json::json!({ "state": "closed" });
    let _: serde_json::Value = octocrab
        .patch(route, Some(&payload))
        .await
        .context("closing PR")?;
    Ok(())
}

/// Reopen a pull request.
pub async fn reopen(octocrab: &Arc<Octocrab>, owner: &str, repo: &str, number: u64) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/pulls/{number}");
    let payload = serde_json::json!({ "state": "open" });
    let _: serde_json::Value = octocrab
        .patch(route, Some(&payload))
        .await
        .context("reopening PR")?;
    Ok(())
}

/// Update a PR branch from the base branch.
pub async fn update_branch(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/pulls/{number}/update-branch");
    let _: serde_json::Value = octocrab
        .put(route, None::<&()>)
        .await
        .context("updating branch")?;
    Ok(())
}

/// Mark a draft PR as ready for review.
pub async fn ready_for_review(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<()> {
    // Uses the GraphQL API to mark as ready for review.
    let query = r"mutation($id: ID!) {
        markPullRequestReadyForReview(input: { pullRequestId: $id }) {
            pullRequest { id }
        }
    }";

    // First get the node ID.
    let route = format!("/repos/{owner}/{repo}/pulls/{number}");
    let pr: serde_json::Value = octocrab
        .get(route, None::<&()>)
        .await
        .context("fetching PR for node_id")?;
    let node_id = pr["node_id"].as_str().context("PR missing node_id")?;

    let variables = serde_json::json!({ "id": node_id });
    let payload = serde_json::json!({
        "query": query,
        "variables": variables,
    });
    let _: serde_json::Value = octocrab
        .post("/graphql", Some(&payload))
        .await
        .context("marking PR as ready for review")?;
    Ok(())
}
