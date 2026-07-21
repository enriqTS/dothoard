//! V1 Acceptance Tests
//!
//! This file verifies every acceptance criterion from PLAN.md in a clean
//! temporary environment. Criteria that require a real terminal, real systemd,
//! or real network are validated structurally rather than end-to-end.
//!
//! Acceptance criteria from PLAN.md:
//! 1. A user can select an existing Git clone in the TUI.
//! 2. Repository initialization and attachment never claim ambiguous content.
//! 3. A user can add files and directories from $HOME.
//! 4. A user can configure and preview ignore rules per source.
//! 5. A preview accurately reports additions, changes, deletions, exclusions.
//! 6. A manual backup creates and pushes a commit only when files changed.
//! 7. An offline backup creates a local commit and a later run pushes it.
//! 8. The user timer runs after startup and after each configured interval.
//! 9. Background failures appear in a desktop notification and the TUI.
//! 10. Concurrent runs cannot corrupt the repository.
//! 11. The application behaves identically from Fish, Bash, or Zsh.
//!
//! Run with: cargo test --test acceptance -- --test-threads=1

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::time::Duration;

use dothoard::backup::coordinator::{self, BackupOutcome};
use dothoard::backup::planner::{PlanInputs, plan_backup};
use dothoard::config::{Config, SourceConfig};
use dothoard::git::{GitCommand, GitRunner, OwnershipState, classify_ownership};
use dothoard::locking;
use dothoard::notification;
use dothoard::paths::{AppPaths, PathInputs};
use dothoard::state::{AppState, RunOutcome};
use dothoard::systemd;

/// Fully isolated environment for acceptance tests.
struct AcceptanceEnv {
    _tmp: tempfile::TempDir,
    paths: AppPaths,
    repository: PathBuf,
    remote: PathBuf,
    home: PathBuf,
    runner: GitRunner,
}

impl AcceptanceEnv {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        let home = root.join("home");
        let config_dir = root.join("config");
        let state_dir = root.join("state");
        let runtime_dir = root.join("runtime");
        let repository = root.join("repo");
        let remote = root.join("remote.git");

        for dir in [
            &home,
            &config_dir,
            &state_dir,
            &runtime_dir,
            &repository,
            &remote,
        ] {
            fs::create_dir_all(dir).unwrap();
        }

        let runner = GitRunner::new(Duration::from_secs(10));

        // Create bare remote.
        let cmd = GitCommand::new(&remote).args(["init", "--bare", "--initial-branch=main"]);
        runner.run(&cmd).unwrap();

        // Create working repo.
        let cmd = GitCommand::new(&repository).args(["init", "--initial-branch=main"]);
        runner.run(&cmd).unwrap();

        let cmd = GitCommand::new(&repository).args([
            "remote",
            "add",
            "origin",
            remote.to_str().unwrap(),
        ]);
        runner.run(&cmd).unwrap();

        // Initial commit and push.
        let cmd = GitCommand::new(&repository).args(["commit", "--allow-empty", "-m", "initial"]);
        runner.run(&cmd).unwrap();

        let cmd = GitCommand::new(&repository)
            .args(["push", "-u", "origin", "main"])
            .network();
        runner.run(&cmd).unwrap();

        let paths = AppPaths::resolve(PathInputs {
            home: Some(home.clone()),
            config_dir: Some(config_dir),
            state_dir: Some(state_dir),
            runtime_dir: Some(runtime_dir),
            use_environment: false,
        })
        .unwrap();

