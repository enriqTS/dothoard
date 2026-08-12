//! Repository initialization and attachment.
//!
//! This module implements the rules for claiming a repository namespace:
//!
//! - **Initialize**: When the ownership state is [`OwnershipState::New`], create
//!   the `home/` directory. Requires explicit confirmation.
//! - **Attach**: When the ownership state is [`OwnershipState::Owned`], verify
//!   the manifest is compatible and allow the application to use the repository.
//!   Requires explicit confirmation after review.
//! - **Refuse**: When the ownership state is [`OwnershipState::InvalidManifest`]
//!   or [`OwnershipState::Ambiguous`], always refuse.
//!
//! The caller (TUI or CLI) is responsible for presenting the user with the
//! appropriate information and obtaining their confirmation before calling
//! these functions.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::backup::mapping;

use super::ownership::OwnershipState;

/// The action that was performed to claim the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitAction {
    /// A new namespace was initialized (home/ directory created).
    Initialized,
    /// The application attached to an existing valid manifest.
    Attached,
}

impl std::fmt::Display for InitAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initialized => write!(f, "initialized new namespace"),
            Self::Attached => write!(f, "attached to existing manifest"),
        }
    }
}

/// Errors from initialization and attachment operations.
#[derive(Debug, Error)]
pub enum InitError {
    /// The repository cannot be initialized because it requires confirmation.
    #[error("initialization requires explicit confirmation")]
    ConfirmationRequired,

    /// The repository cannot be initialized because the ownership state is invalid.
    #[error("cannot initialize repository: {reason}")]
    Refused { reason: String },

    /// Failed to create the managed home directory.
    #[error("failed to create managed directory {path}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Attempt to initialize or attach to a repository based on its ownership state.
///
/// # Arguments
///
/// * `repository` - Absolute path to the repository root.
/// * `namespace` - The selected machine namespace to initialize.
/// * `state` - The classified ownership state (from [`classify_ownership`]).
/// * `confirmed` - Whether the user has explicitly confirmed the action.
///
/// # Returns
///
/// * `Ok(InitAction::Initialized)` if a new namespace was created.
/// * `Ok(InitAction::Attached)` if attaching to an existing valid manifest.
/// * `Err(InitError::ConfirmationRequired)` if `confirmed` is false.
/// * `Err(InitError::Refused)` if the ownership state prevents initialization.
///
/// [`classify_ownership`]: super::ownership::classify_ownership
pub fn initialize_or_attach(
    repository: &Path,
    namespace: &str,
    state: &OwnershipState,
    confirmed: bool,
) -> Result<InitAction, InitError> {
    match state {
        OwnershipState::New => {
            if !confirmed {
                return Err(InitError::ConfirmationRequired);
            }
            create_managed_namespace(repository, namespace)?;
            Ok(InitAction::Initialized)
        }

        OwnershipState::Owned { .. } => {
            if !confirmed {
                return Err(InitError::ConfirmationRequired);
            }
            // The manifest is already valid; nothing to create.
            Ok(InitAction::Attached)
        }

        OwnershipState::InvalidManifest { reason } => Err(InitError::Refused {
            reason: format!("manifest is invalid: {reason}"),
        }),

        OwnershipState::Ambiguous { reason } => Err(InitError::Refused {
            reason: format!("repository content is ambiguous: {reason}"),
        }),
    }
}

/// Create the selected namespace's managed `home/` directory.
///
/// This is the only filesystem mutation performed during initialization.
/// The manifest will be created later by the mirror executor during the
/// first successful backup.
fn create_managed_namespace(repository: &Path, namespace: &str) -> Result<(), InitError> {
    let home_dir = repository.join(namespace).join(mapping::HOME_DIR_NAME);

    if !home_dir.exists() {
        fs::create_dir_all(&home_dir).map_err(|source| InitError::CreateDir {
            path: home_dir,
            source,
        })?;
    }

    Ok(())
}

/// Check whether the repository is ready for backup operations.
///
/// This is a convenience function that returns `Ok(())` if the ownership state
/// allows proceeding (New with confirmation already done, or Owned), and an
/// error otherwise.
pub fn require_usable_state(state: &OwnershipState) -> Result<(), InitError> {
    match state {
        OwnershipState::New | OwnershipState::Owned { .. } => Ok(()),
        OwnershipState::InvalidManifest { reason } => Err(InitError::Refused {
            reason: format!("manifest is invalid: {reason}"),
        }),
        OwnershipState::Ambiguous { reason } => Err(InitError::Refused {
            reason: format!("repository content is ambiguous: {reason}"),
        }),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/git/init.rs"]
mod tests;
