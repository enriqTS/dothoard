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
mode. A separate status row reports progress, warnings, and errors. On narrow
or short terminals the tab header compacts while preserving the selected tab;
Dashboard and History stack their primary panes before secondary details.

## Keyboard reference

| Context | Keys |
|---|---|
| Tab bar | Left/Right or `h`/`l`: select tab; Down/`j`, Enter, or Tab: enter content |
| Any content | Tab or Shift+Tab: tab bar; `q`: quit; Ctrl+C: quit unless text or a confirmation owns input |
| Lists and previews | Up/Down or `j`/`k`: move; Home/End and PageUp/PageDown where shown |
| Filesystem browser | Enter: open directory; Space: select/toggle; `:` or `/`: text entry |
| Refreshable screens | `r`: refresh; Preview and Automation load automatically on first entry when configured |
| History | Enter: view the selected run's logs; Esc: return from logs |
| Anywhere | `Ctrl+T`: open the theme picker |

Focus is explicit: `▶` and underline identify tab/content or nested-control
focus, while reverse video marks the selected row. Screen titles name modes such
as Browsing, Editing, Previewing, Confirming, and Running.

## Themes

Press `Ctrl+T` from anywhere to open the theme picker. `↑`/`↓` or `j`/`k`
move between themes and preview each one immediately; `Enter` saves the
highlighted theme to `theme.toml` in the configuration directory; `Esc`
closes the picker and restores whatever theme was active before it opened.
The picker owns all input while it is open, so it can be opened mid-dialog
or mid-edit without disturbing whatever the rest of the interface is doing
underneath it.

dothoard ships ten built-in themes, defaulting to Catppuccin Mocha:

- Catppuccin Mocha (default)
- Catppuccin Latte
- Dracula
- Nord
- Gruvbox Dark
- Tokyo Night
- Solarized Dark
- Rose Pine
- Everforest
- Kanagawa

Each theme paints an explicit set of colors for the whole interface —
background, chrome, borders, and every semantic color — rather than
inheriting the host terminal's palette, so the chosen theme looks the same
everywhere dothoard runs.

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

## Namespace workflow

Open **Repository** and press `m` to inspect the active namespace and direct
repository siblings. Ownership is shown as New, Owned, Invalid, or Ambiguous;
only New and Owned entries can be selected. Press `n` to create/select a name,
`r` to rename the active namespace, or `d` to delete it after choosing a
replacement. Confirmation dialogs show the affected path. Invalid and ambiguous
namespaces, root-level legacy paths, and siblings remain unmanaged and are
never adopted or changed.

Changing namespaces invalidates source, ignore, and backup previews. Review the
new namespace's Sources, Ignore, and Preview before running a backup.
