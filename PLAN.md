# Dothoard Product Plan

## Goal

`dothoard` is a Rust and Ratatui application that backs up selected files under
a user's home directory into a dedicated Git repository. Multiple computers
can share that repository through independent, user-named namespaces.

The TUI configures and monitors the application. Background work is performed
by a short-lived headless command started manually or by a `systemd --user`
timer; dothoard is not a persistent daemon.

The backend feature set is sufficient for the current release. The current
product objective is improving TUI usability and visual design without
weakening backend safety or adding unrelated functionality.

## Current Scope

- CachyOS and Arch Linux support.
- Validate and use an existing dedicated Git clone.
- Back up regular files, directories, and source-root symlinks beneath `$HOME`.
- Preserve independent backups in explicit machine namespaces.
- Apply per-source ignore rules using Git semantics.
- Preview changes before manual backups.
- Commit and push automatically without interactive prompts.
- Run backups manually, after user-manager startup, and at a configurable
  interval that defaults to five minutes.
- Persist status and history for the TUI.
- Report failures and recovery through desktop notifications when available.
- Create, select, rename, and delete namespaces through the TUI under strict
  ownership rules.
- Require explicit repository and namespace selection during first-run setup.
- Restore a selected owned namespace's source paths and ignore rules from its
  validated manifest into local configuration.
- Backup only; restoring file contents remains deferred.

## Commands

```text
dothoard                 Open the TUI
dothoard backup          Run one backup immediately
dothoard check           Validate configuration and repository
dothoard service install Install and enable the user timer
dothoard service remove  Disable and remove the user timer
dothoard service status  Show automation status
```

The binary, configuration directory, state directory, manifest identifier, and
systemd units use the permanent `dothoard` name.

## Configuration

Configuration is stored at:

```text
~/.config/dothoard/config.toml
```

The current schema is version 2 and requires an explicit namespace:

```toml
version = 2
repository = "~/pessoal/example-repo"
remote = "origin"
namespace = "desktop"
interval_minutes = 5
network_timeout_seconds = 120

[[sources]]
path = ".config/fish"
ignore = [
  "*.log",
  "fish_variables",
]
```

Source paths are relative to `$HOME`. Absolute paths, parent traversal,
overlapping sources, repository recursion, and symlinked parent components are
rejected. The selected source itself may be a symbolic link and is backed up as
a link without following its target.

Namespace names are explicit identifiers rather than inferred hostnames. They
must be nonempty portable ASCII path components containing only letters,
digits, `.`, `_`, or `-`. Absolute paths, separators, `.` and `..` are invalid.
There is no automatic migration from version 1 configuration.

Configuration, manifests, and state are written atomically.

## Repository and Namespace Model

Each configured machine owns only its selected namespace:

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

The namespace manifest uses format identifier `dothoard-manifest`, schema
version 2, and records the namespace, source mapping, and ignore configuration.
Its declared namespace must match its containing directory. The manifest is both the namespace ownership marker and the portable source
selection record. Selecting an owned namespace copies its source paths and
ignore rules into local configuration after validation; selecting a new
namespace starts with no sources. Local edits remain authoritative until the
next successful backup updates that namespace's manifest.

Ownership inspection is confined to the selected namespace:

- An absent namespace may be initialized after confirmation; headless backup
  may initialize a new namespace automatically.
- A valid matching manifest establishes ownership of only that namespace.
- A malformed, unsupported, or mismatched manifest is refused.
- Existing namespace `home/` content without a valid manifest is ambiguous and
  is never silently adopted.
- Root-level legacy `home/` and `.dothoard-manifest.toml` paths are unmanaged.
- Sibling namespaces are unmanaged by the active machine and remain untouched.

Namespace lifecycle operations preserve the same boundaries. Rename and delete
show the affected path, require confirmation, reject collisions or ambiguous
content, and cannot affect a sibling namespace. The active namespace cannot be
deleted until another namespace is selected or created.

