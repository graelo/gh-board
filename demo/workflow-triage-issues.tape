# Triage issues — workflow demo
#
# Demonstrates: switching to the Issues view, navigating, sidebar preview
# with tab cycling, and all issue actions (assign, label, comment, close,
# reopen). Each action is aborted at the prompt.
#
# Usage:
#   vhs demo/workflow-triage-issues.tape
#
# Prerequisites:
#   - A working gh-board config with at least one issues_filter returning results

Output demo/workflow-triage-issues.mp4

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
# Launch and switch to Issues view
# -------------------------------------------------------------------

Type "gh board"
Enter
Sleep 3s

# Switch to Issues view (PRs → Issues)
Type "n"
Sleep 2s

# -------------------------------------------------------------------
# Navigation
# -------------------------------------------------------------------

Type "j"
Sleep 500ms
Type "j"
Sleep 500ms
Type "k"
Sleep 500ms

# Switch filter tabs
Type "l"
Sleep 1500ms
Type "h"
Sleep 1500ms

# -------------------------------------------------------------------
# Sidebar preview
# -------------------------------------------------------------------

Type "p"
Sleep 2s

# Cycle sidebar tabs
Type "]"
Sleep 1500ms
Type "]"
Sleep 1500ms

# Close preview
Type "p"
Sleep 800ms

# -------------------------------------------------------------------
# Issue actions (each aborted at the prompt)
# -------------------------------------------------------------------

# Assign → Escape
Type "a"
Sleep 1500ms
Escape
Sleep 800ms

# Label → Escape
Type "L"
Sleep 1500ms
Escape
Sleep 800ms

# Comment → type text → Escape
Type "c"
Sleep 1500ms
Type "Needs investigation"
Sleep 1000ms
Escape
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

# -------------------------------------------------------------------
# Clipboard & browser shortcuts
# -------------------------------------------------------------------

Type "o"
Sleep 1s
Type "Y"
Sleep 800ms

# -------------------------------------------------------------------
# Quit
# -------------------------------------------------------------------

Type "q"
Sleep 500ms
