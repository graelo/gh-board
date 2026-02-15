use crate::config::types::IconConfig;

/// Fully resolved icon set: every slot has a concrete `String` value
/// (either from a preset or from per-icon user overrides).
#[derive(Debug, Clone)]
pub struct ResolvedIcons {
    // PR state (4)
    pub pr_open: String,
    pub pr_closed: String,
    pub pr_merged: String,
    pub pr_draft: String,
    // PR column headers (6)
    pub header_state: String,
    pub header_comments: String,
    pub header_review: String,
    pub header_ci: String,
    pub header_lines: String,
    pub header_time: String,
    // Review decision (5)
    pub review_approved: String,
    pub review_changes: String,
    pub review_required: String,
    pub review_none: String,
    pub review_commented: String,
    // CI status (4)
    pub ci_success: String,
    pub ci_failure: String,
    pub ci_pending: String,
    pub ci_none: String,
    // Issue state (2)
    pub issue_open: String,
    pub issue_closed: String,
    // Notifications (5)
    pub notif_unread: String,
    pub notif_type_pr: String,
    pub notif_type_issue: String,
    pub notif_type_release: String,
    pub notif_type_discussion: String,
    // Branch (2)
    pub branch_ahead: String,
    pub branch_behind: String,
    // Sidebar checks (3)
    pub check_success: String,
    pub check_failure: String,
    pub check_pending: String,
    // Sidebar decorative (1)
    pub branch_arrow: String,
    // Sidebar tabs (5)
    pub tab_overview: String,
    pub tab_activity: String,
    pub tab_commits: String,
    pub tab_checks: String,
    pub tab_files: String,
    // Author roles (6)
    pub role_newcontributor: String,
    pub role_contributor: String,
    pub role_collaborator: String,
    pub role_member: String,
    pub role_owner: String,
    pub role_unknown: String,
    // Status bar sections (4)
    pub section_prs: String,
    pub section_issues: String,
    pub section_notifications: String,
    pub section_repo: String,
    // Pill caps (rounded edges, 2)
    pub pill_left: String,
    pub pill_right: String,
}

impl ResolvedIcons {
    /// Current hardcoded values — the default preset.
    fn unicode() -> Self {
        Self {
            // PR state
            pr_open: "\u{25cf}".to_owned(),   // ●
            pr_closed: "\u{2716}".to_owned(), // ✖
            pr_merged: "\u{2714}".to_owned(), // ✔
            pr_draft: "\u{25cb}".to_owned(),  // ○
            // PR column headers
            header_state: "\u{1f500}".to_owned(),    // 🔀
            header_comments: "\u{1f4ac}".to_owned(), // 💬
            header_review: "\u{1f464}".to_owned(),   // 👤
            header_ci: "\u{1f4cb}".to_owned(),       // 📋
            header_lines: "\u{00b1}".to_owned(),     // ±
            header_time: "\u{1f550}".to_owned(),     // 🕐
            // Review decision
            review_approved: "\u{2714}".to_owned(), // ✔
            review_changes: "\u{2716}".to_owned(),  // ✖
            review_required: "\u{25cb}".to_owned(), // ○
            review_none: "-".to_owned(),
            review_commented: "\u{1f4ac}".to_owned(), // 💬
            // CI status
            ci_success: "\u{2714}".to_owned(), // ✔
            ci_failure: "\u{2716}".to_owned(), // ✖
            ci_pending: "\u{25cb}".to_owned(), // ○
            ci_none: "-".to_owned(),
            // Issue state
            issue_open: "\u{25cf}".to_owned(),   // ●
            issue_closed: "\u{2714}".to_owned(), // ✔
            // Notifications
            notif_unread: "\u{25cf}".to_owned(),          // ●
            notif_type_pr: "\u{21c4}".to_owned(),         // ⇄
            notif_type_issue: "\u{25cb}".to_owned(),      // ○
            notif_type_release: "\u{25b3}".to_owned(),    // △
            notif_type_discussion: "\u{25a1}".to_owned(), // □
            // Branch
            branch_ahead: "\u{2191}".to_owned(),  // ↑
            branch_behind: "\u{2193}".to_owned(), // ↓
            // Sidebar checks
            check_success: "\u{2714}".to_owned(), // ✔
            check_failure: "\u{2716}".to_owned(), // ✖
            check_pending: "\u{25cb}".to_owned(), // ○
            // Sidebar decorative
            branch_arrow: "\u{2192}".to_owned(), // →
            // Sidebar tabs
            tab_overview: "\u{2630}".to_owned(), // ☰
            tab_activity: "\u{25b7}".to_owned(), // ▷
            tab_commits: "\u{25cb}".to_owned(),  // ○
            tab_checks: "\u{2611}".to_owned(),   // ☑
            tab_files: "\u{25a4}".to_owned(),    // ▤
            // Author roles
            role_newcontributor: "\u{2728}".to_owned(), // ✨
            role_contributor: "\u{2713}".to_owned(),    // ✓
            role_collaborator: "\u{25c6}".to_owned(),   // ◆
            role_member: "\u{25cf}".to_owned(),         // ●
            role_owner: "\u{2605}".to_owned(),          // ★
            role_unknown: "?".to_owned(),
            // Status bar sections
            section_prs: "\u{2195}".to_owned(),            // ↕
            section_issues: "\u{2299}".to_owned(),         // ⊙
            section_notifications: "\u{1f514}".to_owned(), // 🔔
            section_repo: "\u{2387}".to_owned(),           // ⎇
            // Pill caps (no rounded edges in unicode preset)
            pill_left: String::new(),
            pill_right: String::new(),
        }
    }