        Self {
            _tmp: tmp,
            paths,
            repository,
            remote,
            home,
            runner,
        }
    }

    fn write_config(&self, sources: &[SourceConfig]) {
        let config = Config {
            version: 1,
            repository: self.repository.to_str().unwrap().to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 10,
            sources: sources.to_vec(),
        };
        config.save(self.paths.config_file()).unwrap();
    }

    fn create_source_dir(&self, rel_path: &str, files: &[(&str, &str)]) {
        let dir = self.home.join(rel_path);
        fs::create_dir_all(&dir).unwrap();
        for (name, content) in files {
            let path = dir.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, content).unwrap();
        }
    }

    fn create_source_file(&self, rel_path: &str, content: &str) {
        let path = self.home.join(rel_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn run_backup(&self) -> BackupOutcome {
        coordinator::run_backup(&self.paths).unwrap()
    }

    fn commit_count(&self) -> usize {
        let cmd = GitCommand::new(&self.repository).args(["rev-list", "--count", "HEAD"]);
        self.runner
            .run(&cmd)
            .unwrap()
            .stdout_trimmed()
            .parse()
            .unwrap()
    }

    fn remote_commit_count(&self) -> usize {
        let cmd =
            GitCommand::new(&self.repository).args(["rev-list", "--count", "origin/main"]);
        self.runner
            .run(&cmd)
            .unwrap()
            .stdout_trimmed()
            .parse()
            .unwrap()
    }

    fn load_state(&self) -> AppState {
        AppState::load(self.paths.state_dir()).unwrap_or_default()
    }
}

// =============================================================================
// Criterion 1: A user can select an existing Git clone in the TUI.
//
// The TUI is tested via unit tests in tui::screens::repository. Here we verify
// the underlying backend operation: ownership classification of a valid clone.
// =============================================================================

#[test]
fn ac01_existing_clone_is_recognized_as_valid() {
    let env = AcceptanceEnv::new();

    // A fresh repository with no home/ namespace should be classifiable.
    let state = classify_ownership(&env.repository).unwrap();
    assert!(matches!(state, OwnershipState::New));
}

#[test]
fn ac01_clone_with_valid_manifest_is_recognized() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(".config/fish", &[("config.fish", "content")]);
    env.write_config(&[SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }]);

    // Run a backup to establish the manifest.
    let outcome = env.run_backup();
    assert!(outcome.success);

    // Reclassify — should now recognize valid manifest.
    let state = classify_ownership(&env.repository).unwrap();
    assert!(matches!(state, OwnershipState::Owned { .. }));
}

// =============================================================================
// Criterion 2: Repository initialization never claims ambiguous content.
// =============================================================================

#[test]
fn ac02_ambiguous_home_content_is_refused() {
    let env = AcceptanceEnv::new();

    // Create home/ directory with content but NO manifest.
    fs::create_dir_all(env.repository.join("home/.config")).unwrap();
    fs::write(
        env.repository.join("home/.config/something.conf"),
        "mystery content",
    )
    .unwrap();

    // Stage and commit so git tracks it.
    let cmd = GitCommand::new(&env.repository).args(["add", "home/"]);
    env.runner.run(&cmd).unwrap();
    let cmd = GitCommand::new(&env.repository).args(["commit", "-m", "ambiguous content"]);
    env.runner.run(&cmd).unwrap();

    // Classify — should detect ambiguous content.
    let state = classify_ownership(&env.repository).unwrap();
    assert!(
        matches!(state, OwnershipState::Ambiguous { .. }),
        "expected Ambiguous, got: {state:?}"
    );
}

// =============================================================================
// Criterion 3: A user can add files and directories from $HOME.
//
// Verified through the configuration model and backup execution: both directory
// sources and single-file sources are supported.
// =============================================================================

#[test]
fn ac03_directory_source_is_backed_up() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(
        ".config/hypr",
        &[
            ("hyprland.conf", "monitor = DP-1"),
            ("keybinds.conf", "bind = SUPER, Return, exec, kitty"),
        ],
    );
    env.write_config(&[SourceConfig {
        path: ".config/hypr".to_string(),
        ignore: vec![],
    }]);

    let outcome = env.run_backup();
    assert!(outcome.success);
    assert_eq!(outcome.copies, 2);
    assert!(env.repository.join("home/.config/hypr/hyprland.conf").exists());
    assert!(env.repository.join("home/.config/hypr/keybinds.conf").exists());
}

#[test]
fn ac03_single_file_source_is_backed_up() {
    let env = AcceptanceEnv::new();
    env.create_source_file(".bashrc", "export PATH=/usr/bin");
    env.write_config(&[SourceConfig {
        path: ".bashrc".to_string(),
        ignore: vec![],
    }]);

    let outcome = env.run_backup();
    assert!(outcome.success);
    assert_eq!(outcome.copies, 1);
    assert!(env.repository.join("home/.bashrc").exists());
}

#[test]
fn ac03_symlink_source_root_is_preserved_as_link() {
    let env = AcceptanceEnv::new();

    // Create a real directory and a symlink to it.
    let real_dir = env.home.join(".config/real-fish");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("config.fish"), "real content").unwrap();

    // Symlink: ~/.config/fish-link -> real directory
    symlink(&real_dir, env.home.join(".config/fish-link")).unwrap();

    env.write_config(&[SourceConfig {
        path: ".config/fish-link".to_string(),
        ignore: vec![],
    }]);

    let outcome = env.run_backup();
    assert!(outcome.success);

    // The symlink itself should be stored as a link in the repo.
    let dest = env.repository.join("home/.config/fish-link");
    assert!(dest.symlink_metadata().unwrap().file_type().is_symlink());
}

