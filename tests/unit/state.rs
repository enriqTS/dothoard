use super::*;
use chrono::TimeZone;

fn sample_time(hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 7, 21, hour, 0, 0).unwrap()
}

#[test]
fn new_state_has_no_history() {
    let state = AppState::new();

    assert_eq!(state.last_attempt, None);
    assert_eq!(state.last_success, None);
    assert_eq!(state.last_commit, None);
    assert_eq!(state.last_push, None);
    assert!(!state.pending_push);
    assert_eq!(state.latest_warning, None);
    assert_eq!(state.latest_error, None);
    assert!(state.history.is_empty());
}

#[test]
fn round_trips_through_json() {
    let state = AppState {
        last_attempt: Some(sample_time(10)),
        last_success: Some(sample_time(10)),
        last_commit: Some("abc123".to_string()),
        last_push: Some(sample_time(10)),
        pending_push: false,
        latest_warning: None,
        latest_error: None,
        history: vec![RunRecord {
            namespace: String::new(),
            started_at: sample_time(10),
            finished_at: sample_time(10),
            outcome: RunOutcome::Success,
            commit: Some("abc123".to_string()),
            message: None,
            log_file: None,
        }],
    };

    let json = serde_json::to_string_pretty(&state).unwrap();
    let restored: AppState = serde_json::from_str(&json).unwrap();

    assert_eq!(state, restored);
}

#[test]
fn save_and_load_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("state");

    let mut state = AppState::new();
    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(14),
        finished_at: sample_time(14),
        outcome: RunOutcome::Success,
        commit: Some("def456".to_string()),
        message: None,
        log_file: None,
    });

    state.save(&state_dir).unwrap();
    let loaded = AppState::load(&state_dir).unwrap();

    assert_eq!(loaded, state);
}

#[test]
fn load_returns_default_when_file_missing() {
    let tmp = tempfile::tempdir().unwrap();

    let state = AppState::load(tmp.path()).unwrap();

    assert_eq!(state, AppState::new());
}

#[test]
fn record_success_updates_state() {
    let mut state = AppState::new();

    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(10),
        finished_at: sample_time(10),
        outcome: RunOutcome::Success,
        commit: Some("aaa111".to_string()),
        message: None,
        log_file: None,
    });

    assert_eq!(state.last_attempt, Some(sample_time(10)));
    assert_eq!(state.last_success, Some(sample_time(10)));
    assert_eq!(state.last_commit, Some("aaa111".to_string()));
    assert_eq!(state.last_push, Some(sample_time(10)));
    assert!(!state.pending_push);
    assert_eq!(state.latest_error, None);
    assert_eq!(state.history.len(), 1);
}

#[test]
fn record_no_changes_clears_error() {
    let mut state = AppState::new();
    state.latest_error = Some("previous error".to_string());

    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(11),
        finished_at: sample_time(11),
        outcome: RunOutcome::NoChanges,
        commit: None,
        message: None,
        log_file: None,
    });

    assert_eq!(state.last_success, Some(sample_time(11)));
    assert_eq!(state.latest_error, None);
    // No commit or push change.
    assert_eq!(state.last_commit, None);
}

#[test]
fn record_committed_offline_sets_pending_push() {
    let mut state = AppState::new();

    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(12),
        finished_at: sample_time(12),
        outcome: RunOutcome::CommittedOffline,
        commit: Some("bbb222".to_string()),
        message: Some("push failed: network unreachable".to_string()),
        log_file: None,
    });

    assert!(state.pending_push);
    assert_eq!(state.last_commit, Some("bbb222".to_string()));
    assert_eq!(state.last_push, None);
    assert_eq!(
        state.latest_warning,
        Some("push failed: network unreachable".to_string())
    );
    assert_eq!(state.latest_error, None);
}

#[test]
fn record_failure_sets_error() {
    let mut state = AppState::new();

    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(13),
        finished_at: sample_time(13),
        outcome: RunOutcome::Failed,
        commit: None,
        message: Some("source .config/fish not found".to_string()),
        log_file: None,
    });

    assert_eq!(state.last_attempt, Some(sample_time(13)));
    assert_eq!(state.last_success, None);
    assert_eq!(
        state.latest_error,
        Some("source .config/fish not found".to_string())
    );
}

