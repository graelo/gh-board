use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::builtin_themes;
use crate::config::keybindings::KeybindingsConfig;
use crate::config::types::{AppConfig, Defaults, GitHubConfig, PreviewDefaults, Theme};

/// Wrapper used to parse a theme-only TOML file (contains only `[theme.*]`).
#[derive(Deserialize, Default)]
struct ThemeFile {
    #[serde(default)]
    theme: Theme,
}

/// Discover and load the app config.
///
/// Priority (each layer overrides the previous):
/// 1. Global: `$GH_BOARD_CONFIG` → `$XDG_CONFIG_HOME/gh-board/config.toml`
///    → `~/.config/gh-board/config.toml`
/// 2. Ancestor directories: walking up from the Git root toward `$HOME`,
///    each `gh-board.toml` or `.gh-board.toml` found in a parent directory
///    is merged on top of the previous layer (farthest ancestor first).
/// 3. Project: `gh-board.toml` or `.gh-board.toml` at the Git repository root.
///
/// `--config` flag bypasses all discovery and loads only the given file.
///
/// Layers are merged recursively: local values override global for the same
/// key; missing local keys fall through. Filter lists replace the previous
/// layer only when non-empty; `repo_paths` are merged (closer entries override
/// matching keys from farther layers).
pub fn load_config(explicit_path: Option<&Path>) -> Result<AppConfig> {
    // If an explicit path was given, just load that.
    if let Some(path) = explicit_path {
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut config: AppConfig = toml::from_str(&contents)
            .with_context(|| format!("parsing TOML from {}", path.display()))?;
        config.repo_paths = expand_repo_paths(std::mem::take(&mut config.repo_paths));
        apply_theme_file(&mut config)?;
        return Ok(config);
    }

    // Start from the global config (or defaults).
    let mut config = match find_global_config() {
        Some(path) => {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            toml::from_str(&contents)
                .with_context(|| format!("parsing TOML from {}", path.display()))?
        }
        None => AppConfig::default(),
    };

    // Fold ancestor and project configs on top (farthest ancestor first, project last).
    let local_chain = find_local_config_chain();
    for path in &local_chain {
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let layer: AppConfig = toml::from_str(&contents)
            .with_context(|| format!("parsing TOML from {}", path.display()))?;
        config = merge_configs(config, layer);
    }

    config.repo_paths = expand_repo_paths(std::mem::take(&mut config.repo_paths));
    apply_theme_file(&mut config)?;
    Ok(config)
}

/// If `config.theme_file` is set, load and merge it as the base theme.
///
/// Inline `[theme.*]` in the config always wins over the file theme.
fn apply_theme_file(config: &mut AppConfig) -> Result<()> {
    let Some(ref theme_file) = config.theme_file.clone() else {
        return Ok(());
    };

    let file_theme: ThemeFile = if let Some(name) = theme_file.strip_prefix("builtin:") {
        let toml_src = builtin_themes::get(name).with_context(|| {
            let names = builtin_themes::list().join(", ");
            format!("unknown built-in theme {name:?}; available: {names}")
        })?;
        toml::from_str(toml_src).with_context(|| format!("parsing theme file {theme_file:?}"))?
    } else {
        let path = expand_tilde(theme_file);
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("reading theme file {}", path.display()))?;
        toml::from_str(&contents).with_context(|| format!("parsing theme file {theme_file:?}"))?
    };

    // The file provides the base; inline [theme.*] is the overlay.
    let inline = std::mem::take(&mut config.theme);
    config.theme = Theme::merge(file_theme.theme, inline);
    Ok(())
}

