use gh_board::color::ColorDepth;
use gh_board::components::sidebar::SidebarTab;
use gh_board::components::sidebar_tabs;
use gh_board::github::graphql::PrDetail;
use gh_board::github::types::{
    Actor, CheckConclusion, CheckRun, CheckStatus, Commit, File, FileChangeType, PrState,
    PullRequest, Review, ReviewState, TimelineEvent,
};
use gh_board::theme::ResolvedTheme;

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
        labels: vec![gh_board::github::types::Label {
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
            },
            CheckRun {
                name: "Lint".to_owned(),
                status: Some(CheckStatus::Completed),
                conclusion: Some(CheckConclusion::Failure),
                url: None,
            },
        ],
        updated_at: Utc::now(),
        created_at: Utc::now(),
        url: "https://github.com/owner/repo/pull/42".to_owned(),
        repo: Some(gh_board::github::types::RepoRef {
            owner: "owner".to_owned(),
            name: "repo".to_owned(),
        }),
        comment_count: 3,
        author_association: None,
        participants: Vec::new(),
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
            },
            Commit {
                sha: "def456".to_owned(),
                message: "Fix bug".to_owned(),
                author: None,
                committed_date: None,
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
// T073: Overview tab tests
// ---------------------------------------------------------------------------

#[test]
fn overview_metadata_includes_labels() {
    let pr = test_pr();
    let theme = test_theme();
    let lines = sidebar_tabs::render_overview_metadata(&pr, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("bug"), "should contain label name");
}

#[test]
fn overview_metadata_includes_lines() {
    let pr = test_pr();
    let theme = test_theme();
    let lines = sidebar_tabs::render_overview_metadata(&pr, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(text.contains("+10"), "should contain additions");
    assert!(text.contains("-5"), "should contain deletions");
}

#[test]
fn overview_metadata_excludes_author_state_branch() {
    // Author, State, and Branch are now in SidebarMeta, not in overview metadata.
    let pr = test_pr();
    let theme = test_theme();
    let lines = sidebar_tabs::render_overview_metadata(&pr, &theme);
    let text: String = lines
        .iter()
        .flat_map(|l| l.spans.iter())
        .map(|s| s.text.as_str())
        .collect();
    assert!(!text.contains("Author:"), "Author moved to SidebarMeta");
    assert!(!text.contains("State:"), "State moved to SidebarMeta");
    assert!(!text.contains("Branch:"), "Branch moved to SidebarMeta");
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
