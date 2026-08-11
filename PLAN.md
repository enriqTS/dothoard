# Dothoard V1 Plan

`dothoard` is the permanent project name.

## Goal

Build a Rust and Ratatui application that backs up selected files under the
user's home directory into a dedicated Git repository. It commits and pushes
changes automatically after user-manager startup and at a configurable
interval that defaults to five minutes.

The TUI configures and monitors the application. The background work is a
short-lived command started by a `systemd --user` timer, not a persistent
daemon.

## V1 Scope

- CachyOS and Arch Linux support.
- Rust implementation with Ratatui.
- Validate and use an existing dedicated Git clone.
- Back up files and directories located under `$HOME`.
- Per-source ignore rules using `.gitignore` semantics.
- Manual backups, user-manager startup backups, and configurable scheduled
  backups defaulting to every five minutes.
- Git commits and pushes without interactive prompts.
- Desktop notifications for failures and recovery.
- Persistent status visible in the TUI.
- Backup only; restoring is deferred.

## Commands

```text
dothoard                 Open the TUI
dothoard backup          Run one backup immediately
dothoard check           Validate configuration and repository
dothoard service install Install and enable the user timer
dothoard service remove  Disable and remove the user timer
dothoard service status  Show automation status
```

The binary and unit names use the `dothoard` prefix.

## Shell Independence

Internal programs will be executed directly with argument arrays. They will
not be run through the user's login shell.

```rust
Command::new("git")
    .args(["diff", "--cached", "--quiet"])
```

The same rule applies to `systemctl` and `notify-send`. This works identically
under Bash, Zsh, and Fish and avoids quoting and command-injection problems.

If a future feature genuinely requires shell syntax, it will explicitly use
`/usr/bin/bash -c`. Dynamic values will be passed as positional arguments or
environment variables rather than interpolated into shell source.

## Configuration

Configuration will be stored at:

```text
~/.config/dothoard/config.toml
```

Initial schema:

```toml
version = 1
repository = "~/pessoal/example-repo"
remote = "origin"
interval_minutes = 5
network_timeout_seconds = 120

[[sources]]
path = ".config/fish"
ignore = [
  "*.log",
  "fish_variables",
]

[[sources]]
path = ".config/waybar"
ignore = [
  "cache/",
  "*token*",
]
```

Source paths are stored relative to `$HOME`. V1 rejects absolute paths,
parent traversal, and symlinks in parent components between `$HOME` and the
selected source. The selected source itself may be a symbolic link; it is
backed up as a link without following its target, even when that target is
outside `$HOME`.

## Repository Layout

Backed-up paths preserve their location relative to `$HOME` beneath a `home`
directory:

```text
repository/
|-- home/
|   |-- .config/
|   |   |-- fish/
|   |   `-- waybar/
|   `-- .bashrc
`-- .dothoard-manifest.toml
```

The manifest records a format identifier, schema version, source mapping, and
ignore configuration without including credentials. It acts as an ownership
marker and makes the backup self-describing.

The application owns the complete `home/` namespace and the
`.dothoard-manifest.toml` file. It never modifies or stages other repository
paths. Repository setup handles three states explicitly:

- If the managed namespace is absent and there is no manifest, initialize it
  only after user confirmation.
- If a valid manifest already exists, validate its format and version, preview
  its recorded configuration, compare it with the local configuration, and
  require confirmation before attaching to it.
- If the manifest is invalid, or `home/` contains data without a valid
  manifest, refuse to initialize or adopt it. V1 does not silently claim
  ambiguous existing content.

The local configuration remains authoritative for operation. The repository
manifest is an ownership marker and portable description, not configuration
that is applied without review.

Sources may not overlap each other. A source and the repository may not
contain one another, preventing recursive backups.

## Planned Multiple-Machine Namespaces

A future multiple-machine feature will let several computers that use the
same Git repository keep fully independent backups. Each local configuration
will name one machine namespace chosen by the user (for example `desktop`,
`notebook`, or `home-server`); the name is an identifier, not an automatically
inferred hostname.

The repository layout for that feature will be:

```text
repository/
|-- desktop/
|   |-- home/
|   |   `-- .config/...
|   `-- .dothoard-manifest.toml
|-- notebook/
|   |-- home/
|   `-- .dothoard-manifest.toml
`-- home-server/
    |-- home/
    `-- .dothoard-manifest.toml
```

