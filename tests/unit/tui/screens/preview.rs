use super::*;
use crate::backup::changeset::*;
use std::path::PathBuf;

#[test]
fn new_screen_is_not_loaded() {
    let screen = PreviewScreen::new();
    assert!(matches!(screen.load_state, LoadState::NotLoaded));
}

#[test]
fn r_triggers_refresh() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut screen = PreviewScreen::new();
    let action = screen.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert_eq!(action, Action::Refresh);
    assert!(matches!(screen.load_state, LoadState::NotLoaded));
}

#[test]
fn b_triggers_backup() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut screen = PreviewScreen::new();
    let action = screen.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(action, Action::RunBackup);
}

#[test]
fn p_triggers_push() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut screen = PreviewScreen::new();
    let action = screen.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    assert_eq!(action, Action::Push);
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
    let home_prefix = repo.join("desktop/home");

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

    let preview = PreviewScreen::changeset_to_preview(&cs, &repo, "desktop");

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
    let preview = PreviewScreen::changeset_to_preview(&cs, &repo, "desktop");
    assert_eq!(preview.entries.len(), 0);
    assert_eq!(preview.additions, 0);
}

#[test]
fn entry_kind_prefix() {
    assert_eq!(EntryKind::Addition.prefix(), "+");
    assert_eq!(EntryKind::Modification.prefix(), "~");
    assert_eq!(EntryKind::Deletion.prefix(), "-");
    assert_eq!(EntryKind::Exclusion.prefix(), "i");
    assert_eq!(EntryKind::Warning.prefix(), "!");
}
