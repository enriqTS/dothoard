use super::*;
use crate::tui::browser::BrowserConfig;
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;

fn setup_test_dir() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("alpha")).unwrap();
    std::fs::create_dir(root.join("beta")).unwrap();
    std::fs::create_dir(root.join(".hidden")).unwrap();
    std::fs::write(root.join("file.txt"), "content").unwrap();
    std::fs::write(root.join(".dotfile"), "hidden").unwrap();
    std::os::unix::fs::symlink("alpha", root.join("link")).unwrap();
    std::fs::create_dir(root.join("alpha").join("inner")).unwrap();
    std::fs::write(root.join("alpha").join("data.txt"), "data").unwrap();
    tmp
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

// --- Key handling tests ---

#[test]
fn down_moves_selection() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    let action = handle_key(&mut browser, key(KeyCode::Down), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.selected(), 1);
}

#[test]
fn j_moves_selection_down() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    let action = handle_key(&mut browser, key(KeyCode::Char('j')), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.selected(), 1);
}

#[test]
fn up_at_top_returns_not_consumed() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    let action = handle_key(&mut browser, key(KeyCode::Up), 20);
    assert_eq!(action, PickerAction::NotConsumed);
}

#[test]
fn k_at_top_returns_not_consumed() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    let action = handle_key(&mut browser, key(KeyCode::Char('k')), 20);
    assert_eq!(action, PickerAction::NotConsumed);
}

#[test]
fn up_from_nonzero_consumes() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();
    browser.move_down();

    let action = handle_key(&mut browser, key(KeyCode::Up), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.selected(), 0);
}

#[test]
fn enter_opens_directory() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    // Find alpha directory.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let idx = entries
            .iter()
            .position(|e| e.display_name == "alpha")
            .unwrap();
        for _ in 0..idx {
            browser.move_down();
        }
    }

    let action = handle_key(&mut browser, key(KeyCode::Enter), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.current_dir(), tmp.path().join("alpha"));
}

#[test]
fn left_goes_to_parent() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().join("alpha"),
    });

    let action = handle_key(&mut browser, key(KeyCode::Left), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.current_dir(), tmp.path());
}

#[test]
fn h_goes_to_parent() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().join("alpha"),
    });

    let action = handle_key(&mut browser, key(KeyCode::Char('h')), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.current_dir(), tmp.path());
}

#[test]
fn space_selects_entry() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    let action = handle_key(&mut browser, key(KeyCode::Char(' ')), 20);
    assert!(matches!(action, PickerAction::Select(Ok(_))));
}

#[test]
fn escape_cancels() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let action = handle_key(&mut browser, key(KeyCode::Esc), 20);
    assert_eq!(action, PickerAction::Cancel);
}

#[test]
fn home_moves_to_first() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();
    browser.move_down();
    browser.move_down();

    let action = handle_key(&mut browser, key(KeyCode::Home), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.selected(), 0);
}

#[test]
fn end_moves_to_last() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let count = browser.entry_count();

    let action = handle_key(&mut browser, key(KeyCode::End), 20);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.selected(), count - 1);
}

#[test]
fn page_down_moves_by_viewport() {
    let tmp = TempDir::new().unwrap();
    for i in 0..50 {
        std::fs::write(tmp.path().join(format!("f{i:03}.txt")), "x").unwrap();
    }
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    let action = handle_key(&mut browser, key(KeyCode::PageDown), 10);
    assert_eq!(action, PickerAction::Consumed);
    assert_eq!(browser.selected(), 10);
}

#[test]
fn unrecognized_key_not_consumed() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let action = handle_key(&mut browser, key(KeyCode::Char('x')), 20);
    assert_eq!(action, PickerAction::NotConsumed);
}