A machine configuration will store its selected namespace alongside the
repository, remote, schedule, and source configuration. Namespace names must
be valid, nonempty, portable single path components: they may not be absolute,
contain separators, `.`/`..`, or otherwise escape the repository. The TUI will
allow the user to choose or create the name explicitly and will show the
active namespace prominently.

All source mapping, mirroring, deletion, manifest generation, staging, and
staged-path verification for a run will be confined to that machine directory
and its manifest. For example, `.config/fish` on the `desktop` machine maps to
`desktop/home/.config/fish`; it can never write, delete, stage, or normalize
`notebook/` or `home-server/`. Sources remain non-overlapping within a machine,
and the separate namespace roots make mappings from different machines
non-overlapping by construction.

Each namespace has its own manifest so that its source and ignore-rule history
is independent. A valid manifest authorizes only the namespace containing it.
Committed namespaces belonging to other machines are repository content that
the current machine leaves untouched. As today, dirty or staged paths outside
the active namespace must block publication rather than being modified,
discarded, or accidentally committed.

This design reduces ordinary cross-machine synchronization conflicts because
machines commit different paths, but it does not provide configuration merging,
restore, or automatic conflict resolution. Git pull/rebase/push safety rules
continue to apply.

The TUI must provide namespace management both while initially configuring a
repository and afterward: users can create a namespace, select the active
namespace, rename it, or delete it. Creation initializes only the chosen empty
namespace. Renaming and deletion are ownership-sensitive operations that must
show the affected path and require explicit confirmation; they must reject
collisions, invalid or ambiguous namespace content, and any operation that
would affect a sibling namespace. Deleting the active namespace requires
selecting or creating another active namespace first.

There will be no in-application migration from the existing root-level `home/`
and root manifest layout. Users who want to reorganize early development data
will do so manually outside dothoard before configuring a namespace. The old
root-level paths are never adopted, moved, or deleted by this feature and are
treated as unmanaged repository content.

## Ignore Rules and Secret Safety

Per-source rules use `.gitignore` matching semantics and are rooted at the
configured source. Rules are evaluated in order and the last matching rule
wins. Leading slashes, trailing slashes, negation, and escaping follow Git
semantics. As with Git, a child cannot be re-included while its parent
directory remains excluded.

Only rules from the application configuration are evaluated. `.gitignore`
files found inside a source are backed up as ordinary files and are not loaded
automatically. Hidden files are included by default. Symlinks are matched by
their path but never traversed. Nested `.git` directories and unsupported
special files are hard exclusions that cannot be negated.

The backup engine enforces ignores before copying, so ignored files never
enter the Git working tree. The preview and real backup use the same matcher.

The TUI will:

- Preview files matched by an ignore rule.
- Warn about likely private keys, credentials, tokens, cookies, and secrets.
- Detect when a newly ignored file is already tracked.
- Explain that ignoring an existing secret does not remove it from Git
  history and that exposed credentials should be rotated.
- Always exclude nested `.git` directories and unsupported special files.

V1 will not generate per-directory `.gitignore` files. Copy-time exclusion is
the primary safety boundary and avoids conflicts with `.gitignore` files that
are themselves part of an application's configuration.

## Backup Semantics

Each source is mirrored into its corresponding `home/...` destination.

- Copy regular files only when their content changed.
- Replace destination files atomically where the platform permits it.
- Preserve the executable bit supported by Git.
- Preserve symbolic links, including a source-root symlink, without following
  them or reading their targets.
- Never follow destination symlinks. Before writing or deleting, verify that
  the path is lexically inside the repository and that no existing parent
  component in the managed namespace is a symlink.
- Reject or skip sockets, devices, FIFOs, and other special files with a
  warning.
- Propagate deletion of children from an existing source directory.
- Do not delete an entire backup when its configured source root is missing;
  retain the backup and report an error instead.
- Remove destination files that become ignored, stage their deletion, and
  warn if they were previously tracked.
- Do not create Git commits for empty directories because Git cannot track
  them.

Every source is preflighted before mirroring starts. Publication is
all-or-nothing: if any source or manifest update fails, the application does
not stage, commit, pull, or push any part of that run. Changes already made
inside the managed namespace may remain in the worktree and are repaired by a
later run. Atomicity of the entire filesystem mirror is not a V1 goal.

A manual removal of a source in the TUI must ask whether its existing backup
should also be deleted.

## Backup Workflow

1. Acquire an exclusive application lock.
2. Load and validate the configuration.
3. Validate the existing Git clone, branch, configured remote, and repository
   ownership state.
