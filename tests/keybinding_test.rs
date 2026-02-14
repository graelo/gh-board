use gh_board::config::keybindings::{
    BuiltinAction, Keybinding, KeybindingsConfig, MergedBindings, ResolvedBinding, TemplateVars,
    ViewContext, default_prs, default_universal, expand_template, key_event_to_string,
};

use iocraft::prelude::{KeyCode, KeyEventKind, KeyModifiers};

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
fn custom_shell_command_binding() {
    let config = KeybindingsConfig {
        prs: vec![Keybinding {
            key: "z".to_owned(),
            builtin: None,
            command: Some("echo {{.Number}}".to_owned()),
            name: Some("Echo number".to_owned()),
        }],
        ..Default::default()
    };
    let merged = MergedBindings::from_config(&config);
    let binding = merged.resolve("z", ViewContext::Prs);
    match binding {
        Some(ResolvedBinding::ShellCommand(cmd)) => {
            assert_eq!(cmd, "echo {{.Number}}");
        }
        _ => panic!("Expected ShellCommand"),
    }
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
fn universal_fallback() {
    let config = KeybindingsConfig::default();
    let merged = MergedBindings::from_config(&config);
    // 'q' is only in universal defaults.
    let binding = merged.resolve("q", ViewContext::Issues);
    assert!(matches!(
        binding,
        Some(ResolvedBinding::Builtin(BuiltinAction::Quit))
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
fn expand_all_template_vars() {
    let vars = TemplateVars {
        url: "https://github.com/org/repo/pull/42".to_owned(),
        number: "42".to_owned(),
        repo_name: "org/repo".to_owned(),
        head_branch: "feature-x".to_owned(),
        base_branch: "main".to_owned(),
    };
    let result = expand_template("gh pr checkout {{.Number}} --repo {{.RepoName}}", &vars);
    assert_eq!(result, "gh pr checkout 42 --repo org/repo");
}

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
fn expand_template_no_placeholders() {
    let vars = TemplateVars::default();
    assert_eq!(expand_template("echo hello", &vars), "echo hello");
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
// Key string conversion tests
// ---------------------------------------------------------------------------

#[test]
fn key_string_regular_chars() {
    assert_eq!(
        key_event_to_string(
            KeyCode::Char('a'),
            KeyModifiers::empty(),
            KeyEventKind::Press
        ),
        Some("a".to_owned())
    );
    assert_eq!(
        key_event_to_string(KeyCode::Char('Z'), KeyModifiers::SHIFT, KeyEventKind::Press),
        Some("Z".to_owned())
    );
}

#[test]
fn key_string_ctrl_modifier() {
    assert_eq!(
        key_event_to_string(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press
        ),
        Some("ctrl+c".to_owned())
    );
}

#[test]
fn key_string_alt_modifier() {
    assert_eq!(
        key_event_to_string(KeyCode::Char('d'), KeyModifiers::ALT, KeyEventKind::Press),
        Some("alt+d".to_owned())
    );
}

#[test]
fn key_string_special_keys() {
    assert_eq!(
        key_event_to_string(KeyCode::Enter, KeyModifiers::empty(), KeyEventKind::Press),
        Some("enter".to_owned())
    );
    assert_eq!(
        key_event_to_string(KeyCode::Esc, KeyModifiers::empty(), KeyEventKind::Press),
        Some("esc".to_owned())
    );
    assert_eq!(
        key_event_to_string(KeyCode::PageUp, KeyModifiers::empty(), KeyEventKind::Press),
        Some("pageup".to_owned())
    );
}

#[test]
fn key_string_release_returns_none() {
    assert!(
        key_event_to_string(
            KeyCode::Char('a'),
            KeyModifiers::empty(),
            KeyEventKind::Release
        )
        .is_none()
    );
}

// ---------------------------------------------------------------------------
// Default keybinding coverage tests
// ---------------------------------------------------------------------------

#[test]
fn defaults_universal_has_navigation() {
    let bindings = default_universal();
    let keys: Vec<&str> = bindings.iter().map(|b| b.key.as_str()).collect();
    assert!(keys.contains(&"j"));
    assert!(keys.contains(&"k"));
    assert!(keys.contains(&"g"));
    assert!(keys.contains(&"G"));
    assert!(keys.contains(&"ctrl+d"));
    assert!(keys.contains(&"ctrl+u"));
    assert!(keys.contains(&"h"));
    assert!(keys.contains(&"l"));
}

#[test]
fn defaults_prs_has_actions() {
    let bindings = default_prs();
    let keys: Vec<&str> = bindings.iter().map(|b| b.key.as_str()).collect();
    assert!(keys.contains(&"v")); // approve
    assert!(keys.contains(&"c")); // comment
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
        "prev_section",
        "next_section",
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
        "mark_done",
        "mark_all_done",
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
