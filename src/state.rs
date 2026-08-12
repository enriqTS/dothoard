//! Persistent run status and history.
//!
//! State is stored under `~/.local/state/dothoard/` as JSON. It records
//! the outcome of each backup run for the TUI dashboard and notification
//! logic. Writes are atomic to prevent corruption from interrupted saves.

use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Maximum number of historical runs to retain.
const MAX_HISTORY_ENTRIES: usize = 50;

/// Name of the state file within the state directory.
const STATE_FILE_NAME: &str = "status.json";

/// Persistent application state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppState {
    /// Timestamp of the last attempted backup (regardless of outcome).
    pub last_attempt: Option<DateTime<Utc>>,

    /// Timestamp of the last fully successful backup (mirror + commit + push).
    pub last_success: Option<DateTime<Utc>>,

    /// The most recent commit hash created by the application.
    pub last_commit: Option<String>,

    /// Timestamp of the last successful push to the remote.
    pub last_push: Option<DateTime<Utc>>,

    /// Whether there are local commits that have not been pushed yet.
    #[serde(default)]
    pub pending_push: bool,

    /// The latest warning message from a backup run.
    pub latest_warning: Option<String>,

    /// The latest error message from a failed backup run.
    pub latest_error: Option<String>,

    /// Bounded history of recent run results, newest first.
    #[serde(default)]
    pub history: Vec<RunRecord>,
}

/// The outcome of a single backup run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunOutcome {
    /// The run completed successfully.
    Success,
    /// The run completed but nothing changed (no commit needed).
    NoChanges,
    /// The run failed.
    Failed,
    /// The backup and commit succeeded but push failed (offline).
    CommittedOffline,
}

/// A record of a single backup run for the history log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    /// Namespace used for this run.
    #[serde(default)]
    pub namespace: String,

    /// When the run started.
    pub started_at: DateTime<Utc>,

    /// When the run finished.
    pub finished_at: DateTime<Utc>,

    /// The outcome of the run.
    pub outcome: RunOutcome,

    /// Optional commit hash if a commit was created.
    pub commit: Option<String>,

    /// Optional error or warning message.
    pub message: Option<String>,

    /// Log filename for this run (relative to the logs directory).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_file: Option<String>,
}

/// Errors from state I/O operations.
#[derive(Debug, Error)]
pub enum StateError {
    #[error("failed to read state from {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse state from {path}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("failed to serialize state")]
    Serialize(#[from] serde_json::Error),

    #[error("failed to create state directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write state atomically to {path}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to persist temporary state file to {path}")]
    Persist {
        path: PathBuf,
        #[source]
        source: tempfile::PersistError,
    },
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Create a fresh state with no recorded history.
    pub fn new() -> Self {
        Self {
            last_attempt: None,
            last_success: None,
            last_commit: None,
            last_push: None,
            pending_push: false,
            latest_warning: None,
            latest_error: None,
            history: Vec::new(),
        }
    }

    /// Load state from the given state directory.
    ///
    /// Returns a fresh default state if the file does not exist yet.
    pub fn load(state_dir: &Path) -> Result<Self, StateError> {
        let path = state_dir.join(STATE_FILE_NAME);

        if !path.exists() {
            return Ok(Self::new());
        }

        let text = std::fs::read_to_string(&path).map_err(|source| StateError::Read {
            path: path.clone(),
            source,
        })?;

        let state = serde_json::from_str(&text).map_err(|source| StateError::Parse {
            path: path.clone(),
            source,
        })?;

        Ok(state)
    }

    /// Save state atomically to the given state directory.
    ///
    /// Creates the directory if it does not exist.
    pub fn save(&self, state_dir: &Path) -> Result<(), StateError> {
        if !state_dir.exists() {
            std::fs::create_dir_all(state_dir).map_err(|source| StateError::CreateDir {
                path: state_dir.to_path_buf(),
                source,
            })?;
        }

        let path = state_dir.join(STATE_FILE_NAME);
        let text = serde_json::to_string_pretty(self)?;

        let mut tmp =
            tempfile::NamedTempFile::new_in(state_dir).map_err(|source| StateError::Write {
                path: path.clone(),
                source,
            })?;

        tmp.write_all(text.as_bytes())
            .map_err(|source| StateError::Write {
                path: path.clone(),
                source,
            })?;

        tmp.flush().map_err(|source| StateError::Write {
            path: path.clone(),
            source,
        })?;

        tmp.persist(&path).map_err(|source| StateError::Persist {
            path: path.clone(),
            source,
        })?;

        Ok(())
    }

    /// Record a completed run, updating current state and appending to history.
    ///
    /// Enforces the bounded history size by dropping the oldest entries.
    pub fn record_run(&mut self, record: RunRecord) {
        self.last_attempt = Some(record.started_at);

        match &record.outcome {
            RunOutcome::Success => {
                self.last_success = Some(record.finished_at);
                self.pending_push = false;
                self.latest_error = None;
                if let Some(ref commit) = record.commit {
                    self.last_commit = Some(commit.clone());
                    self.last_push = Some(record.finished_at);
                }
            }
            RunOutcome::NoChanges => {
                self.last_success = Some(record.finished_at);
                self.latest_error = None;
                // Push state unchanged — there may still be pending commits
                // from a previous offline run.
            }
            RunOutcome::CommittedOffline => {
                self.pending_push = true;
                self.latest_error = None;
                if let Some(ref commit) = record.commit {
                    self.last_commit = Some(commit.clone());
                }
                if let Some(ref msg) = record.message {
                    self.latest_warning = Some(msg.clone());
                }
            }
            RunOutcome::Failed => {
                if let Some(ref msg) = record.message {
                    self.latest_error = Some(msg.clone());
                }
            }
        }

        self.history.insert(0, record);
        self.history.truncate(MAX_HISTORY_ENTRIES);
    }

    /// Return the state file path for a given state directory.
    pub fn path_in(state_dir: &Path) -> PathBuf {
        state_dir.join(STATE_FILE_NAME)
    }
}

#[cfg(test)]
#[path = "../tests/unit/state.rs"]
mod tests;
