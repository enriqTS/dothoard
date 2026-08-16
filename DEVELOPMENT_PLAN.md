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

- [x] **U01 - Build the TUI shell.** Implement navigation, key handling,
  resizing, terminal restoration, and panic-safe cleanup.
- [x] **U02 - Add nonblocking backend execution.** Long checks and backups must
  not freeze rendering or input.
- [x] **U03 - Build the dashboard.** Show repository, timer, backup, commit,
  push, pending-commit, and latest-error status.
- [x] **U04 - Build repository selection.** Browse for a clone, validate it,
  initialize an unused namespace, or review attachment to a valid manifest.
- [x] **U05 - Build source management.** Browse `$HOME`, add and remove sources,
  detect overlap, and identify source-root symlinks.
- [x] **U06 - Build the ignore editor.** Edit patterns and preview matches,
  secret warnings, and already-tracked ignored files.
- [x] **U07 - Build backup preview.** Show exact additions, modifications,
  deletions, exclusions, staging paths, and warnings.
- [x] **U08 - Add manual backup execution.** Start a reviewed backup and show
  progress and final results without blocking the UI.
- [x] **U09 - Build automation controls.** Install, enable, disable,
  regenerate, remove, and inspect the user timer.
- [x] **U10 - Build history and error details.** Display recent runs and
  actionable diagnostic information.
- [x] **U11 - Add rendering and interaction tests.** Cover important screens,
  navigation, dialogs, and backend-result transitions.

**Milestone gate: Complete.** Every V1 backend capability is available through
the TUI (641 tests: 595 unit + 46 integration).

## 8. Hardening

- [x] **H01 - Audit filesystem boundaries.** Pass adversarial symlink,
  traversal, deletion, malformed-path, and race-oriented tests.
- [x] **H02 - Audit Git boundaries.** Prove that unmanaged files cannot be
  staged, modified, discarded, or committed.
- [x] **H03 - Audit credential handling.** Ensure logs, errors, state, and
  notifications do not expose credentials or credential-bearing URLs.
- [x] **H04 - Test process failures.** Cover timeouts, killed Git commands,
  hook failures, notification failures, and partial filesystem errors.
- [x] **H05 - Test shell independence.** Verify equivalent behavior when
  launched from Fish, Bash, and Zsh without shell interpolation.
- [x] **H06 - Run the complete quality suite.** Formatting, Clippy, unit tests,
  integration tests, and systemd verification must pass together.

**Milestone gate: Complete.** Security boundaries and failure recovery have
explicit test coverage and the complete quality suite passes (690 tests).

## 9. Delivery

- [x] **D01 - Select licensing and release metadata.** Complete and verify Cargo
  package metadata.
- [x] **D02 - Document installation.** Cover Rust installation, `cargo install`,
  repository preparation, configuration, and systemd setup.
- [x] **D03 - Document authentication.** Cover SSH agents, host-key setup,
  HTTPS credential helpers, and noninteractive checks.
- [x] **D04 - Document safety and limitations.** Explain backup-only behavior,
  Git secret history, single-writer expectations, and manual conflict
  recovery.
- [x] **D05 - Run V1 acceptance testing.** Verify every acceptance criterion in
  `PLAN.md` in a clean temporary environment.
- [x] **D06 - Validate supported distributions.** Smoke-test the TUI, Git
  synchronization, notifications, and user systemd on CachyOS and Arch Linux.
- [x] **D07 - Produce release builds.** Provide tested release binaries and
  installation instructions.

**Milestone gate: Complete.** V1 is documented, accepted, and ready for release
on the supported distributions (708 tests passing).

## 10. TUI Usability Improvements

This post-V1 milestone implements the focus and filesystem-browser behavior in
`PLAN.md` "Post-V1 TUI Usability" without changing backend backup semantics or
safety boundaries.

- [x] **UX01 - Introduce explicit top-level focus.** Separate the selected tab
  from `TabBar` and `Content` keyboard focus, start on the Dashboard tab bar,
  and implement Left/Right, `h`/`l`, Down/`j`, Enter, Tab, Shift+Tab, and direct
  number-key behavior with interaction tests.
- [x] **UX02 - Define screen navigation boundaries.** Let each screen report
  whether Up/`k` moved locally or reached its upper boundary; preserve modal
  key capture except for global Ctrl+C and the explicit Tab/Shift+Tab focus
  escape, and implement nested navigation for Ignore Rules and boundary
  behavior for lists and scroll views without resetting screen state.
