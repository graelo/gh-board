# Hero screenshot for gh-board README
#
# Captures the PR view with sidebar preview open, showing markdown rendering
# and rich PR metadata.
#
# Usage:
#   vhs demo/hero.tape
#
# Prerequisites:
#   - A working gh-board config with at least one pr_filter returning results
#   - Adjust the number of `j` presses to land on a PR with a rich body
#     (markdown, code blocks, etc.)

Output demo/hero.mp4

Set Shell "zsh"
Set FontFamily "SFMono Nerd Font"
Set Width 1400
Set Height 800
Set FontSize 14
Set Padding 20
Set Theme "Builtin Solarized Light"

# Solarized Light: light background (0;15) so gh-board detects Background::Light
Env COLORFGBG "0;7"
Env TERM "screen-256color"

# Note: vhs compresses idle frames — Sleep durations don't map 1:1 to GIF time.
# To slow down playback by 3x, post-process with:
#   ffmpeg -i demo/hero.gif -filter:v "setpts=3.0*PTS" -r 10 demo/hero-slow.gif

Type "gh board"
Enter
Sleep 3s

# Navigate to a PR with a rich body (adjust as needed)
Type "j"
Sleep 300ms

# Open sidebar preview
Type "p"
Sleep 2s

# Capture final frame as PNG screenshot
Screenshot demo/hero.png