    /// Nerdfont glyphs — requires a patched font.
    /// Codepoints match gh-dash for visual parity.
    fn nerdfont() -> Self {
        Self {
            // PR state (matching gh-dash constants.go)
            pr_open: "\u{f407}".to_owned(), //  nf-oct-git_pull_request
            pr_closed: "\u{f4dc}".to_owned(), //  nf-oct-git_pull_request_closed
            pr_merged: "\u{f4c9}".to_owned(), //  nf-oct-git_merge
            pr_draft: "\u{ebdb}".to_owned(), //  nf-cod-git_pull_request_draft
            // PR column headers (matching gh-dash section headers)
            header_state: "\u{f407}".to_owned(), //  nf-oct-git_pull_request
            header_comments: "\u{f27b}".to_owned(), //  nf-fa-comment_o
            header_review: "\u{f0be2}".to_owned(), // 󰯢 nf-md-account_supervisor
            header_ci: "\u{f45e}".to_owned(),    //  nf-oct-checklist
            header_lines: "\u{f440}".to_owned(), //  nf-oct-diff
            header_time: "\u{f19bb}".to_owned(), // 󱦻 nf-md-clock_edit_outline
            // Review decision (matching gh-dash)
            review_approved: "\u{f012c}".to_owned(), // 󰄬 nf-md-check
            review_changes: "\u{eb43}".to_owned(),   //  nf-cod-request_changes
            review_required: "\u{e641}".to_owned(),  //  nf-seti-clock (waiting)
            review_none: "\u{eabd}".to_owned(),      //  nf-cod-circle_slash
            review_commented: "\u{f27b}".to_owned(), //  nf-fa-comment_o
            // CI status (matching gh-dash)
            ci_success: "\u{f058}".to_owned(), //  nf-fa-check_circle
            ci_failure: "\u{f0159}".to_owned(), // 󰅙 nf-md-close_circle
            ci_pending: "\u{e641}".to_owned(), //  nf-seti-clock (waiting)
            ci_none: "\u{eabd}".to_owned(),    //  nf-cod-circle_slash
            // Issue state (matching gh-dash)
            issue_open: "\u{f41b}".to_owned(), //  nf-oct-issue_opened
            issue_closed: "\u{f41d}".to_owned(), //  nf-oct-issue_closed
            // Notifications (matching gh-dash)
            notif_unread: "\u{f444}".to_owned(), //  nf-oct-dot_fill
            notif_type_pr: "\u{f407}".to_owned(), //  nf-oct-git_pull_request
            notif_type_issue: "\u{f41b}".to_owned(), //  nf-oct-issue_opened
            notif_type_release: "\u{f412}".to_owned(), //  nf-oct-tag
            notif_type_discussion: "\u{f442}".to_owned(), //  nf-oct-comment_discussion
            // Branch
            branch_ahead: "\u{f062}".to_owned(), //  nf-fa-arrow_up
            branch_behind: "\u{f063}".to_owned(), //  nf-fa-arrow_down
            // Sidebar checks
            check_success: "\u{f058}".to_owned(), //  nf-fa-check_circle
            check_failure: "\u{f0159}".to_owned(), // 󰅙 nf-md-close_circle
            check_pending: "\u{e641}".to_owned(), //  nf-seti-clock
            // Sidebar decorative
            branch_arrow: "\u{e344}".to_owned(), //  nf-pl-left_hard_divider
            // Sidebar tabs (Codicons + Octicons)
            tab_overview: "\u{eb2f}".to_owned(), //  nf-cod-preview
            tab_activity: "\u{f442}".to_owned(), //  nf-oct-comment_discussion
            tab_commits: "\u{f4b6}".to_owned(),  //  nf-oct-git_commit
            tab_checks: "\u{f52e}".to_owned(),   //  nf-oct-checklist
            tab_files: "\u{f4d2}".to_owned(),    //  nf-oct-diff
            // Author roles (matching gh-dash)
            role_newcontributor: "\u{f0394}".to_owned(), // 󰎔 nf-md-new_box
            role_contributor: "\u{edc6}".to_owned(),     //  nf-fa-user_check
            role_collaborator: "\u{edcf}".to_owned(),    //  nf-fa-user_shield
            role_member: "\u{f42b}".to_owned(),          //  nf-oct-organization
            role_owner: "\u{f511}".to_owned(),           //  nf-oct-shield_lock
            role_unknown: "\u{f0b59}".to_owned(),        // 󰭙 nf-md-account_question
            // Status bar sections
            section_prs: "\u{f407}".to_owned(), //  nf-oct-git_pull_request
            section_issues: "\u{f41b}".to_owned(), //  nf-oct-issue_opened
            section_notifications: "\u{eaa2}".to_owned(), //  nf-cod-bell
            section_repo: "\u{e727}".to_owned(), //  nf-dev-git_branch
            // Pill caps (Powerline half-circles for rounded edges)
            pill_left: "\u{e0b6}".to_owned(), //  nf-pl-right_half_circle_thick
            pill_right: "\u{e0b4}".to_owned(), //  nf-pl-left_half_circle_thick
        }
    }

