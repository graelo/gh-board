# gh-board

A fast terminal dashboard for GitHub pull requests, issues, notifications, and
repository branches.

<https://github.com/user-attachments/assets/ea174fd2-1559-475b-90f9-d34d64f56225>

## Features

**Five views, one dashboard** — switch between them with `n`/`N`, organize each
with named filter tabs (`h`/`l`), and press `?` anywhere for contextual help.

### Review & merge PRs

Approve, comment, assign, label, merge, checkout branch, create worktrees, view
diff, and deep-link to CI runs — all from your keyboard.

### Triage issues

Assign, label, comment, close/reopen with confirmation prompts.

### Manage notifications

Mark read, unsubscribe, filter by reason, repo, or status.

### Monitor CI/CD

Browse workflow runs, re-run failed jobs, cancel runs. Jump straight from a PR's
check status to its Actions run with `Ctrl+]`.

### Open any GitHub URL

`gh-board open <URL>` jumps directly to a PR, issue, or workflow run — handy for
links pasted in chat or CI notifications. Works with `github.com` and GitHub
Enterprise hosts.

### Work with branches

Checkout, delete, create new branches, and open PRs — without leaving the
terminal.

### Worktree workflow

Press `w` on any PR to create a git worktree for its branch — the path is
copied to your clipboard so you can `cd` straight into it. The worktree is
placed at `<repo>-worktrees/<branch-slug>/` next to your main clone. Pressing
`w` again on the same PR is a no-op that returns the existing path instantly.

Requires a `[repo_paths]` entry mapping the repository to its local clone:

```toml
[repo_paths]
"owner/repo" = "/path/to/repo"
```

### Under the hood

- **Rich markdown preview** — CommonMark with syntax highlighting (18 languages)
  and GitHub emoji shortcodes
- **Powerful filtering** — full GitHub search syntax for PRs/issues; custom
  qualifier language for notifications; per-filter GitHub Enterprise hosts
- **25 built-in themes** — uniform ANSI-256 + hex color model; indices 0-15
  follow your terminal palette. Run `gh-board themes` to list them, or see
  [THEME.md](THEME.md)
- **Customizable keybindings** — remap any action or wire custom shell commands
  with template variables. See [KEYBINDINGS.md](KEYBINDINGS.md)
- **Fast** — under 500ms to first render; in-memory LRU cache with background
  refresh

## Installation

### From Source

```bash
git clone https://github.com/graelo/gh-board.git
cd gh-board
cargo build --release
sudo cp target/release/gh-board /usr/local/bin/
```

### As a `gh` Extension

```bash
gh extension install graelo/gh-board
```

Then run via:

```bash
gh board              # launch the dashboard
gh board open <URL>   # jump to a specific PR, issue, or Actions run
```

## Prerequisites

- **Authentication**: Either:
  - The [GitHub CLI](https://cli.github.com/) (`gh`) must be installed and
    authenticated, OR
  - Set `GITHUB_TOKEN` or `GH_TOKEN` environment variable
- **Terminal**: 16-color minimum (256-color or true-color recommended)

## Quick Start

1. Create a config file at `~/.config/gh-board/config.toml`:

```toml
[[pr_filters]]
title = "My PRs"
filters = "is:open author:@me"

[[pr_filters]]
title = "Needs Review"
filters = "is:open review-requested:@me"

[[issues_filters]]
title = "Assigned to Me"
filters = "is:open assignee:@me"
```

2. Run `gh-board` (or `gh board`).

3. Navigate with `j`/`k`, switch filters with `h`/`l`, press `?` for help.

See [`examples/config.toml`](examples/config.toml) for a comprehensive example.

## Usage

```bash
gh-board [COMMAND] [OPTIONS] [URL]
```

**Subcommands:**

- `open <URL>`: Open a GitHub PR, issue, or Actions run URL directly in the
  matching view
- `init`: Interactive wizard that generates a starter config
- `themes`: List all built-in theme names

**Options:**

- `-c, --config <PATH>`: Use a specific config file
- `--debug`: Enable debug logging to `debug.log`
- `-h, --help`: Show help
- `-v`: Show version

**Arguments:**

- `[URL]`: A GitHub URL — shorthand for `gh-board open <URL>` (prints a hint
  to stderr suggesting the explicit form)

### Configuration

Configuration files are loaded in this priority order:

1. `--config` flag
2. `.gh-board.toml` in current Git repository root
3. `$GH_BOARD_CONFIG` environment variable
4. `$XDG_CONFIG_HOME/gh-board/config.toml`
5. `~/.config/gh-board/config.toml` (macOS:
   `~/Library/Application Support/gh-board/config.toml`)

Repo-local config (`.gh-board.toml`) merges on top of global config.

## Documentation

| Topic                   | File                                           |
| ----------------------- | ---------------------------------------------- |
| Configuration reference | [`examples/config.toml`](examples/config.toml) |
| Filter syntax           | [`FILTERS.md`](FILTERS.md)                     |
| Keybindings             | [`KEYBINDINGS.md`](KEYBINDINGS.md)             |
| Themes & colors         | [`THEME.md`](THEME.md)                         |
| Architecture            | [`ARCHITECTURE.md`](ARCHITECTURE.md)           |
| Contributing            | [`CONTRIBUTING.md`](CONTRIBUTING.md)           |

## Architecture Highlights

- **TUI Framework**: [iocraft](https://github.com/ccbrown/iocraft) for reactive,
  component-based UI
- **Async Runtimes**: smol (UI thread) + Tokio (engine thread)
- **Syntax Highlighting**: tree-sitter with custom color model integration
- **Markdown**: pulldown-cmark parser with custom ANSI renderer
- **GitHub API**: octocrab (GraphQL for PRs/issues, REST for notifications)
- **Cache**: moka in-process LRU cache with async support

## License

MIT

## Contributing

Contributions welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for development
setup, code style, and how to record demo assets.

## Acknowledgments

Inspired by [gh-dash](https://github.com/dlvhdr/gh-dash) by
[dlvhdr](https://github.com/dlvhdr). This project started to bring the uniform
ANSI + hex color model to the terminal GitHub dashboard space.

## Links

- Issues: <https://github.com/graelo/gh-board/issues>
- Discussions: <https://github.com/graelo/gh-board/discussions>
