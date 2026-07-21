# Project Memory

Last updated: 2026-07-21

This file is the concise resume point for ongoing work. Product details belong
in `PLAN.md`; the complete task list belongs in `DEVELOPMENT_PLAN.md`.

## Current Status

- Active milestone: 2 - Backup Planner (complete).
- Active task: None; milestone 2 is complete.
- Next task: M01 - Enforce destination boundaries (Milestone 3, Mirror Executor).
- Code state: The backup planner is fully implemented and tested. It produces
  deterministic change-sets (additions, modifications, deletions, exclusions,
  warnings) from source and destination inventories without modifying the
  filesystem. Ignore matching uses Git-style semantics. Secret detection warns
  on sensitive file patterns. Missing source roots are protected.
- Blockers: None.

## Durable Decisions

- The application is a Rust binary with a Ratatui interface and a short-lived
  headless backup command; it is not a persistent daemon.
- A `systemd --user` timer runs the command after user-manager startup and at a
  configurable interval that defaults to five minutes.
- V1 validates and uses an existing dedicated Git clone; cloning and repository
  creation are deferred.
- The application owns only repository `home/` and
  `.config-sync-manifest.toml`. A valid manifest establishes ownership.
- Existing `home/` content without a valid manifest is refused rather than
  adopted silently.
- Source and destination traversal never follows symlinks. A source-root
  symlink is copied as a link, while symlinked source parents are rejected.
- Dirty unmanaged repository paths block backup. Dirty managed paths are
  recoverable after interrupted or failed runs.
- Source and manifest failures prevent all staging, committing, pulling, and
  pushing for that run.
- Git staging uses literal pathspecs and is verified to contain only managed
  paths before commit.
- Background Git operations are noninteractive, timeout-bounded, and preserve
  local commits when synchronization fails.
- Ignore rules use per-source Git semantics and are enforced before files enter
  the repository worktree.
- The backend is implemented and tested before the TUI; the TUI depends on
  backend services, never the reverse.
- Configuration stored as TOML; state stored as JSON (machine-readable for TUI).
- Manifest stored as TOML with format identifier `config-sync-manifest`.
- PathInputs.use_environment flag isolates tests from real XDG environment.
- State history is bounded to 50 entries, newest first.
- The `ignore` crate provides gitignore-compatible matching; parent-exclusion
  (a child cannot be re-included while parent is excluded) is enforced manually.
- Content comparison uses byte-by-byte equality with 8KB buffers; size mismatch
  short-circuits the comparison.
- Single-file sources map directly to their destination path (destination_root
  IS the file, not a directory to join into).

## Open Decisions

- Keep `config-sync` as the temporary development name. Choose the permanent
  name before milestone 6, Systemd Automation.
- Choose the project license before milestone 9, Delivery.
- No explicit MSRV is selected; use the current stable Rust toolchain until one
  is chosen.

These decisions do not block milestone 3.

## Next Steps

1. Start M01, Enforce destination boundaries, and record it as active.
2. Every write and deletion must remain beneath the repository and reject
   symlinked destination parents.

## Verification

- `cargo fmt --check` — clean
- `cargo clippy --all-targets --all-features -- -D warnings` — clean
- `cargo test --all-targets --all-features` — 246 unit tests + 1 integration = 247 passed
- All milestone 2 tasks verified: planner produces deterministic change-sets
  covering additions, modifications, deletions, exclusions, secret warnings,
  missing-root protection, and ignore semantics.

## Update Protocol

After each completed task, update this file with:

- The current milestone and active task.
- The most recently verified result.
- The exact next task or resume point.
- Commands used for verification.
- Any unresolved blocker or durable implementation decision.

Remove stale details instead of growing this into a chronological log. Never
record credentials, tokens, private remote URLs, or machine-specific secrets.
