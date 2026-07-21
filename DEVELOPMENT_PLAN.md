# Dothoard Development Plan

This file turns the V1 product requirements in `PLAN.md` into an ordered,
trackable implementation backlog. `PLAN.md` remains the authority for product
behavior and safety decisions. `MEMORY.md` records the current task and recent
progress.

## Tracking Rules

- Work through milestones in order unless a dependency requires otherwise.
- Mark a task complete only after its implementation and relevant tests pass.
- Keep at most one task active in `MEMORY.md`.
- Update the milestone gate only after every task in that milestone is done.
- Do not weaken a safety requirement to complete a task; record blockers in
  `MEMORY.md` instead.

## 0. Bootstrap

- [x] **B01 - Initialize the Rust crate.** Create the binary crate and planned
  backend and TUI module boundaries; `cargo check` must pass.
- [x] **B02 - Centralize application identifiers.** Define the binary name,
  config directory, state directory, manifest name, and systemd unit names in
  one location so the temporary name can be replaced safely.
- [x] **B03 - Configure dependencies and quality checks.** Add only the initial
  dependencies needed by the foundation and make `cargo fmt --check`,
  `cargo clippy -- -D warnings`, and `cargo test` pass.
- [x] **B04 - Create the CLI hierarchy.** Expose all commands from `PLAN.md` in
  `--help`, with unimplemented operations returning clear errors.
- [x] **B05 - Establish diagnostics and errors.** Add structured logging,
  actionable command-boundary errors, and redaction for secrets and
  credential-bearing remote URLs.
- [x] **B06 - Build reusable test fixtures.** Tests must be able to create
  isolated home, config, state, runtime, repository, and remote directories
  without touching the real user environment.

**Milestone gate: Complete.** The crate builds cleanly, exposes the complete
command structure, and passes the full verification baseline.

## 1. Core Models

- [x] **C01 - Implement application path resolution.** Resolve `$HOME` and XDG
  paths through injectable inputs and validate them before use.
- [x] **C02 - Define the configuration schema.** Support schema version,
  repository, remote, interval, network timeout, sources, and ignore rules.
- [x] **C03 - Implement atomic configuration persistence.** Interrupted writes
  must not leave a partially written configuration.
- [x] **C04 - Implement configuration validation.** Reject invalid intervals,
  timeouts, empty source paths, absolute paths, and parent traversal.
- [x] **C05 - Implement source path validation.** Reject symlinked parent
  components while accepting a source-root symlink as an object that will not
  be followed.
- [x] **C06 - Implement overlap and recursion validation.** Reject overlapping
  sources and any containment relationship between a source and repository.
- [x] **C07 - Define the repository manifest.** Include a recognizable format
  identifier, schema version, source mapping, and ignore configuration.
- [x] **C08 - Define persistent run state.** Represent attempts, successful
  backups, commits, pushes, warnings, errors, pending commits, and bounded run
  history with atomic serialization.

**Milestone gate: Complete.** Configuration, manifest, and state round trips
and all path validation rules are covered by unit tests (94 tests).

## 2. Backup Planner

- [x] **P01 - Define the change-set model.** Represent additions,
  modifications, deletions, exclusions, symlinks, executable-mode changes,
  and warnings.
- [x] **P02 - Implement source mapping.** Map every valid home-relative source
  deterministically beneath `repository/home/`.
- [x] **P03 - Implement the no-follow source walker.** Include hidden files,
  preserve symlinks, reject unsupported special files, and never enter nested
  `.git` directories.
- [x] **P04 - Implement ignore matching.** Support ordered Git-style patterns,
  anchoring, directory rules, negation, escaping, and hard exclusions exactly
  as defined in `PLAN.md`.
- [x] **P05 - Implement source inventory.** Collect files, raw symlink targets,
  executable bits, and comparison metadata safely.
- [x] **P06 - Implement destination inventory.** Inspect existing managed
  content without following destination symlinks.
- [x] **P07 - Implement content comparison.** Detect content, file type,
  symlink target, and executable-bit changes while skipping unchanged files.
- [x] **P08 - Implement deletion planning.** Plan removal of missing children
  and newly ignored files while protecting an entire backup when its source
  root is missing.
- [x] **P09 - Implement secret warnings.** Warn for likely private keys,
  credentials, tokens, and cookies without reading excluded file contents
  unnecessarily.
- [x] **P10 - Implement deterministic dry runs.** Produce the same ordered
  preview for the same inputs without modifying the filesystem or invoking
  Git.

**Milestone gate: Complete.** A complete backup can be previewed safely with
unit tests covering all change and ignore semantics (247 tests).

## 3. Mirror Executor

- [x] **M01 - Enforce destination boundaries.** Every write and deletion must
  remain beneath the repository and reject symlinked destination parents.
