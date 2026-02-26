use gh_board::color::ColorDepth;
use gh_board::components::sidebar::SidebarTab;
use gh_board::components::sidebar_tabs;
use gh_board::theme::ResolvedTheme;
use gh_board::types::{
    Actor, CheckConclusion, CheckRun, CheckStatus, Commit, File, FileChangeType, PrDetail, PrState,
    PullRequest, Review, ReviewState, TimelineEvent,
};

fn test_theme() -> ResolvedTheme {
    use gh_board::config::types::Theme;
    use gh_board::theme::Background;
    ResolvedTheme::resolve(&Theme::default(), Background::Dark)
}

fn test_pr() -> PullRequest {
    use chrono::Utc;
    PullRequest {
        number: 42,
        title: "Test PR".to_owned(),
        body: "Some body text".to_owned(),
        author: Some(Actor {
            login: "testuser".to_owned(),
            avatar_url: String::new(),
        }),
        state: PrState::Open,
        is_draft: false,
        mergeable: None,
        review_decision: None,
        additions: 10,
        deletions: 5,
        head_ref: "feature-branch".to_owned(),
        base_ref: "main".to_owned(),
        labels: vec![gh_board::types::Label {
            name: "bug".to_owned(),
            color: "ff0000".to_owned(),
        }],
        assignees: vec![Actor {
            login: "assignee1".to_owned(),
            avatar_url: String::new(),
        }],
        commits: Vec::new(),
        comments: Vec::new(),
        review_threads: Vec::new(),
        review_requests: Vec::new(),
        reviews: Vec::new(),
        timeline_events: Vec::new(),
        files: Vec::new(),
        check_runs: vec![
            CheckRun {
                name: "CI Build".to_owned(),
                status: Some(CheckStatus::Completed),
                conclusion: Some(CheckConclusion::Success),
                url: None,
                workflow_run_id: None,
                workflow_name: None,
                started_at: None,
                completed_at: None,
            },
            CheckRun {
                name: "Lint".to_owned(),
                status: Some(CheckStatus::Completed),
                conclusion: Some(CheckConclusion::Failure),
                url: None,
                workflow_run_id: None,
                workflow_name: None,
                started_at: None,
                completed_at: None,
            },
        ],
        updated_at: Utc::now(),
        created_at: Utc::now(),
        url: "https://github.com/owner/repo/pull/42".to_owned(),
        repo: Some(gh_board::types::RepoRef {
            owner: "owner".to_owned(),
            name: "repo".to_owned(),
        }),
        comment_count: 3,
        author_association: None,
        participants: Vec::new(),
        merge_state_status: None,
        head_repo_owner: None,
        head_repo_name: None,
    }
}

fn test_detail() -> PrDetail {
    use chrono::{Duration, Utc};
    PrDetail {
        body: "Detail body".to_owned(),
        reviews: vec![Review {
            author: Some(Actor {
                login: "reviewer".to_owned(),
                avatar_url: String::new(),
            }),
            state: ReviewState::Approved,
            body: "LGTM".to_owned(),
            submitted_at: Some(Utc::now() - Duration::hours(1)),
        }],
        review_threads: Vec::new(),
        timeline_events: vec![
            TimelineEvent::Comment {
                author: Some("commenter".to_owned()),
                body: "Nice work!".to_owned(),
                created_at: Utc::now() - Duration::hours(3),
            },
            TimelineEvent::Review {
                author: Some("reviewer".to_owned()),
                state: ReviewState::Approved,
                body: "LGTM".to_owned(),
                submitted_at: Utc::now() - Duration::hours(1),
            },
            TimelineEvent::Merged {
                actor: Some("merger".to_owned()),
                created_at: Utc::now() - Duration::minutes(30),
            },
        ],
        commits: vec![
            Commit {
                sha: "abc1234567890".to_owned(),
                message: "Initial commit".to_owned(),
                author: Some("Author One".to_owned()),
                committed_date: Some(Utc::now() - Duration::days(1)),
                check_state: None,
            },
            Commit {
                sha: "def456".to_owned(),
                message: "Fix bug".to_owned(),
                author: None,
                committed_date: None,
                check_state: None,
            },
        ],
        files: vec![
            File {
                path: "src/main.rs".to_owned(),
                additions: 10,
                deletions: 3,
                status: Some(FileChangeType::Modified),
            },
            File {
                path: "src/new.rs".to_owned(),
                additions: 50,
                deletions: 0,
                status: Some(FileChangeType::Added),
            },
        ],
        mergeable: None,
        behind_by: None,
    }
}

