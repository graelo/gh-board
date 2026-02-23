# Theme Reference

Every visual element in gh-board is controlled by your `config.toml`. This
document maps each configuration field to the exact UI elements it affects.

> **Color format** — all color fields accept either an ANSI-256 index (`"0"`
> through `"255"`, where 0-15 use your terminal palette) or a hex color
> (`"#RRGGBB"` or `"#RGB"`).

---

## Text Colors — `[theme.colors.text]`

> **Note:** Some text elements (issue titles, notification titles when read) use
> your terminal's default text color rather than a theme color. These are not
> listed in the tables below.

### `primary`

| View / Component | Element                          |
| ---------------- | -------------------------------- |
| PR list          | PR number (`#27384`), title text |
| Sidebar          | Title, metadata value text       |
| Help overlay     | Overlay title                    |

### `secondary`

| View / Component  | Element                                                   |
| ----------------- | --------------------------------------------------------- |
| PR list           | Repo name (`FreeCAD/FreeCAD`), comment count              |
| Issue list        | Repo name, comment count, reaction count                  |
| Notification list | Repo name                                                 |
| All list views    | Table column header icons                                 |
| Tab bar           | Inactive tab labels                                       |
| Sidebar           | Bold label prefixes (`Labels:`, `Assign:`, `Lines:`)      |
| Sidebar Activity  | Action descriptions (`commented`, `reviewed`, `assigned`) |
| Help overlay      | Key description text                                      |
| Footer            | Context/status text                                       |

### `faint`

| View / Component  | Element                                                        |
| ----------------- | -------------------------------------------------------------- |
| PR list           | `"by"` connector, updated/created dates, empty review/CI (`–`) |
| Issue list        | Assignees, dates                                               |
| Notification list | Dates, read (non-unread) indicator, reason badge               |
| Repo/Branch view  | Ahead/behind counts with icons                                 |
| Sidebar           | Branch arrow (`→`), diff separator (`/`), commit SHA           |
| Sidebar Activity  | Timestamps on timeline events                                  |
| Tab bar           | Bottom border line                                             |
| Footer            | Section labels, help hint text, top border                     |

### `warning`

| View / Component  | Element                                        |
| ----------------- | ---------------------------------------------- |
| PR list           | Pending CI icon, changes-requested review icon |
| Issue list        | _(not directly used)_                          |
| Notification list | Issue-type badge icon                          |
| Sidebar           | Force-push timeline event                      |
| Sidebar Files     | Modified file entries                          |
| Sidebar Checks    | Pending check icon                             |

### `success`

| View / Component  | Element                                                                |
| ----------------- | ---------------------------------------------------------------------- |
| PR list           | Open state icon, approved review icon, passing CI icon, `+N` additions |
| Issue list        | Open state icon                                                        |
| Notification list | Unread indicator dot, PR-type badge icon                               |
| Sidebar           | Merged timeline event, successful check icon                           |
| Sidebar Files     | Added file entries                                                     |
| Help overlay      | Key binding labels                                                     |

### `error`

| View / Component | Element                                            |
| ---------------- | -------------------------------------------------- |
| PR list          | Closed state icon, failing CI icon, `−N` deletions |
| Issue list       | Closed state icon                                  |
| Sidebar          | Closed timeline event, failed check icon           |
| Sidebar Files    | Deleted file entries                               |

### `actor`

| View / Component  | Element                                          |
| ----------------- | ------------------------------------------------ |
| PR list           | Author name (`@graelo`), merged state icon       |
| Issue list        | Author name, closed state icon                   |
| Notification list | Release-type badge icon                          |
| Sidebar           | Author name, assignee names, commit author names |
| Sidebar Activity  | Actor names in timeline events                   |

### `inverted`

| View / Component | Element                                                       |
| ---------------- | ------------------------------------------------------------- |
| All list views   | Text on selected row (when selection needs inverted contrast) |

---

## Background Colors — `[theme.colors.background]`

### `selected`

| View / Component | Element                           |
| ---------------- | --------------------------------- |
| All list views   | Cursor/selected row highlight bar |
| Help overlay     | Overlay background fill           |

---

## Border Colors — `[theme.colors.border]`

### `primary`

| View / Component | Element                     |
| ---------------- | --------------------------- |
| Tab bar          | Active tab bottom indicator |
| Sidebar          | Pane border                 |
| Help overlay     | Overlay border              |
| Sidebar Activity | Timeline box borders        |

### `secondary`

| View / Component | Element                                                      |
| ---------------- | ------------------------------------------------------------ |
| _(reserved)_     | Defined but not currently applied — available for future use |

### `faint`

