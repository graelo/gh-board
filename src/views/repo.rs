use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use iocraft::prelude::*;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{NavigationTarget, ViewKind};
use crate::color::ColorDepth;
use crate::components::footer::{self, ActionFeedback, Footer, RenderedFooter};
use crate::components::help_overlay::{HelpOverlay, HelpOverlayBuildConfig, RenderedHelpOverlay};
use crate::components::sidebar::{RenderedSidebar, Sidebar, SidebarTab};
use crate::components::tab_bar::{RenderedTabBar, Tab, TabBar};
use crate::components::table::{
    Cell, Column, RenderedTable, Row, ScrollableTable, TableBuildConfig,
};
use crate::config::keybindings::{
    BuiltinAction, MergedBindings, ResolvedBinding, TemplateVars, ViewContext,
    execute_shell_command, expand_template, key_event_to_string,
};
use crate::config::types::PrFilter;
use crate::engine::{EngineHandle, Event};
use crate::icons::ResolvedIcons;
use crate::markdown::renderer::{StyledLine, StyledSpan};
use crate::theme::ResolvedTheme;
use crate::types::{PullRequest, RateLimitInfo};

/// Sidebar tabs available for branches (subset of `SidebarTab`).
const BRANCH_TABS: &[SidebarTab] = &[SidebarTab::Overview, SidebarTab::Commits, SidebarTab::Files];

// ---------------------------------------------------------------------------
// T079: Branch type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Branch {
    name: String,
    is_current: bool,
    last_commit_message: String,
    last_updated: Option<DateTime<Utc>>,
    ahead: u32,
    behind: u32,
    worktree_path: Option<PathBuf>,
    repo_label: String,
}

/// File changed on a branch vs its merge-base.
#[derive(Debug, Clone)]
struct BranchFile {
    path: String,
    status: char,
    additions: u32,
    deletions: u32,
}

/// Recent commit info for the sidebar Commits tab.
#[derive(Debug, Clone)]
struct BranchCommit {
    short_sha: String,
    message: String,
    author: String,
    date: String,
}

// ---------------------------------------------------------------------------
// T078: Local Git operations
// ---------------------------------------------------------------------------

/// Discover git worktrees and map branch names to their worktree paths.
fn list_worktrees(repo_path: &Path) -> HashMap<String, PathBuf> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return HashMap::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut result = HashMap::new();
    let mut current_path: Option<PathBuf> = None;
    let mut is_bare = false;

    for line in stdout.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path_str));
            is_bare = false;
        } else if line == "bare" {
            is_bare = true;
        } else if let Some(branch_ref) = line.strip_prefix("branch ")
            && !is_bare
            && let Some(branch_name) = branch_ref.strip_prefix("refs/heads/")
            && let Some(ref path) = current_path
        {
            result.insert(branch_name.to_owned(), path.clone());
        } else if line.is_empty() {
            current_path = None;
            is_bare = false;
        }
    }

    result
}

/// Fetch recent commits for a branch.
fn get_recent_commits(repo_path: &Path, branch: &str, count: usize) -> Vec<BranchCommit> {
    let output = Command::new("git")
        .args([
            "log",
            "--format=%h|%s|%an|%ar",
            "-n",
            &count.to_string(),
            branch,
        ])
        .current_dir(repo_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() < 4 {
                return None;
            }
            Some(BranchCommit {
                short_sha: parts[0].to_owned(),
                message: parts[1].to_owned(),
                author: parts[2].to_owned(),
                date: parts[3].to_owned(),
            })
        })
        .collect()
}

/// Find the merge-base for a branch vs the trunk, using `detect_default_branch()`
/// to dynamically resolve the default branch name.
fn find_merge_base(repo_path: &Path, branch: &str) -> Option<String> {
    let default = detect_default_branch(repo_path);
    let trunk_ref = format!("origin/{default}");
    let output = Command::new("git")
        .args(["merge-base", &trunk_ref, branch])
        .current_dir(repo_path)
        .output();
    if let Ok(o) = output
        && o.status.success()
    {
        let sha = String::from_utf8_lossy(&o.stdout).trim().to_owned();
        if !sha.is_empty() {
            return Some(sha);
        }
    }
    None
}

/// Fetch files changed on a branch vs its merge-base.
fn get_branch_files(repo_path: &Path, branch: &str) -> Vec<BranchFile> {
    let Some(base) = find_merge_base(repo_path, branch) else {
        return Vec::new();
    };

    let range = format!("{base}..{branch}");

    // git diff --numstat <base>..<branch> → adds\tdels\tpath
    let numstat_output = Command::new("git")
        .args(["diff", "--numstat", &range])
        .current_dir(repo_path)
        .output();
    let numstat_output = match numstat_output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    // git diff --name-status <base>..<branch> → status\tpath
    let status_output = Command::new("git")
        .args(["diff", "--name-status", &range])
        .current_dir(repo_path)
        .output();
    let status_output = match status_output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    // Build status map: path -> status char.
    // Renames/copies emit 3 fields (e.g. "R100\told\tnew"), so use splitn(3).
    let status_text = String::from_utf8_lossy(&status_output.stdout);
    let mut status_map = HashMap::new();
    for line in status_text.lines() {
        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() >= 2 {
            let ch = parts[0].chars().next().unwrap_or('?');
            // For renames (3 parts), key on the new path; otherwise key on the only path.
            let key = if parts.len() == 3 { parts[2] } else { parts[1] };
            status_map.insert(key.to_owned(), ch);
        }
    }

    // Parse numstat and zip with status
    let numstat_text = String::from_utf8_lossy(&numstat_output.stdout);
    numstat_text
        .lines()
        .filter(|l| !l.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() < 3 {
                return None;
            }
            let additions = parts[0].parse::<u32>().unwrap_or(0);
            let deletions = parts[1].parse::<u32>().unwrap_or(0);
            let path = parts[2].to_owned();
            let status = status_map.get(&path).copied().unwrap_or('?');
            Some(BranchFile {
                path,
                status,
                additions,
                deletions,
            })
        })
        .collect()
}

/// List local branches with metadata.
fn list_branches(repo_path: &Path, repo_label: &str) -> Vec<Branch> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(HEAD)|%(refname:short)|%(subject)|%(committerdate:iso8601)",
            "refs/heads",
        ])
        .current_dir(repo_path)
        .output();

    let output = match output {
        Ok(o) if o.status.success() => o,
        _ => return Vec::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let default_branch = detect_default_branch(repo_path);
    let origin_default = format!("origin/{default_branch}");
    let worktrees = list_worktrees(repo_path);

    stdout
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(4, '|').collect();
            if parts.len() < 4 {
                return None;
            }
            let is_current = parts[0].trim() == "*";
            let name = parts[1].to_owned();
            let last_commit_message = parts[2].to_owned();
            let last_updated = DateTime::parse_from_str(parts[3].trim(), "%Y-%m-%d %H:%M:%S %z")
                .ok()
                .map(|dt| dt.with_timezone(&Utc));

            let (ahead, behind) = if name == default_branch {
                (0, 0)
            } else {
                get_ahead_behind(repo_path, &name, &origin_default)
            };

            let worktree_path = worktrees.get(&name).cloned();

            Some(Branch {
                name,
                is_current,
                last_commit_message,
                last_updated,
                ahead,
                behind,
                worktree_path,
                repo_label: repo_label.to_owned(),
            })
        })
        .collect()
}

