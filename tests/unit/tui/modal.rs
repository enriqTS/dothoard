use ratatui::{Terminal, backend::TestBackend};

use super::*;

#[test]
fn dialog_area_is_centered_and_clamped() {
    assert_eq!(
        dialog_area(Rect::new(0, 0, 80, 24), 40, 10),
        Rect::new(20, 7, 40, 10)
    );
    assert_eq!(
        dialog_area(Rect::new(0, 0, 10, 3), 40, 10),
        Rect::new(0, 0, 10, 3)
    );
}

#[test]
fn input_dialog_keeps_unicode_cursor_visible_on_narrow_terminal() {
    let backend = TestBackend::new(24, 8);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw_text_input(
                frame,
                frame.area(),
                TextInputSpec {
                    title: "Edit",
                    label: "Value",
                    text: "prefix-界界界",
                    cursor: "prefix-界界".len(),
                    validation: Some(("Invalid value", THEME.error())),
                    submit: "Enter: save",
                    cancel: "Esc: cancel",
                },
            );
        })
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Edit"));
    assert!(text.contains("^"));
    assert!(text.contains("Invalid"));
}

#[test]
fn short_dialog_keeps_explicit_actions_visible() {
    let backend = TestBackend::new(30, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(
                frame,
                frame.area(),
                ModalSpec {
                    title: "Delete namespace",
                    affected: Some("/a/very/long/namespace/path/with-unicode-界/home"),
                    consequence: "This action removes only the owned namespace.",
                    validation: Some(("Confirmation is required", THEME.warning())),
                    confirm: "y: delete",
                    cancel: "Esc: cancel",
                },
            );
        })
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("y: delete"));
    assert!(text.contains("Esc: cancel"));
}

#[test]
fn confirmation_wraps_a_long_affected_path_on_a_short_terminal() {
    let backend = TestBackend::new(30, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            draw(
                frame,
                frame.area(),
                ModalSpec {
                    title: "Delete namespace",
                    affected: Some("/a/very/long/namespace/path/with-unicode-界/home"),
                    consequence: "This action removes only the owned namespace.",
                    validation: Some(("Confirmation is required", THEME.warning())),
                    confirm: "y: delete",
                    cancel: "Esc: cancel",
                },
            );
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(buffer.content.iter().any(|cell| cell.symbol() == "D"));
}
