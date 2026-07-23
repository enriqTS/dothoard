//! Three-pane filesystem picker widget.
//!
//! Renders the browser model as a ranger/yazi-style layout and translates
//! keyboard input into browser navigation actions. Designed to be embedded
//! in repository and source selection screens.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::browser::{Browser, DirListing, EntryKind, Selection, SelectionError};

/// Actions that the picker can produce in response to key events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerAction {
    /// Key was consumed by picker navigation (no external action needed).
    Consumed,
    /// User pressed Space to select the current entry.
    Select(Result<Selection, SelectionError>),
    /// User pressed Escape to cancel/close the picker.
    Cancel,
    /// Key was not consumed by the picker.
    NotConsumed,
}

/// Default viewport height when the actual height is unknown.
const DEFAULT_VIEWPORT: usize = 20;

/// Handle a key event for the browser. Returns the resulting action.
pub fn handle_key(browser: &mut Browser, key: KeyEvent, viewport_height: usize) -> PickerAction {
    let vh = if viewport_height == 0 {
        DEFAULT_VIEWPORT
    } else {
        viewport_height
    };

    match (key.modifiers, key.code) {
        // Navigation: Up/k
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            if browser.move_up() {
                PickerAction::Consumed
            } else {
                // At top boundary — not consumed (allows parent to handle).
                PickerAction::NotConsumed
            }
        }
        // Navigation: Down/j
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            browser.move_down();
            PickerAction::Consumed
        }
        // Navigate into directory: Right/l/Enter
        (KeyModifiers::NONE, KeyCode::Right)
        | (KeyModifiers::NONE, KeyCode::Char('l'))
        | (KeyModifiers::NONE, KeyCode::Enter) => {
            browser.enter_selected();
            PickerAction::Consumed
        }
        // Navigate to parent: Left/h
        (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h')) => {
            browser.go_parent();
            PickerAction::Consumed
        }
        // Select entry: Space
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            let result = browser.try_select();
            PickerAction::Select(result)
        }
        // Home
        (KeyModifiers::NONE, KeyCode::Home) => {
            browser.move_home();
            PickerAction::Consumed
        }
        // End
        (KeyModifiers::NONE, KeyCode::End) => {
            browser.move_end();
            PickerAction::Consumed
        }
        // PageUp
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            browser.page_up(vh);
            PickerAction::Consumed
        }
        // PageDown
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            browser.page_down(vh);
            PickerAction::Consumed
        }
        // Refresh: Ctrl+R
        (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
            browser.refresh_current();
            PickerAction::Consumed
        }
        // Cancel: Escape
        (KeyModifiers::NONE, KeyCode::Esc) => PickerAction::Cancel,
        _ => PickerAction::NotConsumed,
    }
}

/// Render the three-pane browser into the given area.
pub fn draw(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    // Layout: breadcrumb (1 line) + three panes + status (1 line).
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Breadcrumb
            Constraint::Min(3),    // Panes
            Constraint::Length(1), // Status
        ])
        .split(area);

    draw_breadcrumb(frame, outer[0], browser);
    draw_panes(frame, outer[1], browser);
    draw_status(frame, outer[2], browser);
}

