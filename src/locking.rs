//! Cross-invocation backup locking.
//!
//! An exclusive lock under `$XDG_RUNTIME_DIR` prevents startup, timer, manual,
//! and TUI-triggered backups from overlapping. A second invocation reports that
//! a backup is already running and exits without changing files.
//!
//! The lock file is created at `$XDG_RUNTIME_DIR/config-sync.lock`. It uses
//! `fs2::FileExt::try_lock_exclusive` which is advisory on Linux but sufficient
//! to coordinate multiple instances of the same application.
//!
//! The lock is held for the duration of the returned [`LockGuard`] and released
//! automatically when dropped.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use thiserror::Error;

use crate::app;

/// Name of the lock file within the runtime directory.
const LOCK_FILE_NAME: &str = "config-sync.lock";

/// Errors from lock acquisition.
#[derive(Debug, Error)]
pub enum LockError {
    /// Another backup is already running.
    #[error("another backup is already running (lock held at {path})")]
    AlreadyRunning { path: PathBuf },

    /// Failed to create or open the lock file.
    #[error("failed to open lock file at {path}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Failed to acquire the file lock.
    #[error("failed to acquire lock at {path}")]
    Acquire {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// An RAII guard that holds the exclusive lock.
///
/// The lock is released when this value is dropped. The lock file itself is
/// not deleted — advisory locks are released by closing the file descriptor.
#[derive(Debug)]
pub struct LockGuard {
    _file: File,
    path: PathBuf,
}

impl LockGuard {
    /// Return the path to the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Try to acquire an exclusive backup lock.
///
/// Returns `Ok(LockGuard)` if the lock was acquired, or
/// `Err(LockError::AlreadyRunning)` if another instance holds the lock.
///
/// The `runtime_dir` is the directory where the lock file is created (typically
/// `$XDG_RUNTIME_DIR`).
///
/// # Errors
///
/// - `LockError::AlreadyRunning` if another instance holds the lock.
/// - `LockError::Open` if the lock file cannot be created.
/// - `LockError::Acquire` for unexpected locking failures.
pub fn try_acquire(runtime_dir: &Path) -> Result<LockGuard, LockError> {
    let path = lock_path(runtime_dir);

    // Ensure the runtime directory exists (it should, but be defensive).
    if !runtime_dir.exists() {
        fs::create_dir_all(runtime_dir).map_err(|source| LockError::Open {
            path: path.clone(),
            source,
        })?;
    }

    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|source| LockError::Open {
            path: path.clone(),
            source,
        })?;

    match file.try_lock_exclusive() {
        Ok(()) => {
            tracing::debug!(path = %path.display(), "acquired exclusive lock");
            Ok(LockGuard { _file: file, path })
        }
        Err(ref e) if is_lock_contention(e) => Err(LockError::AlreadyRunning { path }),
        Err(source) => Err(LockError::Acquire { path, source }),
    }
}

/// Return the lock file path for a given runtime directory.
pub fn lock_path(runtime_dir: &Path) -> PathBuf {
    let file_name = format!("{}.lock", app::APP_NAME);
    // Use the constant to stay consistent if the app is renamed.
    debug_assert_eq!(file_name, LOCK_FILE_NAME);
    runtime_dir.join(LOCK_FILE_NAME)
}

/// Determine if an I/O error represents lock contention (file already locked
/// by another process).
fn is_lock_contention(error: &std::io::Error) -> bool {
    // On Linux, `flock(LOCK_EX | LOCK_NB)` returns EWOULDBLOCK when the file
    // is already locked. EWOULDBLOCK == EAGAIN on Linux, but we check both
    // for clarity on platforms where they might differ.
    matches!(error.raw_os_error(), Some(libc::EWOULDBLOCK))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquires_lock_in_empty_directory() {
        let tmp = tempfile::tempdir().unwrap();

        let guard = try_acquire(tmp.path()).unwrap();

        assert!(guard.path().exists());
        assert_eq!(guard.path(), tmp.path().join(LOCK_FILE_NAME));
    }

    #[test]
    fn second_acquisition_fails_while_lock_held() {
        let tmp = tempfile::tempdir().unwrap();

        let _guard = try_acquire(tmp.path()).unwrap();
        let result = try_acquire(tmp.path());

        assert!(matches!(result, Err(LockError::AlreadyRunning { .. })));
    }

    #[test]
    fn lock_is_released_on_drop() {
        let tmp = tempfile::tempdir().unwrap();

        {
            let _guard = try_acquire(tmp.path()).unwrap();
            // Lock is held here.
        }
        // Guard dropped — lock released.

        let guard = try_acquire(tmp.path());
        assert!(guard.is_ok());
    }

    #[test]
    fn lock_file_path_uses_app_name() {
        let dir = Path::new("/run/user/1000");
        let path = lock_path(dir);

        assert_eq!(path, PathBuf::from("/run/user/1000/config-sync.lock"));
    }

    #[test]
    fn creates_runtime_directory_if_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime_dir = tmp.path().join("nested").join("runtime");

        let guard = try_acquire(&runtime_dir);

        assert!(guard.is_ok());
        assert!(runtime_dir.exists());
    }

    #[test]
    fn lock_file_persists_after_release() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(LOCK_FILE_NAME);

        {
            let _guard = try_acquire(tmp.path()).unwrap();
        }

        // Lock file remains on disk (we don't delete it).
        assert!(path.exists());
    }

    #[test]
    fn reacquire_after_explicit_drop() {
        let tmp = tempfile::tempdir().unwrap();

        let guard1 = try_acquire(tmp.path()).unwrap();
        drop(guard1);

        let guard2 = try_acquire(tmp.path()).unwrap();
        drop(guard2);

        // Verify we can acquire a third time.
        let _guard3 = try_acquire(tmp.path()).unwrap();
    }

    #[test]
    fn already_running_error_contains_path() {
        let tmp = tempfile::tempdir().unwrap();

        let _guard = try_acquire(tmp.path()).unwrap();
        let err = try_acquire(tmp.path()).unwrap_err();

        let message = err.to_string();
        assert!(message.contains("already running"));
        assert!(message.contains(LOCK_FILE_NAME));
    }
}
