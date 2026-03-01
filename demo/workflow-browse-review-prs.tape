# Browse and review PRs — full workflow demo
#
# Demonstrates: navigation, filter tabs, sidebar preview with tab cycling,
# all 9 PR actions (approve, comment, assign, label, merge, close, reopen,
# mark ready, update from base), clipboard/URL shortcuts, and help overlay.
#
# Usage:
#   vhs demo/workflow-browse-review-prs.tape
#
# Prerequisites:
#   - A working gh-board config with at least one pr_filter returning results
#   - Adjust j/k presses and timings to match your data

Output demo/workflow-browse-review-prs.mp4

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
# Launch
# -------------------------------------------------------------------

Type "gh board"
Sleep 1s
Enter
Sleep 3s

# -------------------------------------------------------------------
# Navigation
# -------------------------------------------------------------------

# Toggle global scope
Type "S"
Sleep 2s

# Switch filter tabs
Type "l"
Sleep 1500ms
Type "l"
Sleep 5s

# Navigate the list
Type "j"
Sleep 500ms
Type "j"
Sleep 500ms
Type "k"
Sleep 500ms

# Jump to first / last
Type "G"
Sleep 800ms
Type "g"
Sleep 800ms

# Switch filter tabs
Type "h"
Sleep 500ms
Type "h"
Sleep 1500ms

# -------------------------------------------------------------------
# Sidebar preview
# -------------------------------------------------------------------

# Open preview
Type "p"
Sleep 2s

# Cycle sidebar tabs
Type "]"
Sleep 1500ms
Type "]"
Sleep 1500ms
Type "]"
Sleep 1500ms
Type "]"
Sleep 1500ms
Type "]"
Sleep 1500ms

# Close preview
Type "p"
Sleep 800ms

# Reopen preview
Type "p"
Sleep 800ms

# -------------------------------------------------------------------
# PR actions (each aborted at the confirmation prompt)
# -------------------------------------------------------------------

# Approve → Escape
Type "v"
Sleep 1500ms
Escape
Sleep 800ms

# Comment → type some text → Escape
Type "C"
Sleep 1500ms
Type "Looks good overall"
Sleep 1000ms
Escape
Sleep 800ms

# Assign → Escape
Type "a"
Sleep 1500ms
Space
Sleep 1s
Escape
Sleep 800ms

# Label → Escape
Type "L"
Sleep 1500ms
Down
Sleep 800ms
Space
Sleep 800ms
Down
Sleep 500ms
Space
Sleep 800ms
Escape
Sleep 800ms

# Merge → abort with n
Type "m"
Sleep 1500ms
Type "n"
Sleep 800ms

# Close → abort with n
Type "x"
Sleep 1500ms
Type "n"
Sleep 800ms

# Reopen → abort with n
Type "X"
Sleep 1500ms
Type "n"
Sleep 800ms

# Mark ready for review → abort with n
Type "W"
Sleep 1500ms
Type "n"
Sleep 800ms

# Update from base → abort with n
Type "u"
Sleep 1500ms
Escape
Sleep 800ms

# -------------------------------------------------------------------
# Clipboard & browser shortcuts
# -------------------------------------------------------------------

# Copy PR number
Type "y"
Sleep 800ms

# Copy PR URL
Type "Y"
Sleep 800ms

# -------------------------------------------------------------------
# Help overlay
# -------------------------------------------------------------------

Type "?"
Sleep 3s
Type "?"
Sleep 800ms

# -------------------------------------------------------------------
# Quit
# -------------------------------------------------------------------

Type "q"
Sleep 500ms