4. Reject source overlap and repository recursion.
5. Reject repositories in merge, rebase, cherry-pick, or bisect states.
6. Inspect staged, unstaged, and untracked worktree changes. Any dirty path
   outside the managed namespace causes a safe failure. Dirty managed paths
   are recoverable and are normalized by rerunning the mirror.
7. Preflight all configured sources and their destination paths.
8. Mirror every configured source into `repository/home/...`.
9. Update the repository manifest.
10. If every mirror and manifest operation succeeded, stage the complete
    managed namespace using literal Git pathspecs.
11. Verify that every staged path is managed and fail before committing if it
    is not.
12. Commit only when the staged tree changed.
13. Reconcile with the remote using pull with rebase.
14. Push local commits.
15. Persist the result for the TUI.
16. Send a desktop notification on failure or recovery.

Suggested commit message:

```text
backup(cachyos-host): 2026-07-21 14:30:00
```

If the network or push is unavailable, the local commit remains intact. Later
runs retry synchronization even if no additional file changes occurred.

If a rebase conflicts, the application aborts the rebase, preserves the local
commit, and reports that manual intervention is required.

## Git Behavior

- Execute the installed `git` binary directly.
- Set `GIT_TERMINAL_PROMPT=0`, disable askpass interaction, and set
  `GCM_INTERACTIVE=Never` where applicable for unattended operations.
- Use OpenSSH batch mode for standard SSH remotes so password, passphrase, and
  host-key prompts cannot block a background run.
- Apply a configurable network-command timeout with a two-minute default and
  terminate the complete Git transport subprocess tree on timeout.
- Default automated commits to unsigned so GPG pinentry cannot block the
  service.
- Continue running repository hooks and report hook failures.
- Stage only `home/` and `.dothoard-manifest.toml`, using literal pathspecs
  and `--` separation, and verify the staged path list before every commit.
- Keep local commits when remote synchronization fails.
- Avoid logging credentials or complete remote URLs containing credentials.

The repository is dedicated to the application, but unexpected unmanaged
changes still cause a safe failure instead of being silently committed or
discarded.

## Concurrency

An exclusive lock under `$XDG_RUNTIME_DIR` prevents startup, timer, manual,
and TUI-triggered backups from overlapping. A second invocation reports that
a backup is already running and exits without changing files.

## Systemd Integration

The service installer creates:

```text
~/.config/systemd/user/dothoard-backup.service
~/.config/systemd/user/dothoard-backup.timer
```

The timer is generated from `interval_minutes` and uses the equivalent of:

```ini
[Timer]
OnStartupSec=1min
OnUnitInactiveSec={interval_minutes}min
Unit=dothoard-backup.service
```

It starts shortly after the user systemd manager starts, normally at the first
login, and runs again for the configured interval after each completed backup.
It does not promise a new startup backup for every graphical or shell login if
the user manager remains active. Enabling user lingering for pre-login
execution is not part of V1.

No empty commit is created when nothing changed. Restricting user-manager
startup backups to once per calendar day is deferred.

The service invokes the absolute binary path directly and logs stdout and
stderr to the systemd journal. It also has a finite service timeout longer than
the Git network-command timeout as a final safeguard against hung subprocesses.

`service install` is idempotent: it atomically regenerates the units, runs
`systemctl --user daemon-reload`, and enables and starts or restarts the timer.
When the interval changes in the TUI and automation is installed, the TUI
regenerates and restarts the timer without stopping an active backup service.
If unit regeneration fails, the configuration remains saved, the failure is
reported, and automation is marked stale. The `check` command detects unit
content that differs from the expected generated version.

## Status and Notifications

Machine-readable state is stored under:

```text
~/.local/state/dothoard/
```

It records:

- Last attempted and successful backup times.
- Last created commit.
- Last successful push.
- Whether local commits are waiting to be pushed.
- Current timer status.
- Latest warning or error.
- A bounded history of recent runs.

Background failures are sent through `notify-send` when available and always
persisted for the TUI. Successful scheduled runs remain quiet. A recovery
notification is sent after a previously failing operation succeeds.

## TUI Screens

### Dashboard

Show the repository, remote, timer state, last backup, last commit, last push,
pending commits, and latest error.

### Repository

Choose an existing local clone and validate its worktree, branch, remote,
noninteractive authentication readiness, and managed-namespace ownership.
Initialize an unused namespace or review and attach to a valid existing
manifest; refuse ambiguous existing `home/` content.

### Sources

Browse `$HOME`, add files or directories, remove sources, detect overlap, and
identify source-root symlinks that will be preserved rather than traversed.

