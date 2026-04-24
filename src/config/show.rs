use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use toml::Value;

use crate::config::builtin_themes;
use crate::config::loader;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// A single flattened config entry with its origin file.
pub struct ConfigEntry {
    /// Source file path (or `"builtin:<name>"` for theme files).
    pub origin: String,
    /// Dotted key path, e.g. `"github.refetch_interval_minutes"`.
    pub key: String,
    /// TOML-formatted value, e.g. `"5"` or `"\"prs\""`.
    pub value: String,
}

/// Load all config layers, merge them as generic TOML tables while tracking
/// per-key origins, then flatten into a sorted list of entries.
pub fn load_config_entries(explicit_path: Option<&Path>) -> Result<Vec<ConfigEntry>> {
    let mut merged = toml::map::Map::new();
    let mut origins: BTreeMap<String, String> = BTreeMap::new();

    if let Some(path) = explicit_path {
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let table = parse_as_table(&contents, path)?;
        let origin = path.display().to_string();
        record_all_origins(&table, "", &origin, &mut origins);
        merged = table;
    } else {
        // Global config.
        if let Some(path) = loader::find_global_config() {
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let table = parse_as_table(&contents, &path)?;
            let origin = path.display().to_string();
            record_all_origins(&table, "", &origin, &mut origins);
            merged = table;
        }

        // Local config chain (farthest ancestor first, project last).
        for path in &loader::find_local_config_chain() {
            let contents = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            let table = parse_as_table(&contents, path)?;
            let origin = path.display().to_string();
            merge_value_layers(&mut merged, &table, &mut origins, &origin);
        }
    }

    // Handle theme_file overlay.
    apply_theme_file_layer(&mut merged, &mut origins)?;

    // Flatten into sorted entries.
    let mut flat = Vec::new();
    flatten(&merged, "", &mut flat);
    flat.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(flat
        .into_iter()
        .map(|(key, value)| {
            let origin = origins.get(&key).cloned().unwrap_or_default();
            ConfigEntry { origin, key, value }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Theme file overlay
// ---------------------------------------------------------------------------

/// If the merged config contains `theme_file`, load that theme as the base for
/// the `theme` subtree, then overlay any inline `[theme.*]` keys on top.
fn apply_theme_file_layer(
    merged: &mut toml::map::Map<String, Value>,
    origins: &mut BTreeMap<String, String>,
) -> Result<()> {
    let Some(Value::String(theme_path)) = merged.get("theme_file") else {
        return Ok(());
    };
    let theme_path = theme_path.clone();

    let (theme_toml_src, theme_origin) = if let Some(name) = theme_path.strip_prefix("builtin:") {
        let src = builtin_themes::get(name).with_context(|| {
            let names = builtin_themes::list().join(", ");
            format!("unknown built-in theme {name:?}; available: {names}")
        })?;
        (src.to_string(), format!("builtin:{name}"))
    } else {
        let path = loader::expand_tilde(&theme_path);
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("reading theme file {}", path.display()))?;
        (contents, path.display().to_string())
    };

    let file_value: Value =
        toml::from_str(&theme_toml_src).with_context(|| format!("parsing theme {theme_path:?}"))?;

    let Some(file_theme_table) = file_value
        .as_table()
        .and_then(|t| t.get("theme"))
        .and_then(Value::as_table)
    else {
        return Ok(());
    };

    // Save inline theme origins (they win over the file theme).
    let inline_origins: BTreeMap<String, String> = origins
        .iter()
        .filter(|(k, _)| k.starts_with("theme."))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    // Save inline theme keys.
    let inline_theme = merged
        .get("theme")
        .and_then(Value::as_table)
        .cloned()
        .unwrap_or_default();

    // Start from the file theme as base.
    let mut result_theme = file_theme_table.clone();
    record_all_origins(file_theme_table, "theme", &theme_origin, origins);

    // Overlay inline keys on top.
    merge_table_recursive(&mut result_theme, &inline_theme, origins, "", "theme");

    // Restore inline origins (they win).
    origins.extend(inline_origins);

    merged.insert("theme".to_string(), Value::Table(result_theme));
    Ok(())
}

// ---------------------------------------------------------------------------
// Merge logic
// ---------------------------------------------------------------------------

/// Top-level keys whose arrays are replaced entirely (not merged element-wise)
/// when the overlay provides a non-empty array.
const FILTER_ARRAY_KEYS: &[&str] = &[
    "pr_filters",
    "issues_filters",
    "actions_filters",
    "notifications_filters",
    "alerts_filters",
];

/// Merge `overlay` on top of `base`, tracking which source provides each key.
fn merge_value_layers(
    base: &mut toml::map::Map<String, Value>,
    overlay: &toml::map::Map<String, Value>,
    origins: &mut BTreeMap<String, String>,
    overlay_origin: &str,
) {
    for (key, overlay_val) in overlay {
        if FILTER_ARRAY_KEYS.contains(&key.as_str()) {
            // Replace-when-non-empty rule.
            if let Value::Array(arr) = overlay_val
                && !arr.is_empty()
            {
                remove_origins_with_prefix(origins, key);
                base.insert(key.clone(), overlay_val.clone());
                record_all_origins_for_value(overlay_val, key, overlay_origin, origins);
            }
        } else if key == "repo_paths" {
            // Individual-key merge.
            if let Value::Table(overlay_table) = overlay_val {
                let base_table = base
                    .entry(key.clone())
                    .or_insert_with(|| Value::Table(toml::map::Map::new()));
                if let Value::Table(bt) = base_table {
                    for (rk, rv) in overlay_table {
                        bt.insert(rk.clone(), rv.clone());
                        let dotted = format!("repo_paths.{}", quote_key(rk));
                        origins.insert(dotted, overlay_origin.to_string());
                    }
                }
            }
        } else if let (Some(Value::Table(_)), Value::Table(overlay_table)) =
            (base.get(key), overlay_val)
        {
            if let Some(Value::Table(bt)) = base.get_mut(key) {
                merge_table_recursive(bt, overlay_table, origins, overlay_origin, key);
            }
        } else {
            base.insert(key.clone(), overlay_val.clone());
            record_all_origins_for_value(overlay_val, key, overlay_origin, origins);
        }
    }
}

/// Recursively merge two TOML tables, building dotted key paths for origin
/// tracking.
fn merge_table_recursive(
    base: &mut toml::map::Map<String, Value>,
    overlay: &toml::map::Map<String, Value>,
    origins: &mut BTreeMap<String, String>,
    overlay_origin: &str,
    prefix: &str,
) {
    for (key, overlay_val) in overlay {
        let dotted = format!("{prefix}.{key}");
        if let (Some(Value::Table(_)), Value::Table(ov_table)) = (base.get(key), overlay_val) {
            if let Some(Value::Table(bt)) = base.get_mut(key) {
                merge_table_recursive(bt, ov_table, origins, overlay_origin, &dotted);
            }
        } else {
            base.insert(key.clone(), overlay_val.clone());
            record_all_origins_for_value(overlay_val, &dotted, overlay_origin, origins);
        }
    }
}

// ---------------------------------------------------------------------------
// Origin tracking helpers
// ---------------------------------------------------------------------------

/// Record origins for all leaf keys in `table` under the given `prefix`.
fn record_all_origins(
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    origin: &str,
    origins: &mut BTreeMap<String, String>,
) {
    for (key, val) in table {
        let dotted = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        record_all_origins_for_value(val, &dotted, origin, origins);
    }
}

/// Record origins for a single value (recursing into tables and arrays).
fn record_all_origins_for_value(
    val: &Value,
    dotted: &str,
    origin: &str,
    origins: &mut BTreeMap<String, String>,
) {
    match val {
        Value::Table(sub) => {
            for (k, v) in sub {
                let child = format!("{dotted}.{k}");
                record_all_origins_for_value(v, &child, origin, origins);
            }
        }
        Value::Array(arr) => {
            for (i, elem) in arr.iter().enumerate() {
                let child = format!("{dotted}.{i}");
                record_all_origins_for_value(elem, &child, origin, origins);
            }
        }
        _ => {
            origins.insert(dotted.to_string(), origin.to_string());
        }
    }
}

/// Remove all origin entries whose key starts with `prefix.` or equals `prefix`.
fn remove_origins_with_prefix(origins: &mut BTreeMap<String, String>, prefix: &str) {
    let prefix_dot = format!("{prefix}.");
    origins.retain(|k, _| k != prefix && !k.starts_with(&prefix_dot));
}

// ---------------------------------------------------------------------------
// Flattening
// ---------------------------------------------------------------------------

/// Recursively flatten a TOML table into `(dotted_key, formatted_value)` pairs.
fn flatten(table: &toml::map::Map<String, Value>, prefix: &str, out: &mut Vec<(String, String)>) {
    for (key, val) in table {
        let full_key = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match val {
            Value::Table(sub) => flatten(sub, &full_key, out),
            Value::Array(arr) => {
                for (i, elem) in arr.iter().enumerate() {
                    let idx_key = format!("{full_key}.{i}");
                    if let Value::Table(t) = elem {
                        flatten(t, &idx_key, out);
                    } else {
                        out.push((idx_key, format_value(elem)));
                    }
                }
            }
            _ => {
                out.push((full_key, format_value(val)));
            }
        }
    }
}

/// Format a TOML value for display.
fn format_value(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{s}\""),
        Value::Integer(n) => n.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Datetime(dt) => dt.to_string(),
        Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_value).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Table(_) => "<table>".to_string(),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_as_table(contents: &str, path: &Path) -> Result<toml::map::Map<String, Value>> {
    let val: Value = toml::from_str(contents)
        .with_context(|| format!("parsing TOML from {}", path.display()))?;
    match val {
        Value::Table(t) => Ok(t),
        _ => anyhow::bail!("expected TOML table in {}", path.display()),
    }
}

/// Quote a key segment if it contains dots or special characters.
fn quote_key(key: &str) -> String {
    if key.contains('.') || key.contains(' ') || key.contains('"') {
        format!("\"{}\"", key.replace('"', "\\\""))
    } else {
        key.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse a TOML string into a table.
    fn table(s: &str) -> toml::map::Map<String, Value> {
        let v: Value = toml::from_str(s).unwrap();
        v.as_table().unwrap().clone()
    }

    #[test]
    fn scalar_override_tracks_origin() {
        let mut merged = table("refetch = 5\n");
        let mut origins = BTreeMap::new();
        record_all_origins(&merged, "", "global.toml", &mut origins);

        let overlay = table("refetch = 10\n");
        merge_value_layers(&mut merged, &overlay, &mut origins, "local.toml");

        assert_eq!(merged["refetch"].as_integer(), Some(10));
        assert_eq!(origins["refetch"], "local.toml");
    }

    #[test]
    fn nested_table_merge_tracks_origin() {
        let mut merged = table("[github]\nscope = \"auto\"\nrefetch = 5\n");
        let mut origins = BTreeMap::new();
        record_all_origins(&merged, "", "global.toml", &mut origins);

        let overlay = table("[github]\nrefetch = 10\n");
        merge_value_layers(&mut merged, &overlay, &mut origins, "local.toml");

        // scope unchanged, origin still global.
        assert_eq!(origins["github.scope"], "global.toml");
        // refetch overridden.
        assert_eq!(origins["github.refetch"], "local.toml");
        assert_eq!(merged["github"]["refetch"].as_integer(), Some(10));
    }

    #[test]
    fn filter_array_replaces_entirely() {
        let mut merged = table(
            r#"
[[pr_filters]]
title = "Global PRs"
filters = "is:pr"
"#,
        );
        let mut origins = BTreeMap::new();
        record_all_origins(&merged, "", "global.toml", &mut origins);

        assert!(origins.contains_key("pr_filters.0.title"));

        let overlay = table(
            r#"
[[pr_filters]]
title = "Local PRs"
filters = "is:pr author:me"
[[pr_filters]]
title = "Review"
filters = "is:pr review-requested:me"
"#,
        );
        merge_value_layers(&mut merged, &overlay, &mut origins, "local.toml");

        // Old single entry replaced by two new ones.
        let arr = merged["pr_filters"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        // All origins now point to local.
        assert_eq!(origins["pr_filters.0.title"], "local.toml");
        assert_eq!(origins["pr_filters.1.title"], "local.toml");
        // Old entry gone.
        assert!(!origins.values().any(|v| v == "global.toml"));
    }

    #[test]
    fn empty_filter_array_does_not_replace() {
        let mut merged = table(
            r#"
[[pr_filters]]
title = "Global"
filters = "is:pr"
"#,
        );
        let mut origins = BTreeMap::new();
        record_all_origins(&merged, "", "global.toml", &mut origins);

        // Local has no pr_filters at all (empty array after default).
        let overlay = table("");
        merge_value_layers(&mut merged, &overlay, &mut origins, "local.toml");

        assert_eq!(origins["pr_filters.0.title"], "global.toml");
    }

    #[test]
    fn repo_paths_individual_merge() {
        let mut merged = table(
            r#"
[repo_paths]
"org/alpha" = "/tmp/alpha"
"org/beta" = "/tmp/beta"
"#,
        );
        let mut origins = BTreeMap::new();
        record_all_origins(&merged, "", "global.toml", &mut origins);

        let overlay = table(
            r#"
[repo_paths]
"org/beta" = "/local/beta"
"org/gamma" = "/local/gamma"
"#,
        );
        merge_value_layers(&mut merged, &overlay, &mut origins, "local.toml");

        let rp = merged["repo_paths"].as_table().unwrap();
        assert_eq!(rp.len(), 3);
        // alpha untouched.
        assert_eq!(origins["repo_paths.org/alpha"], "global.toml");
        // beta overridden.
        assert_eq!(origins["repo_paths.org/beta"], "local.toml");
        assert_eq!(rp["org/beta"].as_str(), Some("/local/beta"));
        // gamma added.
        assert_eq!(origins["repo_paths.org/gamma"], "local.toml");
    }

    #[test]
    fn flatten_produces_sorted_output() {
        let t = table(
            r#"
[github]
scope = "auto"
refetch = 5

[[pr_filters]]
title = "Mine"
filters = "is:pr"
"#,
        );
        let mut flat = Vec::new();
        flatten(&t, "", &mut flat);
        flat.sort_by(|a, b| a.0.cmp(&b.0));

        let keys: Vec<&str> = flat.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"github.scope"));
        assert!(keys.contains(&"github.refetch"));
        assert!(keys.contains(&"pr_filters.0.title"));
        assert!(keys.contains(&"pr_filters.0.filters"));
    }
}
