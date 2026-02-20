use std::sync::Arc;

use anyhow::{Context, Result};
use octocrab::Octocrab;
use octocrab::models::NotificationId;

use crate::github::types::{
    Notification, NotificationReason, NotificationStatus, RepoRef, SubjectType,
};

// ---------------------------------------------------------------------------
// Notification filter parameters (T052)
// ---------------------------------------------------------------------------

/// Parsed filter parameters for the notifications REST API.
#[derive(Debug, Clone, Default)]
pub struct NotificationQueryParams {
    /// If `true`, include read notifications.
    pub all: bool,
    /// Max results per page.
    pub per_page: u8,
    /// Filter by repo (client-side).
    pub repo: Option<String>,
    /// Keep only notifications whose reason matches (client-side).
    pub reason: Option<NotificationReason>,
    /// Exclude notifications whose reason is in this list (client-side).
    pub excluded_reasons: Vec<NotificationReason>,
    /// Filter by read/unread status (client-side).
    pub status: Option<NotificationStatus>,
}

/// Parse a filter string like `"repo:owner/name reason:mention is:unread"`
/// into a `NotificationQueryParams`.
pub fn parse_filters(filter_str: &str, limit: u32) -> NotificationQueryParams {
    #[allow(clippy::cast_possible_truncation)]
    let per_page = limit.min(100) as u8;
    let mut filter = NotificationQueryParams {
        per_page,
        all: true,
        ..Default::default()
    };

    for token in filter_str.split_whitespace() {
        if let Some(repo) = token.strip_prefix("repo:") {
            filter.repo = Some(repo.to_owned());
        } else if let Some(reason_str) = token.strip_prefix("-reason:") {
            filter.excluded_reasons.push(parse_reason(reason_str));
        } else if let Some(reason_str) = token.strip_prefix("reason:") {
            filter.reason = Some(parse_reason(reason_str));
        } else if let Some(status) = token.strip_prefix("is:") {
            match status {
                "unread" => {
                    filter.status = Some(NotificationStatus::Unread);
                    filter.all = false;
                }
                "read" => {
                    filter.status = Some(NotificationStatus::Read);
                    filter.all = true;
                }
                "all" | "done" => {
                    filter.status = None;
                    filter.all = true;
                }
                _ => {}
            }
        }
    }

    filter
}

fn parse_reason(s: &str) -> NotificationReason {
    match s {
        "subscribed" => NotificationReason::Subscribed,
        "review_requested" => NotificationReason::ReviewRequested,
        "mention" => NotificationReason::Mention,
        "author" => NotificationReason::Author,
        "comment" => NotificationReason::Comment,
        "assign" => NotificationReason::Assign,
        "state_change" => NotificationReason::StateChange,
        "ci_activity" => NotificationReason::CiActivity,
        "team_mention" => NotificationReason::TeamMention,
        "security_alert" => NotificationReason::SecurityAlert,
        _ => NotificationReason::Unknown,
    }
}

fn api_url_to_html_url(api_url: &str, subject_type: &str, owner: &str, repo_name: &str) -> String {
    if api_url.is_empty() {
        return String::new();
    }
    match subject_type {
        "Release" => format!("https://github.com/{owner}/{repo_name}/releases"),
        "PullRequest" => api_url
            .replace("https://api.github.com/repos/", "https://github.com/")
            .replace("/pulls/", "/pull/"),
        _ => api_url.replace("https://api.github.com/repos/", "https://github.com/"),
    }
}

fn parse_subject_type(s: &str) -> SubjectType {
    match s {
        "PullRequest" => SubjectType::PullRequest,
        "Issue" => SubjectType::Issue,
        "Release" => SubjectType::Release,
        "Discussion" => SubjectType::Discussion,
        _ => SubjectType::Other,
    }
}

