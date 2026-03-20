# Open a GitHub URL — workflow demo
#
# Demonstrates: the `gh board open` subcommand with four URL patterns:
# a pull request, an issue, an issue comment, and a workflow run. Each
# opens gh-board in the corresponding view with the item pre-selected.
#
# Usage:
#   vhs demo/workflow-open-url.tape
#
# Prerequisites:
#   - A working gh-board config
#   - Adjust the URLs below to match real items in your repositories

Output demo/workflow-open-url.mp4

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
# Open a pull request URL
# -------------------------------------------------------------------

Type "gh board open https://github.com/graelo/gh-board/pull/15"
Enter
Sleep 4s
Type "q"
Sleep 1s

# -------------------------------------------------------------------
# Open an issue URL
# -------------------------------------------------------------------

Type "gh board open https://github.com/graelo/gh-board/issues/10"
Enter
Sleep 4s
Type "q"
Sleep 1s

# -------------------------------------------------------------------
# Open an issue comment URL
# -------------------------------------------------------------------

Type "gh board open https://github.com/graelo/tmux-copyrat/issues/29#issuecomment-3977470347"
Enter
Sleep 4s
Type "q"
Sleep 1s

# -------------------------------------------------------------------
# Open a workflow run URL
# -------------------------------------------------------------------

Type "gh board open https://github.com/graelo/gh-board/actions/runs/12345"
Enter
Sleep 4s
Type "q"
Sleep 500ms
