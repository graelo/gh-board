use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use moka::future::Cache;
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};

use crate::github::types::{
    Actor, AuthorAssociation, CheckConclusion, CheckRun, CheckStatus, Commit, CommitCheckState,
    File, FileChangeType, Issue, IssueState, Label, MergeStateStatus, MergeableState, PrState,
    PullRequest, ReactionGroups, RepoRef, Review, ReviewDecision, ReviewState, ReviewThread,
    TimelineEvent,
};

// Re-export types moved to crate::types so existing importers continue to work.
pub use crate::types::{IssueDetail, PrDetail, RateLimitInfo};

// ---------------------------------------------------------------------------
// GraphQL query strings
// ---------------------------------------------------------------------------

const SEARCH_PULL_REQUESTS_QUERY: &str = r"
query SearchPullRequests($query: String!, $first: Int!, $after: String) {
  rateLimit { limit remaining cost }
  search(query: $query, type: ISSUE, first: $first, after: $after) {
    pageInfo {
      hasNextPage
      endCursor
    }
    nodes {
      ... on PullRequest {
        number
        title
        body
        state
        isDraft
        mergeable
        reviewDecision
        additions
        deletions
        headRefName
        baseRefName
        mergeStateStatus
        headRepository { owner { login } name }
        url
        updatedAt
        createdAt
        author { login avatarUrl }
        authorAssociation
        labels(first: 10) { nodes { name color } }
        assignees(first: 10) { nodes { login } }
        comments { totalCount }
        latestReviews(first: 10) {
          nodes {
            state
            author { login }
          }
        }
        reviewRequests(first: 10) {
          nodes {
            requestedReviewer {
              ... on User { login }
            }
          }
        }
        commits(last: 1) {
          nodes {
            commit {
              statusCheckRollup {
                contexts(first: 50) {
                  nodes {
                    ... on CheckRun {
                      name status conclusion detailsUrl startedAt completedAt
                      checkSuite {
                        workflowRun {
                          databaseId
                          workflow { name }
                        }
                      }
                    }
                    ... on StatusContext { context state targetUrl }
                  }
                }
              }
            }
          }
        }
        participants(first: 30) { nodes { login } }
        repository { nameWithOwner }
      }
    }
  }
}
";

const PR_DETAIL_QUERY: &str = r"
query PullRequestDetail($owner: String!, $repo: String!, $number: Int!) {
  rateLimit { limit remaining cost }
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      body
      mergeable
      reviews(last: 50) {
        nodes { author { login } state body submittedAt }
      }
      reviewThreads(first: 50) {
        nodes { isResolved comments(first: 10) { nodes { author { login } body createdAt } } }
      }
      timelineItems(last: 100) {
        nodes {
          __typename
          ... on IssueComment { author { login } body createdAt }
          ... on PullRequestReview { author { login } state body submittedAt }
          ... on MergedEvent { actor { login } createdAt }
          ... on ClosedEvent { actor { login } createdAt }
          ... on ReopenedEvent { actor { login } createdAt }
          ... on HeadRefForcePushedEvent { actor { login } createdAt }
        }
      }
      commits(first: 100) {
        nodes { commit { oid messageHeadline author { name } committedDate statusCheckRollup { state } } }
      }
      files(first: 100) {
        nodes { path additions deletions changeType }
      }
    }
  }
}
";

const ISSUE_DETAIL_QUERY: &str = r"
query IssueDetail($owner: String!, $repo: String!, $number: Int!) {
  rateLimit { limit remaining cost }
  repository(owner: $owner, name: $repo) {
    issue(number: $number) {
      body
      timelineItems(last: 100) {
        nodes {
          __typename
          ... on IssueComment { author { login } body createdAt }
          ... on ClosedEvent { actor { login } createdAt }
          ... on ReopenedEvent { actor { login } createdAt }
        }
      }
    }
  }
}
";

const REPOSITORY_LABELS_QUERY: &str = r"
query RepositoryLabels($owner: String!, $repo: String!, $first: Int!) {
  rateLimit { limit remaining cost }
  repository(owner: $owner, name: $repo) {
    labels(first: $first, orderBy: { field: NAME, direction: ASC }) {
      nodes { name color description }
    }
  }
}
";

const REPOSITORY_COLLABORATORS_QUERY: &str = r"
query RepositoryCollaborators($owner: String!, $repo: String!, $first: Int!) {
  rateLimit { limit remaining cost }
  repository(owner: $owner, name: $repo) {
    collaborators(first: $first, affiliation: ALL) {
      nodes { login }
    }
  }
}
";

const SEARCH_ISSUES_QUERY: &str = r"
query SearchIssues($query: String!, $first: Int!, $after: String) {
  rateLimit { limit remaining cost }
  search(query: $query, type: ISSUE, first: $first, after: $after) {
    pageInfo { hasNextPage endCursor }
    nodes {
      ... on Issue {
        number
        title
        body
        state
        url
        updatedAt
        createdAt
        author { login avatarUrl }
        assignees(first: 10) { nodes { login } }
        labels(first: 10) { nodes { name color } }
        comments { totalCount }
        reactionGroups { content users { totalCount } }
        participants(first: 30) { nodes { login } }
        repository { nameWithOwner }
      }
    }
  }
}
";

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Ensure the search query contains `is:<type_qualifier>` (e.g. `is:pr` or
/// `is:issue`). If missing, prepend it so that the GraphQL search only returns
/// the expected node type and avoids deserialization failures from mixed results.
fn ensure_type_qualifier(query: &str, qualifier: &str) -> String {
    let tag = format!("is:{qualifier}");
    if query
        .split_whitespace()
        .any(|token| token.eq_ignore_ascii_case(&tag))
    {
        query.to_owned()
    } else {
        format!("{tag} {query}")
    }
}

// ---------------------------------------------------------------------------
// Request payload
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct GraphQLPayload<V: Serialize> {
    query: &'static str,
    variables: V,
}

#[derive(Serialize)]
struct SearchVariables {
    query: String,
    first: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    after: Option<String>,
}

#[derive(Serialize)]
struct PrDetailVariables {
    owner: String,
    repo: String,
    number: i64,
}

// ---------------------------------------------------------------------------
// Response types (mirror the GraphQL response shape)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GraphQLResponse<D> {
    data: Option<D>,
    errors: Option<Vec<GraphQLError>>,
}

#[derive(Debug, Deserialize)]
struct GraphQLError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    search: SearchResult,
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    #[serde(default)]
    nodes: Vec<Option<RawPullRequest>>,
}

#[derive(Debug, Deserialize)]
struct IssueSearchData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    search: IssueSearchResult,
}

#[derive(Debug, Deserialize)]
struct IssueSearchResult {
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
    #[serde(default)]
    nodes: Vec<Option<RawIssue>>,
}

/// Pagination info from GraphQL.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct PageInfo {
    #[serde(rename = "hasNextPage")]
    pub has_next_page: bool,
    #[serde(rename = "endCursor")]
    pub end_cursor: Option<String>,
}

// ---------------------------------------------------------------------------
// PR detail response types (Q2)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct PrDetailData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    repository: Option<PrDetailRepo>,
}

#[derive(Debug, Deserialize)]
struct PrDetailRepo {
    #[serde(rename = "pullRequest")]
    pull_request: Option<RawPrDetail>,
}

#[derive(Debug, Deserialize)]
struct RawPrDetail {
    #[serde(default)]
    body: String,
    mergeable: Option<MergeableState>,
    reviews: Option<Connection<RawReview>>,
    #[serde(rename = "reviewThreads")]
    review_threads: Option<Connection<RawReviewThread>>,
    #[serde(rename = "timelineItems")]
    timeline_items: Option<Connection<RawTimelineItem>>,
    commits: Option<Connection<RawDetailCommitNode>>,
    files: Option<Connection<RawFile>>,
}