// ---------------------------------------------------------------------------
// T072: SidebarTab tests
// ---------------------------------------------------------------------------

#[test]
fn sidebar_tab_cycle_next() {
    assert_eq!(SidebarTab::Overview.next(), SidebarTab::Activity);
    assert_eq!(SidebarTab::Activity.next(), SidebarTab::Commits);
    assert_eq!(SidebarTab::Commits.next(), SidebarTab::Checks);
    assert_eq!(SidebarTab::Checks.next(), SidebarTab::Files);
    assert_eq!(SidebarTab::Files.next(), SidebarTab::Overview);
}

#[test]
fn sidebar_tab_cycle_prev() {
    assert_eq!(SidebarTab::Overview.prev(), SidebarTab::Files);
    assert_eq!(SidebarTab::Files.prev(), SidebarTab::Checks);
    assert_eq!(SidebarTab::Checks.prev(), SidebarTab::Commits);
    assert_eq!(SidebarTab::Commits.prev(), SidebarTab::Activity);
    assert_eq!(SidebarTab::Activity.prev(), SidebarTab::Overview);
}

#[test]
fn sidebar_tab_labels() {
    assert_eq!(SidebarTab::Overview.label(), "Overview");
    assert_eq!(SidebarTab::Activity.label(), "Activity");
    assert_eq!(SidebarTab::Commits.label(), "Commits");
    assert_eq!(SidebarTab::Checks.label(), "Checks");
    assert_eq!(SidebarTab::Files.label(), "Files");
}

#[test]
fn sidebar_tab_all_has_five() {
    assert_eq!(SidebarTab::ALL.len(), 5);
}

// ---------------------------------------------------------------------------
// T073: Overview metadata in SidebarMeta
// ---------------------------------------------------------------------------

#[test]
fn sidebar_meta_line_count_with_all_fields() {
    use gh_board::components::sidebar::SidebarMeta;
    use iocraft::Color;

    let meta = SidebarMeta {
        pill_icon: String::new(),
        pill_text: "Open".into(),
        pill_bg: Color::Green,
        pill_fg: Color::White,
        pill_left: String::new(),
        pill_right: String::new(),
        branch_text: "main <- feat".into(),
        branch_fg: Color::White,
        update_text: None,
        update_fg: Color::White,
        author_login: "user".into(),
        role_icon: String::new(),
        role_text: String::new(),
        role_fg: Color::White,
        label_fg: Color::White,
        participants: vec!["@a".into(), "@b".into()],
        participants_fg: Color::White,
        labels_text: Some("bug".into()),
        assignees_text: Some("assignee1".into()),
        created_text: "2026-01-01 00:00:00".into(),
        created_age: "1d".into(),
        updated_text: "2026-01-02 00:00:00".into(),
        updated_age: "12h".into(),
        lines_added: Some("+10".into()),
        lines_deleted: Some("-5".into()),
        reactions_text: None,
        date_fg: Color::White,
        date_age_fg: Color::White,
        additions_fg: Color::Green,
        deletions_fg: Color::Red,
        separator_fg: Color::DarkGrey,
        primary_fg: Color::White,
        actor_fg: Color::White,
        reactions_fg: Color::White,
    };

    // pill(3: margin+pill+author) + participants(1) + overview(4: margin+created+updated+sep)
    // + labels(1) + assignees(1) + lines(1) = 11
    assert_eq!(meta.line_count(), 11);
}

#[test]
fn sidebar_meta_line_count_minimal() {
    use gh_board::components::sidebar::SidebarMeta;
    use iocraft::Color;

    let meta = SidebarMeta {
        pill_icon: String::new(),
        pill_text: "Open".into(),
        pill_bg: Color::Green,
        pill_fg: Color::White,
        pill_left: String::new(),
        pill_right: String::new(),
        branch_text: String::new(),
        branch_fg: Color::White,
        update_text: None,
        update_fg: Color::White,
        author_login: "user".into(),
        role_icon: String::new(),
        role_text: String::new(),
        role_fg: Color::White,
        label_fg: Color::White,
        participants: vec![],
        participants_fg: Color::White,
        labels_text: None,
        assignees_text: None,
        created_text: "2026-01-01 00:00:00".into(),
        created_age: "1d".into(),
        updated_text: "2026-01-02 00:00:00".into(),
        updated_age: "12h".into(),
        lines_added: None,
        lines_deleted: None,
        reactions_text: None,
        date_fg: Color::White,
        date_age_fg: Color::White,
        additions_fg: Color::Green,
        deletions_fg: Color::Red,
        separator_fg: Color::DarkGrey,
        primary_fg: Color::White,
        actor_fg: Color::White,
        reactions_fg: Color::White,
    };

    // pill(3: margin+pill+author) + overview(4: margin+created+updated+sep) = 7
    assert_eq!(meta.line_count(), 7);
}