/// Draw the breadcrumb/path header.
fn draw_breadcrumb(frame: &mut Frame, area: Rect, browser: &Browser) {
    let path_str = browser.current_dir().to_string_lossy();
    let line = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(
            path_str.to_string(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Draw the three panes: parent (left), current (center), preview (right).
fn draw_panes(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    // Adaptive layout: if terminal is narrow, collapse parent/preview.
    let constraints = if area.width >= 80 {
        vec![
            Constraint::Percentage(20), // Parent
            Constraint::Percentage(50), // Current
            Constraint::Percentage(30), // Preview
        ]
    } else if area.width >= 40 {
        vec![
            Constraint::Percentage(0),  // No parent
            Constraint::Percentage(60), // Current
            Constraint::Percentage(40), // Preview
        ]
    } else {
        vec![
            Constraint::Percentage(0),   // No parent
            Constraint::Percentage(100), // Current only
            Constraint::Percentage(0),   // No preview
        ]
    };

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Parent pane (left).
    if panes[0].width > 0 {
        draw_parent_pane(frame, panes[0], browser);
    }

    // Current directory pane (center).
    let viewport_height = panes[1].height.saturating_sub(2) as usize; // subtract borders
    browser.adjust_scroll_with_height(viewport_height);
    draw_current_pane(frame, panes[1], browser);

    // Preview pane (right).
    if panes[2].width > 0 {
        draw_preview_pane(frame, panes[2], browser);
    }
}

/// Draw the parent directory pane.
fn draw_parent_pane(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(" .. ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let listing = browser.parent_listing();
    let items: Vec<ListItem> = match listing {
        Some(DirListing::Entries(entries)) => entries
            .iter()
            .map(|e| {
                let style = entry_style(e.kind, false);
                ListItem::new(Line::from(Span::styled(
                    truncate_name(&e.display_name, inner.width as usize),
                    style,
                )))
            })
            .collect(),
        Some(DirListing::Error(_)) => {
            vec![ListItem::new(Line::from(Span::styled(
                "<error>",
                Style::default().fg(Color::Red),
            )))]
        }
        None => Vec::new(),
    };

    let list = List::new(items);
    frame.render_widget(list, inner);
}

/// Draw the current directory pane with selection highlight.
fn draw_current_pane(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let listing = browser.current_listing().clone();
    match listing {
        DirListing::Entries(entries) if entries.is_empty() => {
            let msg = Paragraph::new(Line::from(Span::styled(
                " (empty)",
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(msg, inner);
        }
        DirListing::Entries(entries) => {
            let selected = browser.selected();
            let scroll = browser.scroll_offset();
            let viewport = inner.height as usize;

            let items: Vec<ListItem> = entries
                .iter()
                .enumerate()
                .skip(scroll)
                .take(viewport)
                .map(|(i, e)| {
                    let is_selected = i == selected;
                    let style = if is_selected {
                        entry_style(e.kind, true)
                    } else {
                        entry_style(e.kind, false)
                    };

                    let icon = entry_icon(e);
                    let name =
                        truncate_name(&e.display_name, inner.width.saturating_sub(3) as usize);
                    let line = Line::from(vec![
                        Span::styled(format!(" {icon} "), style),
                        Span::styled(name, style),
                    ]);
                    ListItem::new(line)
                })
                .collect();

            let list = List::new(items);
            frame.render_widget(list, inner);
        }
        DirListing::Error(e) => {
            let msg = Paragraph::new(Line::from(Span::styled(
                format!(" Error: {e}"),
                Style::default().fg(Color::Red),
            )));
            frame.render_widget(msg, inner);
        }
    }
}

/// Draw the preview pane (directory contents or file metadata).
fn draw_preview_pane(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Get selected entry info.
    let selected_entry = browser.selected_entry().cloned();

    match selected_entry {
        Some(entry) if entry.kind == EntryKind::Directory => {
            // Show directory preview.
            let preview = browser.preview_listing();
            match preview {
                Some(DirListing::Entries(entries)) if entries.is_empty() => {
                    let msg = Paragraph::new(Line::from(Span::styled(
                        " (empty)",
                        Style::default().fg(Color::DarkGray),
                    )));
                    frame.render_widget(msg, inner);
                }
                Some(DirListing::Entries(entries)) => {
                    let items: Vec<ListItem> = entries
                        .iter()
                        .take(inner.height as usize)
                        .map(|e| {
                            let icon = entry_icon(e);
                            let name = truncate_name(
                                &e.display_name,
                                inner.width.saturating_sub(3) as usize,
                            );
                            ListItem::new(Line::from(vec![
                                Span::styled(format!(" {icon} "), entry_style(e.kind, false)),
                                Span::styled(name, entry_style(e.kind, false)),
                            ]))
                        })
                        .collect();
                    let list = List::new(items);
                    frame.render_widget(list, inner);
                }
                Some(DirListing::Error(e)) => {
                    let msg = Paragraph::new(Line::from(Span::styled(
                        format!(" {e}"),
                        Style::default().fg(Color::Red),
                    )));
                    frame.render_widget(msg, inner);
                }
                None => {}
            }
        }
        Some(entry) => {
            // Show file/symlink metadata.
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(
                format!(" {}", entry.display_name),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            let kind_str = match entry.kind {
                EntryKind::File => "Regular file",
                EntryKind::Symlink => "Symbolic link",
                EntryKind::Special => "Special file",
                EntryKind::Error => "Unreadable",
                EntryKind::Directory => "Directory",
            };
            lines.push(Line::from(vec![
                Span::styled(" Type: ", Style::default().fg(Color::DarkGray)),
                Span::raw(kind_str),
            ]));

            if let Some(size) = entry.size {
                lines.push(Line::from(vec![
                    Span::styled(" Size: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(format_size(size)),
                ]));
            }

            if entry.executable {
                lines.push(Line::from(vec![
                    Span::styled(" Exec: ", Style::default().fg(Color::DarkGray)),
                    Span::styled("yes", Style::default().fg(Color::Green)),
                ]));
            }

            if let Some(ref target) = entry.link_target {
                lines.push(Line::from(vec![
                    Span::styled(" Target: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(target.clone(), Style::default().fg(Color::Magenta)),
                ]));
            }

            if entry.is_lossy {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " ⚠ Non-UTF-8 name (cannot select)",
                    Style::default().fg(Color::Yellow),
                )));
            }

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }
        None => {
            let msg = Paragraph::new(Line::from(Span::styled(
                " (no selection)",
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(msg, inner);
        }
    }
}

/// Draw the status/help bar at the bottom.
fn draw_status(frame: &mut Frame, area: Rect, _browser: &Browser) {
    let help = Line::from(vec![
        Span::styled(" ↑↓", Style::default().fg(Color::Cyan)),
        Span::raw(" navigate "),
        Span::styled("←→", Style::default().fg(Color::Cyan)),
        Span::raw(" open/back "),
        Span::styled("Space", Style::default().fg(Color::Cyan)),
        Span::raw(" select "),
        Span::styled("Esc", Style::default().fg(Color::Cyan)),
        Span::raw(" cancel"),
    ]);
    frame.render_widget(Paragraph::new(help), area);
}

/// Get a short icon character for an entry.
fn entry_icon(entry: &super::browser::Entry) -> char {
    match entry.kind {
        EntryKind::Directory => '📁',
        EntryKind::Symlink => '🔗',
        EntryKind::File if entry.executable => '⚡',
        EntryKind::File => '📄',
        EntryKind::Special => '⚠',
        EntryKind::Error => '✗',
    }
}

/// Get a style for an entry based on kind and selection state.
fn entry_style(kind: EntryKind, selected: bool) -> Style {
    let base = match kind {
        EntryKind::Directory => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
        EntryKind::Symlink => Style::default().fg(Color::Magenta),
        EntryKind::File => Style::default().fg(Color::White),
        EntryKind::Special => Style::default().fg(Color::Yellow),
        EntryKind::Error => Style::default().fg(Color::Red),
    };
    if selected {
        base.bg(Color::DarkGray)
    } else {
        base
    }
}

/// Truncate a name to fit within a given width.
fn truncate_name(name: &str, max_width: usize) -> String {
    if name.len() <= max_width {
        name.to_string()
    } else if max_width > 3 {
        format!("{}...", &name[..max_width - 3])
    } else {
        name[..max_width].to_string()
    }
}

/// Format a file size in human-readable form.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
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
                draw(frame, frame.area(), &mut browser);
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
                draw(frame, frame.area(), &mut browser);
            })
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
                draw(frame, frame.area(), &mut browser);
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
                draw(frame, frame.area(), &mut browser);
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
                draw(frame, frame.area(), &mut browser);
            })
            .unwrap();
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
                draw(frame, frame.area(), &mut browser);
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
                draw(frame, frame.area(), &mut browser);
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
                draw(frame, frame.area(), &mut browser);
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
}
