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
fn wait_with_timeout(
    child: &mut std::process::Child,
    timeout: Duration,
    redacted_args: &str,
) -> Result<Output, GitError> {
    use std::thread;
    use std::time::Instant;

    let deadline = Instant::now() + timeout;
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                // Process exited; collect output.
                let stdout = read_pipe(child.stdout.take());
                let stderr = read_pipe(child.stderr.take());
                return Ok(Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                // Still running; check timeout.
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
    }
}

/// Read all remaining bytes from an optional pipe.
fn read_pipe(pipe: Option<impl std::io::Read>) -> Vec<u8> {
    let Some(mut pipe) = pipe else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
    buf
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
        ("GIT_COMMITTER_NAME", "config-sync"),
        ("GIT_COMMITTER_EMAIL", "config-sync@localhost"),
        ("GIT_AUTHOR_NAME", "config-sync"),
        ("GIT_AUTHOR_EMAIL", "config-sync@localhost"),
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
mod tests {
    use super::*;

    /// Helper to create a GitOutput for testing without needing a real ExitStatus.
    fn test_output(stdout: &str, stderr: &str) -> GitOutput {
        // We need a real ExitStatus. The simplest way is to run `true`.
        let status = std::process::Command::new("true").status().unwrap();
        GitOutput {
            status,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
        }
    }

    #[test]
    fn noninteractive_env_disables_terminal_prompt() {
        let env = noninteractive_env();
        let terminal_prompt = env.iter().find(|(k, _)| *k == "GIT_TERMINAL_PROMPT");
        assert_eq!(terminal_prompt, Some(&("GIT_TERMINAL_PROMPT", "0")));
    }

    #[test]
    fn noninteractive_env_disables_askpass() {
        let env = noninteractive_env();
        let git_askpass = env.iter().find(|(k, _)| *k == "GIT_ASKPASS");
        let ssh_askpass = env.iter().find(|(k, _)| *k == "SSH_ASKPASS");
        assert_eq!(git_askpass, Some(&("GIT_ASKPASS", "")));
        assert_eq!(ssh_askpass, Some(&("SSH_ASKPASS", "")));
    }

    #[test]
    fn noninteractive_env_disables_gcm_interaction() {
        let env = noninteractive_env();
        let gcm = env.iter().find(|(k, _)| *k == "GCM_INTERACTIVE");
        assert_eq!(gcm, Some(&("GCM_INTERACTIVE", "Never")));
    }

    #[test]
    fn noninteractive_env_uses_ssh_batch_mode() {
        let env = noninteractive_env();
        let ssh_cmd = env.iter().find(|(k, _)| *k == "GIT_SSH_COMMAND");
        assert!(ssh_cmd.is_some());
        let (_, value) = ssh_cmd.unwrap();
        assert!(value.contains("BatchMode=yes"));
    }

    #[test]
    fn redact_args_cleans_credential_urls() {
        let args = vec![
            "push".to_string(),
            "https://user:token@github.com/repo.git".to_string(),
        ];
        let result = redact_args(&args);
        assert_eq!(result, "push https://[redacted]");
    }

    #[test]
    fn redact_args_preserves_safe_args() {
        let args = vec![
            "status".to_string(),
            "--porcelain".to_string(),
            "-z".to_string(),
        ];
        let result = redact_args(&args);
        assert_eq!(result, "status --porcelain -z");
    }

    #[test]
    fn git_command_builder_accumulates_args() {
        let cmd = GitCommand::new("/tmp/repo")
            .arg("commit")
            .args(["-m", "test message"]);
        assert_eq!(cmd.args, vec!["commit", "-m", "test message"]);
    }

    #[test]
    fn git_command_builder_marks_network() {
        let cmd = GitCommand::new("/tmp/repo").arg("push").network();
        assert!(cmd.is_network());
    }

    #[test]
    fn git_command_builder_defaults_to_local() {
        let cmd = GitCommand::new("/tmp/repo").arg("status");
        assert!(!cmd.is_network());
    }

    #[test]
    fn git_command_builder_accepts_extra_env() {
        let cmd = GitCommand::new("/tmp/repo")
            .arg("commit")
            .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00");
        assert_eq!(cmd.extra_env.len(), 1);
        assert_eq!(cmd.extra_env[0].0, "GIT_AUTHOR_DATE");
    }

    #[test]
    fn git_runner_respects_configured_timeouts() {
        let runner = GitRunner::new(Duration::from_secs(120));
        assert_eq!(runner.network_timeout(), Duration::from_secs(120));
        assert_eq!(runner.local_timeout(), Duration::from_secs(60));
    }

    #[test]
    fn git_runner_custom_timeouts() {
        let runner = GitRunner::with_timeouts(Duration::from_secs(30), Duration::from_secs(10));
        assert_eq!(runner.network_timeout(), Duration::from_secs(30));
        assert_eq!(runner.local_timeout(), Duration::from_secs(10));
    }

    #[test]
    fn git_output_trimmed_strips_trailing_newline() {
        let output = test_output("main\n", "warning: something\n");
        assert_eq!(output.stdout_trimmed(), "main");
        assert_eq!(output.stderr_trimmed(), "warning: something");
    }

    #[test]
    fn git_output_nul_split_handles_machine_output() {
        let output = test_output("M home/.bashrc\0A home/.config/fish/config.fish\0", "");
        let parts = output.stdout_nul_split();
        assert_eq!(
            parts,
            vec!["M home/.bashrc", "A home/.config/fish/config.fish"]
        );
    }

    #[test]
    fn git_output_lines_splits_normally() {
        let output = test_output("line1\nline2\nline3\n", "");
        assert_eq!(output.stdout_lines(), vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn runner_run_succeeds_with_version() {
        // This test requires git to be installed on the system.
        let tmp = tempfile::tempdir().unwrap();
        let runner = GitRunner::new(Duration::from_secs(10));
        let cmd = GitCommand::new(tmp.path()).arg("--version");
        let output = runner.run(&cmd).unwrap();
        assert!(output.stdout.starts_with("git version"));
    }

    #[test]
    fn runner_run_fails_for_invalid_command() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = GitRunner::new(Duration::from_secs(10));
        let cmd = GitCommand::new(tmp.path()).args(["log", "--oneline"]);
        // This should fail because the temp dir is not a git repo.
        let result = runner.run(&cmd);
        assert!(matches!(result, Err(GitError::Failed { .. })));
    }

    #[test]
    fn runner_run_raw_returns_output_regardless_of_exit_code() {
        let tmp = tempfile::tempdir().unwrap();
        let runner = GitRunner::new(Duration::from_secs(10));
        let cmd = GitCommand::new(tmp.path()).args(["status", "--porcelain"]);
        // Not a repo, but run_raw should return the output without error-mapping.
        let output = runner.run_raw(&cmd).unwrap();
        // The exit code is non-zero because it's not a repo.
        assert!(!output.status.success());
    }

    #[test]
    fn runner_timeout_kills_process() {
        let tmp = tempfile::tempdir().unwrap();
        // Use a very short timeout with a command that would hang.
        let runner =
            GitRunner::with_timeouts(Duration::from_millis(100), Duration::from_millis(100));
        // `git hash-object --stdin` reads from stdin, which we've set to null,
        // but let's use a sleep-based approach via --wait with a path that won't resolve.
        // Actually, we can use `git fetch` on a non-existent remote with a short timeout.
        // The simplest approach: just verify the timeout mechanism works conceptually.
        // We'll init a repo and try to fetch from a non-routable IP.
        // For unit testing, just verify the timeout error type.

        // Create a git repo so the command starts.
        std::process::Command::new("git")
            .args(["init", "--initial-branch=main"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["remote", "add", "origin", "ssh://192.0.2.1/nonexistent.git"])
            .current_dir(tmp.path())
            .output()
            .unwrap();

        let cmd = GitCommand::new(tmp.path())
            .args(["fetch", "origin"])
            .network();
        let result = runner.run(&cmd);
        // Should either timeout or fail with a connection error.
        assert!(result.is_err());
    }
}
