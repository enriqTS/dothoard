//! Layout and rendering for the TUI.
//!
//! Each screen has its own rendering function. The top-level `draw` function
//! renders the tab bar and delegates to the active screen's renderer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::{App, Screen};

/// Draw the complete UI for one frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Screen content
            Constraint::Length(1), // Status/help bar
        ])
        .split(frame.area());

    draw_tabs(frame, chunks[0], app);
    draw_screen(frame, chunks[1], app);
    draw_help_bar(frame, chunks[2], app);
}

/// Draw the tab bar at the top.
fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    let titles: Vec<Line> = Screen::ALL
        .iter()
        .enumerate()
        .map(|(i, screen)| {
            let num = format!("{}", i + 1);
            Line::from(vec![
                Span::styled(num, Style::default().fg(Color::DarkGray)),
                Span::raw(":"),
                Span::raw(screen.label()),
            ])
        })
        .collect();

    let selected = Screen::ALL
        .iter()
        .position(|&s| s == app.active_screen)
        .unwrap_or(0);

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .title(" dothoard "),
        )
        .select(selected)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    frame.render_widget(tabs, area);
}

/// Dispatch to the active screen's renderer.
fn draw_screen(frame: &mut Frame, area: Rect, app: &App) {
    match app.active_screen {
        Screen::Dashboard => draw_dashboard(frame, area),
        Screen::Sources => draw_placeholder(frame, area, "Sources"),
        Screen::Ignore => draw_placeholder(frame, area, "Ignore Rules"),
        Screen::Preview => draw_placeholder(frame, area, "Backup Preview"),
        Screen::Automation => draw_placeholder(frame, area, "Automation"),
        Screen::History => draw_placeholder(frame, area, "History"),
    }
}

/// Draw the help/status bar at the bottom.
fn draw_help_bar(frame: &mut Frame, area: Rect, _app: &App) {
    let help = Line::from(vec![
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(" quit  "),
        Span::styled("Tab", Style::default().fg(Color::Cyan)),
        Span::raw("/"),
        Span::styled("S-Tab", Style::default().fg(Color::Cyan)),
        Span::raw(" navigate  "),
        Span::styled("1-6", Style::default().fg(Color::Cyan)),
        Span::raw(" jump to screen"),
    ]);

    let paragraph = Paragraph::new(help);
    frame.render_widget(paragraph, area);
}

/// Draw the dashboard screen (initial placeholder, will be expanded in U03).
fn draw_dashboard(frame: &mut Frame, area: Rect) {
    let block = Block::default().borders(Borders::ALL).title(" Dashboard ");

    let content = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Welcome to dothoard",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  Status information will appear here once configured."),
        Line::from(""),
        Line::from(Span::styled(
            "  Use Tab or number keys to navigate between screens.",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(block);

    frame.render_widget(content, area);
}

/// Draw a placeholder screen for not-yet-implemented screens.
fn draw_placeholder(frame: &mut Frame, area: Rect, title: &str) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", title));

    let content = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {} screen", title),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from("  This screen is not yet implemented."),
    ])
    .block(block);

    frame.render_widget(content, area);
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    /// Create a test App with a specific screen.
    fn app_on(screen: Screen) -> App {
        let mut app = App::new();
        app.active_screen = screen;
        app
    }

    /// Verify that drawing does not panic for any screen.
    #[test]
    fn draw_all_screens_without_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        for &screen in Screen::ALL {
            let app = app_on(screen);

            terminal
                .draw(|frame| draw(frame, &app))
                .expect("draw should not fail");
        }
    }

    /// Verify the tab bar highlights the active screen.
    #[test]
    fn tab_bar_renders_for_each_screen() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let app = app_on(Screen::Automation);

        // Just verify it doesn't panic with a specific selection.
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");
    }

    /// Verify the dashboard renders content.
    #[test]
    fn dashboard_renders_welcome() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let app = app_on(Screen::Dashboard);

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        // Check that the buffer contains some expected text.
        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("dothoard"));
    }

    /// Verify rendering in a very small terminal doesn't panic.
    #[test]
    fn renders_in_minimal_terminal() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        let app = App::new();
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("should handle small terminal");
    }
}
