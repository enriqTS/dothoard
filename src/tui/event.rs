//! Terminal event handling.
//!
//! Provides a polling-based event loop that reads crossterm events with a
//! configurable tick rate for periodic UI updates.

use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};

/// The tick rate for the event loop (how often to redraw even without input).
const TICK_RATE: Duration = Duration::from_millis(250);

/// Events produced by the terminal event loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A key was pressed.
    Key(KeyEvent),
    /// The terminal was resized.
    Resize,
    /// A tick elapsed without user input (for periodic refresh).
    Tick,
}

/// Poll for the next terminal event.
///
/// Returns `Tick` if no event arrives within the tick interval.
pub fn next_event() -> std::io::Result<AppEvent> {
    if event::poll(TICK_RATE)? {
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => Ok(AppEvent::Key(key)),
            Event::Resize(_, _) => Ok(AppEvent::Resize),
            // Ignore release/repeat key events and other event types.
            _ => Ok(AppEvent::Tick),
        }
    } else {
        Ok(AppEvent::Tick)
    }
}