| View / Component | Element                                                     |
| ---------------- | ----------------------------------------------------------- |
| All list views   | Row separator lines between items                           |
| Tab bar          | Bottom border line                                          |
| Sidebar          | Border, scroll indicator                                    |
| Footer           | Top border line                                             |
| Markdown         | Table cell separators, blockquote borders, horizontal rules |

---

## UI Behavior — `[theme.ui]`

### `filters_show_count`

Controls whether filter tabs display the item count in parentheses,
e.g. `My PRs (5)` vs just `My PRs`.

**Default:** `true`

### `table.show_separator`

Shows or hides the thin horizontal lines between list rows.

**Default:** `true`

### `table.compact`

Reduces row height for denser display.

**Default:** `false`

---

## Pill Colors — `[theme.colors.pill]`

These control the sidebar header that appears on the Overview tab: the colored
state pill, branch info, and the `by @author · age · role` line.

### Pill Background

| Field       | Default fallback | Element                         |
| ----------- | ---------------- | ------------------------------- |
| `draft_bg`  | `text.faint`     | Background of the "Draft" pill  |
| `open_bg`   | `text.success`   | Background of the "Open" pill   |
| `closed_bg` | `text.error`     | Background of the "Closed" pill |
| `merged_bg` | `text.actor`     | Background of the "Merged" pill |
| `fg`        | white (`15`)     | Text inside the pill badge      |

### Meta Line

| Field       | Default fallback | Element                                 |
| ----------- | ---------------- | --------------------------------------- |
| `branch`    | `text.primary`   | Branch text (`main ← feature`)          |
| `author`    | `text.actor`     | Author text (`by @graelo`)              |
| `age`       | `text.faint`     | Age text (`1w`)                         |
| `role`      | `text.secondary` | Role label (`owner`, `member`, etc.)    |
| `separator` | `text.faint`     | Middle-dot separators (`·`) on the line |

---

## Footer Colors — `[theme.colors.footer]`

Per-view background colors for the active filter indicator in the footer bar.
The foreground text is always white and bold for the active filter.

| Field           | Default       | Element                                   |
| --------------- | ------------- | ----------------------------------------- |
| `prs`           | `4` (blue)    | Background of the PRs indicator           |
| `issues`        | `2` (green)   | Background of the Issues indicator        |
| `actions`       | `13` (violet) | Background of the Actions indicator       |
| `notifications` | `5` (magenta) | Background of the Notifications indicator |
| `repo`          | `6` (cyan)    | Background of the Repo indicator          |

---

## Icon Colors — `[theme.colors.icon]`

These color the author-role badge shown next to usernames in the sidebar.

| Field            | Role                       | Suggested color |
| ---------------- | -------------------------- | --------------- |
| `newcontributor` | First-time contributor     | yellow          |
| `contributor`    | Has previous contributions | green           |
| `collaborator`   | Repository collaborator    | blue            |
| `member`         | Organization member        | violet/mauve    |
| `owner`          | Repository owner           | orange/peach    |
| `unknownrole`    | Unknown or undetermined    | gray            |

---

## Icon-to-Color Mapping

Icons themselves are configured under `[theme.icons]`, but their **color**
comes from the text color fields listed above. Here is the full mapping:

### PR State Icons

| Icon field  | Colored by     |
| ----------- | -------------- |
| `pr_open`   | `text.success` |
| `pr_closed` | `text.error`   |
| `pr_merged` | `text.actor`   |
| `pr_draft`  | `text.faint`   |

### Issue State Icons

| Icon field     | Colored by     |
| -------------- | -------------- |
| `issue_open`   | `text.success` |
| `issue_closed` | `text.actor`   |

### Review Decision Icons

| Icon field         | Colored by       |
| ------------------ | ---------------- |
| `review_approved`  | `text.success`   |
| `review_changes`   | `text.warning`   |
| `review_commented` | `text.secondary` |
| `review_required`  | `text.faint`     |
| `review_none`      | `text.faint`     |

### CI Status Icons

| Icon field   | Colored by     |
| ------------ | -------------- |
| `ci_success` | `text.success` |
| `ci_failure` | `text.error`   |
| `ci_pending` | `text.warning` |
| `ci_none`    | `text.faint`   |

### Notification Type Icons

| Icon field              | Colored by       |
| ----------------------- | ---------------- |
| `notif_unread`          | `text.success`   |
| `notif_type_pr`         | `text.success`   |
| `notif_type_issue`      | `text.warning`   |
| `notif_type_release`    | `text.actor`     |
| `notif_type_discussion` | `text.secondary` |

### Branch Status Icons

| Icon field      | Colored by   |
| --------------- | ------------ |
| `branch_ahead`  | `text.faint` |
| `branch_behind` | `text.faint` |
| `branch_arrow`  | `text.faint` |

### Check Status Icons (Sidebar)

| Icon field      | Colored by     |
| --------------- | -------------- |
| `check_success` | `text.success` |
| `check_failure` | `text.error`   |
| `check_pending` | `text.warning` |

