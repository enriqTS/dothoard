# Config Sync V1 Plan

`config-sync` is a temporary working name. The project can be renamed before
packaging without changing the architecture below.

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
config-sync                 Open the TUI
config-sync backup          Run one backup immediately
config-sync check           Validate configuration and repository
config-sync service install Install and enable the user timer
config-sync service remove  Disable and remove the user timer
config-sync service status  Show automation status
```

The final binary and unit names will change when the project receives its
permanent name.

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
~/.config/config-sync/config.toml
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
`-- .config-sync-manifest.toml
```

The manifest records a format identifier, schema version, source mapping, and
ignore configuration without including credentials. It acts as an ownership
marker and makes the backup self-describing.

The application owns the complete `home/` namespace and the
`.config-sync-manifest.toml` file. It never modifies or stages other repository
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
- Stage only `home/` and `.config-sync-manifest.toml`, using literal pathspecs
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
~/.config/systemd/user/config-sync-backup.service
~/.config/systemd/user/config-sync-backup.timer
```

The timer is generated from `interval_minutes` and uses the equivalent of:

```ini
[Timer]
OnStartupSec=1min
OnUnitInactiveSec={interval_minutes}min
Unit=config-sync-backup.service
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
~/.local/state/config-sync/
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
- Multiple-machine conflict management in the TUI.
- Git history rewriting for leaked secrets.
- A continuously running filesystem watcher.
- Multiple backup profiles.
- Encryption before committing.
- AUR packaging and support for distributions other than Arch-based systems.
