//! Process-wide structured diagnostics setup.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::EnvFilter;

use crate::app;

pub fn init() -> anyhow::Result<()> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!(error))
}

/// Initialize tracing for TUI mode, writing to a log file instead of stderr.
///
/// Returns a `WorkerGuard` that must be held for the lifetime of the TUI
/// to ensure logs are flushed. When the guard is dropped, the background
/// thread that writes to the log file will be shut down.
pub fn init_for_tui(log_path: &Path) -> anyhow::Result<WorkerGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Ensure the parent directory exists
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Create a non-blocking file appender
    let file_appender = tracing_appender::rolling::never(
        log_path.parent().unwrap_or(Path::new("")),
        log_path
            .file_name()
            .unwrap_or(std::ffi::OsStr::new("dothoard.log")),
    );
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(non_blocking)
        .compact()
        .try_init()
        .map_err(|error| anyhow::anyhow!(error))?;

    Ok(guard)
}

/// Return the logs directory path inside the state directory.
///
/// The log directory is `<state_dir>/logs/`.
pub fn log_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(app::LOG_DIR_NAME)
}

/// Generate a per-run log filename from a timestamp.
///
/// Format: `run-<YYYY-MM-DDTHH-MM-SS>-<subsec>.log`
/// This produces a sortable, unique filename for each run.
pub fn run_log_filename(timestamp: &DateTime<Utc>) -> String {
    let formatted = timestamp.format("%Y-%m-%dT%H-%M-%S");
    let subsec = timestamp.timestamp_subsec_millis();
    format!("run-{formatted}-{subsec:03}.log")
}

/// Generate the full path for a per-run log file.
pub fn run_log_path(state_dir: &Path, timestamp: &DateTime<Utc>) -> PathBuf {
    log_dir(state_dir).join(run_log_filename(timestamp))
}

/// Initialize tracing for a single backup run (used by CLI mode).
///
/// Returns the `WorkerGuard` that must be held for the run's lifetime and
/// the log filename (relative to the logs directory) for storage in the
/// run record.
pub fn init_for_run(
    state_dir: &Path,
    started_at: &DateTime<Utc>,
) -> anyhow::Result<(WorkerGuard, String)> {
    let filename = run_log_filename(started_at);
    let log_path = log_dir(state_dir).join(&filename);
    let guard = init_for_tui(&log_path)?;
    Ok((guard, filename))
}

/// Extract log lines for a run from the session log file and save them
/// to a dedicated per-run log file.
///
/// This is used in TUI mode where the global tracing subscriber writes to
/// a session-level log file and we need to extract a run's portion into
/// its own file after the run completes.
///
/// Returns the per-run log filename on success.
pub fn extract_run_log(
    session_log_path: &Path,
    state_dir: &Path,
    started_at: &DateTime<Utc>,
    finished_at: &DateTime<Utc>,
) -> Option<String> {
    use std::io::{BufRead, BufReader, Write};

    let filename = run_log_filename(started_at);
    let run_log = log_dir(state_dir).join(&filename);

    // Ensure the log directory exists.
    if let Err(e) = std::fs::create_dir_all(run_log.parent()?) {
        tracing::warn!(error = %e, "failed to create log directory");
        return None;
    }

    // Read the session log and filter by timestamp range.
    let file = std::fs::File::open(session_log_path).ok()?;
    let reader = BufReader::new(file);
    let mut output = Vec::new();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };

        // Parse the timestamp from the beginning of the log line.
        if let Some(ts_end) = line.find(' ') {
            let ts_str = &line[..ts_end];
            if let Ok(ts) = ts_str.parse::<DateTime<Utc>>() {
                if ts >= *started_at && ts <= *finished_at {
                    output.push(line);
                }
            }
        }
    }

    // Write the extracted lines to the per-run log file.
    if output.is_empty() {
        return Some(filename);
    }

    let mut file = match std::fs::File::create(&run_log) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, path = %run_log.display(), "failed to create run log");
            return None;
        }
    };

    for line in &output {
        let _ = writeln!(file, "{line}");
    }

    Some(filename)
}

pub fn redact_remote_url(value: &str) -> Cow<'_, str> {
    let Some((scheme, remainder)) = value.split_once("://") else {
        return Cow::Borrowed(value);
    };

    let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
    let suffix = &remainder[authority_end..];

    if remainder.contains('@') || suffix.contains(['?', '#']) {
        return Cow::Owned(format!("{scheme}://[redacted]"));
    }

    Cow::Borrowed(value)
}

pub fn redact_sensitive_text(value: &str) -> Cow<'_, str> {
    let mut output = String::with_capacity(value.len());
    let mut output_cursor = 0;
    let mut search_cursor = 0;
    let mut changed = false;

    while let Some(relative_marker) = value[search_cursor..].find("://") {
        let marker = search_cursor + relative_marker;
        let mut scheme_start = marker;

        while scheme_start > 0 && is_scheme_character(value.as_bytes()[scheme_start - 1]) {
            scheme_start -= 1;
        }

        if scheme_start == marker {
            search_cursor = marker + 3;
            continue;
        }

        let url_end = value[marker + 3..]
            .char_indices()
            .find_map(|(index, character)| is_url_boundary(character).then_some(marker + 3 + index))
            .unwrap_or(value.len());
        let candidate = &value[scheme_start..url_end];
        let redacted_candidate = redact_remote_url(candidate);

        if matches!(redacted_candidate, Cow::Owned(_)) {
            output.push_str(&value[output_cursor..scheme_start]);
            output.push_str(&redacted_candidate);
            output_cursor = url_end;
            changed = true;
        }

        search_cursor = url_end;
    }

    if changed {
        output.push_str(&value[output_cursor..]);
        Cow::Owned(output)
    } else {
        Cow::Borrowed(value)
    }
}

fn is_scheme_character(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.')
}

fn is_url_boundary(character: char) -> bool {
    character.is_whitespace()
}

#[cfg(test)]
mod tests {
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
}
