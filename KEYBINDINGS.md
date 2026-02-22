# Keybindings Reference

gh-board ships with sensible defaults for every action and lets you remap
any of them — or add custom shell commands — in your `config.toml`.

---

## Default keybindings

### Universal (active in every view)

| Key | Action | Description |
|---|---|---|
| `j` / `↓` | `move_down` | Move cursor down |
| `k` / `↑` | `move_up` | Move cursor up |
| `g` / `Home` | `first` | Jump to first item |
| `G` / `End` | `last` | Jump to last item |
| `ctrl+d` / `PageDown` | `page_down` | Page down |
| `ctrl+u` / `PageUp` | `page_up` | Page up |
| `h` / `←` | `prev_filter` | Previous filter tab |
| `l` / `→` | `next_filter` | Next filter tab |
| `p` | `toggle_preview` | Toggle preview pane |
| `o` | `open_browser` | Open item in browser |
| `r` | `refresh` | Refresh current filter |
| `R` | `refresh_all` | Refresh all filters (clear cache) |
| `/` | `search` | Search / filter |
| `y` | `copy_number` | Copy number to clipboard |
| `Y` | `copy_url` | Copy URL to clipboard |
| `?` | `toggle_help` | Toggle help overlay |
| `q` | `quit` | Quit |

### PR view

| Key | Action | Description |
|---|---|---|
| `v` | `approve` | Approve PR |
| `a` | `assign` | Assign (multi, autocomplete) |
| `A` | `unassign` | Unassign |
| `c` | `comment` | Comment |
| `d` | `view_diff` | View diff in pager |
| `C` / `Space` | `checkout` | Checkout branch |
| `x` | `close` | Close PR |
| `X` | `reopen` | Reopen PR |
| `W` | `mark_ready` | Mark as ready for review |
| `m` | `merge` | Merge PR |
| `u` | `update_from_base` | Update from base branch |
| `n` / `N` | `switch_view` / `switch_view_back` | Switch view |
| `S` | `toggle_scope` | Toggle repo scope |

### Issue view

| Key | Action | Description |
|---|---|---|
| `L` | `label` | Label (autocomplete) |
| `a` | `assign` | Assign (multi, autocomplete) |
| `A` | `unassign` | Unassign |
| `c` | `comment` | Comment |
| `x` | `close` | Close issue |
| `X` | `reopen` | Reopen issue |
| `n` / `N` | `switch_view` / `switch_view_back` | Switch view |
| `S` | `toggle_scope` | Toggle repo scope |

### Notifications view

| Key | Action | Description |
|---|---|---|
| `m` | `mark_read` | Mark as read |
| `M` | `mark_all_read` | Mark all as read |
| `u` | `unsubscribe` | Unsubscribe |
| `n` / `N` | `switch_view` / `switch_view_back` | Switch view |
| `S` | `toggle_scope` | Toggle repo scope |

### Actions view

| Key | Action | Description |
|---|---|---|
| `w` | `toggle_workflow_nav` | Toggle workflow navigator sidebar |
| `ctrl+r` | `rerun_failed` | Re-run failed jobs |
| `ctrl+R` | `rerun_all` | Re-run all jobs |
| `ctrl+x` | `cancel_run` | Cancel run |
| `n` / `N` | `switch_view` / `switch_view_back` | Switch view |

### Branches view

| Key | Action | Description |
|---|---|---|
| `Enter` / `Space` | `checkout` | Checkout branch |
| `Delete` / `D` | `delete_branch` | Delete branch |
| `+` | `new_branch` | Create new branch |
| `p` | `create_pr_from_branch` | Create PR from branch |
| `v` | `view_prs_for_branch` | View PRs for branch |
| `n` / `N` | `switch_view` / `switch_view_back` | Switch view |
| `S` | `toggle_scope` | Toggle repo scope |

---

## Confirmation prompts

Destructive actions (close, merge, delete, cancel run, rerun, etc.) require
a `y/n` confirmation before executing. After pressing the action key, the
text-input bar at the bottom of the screen shows a warning-coloured prompt:

```
Close this PR? (y/n)
```

Press `y` or `Y` to confirm, `n`, `N`, or `Esc` to abort. "Cancelled"
is shown in the footer if you abort.

The `y`/`n` keys are intentionally **not** user-configurable — they are
UI mechanics, not actions.

---

## Customising keybindings

Add a `[keybindings]` section to your config file
(`~/.config/gh-board/config.toml` or wherever your config lives — see
`examples/config.toml` for the full priority order).

