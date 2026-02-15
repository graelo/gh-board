use std::path::Path;
use std::process::Command;

use crate::github::types::RepoRef;

/// Detect `owner/repo` from the git remote of the directory at `path`.
///
/// Tries the `origin` remote first, falls back to the first listed remote.
/// Parses both SSH (`git@github.com:owner/repo.git`) and HTTPS
/// (`https://github.com/owner/repo.git`) URL formats.
pub fn detect_repo(path: &Path) -> Option<RepoRef> {
    let url = remote_url(path, "origin").or_else(|| {
        let first = first_remote_name(path)?;
        remote_url(path, &first)
    })?;
    parse_remote_url(&url)
}

/// Run `git remote get-url <remote>` in the given directory.
fn remote_url(path: &Path, remote: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", remote])
        .current_dir(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if url.is_empty() { None } else { Some(url) }
}

/// Return the name of the first listed remote.
fn first_remote_name(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote"])
        .current_dir(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()?
        .trim()
        .to_owned();
    if name.is_empty() { None } else { Some(name) }
}

/// Parse `owner/repo` from an SSH or HTTPS remote URL.
fn parse_remote_url(url: &str) -> Option<RepoRef> {
    let slug = if let Some(rest) = url.strip_prefix("git@") {
        // SSH: git@github.com:owner/repo.git
        rest.split_once(':')?.1
    } else if url.starts_with("https://") || url.starts_with("http://") {
        // HTTPS: https://github.com/owner/repo.git
        // Skip scheme + host: find the third '/'
        let after_scheme = url.split_once("://")?.1;
        after_scheme.split_once('/')?.1
    } else {
        return None;
    };

    let slug = slug.strip_suffix(".git").unwrap_or(slug);
    RepoRef::from_full_name(slug)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ssh_url() {
        let r = parse_remote_url("git@github.com:graelo/gh-board.git").unwrap();
        assert_eq!(r.owner, "graelo");
        assert_eq!(r.name, "gh-board");
    }

    #[test]
    fn parse_ssh_url_no_git_suffix() {
        let r = parse_remote_url("git@github.com:graelo/gh-board").unwrap();
        assert_eq!(r.owner, "graelo");
        assert_eq!(r.name, "gh-board");
    }

    #[test]
    fn parse_https_url() {
        let r = parse_remote_url("https://github.com/graelo/gh-board.git").unwrap();
        assert_eq!(r.owner, "graelo");
        assert_eq!(r.name, "gh-board");
    }

    #[test]
    fn parse_https_url_no_git_suffix() {
        let r = parse_remote_url("https://github.com/graelo/gh-board").unwrap();
        assert_eq!(r.owner, "graelo");
        assert_eq!(r.name, "gh-board");
    }

    #[test]
    fn parse_invalid_url() {
        assert!(parse_remote_url("not-a-url").is_none());
    }

    #[test]
    fn detect_repo_returns_none_for_no_remote() {
        // If no remotes are configured, detect_repo should return None.
        let tmp = std::env::temp_dir().join("gh-board-test-no-remote");
        let _ = std::fs::create_dir_all(&tmp);
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&tmp)
            .output();
        let result = detect_repo(&tmp);
        assert!(result.is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
