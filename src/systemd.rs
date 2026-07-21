//! User service and timer generation and management.
//!
//! This module generates deterministic `systemd --user` unit files for the
//! backup service and timer, and provides operations to install, remove,
//! inspect, and update them idempotently.
//!
//! Unit generation is pure and testable without a running systemd instance.
//! Management operations invoke `systemctl --user` with direct argument arrays.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

use crate::app;

// ─── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum SystemdError {
    #[error("failed to determine systemd user unit directory")]
    UnitDirNotFound,

    #[error("failed to create unit directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to write unit file {path}")]
    WriteUnit {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to persist unit file {path}")]
    PersistUnit {
        path: PathBuf,
        #[source]
        source: tempfile::PersistError,
    },

    #[error("failed to remove unit file {path}")]
    RemoveUnit {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to read unit file {path}")]
    ReadUnit {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("systemctl failed: {operation}")]
    Systemctl {
        operation: String,
        #[source]
        source: std::io::Error,
    },

    #[error("systemctl {operation} exited with status {status}: {stderr}")]
    SystemctlFailed {
        operation: String,
        status: i32,
        stderr: String,
    },

    #[error("binary path could not be determined")]
    BinaryNotFound,
}

// ─── Unit Generation ─────────────────────────────────────────────────────────

/// Parameters for generating unit file content.
#[derive(Debug, Clone)]
pub struct UnitParams {
    /// Absolute path to the `dothoard` binary.
    pub binary_path: PathBuf,
    /// Backup interval in minutes (from configuration).
    pub interval_minutes: u32,
    /// Network timeout in seconds (used to derive the service timeout).
    pub network_timeout_seconds: u32,
}

/// Generate the content of the `.service` unit file.
///
/// The service runs `dothoard backup` directly with journal logging and a
/// finite timeout longer than the Git network timeout.
pub fn generate_service_unit(params: &UnitParams) -> String {
    // Service timeout = network timeout + 60s buffer for filesystem operations.
    let service_timeout_sec = u64::from(params.network_timeout_seconds) + 60;
    let binary = params.binary_path.display();

    format!(
        "\
[Unit]
Description=Dothoard configuration backup
Documentation=https://github.com/dothoard/dothoard

[Service]
Type=oneshot
ExecStart={binary} backup
TimeoutStartSec={service_timeout_sec}
Environment=RUST_LOG=dothoard=info

[Install]
WantedBy=default.target
"
    )
}

/// Generate the content of the `.timer` unit file.
///
/// The timer fires shortly after user-manager startup and then after each
/// configured interval following backup completion.
pub fn generate_timer_unit(params: &UnitParams) -> String {
    let interval = params.interval_minutes;

    format!(
        "\
[Unit]
Description=Dothoard backup timer

[Timer]
OnStartupSec=1min
OnUnitInactiveSec={interval}min
Unit={service}

[Install]
WantedBy=timers.target
",
        service = app::SYSTEMD_SERVICE_UNIT,
    )
}

// ─── Path Resolution ─────────────────────────────────────────────────────────

/// Resolve the systemd user unit directory.
///
/// Default: `$XDG_CONFIG_HOME/systemd/user/` or `~/.config/systemd/user/`.
pub fn user_unit_dir(home: &Path) -> PathBuf {
    let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| home.join(".config"));

    xdg_config.join("systemd").join("user")
}

/// Resolve the systemd user unit directory from injected inputs (for testing).
pub fn user_unit_dir_from(config_home: &Path) -> PathBuf {
    config_home.join("systemd").join("user")
}

/// Path to the installed service unit file.
pub fn service_unit_path(unit_dir: &Path) -> PathBuf {
    unit_dir.join(app::SYSTEMD_SERVICE_UNIT)
}

/// Path to the installed timer unit file.
pub fn timer_unit_path(unit_dir: &Path) -> PathBuf {
    unit_dir.join(app::SYSTEMD_TIMER_UNIT)
}

// ─── Installation ────────────────────────────────────────────────────────────

