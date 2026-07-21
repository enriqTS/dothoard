//! Terminal initialization, restoration, and the main event loop.
//!
//! Handles raw-mode entry, alternate screen, panic-safe cleanup, and the
//! top-level draw/event cycle.

use std::io::{self, Stdout, stdout};
use std::panic;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;

use super::App;
use super::event::{AppEvent, next_event};
use super::ui;

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Run the TUI application.
///
/// This function takes ownership of the terminal, runs the event loop until
/// the user quits, and restores the terminal on exit (including panics).
pub fn run() -> io::Result<()> {
    install_panic_hook();
    let mut terminal = setup_terminal()?;

    let result = run_loop(&mut terminal);

    restore_terminal()?;
    result
}

/// The main event loop: draw, poll events, update state, repeat.
fn run_loop(terminal: &mut Term) -> io::Result<()> {
    let mut app = App::new();

    loop {
        // Poll for completed background tasks before drawing.
        app.poll_tasks();

        terminal.draw(|frame| ui::draw(frame, &app))?;

        match next_event()? {
            AppEvent::Key(key) => app.handle_key(key),
            AppEvent::Resize => {
                // Ratatui handles resize automatically on the next draw.
            }
            AppEvent::Tick => {
                // Periodic refresh allows background task results to appear.
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Enter raw mode and switch to the alternate screen.
fn setup_terminal() -> io::Result<Term> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Leave the alternate screen and disable raw mode.
///
/// Called on normal exit and from the panic hook.
fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    Ok(())
}

/// Install a panic hook that restores the terminal before printing the panic.
///
/// Without this, a panic leaves the terminal in raw mode with the alternate
/// screen active, making the error message invisible and the shell unusable.
fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // Best-effort terminal restoration; ignore errors since we're panicking.
        let _ = restore_terminal();
        original_hook(panic_info);
    }));
}
