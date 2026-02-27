# gh-board

A fast terminal dashboard for GitHub pull requests, issues, notifications, and
repository branches.

https://github.com/user-attachments/assets/31f0b649-8f4e-4eda-9274-8e47e81e241e

## Features

- **Multiple Views**: PRs, Issues, Notifications, and Repo (branches)
- **Configurable Filters**: Organize items with custom GitHub search filters
- **Rich Markdown Preview**: Full CommonMark rendering with syntax highlighting
  for 18+ languages
- **Uniform Color Model**: Use ANSI-256 indices OR hex colors consistently
  across UI, markdown, and syntax highlighting
- **Terminal Theme Respect**: ANSI indices 0-15 map to your terminal's palette
  (Solarized, Gruvbox, etc.)
- **Powerful Actions**: Approve, comment, assign, merge, checkout, label, and
  more — all from your keyboard
- **GitHub Enterprise Support**: Configure custom hosts per filter
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

See `examples/config.toml` for a comprehensive example.

## Key Concepts

| Concept    | Description                                                                                                                                                                                                                      |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **View**   | One of the four top-level content areas (PRs, Issues, Notifications, Repo). Switch between views with `n` / `N`.                                                                                                                 |
| **Filter** | A named search-filter group within a view, displayed as a tab at the top of the screen. Navigate filters with `h` / `l`. Configured via `[[pr_filters]]`, `[[issues_filters]]`, `[[notifications_filters]]` in your config file. |

## Usage

### Command Line

```bash
gh-board [COMMAND] [OPTIONS] [REPO]
```

**Subcommands:**

- `init`: Interactive wizard that generates a starter
  `~/.config/gh-board/config.toml`
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

### Configuration Structure

The config uses TOML with these main blocks:

```toml
theme_file = "builtin:dracula"      # Built-in theme, or path to a theme TOML file

[github]
scope = "auto"                      # "auto" (repo if in git dir), "repo", or "global"
refetch_interval_minutes = 10       # Cache TTL
prefetch_pr_details = 0             # Background-prefetch first N PR details (0 = off)

[defaults]
view = "prs"                        # Initial view: prs, issues, notifications, repo
date_format = "relative"            # Or strftime format

[defaults.preview]
width = 0.45                        # Fraction of terminal width (0.0-1.0)

[[pr_filters]]
title = "Filter Name"
filters = "is:open author:@me"      # GitHub search syntax
limit = 50                          # Optional max items
host = "github.com"                 # Optional GHE hostname

[[issues_filters]]
# Same structure as pr_filters

[[notifications_filters]]
title = "Unread"
filters = "is:unread"               # Supports: repo:, reason:, is:unread/read/done/all

# Required for the C / Space "Checkout branch" action.
# Maps "owner/repo" (GitHub full name) to the absolute path of your local clone.
# Supports ~/  (tilde is expanded at startup).
# If a repo is missing, checkout shows: "no local path configured for owner/repo"
[repo_paths]
"owner/repo" = "~/code/owner/repo"

[theme.ui]
filters_show_count = true

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

# Override an Actions view binding
[[keybindings.actions]]
key = "e"
builtin = "rerun_failed"
name = "Re-run failed jobs"
```

**Template Variables** (for custom commands):

- `{{.Url}}`: Item URL
- `{{.Number}}`: PR/issue number
- `{{.RepoName}}`: Repository (owner/repo)
- `{{.HeadBranch}}`: PR head branch
- `{{.BaseBranch}}`: PR base branch

## PR List Columns

The PR view displays the following columns:

| Column   | Header | Description                                                                                                                                                      |
| -------- | ------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| State    | (icon) | PR state: open, closed, merged, or draft                                                                                                                         |
| Title    | Title  | Two-line cell: repo + number + author on the first line, PR title on the second                                                                                  |
| Comments | (icon) | Comment count (blank when zero)                                                                                                                                  |
| Review   | (icon) | Review status derived from `reviewDecision` when available, otherwise inferred from the latest reviews: approved, changes requested, commented, pending, or none |
| CI       | (icon) | Aggregate CI status from check runs: success, failure, pending, or none                                                                                          |
| Lines    | (icon) | Lines changed (`+N -M`)                                                                                                                                          |
| Updated  | (icon) | Last updated date                                                                                                                                                |
| Created  | (icon) | Creation date                                                                                                                                                    |

## Keybindings

### Universal (All Views)

