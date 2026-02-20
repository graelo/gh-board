use std::sync::mpsc::Sender;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::actions::{issue_actions, pr_actions};
use crate::config::types::AppConfig;
use crate::github::{
    client::GitHubClient, graphql, notifications as notif,
    rate_limit::{format_rate_limit_message, is_rate_limited},
};

use super::interface::{Engine, EngineHandle, Event, Request};
use super::refresh::{DueEntry, FilterConfig, RefreshScheduler, ViewKind};

/// The real GitHub backend engine.
pub struct GitHubEngine {
    config: AppConfig,
}

impl GitHubEngine {
    pub fn new(config: AppConfig) -> Self {
        Self { config }
    }
}

impl Engine for GitHubEngine {
    fn start(self) -> EngineHandle {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Request>();
        let handle = EngineHandle::new(tx);
        let _ = std::thread::Builder::new()
            .name("gh-engine".to_owned())
            .spawn(move || {
                let rt = tokio::runtime::Runtime::new().expect("tokio runtime init");
                rt.block_on(self.run_loop(rx));
            });
        handle
    }
}

impl GitHubEngine {
    async fn run_loop(self, mut rx: UnboundedReceiver<Request>) {
        let mut client = GitHubClient::new(self.config.github.refetch_interval_minutes);
        let mut scheduler = RefreshScheduler::new();

        let interval_mins = u64::from(self.config.github.refetch_interval_minutes);
        let tick_dur = Duration::from_secs((interval_mins * 60).max(60));
        let mut refresh_tick = tokio::time::interval(tick_dur);
        // Consume the first immediate tick so refresh fires after one full interval.
        refresh_tick.tick().await;

        loop {
            tokio::select! {
                biased;
                maybe_req = rx.recv() => {
                    match maybe_req {
                        None | Some(Request::Shutdown) => {
                            tracing::debug!("engine: shutting down");
                            break;
                        }
                        Some(req) => {
                            handle_request(req, &mut client, &mut scheduler).await;
                        }
                    }
                }
                _ = refresh_tick.tick() => {
                    tick_refresh(&mut client, &mut scheduler).await;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
async fn handle_request(req: Request, client: &mut GitHubClient, scheduler: &mut RefreshScheduler) {
    tracing::debug!("engine: received request");
    match req {
        // --- Fetch PRs ---
        Request::FetchPrs {
            filter_idx,
            filter,
            reply_tx,
        } => {
            let host = filter.host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchPrs") else {
                return;
            };
            let cache = client.cache();
            let limit = filter.limit.unwrap_or(100);
            match graphql::search_pull_requests_all(&octocrab, &filter.filters, limit, Some(&cache))
                .await
            {
                Ok((prs, rate_limit)) => {
                    scheduler.mark_fetched(filter_idx, ViewKind::Prs);
                    tracing::debug!(
                        "engine: sending PrsFetched[{filter_idx}] count={}",
                        prs.len()
                    );
                    let _ = reply_tx.send(Event::PrsFetched {
                        filter_idx,
                        prs,
                        rate_limit,
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchPrs[{filter_idx}] error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchPrs[{filter_idx}]"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Fetch Issues ---
        Request::FetchIssues {
            filter_idx,
            filter,
            reply_tx,
        } => {
            let host = filter.host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchIssues") else {
                return;
            };
            let cache = client.cache();
            let limit = filter.limit.unwrap_or(100);
            match graphql::search_issues_all(&octocrab, &filter.filters, limit, Some(&cache)).await
            {
                Ok((issues, rate_limit)) => {
                    scheduler.mark_fetched(filter_idx, ViewKind::Issues);
                    tracing::debug!(
                        "engine: sending IssuesFetched[{filter_idx}] count={}",
                        issues.len()
                    );
                    let _ = reply_tx.send(Event::IssuesFetched {
                        filter_idx,
                        issues,
                        rate_limit,
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchIssues[{filter_idx}] error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchIssues[{filter_idx}]"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Fetch Notifications ---
        Request::FetchNotifications {
            filter_idx,
            filter,
            reply_tx,
        } => {
            let host = filter.host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchNotifications") else {
                return;
            };
            let limit = filter.limit.unwrap_or(50);
            let params = notif::parse_filters(&filter.filters, limit);
            match notif::fetch_notifications(&octocrab, &params).await {
                Ok(notifications) => {
                    scheduler.mark_fetched(filter_idx, ViewKind::Notifications);
                    tracing::debug!(
                        "engine: sending NotificationsFetched[{filter_idx}] count={}",
                        notifications.len()
                    );
                    let _ = reply_tx.send(Event::NotificationsFetched {
                        filter_idx,
                        notifications,
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchNotifications[{filter_idx}] error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchNotifications[{filter_idx}]"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Fetch PR Detail ---
        Request::FetchPrDetail {
            owner,
            repo,
            number,
            base_ref,
            head_repo_owner,
            head_ref,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchPrDetail")
            else {
                return;
            };
            let cache = client.cache();
            match graphql::fetch_pr_detail(&octocrab, &owner, &repo, number, Some(&cache)).await {
                Ok((mut detail, rate_limit)) => {
                    if detail.behind_by.is_none()
                        && let Some(ref head_owner) = head_repo_owner
                    {
                        match graphql::fetch_compare(
                            &octocrab,
                            &owner,
                            &repo,
                            &base_ref,
                            head_owner,
                            &head_ref,
                        )
                        .await
                        {
                            Ok(n) => detail.behind_by = n,
                            Err(e) => {
                                tracing::debug!(
                                    "engine: compare API failed for #{number}: {e:#}"
                                );
                            }
                        }
                    }
                    tracing::debug!("engine: sending PrDetailFetched #{number}");
                    let _ = reply_tx.send(Event::PrDetailFetched {
                        number,
                        detail,
                        rate_limit,
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchPrDetail #{number} error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchPrDetail #{number}"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Fetch Issue Detail ---
        Request::FetchIssueDetail {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchIssueDetail")
            else {
                return;
            };
            let cache = client.cache();
            match graphql::fetch_issue_detail(&octocrab, &owner, &repo, number, Some(&cache)).await
            {
                Ok((detail, _rate_limit)) => {
                    tracing::debug!("engine: sending IssueDetailFetched #{number}");
                    let _ = reply_tx.send(Event::IssueDetailFetched { number, detail });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchIssueDetail #{number} error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchIssueDetail #{number}"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Prefetch PR Details ---
        Request::PrefetchPrDetails { prs, reply_tx } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "PrefetchPrDetails")
            else {
                return;
            };
            let cache = client.cache();
            for pr in prs {
                let number = pr.number;
                match graphql::fetch_pr_detail(&octocrab, &pr.owner, &pr.repo, number, Some(&cache))
                    .await
                {
                    Ok((mut detail, rate_limit)) => {
                        if detail.behind_by.is_none()
                            && let Some(ref head_owner) = pr.head_repo_owner
                        {
                            match graphql::fetch_compare(
                                &octocrab,
                                &pr.owner,
                                &pr.repo,
                                &pr.base_ref,
                                head_owner,
                                &pr.head_ref,
                            )
                            .await
                            {
                                Ok(n) => detail.behind_by = n,
                                Err(e) => {
                                    tracing::debug!(
                                        "engine: compare API failed for #{number}: {e:#}"
                                    );
                                }
                            }
                        }
                        tracing::debug!("engine: sending PrDetailFetched #{number} (prefetch)");
                        let _ = reply_tx.send(Event::PrDetailFetched {
                            number,
                            detail,
                            rate_limit,
                        });
                    }
                    Err(e) => {
                        tracing::debug!("engine: PrefetchPrDetails #{number} error: {e}");
                        // Continue prefetching remaining PRs even if one fails.
                    }
                }
            }
        }

        // --- Register refresh ---
        Request::RegisterPrsRefresh {
            filter_configs,
            notify_tx,
        } => {
            let interval = Duration::from_secs(600); // default; engine owns the interval
            scheduler.register_prs(filter_configs, interval, &notify_tx);
        }
        Request::RegisterIssuesRefresh {
            filter_configs,
            notify_tx,
        } => {
            let interval = Duration::from_secs(600);
            scheduler.register_issues(filter_configs, interval, &notify_tx);
        }
        Request::RegisterNotificationsRefresh {
            filter_configs,
            notify_tx,
        } => {
            let interval = Duration::from_secs(600);
            scheduler.register_notifications(filter_configs, interval, &notify_tx);
        }

        // -----------------------------------------------------------------------
        // Mutation operations — PR
        // -----------------------------------------------------------------------
        Request::ApprovePr {
            owner,
            repo,
            number,
            body,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ApprovePr") else {
                return;
            };
            match pr_actions::approve(&octocrab, &owner, &repo, number, body.as_deref()).await {
                Ok(()) => {
                    tracing::debug!("engine: sending MutationOk ApprovePr #{number}");
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Approved PR #{number}"),
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: ApprovePr #{number} error: {e}");
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Approve PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::MergePr {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "MergePr") else {
                return;
            };
            match pr_actions::merge(&octocrab, &owner, &repo, number).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Merged PR #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Merge PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::ClosePr {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ClosePr") else {
                return;
            };
            match pr_actions::close(&octocrab, &owner, &repo, number).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Closed PR #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Close PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::ReopenPr {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ReopenPr") else {
                return;
            };
            match pr_actions::reopen(&octocrab, &owner, &repo, number).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Reopened PR #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Reopen PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::AddPrComment {
            owner,
            repo,
            number,
            body,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "AddPrComment")
            else {
                return;
            };
            match pr_actions::add_comment(&octocrab, &owner, &repo, number, &body).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Added comment to PR #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Add comment to PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::UpdateBranch {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "UpdateBranch")
            else {
                return;
            };
            match pr_actions::update_branch(&octocrab, &owner, &repo, number).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Updated branch for PR #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Update branch for PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::ReadyForReview {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ReadyForReview")
            else {
                return;
            };
            match pr_actions::ready_for_review(&octocrab, &owner, &repo, number).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Marked PR #{number} as ready for review"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Mark PR #{number} as ready for review"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::AssignPr {
            owner,
            repo,
            number,
            logins,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "AssignPr") else {
                return;
            };
            // Resolve "@me" to the authenticated user's login.
            let resolved_logins: Vec<String> = if logins.iter().any(|l| l == "@me") {
                match octocrab.current().user().await {
                    Ok(user) => logins
                        .iter()
                        .map(|l| {
                            if l == "@me" {
                                user.login.clone()
                            } else {
                                l.clone()
                            }
                        })
                        .collect(),
                    Err(e) => {
                        let _ = reply_tx.send(Event::MutationError {
                            description: format!("Assign PR #{number}"),
                            message: format!("Failed to resolve current user: {e}"),
                        });
                        return;
                    }
                }
            } else {
                logins
            };
            match pr_actions::assign(&octocrab, &owner, &repo, number, &resolved_logins).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Assigned PR #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Assign PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::UnassignPr {
            owner,
            repo,
            number,
            login,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "UnassignPr") else {
                return;
            };
            // Resolve "@me" to the authenticated user's login.
            let resolved_login = if login == "@me" {
                match octocrab.current().user().await {
                    Ok(user) => user.login,
                    Err(e) => {
                        let _ = reply_tx.send(Event::MutationError {
                            description: format!("Unassign PR #{number}"),
                            message: format!("Failed to resolve current user: {e}"),
                        });
                        return;
                    }
                }
            } else {
                login
            };
            match pr_actions::unassign(&octocrab, &owner, &repo, number, &resolved_login).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Unassigned PR #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Unassign PR #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        // -----------------------------------------------------------------------
        // Mutation operations — Issue
        // -----------------------------------------------------------------------
        Request::CloseIssue {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "CloseIssue") else {
                return;
            };
            match issue_actions::close(&octocrab, &owner, &repo, number).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Closed issue #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Close issue #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::ReopenIssue {
            owner,
            repo,
            number,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ReopenIssue")
            else {
                return;
            };
            match issue_actions::reopen(&octocrab, &owner, &repo, number).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Reopened issue #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Reopen issue #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::AddIssueComment {
            owner,
            repo,
            number,
            body,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "AddIssueComment")
            else {
                return;
            };
            match issue_actions::add_comment(&octocrab, &owner, &repo, number, &body).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Added comment to issue #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Add comment to issue #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::AddIssueLabels {
            owner,
            repo,
            number,
            labels,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "AddIssueLabels")
            else {
                return;
            };
            match issue_actions::add_labels(&octocrab, &owner, &repo, number, &labels).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Added labels to issue #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Add labels to issue #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::AssignIssue {
            owner,
            repo,
            number,
            logins,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "AssignIssue")
            else {
                return;
            };
            match issue_actions::assign(&octocrab, &owner, &repo, number, &logins).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Assigned issue #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Assign issue #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::UnassignIssue {
            owner,
            repo,
            number,
            login,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "UnassignIssue")
            else {
                return;
            };
            match issue_actions::unassign(&octocrab, &owner, &repo, number, &login).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Unassigned issue #{number}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Unassign issue #{number}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        // -----------------------------------------------------------------------
        // Mutation operations — Notification
        // -----------------------------------------------------------------------
        Request::MarkNotificationDone { id, reply_tx } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "MarkNotificationDone")
            else {
                return;
            };
            match notif::mark_as_done(&octocrab, &id).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Marked notification {id} as done"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Mark notification {id} as done"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::MarkNotificationRead { id, reply_tx } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "MarkNotificationRead")
            else {
                return;
            };
            match notif::mark_as_read(&octocrab, &id).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Marked notification {id} as read"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Mark notification {id} as read"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::MarkAllNotificationsRead { reply_tx } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "MarkAllNotificationsRead")
            else {
                return;
            };
            match notif::mark_all_as_read(&octocrab).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: "Marked all notifications as read".to_owned(),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: "Mark all notifications as read".to_owned(),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::UnsubscribeNotification { id, reply_tx } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "UnsubscribeNotification")
            else {
                return;
            };
            match notif::unsubscribe(&octocrab, &id).await {
                Ok(()) => {
                    let _ = reply_tx.send(Event::MutationOk {
                        description: format!("Unsubscribed from notification {id}"),
                    });
                }
                Err(e) => {
                    let _ = reply_tx.send(Event::MutationError {
                        description: format!("Unsubscribe from notification {id}"),
                        message: e.to_string(),
                    });
                }
            }
        }

        Request::Shutdown => unreachable!("handled at run_loop level"),
    }
}

// ---------------------------------------------------------------------------
// Background refresh
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
async fn tick_refresh(client: &mut GitHubClient, scheduler: &mut RefreshScheduler) {
    let due = scheduler.due_entries();
    for DueEntry {
        filter_idx,
        filter,
        notify_tx,
    } in due
    {
        match filter {
            FilterConfig::Pr(pr_filter) => {
                let host = pr_filter.host.as_deref().unwrap_or("github.com");
                let Ok(octocrab) = client.octocrab_for(host) else {
                    continue;
                };
                let cache = client.cache();
                let limit = pr_filter.limit.unwrap_or(100);
                match graphql::search_pull_requests_all(
                    &octocrab,
                    &pr_filter.filters,
                    limit,
                    Some(&cache),
                )
                .await
                {
                    Ok((prs, rate_limit)) => {
                        scheduler.mark_fetched(filter_idx, ViewKind::Prs);
                        tracing::debug!(
                            "engine: refresh PrsFetched[{filter_idx}] count={}",
                            prs.len()
                        );
                        let _ = notify_tx.send(Event::PrsFetched {
                            filter_idx,
                            prs,
                            rate_limit,
                        });
                    }
                    Err(e) => {
                        tracing::debug!("engine: refresh FetchPrs[{filter_idx}] error: {e}");
                    }
                }
            }
            FilterConfig::Issue(issue_filter) => {
                let host = issue_filter.host.as_deref().unwrap_or("github.com");
                let Ok(octocrab) = client.octocrab_for(host) else {
                    continue;
                };
                let cache = client.cache();
                let limit = issue_filter.limit.unwrap_or(100);
                match graphql::search_issues_all(
                    &octocrab,
                    &issue_filter.filters,
                    limit,
                    Some(&cache),
                )
                .await
                {
                    Ok((issues, rate_limit)) => {
                        scheduler.mark_fetched(filter_idx, ViewKind::Issues);
                        tracing::debug!(
                            "engine: refresh IssuesFetched[{filter_idx}] count={}",
                            issues.len()
                        );
                        let _ = notify_tx.send(Event::IssuesFetched {
                            filter_idx,
                            issues,
                            rate_limit,
                        });
                    }
                    Err(e) => {
                        tracing::debug!("engine: refresh FetchIssues[{filter_idx}] error: {e}");
                    }
                }
            }
            FilterConfig::Notification(notif_filter) => {
                let host = notif_filter.host.as_deref().unwrap_or("github.com");
                let Ok(octocrab) = client.octocrab_for(host) else {
                    continue;
                };
                let limit = notif_filter.limit.unwrap_or(50);
                let params = notif::parse_filters(&notif_filter.filters, limit);
                match notif::fetch_notifications(&octocrab, &params).await {
                    Ok(notifications) => {
                        scheduler.mark_fetched(filter_idx, ViewKind::Notifications);
                        tracing::debug!(
                            "engine: refresh NotificationsFetched[{filter_idx}] count={}",
                            notifications.len()
                        );
                        let _ = notify_tx.send(Event::NotificationsFetched {
                            filter_idx,
                            notifications,
                        });
                    }
                    Err(e) => {
                        tracing::debug!(
                            "engine: refresh FetchNotifications[{filter_idx}] error: {e}"
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Get an Octocrab instance for the given host, sending a `FetchError` on failure.
fn get_octocrab(
    client: &mut GitHubClient,
    host: &str,
    reply_tx: &Sender<Event>,
    context: &str,
) -> Option<std::sync::Arc<octocrab::Octocrab>> {
    match client.octocrab_for(host) {
        Ok(o) => Some(o),
        Err(e) => {
            tracing::debug!("engine: {context} — octocrab_for({host}) failed: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: context.to_owned(),
                message: e.to_string(),
            });
            None
        }
    }
}
