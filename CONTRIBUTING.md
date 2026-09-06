# Contributing

Contributions welcome! Please open an issue first for major changes so we can
discuss the approach.

## Development setup

### Prerequisites

- Rust toolchain (MSRV 1.95.0)
- [cargo-nextest](https://nexte.st/) for running tests
- The [GitHub CLI](https://cli.github.com/) (`gh`) authenticated, or a
  `GITHUB_TOKEN` / `GH_TOKEN` environment variable

`make check-all` also requires cargo-deny, cargo-pants, Convco, Poutine,
Zizmor, and Rumdl. `make coverage` requires cargo-llvm-cov.

### Build, test, check

The `Makefile` is the canonical definition of local tasks; run `make help` to
list them. The ones needed day to day are:

```bash
cargo build         # debug build
make release        # release build
make test           # full test suite, including doctests and the MSRV check
make check          # fmt + lint + test — run before `git push`
make check-all      # adds audits, commit lint, Markdown, and workflow checks
make fix            # auto-format and apply Clippy fixes
```

For focused tests, use Nextest directly:

```bash
cargo nextest run <test_name>
cargo nextest run --test config_test
```

### Code coverage

```bash
make coverage
```

The HTML report is written to `target/llvm-cov/html/index.html`.

### Debug logging

```bash
gh-board --debug                # logs written to debug.log
LOG_LEVEL=trace gh-board --debug
```

## Commit messages

Commits follow [Conventional Commits](https://www.conventionalcommits.org/).
`make commits` (part of `make check-all`) verifies the branch with Convco, and
the same check runs in CI — non-conforming messages fail the build.

```text
<type>(<optional scope>): <description>
```

Common types in this repo: `feat`, `fix`, `refactor`, `docs`, `test`, `build`,
`chore`, `ci`. Release commits use `build(release): gh-board v<version>`.

## Code style

- Clippy runs with `-Wclippy::pedantic` (configured in `.cargo/config.toml`).
  Targeted `#[allow]` suppressions exist for `module_name_repetitions`,
  `must_use_candidate`, and `missing_errors_doc`.
- Prescriptive rules — view structure, state binding, module visibility — live
  in [CONVENTIONS.md](CONVENTIONS.md). Read it before adding a view or
  touching the engine boundary.

## Project layout

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full module map and design
decisions.

## Maintainer tasks

Releasing and demo-asset recording are documented in
[MAINTAINING.md](MAINTAINING.md).
