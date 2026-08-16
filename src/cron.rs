//! User crontab generation and lifecycle management.
//!
//! Dothoard owns only one clearly delimited block. Unrelated crontab bytes are
//! preserved when that block is installed, refreshed, or removed. Malformed,
//! duplicate, or ownership-marker-deficient blocks are refused rather than
//! guessed at.

use std::io::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

use crate::config::Config;

const BEGIN_MARKER: &str = "# BEGIN dothoard managed automation v1";
const OWNER_MARKER: &str = "# Managed by dothoard; do not edit this block";
const END_MARKER: &str = "# END dothoard managed automation v1";

/// Parameters used to generate dothoard's managed crontab block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronParams {
    pub binary_path: PathBuf,
    pub runtime_dir: PathBuf,
    pub interval_minutes: u32,
}

/// Cron automation status visible through the scheduler-neutral facade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CronStatus {
    Installed { stale: bool },
    NotInstalled,
}

#[derive(Debug, Error)]
pub enum CronError {
    #[error("dothoard executable path could not be determined")]
    BinaryNotFound,
    #[error("cron runtime directory must be absolute: {0}")]
    RuntimeDirUnavailable(PathBuf),
    #[error("cron supports interval_minutes from 1 through 59, got {0}")]
    UnsupportedInterval(u32),
    #[error("cron cannot safely represent {name} path: {path}")]
    UnsafePath { name: &'static str, path: PathBuf },
    #[error("failed to execute crontab {operation}")]
    Command {
        operation: &'static str,
        #[source]
        source: std::io::Error,
    },
    #[error("crontab {operation} exited with status {status}: {stderr}")]
    CommandFailed {
        operation: &'static str,
        status: i32,
        stderr: String,
    },
    #[error("crontab output is not valid UTF-8")]
    NonUtf8,
    #[error("managed cron block is malformed or ambiguous: {0}")]
    AmbiguousManagedBlock(String),
}

/// Build cron parameters from configuration and the current process.
pub fn params_from_config(config: &Config, runtime_dir: &Path) -> Result<CronParams, CronError> {
    let binary_path = std::env::current_exe().map_err(|_| CronError::BinaryNotFound)?;
    if !runtime_dir.is_absolute() {
        return Err(CronError::RuntimeDirUnavailable(runtime_dir.to_path_buf()));
    }

    Ok(CronParams {
        binary_path,
        runtime_dir: runtime_dir.to_path_buf(),
        interval_minutes: config.interval_minutes,
    })
}

/// Generate the complete dothoard-owned crontab block.
pub fn generate_managed_block(params: &CronParams) -> Result<String, CronError> {
    if !(1..=59).contains(&params.interval_minutes) {
        return Err(CronError::UnsupportedInterval(params.interval_minutes));
    }

    let binary = safe_cron_token(&params.binary_path, "executable")?;
    let runtime = safe_cron_token(&params.runtime_dir, "runtime")?;

    Ok(format!(
        "{BEGIN_MARKER}\n{OWNER_MARKER}\n# backend=cron interval_minutes={interval}\n*/{interval} * * * * XDG_RUNTIME_DIR={runtime} {binary} backup\n{END_MARKER}\n",
        interval = params.interval_minutes,
    ))
}

/// Install or refresh the managed cron block while preserving unrelated text.
pub fn install(params: &CronParams) -> Result<(), CronError> {
    install_with(params, &CommandCrontab)
}

/// Remove the managed cron block while preserving unrelated text.
pub fn remove() -> Result<(), CronError> {
    remove_with(&CommandCrontab)
}

/// Inspect whether the managed cron block exists and matches configuration.
pub fn status(params: &CronParams) -> Result<CronStatus, CronError> {
    status_with(params, &CommandCrontab)
}

fn safe_cron_token<'a>(path: &'a Path, name: &'static str) -> Result<&'a str, CronError> {
    let text = path.to_str().ok_or_else(|| CronError::UnsafePath {
        name,
        path: path.to_path_buf(),
    })?;
    let safe = path.is_absolute()
        && !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._+-".contains(&byte));
    if safe {
        Ok(text)
    } else {
        Err(CronError::UnsafePath {
            name,
            path: path.to_path_buf(),
        })
    }
}

#[derive(Debug)]
struct ManagedBlock {
    range: Range<usize>,
    stale: bool,
}

fn inspect_managed_block(
    crontab: &str,
    expected: Option<&str>,
) -> Result<Option<ManagedBlock>, CronError> {
    let mut begins = Vec::new();
    let mut ends = Vec::new();
    let mut offset = 0;

    for line in crontab.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content == BEGIN_MARKER {
            begins.push(offset);
        }
        if content == END_MARKER {
            ends.push(offset + line.len());
        }
        offset += line.len();
    }

