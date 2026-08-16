# TUI Guide

Open the keyboard-first, pointer-capable interface with `dothoard`.

## Navigation

The selected tab and keyboard focus are separate. When the tab bar has focus,
use Left/Right or `h`/`l` to change tabs; use Down/`j`, Enter, or Tab to enter
content. Tab or Shift+Tab returns to the tab bar from any content state.

- Arrow keys and `h`/`j`/`k`/`l` navigate.
- `Esc` goes back one interaction level; it does not silently apply pending
  source changes.
- `q` and Ctrl+C quit only outside text entry and confirmations.
- `r` refreshes data where a screen supports it.

Mouse and touchpad users can click tabs, list rows, browser entries and
checkboxes, themes, and the visible shortcut labels in the footer. Use the
pointer wheel over a list to navigate it, over the browser's Files pane to move
its selection, or over a regular file's Preview pane to scroll its content.
Pointer actions follow the same focus and confirmation rules as their keyboard
equivalents. Availability of touchscreen gestures depends on whether the
terminal reports them as mouse events.

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
| Filesystem browser | Enter: open directory; Space: select/toggle; `:` or `/`: text entry; `.git` is hidden and `⎇` (`G` in ASCII mode) marks Git repositories |
| Configured repository | Browse within the selected clone; its parent is inaccessible. `c`: choose a replacement starting at `~/` |
| Refreshable screens | `r`: refresh; Preview and Automation load automatically on first entry when configured |
| History | `r`: refresh now; Enter: view the selected run's logs; Esc: return from logs |
| Pointer | Click tabs, rows, entries, checkboxes, themes, or visible footer actions; wheel over the scrollable pane to navigate |
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

dothoard defaults to **System (Terminal)**, which inherits the terminal's
configured default foreground/background and ANSI colors. This is the portable
way for a TUI to follow desktop personalization: when shells such as Noctalia
update the terminal palette, dothoard follows it automatically. Direct Qt/GTK
theme discovery is intentionally unnecessary and would not reliably describe
the colors of the terminal that is displaying the application.

Ten fixed-color themes remain available when a consistent application-specific
palette is preferred:

- Catppuccin Mocha
- Catppuccin Latte
- Dracula
- Nord
- Gruvbox Dark
- Tokyo Night
- Solarized Dark
- Rose Pine
- Everforest
- Kanagawa

The fixed themes paint an explicit set of RGB colors for the whole interface,
so they look the same everywhere. Select **System (Terminal)** again to resume
following terminal colors. A previously saved fixed theme remains selected
until it is changed in the picker.

## First-run workflow

An unconfigured installation uses a four-step setup shell; the seven main tabs
remain hidden until setup finishes.

1. **Repository:** choose an existing dedicated clone with the filesystem
   browser, or choose Clone and enter a Git URL plus a new destination path.
   Clone and repository validation run outside the render thread. Failures stay
   visible and retryable; no configuration is reported complete after failure.
2. **Namespace:** explicitly create or choose a discovered namespace; none is
   preselected. Selecting an owned namespace restores its manifest selections
   and ignore rules.
3. **Automation:** choose systemd, cron, or external and edit the interval. This
   persists the desired backend but does not install or modify a scheduler.
4. **Theme:** Up/Down or `j`/`k` traverses the complete theme list and applies
   the highlighted palette immediately. Enter persists it and opens Dashboard.
   Interrupted setup resumes before the main tabs open.

After setup:

1. **Sources:** selecting an owned namespace restores its manifest selections
   and ignore rules. Review them, or select source files, directories, or
   source-root symlinks below `$HOME` for a new namespace. Space toggles
   selection in the browser. When a regular file is highlighted, the picker
   Preview pane shows its metadata and cached content (up to 256 KiB).
   `Ctrl+↑`/`Ctrl+↓` or `Ctrl+k`/`Ctrl+j` scrolls the content without moving the
   selected file; `Ctrl+R` reloads the directory and content preview.
2. **Ignore:** edit source-relative ignore rules and inspect matches.
3. **Preview:** review planned additions, changes, deletions, exclusions, and
   warnings.
4. **Automation:** press `b` to select systemd, cron, or external automation.
   Install, remove, refresh, or inspect managed backends; for external
   automation, run `dothoard service print-command` and configure the scheduler
   yourself.
5. **History:** inspect namespace-aware run results and logs. The list checks
   persistent state automatically about once per second, so runs started by
   automation appear while the TUI remains open; press `r` to refresh immediately.

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

Changing namespaces reloads source paths and ignore rules from an owned
namespace's validated manifest; a new namespace starts empty. It also
invalidates source, ignore, and backup previews. Review the new namespace's
Sources, Ignore, and Preview before running a backup.
