use crate::color::{Color as AppColor, ColorDepth};
use crate::github::graphql::PrDetail;
use crate::github::types::{
    CheckConclusion, CheckStatus, FileChangeType, PullRequest, ReviewState, TimelineEvent,
};
use crate::markdown::renderer::{StyledLine, StyledSpan};
use crate::theme::ResolvedTheme;

// ---------------------------------------------------------------------------
// T073: Overview tab
// ---------------------------------------------------------------------------

/// Render the Overview tab: metadata + PR body (body rendered elsewhere as markdown).
///
/// Author, State, and Branch are now shown in the `SidebarMeta` pill header,
/// so this function only renders Labels, Assignees, Lines, and a separator.
pub fn render_overview_metadata(pr: &PullRequest, theme: &ResolvedTheme) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    // Labels
    if !pr.labels.is_empty() {
        let label_text = pr
            .labels
            .iter()
            .map(|l| l.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("Labels: ", theme.text_secondary),
            StyledSpan::text(label_text, theme.text_primary),
        ]));
    }

    // Assignees
    if !pr.assignees.is_empty() {
        let assignee_text = pr
            .assignees
            .iter()
            .map(|a| a.login.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("Assign: ", theme.text_secondary),
            StyledSpan::text(assignee_text, theme.text_actor),
        ]));
    }

    // Changes
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold("Lines:  ", theme.text_secondary),
        StyledSpan::text(format!("+{}", pr.additions), theme.text_success),
        StyledSpan::text(" / ", theme.text_faint),
        StyledSpan::text(format!("-{}", pr.deletions), theme.text_error),
    ]));

    // Separator before body
    lines.push(StyledLine::blank());

    lines
}

// ---------------------------------------------------------------------------
// T074: Activity tab
// ---------------------------------------------------------------------------

/// Render the Activity tab: chronological timeline events.
pub fn render_activity(
    detail: &PrDetail,
    theme: &ResolvedTheme,
    depth: ColorDepth,
) -> Vec<StyledLine> {
    if detail.timeline_events.is_empty() {
        return vec![StyledLine::from_span(StyledSpan::text(
            "(no timeline events)",
            theme.text_faint,
        ))];
    }

    let mut lines = Vec::new();
    for event in &detail.timeline_events {
        render_timeline_event(event, theme, depth, &mut lines);
    }
    lines
}

fn render_timeline_event(
    event: &TimelineEvent,
    theme: &ResolvedTheme,
    depth: ColorDepth,
    lines: &mut Vec<StyledLine>,
) {
    match event {
        TimelineEvent::Comment {
            author,
            body,
            created_at,
        } => {
            push_event_header(
                lines,
                author.as_deref(),
                "commented",
                theme.text_secondary,
                created_at,
                theme,
            );
            push_body_markdown(lines, body, theme, depth);
        }
        TimelineEvent::Review {
            author,
            state,
            body,
            submitted_at,
        } => {
            let action = match state {
                ReviewState::Approved => "approved",
                ReviewState::ChangesRequested => "requested changes",
                ReviewState::Dismissed => "dismissed review",
                ReviewState::Commented | ReviewState::Pending | ReviewState::Unknown => "reviewed",
            };
            push_event_header(
                lines,
                author.as_deref(),
                action,
                theme.text_secondary,
                submitted_at,
                theme,
            );
            push_body_markdown(lines, body, theme, depth);
        }
        TimelineEvent::Merged { actor, created_at } => {
            push_event_header(
                lines,
                actor.as_deref(),
                "merged",
                theme.text_success,
                created_at,
                theme,
            );
            lines.push(StyledLine::blank());
        }
        TimelineEvent::Closed { actor, created_at } => {
            push_event_header(
                lines,
                actor.as_deref(),
                "closed",
                theme.text_error,
                created_at,
                theme,
            );
            lines.push(StyledLine::blank());
        }
        TimelineEvent::Reopened { actor, created_at } => {
            push_event_header(
                lines,
                actor.as_deref(),
                "reopened",
                theme.text_success,
                created_at,
                theme,
            );
            lines.push(StyledLine::blank());
        }
        TimelineEvent::ForcePushed { actor, created_at } => {
            push_event_header(
                lines,
                actor.as_deref(),
                "force-pushed",
                theme.text_warning,
                created_at,
                theme,
            );
            lines.push(StyledLine::blank());
        }
    }
}

fn push_event_header(
    lines: &mut Vec<StyledLine>,
    who: Option<&str>,
    action: &str,
    action_color: AppColor,
    when: &chrono::DateTime<chrono::Utc>,
    theme: &ResolvedTheme,
) {
    let who = who.unwrap_or("unknown");
    let when = crate::util::format_date(when, "relative");
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold(format!("{who} "), theme.text_actor),
        StyledSpan::text(format!("{action} "), action_color),
        StyledSpan::text(when, theme.text_faint),
    ]));
}

