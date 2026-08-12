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

    assert_eq!(path, PathBuf::from("/run/user/1000/dothoard.lock"));
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