// =============================================================================
// Criterion 4: A user can configure and preview ignore rules per source.
// =============================================================================

#[test]
fn ac04_ignore_rules_exclude_matched_files_in_preview() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(
        ".config/app",
        &[
            ("config.toml", "keep"),
            ("secrets.env", "API_KEY=sk-xxx"),
            ("cache/data.bin", "cached"),
            ("logs/app.log", "log line"),
        ],
    );

    let sources = vec![SourceConfig {
        path: ".config/app".to_string(),
        ignore: vec![
            "secrets.env".to_string(),
            "cache/".to_string(),
            "*.log".to_string(),
        ],
    }];

    let inputs = PlanInputs {
        home: &env.home,
        repository: &env.repository,
        sources: &sources,
    };
    let changeset = plan_backup(&inputs).unwrap();

    // Only config.toml should be added; the rest excluded.
    assert_eq!(changeset.additions.len(), 1);

    // Exclusions should be reported.
    assert!(
        changeset.exclusions.len() >= 3,
        "expected at least 3 exclusions, got {}",
        changeset.exclusions.len()
    );
}

// =============================================================================
// Criterion 5: A preview accurately reports additions, changes, deletions,
// and exclusions.
// =============================================================================

#[test]
fn ac05_preview_reports_all_change_types() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(
        ".config/app",
        &[
            ("keep.conf", "original"),
            ("remove.conf", "to be removed"),
            ("ignored.log", "excluded"),
        ],
    );

    let sources = vec![SourceConfig {
        path: ".config/app".to_string(),
        ignore: vec!["*.log".to_string()],
    }];

    // First backup — establishes baseline.
    env.write_config(&sources);
    let outcome = env.run_backup();
    assert!(outcome.success);

    // Now modify source: change one file, delete another, add a new one.
    fs::write(env.home.join(".config/app/keep.conf"), "modified").unwrap();
    fs::remove_file(env.home.join(".config/app/remove.conf")).unwrap();
    fs::write(env.home.join(".config/app/new.conf"), "brand new").unwrap();

    let inputs = PlanInputs {
        home: &env.home,
        repository: &env.repository,
        sources: &sources,
    };
    let changeset = plan_backup(&inputs).unwrap();

    // Check all types are reported.
    assert!(
        !changeset.additions.is_empty(),
        "should report new.conf as addition"
    );
    assert!(
        !changeset.modifications.is_empty(),
        "should report keep.conf as modification"
    );
    assert!(
        !changeset.deletions.is_empty(),
        "should report remove.conf as deletion"
    );
    assert!(
        !changeset.exclusions.is_empty(),
        "should report exclusions"
    );
}

// =============================================================================
// Criterion 6: A manual backup creates and pushes a commit only when files
// changed.
// =============================================================================

#[test]
fn ac06_backup_commits_and_pushes_only_when_changed() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(".config/fish", &[("config.fish", "content")]);
    env.write_config(&[SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }]);

    // First backup — should commit and push.
    let outcome1 = env.run_backup();
    assert!(outcome1.success);
    assert!(outcome1.commit.is_some(), "should create a commit");
    assert!(outcome1.pushed, "should push to remote");

    let commits_after_first = env.commit_count();
    let remote_commits_after_first = env.remote_commit_count();

    // Second backup — nothing changed, no commit.
    let outcome2 = env.run_backup();
    assert!(outcome2.success);
    assert!(outcome2.commit.is_none(), "should not create a commit");
    assert_eq!(
        env.commit_count(),
        commits_after_first,
        "commit count unchanged"
    );
    assert_eq!(
        env.remote_commit_count(),
        remote_commits_after_first,
        "remote unchanged"
    );

    // Third backup — file changed, should commit and push.
    fs::write(env.home.join(".config/fish/config.fish"), "updated").unwrap();
    let outcome3 = env.run_backup();
    assert!(outcome3.success);
    assert!(outcome3.commit.is_some(), "should commit the change");
    assert!(outcome3.pushed, "should push the change");
    assert_eq!(env.commit_count(), commits_after_first + 1);
}

// =============================================================================
// Criterion 7: An offline backup creates a local commit and a later run pushes.
// =============================================================================