/// Merge repo-local config on top of global config.
///
/// Filter lists (`pr_filters`, `issues_filters`, `notifications_filters`) from
/// local replace global entirely when non-empty. Other sections are merged
/// recursively: local values override global values for the same key, while
/// missing keys in local config fall back to global config. This applies to:
/// - `github` config fields
/// - `defaults` fields
/// - `theme` (using `Theme::merge`)
/// - `keybindings` (merged by context: universal, prs, issues, actions, branches)
///
/// Repo paths are merged (local entries override matching global keys).
fn merge_configs(global: AppConfig, local: AppConfig) -> AppConfig {
    AppConfig {
        pr_filters: if local.pr_filters.is_empty() {
            global.pr_filters
        } else {
            local.pr_filters
        },
        issues_filters: if local.issues_filters.is_empty() {
            global.issues_filters
        } else {
            local.issues_filters
        },
        actions_filters: if local.actions_filters.is_empty() {
            global.actions_filters
        } else {
            local.actions_filters
        },
        notifications_filters: if local.notifications_filters.is_empty() {
            global.notifications_filters
        } else {
            local.notifications_filters
        },
        alerts_filters: if local.alerts_filters.is_empty() {
            global.alerts_filters
        } else {
            local.alerts_filters
        },
        github: merge_github_config(&global.github, &local.github),
        defaults: merge_defaults(&global.defaults, &local.defaults),
        theme: Theme::merge(global.theme, local.theme),
        keybindings: KeybindingsConfig::merge(&global.keybindings, &local.keybindings),
        repo_paths: {
            let mut paths = global.repo_paths;
            paths.extend(local.repo_paths);
            paths
        },
        theme_file: local.theme_file.or(global.theme_file),
        actions: merge_actions_config(&global.actions, &local.actions),
    }
}

/// Merge two Actions configs, with local values overriding global.
fn merge_actions_config(
    global: &crate::config::types::ActionsConfig,
    local: &crate::config::types::ActionsConfig,
) -> crate::config::types::ActionsConfig {
    crate::config::types::ActionsConfig {
        watch_poll_interval_seconds: local
            .watch_poll_interval_seconds
            .or(global.watch_poll_interval_seconds),
        watch_complete_command: local
            .watch_complete_command
            .clone()
            .or_else(|| global.watch_complete_command.clone()),
    }
}

/// Merge two GitHub configs, with local values overriding global.
fn merge_github_config(global: &GitHubConfig, local: &GitHubConfig) -> GitHubConfig {
    GitHubConfig {
        scope: local.scope.or(global.scope),
        refetch_interval_minutes: local
            .refetch_interval_minutes
            .or(global.refetch_interval_minutes),
        prefetch_pr_details: local.prefetch_pr_details.or(global.prefetch_pr_details),
        auto_clone: local.auto_clone.or(global.auto_clone),
    }
}

/// Merge two Defaults configs, with local values overriding global.
fn merge_defaults(global: &Defaults, local: &Defaults) -> Defaults {
    Defaults {
        view: local.view.or(global.view),
        preview: PreviewDefaults {
            width: local.preview.width.or(global.preview.width),
        },
        date_format: local
            .date_format
            .clone()
            .or_else(|| global.date_format.clone()),
    }
}

fn find_local_config_chain() -> Vec<PathBuf> {
    let Some(cwd) = std::env::current_dir().ok() else {
        return Vec::new();
    };
    find_local_config_chain_from(cwd)
}

/// Build the ordered chain of local config files to merge.
///
/// Returns configs ordered farthest-ancestor-first, project-last, so they can
/// be folded left-to-right with `merge_configs`.
///
/// 1. Walk up from `start` to find the Git root.
/// 2. Check for a project-level config at the Git root.
/// 3. Continue walking up from the Git root's parent toward `$HOME`, collecting
///    ancestor configs.
/// 4. Return ancestor configs (farthest first) followed by the project config.
fn find_local_config_chain_from(start: PathBuf) -> Vec<PathBuf> {
    let home = dirs_fallback();

    // Phase 1: walk up to the Git root.
    let mut dir = start;
    let git_root = loop {
        if dir.join(".git").exists() {
            break Some(dir.clone());
        }
        if !dir.pop() {
            break None;
        }
    };

    // Phase 2: project-level config at the Git root.
    let project_config = git_root.as_ref().and_then(|root| find_config_in(root));

    // Phase 3: ancestor configs above the Git root, stopping at $HOME (inclusive).
    let mut ancestor_configs = Vec::new();
    if let Some(ref root) = git_root {
        let mut parent = root.clone();
        while parent.pop() {
            if let Some(cfg) = find_config_in(&parent) {
                ancestor_configs.push(cfg);
            }
            // Stop once we've checked $HOME.
            if home.as_ref().is_some_and(|h| &parent == h) {
                break;
            }
        }
    }

    // Farthest ancestor first, project last.
    ancestor_configs.reverse();
    ancestor_configs.extend(project_config);
    ancestor_configs
}

