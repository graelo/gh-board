# AGENTS.md

This file contains instructions for coding agents working in this repository.

## Verification

The `Makefile` is the canonical definition of local verification tasks. Read it
before choosing commands, or run `make help` to list every target.

```bash
cargo build                          # debug build
make release                         # release build
make check                           # pre-push gate: fmt + lint + test
make check-all                       # pre-PR gate: adds audits and security checks
make fix                             # auto-format and apply Clippy fixes
make coverage                        # HTML report in target/llvm-cov/html/
cargo nextest run <test_name>        # focused test
cargo nextest run --test config_test # focused integration test file
cargo run -- --debug                 # debug log → ./debug.log
```

`make test` runs the full suite, including doctests and the MSRV 1.95.0 check.
The lint target uses stable Rust with locked dependencies and all features,
matching CI. `make check-all` assumes its external audit, commit, Markdown,
and workflow-security tools are installed locally.

## Architecture

Rust 2024 edition, MSRV 1.95.0. TUI via iocraft + smol; GitHub API via octocrab

- tokio; moka LRU cache.

### Dual async runtimes (thread-isolated)

- **UI thread** — `smol::block_on(...)` drives iocraft. No tokio here.
- **Engine thread** — `std::thread::spawn` + `tokio::runtime::Runtime`. Owns
    octocrab, moka, all API calls.
- **UI → Engine** — `tokio::sync::mpsc::UnboundedSender<Request>` in
    `EngineHandle`.
- **Engine → UI** — per-request `std::sync::mpsc::Sender<Event>`.
- No `async-compat`/`Compat::new` anywhere.

### Key modules

- `engine/interface.rs` — `EngineHandle`, `Request`/`Event` enums (the API
    contract)
- `engine/github.rs` — real impl; `engine/stub.rs` — fixture-based test impl
- `types/` — shared domain types (UI + engine both import from here)
- `github/` — `pub(crate)` API layer, only imported by `engine/github.rs`
- `config/types.rs` — `AppConfig`, filters, theme structs
- `config/loader.rs` — multi-source config: `--config` → `.gh-board.toml` →
    `$GH_BOARD_CONFIG` → XDG → `~/.config/gh-board/config.toml`

### Config merging

Local overrides global per-key. Filter lists replace global only when non-empty.

### Theme pipeline

`Color` is `Copy` (`Ansi256 | Hex`). `Theme::merge(base, overlay)` →
`ResolvedTheme` (fully concrete, used by all components). Builtins in
`examples/themes/*.toml`, embedded via `builtin_themes.rs`.

### Tests

Integration tests in `tests/`. Fixtures in `tests/fixtures/`.

### Keybinding template variables

`{{.Url}}`, `{{.Number}}`, `{{.RepoName}}`, `{{.HeadBranch}}`, `{{.BaseBranch}}`