    /// Plain ASCII fallback — works everywhere.
    fn ascii() -> Self {
        Self {
            // PR state
            pr_open: "o".to_owned(),
            pr_closed: "x".to_owned(),
            pr_merged: "v".to_owned(),
            pr_draft: "-".to_owned(),
            // PR column headers
            header_state: "St".to_owned(),
            header_comments: "Cmt".to_owned(),
            header_review: "Rv".to_owned(),
            header_ci: "CI".to_owned(),
            header_lines: "+/-".to_owned(),
            header_time: "Time".to_owned(),
            // Review decision
            review_approved: "v".to_owned(),
            review_changes: "x".to_owned(),
            review_required: "o".to_owned(),
            review_none: "-".to_owned(),
            review_commented: "c".to_owned(),
            // CI status
            ci_success: "v".to_owned(),
            ci_failure: "x".to_owned(),
            ci_pending: "~".to_owned(),
            ci_none: "-".to_owned(),
            // Issue state
            issue_open: "o".to_owned(),
            issue_closed: "v".to_owned(),
            // Notifications
            notif_unread: "*".to_owned(),
            notif_type_pr: "PR".to_owned(),
            notif_type_issue: "I".to_owned(),
            notif_type_release: "R".to_owned(),
            notif_type_discussion: "D".to_owned(),
            // Branch
            branch_ahead: "^".to_owned(),
            branch_behind: "v".to_owned(),
            // Sidebar checks
            check_success: "v".to_owned(),
            check_failure: "x".to_owned(),
            check_pending: "~".to_owned(),
            // Sidebar decorative
            branch_arrow: "->".to_owned(),
            // Sidebar tabs
            tab_overview: "=".to_owned(),
            tab_activity: ">".to_owned(),
            tab_commits: "o".to_owned(),
            tab_checks: "+".to_owned(),
            tab_files: "#".to_owned(),
            // Author roles
            role_newcontributor: "*".to_owned(),
            role_contributor: "+".to_owned(),
            role_collaborator: "#".to_owned(),
            role_member: "@".to_owned(),
            role_owner: "!".to_owned(),
            role_unknown: "?".to_owned(),
            // Status bar sections
            section_prs: "PR".to_owned(),
            section_issues: "I".to_owned(),
            section_notifications: "N".to_owned(),
            section_repo: "R".to_owned(),
            // Pill caps (no rounded edges in ascii preset)
            pill_left: String::new(),
            pill_right: String::new(),
        }
    }

