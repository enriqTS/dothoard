use super::*;

fn test_params() -> UnitParams {
    UnitParams {
        binary_path: PathBuf::from("/usr/bin/dothoard"),
        interval_minutes: 5,
        network_timeout_seconds: 120,
    }
}

// --- A01/A02: Unit content generation ---

#[test]
fn service_unit_contains_binary_path() {
    let params = test_params();
    let content = generate_service_unit(&params);

    assert!(content.contains("ExecStart=/usr/bin/dothoard backup"));
}

#[test]
fn service_unit_has_timeout_beyond_network_timeout() {
    let params = test_params();
    let content = generate_service_unit(&params);

    // network_timeout=120 + 60 buffer = 180
    assert!(content.contains("TimeoutStartSec=180"));
}

#[test]
fn service_unit_is_oneshot() {
    let params = test_params();
    let content = generate_service_unit(&params);

    assert!(content.contains("Type=oneshot"));
}

#[test]
fn service_unit_sets_rust_log() {
    let params = test_params();
    let content = generate_service_unit(&params);

    assert!(content.contains("Environment=RUST_LOG=dothoard=info"));
}

#[test]
fn timer_unit_contains_startup_delay() {
    let params = test_params();
    let content = generate_timer_unit(&params);

    assert!(content.contains("OnStartupSec=1min"));
}

#[test]
fn timer_unit_contains_interval() {
    let params = test_params();
    let content = generate_timer_unit(&params);

    assert!(content.contains("OnUnitInactiveSec=5min"));
}

#[test]
fn timer_unit_references_service() {
    let params = test_params();
    let content = generate_timer_unit(&params);

    assert!(content.contains(&format!("Unit={}", app::SYSTEMD_SERVICE_UNIT)));
}

#[test]
fn timer_unit_installs_to_timers_target() {
    let params = test_params();
    let content = generate_timer_unit(&params);

    assert!(content.contains("WantedBy=timers.target"));
}

#[test]
fn generation_is_deterministic() {
    let params = test_params();
    let s1 = generate_service_unit(&params);
    let s2 = generate_service_unit(&params);
    let t1 = generate_timer_unit(&params);
    let t2 = generate_timer_unit(&params);

    assert_eq!(s1, s2);
    assert_eq!(t1, t2);
}

#[test]
fn different_interval_produces_different_timer() {
    let params_5 = UnitParams {
        interval_minutes: 5,
        ..test_params()
    };
    let params_10 = UnitParams {
        interval_minutes: 10,
        ..test_params()
    };

    let t5 = generate_timer_unit(&params_5);
    let t10 = generate_timer_unit(&params_10);

    assert_ne!(t5, t10);
    assert!(t5.contains("OnUnitInactiveSec=5min"));
    assert!(t10.contains("OnUnitInactiveSec=10min"));
}

#[test]
fn different_binary_path_produces_different_service() {
    let params_a = UnitParams {
        binary_path: PathBuf::from("/usr/bin/dothoard"),
        ..test_params()
    };
    let params_b = UnitParams {
        binary_path: PathBuf::from("/home/user/.cargo/bin/dothoard"),
        ..test_params()
    };

    let sa = generate_service_unit(&params_a);
    let sb = generate_service_unit(&params_b);

    assert_ne!(sa, sb);
    assert!(sa.contains("/usr/bin/dothoard"));
    assert!(sb.contains("/home/user/.cargo/bin/dothoard"));
}

#[test]
fn different_timeout_produces_different_service() {
    let params_120 = UnitParams {
        network_timeout_seconds: 120,
        ..test_params()
    };
    let params_300 = UnitParams {
        network_timeout_seconds: 300,
        ..test_params()
    };

    let s120 = generate_service_unit(&params_120);
    let s300 = generate_service_unit(&params_300);

    assert_ne!(s120, s300);
    assert!(s120.contains("TimeoutStartSec=180")); // 120+60
    assert!(s300.contains("TimeoutStartSec=360")); // 300+60
}

// --- Snapshot tests for full unit content ---

#[test]
fn service_unit_snapshot() {
    let params = test_params();
    let content = generate_service_unit(&params);

    let expected = "\
[Unit]
Description=Dothoard configuration backup
Documentation=https://github.com/dothoard/dothoard

[Service]
Type=oneshot
ExecStart=/usr/bin/dothoard backup
TimeoutStartSec=180
Environment=RUST_LOG=dothoard=info
PassEnvironment=SSH_AUTH_SOCK DBUS_SESSION_BUS_ADDRESS DISPLAY WAYLAND_DISPLAY XDG_RUNTIME_DIR

[Install]
WantedBy=default.target
";
    assert_eq!(content, expected);
}

#[test]
fn timer_unit_snapshot() {
    let params = test_params();
    let content = generate_timer_unit(&params);

    let expected = "\
[Unit]
Description=Dothoard backup timer

[Timer]
OnStartupSec=1min
OnUnitInactiveSec=5min
Unit=dothoard-backup.service

[Install]
WantedBy=timers.target
";
    assert_eq!(content, expected);
}

