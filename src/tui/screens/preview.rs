//! Backup preview screen state.
//!
//! Shows the dry-run output of the backup planner: what would be added,
//! modified, deleted, excluded, and any warnings.

use std::path::Path;

use crate::backup::changeset::ChangeSet;
use crate::backup::planner::{PlanInputs, plan_backup};
use crate::config::Config;
use crate::tui::task::LoadState;

/// The state of the backup preview screen.
#[derive(Debug)]
pub struct PreviewScreen {
    /// Lifecycle and last usable backup preview.
    pub load_state: LoadState<PreviewData>,
    /// Scroll offset for viewing long lists.
    pub scroll: usize,
}

/// Processed preview data ready for display.
#[derive(Debug, Clone)]
pub struct PreviewData {
    /// Summary counts.
    pub additions: usize,
    pub modifications: usize,
    pub deletions: usize,
    pub exclusions: usize,
    pub warnings: usize,
    /// Flattened list of entries for display.
    pub entries: Vec<PreviewEntry>,
}

/// A single entry in the preview display.
#[derive(Debug, Clone)]
pub struct PreviewEntry {
    /// The kind of change.
    pub kind: EntryKind,
    /// Display path (relative to repository/home/).
    pub path: String,
    /// Additional detail (e.g., "content changed", "newly ignored").
    pub detail: Option<String>,
}

/// Kind of preview entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Addition,
    Modification,
    Deletion,
    Exclusion,
    Warning,
}

impl EntryKind {
    /// Single-character prefix for display.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Addition => "+",
            Self::Modification => "~",
            Self::Deletion => "-",
            Self::Exclusion => "i",
            Self::Warning => "!",
        }
    }
}

impl Default for PreviewScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewScreen {
    pub fn new() -> Self {
        Self {
            load_state: LoadState::NotLoaded,
            scroll: 0,
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Action {
        use crossterm::event::KeyCode;

        match key.code {
            // Refresh preview.
            KeyCode::Char('r') => Action::Refresh,
            // Run backup.
            KeyCode::Char('b') => Action::RunBackup,
            // Push pending commits to remote.
            KeyCode::Char('p') => Action::Push,
            // Scroll.
            KeyCode::Up | KeyCode::Char('k') => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                    Action::Consumed
                } else {
                    // At upper boundary — let parent handle focus return.
                    Action::NotConsumed
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.scroll += 1;
                Action::Consumed
            }
            KeyCode::Home => {
                self.scroll = 0;
                Action::Consumed
            }
            _ => Action::NotConsumed,
        }
    }

    /// Generate a preview from an immutable configuration snapshot.
    ///
    /// This helper performs filesystem reads and must run on a background
    /// worker when called by the TUI.
    pub fn generate(
        config: &Config,
        home: &Path,
        repository: &Path,
    ) -> Result<PreviewData, String> {
        let inputs = PlanInputs {
            home,
            repository,
            namespace: &config.namespace,
            sources: &config.sources,
        };

        let changeset = plan_backup(&inputs).map_err(|e| format!("Planning failed: {e}"))?;

        Ok(Self::changeset_to_preview(
            &changeset,
            repository,
            &config.namespace,
        ))
    }

    /// Convert a ChangeSet into display-ready PreviewData.
    fn changeset_to_preview(cs: &ChangeSet, repository: &Path, namespace: &str) -> PreviewData {
        let mut entries = Vec::new();
        let home_prefix = crate::backup::mapping::managed_home_dir(repository, namespace);

        // Additions.
        for a in &cs.additions {
            let rel = a
                .destination
                .strip_prefix(&home_prefix)
                .unwrap_or(&a.destination);
            entries.push(PreviewEntry {
                kind: EntryKind::Addition,
                path: rel.to_string_lossy().to_string(),
                detail: Some(format!("{}", a.entry_type)),
            });
        }

        // Modifications.
        for m in &cs.modifications {
            let rel = m
                .destination
                .strip_prefix(&home_prefix)
                .unwrap_or(&m.destination);
            entries.push(PreviewEntry {
                kind: EntryKind::Modification,
                path: rel.to_string_lossy().to_string(),
                detail: Some(format!("{}", m.change)),
            });
        }

        // Deletions.
        for d in &cs.deletions {
            let rel = d
                .destination
                .strip_prefix(&home_prefix)
                .unwrap_or(&d.destination);
            entries.push(PreviewEntry {
                kind: EntryKind::Deletion,
                path: rel.to_string_lossy().to_string(),
                detail: Some(format!("{}", d.reason)),
            });
        }

        // Exclusions.
        for e in &cs.exclusions {
            let rel = e.source.strip_prefix(repository).unwrap_or(&e.source);
            entries.push(PreviewEntry {
                kind: EntryKind::Exclusion,
                path: rel.to_string_lossy().to_string(),
                detail: Some(format!("{}", e.reason)),
            });
        }

        // Warnings.
        for w in &cs.warnings {
            entries.push(PreviewEntry {
                kind: EntryKind::Warning,
                path: w.path.to_string_lossy().to_string(),
                detail: Some(format!("{}", w.kind)),
            });
        }

        PreviewData {
            additions: cs.additions.len(),
            modifications: cs.modifications.len(),
            deletions: cs.deletions.len(),
            exclusions: cs.exclusions.len(),
            warnings: cs.warnings.len(),
            entries,
        }
    }
}

/// Actions from the preview screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Consumed,
    NotConsumed,
    /// Refresh the preview.
    Refresh,
    /// Execute the backup.
    RunBackup,
    /// Push pending commits to remote.
    Push,
}

#[cfg(test)]
#[path = "../../../tests/unit/tui/screens/preview.rs"]
mod tests;
