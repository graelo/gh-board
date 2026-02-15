# gh-board

A blazingly fast terminal dashboard for GitHub pull requests, issues,
notifications, and repository branches. A Rust reimplementation of
[gh-dash](https://github.com/dlvhdr/gh-dash) with a uniform ANSI + hex color
model for perfect terminal theme integration.

## Features

- **Multiple Views**: PRs, Issues, Notifications, and Repo (branches)
- **Configurable Sections**: Organize items with custom GitHub search filters
- **Rich Markdown Preview**: Full CommonMark rendering with syntax highlighting
  for 18+ languages
- **Uniform Color Model**: Use ANSI-256 indices OR hex colors consistently
  across UI, markdown, and syntax highlighting
- **Terminal Theme Respect**: ANSI indices 0-15 map to your terminal's palette
  (Solarized, Gruvbox, etc.)
- **Powerful Actions**: Approve, comment, assign, merge, checkout, label, and
  more — all from your keyboard
- **GitHub Enterprise Support**: Configure custom hosts per section
- **Customizable Keybindings**: Rebind any action or define custom shell
  commands
- **Configurable Icons**: Ship with `unicode`, `nerdfont`, and `ascii` presets,
  with per-icon overrides
- **Repo Scope Auto-Detection**: Auto-detects `owner/repo` from git remotes;
  toggle between repo-scoped and global modes with `S`
- **Smart Caching**: In-memory LRU cache with configurable TTL
- **Fast Startup**: Under 500ms to first render on cold cache

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
[[pr_sections]]
title = "My PRs"
filters = "is:open author:@me"

[[pr_sections]]
title = "Needs Review"
filters = "is:open review-requested:@me"

[[issues_sections]]
title = "Assigned to Me"
filters = "is:open assignee:@me"
```

2. Run `gh-board` (or `gh board`).

3. Navigate with `j`/`k`, switch sections with `h`/`l`, press `?` for help.

See `examples/config.toml` for a comprehensive example.

## Usage

### Command Line

```bash
gh-board [OPTIONS] [REPO]
```

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

### Configuration Structure

The config uses TOML with these main sections:

```toml
[defaults]
view = "prs"                        # Initial view: prs, issues, notifications, repo
scope = "auto"                      # "auto" (repo if in git dir), "repo", or "global"
refetch_interval_minutes = 10       # Cache TTL
date_format = "relative"            # Or strftime format

[defaults.preview]
width = 0.45                        # Fraction of terminal width (0.0-1.0)

[[pr_sections]]
title = "Section Name"
filters = "is:open author:@me"      # GitHub search syntax
limit = 50                          # Optional max items
host = "github.com"                 # Optional GHE hostname

[[issues_sections]]
# Same structure as pr_sections

[[notifications_sections]]
title = "Unread"
filters = "is:unread"               # Supports: repo:, reason:, is:unread/read/done/all

[repo_paths]
"owner/repo" = "/path/to/local/clone"  # For checkout operations

[theme.ui]
sections_show_count = true

[theme.ui.table]
show_separator = true
compact = false

[theme.colors.text]
primary = "7"                       # ANSI index or "#RRGGBB"
secondary = "8"
# ... (see examples/config.toml for all options)

[theme.colors.markdown]
heading = "#89b4fa"
code = "6"
# ...

[theme.colors.markdown.syntax]
keyword = "4"
string = "2"
# ... (30+ token types)

[theme.icons]
preset = "unicode"                  # "unicode" (default), "nerdfont", or "ascii"
pr_open = "●"                       # Override individual icons
# ... (see examples/config.toml for all icon fields)

[[keybindings.universal]]
key = "q"
builtin = "quit"
name = "Quit"

[[keybindings.prs]]
key = "v"
builtin = "approve"
name = "Approve PR"

# Custom shell command with template variables
[[keybindings.prs]]
key = "ctrl+b"
command = "open {{.Url}}"
name = "Open in browser"
```

**Template Variables** (for custom commands):

- `{{.Url}}`: Item URL
- `{{.Number}}`: PR/issue number
- `{{.RepoName}}`: Repository (owner/repo)
- `{{.HeadBranch}}`: PR head branch
- `{{.BaseBranch}}`: PR base branch

## PR List Columns

The PR view displays the following columns:

| Column   | Header | Description                                                                                                                                                     |
| -------- | ------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| State    | (icon) | PR state: open, closed, merged, or draft                                                                                                                        |
| Title    | Title  | Two-line cell: repo + number + author on the first line, PR title on the second                                                                                 |
| Comments | (icon) | Comment count (blank when zero)                                                                                                                                 |
| Review   | (icon) | Review status derived from `reviewDecision` when available, otherwise inferred from the latest reviews: approved, changes requested, commented, pending, or none |
| CI       | (icon) | Aggregate CI status from check runs: success, failure, pending, or none                                                                                         |
| Lines    | (icon) | Lines changed (`+N -M`)                                                                                                                                         |
| Updated  | (icon) | Last updated date                                                                                                                                               |
| Created  | (icon) | Creation date                                                                                                                                                   |

## Keybindings

### Universal (All Views)

| Key                   | Action                             |
| --------------------- | ---------------------------------- |
| `j` / `Down`          | Move cursor down                   |
| `k` / `Up`            | Move cursor up                     |
| `g` / `Home`          | Jump to first item                 |
| `G` / `End`           | Jump to last item                  |
| `Ctrl+d` / `PageDown` | Page down (preview pane)           |
| `Ctrl+u` / `PageUp`   | Page up (preview pane)             |
| `h` / `Left`          | Previous section                   |
| `l` / `Right`         | Next section                       |
| `p`                   | Toggle preview pane                |
| `o`                   | Open in browser                    |
| `r`                   | Refresh current section            |
| `R`                   | Refresh all sections (clear cache) |
| `/`                   | Search / filter                    |
| `y`                   | Copy number to clipboard           |
| `Y`                   | Copy URL to clipboard              |
| `?`                   | Toggle help overlay                |
| `q` / `Ctrl+c`        | Quit                               |

### PR View

| Key           | Action                     |
| ------------- | -------------------------- |
| `v`           | Approve                    |
| `a`           | Assign                     |
| `A`           | Unassign                   |
| `c`           | Comment                    |
| `d`           | View diff in pager         |
| `C` / `Space` | Checkout branch            |
| `x`           | Close PR                   |
| `X`           | Reopen PR                  |
| `W`           | Mark as ready for review   |
| `m`           | Merge PR                   |
| `u`           | Update PR from base branch |
| `n`           | Switch view                |
| `N`           | Switch view back           |
| `S`           | Toggle repo scope          |

### Issue View

| Key | Action                    |
| --- | ------------------------- |
| `L` | Label (with autocomplete) |
| `a` | Assign                    |
| `A` | Unassign                  |
| `c` | Comment                   |
| `x` | Close issue               |
| `X` | Reopen issue              |
| `n` | Switch view               |
| `N` | Switch view back          |
| `S` | Toggle repo scope         |

### Notification View

| Key     | Action                    |
| ------- | ------------------------- |
| `Enter` | View notification details |
| `D`     | Mark as done              |
| `Alt+d` | Mark all as done          |
| `m`     | Mark as read              |
| `M`     | Mark all as read          |
| `u`     | Unsubscribe               |
| `n`     | Switch view               |
| `N`     | Switch view back          |
| `S`     | Toggle repo scope         |

### Branches View

| Key      | Action                |
| -------- | --------------------- |
| `Delete` | Delete branch         |
| `+`      | Create new branch     |
| `p`      | Create PR from branch |
| `v`      | View PRs for branch   |
| `n`      | Switch view           |
| `N`      | Switch view back      |
| `S`      | Toggle repo scope     |

All keybindings are customizable via the config.

## Color Model

The uniform color model is the architectural centerpiece:

- **ANSI-256 Indices**: String values `"0"` through `"255"`
  - Indices 0-15 map to your terminal's palette colors (theme-aware)
  - Indices 16-255 are fixed colors
- **Hex Colors**: Standard hex strings: `#RRGGBB` or `#RGB`
- **Everywhere**: The same format works for UI, markdown, and syntax
  highlighting

Example (Gruvbox theme using ANSI indices):

```toml
[theme.colors.text]
primary = "7"       # Uses terminal's color 7 (white/fg)
faint = "8"         # Uses terminal's color 8 (bright black/gray)

[theme.colors.markdown.syntax]
keyword = "4"       # Uses terminal's color 4 (blue)
string = "2"        # Uses terminal's color 2 (green)
```

On terminals with lower color depth, hex colors degrade gracefully. A warning is
logged at startup if using 256-color indices on a 16-color terminal.

## Supported Languages (Syntax Highlighting)

Rust, Go, Python, JavaScript, TypeScript, Ruby, Bash, JSON, TOML, YAML,
Markdown, HTML, CSS, SQL, C, C++, Java, Dockerfile

## Development

### Build

```bash
cargo build
```

### Test

```bash
cargo test
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
[dlvhdr](https://github.com/dlvhdr). This project exists to bring the uniform
ANSI + hex color model to the terminal GitHub dashboard space.

## Links

- Issues: <https://github.com/graelo/gh-board/issues>
- Discussions: <https://github.com/graelo/gh-board/discussions>
