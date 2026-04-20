use std::sync::mpsc::Sender;
use std::time::Duration;

use tokio::sync::mpsc::UnboundedReceiver;

use crate::actions::{issue_actions, pr_actions};
use crate::config::keybindings::{TemplateVars, execute_shell_command, expand_template};
use crate::config::types::AppConfig;
use crate::github::{
    actions as gh_actions,
    client::GitHubClient,
    graphql, notifications as notif,
    rate_limit::{format_rate_limit_message, is_rate_limited},
    security as gh_security,
};
use crate::types::{RunStatus, WorkflowRun};

use super::interface::{Engine, EngineHandle, Event, PrRef, Request};
use super::refresh::{DueEntry, FilterConfig, RefreshScheduler, ViewKind};
use super::watch::WatchScheduler;

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
        let refetch_mins = self.config.github.refetch_interval_minutes.unwrap_or(10);
        let mut client = GitHubClient::new(refetch_mins);
        let mut scheduler = RefreshScheduler::new();

        let watch_poll_secs = u64::from(
            self.config
                .actions
                .watch_poll_interval_seconds
                .unwrap_or(30),
        );
        let mut watch_scheduler = WatchScheduler::new(Duration::from_secs(watch_poll_secs));
        let complete_command = self.config.actions.watch_complete_command.clone();

        let refresh_interval = Duration::from_mins(u64::from(refetch_mins).max(1));
        let poll_dur = Duration::from_secs(30);
        let mut refresh_tick = tokio::time::interval(poll_dur);
        // Consume the first immediate tick so refresh fires after one full interval.
        refresh_tick.tick().await;

        let watch_tick_secs = (watch_poll_secs / 2).max(5);
        let mut watch_tick = tokio::time::interval(Duration::from_secs(watch_tick_secs));
        watch_tick.tick().await;

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
                                handle_request(
                                    req,
                                    &mut client,
                                    &mut scheduler,
                                    &mut watch_scheduler,
                                    complete_command.as_ref(),
                                    refresh_interval,
                                ),
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
                        tick_refresh(&mut client, &mut scheduler, &mut watch_scheduler, complete_command.as_ref(), refresh_interval),
                    )
                    .await
                    .is_err()
                    {
                        tracing::warn!(
                            "engine: tick_refresh timed out after {TICK_REFRESH_TIMEOUT:?}"
                        );
                    }
                }
                _ = watch_tick.tick(), if !watch_scheduler.is_empty() => {
                    tick_watches(&mut client, &mut watch_scheduler, complete_command.as_ref()).await;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

async fn handle_request(
    req: Request,
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    watch_scheduler: &mut WatchScheduler,
    complete_command: Option<&String>,
    refresh_interval: Duration,
) {
    let label = req.label();
    tracing::debug!("engine: received request: {label}");
    match req {
        Request::FetchPrs {
            filter_idx,
            filter,
            force,
            reply_tx,
        } => handle_fetch_prs(client, scheduler, filter_idx, filter, force, reply_tx).await,
        Request::FetchIssues {
            filter_idx,
            filter,
            force,
            reply_tx,
        } => handle_fetch_issues(client, scheduler, filter_idx, filter, force, reply_tx).await,
        Request::FetchActions {
            filter_idx,
            filter,
            reply_tx,
        } => handle_fetch_actions(client, scheduler, filter_idx, filter, reply_tx).await,
        Request::FetchAlerts {
            filter_idx,
            filter,
            reply_tx,
        } => handle_fetch_alerts(client, scheduler, filter_idx, filter, reply_tx).await,
        Request::FetchSecretLocations {
            owner,
            repo,
            alert_number,
            reply_tx,
        } => handle_fetch_secret_locations(client, owner, repo, alert_number, reply_tx).await,
        Request::FetchRunJobs {
            owner,
            repo,
            run_id,
            host,
            reply_tx,
        } => handle_fetch_run_jobs(client, owner, repo, run_id, host, reply_tx).await,
        Request::FetchNotifications {
            filter_idx,
            filter,
            reply_tx,
        } => handle_fetch_notifications(client, scheduler, filter_idx, filter, reply_tx).await,
        Request::FetchPrDetail {
            pr_ref,
            force,
            reply_tx,
        } => handle_fetch_pr_detail(client, pr_ref, force, reply_tx).await,
        Request::FetchIssueDetail {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_fetch_issue_detail(client, owner, repo, number, reply_tx).await,
        Request::PrefetchPrDetails { prs, reply_tx } => {
            handle_prefetch_pr_details(client, prs, reply_tx).await
        }
        Request::RegisterRefresh { configs, notify_tx } => {
            scheduler.register(configs, refresh_interval, &notify_tx)
        }
        Request::ApprovePr {
            owner,
            repo,
            number,
            body,
            reply_tx,
        } => handle_approve_pr(client, owner, repo, number, body, reply_tx).await,
        Request::MergePr {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_merge_pr(client, owner, repo, number, reply_tx).await,
        Request::ClosePr {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_close_pr(client, owner, repo, number, reply_tx).await,
        Request::ReopenPr {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_reopen_pr(client, owner, repo, number, reply_tx).await,
        Request::AddPrComment {
            owner,
            repo,
            number,
            body,
            reply_tx,
        } => handle_add_pr_comment(client, owner, repo, number, body, reply_tx).await,
        Request::UpdateBranch {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_update_branch(client, owner, repo, number, reply_tx).await,
        Request::ReadyForReview {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_ready_for_review(client, owner, repo, number, reply_tx).await,
        Request::SetPrAssignees {
            owner,
            repo,
            number,
            logins,
            reply_tx,
        } => handle_set_pr_assignees(client, owner, repo, number, logins, reply_tx).await,
        Request::SetPrLabels {
            owner,
            repo,
            number,
            labels,
            reply_tx,
        } => handle_set_pr_labels(client, owner, repo, number, labels, reply_tx).await,
        Request::CloseIssue {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_close_issue(client, owner, repo, number, reply_tx).await,
        Request::ReopenIssue {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_reopen_issue(client, owner, repo, number, reply_tx).await,
        Request::AddIssueComment {
            owner,
            repo,
            number,
            body,
            reply_tx,
        } => handle_add_issue_comment(client, owner, repo, number, body, reply_tx).await,
        Request::SetIssueLabels {
            owner,
            repo,
            number,
            labels,
            reply_tx,
        } => handle_set_issue_labels(client, owner, repo, number, labels, reply_tx).await,
        Request::SetIssueAssignees {
            owner,
            repo,
            number,
            logins,
            reply_tx,
        } => handle_set_issue_assignees(client, owner, repo, number, logins, reply_tx).await,
        Request::RerunWorkflowRun {
            owner,
            repo,
            run_id,
            failed_only,
            reply_tx,
        } => handle_rerun_workflow_run(client, owner, repo, run_id, failed_only, reply_tx).await,
        Request::CancelWorkflowRun {
            owner,
            repo,
            run_id,
            reply_tx,
        } => handle_cancel_workflow_run(client, owner, repo, run_id, reply_tx).await,
        Request::MarkNotificationRead { id, reply_tx } => {
            handle_mark_notification_read(client, id, reply_tx).await
        }
        Request::MarkAllNotificationsRead { reply_tx } => {
            handle_mark_all_notifications_read(client, reply_tx).await
        }
        Request::UnsubscribeNotification { id, reply_tx } => {
            handle_unsubscribe_notification(client, id, reply_tx).await
        }
        Request::FetchRepoLabels {
            owner,
            repo,
            reply_tx,
        } => handle_fetch_repo_labels(client, owner, repo, reply_tx).await,
        Request::FetchRepoCollaborators {
            owner,
            repo,
            reply_tx,
        } => handle_fetch_repo_collaborators(client, owner, repo, reply_tx).await,
        Request::RefreshPr {
            owner,
            repo,
            number,
            base_ref,
            head_repo_owner,
            head_ref,
            reply_tx,
        } => {
            let pr_ref = PrRef {
                owner,
                repo,
                number,
                base_ref,
                head_repo_owner,
                head_ref,
            };
            handle_refresh_pr(client, pr_ref, reply_tx).await;
        }
        Request::RefreshIssue {
            owner,
            repo,
            number,
            reply_tx,
        } => handle_refresh_issue(client, owner, repo, number, reply_tx).await,
        Request::FetchRunById {
            owner,
            repo,
            run_id,
            host,
            reply_tx,
        } => handle_fetch_run_by_id(client, owner, repo, run_id, host, reply_tx).await,
        Request::WatchRun {
            owner,
            repo,
            run_id,
            host,
            reply_tx,
        } => {
            handle_watch_run(
                client,
                watch_scheduler,
                complete_command,
                owner,
                repo,
                run_id,
                host,
                reply_tx,
            )
            .await
        }
        Request::UnwatchRun { run_id } => {
            watch_scheduler.remove(run_id);
            tracing::debug!("engine: unwatched run_id={run_id}");
        }
        Request::Shutdown => unreachable!("handled at run_loop level"),
    }
}

// ---------------------------------------------------------------------------
// Per-request handler functions
// ---------------------------------------------------------------------------

/// Format an error for reply, using rate-limit info when available.
fn format_fetch_error(e: &anyhow::Error) -> String {
    if is_rate_limited(e) {
        format_rate_limit_message(e)
    } else {
        e.to_string()
    }
}

async fn handle_fetch_prs(
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    filter_idx: usize,
    filter: crate::config::types::PrFilter,
    force: bool,
    reply_tx: Sender<Event>,
) {
    let host = filter.host.as_deref().unwrap_or("github.com");
    let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchPrs") else {
        return;
    };
    let cache = client.cache();
    let limit = filter.limit.unwrap_or(100);
    let cache_opt = if force { None } else { Some(&cache) };
    match graphql::search_pull_requests_all(&octocrab, &filter.filters, limit, cache_opt).await {
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
            tracing::warn!("engine: FetchPrs[{filter_idx}] error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchPrs[{filter_idx}]"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_fetch_issues(
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    filter_idx: usize,
    filter: crate::config::types::IssueFilter,
    force: bool,
    reply_tx: Sender<Event>,
) {
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
            tracing::warn!("engine: FetchIssues[{filter_idx}] error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchIssues[{filter_idx}]"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_fetch_actions(
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    filter_idx: usize,
    filter: crate::config::types::ActionsFilter,
    reply_tx: Sender<Event>,
) {
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
            tracing::warn!("engine: FetchActions[{filter_idx}] error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchActions[{filter_idx}]"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_fetch_alerts(
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    filter_idx: usize,
    filter: crate::config::types::AlertsFilter,
    reply_tx: Sender<Event>,
) {
    let host = filter.host.as_deref().unwrap_or("github.com");
    let Some(octocrab) = get_octocrab(client, host, &reply_tx, "FetchAlerts") else {
        return;
    };
    let Some((owner, repo)) = filter.repo.split_once('/') else {
        let _ = reply_tx.send(Event::FetchError {
            context: format!("FetchAlerts[{filter_idx}]"),
            message: format!("invalid repo format: {}", filter.repo),
        });
        return;
    };
    let limit = filter.limit.unwrap_or(30).min(100);

    let mut all_alerts = Vec::new();
    let mut last_rl = None;

    match gh_security::fetch_dependabot_alerts(&octocrab, owner, repo, limit).await {
        Ok((alerts, rl)) => {
            all_alerts.extend(alerts);
            if rl.is_some() {
                last_rl = rl;
            }
        }
        Err(e) => tracing::warn!("engine: FetchAlerts[{filter_idx}] dependabot: {e}"),
    }
    match gh_security::fetch_code_scanning_alerts(&octocrab, owner, repo, limit).await {
        Ok((alerts, rl)) => {
            all_alerts.extend(alerts);
            if rl.is_some() {
                last_rl = rl;
            }
        }
        Err(e) => tracing::warn!("engine: FetchAlerts[{filter_idx}] code-scanning: {e}"),
    }
    match gh_security::fetch_secret_scanning_alerts(&octocrab, owner, repo, limit).await {
        Ok((alerts, rl)) => {
            all_alerts.extend(alerts);
            if rl.is_some() {
                last_rl = rl;
            }
        }
        Err(e) => tracing::warn!("engine: FetchAlerts[{filter_idx}] secret-scanning: {e}"),
    }

    all_alerts.sort_by_key(|a| std::cmp::Reverse(a.created_at));

    scheduler.mark_fetched(filter_idx, ViewKind::Alerts);
    tracing::debug!(
        "engine: sending AlertsFetched[{filter_idx}] count={}",
        all_alerts.len()
    );
    let _ = reply_tx.send(Event::AlertsFetched {
        filter_idx,
        alerts: all_alerts,
        rate_limit: last_rl,
    });
}

async fn handle_fetch_secret_locations(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    alert_number: u64,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchSecretLocations")
    else {
        return;
    };
    match gh_security::fetch_secret_alert_locations(&octocrab, &owner, &repo, alert_number).await {
        Ok((locations, rate_limit)) => {
            let _ = reply_tx.send(Event::SecretLocationsFetched {
                alert_number,
                locations,
                rate_limit,
            });
        }
        Err(e) => {
            tracing::warn!("engine: FetchSecretLocations[{alert_number}] error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchSecretLocations[{alert_number}]"),
                message: e.to_string(),
            });
        }
    }
}

async fn handle_fetch_run_jobs(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    run_id: u64,
    host: Option<String>,
    reply_tx: Sender<Event>,
) {
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
            tracing::warn!("engine: FetchRunJobs run_id={run_id} error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchRunJobs[{run_id}]"),
                message: e.to_string(),
            });
        }
    }
}

async fn handle_fetch_notifications(
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    filter_idx: usize,
    filter: crate::config::types::NotificationFilter,
    reply_tx: Sender<Event>,
) {
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
            tracing::warn!("engine: FetchNotifications[{filter_idx}] error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchNotifications[{filter_idx}]"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_fetch_pr_detail(
    client: &mut GitHubClient,
    pr_ref: PrRef,
    force: bool,
    reply_tx: Sender<Event>,
) {
    let PrRef {
        owner,
        repo,
        number,
        base_ref,
        head_repo_owner,
        head_ref,
    } = pr_ref;
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchPrDetail") else {
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
                    Err(e) => tracing::warn!("engine: compare API failed for #{number}: {e:#}"),
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
            tracing::warn!("engine: FetchPrDetail #{number} error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchPrDetail #{number}"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_fetch_issue_detail(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchIssueDetail") else {
        return;
    };
    let cache = client.cache();
    match graphql::fetch_issue_detail(&octocrab, &owner, &repo, number, Some(&cache)).await {
        Ok((detail, rate_limit)) => {
            tracing::debug!("engine: sending IssueDetailFetched #{number}");
            let _ = reply_tx.send(Event::IssueDetailFetched {
                number,
                detail,
                rate_limit,
            });
        }
        Err(e) => {
            tracing::warn!("engine: FetchIssueDetail #{number} error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchIssueDetail #{number}"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_prefetch_pr_details(
    client: &mut GitHubClient,
    prs: Vec<PrRef>,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "PrefetchPrDetails") else {
        return;
    };
    let cache = client.cache();
    for pr in prs {
        let number = pr.number;
        match graphql::fetch_pr_detail(&octocrab, &pr.owner, &pr.repo, number, Some(&cache)).await {
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
                            tracing::debug!("engine: compare API failed for #{number}: {e:#}");
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
                tracing::warn!("engine: PrefetchPrDetails #{number} error: {e}");
            }
        }
    }
}

async fn handle_approve_pr(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    body: Option<String>,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ApprovePr") else {
        return;
    };
    let result = pr_actions::approve(&octocrab, &owner, &repo, number, body.as_deref()).await;
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

async fn handle_merge_pr(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
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

async fn handle_close_pr(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
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

async fn handle_reopen_pr(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
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

async fn handle_add_pr_comment(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    body: String,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "AddPrComment") else {
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

async fn handle_update_branch(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "UpdateBranch") else {
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

async fn handle_ready_for_review(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ReadyForReview") else {
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

async fn handle_set_pr_assignees(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    logins: Vec<String>,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetPrAssignees") else {
        return;
    };
    let result = issue_actions::set_assignees(&octocrab, &owner, &repo, number, &logins).await;
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

async fn handle_set_pr_labels(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    labels: Vec<String>,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetPrLabels") else {
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

async fn handle_close_issue(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
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

async fn handle_reopen_issue(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "ReopenIssue") else {
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

async fn handle_add_issue_comment(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    body: String,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "AddIssueComment") else {
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

async fn handle_set_issue_labels(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    labels: Vec<String>,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetIssueLabels") else {
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

async fn handle_set_issue_assignees(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    logins: Vec<String>,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "SetIssueAssignees") else {
        return;
    };
    let result = issue_actions::set_assignees(&octocrab, &owner, &repo, number, &logins).await;
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

async fn handle_rerun_workflow_run(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    run_id: u64,
    failed_only: bool,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "RerunWorkflowRun") else {
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

async fn handle_cancel_workflow_run(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    run_id: u64,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "CancelWorkflowRun") else {
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

async fn handle_mark_notification_read(
    client: &mut GitHubClient,
    id: String,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "MarkNotificationRead")
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

async fn handle_mark_all_notifications_read(client: &mut GitHubClient, reply_tx: Sender<Event>) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "MarkAllNotificationsRead")
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

async fn handle_unsubscribe_notification(
    client: &mut GitHubClient,
    id: String,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "UnsubscribeNotification")
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

async fn handle_fetch_repo_labels(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchRepoLabels") else {
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
            tracing::warn!("engine: FetchRepoLabels {owner}/{repo} error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchRepoLabels {owner}/{repo}"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_fetch_repo_collaborators(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "FetchRepoCollaborators")
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
            tracing::warn!("engine: FetchRepoCollaborators {owner}/{repo} error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("FetchRepoCollaborators {owner}/{repo}"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_refresh_pr(client: &mut GitHubClient, pr_ref: PrRef, reply_tx: Sender<Event>) {
    let PrRef {
        owner,
        repo,
        number,
        base_ref,
        head_repo_owner,
        head_ref,
    } = pr_ref;
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "RefreshPr") else {
        return;
    };
    let cache = client.cache();
    let full_key = format!("full_pr:{owner}/{repo}#{number}");
    let detail_key = format!("pr:{owner}/{repo}#{number}");
    cache.remove(&full_key).await;
    cache.remove(&detail_key).await;
    match graphql::fetch_single_pr(&octocrab, &owner, &repo, number, Some(&cache)).await {
        Ok((pr, mut detail, rate_limit)) => {
            if detail.behind_by.is_none()
                && let Some(ref head_owner) = head_repo_owner
            {
                match graphql::fetch_compare(
                    &octocrab, &owner, &repo, &base_ref, head_owner, &head_ref,
                )
                .await
                {
                    Ok(n) => detail.behind_by = n,
                    Err(e) => tracing::warn!("engine: compare API failed for #{number}: {e:#}"),
                }
            }
            tracing::debug!("engine: sending PrRefreshed #{number}");
            let _ = reply_tx.send(Event::PrRefreshed {
                number,
                pr: Box::new(pr),
                detail,
                rate_limit,
            });
        }
        Err(e) => {
            tracing::warn!("engine: RefreshPr #{number} error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("RefreshPr #{number}"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_refresh_issue(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    number: u64,
    reply_tx: Sender<Event>,
) {
    let Some(octocrab) = get_octocrab(client, "github.com", &reply_tx, "RefreshIssue") else {
        return;
    };
    let cache = client.cache();
    let full_key = format!("full_issue:{owner}/{repo}#{number}");
    let detail_key = format!("issue:{owner}/{repo}#{number}");
    cache.remove(&full_key).await;
    cache.remove(&detail_key).await;
    match graphql::fetch_single_issue(&octocrab, &owner, &repo, number, Some(&cache)).await {
        Ok((issue, detail, rate_limit)) => {
            tracing::debug!("engine: sending IssueRefreshed #{number}");
            let _ = reply_tx.send(Event::IssueRefreshed {
                number,
                issue: Box::new(issue),
                detail,
                rate_limit,
            });
        }
        Err(e) => {
            tracing::warn!("engine: RefreshIssue #{number} error: {e}");
            let _ = reply_tx.send(Event::FetchError {
                context: format!("RefreshIssue #{number}"),
                message: format_fetch_error(&e),
            });
        }
    }
}

async fn handle_fetch_run_by_id(
    client: &mut GitHubClient,
    owner: String,
    repo: String,
    run_id: u64,
    host: Option<String>,
    reply_tx: Sender<Event>,
) {
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

#[expect(clippy::too_many_arguments)]
async fn handle_watch_run(
    client: &mut GitHubClient,
    watch_scheduler: &mut WatchScheduler,
    complete_command: Option<&String>,
    owner: String,
    repo: String,
    run_id: u64,
    host: Option<String>,
    reply_tx: Sender<Event>,
) {
    watch_scheduler.add(
        owner.clone(),
        repo.clone(),
        run_id,
        host.clone(),
        reply_tx.clone(),
    );
    let api_host = host.as_deref().unwrap_or("github.com");
    let Some(octocrab) = get_octocrab(client, api_host, &reply_tx, "WatchRun") else {
        return;
    };
    match gh_actions::fetch_run_by_id(&octocrab, &owner, &repo, run_id).await {
        Ok((run, rate_limit)) => {
            let completed = run.status == RunStatus::Completed;
            let _ = reply_tx.send(Event::WatchedRunUpdated {
                run_id,
                run: run.clone(),
                completed,
                rate_limit,
            });
            if completed {
                fire_watch_hook(complete_command, &run, &owner, &repo, &reply_tx);
                watch_scheduler.complete(run_id);
            } else {
                watch_scheduler.mark_polled(run_id);
            }
        }
        Err(e) => {
            tracing::warn!("engine: WatchRun initial fetch run_id={run_id} error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Background refresh
// ---------------------------------------------------------------------------

async fn tick_refresh(
    client: &mut GitHubClient,
    scheduler: &mut RefreshScheduler,
    watch_scheduler: &mut WatchScheduler,
    complete_command: Option<&String>,
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
            FilterConfig::Alert(f) => Request::FetchAlerts {
                filter_idx,
                filter: f,
                reply_tx: notify_tx,
            },
        };
        handle_request(
            req,
            client,
            scheduler,
            watch_scheduler,
            complete_command,
            refresh_interval,
        )
        .await;
    }
}

// ---------------------------------------------------------------------------
// Watch polling
// ---------------------------------------------------------------------------

async fn tick_watches(
    client: &mut GitHubClient,
    watch_scheduler: &mut WatchScheduler,
    complete_command: Option<&String>,
) {
    let due: Vec<_> = watch_scheduler.due_entries();
    for entry in due {
        let host = entry.host.as_deref().unwrap_or("github.com");
        let octocrab = match client.octocrab_for(host) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!(
                    "engine: watch poll for run_id={} — octocrab_for({host}) failed: {e}",
                    entry.run_id
                );
                continue;
            }
        };
        match gh_actions::fetch_run_by_id(&octocrab, &entry.owner, &entry.repo, entry.run_id).await
        {
            Ok((run, rate_limit)) => {
                let completed = run.status == RunStatus::Completed;
                let send_ok = entry
                    .reply_tx
                    .send(Event::WatchedRunUpdated {
                        run_id: entry.run_id,
                        run: run.clone(),
                        completed,
                        rate_limit,
                    })
                    .is_ok();
                watch_scheduler.mark_polled(entry.run_id);
                if !send_ok {
                    // Channel closed — UI view dropped.
                    tracing::debug!(
                        "engine: watch poll channel closed for run_id={}, evicting",
                        entry.run_id
                    );
                    watch_scheduler.complete(entry.run_id);
                    continue;
                }
                if completed {
                    fire_watch_hook(
                        complete_command,
                        &run,
                        &entry.owner,
                        &entry.repo,
                        &entry.reply_tx,
                    );
                    watch_scheduler.complete(entry.run_id);
                }
            }
            Err(e) => {
                tracing::warn!("engine: watch poll for run_id={} error: {e}", entry.run_id);
                watch_scheduler.mark_polled(entry.run_id);
            }
        }
    }
}

fn fire_watch_hook(
    complete_command: Option<&String>,
    run: &WorkflowRun,
    owner: &str,
    repo: &str,
    reply_tx: &Sender<Event>,
) {
    let Some(cmd_template) = complete_command else {
        return;
    };
    let conclusion_str = run.conclusion.map_or("unknown", |c| c.as_str()).to_owned();
    let conclusion_emoji_str = run.conclusion.map_or("\u{2753}", |c| c.emoji()).to_owned();
    let vars = TemplateVars {
        url: run.html_url.clone(),
        repo_name: format!("{owner}/{repo}"),
        head_branch: run.head_branch.clone().unwrap_or_default(),
        run_id: run.id.to_string(),
        run_name: run.name.clone(),
        run_number: run.run_number.to_string(),
        conclusion: conclusion_str,
        conclusion_emoji: conclusion_emoji_str,
        ..Default::default()
    };
    let expanded = expand_template(cmd_template, &vars);
    let run_id = run.id;
    let reply = reply_tx.clone();
    tokio::task::spawn_blocking(move || {
        let result = execute_shell_command(&expanded);
        match result {
            Ok(output) => {
                let _ = reply.send(Event::WatchHookResult {
                    run_id,
                    success: true,
                    message: output,
                });
            }
            Err(e) => {
                let _ = reply.send(Event::WatchHookResult {
                    run_id,
                    success: false,
                    message: e.to_string(),
                });
            }
        }
    });
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
            tracing::warn!("engine: {context} — octocrab_for({host}) failed: {e}");
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
