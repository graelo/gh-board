use gh_board::config::keybindings::{
    BuiltinAction, Keybinding, KeybindingsConfig, MergedBindings, ResolvedBinding, TemplateVars,
    ViewContext, default_prs, expand_template,
};

// ---------------------------------------------------------------------------
// T067: Keybinding rebinding tests
// ---------------------------------------------------------------------------

#[test]
fn override_replaces_default() {
    // Override the default "v" → approve with "v" → comment.
    let config = KeybindingsConfig {
        prs: vec![Keybinding {
            key: "v".to_owned(),
            builtin: Some("comment".to_owned()),
            command: None,
            name: Some("Comment via v".to_owned()),
        }],
        ..Default::default()
    };
    let merged = MergedBindings::from_config(&config);
    let binding = merged.resolve("v", ViewContext::Prs);
    assert!(matches!(
        binding,
        Some(ResolvedBinding::Builtin(BuiltinAction::CommentAction))
    ));
}

#[test]
fn override_approve_key() {
    // Remap approve from "v" to "A" (capital A).
    let config = KeybindingsConfig {
        prs: vec![Keybinding {
            key: "A".to_owned(),
            builtin: Some("approve".to_owned()),
            command: None,
            name: Some("Approve".to_owned()),
        }],
        ..Default::default()
    };
    let merged = MergedBindings::from_config(&config);

    // "A" now triggers approve.
    let binding = merged.resolve("A", ViewContext::Prs);
    assert!(matches!(
        binding,
        Some(ResolvedBinding::Builtin(BuiltinAction::Approve))
    ));

    // Original "v" still resolves to approve from defaults (not removed).
    let binding_v = merged.resolve("v", ViewContext::Prs);
    assert!(matches!(
        binding_v,
        Some(ResolvedBinding::Builtin(BuiltinAction::Approve))
    ));
}

#[test]
fn context_priority_over_universal() {
    // If same key is in both universal and prs, prs wins.
    let config = KeybindingsConfig {
        universal: vec![Keybinding {
            key: "z".to_owned(),
            builtin: Some("quit".to_owned()),
            command: None,
            name: None,
        }],
        prs: vec![Keybinding {
            key: "z".to_owned(),
            builtin: Some("approve".to_owned()),
            command: None,
            name: None,
        }],
        ..Default::default()
    };
    let merged = MergedBindings::from_config(&config);
    let binding = merged.resolve("z", ViewContext::Prs);
    assert!(matches!(
        binding,
        Some(ResolvedBinding::Builtin(BuiltinAction::Approve))
    ));
}

#[test]
fn unknown_key_returns_none() {
    let config = KeybindingsConfig::default();
    let merged = MergedBindings::from_config(&config);
    assert!(merged.resolve("zzz", ViewContext::Prs).is_none());
}

// ---------------------------------------------------------------------------
// Template expansion tests
// ---------------------------------------------------------------------------

#[test]
fn expand_template_url() {
    let vars = TemplateVars {
        url: "https://example.com/pr/1".to_owned(),
        ..Default::default()
    };
    let result = expand_template("open {{.Url}}", &vars);
    assert_eq!(result, "open https://example.com/pr/1");
}

#[test]
fn expand_template_repeated_vars() {
    let vars = TemplateVars {
        number: "99".to_owned(),
        ..Default::default()
    };
    let result = expand_template("{{.Number}} {{.Number}}", &vars);
    assert_eq!(result, "99 99");
}

// ---------------------------------------------------------------------------
// Default keybinding coverage tests
// ---------------------------------------------------------------------------

#[test]
fn defaults_prs_has_actions() {
    let bindings = default_prs();
    let keys: Vec<&str> = bindings.iter().map(|b| b.key.as_str()).collect();
    assert!(keys.contains(&"v")); // approve
    assert!(keys.contains(&"c")); // checkout
    assert!(keys.contains(&"m")); // merge
    assert!(keys.contains(&"x")); // close
    assert!(keys.contains(&"X")); // reopen
    assert!(keys.contains(&"d")); // diff
}

#[test]
fn builtin_action_roundtrip() {
    // Every action can be parsed by from_name and described.
    let names = [
        "move_down",
        "move_up",
        "first",
        "last",
        "page_down",
        "page_up",
        "prev_filter",
        "next_filter",
        "toggle_preview",
        "open_browser",
        "refresh",
        "refresh_all",
        "search",
        "copy_number",
        "copy_url",
        "toggle_help",
        "quit",
        "approve",
        "assign",
        "unassign",
        "comment",
        "view_diff",
        "checkout",
        "close",
        "reopen",
        "mark_ready",
        "merge",
        "update_from_base",
        "label",
        "mark_read",
        "mark_all_read",
        "unsubscribe",
        "delete_branch",
        "new_branch",
        "create_pr_from_branch",
        "view_prs_for_branch",
        "switch_view",
    ];
    for name in &names {
        let action = BuiltinAction::from_name(name);
        assert!(action.is_some(), "from_name failed for: {name}");
        let desc = action.unwrap().description();
        assert!(!desc.is_empty(), "empty description for: {name}");
    }
}

#[test]
fn builtin_from_name_unknown_returns_none() {
    assert!(BuiltinAction::from_name("nonexistent").is_none());
}

#[test]
fn builtin_watch_run_from_name() {
    assert_eq!(
        BuiltinAction::from_name("watch_run"),
        Some(BuiltinAction::WatchRun)
    );
}

#[test]
fn default_actions_has_watch_run_and_resolves() {
    let merged = MergedBindings::from_config(&KeybindingsConfig::default());
    let binding = merged.resolve("W", ViewContext::Actions);
    assert!(
        matches!(
            binding,
            Some(ResolvedBinding::Builtin(BuiltinAction::WatchRun))
        ),
        "W should resolve to WatchRun in Actions context"
    );
}
