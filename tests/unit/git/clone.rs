use std::path::Path;
use std::time::Duration;

use super::*;

fn timeout() -> Duration {
    Duration::from_secs(5)
}

#[test]
fn clones_local_remote_into_new_destination() {
    let temp = tempfile::tempdir().unwrap();
    let remote = temp.path().join("remote.git");
    let parent = temp.path().join("clones");
    let destination = parent.join("backup");
    std::fs::create_dir(&parent).unwrap();

    let init = std::process::Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(&remote)
        .status()
        .unwrap();
    assert!(init.success());

    let cloned =
        clone_repository(remote.to_str().unwrap(), destination.as_path(), timeout()).unwrap();

    assert_eq!(cloned, destination);
    assert!(destination.join(".git").is_dir());
    let origin = std::process::Command::new("git")
        .current_dir(&destination)
        .args(["remote", "get-url", "origin"])
        .output()
        .unwrap();
    assert!(origin.status.success());
    assert_eq!(
        String::from_utf8_lossy(&origin.stdout).trim(),
        remote.to_str().unwrap()
    );
}

#[test]
fn refuses_empty_url_relative_destination_and_missing_parent() {
    let temp = tempfile::tempdir().unwrap();
    assert!(matches!(
        clone_repository(" ", &temp.path().join("repo"), timeout()),
        Err(CloneError::EmptyUrl)
    ));
    assert!(matches!(
        clone_repository("remote", Path::new("relative/repo"), timeout()),
        Err(CloneError::RelativeDestination(_))
    ));
    assert!(matches!(
        clone_repository(
            "remote",
            &temp.path().join("missing-parent/repo"),
            timeout()
        ),
        Err(CloneError::ParentMissing(_))
    ));
}

#[test]
fn never_overwrites_an_existing_destination() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    std::fs::create_dir(&destination).unwrap();
    std::fs::write(destination.join("keep"), "unchanged").unwrap();

    let error = clone_repository("not-used", &destination, timeout()).unwrap_err();

    assert!(matches!(error, CloneError::DestinationExists(_)));
    assert_eq!(
        std::fs::read_to_string(destination.join("keep")).unwrap(),
        "unchanged"
    );
}

#[cfg(unix)]
#[test]
fn refuses_symlinked_destination_parent() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let real = temp.path().join("real");
    let link = temp.path().join("link");
    std::fs::create_dir(&real).unwrap();
    symlink(&real, &link).unwrap();

    let error = clone_repository("not-used", &link.join("repo"), timeout()).unwrap_err();

    assert!(matches!(error, CloneError::SymlinkParent(path) if path == link));
    assert!(!real.join("repo").exists());
}

#[test]
fn clone_failure_redacts_credentials_and_does_not_report_success() {
    let temp = tempfile::tempdir().unwrap();
    let destination = temp.path().join("repo");
    let secret = "very-secret-password";
    let url = format!("https://user:{secret}@127.0.0.1:1/repository.git");

    let error = clone_repository(&url, &destination, timeout()).unwrap_err();
    let message = error.to_string();

    assert!(!message.contains(secret), "credential leaked in: {message}");
    assert!(!destination.join(".git").exists());
}
