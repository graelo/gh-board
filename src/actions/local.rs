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
        .with_context(|| format!("cannot run git checkout in {}", repo_path.display()))?;

    if output.status.success() {
        Ok(format!("Checked out {head_ref}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        if stderr.contains("did not match any") {
            anyhow::bail!(
                "branch '{head_ref}' not found locally — it may have been deleted from the remote"
            )
        }
        anyhow::bail!("git checkout failed: {stderr}")
    }
}

/// Create or locate a git worktree for a PR branch.
///
/// Worktrees are placed under `<repo_parent>/<repo_dir_name>-worktrees/<branch-slug>/`.
/// If the worktree already exists, the path is returned immediately (idempotent).
pub fn create_or_open_worktree<S: std::hash::BuildHasher>(
    head_ref: &str,
    repo_full_name: &str,
    repo_paths: &HashMap<String, PathBuf, S>,
) -> Result<String> {
    let repo_path = repo_paths
        .get(repo_full_name)
        .context(format!("no local path configured for {repo_full_name}"))?;

    let repo_path = repo_path
        .canonicalize()
        .context("canonicalizing repo path")?;

    let parent = repo_path
        .parent()
        .context("repo path has no parent directory")?;

    let dir_name = repo_path
        .file_name()
        .context("repo path has no directory name")?
        .to_string_lossy();

    let worktree_base = parent.join(format!("{dir_name}-worktrees"));
    let slug = slugify_branch(head_ref);
    let worktree_path = worktree_base.join(&slug);

    if worktree_path.exists() {
        return Ok(worktree_path.to_string_lossy().into_owned());
    }

    std::fs::create_dir_all(&worktree_base).context("creating worktree base directory")?;

    let fetch = std::process::Command::new("git")
        .args(["fetch", "origin", head_ref])
        .current_dir(&repo_path)
        .output()
        .context("running git fetch")?;

    if !fetch.status.success() {
        let stderr = String::from_utf8_lossy(&fetch.stderr);
        anyhow::bail!("git fetch failed: {stderr}");
    }

    let add = std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            &worktree_path.to_string_lossy(),
            head_ref,
        ])
        .current_dir(&repo_path)
        .output()
        .context("running git worktree add")?;

    if !add.status.success() {
        let stderr = String::from_utf8_lossy(&add.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    Ok(worktree_path.to_string_lossy().into_owned())
}

fn slugify_branch(branch: &str) -> String {
    branch.replace('/', "-").trim_matches('-').to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_simple_branch() {
        assert_eq!(slugify_branch("feat/my-feature"), "feat-my-feature");
    }

    #[test]
    fn slugify_nested_slashes() {
        assert_eq!(
            slugify_branch("user/feat/deep/branch"),
            "user-feat-deep-branch"
        );
    }

    #[test]
    fn slugify_no_slashes() {
        assert_eq!(slugify_branch("main"), "main");
    }

    #[test]
    fn slugify_leading_trailing_slashes() {
        assert_eq!(slugify_branch("/leading/trailing/"), "leading-trailing");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify_branch(""), "");
    }
}