| Key           | Action                            |
| ------------- | --------------------------------- |
| `j` / `Down`  | Move cursor down                  |
| `k` / `Up`    | Move cursor up                    |
| `g` / `Home`  | Jump to first item                |
| `G` / `End`   | Jump to last item                 |
| `Ctrl+d`      | Half page down / scroll sidebar   |
| `Ctrl+u`      | Half page up / scroll sidebar     |
| `PageDown`    | Page down                         |
| `PageUp`      | Page up                           |
| `h` / `Left`  | Previous filter                   |
| `l` / `Right` | Next filter                       |
| `p`           | Toggle preview pane               |
| `o`           | Open in browser                   |
| `r`           | Refresh current filter            |
| `R`           | Refresh all filters (clear cache) |
| `/`           | Search / filter                   |
| `y`           | Copy number to clipboard          |
| `Y`           | Copy URL to clipboard             |
| `?`           | Toggle help overlay               |
| `q`           | Quit                              |

### PR View

| Key           | Action                        |
| ------------- | ----------------------------- |
| `v`           | Approve                       |
| `L`           | Label (autocomplete)          |
| `a`           | Assign/Unassign (multiselect) |
| `c`           | Comment                       |
| `d`           | View diff in pager            |
| `C` / `Space` | Checkout branch ¹             |
| `x`           | Close PR                      |
| `X`           | Reopen PR                     |
| `W`           | Mark as ready for review      |
| `m`           | Merge PR                      |
| `u`           | Update PR from base branch    |
| `Ctrl+]`      | Jump to Actions run           |
| `n`           | Switch view                   |
| `N`           | Switch view back              |
| `S`           | Toggle repo scope             |

¹ Requires `[repo_paths]` entry for the repo — see Configuration Structure.

### Issue View

| Key | Action                        |
| --- | ----------------------------- |
| `L` | Label (with autocomplete)     |
| `a` | Assign/Unassign (multiselect) |
| `c` | Comment                       |
| `x` | Close issue                   |
| `X` | Reopen issue                  |
| `n` | Switch view                   |
| `N` | Switch view back              |
| `S` | Toggle repo scope             |

### Notification View

| Key | Action            |
| --- | ----------------- |
| `m` | Mark as read      |
| `M` | Mark all as read  |
| `u` | Unsubscribe       |
| `n` | Switch view       |
| `N` | Switch view back  |
| `S` | Toggle repo scope |

### Actions View

| Key      | Action                    |
| -------- | ------------------------- |
| `w`      | Toggle workflow navigator |
| `Ctrl+t` | Go back to previous view  |
| `d`      | Close ephemeral tab       |
| `e`      | Re-run failed jobs        |
| `E`      | Re-run all jobs           |
| `Ctrl+x` | Cancel run                |
| `n`      | Switch view               |
| `N`      | Switch view back          |
| `S`      | Toggle repo scope         |

### Branches View

| Key               | Action                |
| ----------------- | --------------------- |
| `Enter` / `Space` | Checkout branch       |
| `Delete` / `D`    | Delete branch         |
| `+`               | Create new branch     |
| `p`               | Create PR from branch |
| `v`               | View PRs for branch   |
| `n`               | Switch view           |
| `N`               | Switch view back      |
| `S`               | Toggle repo scope     |

All keybindings are customizable via the config.

## Themes

gh-board ships with 24 built-in themes. Reference one with a single line in your
config:

```toml
theme_file = "builtin:dracula"
```

Or point to a custom theme file:

```toml
theme_file = "~/.config/gh-board/themes/my-theme.toml"
```

Run `gh-board themes` to list all available built-in theme names:

```text
ayu-dark, base16-default, catppuccin-latte, catppuccin-mocha,
dracula, everforest, gruvbox-dark, iceberg, kanagawa,
modus-operandi, modus-vivendi, monokai, nightfox, night-owl,
nord, one-dark, onehalf-dark, palenight, rose-pine,
solarized-16, solarized-dark, solarized-light, srcery,
tokyo-night, zenburn
```

> **`solarized-16`** uses ANSI indices 0–15 and relies on your terminal being
> configured with the Solarized palette. Light or dark depends on your
> terminal's own background color.

Any `[theme.*]` blocks in `config.toml` are merged on top of the theme file, so
you can selectively override individual colors without forking the whole theme.

---

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
[dlvhdr](https://github.com/dlvhdr). This project startedd to bring the uniform
ANSI + hex color model to the terminal GitHub dashboard space.

## Links

- Issues: <https://github.com/graelo/gh-board/issues>
- Discussions: <https://github.com/graelo/gh-board/discussions>
