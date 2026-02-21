# Filter Language Reference

gh-board supports two distinct filter languages: one for PR/Issue views and
one for the Notifications view. They look similar but are implemented
differently, so it's worth understanding each one.

---

## PR & Issue Filters

Filters for PR and Issue views are forwarded **verbatim** to the GitHub
GraphQL search API. The app automatically prepends `is:pr` or `is:issue`, so
you only need to write the qualifiers that describe what you want.

Because the full GitHub search syntax applies, any qualifier that works on
`github.com` works here. The full reference is at:
<https://docs.github.com/en/search-github/searching-on-github/searching-issues-and-pull-requests>

### Common qualifiers

| Qualifier | Example | Effect |
|---|---|---|
| `author:` | `author:@me` | PRs/issues you opened |
| `assignee:` | `assignee:@me` | Assigned to you |
| `review-requested:` | `review-requested:@me` | Review requested from you |
| `involves:` | `involves:@me` | You opened, commented, were assigned, or review-requested |
| `is:open` / `is:closed` / `is:merged` | `is:open` | Limit by state |
| `label:` | `label:bug` | Has a specific label |
| `milestone:` | `milestone:"v1.0"` | Belongs to a milestone |
| `repo:` | `repo:owner/name` | Restrict to one repository |
| `sort:` | `sort:updated-desc` | Sort order |

### Config example

```toml
[[pr_filters]]
title = "My PRs"
filters = "author:@me is:open"

[[pr_filters]]
title = "Needs Review"
filters = "review-requested:@me is:open sort:updated-desc"

[[issues_filters]]
title = "Assigned"
filters = "assignee:@me is:open label:bug"
```

---

## Notification Filters

Notification filters are **emulated**: the app maps a small custom language
onto the GitHub REST notifications API (`GET /notifications`), which exposes
only an `unread` boolean per notification. The GitHub web UI's concept of
"Done" (archived) is **not** surfaced by the REST API and cannot be filtered
accurately — `is:read` in this app means "not unread", which includes both
read and done in GitHub's sense.

### Status qualifiers

| Qualifier | Meaning | API behaviour |
|---|---|---|
| *(none)* | Unread only **(default)** | `all=false` |
| `is:unread` | Unread only (explicit) | `all=false` |
| `is:read` | Not-unread (read + done) | `all=true`, then filtered client-side |
| `is:all` | Everything | `all=true` |
| `-is:unread` | Same as `is:read` | `all=true`, then filtered client-side |
| `-is:read` | Same as `is:unread` | `all=false` |

> **Note:** The default is unread-only, matching the GitHub web UI inbox.
> A bare filter like `reason:subscribed` fetches only unread notifications.
> Use `is:all` or `is:read` to widen the scope.

### Reason qualifiers

Filter by the reason GitHub sent the notification:

| Qualifier | Example |
|---|---|
| `reason:<value>` | `reason:subscribed` |
| `-reason:<value>` | `-reason:subscribed` |

Valid reason values:

- `subscribed` — you are watching the thread/repository
- `mention` — you were @mentioned
- `review_requested` — your review was requested
- `author` — you created the issue/PR
- `comment` — you commented
- `assign` — you were assigned
- `state_change` — the state changed (e.g. closed, merged)
- `ci_activity` — a CI run completed on your push
- `team_mention` — a team you belong to was @mentioned
- `security_alert` — a Dependabot security alert

### Repository qualifier

```
repo:owner/name
```

Restricts to a single repository (client-side filter).

### Config examples

```toml
[[notifications_filters]]
title = "Inbox"
filters = ""                          # unread only (default)

[[notifications_filters]]
title = "Subscribed"
filters = "reason:subscribed"         # unread subscribed (default is unread)

[[notifications_filters]]
title = "All Subscribed"
filters = "reason:subscribed is:all"  # includes read + done

[[notifications_filters]]
title = "Read"
filters = "is:read"                   # only not-unread (read + done)

[[notifications_filters]]
title = "My Repo"
filters = "repo:owner/my-repo"        # unread from one repo
```

### Search bar (in-app filter)

While viewing notifications you can open the search bar and type the same
qualifier language to narrow the currently loaded list client-side. The same
prefixes apply (`is:`, `-is:`, `reason:`, `repo:`). Free text (without a
prefix) matches against the notification title, reason, and repository name.