### Ignore Rules

Edit patterns for one source and preview matched files. Clearly flag matches
that Git already tracks.

### Backup Preview

Show additions, modifications, deletions, ignored files, warnings, and the
exact paths that would be staged. Allow a manual backup after review.

### Automation

Install, enable, disable, remove, and inspect the systemd user timer.

### History

Show recent runs, commits, push results, and actionable error details.

## Post-V1 TUI Usability

The first post-V1 improvement replaces manual filesystem path entry and makes
keyboard focus predictable across the complete TUI. These changes affect only
interactive configuration and navigation. They do not weaken backend path,
ownership, symlink, or publication safety rules.

### Focus and Tab Navigation

The TUI has two explicit top-level focus states: the tab bar and the active
tab's content. The selected tab remains visible in both states, but rendering
must distinguish which level currently receives keyboard input.

The application starts on Dashboard with focus on the tab bar. While the tab
bar has focus:

- Left and Right, or `h` and `l`, select the previous and next tabs.
- Tab, Down, `j`, or Enter moves focus into the selected tab.
- Shift+Tab selects the previous tab without entering its content.
- Number keys `1` through `7` select a tab directly.

While tab content has focus:

- Screen-specific navigation and actions receive keyboard input.
- Tab returns directly to tab-bar focus without changing the selected tab.
- Shift+Tab also returns to tab-bar focus without changing the selected tab;
  pressing it again there selects the previous tab.
- Up or `k` moves upward inside the tab while another item or parent control
  exists.
- Up or `k` returns to tab-bar focus only from the uppermost item or control.
- Left and Right, or `h` and `l`, remain local to the active screen and do not
  change tabs.

Arrow and Vim keys are exact navigation aliases. A screen with nested
navigation must expose its hierarchy rather than making the application infer
it. For example, Ignore Rules moves from its pattern list to its source
selector before it can return to the tab bar.

Tab always provides a direct route from content to the tab bar, including from
nested browsers, editors, and confirmation states. Ctrl+C remains a global
exit. Other quit, cancel, and action keys retain their screen-specific meaning
and must not leak through a modal dialog or text editor.

Changing focus or tabs preserves screen-local state, including selections,
scroll positions, browser location, validation results, and open confirmation
states. Entering a tab again resumes that state rather than silently resetting
the user's work.

The help bar must describe the currently focused level and mode. Tab focus,
content focus, filesystem browsing, text editing, and confirmation dialogs
must not display shortcuts that are unavailable in that state.

### Filesystem Browser

Repository and source selection use a shared three-pane filesystem browser
instead of free-form path entry. The browser follows a ranger/yazi-style
layout:

- The left pane provides parent-directory context.
- The center pane lists entries in the current directory and owns selection.
- The right pane previews a selected directory or shows file, symlink, and
  metadata details.
- A header or breadcrumb shows the complete current path.
- A status area reports loading, permission, validation, and selection errors.

Inside the browser:

- Up and Down, or `k` and `j`, move the selected entry.
- Left or `h` moves to the parent directory when the picker boundary permits
  it.
- Right, `l`, or Enter opens the selected real directory.
- Space selects the highlighted entry for the calling screen.
- Home, End, PageUp, and PageDown provide efficient navigation in long lists.
- Tab returns to the application tab bar without discarding browser state.

Hidden entries are visible because dotfiles are primary backup candidates.
Entries are ordered deterministically with directories grouped before other
entry types. Selection and scrolling remain visible in small terminals and in
directories containing more entries than the available viewport.

Directory data is loaded and cached when browser state changes, not during
rendering. Listings are shallow and never recursively scan a tree. Read,
metadata, permission, and race errors are shown without crashing or replacing
the last usable location.

The browser keeps paths as filesystem-native `PathBuf` values. Since the
configuration schema stores UTF-8 strings, a non-UTF-8 entry may be displayed
lossily for navigation but cannot be selected; the UI must explain why.

### Repository Selection

Repository browsing may move throughout the local filesystem. Only existing
directories that validate as Git worktrees can be configured. Selecting a
directory starts the existing repository validation and ownership review; it
does not bypass initialization or attachment confirmation.

Validation uses the configured remote name and network timeout when replacing
an existing repository. If Git accepts a selected subdirectory, the TUI stores
the worktree root returned by repository validation rather than the arbitrary
subdirectory. Configuration persistence errors are reported and must not be
presented as successful repository setup.