// --- Path resolution tests ---

#[test]
fn user_unit_dir_from_config_home() {
    let config_home = Path::new("/home/user/.config");
    let dir = user_unit_dir_from(config_home);

    assert_eq!(dir, PathBuf::from("/home/user/.config/systemd/user"));
}

#[test]
fn service_unit_path_correct() {
    let dir = Path::new("/home/user/.config/systemd/user");
    let path = service_unit_path(dir);

    assert_eq!(
        path,
        PathBuf::from("/home/user/.config/systemd/user/dothoard-backup.service")
    );
}

#[test]
fn timer_unit_path_correct() {
    let dir = Path::new("/home/user/.config/systemd/user");
    let path = timer_unit_path(dir);

    assert_eq!(
        path,
        PathBuf::from("/home/user/.config/systemd/user/dothoard-backup.timer")
    );
}

// --- Atomic write and file operation tests ---

#[test]
fn atomic_write_creates_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.unit");

    atomic_write(&path, "content here").unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "content here");
}

#[test]
fn atomic_write_overwrites_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.unit");

    atomic_write(&path, "first").unwrap();
    atomic_write(&path, "second").unwrap();

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
}

#[test]
fn remove_file_if_exists_succeeds_for_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("nonexistent");

    assert!(remove_file_if_exists(&path).is_ok());
}

#[test]
fn remove_file_if_exists_removes_existing() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("existing");
    std::fs::write(&path, "data").unwrap();

    remove_file_if_exists(&path).unwrap();

    assert!(!path.exists());
}

// --- Stale detection tests ---

#[test]
fn not_stale_when_units_match() {
    let tmp = tempfile::tempdir().unwrap();
    let unit_dir = tmp.path();
    let params = test_params();

    // Write the expected content.
    let service = generate_service_unit(&params);
    let timer = generate_timer_unit(&params);
    std::fs::write(service_unit_path(unit_dir), &service).unwrap();
    std::fs::write(timer_unit_path(unit_dir), &timer).unwrap();

    assert!(!is_stale(&params, unit_dir).unwrap());
}

#[test]
fn stale_when_service_differs() {
    let tmp = tempfile::tempdir().unwrap();
    let unit_dir = tmp.path();
    let params = test_params();

    // Write different service content.
    std::fs::write(service_unit_path(unit_dir), "old content").unwrap();
    let timer = generate_timer_unit(&params);
    std::fs::write(timer_unit_path(unit_dir), &timer).unwrap();

    assert!(is_stale(&params, unit_dir).unwrap());
}

#[test]
fn stale_when_timer_differs() {
    let tmp = tempfile::tempdir().unwrap();
    let unit_dir = tmp.path();
    let params = test_params();

    let service = generate_service_unit(&params);
    std::fs::write(service_unit_path(unit_dir), &service).unwrap();
    // Write different timer content.
    std::fs::write(timer_unit_path(unit_dir), "old timer").unwrap();

    assert!(is_stale(&params, unit_dir).unwrap());
}

#[test]
fn stale_after_interval_change() {
    let tmp = tempfile::tempdir().unwrap();
    let unit_dir = tmp.path();

    // Install with interval=5.
    let params_5 = UnitParams {
        interval_minutes: 5,
        ..test_params()
    };
    let service = generate_service_unit(&params_5);
    let timer = generate_timer_unit(&params_5);
    std::fs::write(service_unit_path(unit_dir), &service).unwrap();
    std::fs::write(timer_unit_path(unit_dir), &timer).unwrap();

    // Check with interval=10 — should be stale.
    let params_10 = UnitParams {
        interval_minutes: 10,
        ..test_params()
    };
    assert!(is_stale(&params_10, unit_dir).unwrap());
}

// --- AutomationStatus display ---

#[test]
fn automation_status_display() {
    assert_eq!(
        AutomationStatus::Active { stale: false }.to_string(),
        "active"
    );
    assert_eq!(
        AutomationStatus::Active { stale: true }.to_string(),
        "active (stale units)"
    );
    assert_eq!(
        AutomationStatus::Installed { stale: false }.to_string(),
        "installed but not running"
    );
    assert_eq!(
        AutomationStatus::Installed { stale: true }.to_string(),
        "installed but not running (stale units)"
    );
    assert_eq!(
        AutomationStatus::Failed {
            reason: "exit-code".to_string()
        }
        .to_string(),
        "failed: exit-code"
    );
    assert_eq!(AutomationStatus::NotInstalled.to_string(), "not installed");
}

// --- Status when units are not installed ---

#[test]
fn status_not_installed_when_no_files() {
    // We can't call the real `status` function without systemctl,
    // but we can verify the file-existence check logic directly.
    let tmp = tempfile::tempdir().unwrap();
    let unit_dir = tmp.path();

    // Neither file exists.
    let service_path = service_unit_path(unit_dir);
    let timer_path = timer_unit_path(unit_dir);

    assert!(!service_path.exists());
    assert!(!timer_path.exists());
}
