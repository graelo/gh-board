use indexmap::IndexMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use anyhow::{Context, Result};

/// Fork from which a cross-fork PR originates.
#[derive(Debug, Clone)]
pub struct ForkSource {
    /// Fork owner login — also used as the git remote name.
    pub owner: String,
    /// Repository name on the fork.
    pub repo_name: String,
}

/// Query the user's preferred git protocol (`ssh` or `https`) via `gh config`.
fn detect_git_protocol(host: &str) -> String {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["config", "get", "git_protocol"]);
    if host != "github.com" {
        cmd.env("GH_HOST", host);
    }
    cmd.output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "https".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}

/// Build a remote URL for a fork, respecting the chosen protocol.
fn fork_remote_url(host: &str, fork: &ForkSource, protocol: &str) -> String {
    if protocol == "ssh" {
        format!("git@{host}:{}/{}.git", fork.owner, fork.repo_name)
    } else {
        format!("https://{host}/{}/{}.git", fork.owner, fork.repo_name)
    }
}

/// Idempotently add the fork owner as a git remote in `repo_path`.
fn ensure_fork_remote(repo_path: &Path, host: &str, fork: &ForkSource) -> Result<()> {
    // Check whether the remote already exists.
    let check = std::process::Command::new("git")
        .args(["remote", "get-url", &fork.owner])
        .current_dir(repo_path)
        .output()
        .context("checking fork remote")?;

    if check.status.success() {
        return Ok(());
    }

    let protocol = detect_git_protocol(host);
    let url = fork_remote_url(host, fork, &protocol);

    let add = std::process::Command::new("git")
        .args(["remote", "add", &fork.owner, &url])
        .current_dir(repo_path)
        .output()
        .with_context(|| format!("adding remote {} → {url}", fork.owner))?;

    if !add.status.success() {
        let stderr = String::from_utf8_lossy(&add.stderr).trim().to_owned();
        anyhow::bail!("git remote add {} failed: {stderr}", fork.owner);
    }

    Ok(())
}