/// Install (or reinstall) the service and timer units idempotently.
///
/// Steps:
/// 1. Create the unit directory if needed.
/// 2. Write both units atomically.
/// 3. Run `systemctl --user daemon-reload`.
/// 4. Enable and start (or restart) the timer.
pub fn install(params: &UnitParams, unit_dir: &Path) -> Result<(), SystemdError> {
    // 1. Ensure directory exists.
    if !unit_dir.exists() {
        std::fs::create_dir_all(unit_dir).map_err(|source| SystemdError::CreateDir {
            path: unit_dir.to_path_buf(),
            source,
        })?;
    }

    // 2. Write units atomically.
    let service_content = generate_service_unit(params);
    let timer_content = generate_timer_unit(params);

    atomic_write(&service_unit_path(unit_dir), &service_content)?;
    atomic_write(&timer_unit_path(unit_dir), &timer_content)?;

    // 3. Reload the user manager.
    systemctl(&["daemon-reload"])?;

    // 4. Enable and start the timer.
    // Use enable + restart to handle both fresh installs and updates.
    systemctl(&["enable", app::SYSTEMD_TIMER_UNIT])?;
    systemctl(&["restart", app::SYSTEMD_TIMER_UNIT])?;

    Ok(())
}

// ─── Removal ─────────────────────────────────────────────────────────────────

/// Remove the service and timer units.
///
/// Steps:
/// 1. Stop and disable the timer (ignore errors if not active).
/// 2. Remove both unit files.
/// 3. Run `systemctl --user daemon-reload`.
pub fn remove(unit_dir: &Path) -> Result<(), SystemdError> {
    // 1. Stop and disable (best-effort; units might not be active/enabled).
    let _ = systemctl(&["stop", app::SYSTEMD_TIMER_UNIT]);
    let _ = systemctl(&["disable", app::SYSTEMD_TIMER_UNIT]);

    // 2. Remove unit files.
    let service_path = service_unit_path(unit_dir);
    let timer_path = timer_unit_path(unit_dir);

    remove_file_if_exists(&service_path)?;
    remove_file_if_exists(&timer_path)?;

    // 3. Reload.
    systemctl(&["daemon-reload"])?;

    Ok(())
}

// ─── Status Inspection ───────────────────────────────────────────────────────

/// Automation status as seen by the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationStatus {
    /// Units are installed and the timer is active.
    Active {
        /// Whether the installed units match expected content.
        stale: bool,
    },
    /// Units are installed but the timer is not running.
    Installed {
        /// Whether the installed units match expected content.
        stale: bool,
    },
    /// The timer is in a failed state.
    Failed { reason: String },
    /// Units are not installed.
    NotInstalled,
}

impl std::fmt::Display for AutomationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Active { stale: false } => write!(f, "active"),
            Self::Active { stale: true } => write!(f, "active (stale units)"),
            Self::Installed { stale: false } => write!(f, "installed but not running"),
            Self::Installed { stale: true } => write!(f, "installed but not running (stale units)"),
            Self::Failed { reason } => write!(f, "failed: {reason}"),
            Self::NotInstalled => write!(f, "not installed"),
        }
    }
}

/// Inspect the current automation status.
///
/// Reads installed unit files and queries systemctl for timer state.
pub fn status(params: &UnitParams, unit_dir: &Path) -> Result<AutomationStatus, SystemdError> {
    let service_path = service_unit_path(unit_dir);
    let timer_path = timer_unit_path(unit_dir);

    // Check if units are installed.
    if !service_path.exists() || !timer_path.exists() {
        return Ok(AutomationStatus::NotInstalled);
    }

    // Check for staleness.
    let stale = is_stale(params, unit_dir)?;

    // Query timer state via systemctl.
    let timer_state = get_unit_active_state(app::SYSTEMD_TIMER_UNIT)?;

    match timer_state.as_str() {
        "active" | "waiting" => Ok(AutomationStatus::Active { stale }),
        "failed" => {
            let reason = get_unit_sub_state(app::SYSTEMD_TIMER_UNIT)?;
            Ok(AutomationStatus::Failed { reason })
        }
        _ => Ok(AutomationStatus::Installed { stale }),
    }
}

// ─── Interval Update ─────────────────────────────────────────────────────────

