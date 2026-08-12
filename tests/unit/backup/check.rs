use super::*;

#[test]
fn check_status_display() {
    assert_eq!(CheckStatus::Ok.to_string(), "ok");
    assert_eq!(
        CheckStatus::Warning("minor issue".to_string()).to_string(),
        "warning: minor issue"
    );
    assert_eq!(
        CheckStatus::Error("bad thing".to_string()).to_string(),
        "error: bad thing"
    );
}

#[test]
fn check_status_predicates() {
    assert!(CheckStatus::Ok.is_ok());
    assert!(!CheckStatus::Ok.is_error());
    assert!(!CheckStatus::Warning("x".to_string()).is_ok());
    assert!(!CheckStatus::Warning("x".to_string()).is_error());
    assert!(!CheckStatus::Error("x".to_string()).is_ok());
    assert!(CheckStatus::Error("x".to_string()).is_error());
}

#[test]
fn healthy_report_with_no_errors() {
    let report = CheckReport {
        results: vec![
            CheckResult {
                category: "config",
                label: "test".to_string(),
                status: CheckStatus::Ok,
            },
            CheckResult {
                category: "config",
                label: "test2".to_string(),
                status: CheckStatus::Warning("minor".to_string()),
            },
        ],
    };

    assert!(report.is_healthy());
    assert_eq!(report.error_count(), 0);
    assert_eq!(report.warning_count(), 1);
}

#[test]
fn unhealthy_report_with_errors() {
    let report = CheckReport {
        results: vec![
            CheckResult {
                category: "config",
                label: "test".to_string(),
                status: CheckStatus::Ok,
            },
            CheckResult {
                category: "repository",
                label: "git".to_string(),
                status: CheckStatus::Error("not a repo".to_string()),
            },
        ],
    };

    assert!(!report.is_healthy());
    assert_eq!(report.error_count(), 1);
}

#[test]
fn check_fails_with_missing_config() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("home")).unwrap();
    std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();

    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(tmp.path().join("home")),
        config_dir: Some(tmp.path().join("config")),
        state_dir: Some(tmp.path().join("state")),
        runtime_dir: Some(tmp.path().join("runtime")),
        use_environment: false,
    })
    .unwrap();

    let report = run_check(&paths);

    assert!(!report.is_healthy());
    assert!(report.results[0].status.is_error());
    assert_eq!(report.results.len(), 1); // Stops early without config.
}

#[test]
fn check_reports_invalid_config() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("home")).unwrap();
    std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Write an invalid config (zero interval).
    std::fs::write(
        config_dir.join("config.toml"),
        "version = 1\nrepository = \"~/repo\"\ninterval_minutes = 0\n",
    )
    .unwrap();

    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(tmp.path().join("home")),
        config_dir: Some(config_dir),
        state_dir: Some(tmp.path().join("state")),
        runtime_dir: Some(tmp.path().join("runtime")),
        use_environment: false,
    })
    .unwrap();

    let report = run_check(&paths);

    assert!(!report.is_healthy());
    // Should have: config ok, then validation error.
    assert!(report.results.iter().any(|r| r.status.is_error()));
}

#[test]
fn check_reports_valid_config_with_missing_repo() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(tmp.path().join("runtime")).unwrap();
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Valid config but repo doesn't exist.
    let config = Config::new("~/nonexistent-repo", "test-machine");
    config.save(&config_dir.join("config.toml")).unwrap();

    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(home),
        config_dir: Some(config_dir),
        state_dir: Some(tmp.path().join("state")),
        runtime_dir: Some(tmp.path().join("runtime")),
        use_environment: false,
    })
    .unwrap();

    let report = run_check(&paths);

    // Should have config ok, active namespace, validation ok, and repo error.
    assert!(!report.is_healthy());
    assert!(report.results.iter().any(|result| {
        result.label == "active namespace \"test-machine\"" && result.status.is_ok()
    }));
    assert!(
        report
            .results
            .iter()
            .any(|r| r.category == "repository" && r.status.is_error())
    );
}