#[test]
fn history_is_bounded() {
    let mut state = AppState::new();

    for hour in 0..(MAX_HISTORY_ENTRIES + 10) {
        state.record_run(RunRecord {
            namespace: String::new(),
            started_at: sample_time(hour as u32 % 24),
            finished_at: sample_time(hour as u32 % 24),
            outcome: RunOutcome::NoChanges,
            commit: None,
            message: None,
            log_file: None,
        });
    }

    assert_eq!(state.history.len(), MAX_HISTORY_ENTRIES);
}

#[test]
fn history_newest_first() {
    let mut state = AppState::new();

    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(8),
        finished_at: sample_time(8),
        outcome: RunOutcome::NoChanges,
        commit: None,
        message: None,
        log_file: None,
    });
    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(9),
        finished_at: sample_time(9),
        outcome: RunOutcome::Success,
        commit: Some("ccc".to_string()),
        message: None,
        log_file: None,
    });

    assert_eq!(state.history[0].started_at, sample_time(9));
    assert_eq!(state.history[1].started_at, sample_time(8));
}

#[test]
fn success_after_failure_clears_pending_push() {
    let mut state = AppState::new();
    state.pending_push = true;

    state.record_run(RunRecord {
        namespace: String::new(),
        started_at: sample_time(15),
        finished_at: sample_time(15),
        outcome: RunOutcome::Success,
        commit: Some("ddd".to_string()),
        message: None,
        log_file: None,
    });

    assert!(!state.pending_push);
}

#[test]
fn save_creates_state_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("nested").join("state");

    let state = AppState::new();
    state.save(&state_dir).unwrap();

    assert!(state_dir.exists());
    assert!(state_dir.join(STATE_FILE_NAME).exists());
}

#[test]
fn state_file_path_is_deterministic() {
    let dir = Path::new("/home/user/.local/state/dothoard");

    assert_eq!(
        AppState::path_in(dir),
        PathBuf::from("/home/user/.local/state/dothoard/status.json")
    );
}

#[test]
fn log_file_round_trips_through_json() {
    let state = AppState {
        last_attempt: Some(sample_time(10)),
        last_success: Some(sample_time(10)),
        last_commit: Some("abc123".to_string()),
        last_push: Some(sample_time(10)),
        pending_push: false,
        latest_warning: None,
        latest_error: None,
        history: vec![RunRecord {
            namespace: String::new(),
            started_at: sample_time(10),
            finished_at: sample_time(10),
            outcome: RunOutcome::Success,
            commit: Some("abc123".to_string()),
            message: None,
            log_file: Some("run-2026-07-21T10-00-00-000.log".to_string()),
        }],
    };

    let json = serde_json::to_string_pretty(&state).unwrap();
    let restored: AppState = serde_json::from_str(&json).unwrap();

    assert_eq!(state, restored);
    assert_eq!(
        restored.history[0].log_file.as_deref(),
        Some("run-2026-07-21T10-00-00-000.log")
    );
}

#[test]
fn log_file_none_is_omitted_from_json() {
    let record = RunRecord {
        namespace: String::new(),
        started_at: sample_time(10),
        finished_at: sample_time(10),
        outcome: RunOutcome::Success,
        commit: None,
        message: None,
        log_file: None,
    };

    let json = serde_json::to_string(&record).unwrap();
    // The log_file field should not appear when None.
    assert!(!json.contains("log_file"));
}

#[test]
fn deserializes_legacy_record_without_log_file() {
    // Simulates loading a state file from before the log_file field existed.
    let json = r#"{
            "started_at": "2026-07-21T10:00:00Z",
            "finished_at": "2026-07-21T10:00:03Z",
            "outcome": "Success",
            "commit": "abc123",
            "message": null
        }"#;

    let record: RunRecord = serde_json::from_str(json).unwrap();
    assert_eq!(record.log_file, None);
    assert_eq!(record.namespace, "");
    assert_eq!(record.commit, Some("abc123".to_string()));
}
