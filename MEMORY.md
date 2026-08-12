# Project Memory

Last updated: 2026-08-12

This file is the concise resume point for ongoing work. Product details belong
in `PLAN.md`; the complete task list belongs in `DEVELOPMENT_PLAN.md`.

## Current Status

- All V1 milestones complete (0 through 9).
- Post-V1 milestones 10 and 11 complete.
- **Milestone 12 complete**: Multi-Select Source Browser.
  MS01–MS08 all done. 827 unit tests passing.
- **Milestone 13 complete**: MN01–MN09 implement namespace schemas, ownership,
  namespace-scoped backup/Git behavior, lifecycle operations, TUI namespace
  selection/create/rename/delete controls, and multi-machine documentation.
- **Milestone 14 in progress**: TU01–TU14 define the post-namespace TUI
  usability and visual design work. **TU01–TU06 are complete**: Unicode text,
  reliable viewports, consistent interaction exits, nonblocking slow TUI reads,
  separate contextual help/status regions, and a shared semantic visual theme
  with explicit focus and mode language are implemented. **TU07 is complete**:
  reusable centered, de-emphasizing modal and Unicode-safe text-input overlays
  now cover repository and namespace actions, source apply/removal, ignore input,
  and automation confirmations. **TU08 is complete**: Dashboard primary
  summaries now prioritize health, last success, pending push, automation,
  latest check, and a recommended action; `d` opens complete issue details.
  **TU09 is complete**: Repository now has a visible discovered-namespace
  management view (`m`) with active/sibling and New/Owned/Invalid/Ambiguous
  states; lifecycle commands remain routed through the existing backend.
  History rows, details, and log views identify namespaces. **TU10 is complete**:
  Sources reviews exact additions, removals, and generated ignore rules before
  apply; Ignore names nested focus and preview rule context; Preview has labeled
  counts; the picker identifies callers and offers ASCII-safe icons. **TU11 is
  next**: add actionable empty, loading, stale, and failure states.
- **Milestone 15 is partially complete**: PV01–PV03 provide the GitHub landing
  page, sanitized SVG visual demonstrations, and reorganized user
  documentation. PV04–PV06 remain for the documentation website, public trust
  automation, and acceptance.

## Durable Decisions

- `PLAN.md` is curated as the current product behavior and active objective;
  completed implementation steps and historical context do not remain there.
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
- Esc backs out one content interaction level and only quits from tab-bar
  focus. `q` and Ctrl+C explicitly quit outside text input and confirmation
  ownership. Changed source-browser sessions require an apply/discard/continue
  choice; removals retain a separate confirmation.
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
- History lists, History logs, and Ignore Preview use shared viewport state
  whose page size comes from the most recently rendered area. Selection and
  offsets are clamped after resize, refresh, and data shrinkage.
- Repository validation, Backup Preview, Ignore Preview, and automation status
  inspection run as keyed background tasks. Their typed load states preserve
  safe prior data during refresh, suppress duplicates, and reject results from
  invalidated request generations. Preview and Automation load on first entry.
- The bottom shortcut footer is the single authoritative, mode-aware help
  surface. Transient success, warning, error, and running feedback uses a
  separate typed status row; severity priority prevents lower-priority
  replacement, event-loop ticks expire non-running messages, and narrow status
  text is truncated by display width.
- TUI semantic styles are centralized in `src/tui/theme.rs`. Color reinforces
  rather than owns meaning: focus uses visible `▶` labels plus underline,
  selection uses markers plus reverse video, screen titles name the active
  mode, and the picker labels Parent, Files, and Preview with explicit focus.
- Namespace names are explicit user-selected portable ASCII path components:
  letters, digits, `.`, `_`, and `-` only; empty names, path separators,
  `.`/`..`, absolute paths, and other characters are rejected. Configuration
  schema version 2 adds the required `namespace` field; old configuration can
  deserialize only to report validation errors and has no automatic migration.

## Verification

- TU10 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 926 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
  - Rendering tests cover source-diff details, labeled preview totals, Ignore
    source/rule context and nested focus, plus picker caller context and ASCII.
- TU09 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 925 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
  - Tests cover discovered active/owned, new, ambiguous, invalid, and narrow
    namespace presentations plus History namespace and log context.
- TU08 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 919 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
  - Dashboard tests cover configured/unconfigured, unhealthy check issue
    precedence, pending push, automation unavailable/loading, first-entry
    inspection, narrow stacking, long Unicode detail dialogs, and detail input
    ownership.
- TU07 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 913 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
  - Rendering tests cover centered and clamped modal geometry, background
    de-emphasis and modal precedence, Unicode cursor visibility, validation
    display, long affected paths, narrow, and short terminal areas.
- TU06 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 908 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
  - Style-sensitive tests cover standard and color-reduced palettes, explicit
    tab/content/nested focus, selected rows, mode labels, and picker panes.
  - A real-terminal smoke attempt could not allocate a PTY in this sandbox
    (`script: failed to create pseudo-terminal: Permission denied`); retain the
    dark/light real-terminal acceptance in TU13.
- Public documentation uses `README.md` as the concise landing page and
  `docs/README.md` as its index. PV02 visual assets are sanitized SVG terminal
  renderings in `assets/screenshots/`; `assets/social-preview.svg` is ready to
  upload through GitHub repository settings. Repository description, topics,
  and social-preview selection themselves require GitHub settings access.
- PV01–PV03 verification: local Markdown links resolve and SVG XML parses;
  `git diff --check` is clean. With Rust 1.97.1 now available in the image,
  `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D
  warnings`, and `cargo test --all-targets --all-features --
  --test-threads=1` all pass (908 library tests; acceptance, bootstrap, Git,
  hardening, mirror, and orchestration suites also pass). No Rust code changed.
- TU05 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 901 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
- TU04 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 894 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
- TU03 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 881 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
- TU02 verified with Rust 1.97.1:
  - `cargo fmt --check` — clean
  - `cargo clippy --all-targets --all-features -- -D warnings` — clean
  - `cargo test --all-targets --all-features -- --test-threads=1` — clean
  - 873 library tests, 18 acceptance tests, 1 bootstrap test, 15 Git workflow
    tests, 49 hardening tests, 21 mirror tests, and 14 orchestration tests pass.
- The isolated Debian test image lacks `ssh`; the offline orchestration case
  passes with a test-only external SSH stub that reports connection refusal.
- Orchestration/acceptance lock-contention races in parallel mode are pre-existing
  and pass with `--test-threads=1`.
- All tests passing after fixing incorrect test assertions in TUI rendering tests.
- MN07 verification: `cargo fmt --check`,
  `cargo clippy --all-targets --all-features -- -D warnings`, and
  `cargo test --all-targets --all-features -- --test-threads=1` pass.
  Lifecycle tests cover confirmation, collisions, ownership refusal, rename
  manifest/config updates, protected deletion, and dirty sibling worktree refusal.
- MN08/MN09 verification: formatting, Clippy, and the complete serialized test
  baseline pass (843 library tests plus acceptance, bootstrap, Git, hardening,
  mirror, and orchestration integration suites). TUI namespace input supports
  explicit confirmation and safely invalidates source/ignore/backup previews.
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
- Advanced multiple-machine conflict management beyond Git's normal rebase recovery
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
