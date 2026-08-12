use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn new_screen_starts_in_list_mode() {
    let screen = SourcesScreen::new();
    assert_eq!(screen.mode, Mode::List);
    assert_eq!(screen.selected, 0);
}

#[test]
fn up_down_navigates_list() {
    let mut screen = SourcesScreen::new();
    screen.handle_key(key(KeyCode::Down), 3);
    assert_eq!(screen.selected, 1);
    screen.handle_key(key(KeyCode::Down), 3);
    assert_eq!(screen.selected, 2);
    // Should not go past the end.
    screen.handle_key(key(KeyCode::Down), 3);
    assert_eq!(screen.selected, 2);
    screen.handle_key(key(KeyCode::Up), 3);
    assert_eq!(screen.selected, 1);
}

#[test]
fn up_does_not_go_negative() {
    let mut screen = SourcesScreen::new();
    screen.handle_key(key(KeyCode::Up), 3);
    assert_eq!(screen.selected, 0);
}

#[test]
fn a_enters_browse_mode() {
    let mut screen = SourcesScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('a')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::Browse);
}

#[test]
fn typing_in_add_mode() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::AddInput;
    screen.handle_key(key(KeyCode::Char('.')), 0);
    screen.handle_key(key(KeyCode::Char('c')), 0);
    screen.handle_key(key(KeyCode::Char('o')), 0);
    assert_eq!(screen.input, ".co");
}

#[test]
fn add_input_handles_multibyte_characters() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::AddInput;
    screen.handle_key(key(KeyCode::Char('界')), 0);
    screen.handle_key(key(KeyCode::Char('é')), 0);
    screen.handle_key(key(KeyCode::Left), 0);
    screen.handle_key(key(KeyCode::Backspace), 0);
    assert_eq!(screen.input, "é");
    assert_eq!(screen.cursor, 0);
    screen.handle_key(key(KeyCode::Delete), 0);
    assert!(screen.input.is_empty());
}

#[test]
fn enter_in_add_mode_returns_add_action() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::AddInput;
    screen.input = ".config/fish".to_string();
    screen.cursor = screen.input.len();
    let action = screen.handle_key(key(KeyCode::Enter), 0);
    assert_eq!(action, Action::AddSource(".config/fish".to_string()));
}

#[test]
fn esc_in_add_mode_returns_to_browser() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::AddInput;
    screen.input = "partial".to_string();
    screen.handle_key(key(KeyCode::Esc), 0);
    assert_eq!(screen.mode, Mode::Browse);
}

#[test]
fn d_enters_confirm_delete() {
    let mut screen = SourcesScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('d')), 2);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::ConfirmDelete);
}

#[test]
fn d_does_nothing_when_empty() {
    let mut screen = SourcesScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('d')), 0);
    // Not consumed because the guard `source_count > 0` fails.
    assert_eq!(action, Action::NotConsumed);
    assert_eq!(screen.mode, Mode::List);
}

#[test]
fn confirm_delete_y_removes() {
    let mut screen = SourcesScreen::new();
    screen.selected = 1;
    screen.mode = Mode::ConfirmDelete;
    let action = screen.handle_key(key(KeyCode::Char('y')), 3);
    assert_eq!(action, Action::RemoveSource(1));
}

#[test]
fn confirm_delete_n_cancels() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::ConfirmDelete;
    screen.handle_key(key(KeyCode::Char('n')), 3);
    assert_eq!(screen.mode, Mode::List);
}

#[test]
fn validate_rejects_absolute_path() {
    let result = SourcesScreen::validate_source("/etc/foo", &[], Path::new("/home/user"), None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("relative"));
}

#[test]
fn validate_rejects_parent_traversal() {
    let result = SourcesScreen::validate_source("../outside", &[], Path::new("/home/user"), None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("traversal"));
}

#[test]
fn validate_rejects_duplicate() {
    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];
    let result =
        SourcesScreen::validate_source(".config/fish", &existing, Path::new("/home/user"), None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("already configured"));
}

#[test]
fn validate_accepts_new_valid_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let source_dir = home.join(".config/test");
    std::fs::create_dir_all(&source_dir).unwrap();

    let result = SourcesScreen::validate_source(".config/test", &[], home, None);
    assert!(result.is_ok());
    let info = result.unwrap();
    assert_eq!(info.path, ".config/test");
    assert!(info.exists);
    assert!(!info.is_symlink);
}

