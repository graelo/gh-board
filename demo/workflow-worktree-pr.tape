# Create a worktree for a PR — workflow demo
#
# Demonstrates: selecting a PR, opening the preview, and pressing `w` to
# create an isolated git worktree for the PR's head branch. The worktree
# creation is aborted at the confirmation prompt.
#
# Usage:
#   vhs demo/workflow-worktree-pr.tape
#
# Prerequisites:
#   - A working gh-board config with at least one pr_filter returning results
#   - A [repo_paths] entry mapping the repository to a local clone

Output demo/workflow-worktree-pr.mp4

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
# Launch and navigate
# -------------------------------------------------------------------

Type "gh board"
Enter
Sleep 3s

# Navigate to a PR
Type "j"
Sleep 500ms

# Open sidebar preview
Type "p"
Sleep 2s

# -------------------------------------------------------------------
# Create worktree — abort at confirmation
# -------------------------------------------------------------------

Type "w"
Sleep 2s
Type "n"
Sleep 1s

# -------------------------------------------------------------------
# Close preview and quit
# -------------------------------------------------------------------

Type "p"
Sleep 800ms

Type "q"
Sleep 500ms
