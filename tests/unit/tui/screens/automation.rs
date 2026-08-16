use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn new_screen_is_not_loaded() {
    let screen = AutomationScreen::new();
    assert!(matches!(screen.status_state, LoadState::NotLoaded));
}

#[test]
fn b_selects_next_backend() {
    let mut screen = AutomationScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('b')));
    assert_eq!(action, Action::SelectNextBackend);
}

#[test]
fn r_triggers_refresh() {
    let mut screen = AutomationScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('r')));
    assert_eq!(action, Action::RefreshStatus);
    assert!(matches!(screen.status_state, LoadState::NotLoaded));
}

#[test]
fn i_prompts_install_confirmation() {
    let mut screen = AutomationScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('i')));
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.confirm, ConfirmAction::Install);
}

#[test]
fn x_prompts_remove_confirmation() {
    let mut screen = AutomationScreen::new();
    let action = screen.handle_key(key(KeyCode::Char('x')));
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.confirm, ConfirmAction::Remove);
}

#[test]
fn confirm_y_installs() {
    let mut screen = AutomationScreen::new();
    screen.confirm = ConfirmAction::Install;
    let action = screen.handle_key(key(KeyCode::Char('y')));
    assert_eq!(action, Action::Install);
    assert_eq!(screen.confirm, ConfirmAction::None);
}

#[test]
fn confirm_y_removes() {
    let mut screen = AutomationScreen::new();
    screen.confirm = ConfirmAction::Remove;
    let action = screen.handle_key(key(KeyCode::Char('y')));
    assert_eq!(action, Action::Remove);
    assert_eq!(screen.confirm, ConfirmAction::None);
}

#[test]
fn confirm_n_cancels() {
    let mut screen = AutomationScreen::new();
    screen.confirm = ConfirmAction::Install;
    let action = screen.handle_key(key(KeyCode::Char('n')));
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.confirm, ConfirmAction::None);
}

#[test]
fn confirm_esc_cancels() {
    let mut screen = AutomationScreen::new();
    screen.confirm = ConfirmAction::Remove;
    let action = screen.handle_key(key(KeyCode::Esc));
    assert_eq!(action, Action::Consumed);
    assert_eq!(screen.confirm, ConfirmAction::None);
}