Repository selection continues to distinguish new, owned, invalid-manifest,
and ambiguous namespaces. Invalid or ambiguous managed content remains
unselectable for operation even when it is visible in the browser.

### Source Selection

Source browsing is rooted at `$HOME` and cannot navigate above it. The browser
operates in a persistent multi-select mode: it remains open while the user
selects and deselects multiple entries before applying all changes at once.

The browser uses `symlink_metadata` and never opens or previews a symlink as a
directory. A selected source-root symlink is configured as the link itself and
the UI explains that its target will not be followed. Symlinks in parent
components remain invalid. Sockets, devices, FIFOs, and other unsupported
special files may be identified in the listing but cannot be selected.

#### Multi-Select Behavior

Each entry in the browser displays a checkbox indicator:

- `[●]` — Explicitly selected as a source (green/cyan).
- `[◉]` — Inherited selection: the entry is inside a selected folder source
  (dim/gray).
- `[ ]` — Not selected (dim).

Space toggles the selection state of the highlighted entry:

- An unselected file or directory becomes explicitly selected.
- An explicitly selected entry becomes unselected.
- An inherited entry (child of a selected folder) becomes deselected, which
  will generate an ignore rule for that path within the parent source.
- A deselected inherited entry returns to inherited state when toggled again.

Selecting a folder implicitly selects everything inside it. Navigating into a
selected folder shows its children with the inherited `[◉]` indicator.
Deselecting specific children inside a selected folder creates per-source
ignore rules using the full relative path anchored from the source root (e.g.,
`/completions/git.fish` or `/subfolder/`). This prevents accidental matches
against files with the same name in unrelated directories.

#### Session Persistence

When entering the browser, existing configured sources appear pre-checked.
The browser remembers its navigation position within the TUI session but not
across restarts. Re-entering the browser after a previous apply resumes from
the last browsed directory.

#### Apply on Escape

Pressing Escape closes the browser and computes a diff between the current
selection state and the persisted configuration:

- New selections are added as sources.
- Deselected children within folder sources are added as anchored ignore
  rules on the corresponding source.
- Previously configured sources that were unchecked are candidates for
  removal.

If the diff includes source removals, a confirmation dialog lists the sources
to be removed and requires explicit `y` to proceed. Pressing `n` or Escape
returns to the browser with the selection intact.

If the diff contains only additions or ignore-rule changes, it is applied
immediately without confirmation.

After a successful apply, backup and ignore previews are marked stale.
Validation or persistence failure leaves the browser open with an actionable
error message.

#### Validation

Source validation continues to reject absolute paths, parent traversal,
overlapping sources, repository recursion, and symlinked parent components.
Each new source candidate is validated individually before the complete diff
is applied. A validation failure for one source does not discard the rest of
the pending changes.

### TUI Verification

Interaction tests cover the complete focus transition matrix, nested upward
navigation, modal key precedence, tab-state persistence, and Arrow/Vim key
parity. Filesystem-browser tests use temporary roots and cover deterministic
ordering, hidden entries, files, directories, symlinks, special files,
unreadable and disappearing entries, root boundaries, scrolling, and
non-UTF-8 names.

Repository and source integration tests cover selection, validation,
confirmation, worktree-root normalization, home-relative conversion,
configuration persistence failure, and dependent-preview invalidation.
Rendering tests verify focused and unfocused tab styles, content focus,
three-pane layouts, contextual help, errors, and narrow terminals.

## Project Structure