- [x] **M02 - Implement atomic regular-file copying.** Preserve content and the
  Git-supported executable bit without exposing partially written files.
- [x] **M03 - Implement symlink copying.** Preserve raw link targets without
  following or reading their targets.
- [x] **M04 - Implement safe mirror deletion.** Remove files and links without
  traversing symlinks or escaping the managed namespace.
- [x] **M05 - Implement manifest generation.** Generate and atomically update
  the manifest from the validated configuration.
- [x] **M06 - Implement source preflight.** Validate every source root and
  destination before mutation starts.
- [x] **M07 - Enforce publication boundaries.** Any mirror or manifest failure
  must prevent staging, committing, pulling, and pushing for that run.
- [x] **M08 - Implement interrupted-run recovery.** A later run must normalize
  dirty managed paths left by an interrupted or failed mirror.
- [x] **M09 - Add filesystem integration tests.** Cover initial copies,
  modifications, deletions, ignores, missing roots, symlinks, special files,
  failures, and recovery.

**Milestone gate: Complete.** Temporary-directory tests prove that mirroring is
safe, deterministic, and recoverable without Git (334 tests).

## 4. Git Layer

- [x] **G01 - Implement the Git command runner.** Use direct argument arrays,
  controlled environment variables, redacted logging, process-tree cleanup,
  and command timeouts.
- [x] **G02 - Enforce noninteractive execution.** Disable terminal and askpass
  prompts, disable interactive GCM behavior where applicable, and use batch
  mode for standard SSH remotes.
- [x] **G03 - Validate repository structure.** Detect worktree, branch, remote,
  and merge, rebase, cherry-pick, or bisect states.
- [x] **G04 - Classify repository ownership.** Distinguish a new namespace, a
  valid existing manifest, an invalid manifest, and ambiguous `home/` data.
- [x] **G05 - Implement initialization and attachment.** Require the defined
  confirmations and never claim ambiguous existing repository content.
- [x] **G06 - Classify worktree changes.** Allow recovery of managed dirty
  paths while blocking staged, unstaged, or untracked unmanaged changes.
- [x] **G07 - Implement restricted staging.** Stage only `home/` and the
  manifest using literal pathspecs and `--` separation.
- [x] **G08 - Verify staged boundaries.** Refuse to commit if any staged path
  falls outside the managed namespace.
- [x] **G09 - Implement commits.** Skip empty commits, default automated
  commits to unsigned, and preserve and report repository hook failures.
- [x] **G10 - Implement remote reconciliation.** Pull with rebase and push
  noninteractively while preserving local commits on network or remote
  failure.
- [x] **G11 - Implement conflict recovery.** Abort a conflicting rebase,
  preserve the original local commit, and report that manual intervention is
  required.
- [x] **G12 - Detect tracked ignored files.** Identify ignored destination
  paths that Git already tracks and expose them as preview warnings.
- [x] **G13 - Implement authentication checks.** Report noninteractive remote
  readiness without exposing credentials.
- [x] **G14 - Add Git integration tests.** Use temporary worktrees and bare
  remotes to cover initial push, no-op runs, offline commits, retries,
  conflicts, hooks, unmanaged changes, and pathspec metacharacters.

**Milestone gate: Complete.** The backend can safely mirror, commit, and
synchronize a backup using temporary Git repositories (455 tests).

## 5. Orchestration

- [x] **O01 - Implement exclusive locking.** Manual, timer, startup, and TUI
  backups must not overlap.
- [x] **O02 - Implement the backup coordinator.** Execute the complete workflow
  in the validated order specified by `PLAN.md`.
- [x] **O03 - Handle pending local commits.** Retry synchronization on later
  runs even when no new source files changed.
- [x] **O04 - Persist run status.** Atomically record every attempt and maintain
  bounded current and historical status.
- [x] **O05 - Implement notification transitions.** Notify for failures and
  recovery, keep successful scheduled runs quiet, and tolerate unavailable
  notification tooling.
- [x] **O06 - Complete `dothoard backup`.** Provide useful exit codes,
  diagnostics, locking, persistence, and notifications.
- [x] **O07 - Complete `dothoard check`.** Report configuration, path,
  ownership, repository, authentication, and automation-drift results
  together.
- [x] **O08 - Add headless end-to-end tests.** Cover initial backup, no-op
  backup, failure recovery, concurrency, offline synchronization, and conflict
  behavior.

**Milestone gate: Complete.** The application is fully usable and testable
without the TUI (498 tests).

## Automation Prerequisite

- [x] **N01 - Finalize the permanent name.** Rename the binary, crate, manifest
  identifier, XDG paths, and planned systemd units together before any real
  automation paths are installed.

**Prerequisite gate:** Complete. The permanent name `dothoard` is reflected in
the code, tests, and planning documents.

## 6. Systemd Automation

