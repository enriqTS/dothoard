//! Core Git command runner with safety guarantees.
//!
//! The runner executes `git` as a direct subprocess with argument arrays,
//! never through a shell. Environment variables are controlled explicitly
//! to prevent interactive prompts and credential leaks.

use std::path::PathBuf;
use std::process::{Command, ExitStatus, Output, Stdio};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

use thiserror::Error;

use crate::diagnostics;

/// Errors that can occur during Git command execution.
#[derive(Debug, Error)]
pub enum GitError {
    /// The `git` binary could not be spawned.
    #[error("failed to spawn git: {source}")]
    Spawn {
        #[source]
        source: std::io::Error,
    },

    /// The command exceeded its configured timeout and was killed.
    #[error("git command timed out after {timeout:?}: git {args}")]
    Timeout { timeout: Duration, args: String },

    /// The command exited with a non-zero status.
    #[error("git {args} failed with exit code {code}: {stderr}")]
    Failed {
        args: String,
        code: i32,
        stdout: String,
        stderr: String,
    },

    /// The command was terminated by a signal without an exit code.
    #[error("git {args} was terminated by signal: {stderr}")]
    Signal { args: String, stderr: String },

    /// Failed to wait on the child process.
    #[error("failed to wait on git process: {source}")]
    Wait {
        #[source]
        source: std::io::Error,
    },

    /// Failed to kill a timed-out process tree.
    #[error("failed to kill git process tree: {source}")]
    Kill {
        #[source]
        source: std::io::Error,
    },
}

/// The captured output of a successful Git command.
#[derive(Debug, Clone)]
pub struct GitOutput {
    /// The process exit status.
    pub status: ExitStatus,
    /// Captured stdout, decoded as lossy UTF-8.
    pub stdout: String,
    /// Captured stderr, decoded as lossy UTF-8.
    pub stderr: String,
}

impl GitOutput {
    /// Returns stdout with trailing newline stripped.
    pub fn stdout_trimmed(&self) -> &str {
        self.stdout.trim_end()
    }

    /// Returns stderr with trailing newline stripped.
    pub fn stderr_trimmed(&self) -> &str {
        self.stderr.trim_end()
    }

    /// Split stdout into lines, stripping trailing whitespace.
    pub fn stdout_lines(&self) -> Vec<&str> {
        self.stdout.lines().collect()
    }

    /// Split stdout by NUL bytes for machine-readable output.
    pub fn stdout_nul_split(&self) -> Vec<&str> {
        self.stdout.split('\0').filter(|s| !s.is_empty()).collect()
    }
}

/// A builder for constructing a Git command with controlled arguments and
/// environment.
#[derive(Debug, Clone)]
pub struct GitCommand {
    /// Working directory for the command.
    work_dir: PathBuf,
    /// The git subcommand and arguments.
    args: Vec<String>,
    /// Additional environment variables to set beyond the noninteractive base.
    extra_env: Vec<(String, String)>,
    /// Whether this is a network-facing command that should use the full
    /// network timeout (as opposed to a local-only command).
    network: bool,
}

impl GitCommand {
    /// Create a new Git command builder for the given working directory.
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
            args: Vec::new(),
            extra_env: Vec::new(),
            network: false,
        }
    }

    /// Append one or more arguments.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Append a single argument.
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set an additional environment variable.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_env.push((key.into(), value.into()));
        self
    }

    /// Mark this command as network-facing (uses the full network timeout).
    pub fn network(mut self) -> Self {
        self.network = true;
        self
    }

    /// Returns whether this is a network-facing command.
    pub fn is_network(&self) -> bool {
        self.network
    }

    /// Returns the arguments as a display string for logging (redacted).
    #[allow(dead_code)]
    pub(crate) fn display_args(&self) -> String {
        self.args.join(" ")
    }
}

/// The Git command runner that enforces safety invariants on every execution.
///
/// It holds configuration that applies across all commands: the timeout for
/// network-facing operations and the timeout for local operations.
#[derive(Debug, Clone)]
pub struct GitRunner {
    /// Timeout for network-facing git commands (fetch, push, pull, ls-remote).
    network_timeout: Duration,
    /// Timeout for local-only git commands (status, diff, commit, etc.).
    local_timeout: Duration,
}

impl GitRunner {
    /// Create a new runner with the given network timeout.
    ///
    /// Local commands use a generous but bounded timeout to prevent hangs
    /// from unexpected conditions (e.g., repository corruption).
    pub fn new(network_timeout: Duration) -> Self {
        Self {
            network_timeout,
            // Local commands should complete quickly; 60s is generous.
            local_timeout: Duration::from_secs(60),
        }
    }

    /// Create a runner with explicit timeouts for both network and local ops.
    pub fn with_timeouts(network_timeout: Duration, local_timeout: Duration) -> Self {
        Self {
            network_timeout,
            local_timeout,
        }
    }

