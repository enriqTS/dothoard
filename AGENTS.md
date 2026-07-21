# AGENTS.md

This file orients contributors and coding agents working in this repository.
Read it together with `PLAN.md`, `DEVELOPMENT_PLAN.md`, and `MEMORY.md` before
making changes.

## Document Roles

- `PLAN.md` is the source of truth for V1 behavior, scope, and safety rules.
- `DEVELOPMENT_PLAN.md` is the ordered implementation backlog and completion
  checklist.
- `MEMORY.md` records the current task, durable decisions, blockers, and the
  next resume point.
- `README.md` is user-facing documentation and will grow as usable features are
  delivered.

If documents conflict, preserve the safety requirement, stop implementation,
and resolve the conflict explicitly. Do not silently reinterpret `PLAN.md`.

## Current State

The Bootstrap milestone is complete and the Rust crate exists. Begin with the
active or next task recorded in `MEMORY.md`; do not skip ahead to the TUI.

## Architecture

The application is a Rust binary with two operating modes:

- A short-lived headless command performs validation, backup, Git
  synchronization, status persistence, notifications, and systemd management.
- A Ratatui interface configures and monitors the same backend capabilities.

The backup, Git, state, notification, and systemd layers must not depend on the
TUI. CLI and TUI code call shared backend services. Business rules must remain
testable without a terminal, a real home directory, or a real user systemd
manager.

The expected source layout is:

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

Adjust this layout only when a concrete implementation need justifies it. Keep
the smallest correct module structure.

## Safety Invariants

These rules are non-negotiable unless `PLAN.md` is deliberately revised:

- Never follow source or destination symlinks during traversal.
- A source-root symlink is copied as a link; its target is not read.
- Reject symlinks in source parent components beneath `$HOME`.
- Every destination write and deletion must remain beneath the repository.
- The application owns only `home/` and `.config-sync-manifest.toml`.
- Never modify, stage, discard, or commit unmanaged repository paths.
- Refuse `home/` content that lacks a valid ownership manifest.
- Dirty unmanaged paths block backup; dirty managed paths are recoverable.
- A failed source or manifest operation prevents all Git publication for that
  run.
- A missing source root never deletes its complete existing backup.
- Ignored files are excluded before copying and therefore never enter the Git
  worktree.
- Nested `.git` directories and unsupported special files are always excluded.
- External commands use direct argument arrays, not a login shell.
- Background Git operations must be noninteractive and bounded by timeouts.
- Never log credentials, secret values, or complete credential-bearing URLs.
- Tests must not mutate the real home directory, repository, user units, or
  desktop notification state.

## Rust Conventions

- Prefer the current stable Rust toolchain until an explicit MSRV is chosen.
- Keep dependencies minimal and confirm each dependency is needed by the
  current milestone.
- Use typed domain errors inside reusable layers and add context at command
  boundaries.
- Keep filesystem and process side effects behind narrow interfaces that can
  be exercised with temporary directories and controlled commands.
- Prefer deterministic data ordering for previews, state, manifests, and
  tests.
- Use atomic replacement for configuration, manifest, state, and regular-file
  destination updates.
- Avoid broad compatibility abstractions before a shipped compatibility need
  exists.
- Comments should explain safety constraints or non-obvious behavior, not
  restate code.

## Command Rules

Invoke `git`, `systemctl`, `notify-send`, and other programs directly with
argument arrays. Do not interpolate dynamic values into shell source.

Git-specific requirements include:

- Set noninteractive environment variables for unattended operations.
- Use literal pathspecs and `--` when staging managed paths.
- Parse machine-readable output, preferably NUL-delimited where paths are
  involved.
- Verify the complete staged path list before every commit.
- Redact remote URLs before logging or persisting them.
- Preserve local commits when pull, rebase, or push fails.

Systemd-specific requirements include:

- Generate deterministic unit content.
- Write units atomically and make installation idempotent.
- Never install or enable real user units from automated tests.
- Prefer snapshot validation and `systemd-analyze verify` when available.

## Testing Rules

Add tests with the implementation task rather than postponing them to a later
cleanup phase.

Use these test layers:

- Unit tests for parsing, validation, mapping, ignore semantics, state
  transitions, and deterministic generation.
- Filesystem integration tests with temporary home and repository roots.
- Git integration tests with temporary worktrees and local bare remotes.
- TUI rendering and interaction tests with an in-memory backend or terminal.
- Manual smoke tests only for real notification, shell, distribution, and user
  systemd integration.

Once the Rust crate exists, the normal verification baseline is:

```text
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

Run narrower tests while iterating, then run the complete baseline before
marking a development task complete.

## Task Workflow

1. Read `MEMORY.md` and confirm the active or next task.
2. Read that task and its milestone gate in `DEVELOPMENT_PLAN.md`.
3. Recheck the relevant behavior in `PLAN.md`.
4. Implement the smallest complete change, including tests.
5. Run the relevant verification commands.
6. Mark the task complete in `DEVELOPMENT_PLAN.md` only after verification.
7. Update `MEMORY.md` with completed work, active task, next task, tests, and
   blockers.
8. Update `README.md` when user-visible behavior becomes usable.

Do not mark milestone gates complete merely because code exists. The gate must
be demonstrated by its specified verification.

## Commit Style

- Use concise, one-line commit messages with a conventional prefix, such as
  `feat: initialize Rust environment` or `docs: update project memory`.
- Keep each completed development task in a separate commit.
- Keep documentation-only progress updates in a separate `docs:` commit.
- Stage only files and hunks that belong to the task being committed.

## Memory Hygiene

Keep `MEMORY.md` concise and durable:

- Record decisions that affect future implementation.
- Record only the current state and recent verified progress.
- Remove stale blockers and superseded next steps.
- Point to task IDs and source documents instead of duplicating long plans.
- Never store credentials, machine-specific secrets, or sensitive paths.
