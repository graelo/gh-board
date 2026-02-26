# Sidebar scroll clamping: visual row estimation gap

## Problem

The sidebar (PR preview, issue preview, actions job detail) stores a **logical
line index** as its scroll offset. When the user presses `ctrl+d`/`ctrl+u`, this
offset increments or decrements. Without clamping, it grows past the content
length, requiring excess key presses to scroll back.

Clamping requires knowing the **maximum scroll offset** such that the remaining
content still fills the viewport. This in turn requires knowing how many
**visual rows** each logical line occupies after wrapping.

## The estimation gap

We estimate visual rows with:

```rust
let content_width = sidebar_width.saturating_sub(5); // border + padding + scrollbar + margin
let visual_rows = line.display_width().div_ceil(content_width);
```

This is **character-level** arithmetic: it divides the total display width by the
available columns.

iocraft's `TextWrap::Wrap` uses **word-level wrapping** (Unicode line-breaking
rules). A word straddling the column boundary pushes the entire word to the next
line, producing more visual rows than `div_ceil` predicts.

Example: a 48-char line with `content_width = 50` estimates to 1 row. But if
iocraft breaks at a word boundary at column 42, it becomes 2 rows.

## Current workaround

In `RenderedSidebar::build_tabbed` (`src/components/sidebar.rs`), the clamping
treats the viewport as 2 rows shorter than reality:

```rust
let clamp_margin: usize = 2;
let effective_visible = visible_lines.saturating_sub(clamp_margin);
```

This absorbs the estimation error for most content. It is imperfect: for very
long lines or narrow sidebars, the margin may not be enough.

## iocraft's wrapping internals

### `SegmentedString::wrap()` (`iocraft/src/segmented_string.rs`)

The wrapping algorithm (~130 lines):

1. Computes **break opportunities** via Unicode UAX#14 line-breaking rules
   (vendored `unicode_linebreak` module).
2. Accumulates segments on the current line. When adding the next segment (minus
   trailing whitespace width) would exceed `width`, starts a new line.
3. For single segments wider than `width`, falls back to **character-by-character
   forced breaking**.
4. Handles mandatory breaks (`\n`).

### Vendored `unicode_linebreak`

iocraft vendors a modified copy of
[axelf4/unicode-linebreak](https://github.com/axelf4/unicode-linebreak) inside
`src/unicode_linebreak/` (~900 lines, mostly auto-generated Unicode 15.0 lookup
tables + 85 lines of logic). The modification adds a `linebreaks_iter()` variant
that works with arbitrary index types — needed for `SegmentedString`'s
multi-segment iteration.

This is tracked by upstream PR:
**<https://github.com/axelf4/unicode-linebreak/pull/11>** (open since Oct 2024).
Once merged, iocraft plans to drop the vendored copy and use the upstream crate.

### Visibility

- `SegmentedString::wrap()` is `pub(crate)` — not exported.
- `Text::measure_func()` is `pub(crate)` — not accessible.
- No post-layout API exposes the visual row count.

## Options for exact row counting

### Option A: add `unicode-linebreak` as a direct dependency

Use the upstream crate to compute break opportunities, then write a simplified
row-counting function (~40 lines) that walks breaks and tracks line width —
without the full `SegmentedString` segment-tracking machinery.

**Pros:** clean, small code, reuses a maintained crate.
**Cons:** the upstream crate does **not** have the `linebreaks_iter()` variant
yet (PR #11 pending). We'd use the plain `linebreaks(&str)` function which works
on `&str` directly — sufficient for our use case since `StyledLine` spans can be
concatenated.  However, minor behavioral differences with iocraft's vendored copy
are possible until the PR merges.

### Option B: vendor the same `unicode_linebreak` module

Copy iocraft's `src/unicode_linebreak/` (3 files, ~900 lines) into our crate.
Write the same ~40-line row-counting function on top.

**Pros:** exact match with iocraft's behavior.
**Cons:** ~900 lines of vendored tables to maintain. Tight coupling to iocraft's
internal copy — if they update their vendored version, we'd need to sync.

### Option C: write a simplified word-wrap estimator

Without the full Unicode line-breaking tables, approximate word boundaries by
breaking on ASCII whitespace and hyphens. This would be closer to iocraft's
behavior than `div_ceil` but still not exact.

**Pros:** no new dependencies, small code.
**Cons:** still an approximation — Unicode text (CJK, etc.) won't match.

### Recommendation

**Option A** is the best path once `unicode-linebreak` PR #11 merges (subscribe
to get notified). Until then, the `clamp_margin = 2` workaround is adequate.

When the PR merges, the implementation would be:

```rust
// In Cargo.toml:
// unicode-linebreak = "0.1"  (or whatever version ships the PR)

fn visual_row_count(text: &str, width: usize) -> usize {
    if text.is_empty() || width == 0 {
        return 1;
    }
    let mut lines = 1usize;
    let mut line_width = 0usize;
    let mut last_break = 0;
    for (idx, opportunity) in unicode_linebreak::linebreaks(text) {
        let segment = &text[last_break..idx];
        let seg_width = unicode_width::UnicodeWidthStr::width(segment);
        let trimmed_width = unicode_width::UnicodeWidthStr::width(segment.trim_end());
        if line_width + trimmed_width > width {
            // Word doesn't fit — start new line.
            // TODO: handle forced character breaking for segments > width.
            lines += 1;
            line_width = seg_width;
        } else {
            line_width += seg_width;
        }
        if opportunity == unicode_linebreak::BreakOpportunity::Mandatory {
            lines += 1;
            line_width = 0;
        }
        last_break = idx;
    }
    lines
}
```

This sketch omits the forced character-breaking fallback (needed when a single
word exceeds `width`), but illustrates the approach.

## Upstream PR to watch

**<https://github.com/axelf4/unicode-linebreak/pull/11>**

> Created a `linebreaks_iter()` variant of `linebreaks`

Open since 2024-10-09, last updated 2025-04-24. Subscribe to get notified when
it merges — that's the signal to revisit this topic.

## Files involved

| File | Role |
|------|------|
| `src/components/sidebar.rs` | `build_tabbed`: clamping logic, `visual_row_count` closure |
| `src/views/prs.rs` | Reads `sidebar.clamped_scroll`, stores back to `preview_scroll` |
| `src/views/issues.rs` | Same pattern with `preview_scroll` |
| `src/views/actions.rs` | Same pattern with `detail_scroll` |
| `src/components/markdown_view.rs` | `RenderedMarkdown::build` — slices lines for display |
| `src/components/scrollbar.rs` | `ScrollInfo` — scrollbar thumb geometry |