    /// Execute a Git command, enforcing noninteractive environment,
    /// timeout, and process-tree cleanup.
    ///
    /// On success (exit code 0), returns `GitOutput`. On failure, returns
    /// a typed `GitError` with redacted output.
    pub fn run(&self, cmd: &GitCommand) -> Result<GitOutput, GitError> {
        let timeout = if cmd.is_network() {
            self.network_timeout
        } else {
            self.local_timeout
        };

        let redacted_args = redact_args(&cmd.args);
        tracing::debug!(
            work_dir = %cmd.work_dir.display(),
            args = %redacted_args,
            timeout_secs = timeout.as_secs(),
            network = cmd.is_network(),
            "executing git command"
        );

        let mut process = Command::new("git");
        process
            .current_dir(&cmd.work_dir)
            .args(&cmd.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Clear environment and set only controlled variables.
        process.env_clear();
        for (key, value) in noninteractive_env() {
            process.env(key, value);
        }
        // Inherit PATH so git can find itself and its helpers.
        if let Ok(path) = std::env::var("PATH") {
            process.env("PATH", &path);
        }
        // Inherit HOME for .gitconfig resolution.
        if let Ok(home) = std::env::var("HOME") {
            process.env("HOME", &home);
        }
        // Inherit XDG_CONFIG_HOME for git config in non-default locations.
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            process.env("XDG_CONFIG_HOME", &xdg);
        }
        // Inherit SSH_AUTH_SOCK for SSH agent authentication.
        if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
            process.env("SSH_AUTH_SOCK", &sock);
        }
        // Inherit DBUS_SESSION_BUS_ADDRESS so credential helpers that use
        // D-Bus (e.g., GCM with secretservice, libsecret, GNOME Keyring)
        // can reach the session keyring when running under a user systemd
        // service.
        if let Ok(dbus) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
            process.env("DBUS_SESSION_BUS_ADDRESS", &dbus);
        }
        // Inherit display variables so credential helpers that check for a
        // graphical session (GCM secretservice, libsecret) can function.
        if let Ok(display) = std::env::var("DISPLAY") {
            process.env("DISPLAY", &display);
        }
        if let Ok(wayland) = std::env::var("WAYLAND_DISPLAY") {
            process.env("WAYLAND_DISPLAY", &wayland);
        }
        // Inherit XDG_RUNTIME_DIR for Wayland socket and credential daemon
        // socket access.
        if let Ok(xrd) = std::env::var("XDG_RUNTIME_DIR") {
            process.env("XDG_RUNTIME_DIR", &xrd);
        }
        // Apply any extra environment variables from the command builder.
        for (key, value) in &cmd.extra_env {
            process.env(key, value);
        }

        // Start a new process group so we can kill the tree on timeout.
        #[cfg(unix)]
        unsafe {
            process.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        let mut child = process
            .spawn()
            .map_err(|source| GitError::Spawn { source })?;

        // Wait with timeout.
        let output = wait_with_timeout(&mut child, timeout, &redacted_args)?;

        let git_output = GitOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };

        if output.status.success() {
            tracing::trace!(
                args = %redacted_args,
                stdout_len = git_output.stdout.len(),
                stderr_len = git_output.stderr.len(),
                "git command succeeded"
            );
            Ok(git_output)
        } else {
            let code = output.status.code();
            let redacted_stderr =
                diagnostics::redact_sensitive_text(&git_output.stderr).into_owned();

            tracing::debug!(
                args = %redacted_args,
                code = ?code,
                stderr = %redacted_stderr,
                "git command failed"
            );

            match code {
                Some(code) => Err(GitError::Failed {
                    args: redacted_args,
                    code,
                    stdout: diagnostics::redact_sensitive_text(&git_output.stdout).into_owned(),
                    stderr: redacted_stderr,
                }),
                None => Err(GitError::Signal {
                    args: redacted_args,
                    stderr: redacted_stderr,
                }),
            }
        }
    }

    /// Execute a Git command, returning the raw output regardless of exit code.
    ///
    /// Use this when you need to inspect the exit code yourself (e.g.,
    /// `git diff --cached --quiet` uses exit 1 to mean "there are changes").
    pub fn run_raw(&self, cmd: &GitCommand) -> Result<GitOutput, GitError> {
        let timeout = if cmd.is_network() {
            self.network_timeout
        } else {
            self.local_timeout
        };

        let redacted_args = redact_args(&cmd.args);
        tracing::debug!(
            work_dir = %cmd.work_dir.display(),
            args = %redacted_args,
            timeout_secs = timeout.as_secs(),
            network = cmd.is_network(),
            "executing git command (raw)"
        );

        let mut process = Command::new("git");
        process
            .current_dir(&cmd.work_dir)
            .args(&cmd.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        process.env_clear();
        for (key, value) in noninteractive_env() {
            process.env(key, value);
        }
        if let Ok(path) = std::env::var("PATH") {
            process.env("PATH", &path);
        }
        if let Ok(home) = std::env::var("HOME") {
            process.env("HOME", &home);
        }
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            process.env("XDG_CONFIG_HOME", &xdg);
        }
        if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
            process.env("SSH_AUTH_SOCK", &sock);
        }
        if let Ok(dbus) = std::env::var("DBUS_SESSION_BUS_ADDRESS") {
            process.env("DBUS_SESSION_BUS_ADDRESS", &dbus);
        }
        if let Ok(display) = std::env::var("DISPLAY") {
            process.env("DISPLAY", &display);
        }
        if let Ok(wayland) = std::env::var("WAYLAND_DISPLAY") {
            process.env("WAYLAND_DISPLAY", &wayland);
        }
        if let Ok(xrd) = std::env::var("XDG_RUNTIME_DIR") {
            process.env("XDG_RUNTIME_DIR", &xrd);
        }
        for (key, value) in &cmd.extra_env {
            process.env(key, value);
        }

        #[cfg(unix)]
        unsafe {
            process.pre_exec(|| {
                libc::setpgid(0, 0);
                Ok(())
            });
        }

        let mut child = process
            .spawn()
            .map_err(|source| GitError::Spawn { source })?;
        let output = wait_with_timeout(&mut child, timeout, &redacted_args)?;

        Ok(GitOutput {
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    /// Returns the configured network timeout.
    pub fn network_timeout(&self) -> Duration {
        self.network_timeout
    }

    /// Returns the configured local timeout.
    pub fn local_timeout(&self) -> Duration {
        self.local_timeout
    }
}

/// Wait for a child process with a timeout, killing the process group on
/// expiry.
///
/// This spawns reader threads for stdout and stderr to prevent pipe deadlocks
/// (where the child blocks on write because the pipe buffer is full, and we
/// block on wait because the child hasn't exited).
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    redacted_args: &str,
) -> Result<Output, GitError> {
    use std::thread;
    use std::time::Instant;

    // Take ownership of the pipes and read them in background threads to
    // prevent pipe-buffer deadlocks.
    let stdout_handle = child.stdout.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
            buf
        })
    });
    let stderr_handle = child.stderr.take().map(|mut pipe| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
            buf
        })
    });

    let deadline = Instant::now() + timeout;
    let poll_interval = Duration::from_millis(50);

    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if Instant::now() >= deadline {
                    kill_process_tree(child);
                    return Err(GitError::Timeout {
                        timeout,
                        args: redacted_args.to_string(),
                    });
                }
                thread::sleep(poll_interval);
            }
            Err(source) => return Err(GitError::Wait { source }),
        }
    };

    let stdout = stdout_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();

    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

