//! Terminal initialization, restoration, and the main event loop.
//!
//! Handles raw-mode entry, alternate screen, panic-safe cleanup, and the
//! top-level draw/event cycle.

use std::io::{self, Stdout, stdout};
use std::panic;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::prelude::CrosstermBackend;
use tracing_appender::non_blocking::WorkerGuard;

use super::App;
use super::event::{AppEvent, next_event};
use super::ui;
use crate::diagnostics;
use crate::paths::AppPaths;

type Term = Terminal<CrosstermBackend<Stdout>>;

/// Run the TUI application.
///
/// This function takes ownership of the terminal, runs the event loop until
/// the user quits, and restores the terminal on exit (including panics).
/// It initializes tracing to a log file to prevent display corruption.
pub fn run() -> io::Result<()> {
    // Initialize tracing to a log file for TUI mode
    let _log_guard = init_tui_logging().map_err(io::Error::other)?;

    install_panic_hook();
    let mut terminal = setup_terminal()?;

    let result = run_loop(&mut terminal);

    restore_terminal()?;
    result
}

/// Initialize logging for TUI mode, writing to a log file.
///
/// Returns a `WorkerGuard` that must be held for the lifetime of the TUI
/// to ensure logs are flushed to the file.
fn init_tui_logging() -> anyhow::Result<WorkerGuard> {
    let paths = AppPaths::from_environment()?;
    let log_path = paths.state_dir().join("dothoard.log");
    diagnostics::init_for_tui(&log_path)
}

/// The main event loop: draw, poll events, update state, repeat.
fn run_loop(terminal: &mut Term) -> io::Result<()> {
    let mut app = App::new();

    loop {
        // Poll local tasks and scheduler-written state before drawing.
        app.poll_tasks();
        app.poll_external_state();

        terminal.draw(|frame| ui::draw(frame, &mut app))?;

        match next_event()? {
            AppEvent::Key(key) => app.handle_key(key),
            AppEvent::Mouse(mouse) => app.handle_mouse(mouse),
            AppEvent::Resize => {
                // Ratatui handles resize automatically on the next draw.
            }
            AppEvent::Tick => app.tick(),
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
    let result = (|| {
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        Terminal::new(backend)
    })();
    if result.is_err() {
        // Best effort: setup may have failed after mouse capture or the
        // alternate screen was enabled.
        let _ = restore_terminal();
    }
    result
}

/// Leave the alternate screen and disable raw mode.
///
/// Called on normal exit and from the panic hook.
fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), DisableMouseCapture, LeaveAlternateScreen)?;
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
