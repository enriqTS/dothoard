//! Health check: validates configuration, paths, repository, and automation.
//!
//! The `check` command reports problems across all validation layers without
//! performing a backup. It exits with code 0 if everything is healthy, or
//! code 1 if any issue is found.
//!
//! Checks performed:
//! 1. Configuration file exists and parses.
//! 2. Configuration passes semantic validation.
//! 3. Source paths are valid on the filesystem (no symlinked parents).
//! 4. No source overlaps or repository containment.
//! 5. Repository exists and is a valid Git worktree with a branch and remote.
//! 6. Repository is not in a conflicting operation state.
//! 7. Repository ownership is usable (New or Owned).
//! 8. Remote is accessible noninteractively (authentication).
//! 9. (Future) Systemd timer status and staleness.

use std::path::PathBuf;
use std::time::Duration;

use crate::config::Config;
use crate::git::{self, AuthStatus, GitRunner, OwnershipState};
use crate::paths::{self, AppPaths};

/// A single check result with a category and status.
#[derive(Debug, Clone)]
pub struct CheckResult {
    pub category: &'static str,
    pub label: String,
    pub status: CheckStatus,
}

/// The outcome of a single check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckStatus {
    /// The check passed.
    Ok,
    /// The check found a non-fatal issue (informational).
    Warning(String),
    /// The check found a problem that would prevent backup.
    Error(String),
}

impl CheckStatus {
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok)
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }
}

impl std::fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ok => write!(f, "ok"),
            Self::Warning(msg) => write!(f, "warning: {msg}"),
            Self::Error(msg) => write!(f, "error: {msg}"),
        }
    }
}

/// The overall check report.
#[derive(Debug)]
pub struct CheckReport {
    pub results: Vec<CheckResult>,
}

impl CheckReport {
    /// Returns true if all checks passed (no errors).
    pub fn is_healthy(&self) -> bool {
        !self.results.iter().any(|r| r.status.is_error())
    }

    /// Count of errors.
    pub fn error_count(&self) -> usize {
        self.results.iter().filter(|r| r.status.is_error()).count()
    }

    /// Count of warnings.
    pub fn warning_count(&self) -> usize {
        self.results
            .iter()
            .filter(|r| matches!(r.status, CheckStatus::Warning(_)))
            .count()
    }
}

