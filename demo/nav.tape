# Navigation walkthrough GIF for gh-board README
#
# Demonstrates the core workflow loop: scrolling, sidebar preview with tabs,
# filter switching, view switching, and help overlay.
#
# Usage:
#   vhs demo/nav.tape
#
# Prerequisites:
#   - A working gh-board config with PR and Issue filters returning results
#   - Adjust timings and key presses to match your data

Output demo/nav.gif
Output demo/nav.mp4

Set Framerate 20
Set Shell "zsh"
Set FontFamily "SFMono Nerd Font"
Set Width 1400
Set Height 800
Set FontSize 14
Set Padding 20
Set Theme "Builtin Solarized Light"

# Solarized Light: light background (0;15) so gh-board detects Background::Light
Env COLORFGBG "0;15"
Env TERM "screen-256color"

# Note: vhs compresses idle frames — Sleep durations don't map 1:1 to GIF time.
# To slow down playback by 3x, post-process with:
#   ffmpeg -i demo/nav.gif -filter:v "setpts=3.0*PTS" -r 10 demo/nav-slow.gif

Type "gh board"
Sleep 1s
Enter
Sleep 300ms

# -------------------------------------------------------------------
# PRs
# -------------------------------------------------------------------

# Switch to "all repo" mode
Type "S"
Sleep 3s

# Open sidebar preview
Type "p"
Sleep 2s

# Scroll down through the PR list
Type "j"
Sleep 1000ms
Type "k"
Sleep 400ms

# Cycle sidebar tabs: Overview -> Checks -> Files
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

# Close sidebar
Type "p"
Sleep 800ms

# Open sidebar preview
Type "p"
Sleep 2s

# Switch to next filter tab
Type "l"
Sleep 1s
Type "l"
Sleep 9s

# -------------------------------------------------------------------
# Issues
# -------------------------------------------------------------------

# Switch to Issues view
Type "n"
Sleep 2s

# Open help overlay
Type "?"
Sleep 2s

# Dismiss help
Type "?"
Sleep 1s

# Open sidebar preview
Type "p"
Sleep 2s

# Scroll down through the Issues list
Type "j"
Sleep 1000ms

# Cycle sidebar tabs: Overview -> Activity
Type "]"
Sleep 1500ms
Type "]"
Sleep 1500ms

# Switch to last filter tab
Type "h"
Sleep 9s

# Scroll down through the Issues list
Type "j"
Sleep 2s
Type "j"
Sleep 2s


# -------------------------------------------------------------------
# Actions
# -------------------------------------------------------------------

# Switch to Issues view
Type "n"
Sleep 2s

# Open sidebar preview
Type "p"
Sleep 6s

# Open help overlay
Type "?"
Sleep 2s

# Dismiss help
Type "?"
Sleep 1s

# Scroll down through the Issues list
Type "j"
Sleep 1000ms

# Cycle sidebar tabs: Overview -> Activity
Type "]"
Sleep 1500ms
Type "]"
Sleep 1500ms

# Switch to last filter tab
Type "h"
Sleep 9s

# Scroll down through the Issues list
Type "j"
Sleep 2s
Type "j"
Sleep 2s