fn push_body_markdown(
    lines: &mut Vec<StyledLine>,
    body: &str,
    theme: &ResolvedTheme,
    depth: ColorDepth,
) {
    if body.trim().is_empty() {
        lines.push(StyledLine::blank());
        return;
    }
    let rendered = crate::markdown::renderer::render_markdown(body, theme, depth);
    for mut line in rendered {
        // Indent each line by 2 spaces to nest under the event header
        if line.spans.is_empty() {
            // Blank lines from the markdown renderer — keep as separator
            line.spans.push(StyledSpan::text("  ", theme.md_text));
        } else {
            line.spans[0].text = format!("  {}", line.spans[0].text);
        }
        lines.push(line);
    }
    lines.push(StyledLine::blank());
}

// ---------------------------------------------------------------------------
// T075: Commits tab
// ---------------------------------------------------------------------------

/// Render the Commits tab: list of commits with sha, message, author, date.
pub fn render_commits(detail: &PrDetail, theme: &ResolvedTheme) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    if detail.commits.is_empty() {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "(no commits)",
            theme.text_faint,
        )));
        return lines;
    }

    for commit in &detail.commits {
        let short_sha = if commit.sha.len() >= 7 {
            &commit.sha[..7]
        } else {
            &commit.sha
        };
        let author = commit.author.as_deref().unwrap_or("");
        let date = commit
            .committed_date
            .as_ref()
            .map(|d| crate::util::format_date(d, "relative"))
            .unwrap_or_default();

        lines.push(StyledLine::from_spans(vec![
            StyledSpan::text(format!("{short_sha} "), theme.text_warning),
            StyledSpan::text(&commit.message, theme.text_primary),
        ]));
        if !author.is_empty() || !date.is_empty() {
            lines.push(StyledLine::from_spans(vec![
                StyledSpan::text(format!("        {author}"), theme.text_actor),
                StyledSpan::text(format!("  {date}"), theme.text_faint),
            ]));
        }
    }

    lines
}

// ---------------------------------------------------------------------------
// T076: Checks tab
// ---------------------------------------------------------------------------

/// Render the Checks tab: list of check runs with status icons.
pub fn render_checks(pr: &PullRequest, theme: &ResolvedTheme) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    if pr.check_runs.is_empty() {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "(no checks)",
            theme.text_faint,
        )));
        return lines;
    }

    for check in &pr.check_runs {
        let (icon, icon_color) = check_status_icon(check.status, check.conclusion, theme);
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::text(format!("{icon} "), icon_color),
            StyledSpan::text(&check.name, theme.text_primary),
        ]));
    }

    lines
}

fn check_status_icon(
    status: Option<CheckStatus>,
    conclusion: Option<CheckConclusion>,
    theme: &ResolvedTheme,
) -> (String, AppColor) {
    let icons = &theme.icons;
    match (status, conclusion) {
        (Some(CheckStatus::Completed), Some(CheckConclusion::Success)) => {
            (icons.check_success.clone(), theme.text_success)
        }
        (
            Some(CheckStatus::Completed),
            Some(CheckConclusion::Failure | CheckConclusion::TimedOut),
        ) => (icons.check_failure.clone(), theme.text_error),
        (Some(CheckStatus::Completed), Some(CheckConclusion::Cancelled))
        | (Some(CheckStatus::InProgress | CheckStatus::Queued), _) => {
            (icons.check_pending.clone(), theme.text_warning)
        }
        _ => (icons.check_pending.clone(), theme.text_faint),
    }
}

// ---------------------------------------------------------------------------
// T077: Files Changed tab
// ---------------------------------------------------------------------------

/// Render the Files Changed tab: list of files with stats.
pub fn render_files(detail: &PrDetail, theme: &ResolvedTheme) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    if detail.files.is_empty() {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "(no files changed)",
            theme.text_faint,
        )));
        return lines;
    }

    for file in &detail.files {
        let change = match file.status {
            Some(FileChangeType::Added) => "A",
            Some(FileChangeType::Deleted) => "D",
            Some(FileChangeType::Modified) => "M",
            Some(FileChangeType::Renamed) => "R",
            Some(FileChangeType::Copied) => "C",
            _ => "?",
        };
        let change_color = match file.status {
            Some(FileChangeType::Added) => theme.text_success,
            Some(FileChangeType::Deleted) => theme.text_error,
            _ => theme.text_warning,
        };

        lines.push(StyledLine::from_spans(vec![
            StyledSpan::text(format!("{change} "), change_color),
            StyledSpan::text(&file.path, theme.text_primary),
            StyledSpan::text(
                format!("  +{} -{}", file.additions, file.deletions),
                theme.text_faint,
            ),
        ]));
    }

    lines
}