There is no in-application migration of the legacy root-level layout. Manual
migration must be completed and committed outside dothoard while the Git
worktree is otherwise clean.

## Safety Invariants

These rules are non-negotiable unless this plan is deliberately revised:

- Never follow source or destination symlinks during traversal.
- A source-root symlink is copied as a link; its target is not read.
- Reject symlinks in source parent components beneath `$HOME`.
- Keep every destination write and deletion beneath the repository and active
  namespace.
- Never modify, stage, discard, normalize, or commit unmanaged paths.
- Refuse managed-namespace content that lacks a valid ownership manifest.
- Dirty unmanaged paths block backup; dirty active-namespace paths are
  recoverable.
- A source, manifest, or ownership failure prevents all Git publication for
  that run.
- A missing source root never deletes its complete existing backup.
- Exclude ignored files before copying so they never enter the Git worktree.
- Always exclude nested `.git` directories and unsupported special files.
- Invoke external commands directly with argument arrays, never through a
  login shell.
- Keep background Git operations noninteractive and bounded by timeouts.
- Never log credentials, secret values, or complete credential-bearing URLs.
- Tests must not mutate the real home directory, repository, user units, or
  desktop notification state.

## Ignore Rules and Secret Safety

Per-source rules use `.gitignore` semantics rooted at the configured source.
Rules are evaluated in order and the last matching rule wins. Leading slashes,
trailing slashes, negation, and escaping follow Git behavior. A child cannot be
re-included while its parent remains excluded.

Only rules in dothoard configuration are evaluated. `.gitignore` files inside
a source are backed up as ordinary files and are not loaded automatically.
Hidden files are included by default. Symlinks are matched by path but never
traversed. Nested `.git` directories and unsupported special files are hard
exclusions that cannot be negated.

The TUI previews matches, warns about likely private keys, credentials, tokens,
cookies, and secrets, and identifies newly ignored paths that Git already
tracks. It explains that ignoring a tracked secret does not remove Git history
and that exposed credentials should be rotated.

## Backup and Publication Behavior

Each source is mirrored beneath `<namespace>/home/` while preserving its path
relative to `$HOME`.

- Copy regular files only when content or executable mode changed.
- Replace regular destination files atomically where supported.
- Preserve symbolic links without following or reading their targets.
- Reject destination paths with symlinked parents.
- Remove missing or newly ignored children safely without traversing symlinks.
- Retain a complete existing backup when its configured source root is missing.
- Skip unsupported special files with a warning.
- Do not create commits for empty directories.

Every source and destination is preflighted before mirroring. A mirror may
leave recoverable active-namespace changes after failure, but publication is
all-or-nothing: any source or manifest failure prevents staging, committing,
pulling, and pushing. A later run repairs dirty managed content.

The backup workflow is:

1. Acquire the exclusive application lock.
2. Load and validate configuration and the selected namespace.
3. Validate repository structure, branch, remote, ownership, and operation
   state.
4. Reject source overlap, repository recursion, and merge-like Git states.
5. Block dirty or staged paths outside the active namespace.
6. Preflight every source and destination.
7. Mirror sources into the active namespace.
8. Atomically update the active namespace manifest.
9. Stage only the active namespace with literal Git pathspecs.
10. Verify every staged path is inside that namespace.
11. Commit only when the staged tree changed.
12. Pull with rebase and push noninteractively.
13. Persist status and send failure or recovery notifications.

Failed network reconciliation preserves local commits. Later runs retry pending
pushes even when no new files changed. A rebase conflict is aborted and
reported for manual resolution.

## Git and Process Behavior

- Execute `git`, `systemctl`, and `notify-send` directly with argument arrays.
- Use machine-readable Git output, including NUL-delimited paths where needed.
- Stage with literal pathspecs and `--` separation.
- Verify the complete staged path list before every commit.
- Disable terminal prompts, askpass, interactive credential managers, and
  signed automated commits.
- Use SSH batch mode for standard SSH remotes.
- Bound network commands with configurable timeouts and terminate complete
  transport process groups on timeout.
