# Changelog

All notable changes to gh-board are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/), and this
project adheres to [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.12.0] - 2026-04-21

### Added

- **Repo picker** — press `s` to open a selection overlay listing all repos
  from `[repo_paths]`. The detected local repo appears first (marked with an
  anchor icon), followed by config entries in TOML insertion order. Selecting a
  repo sets it as the active scope; `S` then toggles between the selected repo
  and global as usual. Esc cancels
- **`repo_anchor` icon** — new icon field in all three presets (unicode: ⚓,
  nerdfont: nf-fa-anchor, ascii: @), overridable via `[theme.icons]`

### Changed

- **`repo_paths` uses `IndexMap`** — preserves TOML insertion order instead of
  random `HashMap` ordering. The generic `BuildHasher` parameter on
  `actions::local` functions is dropped as a result

## [0.11.1] - 2026-04-20

### Changed

- **MSRV bumped to 1.95.0** — enables `Duration::from_mins`, let-chains, and
  `#[expect]` lint attributes
- **Clippy suppression overhaul** — all 51 `#[allow(clippy::...)]` eliminated;
  25 unavoidable suppressions converted to `#[expect]` which errors if the lint
  no longer fires. Introduced color config structs (`SidebarColors`,
  `FooterColors`, `TabBarColors`, `TextInputColors`) and param bundles
  (`FooterContent`, `SidebarTabConfig`, `InputContext`, `RunContext`) to reduce
  argument counts. Extracted helpers from long functions: `handle_request` split
  into 35 per-variant async handlers, shared GraphQL extraction helpers,
  `handle_multiselect_input` replacing 4 duplicated assign/label input handlers,
  PR cell builders, alert detail formatters, sidebar visual layout, init wizard
  prompts
- **CI hardening** — install cargo-nextest via `taiki-e/install-action`
  (SHA-pinned) instead of curl; add `.config/nextest.toml` with ci profile; move
  poutine config to `.github/poutine.yml`; add `CODEOWNERS`; clippy with
  `--all-targets --all-features`; fix poutine-action pin to v1.1.4 for config
  discovery
- **Renovate** — switch to daily schedule, enable `platformAutomerge` and
  `automerge` on patch/minor updates

### Added

- **80 new unit tests** covering GraphQL extraction helpers, PR cell builders,
  alert detail formatters, table body row rendering, sidebar visual layout,
  scrollbar geometry, and init config generation
- **4 doc tests** for `Color::parse`, `RepoRef::full_name`,
  `RepoRef::from_full_name`, and `parse_github_url`

### Fixed

- Resolve nightly clippy lints (`duration_suboptimal_units`,
  `float_literal_f32_fallback`)
- Pin poutine-action to v1.1.4 (was v1.1.2) so `.github/poutine.yml` skip list
  is discovered — fixes 12 false-positive code scanning alerts

## [0.11.0] - 2026-04-15

### Added

- **Alerts view** — 6th view surfacing Dependabot, Code Scanning (CodeQL,
  zizmor, poutine, …), and Secret Scanning alerts via GitHub REST API. Two-level
  tree navigator groups by category and tool. Tabbed sidebar with severity pill,
  markdown-rendered summary, and category-specific detail tab (Remediation /
  Instances / Locations). Secret alert locations lazy-fetched on demand.
  Configured via `[[alerts_filters]]` with `@current` support. Yellow-themed
  footer color across all 25 builtin themes. Dedicated `[keybindings.alerts]`
  config section
- **Goto-view keybindings** (`1`–`6`) — jump directly to any view without
  cycling through `n`/`N`. Updates `previous_view` so `ctrl+t` returns to the
  origin
- **Sidebar resize** (`+`/`-`/`=`) — widen or narrow the sidebar by 5% per
  press, reset to config default with `=`. Width is shared across views and
  persists through sidebar toggle. Clamped to 15%–80%

## [0.10.3] - 2026-04-15

### Added

- **Config file discovery** — support `gh-board.toml` (without dot prefix)
  alongside `.gh-board.toml` for repo-local configuration

### Changed

- **CI hardening** — pin all actions to commit SHAs, least-privilege
  permissions, persist-credentials: false, template injection fixes, cache
  restricted to PRs, semver-only tag filter, 1-day artifact retention
- **Supply chain audits** — add reusable cargo-audit and ci-security workflows
  (zizmor + poutine), conditional on file changes and scheduled Tue/Fri
