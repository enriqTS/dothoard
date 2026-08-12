use super::*;

#[test]
fn load_state_preserves_usable_data_and_rejects_stale_results() {
    let mut state = LoadState::Loaded("old".to_string());
    let first = RequestId(1);
    state.begin(first, true);
    assert_eq!(state.data().map(String::as_str), Some("old"));

    state.invalidate();
    assert!(matches!(state, LoadState::Stale { .. }));
    assert!(!state.finish(first, Ok("obsolete".to_string())));
    assert_eq!(state.data().map(String::as_str), Some("old"));

    let second = RequestId(2);
    state.begin(second, true);
    assert!(state.finish(second, Err("failed".to_string())));
    assert_eq!(state.error(), Some("failed"));
    assert_eq!(state.data().map(String::as_str), Some("old"));
}

#[test]
fn keyed_loads_suppress_duplicates_but_allow_unrelated_work() {
    let mut manager = TaskManager::new_controlled();
    assert!(manager.begin_load(LoadTaskKind::BackupPreview).is_some());
    assert!(manager.begin_load(LoadTaskKind::BackupPreview).is_none());
    assert!(
        manager
            .begin_load(LoadTaskKind::AutomationInspection)
            .is_some()
    );
    assert!(manager.is_load_active(LoadTaskKind::BackupPreview));
    assert!(manager.is_load_active(LoadTaskKind::AutomationInspection));
    assert!(!manager.is_busy());
}

#[test]
fn invalidated_old_result_does_not_clear_replacement_request() {
    let mut manager = TaskManager::new_controlled();
    let old = manager
        .begin_load(LoadTaskKind::AutomationInspection)
        .unwrap();
    manager.invalidate_load(LoadTaskKind::AutomationInspection);
    let replacement = manager
        .begin_load(LoadTaskKind::AutomationInspection)
        .unwrap();

    manager
        .sender
        .send(TaskResult::AutomationInspection {
            request_id: old,
            result: Ok("old".to_string()),
        })
        .unwrap();
    let _ = manager.poll();
    assert!(manager.is_load_active(LoadTaskKind::AutomationInspection));

    manager
        .sender
        .send(TaskResult::AutomationInspection {
            request_id: replacement,
            result: Ok("new".to_string()),
        })
        .unwrap();
    let _ = manager.poll();
    assert!(!manager.is_load_active(LoadTaskKind::AutomationInspection));
}

#[test]
fn new_task_manager_is_not_busy() {
    let tm = TaskManager::new();
    assert!(!tm.is_busy());
    assert_eq!(tm.active_task(), None);
}

#[test]
fn poll_returns_none_when_empty() {
    let mut tm = TaskManager::new();
    assert!(tm.poll().is_none());
}

#[test]
fn cannot_spawn_when_busy() {
    let mut tm = TaskManager::new();
    // Manually set active to simulate a running task.
    tm.active = Some(TaskKind::Backup);

    // Cannot spawn another task while one is active.
    let paths = unsafe_test_paths();
    assert!(!tm.spawn_backup(paths.clone()));
    assert!(!tm.spawn_check(paths));
}

#[test]
fn direct_channel_send_receive() {
    // Test the channel mechanism directly without spawning real tasks.
    let mut tm = TaskManager::new();
    tm.active = Some(TaskKind::Check);

    // Simulate a task completing by sending directly on the channel.
    let sender = tm.sender.clone();
    sender
        .send(TaskResult::Check(CheckResult {
            healthy: true,
            results: vec![CheckItem {
                label: "test".to_string(),
                status: CheckItemStatus::Ok,
                detail: Some("all good".to_string()),
            }],
        }))
        .unwrap();

    let result = tm.poll();
    assert!(result.is_some());
    assert!(!tm.is_busy());

    match result.unwrap() {
        TaskResult::Check(cr) => {
            assert!(cr.healthy);
            assert_eq!(cr.results.len(), 1);
        }
        _ => panic!("expected Check result"),
    }
}

#[test]
fn poll_clears_active_state() {
    let mut tm = TaskManager::new();
    tm.active = Some(TaskKind::Backup);

    let sender = tm.sender.clone();
    sender
        .send(TaskResult::Backup(BackupResult {
            success: true,
            commit: Some("abc123".to_string()),
            pushed: true,
            copies: 5,
            deletions: 1,
            warnings: Vec::new(),
            error: None,
        }))
        .unwrap();

    assert!(tm.is_busy());
    let _ = tm.poll();
    assert!(!tm.is_busy());
    assert_eq!(tm.active_task(), None);
}

/// Create AppPaths suitable for tests that won't actually run tasks.
/// This is only used to test spawn rejection logic — the paths won't be
/// accessed because we verify spawn is rejected when busy.
fn unsafe_test_paths() -> crate::paths::AppPaths {
    // Create a temporary directory structure for path resolution.
    let tmp = std::env::temp_dir().join("dothoard-task-test");
    let _ = std::fs::create_dir_all(&tmp);
    let config_dir = tmp.join("config");
    let _ = std::fs::create_dir_all(&config_dir);
    let state_dir = tmp.join("state");
    let _ = std::fs::create_dir_all(&state_dir);
    let runtime_dir = tmp.join("runtime");
    let _ = std::fs::create_dir_all(&runtime_dir);

    let inputs = crate::paths::PathInputs {
        home: Some(tmp.clone()),
        config_dir: Some(config_dir),
        state_dir: Some(state_dir),
        runtime_dir: Some(runtime_dir),
        use_environment: false,
    };

    crate::paths::AppPaths::resolve(inputs).unwrap_or_else(|_| {
        // Fallback — construct manually if resolution fails.
        // This shouldn't happen with the dirs we created above.
        panic!("failed to create test AppPaths");
    })
}
