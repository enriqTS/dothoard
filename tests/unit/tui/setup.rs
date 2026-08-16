use super::*;

#[test]
fn setup_starts_with_existing_repository_choice() {
    let setup = SetupState::new();
    assert_eq!(setup.step, SetupStep::Repository);
    assert_eq!(setup.repository_mode, RepositorySetupMode::Choose);
    assert_eq!(setup.repository_method, RepositoryMethod::Existing);
    assert_eq!(setup.clone_field, CloneField::Url);
    assert_eq!(setup.automation_field, AutomationField::Backend);
    assert_eq!(setup.automation_backend, AutomationBackend::Systemd);
    assert_eq!(setup.interval_input, "5");
    assert!(!setup.cloning());
    assert!(setup.clone_error().is_none());
}

#[test]
fn repository_method_toggle_is_reversible() {
    assert_eq!(RepositoryMethod::Existing.toggle(), RepositoryMethod::Clone);
    assert_eq!(RepositoryMethod::Clone.toggle(), RepositoryMethod::Existing);
}

#[test]
fn backend_navigation_covers_every_option() {
    let mut setup = SetupState::new();
    setup.next_backend();
    assert_eq!(setup.automation_backend, AutomationBackend::Cron);
    setup.next_backend();
    assert_eq!(setup.automation_backend, AutomationBackend::External);
    setup.next_backend();
    assert_eq!(setup.automation_backend, AutomationBackend::Systemd);
    setup.previous_backend();
    assert_eq!(setup.automation_backend, AutomationBackend::External);
}

#[test]
fn incomplete_marker_round_trips_and_resume_keeps_configuration() {
    let temp = tempfile::tempdir().unwrap();
    mark_incomplete(temp.path()).unwrap();
    assert!(is_incomplete(temp.path()));

    let mut config = crate::config::Config::new("/repo", "desktop");
    config.automation_backend = AutomationBackend::Cron;
    config.interval_minutes = 15;
    let resumed = SetupState::resume(&config, ThemeId::Nord);
    assert_eq!(resumed.step, SetupStep::Automation);
    assert_eq!(resumed.automation_backend, AutomationBackend::Cron);
    assert_eq!(resumed.interval_input, "15");
    assert_eq!(resumed.theme_selected, ThemeId::Nord);

    clear_incomplete(temp.path()).unwrap();
    assert!(!is_incomplete(temp.path()));
}