- [x] **A01 - Generate deterministic service units.** Use the absolute binary
  path, direct arguments, journal logging, and a finite service timeout.
- [x] **A02 - Generate the timer unit.** Render the startup delay and validated
  `interval_minutes` deterministically.
- [x] **A03 - Implement idempotent installation.** Write units atomically,
  reload the user manager, and safely enable and start or restart the timer.
- [x] **A04 - Implement removal.** Disable and remove generated units without
  affecting unrelated user services.
- [x] **A05 - Implement status inspection.** Report installed, active, stale,
  failed, and missing states accurately.
- [x] **A06 - Implement interval updates.** Regenerate and restart an installed
  timer after configuration changes without stopping an active backup.
- [x] **A07 - Detect stale units.** Compare installed unit content with the
  expected generated version in `check` and service status.
- [x] **A08 - Test unit generation.** Add snapshot tests and optional
  `systemd-analyze verify` coverage without installing real user units.

**Milestone gate: Complete.** A headless installation runs after user-manager
startup and after every configured interval (506 tests).

## 7. TUI

- [ ] **U01 - Build the TUI shell.** Implement navigation, key handling,
  resizing, terminal restoration, and panic-safe cleanup.
- [ ] **U02 - Add nonblocking backend execution.** Long checks and backups must
  not freeze rendering or input.
- [ ] **U03 - Build the dashboard.** Show repository, timer, backup, commit,
  push, pending-commit, and latest-error status.
- [ ] **U04 - Build repository selection.** Browse for a clone, validate it,
  initialize an unused namespace, or review attachment to a valid manifest.
- [ ] **U05 - Build source management.** Browse `$HOME`, add and remove sources,
  detect overlap, and identify source-root symlinks.
- [ ] **U06 - Build the ignore editor.** Edit patterns and preview matches,
  secret warnings, and already-tracked ignored files.
- [ ] **U07 - Build backup preview.** Show exact additions, modifications,
  deletions, exclusions, staging paths, and warnings.
- [ ] **U08 - Add manual backup execution.** Start a reviewed backup and show
  progress and final results without blocking the UI.
- [ ] **U09 - Build automation controls.** Install, enable, disable,
  regenerate, remove, and inspect the user timer.
- [ ] **U10 - Build history and error details.** Display recent runs and
  actionable diagnostic information.
- [ ] **U11 - Add rendering and interaction tests.** Cover important screens,
  navigation, dialogs, and backend-result transitions.

**Milestone gate:** Every V1 backend capability is available through the TUI.

## 8. Hardening

- [ ] **H01 - Audit filesystem boundaries.** Pass adversarial symlink,
  traversal, deletion, malformed-path, and race-oriented tests.
- [ ] **H02 - Audit Git boundaries.** Prove that unmanaged files cannot be
  staged, modified, discarded, or committed.
- [ ] **H03 - Audit credential handling.** Ensure logs, errors, state, and
  notifications do not expose credentials or credential-bearing URLs.
- [ ] **H04 - Test process failures.** Cover timeouts, killed Git commands,
  hook failures, notification failures, and partial filesystem errors.
- [ ] **H05 - Test shell independence.** Verify equivalent behavior when
  launched from Fish, Bash, and Zsh without shell interpolation.
- [ ] **H06 - Run the complete quality suite.** Formatting, Clippy, unit tests,
  integration tests, and systemd verification must pass together.

**Milestone gate:** Security boundaries and failure recovery have explicit test
coverage and the complete quality suite passes.

## 9. Delivery

- [ ] **D01 - Select licensing and release metadata.** Complete and verify Cargo
  package metadata.
- [ ] **D02 - Document installation.** Cover Rust installation, `cargo install`,
  repository preparation, configuration, and systemd setup.
- [ ] **D03 - Document authentication.** Cover SSH agents, host-key setup,
  HTTPS credential helpers, and noninteractive checks.
- [ ] **D04 - Document safety and limitations.** Explain backup-only behavior,
  Git secret history, single-writer expectations, and manual conflict
  recovery.
- [ ] **D05 - Run V1 acceptance testing.** Verify every acceptance criterion in
  `PLAN.md` in a clean temporary environment.
- [ ] **D06 - Validate supported distributions.** Smoke-test the TUI, Git
  synchronization, notifications, and user systemd on CachyOS and Arch Linux.
- [ ] **D07 - Produce release builds.** Provide tested release binaries and
  installation instructions.

**Milestone gate:** V1 is documented, accepted, and ready for release on the
supported distributions.

## Execution Order

```text
Bootstrap
  -> Core Models
  -> Backup Planner
  -> Mirror Executor
  -> Git Layer
  -> Orchestration
  -> Permanent Name
  -> Systemd Automation
  -> TUI
  -> Hardening
  -> Delivery
```

The explicit naming prerequisite avoids introducing installed paths and unit
names that would require a migration before release.