#[test]
fn ctrl_arrows_and_ctrl_jk_scroll_preview_without_moving_selection() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    browser.set_preview_extent(10, 3);
    let selected = browser.selected();

    assert_eq!(
        handle_key(
            &mut browser,
            KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL),
            20,
        ),
        PickerAction::Consumed
    );
    assert_eq!(
        handle_key(
            &mut browser,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
            20,
        ),
        PickerAction::Consumed
    );
    assert_eq!(browser.preview_scroll(), 2);
    assert_eq!(browser.selected(), selected);

    handle_key(
        &mut browser,
        KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL),
        20,
    );
    handle_key(
        &mut browser,
        KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL),
        20,
    );
    assert_eq!(browser.preview_scroll(), 0);
    assert_eq!(browser.selected(), selected);

    browser.set_preview_extent(10, 3);
    browser.scroll_preview_down();
    browser.move_down();
    assert_eq!(browser.preview_scroll(), 0);
    assert_eq!(browser.selected(), selected + 1);
}

#[test]
fn ctrl_r_refreshes() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let _ = browser.current_listing();

    let action = handle_key(
        &mut browser,
        KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        20,
    );
    assert_eq!(action, PickerAction::Consumed);
}

// --- Rendering tests ---

#[test]
fn renders_without_panic_wide_terminal() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

#[test]
fn renders_without_panic_narrow_terminal() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let backend = TestBackend::new(30, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

#[test]
fn renders_unicode_entry_and_breadcrumb_in_narrow_terminal() {
    let tmp = TempDir::new().unwrap();
    let unicode_dir = tmp.path().join("配置界e\u{301}");
    std::fs::create_dir(&unicode_dir).unwrap();
    std::fs::write(unicode_dir.join("🙂界é-long-name.conf"), "x").unwrap();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: unicode_dir,
    });

    let backend = TestBackend::new(16, 7);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| draw(frame, frame.area(), &mut browser, None))
        .unwrap();
}

#[test]
fn renders_without_panic_medium_terminal() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

#[test]
fn renders_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let empty = tmp.path().join("empty");
    std::fs::create_dir(&empty).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: empty,
    });

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

#[test]
fn renders_directory_with_many_entries() {
    let tmp = TempDir::new().unwrap();
    for i in 0..100 {
        std::fs::write(tmp.path().join(format!("file_{i:03}.txt")), "x").unwrap();
    }

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    // Move to a mid position to test scroll.
    for _ in 0..50 {
        browser.move_down();
    }

    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

#[test]
fn selected_regular_file_preview_renders_cat_content() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("--preview.conf"),
        "set editor helix\nset greeting olá",
    )
    .unwrap();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| draw(frame, frame.area(), &mut browser, None))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(content.contains("Type: Regular file"));
    assert!(content.contains("Content"));
    assert!(content.contains("set editor helix"));
    assert!(content.contains("set greeting olá"));
}

#[test]
fn oversized_regular_file_preview_explains_limit() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("large.conf"), vec![b'x'; 256 * 1024 + 1]).unwrap();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| draw(frame, frame.area(), &mut browser, None))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(content.contains("Content"));
    assert!(content.contains("file exceeds 256 KB"));
}

#[test]
fn regular_file_content_scrolls_independently() {
    let tmp = TempDir::new().unwrap();
    let content = (0..30)
        .map(|index| format!("preview-row-{index:02}-value"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(tmp.path().join("scroll.conf"), content).unwrap();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| draw(frame, frame.area(), &mut browser, None))
        .unwrap();
    handle_key(
        &mut browser,
        KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL),
        20,
    );
    handle_key(
        &mut browser,
        KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL),
        20,
    );
    terminal
        .draw(|frame| draw(frame, frame.area(), &mut browser, None))
        .unwrap();

    let rendered: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(rendered.contains("Content 3-"));
    assert!(!rendered.contains("preview-row-00-value"));
    assert!(rendered.contains("preview-row-02-value"));
    assert!(rendered.contains("Type: Regular file"));
}

#[test]
fn renders_with_symlink_preview() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Select the symlink.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::Symlink)
            .unwrap();
        for _ in 0..idx {
            browser.move_down();
        }
    }

    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

#[test]
fn renders_directory_preview_pane() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Select "alpha" directory.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let idx = entries
            .iter()
            .position(|e| e.display_name == "alpha")
            .unwrap();
        for _ in 0..idx {
            browser.move_down();
        }
    }

    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

