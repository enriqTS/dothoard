use super::*;
use chrono::{TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::io::Write;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn sample_record(outcome: RunOutcome) -> RunRecord {
    RunRecord {
        namespace: String::new(),
        started_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 0).unwrap(),
        finished_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 3).unwrap(),
        outcome,
        commit: Some("abc123".to_string()),
        message: None,
        log_file: None,
    }
}

#[test]
fn new_screen_starts_at_zero() {
    let screen = HistoryScreen::new();
    assert_eq!(screen.selected, 0);
    assert!(matches!(screen.mode, Mode::History));
}

#[test]
fn navigate_up_down() {
    let mut screen = HistoryScreen::new();
    screen.handle_key(key(KeyCode::Down), 5);
    assert_eq!(screen.selected, 1);
    screen.handle_key(key(KeyCode::Down), 5);
    assert_eq!(screen.selected, 2);
    screen.handle_key(key(KeyCode::Up), 5);
    assert_eq!(screen.selected, 1);
}

#[test]
fn does_not_go_past_bounds() {
    let mut screen = HistoryScreen::new();
    screen.handle_key(key(KeyCode::Up), 5);
    assert_eq!(screen.selected, 0);

    screen.selected = 4;
    screen.handle_key(key(KeyCode::Down), 5);
    assert_eq!(screen.selected, 4);
}

#[test]
fn home_end_navigation() {
    let mut screen = HistoryScreen::new();
    screen.selected = 3;
    screen.handle_key(key(KeyCode::Home), 10);
    assert_eq!(screen.selected, 0);
    screen.handle_key(key(KeyCode::End), 10);
    assert_eq!(screen.selected, 9);
}

#[test]
fn one_row_history_has_stable_selection_and_viewport() {
    let mut screen = HistoryScreen::new();
    screen.set_list_viewport_height(4, 1);
    screen.handle_key(key(KeyCode::End), 1);
    screen.handle_key(key(KeyCode::Down), 1);

    assert_eq!(screen.selected, 0);
    assert_eq!(screen.list_viewport.visible_range(1), 0..1);
}

#[test]
fn long_list_navigation_keeps_selection_visible() {
    let mut screen = HistoryScreen::new();
    screen.set_list_viewport_height(3, 10);

    for _ in 0..5 {
        screen.handle_key(key(KeyCode::Down), 10);
    }

    assert_eq!(screen.selected, 5);
    assert_eq!(screen.list_viewport.visible_range(10), 3..6);
}

#[test]
fn history_page_navigation_uses_rendered_viewport_height() {
    let mut screen = HistoryScreen::new();
    screen.set_list_viewport_height(4, 12);

    screen.handle_key(key(KeyCode::PageDown), 12);
    assert_eq!(screen.selected, 4);
    assert_eq!(screen.list_viewport.visible_range(12), 1..5);

    screen.handle_key(key(KeyCode::End), 12);
    assert_eq!(screen.selected, 11);
    assert_eq!(screen.list_viewport.visible_range(12), 8..12);

    screen.handle_key(key(KeyCode::PageUp), 12);
    assert_eq!(screen.selected, 7);
    assert_eq!(screen.list_viewport.visible_range(12), 7..11);
}

#[test]
fn history_viewport_clamps_after_data_shrinks() {
    let mut screen = HistoryScreen::new();
    screen.set_list_viewport_height(3, 10);
    screen.selected = 9;
    screen.clamp_history(10);

    screen.clamp_history(2);

    assert_eq!(screen.selected, 1);
    assert_eq!(screen.list_viewport.visible_range(2), 0..2);
}

#[test]
fn list_viewport_survives_tab_focus_escape() {
    let mut screen = HistoryScreen::new();
    screen.set_list_viewport_height(3, 10);
    screen.selected = 7;
    screen.clamp_history(10);
    let range = screen.list_viewport.visible_range(10);

    let action = screen.handle_key(key(KeyCode::Tab), 10);

    assert_eq!(action, Action::NotConsumed);
    assert_eq!(screen.list_viewport.visible_range(10), range);
}

#[test]
fn enter_returns_view_logs_action_for_selected_run() {
    let mut screen = HistoryScreen::new();
    let result = screen.handle_key(key(KeyCode::Enter), 5);
    assert_eq!(result, Action::ViewLogs);
}

#[test]
fn enter_is_disabled_when_history_is_empty() {
    let mut screen = HistoryScreen::new();
    let result = screen.handle_key(key(KeyCode::Enter), 0);
    assert_eq!(result, Action::Consumed);
    assert_eq!(screen.mode, Mode::History);
}

