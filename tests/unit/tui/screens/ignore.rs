use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn new_screen_starts_in_list_mode() {
    let screen = IgnoreScreen::new();
    assert_eq!(screen.mode, Mode::List);
    assert_eq!(screen.source_idx, 0);
    assert_eq!(screen.pattern_idx, 0);
}

#[test]
fn up_down_navigates_patterns() {
    let mut screen = IgnoreScreen::new();
    // Start at SourceSelector; Down enters PatternList.
    screen.handle_key(key(KeyCode::Down), 3, 1);
    assert_eq!(screen.list_focus, ListFocus::PatternList);
    assert_eq!(screen.pattern_idx, 0);
    // Down again advances pattern_idx.
    screen.handle_key(key(KeyCode::Down), 3, 1);
    assert_eq!(screen.pattern_idx, 1);
    // Up returns to previous pattern.
    screen.handle_key(key(KeyCode::Up), 3, 1);
    assert_eq!(screen.pattern_idx, 0);
    // Up at pattern_idx 0 returns to SourceSelector.
    screen.handle_key(key(KeyCode::Up), 3, 1);
    assert_eq!(screen.list_focus, ListFocus::SourceSelector);
}

#[test]
fn left_right_switches_source() {
    let mut screen = IgnoreScreen::new();
    screen.handle_key(key(KeyCode::Right), 0, 3);
    assert_eq!(screen.source_idx, 1);
    screen.handle_key(key(KeyCode::Left), 0, 3);
    assert_eq!(screen.source_idx, 0);
}

#[test]
fn a_enters_add_mode() {
    let mut screen = IgnoreScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('a')), 0, 1);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.mode, Mode::AddInput);
}

#[test]
fn add_input_handles_multibyte_characters() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::AddInput;
    screen.handle_key(key(KeyCode::Char('界')), 0, 1);
    screen.handle_key(key(KeyCode::Char('é')), 0, 1);
    screen.handle_key(key(KeyCode::Left), 0, 1);
    screen.handle_key(key(KeyCode::Backspace), 0, 1);
    assert_eq!(screen.input, "é");
    assert_eq!(screen.cursor, 0);
    screen.handle_key(key(KeyCode::Delete), 0, 1);
    assert!(screen.input.is_empty());
}

#[test]
fn enter_in_add_mode_returns_add_action() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::AddInput;
    screen.input = "*.log".to_string();
    screen.cursor = screen.input.len();
    let action = screen.handle_key(key(KeyCode::Enter), 0, 1);
    assert_eq!(action, Action::AddPattern(0, "*.log".to_string()));
}

#[test]
fn esc_in_add_mode_returns_to_list() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::AddInput;
    screen.handle_key(key(KeyCode::Esc), 0, 1);
    assert_eq!(screen.mode, Mode::List);
}

#[test]
fn d_returns_remove_action() {
    let mut screen = IgnoreScreen::new();
    screen.pattern_idx = 1;
    let action = screen.handle_key(key(KeyCode::Char('d')), 3, 1);
    assert_eq!(action, Action::RemovePattern(0, 1));
}

#[test]
fn p_enters_preview_mode() {
    let mut screen = IgnoreScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('p')), 0, 1);
    assert_eq!(action, Action::RefreshPreview(0));
    assert_eq!(screen.mode, Mode::Preview);
}

#[test]
fn esc_in_preview_returns_to_list() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::Preview;
    screen.handle_key(key(KeyCode::Esc), 0, 1);
    assert_eq!(screen.mode, Mode::List);
}

#[test]
fn r_refreshes_while_preview_remains_open() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::Preview;
    assert_eq!(
        screen.handle_key(key(KeyCode::Char('r')), 0, 1),
        Action::RefreshPreview(0)
    );
    assert_eq!(screen.mode, Mode::Preview);
}

#[test]
fn one_row_preview_does_not_scroll_out_of_view() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::Preview;
    screen.replace_preview(vec![PreviewEntry {
        path: "only-file".to_string(),
        ignored: false,
        matched_by: None,
        secret_warning: false,
    }]);
    screen.set_preview_viewport_height(4);

    screen.handle_key(key(KeyCode::End), 0, 1);
    screen.handle_key(key(KeyCode::Down), 0, 1);

    assert_eq!(screen.preview_viewport.visible_range(1), 0..1);
}

