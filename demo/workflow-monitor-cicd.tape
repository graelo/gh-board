# Monitor CI/CD — workflow demo
#
# Demonstrates: switching to the Actions view, navigating workflow runs,
# sidebar preview with tab cycling, workflow navigator, re-run/cancel
# actions (all aborted), and deep-linking from PRs to Actions with an
# ephemeral tab.
#
# Usage:
#   vhs demo/workflow-monitor-cicd.tape
#
# Prerequisites:
#   - A working gh-board config with at least one actions_filter or pr_filter
#   - Some recent workflow runs in your repositories

Output demo/workflow-monitor-cicd.mp4

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
# Launch and switch to Actions view
# -------------------------------------------------------------------

Type "gh board"
Enter
Sleep 3s

# Switch to Actions view (PRs → Issues → Actions)
Type "n"
Sleep 1s
Type "n"
Sleep 2s

# -------------------------------------------------------------------
# Navigation and preview
# -------------------------------------------------------------------

Type "j"
Sleep 500ms
Type "j"
Sleep 500ms

# Open sidebar preview
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
# Workflow navigator
# -------------------------------------------------------------------

Type "w"
Sleep 1500ms
Type "j"
Sleep 500ms
Type "k"
Sleep 500ms
Escape
Sleep 800ms

# -------------------------------------------------------------------
# Actions (all aborted)
# -------------------------------------------------------------------

# Re-run failed jobs → abort with n
Type "e"
Sleep 1500ms
Type "n"
Sleep 800ms

# Re-run all jobs → abort with n
Type "E"
Sleep 1500ms
Type "n"
Sleep 800ms

# Cancel run → abort with n
Ctrl+X
Sleep 1500ms
Type "n"
Sleep 800ms

# -------------------------------------------------------------------
# Deep-link from PRs view
# -------------------------------------------------------------------

# Switch back to PRs view (Actions → Notifications → Repo → PRs)
Type "N"
Sleep 1s
Type "N"
Sleep 1s
Type "N"
Sleep 2s

# Select a PR and deep-link to its Actions run
Type "j"
Sleep 500ms
Ctrl+]
Sleep 3s

# Close the ephemeral tab
Type "d"
Sleep 1s

# -------------------------------------------------------------------
# Quit
# -------------------------------------------------------------------

Type "q"
Sleep 500ms