/// Look for `gh-board.toml` (preferred) or `.gh-board.toml` in `dir`.
fn find_config_in(dir: &Path) -> Option<PathBuf> {
    for name in &["gh-board.toml", ".gh-board.toml"] {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_global_config() -> Option<PathBuf> {
    // $GH_BOARD_CONFIG
    if let Ok(path) = std::env::var("GH_BOARD_CONFIG") {
        let p = PathBuf::from(&path);
        if p.is_file() {
            return Some(p);
        }
    }

    // $XDG_CONFIG_HOME/gh-board/config.toml
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg).join("gh-board/config.toml");
        if p.is_file() {
            return Some(p);
        }
    }

    // ~/.config/gh-board/config.toml
    if let Some(home) = dirs_fallback() {
        let p = home.join(".config/gh-board/config.toml");
        if p.is_file() {
            return Some(p);
        }
    }

    None
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs_fallback()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

fn expand_repo_paths(
    paths: indexmap::IndexMap<String, PathBuf>,
) -> indexmap::IndexMap<String, PathBuf> {
    paths
        .into_iter()
        .map(|(k, v)| {
            let expanded = v.to_str().map(std::borrow::ToOwned::to_owned);
            let expanded = match expanded {
                Some(s) => expand_tilde(&s),
                None => v,
            };
            (k, expanded)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::keybindings::Keybinding;
    use crate::config::types::IconConfig;

    #[test]
    fn merge_configs_preserves_global_theme_with_empty_local() {
        let mut global = AppConfig::default();
        global.theme.icons.preset = Some("nerdfont".to_string());

        let local = AppConfig::default(); // Empty local config

        let merged = merge_configs(global, local);
        assert_eq!(merged.theme.icons.preset, Some("nerdfont".to_string()));
    }

    #[test]
    fn merge_configs_local_overrides_global_theme_preset() {
        let global = AppConfig {
            theme: Theme {
                icons: IconConfig {
                    preset: Some("nerdfont".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let local = AppConfig {
            theme: Theme {
                icons: IconConfig {
                    preset: Some("unicode".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let merged = merge_configs(global, local);
        assert_eq!(merged.theme.icons.preset, Some("unicode".to_string()));
    }

    #[test]
    fn merge_configs_preserves_global_keybindings_with_empty_local() {
        let mut global = AppConfig::default();
        global.keybindings.universal.push(Keybinding {
            key: "test".to_string(),
            builtin: Some("quit".to_string()),
            command: None,
            name: Some("Test".to_string()),
        });

        let local = AppConfig::default(); // Empty local config

        let merged = merge_configs(global, local);
        // Should have default universal bindings plus the test binding
        assert!(merged.keybindings.universal.iter().any(|b| b.key == "test"));
        assert!(merged.keybindings.universal.iter().any(|b| b.key == "q")); // Default quit
    }

    #[test]
    fn merge_configs_overrides_keybindings() {
        let mut global = AppConfig::default();
        global.keybindings.universal.push(Keybinding {
            key: "j".to_string(),
            builtin: Some("move_down".to_string()),
            command: None,
            name: Some("Move Down".to_string()),
        });

        let mut local = AppConfig::default();
        local.keybindings.universal.push(Keybinding {
            key: "j".to_string(),
            builtin: Some("first".to_string()),
            command: None,
            name: Some("First".to_string()),
        });

        let merged = merge_configs(global, local);
        // j should be overridden to "first"
        let j_binding = merged
            .keybindings
            .universal
            .iter()
            .find(|b| b.key == "j")
            .expect("Should have j binding");
        assert_eq!(j_binding.builtin, Some("first".to_string()));
    }

    #[test]
    fn merge_configs_merges_repo_paths() {
        let mut global = AppConfig::default();
        global
            .repo_paths
            .insert("org/repo1".to_string(), PathBuf::from("/tmp/repo1"));

        let mut local = AppConfig::default();
        local
            .repo_paths
            .insert("org/repo2".to_string(), PathBuf::from("/tmp/repo2"));

        let merged = merge_configs(global, local);
        assert_eq!(merged.repo_paths.len(), 2);
        assert!(merged.repo_paths.contains_key("org/repo1"));
        assert!(merged.repo_paths.contains_key("org/repo2"));
    }

    #[test]
    fn merge_configs_overrides_repo_paths() {
        let mut global = AppConfig::default();
        global
            .repo_paths
            .insert("org/repo".to_string(), PathBuf::from("/tmp/global"));

        let mut local = AppConfig::default();
        local
            .repo_paths
            .insert("org/repo".to_string(), PathBuf::from("/tmp/local"));

        let merged = merge_configs(global, local);
        assert_eq!(merged.repo_paths.len(), 1);
        assert_eq!(
            merged.repo_paths.get("org/repo").unwrap(),
            &PathBuf::from("/tmp/local")
        );
    }

    #[test]
    fn merge_configs_preserves_global_defaults_with_empty_local() {
        let mut global = AppConfig::default();
        global.defaults.view = Some(crate::config::types::View::Issues);

        let local = AppConfig::default(); // Empty local config

        let merged = merge_configs(global, local);
        assert_eq!(
            merged.defaults.view,
            Some(crate::config::types::View::Issues)
        );
    }

    #[test]
    fn merge_configs_overrides_defaults() {
        let mut global = AppConfig::default();
        global.defaults.view = Some(crate::config::types::View::Actions);

        let mut local = AppConfig::default();
        local.defaults.view = Some(crate::config::types::View::Issues);

        let merged = merge_configs(global, local);
        assert_eq!(
            merged.defaults.view,
            Some(crate::config::types::View::Issues)
        );
    }

    #[test]
    fn merge_configs_preserves_global_github_config_with_empty_local() {
        let mut global = AppConfig::default();
        global.github.auto_clone = Some(true);

        let local = AppConfig::default(); // Empty local config

        let merged = merge_configs(global, local);
        assert_eq!(merged.github.auto_clone, Some(true));
    }

    #[test]
    fn merge_configs_overrides_github_config() {
        let mut global = AppConfig::default();
        global.github.auto_clone = Some(false);

        let mut local = AppConfig::default();
        local.github.auto_clone = Some(true);

        let merged = merge_configs(global, local);
        assert_eq!(merged.github.auto_clone, Some(true));
    }

    #[test]
    fn merge_configs_local_filters_replace_global() {
        let mut global = AppConfig::default();
        global.pr_filters.push(crate::config::types::PrFilter {
            title: "Global Filter".to_string(),
            filters: "is:open author:@me".to_string(),
            limit: Some(50),
            host: None,
            layout: None,
        });

        let mut local = AppConfig::default();
        local.pr_filters.push(crate::config::types::PrFilter {
            title: "Local Filter".to_string(),
            filters: "is:open review-requested:@me".to_string(),
            limit: Some(30),
            host: None,
            layout: None,
        });

        let merged = merge_configs(global, local);
        assert_eq!(merged.pr_filters.len(), 1);
        assert_eq!(merged.pr_filters[0].title, "Local Filter");
    }

    #[test]
    fn merge_configs_empty_local_filters_use_global() {
        let mut global = AppConfig::default();
        global.pr_filters.push(crate::config::types::PrFilter {
            title: "Global Filter".to_string(),
            filters: "is:open author:@me".to_string(),
            limit: Some(50),
            host: None,
            layout: None,
        });

        let local = AppConfig::default(); // Empty filters

        let merged = merge_configs(global, local);
        assert_eq!(merged.pr_filters.len(), 1);
        assert_eq!(merged.pr_filters[0].title, "Global Filter");
    }

    #[test]
    fn merge_configs_theme_file_from_local() {
        let global = AppConfig::default();

        let local = AppConfig {
            theme_file: Some("builtin:catppuccin-mocha".to_string()),
            ..Default::default()
        };

        let merged = merge_configs(global, local);
        assert_eq!(
            merged.theme_file,
            Some("builtin:catppuccin-mocha".to_string())
        );
    }

    #[test]
    fn merge_configs_local_repo_paths_only_preserves_global_theme() {
        // This test simulates the issue: user has global config with nerdfont icons,
        // but only sets repo_paths in local .gh-board.toml
        let global = AppConfig {
            theme: Theme {
                icons: IconConfig {
                    preset: Some("nerdfont".to_string()),
                    ..Default::default()
                },
                ..Default::default()
            },
            ..Default::default()
        };

        let mut local = AppConfig::default();
        // Local config only has repo_paths, no theme settings
        local
            .repo_paths
            .insert("FreeCAD/FreeCAD".to_string(), PathBuf::from("/tmp/freecad"));

        let merged = merge_configs(global, local);
        // Theme should be preserved from global
        assert_eq!(merged.theme.icons.preset, Some("nerdfont".to_string()));
        // Repo paths should be merged
        assert_eq!(merged.repo_paths.len(), 1);
        assert!(merged.repo_paths.contains_key("FreeCAD/FreeCAD"));
    }

    #[test]
    fn merge_configs_local_repo_paths_only_preserves_global_keybindings() {
        // This test simulates the issue: user has custom keybindings in global config,
        // but only sets repo_paths in local .gh-board.toml
        let mut global = AppConfig::default();
        // Add a custom keybinding to global config
        global.keybindings.universal.push(Keybinding {
            key: "ctrl+shift+j".to_string(),
            builtin: Some("move_down".to_string()),
            command: None,
            name: Some("Custom Move Down".to_string()),
        });

        let mut local = AppConfig::default();
        // Local config only has repo_paths, no keybinding overrides
        local
            .repo_paths
            .insert("org/repo".to_string(), PathBuf::from("/tmp/repo"));

        let merged = merge_configs(global, local);
        // Custom global keybinding should be preserved
        assert!(
            merged
                .keybindings
                .universal
                .iter()
                .any(|b| b.key == "ctrl+shift+j")
        );
        // Default keybindings should also be present
        assert!(merged.keybindings.universal.iter().any(|b| b.key == "j"));
        // Repo paths should be merged
        assert_eq!(merged.repo_paths.len(), 1);
    }

    #[test]
    fn merge_configs_local_repo_paths_only_preserves_global_theme_file() {
        // This test simulates the issue: user has theme_file in global config,
        // but only sets repo_paths in local .gh-board.toml
        let global = AppConfig {
            theme_file: Some("builtin:catppuccin-mocha".to_string()),
            ..Default::default()
        };

        let mut local = AppConfig::default();
        // Local config only has repo_paths, no theme_file override
        local
            .repo_paths
            .insert("org/repo".to_string(), PathBuf::from("/tmp/repo"));

        let merged = merge_configs(global, local);
        // Theme file should be preserved from global
        assert_eq!(
            merged.theme_file,
            Some("builtin:catppuccin-mocha".to_string())
        );
        // Repo paths should be merged
        assert_eq!(merged.repo_paths.len(), 1);
    }

    #[test]
    fn config_chain_prefers_gh_board_toml_over_dot() {
        let temp_dir = tempfile::tempdir().unwrap();
        let git_dir = temp_dir.path().join(".git");
        std::fs::create_dir(&git_dir).unwrap();

        std::fs::write(
            temp_dir.path().join("gh-board.toml"),
            "defaults.view = \"prs\"",
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join(".gh-board.toml"),
            "defaults.view = \"issues\"",
        )
        .unwrap();

        let chain = find_local_config_chain_from(temp_dir.path().to_path_buf());
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].file_name().unwrap(), "gh-board.toml");
    }

    #[test]
    fn config_chain_only_at_git_root_not_subdir() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp_dir.path().join(".git")).unwrap();

        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        std::fs::write(subdir.join("gh-board.toml"), "defaults.view = \"prs\"").unwrap();

        let chain = find_local_config_chain_from(subdir);
        assert!(chain.is_empty());
    }

    #[test]
    fn config_chain_finds_at_git_root_from_subdir() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp_dir.path().join(".git")).unwrap();
        std::fs::write(
            temp_dir.path().join("gh-board.toml"),
            "defaults.view = \"prs\"",
        )
        .unwrap();

        let subdir = temp_dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();

        let chain = find_local_config_chain_from(subdir);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].file_name().unwrap(), "gh-board.toml");
    }

    #[test]
    fn config_chain_includes_ancestor_above_git_root() {
        // Layout: parent/.gh-board.toml + parent/repo/.git + parent/repo/gh-board.toml
        let temp_dir = tempfile::tempdir().unwrap();
        let parent = temp_dir.path().join("parent");
        let repo = parent.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();

        std::fs::write(parent.join(".gh-board.toml"), "[github]\nscope = 1").unwrap();
        std::fs::write(repo.join("gh-board.toml"), "[github]\nscope = 2").unwrap();

        let chain = find_local_config_chain_from(repo);
        assert_eq!(chain.len(), 2);
        // Farthest ancestor first
        assert_eq!(chain[0].parent().unwrap().file_name().unwrap(), "parent");
        // Project last
        assert_eq!(chain[1].file_name().unwrap(), "gh-board.toml");
    }

    #[test]
    fn config_chain_multiple_ancestors_ordered_farthest_first() {
        // Layout: a/.gh-board.toml > a/b/.gh-board.toml > a/b/c/.git + gh-board.toml
        let temp_dir = tempfile::tempdir().unwrap();
        let a = temp_dir.path().join("a");
        let b = a.join("b");
        let c = b.join("c");
        std::fs::create_dir_all(c.join(".git")).unwrap();

        std::fs::write(a.join(".gh-board.toml"), "").unwrap();
        std::fs::write(b.join(".gh-board.toml"), "").unwrap();
        std::fs::write(c.join("gh-board.toml"), "").unwrap();

        let chain = find_local_config_chain_from(c.clone());
        assert_eq!(chain.len(), 3);
        assert_eq!(chain[0].parent().unwrap().file_name().unwrap(), "a");
        assert_eq!(chain[1].parent().unwrap().file_name().unwrap(), "b");
        assert_eq!(chain[2].parent().unwrap().file_name().unwrap(), "c");
    }

    #[test]
    fn config_chain_no_ancestor_config_returns_project_only() {
        let temp_dir = tempfile::tempdir().unwrap();
        let repo = temp_dir.path().join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join("gh-board.toml"), "").unwrap();

        let chain = find_local_config_chain_from(repo.clone());
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].parent().unwrap().file_name().unwrap(), "repo");
    }

    #[test]
    fn config_chain_ancestor_only_no_project_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let parent = temp_dir.path().join("parent");
        let repo = parent.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();

        std::fs::write(parent.join(".gh-board.toml"), "").unwrap();
        // No config at repo level.

        let chain = find_local_config_chain_from(repo);
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].parent().unwrap().file_name().unwrap(), "parent");
    }
}
