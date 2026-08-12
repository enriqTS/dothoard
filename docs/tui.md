# TUI Guide

Open the keyboard-first interface with `dothoard`.

## Navigation

The selected tab and keyboard focus are separate. When the tab bar has focus,
use Left/Right or `h`/`l` to change tabs; use Down/`j`, Enter, or Tab to enter
content. Tab or Shift+Tab returns to the tab bar from any content state.

- Arrow keys and `h`/`j`/`k`/`l` navigate.
- `Esc` goes back one interaction level; it does not silently apply pending
  source changes.
- `q` and Ctrl+C quit only outside text entry and confirmations.
- `r` refreshes data where a screen supports it.

The bottom shortcut footer is the authoritative list of keys for the current
mode. A separate status row reports progress, warnings, and errors.

## First-run workflow

1. **Repository:** select an existing dedicated clone and create or choose a
   namespace.
2. **Sources:** select source files, directories, or source-root symlinks below
   `$HOME`. Space toggles selection in the browser.
3. **Ignore:** edit source-relative ignore rules and inspect matches.
4. **Preview:** review planned additions, changes, deletions, exclusions, and
   warnings.
5. **Automation:** install or inspect the systemd user timer after a successful
   manual backup.
6. **History:** inspect namespace-aware run results and logs.

## Sources apply/discard flow

The source browser keeps a pending editing session. Press Esc when finished:

- `a` applies the displayed changes.
- `d` discards the entire editing session.
- `c` or Esc continues editing.

If applying removes sources, dothoard asks for a separate confirmation. A child
deselected beneath a selected directory becomes an anchored ignore rule instead
of a separate source removal.

For the complete key ownership matrix, see [TUI key transitions](tui-key-transitions.md).
