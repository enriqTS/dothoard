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
mod tests {
    use std::fs;

    use super::*;
    use crate::backup::manifest::Manifest;
    use crate::config::SourceConfig;
    use crate::git::ownership::{ManifestSourceInfo, OwnedManifest, classify_ownership};

    const NAMESPACE: &str = "desktop";

    #[test]
    fn initializes_new_namespace_when_confirmed() {
        let tmp = tempfile::tempdir().unwrap();
        let state = OwnershipState::New;

        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, true).unwrap();

        assert_eq!(result, InitAction::Initialized);
        assert!(tmp.path().join(NAMESPACE).join("home").exists());
    }

    #[test]
    fn refuses_new_namespace_without_confirmation() {
        let tmp = tempfile::tempdir().unwrap();
        let state = OwnershipState::New;

        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, false);

        assert!(matches!(result, Err(InitError::ConfirmationRequired)));
        assert!(!tmp.path().join(NAMESPACE).join("home").exists());
    }

    #[test]
    fn attaches_to_owned_repository_when_confirmed() {
        let tmp = tempfile::tempdir().unwrap();

        // Create a valid manifest so the state is Owned.
        let manifest = Manifest::from_sources(&[SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        }]);
        fs::create_dir(tmp.path().join(NAMESPACE)).unwrap();
        manifest.save(&tmp.path().join(NAMESPACE)).unwrap();

        let state = classify_ownership(tmp.path(), NAMESPACE).unwrap();

        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, true).unwrap();
        assert_eq!(result, InitAction::Attached);
    }

    #[test]
    fn refuses_owned_repository_without_confirmation() {
        let state = OwnershipState::Owned {
            manifest: OwnedManifest {
                sources: vec![ManifestSourceInfo {
                    path: ".bashrc".to_string(),
                    ignore_count: 0,
                }],
            },
        };

        let tmp = tempfile::tempdir().unwrap();
        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, false);

        assert!(matches!(result, Err(InitError::ConfirmationRequired)));
    }

    #[test]
    fn refuses_invalid_manifest_regardless_of_confirmation() {
        let state = OwnershipState::InvalidManifest {
            reason: "unsupported version 99".to_string(),
        };

        let tmp = tempfile::tempdir().unwrap();

        // Even with confirmation, it refuses.
        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, true);
        assert!(matches!(result, Err(InitError::Refused { .. })));

        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, false);
        assert!(matches!(result, Err(InitError::Refused { .. })));
    }

    #[test]
    fn refuses_ambiguous_state_regardless_of_confirmation() {
        let state = OwnershipState::Ambiguous {
            reason: "home/ has data but no manifest".to_string(),
        };

        let tmp = tempfile::tempdir().unwrap();

        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, true);
        assert!(matches!(result, Err(InitError::Refused { .. })));

        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, false);
        assert!(matches!(result, Err(InitError::Refused { .. })));
    }

    #[test]
    fn initialization_creates_home_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let state = OwnershipState::New;

        initialize_or_attach(tmp.path(), NAMESPACE, &state, true).unwrap();

        let home = tmp.path().join(NAMESPACE).join("home");
        assert!(home.is_dir());
    }

    #[test]
    fn initialization_leaves_root_and_sibling_namespaces_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("home")).unwrap();
        fs::write(tmp.path().join("home/legacy"), "legacy").unwrap();
        fs::create_dir_all(tmp.path().join("notebook/home")).unwrap();
        fs::write(tmp.path().join("notebook/home/settings"), "sibling").unwrap();

        initialize_or_attach(tmp.path(), NAMESPACE, &OwnershipState::New, true).unwrap();

        assert_eq!(
            fs::read_to_string(tmp.path().join("home/legacy")).unwrap(),
            "legacy"
        );
        assert_eq!(
            fs::read_to_string(tmp.path().join("notebook/home/settings")).unwrap(),
            "sibling"
        );
        assert!(tmp.path().join(NAMESPACE).join("home").is_dir());
    }

    #[test]
    fn initialization_is_idempotent_for_existing_home_dir() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join(NAMESPACE).join("home")).unwrap();
        let state = OwnershipState::New;

        // Should not fail if home/ already exists (empty).
        let result = initialize_or_attach(tmp.path(), NAMESPACE, &state, true);
        assert!(result.is_ok());
    }

    #[test]
    fn require_usable_state_accepts_new() {
        let state = OwnershipState::New;
        assert!(require_usable_state(&state).is_ok());
    }

    #[test]
    fn require_usable_state_accepts_owned() {
        let state = OwnershipState::Owned {
            manifest: OwnedManifest { sources: vec![] },
        };
        assert!(require_usable_state(&state).is_ok());
    }

    #[test]
    fn require_usable_state_rejects_invalid_manifest() {
        let state = OwnershipState::InvalidManifest {
            reason: "bad format".to_string(),
        };
        assert!(matches!(
            require_usable_state(&state),
            Err(InitError::Refused { .. })
        ));
    }

    #[test]
    fn require_usable_state_rejects_ambiguous() {
        let state = OwnershipState::Ambiguous {
            reason: "orphaned data".to_string(),
        };
        assert!(matches!(
            require_usable_state(&state),
            Err(InitError::Refused { .. })
        ));
    }

    #[test]
    fn init_action_display() {
        assert_eq!(
            InitAction::Initialized.to_string(),
            "initialized new namespace"
        );
        assert_eq!(
            InitAction::Attached.to_string(),
            "attached to existing manifest"
        );
    }

    #[test]
    fn refused_error_contains_reason() {
        let state = OwnershipState::InvalidManifest {
            reason: "version 42 not supported".to_string(),
        };
        let tmp = tempfile::tempdir().unwrap();

        let err = initialize_or_attach(tmp.path(), NAMESPACE, &state, true).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("version 42 not supported"));
    }
}
