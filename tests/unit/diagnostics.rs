use super::*;

#[test]
fn redacts_url_userinfo() {
    let value = "https://user:secret@example.com/repository.git";

    assert_eq!(redact_remote_url(value), "https://[redacted]");
}

#[test]
fn redacts_url_query_and_fragment() {
    assert_eq!(
        redact_remote_url("https://example.com/repository.git?token=secret"),
        "https://[redacted]"
    );
    assert_eq!(
        redact_remote_url("https://example.com/repository.git#secret"),
        "https://[redacted]"
    );
}

#[test]
fn preserves_urls_without_credential_components() {
    let value = "https://example.com/repository.git";

    assert_eq!(redact_remote_url(value), value);
}

#[test]
fn preserves_scp_style_ssh_remote() {
    let value = "git@example.com:repository.git";

    assert_eq!(redact_remote_url(value), value);
}

#[test]
fn redacts_every_sensitive_url_in_diagnostic_text() {
    let value = concat!(
        "fetch from https://example.com/public.git failed; ",
        "push to https://user:secret@example.com/private.git failed; ",
        "see https://example.com/details?token=secret"
    );

    assert_eq!(
        redact_sensitive_text(value),
        concat!(
            "fetch from https://example.com/public.git failed; ",
            "push to https://[redacted] failed; ",
            "see https://[redacted]"
        )
    );
}

#[test]
fn redacts_adjacent_sensitive_urls() {
    let value = concat!(
        "https://example.com/public.git,",
        "https://user:secret@example.com/private.git"
    );

    assert_eq!(redact_sensitive_text(value), "https://[redacted]");
}

#[test]
fn redacts_sensitive_query_after_url_punctuation() {
    let value = "request to https://example.com/a,b?token=secret failed";

    assert_eq!(
        redact_sensitive_text(value),
        "request to https://[redacted] failed"
    );
}

#[test]
fn preserves_non_sensitive_diagnostic_text_as_borrowed() {
    let value = "fetch from https://example.com/public.git failed";

    assert!(matches!(redact_sensitive_text(value), Cow::Borrowed(_)));
}

/// Test that init_for_tui creates a log file and writes to it.
///
/// Note: This test initializes tracing, which can only be done once per process.
/// Run this test in isolation if needed, or run with `--test-threads=1`.
#[test]
fn init_for_tui_creates_log_file() {
    let tmp = tempfile::tempdir().unwrap();
    let log_path = tmp.path().join("dothoard.log");

    // Initialize tracing with file appender
    let _guard = init_for_tui(&log_path).expect("should initialize successfully");

    // Log a test message
    tracing::info!("test log message from init_for_tui");

    // Drop the guard to ensure all logs are flushed
    drop(_guard);

    // Read the log file and verify it contains our message
    let log_contents = std::fs::read_to_string(&log_path).expect("should read log file");
    assert!(
        log_contents.contains("test log message from init_for_tui"),
        "log file should contain the test message. Contents: {}",
        log_contents
    );
}

#[test]
fn run_log_filename_is_sortable_and_unique() {
    use chrono::TimeZone;

    let ts1 = Utc.with_ymd_and_hms(2026, 7, 28, 14, 30, 0).unwrap();
    let ts2 = Utc.with_ymd_and_hms(2026, 7, 28, 14, 30, 1).unwrap();

    let f1 = run_log_filename(&ts1);
    let f2 = run_log_filename(&ts2);

    assert!(f1.starts_with("run-2026-07-28T14-30-00-"));
    assert!(f1.ends_with(".log"));
    assert!(f1 < f2, "filenames should be lexicographically sortable");
}

#[test]
fn run_log_path_uses_logs_subdirectory() {
    use chrono::TimeZone;

    let state_dir = Path::new("/home/user/.local/state/dothoard");
    let ts = Utc.with_ymd_and_hms(2026, 7, 28, 10, 0, 0).unwrap();

    let path = run_log_path(state_dir, &ts);

    assert!(path.starts_with("/home/user/.local/state/dothoard/logs/"));
    assert!(path.to_str().unwrap().ends_with(".log"));
}

#[test]
fn log_dir_appends_logs_to_state_dir() {
    let state_dir = Path::new("/tmp/state");
    assert_eq!(log_dir(state_dir), Path::new("/tmp/state/logs"));
}

#[test]
fn extract_run_log_creates_per_run_file() {
    use chrono::TimeZone;
    use std::io::Write;

    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    // Create a session log with timestamped lines.
    let session_log = state_dir.join("dothoard.log");
    let mut f = std::fs::File::create(&session_log).unwrap();
    writeln!(
        f,
        "2026-07-28T10:00:00.500000000Z INFO backup lock acquired"
    )
    .unwrap();
    writeln!(
        f,
        "2026-07-28T10:00:01.000000000Z INFO configuration loaded"
    )
    .unwrap();
    writeln!(f, "2026-07-28T10:00:02.000000000Z INFO mirror completed").unwrap();
    writeln!(
        f,
        "2026-07-28T10:00:05.000000000Z INFO unrelated later event"
    )
    .unwrap();

    let start = Utc.with_ymd_and_hms(2026, 7, 28, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 7, 28, 10, 0, 3).unwrap();

    let result = extract_run_log(&session_log, &state_dir, &start, &end);

    assert!(result.is_some());
    let filename = result.unwrap();
    assert!(filename.starts_with("run-"));
    assert!(filename.ends_with(".log"));

    // Verify the per-run file was created in the logs/ subdirectory.
    let run_log_path = log_dir(&state_dir).join(&filename);
    assert!(run_log_path.exists());

    let content = std::fs::read_to_string(&run_log_path).unwrap();
    assert!(content.contains("backup lock acquired"));
    assert!(content.contains("configuration loaded"));
    assert!(content.contains("mirror completed"));
    // Line after the end timestamp should NOT be included.
    assert!(!content.contains("unrelated later event"));
}

#[test]
fn extract_run_log_handles_missing_session_log() {
    use chrono::TimeZone;

    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    let session_log = state_dir.join("nonexistent.log");
    let start = Utc.with_ymd_and_hms(2026, 7, 28, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 7, 28, 10, 0, 3).unwrap();

    let result = extract_run_log(&session_log, &state_dir, &start, &end);

    assert!(result.is_none());
}

#[test]
fn extract_run_log_returns_filename_when_no_matching_lines() {
    use chrono::TimeZone;
    use std::io::Write;

    let tmp = tempfile::tempdir().unwrap();
    let state_dir = tmp.path().join("state");
    std::fs::create_dir_all(&state_dir).unwrap();

    let session_log = state_dir.join("dothoard.log");
    let mut f = std::fs::File::create(&session_log).unwrap();
    writeln!(f, "2026-07-28T09:00:00.000000000Z INFO earlier event").unwrap();

    let start = Utc.with_ymd_and_hms(2026, 7, 28, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2026, 7, 28, 10, 0, 3).unwrap();

    let result = extract_run_log(&session_log, &state_dir, &start, &end);

    // Should return the filename even when no lines match.
    assert!(result.is_some());
}
