//! Transient TUI status and progress messages.

/// Number of 250 ms event-loop ticks before a transient message expires.
const SUCCESS_TICKS: u16 = 16;
const WARNING_TICKS: u16 = 24;
const ERROR_TICKS: u16 = 40;

/// Semantic category for a status message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StatusKind {
    Success,
    Running,
    Warning,
    Error,
}

impl StatusKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "Success",
            Self::Running => "Running",
            Self::Warning => "Warning",
            Self::Error => "Error",
        }
    }

    fn lifetime(self) -> Option<u16> {
        match self {
            Self::Success => Some(SUCCESS_TICKS),
            Self::Warning => Some(WARNING_TICKS),
            Self::Error => Some(ERROR_TICKS),
            Self::Running => None,
        }
    }
}

/// A typed status message with deterministic tick-based expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusMessage {
    pub kind: StatusKind,
    pub text: String,
    remaining_ticks: Option<u16>,
}

impl StatusMessage {
    pub fn new(kind: StatusKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
            remaining_ticks: kind.lifetime(),
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Success, text)
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Warning, text)
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Error, text)
    }

    pub fn running(text: impl Into<String>) -> Self {
        Self::new(StatusKind::Running, text)
    }

    /// Advance the lifecycle by one event-loop tick. Returns true on expiry.
    pub fn tick(&mut self) -> bool {
        let Some(remaining) = self.remaining_ticks.as_mut() else {
            return false;
        };
        *remaining = remaining.saturating_sub(1);
        *remaining == 0
    }

    pub fn contains(&self, pattern: &str) -> bool {
        self.text.contains(pattern)
    }
}

/// Publish a message using semantic priority. A task completion may always
/// replace its running indicator; otherwise lower-priority feedback cannot
/// hide a more important transient message.
pub fn publish(slot: &mut Option<StatusMessage>, incoming: StatusMessage) {
    let replace = slot
        .as_ref()
        .is_none_or(|current| current.kind == StatusKind::Running || incoming.kind >= current.kind);
    if replace {
        *slot = Some(incoming);
    }
}

#[cfg(test)]
mod tests {
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
}
