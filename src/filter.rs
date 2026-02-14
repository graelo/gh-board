use crate::components::table::Row;
use crate::github::types::Notification;

// ---------------------------------------------------------------------------
// Generic row filter (T088)
// ---------------------------------------------------------------------------

/// Filter rows by case-insensitive substring match on any cell's text.
/// Returns indices of matching rows.
pub(crate) fn filter_rows(rows: &[Row], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..rows.len()).collect();
    }
    let lower = query.to_lowercase();
    rows.iter()
        .enumerate()
        .filter(|(_, row)| {
            row.values()
                .any(|cell| cell.text.to_lowercase().contains(&lower))
        })
        .map(|(i, _)| i)
        .collect()
}

// ---------------------------------------------------------------------------
// Structured notification filter (T089)
// ---------------------------------------------------------------------------

/// Parsed structured query for notifications.
#[derive(Debug, Default)]
pub(crate) struct NotificationQuery {
    /// Free-text substring to match against title/reason/repo.
    pub(crate) text: String,
    /// Filter by repo full name (e.g., "owner/repo").
    pub(crate) repo: Option<String>,
    /// Filter by reason (e.g., "mention", "review requested").
    pub(crate) reason: Option<String>,
    /// Filter by read status.
    pub(crate) read_filter: Option<ReadFilter>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReadFilter {
    Unread,
    Read,
    All,
}

/// Parse a query string, extracting structured prefixes.
///
/// Supported prefixes:
/// - `repo:owner/name`
/// - `reason:mention`
/// - `is:unread` / `is:read` / `is:all`
///
/// Remaining text is used for free-text matching.
pub(crate) fn parse_notification_query(query: &str) -> NotificationQuery {
    let mut result = NotificationQuery::default();
    let mut text_parts = Vec::new();

    for token in query.split_whitespace() {
        if let Some(val) = token.strip_prefix("repo:") {
            result.repo = Some(val.to_lowercase());
        } else if let Some(val) = token.strip_prefix("reason:") {
            result.reason = Some(val.to_lowercase());
        } else if let Some(val) = token.strip_prefix("is:") {
            result.read_filter = match val.to_lowercase().as_str() {
                "unread" => Some(ReadFilter::Unread),
                "read" => Some(ReadFilter::Read),
                "all" | "done" => Some(ReadFilter::All),
                _ => None,
            };
        } else {
            text_parts.push(token);
        }
    }

    result.text = text_parts.join(" ");
    result
}

/// Filter notifications using structured query.
/// Returns indices of matching notifications.
pub(crate) fn filter_notifications(
    notifications: &[Notification],
    rows: &[Row],
    query: &str,
) -> Vec<usize> {
    if query.is_empty() {
        return (0..notifications.len()).collect();
    }

    let parsed = parse_notification_query(query);

    notifications
        .iter()
        .enumerate()
        .filter(|(i, notif)| {
            // Check structured filters first.
            if let Some(repo_filter) = &parsed.repo {
                let repo_name = notif
                    .repository
                    .as_ref()
                    .map(|r| format!("{}/{}", r.owner, r.name).to_lowercase())
                    .unwrap_or_default();
                if !repo_name.contains(repo_filter.as_str()) {
                    return false;
                }
            }

            if let Some(reason_filter) = &parsed.reason {
                let reason = notif.reason.as_str().to_lowercase();
                if !reason.contains(reason_filter.as_str()) {
                    return false;
                }
            }

            if let Some(read_filter) = &parsed.read_filter {
                match read_filter {
                    ReadFilter::Unread => {
                        if !notif.unread {
                            return false;
                        }
                    }
                    ReadFilter::Read => {
                        if notif.unread {
                            return false;
                        }
                    }
                    ReadFilter::All => {} // no filtering
                }
            }

            // Free text matching on row cells.
            if !parsed.text.is_empty() {
                let lower = parsed.text.to_lowercase();
                let row_matches = rows.get(*i).is_some_and(|row| {
                    row.values()
                        .any(|cell| cell.text.to_lowercase().contains(&lower))
                });
                if !row_matches {
                    return false;
                }
            }

            true
        })
        .map(|(i, _)| i)
        .collect()
}

// ---------------------------------------------------------------------------
// Tests (T090)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::table::Cell;
    use crate::github::types::{NotificationReason, RepoRef};
    use chrono::Utc;