- Keep repository hooks enabled and report hook failures.
- Redact credentials and credential-bearing URLs in logs, errors, state, and
  notifications.

An exclusive lock beneath `$XDG_RUNTIME_DIR` prevents overlapping startup,
timer, manual, and TUI backups.

## Automation, State, and Notifications

The service installer manages:

```text
~/.config/systemd/user/dothoard-backup.service
~/.config/systemd/user/dothoard-backup.timer
```

The timer starts shortly after the user manager starts and runs again after
each configured interval. Unit content is deterministic, written atomically,
and regenerated idempotently. The service timeout exceeds the configured Git
network timeout. Tests never install or enable real user units.

Machine-readable state is stored beneath:

```text
~/.local/state/dothoard/
```

It records attempts, successful backups, commits, pushes, pending commits,
timer state, warnings, errors, namespace identity, and a newest-first history
bounded to 50 entries. Background failures are persisted and sent through
`notify-send` when available. Successful scheduled runs remain quiet; recovery
after a previous failure produces a notification.

## TUI Screens

The keyboard-first TUI has seven screens:

- **Dashboard:** Backup health, repository, active namespace, automation,
  synchronization state, and actionable errors.
- **Repository:** Existing-clone selection, repository validation, ownership
  review, and namespace lifecycle operations.
- **Sources:** Persistent multi-select browser rooted at `$HOME`, including
  inherited directory selection and anchored ignore generation for deselected
  children.
- **Ignore Rules:** Per-source pattern editing and match/secret preview.
- **Backup Preview:** Additions, modifications, deletions, exclusions,
  warnings, exact managed paths, and manual backup or push actions.
- **Automation:** Installation, removal, refresh, and status for the user timer.
- **History:** Recent namespace-aware runs, details, errors, and filtered logs.

Repository and source selection use a shared no-follow filesystem browser.
Its Preview pane shows directory contents, symlink metadata, and the actual
content of selected regular files through `cat`; regular-file output is cached,
refreshable, display-safe, and limited to 256 KiB. Repository browsing may
traverse the local filesystem; source browsing cannot move above `$HOME`.
`.git` metadata directories are hidden from pickers, while
their containing directories use a Git-repository icon. After
a repository is configured, Repository browsing is rooted at the selected
repository: its contents remain visible, but its parent cannot be viewed or
entered. The explicit `c` change action restarts selection at `$HOME`.
Non-UTF-8 entries may be displayed lossily for navigation but
cannot be stored in configuration. On a fresh installation, the TUI opens
repository setup directly; after repository validation it lists existing
namespaces and requires the user to select one or explicitly create one. No
`desktop` or hostname-derived namespace is chosen implicitly.

## Current Objective: TUI Usability and Visual Design

The interface must communicate location, focus, mode, status, and available
actions consistently. Existing backend behavior and safety boundaries remain
unchanged.

### Interaction Safety and Consistency

- Text editing and displayed-path truncation must respect UTF-8 character
  boundaries and terminal display width. Unicode input and filenames must not
  panic, corrupt input, or produce invalid cursor positions.
- Every scrollable list or preview must keep its active selection visible and
  expose a real viewport. This includes History and Ignore Preview.
- `Esc` means back or cancel at the current interaction level. It must not quit
  unexpectedly or silently apply pending changes. `q` and `Ctrl+C` are the
  explicit quit actions outside text-entry modes.
- Applying source-selection changes must be explicit. The source browser must
  clearly distinguish apply, discard, and continue editing; removals continue
  to require confirmation.
- Potentially slow validation and previews use background tasks and visible
  working states rather than freezing redraw and input.
- Empty states disable invalid actions and identify a valid next step.

### Focus, Navigation, and Modes

- Distinguish tab-bar focus, content focus, nested-control focus, and selected
  items through more than color alone.
- Show a concise mode where useful, such as `Browsing`, `Editing`,
  `Previewing`, `Confirming`, or `Running`.
