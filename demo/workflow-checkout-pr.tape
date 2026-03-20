# Checkout a PR branch — workflow demo
#
# Demonstrates: selecting a PR, opening the preview, and pressing `c` to
# checkout the PR's head branch locally. The checkout is aborted at the
# confirmation prompt.
#
# Usage:
#   vhs demo/workflow-checkout-pr.tape
#
# Prerequisites:
#   - A working gh-board config with at least one pr_filter returning results
#   - A [repo_paths] entry mapping the repository to a local clone

Output demo/workflow-checkout-pr.mp4

Set Framerate 20
Set Shell "zsh"
Set FontFamily "SFMono Nerd Font"
Set Width 1400
Set Height 800
Set FontSize 14
Set Padding 20
Set Theme "Builtin Solarized Light"

Env COLORFGBG "0;15"
Env TERM "screen-256color"

# -------------------------------------------------------------------
# Show initial state
# -------------------------------------------------------------------

# Info comment
Type "# Checkout a PR branch — the clone dir does not exist yet"
Sleep 1s
Enter
Sleep 500ms

# Show ~/code does not exist
Type "ls ~/code 2>/dev/null || echo 'No such file or directory'"
Sleep 500ms
Enter
Sleep 1500ms

# Show repo_paths in config
Type "tail -15 ~/.config/gh-board/config.toml"
Sleep 500ms
Enter
Sleep 3s

# -------------------------------------------------------------------
# Launch and navigate
# -------------------------------------------------------------------

Type "gh board"
Sleep 500ms
Enter
Sleep 3s

# Navigate to a PR
Type "j"
Sleep 500ms

# Open sidebar preview
Type "p"
Sleep 2s

# -------------------------------------------------------------------
# Checkout branch — abort at confirmation
# -------------------------------------------------------------------

Type "c"
Sleep 2s
Escape
Sleep 1s

# -------------------------------------------------------------------
# Close preview and quit
# -------------------------------------------------------------------

Type "p"
Sleep 800ms

Type "q"
Sleep 500ms