- [x] **UX03 - Build the filesystem-browser model.** Add a reusable three-pane
  picker with filesystem-native paths, deterministic ordering, hidden entries,
  parent boundaries, selection and scrolling state, cached shallow listings,
  metadata errors, and tests using temporary filesystems.
- [x] **UX04 - Enforce picker filesystem safety.** Use no-follow metadata,
  prevent directory traversal through symlinks, identify unsupported special
  files, reject non-UTF-8 selections cleanly, tolerate disappearing entries,
  and cover source-root symlinks and boundary failures with tests.
- [x] **UX05 - Render and operate the three-pane picker.** Draw parent,
  directory, and preview/metadata panes with breadcrumbs, entry types,
  selection, scroll state, and contextual status; implement Arrow/Vim
  navigation, Enter-to-open, Space-to-select, paging, and narrow-terminal
  rendering tests.
- [x] **UX06 - Integrate repository browsing.** Replace repository path text
  entry with the picker, validate selected directories using the configured
  remote and timeout, persist the validated Git worktree root, retain ownership
  confirmation, and report save failures without false success.
- [x] **UX07 - Integrate source browsing.** Replace source path text entry with
  a `$HOME`-rooted picker, allow regular files, directories, and source-root
  symlinks, convert selections to validated home-relative paths, and keep the
  picker open after validation or persistence failure.
- [x] **UX08 - Make focus and help visually explicit.** Distinguish focused
  tabs from active content, show local focus in nested controls, and make the
  help bar accurate for tab, content, picker, editor, preview, and confirmation
  modes with style-sensitive rendering tests.
- [x] **UX09 - Synchronize dependent TUI state.** After repository or source
  changes, clamp affected selections, mark backup and ignore previews stale,
  preserve valid browser state, and test transitions across tabs.
- [x] **UX10 - Complete usability acceptance.** Update `README.md`, run the
  complete formatting, Clippy, and test baseline, and manually smoke-test the
  focus model and repository/source picker in a real terminal on an Arch-based
  system.

**Milestone gate: Complete.** A user can configure the repository and add files,
directories, or source-root symlinks without typing a path. Arrow and Vim keys
navigate tabs and all nested content predictably, Tab always returns from
content to tab focus, all picker safety tests pass, and the complete quality
suite remains clean.

## 11. TUI Bug Fixes

Three bugs degrade the TUI experience: backup tracing output corrupts the
display, the repository browser never auto-loads, and sync errors show
unhelpful messages with no log access. These must be resolved before further
feature work.

- [x] **F01 - Redirect tracing to a log file during TUI mode.** Add
  `tracing-appender` to dependencies. Create a `diagnostics::init_for_tui()`
  function that writes to `~/.local/state/dothoard/dothoard.log` using a
  non-blocking file appender. Call it from the TUI entry point instead of the
  default stderr subscriber. Hold the `WorkerGuard` for the entire event loop
  lifetime. Verify that running a backup from the TUI no longer corrupts the
  display and that log lines appear in the file.

- [x] **F02 - Fix repository browser initialization and placeholder.** Call
  `ensure_browser()` when focus transitions from the tab bar into content on
  the Repository screen (in `handle_key_tab_bar` after setting
  `Focus::Content`). Change the "Loading browser..." fallback message to
  "Press Enter or ↓ to start browsing". Add a test that after simulating a
  Down key from TabBar on the Repository screen, `app.repo_screen.browser` is
  `Some(...)`.  
  **Verification**: Test passes, clippy clean, fmt clean.

- [x] **F03 - Fix SyncError display to include underlying GitError.** Change
  `SyncError::Git` from `#[error("sync failed")]` to
  `#[error("sync failed: {0}")]` so the inner `GitError` details propagate.
  Remove the redundant "sync failed:" prefix in the coordinator's
  `format!("sync failed: {e}")`, replacing it with `format!("{e}")`. Add a
  unit test that `SyncError::Git(GitError::Failed { .. })` includes the git
  error details in its Display output.  
  **Verification**: Test passes, clippy clean, fmt clean.

