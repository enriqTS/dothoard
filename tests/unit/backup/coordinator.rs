use super::*;

#[test]
fn format_commit_message_contains_timestamp() {
    let ts = Utc::now();
    let msg = format_commit_message(&ts);

    assert!(msg.starts_with("backup("));
    assert!(msg.contains("): "));
    // Should contain a date-like string.
    assert!(msg.contains('-'));
    assert!(msg.contains(':'));
}

#[test]
fn backup_outcome_failed_sets_fields_correctly() {
    let outcome = BackupOutcome::failed("some error".to_string(), vec!["warn".to_string()]);

    assert!(!outcome.success);
    assert_eq!(outcome.error, Some("some error".to_string()));
    assert_eq!(outcome.warnings, vec!["warn"]);
    assert_eq!(outcome.commit, None);
    assert!(!outcome.pushed);
    assert!(!outcome.pending_push);
}

#[test]
fn backup_outcome_failed_with_commit_preserves_pending() {
    let outcome = BackupOutcome::failed_with_commit(
        "conflict".to_string(),
        Some("abc123".to_string()),
        vec![],
        5,
        2,
    );

    assert!(!outcome.success);
    assert_eq!(outcome.commit, Some("abc123".to_string()));
    assert!(outcome.pending_push);
    assert_eq!(outcome.copies, 5);
    assert_eq!(outcome.deletions, 2);
}

#[test]
fn persist_outcome_records_success() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("state");

    // Create the required directories.
    std::fs::create_dir_all(tmp.path().join("home")).unwrap();
    std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();

    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(tmp.path().join("home")),
        config_dir: Some(tmp.path().join("config")),
        state_dir: Some(state_dir.clone()),
        runtime_dir: Some(tmp.path().join("runtime")),
        use_environment: false,
    })
    .unwrap();

    let outcome = BackupOutcome {
        namespace: "desktop".to_string(),
        success: true,
        commit: Some("deadbeef".to_string()),
        pushed: true,
        pending_push: false,
        warnings: vec![],
        error: None,
        copies: 3,
        deletions: 1,
    };

    let started_at = Utc::now();
    persist_outcome(&paths, &outcome, started_at, Some("test.log".to_string())).unwrap();

    let loaded = AppState::load(&state_dir).unwrap();
    assert_eq!(loaded.last_commit, Some("deadbeef".to_string()));
    assert!(!loaded.pending_push);
    assert_eq!(loaded.history.len(), 1);
    assert_eq!(loaded.history[0].outcome, RunOutcome::Success);
}

#[test]
fn persist_outcome_records_offline_commit() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(tmp.path().join("home")).unwrap();
    std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();

    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(tmp.path().join("home")),
        config_dir: Some(tmp.path().join("config")),
        state_dir: Some(state_dir.clone()),
        runtime_dir: Some(tmp.path().join("runtime")),
        use_environment: false,
    })
    .unwrap();

    let outcome = BackupOutcome {
        namespace: "desktop".to_string(),
        success: true,
        commit: Some("abc123".to_string()),
        pushed: false,
        pending_push: true,
        warnings: vec!["push deferred: network unreachable".to_string()],
        error: None,
        copies: 1,
        deletions: 0,
    };

    let started_at = Utc::now();
    persist_outcome(&paths, &outcome, started_at, Some("test.log".to_string())).unwrap();

    let loaded = AppState::load(&state_dir).unwrap();
    assert!(loaded.pending_push);
    assert_eq!(loaded.last_commit, Some("abc123".to_string()));
    assert_eq!(loaded.history[0].outcome, RunOutcome::CommittedOffline);
}

#[test]
fn persist_outcome_records_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(tmp.path().join("home")).unwrap();
    std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();

    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(tmp.path().join("home")),
        config_dir: Some(tmp.path().join("config")),
        state_dir: Some(state_dir.clone()),
        runtime_dir: Some(tmp.path().join("runtime")),
        use_environment: false,
    })
    .unwrap();

    let outcome = BackupOutcome::failed("source not found".to_string(), vec![]);

    let started_at = Utc::now();
    persist_outcome(&paths, &outcome, started_at, Some("test.log".to_string())).unwrap();

    let loaded = AppState::load(&state_dir).unwrap();
    assert_eq!(loaded.latest_error, Some("source not found".to_string()));
    assert_eq!(loaded.history[0].outcome, RunOutcome::Failed);
}