```text
src/
|-- main.rs
|-- lib.rs
|-- app.rs
|-- cli.rs
|-- config.rs
|-- diagnostics.rs
|-- paths.rs
|-- git.rs
|-- locking.rs
|-- notification.rs
|-- state.rs
|-- systemd.rs
|-- backup/
`-- tui/
```

The backup and Git layers must not depend on the TUI. This allows integration
testing and unattended execution without a terminal.

## Expected Dependencies

- `ratatui` and `crossterm` for the TUI.
- `clap` for command parsing.
- `serde` and `toml` for configuration and manifests.
- `ignore` for `.gitignore`-compatible matching.
- `fs2` for the process lock.
- `tempfile` for atomic file updates and tests.
- `thiserror` and `anyhow` for errors and command boundaries.
- `tracing` and `tracing-subscriber` for structured diagnostics.
- `directories` for XDG locations.
- `chrono` for timestamps.

Dependency choices and versions will be confirmed against the current Rust
toolchain when implementation begins.

## Implementation Phases

### 1. Foundation

- Initialize the Rust crate.
- Add the CLI command hierarchy.
- Define configuration, manifest, result, and error types.
- Resolve XDG directories and `$HOME` safely.
- Implement configuration loading, saving, migration versioning, and
  validation.

### 2. Backup Engine

- Implement safe source-to-destination mapping.
- Implement per-source ignore matching.
- Add content comparison and atomic copying.
- Add source and destination symlink safety and executable-bit handling.
- Add mirror deletion and missing-root protection.
- Add preflight and recoverable managed-worktree handling.
- Produce a dry-run change set for the TUI and tests.

### 3. Git Synchronization

- Validate repository and operation state.
- Validate repository ownership and attachment states.
- Stage only managed paths with literal pathspecs and verify the staged tree.
- Commit non-empty staged changes.
- Pull with rebase and push.
- Enforce noninteractive authentication and network-command timeouts.
- Preserve commits on offline or remote failure.
- Detect and safely abort conflicts.

### 4. Background Operation

- Add exclusive locking.
- Persist run status and bounded history.
- Add optional desktop notifications.
- Generate, install, and manage systemd user units.

### 5. TUI

- Build the dashboard and navigation shell.
- Add repository selection and validation.
- Add the home-directory source picker.
- Add ignore editing and match previews.
- Add backup preview and manual execution.
- Add automation controls and run history.

### 6. Delivery

- Document installation and Git authentication.
- Add `cargo install` instructions.
- Test operation from Fish, Bash, and Zsh sessions.
- Add release builds.
- Consider an AUR package after the binary name stabilizes.

## Verification Strategy

Unit tests cover:

- Home-relative path validation and traversal rejection.
- Source overlap and repository recursion.
- Repository initialization, attachment, and ambiguous-content refusal.
- Ignore pattern semantics.
- Source-to-repository path mapping.
- Source-root and destination symlink handling.
- Manifest and configuration serialization.
- Status transitions and notification recovery logic.

Integration tests use temporary home directories, local Git repositories, and
bare remotes. They cover:

- Initial backup, commit, and push.
- File modification and deletion.
- Ignored files never entering the repository.
- Newly ignored tracked files.
- Missing source-root protection.
- Symbolic links that point outside the source.
- Destination symlinks that attempt to escape the repository.
- Interrupted mirrors followed by successful managed-path recovery.
- A source failure preventing any commit or push from that run.
- Offline commits followed by a later successful push.
- Rebase conflicts.
- Unexpected repository changes.
- Git pathspec metacharacters and staged-path boundary verification.
- Concurrent backup attempts.
- No commit when nothing changed.
- Noninteractive authentication failure and network-command timeout.
- Direct execution independent of the active login shell.

Systemd generation is verified with snapshot tests and, when available,
`systemd-analyze verify`. Tests also cover interval regeneration and stale-unit
detection. They must not install or enable real user units.

## V1 Acceptance Criteria

- A user can select an existing Git clone in the TUI.
- Repository initialization and attachment never claim ambiguous existing
  `home/` content.
- A user can add files and directories from `$HOME`.
- A user can configure and preview ignore rules per source.
- A preview accurately reports additions, changes, deletions, and exclusions.
- A manual backup creates and pushes a commit only when files changed.
- An offline backup creates a local commit and a later run pushes it.
- The user timer runs after user-manager startup and after each configured
  interval.
- Background failures appear in a desktop notification and the TUI.
- Concurrent runs cannot corrupt the repository.
- The application behaves identically when launched from Fish, Bash, or Zsh.

## Deferred Work

- Restore support.
- Once-per-calendar-day startup tracking.
- Per-login startup integration beyond user-manager startup.
- Repository creation and cloning.
- Paths outside `$HOME` and privileged files.
- Git history rewriting for leaked secrets.
- A continuously running filesystem watcher.
- Multiple backup profiles.
- Encryption before committing.
- AUR packaging and support for distributions other than Arch-based systems.

## TUI Bug Fixes

Three bugs degrade the TUI experience and must be resolved before further
feature work.

### Bug 1: Backup Corrupts TUI Display

Running a backup from the TUI (e.g. pressing `b` on the Preview or Dashboard
screen) causes tracing output to appear below the alternate screen, breaking
the terminal display. The root cause is that `diagnostics::init()` sets up
`tracing_subscriber::fmt()` writing to stderr. When the background backup
thread emits `tracing::info!()` calls, the output bypasses the alternate
screen and corrupts the display.

**Fix:** When the TUI is active, redirect tracing to a log file at
`~/.local/state/dothoard/dothoard.log` using `tracing-appender`. The file
serves both as a persistent debug log and as the backing store for a future
scrollable log viewer in the History screen. The `WorkerGuard` must be held
for the entire TUI event loop lifetime.

### Bug 2: Repository Browser Never Loads

The Repository tab shows "Loading browser..." indefinitely because
`ensure_browser()` is only called inside `handle_repository_key()`, which
executes on content-focus key events. If the user navigates to the tab and
enters content focus, the browser is not initialized until another keypress.

**Fix:** Call `ensure_browser()` when the user transitions focus from the tab
bar into content on the Repository screen. Change the placeholder message from
"Loading browser..." to an actionable hint such as "Press Enter or ↓ to start
browsing" for the rare case where browser initialization has not yet occurred.

### Bug 3: History Shows Unhelpful Error Messages

When a sync fails, the History detail pane shows "sync failed: sync failed"
because `SyncError::Git` has `#[error("sync failed")]` which discards the
inner `GitError` details. The actual cause (authentication failure, network
timeout, remote rejection) is lost.

