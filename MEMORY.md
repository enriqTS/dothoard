# Project Memory

Last updated: 2026-07-21

This file is the concise resume point for ongoing work. Product details belong
in `PLAN.md`; the complete task list belongs in `DEVELOPMENT_PLAN.md`.

## Current Status

- Active milestone: 8 - Hardening (complete).
- Active task: None; milestone 8 is complete.
- Next task: D01 - Select licensing and release metadata (milestone 9, Delivery).
- Code state: All security boundaries have explicit adversarial test coverage.
  Pre-existing clippy warnings in TUI code fixed (collapsible_if, len_zero,
  matches! macro, only_used_in_recursion). The complete quality suite passes.
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
  is enforced manually.
- Content comparison uses byte-by-byte equality with 8KB buffers.
- Single-file sources map directly to their destination path.
- Atomic file writes use tempfile::NamedTempFile with permissions set before
  persist.
- Empty parent directories are cleaned up after deletions (best-effort).
- Recovery is inherent: the planner is stateless and the executor is idempotent.
- Git runner uses `setpgid(0,0)` for process-group isolation and spawns reader
  threads to prevent pipe deadlocks.
- Noninteractive env: GIT_TERMINAL_PROMPT=0, GIT_ASKPASS="", SSH_ASKPASS="",
  SSH_ASKPASS_REQUIRE=never, GCM_INTERACTIVE=Never, GIT_CONFIG_NOSYSTEM=1,
  GIT_SSH_COMMAND="ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new".
- Commits are unsigned by default and hooks are never bypassed.
- Conflict recovery aborts rebase and preserves the local commit intact.
- Exclusive locking uses fs2::try_lock_exclusive on
  `$XDG_RUNTIME_DIR/dothoard.lock`.
- Notifications use notify-send with --urgency critical/normal.
- The backup coordinator auto-initializes new namespaces in headless mode.
- Commit messages use format `backup(<hostname>): <timestamp>`.
- Orchestration tests require `--test-threads=1`.
- Permanent name: `dothoard`.
- Systemd units written to `~/.config/systemd/user/`.
- Service timeout = network_timeout_seconds + 60s buffer.
- Timer uses OnStartupSec=1min and OnUnitInactiveSec={interval_minutes}min.
- Stale detection compares installed unit content byte-for-byte.
- TUI uses ratatui + crossterm with 250ms tick rate event loop.
- TUI has 7 tabs: Dashboard, Repository, Sources, Ignore, Preview, Automation,
  History.
- Background tasks (backup, check) run on std::thread with mpsc channel
  communication back to the main event loop.
- Screen-specific key handling prevents global keys (q, Esc, number keys) from
  triggering while typing in text inputs.
- Preview screen runs the planner synchronously (read-only, fast).
- Backup execution available from Dashboard ('b') and Preview ('b') screens.

## Open Decisions

- Choose the project license before milestone 9, Delivery.
- No explicit MSRV is selected; use the current stable Rust toolchain until one
  is chosen.

These decisions do not block milestone 9.

## Next Steps

1. Start D01, Select licensing and release metadata (milestone 9, Delivery).
2. Complete and verify Cargo package metadata.

## Verification

- `cargo fmt --check` — clean
- `cargo clippy --all-targets --all-features -- -D warnings` — clean
- `cargo test --all-targets --all-features -- --test-threads=1` — 690 tests passed
  - 595 unit tests (lib)
  - 1 bootstrap integration test
  - 12 git_workflow integration tests
  - 49 hardening tests
  - 20 mirror integration tests
  - 13 orchestration integration tests

## Update Protocol

After each completed task, update this file with:

- The current milestone and active task.
- The most recently verified result.
- The exact next task or resume point.
- Commands used for verification.
- Any unresolved blocker or durable implementation decision.

Remove stale details instead of growing this into a chronological log. Never
record credentials, tokens, private remote URLs, or machine-specific secrets.