- Preserve Arrow and Vim aliases with one consistent navigation model.
- Keep `Tab` as a reliable route to the tab bar.
- Use consistent keys for equivalent refresh, confirm, cancel, scroll, and back
  actions across screens.

### Help, Status, and Progress

- Keep one authoritative, mode-aware shortcut bar at the bottom. Remove
  duplicated in-screen shortcut lines.
- Show transient success, warning, error, and progress messages separately so
  they never replace keyboard help.
- Make transient messages expire or dismissible.
- Identify active background work, such as checking, generating a preview, or
  backing up.
- Communicate status with words or symbols in addition to color and retain
  access to complete errors and logs.

### Dashboard Hierarchy

The Dashboard must answer immediately:

1. Are backups healthy?
2. When did the last successful backup occur?
3. Are commits waiting to be pushed?
4. Is automation installed and active?
5. What action should the user take next?

Backup health, remote synchronization, and automation are primary summaries.
Repository, namespace, source count, and schedule are secondary. The latest
check result and first actionable issue must be visible. Long errors and paths
wrap or truncate safely with access to their complete value.

### Dialogs and Inputs

Destructive or ownership-sensitive actions use visually distinct modal dialogs:

- De-emphasize the background while the dialog owns input.
- State the action, affected object or repository path, and consequence.
- Present explicit confirm and cancel choices.
- Preserve safety confirmations for namespace deletion and rename, source
  removal, and automation changes.
- Render text inputs with a consistent visible cursor, label, validation state,
  and cancellation behavior.

### Screen Improvements

- **Repository:** Visibly list the active and available namespaces with
  ownership state and discoverable create, select, rename, and delete actions.
- **Sources:** Show selection state and pending additions, removals, and ignore
  changes; make apply and discard unambiguous.
- **Ignore Rules:** Clearly distinguish focus between source selector and
  pattern list; provide a scrollable match preview with active rule context.
- **Backup Preview:** Use labeled Added, Changed, Deleted, Ignored, and Warning
  counts instead of unexplained symbols; retain scrolling and exact path
  details.
- **Automation:** Load status on first entry where possible and distinguish
  unavailable, loading, installed, active, stale, and failed states.
- **History:** Keep selection visible, include namespace identity, and preserve
  detailed error and log access.
- **Empty states:** Explain why content is absent and identify the next action.

### Visual System and Responsive Layout

- Define reusable styles for focused controls, selections, headings, labels,
  muted text, success, warnings, errors, and dialogs.
- Avoid low-contrast muted text for essential information and avoid color-only
  semantics. Reinforce focus and selection with borders, markers, bold text,
  or reverse video.
- Label browser panes as Parent, Files, and Preview and identify the active
  pane. Use symbols with predictable terminal cell width or provide an
  ASCII-safe option.
- Stack Dashboard and History panes on narrow terminals, compact or scroll the
  tab header, and preserve primary actions before secondary details.
- Truncate breadcrumbs, paths, names, and messages by terminal cell width, not
  bytes. Keep complete values available in a detail view.

### Loading and Refresh States

Screens distinguish `not loaded`, `loading`, `loaded`, `stale`, and `failed`.
Preview and Automation load on first entry when configuration permits; `r`
remains available for explicit refresh. Configuration and namespace changes
mark dependent data stale and the UI must not present stale data as current.

### Delivery Order

1. UTF-8 safety, History viewport tracking, and Ignore Preview scrolling.
2. Consistent back, cancel, apply, and quit behavior.
3. Persistent contextual help with separate transient status and progress.
4. Shared focus styles, mode indicators, modal dialogs, and text inputs.
5. Dashboard hierarchy and actionable empty/loading states.
6. Discoverable namespace controls and namespace-aware History.
7. Responsive layouts, browser labels, contrast, icons, and visual polish.
8. README updates, automated verification, and real-terminal acceptance.

### Verification

Interaction and rendering tests must cover:

