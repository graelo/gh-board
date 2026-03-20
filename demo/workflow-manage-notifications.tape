# Manage notifications — workflow demo
#
# Demonstrates: switching to the Notifications view, navigating, marking
# as read, mark-all-as-read (aborted), unsubscribe (aborted), and
# search with qualifiers.
#
# Usage:
#   vhs demo/workflow-manage-notifications.tape
#
# Prerequisites:
#   - A working gh-board config with at least one notifications_filter
#   - Some unread notifications in your GitHub inbox

Output demo/workflow-manage-notifications.mp4

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
# Launch and switch to Notifications view
# -------------------------------------------------------------------

Type "gh board"
Enter
Sleep 3s

# Switch to Notifications view (PRs → Issues → Actions → Notifications)
Type "n"
Sleep 1s
Type "n"
Sleep 1s
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
# Mark single notification as read
# -------------------------------------------------------------------

Type "m"
Sleep 1500ms

# -------------------------------------------------------------------
# Mark all as read — abort with n
# -------------------------------------------------------------------

Type "M"
Sleep 1500ms
Type "n"
Sleep 800ms

# -------------------------------------------------------------------
# Unsubscribe — abort with n
# -------------------------------------------------------------------

Type "u"
Sleep 1500ms
Type "n"
Sleep 800ms

# -------------------------------------------------------------------
# Search with qualifier
# -------------------------------------------------------------------

Type "/"
Sleep 800ms
Type "reason:review_requested"
Sleep 1000ms
Enter
Sleep 2s

# Clear search
Type "/"
Sleep 500ms
Ctrl+U
Sleep 300ms
Enter
Sleep 1s

# -------------------------------------------------------------------
# Quit
# -------------------------------------------------------------------

Type "q"
Sleep 500ms