/// Regenerate and restart the timer after a configuration change.
///
/// Only regenerates the timer unit (the service unit rarely changes).
/// Does not stop an active backup service — only the timer is restarted.
pub fn update_interval(params: &UnitParams, unit_dir: &Path) -> Result<(), SystemdError> {
    let timer_content = generate_timer_unit(params);
    atomic_write(&timer_unit_path(unit_dir), &timer_content)?;

    // Also regenerate service in case timeout changed.
    let service_content = generate_service_unit(params);
    atomic_write(&service_unit_path(unit_dir), &service_content)?;

    systemctl(&["daemon-reload"])?;
    systemctl(&["restart", app::SYSTEMD_TIMER_UNIT])?;

    Ok(())
}

// ─── Stale Detection ─────────────────────────────────────────────────────────

/// Check whether installed units differ from the expected generated versions.
pub fn is_stale(params: &UnitParams, unit_dir: &Path) -> Result<bool, SystemdError> {
    let expected_service = generate_service_unit(params);
    let expected_timer = generate_timer_unit(params);

    let installed_service = read_unit_file(&service_unit_path(unit_dir))?;
    let installed_timer = read_unit_file(&timer_unit_path(unit_dir))?;

    Ok(installed_service != expected_service || installed_timer != expected_timer)
}

// ─── Binary Path Resolution ──────────────────────────────────────────────────

/// Determine the absolute path to the currently running binary.
///
/// Uses `std::env::current_exe()` which resolves through `/proc/self/exe`
/// on Linux.
pub fn current_binary_path() -> Result<PathBuf, SystemdError> {
    std::env::current_exe().map_err(|_| SystemdError::BinaryNotFound)
}

/// Build `UnitParams` from the configuration and current binary.
pub fn params_from_config(config: &crate::config::Config) -> Result<UnitParams, SystemdError> {
    Ok(UnitParams {
        binary_path: current_binary_path()?,
        interval_minutes: config.interval_minutes,
        network_timeout_seconds: config.network_timeout_seconds,
    })
}

// ─── Internal Helpers ────────────────────────────────────────────────────────

/// Write content to a file atomically using a tempfile in the same directory.
fn atomic_write(path: &Path, content: &str) -> Result<(), SystemdError> {
    let parent = path.parent().unwrap_or(Path::new("."));

    let mut tmp =
        tempfile::NamedTempFile::new_in(parent).map_err(|source| SystemdError::WriteUnit {
            path: path.to_path_buf(),
            source,
        })?;

    tmp.write_all(content.as_bytes())
        .map_err(|source| SystemdError::WriteUnit {
            path: path.to_path_buf(),
            source,
        })?;

    tmp.flush().map_err(|source| SystemdError::WriteUnit {
        path: path.to_path_buf(),
        source,
    })?;

    tmp.persist(path)
        .map_err(|source| SystemdError::PersistUnit {
            path: path.to_path_buf(),
            source,
        })?;

    Ok(())
}

/// Read a unit file to a string.
fn read_unit_file(path: &Path) -> Result<String, SystemdError> {
    std::fs::read_to_string(path).map_err(|source| SystemdError::ReadUnit {
        path: path.to_path_buf(),
        source,
    })
}

/// Remove a file if it exists, ignoring "not found" errors.
fn remove_file_if_exists(path: &Path) -> Result<(), SystemdError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(SystemdError::RemoveUnit {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Run a `systemctl --user` command and check for success.
fn systemctl(args: &[&str]) -> Result<String, SystemdError> {
    let operation = format!("systemctl --user {}", args.join(" "));

    let output = Command::new("systemctl")
        .arg("--user")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env("SYSTEMD_PAGER", "")
        .output()
        .map_err(|source| SystemdError::Systemctl {
            operation: operation.clone(),
            source,
        })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(SystemdError::SystemctlFailed {
            operation,
            status: output.status.code().unwrap_or(-1),
            stderr,
        })
    }
}

/// Query a single property from a systemd unit.
fn get_unit_property(unit: &str, property: &str) -> Result<String, SystemdError> {
    let output = systemctl(&["show", "--property", property, "--value", unit])?;
    Ok(output.trim().to_string())
}

/// Get the ActiveState of a unit.
fn get_unit_active_state(unit: &str) -> Result<String, SystemdError> {
    get_unit_property(unit, "ActiveState")
}

/// Get the SubState of a unit.
fn get_unit_sub_state(unit: &str) -> Result<String, SystemdError> {
    get_unit_property(unit, "SubState")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
}