- **Release workflow** — replace ncipollo/release-action with gh CLI, add build
  provenance attestation via Sigstore
- **Secrets management** — replace PATs with GitHub App tokens for Homebrew and
  Renovate jobs, scoped to dedicated environments
- **Renovate config** — add pinDigests for GitHub Actions, set gitAuthor to
  graelo-ci-bot

## [0.10.2] - 2026-04-10

### Added

- **Cross-fork PRs** — checkout and worktree actions now work for PRs opened
  from forks; the fork owner is added as a named git remote using the user's
  preferred protocol (SSH/HTTPS) and the branch tracks the fork's remote

## [0.10.1] - 2026-03-24

### Changed

- **Dependencies** — bump deps (iocraft 0.8)

## [0.10.0] - 2026-03-24

### Added

- **Actions duration** — add duration column to workflow runs table

### Changed

- Bump dependencies
- Bump bump-homebrew-formula-action to v4

## [0.9.0] - 2026-03-21

### Added

- **Watch workflow run** — add watch mode for workflow runs with completion hook
  (`f4afa71`)
- **`{{.ConclusionEmoji}}` template variable** for watch keybinding templates
- **Granular CI/check status icons** — separate icons for running, skipped,
  cancelled, and action-required states

### Fixed

- Replace watched-run icon with `nf-seti-search`
- Adapt watch tick rate to configured poll interval
- Use `text.secondary` for queued CI/check status color

### Changed

- Use dashed line for row separators in tables
- Bump GitHub Actions to node24-compatible versions
- Bump dependencies

## [0.8.1] - 2026-03-20

### Changed

- **Config merging** — when both global and repo-local config exist, they are
  now merged recursively: local values override global per-key, missing keys
  fall back to global defaults, filter lists replace global only when non-empty,
  `repo_paths` from both configs are merged
- **Config fields wrapped in `Option<T>`** — all configuration fields now use
  `Option<T>` to distinguish between explicit values and missing defaults
- Keybindings merge across contexts via `KeybindingsConfig::merge()`

### Fixed

- App initialization to handle `Option<T>` config fields
- Engine initialization with `refetch_interval_minutes` fallback
- Stale docstring, imports, and test correctness in config modules
- Clippy warnings in config loader
- Preserve global settings when local config is partial

## [0.8.0] - 2026-03-08

### Added

- **Repo view** — worktree column, toggleable sidebar, files sidebar tab,
  JumpToPr action, all-repos mode with scope filtering
- **Per-item refresh** with combined GraphQL queries
- **Persistent logging** — always log warn+ to `~/.cache/gh-board/gh-board.log`
  (`--debug` lowers to debug level in `./debug.log`)
- **API rate-limit display** in footer, split by API pool (GraphQL vs REST)
- **Per-request timeout** to prevent hung HTTP calls from blocking refresh
- **Typed ActionFeedback** with icons and dedicated status slot in footer
- **Worktree creation** from branch view (`cfa64c0`)
- Pre-commit config for local checks

### Fixed

- Premature ephemeral tab on deep-link race
- Duplicate `FetchRunJobs` and missing repo refresh feedback
- Excessive GitHub API consumption from PR fetches
- False-positive 403 rate-limit detection
- Compare ahead/behind against origin instead of local default
- Propagate `rate_limit` in `IssueDetailFetched`
- Sticky refresh status message in repo view

### Changed

- Remap create-pr-from-branch from `p` to `P`; use `c` for checkout
- Always prompt for confirmation before worktree creation
- Consolidate `Request` enum variants, extract helpers, centralize rate-limit
  state

## [0.7.0] - 2026-02-28

### Added

- **`gh-board open <URL>`** — deep-link to PRs, issues, and actions runs
- **Worktree keybinding (`w`)** — create/open git worktree for PR branch
- **Auto-clone** — automatically clone repo when path is missing; new
  `auto_clone` option in `[github]` section

### Fixed

- Bypass moka cache for PR detail on refresh and after mutations
- Swap checkout/comment keys in PRs view
- Fetch remote branch before `git checkout`
- Improve error messages for failed branch checkout
- Use correct ref format for same-repo PRs in compare view

## [0.6.0] - 2026-02-27

### Added

- **GitHub emoji shortcodes** — expand shortcodes in commits and checks
- Wire all remaining hardcoded icons and actions status icons through
  `ResolvedIcons`
- Per-view accent color for scrollbar thumb