#[test]
fn ac07_offline_commit_preserved_and_pushed_later() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(".config/fish", &[("config.fish", "initial")]);
    env.write_config(&[SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }]);

    // First backup online.
    let outcome1 = env.run_backup();
    assert!(outcome1.success);
    assert!(outcome1.pushed);

    // Break the remote by renaming it.
    let broken_remote = env._tmp.path().join("broken-remote.git");
    fs::rename(&env.remote, &broken_remote).unwrap();

    // Modify and backup — commit should succeed, push should fail.
    fs::write(env.home.join(".config/fish/config.fish"), "offline change").unwrap();
    let outcome2 = env.run_backup();

    // The local commit should exist regardless of push outcome.
    let local_commits = env.commit_count();
    assert!(
        local_commits >= 3,
        "local commit should exist: {local_commits}"
    );

    // Verify the outcome records pending push state.
    if outcome2.success {
        // Some implementations report success with pending_push=true.
        assert!(
            outcome2.pending_push || !outcome2.pushed,
            "offline run should have pending push or not pushed"
        );
    }

    // Restore the remote.
    fs::rename(&broken_remote, &env.remote).unwrap();

    // Modify source again to ensure backup has something to do, triggering sync.
    fs::write(env.home.join(".config/fish/config.fish"), "online again").unwrap();

    // Run again — should push all pending commits.
    let outcome3 = env.run_backup();
    assert!(outcome3.success);
    assert!(outcome3.pushed, "should push after remote is restored");

    // Remote should now be in sync.
    let cmd = GitCommand::new(&env.repository)
        .args(["fetch", "origin"])
        .network();
    env.runner.run(&cmd).unwrap();

    let local_head = {
        let cmd = GitCommand::new(&env.repository).args(["rev-parse", "HEAD"]);
        env.runner.run(&cmd).unwrap().stdout_trimmed().to_string()
    };
    let remote_head = {
        let cmd = GitCommand::new(&env.repository).args(["rev-parse", "origin/main"]);
        env.runner.run(&cmd).unwrap().stdout_trimmed().to_string()
    };
    assert_eq!(local_head, remote_head, "local and remote should be in sync");
}

// =============================================================================
// Criterion 8: The user timer runs after startup and after each interval.
//
// Verified structurally: we generate the timer unit and verify its content
// matches the expected OnStartupSec and OnUnitInactiveSec values.
// =============================================================================

#[test]
fn ac08_timer_unit_has_correct_schedule() {
    let config = Config {
        version: 1,
        repository: "/tmp/test-repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 7,
        network_timeout_seconds: 120,
        sources: vec![],
    };

    let params = systemd::params_from_config(&config).unwrap();
    let timer_content = systemd::generate_timer_unit(&params);

    assert!(
        timer_content.contains("OnStartupSec=1min"),
        "timer should fire 1min after startup: {timer_content}"
    );
    assert!(
        timer_content.contains("OnUnitInactiveSec=7min"),
        "timer should fire every 7 minutes: {timer_content}"
    );
}

#[test]
fn ac08_service_unit_has_finite_timeout() {
    let config = Config {
        version: 1,
        repository: "/tmp/test-repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        network_timeout_seconds: 120,
        sources: vec![],
    };

    let params = systemd::params_from_config(&config).unwrap();
    let service_content = systemd::generate_service_unit(&params);

    // Service timeout should be network_timeout + 60s = 180s.
    assert!(
        service_content.contains("TimeoutStartSec=180"),
        "service should have finite timeout: {service_content}"
    );
}

// =============================================================================
// Criterion 9: Background failures appear in notification and TUI state.
// =============================================================================

#[test]
fn ac09_failure_is_persisted_to_state() {
    let env = AcceptanceEnv::new();
    env.write_config(&[SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }]);

    // Create dirty unmanaged path to cause failure.
    fs::write(env.repository.join("unmanaged.txt"), "block").unwrap();
    let cmd = GitCommand::new(&env.repository).args(["add", "unmanaged.txt"]);
    env.runner.run(&cmd).unwrap();

    let outcome = env.run_backup();
    assert!(!outcome.success);
    assert!(outcome.error.is_some());

    // State should record the failure.
    let state = env.load_state();
    assert!(state.latest_error.is_some());
    assert!(!state.history.is_empty());
    assert_eq!(
        state.history[0].outcome,
        RunOutcome::Failed,
        "most recent history entry should be failure"
    );
}

