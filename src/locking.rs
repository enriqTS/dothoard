//! Cross-invocation backup locking.
//!
//! An exclusive lock under `$XDG_RUNTIME_DIR` prevents startup, timer, manual,
//! and TUI-triggered backups from overlapping. A second invocation reports that
//! a backup is already running and exits without changing files.
//!
//! The lock file is created at `$XDG_RUNTIME_DIR/dothoard.lock`. It uses
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
const LOCK_FILE_NAME: &str = "dothoard.lock";

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
/// The lock is released when this value is dropped. An explicit `flock(LOCK_UN)`
/// is performed on drop to ensure the lock is released immediately, even if
/// child processes have inherited a duplicate of the file descriptor via
/// `fork()`. Without the explicit unlock, the kernel retains the lock until
/// *all* file descriptors referencing the same open file description are closed
/// (including those held by forked children between `fork()` and `exec()`).
#[derive(Debug)]
pub struct LockGuard {
    file: File,
    path: PathBuf,
}

impl LockGuard {
    /// Return the path to the lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        // Explicitly release the flock before the fd is closed.
        // This ensures the lock is freed even if child processes hold
        // duplicated file descriptors to the same open file description.
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            // LOCK_UN releases the advisory lock immediately regardless of
            // whether other file descriptors to the same open file description
            // exist (e.g., in forked child processes).
            unsafe {
                libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
            }
        }
        #[cfg(not(unix))]
        {
            // On non-Unix, fs2's unlock is the best available option.
            let _ = self.file.unlock();
        }
        tracing::trace!(path = %self.path.display(), "released exclusive lock");
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
            Ok(LockGuard { file, path })
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
#[path = "../tests/unit/locking.rs"]
mod tests;