#[test]
fn validate_detects_symlink_source() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let target = home.join("real-dir");
    std::fs::create_dir_all(&target).unwrap();
    let link = home.join("link-dir");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let result = SourcesScreen::validate_source("link-dir", &[], home, None);
    assert!(result.is_ok());
    let info = result.unwrap();
    assert!(info.is_symlink);
    assert!(info.warning.is_some());
}

#[test]
fn validate_rejects_empty() {
    let result = SourcesScreen::validate_source("", &[], Path::new("/home/user"), None);
    assert!(result.is_err());
}

#[test]
fn j_k_navigate_like_vim() {
    let mut screen = SourcesScreen::new();
    screen.handle_key(key(KeyCode::Char('j')), 5);
    assert_eq!(screen.selected, 1);
    screen.handle_key(key(KeyCode::Char('k')), 5);
    assert_eq!(screen.selected, 0);
}

#[test]
fn up_at_zero_returns_not_consumed() {
    let mut screen = SourcesScreen::new();
    screen.selected = 0;
    let action = screen.handle_key(key(KeyCode::Up), 3);
    assert_eq!(action, Action::NotConsumed);
    assert_eq!(screen.selected, 0);
}

#[test]
fn tab_in_add_input_returns_not_consumed() {
    use crossterm::event::KeyModifiers;
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::AddInput;
    screen.input = "partial".to_string();
    let action = screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 0);
    assert_eq!(action, Action::NotConsumed);
    // Input preserved.
    assert_eq!(screen.input, "partial");
}

#[test]
fn tab_in_confirm_delete_returns_not_consumed() {
    use crossterm::event::KeyModifiers;
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::ConfirmDelete;
    let action = screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 3);
    assert_eq!(action, Action::NotConsumed);
}

// --- Browser mode tests ---

#[test]
fn browse_esc_returns_apply_selection() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    let action = screen.handle_key(key(KeyCode::Esc), 0);
    assert_eq!(action, Action::ApplySelection);
}

#[test]
fn browse_colon_switches_to_text_input() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    screen.ensure_browser(Path::new("/tmp"));
    let action = screen.handle_key(key(KeyCode::Char(':')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::AddInput);
}

#[test]
fn browse_space_toggles_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::create_dir(home.join(".config")).unwrap();

    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    screen.ensure_browser(home);
    screen.ensure_selection(&[], home);

    // Space toggles (adds to selection) and returns Consumed, not AddSource.
    let action = screen.handle_key(key(KeyCode::Char(' ')), 0);
    assert_eq!(action, Action::Consumed);
    // Browser stays open.
    assert_eq!(screen.mode, Mode::Browse);
    // Entry is now selected.
    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&home.join(".config")),
        crate::tui::selection::CheckState::Explicit
    );
}

#[test]
fn browse_space_toggles_file() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::write(home.join(".bashrc"), "# bash").unwrap();

    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    screen.ensure_browser(home);
    screen.ensure_selection(&[], home);

    let action = screen.handle_key(key(KeyCode::Char(' ')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::Browse);
    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&home.join(".bashrc")),
        crate::tui::selection::CheckState::Explicit
    );
}

#[test]
fn browse_space_toggles_symlink() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::create_dir(home.join("real")).unwrap();
    std::os::unix::fs::symlink("real", home.join("link")).unwrap();

    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    screen.ensure_browser(home);
    screen.ensure_selection(&[], home);

    // Navigate to the symlink entry.
    if let Some(ref mut browser) = screen.browser {
        let _ = browser.current_listing();
        use crate::tui::browser::DirListing;
        let listing = browser.current_listing().clone();
        if let DirListing::Entries(entries) = &listing {
            let idx = entries
                .iter()
                .position(|e| e.display_name == "link")
                .unwrap();
            for _ in 0..idx {
                browser.move_down();
            }
        }
    }

    let action = screen.handle_key(key(KeyCode::Char(' ')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::Browse);
    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&home.join("link")),
        crate::tui::selection::CheckState::Explicit
    );
}

#[test]
fn browser_root_is_home() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let mut screen = SourcesScreen::new();
    screen.ensure_browser(home);
    assert!(screen.browser.is_some());
    let browser = screen.browser.as_ref().unwrap();
    assert_eq!(browser.root(), home);
}

