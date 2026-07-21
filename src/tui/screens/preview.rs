//! Backup preview screen state.
//!
//! Shows the dry-run output of the backup planner: what would be added,
//! modified, deleted, excluded, and any warnings.

use std::path::Path;

use crate::backup::changeset::ChangeSet;
use crate::backup::planner::{PlanInputs, plan_backup};
use crate::config::Config;

/// The state of the backup preview screen.
#[derive(Debug)]
pub struct PreviewScreen {
    /// The computed preview (if available).
    pub preview: Option<PreviewData>,
    /// Scroll offset for viewing long lists.
    pub scroll: usize,
    /// Error message if preview generation failed.
    pub error: Option<String>,
    /// Whether a refresh is needed.
    pub stale: bool,
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
            Self::Exclusion => "○",
            Self::Warning => "⚠",
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
            preview: None,
            scroll: 0,
            error: None,
            stale: true,
        }
    }

    /// Handle a key event.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Action {
        use crossterm::event::KeyCode;

        match key.code {
            // Refresh preview.
            KeyCode::Char('r') => {
                self.stale = true;
                Action::Refresh
            }
            // Scroll.
            KeyCode::Up | KeyCode::Char('k') => {
                if self.scroll > 0 {
                    self.scroll -= 1;
                }
                Action::Consumed
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

    /// Generate the preview from the current configuration.
    ///
    /// This runs the planner synchronously (it only reads the filesystem,
    /// does not modify anything).
    pub fn generate(
        config: &Config,
        home: &Path,
        repository: &Path,
    ) -> Result<PreviewData, String> {
        let inputs = PlanInputs {
            home,
            repository,
            sources: &config.sources,
        };

        let changeset = plan_backup(&inputs).map_err(|e| format!("Planning failed: {e}"))?;

        Ok(Self::changeset_to_preview(&changeset, repository))
    }

    /// Convert a ChangeSet into display-ready PreviewData.
    fn changeset_to_preview(cs: &ChangeSet, repository: &Path) -> PreviewData {
        let mut entries = Vec::new();
        let home_prefix = repository.join("home");

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::changeset::*;
    use std::path::PathBuf;

    #[test]
    fn new_screen_is_stale() {
        let screen = PreviewScreen::new();
        assert!(screen.stale);
        assert!(screen.preview.is_none());
    }

    #[test]
    fn r_triggers_refresh() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut screen = PreviewScreen::new();
        screen.stale = false;
        let action = screen.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(action, Action::Refresh);
        assert!(screen.stale);
    }

    #[test]
    fn scroll_navigation() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut screen = PreviewScreen::new();
        screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(screen.scroll, 1);
        screen.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(screen.scroll, 2);
        screen.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(screen.scroll, 1);
        screen.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
        assert_eq!(screen.scroll, 0);
    }

    #[test]
    fn changeset_to_preview_maps_all_categories() {
        let repo = PathBuf::from("/repo");
        let home_prefix = repo.join("home");

        let mut cs = ChangeSet::new();
        cs.additions.push(Addition {
            source: PathBuf::from("/home/user/.config/fish/config.fish"),
            destination: home_prefix.join(".config/fish/config.fish"),
            entry_type: EntryType::RegularFile,
        });
        cs.modifications.push(Modification {
            source: PathBuf::from("/home/user/.bashrc"),
            destination: home_prefix.join(".bashrc"),
            change: ChangeKind::ContentChanged,
        });
        cs.deletions.push(Deletion {
            destination: home_prefix.join(".old_file"),
            reason: DeletionReason::SourceRemoved,
        });
        cs.exclusions.push(Exclusion {
            source: PathBuf::from("/home/user/.config/fish/fish_history"),
            entry_type: EntryType::RegularFile,
            reason: ExclusionReason::IgnorePattern {
                pattern: "*_history".to_string(),
            },
        });
        cs.warnings.push(PlanWarning {
            path: PathBuf::from(".ssh/id_rsa"),
            kind: WarningKind::PossibleSecret {
                reason: "SSH private key".to_string(),
            },
        });

        let preview = PreviewScreen::changeset_to_preview(&cs, &repo);

        assert_eq!(preview.additions, 1);
        assert_eq!(preview.modifications, 1);
        assert_eq!(preview.deletions, 1);
        assert_eq!(preview.exclusions, 1);
        assert_eq!(preview.warnings, 1);
        assert_eq!(preview.entries.len(), 5);

        assert_eq!(preview.entries[0].kind, EntryKind::Addition);
        assert!(preview.entries[0].path.contains("config.fish"));

        assert_eq!(preview.entries[1].kind, EntryKind::Modification);
        assert!(preview.entries[1].path.contains(".bashrc"));

        assert_eq!(preview.entries[2].kind, EntryKind::Deletion);
        assert_eq!(preview.entries[3].kind, EntryKind::Exclusion);
        assert_eq!(preview.entries[4].kind, EntryKind::Warning);
    }

    #[test]
    fn empty_changeset_produces_empty_preview() {
        let repo = PathBuf::from("/repo");
        let cs = ChangeSet::new();
        let preview = PreviewScreen::changeset_to_preview(&cs, &repo);
        assert_eq!(preview.entries.len(), 0);
        assert_eq!(preview.additions, 0);
    }

    #[test]
    fn entry_kind_prefix() {
        assert_eq!(EntryKind::Addition.prefix(), "+");
        assert_eq!(EntryKind::Modification.prefix(), "~");
        assert_eq!(EntryKind::Deletion.prefix(), "-");
        assert_eq!(EntryKind::Exclusion.prefix(), "○");
        assert_eq!(EntryKind::Warning.prefix(), "⚠");
    }
}
