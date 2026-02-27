# gh-board

A fast terminal dashboard for GitHub pull requests, issues, notifications, and
repository branches.

https://github.com/user-attachments/assets/31f0b649-8f4e-4eda-9274-8e47e81e241e

## Features

**Five views, one dashboard** — switch between them with `n`/`N`, organize each
with named filter tabs (`h`/`l`), and press `?` anywhere for contextual help.

### Review & merge PRs

Approve, comment, assign, label, merge, checkout branch, view diff, and
deep-link to CI runs — all from your keyboard.

### Triage issues

Assign, label, comment, close/reopen with confirmation prompts.

### Manage notifications

Mark read, unsubscribe, filter by reason, repo, or status.

### Monitor CI/CD

Browse workflow runs, re-run failed jobs, cancel runs. Jump straight from a PR's
check status to its Actions run with `Ctrl+]`.

### Work with branches

Checkout, delete, create new branches, and open PRs — without leaving the
terminal.

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
gh board [REPO]
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
gh-board [COMMAND] [OPTIONS] [REPO]
```

**Subcommands:**

- `init`: Interactive wizard that generates a starter config
- `themes`: List all built-in theme names

**Options:**

- `-c, --config <PATH>`: Use a specific config file
- `--debug`: Enable debug logging to `debug.log`
- `-h, --help`: Show help
- `-v`: Show version

**Arguments:**

- `[REPO]`: Optional repository in `owner/repo` format to scope the view

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

| Topic | File |
|-------|------|
| Configuration reference | [`examples/config.toml`](examples/config.toml) |
| Filter syntax | [`FILTERS.md`](FILTERS.md) |
| Keybindings | [`KEYBINDINGS.md`](KEYBINDINGS.md) |
| Themes & colors | [`THEME.md`](THEME.md) |
| Architecture | [`ARCHITECTURE.md`](ARCHITECTURE.md) |

## Development

### Build

```bash
cargo build
```

### Test

```bash
cargo nextest run
cargo test --doc
cargo clippy
```

### Debug Logging

```bash
gh-board --debug
# Logs written to debug.log
# Control level via LOG_LEVEL env var
```

## Architecture Highlights

- **TUI Framework**: [iocraft](https://github.com/ccbrown/iocraft) for reactive,
  component-based UI
- **Async Runtime**: smol (iocraft's internal runtime)
- **Syntax Highlighting**: tree-sitter with custom color model integration
- **Markdown**: pulldown-cmark parser with custom ANSI renderer
- **GitHub API**: octocrab (GraphQL for PRs/issues, REST for notifications)
- **Cache**: moka in-process LRU cache with async support

## License

MIT

## Contributing

Contributions welcome. Please open an issue first for major changes.

## Acknowledgments

Inspired by [gh-dash](https://github.com/dlvhdr/gh-dash) by
[dlvhdr](https://github.com/dlvhdr). This project started to bring the uniform
ANSI + hex color model to the terminal GitHub dashboard space.

## Links

- Issues: <https://github.com/graelo/gh-board/issues>
- Discussions: <https://github.com/graelo/gh-board/discussions>
