use std::sync::Arc;

use anyhow::{Context, Result};
use octocrab::Octocrab;

// ---------------------------------------------------------------------------
// Issue action API calls (T084)
// ---------------------------------------------------------------------------

/// Close an issue.
pub(crate) async fn close(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/issues/{number}");
    let payload = serde_json::json!({ "state": "closed" });
    let _: serde_json::Value = octocrab
        .patch(route, Some(&payload))
        .await
        .context("closing issue")?;
    Ok(())
}

/// Reopen an issue.
pub(crate) async fn reopen(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/issues/{number}");
    let payload = serde_json::json!({ "state": "open" });
    let _: serde_json::Value = octocrab
        .patch(route, Some(&payload))
        .await
        .context("reopening issue")?;
    Ok(())
}

/// Add a comment to an issue.
pub(crate) async fn add_comment(
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
        .context("adding comment to issue")?;
    Ok(())
}

/// Replace all labels on an issue/PR (PUT semantics — removes labels not in the list).
pub(crate) async fn set_labels(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    labels: &[String],
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/issues/{number}/labels");
    let payload = serde_json::json!({ "labels": labels });
    let _: serde_json::Value = octocrab
        .put(route, Some(&payload))
        .await
        .context("setting labels on issue")?;
    Ok(())
}

/// Replace all assignees on an issue/PR. An empty `logins` slice unassigns everyone.
pub(crate) async fn set_assignees(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    logins: &[String],
) -> Result<()> {
    let route = format!("/repos/{owner}/{repo}/issues/{number}");
    let payload = serde_json::json!({ "assignees": logins });
    let _: serde_json::Value = octocrab
        .patch(route, Some(&payload))
        .await
        .context("setting assignees on issue")?;
    Ok(())
}