    fn make_row(cells: &[(&str, &str)]) -> Row {
        cells
            .iter()
            .map(|(k, v)| ((*k).to_owned(), Cell::plain(*v)))
            .collect()
    }

    fn make_notification(
        title: &str,
        repo: &str,
        reason: NotificationReason,
        unread: bool,
    ) -> Notification {
        let parts: Vec<&str> = repo.split('/').collect();
        Notification {
            id: "1".to_owned(),
            subject_type: None,
            subject_title: title.to_owned(),
            reason,
            unread,
            repository: if repo.is_empty() {
                None
            } else {
                Some(RepoRef {
                    owner: parts[0].to_owned(),
                    name: parts.get(1).unwrap_or(&"").to_string(),
                })
            },
            url: String::new(),
            updated_at: Utc::now(),
        }
    }

    // --- filter_rows tests ---

    #[test]
    fn filter_rows_empty_query_returns_all() {
        let rows = vec![
            make_row(&[("title", "Fix bug"), ("author", "alice")]),
            make_row(&[("title", "Add feature"), ("author", "bob")]),
        ];
        let result = filter_rows(&rows, "");
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn filter_rows_matches_title() {
        let rows = vec![
            make_row(&[("title", "Fix bug"), ("author", "alice")]),
            make_row(&[("title", "Add feature"), ("author", "bob")]),
        ];
        let result = filter_rows(&rows, "fix");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn filter_rows_matches_author() {
        let rows = vec![
            make_row(&[("title", "Fix bug"), ("author", "alice")]),
            make_row(&[("title", "Add feature"), ("author", "bob")]),
        ];
        let result = filter_rows(&rows, "bob");
        assert_eq!(result, vec![1]);
    }

    #[test]
    fn filter_rows_case_insensitive() {
        let rows = vec![make_row(&[("title", "Fix BUG"), ("author", "Alice")])];
        let result = filter_rows(&rows, "bug");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn filter_rows_no_match() {
        let rows = vec![make_row(&[("title", "Fix bug"), ("author", "alice")])];
        let result = filter_rows(&rows, "xyz");
        assert!(result.is_empty());
    }

    #[test]
    fn filter_rows_matches_any_column() {
        let rows = vec![make_row(&[("title", "Fix bug"), ("repo", "owner/project")])];
        let result = filter_rows(&rows, "project");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn filter_rows_empty_rows() {
        let rows: Vec<Row> = vec![];
        let result = filter_rows(&rows, "test");
        assert!(result.is_empty());
    }

    // --- parse_notification_query tests ---

    #[test]
    fn parse_empty_query() {
        let q = parse_notification_query("");
        assert!(q.text.is_empty());
        assert!(q.repo.is_none());
        assert!(q.reason.is_none());
        assert!(q.read_filter.is_none());
    }

    #[test]
    fn parse_plain_text() {
        let q = parse_notification_query("fix bug");
        assert_eq!(q.text, "fix bug");
        assert!(q.repo.is_none());
    }

    #[test]
    fn parse_repo_prefix() {
        let q = parse_notification_query("repo:owner/repo");
        assert_eq!(q.repo.as_deref(), Some("owner/repo"));
        assert!(q.text.is_empty());
    }

    #[test]
    fn parse_reason_prefix() {
        let q = parse_notification_query("reason:mention");
        assert_eq!(q.reason.as_deref(), Some("mention"));
    }

    #[test]
    fn parse_is_unread() {
        let q = parse_notification_query("is:unread");
        assert_eq!(q.read_filter, Some(ReadFilter::Unread));
    }

    #[test]
    fn parse_is_read() {
        let q = parse_notification_query("is:read");
        assert_eq!(q.read_filter, Some(ReadFilter::Read));
    }

    #[test]
    fn parse_is_all() {
        let q = parse_notification_query("is:all");
        assert_eq!(q.read_filter, Some(ReadFilter::All));
    }

    #[test]
    fn parse_combined_query() {
        let q = parse_notification_query("repo:owner/repo is:unread fix");
        assert_eq!(q.repo.as_deref(), Some("owner/repo"));
        assert_eq!(q.read_filter, Some(ReadFilter::Unread));
        assert_eq!(q.text, "fix");
    }

    // --- filter_notifications tests ---

    #[test]
    fn filter_notifications_empty_query() {
        let notifs = vec![make_notification(
            "Bug fix",
            "owner/repo",
            NotificationReason::Mention,
            true,
        )];
        let rows = vec![make_row(&[("title", "Bug fix"), ("repo", "owner/repo")])];
        let result = filter_notifications(&notifs, &rows, "");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn filter_notifications_by_repo() {
        let notifs = vec![
            make_notification("Bug fix", "owner/repo-a", NotificationReason::Mention, true),
            make_notification("Feature", "owner/repo-b", NotificationReason::Mention, true),
        ];
        let rows = vec![
            make_row(&[("title", "Bug fix"), ("repo", "owner/repo-a")]),
            make_row(&[("title", "Feature"), ("repo", "owner/repo-b")]),
        ];
        let result = filter_notifications(&notifs, &rows, "repo:repo-a");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn filter_notifications_by_unread() {
        let notifs = vec![
            make_notification("Bug fix", "owner/repo", NotificationReason::Mention, true),
            make_notification("Feature", "owner/repo", NotificationReason::Mention, false),
        ];
        let rows = vec![
            make_row(&[("title", "Bug fix")]),
            make_row(&[("title", "Feature")]),
        ];
        let result = filter_notifications(&notifs, &rows, "is:unread");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn filter_notifications_by_read() {
        let notifs = vec![
            make_notification("Bug fix", "owner/repo", NotificationReason::Mention, true),
            make_notification("Feature", "owner/repo", NotificationReason::Mention, false),
        ];
        let rows = vec![
            make_row(&[("title", "Bug fix")]),
            make_row(&[("title", "Feature")]),
        ];
        let result = filter_notifications(&notifs, &rows, "is:read");
        assert_eq!(result, vec![1]);
    }

    #[test]
    fn filter_notifications_combined() {
        let notifs = vec![
            make_notification("Bug fix", "owner/repo-a", NotificationReason::Mention, true),
            make_notification(
                "Bug report",
                "owner/repo-b",
                NotificationReason::Mention,
                true,
            ),
            make_notification(
                "Feature",
                "owner/repo-a",
                NotificationReason::Mention,
                false,
            ),
        ];
        let rows = vec![
            make_row(&[("title", "Bug fix"), ("repo", "owner/repo-a")]),
            make_row(&[("title", "Bug report"), ("repo", "owner/repo-b")]),
            make_row(&[("title", "Feature"), ("repo", "owner/repo-a")]),
        ];
        let result = filter_notifications(&notifs, &rows, "repo:repo-a is:unread");
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn filter_notifications_text_and_structured() {
        let notifs = vec![
            make_notification("Bug fix", "owner/repo", NotificationReason::Mention, true),
            make_notification(
                "Feature add",
                "owner/repo",
                NotificationReason::Mention,
                true,
            ),
        ];
        let rows = vec![
            make_row(&[("title", "Bug fix")]),
            make_row(&[("title", "Feature add")]),
        ];
        let result = filter_notifications(&notifs, &rows, "is:unread feature");
        assert_eq!(result, vec![1]);
    }
}