// ---------------------------------------------------------------------------
// T074: Activity tab tests
// ---------------------------------------------------------------------------

#[test]
fn activity_renders_timeline_events() {
    let detail = test_detail();
    let theme = test_theme();
    let lines = sidebar_tabs::render_activity(&detail, &theme, ColorDepth::TrueColor);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("commenter"), "should show comment author");
    assert!(text.contains("commented"), "should show 'commented' action");
    assert!(text.contains("reviewer"), "should show reviewer");
    assert!(text.contains("approved"), "should show 'approved' action");
    assert!(text.contains("merger"), "should show merger actor");
    assert!(text.contains("merged"), "should show 'merged' action");
}

#[test]
fn activity_empty_shows_placeholder() {
    let detail = PrDetail {
        body: String::new(),
        reviews: Vec::new(),
        review_threads: Vec::new(),
        timeline_events: Vec::new(),
        commits: Vec::new(),
        files: Vec::new(),
        mergeable: None,
        behind_by: None,
    };
    let theme = test_theme();
    let lines = sidebar_tabs::render_activity(&detail, &theme, ColorDepth::TrueColor);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("no timeline events"));
}

// ---------------------------------------------------------------------------
// T075: Commits tab tests
// ---------------------------------------------------------------------------

#[test]
fn commits_renders_sha_and_message() {
    let detail = test_detail();
    let theme = test_theme();
    let lines = sidebar_tabs::render_commits(&detail, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("abc1234"), "should show short SHA");
    assert!(
        text.contains("Initial commit"),
        "should show commit message"
    );
    assert!(text.contains("Author One"), "should show author name");
    assert!(text.contains("def456"), "should show second commit SHA");
}

#[test]
fn commits_empty_shows_placeholder() {
    let detail = PrDetail {
        body: String::new(),
        reviews: Vec::new(),
        review_threads: Vec::new(),
        timeline_events: Vec::new(),
        commits: Vec::new(),
        files: Vec::new(),
        mergeable: None,
        behind_by: None,
    };
    let theme = test_theme();
    let lines = sidebar_tabs::render_commits(&detail, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("no commits"));
}

// ---------------------------------------------------------------------------
// T076: Checks tab tests
// ---------------------------------------------------------------------------

#[test]
fn checks_renders_status_icons() {
    let pr = test_pr();
    let theme = test_theme();
    let lines = sidebar_tabs::render_checks(&pr, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("CI Build"), "should show check name");
    assert!(text.contains("Lint"), "should show second check name");
    // ✔ for success, ✖ for failure
    assert!(text.contains('\u{2714}'), "should show success icon");
    assert!(text.contains('\u{2716}'), "should show failure icon");
}

#[test]
fn checks_empty_shows_placeholder() {
    let mut pr = test_pr();
    pr.check_runs.clear();
    let theme = test_theme();
    let lines = sidebar_tabs::render_checks(&pr, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("no checks"));
}

// ---------------------------------------------------------------------------
// T077: Files Changed tab tests
// ---------------------------------------------------------------------------

#[test]
fn files_renders_paths_and_stats() {
    let detail = test_detail();
    let theme = test_theme();
    let lines = sidebar_tabs::render_files(&detail, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("src/main.rs"), "should show file path");
    assert!(text.contains("src/new.rs"), "should show added file");
    assert!(text.contains("M "), "should show Modified marker");
    assert!(text.contains("A "), "should show Added marker");
    assert!(text.contains("+10 -3"), "should show change stats");
}

