use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::builtin_themes;
use crate::config::types::{AppConfig, Defaults, GitHubConfig, PreviewDefaults, Theme};

/// Wrapper used to parse a theme-only TOML file (contains only `[theme.*]`).
#[derive(Deserialize, Default)]
struct ThemeFile {
    #[serde(default)]
    theme: Theme,
}

/// Discover and load the app config.
///
/// Priority:
/// 1. `--config` flag (explicit path)
/// 2. `.gh-board.toml` in the current Git repository root
/// 3. `$GH_BOARD_CONFIG` environment variable
/// 4. `$XDG_CONFIG_HOME/gh-board/config.toml`
/// 5. `~/.config/gh-board/config.toml`
///
/// If both a global and a repo-local config exist, repo-local sections replace
/// their global counterparts entirely (defaults, theme, keybindings are taken
/// from the local config; filter lists are replaced if non-empty; `repo_paths`
/// are merged). Users should duplicate all needed settings in repo-local configs.
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

    let global_path = find_global_config();
    let local_path = find_repo_local_config();

    let mut config = match (global_path, local_path) {
        (Some(global), Some(local)) => {
            // Parse global first, then overlay local.
            let global_str = std::fs::read_to_string(&global)
                .with_context(|| format!("reading {}", global.display()))?;
            let global_cfg: AppConfig = toml::from_str(&global_str)
                .with_context(|| format!("parsing TOML from {}", global.display()))?;

            let local_str = std::fs::read_to_string(&local)
                .with_context(|| format!("reading {}", local.display()))?;
            let local_cfg: AppConfig = toml::from_str(&local_str)
                .with_context(|| format!("parsing TOML from {}", local.display()))?;

            merge_configs(global_cfg, local_cfg)
        }
        (Some(path), None) | (None, Some(path)) => {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let config: AppConfig = toml::from_str(&contents)
                .with_context(|| format!("parsing TOML from {}", path.display()))?;
            config
        }
        (None, None) => {
            // No config found — use defaults.
            AppConfig::default()
        }
    };

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
        github: merge_github_config(&global.github, &local.github),
        defaults: merge_defaults(&global.defaults, &local.defaults),
        theme: Theme::merge(global.theme, local.theme),
        keybindings: merge_keybindings(&global.keybindings, &local.keybindings),
        repo_paths: {
            let mut paths = global.repo_paths;
            paths.extend(local.repo_paths);
            paths
        },
        theme_file: local.theme_file.or(global.theme_file),
    }
}

/// Merge two GitHub configs, with local values overriding global.
///
/// Fields are compared against their `Default` values; a non-default local
/// value wins, otherwise the global value is used. Note: because primitive
/// fields cannot distinguish "absent" from "explicitly set to default", a
/// local value that matches the default will fall through to global.
fn merge_github_config(global: &GitHubConfig, local: &GitHubConfig) -> GitHubConfig {
    let defaults = GitHubConfig::default();

    GitHubConfig {
        scope: if local.scope == defaults.scope {
            global.scope
        } else {
            local.scope
        },
        refetch_interval_minutes: if local.refetch_interval_minutes
            == defaults.refetch_interval_minutes
        {
            global.refetch_interval_minutes
        } else {
            local.refetch_interval_minutes
        },
        prefetch_pr_details: if local.prefetch_pr_details == defaults.prefetch_pr_details {
            global.prefetch_pr_details
        } else {
            local.prefetch_pr_details
        },
        // Bool fields cannot distinguish "absent" from "explicitly false",
        // so local can only turn this on, never override a global `true`.
        auto_clone: local.auto_clone || global.auto_clone,
    }
}

/// Merge two Defaults configs, with local values overriding global.
///
/// Same caveat as `merge_github_config`: primitive fields that match their
/// `Default` value are treated as "not set".
#[allow(clippy::float_cmp)] // TOML-parsed f64 vs compile-time constant: exact comparison is safe
fn merge_defaults(global: &Defaults, local: &Defaults) -> Defaults {
    let defaults = Defaults::default();

    Defaults {
        view: if local.view == defaults.view {
            global.view
        } else {
            local.view
        },
        preview: PreviewDefaults {
            width: if local.preview.width == defaults.preview.width {
                global.preview.width
            } else {
                local.preview.width
            },
        },
        date_format: if !local.date_format.is_empty() {
            local.date_format.clone()
        } else if !global.date_format.is_empty() {
            global.date_format.clone()
        } else {
            defaults.date_format
        },
    }
}

