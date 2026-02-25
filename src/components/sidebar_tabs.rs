use crate::color::{Color as AppColor, ColorDepth};
use crate::markdown::renderer::{StyledLine, StyledSpan};
use crate::theme::ResolvedTheme;
use crate::types::{
    CheckConclusion, CheckRun, CheckStatus, CommitCheckState, FileChangeType, Issue, IssueDetail,
    PrDetail, PullRequest, ReviewState, TimelineEvent,
};
use unicode_width::UnicodeWidthStr;

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
            .map(|l| crate::util::expand_emoji(&l.name))
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

    // Dates: local time (age)
    let fmt = "%Y-%m-%d %H:%M:%S";
    let created_age = crate::util::format_date(&pr.created_at, "relative");
    let updated_age = crate::util::format_date(&pr.updated_at, "relative");
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold("Created:", theme.text_secondary),
        StyledSpan::text(
            format!(
                " {}",
                pr.created_at.with_timezone(&chrono::Local).format(fmt)
            ),
            theme.text_faint,
        ),
        StyledSpan::text(" (", theme.text_faint),
        StyledSpan::text(created_age, theme.text_secondary),
        StyledSpan::text(")", theme.text_faint),
    ]));
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold("Updated:", theme.text_secondary),
        StyledSpan::text(
            format!(
                " {}",
                pr.updated_at.with_timezone(&chrono::Local).format(fmt)
            ),
            theme.text_faint,
        ),
        StyledSpan::text(" (", theme.text_faint),
        StyledSpan::text(updated_age, theme.text_secondary),
        StyledSpan::text(")", theme.text_faint),
    ]));

    // Changes
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold("Lines:  ", theme.text_secondary),
        StyledSpan::text(format!("+{}", pr.additions), theme.text_success),
        StyledSpan::text(" / ", theme.text_faint),
        StyledSpan::text(format!("-{}", pr.deletions), theme.text_error),
    ]));

    // Centered horizontal rule before body
    lines.push(StyledLine::from_span(StyledSpan::text(
        "─".repeat(20),
        theme.md_horizontal_rule,
    )));

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

        let mut spans = vec![StyledSpan::text(
            format!("{short_sha} "),
            theme.text_warning,
        )];
        if let Some(state) = commit.check_state {
            let (icon, color) = commit_check_state_icon(state, theme);
            spans.push(StyledSpan::text(format!("{icon} "), color));
        }
        spans.push(StyledSpan::text(&commit.message, theme.text_primary));
        lines.push(StyledLine::from_spans(spans));
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

/// Render the Checks tab: check runs grouped by workflow, with duration column.
pub fn render_checks(pr: &PullRequest, theme: &ResolvedTheme) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    if pr.check_runs.is_empty() {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "(no checks)",
            theme.text_faint,
        )));
        return lines;
    }

    // Group checks by workflow_name, preserving insertion order via IndexMap-like Vec.
    let groups = group_checks_by_workflow(&pr.check_runs);

    // Global max name width across ALL checks for uniform duration column.
    let max_name_w = pr
        .check_runs
        .iter()
        .map(|c| UnicodeWidthStr::width(c.name.as_str()))
        .max()
        .unwrap_or(0);

    for (i, (wf_name, checks)) in groups.iter().enumerate() {
        if i > 0 {
            lines.push(StyledLine::blank());
        }
        // Header line
        let header = wf_name.as_deref().unwrap_or("(other)");
        lines.push(StyledLine::from_span(StyledSpan::text(
            header,
            theme.text_faint,
        )));

        for check in checks {
            let (icon, icon_color) = check_status_icon(check.status, check.conclusion, theme);
            let dur = crate::util::format_duration(check.started_at, check.completed_at);
            let mut spans = vec![
                StyledSpan::text("  ", theme.text_primary),
                StyledSpan::text(format!("{icon} "), icon_color),
                StyledSpan::text(&check.name, theme.text_primary),
            ];
            if !dur.is_empty() {
                let name_w = UnicodeWidthStr::width(check.name.as_str());
                let pad = max_name_w - name_w + 2;
                spans.push(StyledSpan::text(
                    format!("{:pad$}{dur}", "", pad = pad),
                    theme.text_faint,
                ));
            }
            lines.push(StyledLine::from_spans(spans));
        }
    }

    lines
}