/// Check whether a repo needs to be cloned (path is configured but doesn't exist yet).
pub fn repo_needs_clone(repo_full_name: &str, repo_paths: &IndexMap<String, PathBuf>) -> bool {
    repo_paths.get(repo_full_name).is_some_and(|p| !p.exists())
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
pub fn checkout_branch(
    head_ref: &str,
    repo_full_name: &str,
    repo_paths: &IndexMap<String, PathBuf>,
    host: &str,
    fork: Option<&ForkSource>,
) -> Result<String> {
    let repo_path = repo_paths
        .get(repo_full_name)
        .with_context(|| format!("no local path configured for {repo_full_name}"))?;

    let cloned = ensure_repo_cloned(repo_full_name, repo_path, host)?;

    let remote_name = if let Some(fork) = fork {
        ensure_fork_remote(repo_path, host, fork)?;
        fork.owner.as_str()
    } else {
        "origin"
    };

    // Fetch the branch so the remote-tracking ref is up to date.
    let _fetch = std::process::Command::new("git")
        .args(["fetch", remote_name, head_ref])
        .current_dir(repo_path)
        .output();

    // For cross-fork PRs, create a local tracking branch explicitly to avoid
    // ambiguity when `origin` also has a branch with the same name.
    let output = if fork.is_some() {
        let tracking_ref = format!("{remote_name}/{head_ref}");
        let try_create = std::process::Command::new("git")
            .args(["checkout", "-b", head_ref, "--track", &tracking_ref])
            .current_dir(repo_path)
            .output()
            .with_context(|| format!("cannot run git checkout in {}", repo_path.display()))?;

        if try_create.status.success() {
            try_create
        } else {
            // Branch already exists locally — just switch to it.
            std::process::Command::new("git")
                .args(["checkout", head_ref])
                .current_dir(repo_path)
                .output()
                .with_context(|| format!("cannot run git checkout in {}", repo_path.display()))?
        }
    } else {
        std::process::Command::new("git")
            .args(["checkout", head_ref])
            .current_dir(repo_path)
            .output()
            .with_context(|| format!("cannot run git checkout in {}", repo_path.display()))?
    };

    if output.status.success() {
        if cloned {
            Ok(format!(
                "Cloned {repo_full_name} then checked out {head_ref}"
            ))
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

/// Create or locate a git worktree for a branch at the given repo path.
///
/// Worktrees are placed under `<repo_parent>/<repo_dir_name>-worktrees/<branch-slug>/`.
/// If the worktree already exists, the path is returned immediately (idempotent).
///
/// The caller is responsible for ensuring the repo exists on disk and for gating
/// on user confirmation when needed.
pub fn create_worktree_at(
    branch: &str,
    repo_path: &Path,
    host: &str,
    fork: Option<&ForkSource>,
) -> Result<String> {
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
    let slug = slugify_branch(branch);
    let worktree_path = worktree_base.join(&slug);

    if worktree_path.exists() {
        return Ok(worktree_path.to_string_lossy().into_owned());
    }

    std::fs::create_dir_all(&worktree_base).context("creating worktree base directory")?;

    let remote_name = if let Some(fork) = fork {
        ensure_fork_remote(&repo_path, host, fork)?;
        fork.owner.as_str()
    } else {
        "origin"
    };

    // Best-effort fetch — ignore errors for local-only branches.
    let _ = std::process::Command::new("git")
        .args(["fetch", remote_name, branch])
        .current_dir(&repo_path)
        .output();

    let add = if fork.is_some() {
        // Create a local branch tracking the fork's remote branch.
        let start_point = format!("{remote_name}/{branch}");
        std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                branch,
                &worktree_path.to_string_lossy(),
                &start_point,
            ])
            .current_dir(&repo_path)
            .output()
            .context("running git worktree add")?
    } else {
        std::process::Command::new("git")
            .args(["worktree", "add", &worktree_path.to_string_lossy(), branch])
            .current_dir(&repo_path)
            .output()
            .context("running git worktree add")?
    };

    if !add.status.success() {
        let stderr = String::from_utf8_lossy(&add.stderr);
        anyhow::bail!("git worktree add failed: {stderr}");
    }

    Ok(worktree_path.to_string_lossy().into_owned())
}

/// Create or locate a git worktree for a PR branch.
///
/// Worktrees are placed under `<repo_parent>/<repo_dir_name>-worktrees/<branch-slug>/`.
/// If the worktree already exists, the path is returned immediately (idempotent).
///
/// Always clones the repo first if the configured path doesn't exist yet.
/// The caller is responsible for gating on user confirmation when needed.
pub fn create_or_open_worktree(
    head_ref: &str,
    repo_full_name: &str,
    repo_paths: &IndexMap<String, PathBuf>,
    host: &str,
    fork: Option<&ForkSource>,
) -> Result<String> {
    let repo_path = repo_paths
        .get(repo_full_name)
        .with_context(|| format!("no local path configured for {repo_full_name}"))?;

    ensure_repo_cloned(repo_full_name, repo_path, host)?;

    create_worktree_at(head_ref, repo_path, host, fork)
}

/// Spawn a background thread that runs [`checkout_branch`] and sends the result
/// (success message or formatted error) through `reply_tx`.
///
/// Returns immediately so the UI can show a progress message before the
/// blocking clone/checkout finishes.
pub fn spawn_checkout(
    head_ref: String,
    repo_full_name: String,
    repo_paths: IndexMap<String, PathBuf>,
    host: String,
    fork: Option<ForkSource>,
    reply_tx: Sender<String>,
) {
    std::thread::spawn(move || {
        let msg = match checkout_branch(
            &head_ref,
            &repo_full_name,
            &repo_paths,
            &host,
            fork.as_ref(),
        ) {
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
pub fn spawn_worktree(
    head_ref: String,
    repo_full_name: String,
    repo_paths: IndexMap<String, PathBuf>,
    host: String,
    fork: Option<ForkSource>,
    reply_tx: Sender<String>,
) {
    std::thread::spawn(move || {
        let msg = match create_or_open_worktree(
            &head_ref,
            &repo_full_name,
            &repo_paths,
            &host,
            fork.as_ref(),
        ) {
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

    #[test]
    fn fork_remote_url_https() {
        let fork = ForkSource {
            owner: "alice".into(),
            repo_name: "my-repo".into(),
        };
        assert_eq!(
            fork_remote_url("github.com", &fork, "https"),
            "https://github.com/alice/my-repo.git"
        );
    }

    #[test]
    fn fork_remote_url_ssh() {
        let fork = ForkSource {
            owner: "alice".into(),
            repo_name: "my-repo".into(),
        };
        assert_eq!(
            fork_remote_url("github.com", &fork, "ssh"),
            "git@github.com:alice/my-repo.git"
        );
    }

    #[test]
    fn fork_remote_url_ghe_ssh() {
        let fork = ForkSource {
            owner: "bob".into(),
            repo_name: "internal-tool".into(),
        };
        assert_eq!(
            fork_remote_url("github.corp.example.com", &fork, "ssh"),
            "git@github.corp.example.com:bob/internal-tool.git"
        );
    }
}