/// Merge two Keybindings configs by context.
/// Local bindings for a given key replace global bindings for that key within each context.
fn merge_keybindings(
    global: &crate::config::keybindings::KeybindingsConfig,
    local: &crate::config::keybindings::KeybindingsConfig,
) -> crate::config::keybindings::KeybindingsConfig {
    use std::collections::HashMap;

    use crate::config::keybindings::{
        Keybinding, default_actions, default_branches, default_issues, default_prs,
        default_universal, merge_lists,
    };

    // Helper to merge two binding lists, with local overriding global
    fn merge_binding_lists(global: &[Keybinding], local: &[Keybinding]) -> Vec<Keybinding> {
        let mut result = global.to_vec();

        // Create a map of local bindings by key for quick lookup
        let local_map: HashMap<&str, &Keybinding> =
            local.iter().map(|b| (b.key.as_str(), b)).collect();

        // Remove any global bindings that have local overrides
        result.retain(|b| !local_map.contains_key(b.key.as_str()));

        // Add all local bindings (they take precedence)
        result.extend(local.iter().cloned());

        result
    }

    // For each context, merge defaults with (global + local) overrides
    // This preserves custom global bindings while allowing local to override them
    crate::config::keybindings::KeybindingsConfig {
        universal: merge_lists(
            &default_universal(),
            &merge_binding_lists(&global.universal, &local.universal),
        ),
        prs: merge_lists(
            &default_prs(),
            &merge_binding_lists(&global.prs, &local.prs),
        ),
        issues: merge_lists(
            &default_issues(),
            &merge_binding_lists(&global.issues, &local.issues),
        ),
        actions: merge_lists(
            &default_actions(),
            &merge_binding_lists(&global.actions, &local.actions),
        ),
        branches: merge_lists(
            &default_branches(),
            &merge_binding_lists(&global.branches, &local.branches),
        ),
    }
}

fn find_repo_local_config() -> Option<PathBuf> {
    // Walk up from CWD looking for `.gh-board.toml` next to a `.git` directory.
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".gh-board.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            // Reached git root without finding config.
            return None;
        }
        if !dir.pop() {
            return None;
        }
    }
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
    paths: std::collections::HashMap<String, PathBuf>,
) -> std::collections::HashMap<String, PathBuf> {
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
    use crate::config::types::{AppConfig, IconConfig};

    #[test]
    fn merge_configs_preserves_global_theme_with_empty_local() {
        let mut global = AppConfig::default();
        global.theme.icons.preset = Some("nerdfont".to_string());

        let local = AppConfig::default(); // Empty local config

        let merged = merge_configs(global.clone(), local);
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
    fn merge_configs_overrides_theme_preset() {
        let mut global = AppConfig::default();
        global.theme.icons.preset = Some("unicode".to_string());

        let mut local = AppConfig::default();
        local.theme.icons.preset = Some("nerdfont".to_string());

        let merged = merge_configs(global, local);
        assert_eq!(merged.theme.icons.preset, Some("nerdfont".to_string()));
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
        global.defaults.view = crate::config::types::View::Issues;

        let local = AppConfig::default(); // Empty local config

        let merged = merge_configs(global, local);
        assert_eq!(merged.defaults.view, crate::config::types::View::Issues);
    }

    #[test]
    fn merge_configs_overrides_defaults() {
        let mut global = AppConfig::default();
        global.defaults.view = crate::config::types::View::Prs;

        let mut local = AppConfig::default();
        local.defaults.view = crate::config::types::View::Issues;

        let merged = merge_configs(global, local);
        assert_eq!(merged.defaults.view, crate::config::types::View::Issues);
    }

    #[test]
    fn merge_configs_preserves_global_github_config_with_empty_local() {
        let mut global = AppConfig::default();
        global.github.auto_clone = true;

        let local = AppConfig::default(); // Empty local config

        let merged = merge_configs(global, local);
        assert!(merged.github.auto_clone);
    }

    #[test]
    fn merge_configs_overrides_github_config() {
        let mut global = AppConfig::default();
        global.github.auto_clone = false;

        let mut local = AppConfig::default();
        local.github.auto_clone = true;

        let merged = merge_configs(global, local);
        assert!(merged.github.auto_clone);
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
}
