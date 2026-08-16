use super::*;

#[test]
fn backend_names_descriptions_and_cycle_are_explicit() {
    assert_eq!(Backend::Systemd.name(), "systemd");
    assert_eq!(Backend::Systemd.description(), "systemd user timer");
    assert_eq!(Backend::Systemd.next(), Backend::Cron);
    assert_eq!(Backend::Cron.name(), "cron");
    assert_eq!(Backend::Cron.description(), "user crontab");
    assert_eq!(Backend::Cron.next(), Backend::External);
    assert_eq!(Backend::External.name(), "external");
    assert_eq!(
        Backend::External.description(),
        "externally managed scheduler"
    );
    assert_eq!(Backend::External.next(), Backend::Systemd);
}

#[test]
fn selected_backend_comes_from_configuration() {
    let mut config = Config::new("~/repo", "machine");
    assert_eq!(selected_backend(&config), Backend::Systemd);
    config.automation_backend = Backend::Cron;
    assert_eq!(selected_backend(&config), Backend::Cron);
}

#[test]
fn systemd_paths_follow_injected_app_paths_instead_of_process_environment() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let config_home = temp.path().join("isolated-config");
    let config_dir = config_home.join("dothoard");
    let state_dir = temp.path().join("state");
    let runtime_dir = temp.path().join("run");
    for directory in [&home, &config_dir, &state_dir, &runtime_dir] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(home),
        config_dir: Some(config_dir),
        state_dir: Some(state_dir),
        runtime_dir: Some(runtime_dir),
        use_environment: false,
    })
    .unwrap();

    assert_eq!(systemd_unit_dir(&paths), config_home.join("systemd/user"));
}

#[test]
fn external_command_is_copyable_and_shell_quotes_paths() {
    let quoted = shell_quote(
        std::path::Path::new("/run/user/name with ' quote"),
        "runtime directory",
    )
    .unwrap();
    assert_eq!(quoted, "'/run/user/name with '\\'' quote'");
}

#[test]
fn external_backend_reports_command_but_refuses_lifecycle_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let config_dir = temp.path().join("config/dothoard");
    let state_dir = temp.path().join("state");
    let runtime_dir = temp.path().join("run");
    for directory in [&home, &config_dir, &state_dir, &runtime_dir] {
        std::fs::create_dir_all(directory).unwrap();
    }
    let paths = AppPaths::resolve(crate::paths::PathInputs {
        home: Some(home),
        config_dir: Some(config_dir),
        state_dir: Some(state_dir),
        runtime_dir: Some(runtime_dir.clone()),
        use_environment: false,
    })
    .unwrap();
    let mut config = Config::new("~/repo", "machine");
    config.automation_backend = Backend::External;

    let command = external_command(&paths).unwrap();
    assert!(command.starts_with(&format!("XDG_RUNTIME_DIR='{}' ", runtime_dir.display())));
    assert!(command.ends_with(" backup"));
    assert_eq!(
        status(&config, &paths).unwrap(),
        Status::External {
            command: command.clone()
        }
    );
    assert!(!is_installed(&config, &paths).unwrap());
    assert!(!is_stale(&config, &paths).unwrap());
    assert!(matches!(
        install(&config, &paths),
        Err(AutomationError::ExternallyManaged {
            operation: "installation"
        })
    ));
    assert!(matches!(
        remove(&config, &paths),
        Err(AutomationError::ExternallyManaged {
            operation: "removal"
        })
    ));
}

#[test]
fn provider_status_maps_to_scheduler_neutral_status() {
    assert_eq!(
        Status::from(systemd::AutomationStatus::Installed { stale: true }),
        Status::Installed {
            stale: true,
            activity: ActivityStatus::Inactive,
        }
    );
    assert_eq!(
        Status::from(cron::CronStatus::Installed { stale: false }),
        Status::Installed {
            stale: false,
            activity: ActivityStatus::NotInspected,
        }
    );
    assert_eq!(
        Status::from(systemd::AutomationStatus::Failed {
            reason: "exit-code".to_string(),
        }),
        Status::Failed {
            reason: "exit-code".to_string(),
        }
    );
    assert_eq!(
        Status::from(cron::CronStatus::NotInstalled),
        Status::NotInstalled
    );
}

#[test]
fn generic_status_describes_backend_activity_limitations() {
    assert_eq!(
        Status::External {
            command: "'/usr/bin/dothoard' backup".to_string()
        }
        .to_string(),
        "externally managed; schedule `'/usr/bin/dothoard' backup`"
    );
    assert_eq!(
        Status::Active { stale: true }.to_string(),
        "active (stale configuration)"
    );
    assert_eq!(
        Status::Installed {
            stale: false,
            activity: ActivityStatus::NotInspected,
        }
        .to_string(),
        "installed (scheduler activity not inspected)"
    );
}