### File Change Icons (Sidebar)

| Change type | Colored by     |
| ----------- | -------------- |
| Added       | `text.success` |
| Deleted     | `text.error`   |
| Modified    | `text.warning` |

### Pill Cap Glyphs (Sidebar)

| Icon field   | Preset defaults                                            | Colored by  |
| ------------ | ---------------------------------------------------------- | ----------- |
| `pill_left`  | nerdfont: U+E0B6 (left half-circle), unicode/ascii: empty  | `pill.*_bg` |
| `pill_right` | nerdfont: U+E0B4 (right half-circle), unicode/ascii: empty | `pill.*_bg` |

The left/right cap glyphs are rendered with `fg = pill background color` (no
background), creating the rounded-edge illusion via Powerline half-circle
characters. When the strings are empty (unicode / ascii presets), the pill
renders with square edges.

---

## Markdown Colors — `[theme.colors.markdown]`

These control the preview pane rendering.

### Block Elements

| Field             | Element                                         |
| ----------------- | ----------------------------------------------- |
| `text`            | Default body text                               |
| `heading`         | Fallback heading color (when `h1`–`h6` not set) |
| `h1`              | Level 1 headings                                |
| `h2`              | Level 2 headings                                |
| `h3`              | Level 3 headings                                |
| `h4`              | Level 4 headings                                |
| `h5`              | Level 5 headings                                |
| `h6`              | Level 6 headings                                |
| `horizontal_rule` | Horizontal rules (`---`)                        |
| `code_block`      | Fenced code block border/background             |

### Inline Elements

| Field           | Element                              |
| --------------- | ------------------------------------ |
| `code`          | Inline code spans                    |
| `link`          | Bare URLs and autolinks (`↗` symbol) |
| `link_text`     | Link display text (`[text](url)`)    |
| `image`         | Image URLs                           |
| `image_text`    | Image alt text                       |
| `emph`          | Italic text (`*text*`)               |
| `strong`        | Bold text (`**text**`)               |
| `strikethrough` | Strikethrough text (`~~text~~`)      |

### Blockquote Alerts

GitHub-style alert blockquotes use semantic text colors:

| Alert type       | Colored by      |
| ---------------- | --------------- |
| `> [!NOTE]`      | `markdown.link` |
| `> [!TIP]`       | `text.success`  |
| `> [!IMPORTANT]` | `markdown.link` |
| `> [!WARNING]`   | `text.warning`  |
| `> [!CAUTION]`   | `text.error`    |

---

## Syntax Highlighting — `[theme.colors.markdown.syntax]`

Colors for fenced code blocks with language annotation.

### Structure

| Field              | Token type                      |
| ------------------ | ------------------------------- |
| `text`             | Default text in code blocks     |
| `background`       | Code block background           |
| `error`            | Error tokens                    |
| `error_background` | Error highlight background      |
| `punctuation`      | Semicolons, braces, brackets    |
| `operator`         | `+`, `-`, `*`, `==`, `=>`, etc. |

### Comments

| Field             | Token type                                        |
| ----------------- | ------------------------------------------------- |
| `comment`         | Line and block comments                           |
| `comment_preproc` | Preprocessor directives (`#include`, `#[derive]`) |

### Keywords

| Field               | Token type                                    |
| ------------------- | --------------------------------------------- |
| `keyword`           | General keywords (`fn`, `let`, `const`, `if`) |
| `keyword_reserved`  | Reserved/unsafe keywords (`unsafe`, `async`)  |
| `keyword_namespace` | Namespace keywords (`use`, `import`, `mod`)   |
| `keyword_type`      | Type keywords (`i32`, `String`, `bool`)       |

### Names / Identifiers

| Field            | Token type                                       |
| ---------------- | ------------------------------------------------ |
| `name`           | Default identifier                               |
| `name_builtin`   | Built-in types and functions (`println!`, `Vec`) |
| `name_tag`       | Tags (`<div>`, `<span>`)                         |
| `name_attribute` | Attributes, struct fields                        |
| `name_class`     | Class/type definitions                           |
| `name_decorator` | Decorators, proc-macro attributes                |
| `name_function`  | Function names                                   |

### Literals

| Field           | Token type                               |
| --------------- | ---------------------------------------- |
| `number`        | Numeric literals (`42`, `3.14`, `0xFF`)  |
| `string`        | String literals (`"hello"`)              |
| `string_escape` | Escape sequences (`\n`, `\t`, `\u{...}`) |

### Diff

| Field        | Token type                                   |
| ------------ | -------------------------------------------- |
| `deleted`    | Removed lines in diffs                       |
| `inserted`   | Added lines in diffs                         |
| `subheading` | Diff hunk headers, markdown headings in code |
