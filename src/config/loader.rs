use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::config::builtin_themes;
use crate::config::types::{AppConfig, Theme};

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
/// from the local config; section lists are replaced if non-empty; `repo_paths`
/// are merged). Users should duplicate all needed settings in repo-local configs.
pub fn load_config(explicit_path: Option<&Path>) -> Result<AppConfig> {
    // If an explicit path was given, just load that.
    if let Some(path) = explicit_path {
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut config: AppConfig = toml::from_str(&contents)
            .with_context(|| format!("parsing TOML from {}", path.display()))?;
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

    let toml_src = if let Some(name) = theme_file.strip_prefix("builtin:") {
        builtin_themes::get(name).with_context(|| {
            let names = builtin_themes::list().join(", ");
            format!("unknown built-in theme {name:?}; available: {names}")
        })?
    } else {
        // Filesystem path — expand leading `~`.
        let path = expand_tilde(theme_file);
        // We need a static str but we have a runtime String — store it in
        // a Box and leak it so we can treat it as `&'static str`.
        // This happens at most once per run so the tiny leak is acceptable.
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("reading theme file {}", path.display()))?;
        Box::leak(contents.into_boxed_str())
    };

    let file_theme: ThemeFile = toml::from_str(toml_src)
        .with_context(|| format!("parsing theme file {theme_file:?}"))?;

    // The file provides the base; inline [theme.*] is the overlay.
    let inline = std::mem::take(&mut config.theme);
    config.theme = Theme::merge(file_theme.theme, inline);
    Ok(())
}

/// Merge repo-local config on top of global config.
///
/// Section lists (`pr_sections`, `issues_sections`, `notifications_sections`) from
/// local replace global entirely when non-empty. Defaults, theme, and
/// keybindings from local replace global wholesale. Repo paths are merged
/// (local entries override matching global keys).
fn merge_configs(global: AppConfig, local: AppConfig) -> AppConfig {
    AppConfig {
        pr_sections: if local.pr_sections.is_empty() {
            global.pr_sections
        } else {
            local.pr_sections
        },
        issues_sections: if local.issues_sections.is_empty() {
            global.issues_sections
        } else {
            local.issues_sections
        },
        notifications_sections: if local.notifications_sections.is_empty() {
            global.notifications_sections
        } else {
            local.notifications_sections
        },
        github: local.github,
        defaults: local.defaults,
        theme: local.theme,
        keybindings: local.keybindings,
        repo_paths: {
            let mut paths = global.repo_paths;
            paths.extend(local.repo_paths);
            paths
        },
        theme_file: local.theme_file,
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