/// Convert octocrab Notification into our domain type.
fn into_domain(n: octocrab::models::activity::Notification) -> Notification {
    let subject_type = Some(parse_subject_type(&n.subject.r#type));
    let repo = RepoRef {
        owner: n
            .repository
            .owner
            .as_ref()
            .map_or_else(String::new, |o| o.login.clone()),
        name: n.repository.name.clone(),
    };
    let reason = parse_reason(&n.reason);
    let url = n.subject.url.map_or_else(String::new, |u| {
        api_url_to_html_url(u.as_str(), &n.subject.r#type, &repo.owner, &repo.name)
    });

    Notification {
        id: n.id.0.to_string(),
        subject_type,
        subject_title: n.subject.title,
        reason,
        unread: n.unread,
        repository: Some(repo),
        url,
        updated_at: n.updated_at,
    }
}

// ---------------------------------------------------------------------------
// Public API (T050 + T051)
// ---------------------------------------------------------------------------

/// Fetch notifications from the REST API, applying the given filter.
pub async fn fetch_notifications(
    octocrab: &Arc<Octocrab>,
    filter: &NotificationQueryParams,
) -> Result<Vec<Notification>> {
    let page = octocrab
        .activity()
        .notifications()
        .list()
        .all(filter.all)
        .per_page(filter.per_page)
        .send()
        .await
        .context("fetching notifications")?;

    let mut notifications: Vec<Notification> = page.items.into_iter().map(into_domain).collect();

    // Client-side filters.
    if let Some(ref repo_filter) = filter.repo {
        notifications.retain(|n| {
            n.repository
                .as_ref()
                .is_some_and(|r| r.full_name() == *repo_filter)
        });
    }
    if let Some(reason_filter) = filter.reason {
        notifications.retain(|n| n.reason == reason_filter);
    }
    if !filter.excluded_reasons.is_empty() {
        notifications.retain(|n| !filter.excluded_reasons.contains(&n.reason));
    }
    if let Some(status) = filter.status {
        match status {
            NotificationStatus::Unread => notifications.retain(|n| n.unread),
            NotificationStatus::Read => notifications.retain(|n| !n.unread),
        }
    }

    Ok(notifications)
}

/// Mark a single notification as read.
pub async fn mark_as_read(octocrab: &Arc<Octocrab>, thread_id: &str) -> Result<()> {
    let id: u64 = thread_id.parse().context("invalid notification id")?;
    octocrab
        .activity()
        .notifications()
        .mark_as_read(NotificationId(id))
        .await
        .context("marking notification as read")?;
    Ok(())
}

/// Mark all notifications as read.
pub async fn mark_all_as_read(octocrab: &Arc<Octocrab>) -> Result<()> {
    octocrab
        .activity()
        .notifications()
        .mark_all_as_read(chrono::Utc::now())
        .await
        .context("marking all notifications as read")?;
    Ok(())
}

/// Mark a notification as done (DELETE /notifications/threads/{id}).
///
/// Not natively supported by octocrab, so we use the raw `_delete` method.
pub async fn mark_as_done(octocrab: &Arc<Octocrab>, thread_id: &str) -> Result<()> {
    let route = format!("/notifications/threads/{thread_id}");
    let uri = http::Uri::builder()
        .path_and_query(route)
        .build()
        .context("building URI for mark-as-done")?;
    let response = octocrab
        ._delete(uri, None::<&()>)
        .await
        .context("marking notification as done")?;
    octocrab::map_github_error(response)
        .await
        .map(drop)
        .context("mark-as-done API error")?;
    Ok(())
}

/// Unsubscribe from a notification thread.
pub async fn unsubscribe(octocrab: &Arc<Octocrab>, thread_id: &str) -> Result<()> {
    let id: u64 = thread_id.parse().context("invalid notification id")?;
    octocrab
        .activity()
        .notifications()
        .delete_thread_subscription(id.into())
        .await
        .context("unsubscribing from thread")?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_filter() {
        let f = parse_filters("", 30);
        assert!(f.all);
        assert_eq!(f.per_page, 30);
        assert!(f.repo.is_none());
        assert!(f.reason.is_none());
        assert!(f.status.is_none());
    }

    #[test]
    fn parse_unread_filter() {
        let f = parse_filters("is:unread", 50);
        assert!(!f.all);
        assert_eq!(f.status, Some(NotificationStatus::Unread));
    }

    #[test]
    fn parse_read_filter() {
        let f = parse_filters("is:read", 25);
        assert!(f.all);
        assert_eq!(f.status, Some(NotificationStatus::Read));
    }

    #[test]
    fn parse_repo_filter() {
        let f = parse_filters("repo:owner/name", 30);
        assert_eq!(f.repo.as_deref(), Some("owner/name"));
    }

    #[test]
    fn parse_reason_filter() {
        let f = parse_filters("reason:mention", 30);
        assert_eq!(f.reason, Some(NotificationReason::Mention));
    }

    #[test]
    fn parse_combined_filters() {
        let f = parse_filters("repo:torvalds/linux reason:review_requested is:unread", 100);
        assert_eq!(f.repo.as_deref(), Some("torvalds/linux"));
        assert_eq!(f.reason, Some(NotificationReason::ReviewRequested));
        assert_eq!(f.status, Some(NotificationStatus::Unread));
        assert!(f.excluded_reasons.is_empty());
        assert!(!f.all);
        assert_eq!(f.per_page, 100);
    }

    #[test]
    fn parse_excluded_reason_single() {
        let f = parse_filters("is:unread -reason:subscribed", 50);
        assert_eq!(f.status, Some(NotificationStatus::Unread));
        assert!(f.reason.is_none());
        assert_eq!(f.excluded_reasons, vec![NotificationReason::Subscribed]);
    }

    #[test]
    fn parse_excluded_reason_multiple() {
        let f = parse_filters("is:unread -reason:subscribed -reason:review_requested", 50);
        assert_eq!(f.status, Some(NotificationStatus::Unread));
        assert!(f.reason.is_none());
        assert_eq!(
            f.excluded_reasons,
            vec![
                NotificationReason::Subscribed,
                NotificationReason::ReviewRequested,
            ]
        );
    }

    #[test]
    fn parse_all_filter() {
        let f = parse_filters("is:all", 30);
        assert!(f.all);
        assert!(f.status.is_none());
    }

    #[test]
    fn per_page_capped_at_100() {
        let f = parse_filters("", 200);
        assert_eq!(f.per_page, 100);
    }

    #[test]
    fn parse_reason_values() {
        assert_eq!(parse_reason("subscribed"), NotificationReason::Subscribed);
        assert_eq!(
            parse_reason("review_requested"),
            NotificationReason::ReviewRequested
        );
        assert_eq!(parse_reason("mention"), NotificationReason::Mention);
        assert_eq!(parse_reason("author"), NotificationReason::Author);
        assert_eq!(parse_reason("comment"), NotificationReason::Comment);
        assert_eq!(parse_reason("assign"), NotificationReason::Assign);
        assert_eq!(
            parse_reason("state_change"),
            NotificationReason::StateChange
        );
        assert_eq!(parse_reason("ci_activity"), NotificationReason::CiActivity);
        assert_eq!(
            parse_reason("team_mention"),
            NotificationReason::TeamMention
        );
        assert_eq!(
            parse_reason("security_alert"),
            NotificationReason::SecurityAlert
        );
        assert_eq!(parse_reason("unknown_thing"), NotificationReason::Unknown);
    }

    #[test]
    fn api_url_issue_conversion() {
        assert_eq!(
            api_url_to_html_url(
                "https://api.github.com/repos/owner/repo/issues/123",
                "Issue",
                "owner",
                "repo",
            ),
            "https://github.com/owner/repo/issues/123"
        );
    }

    #[test]
    fn api_url_pull_request_conversion() {
        assert_eq!(
            api_url_to_html_url(
                "https://api.github.com/repos/owner/repo/pulls/42",
                "PullRequest",
                "owner",
                "repo",
            ),
            "https://github.com/owner/repo/pull/42"
        );
    }

    #[test]
    fn api_url_discussion_conversion() {
        assert_eq!(
            api_url_to_html_url(
                "https://api.github.com/repos/owner/repo/discussions/7",
                "Discussion",
                "owner",
                "repo",
            ),
            "https://github.com/owner/repo/discussions/7"
        );
    }

    #[test]
    fn api_url_release_falls_back_to_listing() {
        assert_eq!(
            api_url_to_html_url(
                "https://api.github.com/repos/owner/repo/releases/99999999",
                "Release",
                "owner",
                "repo",
            ),
            "https://github.com/owner/repo/releases"
        );
    }

    #[test]
    fn api_url_empty_passthrough() {
        assert_eq!(api_url_to_html_url("", "Issue", "owner", "repo"), "");
    }

    #[test]
    fn parse_subject_type_values() {
        assert_eq!(parse_subject_type("PullRequest"), SubjectType::PullRequest);
        assert_eq!(parse_subject_type("Issue"), SubjectType::Issue);
        assert_eq!(parse_subject_type("Release"), SubjectType::Release);
        assert_eq!(parse_subject_type("Discussion"), SubjectType::Discussion);
        assert_eq!(parse_subject_type("SomethingElse"), SubjectType::Other);
    }
}