    /// Build a resolved icon set from user config: pick a preset, then apply
    /// per-icon overrides.
    pub fn resolve(config: &IconConfig) -> Self {
        let base = match config.preset.as_deref() {
            Some("nerdfont") => Self::nerdfont(),
            Some("ascii") => Self::ascii(),
            _ => Self::unicode(),
        };

        Self {
            pr_open: config.pr_open.clone().unwrap_or(base.pr_open),
            pr_closed: config.pr_closed.clone().unwrap_or(base.pr_closed),
            pr_merged: config.pr_merged.clone().unwrap_or(base.pr_merged),
            pr_draft: config.pr_draft.clone().unwrap_or(base.pr_draft),
            header_state: config.header_state.clone().unwrap_or(base.header_state),
            header_comments: config
                .header_comments
                .clone()
                .unwrap_or(base.header_comments),
            header_review: config.header_review.clone().unwrap_or(base.header_review),
            header_ci: config.header_ci.clone().unwrap_or(base.header_ci),
            header_lines: config.header_lines.clone().unwrap_or(base.header_lines),
            header_time: config.header_time.clone().unwrap_or(base.header_time),
            review_approved: config
                .review_approved
                .clone()
                .unwrap_or(base.review_approved),
            review_changes: config.review_changes.clone().unwrap_or(base.review_changes),
            review_required: config
                .review_required
                .clone()
                .unwrap_or(base.review_required),
            review_none: config.review_none.clone().unwrap_or(base.review_none),
            review_commented: config
                .review_commented
                .clone()
                .unwrap_or(base.review_commented),
            ci_success: config.ci_success.clone().unwrap_or(base.ci_success),
            ci_failure: config.ci_failure.clone().unwrap_or(base.ci_failure),
            ci_pending: config.ci_pending.clone().unwrap_or(base.ci_pending),
            ci_none: config.ci_none.clone().unwrap_or(base.ci_none),
            issue_open: config.issue_open.clone().unwrap_or(base.issue_open),
            issue_closed: config.issue_closed.clone().unwrap_or(base.issue_closed),
            notif_unread: config.notif_unread.clone().unwrap_or(base.notif_unread),
            notif_type_pr: config.notif_type_pr.clone().unwrap_or(base.notif_type_pr),
            notif_type_issue: config
                .notif_type_issue
                .clone()
                .unwrap_or(base.notif_type_issue),
            notif_type_release: config
                .notif_type_release
                .clone()
                .unwrap_or(base.notif_type_release),
            notif_type_discussion: config
                .notif_type_discussion
                .clone()
                .unwrap_or(base.notif_type_discussion),
            branch_ahead: config.branch_ahead.clone().unwrap_or(base.branch_ahead),
            branch_behind: config.branch_behind.clone().unwrap_or(base.branch_behind),
            check_success: config.check_success.clone().unwrap_or(base.check_success),
            check_failure: config.check_failure.clone().unwrap_or(base.check_failure),
            check_pending: config.check_pending.clone().unwrap_or(base.check_pending),
            branch_arrow: config.branch_arrow.clone().unwrap_or(base.branch_arrow),
            tab_overview: config.tab_overview.clone().unwrap_or(base.tab_overview),
            tab_activity: config.tab_activity.clone().unwrap_or(base.tab_activity),
            tab_commits: config.tab_commits.clone().unwrap_or(base.tab_commits),
            tab_checks: config.tab_checks.clone().unwrap_or(base.tab_checks),
            tab_files: config.tab_files.clone().unwrap_or(base.tab_files),
            role_newcontributor: config
                .role_newcontributor
                .clone()
                .unwrap_or(base.role_newcontributor),
            role_contributor: config
                .role_contributor
                .clone()
                .unwrap_or(base.role_contributor),
            role_collaborator: config
                .role_collaborator
                .clone()
                .unwrap_or(base.role_collaborator),
            role_member: config.role_member.clone().unwrap_or(base.role_member),
            role_owner: config.role_owner.clone().unwrap_or(base.role_owner),
            role_unknown: config.role_unknown.clone().unwrap_or(base.role_unknown),
            section_prs: config.section_prs.clone().unwrap_or(base.section_prs),
            section_issues: config.section_issues.clone().unwrap_or(base.section_issues),
            section_notifications: config
                .section_notifications
                .clone()
                .unwrap_or(base.section_notifications),
            section_repo: config.section_repo.clone().unwrap_or(base.section_repo),
            pill_left: config.pill_left.clone().unwrap_or(base.pill_left),
            pill_right: config.pill_right.clone().unwrap_or(base.pill_right),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_preset_is_default() {
        let config = IconConfig::default();
        let icons = ResolvedIcons::resolve(&config);
        assert_eq!(icons.pr_open, "\u{25cf}");
        assert_eq!(icons.ci_success, "\u{2714}");
    }

    #[test]
    fn nerdfont_preset() {
        let config = IconConfig {
            preset: Some("nerdfont".to_owned()),
            ..Default::default()
        };
        let icons = ResolvedIcons::resolve(&config);
        assert_eq!(icons.pr_open, "\u{f407}");
    }

    #[test]
    fn ascii_preset() {
        let config = IconConfig {
            preset: Some("ascii".to_owned()),
            ..Default::default()
        };
        let icons = ResolvedIcons::resolve(&config);
        assert_eq!(icons.pr_open, "o");
        assert_eq!(icons.ci_success, "v");
    }

    #[test]
    fn per_icon_override() {
        let config = IconConfig {
            pr_open: Some("X".to_owned()),
            ..Default::default()
        };
        let icons = ResolvedIcons::resolve(&config);
        assert_eq!(icons.pr_open, "X");
        // Others stay at unicode defaults.
        assert_eq!(icons.pr_closed, "\u{2716}");
    }

    #[test]
    fn override_on_top_of_preset() {
        let config = IconConfig {
            preset: Some("ascii".to_owned()),
            pr_open: Some("CUSTOM".to_owned()),
            ..Default::default()
        };
        let icons = ResolvedIcons::resolve(&config);
        assert_eq!(icons.pr_open, "CUSTOM");
        // Rest stays ascii.
        assert_eq!(icons.pr_closed, "x");
    }
}
