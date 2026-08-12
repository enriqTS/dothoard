use super::*;
use crate::state::AppState;

#[test]
fn success_after_clean_state_is_quiet() {
    let state = AppState::new();

    let result = decide_notification("desktop", true, None, &state);

    assert_eq!(result, None);
}

#[test]
fn success_after_failure_triggers_recovery() {
    let mut state = AppState::new();
    state.latest_error = Some("previous failure".to_string());

    let result = decide_notification("desktop", true, None, &state);

    assert!(result.is_some());
    let (summary, body, urgency) = result.unwrap();
    assert!(summary.contains("desktop"));
    assert!(summary.contains("recovered"));
    assert!(body.contains("working again"));
    assert_eq!(urgency, Urgency::Normal);
}

#[test]
fn failure_always_notifies() {
    let state = AppState::new();

    let result = decide_notification("desktop", false, Some("source not found"), &state);

    assert!(result.is_some());
    let (summary, body, urgency) = result.unwrap();
    assert!(summary.contains("desktop"));
    assert!(summary.contains("failed"));
    assert!(body.contains("source not found"));
    assert_eq!(urgency, Urgency::Critical);
}

#[test]
fn failure_with_no_message_uses_generic_text() {
    let state = AppState::new();

    let result = decide_notification("desktop", false, None, &state);

    assert!(result.is_some());
    let (_summary, body, _urgency) = result.unwrap();
    assert!(body.contains("unknown error"));
}

#[test]
fn repeated_failure_still_notifies() {
    let mut state = AppState::new();
    state.latest_error = Some("old error".to_string());

    let result = decide_notification("desktop", false, Some("new error"), &state);

    assert!(result.is_some());
    let (_summary, body, urgency) = result.unwrap();
    assert!(body.contains("new error"));
    assert_eq!(urgency, Urgency::Critical);
}

#[test]
fn success_after_success_is_quiet() {
    let mut state = AppState::new();
    // No latest_error means last run succeeded.
    state.latest_error = None;

    let result = decide_notification("desktop", true, None, &state);

    assert_eq!(result, None);
}

#[test]
fn urgency_values() {
    assert_eq!(Urgency::Normal, Urgency::Normal);
    assert_ne!(Urgency::Normal, Urgency::Critical);
}
