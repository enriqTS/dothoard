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

mod init;
mod ownership;
mod repository;
mod runner;
mod staging;
mod worktree;

pub use init::{InitAction, InitError, initialize_or_attach, require_usable_state};
pub use ownership::{
    ManifestSourceInfo, OwnedManifest, OwnershipError, OwnershipState, classify_ownership,
};
pub use repository::{BlockingOperation, RepositoryError, RepositoryInfo, validate_repository};
pub use runner::{GitCommand, GitError, GitOutput, GitRunner};
pub use staging::{
    StagingError, has_staged_changes, stage_managed_namespace, verify_staged_boundaries,
};
pub use worktree::{WorktreeError, WorktreeStatus, classify_worktree};
