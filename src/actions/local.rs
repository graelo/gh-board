use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use anyhow::{Context, Result};

/// Check whether a repo needs to be cloned (path is configured but doesn't exist yet).
pub fn repo_needs_clone<S: std::hash::BuildHasher>(
    repo_full_name: &str,
    repo_paths: &HashMap<String, PathBuf, S>,
) -> bool {
    repo_paths
        .get(repo_full_name)
        .is_some_and(|p| !p.exists())
}

/// Clone a repo via `gh repo clone` if the target path doesn't exist yet.
///
/// Returns `true` if a clone was performed, `false` if the path already existed.
fn ensure_repo_cloned(repo_full_name: &str, target: &Path, host: &str) -> Result<bool> {
    if target.exists() {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating parent dirs for {}", target.display()))?;
    }
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["repo", "clone", repo_full_name, &target.to_string_lossy()]);
    if host != "github.com" {
        cmd.env("GH_HOST", host);
    }
    let output = cmd.output().context("running gh repo clone")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        anyhow::bail!("clone failed for {repo_full_name}: {stderr}");
    }
    Ok(true)
}

/// Checkout a PR branch locally.
///
/// Always clones the repo first if the configured path doesn't exist yet.
/// The caller is responsible for gating on user confirmation when needed.
pub fn checkout_branch<S: std::hash::BuildHasher>(
    head_ref: &str,
    repo_full_name: &str,
    repo_paths: &HashMap<String, PathBuf, S>,
    host: &str,
) -> Result<String> {
    let repo_path = repo_paths
        .get(repo_full_name)
        .context(format!("no local path configured for {repo_full_name}"))?;

    let cloned = ensure_repo_cloned(repo_full_name, repo_path, host)?;

    let output = std::process::Command::new("git")
        .arg("checkout")
        .arg(head_ref)
        .current_dir(repo_path)
        .output()
        .with_context(|| format!("cannot run git checkout in {}", repo_path.display()))?;

    if output.status.success() {
        if cloned {
            Ok(format!("Cloned {repo_full_name} then checked out {head_ref}"))
        } else {
            Ok(format!("Checked out {head_ref}"))
        }
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
///
/// Always clones the repo first if the configured path doesn't exist yet.
/// The caller is responsible for gating on user confirmation when needed.
pub fn create_or_open_worktree<S: std::hash::BuildHasher>(
    head_ref: &str,
    repo_full_name: &str,
    repo_paths: &HashMap<String, PathBuf, S>,
    host: &str,
) -> Result<String> {
    let repo_path = repo_paths
        .get(repo_full_name)
        .context(format!("no local path configured for {repo_full_name}"))?;

    ensure_repo_cloned(repo_full_name, repo_path, host)?;

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

/// Spawn a background thread that runs [`checkout_branch`] and sends the result
/// (success message or formatted error) through `reply_tx`.
///
/// Returns immediately so the UI can show a progress message before the
/// blocking clone/checkout finishes.
pub fn spawn_checkout<S: std::hash::BuildHasher + Send + 'static>(
    head_ref: String,
    repo_full_name: String,
    repo_paths: HashMap<String, PathBuf, S>,
    host: String,
    reply_tx: Sender<String>,
) {
    std::thread::spawn(move || {
        let msg = match checkout_branch(&head_ref, &repo_full_name, &repo_paths, &host) {
            Ok(m) => m,
            Err(e) => format!("Checkout error: {e:#}"),
        };
        let _ = reply_tx.send(msg);
    });
}

/// Spawn a background thread that runs [`create_or_open_worktree`] and sends the
/// result through `reply_tx`. Copies the worktree path to the clipboard on success.
///
/// Returns immediately so the UI can show a progress message.
pub fn spawn_worktree<S: std::hash::BuildHasher + Send + 'static>(
    head_ref: String,
    repo_full_name: String,
    repo_paths: HashMap<String, PathBuf, S>,
    host: String,
    reply_tx: Sender<String>,
) {
    std::thread::spawn(move || {
        let msg = match create_or_open_worktree(&head_ref, &repo_full_name, &repo_paths, &host) {
            Ok(path) => match crate::actions::clipboard::copy_to_clipboard(&path) {
                Ok(()) => format!("Worktree ready (copied): {path}"),
                Err(e) => format!("Worktree ready: {path} (clipboard: {e})"),
            },
            Err(e) => format!("Worktree error: {e:#}"),
        };
        let _ = reply_tx.send(msg);
    });
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