/// Collect branches from CWD and all configured `repo_paths`.
fn list_all_branches(
    cwd_path: Option<&Path>,
    cwd_label: &str,
    repo_paths: Option<&HashMap<String, PathBuf>>,
) -> Vec<Branch> {
    let mut branches = Vec::new();

    if let Some(cwd) = cwd_path {
        branches.extend(list_branches(cwd, cwd_label));
    }

    if let Some(paths) = repo_paths {
        for (label, path) in paths {
            // Skip entries whose path matches CWD to avoid duplicates.
            if let Some(cwd) = cwd_path
                && path == cwd
            {
                continue;
            }
            branches.extend(list_branches(path, label));
        }
    }

    // Sort: repo_label ascending, then worktree branches first, then most
    // recent first (descending date), then name ascending as final tiebreaker.
    branches.sort_by(|a, b| {
        a.repo_label
            .cmp(&b.repo_label)
            .then_with(|| {
                let a_wt = a.worktree_path.is_some();
                let b_wt = b.worktree_path.is_some();
                b_wt.cmp(&a_wt) // true > false, so reverse for "worktree first"
            })
            .then_with(|| b.last_updated.cmp(&a.last_updated)) // most recent first
            .then_with(|| a.name.cmp(&b.name))
    });

    branches
}

fn detect_default_branch(repo_path: &Path) -> String {
    // Try symbolic-ref for HEAD's upstream default
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD", "--short"])
        .current_dir(repo_path)
        .output();
    if let Ok(o) = output
        && o.status.success()
    {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_owned();
        // "origin/main" → "main"
        if let Some(branch) = s.strip_prefix("origin/") {
            return branch.to_owned();
        }
    }
    "main".to_owned()
}

fn get_ahead_behind(repo_path: &Path, branch: &str, base: &str) -> (u32, u32) {
    let output = Command::new("git")
        .args([
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base}...{branch}"),
        ])
        .current_dir(repo_path)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            let parts: Vec<&str> = s.trim().split('\t').collect();
            if parts.len() == 2 {
                let behind = parts[0].parse().unwrap_or(0);
                let ahead = parts[1].parse().unwrap_or(0);
                (ahead, behind)
            } else {
                (0, 0)
            }
        }
        _ => (0, 0),
    }
}

// ---------------------------------------------------------------------------
// T081: Branch actions
// ---------------------------------------------------------------------------