#[derive(Debug, Deserialize)]
struct RawReview {
    author: Option<RawActor>,
    state: ReviewState,
    #[serde(default)]
    body: String,
    #[serde(rename = "submittedAt")]
    submitted_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct RawReviewThread {
    #[serde(rename = "isResolved", default)]
    is_resolved: bool,
    comments: Option<Connection<RawComment>>,
}

#[derive(Debug, Deserialize)]
struct RawComment {
    author: Option<RawActor>,
    #[serde(default)]
    body: String,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RawTimelineItem {
    #[serde(rename = "__typename")]
    typename: String,
    // IssueComment / PullRequestReview fields
    author: Option<RawActor>,
    #[serde(default)]
    body: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<DateTime<Utc>>,
    #[serde(rename = "submittedAt")]
    submitted_at: Option<DateTime<Utc>>,
    state: Option<ReviewState>,
    // MergedEvent / ClosedEvent / ReopenedEvent / HeadRefForcePushedEvent
    actor: Option<RawActor>,
}

#[derive(Debug, Deserialize)]
struct RawDetailCommitNode {
    commit: Option<RawDetailCommit>,
}

#[derive(Debug, Deserialize)]
struct RawDetailCommit {
    oid: String,
    #[serde(rename = "messageHeadline", default)]
    message_headline: String,
    author: Option<RawCommitAuthor>,
    #[serde(rename = "committedDate")]
    committed_date: Option<DateTime<Utc>>,
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<RawDetailStatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
struct RawCommitAuthor {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawDetailStatusCheckRollup {
    state: Option<CommitCheckState>,
}

#[derive(Debug, Deserialize)]
struct RawFile {
    path: String,
    #[serde(default)]
    additions: u32,
    #[serde(default)]
    deletions: u32,
    #[serde(rename = "changeType")]
    change_type: Option<FileChangeType>,
}

// ---------------------------------------------------------------------------
// Raw search PR response types (Q1)
// ---------------------------------------------------------------------------

/// Raw PR as returned by the GraphQL API (camelCase, nested connections).
#[derive(Debug, Deserialize)]
struct RawPullRequest {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    state: PrState,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
    mergeable: Option<MergeableState>,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<ReviewDecision>,
    #[serde(default)]
    additions: u32,
    #[serde(default)]
    deletions: u32,
    #[serde(rename = "headRefName", default)]
    head_ref_name: String,
    #[serde(rename = "baseRefName", default)]
    base_ref_name: String,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: Option<MergeStateStatus>,
    #[serde(rename = "headRepository")]
    head_repository: Option<RawHeadRepository>,
    url: String,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<Utc>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
    author: Option<RawActor>,
    #[serde(rename = "authorAssociation")]
    author_association: Option<AuthorAssociation>,
    labels: Option<Connection<RawLabel>>,
    assignees: Option<Connection<RawAssignee>>,
    comments: Option<TotalCount>,
    #[serde(rename = "latestReviews")]
    latest_reviews: Option<Connection<RawLatestReview>>,
    #[serde(rename = "reviewRequests")]
    review_requests: Option<Connection<RawReviewRequest>>,
    commits: Option<Connection<RawCommitNode>>,
    participants: Option<Connection<RawAssignee>>,
    repository: Option<RawRepository>,
}

#[derive(Debug, Deserialize)]
struct RawActor {
    login: String,
    #[serde(rename = "avatarUrl", default)]
    avatar_url: String,
}

#[derive(Debug, Deserialize)]
struct RawHeadRepository {
    owner: RawActorLogin,
    name: String,
}

/// Minimal actor with only `login` (no `avatarUrl`), used for nested owner nodes.
#[derive(Debug, Deserialize)]
struct RawActorLogin {
    login: String,
}

#[derive(Debug, Deserialize)]
#[serde(bound(deserialize = "T: serde::de::DeserializeOwned"))]
struct Connection<T> {
    #[serde(default)]
    nodes: Vec<Option<T>>,
}

#[derive(Debug, Deserialize)]
struct RawLabel {
    name: String,
    color: String,
}

#[derive(Debug, Deserialize)]
struct RawAssignee {
    login: String,
}

#[derive(Debug, Deserialize)]
struct TotalCount {
    #[serde(rename = "totalCount", default)]
    total_count: u32,
}

#[derive(Debug, Deserialize)]
struct RawLatestReview {
    state: Option<ReviewState>,
    author: Option<RawActor>,
}

#[derive(Debug, Deserialize)]
struct RawReviewRequest {
    #[serde(rename = "requestedReviewer")]
    requested_reviewer: Option<RawReviewer>,
}

#[derive(Debug, Deserialize)]
struct RawReviewer {
    login: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawCommitNode {
    commit: Option<RawCommit>,
}

#[derive(Debug, Deserialize)]
struct RawCommit {
    #[serde(rename = "statusCheckRollup")]
    status_check_rollup: Option<RawStatusCheckRollup>,
}

#[derive(Debug, Deserialize)]
struct RawStatusCheckRollup {
    contexts: Option<Connection<RawCheckContext>>,
}

/// A check context can be either a `CheckRun` or a `StatusContext`.
/// We unify them into our `CheckRun` domain type.
#[derive(Debug, Deserialize)]
struct RawCheckContext {
    // CheckRun fields
    name: Option<String>,
    status: Option<CheckStatus>,
    conclusion: Option<CheckConclusion>,
    #[serde(rename = "detailsUrl")]
    details_url: Option<String>,
    #[serde(rename = "startedAt")]
    started_at: Option<DateTime<Utc>>,
    #[serde(rename = "completedAt")]
    completed_at: Option<DateTime<Utc>>,
    #[serde(rename = "checkSuite")]
    check_suite: Option<RawCheckSuite>,
    // StatusContext fields
    context: Option<String>,
    state: Option<String>,
    #[serde(rename = "targetUrl")]
    target_url: Option<String>,
}

/// Nested `checkSuite.workflowRun` from the GraphQL response.
#[derive(Debug, Deserialize)]
struct RawCheckSuite {
    #[serde(rename = "workflowRun")]
    workflow_run: Option<RawCheckSuiteWorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct RawCheckSuiteWorkflowRun {
    #[serde(rename = "databaseId")]
    database_id: Option<u64>,
    workflow: Option<RawCheckSuiteWorkflow>,
}

#[derive(Debug, Deserialize)]
struct RawCheckSuiteWorkflow {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRepository {
    #[serde(rename = "nameWithOwner")]
    name_with_owner: String,
}

/// Raw Issue as returned by the GraphQL API.
#[derive(Debug, Deserialize)]
struct RawIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    state: IssueState,
    url: String,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<Utc>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
    author: Option<RawActor>,
    assignees: Option<Connection<RawAssignee>>,
    labels: Option<Connection<RawLabel>>,
    comments: Option<TotalCount>,
    #[serde(rename = "reactionGroups", default)]
    reaction_groups: Vec<RawReactionGroup>,
    participants: Option<Connection<RawAssignee>>,
    repository: Option<RawRepository>,
}

#[derive(Debug, Deserialize)]
struct RawReactionGroup {
    content: String,
    users: TotalCount,
}

// ---------------------------------------------------------------------------
// Conversion helpers: Raw → Domain
// ---------------------------------------------------------------------------

fn extract_labels(labels: Option<Connection<RawLabel>>) -> Vec<Label> {
    labels
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .map(|l| Label {
                    name: l.name,
                    color: l.color,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_assignees(assignees: Option<Connection<RawAssignee>>) -> Vec<Actor> {
    assignees
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .map(|a| Actor {
                    login: a.login,
                    avatar_url: String::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_review_requests(requests: Option<Connection<RawReviewRequest>>) -> Vec<Actor> {
    requests
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .filter_map(|rr| {
                    rr.requested_reviewer.and_then(|r| {
                        r.login.map(|login| Actor {
                            login,
                            avatar_url: String::new(),
                        })
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_latest_reviews(reviews: Option<Connection<RawLatestReview>>) -> Vec<Review> {
    reviews
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .map(|r| Review {
                    author: r.author.map(|a| Actor {
                        login: a.login,
                        avatar_url: a.avatar_url,
                    }),
                    state: r.state.unwrap_or(ReviewState::Unknown),
                    body: String::new(),
                    submitted_at: None,
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Convert a single `RawCheckContext` into a domain `CheckRun`.
fn convert_check_context(ctx: RawCheckContext) -> CheckRun {
    let name = ctx
        .name
        .or(ctx.context)
        .unwrap_or_else(|| "<unknown>".to_owned());
    let url = ctx.details_url.or(ctx.target_url);

    // StatusContext uses "state" (string) instead of typed status/conclusion.
    let (status, conclusion) = if ctx.status.is_some() {
        (ctx.status, ctx.conclusion)
    } else if let Some(ref state) = ctx.state {
        match state.as_str() {
            "success" => (Some(CheckStatus::Completed), Some(CheckConclusion::Success)),
            "failure" | "error" => (Some(CheckStatus::Completed), Some(CheckConclusion::Failure)),
            "pending" => (Some(CheckStatus::InProgress), None),
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    let (workflow_run_id, workflow_name) = ctx
        .check_suite
        .and_then(|cs| cs.workflow_run)
        .map_or((None, None), |wr| {
            (wr.database_id, wr.workflow.and_then(|w| w.name))
        });

    CheckRun {
        name,
        status,
        conclusion,
        url,
        workflow_run_id,
        workflow_name,
        started_at: ctx.started_at,
        completed_at: ctx.completed_at,
    }
}

/// Extract check runs from the commits connection (last-commit rollup).
fn extract_check_runs(commits: Option<Connection<RawCommitNode>>) -> Vec<CheckRun> {
    commits
        .and_then(|c| c.nodes.into_iter().flatten().next())
        .and_then(|cn| cn.commit)
        .and_then(|c| c.status_check_rollup)
        .and_then(|sr| sr.contexts)
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .map(convert_check_context)
                .collect()
        })
        .unwrap_or_default()
}

fn extract_participants(participants: Option<Connection<RawAssignee>>) -> Vec<String> {
    participants
        .map(|c| c.nodes.into_iter().flatten().map(|a| a.login).collect())
        .unwrap_or_default()
}

fn extract_timeline_events(
    timeline_items: Option<Connection<RawTimelineItem>>,
) -> Vec<TimelineEvent> {
    timeline_items
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .filter_map(convert_timeline_item)
                .collect()
        })
        .unwrap_or_default()
}

fn extract_detail_commits(all_commits: Option<Connection<RawDetailCommitNode>>) -> Vec<Commit> {
    all_commits
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .filter_map(|cn| {
                    let c = cn.commit?;
                    Some(Commit {
                        sha: c.oid,
                        message: c.message_headline,
                        author: c.author.and_then(|a| a.name),
                        committed_date: c.committed_date,
                        check_state: c.status_check_rollup.and_then(|r| r.state),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_files(files: Option<Connection<RawFile>>) -> Vec<File> {
    files
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .map(|f| File {
                    path: f.path,
                    additions: f.additions,
                    deletions: f.deletions,
                    status: f.change_type,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_review_threads(
    review_threads: Option<Connection<RawReviewThread>>,
) -> Vec<ReviewThread> {
    review_threads
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .map(|rt| ReviewThread {
                    is_resolved: rt.is_resolved,
                    comments: rt
                        .comments
                        .map(|cc| {
                            cc.nodes
                                .into_iter()
                                .flatten()
                                .map(|rc| crate::github::types::Comment {
                                    author: rc.author.map(raw_actor_to_actor),
                                    body: rc.body,
                                    created_at: rc.created_at,
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn extract_detail_reviews(reviews: Option<Connection<RawReview>>) -> Vec<Review> {
    reviews
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .map(|r| Review {
                    author: r.author.map(raw_actor_to_actor),
                    state: r.state,
                    body: r.body,
                    submitted_at: r.submitted_at,
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Conversion: Raw → Domain
// ---------------------------------------------------------------------------

impl RawPullRequest {
    fn into_domain(self) -> PullRequest {
        let author = self.author.map(|a| Actor {
            login: a.login,
            avatar_url: a.avatar_url,
        });

        let repo = self
            .repository
            .and_then(|r| RepoRef::from_full_name(&r.name_with_owner));

        PullRequest {
            number: self.number,
            title: self.title,
            body: self.body,
            author,
            state: self.state,
            is_draft: self.is_draft,
            mergeable: self.mergeable,
            review_decision: self.review_decision,
            additions: self.additions,
            deletions: self.deletions,
            head_ref: self.head_ref_name,
            base_ref: self.base_ref_name,
            labels: extract_labels(self.labels),
            assignees: extract_assignees(self.assignees),
            commits: Vec::new(),
            comments: Vec::new(),
            review_threads: Vec::new(),
            review_requests: extract_review_requests(self.review_requests),
            reviews: extract_latest_reviews(self.latest_reviews),
            timeline_events: Vec::new(),
            files: Vec::new(),
            check_runs: extract_check_runs(self.commits),
            updated_at: self.updated_at,
            created_at: self.created_at,
            url: self.url,
            repo,
            comment_count: self.comments.map_or(0, |c| c.total_count),
            author_association: self.author_association,
            participants: extract_participants(self.participants),
            merge_state_status: self.merge_state_status,
            head_repo_owner: self.head_repository.as_ref().map(|r| r.owner.login.clone()),
            head_repo_name: self.head_repository.map(|r| r.name),
        }
    }
}

impl RawIssue {
    fn into_domain(self) -> Issue {
        let author = self.author.map(|a| Actor {
            login: a.login,
            avatar_url: a.avatar_url,
        });

        let labels = self
            .labels
            .map(|c| {
                c.nodes
                    .into_iter()
                    .flatten()
                    .map(|l| Label {
                        name: l.name,
                        color: l.color,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let assignees = self
            .assignees
            .map(|c| {
                c.nodes
                    .into_iter()
                    .flatten()
                    .map(|a| Actor {
                        login: a.login,
                        avatar_url: String::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let comment_count = self.comments.map_or(0, |c| c.total_count);

        let reactions = parse_reaction_groups(&self.reaction_groups);

        let repo = self
            .repository
            .and_then(|r| RepoRef::from_full_name(&r.name_with_owner));

        Issue {
            number: self.number,
            title: self.title,
            body: self.body,
            author,
            state: self.state,
            assignees,
            comments: Vec::new(),
            reactions,
            labels,
            updated_at: self.updated_at,
            created_at: self.created_at,
            url: self.url,
            repo,
            comment_count,
            participants: self
                .participants
                .map(|c| c.nodes.into_iter().flatten().map(|a| a.login).collect())
                .unwrap_or_default(),
        }
    }
}

fn parse_reaction_groups(groups: &[RawReactionGroup]) -> ReactionGroups {
    let mut r = ReactionGroups::default();
    for g in groups {
        let count = g.users.total_count;
        match g.content.as_str() {
            "THUMBS_UP" => r.thumbs_up = count,
            "THUMBS_DOWN" => r.thumbs_down = count,
            "LAUGH" => r.laugh = count,
            "HOORAY" => r.hooray = count,
            "CONFUSED" => r.confused = count,
            "HEART" => r.heart = count,
            "ROCKET" => r.rocket = count,
            "EYES" => r.eyes = count,
            _ => {}
        }
    }
    r
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Result of a single page of PR search results.
pub(crate) struct SearchPrPage {
    pub pull_requests: Vec<PullRequest>,
    pub page_info: PageInfo,
    pub rate_limit: Option<RateLimitInfo>,
}

/// Execute the `SearchPullRequests` GraphQL query for a single page.
///
/// Automatically prepends `is:pr` to the query if not already present, so that
/// the search only returns pull requests (not issues).
pub async fn search_pull_requests(
    octocrab: &Arc<Octocrab>,
    query: &str,
    limit: u32,
    after: Option<String>,
) -> Result<SearchPrPage> {
    let effective_query = ensure_type_qualifier(query, "pr");
    let payload = GraphQLPayload {
        query: SEARCH_PULL_REQUESTS_QUERY,
        variables: SearchVariables {
            query: effective_query,
            first: limit,
            after,
        },
    };

    let response: GraphQLResponse<SearchData> = octocrab
        .graphql(&payload)
        .await
        .with_context(|| format!("GraphQL PR search failed for query: {query}"))?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    let rate_limit = data.rate_limit;

    let pull_requests = data
        .search
        .nodes
        .into_iter()
        .flatten()
        .map(RawPullRequest::into_domain)
        .collect();

    Ok(SearchPrPage {
        pull_requests,
        page_info: data.search.page_info,
        rate_limit,
    })
}

/// Fetch all pages of PR search results up to the given limit.
///
/// When a `cache` is provided, results are served from the moka LRU cache
/// if a fresh entry exists (TTL is set at client creation time).
///
/// Returns `(pull_requests, rate_limit)`. On cache hit, `rate_limit` is `None`.
pub async fn search_pull_requests_all(
    octocrab: &Arc<Octocrab>,
    query: &str,
    limit: u32,
    cache: Option<&Cache<String, String>>,
) -> Result<(Vec<PullRequest>, Option<RateLimitInfo>)> {
    let cache_key = format!("prs:{query}:{limit}");

    // Try cache first.
    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok(prs) = serde_json::from_str::<Vec<PullRequest>>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((prs, None));
    }

    let page_size = limit.min(100); // GitHub caps at 100 per page
    let mut all_prs = Vec::new();
    let mut cursor: Option<String> = None;
    let mut last_rate_limit: Option<RateLimitInfo> = None;

    loop {
        let remaining = limit.saturating_sub(u32::try_from(all_prs.len()).unwrap_or(u32::MAX));
        if remaining == 0 {
            break;
        }
        let fetch_count = remaining.min(page_size);

        let page = search_pull_requests(octocrab, query, fetch_count, cursor).await?;
        all_prs.extend(page.pull_requests);
        if page.rate_limit.is_some() {
            last_rate_limit = page.rate_limit;
        }

        if !page.page_info.has_next_page || page.page_info.end_cursor.is_none() {
            break;
        }
        cursor = page.page_info.end_cursor;
    }

    // Store in cache.
    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&all_prs)
    {
        c.insert(cache_key, json).await;
    }

    Ok((all_prs, last_rate_limit))
}

// ---------------------------------------------------------------------------
// Issue search API
// ---------------------------------------------------------------------------

/// Result of a single page of Issue search results.
pub(crate) struct SearchIssuePage {
    pub issues: Vec<Issue>,
    pub page_info: PageInfo,
    pub rate_limit: Option<RateLimitInfo>,
}

/// Execute the `SearchIssues` GraphQL query for a single page.
///
/// Automatically prepends `is:issue` to the query if not already present, so
/// that the search only returns issues (not pull requests).
pub async fn search_issues(
    octocrab: &Arc<Octocrab>,
    query: &str,
    limit: u32,
    after: Option<String>,
) -> Result<SearchIssuePage> {
    let effective_query = ensure_type_qualifier(query, "issue");
    let payload = GraphQLPayload {
        query: SEARCH_ISSUES_QUERY,
        variables: SearchVariables {
            query: effective_query,
            first: limit,
            after,
        },
    };

    let response: GraphQLResponse<IssueSearchData> = octocrab
        .graphql(&payload)
        .await
        .context("GraphQL request failed")?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    let rate_limit = data.rate_limit;

    let issues = data
        .search
        .nodes
        .into_iter()
        .flatten()
        .map(RawIssue::into_domain)
        .collect();

    Ok(SearchIssuePage {
        issues,
        page_info: data.search.page_info,
        rate_limit,
    })
}

/// Fetch all pages of Issue search results up to the given limit.
///
/// When a `cache` is provided, results are served from the moka LRU cache
/// if a fresh entry exists.
///
/// Returns `(issues, rate_limit)`. On cache hit, `rate_limit` is `None`.
pub async fn search_issues_all(
    octocrab: &Arc<Octocrab>,
    query: &str,
    limit: u32,
    cache: Option<&Cache<String, String>>,
) -> Result<(Vec<Issue>, Option<RateLimitInfo>)> {
    let cache_key = format!("issues:{query}:{limit}");

    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok(issues) = serde_json::from_str::<Vec<Issue>>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((issues, None));
    }

    let page_size = limit.min(100);
    let mut all_issues = Vec::new();
    let mut cursor: Option<String> = None;
    let mut last_rate_limit: Option<RateLimitInfo> = None;

    loop {
        let remaining = limit.saturating_sub(u32::try_from(all_issues.len()).unwrap_or(u32::MAX));
        if remaining == 0 {
            break;
        }
        let fetch_count = remaining.min(page_size);

        let page = search_issues(octocrab, query, fetch_count, cursor).await?;
        all_issues.extend(page.issues);
        if page.rate_limit.is_some() {
            last_rate_limit = page.rate_limit;
        }

        if !page.page_info.has_next_page || page.page_info.end_cursor.is_none() {
            break;
        }
        cursor = page.page_info.end_cursor;
    }

    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&all_issues)
    {
        c.insert(cache_key, json).await;
    }

    Ok((all_issues, last_rate_limit))
}

// ---------------------------------------------------------------------------
// PR detail API (Q2 — sidebar tabs)
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Issue detail response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct IssueDetailData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    repository: Option<IssueDetailRepo>,
}

#[derive(Debug, Deserialize)]
struct IssueDetailRepo {
    issue: Option<RawIssueDetail>,
}

#[derive(Debug, Deserialize)]
struct RawIssueDetail {
    #[serde(default)]
    body: String,
    #[serde(rename = "timelineItems")]
    timeline_items: Option<Connection<RawTimelineItem>>,
}

fn raw_actor_login(a: Option<RawActor>) -> Option<String> {
    a.map(|a| a.login)
}

fn raw_actor_to_actor(a: RawActor) -> Actor {
    Actor {
        login: a.login,
        avatar_url: String::new(),
    }
}

fn convert_timeline_item(item: RawTimelineItem) -> Option<TimelineEvent> {
    match item.typename.as_str() {
        "IssueComment" => Some(TimelineEvent::Comment {
            author: raw_actor_login(item.author),
            body: item.body.unwrap_or_default(),
            created_at: item.created_at?,
        }),
        "PullRequestReview" => Some(TimelineEvent::Review {
            author: raw_actor_login(item.author),
            state: item.state.unwrap_or(ReviewState::Unknown),
            body: item.body.unwrap_or_default(),
            submitted_at: item.submitted_at.or(item.created_at)?,
        }),
        "MergedEvent" => Some(TimelineEvent::Merged {
            actor: raw_actor_login(item.actor),
            created_at: item.created_at?,
        }),
        "ClosedEvent" => Some(TimelineEvent::Closed {
            actor: raw_actor_login(item.actor),
            created_at: item.created_at?,
        }),
        "ReopenedEvent" => Some(TimelineEvent::Reopened {
            actor: raw_actor_login(item.actor),
            created_at: item.created_at?,
        }),
        "HeadRefForcePushedEvent" => Some(TimelineEvent::ForcePushed {
            actor: raw_actor_login(item.actor),
            created_at: item.created_at?,
        }),
        _ => None,
    }
}

impl RawPrDetail {
    fn into_domain(self) -> PrDetail {
        PrDetail {
            body: self.body,
            reviews: extract_detail_reviews(self.reviews),
            review_threads: extract_review_threads(self.review_threads),
            timeline_events: extract_timeline_events(self.timeline_items),
            commits: extract_detail_commits(self.commits),
            files: extract_files(self.files),
            mergeable: self.mergeable,
            behind_by: None, // Populated by fetch_compare after the GraphQL call.
        }
    }
}

/// Fetch detailed PR data for sidebar tabs.
///
/// When a `cache` is provided, results are served from the moka LRU cache
/// if a fresh entry exists.
///
/// Returns `(detail, rate_limit)`. On cache hit, `rate_limit` is `None`.
pub async fn fetch_pr_detail(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    cache: Option<&Cache<String, String>>,
) -> Result<(PrDetail, Option<RateLimitInfo>)> {
    let cache_key = format!("pr:{owner}/{repo}#{number}");

    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok(detail) = serde_json::from_str::<PrDetail>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((detail, None));
    }

    let payload = GraphQLPayload {
        query: PR_DETAIL_QUERY,
        variables: PrDetailVariables {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number: i64::try_from(number).context("PR number too large")?,
        },
    };

    let response: GraphQLResponse<PrDetailData> = octocrab
        .graphql(&payload)
        .await
        .context("GraphQL PR detail request failed")?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    let rate_limit = data.rate_limit;

    let raw = data
        .repository
        .and_then(|r| r.pull_request)
        .context("PR not found")?;

    let detail = raw.into_domain();

    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&detail)
    {
        c.insert(cache_key, json).await;
    }

    Ok((detail, rate_limit))
}

// ---------------------------------------------------------------------------
// Issue detail API (sidebar tabs)
// ---------------------------------------------------------------------------

/// Fetch detailed Issue data for sidebar tabs.
///
/// When a `cache` is provided, results are served from the moka LRU cache
/// if a fresh entry exists.
///
/// Returns `(detail, rate_limit)`. On cache hit, `rate_limit` is `None`.
pub async fn fetch_issue_detail(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    cache: Option<&Cache<String, String>>,
) -> Result<(IssueDetail, Option<RateLimitInfo>)> {
    let cache_key = format!("issue:{owner}/{repo}#{number}");

    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok(detail) = serde_json::from_str::<IssueDetail>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((detail, None));
    }

    let payload = GraphQLPayload {
        query: ISSUE_DETAIL_QUERY,
        variables: PrDetailVariables {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number: i64::try_from(number).context("Issue number too large")?,
        },
    };

    let response: GraphQLResponse<IssueDetailData> = octocrab
        .graphql(&payload)
        .await
        .context("GraphQL issue detail request failed")?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    let rate_limit = data.rate_limit;

    let raw = data
        .repository
        .and_then(|r| r.issue)
        .context("Issue not found")?;

    let timeline_events = raw
        .timeline_items
        .map(|c| {
            c.nodes
                .into_iter()
                .flatten()
                .filter_map(convert_timeline_item)
                .collect()
        })
        .unwrap_or_default();

    let detail = IssueDetail {
        body: raw.body,
        timeline_events,
    };

    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&detail)
    {
        c.insert(cache_key, json).await;
    }

    Ok((detail, rate_limit))
}

// ---------------------------------------------------------------------------
// Compare API (REST — branch update status)
// ---------------------------------------------------------------------------

/// Fetch how many commits behind `base_ref` the head branch is.
///
/// Uses `GET /repos/{base_owner}/{base_repo}/compare/{base_ref}...{head_ref}` for same-repo
/// PRs, or `...{head_owner}:{head_ref}` for cross-fork PRs.
/// Returns `Some(n)` where `n` is the number of commits the head is behind base,
/// or `None` if the request fails or the field is absent.
pub async fn fetch_compare(
    octocrab: &Arc<Octocrab>,
    base_owner: &str,
    base_repo: &str,
    base_ref: &str,
    head_owner: &str,
    head_ref: &str,
) -> Result<Option<u32>> {
    let route = if head_owner == base_owner {
        format!("/repos/{base_owner}/{base_repo}/compare/{base_ref}...{head_ref}")
    } else {
        format!("/repos/{base_owner}/{base_repo}/compare/{base_ref}...{head_owner}:{head_ref}")
    };
    let head_spec = if head_owner == base_owner {
        head_ref.to_owned()
    } else {
        format!("{head_owner}:{head_ref}")
    };
    let response: serde_json::Value =
        octocrab.get(route, None::<&()>).await.with_context(|| {
            format!("compare request failed for {base_owner}/{base_repo}: {base_ref}...{head_spec}")
        })?;
    let behind_by = response["behind_by"]
        .as_u64()
        .and_then(|n| u32::try_from(n).ok());
    Ok(behind_by)
}

// ---------------------------------------------------------------------------
// Q4: Repository Labels (T083)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct RepoLabelsVariables {
    owner: String,
    repo: String,
    first: u32,
}

#[derive(Debug, Deserialize)]
struct RepoLabelsData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    repository: Option<RepoLabelsRepo>,
}

#[derive(Debug, Deserialize)]
struct RepoLabelsRepo {
    labels: Option<RepoLabelsConnection>,
}

#[derive(Debug, Deserialize)]
struct RepoLabelsConnection {
    nodes: Option<Vec<RawRepoLabel>>,
}

#[derive(Debug, Deserialize)]
struct RawRepoLabel {
    name: String,
    color: String,
    description: Option<String>,
}

/// A repository label with name, color, and optional description.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RepoLabel {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Serialize)]
struct RepoCollaboratorsVariables {
    owner: String,
    repo: String,
    first: u32,
}

#[derive(Debug, Deserialize)]
struct RepoCollaboratorsData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    repository: Option<RepoCollaboratorsRepo>,
}

#[derive(Debug, Deserialize)]
struct RepoCollaboratorsRepo {
    collaborators: Option<RepoCollaboratorsConnection>,
}

#[derive(Debug, Deserialize)]
struct RepoCollaboratorsConnection {
    nodes: Option<Vec<RawCollaborator>>,
}

#[derive(Debug, Deserialize)]
struct RawCollaborator {
    login: String,
}

/// Fetch all labels for a repository (for autocomplete).
///
/// When a `cache` is provided, results are served from the moka LRU cache
/// if a fresh entry exists.
///
/// Returns `(labels, rate_limit)`. On cache hit, `rate_limit` is `None`.
pub async fn fetch_repo_labels(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    cache: Option<&Cache<String, String>>,
) -> Result<(Vec<RepoLabel>, Option<RateLimitInfo>)> {
    let cache_key = format!("labels:{owner}/{repo}");

    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok(labels) = serde_json::from_str::<Vec<RepoLabel>>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((labels, None));
    }

    let payload = GraphQLPayload {
        query: REPOSITORY_LABELS_QUERY,
        variables: RepoLabelsVariables {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            first: 100,
        },
    };

    let response: GraphQLResponse<RepoLabelsData> = octocrab
        .graphql(&payload)
        .await
        .context("GraphQL repo labels request failed")?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    let rate_limit = data.rate_limit;

    let labels: Vec<RepoLabel> = data
        .repository
        .and_then(|r| r.labels)
        .and_then(|l| l.nodes)
        .unwrap_or_default()
        .into_iter()
        .map(|l| RepoLabel {
            name: l.name,
            color: l.color,
            description: l.description,
        })
        .collect();

    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&labels)
    {
        c.insert(cache_key, json).await;
    }

    Ok((labels, rate_limit))
}

/// Fetch all collaborators for a repository (for assignee autocomplete).
///
/// When a `cache` is provided, results are served from the moka LRU cache
/// if a fresh entry exists.
///
/// Returns `(logins, rate_limit)`. On cache hit, `rate_limit` is `None`.
pub async fn fetch_repo_collaborators(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    cache: Option<&Cache<String, String>>,
) -> Result<(Vec<String>, Option<RateLimitInfo>)> {
    let cache_key = format!("collaborators:{owner}/{repo}");

    // Check cache first
    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok(logins) = serde_json::from_str::<Vec<String>>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((logins, None));
    }

    // Execute GraphQL query
    let payload = GraphQLPayload {
        query: REPOSITORY_COLLABORATORS_QUERY,
        variables: RepoCollaboratorsVariables {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            first: 100,
        },
    };

    let response: GraphQLResponse<RepoCollaboratorsData> = octocrab
        .graphql(&payload)
        .await
        .context("GraphQL repo collaborators request failed")?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    // Extract rate limit
    let rate_limit = data.rate_limit;

    // Parse collaborators
    let logins: Vec<String> = data
        .repository
        .and_then(|r| r.collaborators)
        .and_then(|c| c.nodes)
        .unwrap_or_default()
        .into_iter()
        .map(|collab| collab.login)
        .collect();

    // Cache the result
    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&logins)
    {
        c.insert(cache_key, json).await;
    }

    Ok((logins, rate_limit))
}

// ---------------------------------------------------------------------------
// Single-item combined queries (RefreshItem)
// ---------------------------------------------------------------------------

/// Combined query that returns all search-row fields AND detail fields for a
/// single PR, so one API call can update both the table row and the sidebar.
const SINGLE_PR_QUERY: &str = r"
query SinglePullRequest($owner: String!, $repo: String!, $number: Int!) {
  rateLimit { limit remaining cost }
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      number
      title
      body
      state
      isDraft
      mergeable
      reviewDecision
      additions
      deletions
      headRefName
      baseRefName
      mergeStateStatus
      headRepository { owner { login } name }
      url
      updatedAt
      createdAt
      author { login avatarUrl }
      authorAssociation
      labels(first: 10) { nodes { name color } }
      assignees(first: 10) { nodes { login } }
      comments { totalCount }
      latestReviews(first: 10) {
        nodes {
          state
          author { login }
        }
      }
      reviewRequests(first: 10) {
        nodes {
          requestedReviewer {
            ... on User { login }
          }
        }
      }
      lastCommit: commits(last: 1) {
        nodes {
          commit {
            statusCheckRollup {
              contexts(first: 50) {
                nodes {
                  ... on CheckRun {
                    name status conclusion detailsUrl startedAt completedAt
                    checkSuite {
                      workflowRun {
                        databaseId
                        workflow { name }
                      }
                    }
                  }
                  ... on StatusContext { context state targetUrl }
                }
              }
            }
          }
        }
      }
      participants(first: 30) { nodes { login } }
      repository { nameWithOwner }
      reviews(last: 50) {
        nodes { author { login } state body submittedAt }
      }
      reviewThreads(first: 50) {
        nodes { isResolved comments(first: 10) { nodes { author { login } body createdAt } } }
      }
      timelineItems(last: 100) {
        nodes {
          __typename
          ... on IssueComment { author { login } body createdAt }
          ... on PullRequestReview { author { login } state body submittedAt }
          ... on MergedEvent { actor { login } createdAt }
          ... on ClosedEvent { actor { login } createdAt }
          ... on ReopenedEvent { actor { login } createdAt }
          ... on HeadRefForcePushedEvent { actor { login } createdAt }
        }
      }
      allCommits: commits(first: 100) {
        nodes { commit { oid messageHeadline author { name } committedDate statusCheckRollup { state } } }
      }
      files(first: 100) {
        nodes { path additions deletions changeType }
      }
    }
  }
}
";

/// Combined query that returns all search-row fields AND detail fields for a
/// single Issue.
const SINGLE_ISSUE_QUERY: &str = r"
query SingleIssue($owner: String!, $repo: String!, $number: Int!) {
  rateLimit { limit remaining cost }
  repository(owner: $owner, name: $repo) {
    issue(number: $number) {
      number
      title
      body
      state
      url
      updatedAt
      createdAt
      author { login avatarUrl }
      assignees(first: 10) { nodes { login } }
      labels(first: 10) { nodes { name color } }
      comments { totalCount }
      reactionGroups { content users { totalCount } }
      participants(first: 30) { nodes { login } }
      repository { nameWithOwner }
      timelineItems(last: 100) {
        nodes {
          __typename
          ... on IssueComment { author { login } body createdAt }
          ... on ClosedEvent { actor { login } createdAt }
          ... on ReopenedEvent { actor { login } createdAt }
        }
      }
    }
  }
}
";

// ---------------------------------------------------------------------------
// Response types for single-item queries
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct SinglePrData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    repository: Option<SinglePrRepo>,
}

#[derive(Debug, Deserialize)]
struct SinglePrRepo {
    #[serde(rename = "pullRequest")]
    pull_request: Option<RawFullPullRequest>,
}

/// Full PR response containing both search-row and detail fields.
#[derive(Debug, Deserialize)]
struct RawFullPullRequest {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    state: PrState,
    #[serde(rename = "isDraft", default)]
    is_draft: bool,
    mergeable: Option<MergeableState>,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<ReviewDecision>,
    #[serde(default)]
    additions: u32,
    #[serde(default)]
    deletions: u32,
    #[serde(rename = "headRefName", default)]
    head_ref_name: String,
    #[serde(rename = "baseRefName", default)]
    base_ref_name: String,
    #[serde(rename = "mergeStateStatus")]
    merge_state_status: Option<MergeStateStatus>,
    #[serde(rename = "headRepository")]
    head_repository: Option<RawHeadRepository>,
    url: String,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<Utc>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
    author: Option<RawActor>,
    #[serde(rename = "authorAssociation")]
    author_association: Option<AuthorAssociation>,
    labels: Option<Connection<RawLabel>>,
    assignees: Option<Connection<RawAssignee>>,
    comments: Option<TotalCount>,
    #[serde(rename = "latestReviews")]
    latest_reviews: Option<Connection<RawLatestReview>>,
    #[serde(rename = "reviewRequests")]
    review_requests: Option<Connection<RawReviewRequest>>,
    /// Aliased: `lastCommit: commits(last: 1)` — for `check_runs` on the search row.
    #[serde(rename = "lastCommit")]
    last_commit: Option<Connection<RawCommitNode>>,
    participants: Option<Connection<RawAssignee>>,
    repository: Option<RawRepository>,
    // Detail fields
    reviews: Option<Connection<RawReview>>,
    #[serde(rename = "reviewThreads")]
    review_threads: Option<Connection<RawReviewThread>>,
    #[serde(rename = "timelineItems")]
    timeline_items: Option<Connection<RawTimelineItem>>,
    /// Aliased: `allCommits: commits(first: 100)` — for detail commits.
    #[serde(rename = "allCommits")]
    all_commits: Option<Connection<RawDetailCommitNode>>,
    files: Option<Connection<RawFile>>,
}

impl RawFullPullRequest {
    /// Split the combined response into a search-row `PullRequest` and a `PrDetail`.
    fn into_domain(self) -> (PullRequest, PrDetail) {
        let author = self.author.map(|a| Actor {
            login: a.login,
            avatar_url: a.avatar_url,
        });

        let repo = self
            .repository
            .and_then(|r| RepoRef::from_full_name(&r.name_with_owner));

        let pr = PullRequest {
            number: self.number,
            title: self.title,
            body: self.body.clone(),
            author,
            state: self.state,
            is_draft: self.is_draft,
            mergeable: self.mergeable,
            review_decision: self.review_decision,
            additions: self.additions,
            deletions: self.deletions,
            head_ref: self.head_ref_name,
            base_ref: self.base_ref_name,
            labels: extract_labels(self.labels),
            assignees: extract_assignees(self.assignees),
            commits: Vec::new(),
            comments: Vec::new(),
            review_threads: Vec::new(),
            review_requests: extract_review_requests(self.review_requests),
            reviews: extract_latest_reviews(self.latest_reviews),
            timeline_events: Vec::new(),
            files: Vec::new(),
            check_runs: extract_check_runs(self.last_commit),
            updated_at: self.updated_at,
            created_at: self.created_at,
            url: self.url,
            repo,
            comment_count: self.comments.map_or(0, |c| c.total_count),
            author_association: self.author_association,
            participants: extract_participants(self.participants),
            merge_state_status: self.merge_state_status,
            head_repo_owner: self.head_repository.as_ref().map(|r| r.owner.login.clone()),
            head_repo_name: self.head_repository.map(|r| r.name),
        };

        let detail = PrDetail {
            body: self.body,
            reviews: extract_detail_reviews(self.reviews),
            review_threads: extract_review_threads(self.review_threads),
            timeline_events: extract_timeline_events(self.timeline_items),
            commits: extract_detail_commits(self.all_commits),
            files: extract_files(self.files),
            mergeable: self.mergeable,
            behind_by: None,
        };

        (pr, detail)
    }
}

// Response types for single issue query

#[derive(Debug, Deserialize)]
struct SingleIssueData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RateLimitInfo>,
    repository: Option<SingleIssueRepo>,
}

#[derive(Debug, Deserialize)]
struct SingleIssueRepo {
    issue: Option<RawFullIssue>,
}

/// Full Issue response containing both search-row and detail fields.
#[derive(Debug, Deserialize)]
struct RawFullIssue {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    state: IssueState,
    url: String,
    #[serde(rename = "updatedAt")]
    updated_at: DateTime<Utc>,
    #[serde(rename = "createdAt")]
    created_at: DateTime<Utc>,
    author: Option<RawActor>,
    assignees: Option<Connection<RawAssignee>>,
    labels: Option<Connection<RawLabel>>,
    comments: Option<TotalCount>,
    #[serde(rename = "reactionGroups", default)]
    reaction_groups: Vec<RawReactionGroup>,
    participants: Option<Connection<RawAssignee>>,
    repository: Option<RawRepository>,
    #[serde(rename = "timelineItems")]
    timeline_items: Option<Connection<RawTimelineItem>>,
}

impl RawFullIssue {
    fn into_domain(self) -> (Issue, IssueDetail) {
        let author = self.author.map(|a| Actor {
            login: a.login,
            avatar_url: a.avatar_url,
        });

        let labels = self
            .labels
            .map(|c| {
                c.nodes
                    .into_iter()
                    .flatten()
                    .map(|l| Label {
                        name: l.name,
                        color: l.color,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let assignees = self
            .assignees
            .map(|c| {
                c.nodes
                    .into_iter()
                    .flatten()
                    .map(|a| Actor {
                        login: a.login,
                        avatar_url: String::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let comment_count = self.comments.map_or(0, |c| c.total_count);
        let reactions = parse_reaction_groups(&self.reaction_groups);

        let repo = self
            .repository
            .and_then(|r| RepoRef::from_full_name(&r.name_with_owner));

        let timeline_events = self
            .timeline_items
            .map(|c| {
                c.nodes
                    .into_iter()
                    .flatten()
                    .filter_map(convert_timeline_item)
                    .collect()
            })
            .unwrap_or_default();

        let issue = Issue {
            number: self.number,
            title: self.title,
            body: self.body.clone(),
            author,
            state: self.state,
            assignees,
            comments: Vec::new(),
            reactions,
            labels,
            updated_at: self.updated_at,
            created_at: self.created_at,
            url: self.url,
            repo,
            comment_count,
            participants: self
                .participants
                .map(|c| c.nodes.into_iter().flatten().map(|a| a.login).collect())
                .unwrap_or_default(),
        };

        let detail = IssueDetail {
            body: self.body,
            timeline_events,
        };

        (issue, detail)
    }
}

// ---------------------------------------------------------------------------
// Public API for single-item refresh
// ---------------------------------------------------------------------------

/// Fetch a single PR with combined search-row + detail fields in one query.
///
/// Returns `(pull_request, pr_detail, rate_limit)`.
pub async fn fetch_single_pr(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    cache: Option<&Cache<String, String>>,
) -> Result<(PullRequest, PrDetail, Option<RateLimitInfo>)> {
    let cache_key = format!("full_pr:{owner}/{repo}#{number}");

    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok((pr, detail)) = serde_json::from_str::<(PullRequest, PrDetail)>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((pr, detail, None));
    }

    let payload = GraphQLPayload {
        query: SINGLE_PR_QUERY,
        variables: PrDetailVariables {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number: i64::try_from(number).context("PR number too large")?,
        },
    };

    let response: GraphQLResponse<SinglePrData> = octocrab
        .graphql(&payload)
        .await
        .context("GraphQL single PR request failed")?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    let rate_limit = data.rate_limit;

    let raw = data
        .repository
        .and_then(|r| r.pull_request)
        .context("PR not found")?;

    let (pr, detail) = raw.into_domain();

    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&(&pr, &detail))
    {
        c.insert(cache_key, json).await;
    }

    Ok((pr, detail, rate_limit))
}

/// Fetch a single Issue with combined search-row + detail fields in one query.
///
/// Returns `(issue, issue_detail, rate_limit)`.
pub async fn fetch_single_issue(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    number: u64,
    cache: Option<&Cache<String, String>>,
) -> Result<(Issue, IssueDetail, Option<RateLimitInfo>)> {
    let cache_key = format!("full_issue:{owner}/{repo}#{number}");

    if let Some(c) = cache
        && let Some(cached) = c.get(&cache_key).await
        && let Ok((issue, detail)) = serde_json::from_str::<(Issue, IssueDetail)>(&cached)
    {
        tracing::debug!("cache hit for {cache_key}");
        return Ok((issue, detail, None));
    }

    let payload = GraphQLPayload {
        query: SINGLE_ISSUE_QUERY,
        variables: PrDetailVariables {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number: i64::try_from(number).context("Issue number too large")?,
        },
    };

    let response: GraphQLResponse<SingleIssueData> = octocrab
        .graphql(&payload)
        .await
        .context("GraphQL single issue request failed")?;

    if let Some(errors) = response.errors {
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        bail!("GraphQL errors: {}", messages.join("; "));
    }

    let data = response
        .data
        .context("GraphQL response missing data field")?;

    let rate_limit = data.rate_limit;

    let raw = data
        .repository
        .and_then(|r| r.issue)
        .context("Issue not found")?;

    let (issue, detail) = raw.into_domain();

    if let Some(c) = cache
        && let Ok(json) = serde_json::to_string(&(&issue, &detail))
    {
        c.insert(cache_key, json).await;
    }

    Ok((issue, detail, rate_limit))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- extract_labels ---

    #[test]
    fn extract_labels_none_returns_empty() {
        let result = extract_labels(None);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_labels_some_with_labels() {
        let conn = Connection {
            nodes: vec![
                Some(RawLabel {
                    name: "bug".to_owned(),
                    color: "d73a4a".to_owned(),
                }),
                None, // null nodes are filtered out
                Some(RawLabel {
                    name: "enhancement".to_owned(),
                    color: "a2eeef".to_owned(),
                }),
            ],
        };
        let result = extract_labels(Some(conn));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "bug");
        assert_eq!(result[0].color, "d73a4a");
        assert_eq!(result[1].name, "enhancement");
        assert_eq!(result[1].color, "a2eeef");
    }

    // --- extract_assignees ---

    #[test]
    fn extract_assignees_none_returns_empty() {
        let result = extract_assignees(None);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_assignees_some_with_actors() {
        let conn = Connection {
            nodes: vec![
                Some(RawAssignee {
                    login: "alice".to_owned(),
                }),
                None,
                Some(RawAssignee {
                    login: "bob".to_owned(),
                }),
            ],
        };
        let result = extract_assignees(Some(conn));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].login, "alice");
        assert!(
            result[0].avatar_url.is_empty(),
            "avatar_url should be empty for assignees"
        );
        assert_eq!(result[1].login, "bob");
    }

    // --- extract_check_runs ---

    #[test]
    fn extract_check_runs_none_returns_empty() {
        let result = extract_check_runs(None);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_check_runs_empty_commits_returns_empty() {
        let conn = Connection { nodes: vec![] };
        let result = extract_check_runs(Some(conn));
        assert!(result.is_empty());
    }

    #[test]
    fn extract_check_runs_with_check_run_contexts() {
        let ctx_success = RawCheckContext {
            name: Some("CI".to_owned()),
            status: Some(CheckStatus::Completed),
            conclusion: Some(CheckConclusion::Success),
            details_url: Some("https://example.com/ci".to_owned()),
            started_at: None,
            completed_at: None,
            check_suite: None,
            context: None,
            state: None,
            target_url: None,
        };
        let ctx_failure = RawCheckContext {
            name: Some("Lint".to_owned()),
            status: Some(CheckStatus::Completed),
            conclusion: Some(CheckConclusion::Failure),
            details_url: None,
            started_at: None,
            completed_at: None,
            check_suite: None,
            context: None,
            state: None,
            target_url: None,
        };
        let rollup = RawStatusCheckRollup {
            contexts: Some(Connection {
                nodes: vec![Some(ctx_success), Some(ctx_failure)],
            }),
        };
        let commit = RawCommit {
            status_check_rollup: Some(rollup),
        };
        let commit_node = RawCommitNode {
            commit: Some(commit),
        };
        let conn = Connection {
            nodes: vec![Some(commit_node)],
        };
        let result = extract_check_runs(Some(conn));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "CI");
        assert_eq!(result[0].status, Some(CheckStatus::Completed));
        assert_eq!(result[0].conclusion, Some(CheckConclusion::Success));
        assert_eq!(result[0].url.as_deref(), Some("https://example.com/ci"));
        assert_eq!(result[1].name, "Lint");
        assert_eq!(result[1].conclusion, Some(CheckConclusion::Failure));
    }

    // --- convert_check_context ---

    #[test]
    fn convert_check_context_check_run() {
        let ctx = RawCheckContext {
            name: Some("build".to_owned()),
            status: Some(CheckStatus::InProgress),
            conclusion: None,
            details_url: Some("https://ci.example.com".to_owned()),
            started_at: None,
            completed_at: None,
            check_suite: Some(RawCheckSuite {
                workflow_run: Some(RawCheckSuiteWorkflowRun {
                    database_id: Some(42),
                    workflow: Some(RawCheckSuiteWorkflow {
                        name: Some("CI".to_owned()),
                    }),
                }),
            }),
            context: None,
            state: None,
            target_url: None,
        };
        let cr = convert_check_context(ctx);
        assert_eq!(cr.name, "build");
        assert_eq!(cr.status, Some(CheckStatus::InProgress));
        assert!(cr.conclusion.is_none());
        assert_eq!(cr.url.as_deref(), Some("https://ci.example.com"));
        assert_eq!(cr.workflow_run_id, Some(42));
        assert_eq!(cr.workflow_name.as_deref(), Some("CI"));
    }

    #[test]
    fn convert_check_context_status_context_success() {
        let ctx = RawCheckContext {
            name: None,
            status: None,
            conclusion: None,
            details_url: None,
            started_at: None,
            completed_at: None,
            check_suite: None,
            context: Some("ci/circleci".to_owned()),
            state: Some("success".to_owned()),
            target_url: Some("https://circleci.com/build/123".to_owned()),
        };
        let cr = convert_check_context(ctx);
        assert_eq!(cr.name, "ci/circleci");
        assert_eq!(cr.status, Some(CheckStatus::Completed));
        assert_eq!(cr.conclusion, Some(CheckConclusion::Success));
        assert_eq!(cr.url.as_deref(), Some("https://circleci.com/build/123"));
    }

    #[test]
    fn convert_check_context_status_context_failure() {
        let ctx = RawCheckContext {
            name: None,
            status: None,
            conclusion: None,
            details_url: None,
            started_at: None,
            completed_at: None,
            check_suite: None,
            context: Some("deploy".to_owned()),
            state: Some("failure".to_owned()),
            target_url: None,
        };
        let cr = convert_check_context(ctx);
        assert_eq!(cr.name, "deploy");
        assert_eq!(cr.status, Some(CheckStatus::Completed));
        assert_eq!(cr.conclusion, Some(CheckConclusion::Failure));
    }

    #[test]
    fn convert_check_context_status_context_pending() {
        let ctx = RawCheckContext {
            name: None,
            status: None,
            conclusion: None,
            details_url: None,
            started_at: None,
            completed_at: None,
            check_suite: None,
            context: Some("pending-job".to_owned()),
            state: Some("pending".to_owned()),
            target_url: None,
        };
        let cr = convert_check_context(ctx);
        assert_eq!(cr.name, "pending-job");
        assert_eq!(cr.status, Some(CheckStatus::InProgress));
        assert!(cr.conclusion.is_none());
    }

    #[test]
    fn convert_check_context_no_name_or_context_uses_unknown() {
        let ctx = RawCheckContext {
            name: None,
            status: None,
            conclusion: None,
            details_url: None,
            started_at: None,
            completed_at: None,
            check_suite: None,
            context: None,
            state: None,
            target_url: None,
        };
        let cr = convert_check_context(ctx);
        assert_eq!(cr.name, "<unknown>");
    }

    // --- extract_review_requests ---

    #[test]
    fn extract_review_requests_none_returns_empty() {
        let result = extract_review_requests(None);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_review_requests_with_requests() {
        let conn = Connection {
            nodes: vec![
                Some(RawReviewRequest {
                    requested_reviewer: Some(RawReviewer {
                        login: Some("reviewer1".to_owned()),
                    }),
                }),
                // Reviewer without login (e.g. team) is filtered out
                Some(RawReviewRequest {
                    requested_reviewer: Some(RawReviewer { login: None }),
                }),
                None,
                Some(RawReviewRequest {
                    requested_reviewer: Some(RawReviewer {
                        login: Some("reviewer2".to_owned()),
                    }),
                }),
            ],
        };
        let result = extract_review_requests(Some(conn));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].login, "reviewer1");
        assert_eq!(result[1].login, "reviewer2");
    }

    #[test]
    fn extract_review_requests_no_reviewer_field() {
        let conn = Connection {
            nodes: vec![Some(RawReviewRequest {
                requested_reviewer: None,
            })],
        };
        let result = extract_review_requests(Some(conn));
        assert!(result.is_empty());
    }

    // --- extract_latest_reviews ---

    #[test]
    fn extract_latest_reviews_none_returns_empty() {
        let result = extract_latest_reviews(None);
        assert!(result.is_empty());
    }

    #[test]
    fn extract_latest_reviews_with_reviews() {
        let conn = Connection {
            nodes: vec![
                Some(RawLatestReview {
                    state: Some(ReviewState::Approved),
                    author: Some(RawActor {
                        login: "alice".to_owned(),
                        avatar_url: "https://avatar.example.com/alice".to_owned(),
                    }),
                }),
                Some(RawLatestReview {
                    state: None,
                    author: None,
                }),
            ],
        };
        let result = extract_latest_reviews(Some(conn));
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].state, ReviewState::Approved);
        assert_eq!(result[0].author.as_ref().unwrap().login, "alice");
        assert_eq!(
            result[0].author.as_ref().unwrap().avatar_url,
            "https://avatar.example.com/alice"
        );
        // None state defaults to Unknown
        assert_eq!(result[1].state, ReviewState::Unknown);
        assert!(result[1].author.is_none());
    }

    // --- ensure_type_qualifier ---

    #[test]
    fn ensure_type_qualifier_adds_missing_pr() {
        let q = ensure_type_qualifier("repo:foo/bar", "pr");
        assert!(q.starts_with("is:pr "));
        assert!(q.contains("repo:foo/bar"));
    }

    #[test]
    fn ensure_type_qualifier_does_not_duplicate() {
        let q = ensure_type_qualifier("is:pr repo:foo/bar", "pr");
        assert_eq!(q, "is:pr repo:foo/bar");
    }

    #[test]
    fn ensure_type_qualifier_case_insensitive() {
        let q = ensure_type_qualifier("Is:Pr repo:foo/bar", "pr");
        // Should not add another is:pr
        assert!(!q.starts_with("is:pr is:"));
    }
}
