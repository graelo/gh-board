use crate::color::{Color as AppColor, ColorDepth};
use crate::markdown::renderer::{StyledLine, StyledSpan};
use crate::theme::ResolvedTheme;
use crate::types::{
    CheckConclusion, CheckRun, CheckStatus, CommitCheckState, FileChangeType, IssueDetail,
    PrDetail, PullRequest, ReviewState, TimelineEvent,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
        spans.push(StyledSpan::text(
            crate::util::expand_emoji(&commit.message),
            theme.text_primary,
        ));
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
pub fn render_checks(
    pr: &PullRequest,
    theme: &ResolvedTheme,
    sidebar_width: u16,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    if pr.check_runs.is_empty() {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "(no checks)",
            theme.text_faint,
        )));
        return lines;
    }

    // Content width = sidebar minus left border (1) + padding (2) + scrollbar (1).
    let content_width = usize::from(sidebar_width).saturating_sub(4).max(1);

    // Group checks by workflow_name, preserving insertion order via IndexMap-like Vec.
    let groups = group_checks_by_workflow(&pr.check_runs);

    // Pre-expand check names so width calculations use the rendered text.
    let expanded_names: Vec<Vec<std::borrow::Cow<'_, str>>> = groups
        .iter()
        .map(|(_, checks)| {
            checks
                .iter()
                .map(|c| crate::util::expand_emoji(&c.name))
                .collect()
        })
        .collect();

    // Pre-format durations and find the widest one.
    let durations: Vec<Vec<String>> = groups
        .iter()
        .map(|(_, checks)| {
            checks
                .iter()
                .map(|c| crate::util::format_duration(c.started_at, c.completed_at))
                .collect()
        })
        .collect();

    let max_dur_w = durations
        .iter()
        .flat_map(|ds| ds.iter())
        .map(|d| UnicodeWidthStr::width(d.as_str()))
        .max()
        .unwrap_or(0);

    // Fixed overhead per check line: indent (2) + icon + space (2) + min gap (1).
    let fixed_cols = 2 + 2 + 1 + if max_dur_w > 0 { 1 + max_dur_w } else { 0 };
    let name_budget = content_width.saturating_sub(fixed_cols);

    // Natural alignment: use the longest name, but cap to the budget.
    let natural_max = expanded_names
        .iter()
        .flat_map(|names| names.iter())
        .map(|n| UnicodeWidthStr::width(n.as_ref()))
        .max()
        .unwrap_or(0);
    let name_col_width = natural_max.min(name_budget);

    for (i, ((wf_name, checks), (names, durs))) in groups
        .iter()
        .zip(expanded_names.iter().zip(&durations))
        .enumerate()
    {
        let _ = checks; // used via names/durs iterators
        if i > 0 {
            lines.push(StyledLine::blank());
        }
        // Header line
        let header = wf_name.as_deref().unwrap_or("(other)");
        lines.push(StyledLine::from_span(StyledSpan::text(
            header,
            theme.text_faint,
        )));

        for ((check, expanded_name), dur) in checks.iter().zip(names).zip(durs) {
            let (icon, icon_color) = check_status_icon(check.status, check.conclusion, theme);
            let name_w = UnicodeWidthStr::width(expanded_name.as_ref());
            let (display_name, display_w) = if name_w > name_col_width {
                truncate_with_ellipsis(expanded_name.as_ref(), name_col_width)
            } else {
                (expanded_name.to_string(), name_w)
            };
            let mut spans = vec![
                StyledSpan::text("  ", theme.text_primary),
                StyledSpan::text(format!("{icon} "), icon_color),
                StyledSpan::text(display_name, theme.text_primary),
            ];
            if !dur.is_empty() {
                let pad = name_col_width.saturating_sub(display_w) + 1;
                spans.push(StyledSpan::text(
                    format!("{:pad$}{:>width$}", "", dur, pad = pad, width = max_dur_w),
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
    // Sort named groups alphabetically (None < Some, so None goes first).
    groups.sort_by(|a, b| a.0.cmp(&b.0));

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
///
/// `sidebar_width` is the total sidebar width in columns (including border,
/// padding, and scrollbar). When provided, paths that would push the stats
/// columns beyond the sidebar edge are truncated with `…`.
pub fn render_files(
    detail: &PrDetail,
    theme: &ResolvedTheme,
    sidebar_width: u16,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    if detail.files.is_empty() {
        lines.push(StyledLine::from_span(StyledSpan::text(
            "(no files changed)",
            theme.text_faint,
        )));
        return lines;
    }

    // Content width = sidebar minus left border (1) + padding (2) + scrollbar (1).
    let content_width = usize::from(sidebar_width).saturating_sub(4).max(1);

    // Pre-compute column widths so both +N and -N are right-aligned.
    let max_add_width = detail
        .files
        .iter()
        .map(|f| format!("+{}", f.additions).len())
        .max()
        .unwrap_or(2);
    let max_del_width = detail
        .files
        .iter()
        .map(|f| format!("-{}", f.deletions).len())
        .max()
        .unwrap_or(2);

    // Fixed overhead: status letter + space (2), min gap (1), space between
    // stats (1), plus the two stat columns.
    let fixed_cols = 2 + 1 + max_add_width + 1 + max_del_width;
    let path_budget = content_width.saturating_sub(fixed_cols);

    // Natural alignment: use the longest path width, but cap to the budget so
    // stats columns never overflow the sidebar.
    let natural_max = detail
        .files
        .iter()
        .map(|f| UnicodeWidthStr::width(f.path.as_str()))
        .max()
        .unwrap_or(0);
    let path_col_width = natural_max.min(path_budget);

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

        let path_w = UnicodeWidthStr::width(file.path.as_str());
        let (display_path, display_w) = if path_w > path_col_width {
            truncate_with_ellipsis(&file.path, path_col_width)
        } else {
            (file.path.clone(), path_w)
        };
        let pad = path_col_width.saturating_sub(display_w) + 1; // +1 = min gap

        lines.push(StyledLine::from_spans(vec![
            StyledSpan::text(format!("{change} "), change_color),
            StyledSpan::text(display_path, theme.text_primary),
            StyledSpan::text(
                format!(
                    "{:pad$}{:>width$}",
                    "",
                    format!("+{}", file.additions),
                    pad = pad,
                    width = max_add_width
                ),
                theme.text_success,
            ),
            StyledSpan::text(
                format!(
                    " {:>width$}",
                    format!("-{}", file.deletions),
                    width = max_del_width
                ),
                theme.text_error,
            ),
        ]));
    }

    lines
}

/// Truncate a string to fit within `max_width` display columns, appending `…`
/// if truncation occurs. Returns `(truncated_string, display_width)`.
fn truncate_with_ellipsis(s: &str, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }
    let ellipsis_w = 1; // `…` is 1 column wide
    let target = max_width.saturating_sub(ellipsis_w);
    let mut buf = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > target {
            break;
        }
        buf.push(ch);
        w += cw;
    }
    buf.push('…');
    w += ellipsis_w;
    (buf, w)
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