#[test]
fn preview_scrolls_with_arrows_pages_home_and_end() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::Preview;
    screen.replace_preview(
        (0..12)
            .map(|i| PreviewEntry {
                path: format!("file-{i}"),
                ignored: false,
                matched_by: None,
                secret_warning: false,
            })
            .collect(),
    );
    screen.set_preview_viewport_height(4);

    screen.handle_key(key(KeyCode::Down), 0, 1);
    assert_eq!(screen.preview_viewport.offset(), 1);
    screen.handle_key(key(KeyCode::PageDown), 0, 1);
    assert_eq!(screen.preview_viewport.offset(), 5);
    screen.handle_key(key(KeyCode::End), 0, 1);
    assert_eq!(screen.preview_viewport.visible_range(12), 8..12);
    screen.handle_key(key(KeyCode::PageUp), 0, 1);
    assert_eq!(screen.preview_viewport.offset(), 4);
    screen.handle_key(key(KeyCode::Home), 0, 1);
    assert_eq!(screen.preview_viewport.offset(), 0);
}

#[test]
fn preview_viewport_clamps_after_refresh_shrinks_data() {
    let mut screen = IgnoreScreen::new();
    screen.replace_preview(
        (0..10)
            .map(|i| PreviewEntry {
                path: format!("file-{i}"),
                ignored: false,
                matched_by: None,
                secret_warning: false,
            })
            .collect(),
    );
    screen.set_preview_viewport_height(3);
    screen.preview_viewport.end(10);

    let shortened = screen.preview().unwrap()[..2].to_vec();
    screen.replace_preview(shortened);

    assert_eq!(screen.preview_viewport.visible_range(2), 0..2);
}

#[test]
fn preview_viewport_survives_tab_focus_escape() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::Preview;
    screen.replace_preview(
        (0..8)
            .map(|i| PreviewEntry {
                path: format!("file-{i}"),
                ignored: false,
                matched_by: None,
                secret_warning: false,
            })
            .collect(),
    );
    screen.set_preview_viewport_height(3);
    screen.handle_key(key(KeyCode::PageDown), 0, 1);

    let action = screen.handle_key(key(KeyCode::Tab), 0, 1);

    assert_eq!(action, Action::NotConsumed);
    assert_eq!(screen.preview_viewport.offset(), 3);
}

#[test]
fn generate_preview_for_nonexistent_source() {
    let home = std::path::Path::new("/tmp/nonexistent-dothoard-test");
    let entries = IgnoreScreen::generate_preview("missing-source", &[], home);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].path.contains("does not exist"));
}

#[test]
fn generate_preview_marks_ignored_files() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let source = home.join("test-source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("keep.txt"), "keep").unwrap();
    std::fs::write(source.join("remove.log"), "log").unwrap();

    let patterns = vec!["*.log".to_string()];
    let entries = IgnoreScreen::generate_preview("test-source", &patterns, home);

    let log_entry = entries.iter().find(|e| e.path.contains("remove.log"));
    let keep_entry = entries.iter().find(|e| e.path.contains("keep.txt"));

    assert!(log_entry.is_some(), "log file should appear in preview");
    assert!(log_entry.unwrap().ignored);
    assert!(keep_entry.is_some(), "keep file should appear in preview");
    assert!(!keep_entry.unwrap().ignored);
}

#[test]
fn generate_preview_detects_secrets() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let source = home.join("ssh-test");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("id_rsa"), "private key").unwrap();
    std::fs::write(source.join("config"), "config").unwrap();

    let entries = IgnoreScreen::generate_preview("ssh-test", &[], home);

    let key_entry = entries.iter().find(|e| e.path.contains("id_rsa"));
    assert!(key_entry.is_some());
    assert!(key_entry.unwrap().secret_warning);
}

#[test]
fn up_at_source_selector_returns_not_consumed() {
    let mut screen = IgnoreScreen::new();
    screen.list_focus = ListFocus::SourceSelector;
    let action = screen.handle_key(key(KeyCode::Up), 3, 2);
    assert_eq!(action, Action::NotConsumed);
}

#[test]
fn up_at_pattern_zero_moves_to_source_selector() {
    let mut screen = IgnoreScreen::new();
    screen.list_focus = ListFocus::PatternList;
    screen.pattern_idx = 0;
    let action = screen.handle_key(key(KeyCode::Up), 3, 2);
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.list_focus, ListFocus::SourceSelector);
}

#[test]
fn tab_in_add_input_returns_not_consumed() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::AddInput;
    screen.input = "*.log".to_string();
    let action = screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 0, 1);
    assert_eq!(action, Action::NotConsumed);
    assert_eq!(screen.input, "*.log");
}

#[test]
fn tab_in_preview_returns_not_consumed() {
    let mut screen = IgnoreScreen::new();
    screen.mode = Mode::Preview;
    let action = screen.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE), 0, 1);
    assert_eq!(action, Action::NotConsumed);
}