Additionally, there is no way to see full log output for a specific run.

**Fix (error chain):** Change `SyncError::Git`'s error attribute to include
the source: `#[error("sync failed: {0}")]`. Remove the redundant "sync
failed:" prefix in the coordinator's error formatting so the final message
reads something like "sync failed: git push origin main failed with exit code
128: fatal: ...".

**Fix (log viewer):** Add a scrollable log viewer accessible from the History
screen. When the user presses Enter on a selected run, the TUI reads the log
file, filters lines by the run's timestamp range, and displays them in a
scrollable view. Escape returns to the history list.

## Post-Namespace TUI Usability and Visual Design

The backend feature set is sufficient for the current release. The next product
focus is making the TUI easier to understand, safer to operate, and visually
coherent without weakening backend safety or adding unrelated functionality.
The existing seven screens and keyboard-first operation may remain, but the
interface must communicate location, focus, mode, status, and available actions
consistently.

### Interaction Safety and Consistency

Correct interaction defects before visual polish:

- Text editing and displayed-path truncation must respect UTF-8 character
  boundaries and terminal display width. Unicode input and filenames must not
  panic, corrupt input, or produce invalid cursor positions.
- Every scrollable list or preview must keep its active selection visible and
  expose a real viewport. This includes History and Ignore Preview.
- `Esc` means back or cancel at the current interaction level. It must not quit
  unexpectedly or silently apply pending changes. `q` and `Ctrl+C` remain the
  explicit quit actions outside text-entry modes.
- Applying source-selection changes remains an explicit action. Leaving the
  source browser must clearly distinguish applying changes, discarding them,
  and returning to editing; removals continue to require confirmation.
- Repository validation, filesystem previews, and other potentially slow work
  must not make the interface appear frozen. Long operations use the existing
  background-task model and show a visible working state.
- Empty states must disable invalid actions and offer a valid next step rather
  than entering unusable modes.

### Focus, Modes, and Navigation

The active keyboard context must be apparent without consulting documentation:

- Distinguish tab-bar focus, content focus, the focused nested control, and the
  selected item through more than color alone.
- Show a concise mode indicator where applicable, such as `Browsing`,
  `Editing`, `Previewing`, `Confirming`, or `Running`.
- Keep Arrow and Vim navigation aliases, but present one consistent navigation
  model across screens.
- `Tab` continues to provide a reliable route to the tab bar. Contextual help
  must accurately describe the current focus and mode.
- Repository, Sources, Ignore, Preview, Automation, and History must use
  consistent keys for equivalent actions such as refresh, confirm, cancel,
  scrolling, and returning.

### Help, Status, and Progress

Keyboard help and operation feedback serve different purposes and must not
replace one another:

- Keep one authoritative, mode-aware shortcut bar at the bottom of the screen.
  Remove duplicated in-screen shortcut lines that consume content space or can
  disagree with the footer.
- Display transient success, warning, error, and progress messages in a
  separate status region. Messages should expire or be dismissible without
  hiding keyboard guidance.
- Background work must show its operation and state, for example
  `Checking repository`, `Generating preview`, or `Backing up`.
- Success, warning, and failure must include words or symbols in addition to
  color. Errors remain actionable and preserve access to full details and logs.

### Dashboard Hierarchy

The Dashboard is the primary health summary, not merely a configuration dump.
It must make these answers immediately visible:

1. Whether backups are healthy.
2. When the last successful backup occurred.
3. Whether commits are waiting to be pushed.
4. Whether automation is installed and active.
5. What action, if any, the user should take next.