### Fixed

- Scope toggling for Actions view
- Sidebar scrollbar height estimation, scroll offset clamping, and content
  alignment
- Table column width clamping to prevent scrollbar occlusion
- Sort workflow groups and jobs alphabetically for stable display
- Align check duration columns in sidebar Checks tab
- Align file diff stats columns in sidebar Files tab
- Add missing actions footer color to all 25 builtin themes

### Changed

- Switch test runner to cargo-nextest

## [0.5.0] - 2026-02-25

### Added

- **Ephemeral tabs** for deep-linked repos in Actions view
- Duration columns and workflow grouping in sidebar checks

## [0.4.0] - 2026-02-24

### Added

- **Deep-link navigation** from PR checks to Actions run
- **Half-page scroll** (`HalfPageDown`/`HalfPageUp`) for sidebar paging
- **Help overlay** — two-column side-by-side layout
- Created/updated dates in sidebar overview tab
- Author+role moved from pill line to metadata section

### Fixed

- Skip branch update status for closed and merged PRs
- In-flight tracking for deep-link `FetchRunById` gate
- Account for row separator border in visible_rows calculation
- Normalize crossterm legacy ctrl+bracket key mapping

## [0.3.0] - 2026-02-23

### Added

- **GitHub Actions view** — workflow runs with sidebar, rerun, and cancel
- **Label assignment** with multiselect and autocomplete (`L` key)
- **Assignee multiselect dialog** replacing text-based input
- **`@current` sentinel** for `ActionsFilter.repo`
- REST rate limit extraction for Actions and Notifications
- `MergedBindings::resolve()` wired into all five views
- Title column truncation with ellipsis

### Fixed

- Sidebar job-fetch under workflow nav filter
- Missing universal keybindings `r`, `R`, `y` in actions view
- GHE support: use per-filter host in `FetchRunJobs`

### Changed

- Confirmations migrated to `y`/`n` `InputMode::Confirm`
- Remove ctrl+c quit; use ctrl+x for cancel_run
- Swap actions/repo footer colors

## [0.2.0] - 2026-02-21

### Added

- **Configurable background prefetch** for PR details
- **Branch update status** in PR table and sidebar
- **Commit check status** in PR sidebar Commits tab
- **24 builtin themes** and interactive `init` wizard
- **Background refresh** for issues and notifications
- **Rate-limit monitoring** re-added
- Labels/collaborators autocomplete via engine
- Mark notification as done
- Notification `-reason:` exclusion filter syntax
- `R` keybinding to refresh all filters

### Fixed

- Bypass cache and honour configured refresh interval
- Register background refresh once at mount
- Always-visible footer with API rate limit display for notifications
- Expand tilde in `repo_paths`
- Convert API URLs to HTML URLs for notification `o` shortcut
- Decouple poll period from refresh interval
- Use wall-clock time for refresh due-check
- Re-register background refresh when scope changes
- Skip prefetch of detail for closed and merged PRs

### Changed

- Restructure config with `[github]` and `[defaults]` sections
- Backend-frontend split into engine/views architecture
- Plain binaries added to release for `gh extension install`

## [0.1.0] - 2026-02-17

Initial release — terminal dashboard for GitHub pull requests, issues, and
notifications with configurable filters, themes, and keybindings.

[Unreleased]: https://github.com/graelo/gh-board/compare/v0.12.0...HEAD
[0.12.0]: https://github.com/graelo/gh-board/compare/v0.11.1...v0.12.0
[0.11.1]: https://github.com/graelo/gh-board/compare/v0.11.0...v0.11.1
[0.11.0]: https://github.com/graelo/gh-board/compare/v0.10.3...v0.11.0
[0.10.3]: https://github.com/graelo/gh-board/compare/v0.10.2...v0.10.3
[0.10.2]: https://github.com/graelo/gh-board/compare/v0.10.1...v0.10.2
[0.10.1]: https://github.com/graelo/gh-board/compare/v0.10.0...v0.10.1
[0.10.0]: https://github.com/graelo/gh-board/compare/v0.9.0...v0.10.0
[0.9.0]: https://github.com/graelo/gh-board/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/graelo/gh-board/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/graelo/gh-board/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/graelo/gh-board/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/graelo/gh-board/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/graelo/gh-board/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/graelo/gh-board/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/graelo/gh-board/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/graelo/gh-board/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/graelo/gh-board/releases/tag/v0.1.0