/// Kill the entire process group of a child process.
///
/// On Unix, the child is started in its own process group (via `setpgid`),
/// so killing the group terminates all descendant processes (e.g., SSH
/// transport helpers spawned by git).
fn kill_process_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let pid = child.id() as libc::pid_t;
        // Send SIGTERM to the process group (negative PID).
        unsafe {
            libc::kill(-pid, libc::SIGTERM);
        }
        // Give processes a brief moment to exit gracefully.
        std::thread::sleep(Duration::from_millis(200));
        // Force kill if still alive.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }

    // Ensure the child is reaped regardless.
    let _ = child.wait();
}

/// Returns the base environment variables that ensure noninteractive execution.
///
/// These prevent git and its helpers from prompting for passwords, passphrases,
/// host-key confirmation, or credential-manager interaction.
fn noninteractive_env() -> Vec<(&'static str, &'static str)> {
    vec![
        // Prevent git from opening a terminal for prompts.
        ("GIT_TERMINAL_PROMPT", "0"),
        // Disable the askpass helper (no GUI password dialogs).
        ("GIT_ASKPASS", ""),
        ("SSH_ASKPASS", ""),
        // Prevent SSH_ASKPASS from being used even if set.
        ("SSH_ASKPASS_REQUIRE", "never"),
        // Disable Git Credential Manager interactive mode.
        ("GCM_INTERACTIVE", "Never"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        // Use SSH batch mode for standard remotes (no prompts for
        // password, passphrase, or host-key confirmation).
        (
            "GIT_SSH_COMMAND",
            "ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new",
        ),
        // Disable any pager.
        ("GIT_PAGER", "cat"),
        // Ensure consistent output regardless of locale.
        ("LC_ALL", "C"),
        // Disable GPG signing by default for automated commits.
        ("GIT_COMMITTER_NAME", "dothoard"),
        ("GIT_COMMITTER_EMAIL", "dothoard@localhost"),
        ("GIT_AUTHOR_NAME", "dothoard"),
        ("GIT_AUTHOR_EMAIL", "dothoard@localhost"),
    ]
}

/// Redact arguments that might contain credential-bearing URLs.
fn redact_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| diagnostics::redact_remote_url(arg).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "../../tests/unit/git/runner.rs"]
mod tests;