- UTF-8 editing, cursor movement, and display-width-safe truncation.
- Escape, cancel, apply, and quit behavior in every mode.
- History and Ignore Preview viewport tracking.
- Status messages coexisting with contextual help.
- Visible nested focus, modal ownership, and input cursors.
- Empty, loading, stale, success, warning, and failure states.
- Long paths and representative wide, medium, narrow, and short terminals.
- Namespace visibility in Repository and History.
- Style-sensitive focus and selection, not only rendered text.

Each implementation slice must pass:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features -- --test-threads=1
```

Visual changes also require a manual smoke test in a real terminal using dark
and light-compatible palettes. The README will include a concise keyboard guide
and first-run TUI workflow.

## 15. Project Visibility and Documentation

Improve the public presentation of dothoard without changing backup behavior,
ownership boundaries, or safety guarantees. The project is not yet ready for a
stable release, so public status must clearly distinguish experimental software
from production-ready functionality.

### Public Positioning

Present dothoard as:

> A safe, Git-native dotfile backup tool for Linux users who want unattended
> backups without giving up control.

Explain clearly that dothoard is a focused backup and synchronization tool—not
a restore manager, cloud backup service, persistent daemon, or general-purpose
repository manager.

### Repository Presentation

Improve the GitHub landing page with:

- A concise repository description and relevant topics such as `dotfiles`,
  `backup`, `git`, `rust`, `linux`, `ratatui`, and `systemd`.
- A prominent tagline, project status label (`experimental`, `alpha`, or
  equivalent), and a social preview image.
- A short first-run workflow that explains installation, repository setup,
  source selection, preview, and timer installation.
- Screenshots of the Dashboard, source browser, and backup preview, plus a
  short terminal recording where practical.
- A concise safety-guarantees section explaining no-follow traversal, managed
  path boundaries, namespace isolation, noninteractive operation, failed-sync
  recovery, and secret warnings.
- A comparison with adjacent tools that clarifies dothoard's intended niche
  without making unsupported superiority claims.

### Documentation Website

Publish a small static documentation website, preferably with Astro Starlight
unless implementation experience shows that MkDocs Material or another static
Markdown-based tool is a better fit. Host it on GitHub Pages and keep source
content in the repository.

The website should provide a welcoming landing page and documentation for:

- Installation and a five-minute quick start.
- First-run TUI workflow and keyboard navigation.
- Configuration and ignore-rule reference.
- Multi-machine namespaces and lifecycle operations.
- Authentication and noninteractive Git setup.
- Safety model, limitations, backup-only behavior, and conflict recovery.
- Troubleshooting, FAQ, development, and contribution guidance.

### Trust and Discoverability

Before actively promoting the project, add continuous integration for formatting,
Clippy, and tests; clearly documented supported distributions; pre-release
notes or GitHub Releases; issue templates; and a `SECURITY.md` policy. Consider
AUR packaging after the release workflow is stable. Do not describe unfinished
features as available, and do not present experimental builds as stable.

### Delivery Order

1. Improve the GitHub description, topics, social preview, and README opening.
2. Add polished screenshots and a short demo recording.
3. Add CI and clearly marked pre-release metadata.
4. Restructure documentation into beginner, usage, and reference sections.
5. Publish the static GitHub Pages documentation website.
6. Add release binaries or distribution packaging.
7. Promote the project through relevant Rust, Linux, Arch, and dotfile
   communities.

### Milestone Verification

Verify that a new visitor can understand the purpose, safety model, supported
platforms, experimental status, and first-run path within the first page of the
README or website. Check all screenshots and commands against the current
implementation, build the site successfully, validate links, and confirm that
CI covers the documented quality baseline.

## Deferred Work

- Restore support.
- Repository creation and cloning.
- Paths outside `$HOME` and privileged files.
- Advanced conflict management beyond Git's normal rebase recovery.
- Git history rewriting for leaked secrets.
- Once-per-calendar-day startup tracking.
- Per-login startup integration beyond user-manager startup.
- A continuously running filesystem watcher.
- Multiple backup profiles.
- Encryption before committing.
- AUR packaging and support for distributions other than Arch-based systems.