#[test]
fn ac09_notification_decision_reflects_failure_and_recovery() {
    // Failure notification.
    let empty_state = AppState::default();
    let result = notification::decide_notification(false, Some("network timeout"), &empty_state);
    assert!(result.is_some());
    let (summary, _body, urgency) = result.unwrap();
    assert!(summary.contains("failed"));
    assert_eq!(urgency, notification::Urgency::Critical);

    // Recovery notification: previous state had an error.
    let mut failed_state = AppState::default();
    failed_state.latest_error = Some("previous failure".to_string());
    let result = notification::decide_notification(true, None, &failed_state);
    assert!(result.is_some());
    let (summary, _body, urgency) = result.unwrap();
    assert!(summary.contains("recovered"));
    assert_eq!(urgency, notification::Urgency::Normal);

    // Quiet success: no previous error, no notification.
    let result = notification::decide_notification(true, None, &empty_state);
    assert!(result.is_none());
}

// =============================================================================
// Criterion 10: Concurrent runs cannot corrupt the repository.
// =============================================================================

#[test]
fn ac10_second_backup_is_locked_out() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(".config/fish", &[("config.fish", "content")]);
    env.write_config(&[SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }]);

    // Hold the exclusive lock.
    let _guard = locking::try_acquire(env.paths.runtime_dir()).unwrap();

    // A second backup attempt should fail with AlreadyRunning.
    let result = coordinator::run_backup(&env.paths);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("already running"),
        "should report lock contention: {err}"
    );
}

#[test]
fn ac10_lock_is_released_after_backup() {
    let env = AcceptanceEnv::new();
    env.create_source_dir(".config/fish", &[("config.fish", "content")]);
    env.write_config(&[SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }]);

    // Run a backup — lock should be released afterwards.
    let outcome = env.run_backup();
    assert!(outcome.success);

    // A second run should succeed (lock was released).
    fs::write(env.home.join(".config/fish/config.fish"), "v2").unwrap();
    let outcome2 = env.run_backup();
    assert!(outcome2.success);
}

// =============================================================================
// Criterion 11: The application behaves identically from Fish, Bash, or Zsh.
//
// Verified structurally: all external commands use argument arrays, never shell
// interpolation. The Git runner test verifies the same behavior for all shells.
// =============================================================================

#[test]
fn ac11_git_commands_handle_special_characters_without_shell() {
    // Verify that GitCommand handles special characters correctly — proving
    // no shell interpolation occurs (behavior is shell-independent).
    let tmp = tempfile::tempdir().unwrap();
    let runner = GitRunner::new(Duration::from_secs(5));

    // Init a repo.
    let init_cmd = GitCommand::new(tmp.path()).args(["init", "--initial-branch=main"]);
    runner.run(&init_cmd).unwrap();

    // Create a file with spaces and special characters.
    let special_file = tmp.path().join("file with spaces & 'quotes'.txt");
    fs::write(&special_file, "content").unwrap();

    let add_cmd = GitCommand::new(tmp.path()).args(["add", "file with spaces & 'quotes'.txt"]);
    runner.run(&add_cmd).unwrap();

    let commit_cmd = GitCommand::new(tmp.path()).args(["commit", "-m", "test special chars"]);
    runner.run(&commit_cmd).unwrap();

    // Verify the file was committed — proves shell interpolation wasn't used.
    let ls_cmd = GitCommand::new(tmp.path()).args(["ls-files"]);
    let output = runner.run(&ls_cmd).unwrap();
    assert!(
        output.stdout.contains("file with spaces & 'quotes'.txt"),
        "special characters in filename should work without shell: {}",
        output.stdout
    );
}

#[test]
fn ac11_systemd_service_invokes_binary_directly() {
    // Verify the service unit invokes the binary directly, not through a shell.
    let config = Config {
        version: 1,
        repository: "/home/user/dotfiles".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        network_timeout_seconds: 120,
        sources: vec![],
    };

    let params = systemd::params_from_config(&config).unwrap();
    let service = systemd::generate_service_unit(&params);

    // ExecStart should be a direct path, not "bash -c ..." or "sh -c ...".
    assert!(
        !service.contains("sh -c"),
        "service should not use shell: {service}"
    );
    assert!(
        !service.contains("bash -c"),
        "service should not use bash: {service}"
    );
    // Should have ExecStart with a path and "backup" argument.
    assert!(
        service.contains("ExecStart="),
        "service should have ExecStart"
    );
    assert!(
        service.contains(" backup"),
        "should invoke with backup argument: {service}"
    );
    // ExecStart must begin with an absolute path (direct invocation).
    let exec_line = service
        .lines()
        .find(|l| l.starts_with("ExecStart="))
        .expect("ExecStart line should exist");
    let exec_value = exec_line.strip_prefix("ExecStart=").unwrap();
    assert!(
        exec_value.starts_with('/'),
        "ExecStart should be an absolute path (direct invocation): {exec_value}"
    );
}