#[test]
fn files_empty_shows_placeholder() {
    let detail = PrDetail {
        body: String::new(),
        reviews: Vec::new(),
        review_threads: Vec::new(),
        timeline_events: Vec::new(),
        commits: Vec::new(),
        files: Vec::new(),
        mergeable: None,
        behind_by: None,
    };
    let theme = test_theme();
    let lines = sidebar_tabs::render_files(&detail, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("no files changed"));
}

// ---------------------------------------------------------------------------
// Checks: workflow grouping + duration
// ---------------------------------------------------------------------------

#[test]
fn checks_grouped_by_workflow() {
    use chrono::{Duration, Utc};

    let mut pr = test_pr();
    let now = Utc::now();
    pr.check_runs = vec![
        CheckRun {
            name: "build".to_owned(),
            status: Some(CheckStatus::Completed),
            conclusion: Some(CheckConclusion::Success),
            url: None,
            workflow_run_id: Some(1),
            workflow_name: Some("CI".to_owned()),
            started_at: Some(now - Duration::seconds(90)),
            completed_at: Some(now),
        },
        CheckRun {
            name: "test".to_owned(),
            status: Some(CheckStatus::Completed),
            conclusion: Some(CheckConclusion::Success),
            url: None,
            workflow_run_id: Some(1),
            workflow_name: Some("CI".to_owned()),
            started_at: Some(now - Duration::seconds(45)),
            completed_at: Some(now),
        },
        CheckRun {
            name: "deploy".to_owned(),
            status: Some(CheckStatus::Completed),
            conclusion: Some(CheckConclusion::Success),
            url: None,
            workflow_run_id: Some(2),
            workflow_name: Some("Deploy".to_owned()),
            started_at: Some(now - Duration::seconds(5)),
            completed_at: Some(now),
        },
        CheckRun {
            name: "external-check".to_owned(),
            status: Some(CheckStatus::Completed),
            conclusion: Some(CheckConclusion::Success),
            url: None,
            workflow_run_id: None,
            workflow_name: None,
            started_at: None,
            completed_at: None,
        },
    ];

    let theme = test_theme();
    let lines = sidebar_tabs::render_checks(&pr, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();

    // Group headers appear
    assert!(text.contains("CI"), "should contain CI workflow header");
    assert!(
        text.contains("Deploy"),
        "should contain Deploy workflow header"
    );
    assert!(
        text.contains("(other)"),
        "should contain (other) group header for workflow_name=None"
    );

    // Durations appear for completed checks with timestamps
    assert!(text.contains("1m 30s"), "build should show 1m 30s");
    assert!(text.contains("45s"), "test should show 45s");
    assert!(text.contains("5s"), "deploy should show 5s");

    // Check names appear
    assert!(text.contains("build"));
    assert!(text.contains("test"));
    assert!(text.contains("deploy"));
    assert!(text.contains("external-check"));

    // Blank lines separate groups (blank = all spans empty or whitespace-only)
    let blank_count = lines
        .iter()
        .filter(|l| l.spans.is_empty() || l.spans.iter().all(|s| s.text.trim().is_empty()))
        .count();
    assert!(
        blank_count >= 2,
        "expected at least 2 blank lines between 3 groups, got {blank_count}"
    );

    // Durations are globally aligned: the padding before each duration should
    // account for the longest name across all groups ("external-check" = 14 chars)
    // so shorter names get more padding. Verify "build" line has more spaces
    // before "1m 30s" than "external-check" line would.
    for line in &lines {
        let line_text: String = line.spans.iter().map(|s| s.text.as_str()).collect();
        if line_text.contains("build") && line_text.contains("1m 30s") {
            // "build" is 5 chars, "external-check" is 14 chars → 9 extra spaces of padding
            // plus the base 2 = 11 spaces between "build" and "1m 30s"
            let idx = line_text.find("1m 30s").unwrap();
            let before_dur = &line_text[..idx];
            let trailing_spaces = before_dur.len() - before_dur.trim_end().len();
            assert!(
                trailing_spaces >= 5,
                "expected global alignment padding, got {trailing_spaces} spaces before duration"
            );
        }
    }
}

#[test]
fn checks_with_no_workflow_all_in_other_group() {
    let pr = test_pr(); // all check_runs have workflow_name: None
    let theme = test_theme();
    let lines = sidebar_tabs::render_checks(&pr, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();

    // Single (other) group header
    assert!(text.contains("(other)"));
    assert!(text.contains("CI Build"));
    assert!(text.contains("Lint"));
}