### Remap a built-in action

A user binding for a key **replaces** the default binding for that key.
Defaults for all other keys are preserved.

```toml
# Remap "approve" from v to a
[[keybindings.prs]]
key = "a"
builtin = "approve"
name = "Approve"
```

### Add a shell command

Bind a key to an arbitrary shell command. Template variables are expanded
before the command is run.

```toml
[[keybindings.prs]]
key = "ctrl+b"
command = "open {{.Url}}"
name = "Open in browser (custom)"
```

Available template variables:

| Variable | Value |
|---|---|
| `{{.Url}}` | Item's HTML URL |
| `{{.Number}}` | PR / issue number |
| `{{.RepoName}}` | `owner/repo` string |
| `{{.HeadBranch}}` | Head branch name (PRs only) |
| `{{.BaseBranch}}` | Base branch name (PRs only) |

### Available contexts

| TOML key | Active in |
|---|---|
| `[[keybindings.universal]]` | All views |
| `[[keybindings.prs]]` | PR view |
| `[[keybindings.issues]]` | Issue view |
| `[[keybindings.actions]]` | Actions view |
| `[[keybindings.branches]]` | Branches view |

> **Note:** Notifications keybindings are not currently user-configurable.

### Resolution order

1. Context-specific binding for the pressed key (if any).
2. Universal binding for the pressed key (if any).
3. Key is ignored.

Context bindings take priority over universal ones. This lets you shadow a
universal binding in a specific view without affecting other views.

---

## Built-in action names

The full list of names accepted by the `builtin` field:

| Name | Description |
|---|---|
| `move_down` | Move cursor down |
| `move_up` | Move cursor up |
| `first` | Jump to first item |
| `last` | Jump to last item |
| `page_down` | Page down |
| `page_up` | Page up |
| `prev_filter` | Previous filter tab |
| `next_filter` | Next filter tab |
| `toggle_preview` | Toggle preview pane |
| `open_browser` | Open in browser |
| `refresh` | Refresh current filter |
| `refresh_all` | Refresh all filters |
| `search` | Search / filter |
| `copy_number` | Copy number to clipboard |
| `copy_url` | Copy URL to clipboard |
| `toggle_help` | Toggle help overlay |
| `quit` | Quit |
| `approve` | Approve PR |
| `assign` | Assign (autocomplete) |
| `unassign` | Unassign |
| `comment` | Comment |
| `view_diff` | View diff in pager |
| `checkout` | Checkout branch |
| `close` | Close PR or issue |
| `reopen` | Reopen PR or issue |
| `mark_ready` | Mark PR as ready for review |
| `merge` | Merge PR |
| `update_from_base` | Update PR from base branch |
| `label` | Label (autocomplete, issues) |
| `mark_read` | Mark notification as read |
| `mark_all_read` | Mark all notifications as read |
| `unsubscribe` | Unsubscribe from notification |
| `delete_branch` | Delete branch |
| `new_branch` | Create new branch |
| `create_pr_from_branch` | Create PR from branch |
| `view_prs_for_branch` | View PRs for branch |
| `switch_view` | Switch to next view |
| `switch_view_back` | Switch to previous view |
| `toggle_scope` | Toggle repo scope |
| `toggle_workflow_nav` | Toggle workflow navigator (actions) |
| `rerun_failed` | Re-run failed jobs (actions) |
| `rerun_all` | Re-run all jobs (actions) |
| `cancel_run` | Cancel workflow run (actions) |

---

## Non-configurable keys

The following keys are UI mechanics handled directly by the input layer and
cannot be rebound:

| Key | Where | Role |
|---|---|---|
| `y` / `Y` | All confirmation prompts | Confirm action |
| `n` / `N` / `Esc` | All confirmation prompts | Abort action |
| `Esc` | Search / text-input modes | Exit mode |
| `Enter` | Search mode | Submit search |
| `Backspace` | Text-input modes | Delete character |
| Printable chars | Text-input modes (search, comment, branch name, assignee, label) | Character input |
| `Tab` / `Shift+Tab` / `↑` / `↓` / `Enter` | Autocomplete suggestion lists (assign, label) | Navigate and select suggestions |
| `Ctrl+D` | Comment / assign submit | Submit multi-line input |
| `m` / `M` | PR update-branch method picker | Choose merge strategy |
| `?` / `Esc` | Help overlay | Dismiss overlay |
| `j` / `k` / `↑` / `↓` / `Enter` / `Esc` | Actions workflow nav panel (when focused) | Navigate the popup list |