/// Group check runs by `workflow_name`, keeping insertion order.
/// The `None`-keyed group (non-Actions checks) is placed last.
fn group_checks_by_workflow(checks: &[CheckRun]) -> Vec<(Option<String>, Vec<&CheckRun>)> {
    let mut groups: Vec<(Option<String>, Vec<&CheckRun>)> = Vec::new();
    for check in checks {
        let key = &check.workflow_name;
        if let Some(pos) = groups.iter().position(|(k, _)| k == key) {
            groups[pos].1.push(check);
        } else {
            groups.push((key.clone(), vec![check]));
        }
    }
    // Move the None-keyed group to the end.
    if let Some(pos) = groups.iter().position(|(k, _)| k.is_none()) {
        let none_group = groups.remove(pos);
        groups.push(none_group);
    }
    groups
}

fn commit_check_state_icon(state: CommitCheckState, theme: &ResolvedTheme) -> (String, AppColor) {
    let icons = &theme.icons;
    match state {
        CommitCheckState::Success => (icons.check_success.clone(), theme.text_success),
        CommitCheckState::Failure | CommitCheckState::Error => {
            (icons.check_failure.clone(), theme.text_error)
        }
        _ => (icons.check_pending.clone(), theme.text_warning),
    }
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

// ---------------------------------------------------------------------------
// Issue Overview tab
// ---------------------------------------------------------------------------

/// Render the Overview tab metadata for an issue: labels, assignees, reactions.
pub fn render_issue_overview_metadata(issue: &Issue, theme: &ResolvedTheme) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    // Labels
    if !issue.labels.is_empty() {
        let label_text = issue
            .labels
            .iter()
            .map(|l| crate::util::expand_emoji(&l.name))
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("Labels: ", theme.text_secondary),
            StyledSpan::text(label_text, theme.text_primary),
        ]));
    }

    // Assignees
    if !issue.assignees.is_empty() {
        let assignee_text = issue
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

    // Dates: local time (age)
    let fmt = "%Y-%m-%d %H:%M:%S";
    let created_age = crate::util::format_date(&issue.created_at, "relative");
    let updated_age = crate::util::format_date(&issue.updated_at, "relative");
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold("Created:", theme.text_secondary),
        StyledSpan::text(
            format!(
                " {}",
                issue.created_at.with_timezone(&chrono::Local).format(fmt)
            ),
            theme.text_faint,
        ),
        StyledSpan::text(" (", theme.text_faint),
        StyledSpan::text(created_age, theme.text_secondary),
        StyledSpan::text(")", theme.text_faint),
    ]));
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold("Updated:", theme.text_secondary),
        StyledSpan::text(
            format!(
                " {}",
                issue.updated_at.with_timezone(&chrono::Local).format(fmt)
            ),
            theme.text_faint,
        ),
        StyledSpan::text(" (", theme.text_faint),
        StyledSpan::text(updated_age, theme.text_secondary),
        StyledSpan::text(")", theme.text_faint),
    ]));

    // Blank line between metadata and reactions
    lines.push(StyledLine::blank());

    // Reactions
    let r = &issue.reactions;
    let total = r.total();
    if total > 0 {
        let mut parts = Vec::new();
        if r.thumbs_up > 0 {
            parts.push(format!("\u{1f44d} {}", r.thumbs_up));
        }
        if r.thumbs_down > 0 {
            parts.push(format!("\u{1f44e} {}", r.thumbs_down));
        }
        if r.laugh > 0 {
            parts.push(format!("\u{1f604} {}", r.laugh));
        }
        if r.hooray > 0 {
            parts.push(format!("\u{1f389} {}", r.hooray));
        }
        if r.confused > 0 {
            parts.push(format!("\u{1f615} {}", r.confused));
        }
        if r.heart > 0 {
            parts.push(format!("\u{2764}\u{fe0f} {}", r.heart));
        }
        if r.rocket > 0 {
            parts.push(format!("\u{1f680} {}", r.rocket));
        }
        if r.eyes > 0 {
            parts.push(format!("\u{1f440} {}", r.eyes));
        }
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("React:  ", theme.text_secondary),
            StyledSpan::text(parts.join("  "), theme.text_primary),
        ]));
    }

    // Horizontal rule separator before body
    lines.push(StyledLine::from_span(StyledSpan::text(
        "\u{2500}".repeat(20),
        theme.md_horizontal_rule,
    )));

    lines
}

// ---------------------------------------------------------------------------
// Issue Activity tab
// ---------------------------------------------------------------------------

/// Render the Activity tab for an issue: chronological timeline events.
pub fn render_issue_activity(
    detail: &IssueDetail,
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
