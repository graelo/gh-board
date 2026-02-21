use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// Checkout a PR branch locally.
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

/// Open a PR diff in the configured pager.
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
