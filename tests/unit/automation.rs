use super::*;

#[test]
fn backend_names_descriptions_and_cycle_are_explicit() {
    assert_eq!(Backend::Systemd.name(), "systemd");
    assert_eq!(Backend::Systemd.description(), "systemd user timer");
    assert_eq!(Backend::Systemd.next(), Backend::Cron);
    assert_eq!(Backend::Cron.name(), "cron");
    assert_eq!(Backend::Cron.description(), "user crontab");
    assert_eq!(Backend::Cron.next(), Backend::Systemd);
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
