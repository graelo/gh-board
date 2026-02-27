# Workflows

Step-by-step guides for common gh-board tasks. Each section lists the
prerequisites and walks through the workflow end-to-end. Keys shown are the
defaults — see [KEYBINDINGS.md](KEYBINDINGS.md) for remapping.

---

## Browse and review PRs

Open PRs, review code, and take action — all from the keyboard.

> **Prerequisites:** GitHub authentication (`gh auth login` or `GITHUB_TOKEN`),
> at least one `[[pr_filters]]` in your config.

1. Launch `gh-board` — the PR view is the default.
2. Switch filter tabs with `h`/`l`.
3. Navigate the list with `j`/`k` (or `g`/`G` to jump to first/last).
4. Press `p` to open the preview pane (markdown body + checks).
5. Take action on the selected PR:

   | Key | Action                                       |
   | --- | -------------------------------------------- |
   | `v` | Approve                                      |
   | `C` | Comment                                      |
   | `a` | Assign / unassign (multiselect autocomplete) |
   | `L` | Label (autocomplete)                         |
   | `m` | Merge                                        |
   | `x` | Close                                        |
   | `X` | Reopen                                       |
   | `W` | Mark as ready for review                     |
   | `u` | Update from base branch                      |

6. Destructive actions show a `y/n` confirmation prompt — press `y` to confirm,
   `n` or `Esc` to abort.

Useful extras: `o` opens the PR in your browser, `y` copies the number, `Y`
copies the URL.

---

## Checkout a PR branch

Switch your local clone to a PR's head branch. Each checkout replaces the
working tree in place — if you were on another branch, uncommitted changes
must be dealt with first.

> **Prerequisites:** A `[repo_paths]` entry mapping the repository. The local
> clone does not need to exist yet — gh-board will offer to clone it for you
> (or clone automatically when `auto_clone = true` is set under `[github]`).

```toml
[repo_paths]
"owner/repo" = "/path/to/repo"
"graelo/gh-board" = "~/code/gh-board"
```

1. Select a PR in the PR view.
2. Press `c` to checkout the branch.
3. gh-board automatically fetches the branch from the remote and checks it out
   in `~/code/gh-board`.

> **Note:** The PR's head branch must still exist on the remote. This works
> reliably on **open** and **draft** PRs. For merged PRs whose branch was
> deleted, the fetch will fail.

---

## Create a worktree for a PR

Create an isolated git worktree so you can review or test a PR without
disturbing your current branch. Unlike checkout, which switches the working
tree in place, each worktree lives in its own directory — so you can have
multiple PRs checked out side by side.

> **Prerequisites:** Same as checkout — a `[repo_paths]` entry for the
> repository. If the clone doesn't exist yet, gh-board will offer to clone it
> first (or auto-clone with `auto_clone = true`).

1. Select a PR in the PR view.
2. Press `w` to create the worktree.
3. gh-board fetches the branch and creates a worktree next to your clone.
4. The path is copied to your clipboard — `cd` into it to start working.

For example, with the `[repo_paths]` mapping above, pressing `w` on two PRs
with branches `feat/dark-mode` and `fix/scroll-offset` produces:

```text
~/code/
├── gh-board/                          # your main clone
├── gh-board-worktrees/
│   ├── feat-dark-mode/                # worktree for first PR
│   └── fix-scroll-offset/             # worktree for second PR
```

Running `git worktree list` from any of these directories shows all linked
worktrees:

```text
$ cd ~/code/gh-board-worktrees/feat-dark-mode
$ git worktree list
/Users/you/code/gh-board                              abc1234 [main]
/Users/you/code/gh-board-worktrees/feat-dark-mode     def5678 [feat/dark-mode]
/Users/you/code/gh-board-worktrees/fix-scroll-offset  9ab0cde [fix/scroll-offset]
```

Pressing `w` again on the same PR is a no-op — it returns the existing
worktree path instantly.

> **Note:** The naming convention is `<clone-dir>-worktrees/<branch-slug>/`,
> placed next to the configured clone path. The same remote-branch requirement
> as checkout applies: the PR's head branch must still exist on the remote.

---

## Open a GitHub URL

Jump directly to a PR, issue, or workflow run from a URL — handy for links
pasted in chat or CI notifications.

> **Prerequisites:** A valid `github.com` or GitHub Enterprise URL.

```bash
gh-board open https://github.com/owner/repo/pull/42
```

Supported URL patterns:

| Pattern           | Opens in     |
| ----------------- | ------------ |
| `/pull/N`         | PR view      |
| `/issues/N`       | Issues view  |
| `/actions/runs/N` | Actions view |

