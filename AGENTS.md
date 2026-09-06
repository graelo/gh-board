# AGENTS.md

Operational briefing for coding agents working in this repository.

This file routes and constrains; it does not decide. Rules and their rationale
live in the documents linked below — when they disagree with this file, they
win, and this file is the thing to fix.

## Where things are defined

| Looking for                                   | Read                                 |
| --------------------------------------------- | ------------------------------------ |
| What is acceptable, code style, commit format | [CONTRIBUTING.md](CONTRIBUTING.md)   |
| Runtime model, module map, data flow          | [ARCHITECTURE.md](ARCHITECTURE.md)   |
| Prescriptive code rules, view structure       | [CONVENTIONS.md](CONVENTIONS.md)     |
| Releasing, demo assets                        | [MAINTAINING.md](MAINTAINING.md)     |
| Config discovery and merge order              | [README.md](README.md#configuration) |
| Keybindings and template variables            | [KEYBINDINGS.md](KEYBINDINGS.md)     |
| Filter syntax                                 | [FILTERS.md](FILTERS.md)             |
| Theme reference                               | [THEME.md](THEME.md)                 |
| Every local task                              | [`Makefile`](Makefile) — `make help` |

## Definition of done

```bash
make check       # fmt + lint + test — must pass before handing work back
make check-all   # pre-PR gate: adds audits, commit lint, Markdown, workflows
```

Rust 2024 edition, MSRV 1.95.0 — `make test` refuses to run on an older
toolchain. `make check-all` assumes its external audit, commit, Markdown, and
workflow-security tools are installed locally; see
[CONTRIBUTING.md](CONTRIBUTING.md#prerequisites).

While iterating:

```bash
cargo nextest run <test_name>        # focused test
cargo nextest run --test config_test # focused integration test file
cargo run -- --debug                 # debug log → ./debug.log
make fix                             # auto-format, apply Clippy fixes
```

## Guardrails

Restatements of rules defined elsewhere, kept here because they apply to
nearly every task. The source is authoritative.

- **No `async-compat` / `Compat::new`.** The two runtimes are thread-isolated
  by design — smol drives the UI, tokio owns the engine.
  (source: [ARCHITECTURE.md](ARCHITECTURE.md#runtime-and-thread-model))
- **Views never import `crate::github::*`.** Domain types come from
  `crate::types::*`; `github/` is `pub(crate)` and belongs to
  `engine/github.rs` alone.
  (source: [CONVENTIONS.md](CONVENTIONS.md#6-module-visibility))
- **No speculative complexity.** Three similar lines beat a premature
  abstraction; remove code completely rather than leaving shims.
  (source: [CONVENTIONS.md](CONVENTIONS.md#7-no-speculative-complexity))
- **Conventional Commits**, verified by `make commits` and by CI.
  (source: [CONTRIBUTING.md](CONTRIBUTING.md#commit-messages))
- **Prefer editing existing files over creating new ones.**
- **Keep changes minimal** — don't refactor surrounding code as part of a bug
  fix.

## When a gate fails

- `make fmt` / `make lint` — run `make fix` first; it resolves most of both.
- `make lint` — Clippy runs with `-Wclippy::pedantic` and `-D warnings`, so
  pedantic lints are hard failures. Prefer restructuring the code; when a
  suppression is genuinely warranted, use `#[expect(clippy::…)]` with a
  reason rather than `#[allow]`.
- `make test` — integration tests run against `engine/stub.rs` and its
  fixtures in `tests/fixtures/`, never the network. A test that suddenly
  needs a token is a sign the stub path was bypassed.
- `make md` — Markdown is linted against `rumdl.toml`; documentation edits are
  subject to it too.
