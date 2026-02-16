use std::collections::HashMap;
use std::path::PathBuf;
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

/// Assign users to a PR.
pub async fn assign(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    logins: &[String],
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/issues/{number}/assignees");
    let payload = serde_json::json!({ "assignees": logins });
    let _: serde_json::Value = octocrab
        .post(route, Some(&payload))
        .await
        .context("assigning users")?;
    Ok(())
}

/// Unassign a user from a PR.
pub async fn unassign(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    login: &str,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/issues/{number}/assignees");
    let payload = serde_json::json!({ "assignees": [login] });
    let _: serde_json::Value = octocrab
        .delete(route, Some(&payload))
        .await
        .context("unassigning user")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Local actions (T059, T060)
// ---------------------------------------------------------------------------

/// Checkout a PR branch locally (T059).
pub fn checkout_branch<S: std::hash::BuildHasher>(
    head_ref: &str,
    repo_full_name: &str,
    repo_paths: &HashMap<String, PathBuf, S>,
) -> Result<String> {
    let repo_path = repo_paths
        .get(repo_full_name)
        .context(format!("no local path configured for {repo_full_name}"))?;

    let output = std::process::Command::new("git")
        .arg("checkout")
        .arg(head_ref)
        .current_dir(repo_path)
        .output()
        .context("running git checkout")?;

    if output.status.success() {
        Ok(format!("Checked out {head_ref}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git checkout failed: {stderr}")
    }
}

/// Open a PR diff in the configured pager (T060).
pub fn open_diff(owner: &str, repo: &str, number: u64) -> Result<String> {
    let output = std::process::Command::new("gh")
        .args([
            "pr",
            "diff",
            &number.to_string(),
            "--repo",
            &format!("{owner}/{repo}"),
        ])
        .output()
        .context("running gh pr diff")?;

    if output.status.success() {
        Ok("Diff opened".to_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("gh pr diff failed: {stderr}")
    }
}