/// Run all health checks and produce a report.
pub fn run_check(paths: &AppPaths) -> CheckReport {
    let mut results = Vec::new();

    // 1. Configuration file exists and parses.
    let config = match check_config(paths) {
        Ok(cfg) => {
            results.push(CheckResult {
                category: "config",
                label: "configuration file".to_string(),
                status: CheckStatus::Ok,
            });
            Some(cfg)
        }
        Err(status) => {
            results.push(CheckResult {
                category: "config",
                label: "configuration file".to_string(),
                status,
            });
            None
        }
    };

    // If config failed to load, we can't proceed with other checks.
    let Some(config) = config else {
        return CheckReport { results };
    };

    // 2. Configuration semantic validation.
    let validation_errors = config.validate();
    if validation_errors.is_empty() {
        results.push(CheckResult {
            category: "config",
            label: "configuration validity".to_string(),
            status: CheckStatus::Ok,
        });
    } else {
        for err in &validation_errors {
            results.push(CheckResult {
                category: "config",
                label: "configuration validity".to_string(),
                status: CheckStatus::Error(err.to_string()),
            });
        }
    }

    // If there are validation errors, some subsequent checks might fail.
    if !validation_errors.is_empty() {
        return CheckReport { results };
    }

    // 3. Source path filesystem validation.
    let mut source_paths: Vec<PathBuf> = Vec::new();
    for source in &config.sources {
        match paths::validate_source_path(paths.home(), &source.path) {
            Ok(abs_path) => {
                results.push(CheckResult {
                    category: "sources",
                    label: format!("source \"{}\"", source.path),
                    status: CheckStatus::Ok,
                });
                source_paths.push(abs_path);
            }
            Err(e) => {
                results.push(CheckResult {
                    category: "sources",
                    label: format!("source \"{}\"", source.path),
                    status: CheckStatus::Error(e.to_string()),
                });
                // Use the simple join for overlap check even on failure.
                source_paths.push(paths.home().join(&source.path));
            }
        }
    }

    // 4. Overlap and recursion validation.
    let repository = config.repository_path(paths.home());
    let overlaps = paths::check_overlaps(&source_paths, &repository);
    if overlaps.is_empty() {
        results.push(CheckResult {
            category: "sources",
            label: "overlap/recursion".to_string(),
            status: CheckStatus::Ok,
        });
    } else {
        for overlap in &overlaps {
            results.push(CheckResult {
                category: "sources",
                label: "overlap/recursion".to_string(),
                status: CheckStatus::Error(overlap.to_string()),
            });
        }
    }

    // 5-6. Repository validation.
    let timeout = Duration::from_secs(u64::from(config.network_timeout_seconds));
    let runner = GitRunner::new(timeout);

    let repo_info = match git::validate_repository(&runner, &repository, &config.remote) {
        Ok(info) => {
            results.push(CheckResult {
                category: "repository",
                label: "git repository".to_string(),
                status: CheckStatus::Ok,
            });
            results.push(CheckResult {
                category: "repository",
                label: format!("branch \"{}\"", info.branch),
                status: CheckStatus::Ok,
            });
            results.push(CheckResult {
                category: "repository",
                label: format!("remote \"{}\"", info.remote),
                status: CheckStatus::Ok,
            });
            Some(info)
        }
        Err(e) => {
            results.push(CheckResult {
                category: "repository",
                label: "git repository".to_string(),
                status: CheckStatus::Error(e.to_string()),
            });
            None
        }
    };

    // 7. Repository ownership.
    match git::classify_ownership(&repository) {
        Ok(OwnershipState::Owned { .. }) => {
            results.push(CheckResult {
                category: "repository",
                label: "ownership".to_string(),
                status: CheckStatus::Ok,
            });
        }
        Ok(OwnershipState::New) => {
            results.push(CheckResult {
                category: "repository",
                label: "ownership".to_string(),
                status: CheckStatus::Warning(
                    "namespace not yet initialized (will initialize on first backup)".to_string(),
                ),
            });
        }
        Ok(OwnershipState::InvalidManifest { reason }) => {
            results.push(CheckResult {
                category: "repository",
                label: "ownership".to_string(),
                status: CheckStatus::Error(format!("invalid manifest: {reason}")),
            });
        }
        Ok(OwnershipState::Ambiguous { reason }) => {
            results.push(CheckResult {
                category: "repository",
                label: "ownership".to_string(),
                status: CheckStatus::Error(format!("ambiguous content: {reason}")),
            });
        }
        Err(e) => {
            results.push(CheckResult {
                category: "repository",
                label: "ownership".to_string(),
                status: CheckStatus::Error(e.to_string()),
            });
        }
    }

    // 8. Authentication readiness (only if repo validated).
    if let Some(ref info) = repo_info {
        match git::check_auth(&runner, &info.worktree, &config.remote) {
            Ok(AuthStatus::Ready) => {
                results.push(CheckResult {
                    category: "auth",
                    label: "remote authentication".to_string(),
                    status: CheckStatus::Ok,
                });
            }
            Ok(AuthStatus::NotReady { reason }) => {
                results.push(CheckResult {
                    category: "auth",
                    label: "remote authentication".to_string(),
                    status: CheckStatus::Warning(format!("not accessible: {reason}")),
                });
            }
            Err(e) => {
                results.push(CheckResult {
                    category: "auth",
                    label: "remote authentication".to_string(),
                    status: CheckStatus::Warning(format!("check failed: {e}")),
                });
            }
        }
    }

    // 9. Automation status (placeholder for systemd milestone).
    results.push(CheckResult {
        category: "automation",
        label: "systemd timer".to_string(),
        status: CheckStatus::Warning("automation check not yet implemented".to_string()),
    });

    CheckReport { results }
}

/// Try to load and return the configuration.
fn check_config(paths: &AppPaths) -> Result<Config, CheckStatus> {
    match Config::load(paths.config_file()) {
        Ok(config) => Ok(config),
        Err(crate::config::ConfigError::NotFound { path }) => Err(CheckStatus::Error(format!(
            "configuration file not found: {}",
            path.display()
        ))),
        Err(e) => Err(CheckStatus::Error(e.to_string())),
    }
}

/// Print a human-readable report to the terminal.
pub fn print_report(report: &CheckReport) {
    for result in &report.results {
        let icon = match &result.status {
            CheckStatus::Ok => "✓",
            CheckStatus::Warning(_) => "⚠",
            CheckStatus::Error(_) => "✗",
        };
        let status_str = match &result.status {
            CheckStatus::Ok => "ok".to_string(),
            CheckStatus::Warning(msg) => msg.clone(),
            CheckStatus::Error(msg) => msg.clone(),
        };

        tracing::info!(
            category = result.category,
            check = %result.label,
            status = %result.status,
            "{icon} [{category}] {label}: {status_str}",
            icon = icon,
            category = result.category,
            label = result.label,
            status_str = status_str,
        );
    }

    let errors = report.error_count();
    let warnings = report.warning_count();
    if errors > 0 {
        tracing::error!(errors = errors, warnings = warnings, "check completed with errors");
    } else if warnings > 0 {
        tracing::warn!(warnings = warnings, "check completed with warnings");
    } else {
        tracing::info!("all checks passed");
    }
}

#[cfg(test)]
mod tests {
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
        let config = Config::new("~/nonexistent-repo");
        config
            .save(&config_dir.join("config.toml"))
            .unwrap();

        let paths = AppPaths::resolve(crate::paths::PathInputs {
            home: Some(home),
            config_dir: Some(config_dir),
            state_dir: Some(tmp.path().join("state")),
            runtime_dir: Some(tmp.path().join("runtime")),
            use_environment: false,
        })
        .unwrap();

        let report = run_check(&paths);

        // Should have config ok, validation ok, repo error.
        assert!(!report.is_healthy());
        assert!(report
            .results
            .iter()
            .any(|r| r.category == "repository" && r.status.is_error()));
    }
}