#[test]
fn log_view_mode_esc_exits() {
    let mut screen = HistoryScreen::new();
    screen.mode = Mode::LogView;
    screen.log_lines = vec!["line1".to_string()];
    screen.log_viewport.scroll_down(5, 10);

    let result = screen.handle_key(key(KeyCode::Esc), 5);

    assert_eq!(result, Action::Consumed);
    assert!(matches!(screen.mode, Mode::History));
    assert!(screen.log_lines.is_empty());
    assert_eq!(screen.log_viewport.offset(), 0);
}

#[test]
fn log_view_scroll_up_down() {
    let mut screen = HistoryScreen::new();
    screen.mode = Mode::LogView;
    screen.log_lines = vec!["line1".to_string(), "line2".to_string()];
    screen.log_viewport.scroll_down(1, screen.log_lines.len());

    // Scroll up
    screen.handle_key(key(KeyCode::Up), 5);
    assert_eq!(screen.log_viewport.offset(), 0);

    // Scroll up at 0 stays at 0
    screen.handle_key(key(KeyCode::Up), 5);
    assert_eq!(screen.log_viewport.offset(), 0);

    // Scroll down
    screen.handle_key(key(KeyCode::Down), 5);
    assert_eq!(screen.log_viewport.offset(), 1);
}

#[test]
fn log_view_page_up_down() {
    let mut screen = HistoryScreen::new();
    screen.mode = Mode::LogView;
    screen.log_lines = (0..50).map(|i| format!("line {i}")).collect();
    screen.set_log_viewport_height(10);
    screen.log_viewport.scroll_down(25, screen.log_lines.len());

    screen.handle_key(key(KeyCode::PageUp), 5);
    assert_eq!(screen.log_viewport.offset(), 15);

    screen.handle_key(key(KeyCode::PageDown), 5);
    assert_eq!(screen.log_viewport.offset(), 25);
}

#[test]
fn log_view_home_resets_scroll() {
    let mut screen = HistoryScreen::new();
    screen.mode = Mode::LogView;
    screen.log_lines = (0..60).map(|i| format!("line {i}")).collect();
    screen.log_viewport.scroll_down(50, screen.log_lines.len());

    screen.handle_key(key(KeyCode::Home), 5);
    assert_eq!(screen.log_viewport.offset(), 0);
}

#[test]
fn enter_log_view_clears_previous_lines() {
    let mut screen = HistoryScreen::new();
    screen.log_lines = vec!["old".to_string()];

    let record = sample_record(RunOutcome::Success);
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path();

    screen.enter_log_view(&record, state_dir);

    assert!(matches!(screen.mode, Mode::LogView));
    assert!(screen.log_lines.is_empty()); // No log file exists, so empty
    assert_eq!(screen.log_viewport.offset(), 0);
}

#[test]
fn filter_logs_by_timestamp_extracts_matching_lines() {
    let tmp = tempfile::tempdir().unwrap();
    let log_path = tmp.path().join("dothoard.log");
    let mut file = std::fs::File::create(&log_path).unwrap();

    // Write log lines with timestamps
    writeln!(file, "2026-07-21T14:30:01.123456789Z INFO before").unwrap();
    writeln!(file, "2026-07-21T14:30:02.123456789Z INFO during 1").unwrap();
    writeln!(file, "2026-07-21T14:30:02.500000000Z INFO during 2").unwrap();
    writeln!(file, "2026-07-21T14:30:03.123456789Z INFO at end").unwrap();
    writeln!(file, "2026-07-21T14:30:04.000000000Z INFO after").unwrap();
    writeln!(file, "not a timestamp line").unwrap();

    let start = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 2).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 3).unwrap();

    let lines = HistoryScreen::filter_logs_by_timestamp(&log_path, start, end);

    // Lines with timestamps >= start (14:30:02) and <= end (14:30:03)
    // - 14:30:01 - before start (excluded)
    // - 14:30:02.123 - during 1 (included)
    // - 14:30:02.500 - during 2 (included)
    // - 14:30:03.123 - after end (excluded, 03.123 > 03.000)
    // - 14:30:04 - after end (excluded)
    // - not a timestamp (excluded)
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("during 1"));
    assert!(lines[1].contains("during 2"));
}

#[test]
fn filter_logs_handles_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let log_path = tmp.path().join("nonexistent.log");

    let start = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 7, 21, 14, 30, 10).unwrap();

    let lines = HistoryScreen::filter_logs_by_timestamp(&log_path, start, end);

    assert!(lines.is_empty());
}