Use prominent status summaries for backup health, remote synchronization, and
automation. Repository, namespace, source count, and schedule are secondary
information. Display the latest check result, including the first actionable
issue, rather than storing it invisibly. Long errors and paths must wrap or
truncate safely with a route to their complete value.

### Dialogs and Text Input

Destructive or ownership-sensitive operations use visually distinct modal
dialogs rather than confirmation text appended to ordinary content:

- Dim or otherwise de-emphasize the background while a dialog owns input.
- State the action, affected repository path or object, and consequence.
- Present explicit confirm and cancel choices and preserve existing safety
  confirmations for namespace deletion, namespace rename, source removal, and
  automation changes.
- Render all text inputs with a consistent visible cursor, label, validation
  state, and cancellation behavior.

### Screen-Specific Improvements

- **Repository:** Show the active namespace, available namespaces, and
  ownership state as visible controls. Create, select, rename, and delete must
  be discoverable without memorizing hidden keys. Preserve all namespace
  ownership and sibling-isolation rules.
- **Sources:** Make selection state and pending changes clear. Applying,
  discarding, and confirming removals must be unambiguous.
- **Ignore Rules:** Visually distinguish focus between the source selector and
  pattern list. Preview all matches through a scrollable viewport and show the
  active rule context.
- **Backup Preview:** Replace unexplained symbolic totals with labeled Added,
  Changed, Deleted, Ignored, and Warning counts. Retain a scroll position and
  expose exact staged paths and warning details.
- **Automation:** Load status on first entry when possible and distinguish
  unavailable, loading, installed, active, stale, and failed states.
- **History:** Keep the selected run visible, include its namespace, and retain
  access to detailed errors and filtered logs.
- **Empty states:** Explain why the screen is empty and name the next available
  action, such as adding sources, generating a preview, installing automation,
  or running the first backup.

### Visual System and Responsive Layout

Create a small shared visual system instead of styling each screen ad hoc:

- Define reusable styles for focused controls, selections, headings, labels,
  muted text, success, warnings, errors, and dialogs.
- Avoid low-contrast `DarkGray` for essential text and avoid color-only status
  semantics. Selection and focus should also use borders, markers, bold text,
  or reverse video.
- Label filesystem-browser panes as Parent, Files, and Preview, and visibly
  identify the active pane. Provide an ASCII-safe icon option or use symbols
  whose cell width is predictable across supported terminals.
- Add compact layouts for narrow terminals: stack Dashboard and History panes,
  compact or scroll the seven-tab header, and preserve important actions before
  secondary details.
- Truncate breadcrumbs, paths, names, and messages by terminal cell width, not
  byte count. Complete values must remain available in a detail view.

### Loading and Refresh Behavior

Screens distinguish `not loaded`, `loading`, `loaded`, `stale`, and `failed`.
Preview and Automation should load on first entry when configuration permits;
`r` remains available for an explicit refresh. Configuration or namespace
changes continue to mark dependent data stale and must display that state
clearly rather than presenting old data as current.

### Documentation and Verification

Update the README with a concise keyboard guide and first-run TUI workflow.
Interaction and rendering tests must cover:

- UTF-8 editing, cursor movement, and display-width-safe truncation.
- Consistent Escape, cancel, apply, and quit behavior in every mode.
- History and Ignore Preview viewport tracking.
- Status messages coexisting with contextual help.
- Visible nested focus, modal ownership, and input cursors.
- Empty, loading, stale, success, warning, and failure states.
- Long paths and representative wide, medium, narrow, and short terminals.
- Namespace visibility in Repository and History.
- Style-sensitive focus and selection assertions, not only rendered text.

The complete formatting, Clippy, and serialized test baseline must pass after
each implementation slice. Visual changes should additionally receive a manual
smoke test in a real terminal using both a dark and a light-compatible terminal
palette.

### Delivery Order

Implement the usability work in this order:

1. UTF-8 safety, History viewport tracking, and Ignore Preview scrolling.
2. Consistent back, cancel, apply, and quit behavior.
3. Persistent contextual help with separate transient status and progress.
4. Shared focus styles, mode indicators, modal dialogs, and text inputs.
5. Dashboard hierarchy and actionable empty/loading states.
6. Discoverable namespace controls and namespace-aware History.
7. Responsive layouts, browser labels, contrast, icons, and final visual polish.
8. README updates, complete automated verification, and real-terminal visual
   acceptance.
