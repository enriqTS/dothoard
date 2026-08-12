use super::*;

#[test]
fn transient_message_expires_on_ticks() {
    let mut message = StatusMessage::success("saved");
    for _ in 1..SUCCESS_TICKS {
        assert!(!message.tick());
    }
    assert!(message.tick());
}

#[test]
fn running_message_does_not_expire() {
    let mut message = StatusMessage::running("working");
    for _ in 0..100 {
        assert!(!message.tick());
    }
}

#[test]
fn priority_preserves_error_but_completion_replaces_running() {
    let mut slot = Some(StatusMessage::error("failure"));
    publish(&mut slot, StatusMessage::success("saved"));
    assert_eq!(slot.unwrap().text, "failure");

    let mut slot = Some(StatusMessage::running("working"));
    publish(&mut slot, StatusMessage::success("done"));
    assert_eq!(slot.unwrap().text, "done");
}
