//! Safe Git command execution and repository synchronization.
//!
//! This module provides a controlled interface for executing the installed `git`
//! binary directly with argument arrays. It enforces:
//!
//! - **Noninteractive execution**: `GIT_TERMINAL_PROMPT=0`, askpass disabled,
//!   `GCM_INTERACTIVE=Never`, and SSH batch mode prevent prompts from blocking
//!   background runs.
//! - **Controlled environment**: Only explicitly allowed environment variables
//!   are inherited, and credential-bearing values are never logged.
//! - **Redacted logging**: Remote URLs and command outputs containing credentials
//!   are redacted before reaching diagnostics.
//! - **Process-tree cleanup**: Timed-out commands terminate the full subprocess
//!   tree to prevent orphaned transport processes.
//! - **Timeout enforcement**: A configurable deadline prevents Git transport
//!   operations from blocking indefinitely.

mod repository;
mod runner;

pub use repository::{BlockingOperation, RepositoryError, RepositoryInfo, validate_repository};
pub use runner::{GitCommand, GitError, GitOutput, GitRunner};
