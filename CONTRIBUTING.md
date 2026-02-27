# Contributing

Contributions welcome! Please open an issue first for major changes so we can
discuss the approach.

## Development setup

### Prerequisites

- Rust toolchain (MSRV 1.88.0)
- [cargo-nextest](https://nexte.st/) for running tests
- The [GitHub CLI](https://cli.github.com/) (`gh`) authenticated, or a
  `GITHUB_TOKEN` / `GH_TOKEN` environment variable

### Build & test

```bash
cargo build
cargo nextest run               # unit + integration tests
cargo test --doc                # doctests (nextest doesn't run these)
cargo clippy                    # pedantic flags configured in .cargo/config.toml
cargo fmt --all -- --check      # formatting check
```

### Full CI check

```bash
./ci/test_full.sh               # runs all of the above, also checks MSRV
```

### Debug logging

```bash
gh-board --debug                # logs written to debug.log
LOG_LEVEL=trace gh-board --debug
```

## Code style

- Clippy runs with `-Wclippy::pedantic` (configured in `.cargo/config.toml`).
  Targeted `#[allow]` suppressions exist for `module_name_repetitions`,
  `must_use_candidate`, and `missing_errors_doc`.
- Prefer editing existing files over creating new ones.
- Keep changes minimal — don't refactor surrounding code as part of a bug fix.

## Recording demo assets

Demo videos and screenshots live in `demo/` and are recorded with
[VHS](https://github.com/charmbracelet/vhs).

### Files

| File | Purpose |
|------|---------|
| `demo/hero.tape` | VHS script — captures a static screenshot (`hero.png`) |
| `demo/hero.png` | Static screenshot used as the video poster frame |
| `demo/nav.tape` | VHS script — records the navigation walkthrough |
| `demo/nav.gif` | GIF output of the walkthrough |
| `demo/nav.mp4` | MP4 output of the walkthrough (embedded in README) |

### Re-recording

```bash
# Install VHS: https://github.com/charmbracelet/vhs#installation

# 1. Record the hero screenshot
vhs demo/hero.tape              # produces demo/hero.png and demo/hero.gif

# 2. Record the navigation walkthrough
vhs demo/nav.tape               # produces demo/nav.gif and demo/nav.mp4
```

Both tapes require a working gh-board config with filters that return results.
Adjust the `Sleep` durations and key presses in the tape files to match your
data.

### Adding a poster frame

GitHub's video player uses the first frame as its thumbnail. Since the VHS
recording starts from a blank terminal, prepend the hero screenshot as a
1-second still frame:

```bash
ffmpeg -loop 1 -framerate 25 -t 1 -i demo/hero.png \
  -i demo/nav.mp4 \
  -filter_complex \
    "[0:v]format=yuv420p[poster];[1:v]format=yuv420p[main];[poster][main]concat=n=2:v=1:a=0[out]" \
  -map "[out]" -c:v libx264 -preset medium -crf 18 -movflags +faststart \
  demo/nav-final.mp4

mv demo/nav-final.mp4 demo/nav.mp4
```

Verify the first frame:

```bash
ffmpeg -i demo/nav.mp4 -vframes 1 /tmp/poster-check.png
open /tmp/poster-check.png
```

### Updating the README video

The README embeds the video via a GitHub `user-attachments` URL (not a local
file path). After producing a new `demo/nav.mp4`:

1. Open a GitHub issue or PR comment in the repo
2. Drag-and-drop `demo/nav.mp4` into the comment box
3. GitHub processes the upload and inserts a `user-attachments` URL
4. Copy that URL and replace line 6 of `README.md`
5. Discard the comment (the uploaded asset persists)

## Project layout

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full module map and design
decisions.