You can also pass the URL as a bare argument (`gh-board <URL>`); gh-board will
print a hint suggesting the explicit `open` form.

> **Note:** PRs and issues must match a configured filter to appear. Actions
> URLs for repos without a matching `[[actions_filters]]` tab get an
> auto-created ephemeral tab.

---

## Triage issues

Manage your issue backlog with the same keyboard-driven workflow as PRs.

> **Prerequisites:** GitHub authentication, at least one `[[issues_filters]]`
> in your config.

1. Switch to the Issues view with `n` (or `N` to go backwards).
2. Navigate and preview as usual (`j`/`k`, `p`).
3. Take action:

   | Key | Action            |
   | --- | ----------------- |
   | `a` | Assign / unassign |
   | `L` | Label             |
   | `c` | Comment           |
   | `x` | Close             |
   | `X` | Reopen            |

See [FILTERS.md](FILTERS.md) for the full PR/Issue filter syntax.

---

## Manage notifications

Triage your GitHub notification inbox without leaving the terminal.

> **Prerequisites:** GitHub authentication, at least one
> `[[notifications_filters]]` in your config.

```toml
[[notifications_filters]]
title = "Inbox"
filters = ""

[[notifications_filters]]
title = "Review Requested"
filters = "reason:review_requested"
```

1. Switch to the Notifications view with `n`.
2. Navigate the list with `j`/`k`.
3. Take action:

   | Key | Action           |
   | --- | ---------------- |
   | `m` | Mark as read     |
   | `M` | Mark all as read |
   | `u` | Unsubscribe      |

4. Press `/` to search within loaded notifications — supports `reason:`,
   `repo:`, `is:` qualifiers and free-text matching.

> **Note:** The GitHub REST API does not expose the "Done" (archived) state.
> `is:read` in gh-board means "not unread", which includes both read and done
> in GitHub's sense. See [FILTERS.md](FILTERS.md) for details.

---

## Monitor CI/CD

Browse workflow runs, re-run jobs, and cancel runs.

> **Prerequisites:** GitHub authentication. Either `[[actions_filters]]` in
> your config, or use ephemeral tabs via deep-linking.

```toml
[[actions_filters]]
title = "CI"
repo  = "@current"
```

1. Switch to the Actions view with `n`.
2. Navigate runs with `j`/`k`, preview with `p`.
3. Take action:

   | Key      | Action             |
   | -------- | ------------------ |
   | `e`      | Re-run failed jobs |
   | `E`      | Re-run all jobs    |
   | `ctrl+x` | Cancel run         |

   All three require `y/n` confirmation.

**Deep-linking from PRs:** Press `ctrl+]` on a PR to jump directly to its
latest Actions run. If the repository has no matching `[[actions_filters]]`
tab, an ephemeral tab is auto-created (prefixed with `◌`). Close ephemeral
tabs with `d`. A maximum of 5 ephemeral tabs are kept per session.

> **Note:** `@current` is just an alias for whatever repository gh-board
> detects from your working directory — it resolves to the `owner/repo` of the
> git remote where you launched the command. The tab shows empty when launched
> outside a git repo or when the scope is toggled to global mode.
>
> For repositories you always want to monitor regardless of where you launch
> gh-board, add dedicated filters with an explicit `repo`:
>
> ```toml
> [[actions_filters]]
> title = "CI"
> repo  = "@current"
>
> [[actions_filters]]
> title = "API"
> repo  = "myorg/api-server"
>
> [[actions_filters]]
> title = "Infra"
> repo  = "myorg/infrastructure"
> ```

---

## Work with branches

Manage local branches from the Branches view.

> **Prerequisites:** Launch gh-board from inside a git repository, or have a
> `[repo_paths]` entry.

1. Switch to the Branches view with `n`.
2. Take action:

   | Key               | Action                |
   | ----------------- | --------------------- |
   | `Enter` / `Space` | Checkout branch       |
   | `+`               | Create new branch     |
   | `Delete` / `D`    | Delete branch         |
   | `p`               | Create PR from branch |
   | `v`               | View PRs for branch   |

---

## Tips

- Press `?` in any view to see a contextual help overlay with all available
  keys.
- `r` refreshes the current filter tab; `R` refreshes all tabs and clears the
  cache.
- `S` toggles between repo-scoped and global scope.
- Run `gh-board init` for an interactive wizard that generates a starter config.
- See [KEYBINDINGS.md](KEYBINDINGS.md) for remapping keys and adding custom
  shell commands.
- See [FILTERS.md](FILTERS.md) for the full filter query language.
- See [THEME.md](THEME.md) for theme customization.