    if !crontab.is_empty() && !crontab.ends_with('\n') {
        let line_start = crontab.rfind('\n').map_or(0, |index| index + 1);
        let content = &crontab[line_start..];
        if content == BEGIN_MARKER && !begins.contains(&line_start) {
            begins.push(line_start);
        }
        if content == END_MARKER && !ends.contains(&crontab.len()) {
            ends.push(crontab.len());
        }
    }

    if begins.is_empty() && ends.is_empty() {
        return Ok(None);
    }
    if begins.len() != 1 || ends.len() != 1 || begins[0] >= ends[0] {
        return Err(CronError::AmbiguousManagedBlock(
            "expected exactly one ordered marker pair".to_string(),
        ));
    }

    let range = begins[0]..ends[0];
    let block = &crontab[range.clone()];
    if !block.lines().any(|line| line == OWNER_MARKER) {
        return Err(CronError::AmbiguousManagedBlock(
            "ownership line is missing".to_string(),
        ));
    }

    Ok(Some(ManagedBlock {
        range,
        stale: expected.is_some_and(|expected| block != expected),
    }))
}

fn install_with(params: &CronParams, runner: &impl CrontabRunner) -> Result<(), CronError> {
    let expected = generate_managed_block(params)?;
    let current = runner.list()?.unwrap_or_default();
    let inspected = inspect_managed_block(&current, Some(&expected))?;

    if inspected.as_ref().is_some_and(|block| !block.stale) {
        return Ok(());
    }

    let replacement = if let Some(block) = inspected {
        let mut text = current;
        text.replace_range(block.range, &expected);
        text
    } else {
        let mut text = current;
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&expected);
        text
    };

    runner.replace(&replacement)
}

fn remove_with(runner: &impl CrontabRunner) -> Result<(), CronError> {
    let Some(current) = runner.list()? else {
        return Ok(());
    };
    let Some(block) = inspect_managed_block(&current, None)? else {
        return Ok(());
    };

    let mut replacement = current;
    replacement.replace_range(block.range, "");
    runner.replace(&replacement)
}

fn status_with(params: &CronParams, runner: &impl CrontabRunner) -> Result<CronStatus, CronError> {
    let expected = generate_managed_block(params)?;
    let Some(current) = runner.list()? else {
        return Ok(CronStatus::NotInstalled);
    };

    Ok(match inspect_managed_block(&current, Some(&expected))? {
        Some(block) => CronStatus::Installed { stale: block.stale },
        None => CronStatus::NotInstalled,
    })
}

trait CrontabRunner {
    fn list(&self) -> Result<Option<String>, CronError>;
    fn replace(&self, content: &str) -> Result<(), CronError>;
}

struct CommandCrontab;

impl CrontabRunner for CommandCrontab {
    fn list(&self) -> Result<Option<String>, CronError> {
        let output = Command::new("crontab")
            .arg("-l")
            .stdin(Stdio::null())
            .output()
            .map_err(|source| CronError::Command {
                operation: "-l",
                source,
            })?;

        if output.status.success() {
            return String::from_utf8(output.stdout)
                .map(Some)
                .map_err(|_| CronError::NonUtf8);
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if output.status.code() == Some(1)
            && output.stdout.is_empty()
            && stderr.to_ascii_lowercase().contains("no crontab")
        {
            return Ok(None);
        }

        Err(CronError::CommandFailed {
            operation: "-l",
            status: output.status.code().unwrap_or(-1),
            stderr,
        })
    }

    fn replace(&self, content: &str) -> Result<(), CronError> {
        let mut child = Command::new("crontab")
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| CronError::Command {
                operation: "-",
                source,
            })?;

        child
            .stdin
            .take()
            .ok_or_else(|| CronError::Command {
                operation: "-",
                source: std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "crontab stdin unavailable",
                ),
            })?
            .write_all(content.as_bytes())
            .map_err(|source| CronError::Command {
                operation: "-",
                source,
            })?;

        let output = child
            .wait_with_output()
            .map_err(|source| CronError::Command {
                operation: "-",
                source,
            })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(CronError::CommandFailed {
                operation: "-",
                status: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            })
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/cron.rs"]
mod tests;
