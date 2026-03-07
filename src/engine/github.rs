use std::sync::mpsc::Sender;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::actions::{issue_actions, pr_actions};
use crate::config::types::AppConfig;
use crate::github::{
    actions as gh_actions,
    client::GitHubClient,
    graphql, notifications as notif,
    rate_limit::{format_rate_limit_message, is_rate_limited},
};

use super::interface::{Engine, EngineHandle, Event, PrRef, Request};
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

/// Maximum time a single request handler may run before being cancelled.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum time the periodic background refresh may run before being cancelled.
const TICK_REFRESH_TIMEOUT: Duration = Duration::from_secs(30);

impl GitHubEngine {
    async fn run_loop(self, mut rx: UnboundedReceiver<Request>) {
        let mut client = GitHubClient::new(self.config.github.refetch_interval_minutes);
        let mut scheduler = RefreshScheduler::new();

        let interval_mins = u64::from(self.config.github.refetch_interval_minutes);
        let refresh_interval = Duration::from_secs((interval_mins * 60).max(60));
        let poll_dur = Duration::from_secs(30);
        let mut refresh_tick = tokio::time::interval(poll_dur);
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
                            let label = req.label();
                            let reply_tx = req.reply_tx();
                            if tokio::time::timeout(
                                REQUEST_TIMEOUT,
                                handle_request(req, &mut client, &mut scheduler, refresh_interval),
                            )
                            .await
                            .is_err()
                            {
                                tracing::warn!(
                                    "engine: {label} timed out after {REQUEST_TIMEOUT:?}, \
                                     cancelling to unblock engine"
                                );
                                if let Some(tx) = reply_tx {
                                    let _ = tx.send(Event::FetchError {
                                        context: label.to_owned(),
                                        message: format!(
                                            "Request timed out after {}s",
                                            REQUEST_TIMEOUT.as_secs()
                                        ),
                                    });
                                }
                            }
                        }
                    }
                }
                _ = refresh_tick.tick() => {
                    if tokio::time::timeout(
                        TICK_REFRESH_TIMEOUT,
                        tick_refresh(&mut client, &mut scheduler, refresh_interval),
                    )
                    .await
                    .is_err()
                    {
                        tracing::warn!(
                            "engine: tick_refresh timed out after {TICK_REFRESH_TIMEOUT:?}"
                        );
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_lines)]
async fn handle_request(
    req: Request,
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    refresh_interval: Duration,
) {
    let label = req.label();
    tracing::debug!("engine: received request: {label}");
    match req {
        // --- Fetch PRs ---
        Request::FetchPrs {
            filter_idx,
            filter,
            force,
            reply_tx,
        } => {
            let host = filter.host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchPrs") else {
                return;
            };
            let cache = client.cache();
            let limit = filter.limit.unwrap_or(100);
            let cache_opt = if force { None } else { Some(&cache) };
            match graphql::search_pull_requests_all(&octocrab, &filter.filters, limit, cache_opt)
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
            force,
            reply_tx,
        } => {
            let host = filter.host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchIssues") else {
                return;
            };
            let cache = client.cache();
            let limit = filter.limit.unwrap_or(100);
            let cache_opt = if force { None } else { Some(&cache) };
            match graphql::search_issues_all(&octocrab, &filter.filters, limit, cache_opt).await {
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

        // --- Fetch Actions ---
        Request::FetchActions {
            filter_idx,
            filter,
            reply_tx,
        } => {
            let host = filter.host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchActions") else {
                return;
            };
            match gh_actions::fetch_workflow_runs(&octocrab, &filter).await {
                Ok((runs, rate_limit)) => {
                    scheduler.mark_fetched(filter_idx, ViewKind::Actions);
                    tracing::debug!(
                        "engine: sending ActionsFetched[{filter_idx}] count={}",
                        runs.len()
                    );
                    let _ = reply_tx.send(Event::ActionsFetched {
                        filter_idx,
                        runs,
                        rate_limit,
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchActions[{filter_idx}] error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchActions[{filter_idx}]"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Fetch Run Jobs ---
        Request::FetchRunJobs {
            owner,
            repo,
            run_id,
            host,
            reply_tx,
        } => {
            let host = host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchRunJobs") else {
                return;
            };
            match gh_actions::fetch_run_jobs(&octocrab, &owner, &repo, run_id).await {
                Ok((jobs, rate_limit)) => {
                    tracing::debug!(
                        "engine: sending RunJobsFetched run_id={run_id} count={}",
                        jobs.len()
                    );
                    let _ = reply_tx.send(Event::RunJobsFetched {
                        run_id,
                        jobs,
                        rate_limit,
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchRunJobs run_id={run_id} error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchRunJobs[{run_id}]"),
                        message: e.to_string(),
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
                Ok((notifications, rate_limit)) => {
                    scheduler.mark_fetched(filter_idx, ViewKind::Notifications);
                    tracing::debug!(
                        "engine: sending NotificationsFetched[{filter_idx}] count={}",
                        notifications.len()
                    );
                    let _ = reply_tx.send(Event::NotificationsFetched {
                        filter_idx,
                        notifications,
                        rate_limit,
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
            pr_ref,
            force,
            reply_tx,
        } => {
            let PrRef {
                owner,
                repo,
                number,
                base_ref,
                head_repo_owner,
                head_ref,
            } = pr_ref;
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchPrDetail")
            else {
                return;
            };
            let cache = client.cache();
            if force {
                let cache_key = format!("pr:{owner}/{repo}#{number}");
                cache.remove(&cache_key).await;
            }
            match graphql::fetch_pr_detail(&octocrab, &owner, &repo, number, Some(&cache)).await {
                Ok((mut detail, rate_limit)) => {
                    if detail.behind_by.is_none()
                        && let Some(ref head_owner) = head_repo_owner
                    {
                        match graphql::fetch_compare(
                            &octocrab, &owner, &repo, &base_ref, head_owner, &head_ref,
                        )
                        .await
                        {
                            Ok(n) => detail.behind_by = n,
                            Err(e) => {
                                tracing::debug!("engine: compare API failed for #{number}: {e:#}");
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
                Ok((detail, rate_limit)) => {
                    tracing::debug!("engine: sending IssueDetailFetched #{number}");
                    let _ = reply_tx.send(Event::IssueDetailFetched {
                        number,
                        detail,
                        rate_limit,
                    });
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
        Request::RegisterRefresh { configs, notify_tx } => {
            scheduler.register(configs, refresh_interval, &notify_tx);
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
            let result =
                pr_actions::approve(&octocrab, &owner, &repo, number, body.as_deref()).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Approved PR #{number}"),
                format!("Approve PR #{number}"),
                ck,
            )
            .await;
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
            let result = pr_actions::merge(&octocrab, &owner, &repo, number).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Merged PR #{number}"),
                format!("Merge PR #{number}"),
                ck,
            )
            .await;
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
            let result = pr_actions::close(&octocrab, &owner, &repo, number).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Closed PR #{number}"),
                format!("Close PR #{number}"),
                ck,
            )
            .await;
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
            let result = pr_actions::reopen(&octocrab, &owner, &repo, number).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Reopened PR #{number}"),
                format!("Reopen PR #{number}"),
                ck,
            )
            .await;
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
            let result = pr_actions::add_comment(&octocrab, &owner, &repo, number, &body).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Added comment to PR #{number}"),
                format!("Add comment to PR #{number}"),
                ck,
            )
            .await;
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
            let result = pr_actions::update_branch(&octocrab, &owner, &repo, number).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Updated branch for PR #{number}"),
                format!("Update branch for PR #{number}"),
                ck,
            )
            .await;
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
            let result = pr_actions::ready_for_review(&octocrab, &owner, &repo, number).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Marked PR #{number} as ready for review"),
                format!("Mark PR #{number} as ready for review"),
                ck,
            )
            .await;
        }

        Request::SetPrAssignees {
            owner,
            repo,
            number,
            logins,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetPrAssignees")
            else {
                return;
            };
            let result =
                issue_actions::set_assignees(&octocrab, &owner, &repo, number, &logins).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Set assignees on PR #{number}"),
                format!("Set assignees on PR #{number}"),
                ck,
            )
            .await;
        }

        Request::SetPrLabels {
            owner,
            repo,
            number,
            labels,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetPrLabels")
            else {
                return;
            };
            let result = issue_actions::set_labels(&octocrab, &owner, &repo, number, &labels).await;
            let ck = Some(format!("pr:{owner}/{repo}#{number}"));
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Set labels on PR #{number}"),
                format!("Set labels on PR #{number}"),
                ck,
            )
            .await;
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
            let result = issue_actions::close(&octocrab, &owner, &repo, number).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Closed issue #{number}"),
                format!("Close issue #{number}"),
                None,
            )
            .await;
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
            let result = issue_actions::reopen(&octocrab, &owner, &repo, number).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Reopened issue #{number}"),
                format!("Reopen issue #{number}"),
                None,
            )
            .await;
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
            let result = issue_actions::add_comment(&octocrab, &owner, &repo, number, &body).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Added comment to issue #{number}"),
                format!("Add comment to issue #{number}"),
                None,
            )
            .await;
        }

        Request::SetIssueLabels {
            owner,
            repo,
            number,
            labels,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetIssueLabels")
            else {
                return;
            };
            let result = issue_actions::set_labels(&octocrab, &owner, &repo, number, &labels).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Set labels on issue #{number}"),
                format!("Set labels on issue #{number}"),
                None,
            )
            .await;
        }

        Request::SetIssueAssignees {
            owner,
            repo,
            number,
            logins,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetIssueAssignees")
            else {
                return;
            };
            let result =
                issue_actions::set_assignees(&octocrab, &owner, &repo, number, &logins).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Set assignees on issue #{number}"),
                format!("Set assignees on issue #{number}"),
                None,
            )
            .await;
        }

        // -----------------------------------------------------------------------
        // Mutation operations — Actions
        // -----------------------------------------------------------------------
        Request::RerunWorkflowRun {
            owner,
            repo,
            run_id,
            failed_only,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "RerunWorkflowRun")
            else {
                return;
            };
            let result =
                gh_actions::rerun_workflow_run(&octocrab, &owner, &repo, run_id, failed_only).await;
            let label = if failed_only {
                "failed jobs"
            } else {
                "all jobs"
            };
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Re-run {label} queued for run #{run_id}"),
                format!("Re-run workflow run #{run_id}"),
                None,
            )
            .await;
        }

        Request::CancelWorkflowRun {
            owner,
            repo,
            run_id,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "CancelWorkflowRun")
            else {
                return;
            };
            let result = gh_actions::cancel_workflow_run(&octocrab, &owner, &repo, run_id).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Cancelled run #{run_id}"),
                format!("Cancel workflow run #{run_id}"),
                None,
            )
            .await;
        }

        // -----------------------------------------------------------------------
        // Mutation operations — Notification
        // -----------------------------------------------------------------------
        Request::MarkNotificationRead { id, reply_tx } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "MarkNotificationRead")
            else {
                return;
            };
            let result = notif::mark_as_read(&octocrab, &id).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Marked notification {id} as read"),
                format!("Mark notification {id} as read"),
                None,
            )
            .await;
        }

        Request::MarkAllNotificationsRead { reply_tx } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "MarkAllNotificationsRead")
            else {
                return;
            };
            let result = notif::mark_all_as_read(&octocrab).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                "Marked all notifications as read".to_owned(),
                "Mark all notifications as read".to_owned(),
                None,
            )
            .await;
        }

        Request::UnsubscribeNotification { id, reply_tx } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "UnsubscribeNotification")
            else {
                return;
            };
            let result = notif::unsubscribe(&octocrab, &id).await;
            send_mutation_result(
                client,
                &reply_tx,
                result,
                format!("Unsubscribed from notification {id}"),
                format!("Unsubscribe from notification {id}"),
                None,
            )
            .await;
        }

        // --- Fetch Repo Labels ---
        Request::FetchRepoLabels {
            owner,
            repo,
            reply_tx,
        } => {
            let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchRepoLabels")
            else {
                return;
            };
            let cache = client.cache();
            match graphql::fetch_repo_labels(&octocrab, &owner, &repo, Some(&cache)).await {
                Ok((labels, rate_limit)) => {
                    tracing::debug!(
                        "engine: sending RepoLabelsFetched {owner}/{repo} count={}",
                        labels.len()
                    );
                    let _ = reply_tx.send(Event::RepoLabelsFetched {
                        labels: labels.into_iter().map(|l| l.name).collect(),
                        rate_limit,
                    });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchRepoLabels {owner}/{repo} error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchRepoLabels {owner}/{repo}"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Fetch Repo Collaborators ---
        Request::FetchRepoCollaborators {
            owner,
            repo,
            reply_tx,
        } => {
            let Some(octocrab) =
                get_octocrab(client, "github.com", &reply_tx, "FetchRepoCollaborators")
            else {
                return;
            };
            let cache = client.cache();
            match graphql::fetch_repo_collaborators(&octocrab, &owner, &repo, Some(&cache)).await {
                Ok((logins, rate_limit)) => {
                    tracing::debug!(
                        "engine: sending RepoCollaboratorsFetched {owner}/{repo} count={}",
                        logins.len()
                    );
                    let _ = reply_tx.send(Event::RepoCollaboratorsFetched { logins, rate_limit });
                }
                Err(e) => {
                    tracing::debug!("engine: FetchRepoCollaborators {owner}/{repo} error: {e}");
                    let _ = reply_tx.send(Event::FetchError {
                        context: format!("FetchRepoCollaborators {owner}/{repo}"),
                        message: if is_rate_limited(&e) {
                            format_rate_limit_message(&e)
                        } else {
                            e.to_string()
                        },
                    });
                }
            }
        }

        // --- Fetch single run by ID (deep-link navigation) ---
        Request::FetchRunById {
            owner,
            repo,
            run_id,
            host,
            reply_tx,
        } => {
            let host = host.as_deref().unwrap_or("github.com");
            let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchRunById") else {
                return;
            };
            match gh_actions::fetch_run_by_id(&octocrab, &owner, &repo, run_id).await {
                Ok((run, rate_limit)) => {
                    tracing::debug!("engine: sending SingleRunFetched run_id={run_id}");
                    let _ = reply_tx.send(Event::SingleRunFetched {
                        run_id,
                        run: Some(run),
                        rate_limit,
                    });
                }
                Err(e) => {
                    tracing::warn!("engine: FetchRunById run_id={run_id} error: {e}");
                    let _ = reply_tx.send(Event::SingleRunFetched {
                        run_id,
                        run: None,
                        rate_limit: None,
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

async fn tick_refresh(
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    refresh_interval: Duration,
) {
    for DueEntry {
        filter_idx,
        filter,
        notify_tx,
    } in scheduler.due_entries()
    {
        let req = match filter {
            FilterConfig::Pr(f) => Request::FetchPrs {
                filter_idx,
                filter: f,
                force: true,
                reply_tx: notify_tx,
            },
            FilterConfig::Issue(f) => Request::FetchIssues {
                filter_idx,
                filter: f,
                force: true,
                reply_tx: notify_tx,
            },
            FilterConfig::Notification(f) => Request::FetchNotifications {
                filter_idx,
                filter: f,
                reply_tx: notify_tx,
            },
            FilterConfig::Action(f) => Request::FetchActions {
                filter_idx,
                filter: f,
                reply_tx: notify_tx,
            },
        };
        handle_request(req, client, scheduler, refresh_interval).await;
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

/// Dispatch a mutation result: on success, optionally invalidate the cache and
/// send `MutationOk`; on failure, send `MutationError`.
async fn send_mutation_result(
    client: &GitHubClient,
    reply_tx: &Sender<Event>,
    result: Result<(), anyhow::Error>,
    ok_desc: String,
    err_desc: String,
    cache_key: Option<String>,
) {
    match result {
        Ok(()) => {
            if let Some(key) = cache_key {
                client.cache().remove(&key).await;
            }
            let _ = reply_tx.send(Event::MutationOk {
                description: ok_desc,
            });
        }
        Err(e) => {
            let _ = reply_tx.send(Event::MutationError {
                description: err_desc,
                message: e.to_string(),
            });
        }
    }
}
