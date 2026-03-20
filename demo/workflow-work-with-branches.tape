# Work with branches — workflow demo
#
# Demonstrates: switching to the Branches view, navigating branches,
# checkout, creating a new branch (aborted), deleting a branch (aborted),
# creating a PR from a branch, and viewing PRs for a branch.
#
# Usage:
#   vhs demo/workflow-work-with-branches.tape
#
# Prerequisites:
#   - Launch gh-board from inside a git repository, or have a [repo_paths] entry
#   - At least a few local branches

Output demo/workflow-work-with-branches.mp4

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
# Launch and switch to Branches view
# -------------------------------------------------------------------

Type "gh board"
Enter
Sleep 3s

# Switch to Branches view (PRs → Issues → Actions → Notifications → Repo)
Type "n"
Sleep 800ms
Type "n"
Sleep 800ms
Type "n"
Sleep 800ms
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

# -------------------------------------------------------------------
# Checkout branch
# -------------------------------------------------------------------

Enter
Sleep 2s

# -------------------------------------------------------------------
# Create new branch — type a name then abort
# -------------------------------------------------------------------

Type "+"
Sleep 1s
Type "feat/demo-branch"
Sleep 1000ms
Escape
Sleep 800ms

# -------------------------------------------------------------------
# Delete branch — abort with n
# -------------------------------------------------------------------

Type "D"
Sleep 1500ms
Type "n"
Sleep 800ms

# -------------------------------------------------------------------
# Create PR from branch
# -------------------------------------------------------------------

Type "p"
Sleep 2s

# -------------------------------------------------------------------
# View PRs for branch
# -------------------------------------------------------------------

Type "v"
Sleep 2s

# -------------------------------------------------------------------
# Quit
# -------------------------------------------------------------------

Type "q"
Sleep 500ms