#[test]
fn format_entry_includes_namespace_identity() {
    let mut record = sample_record(RunOutcome::Success);
    record.namespace = "notebook".to_string();

    let display = HistoryScreen::format_entry(&record);

    assert_eq!(display.namespace.as_deref(), Some("notebook"));
}

#[test]
fn log_view_retains_run_namespace_context() {
    let mut record = sample_record(RunOutcome::Success);
    record.namespace = "desktop".to_string();
    let tmp = tempfile::tempdir().unwrap();
    let mut screen = HistoryScreen::new();

    screen.enter_log_view(&record, tmp.path());

    assert_eq!(screen.log_namespace.as_deref(), Some("desktop"));
    screen.exit_log_view();
    assert!(screen.log_namespace.is_none());
}

#[test]
fn format_success_entry() {
    let record = sample_record(RunOutcome::Success);
    let display = HistoryScreen::format_entry(&record);
    assert_eq!(display.outcome, "Success");
    assert_eq!(display.duration, "3s");
    assert!(!display.is_error);
    assert!(!display.is_warning);
    assert!(display.time.contains("2026-07-21"));
}

#[test]
fn format_failed_entry() {
    let mut record = sample_record(RunOutcome::Failed);
    record.message = Some("network timeout".to_string());
    record.commit = None;
    let display = HistoryScreen::format_entry(&record);
    assert_eq!(display.outcome, "Failed");
    assert!(display.is_error);
    assert_eq!(display.message.as_deref(), Some("network timeout"));
}

#[test]
fn format_offline_entry() {
    let record = sample_record(RunOutcome::CommittedOffline);
    let display = HistoryScreen::format_entry(&record);
    assert_eq!(display.outcome, "Committed (offline)");
    assert!(display.is_warning);
}

#[test]
fn format_no_changes_entry() {
    let mut record = sample_record(RunOutcome::NoChanges);
    record.commit = None;
    let display = HistoryScreen::format_entry(&record);
    assert_eq!(display.outcome, "No changes");
}

#[test]
fn format_sub_second_duration() {
    let mut record = sample_record(RunOutcome::Success);
    record.finished_at = record.started_at + chrono::Duration::milliseconds(450);
    let display = HistoryScreen::format_entry(&record);
    assert_eq!(display.duration, "450ms");
}

#[test]
fn enter_log_view_reads_per_run_log_file() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path();

    // Create the logs directory and a per-run log file.
    let logs_dir = state_dir.join("logs");
    std::fs::create_dir_all(&logs_dir).unwrap();
    let log_filename = "run-2026-07-21T14-30-00-000.log";
    let log_path = logs_dir.join(log_filename);
    std::fs::write(&log_path, "INFO backup started\nINFO mirror completed\n").unwrap();

    // Create a record with the log_file field set.
    let mut record = sample_record(RunOutcome::Success);
    record.log_file = Some(log_filename.to_string());

    let mut screen = HistoryScreen::new();
    screen.enter_log_view(&record, state_dir);

    assert!(matches!(screen.mode, Mode::LogView));
    assert_eq!(screen.log_lines.len(), 2);
    assert!(screen.log_lines[0].contains("backup started"));
    assert!(screen.log_lines[1].contains("mirror completed"));
}

#[test]
fn enter_log_view_falls_back_to_timestamp_filter_when_no_log_file() {
    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path();

    // Create the session log in the state directory.
    let session_log = state_dir.join("dothoard.log");
    let mut f = std::fs::File::create(&session_log).unwrap();
    writeln!(f, "2026-07-21T14:30:01.000000000Z INFO during run").unwrap();
    writeln!(f, "2026-07-21T14:31:00.000000000Z INFO after run").unwrap();

    // Record has no log_file — should use legacy timestamp filtering.
    let record = sample_record(RunOutcome::Success);
    assert!(record.log_file.is_none());

    let mut screen = HistoryScreen::new();
    screen.enter_log_view(&record, state_dir);

    assert!(matches!(screen.mode, Mode::LogView));
    // The first line (14:30:01) is within [14:30:00, 14:30:03].
    assert_eq!(screen.log_lines.len(), 1);
    assert!(screen.log_lines[0].contains("during run"));
}

#[test]
fn read_log_file_returns_empty_for_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("nonexistent.log");

    let lines = HistoryScreen::read_log_file(&missing);
    assert!(lines.is_empty());
}