- [x] **F04 - Add a scrollable log viewer to the History screen.** Add a
  `LogView` mode to `HistoryScreen` with scroll state and cached filtered
  lines. When the user presses Enter on a selected history entry, read the
  log file, filter lines by the run's timestamp range (`started_at` to
  `finished_at`), and display them in a scrollable paragraph. Add `j`/`k`
  and Up/Down for scrolling, Escape to return to the history list. Update
  the detail help line to show `Enter: view logs`. Add tests for
  timestamp-based filtering and log-view key handling.  
  **Verification**: Tests pass, clippy clean, fmt clean.

**Milestone gate: Complete.** The TUI remains visually intact during background backups,
the repository browser loads on focus entry, sync errors show actionable
details, and run logs are viewable from the History screen. All tests pass.

## 12. Multi-Select Source Browser

Replace the single-select source browser with a persistent multi-select
browser that shows existing sources as pre-checked, supports toggling
selections, generates ignore rules for deselected children inside folder
sources, and applies all changes atomically on Escape.

- [x] **MS01 - Add a multi-selection state model.** Create a `SourceSelection`
  struct with `selected: HashSet<PathBuf>` (explicitly checked sources) and
  `deselected: HashMap<PathBuf, Vec<String>>` (per-source relative paths to
  ignore). Implement `toggle(path)`, `is_selected(path) -> CheckState`
  (returning `Explicit`, `Inherited`, or `Unchecked`),
  `load_from_config(sources, home)`, and
  `diff_against_config(sources, home)` (producing adds, removes, and new
  ignore rules). Add unit tests for toggling, inheritance detection, config
  loading, and diffing.

- [x] **MS02 - Integrate selection state into the sources screen.** Add
  `selection: Option<SourceSelection>` to `SourcesScreen`. Initialize it from
  the current config when entering Browse mode (if not already initialized,
  preserving session persistence). Change Space in Browse mode to call
  `selection.toggle()` and return `Action::Consumed` instead of
  `Action::AddSource`. Keep the browser open after toggle. Add tests that
  Space toggles without closing, and that re-entering Browse mode preserves
  selection state.

- [x] **MS03 - Render checkboxes in the picker.** Add an optional
  `check_state: Option<&dyn Fn(&Path) -> CheckState>` parameter to
  `picker::draw()`. Render a prefix per entry: `[●]` (green/cyan) for
  `Explicit`, `[◉]` (dim) for `Inherited`, `[ ]` (dim) for `Unchecked`. Adjust
  column width for the 4-char prefix. When no check function is provided
  (repository browser), render without checkboxes. Add rendering tests at
  various terminal widths.

- [x] **MS04 - Update key handling for multi-select.** In
  `SourcesScreen::handle_key_browse`, intercept Space before delegating to
  the picker: get the current entry path, call `selection.toggle()`, return
  `Action::Consumed`. Esc now transitions to the apply step instead of
  returning to List mode immediately. Navigation keys remain unchanged. Add
  tests for Space toggling explicit/inherited/unchecked entries and Esc
  triggering apply.

- [x] **MS05 - Implement apply-on-Esc with removal confirmation.** Add
  `Mode::ConfirmApply` to the sources screen. On Esc from Browse mode,
  compute the diff. If removals exist, transition to `ConfirmApply` showing
  a summary ("Add N, remove M, add K ignore rules. Remove sources? y/n").
  On `y`, apply all changes (add new `SourceConfig` entries, remove unchecked
  ones, append anchored ignore rules), save config atomically, mark previews
  stale, return to List mode. On `n`/Esc, return to Browse mode. If no
  removals, apply immediately. Add tests for confirmation flow, no-removal
  fast path, and config persistence.

- [x] **MS06 - Handle inherited selection and ignore-rule generation.**
  Implement ancestor-walking in `is_selected()` to detect inheritance.
  When toggling an inherited entry off, compute the relative path from the
  ancestor source and store in `deselected[ancestor]`. Ignore rules use full
  relative paths with leading `/` for anchoring (e.g., `/completions/git.fish`,
  `/subfolder/`). Toggling a deselected entry back on removes it from the
  deselect list. Add tests for nested navigation, multi-level inheritance,
  directory vs file rule format, and re-selection.

