/// Parsed GitHub URL for deep-linking into gh-board views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedGitHubUrl {
    PullRequest {
        host: Option<String>,
        owner: String,
        repo: String,
        number: u64,
    },
    Issue {
        host: Option<String>,
        owner: String,
        repo: String,
        number: u64,
    },
    ActionsRun {
        host: Option<String>,
        owner: String,
        repo: String,
        run_id: u64,
    },
}

/// Extract `(owner, repo)` from a GitHub URL.
///
/// Accepts both `https://` and `http://` schemes. Returns `None` when the URL
/// is malformed or does not contain at least an `owner/repo` path.
pub(crate) fn owner_repo_from_url(url: &str) -> Option<(String, String)> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let after_host = after_scheme.split_once('/')?.1;
    let mut parts = after_host.splitn(3, '/');
    let owner = parts.next().filter(|s| !s.is_empty())?;
    let repo = parts.next().filter(|s| !s.is_empty())?;
    Some((owner.to_owned(), repo.to_owned()))
}

/// Parse a GitHub URL into a structured deep-link target.
///
/// Supported patterns:
/// - `https://<host>/<owner>/<repo>/pull/<number>`
/// - `https://<host>/<owner>/<repo>/issues/<number>`
/// - `https://<host>/<owner>/<repo>/actions/runs/<run_id>`
///
/// Both `https://` and `http://` schemes are accepted (the latter for
/// convenience when pasting URLs from dev-tools or non-standard setups).
/// Query strings (`?tab=files`) and fragments (`#L42`) are stripped before
/// parsing, so browser-copied URLs work as expected.
///
/// Returns `None` for unrecognised or malformed URLs.
pub fn parse_github_url(url: &str) -> Option<ParsedGitHubUrl> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;

    // Split into host and remaining path.
    let (host_str, path) = after_scheme.split_once('/')?;
    if host_str.is_empty() {
        return None;
    }

    // Strip query string and fragment before splitting into segments.
    let path = path.split_once('?').map_or(path, |(p, _)| p);
    let path = path.split_once('#').map_or(path, |(p, _)| p);

    let segments: Vec<&str> = path.split('/').collect();

    // Need at least owner/repo/<type>/<id>.
    if segments.len() < 4 {
        return None;
    }

    let owner = segments[0];
    let repo = segments[1];
    if owner.is_empty() || repo.is_empty() {
        return None;
    }

    let host = if host_str == "github.com" {
        None
    } else {
        Some(host_str.to_owned())
    };

    match segments[2] {
        "pull" => {
            let number = segments[3].parse::<u64>().ok()?;
            Some(ParsedGitHubUrl::PullRequest {
                host,
                owner: owner.to_owned(),
                repo: repo.to_owned(),
                number,
            })
        }
        "issues" => {
            let number = segments[3].parse::<u64>().ok()?;
            Some(ParsedGitHubUrl::Issue {
                host,
                owner: owner.to_owned(),
                repo: repo.to_owned(),
                number,
            })
        }
        "actions" if segments.len() >= 5 && segments[3] == "runs" => {
            let run_id = segments[4].parse::<u64>().ok()?;
            Some(ParsedGitHubUrl::ActionsRun {
                host,
                owner: owner.to_owned(),
                repo: repo.to_owned(),
                run_id,
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pr_url() {
        let url = "https://github.com/graelo/gh-board/pull/42";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::PullRequest {
                host: None,
                owner: "graelo".to_owned(),
                repo: "gh-board".to_owned(),
                number: 42,
            })
        );
    }

    #[test]
    fn parse_issue_url() {
        let url = "https://github.com/rust-lang/rust/issues/1234";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::Issue {
                host: None,
                owner: "rust-lang".to_owned(),
                repo: "rust".to_owned(),
                number: 1234,
            })
        );
    }

    #[test]
    fn parse_actions_run_url() {
        let url = "https://github.com/graelo/gh-board/actions/runs/9876543210";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::ActionsRun {
                host: None,
                owner: "graelo".to_owned(),
                repo: "gh-board".to_owned(),
                run_id: 9_876_543_210,
            })
        );
    }

    #[test]
    fn parse_ghe_pr_url() {
        let url = "https://git.corp.example.com/team/project/pull/7";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::PullRequest {
                host: Some("git.corp.example.com".to_owned()),
                owner: "team".to_owned(),
                repo: "project".to_owned(),
                number: 7,
            })
        );
    }

    #[test]
    fn parse_http_url() {
        let url = "http://github.com/owner/repo/issues/99";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::Issue {
                host: None,
                owner: "owner".to_owned(),
                repo: "repo".to_owned(),
                number: 99,
            })
        );
    }

    #[test]
    fn parse_invalid_urls() {
        assert_eq!(parse_github_url("not-a-url"), None);
        assert_eq!(parse_github_url("https://github.com/owner"), None);
        assert_eq!(parse_github_url("https://github.com/owner/repo"), None);
        assert_eq!(
            parse_github_url("https://github.com/owner/repo/tree/main"),
            None
        );
        assert_eq!(
            parse_github_url("https://github.com/owner/repo/pull/abc"),
            None
        );
        assert_eq!(
            parse_github_url("https://github.com/owner/repo/actions/runs/abc"),
            None
        );
    }

    #[test]
    fn parse_url_with_trailing_segments() {
        // URLs with extra path segments (e.g. /files) should still parse the number.
        let url = "https://github.com/owner/repo/pull/42/files";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::PullRequest {
                host: None,
                owner: "owner".to_owned(),
                repo: "repo".to_owned(),
                number: 42,
            })
        );
    }

    #[test]
    fn parse_url_with_query_string() {
        let url = "https://github.com/owner/repo/pull/42?tab=files";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::PullRequest {
                host: None,
                owner: "owner".to_owned(),
                repo: "repo".to_owned(),
                number: 42,
            })
        );
    }

    #[test]
    fn parse_url_with_fragment() {
        let url = "https://github.com/owner/repo/issues/99#issuecomment-123";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::Issue {
                host: None,
                owner: "owner".to_owned(),
                repo: "repo".to_owned(),
                number: 99,
            })
        );
    }

    #[test]
    fn parse_url_with_query_and_fragment() {
        let url = "https://github.com/owner/repo/actions/runs/123456?check_suite_focus=true#step:3";
        assert_eq!(
            parse_github_url(url),
            Some(ParsedGitHubUrl::ActionsRun {
                host: None,
                owner: "owner".to_owned(),
                repo: "repo".to_owned(),
                run_id: 123_456,
            })
        );
    }

    #[test]
    fn owner_repo_from_url_basic() {
        assert_eq!(
            owner_repo_from_url("https://github.com/owner/repo/pull/42"),
            Some(("owner".to_owned(), "repo".to_owned()))
        );
        assert_eq!(owner_repo_from_url("https://github.com/owner"), None);
    }
}
