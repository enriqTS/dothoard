use super::*;

#[test]
fn current_backend_is_explicitly_systemd() {
    let backend = selected_backend();
    assert_eq!(backend, Backend::Systemd);
    assert_eq!(backend.name(), "systemd");
    assert_eq!(backend.description(), "systemd user timer");
}

#[test]
fn provider_status_maps_to_scheduler_neutral_status() {
    let cases = [
        (
            systemd::AutomationStatus::Active { stale: false },
            Status::Active { stale: false },
        ),
        (
            systemd::AutomationStatus::Installed { stale: true },
            Status::Installed { stale: true },
        ),
        (
            systemd::AutomationStatus::Failed {
                reason: "exit-code".to_string(),
            },
            Status::Failed {
                reason: "exit-code".to_string(),
            },
        ),
        (
            systemd::AutomationStatus::NotInstalled,
            Status::NotInstalled,
        ),
    ];

    for (provider, expected) in cases {
        assert_eq!(Status::from(provider), expected);
    }
}

#[test]
fn generic_status_language_does_not_name_provider_files() {
    assert_eq!(
        Status::Active { stale: true }.to_string(),
        "active (stale configuration)"
    );
    assert_eq!(Status::NotInstalled.to_string(), "not installed");
}
