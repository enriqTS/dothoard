# Project Memory

Last updated: 2026-07-28

This file is the concise resume point for ongoing work. Product details belong
in `PLAN.md`; the complete task list belongs in `DEVELOPMENT_PLAN.md`.

## Current Status

- All V1 milestones complete (0 through 9).
- Post-V1 milestones 10 and 11 complete.
- **Milestone 12 complete**: Multi-Select Source Browser.
  MS01–MS08 all done. 827 unit tests passing.
- **Milestone 13 active — MN03**: MN02 is complete. Ownership classification
  and initialization inspect only the selected namespace; next, make mapping
  and mirror boundaries namespace-aware.

## Durable Decisions

- License: GPL-3.0-or-later.
- MSRV: 1.85 (Rust 2024 edition).
- The application is a Rust binary with a Ratatui interface and a short-lived
  headless backup command; it is not a persistent daemon.
- A `systemd --user` timer runs the command after user-manager startup and at a
  configurable interval that defaults to five minutes.
- V1 validates and uses an existing dedicated Git clone; cloning and repository
  creation are deferred.
- Ownership classification and initialization are scoped only to the selected
  `<namespace>/home/` and `<namespace>/.dothoard-manifest.toml`. Root-level V1
  paths and sibling namespaces are unmanaged and ignored by ownership checks.
  A valid manifest establishes ownership.
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
- State history is bounded to 50 entries, newest first.
- Content comparison uses byte-by-byte equality with 8KB buffers.
- Single-file sources map directly to their destination path.
- Atomic file writes use tempfile::NamedTempFile with permissions set before
  persist.
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
- Permanent name: `dothoard`.
- Systemd units written to `~/.config/systemd/user/`.
- Service timeout = network_timeout_seconds + 60s buffer.
- Timer uses OnStartupSec=1min and OnUnitInactiveSec={interval_minutes}min.
- TUI uses ratatui + crossterm with 250ms tick rate event loop.
- TUI has 7 tabs: Dashboard, Repository, Sources, Ignore, Preview, Automation,
  History.
- Post-V1 TUI navigation separates tab-bar focus from content focus and starts
  on the Dashboard tab bar.
- Tab focus uses Left/Right or h/l to select tabs and Down/j, Enter, or Tab to
  enter content; Tab from content returns directly to the tab bar.
- Up/k leaves content only at its uppermost navigation boundary, including
  nested controls.
- Ignore screen has nested ListFocus: SourceSelector → PatternList. Up at
  pattern_idx 0 moves to SourceSelector; Up at SourceSelector returns to tab
  bar. Left/Right for source switching resets to SourceSelector.
- Tab and Shift+Tab always return to tab bar even from modal/input states
  (Repository text input, Sources add/confirm, Ignore add/preview). Screen
  state is preserved across focus transitions.
- Repository and source paths use a shared three-pane filesystem browser;
  Enter opens directories and Space selects an entry.
- Source browsing is rooted at `$HOME`, shows hidden entries, and never enters
  symlinked directories. Repository browsing may traverse the local filesystem.
- Browser uses ranger/yazi-style three-pane layout: parent context, current
  entries, and preview/metadata. Entries sorted dirs-first, case-insensitive.
- Browser validates selections: rejects non-UTF-8, special files, and
  disappeared entries; re-checks with symlink_metadata at selection time.
- Help bar is context-sensitive: shows mode-appropriate shortcuts for browser,
  text input, and confirmation states.
- Source/repository changes mark preview and ignore previews stale and clamp
  dependent screen selections (sources list, ignore source index).
- Release profile: lto=true, strip=true, codegen-units=1.
- Namespace names are explicit user-selected portable ASCII path components:
  letters, digits, `.`, `_`, and `-` only; empty names, path separators,
  `.`/`..`, absolute paths, and other characters are rejected. Configuration
  schema version 2 adds the required `namespace` field; old configuration can
  deserialize only to report validation errors and has no automatic migration.

## Verification

- `cargo fmt --check` — clean
- `cargo clippy --all-targets --all-features -- -D warnings` — clean
- `cargo test --all-targets --all-features -- --test-threads=1` — 835 unit tests passed
  - 835 unit tests (lib) including browser, picker, state-sync, selection, diagnostics, and namespace-validation tests
  - 18 acceptance tests
  - 1 bootstrap integration test
  - 12 git_workflow integration tests
  - 49 hardening tests
  - 20 mirror integration tests
  - 13 orchestration integration tests
- Orchestration/acceptance lock-contention races in parallel mode are pre-existing
  and pass with `--test-threads=1`.
- All tests passing after fixing incorrect test assertions in TUI rendering tests.
- MN02 verification: `cargo fmt --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test --all-targets --all-features -- --test-threads=1` all pass
  (838 lib tests plus integration suites).
- Release binary: `target/release/dothoard` (3.3MB, x86_64)
- Platform: CachyOS (Arch Linux), Rust 1.97.1

## Deliverables

- `LICENSE` — GPL-3.0-or-later
- `README.md` — comprehensive installation, configuration, and usage guide
- `docs/authentication.md` — SSH and HTTPS noninteractive setup
- `docs/safety.md` — safety model, limitations, conflict recovery
- `Makefile` — build, install, test targets
- `scripts/build-release.sh` — full quality + release build script
- `tests/acceptance.rs` — 18 tests covering all V1 acceptance criteria

## Deferred Work

See PLAN.md "Deferred Work" section. Key items:
- Restore support
- Repository creation and cloning
- Multiple-machine conflict management in the TUI
- AUR packaging
- Encryption before committing

## Update Protocol

After each completed task, update this file with:

- The current milestone and active task.
- The most recently verified result.
- The exact next task or resume point.
- Commands used for verification.
- Any unresolved blocker or durable implementation decision.

Remove stale details instead of growing this into a chronological log. Never
record credentials, tokens, private remote URLs, or machine-specific secrets.