fn delete_branch(repo_path: &Path, branch: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["branch", "-d", branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!("Deleted branch {branch}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn create_branch(repo_path: &Path, name: &str, from: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["branch", name, from])
        .current_dir(repo_path)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!("Created branch {name} from {from}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn checkout_branch(repo_path: &Path, branch: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args(["checkout", branch])
        .current_dir(repo_path)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(format!("Switched to {branch}"))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

// ---------------------------------------------------------------------------
// T080: Table columns and row conversion
// ---------------------------------------------------------------------------

fn branch_columns(icons: &ResolvedIcons, multi_repo: bool) -> Vec<Column> {
    let mut cols = vec![Column {
        id: "current".to_owned(),
        header: " ".to_owned(),
        default_width_pct: 0.03,
        align: TextAlign::Center,
        fixed_width: Some(3),
    }];

    if multi_repo {
        cols.push(Column {
            id: "repo".to_owned(),
            header: "Repo".to_owned(),
            default_width_pct: 0.14,
            align: TextAlign::Left,
            fixed_width: None,
        });
    }

    let (name_pct, message_pct) = if multi_repo {
        (0.14, 0.18)
    } else {
        (0.18, 0.26)
    };

    cols.extend([
        Column {
            id: "name".to_owned(),
            header: "Branch".to_owned(),
            default_width_pct: name_pct,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "pr".to_owned(),
            header: "PR".to_owned(),
            default_width_pct: 0.06,
            align: TextAlign::Left,
            fixed_width: Some(7),
        },
        Column {
            id: "worktree".to_owned(),
            header: "Worktree".to_owned(),
            default_width_pct: 0.15,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "ahead_behind".to_owned(),
            header: format!("{}/{}", icons.branch_ahead, icons.branch_behind),
            default_width_pct: 0.10,
            align: TextAlign::Center,
            fixed_width: Some(10),
        },
        Column {
            id: "message".to_owned(),
            header: "Last Commit".to_owned(),
            default_width_pct: message_pct,
            align: TextAlign::Left,
            fixed_width: None,
        },
        Column {
            id: "updated".to_owned(),
            header: "Updated".to_owned(),
            default_width_pct: 0.12,
            align: TextAlign::Right,
            fixed_width: Some(8),
        },
    ]);

    cols
}

/// Get the branch at `idx` in the scope-filtered list.
fn filtered_branch_at(
    state: &State<Vec<Branch>>,
    scope: Option<&str>,
    idx: usize,
) -> Option<Branch> {
    let all = state.read();
    match scope {
        Some(s) => all.iter().filter(|b| b.repo_label == s).nth(idx).cloned(),
        None => all.get(idx).cloned(),
    }
}

/// Composite key for the PR map: `"{repo_label}\0{branch_name}"`.
fn pr_map_key(repo_label: &str, branch_name: &str) -> String {
    format!("{repo_label}\0{branch_name}")
}

fn branch_to_row(
    branch: &Branch,
    theme: &ResolvedTheme,
    date_format: &str,
    pr_map: &HashMap<String, PullRequest>,
) -> Row {
    let mut row = HashMap::new();

    let marker = if branch.is_current { "*" } else { " " };
    let marker_color = if branch.is_current {
        theme.text_success
    } else {
        theme.text_faint
    };
    row.insert("current".to_owned(), Cell::colored(marker, marker_color));

    let name_color = if branch.is_current {
        theme.text_success
    } else {
        theme.text_primary
    };
    row.insert("name".to_owned(), Cell::colored(&branch.name, name_color));

    row.insert(
        "repo".to_owned(),
        Cell::colored(&branch.repo_label, theme.text_faint),
    );

    let (pr_text, pr_color) = match pr_map.get(&pr_map_key(&branch.repo_label, &branch.name)) {
        Some(pr) => (format!("#{}", pr.number), theme.text_success),
        None => (String::new(), theme.text_faint),
    };
    row.insert("pr".to_owned(), Cell::colored(pr_text, pr_color));

    row.insert(
        "message".to_owned(),
        Cell::colored(&branch.last_commit_message, theme.text_secondary),
    );

    let worktree_label = branch
        .worktree_path
        .as_ref()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("");
    row.insert(
        "worktree".to_owned(),
        Cell::colored(worktree_label, theme.text_faint),
    );

    let icons = &theme.icons;
    let ab_text = if branch.ahead == 0 && branch.behind == 0 {
        String::new()
    } else {
        format!(
            "{}{} {}{}",
            icons.branch_ahead, branch.ahead, icons.branch_behind, branch.behind
        )
    };
    row.insert(
        "ahead_behind".to_owned(),
        Cell::colored(ab_text, theme.text_faint),
    );

    let updated = branch
        .last_updated
        .as_ref()
        .map(|dt| crate::util::format_date(dt, date_format))
        .unwrap_or_default();
    row.insert(
        "updated".to_owned(),
        Cell::colored(updated, theme.text_faint),
    );

    row
}

// ---------------------------------------------------------------------------
// Input mode for branch actions (T081)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum InputMode {
    Normal,
    ConfirmDelete,
    ConfirmWorktree,
    CreateBranch,
}

// ---------------------------------------------------------------------------
// T080/T082: RepoView component
// ---------------------------------------------------------------------------

#[derive(Default, Props)]
pub struct RepoViewProps<'a> {
    pub theme: Option<&'a ResolvedTheme>,
    /// Merged keybindings for help overlay.
    pub keybindings: Option<&'a MergedBindings>,
    pub color_depth: ColorDepth,
    pub width: u16,
    pub height: u16,
    pub preview_width_pct: f64,
    pub show_separator: bool,
    pub should_exit: Option<State<bool>>,
    pub switch_view: Option<State<bool>>,
    /// Signal to switch to the previous view.
    pub switch_view_back: Option<State<bool>>,
    /// Signal to toggle repo scope.
    pub scope_toggle: Option<State<bool>>,
    /// Active scope repo (e.g. `"owner/repo"`), or `None` for global.
    pub scope_repo: Option<String>,
    pub repo_path: Option<&'a std::path::Path>,
    /// Detected repo (owner/name) from CWD remote.
    pub detected_repo: Option<&'a crate::types::common::RepoRef>,
    /// Configured `repo_paths` mapping `"owner/repo"` to local paths.
    pub repo_paths: Option<&'a HashMap<String, PathBuf>>,
    /// Engine handle for async PR data fetching (optional).
    pub engine: Option<&'a EngineHandle>,
    /// Navigation target state for cross-view deep-linking.
    pub nav_target: Option<State<Option<NavigationTarget>>>,
    pub date_format: Option<&'a str>,
    /// Whether this view is the currently active (visible) one.
    pub is_active: bool,
    /// Auto-refetch interval in minutes (0 = disabled).
    pub refetch_interval_minutes: u32,
    /// Shared rate-limit state (owned by App).
    pub rate_limit: Option<State<Option<RateLimitInfo>>>,
}

#[component]
#[allow(clippy::too_many_lines)]
pub fn RepoView<'a>(props: &RepoViewProps<'a>, mut hooks: Hooks) -> impl Into<AnyElement<'a>> {
    let theme = props.theme.cloned().unwrap_or_else(default_theme);
    let depth = props.color_depth;
    let should_exit = props.should_exit;
    let switch_view = props.switch_view;
    let switch_view_back = props.switch_view_back;
    let scope_toggle = props.scope_toggle;
    let scope_repo = &props.scope_repo;
    let detected_repo = props.detected_repo.cloned();
    let nav_target = props.nav_target;
    let date_format = props.date_format.unwrap_or("relative");
    let is_active = props.is_active;

    let preview_pct = if props.preview_width_pct > 0.0 {
        props.preview_width_pct
    } else {
        0.45
    };

    let mut cursor = hooks.use_state(|| 0usize);
    let mut scroll_offset = hooks.use_state(|| 0usize);
    let mut input_mode = hooks.use_state(|| InputMode::Normal);
    let mut input_buffer = hooks.use_state(String::new);
    let mut action_status = hooks.use_state(|| Option::<ActionFeedback>::None);
    let mut status_set_at = hooks.use_state(|| Option::<std::time::Instant>::None);
    let mut help_visible = hooks.use_state(|| false);

    // Sidebar state.
    let mut preview_open = hooks.use_state(|| false);
    let mut preview_scroll = hooks.use_state(|| 0usize);
    let mut sidebar_tab = hooks.use_state(|| SidebarTab::Overview);
    // Cache of recent commits per branch name.
    let mut commits_cache = hooks.use_state(HashMap::<String, Vec<BranchCommit>>::new);
    // Cache of changed files per branch name.
    let mut files_cache = hooks.use_state(HashMap::<String, Vec<BranchFile>>::new);

    // State: last fetch time (for status bar).
    let mut last_fetch_time = hooks.use_state(|| Option::<std::time::Instant>::None);

    // Load branches.
    let mut branches_state = hooks.use_state(Vec::<Branch>::new);
    let mut loaded = hooks.use_state(|| false);

    // PR indicator data: maps "{repo_label}\0{branch_name}" -> PullRequest.
    let mut pr_map = hooks.use_state(HashMap::<String, PullRequest>::new);
    let mut pr_repos_fetched = hooks.use_state(HashSet::<String>::new);

    // Rate-limit info from engine responses.
    let fallback_rl = hooks.use_state(|| None);
    let mut rate_limit_state = props.rate_limit.unwrap_or(fallback_rl);

    // Per-view event channel for engine replies.
    let event_channel = hooks.use_state(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Event>();
        (tx, Arc::new(Mutex::new(rx)))
    });
    let (event_tx, event_rx_arc) = event_channel.read().clone();

    // Poll engine events (PR data).
    {
        let rx_for_poll = event_rx_arc.clone();
        hooks.use_future(async move {
            loop {
                smol::Timer::after(std::time::Duration::from_millis(100)).await;
                let rx = rx_for_poll.lock().unwrap();
                while let Ok(ev) = rx.try_recv() {
                    if let Event::PrsFetched {
                        prs, rate_limit, ..
                    } = ev
                    {
                        if rate_limit.is_some() {
                            rate_limit_state.set(rate_limit);
                        }
                        let mut map = pr_map.read().clone();
                        for pr in prs {
                            if let Some(repo_ref) = &pr.repo {
                                let label = format!("{}/{}", repo_ref.owner, repo_ref.name);
                                let key = pr_map_key(&label, &pr.head_ref);
                                let dominated = map
                                    .get(&key)
                                    .is_some_and(|existing| existing.updated_at >= pr.updated_at);
                                if !dominated {
                                    map.insert(key, pr);
                                }
                            }
                        }
                        pr_map.set(map);
                    }
                }
            }
        });
    }

    // Timer tick for periodic re-renders (supports auto-refetch).
    let mut tick = hooks.use_state(|| 0u64);
    hooks.use_future(async move {
        loop {
            smol::Timer::after(std::time::Duration::from_secs(60)).await;
            tick.set(tick.get() + 1);
        }
    });

    // Auto-clear action status after 60 seconds.
    {
        hooks.use_future(async move {
            loop {
                smol::Timer::after(std::time::Duration::from_secs(1)).await;
                if let Some(t) = status_set_at.get()
                    && t.elapsed().as_secs() >= 60
                {
                    action_status.set(None);
                    status_set_at.set(None);
                }
            }
        });
    }

    // Auto-refetch if interval has elapsed (only for already-visited views).
    let refetch_interval = props.refetch_interval_minutes;
    if loaded.get()
        && is_active
        && refetch_interval > 0
        && let Some(last) = last_fetch_time.get()
        && last.elapsed() >= std::time::Duration::from_secs(u64::from(refetch_interval) * 60)
    {
        loaded.set(false);
        pr_repos_fetched.set(HashSet::new());
        pr_map.set(HashMap::new());
    }

    // Compute CWD repo label.
    let cwd_label = detected_repo.as_ref().map_or_else(
        || {
            props
                .repo_path
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("local")
                .to_owned()
        },
        crate::types::common::RepoRef::full_name,
    );

    let multi_repo = props.repo_paths.is_some_and(|m| !m.is_empty());

    if !loaded.get() && is_active {
        loaded.set(true);
        branches_state.set(list_all_branches(
            props.repo_path,
            &cwd_label,
            props.repo_paths,
        ));
        last_fetch_time.set(Some(std::time::Instant::now()));

        // Fetch open PRs for each distinct repo label.
        if let Some(engine) = props.engine {
            let repos: HashSet<String> = branches_state
                .read()
                .iter()
                .map(|b| b.repo_label.clone())
                .collect();
            let repos_to_fetch: Vec<&String> = if let Some(scope) = scope_repo {
                repos.iter().filter(|r| *r == scope).collect()
            } else {
                repos.iter().collect()
            };
            let fetched = pr_repos_fetched.read();
            for repo_label in repos_to_fetch {
                if fetched.contains(repo_label) {
                    continue;
                }
                let filter = PrFilter {
                    title: repo_label.clone(),
                    filters: format!("repo:{repo_label} is:pr is:open"),
                    host: None,
                    limit: Some(50),
                    layout: None,
                };
                // filter_idx is ignored in the repo view polling loop;
                // all PR events are merged into pr_map by head_ref key.
                engine.send(crate::engine::Request::FetchPrs {
                    filter_idx: 0,
                    filter,
                    force: false,
                    reply_tx: event_tx.clone(),
                });
            }
            drop(fetched);
            pr_repos_fetched.set(repos);
        }
    }

    let branches = branches_state.read();

    // Apply scope filter.
    let branches: Vec<&Branch> = if let Some(scope) = scope_repo {
        branches.iter().filter(|b| b.repo_label == *scope).collect()
    } else {
        branches.iter().collect()
    };
    let total_rows = branches.len();
    let visible_rows = (props.height.saturating_sub(5) / 2).max(1) as usize;

    // Keyboard handling.
    let repo_path_owned = props.repo_path.map(std::borrow::ToOwned::to_owned);
    let keybindings = props.keybindings.cloned();
    let scope_repo_owned = scope_repo.clone();
    let cwd_label_owned = cwd_label.clone();
    let repo_paths_owned = props.repo_paths.cloned();
    hooks.use_terminal_events({
        move |event| match event {
            TerminalEvent::Key(KeyEvent {
                code,
                kind,
                modifiers,
                ..
            }) if kind != KeyEventKind::Release => {
                // Only process events when this view is active.
                if !is_active {
                    return;
                }
                // Help overlay: intercept all keys when visible.
                if help_visible.get() {
                    if matches!(code, KeyCode::Char('?') | KeyCode::Esc) {
                        help_visible.set(false);
                    }
                    return;
                }
                let current_mode = input_mode.read().clone();

                let reload = |state: &mut State<Vec<Branch>>| {
                    state.set(list_all_branches(
                        repo_path_owned.as_deref(),
                        &cwd_label_owned,
                        repo_paths_owned.as_ref(),
                    ));
                };

                match current_mode {
                    InputMode::ConfirmDelete => match code {
                        KeyCode::Char('y' | 'Y') => {
                            if let Some(ref repo_path) = repo_path_owned {
                                let branch_name = filtered_branch_at(&branches_state, scope_repo_owned.as_deref(), cursor.get())
                                    .map(|b| b.name.clone());
                                if let Some(name) = branch_name {
                                    match delete_branch(repo_path, &name) {
                                        Ok(msg) => {
                                            action_status.set(Some(ActionFeedback::Success(msg)));
                                            status_set_at.set(Some(std::time::Instant::now()));
                                            reload(&mut branches_state);
                                            let filtered_len = {
                                                let all = branches_state.read();
                                                match &scope_repo_owned {
                                                    Some(s) => all.iter().filter(|b| b.repo_label == *s).count(),
                                                    None => all.len(),
                                                }
                                            };
                                            if cursor.get() >= filtered_len {
                                                cursor.set(filtered_len.saturating_sub(1));
                                            }
                                        }
                                        Err(e) => {
                                            action_status.set(Some(ActionFeedback::Error(format!("Delete failed: {e}"))));
                                            status_set_at.set(Some(std::time::Instant::now()));
                                        }
                                    }
                                }
                            }
                            input_mode.set(InputMode::Normal);
                        }
                        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            action_status.set(Some(ActionFeedback::Info("Cancelled".to_owned())));
                            status_set_at.set(Some(std::time::Instant::now()));
                        }
                        _ => {}
                    },
                    InputMode::ConfirmWorktree => match code {
                        KeyCode::Char('y' | 'Y') => {
                            input_mode.set(InputMode::Normal);
                            if let Some(branch) = filtered_branch_at(
                                &branches_state, scope_repo_owned.as_deref(), cursor.get()
                            ) {
                                let path = if branch.repo_label == cwd_label_owned {
                                    repo_path_owned.clone()
                                } else {
                                    repo_paths_owned.as_ref()
                                        .and_then(|m| m.get(&branch.repo_label))
                                        .cloned()
                                };
                                if let Some(repo_path) = path {
                                    match crate::actions::local::create_worktree_at(&branch.name, &repo_path) {
                                        Ok(wt_path) => {
                                            let msg = match crate::actions::clipboard::copy_to_clipboard(&wt_path) {
                                                Ok(()) => format!("Worktree ready (copied): {wt_path}"),
                                                Err(e) => format!("Worktree ready: {wt_path} (clipboard: {e})"),
                                            };
                                            action_status.set(Some(ActionFeedback::Success(msg)));
                                            status_set_at.set(Some(std::time::Instant::now()));
                                            reload(&mut branches_state);
                                        }
                                        Err(e) => {
                                            action_status.set(Some(ActionFeedback::Error(
                                                format!("Worktree error: {e:#}")
                                            )));
                                            status_set_at.set(Some(std::time::Instant::now()));
                                        }
                                    }
                                }
                            }
                        }
                        KeyCode::Char('n' | 'N') | KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            action_status.set(None);
                        }
                        _ => {}
                    },
                    InputMode::CreateBranch => match code {
                        KeyCode::Enter => {
                            let name = input_buffer.read().clone();
                            if !name.is_empty()
                                && let Some(ref repo_path) = repo_path_owned
                            {
                                let from = filtered_branch_at(&branches_state, scope_repo_owned.as_deref(), cursor.get())
                                    .map_or_else(|| "HEAD".to_owned(), |b| b.name.clone());
                                match create_branch(repo_path, &name, &from) {
                                    Ok(msg) => {
                                        action_status.set(Some(ActionFeedback::Success(msg)));
                                        status_set_at.set(Some(std::time::Instant::now()));
                                        reload(&mut branches_state);
                                    }
                                    Err(e) => {
                                        action_status.set(Some(ActionFeedback::Error(format!("Create failed: {e}"))));
                                        status_set_at.set(Some(std::time::Instant::now()));
                                    }
                                }
                            }
                            input_mode.set(InputMode::Normal);
                            input_buffer.set(String::new());
                        }
                        KeyCode::Esc => {
                            input_mode.set(InputMode::Normal);
                            input_buffer.set(String::new());
                        }
                        KeyCode::Backspace => {
                            let mut buf = input_buffer.read().clone();
                            buf.pop();
                            input_buffer.set(buf);
                        }
                        KeyCode::Char(ch) if !modifiers.contains(KeyModifiers::CONTROL) => {
                            let mut buf = input_buffer.read().clone();
                            buf.push(ch);
                            input_buffer.set(buf);
                        }
                        _ => {}
                    },
                    InputMode::Normal => {
                        if let Some(key_str) = key_event_to_string(code, modifiers, kind) {
                            let current_branch = filtered_branch_at(&branches_state, scope_repo_owned.as_deref(), cursor.get())
                                .map(|b| b.name.clone())
                                .unwrap_or_default();
                            let vars = TemplateVars {
                                head_branch: current_branch.clone(),
                                ..Default::default()
                            };
                            match keybindings
                                .as_ref()
                                .and_then(|kb| kb.resolve(&key_str, ViewContext::Branches))
                            {
                                Some(ResolvedBinding::Builtin(action)) => match action {
                                    BuiltinAction::Quit => {
                                        if let Some(mut exit) = should_exit {
                                            exit.set(true);
                                        }
                                    }
                                    BuiltinAction::SwitchView => {
                                        if let Some(mut sv) = switch_view {
                                            sv.set(true);
                                        }
                                    }
                                    BuiltinAction::SwitchViewBack => {
                                        if let Some(mut sv) = switch_view_back {
                                            sv.set(true);
                                        }
                                    }
                                    BuiltinAction::ToggleScope => {
                                        if let Some(mut st) = scope_toggle {
                                            st.set(true);
                                        }
                                    }
                                    BuiltinAction::Checkout => {
                                        if let Some(ref repo_path) = repo_path_owned {
                                            let branch_name = filtered_branch_at(&branches_state, scope_repo_owned.as_deref(), cursor.get())
                                                .map(|b| b.name.clone());
                                            if let Some(name) = branch_name {
                                                match checkout_branch(repo_path, &name) {
                                                    Ok(msg) => {
                                                        action_status.set(Some(ActionFeedback::Success(msg)));
                                                        status_set_at.set(Some(std::time::Instant::now()));
                                                        reload(&mut branches_state);
                                                    }
                                                    Err(e) => {
                                                        action_status.set(Some(ActionFeedback::Error(format!(
                                                            "Checkout failed: {e}"
                                                        ))));
                                                        status_set_at.set(Some(std::time::Instant::now()));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    BuiltinAction::Worktree => {
                                        if let Some(branch) = filtered_branch_at(
                                            &branches_state, scope_repo_owned.as_deref(), cursor.get()
                                        ) {
                                            if branch.is_current {
                                                action_status.set(Some(ActionFeedback::Warning(
                                                    "Cannot create worktree for the checked-out branch".into()
                                                )));
                                                status_set_at.set(Some(std::time::Instant::now()));
                                            } else if let Some(ref wt) = branch.worktree_path {
                                                let path = wt.to_string_lossy().to_string();
                                                let msg = match crate::actions::clipboard::copy_to_clipboard(&path) {
                                                    Ok(()) => format!("Worktree exists (copied): {path}"),
                                                    Err(_) => format!("Worktree exists: {path}"),
                                                };
                                                action_status.set(Some(ActionFeedback::Info(msg)));
                                                status_set_at.set(Some(std::time::Instant::now()));
                                            } else {
                                                input_mode.set(InputMode::ConfirmWorktree);
                                                action_status.set(None);
                                            }
                                        }
                                    }
                                    BuiltinAction::DeleteBranch => {
                                        input_mode.set(InputMode::ConfirmDelete);
                                        action_status.set(None);
                                    }
                                    BuiltinAction::NewBranch => {
                                        input_mode.set(InputMode::CreateBranch);
                                        input_buffer.set(String::new());
                                        action_status.set(None);
                                    }
                                    BuiltinAction::TogglePreview => {
                                        preview_open.set(!preview_open.get());
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::MoveDown if total_rows > 0 => {
                                        let new_cursor =
                                            (cursor.get() + 1).min(total_rows.saturating_sub(1));
                                        cursor.set(new_cursor);
                                        if new_cursor >= scroll_offset.get() + visible_rows {
                                            scroll_offset
                                                .set(new_cursor.saturating_sub(visible_rows) + 1);
                                        }
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::MoveUp => {
                                        let new_cursor = cursor.get().saturating_sub(1);
                                        cursor.set(new_cursor);
                                        if new_cursor < scroll_offset.get() {
                                            scroll_offset.set(new_cursor);
                                        }
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::First => {
                                        cursor.set(0);
                                        scroll_offset.set(0);
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::Last if total_rows > 0 => {
                                        cursor.set(total_rows.saturating_sub(1));
                                        scroll_offset.set(total_rows.saturating_sub(visible_rows));
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::PageDown if total_rows > 0 => {
                                        let new_cursor = (cursor.get() + visible_rows)
                                            .min(total_rows.saturating_sub(1));
                                        cursor.set(new_cursor);
                                        scroll_offset.set(
                                            new_cursor
                                                .saturating_sub(visible_rows.saturating_sub(1)),
                                        );
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::PageUp => {
                                        let new_cursor = cursor.get().saturating_sub(visible_rows);
                                        cursor.set(new_cursor);
                                        scroll_offset
                                            .set(scroll_offset.get().saturating_sub(visible_rows));
                                        preview_scroll.set(0);
                                    }
                                    BuiltinAction::HalfPageDown => {
                                        let half = visible_rows / 2;
                                        if preview_open.get() {
                                            preview_scroll.set(preview_scroll.get() + half);
                                        } else if total_rows > 0 {
                                            let new_cursor = (cursor.get() + half)
                                                .min(total_rows.saturating_sub(1));
                                            cursor.set(new_cursor);
                                            if new_cursor >= scroll_offset.get() + visible_rows {
                                                scroll_offset.set(
                                                    new_cursor.saturating_sub(visible_rows) + 1,
                                                );
                                            }
                                            preview_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::HalfPageUp => {
                                        let half = visible_rows / 2;
                                        if preview_open.get() {
                                            preview_scroll
                                                .set(preview_scroll.get().saturating_sub(half));
                                        } else {
                                            let new_cursor = cursor.get().saturating_sub(half);
                                            cursor.set(new_cursor);
                                            if new_cursor < scroll_offset.get() {
                                                scroll_offset.set(new_cursor);
                                            }
                                            preview_scroll.set(0);
                                        }
                                    }
                                    BuiltinAction::RefreshItem
                                    | BuiltinAction::Refresh
                                    | BuiltinAction::RefreshAll => {
                                        loaded.set(false);
                                        commits_cache.set(HashMap::new());
                                        files_cache.set(HashMap::new());
                                        pr_repos_fetched.set(HashSet::new());
                                        pr_map.set(HashMap::new());
                                        action_status.set(None);
                                    }
                                    BuiltinAction::ToggleHelp => {
                                        help_visible.set(true);
                                    }
                                    BuiltinAction::CopyNumber | BuiltinAction::CopyUrl => {
                                        let _ = crate::actions::clipboard::copy_to_clipboard(
                                            &current_branch,
                                        );
                                    }
                                    BuiltinAction::CreatePrFromBranch => {
                                        if let Some(ref repo) = detected_repo {
                                            let url = format!(
                                                "https://github.com/{}/compare/{current_branch}?expand=1",
                                                repo.full_name(),
                                            );
                                            match crate::actions::clipboard::open_in_browser(&url) {
                                                Ok(()) => {
                                                    action_status.set(Some(ActionFeedback::Success(format!(
                                                        "Opened PR creation for {current_branch}"
                                                    ))));
                                                    status_set_at.set(Some(std::time::Instant::now()));
                                                }
                                                Err(e) => {
                                                    action_status.set(Some(ActionFeedback::Error(format!(
                                                        "Failed to open browser: {e}"
                                                    ))));
                                                    status_set_at.set(Some(std::time::Instant::now()));
                                                }
                                            }
                                        }
                                    }
                                    BuiltinAction::JumpToPr => {
                                        if let Some(branch) = filtered_branch_at(
                                            &branches_state,
                                            scope_repo_owned.as_deref(),
                                            cursor.get(),
                                        ) {
                                            let map = pr_map.read();
                                            if let Some(pr) = map.get(&pr_map_key(
                                                &branch.repo_label,
                                                &branch.name,
                                            ))
                                                && let Some(repo_ref) = &pr.repo
                                            {
                                                if let Some(mut nt) = nav_target {
                                                    nt.set(Some(NavigationTarget::PullRequest {
                                                        owner: repo_ref.owner.clone(),
                                                        repo: repo_ref.name.clone(),
                                                        number: pr.number,
                                                        host: None,
                                                    }));
                                                }
                                            } else {
                                                action_status.set(Some(ActionFeedback::Info(
                                                    "No open PR for this branch".to_owned(),
                                                )));
                                                status_set_at
                                                    .set(Some(std::time::Instant::now()));
                                            }
                                        }
                                    }
                                    BuiltinAction::ViewPrsForBranch => {
                                        if let Some(ref repo) = detected_repo {
                                            let url = format!(
                                                "https://github.com/{}/pulls?q=is%3Apr+head%3A{current_branch}",
                                                repo.full_name(),
                                            );
                                            match crate::actions::clipboard::open_in_browser(&url) {
                                                Ok(()) => {
                                                    action_status.set(Some(ActionFeedback::Success(format!(
                                                        "Opened PRs for {current_branch}"
                                                    ))));
                                                    status_set_at.set(Some(std::time::Instant::now()));
                                                }
                                                Err(e) => {
                                                    action_status.set(Some(ActionFeedback::Error(format!(
                                                        "Failed to open browser: {e}"
                                                    ))));
                                                    status_set_at.set(Some(std::time::Instant::now()));
                                                }
                                            }
                                        }
                                    }
                                    _ => {}
                                },
                                Some(ResolvedBinding::ShellCommand(cmd)) => {
                                    let expanded = expand_template(&cmd, &vars);
                                    let _ = execute_shell_command(&expanded);
                                }
                                None => {
                                    if key_str == "]" {
                                        let current = sidebar_tab.get();
                                        let idx = BRANCH_TABS.iter().position(|&t| t == current).unwrap_or(0);
                                        sidebar_tab.set(BRANCH_TABS[(idx + 1) % BRANCH_TABS.len()]);
                                        preview_scroll.set(0);
                                    } else if key_str == "[" {
                                        let current = sidebar_tab.get();
                                        let idx = BRANCH_TABS.iter().position(|&t| t == current).unwrap_or(0);
                                        sidebar_tab.set(BRANCH_TABS[if idx == 0 { BRANCH_TABS.len() - 1 } else { idx - 1 }]);
                                        preview_scroll.set(0);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    });

    // Skip heavy rendering for inactive views (all hooks above are unconditional).
    if !is_active {
        return element! {
            View(flex_direction: FlexDirection::Column)
        }
        .into_any();
    }

    // Compute widths for table vs sidebar.
    let is_preview_open = preview_open.get();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (table_width, sidebar_width) = if is_preview_open {
        let sb_w = (f64::from(props.width) * preview_pct).round() as u16;
        (props.width.saturating_sub(sb_w), sb_w)
    } else {
        (props.width, 0)
    };

    // Build table.
    let show_repo_col = multi_repo && scope_repo.is_none();
    let columns = branch_columns(&theme.icons, show_repo_col);
    let pr_map_read = pr_map.read();
    let rows: Vec<Row> = branches
        .iter()
        .map(|b| branch_to_row(b, &theme, date_format, &pr_map_read))
        .collect();

    let rendered_table = RenderedTable::build(&TableBuildConfig {
        columns: &columns,
        rows: &rows,
        cursor: cursor.get(),
        scroll_offset: scroll_offset.get(),
        visible_rows,
        hidden_columns: None,
        width_overrides: None,
        total_width: table_width,
        depth,
        selected_bg: Some(theme.bg_selected),
        header_color: Some(theme.text_secondary),
        border_color: Some(theme.border_faint),
        show_separator: props.show_separator,
        empty_message: Some("No branches found"),
        subtitle_column: None,
        row_separator: true,
        scrollbar_thumb_color: Some(theme.border_primary),
    });

    let tabs = vec![Tab {
        title: "Branches".to_owned(),
        count: Some(total_rows),
        is_ephemeral: false,
    }];
    let rendered_tab_bar = RenderedTabBar::build(
        &tabs,
        0,
        true,
        depth,
        Some(theme.footer_repo),
        Some(theme.footer_repo),
        Some(theme.border_faint),
        &theme.icons.tab_filter,
        &theme.icons.tab_ephemeral,
    );

    let current_mode = input_mode.read().clone();

    let rendered_text_input = match &current_mode {
        InputMode::CreateBranch => Some(crate::components::text_input::RenderedTextInput::build(
            "New branch name:",
            &input_buffer.read(),
            depth,
            Some(theme.text_primary),
            Some(theme.text_secondary),
            Some(theme.border_faint),
        )),
        InputMode::ConfirmDelete => {
            let branch_name = branches.get(cursor.get()).map_or("?", |b| b.name.as_str());
            let prompt = format!("Delete branch '{branch_name}'? (y/n)");
            Some(crate::components::text_input::RenderedTextInput::build(
                &prompt,
                "",
                depth,
                Some(theme.text_primary),
                Some(theme.text_warning),
                Some(theme.border_faint),
            ))
        }
        InputMode::ConfirmWorktree => {
            let branch_name = branches.get(cursor.get()).map_or("?", |b| b.name.as_str());
            let prompt = format!("Create worktree for '{branch_name}'? (y/n)");
            Some(crate::components::text_input::RenderedTextInput::build(
                &prompt,
                "",
                depth,
                Some(theme.text_primary),
                Some(theme.text_secondary),
                Some(theme.border_faint),
            ))
        }
        InputMode::Normal => None,
    };

    let context_text = {
        let cursor_pos = if total_rows > 0 { cursor.get() + 1 } else { 0 };
        format!("Branch {cursor_pos}/{total_rows}")
    };
    let updated_text = footer::format_updated_ago(last_fetch_time.get());

    let scope_label = match scope_repo {
        Some(repo) => repo.clone(),
        None => "all repos".to_owned(),
    };
    let rendered_footer = RenderedFooter::build(
        ViewKind::Repo,
        &theme.icons,
        scope_label,
        context_text,
        updated_text,
        footer::format_rate_limit(rate_limit_state.read().as_ref()),
        action_status.read().as_ref(),
        &theme,
        depth,
        [
            Some(theme.footer_prs),
            Some(theme.footer_issues),
            Some(theme.footer_actions),
            Some(theme.footer_notifications),
            Some(theme.footer_repo),
        ],
        Some(theme.text_faint),
        Some(theme.text_faint),
        Some(theme.border_faint),
    );

    // Pre-render sidebar.
    let rendered_sidebar = if is_preview_open {
        let current_branch = branches.get(cursor.get()).copied();
        let title = current_branch.map_or("Preview", |b| b.name.as_str());
        let current_tab = sidebar_tab.get();

        let md_lines: Vec<StyledLine> = match current_tab {
            SidebarTab::Overview => {
                if let Some(branch) = current_branch {
                    let def_branch = props
                        .repo_path
                        .map_or_else(|| "main".to_owned(), detect_default_branch);
                    render_branch_overview(branch, &theme, &pr_map_read, &def_branch)
                } else {
                    Vec::new()
                }
            }
            SidebarTab::Commits => {
                if let Some(branch) = current_branch {
                    // Lazy-fetch and cache commits (keyed by repo+branch for multi-repo).
                    let cache_key = pr_map_key(&branch.repo_label, &branch.name);
                    let cached = commits_cache.read();
                    let commits = if let Some(c) = cached.get(&cache_key) {
                        c.clone()
                    } else {
                        drop(cached);
                        let c = props
                            .repo_path
                            .map(|rp| get_recent_commits(rp, &branch.name, 20))
                            .unwrap_or_default();
                        let mut new_cache = commits_cache.read().clone();
                        new_cache.insert(cache_key, c.clone());
                        commits_cache.set(new_cache);
                        c
                    };
                    render_branch_commits(&commits, &theme)
                } else {
                    Vec::new()
                }
            }
            SidebarTab::Files => {
                if let Some(branch) = current_branch {
                    let cache_key = pr_map_key(&branch.repo_label, &branch.name);
                    let cached = files_cache.read();
                    let files = if let Some(f) = cached.get(&cache_key) {
                        f.clone()
                    } else {
                        drop(cached);
                        let f = props
                            .repo_path
                            .map(|rp| get_branch_files(rp, &branch.name))
                            .unwrap_or_default();
                        let mut new_cache = files_cache.read().clone();
                        new_cache.insert(cache_key, f.clone());
                        files_cache.set(new_cache);
                        f
                    };
                    render_branch_files(&files, &theme, sidebar_width)
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        };

        // Build tab label overrides for dynamic file count.
        let mut tab_overrides = HashMap::new();
        if let Some(branch) = current_branch {
            let cache_key = pr_map_key(&branch.repo_label, &branch.name);
            let cached = files_cache.read();
            if let Some(files) = cached.get(&cache_key) {
                let icon = &theme.icons.tab_files;
                tab_overrides.insert(SidebarTab::Files, format!("{icon} Files ({})", files.len()));
            }
        }

        #[allow(clippy::cast_possible_truncation)]
        let sidebar_visible_lines = props.height.saturating_sub(8) as usize;

        let tab_overrides_ref = if tab_overrides.is_empty() {
            None
        } else {
            Some(&tab_overrides)
        };

        let sidebar = RenderedSidebar::build_tabbed(
            title,
            &md_lines,
            preview_scroll.get(),
            sidebar_visible_lines,
            sidebar_width,
            depth,
            Some(theme.text_primary),
            Some(theme.border_faint),
            Some(theme.text_faint),
            Some(theme.border_primary),
            Some(current_tab),
            Some(&theme.icons),
            None,
            Some(BRANCH_TABS),
            tab_overrides_ref,
        );
        if preview_scroll.get() != sidebar.clamped_scroll {
            preview_scroll.set(sidebar.clamped_scroll);
        }
        Some(sidebar)
    } else {
        None
    };

    let rendered_help = if help_visible.get() {
        props.keybindings.map(|kb| {
            RenderedHelpOverlay::build(&HelpOverlayBuildConfig {
                bindings: kb,
                context: ViewContext::Branches,
                depth,
                title_color: Some(theme.text_primary),
                key_color: Some(theme.text_success),
                desc_color: Some(theme.text_secondary),
                border_color: Some(theme.border_primary),
            })
        })
    } else {
        None
    };

    let width = u32::from(props.width);
    let height = u32::from(props.height);

    element! {
        View(flex_direction: FlexDirection::Column, width, height) {
            TabBar(tab_bar: rendered_tab_bar)
            View(flex_grow: 1.0, flex_direction: FlexDirection::Row, overflow: Overflow::Hidden) {
                View(flex_grow: 1.0, flex_direction: FlexDirection::Column) {
                    ScrollableTable(table: rendered_table)
                }
                Sidebar(sidebar: rendered_sidebar)
            }
            crate::components::text_input::TextInput(input: rendered_text_input)
            Footer(footer: rendered_footer)
            HelpOverlay(overlay: rendered_help, width: props.width, height: props.height)
        }
    }
    .into_any()
}

/// Render the Overview tab for a branch sidebar.
fn render_branch_overview(
    branch: &Branch,
    theme: &ResolvedTheme,
    pr_map: &HashMap<String, PullRequest>,
    default_branch: &str,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();

    // Status badge
    let status = if branch.is_current {
        "* current"
    } else {
        "branch"
    };
    let status_color = if branch.is_current {
        theme.text_success
    } else {
        theme.text_secondary
    };
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::text(format!("[{status}]"), status_color),
        StyledSpan::text(format!(" {}", branch.name), theme.text_primary),
    ]));

    // Blank line
    lines.push(StyledLine::from_spans(vec![]));

    // PR info
    if let Some(pr) = pr_map.get(&pr_map_key(&branch.repo_label, &branch.name)) {
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("PR:       ", theme.text_secondary),
            StyledSpan::text(format!("#{}", pr.number), theme.text_success),
            StyledSpan::text(format!(" {}", pr.title), theme.text_primary),
        ]));
    }

    // Repo label
    if !branch.repo_label.is_empty() {
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("Repo:     ", theme.text_secondary),
            StyledSpan::text(&branch.repo_label, theme.text_secondary),
        ]));
    }

    // Worktree path (replace $HOME prefix with ~ for brevity)
    if let Some(ref wt) = branch.worktree_path {
        let display_path = if let Some(home) = std::env::var_os("HOME") {
            let home = Path::new(&home);
            wt.strip_prefix(home).map_or_else(
                |_| wt.display().to_string(),
                |rel| format!("~/{}", rel.display()),
            )
        } else {
            wt.display().to_string()
        };
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("Worktree: ", theme.text_secondary),
            StyledSpan::text(display_path, theme.text_secondary),
        ]));
    }

    // Tracking info
    let tracking = if branch.ahead == 0 && branch.behind == 0 {
        "Up to date".to_owned()
    } else {
        format!("↑{} ahead  ↓{} behind", branch.ahead, branch.behind)
    };
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold(format!("vs {default_branch}: "), theme.text_secondary),
        StyledSpan::text(tracking, theme.text_secondary),
    ]));

    // Last commit
    lines.push(StyledLine::from_spans(vec![
        StyledSpan::bold("Commit:   ", theme.text_secondary),
        StyledSpan::text(&branch.last_commit_message, theme.text_primary),
    ]));

    // Updated
    if let Some(ref dt) = branch.last_updated {
        let formatted = crate::util::format_date(dt, "relative");
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::bold("Updated:  ", theme.text_secondary),
            StyledSpan::text(formatted, theme.text_faint),
        ]));
    }

    lines
}

/// Render the Commits tab for a branch sidebar.
fn render_branch_commits(commits: &[BranchCommit], theme: &ResolvedTheme) -> Vec<StyledLine> {
    if commits.is_empty() {
        return vec![StyledLine::from_span(StyledSpan::text(
            "(no commits)",
            theme.text_faint,
        ))];
    }

    let mut lines = Vec::new();
    for c in commits {
        lines.push(StyledLine::from_spans(vec![
            StyledSpan::text(format!("{} ", c.short_sha), theme.text_warning),
            StyledSpan::text(crate::util::expand_emoji(&c.message), theme.text_primary),
        ]));
        if !c.author.is_empty() || !c.date.is_empty() {
            lines.push(StyledLine::from_spans(vec![
                StyledSpan::text(format!("        {}", c.author), theme.text_actor),
                StyledSpan::text(format!("  {}", c.date), theme.text_faint),
            ]));
        }
    }
    lines
}

fn render_branch_files(
    files: &[BranchFile],
    theme: &ResolvedTheme,
    sidebar_width: u16,
) -> Vec<StyledLine> {
    if files.is_empty() {
        return vec![StyledLine::from_span(StyledSpan::text(
            "(no files changed)",
            theme.text_faint,
        ))];
    }

    let content_width = usize::from(sidebar_width).saturating_sub(4).max(1);

    let max_add_width = files
        .iter()
        .map(|f| format!("+{}", f.additions).len())
        .max()
        .unwrap_or(2);
    let max_del_width = files
        .iter()
        .map(|f| format!("-{}", f.deletions).len())
        .max()
        .unwrap_or(2);

    let fixed_cols = 2 + 1 + max_add_width + 1 + max_del_width;
    let path_budget = content_width.saturating_sub(fixed_cols);

    let natural_max = files
        .iter()
        .map(|f| UnicodeWidthStr::width(f.path.as_str()))
        .max()
        .unwrap_or(0);
    let path_col_width = natural_max.min(path_budget);

    let mut lines = Vec::new();
    for file in files {
        let change = match file.status {
            'A' => "A",
            'D' => "D",
            'M' => "M",
            'R' => "R",
            'C' => "C",
            _ => "?",
        };
        let change_color = match file.status {
            'A' => theme.text_success,
            'D' => theme.text_error,
            _ => theme.text_warning,
        };

        let path_w = UnicodeWidthStr::width(file.path.as_str());
        let (display_path, display_w) = if path_w > path_col_width {
            truncate_path_with_ellipsis(&file.path, path_col_width)
        } else {
            (file.path.clone(), path_w)
        };
        let pad = path_col_width.saturating_sub(display_w) + 1;

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

/// Truncate a path to fit within `max_width` display columns, appending `…`.
fn truncate_path_with_ellipsis(s: &str, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }
    let target = max_width.saturating_sub(1); // 1 for `…`
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
    w += UnicodeWidthChar::width('…').unwrap_or(1);
    (buf, w)
}

fn default_theme() -> ResolvedTheme {
    super::default_theme()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> ResolvedTheme {
        default_theme()
    }

    fn sample_branch(name: &str, is_current: bool) -> Branch {
        Branch {
            name: name.to_owned(),
            is_current,
            last_commit_message: "test commit".to_owned(),
            last_updated: Some(chrono::Utc::now()),
            ahead: 2,
            behind: 1,
            worktree_path: None,
            repo_label: String::new(),
        }
    }

    fn empty_pr_map() -> HashMap<String, PullRequest> {
        HashMap::new()
    }

    #[test]
    fn branch_columns_single_repo_has_seven() {
        let theme = test_theme();
        let cols = branch_columns(&theme.icons, false);
        assert_eq!(cols.len(), 7);
        assert_eq!(cols[0].id, "current");
        assert_eq!(cols[1].id, "name");
        assert_eq!(cols[2].id, "pr");
        assert_eq!(cols[3].id, "worktree");
        assert_eq!(cols[4].id, "ahead_behind");
        assert_eq!(cols[5].id, "message");
        assert_eq!(cols[6].id, "updated");
    }

    #[test]
    fn branch_columns_multi_repo_has_eight() {
        let theme = test_theme();
        let cols = branch_columns(&theme.icons, true);
        assert_eq!(cols.len(), 8);
        assert_eq!(cols[0].id, "current");
        assert_eq!(cols[1].id, "repo");
        assert_eq!(cols[2].id, "name");
        assert_eq!(cols[3].id, "pr");
        assert_eq!(cols[4].id, "worktree");
        assert_eq!(cols[5].id, "ahead_behind");
        assert_eq!(cols[6].id, "message");
        assert_eq!(cols[7].id, "updated");
    }

    #[test]
    fn branch_to_row_repo_cell() {
        let theme = test_theme();
        let mut branch = sample_branch("main", true);
        branch.repo_label = "owner/repo".to_owned();
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("repo").unwrap().text(), "owner/repo");
    }

    #[test]
    fn branch_to_row_pr_cell_empty() {
        let theme = test_theme();
        let branch = sample_branch("main", true);
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("pr").unwrap().text(), "");
    }

    #[test]
    fn branch_to_row_current_marker() {
        let theme = test_theme();
        let branch = sample_branch("main", true);
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("current").unwrap().text(), "*");
    }

    #[test]
    fn branch_to_row_non_current_marker() {
        let theme = test_theme();
        let branch = sample_branch("feature", false);
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("current").unwrap().text(), " ");
    }

    #[test]
    fn branch_to_row_name() {
        let theme = test_theme();
        let branch = sample_branch("feature-xyz", false);
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("name").unwrap().text(), "feature-xyz");
    }

    #[test]
    fn branch_to_row_ahead_behind_nonzero() {
        let theme = test_theme();
        let branch = sample_branch("dev", false);
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        let ab = &row.get("ahead_behind").unwrap().text();
        assert!(ab.contains('2'), "should contain ahead count");
        assert!(ab.contains('1'), "should contain behind count");
    }

    #[test]
    fn branch_to_row_ahead_behind_zero() {
        let theme = test_theme();
        let mut branch = sample_branch("main", true);
        branch.ahead = 0;
        branch.behind = 0;
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("ahead_behind").unwrap().text(), "");
    }

    #[test]
    fn branch_to_row_commit_message() {
        let theme = test_theme();
        let mut branch = sample_branch("fix", false);
        branch.last_commit_message = "fix: resolve bug".to_owned();
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("message").unwrap().text(), "fix: resolve bug");
    }

    #[test]
    fn branch_to_row_worktree_cell() {
        let theme = test_theme();
        let mut branch = sample_branch("feat-x", false);
        branch.worktree_path = Some(PathBuf::from("/home/user/worktrees/feat-x"));
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("worktree").unwrap().text(), "feat-x");
    }

    #[test]
    fn branch_to_row_worktree_empty() {
        let theme = test_theme();
        let branch = sample_branch("main", true);
        let row = branch_to_row(&branch, &theme, "relative", &empty_pr_map());
        assert_eq!(row.get("worktree").unwrap().text(), "");
    }

    #[test]
    fn list_branches_nonexistent_path_returns_empty() {
        let branches = list_branches(Path::new("/nonexistent/path"), "test");
        assert!(branches.is_empty());
    }

    #[test]
    fn detect_default_branch_nonexistent_path_returns_main() {
        let default = detect_default_branch(Path::new("/nonexistent/path"));
        assert_eq!(default, "main");
    }

    #[test]
    fn get_ahead_behind_nonexistent_returns_zero() {
        let (ahead, behind) = get_ahead_behind(Path::new("/nonexistent/path"), "foo", "bar");
        assert_eq!(ahead, 0);
        assert_eq!(behind, 0);
    }

    #[test]
    fn delete_branch_nonexistent_path_returns_err() {
        let result = delete_branch(Path::new("/nonexistent/path"), "foo");
        assert!(result.is_err());
    }

    #[test]
    fn create_branch_nonexistent_path_returns_err() {
        let result = create_branch(Path::new("/nonexistent/path"), "foo", "HEAD");
        assert!(result.is_err());
    }

    #[test]
    fn checkout_branch_nonexistent_path_returns_err() {
        let result = checkout_branch(Path::new("/nonexistent/path"), "foo");
        assert!(result.is_err());
    }
}