#[test]
fn renders_at_minimum_size() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Very small terminal — should not crash.
    let backend = TestBackend::new(10, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();
}

// --- Utility tests ---

#[test]
fn truncate_name_short() {
    assert_eq!(truncate_name("hello", 10), "hello");
}

#[test]
fn truncate_name_exact() {
    assert_eq!(truncate_name("hello", 5), "hello");
}

#[test]
fn truncate_name_long() {
    assert_eq!(truncate_name("hello_world", 8), "hello...");
}

#[test]
fn truncate_name_uses_terminal_width_for_unicode() {
    assert_eq!(truncate_name("界界界", 5), "界...");
    assert_eq!(truncate_name("e\u{301}界", 3), "e\u{301}界");
}

#[test]
fn format_size_bytes() {
    assert_eq!(format_size(42), "42 B");
}

#[test]
fn format_size_kb() {
    assert_eq!(format_size(2048), "2.0 KB");
}

#[test]
fn format_size_mb() {
    assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
}

// --- Checkbox rendering tests ---

#[test]
fn renders_with_checkboxes_without_panic() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let check_fn = |_path: &std::path::Path| CheckState::Unchecked;

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, Some(&check_fn));
        })
        .unwrap();
}

#[test]
fn renders_with_checkboxes_narrow_terminal() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let check_fn = |_path: &std::path::Path| CheckState::Explicit;

    let backend = TestBackend::new(30, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, Some(&check_fn));
        })
        .unwrap();
}

#[test]
fn renders_mixed_check_states() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Alternate between states based on entry index.
    let root = tmp.path().to_path_buf();
    let alpha_path = root.join("alpha");
    let hidden_prefix = root.join(".hidden");
    let check_fn = move |path: &std::path::Path| {
        if path == alpha_path {
            CheckState::Explicit
        } else if path.starts_with(&hidden_prefix) {
            CheckState::Inherited
        } else {
            CheckState::Unchecked
        }
    };

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, Some(&check_fn));
        })
        .unwrap();

    // Verify that checkbox indicators appear in the buffer.
    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(content.contains('['), "should contain checkbox brackets");
}

#[test]
fn pane_focus_and_selection_are_visible_without_color() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().join("alpha"),
    });
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| draw(frame, frame.area(), &mut browser, None))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content: String = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(content.contains("Parent"));
    assert!(content.contains("▶ Files [ACTIVE: Browser]"));
    assert!(content.contains("Preview"));
    // Selection is conveyed by the "▶" marker (already asserted above via
    // text content) plus a bold weight, not by color alone.
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.modifier.contains(Modifier::BOLD))
    );
}

#[test]
fn ascii_presentation_uses_one_cell_icons_and_caller_context() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| {
            draw_with_presentation(
                frame,
                frame.area(),
                &mut browser,
                None,
                Presentation::SOURCES.ascii_safe(),
            );
        })
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
        .collect();
    assert!(content.contains("▶ Files [ACTIVE: Sources]"));
    assert!(content.contains("D") || content.contains("F"));
    assert!(!content.contains('📁'));
}

#[test]
fn renders_git_repository_icon_beside_directory_name() {
    let tmp = setup_test_dir();
    std::fs::create_dir(tmp.path().join("alpha/.git")).unwrap();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    let backend = TestBackend::new(80, 20);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|frame| draw(frame, frame.area(), &mut browser, None))
        .unwrap();

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(content.contains("⎇ alpha"));
    assert!(!content.contains(".git"));
}

#[test]
fn renders_without_checkboxes_when_none() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(frame, frame.area(), &mut browser, None);
        })
        .unwrap();

    // Without check_fn, no checkbox brackets should appear in the main pane.
    // (Brackets may appear in the path breadcrumb, so this checks the general pattern.)
    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
        .collect();
    // [●] or [◉] or [ ] should NOT appear.
    assert!(
        !content.contains("[●]"),
        "should not contain explicit checkbox"
    );
    assert!(
        !content.contains("[◉]"),
        "should not contain inherited checkbox"
    );
}
