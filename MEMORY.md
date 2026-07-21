# Project Memory

Last updated: 2026-07-21

This file is the concise resume point for ongoing work. Product details belong
in `PLAN.md`; the complete task list belongs in `DEVELOPMENT_PLAN.md`.

## Current Status

- Active milestone: 6 - Systemd Automation (complete).
- Active task: None; milestone 6 is complete.
- Next task: U01 - Build the TUI shell (milestone 7).
- Code state: The systemd module generates deterministic service and timer
  units from the binary path and configuration, installs/removes them
  idempotently via `systemctl --user`, inspects timer state, detects stale
  units, and updates the timer after interval changes. The `dothoard service
  install|remove|status` CLI commands are fully wired. The `dothoard check`
  command reports real automation status (installed, stale, not installed).
- Blockers: None.

## Durable Decisions

- The application is a Rust binary with a Ratatui interface and a short-lived
  headless backup command; it is not a persistent daemon.
- A `systemd --user` timer runs the command after user-manager startup and at a
  configurable interval that defaults to five minutes.
- V1 validates and uses an existing dedicated Git clone; cloning and repository
  creation are deferred.
- The application owns only repository `home/` and
  `.dothoard-manifest.toml`. A valid manifest establishes ownership.
- Existing `home/` content without a valid manifest is refused rather than
  adopted silently.
- Source and destination traversal never follows symlinks. A source-root
  symlink is copied as a link, while symlinked source parents are rejected.
- Dirty unmanaged repository paths block backup. Dirty managed paths are
  recoverable after interrupted or failed runs.
- Source and manifest failures prevent all staging, committing, pulling, and
  pushing for that run.
- Git staging uses literal pathspecs (`:(literal)` prefix) and is verified to
  contain only managed paths before commit.
- Background Git operations are noninteractive, timeout-bounded, and preserve
  local commits when synchronization fails.
- Ignore rules use per-source Git semantics and are enforced before files enter
  the repository worktree.
- The backend is implemented and tested before the TUI; the TUI depends on
  backend services, never the reverse.
- Configuration stored as TOML; state stored as JSON (machine-readable for TUI).
- Manifest stored as TOML with format identifier `dothoard-manifest`.
- PathInputs.use_environment flag isolates tests from real XDG environment.
- State history is bounded to 50 entries, newest first.
- The `ignore` crate provides gitignore-compatible matching; parent-exclusion
  (a child cannot be re-included while parent is excluded) is enforced manually.
- Content comparison uses byte-by-byte equality with 8KB buffers; size mismatch
  short-circuits the comparison.
- Single-file sources map directly to their destination path (destination_root
  IS the file, not a directory to join into).
- Atomic file writes use tempfile::NamedTempFile in the same directory as the
  destination, with permissions set before persist.
- Empty parent directories are cleaned up after deletions (best-effort, toward
  the repository root).
- Recovery is inherent: the planner is stateless, re-reads source/destination
  on each run, and the executor operations are idempotent/atomic.
- Git runner uses `setpgid(0,0)` for process-group isolation and spawns reader
  threads for stdout/stderr to prevent pipe deadlocks.
- Noninteractive env: GIT_TERMINAL_PROMPT=0, GIT_ASKPASS="", SSH_ASKPASS="",
  SSH_ASKPASS_REQUIRE=never, GCM_INTERACTIVE=Never, GIT_CONFIG_NOSYSTEM=1,
  GIT_SSH_COMMAND="ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new".
- Commits are unsigned by default (--no-gpg-sign) and hooks are never bypassed.
- Conflict recovery aborts rebase and preserves the local commit intact.
- Exclusive locking uses fs2::try_lock_exclusive on
  `$XDG_RUNTIME_DIR/dothoard.lock`. RAII guard releases on drop.
- Notifications use notify-send with --urgency critical/normal. Recovery
  notifies after a previously failing run succeeds. Quiet on normal success.
- The backup coordinator auto-initializes new namespaces in headless mode
  (the user chose the repo in config).
- Commit messages use format `backup(<hostname>): <timestamp>`.
- Orchestration tests require `--test-threads=1` due to git process contention.
- Permanent name chosen: `dothoard`. Binary, crate, manifest identifier,
  XDG paths, and systemd unit names all use this name.
- Systemd units are written to `~/.config/systemd/user/` (XDG_CONFIG_HOME).
- Service timeout = network_timeout_seconds + 60s buffer.
- Timer uses OnStartupSec=1min and OnUnitInactiveSec={interval_minutes}min.
- Stale detection compares installed unit content byte-for-byte with expected
  generated output.
- `service install` is idempotent: atomic write, daemon-reload, enable+restart.
- `service remove` is best-effort stop/disable, then remove files and reload.

## Open Decisions

- Choose the project license before milestone 9, Delivery.
- No explicit MSRV is selected; use the current stable Rust toolchain until one
  is chosen.

These decisions do not block milestone 7.

## Next Steps

1. Start U01, Build the TUI shell (milestone 7).
2. Implement navigation, key handling, resizing, terminal restoration, and
   panic-safe cleanup using Ratatui and crossterm.

## Verification

- `cargo fmt --check` — clean
- `cargo clippy --all-targets --all-features -- -D warnings` — clean
- `cargo test --lib --all-features` — 479 unit tests passed
- `cargo test --test bootstrap` — 1 test passed
- `cargo test --test git_workflow` — 12 tests passed
- `cargo test --test mirror` — 20 tests passed
- `cargo test --test orchestration -- --test-threads=1` — 13 tests passed
- Total: 525 tests (479 unit + 46 integration)

## Update Protocol

After each completed task, update this file with:

- The current milestone and active task.
- The most recently verified result.
- The exact next task or resume point.
- Commands used for verification.
- Any unresolved blocker or durable implementation decision.

Remove stale details instead of growing this into a chronological log. Never
record credentials, tokens, private remote URLs, or machine-specific secrets.