- [x] **MS07 - Update UI rendering and help bar.** Update the browser status
  area to show selection summary ("N sources, M excluded"). Update help bar
  for Browse mode: `Space: toggle │ Esc: apply │ ↑↓←→ navigate │ :/ text`.
  Add `ConfirmApply` rendering with change summary and y/n prompt. Update
  feedback message after apply ("Added 2 sources, removed 1, added 3 ignore
  rules"). Add rendering tests at various terminal sizes.

- [x] **MS08 - Integration testing and edge cases.** Add end-to-end tests:
  enter browser → multi-select → Esc → confirm → verify config. Test:
  uncheck existing source with confirmation, inherited deselection produces
  correct anchored ignore rules, re-entering browser reflects applied config,
  empty selection is a no-op, overlap validation still catches conflicts,
  source validation rejects invalid entries, repository browser remains
  unaffected (no checkboxes). Run full quality suite.

**Milestone gate:** The source browser supports persistent multi-select with
visual checkbox indicators. Existing sources appear pre-checked, folder
selection inherits to children, deselecting children generates anchored ignore
rules, removals require confirmation, and the complete quality suite passes.

## 13. Multiple-Machine Namespaces

Replace the single root-level managed namespace with a user-named namespace
inside the configured repository. This milestone implements the planned layout
in `PLAN.md` "Planned Multiple-Machine Namespaces". A configured machine owns
only `<namespace>/home/` and `<namespace>/.dothoard-manifest.toml`; it must
never write, delete, stage, normalize, or claim another machine's directory.
Existing root-level `home/` and `.dothoard-manifest.toml` paths are unmanaged
repository content; this milestone provides no in-application migration.

- [x] **MN01 - Define namespace-aware schemas and validation.** Add an explicit
  user-selected machine namespace to local configuration and version the
  configuration and manifest schemas as required. Reject empty names, absolute
  paths, separators, `.`/`..`, traversal, and non-portable path components.
  Keep the namespace independent from the detected hostname. Add serialization
  and validation tests.

- [x] **MN02 - Define safe namespace ownership states.** Extend ownership
  classification to inspect only the selected namespace and its manifest. Add
  explicit states for new, owned, invalid, and ambiguous namespaces. Treat
  root-level V1 paths and sibling namespaces as unmanaged content: never
  silently adopt, move, or delete them. Test every state with temporary
  repositories.

- [x] **MN03 - Make source mapping and mirror boundaries namespace-aware.** Map
  home-relative sources beneath `<repository>/<namespace>/home/`, and update
  reverse mapping, managed-path checks, destination preflight, symlink checks,
  copying, deletion, dry-run paths, and change-set presentation accordingly.
  Preserve all no-follow and repository-boundary guarantees. Add filesystem
  regression tests proving a run cannot affect sibling namespaces.

- [x] **MN04 - Generate per-namespace manifests.** Store, validate, atomically
  replace, and compare each manifest at
  `<repository>/<namespace>/.dothoard-manifest.toml`. Record the namespace in
  the manifest when the versioned schema requires it and reject a manifest
  whose declared namespace does not match its directory. Test cross-namespace
  manifest substitution and malformed namespace inputs.

- [x] **MN05 - Restrict Git worktree handling and publication to one namespace.**
  Stage only the active namespace directory with literal pathspecs and verify
  that every staged path is within it. Classify a clean, committed sibling
  namespace as untouched repository content; staged or dirty paths outside the
  active namespace remain blocking unmanaged changes. Preserve current
  noninteractive synchronization and conflict recovery behavior. Add Git
  integration tests with two machine namespaces sharing one local bare remote.

- [x] **MN06 - Integrate namespace selection into orchestration and headless
  commands.** Load and validate the selected namespace before planning,
  ownership initialization, mirroring, and publication. Ensure `backup` and
  `check` report the active namespace and actionable ownership errors, while
  state and notifications identify the relevant machine without exposing
  sensitive data. Test initial backup, no-op backup, offline retry, and blocked
  external changes for two independent namespaces.

- [x] **MN07 - Implement safe namespace lifecycle operations.** Implement the
  backend operations needed by TUI create, select, rename, and delete actions
  with narrow filesystem boundaries, atomic configuration updates where
  applicable, and recoverable failure handling. Creation may initialize only a
  chosen empty namespace. A rename may affect only the selected namespace;
  deletion may remove only that namespace's owned `home/` and manifest after
  confirmation. Never adopt, move, or delete root-level V1 paths or sibling
  namespaces. Add filesystem and Git tests for collisions, cancellation,
  partial failures, and staged-boundary protection.

- [x] **MN08 - Build TUI namespace management.** Let users create, select,
  rename, and delete valid namespaces both during initial repository setup and
  at any later time. Display the active namespace in Repository and Dashboard
  views. Renaming and deletion must show the affected path, require explicit
  confirmation, reject collisions and invalid or ambiguous content, and never
  affect a sibling namespace. Require the user to select or create another
  active namespace before deleting the current one. Keep source and ignore
  editing scoped to the active namespace; changing it invalidates dependent
  previews safely. Add interaction and rendering tests.

- [x] **MN09 - Document and accept multiple-machine operation.** Update the
  README and safety documentation with repository layout, independent machine
  setup, namespace lifecycle, synchronization expectations, and limitations. Run the
  complete formatting, Clippy, unit, filesystem, Git, orchestration, and TUI
  test baseline, including a manual two-machine smoke test against a shared
  remote.

**Milestone gate:** Two or more computers can use one repository through
user-named, non-overlapping namespaces. Each machine can back up and
synchronize only its own directory; it cannot alter or publish a sibling
namespace. Users can create, select, rename, and delete namespaces safely in
the TUI, while root-level V1 data remains unmanaged and untouched. The full
quality suite passes.

## 14. TUI Usability and Visual Design

Refine the existing TUI according to `PLAN.md` "Current Objective: TUI
Usability and Visual Design". This milestone changes presentation and
interaction behavior, not backup, ownership, Git publication, or namespace
safety. Complete tasks in order. Every defect correction begins with a
regression test that demonstrates the current failure.

- [x] **TU01 - Make text editing and truncation Unicode-safe.** Add failing
  regression tests for multibyte repository paths, source paths, ignore
  patterns, namespace input, picker entries, breadcrumbs, dashboard values,
  and errors. Replace byte-by-byte cursor movement, insertion, deletion, and
  string slicing with shared UTF-8-boundary-safe helpers. Truncate by terminal
  display cells and handle wide and combining characters without panic. Keep
  filesystem-native paths until the existing configuration UTF-8 boundary;
  non-UTF-8 entries remain navigable but unselectable. Reuse Ratatui APIs or add
  only the smallest necessary width dependency. Verify all text-input modes and
  narrow picker rendering.

- [x] **TU02 - Implement reliable list and preview viewports.** Add explicit
  viewport state for History and Ignore Preview, calculate visible rows from
  the actual render area, and keep the selected or active row visible during
  Up/Down, Vim navigation, Home/End, and PageUp/PageDown. Replace Ignore
  Preview's fixed first-20 rendering and no-op scroll handling with a real
  viewport and range indicator. Preserve viewport state across tab-focus
  changes, clamp it after data refresh or shrinkage, and test empty, one-row,
  long-list, resize, first-row, and last-row cases.

- [x] **TU03 - Standardize back, cancel, apply, and quit behavior.** Make `Esc`
  back out one interaction level and prevent Repository browser Escape from
  quitting the application. Reserve `q` and `Ctrl+C` for explicit quit outside
  text entry and modal ownership. When leaving a changed Sources browser,
  present an explicit pending-changes choice to apply, discard, or continue
  editing; never silently apply changes. Preserve removal confirmation and
  distinguish cancellation from discard. Define a complete key-transition
  matrix for every screen mode and add interaction tests for Esc, `q`, Tab,
  Shift+Tab, confirmations, unchanged source sessions, and pending additions,
  removals, and ignore rules.

- [x] **TU04 - Move slow TUI reads and validation off the render thread.** Use
  the existing task/event architecture for repository validation, backup
  preview generation, Ignore Preview generation, and initial automation
  inspection. Introduce typed per-screen `NotLoaded`, `Loading`, `Loaded`,
  `Stale`, and `Failed` states rather than overlapping booleans and optional
  strings. Prevent duplicate conflicting tasks, preserve the last usable data
  while a refresh runs where safe, and ignore stale results after repository,
  namespace, or source changes. Test task start, completion, failure,
  cancellation-by-invalidation, and continued input/render responsiveness with
  controlled backends rather than real external state.

- [x] **TU05 - Separate contextual help from status and progress.** Establish
  one authoritative, mode-aware shortcut footer and remove duplicated in-body
  key hints. Add a separate status region for transient success, warning,
  error, and running messages so feedback never hides shortcuts. Define message
  priority, expiry or dismissal, and behavior for narrow terminals. Ensure all
  screen modes advertise only actions they currently accept, including Ignore
  input/preview, Sources pending changes, Repository namespace actions, and
  confirmations. Add rendering and timer/event tests for help/status
  coexistence and message lifecycle.

- [x] **TU06 - Introduce a shared visual theme and explicit focus language.**
  Centralize styles for screen borders, headings, labels, muted text, focused
  controls, selections, success, warning, error, progress, and disabled states.
  Make tab-bar focus, content focus, nested-control focus, and selected rows
  distinguishable without relying on color alone. Add visible mode labels such
  as Browsing, Editing, Previewing, Confirming, and Running where they clarify
  input ownership. Replace essential low-contrast text and add style-sensitive
  buffer assertions for dark, light-compatible, color-reduced, focused, and
  unfocused states.

- [x] **TU07 - Build consistent modal and text-input presentation.** Create
  reusable centered modal rendering with background de-emphasis, title,
  affected object or path, consequence, validation/error area, and explicit
  confirm/cancel actions. Adopt it for namespace create/select/rename/delete,
  repository initialize/attach, source apply/removal, and automation changes
  without weakening existing confirmations. Render all text inputs with the
  shared Unicode-safe editor, visible cursor, label, validation state, and
  consistent submit/cancel behavior. Test input ownership, modal precedence,
  resize behavior, long affected paths, and narrow/short terminals.

- [x] **TU08 - Redesign the Dashboard around health and next actions.** Make
  backup health, last successful backup, pending push state, and automation
  health the primary summaries. Keep repository, active namespace, source
  count, schedule, and timeout secondary. Render the latest check result and
  its first actionable issue, including running and unavailable states. Add a
  clear recommended next action for unconfigured, unhealthy, pending-push, and
  healthy states. Replace unsafe path/error truncation, provide access to full
  details, stack content on narrow terminals, and test all health combinations.

- [x] **TU09 - Make namespace management visible and history namespace-aware.**
  Add a Repository namespace control that lists discovered namespaces,
  identifies the active namespace, and shows ownership state. Expose create,
  select, rename, and delete actions visibly while continuing to call the
  existing safety-sensitive backend lifecycle operations. Do not infer
  ownership or adopt ambiguous content in the UI. Include namespace identity
  in History rows, details, and log context; clamp selection after namespace or
  state changes. Test new, owned, invalid, ambiguous, active, sibling,
  collision, and narrow-layout presentations.

- [x] **TU10 - Clarify Sources, Ignore, Preview, and browser presentation.** Show
  pending source additions, removals, and generated ignore rules before apply.
  Make Ignore's source-selector and pattern-list focus visually distinct and
  show the active rule context in preview. Replace symbolic Preview totals with
  labeled Added, Changed, Deleted, Ignored, and Warning counts while retaining
  exact managed paths and warning details. Label picker panes Parent, Files,
  and Preview; mark the active pane; make picker instructions caller-specific;
  and use predictable-width symbols or an ASCII-safe fallback. Add focused,
  empty, populated, warning, and long-path rendering tests.

- [x] **TU11 - Add actionable empty, loading, stale, and failure states.** Audit
  all seven screens and ensure each non-content state explains why data is
  absent, identifies the next valid action, and disables actions without a
  valid target. Load Preview and Automation on first entry when configuration
  permits, retain explicit refresh, and mark dependent data visibly stale after
  repository, namespace, source, or ignore changes. Ensure errors offer retry
  or detail access without discarding the last safe data. Test first run,
  missing config, no sources, no history, unavailable automation, stale
  previews, loading, failure, retry, and recovery.

- [x] **TU12 - Complete responsive layout behavior.** Define supported wide,
  medium, narrow, and short-terminal breakpoints for the global shell and every
  screen. Stack Dashboard and History panes when columns become unusable;
  compact or scroll the seven-tab header; preserve the active tab, primary
  status, focused control, modal actions, and footer before secondary details.
  Apply display-width-safe wrapping and truncation to breadcrumbs, tabs, paths,
  messages, and dialog content. Add geometry- and style-aware rendering tests
  at representative dimensions rather than relying only on text-presence or
  no-panic assertions.

- [x] **TU13 - Document and visually accept the refined interaction model.**
  Update `README.md` with a concise keyboard reference, focus explanation,
  source apply/discard workflow, namespace management workflow, and first-run
  path from repository selection through automation. Manually smoke-test all
  screens in a real terminal with dark and light-compatible palettes, keyboard
  and Vim aliases, Unicode filenames, long paths, resize events, and narrow
  layouts. Record only durable findings and resolve visual or interaction
  defects before the milestone gate.

- [x] **TU14 - Run complete TUI usability acceptance.** Add end-to-end
  interaction tests for the first-run workflow, routine preview/backup flow,
  namespace switching, source editing, ignore preview, automation, History and
  logs, task failures, and recovery. Re-run the complete formatting, Clippy,
  serialized test, filesystem, Git, orchestration, and TUI baseline. Confirm
  that this milestone changes no managed-path, ownership, staging, publication,
  or noninteractive-process safety boundary.

**Milestone gate: Complete.** Unicode input and long filenames cannot crash
rendering or editing; every list keeps its active item visible; back, apply,
cancel, and quit behavior is consistent; slow work never freezes the event
loop; help remains visible beside status; focus and modal ownership are visually
explicit; the Dashboard presents health and next actions; namespaces are
discoverable and appear in History; all screens remain usable at supported
terminal sizes; the README and real-terminal smoke test match the
implementation; and the complete quality suite passes without weakening backend
safety.

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
  -> TUI Usability Improvements
  -> TUI Bug Fixes
  -> Multi-Select Source Browser
  -> Multiple-Machine Namespaces
  -> TUI Usability and Visual Design
  -> Project Visibility and Documentation
  -> Portable Namespace Setup
```

The explicit naming prerequisite avoids introducing installed paths and unit
names that would require a migration before release.

## 15. Project Visibility and Documentation

Improve the public presentation of dothoard without changing backup behavior,
ownership boundaries, or safety guarantees. The project is experimental until
its release readiness is explicitly established.

- [x] **PV01 - Improve the GitHub landing page.** Rewrite the opening README
  section around the product value, intended audience, experimental status,
  supported platforms, and a five-minute first-run workflow. Add a concise
  safety-guarantees section, relevant repository topics, a useful description,
  and a social preview asset.
- [x] **PV02 - Add visual demonstrations.** Capture polished screenshots of the
  Dashboard, source browser, and backup preview, and add a short terminal
  recording where practical. Ensure all visuals reflect the current UI and do
  not reveal personal paths, credentials, or private repository information.
- [x] **PV03 - Restructure user documentation.** Organize beginner, usage, and
  reference material covering installation, first-run TUI use, keyboard
  navigation, configuration, ignore rules, namespaces, authentication, safety,
  limitations, troubleshooting, FAQ, development, and contribution guidance.
- [x] **PV04 - Publish a static documentation website.** Create a Markdown-based
  static site, preferably with Astro Starlight unless another option is shown
  to fit better, and publish it through GitHub Pages. Include a welcoming
  landing page and searchable or clearly navigable documentation.
- [x] **PV05 - Establish public trust and discoverability.** Add CI for the
  formatting, Clippy, and test baseline; provide clearly marked pre-release
  notes or GitHub Releases, issue templates, and `SECURITY.md`. Evaluate AUR
  packaging after the release workflow is stable.
- [ ] **PV06 - Complete visibility acceptance.** Verify README and website
  links, commands, screenshots, supported-platform claims, experimental status,
  and safety statements against the implementation. Confirm that a new visitor
  can understand the project and first-run path from the first page, and that
  the documentation site builds successfully.
- [x] **PV07 - Automate release builds and provide an install script.** Add a
  tag-triggered GitHub Actions workflow that repeats the CI quality baseline,
  builds an x86_64 Linux binary, and uploads it with a checksum as a draft
  Release for a maintainer to finish and publish. Add `scripts/install.sh`, a
  POSIX-sh curl-pipeable installer that detects platform, downloads and
  checksum-verifies the requested (default latest) release, and installs to
  `$INSTALL_DIR` (default `~/.local/bin`). Update the README and
  `docs/quick-start.md`/`docs/releases.md`/`docs/development.md` accordingly.

**Milestone gate:** The GitHub page and documentation website clearly explain
what dothoard does, who it is for, how to try it, and why its safety model is
trustworthy. Visuals are current and sanitized, unfinished functionality is not
presented as stable, documentation links and site builds pass, and CI covers
the documented quality baseline.

## 16. Portable Namespace Setup

Make namespace manifests useful when attaching a fresh installation, and remove
the implicit first-run namespace.

- [x] **PN01 - Restore namespace source configuration.** When an owned namespace
  is selected, validate and copy its manifest source paths and ignore rules into
  local configuration. Selecting a new namespace clears sources, and selecting
  a replacement during deletion loads the replacement manifest. Invalid
  manifest source configuration is refused.
- [x] **PN02 - Require first-run namespace choice.** Start an unconfigured TUI in
  Repository setup. Validate the repository before namespace ownership, list
  discovered namespaces, and require explicit selection or creation without a
  `desktop` or hostname-derived default.
- [x] **PN03 - Document and verify portable setup.** Update product and user
  documentation, add backend and TUI regressions, and run the complete Rust
  quality baseline.

**Milestone gate: Complete.** A fresh installation selects its repository and
then explicitly selects or creates a namespace. Selecting an existing owned
namespace restores its validated source and ignore configuration, while all
managed-path and sibling-namespace safety boundaries remain unchanged.

## 17. Portable Backup Automation

Extend scheduled backups beyond systemd without turning dothoard into a
persistent daemon. Keep `dothoard backup` as the scheduler-independent execution
boundary and preserve locking, noninteractive Git behavior, bounded timeouts,
state persistence, and notifications for every scheduler.

- [x] **AP01 - Document external scheduler operation.** Document safe direct
  invocation of the absolute `dothoard backup` path from cron and comparable
  schedulers, including minimal-environment, missed-run, fixed-wall-clock,
  credential-agent, notification, logging, and overlap considerations. Keep
  systemd as the only automation backend managed by the application at this
  stage.
- [ ] **AP02 - Introduce a scheduler-neutral automation layer.** Move systemd
  generation and management behind generic install, remove, status, refresh,
  and staleness concepts. Update CLI checks and TUI language to depend on the
  generic layer while preserving systemd unit content and behavior exactly.
- [ ] **AP03 - Add explicit cron automation.** Add a configuration-selected cron
  backend with deterministic, clearly delimited managed content and safe
  install, removal, status, and update behavior. Preserve unrelated crontab
  content, reject malformed or ambiguous managed blocks, invoke `crontab`
  directly without shell interpolation, and document that cron does not replay
  missed runs or provide systemd's completion-relative timing.
- [ ] **AP04 - Complete portable-automation acceptance.** Cover provider
  selection, generation, lifecycle operations, health checks, TUI status, and
  controlled command execution without touching a real crontab or user service
  manager. Update user documentation and supported-platform claims, then run
  the complete serialized quality baseline.

**Milestone gate:** A user can explicitly choose systemd or cron automation,
install and inspect it through the CLI and TUI, and receive the same safe
short-lived backup behavior. Existing systemd installations remain compatible,
unrelated scheduler configuration is untouched, tests mutate no real scheduler
state, and dothoard remains non-daemonized.

## Maintenance

- [x] **UI01 - Simplify configured repository selection.** Hide `.git`
  directories from filesystem pickers and mark their containing repositories
  with a Git icon. Once configured, root browsing at the
  selected repository so its contents remain visible but its parent is
  inaccessible; expose `c` as an explicit change action that restarts selection
  at `$HOME`. Cover listing, interaction,
  root-boundary, help, and rendering behavior with regressions.
- [x] **UI02 - Preview selected file contents.** Extend the shared picker Preview
  pane to show cached, refreshable regular-file content using a direct external
  reader while preserving no-follow safety. Limit previews to 256 KiB, sanitize
  control characters for terminal rendering, retain file metadata, and cover
  content, refresh, and oversized-file behavior with regressions.
- [x] **UI03 - Scroll file content previews from the keyboard.** Keep file
  metadata visible while independently scrolling wrapped content with
  `Ctrl+Up`/`Ctrl+Down` or `Ctrl+k`/`Ctrl+j`. Reset the content viewport when the
  selected entry changes, show its visible range, update contextual help, and
  keep implementation details out of user-facing labels and documentation.
- [x] **UI04 - Add general pointer support.** Enable terminal mouse capture and
  use render-time hit regions so clicks select tabs, list rows, namespace/source
  controls, picker entries, checkboxes, themes, and visible shortcut actions.
  Route touchpad/mouse wheel input to the scrollable list or picker pane beneath
  the pointer, preserve modal input ownership, restore terminal capture state,
  and cover pointer dispatch and rendered geometry with regressions.
- [x] **UI05 - Follow terminal personalization by default.** Add a default
  System theme that uses the terminal's configured foreground, background, and
  ANSI colors so compatible live palette changes propagate automatically.
  Preserve fixed RGB themes and preference persistence, and document how to
  return to System from the theme picker.
