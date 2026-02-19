use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use moka::future::Cache;
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};

use crate::github::types::{
    Actor, AuthorAssociation, CheckConclusion, CheckRun, CheckStatus, Commit, File, FileChangeType,
    Issue, IssueState, Label, MergeableState, MergeStateStatus, PrState, PullRequest,
    ReactionGroups, RepoRef, Review, ReviewDecision, ReviewState, ReviewThread, TimelineEvent,
};

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
                    ... on CheckRun { name status conclusion detailsUrl }
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
        nodes { commit { oid messageHeadline author { name } committedDate } }
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

#[derive(Debug, Clone, Deserialize)]
struct RawRateLimit {
    limit: u32,
    remaining: u32,
    cost: u32,
}

/// Public rate limit info extracted from GraphQL responses.
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    pub limit: u32,
    pub remaining: u32,
    pub cost: u32,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RawRateLimit>,
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
    rate_limit: Option<RawRateLimit>,
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
pub struct PageInfo {
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
    rate_limit: Option<RawRateLimit>,
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
}

#[derive(Debug, Deserialize)]
struct RawCommitAuthor {
    name: Option<String>,
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
    // StatusContext fields
    context: Option<String>,
    state: Option<String>,
    #[serde(rename = "targetUrl")]
    target_url: Option<String>,
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
// Conversion: Raw → Domain
// ---------------------------------------------------------------------------

impl RawPullRequest {
    #[allow(clippy::too_many_lines)]
    fn into_domain(self) -> PullRequest {
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

        let review_requests = self
            .review_requests
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
            .unwrap_or_default();

        let check_runs = self
            .commits
            .and_then(|c| c.nodes.into_iter().flatten().next())
            .and_then(|cn| cn.commit)
            .and_then(|c| c.status_check_rollup)
            .and_then(|sr| sr.contexts)
            .map(|c| {
                c.nodes
                    .into_iter()
                    .flatten()
                    .map(|ctx| {
                        // Unify CheckRun and StatusContext into our CheckRun type.
                        let name = ctx
                            .name
                            .or(ctx.context)
                            .unwrap_or_else(|| "<unknown>".to_owned());
                        let url = ctx.details_url.or(ctx.target_url);

                        // StatusContext uses "state" (string) instead of typed
                        // status/conclusion. Map common values.
                        let (status, conclusion) = if ctx.status.is_some() {
                            (ctx.status, ctx.conclusion)
                        } else if let Some(ref state) = ctx.state {
                            match state.as_str() {
                                "success" => {
                                    (Some(CheckStatus::Completed), Some(CheckConclusion::Success))
                                }
                                "failure" | "error" => {
                                    (Some(CheckStatus::Completed), Some(CheckConclusion::Failure))
                                }
                                "pending" => (Some(CheckStatus::InProgress), None),
                                _ => (None, None),
                            }
                        } else {
                            (None, None)
                        };

                        CheckRun {
                            name,
                            status,
                            conclusion,
                            url,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

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
            labels,
            assignees,
            commits: Vec::new(),
            comments: Vec::new(),
            review_threads: Vec::new(),
            review_requests,
            reviews: self
                .latest_reviews
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
                .unwrap_or_default(),
            timeline_events: Vec::new(),
            files: Vec::new(),
            check_runs,
            updated_at: self.updated_at,
            created_at: self.created_at,
            url: self.url,
            repo,
            comment_count,
            author_association: self.author_association,
            participants: self
                .participants
                .map(|c| c.nodes.into_iter().flatten().map(|a| a.login).collect())
                .unwrap_or_default(),
            merge_state_status: self.merge_state_status,
            head_repo_owner: self
                .head_repository
                .as_ref()
                .map(|r| r.owner.login.clone()),
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
pub struct SearchPrPage {
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

    let rate_limit = data.rate_limit.map(|rl| RateLimitInfo {
        limit: rl.limit,
        remaining: rl.remaining,
        cost: rl.cost,
    });

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
pub struct SearchIssuePage {
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

    let rate_limit = data.rate_limit.map(|rl| RateLimitInfo {
        limit: rl.limit,
        remaining: rl.remaining,
        cost: rl.cost,
    });

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

/// Detailed PR data fetched for the sidebar tabs.
#[derive(Clone, Serialize, Deserialize)]
pub struct PrDetail {
    pub body: String,
    pub reviews: Vec<Review>,
    pub review_threads: Vec<ReviewThread>,
    pub timeline_events: Vec<TimelineEvent>,
    pub commits: Vec<Commit>,
    pub files: Vec<File>,
    /// Mergeability from the detail query (`mergeable` field).
    pub mergeable: Option<MergeableState>,
    /// How many commits behind base this PR is (from REST compare API).
    pub behind_by: Option<u32>,
}

/// Detailed Issue data fetched for the sidebar tabs.
#[derive(Clone, Serialize, Deserialize)]
pub struct IssueDetail {
    pub body: String,
    pub timeline_events: Vec<TimelineEvent>,
}

// ---------------------------------------------------------------------------
// Issue detail response types
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct IssueDetailData {
    #[serde(rename = "rateLimit", default)]
    rate_limit: Option<RawRateLimit>,
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
        let reviews = self
            .reviews
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
            .unwrap_or_default();

        let review_threads = self
            .review_threads
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
            .unwrap_or_default();

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

        let commits = self
            .commits
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
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        let files = self
            .files
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
            .unwrap_or_default();

        PrDetail {
            body: self.body,
            reviews,
            review_threads,
            timeline_events,
            commits,
            files,
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

    let rate_limit = data.rate_limit.map(|rl| RateLimitInfo {
        limit: rl.limit,
        remaining: rl.remaining,
        cost: rl.cost,
    });

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

    let rate_limit = data.rate_limit.map(|rl| RateLimitInfo {
        limit: rl.limit,
        remaining: rl.remaining,
        cost: rl.cost,
    });

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
/// Uses `GET /repos/{base_owner}/{base_repo}/compare/{base_ref}...{head_owner}:{head_ref}`.
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
    let route = format!(
        "/repos/{base_owner}/{base_repo}/compare/{base_ref}...{head_owner}:{head_ref}"
    );
    let response: serde_json::Value = octocrab.get(route, None::<&()>).await.with_context(|| {
        format!(
            "compare request failed for {base_owner}/{base_repo}: \
             {base_ref}...{head_owner}:{head_ref}"
        )
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
    rate_limit: Option<RawRateLimit>,
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
pub struct RepoLabel {
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
    rate_limit: Option<RawRateLimit>,
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

    let rate_limit = data.rate_limit.map(|rl| RateLimitInfo {
        limit: rl.limit,
        remaining: rl.remaining,
        cost: rl.cost,
    });

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
    let rate_limit = data.rate_limit.map(|rl| RateLimitInfo {
        limit: rl.limit,
        remaining: rl.remaining,
        cost: rl.cost,
    });

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