#[test]
fn browse_tab_escapes_to_tab_bar() {
    use crossterm::event::KeyModifiers;
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    let action = screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 0);
    assert_eq!(action, Action::NotConsumed);
}

// --- Multi-select session persistence tests ---

#[test]
fn selection_persists_across_browse_reentry() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::create_dir(home.join(".config")).unwrap();

    let sources = vec![SourceConfig {
        path: ".config".to_string(),
        ignore: vec![],
    }];

    let mut screen = SourcesScreen::new();
    screen.ensure_browser(home);
    screen.ensure_selection(&sources, home);

    // Selection should reflect existing config.
    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&home.join(".config")),
        crate::tui::selection::CheckState::Explicit
    );

    // Re-calling ensure_selection doesn't reset it.
    screen.ensure_selection(&[], home);
    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&home.join(".config")),
        crate::tui::selection::CheckState::Explicit
    );
}

#[test]
fn space_toggle_does_not_close_browser() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::write(home.join("file.txt"), "x").unwrap();

    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    screen.ensure_browser(home);
    screen.ensure_selection(&[], home);

    // Toggle on.
    let action = screen.handle_key(key(KeyCode::Char(' ')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::Browse);

    // Toggle off.
    let action = screen.handle_key(key(KeyCode::Char(' ')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::Browse);

    // Entry should be unchecked again.
    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&home.join("file.txt")),
        crate::tui::selection::CheckState::Unchecked
    );
}

#[test]
fn confirm_apply_y_returns_confirm_action() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::ConfirmApply;
    let action = screen.handle_key(key(KeyCode::Char('y')), 0);
    assert_eq!(action, Action::ConfirmApply);
}

#[test]
fn confirm_apply_n_returns_to_pending_choices() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::ConfirmApply;
    let action = screen.handle_key(key(KeyCode::Char('n')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::PendingChanges);
}

#[test]
fn confirm_apply_esc_returns_to_pending_choices() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::ConfirmApply;
    let action = screen.handle_key(key(KeyCode::Esc), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::PendingChanges);
}

#[test]
fn pending_changes_has_distinct_apply_discard_and_continue_actions() {
    let mut screen = SourcesScreen::new();
    screen.mode = Mode::PendingChanges;
    assert_eq!(
        screen.handle_key(key(KeyCode::Char('a')), 0),
        Action::ChooseApply
    );

    screen.mode = Mode::PendingChanges;
    assert_eq!(
        screen.handle_key(key(KeyCode::Char('d')), 0),
        Action::DiscardSelection
    );

    screen.mode = Mode::PendingChanges;
    assert_eq!(screen.handle_key(key(KeyCode::Esc), 0), Action::Consumed);
    assert_eq!(screen.mode, Mode::Browse);
}

#[test]
fn browse_space_toggles_inherited_entry_to_deselected() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let config_dir = home.join(".config");
    std::fs::create_dir(&config_dir).unwrap();
    std::fs::write(config_dir.join("file.txt"), "x").unwrap();

    let sources = vec![SourceConfig {
        path: ".config".to_string(),
        ignore: vec![],
    }];

    let mut screen = SourcesScreen::new();
    screen.mode = Mode::Browse;
    screen.ensure_browser(home);
    screen.ensure_selection(&sources, home);

    // Navigate into .config.
    if let Some(ref mut browser) = screen.browser {
        let _ = browser.current_listing();
        use crate::tui::browser::DirListing;
        let listing = browser.current_listing().clone();
        if let DirListing::Entries(entries) = &listing {
            let idx = entries
                .iter()
                .position(|e| e.display_name == ".config")
                .unwrap();
            for _ in 0..idx {
                browser.move_down();
            }
        }
        browser.enter_selected();
    }

    // file.txt inside .config should be inherited.
    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&config_dir.join("file.txt")),
        crate::tui::selection::CheckState::Inherited
    );

    // Toggle it (deselect).
    let action = screen.handle_key(key(KeyCode::Char(' ')), 0);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::Browse);

    let sel = screen.selection.as_ref().unwrap();
    assert_eq!(
        sel.is_selected(&config_dir.join("file.txt")),
        crate::tui::selection::CheckState::Unchecked
    );
}
