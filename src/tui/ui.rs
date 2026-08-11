//! Layout and rendering for the TUI.
//!
//! Each screen has its own rendering function. The top-level `draw` function
//! renders the tab bar and delegates to the active screen's renderer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::{App, Screen, text};

/// Draw the complete UI for one frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Screen content
            Constraint::Length(1), // Transient status/progress
            Constraint::Length(1), // Contextual help
        ])
        .split(frame.area());

    draw_tabs(frame, chunks[0], app);
    draw_screen(frame, chunks[1], app);
    draw_status_bar(frame, chunks[2], app);
    draw_help_bar(frame, chunks[3], app);
}

/// Draw the tab bar at the top.
fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    use super::Focus;

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

    // Highlight style depends on whether the tab bar has focus.
    let highlight_style = match app.focus {
        Focus::TabBar => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        Focus::Content => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    };

    // Border style: brighter when focused.
    let border_style = match app.focus {
        Focus::TabBar => Style::default().fg(Color::Cyan),
        Focus::Content => Style::default().fg(Color::DarkGray),
    };

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(border_style)
                .title(" dothoard "),
        )
        .select(selected)
        .style(Style::default().fg(Color::White))
        .highlight_style(highlight_style);

    frame.render_widget(tabs, area);
}

/// Dispatch to the active screen's renderer.
fn draw_screen(frame: &mut Frame, area: Rect, app: &mut App) {
    match app.active_screen {
        Screen::Dashboard => draw_dashboard(frame, area, app),
        Screen::Repository => draw_repository(frame, area, app),
        Screen::Sources => draw_sources(frame, area, app),
        Screen::Ignore => draw_ignore(frame, area, app),
        Screen::Preview => draw_preview(frame, area, app),
        Screen::Automation => draw_automation(frame, area, app),
        Screen::History => draw_history(frame, area, app),
    }
}

/// Border style for the active screen block, based on focus.
fn content_border_style(app: &App) -> Style {
    match app.focus {
        super::Focus::Content => Style::default().fg(Color::Cyan),
        super::Focus::TabBar => Style::default().fg(Color::DarkGray),
    }
}

/// Draw transient feedback and progress independently from keyboard help.
fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    use super::status::{StatusKind, StatusMessage};

    let fallback = if let Some(message) = app.sources_screen.message.as_ref() {
        let kind = match message.kind {
            crate::tui::screens::sources::MessageKind::Info => StatusKind::Success,
            crate::tui::screens::sources::MessageKind::Warning => StatusKind::Warning,
            crate::tui::screens::sources::MessageKind::Error => StatusKind::Error,
        };
        Some(StatusMessage::new(kind, &message.text))
    } else if let Some(message) = app.ignore_screen.message.as_ref() {
        Some(StatusMessage::new(
            match message.kind {
                crate::tui::screens::ignore::MessageKind::Success => StatusKind::Success,
                crate::tui::screens::ignore::MessageKind::Error => StatusKind::Error,
            },
            &message.text,
        ))
    } else if let Some(message) = app.automation_screen.message.as_ref() {
        Some(StatusMessage::new(
            if message.success {
                StatusKind::Success
            } else {
                StatusKind::Error
            },
            &message.text,
        ))
    } else if app.tasks.is_busy() {
        let operation = app.tasks.active_task().map_or("Task", |kind| match kind {
            super::task::TaskKind::Backup => "Backup",
            super::task::TaskKind::Check => "Check",
            super::task::TaskKind::Push => "Push",
        });
        Some(StatusMessage::running(format!(
            "{operation} in progress..."
        )))
    } else if app.repo_screen.validation.is_loading() {
        Some(StatusMessage::running("Checking repository..."))
    } else if app.preview_screen.load_state.is_loading() {
        Some(StatusMessage::running("Generating backup preview..."))
    } else if app.ignore_screen.preview_state.is_loading() {
        Some(StatusMessage::running("Generating ignore preview..."))
    } else if app.automation_screen.status_state.is_loading() {
        Some(StatusMessage::running("Checking automation..."))
    } else {
        None
    };

    if let Some(message) = app.status_message.as_ref().or(fallback.as_ref()) {
        let color = match message.kind {
            StatusKind::Success => Color::Green,
            StatusKind::Running => Color::Cyan,
            StatusKind::Warning => Color::Yellow,
            StatusKind::Error => Color::Red,
        };
        let rendered = format!(" {}: {}", message.kind.label(), message.text);
        let rendered = text::truncate(&rendered, area.width as usize);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                rendered,
                Style::default().fg(color),
            ))),
            area,
        );
    }
}

/// Draw the authoritative mode-aware shortcut footer.
fn draw_help_bar(frame: &mut Frame, area: Rect, app: &App) {
    use super::Focus;

    let line = match app.focus {
        Focus::TabBar => help_bar_tab_focus(),
        Focus::Content => help_bar_content_focus(app),
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// Help bar when the tab bar has focus.
fn help_bar_tab_focus() -> Line<'static> {
    Line::from(vec![
        Span::styled("←→/hl", Style::default().fg(Color::Cyan)),
        Span::raw(" tabs  "),
        Span::styled("↓/j/Enter", Style::default().fg(Color::Cyan)),
        Span::raw(" content  "),
        Span::styled("1-7", Style::default().fg(Color::Cyan)),
        Span::raw(" jump  "),
        Span::styled("q", Style::default().fg(Color::Cyan)),
        Span::raw(" quit"),
    ])
}

/// Help bar when content has focus.
fn help_bar_content_focus(app: &App) -> Line<'static> {
    match app.active_screen {
        Screen::Dashboard => {
            let mut spans = vec![
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" tabs  "),
            ];
            if !app.tasks.is_busy() {
                spans.extend([
                    Span::styled("b", Style::default().fg(Color::Cyan)),
                    Span::raw(" backup  "),
                    Span::styled("c", Style::default().fg(Color::Cyan)),
                    Span::raw(" check  "),
                ]);
            }
            spans.extend([
                Span::styled("q", Style::default().fg(Color::Cyan)),
                Span::raw(" quit"),
            ]);
            Line::from(spans)
        }
        Screen::Repository => help_bar_repository(app),
        Screen::Sources => help_bar_sources(app),
        Screen::Ignore => help_bar_ignore(app),
        Screen::Preview => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::raw(" refresh  "),
            Span::styled("b", Style::default().fg(Color::Cyan)),
            Span::raw(" backup  "),
            Span::styled("p", Style::default().fg(Color::Cyan)),
            Span::raw(" push  "),
            Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
            Span::raw(" scroll"),
        ]),
        Screen::Automation => {
            use crate::tui::screens::automation::ConfirmAction;
            if app.automation_screen.confirm == ConfirmAction::None {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(Color::Cyan)),
                    Span::raw(" tabs  "),
                    Span::styled("r", Style::default().fg(Color::Cyan)),
                    Span::raw(" refresh  "),
                    Span::styled("i", Style::default().fg(Color::Cyan)),
                    Span::raw(" install  "),
                    Span::styled("x", Style::default().fg(Color::Cyan)),
                    Span::raw(" remove"),
                ])
            } else {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(Color::Cyan)),
                    Span::raw(" tabs  "),
                    Span::styled("y", Style::default().fg(Color::Cyan)),
                    Span::raw(" confirm  "),
                    Span::styled("n/Esc", Style::default().fg(Color::Cyan)),
                    Span::raw(" cancel"),
                ])
            }
        }
        Screen::History => help_bar_history(app),
    }
}

/// Context-sensitive help for the Repository screen.
fn help_bar_repository(app: &App) -> Line<'static> {
    use crate::tui::screens::repository::{ConfirmState, RepoMode};

    if app.repo_screen.confirm_state == ConfirmState::AskInitialize
        || app.repo_screen.confirm_state == ConfirmState::AskAttach
    {
        return Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("y", Style::default().fg(Color::Cyan)),
            Span::raw(" confirm  "),
            Span::styled("n/Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel"),
        ]);
    }

    match app.repo_screen.mode {
        RepoMode::Browser => {
            let mut spans = vec![
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" tabs  "),
                Span::styled("Space", Style::default().fg(Color::Cyan)),
                Span::raw(" select  "),
                Span::styled("n", Style::default().fg(Color::Cyan)),
                Span::raw(" namespace  "),
            ];
            if app.config.is_some() {
                spans.extend([
                    Span::styled("r", Style::default().fg(Color::Cyan)),
                    Span::raw(" rename  "),
                    Span::styled("d", Style::default().fg(Color::Cyan)),
                    Span::raw(" delete  "),
                ]);
            }
            spans.extend([
                Span::styled("↑↓←→", Style::default().fg(Color::Cyan)),
                Span::raw(" navigate  "),
                Span::styled(":/", Style::default().fg(Color::Cyan)),
                Span::raw(" text input"),
            ]);
            Line::from(spans)
        }
        RepoMode::NamespaceInput if app.repo_screen.namespace_confirmation.is_some() => {
            Line::from(vec![
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" tabs  "),
                Span::styled("y", Style::default().fg(Color::Cyan)),
                Span::raw(" confirm  "),
                Span::styled("n/Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" cancel"),
            ])
        }
        RepoMode::NamespaceInput => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" review  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel  "),
            Span::styled("Ctrl+U", Style::default().fg(Color::Cyan)),
            Span::raw(" clear"),
        ]),
        RepoMode::TextInput => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" validate  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" browser  "),
            Span::styled("Ctrl+U", Style::default().fg(Color::Cyan)),
            Span::raw(" clear"),
        ]),
    }
}

/// Context-sensitive help for the Sources screen.
fn help_bar_sources(app: &App) -> Line<'static> {
    use crate::tui::screens::sources::Mode;

    match app.sources_screen.mode {
        Mode::List => {
            let has_sources = app
                .config
                .as_ref()
                .is_some_and(|config| !config.sources.is_empty());
            let mut spans = vec![
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" tabs  "),
                Span::styled("a", Style::default().fg(Color::Cyan)),
                Span::raw(" add"),
            ];
            if has_sources {
                spans.extend([
                    Span::raw("  "),
                    Span::styled("d", Style::default().fg(Color::Cyan)),
                    Span::raw(" delete  "),
                    Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
                    Span::raw(" navigate"),
                ]);
            }
            Line::from(spans)
        }
        Mode::Browse => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("Space", Style::default().fg(Color::Cyan)),
            Span::raw(" toggle  "),
            Span::styled("↑↓←→", Style::default().fg(Color::Cyan)),
            Span::raw(" navigate  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" review changes  "),
            Span::styled(":/", Style::default().fg(Color::Cyan)),
            Span::raw(" text"),
        ]),
        Mode::AddInput => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" add  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" browser  "),
            Span::styled("Ctrl+U", Style::default().fg(Color::Cyan)),
            Span::raw(" clear"),
        ]),
        Mode::ConfirmDelete => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("y", Style::default().fg(Color::Cyan)),
            Span::raw(" confirm  "),
            Span::styled("n/Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel"),
        ]),
        Mode::PendingChanges => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("a", Style::default().fg(Color::Cyan)),
            Span::raw(" apply  "),
            Span::styled("d", Style::default().fg(Color::Cyan)),
            Span::raw(" discard  "),
            Span::styled("c/Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" continue editing"),
        ]),
        Mode::ConfirmApply => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("y", Style::default().fg(Color::Cyan)),
            Span::raw(" remove and apply  "),
            Span::styled("n/Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" back to choices"),
        ]),
    }
}

/// Context-sensitive help for the Ignore screen.
fn help_bar_ignore(app: &App) -> Line<'static> {
    use crate::tui::screens::ignore::Mode;

    match app.ignore_screen.mode {
        Mode::AddInput => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" add  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" cancel  "),
            Span::styled("Ctrl+U", Style::default().fg(Color::Cyan)),
            Span::raw(" clear"),
        ]),
        Mode::Preview => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" back  "),
            Span::styled("r", Style::default().fg(Color::Cyan)),
            Span::raw(" refresh  "),
            Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
            Span::raw(" scroll  "),
            Span::styled("PgUp/PgDn", Style::default().fg(Color::Cyan)),
            Span::raw(" page"),
        ]),
        Mode::List => {
            let has_sources = app
                .config
                .as_ref()
                .is_some_and(|config| !config.sources.is_empty());
            if !has_sources {
                return Line::from(vec![
                    Span::styled("Tab", Style::default().fg(Color::Cyan)),
                    Span::raw(" tabs"),
                ]);
            }
            let has_patterns = app
                .config
                .as_ref()
                .and_then(|config| config.sources.get(app.ignore_screen.source_idx))
                .is_some_and(|source| !source.ignore.is_empty());
            let mut spans = vec![
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" tabs  "),
                Span::styled("a", Style::default().fg(Color::Cyan)),
                Span::raw(" add  "),
            ];
            if has_patterns {
                spans.extend([
                    Span::styled("d", Style::default().fg(Color::Cyan)),
                    Span::raw(" delete  "),
                ]);
            }
            spans.extend([
                Span::styled("p", Style::default().fg(Color::Cyan)),
                Span::raw(" preview  "),
                Span::styled("←→/hl", Style::default().fg(Color::Cyan)),
                Span::raw(" source  "),
                Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
                Span::raw(" focus/item"),
            ]);
            Line::from(spans)
        }
    }
}

/// Context-sensitive help for the History screen.
fn help_bar_history(app: &App) -> Line<'static> {
    use crate::tui::screens::history::Mode;

    match app.history_screen.mode {
        Mode::LogView => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" back  "),
            Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
            Span::raw(" scroll  "),
            Span::styled("PgUp/PgDn", Style::default().fg(Color::Cyan)),
            Span::raw(" page"),
        ]),
        Mode::History => {
            let has_history = app
                .state
                .as_ref()
                .is_some_and(|state| !state.history.is_empty());
            let mut spans = vec![
                Span::styled("Tab", Style::default().fg(Color::Cyan)),
                Span::raw(" tabs  "),
            ];
            if has_history {
                spans.extend([
                    Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
                    Span::raw(" navigate  "),
                    Span::styled("Enter", Style::default().fg(Color::Cyan)),
                    Span::raw(" view logs  "),
                ]);
            }
            spans.extend([
                Span::styled("q", Style::default().fg(Color::Cyan)),
                Span::raw(" quit"),
            ]);
            Line::from(spans)
        }
    }
}

/// Draw the dashboard screen with real status information.
fn draw_dashboard(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style(app))
        .title(" Dashboard ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split inner area into two columns: status on left, info on right.
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);

    draw_dashboard_status(frame, columns[0], app);
    draw_dashboard_info(frame, columns[1], app);
}

/// Left column: backup/push/commit status.
fn draw_dashboard_status(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    // Running indicator
    if app.tasks.is_busy() {
        let kind = match app.tasks.active_task() {
            Some(super::task::TaskKind::Backup) => "backup",
            Some(super::task::TaskKind::Check) => "check",
            Some(super::task::TaskKind::Push) => "push",
            None => "task",
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("● ", Style::default().fg(Color::Yellow)),
            Span::styled(
                format!("Running {kind}..."),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(""));
    }

    // Repository
    lines.push(section_header("Repository"));
    if let Some(ref config) = app.config {
        lines.push(field_line("  Path", config.repository.clone()));
        lines.push(field_line("  Namespace", config.namespace.clone()));
        lines.push(field_line("  Remote", config.remote.clone()));
    } else {
        lines.push(dim_line("  Not configured"));
    }
    lines.push(Line::from(""));

    // Last backup
    lines.push(section_header("Backup"));
    if let Some(ref state) = app.state {
        if let Some(ref ts) = state.last_success {
            lines.push(field_line("  Last success", format_time(ts)));
        } else {
            lines.push(dim_line("  No successful backup yet"));
        }
        if let Some(ref ts) = state.last_attempt {
            lines.push(field_line("  Last attempt", format_time(ts)));
        }
    } else {
        lines.push(dim_line("  No state available"));
    }
    lines.push(Line::from(""));

    // Last commit
    lines.push(section_header("Commit"));
    if let Some(ref state) = app.state {
        if let Some(ref sha) = state.last_commit {
            let short = text::prefix(sha, 8);
            lines.push(field_line("  Last SHA", short));
        } else {
            lines.push(dim_line("  No commits yet"));
        }
    } else {
        lines.push(dim_line("  —"));
    }
    lines.push(Line::from(""));

    // Push status
    lines.push(section_header("Push"));
    if let Some(ref state) = app.state {
        if let Some(ref ts) = state.last_push {
            lines.push(field_line("  Last push", format_time(ts)));
        }
        if state.pending_push {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "⚠ Pending commits not yet pushed",
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        } else if state.last_push.is_some() {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("✓ Up to date", Style::default().fg(Color::Green)),
            ]));
        } else {
            lines.push(dim_line("  No push yet"));
        }
    } else {
        lines.push(dim_line("  —"));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Right column: timer, errors, config summary.
fn draw_dashboard_info(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines: Vec<Line> = Vec::new();

    // Timer / automation
    lines.push(section_header("Automation"));
    if let Some(ref config) = app.config {
        lines.push(field_line(
            "  Interval",
            format!("{} min", config.interval_minutes),
        ));
        lines.push(field_line(
            "  Timeout",
            format!("{}s", config.network_timeout_seconds),
        ));
    } else {
        lines.push(dim_line("  Not configured"));
    }
    lines.push(Line::from(""));

    // Sources
    lines.push(section_header("Sources"));
    if let Some(ref config) = app.config {
        if config.sources.is_empty() {
            lines.push(dim_line("  No sources configured"));
        } else {
            for (i, src) in config.sources.iter().enumerate().take(6) {
                if i == 5 && config.sources.len() > 6 {
                    lines.push(dim_line(format!(
                        "  ...and {} more",
                        config.sources.len() - 5
                    )));
                    break;
                }
                lines.push(field_line("  ", src.path.clone()));
            }
        }
    } else {
        lines.push(dim_line("  —"));
    }
    lines.push(Line::from(""));

    // Latest error
    lines.push(section_header("Latest Error"));
    if let Some(ref state) = app.state {
        if let Some(ref err) = state.latest_error {
            let display = text::truncate(err, 60);
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(display, Style::default().fg(Color::Red)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("None", Style::default().fg(Color::Green)),
            ]));
        }
    } else {
        lines.push(dim_line("  —"));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Create a section header line.
fn section_header(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ))
}

/// Create a "label: value" field line with owned strings.
fn field_line(label: &'static str, value: impl Into<String>) -> Line<'static> {
    let val: String = value.into();
    Line::from(vec![
        Span::styled(label, Style::default().fg(Color::DarkGray)),
        Span::raw(": "),
        Span::raw(val),
    ])
}

/// Create a dim informational line with an owned string.
fn dim_line(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(
        text.into(),
        Style::default().fg(Color::DarkGray),
    ))
}

/// Format a DateTime for display.
fn format_time(ts: &chrono::DateTime<chrono::Utc>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S UTC").to_string()
}

/// Draw the history screen with recent runs and detail for the selected entry.
fn draw_history(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::tui::screens::history::{HistoryScreen, Mode};

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style(app))
        .title(" History ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let history = app
        .state
        .as_ref()
        .map(|s| s.history.as_slice())
        .unwrap_or(&[]);

    let mut lines: Vec<Line> = Vec::new();

    if history.is_empty() {
        app.history_screen.set_list_viewport_height(0, 0);
        lines.push(Line::from(""));
        lines.push(dim_line("  No backup history available."));
        lines.push(Line::from(""));
        lines.push(dim_line("  Run a backup to see results here."));
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
        return;
    }

    // If in log view mode, show the log viewer.
    if app.history_screen.mode == Mode::LogView {
        draw_log_view(frame, inner, app);
        return;
    }

    // Split: list on left, detail on right.
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(inner);

    // Left: list of runs.
    let mut list_lines: Vec<Line> = Vec::new();
    list_lines.push(Line::from(Span::styled(
        " Recent runs:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    list_lines.push(Line::from(""));

    let available_rows = columns[0].height.saturating_sub(2) as usize;
    let show_range = history.len() > available_rows && available_rows > 1;
    let visible_rows = available_rows.saturating_sub(usize::from(show_range));
    app.history_screen
        .set_list_viewport_height(visible_rows, history.len());
    let visible_range = app
        .history_screen
        .list_viewport
        .visible_range(history.len());
    for i in visible_range.clone() {
        let record = &history[i];
        let entry = HistoryScreen::format_entry(record);
        let marker = if i == app.history_screen.selected {
            "▶ "
        } else {
            "  "
        };

        let outcome_color = if entry.is_error {
            Color::Red
        } else if entry.is_warning {
            Color::Yellow
        } else {
            Color::Green
        };

        let style = if i == app.history_screen.selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };

        list_lines.push(Line::from(vec![
            Span::styled(marker, Style::default().fg(Color::Cyan)),
            Span::styled(entry.time.clone(), style),
            Span::raw(" "),
            Span::styled(entry.outcome.clone(), Style::default().fg(outcome_color)),
        ]));
    }

    if show_range {
        list_lines.push(dim_line(format!(
            " [{}-{} of {}]",
            visible_range.start + 1,
            visible_range.end,
            history.len()
        )));
    }

    let list_paragraph = Paragraph::new(list_lines);
    frame.render_widget(list_paragraph, columns[0]);

    // Right: detail of selected entry.
    let mut detail_lines: Vec<Line> = Vec::new();
    if let Some(record) = history.get(app.history_screen.selected) {
        let entry = HistoryScreen::format_entry(record);

        detail_lines.push(Line::from(Span::styled(
            " Details:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        detail_lines.push(Line::from(""));

        let outcome_color = if entry.is_error {
            Color::Red
        } else if entry.is_warning {
            Color::Yellow
        } else {
            Color::Green
        };

        detail_lines.push(Line::from(vec![
            Span::styled(" Outcome: ", Style::default().fg(Color::DarkGray)),
            Span::styled(entry.outcome, Style::default().fg(outcome_color)),
        ]));
        detail_lines.push(field_line(" Started", entry.time));
        detail_lines.push(field_line(" Duration", entry.duration));

        if let Some(ref sha) = entry.commit {
            let short = text::prefix(sha, 8);
            detail_lines.push(field_line(" Commit", short));
        }

        if let Some(ref msg) = entry.message {
            detail_lines.push(Line::from(""));
            detail_lines.push(Line::from(Span::styled(
                " Message:",
                Style::default().fg(Color::DarkGray),
            )));
            // Wrap long messages without splitting UTF-8 characters.
            let max_width = columns[1].width.saturating_sub(3) as usize;
            for message_line in text::wrap(msg, max_width) {
                detail_lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        message_line,
                        Style::default().fg(if entry.is_error {
                            Color::Red
                        } else {
                            Color::Yellow
                        }),
                    ),
                ]));
            }
        }
    }

    let detail_paragraph = Paragraph::new(detail_lines);
    frame.render_widget(detail_paragraph, columns[1]);
}

/// Draw the log view for a selected history entry.
fn draw_log_view(frame: &mut Frame, area: Rect, app: &mut App) {
    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(Span::styled(
        " Log View:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    if app.history_screen.log_lines.is_empty() {
        lines.push(dim_line("  No log entries found for this run."));
        lines.push(Line::from(""));
        lines.push(dim_line(
            "  Logs may have been rotated or the log file is not available.",
        ));
    } else {
        let available_rows = area.height.saturating_sub(2) as usize;
        let show_range = app.history_screen.log_lines.len() > available_rows && available_rows > 1;
        let visible_rows = available_rows.saturating_sub(usize::from(show_range));
        app.history_screen.set_log_viewport_height(visible_rows);
        let visible_range = app
            .history_screen
            .log_viewport
            .visible_range(app.history_screen.log_lines.len());

        for line in &app.history_screen.log_lines[visible_range.clone()] {
            // Truncate very long lines to prevent wrapping issues.
            let display_line = text::truncate(line, 200);
            lines.push(Line::from(Span::raw(display_line)));
        }

        if show_range {
            lines.push(dim_line(format!(
                "  [{}-{} of {}]",
                visible_range.start + 1,
                visible_range.end,
                app.history_screen.log_lines.len()
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

/// Draw the automation controls screen.
fn draw_automation(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::screens::automation::ConfirmAction;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style(app))
        .title(" Automation ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Systemd Timer",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Status.
    use crate::tui::task::LoadState;
    match &app.automation_screen.status_state {
        LoadState::NotLoaded => {
            lines.push(dim_line(
                "  Status not loaded. Entering this screen checks it.",
            ));
        }
        LoadState::Loading { previous, .. } => {
            lines.push(Line::from(Span::styled(
                "  Checking automation status...",
                Style::default().fg(Color::Yellow),
            )));
            if let Some(status) = previous {
                lines.push(field_line("  Previous", status.clone()));
            }
        }
        LoadState::Loaded(status) => lines.push(field_line("  Status", status.clone())),
        LoadState::Stale { previous } => {
            lines.push(dim_line("  Automation status is stale."));
            if let Some(status) = previous {
                lines.push(field_line("  Previous", status.clone()));
            }
        }
        LoadState::Failed { error, previous } => {
            lines.push(Line::from(Span::styled(
                format!("  Status check failed: {error}"),
                Style::default().fg(Color::Red),
            )));
            if let Some(status) = previous {
                lines.push(field_line("  Previous", status.clone()));
            }
        }
    }

    // Config info.
    if let Some(ref config) = app.config {
        lines.push(Line::from(""));
        lines.push(field_line(
            "  Interval",
            format!("{} min", config.interval_minutes),
        ));
        lines.push(field_line(
            "  Timeout",
            format!("{}s", config.network_timeout_seconds),
        ));
    }

    lines.push(Line::from(""));

    // Confirmation dialogs.
    match app.automation_screen.confirm {
        ConfirmAction::Install => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Install and enable the timer?",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        ConfirmAction::Remove => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Remove the timer?",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        ConfirmAction::None => {}
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the backup preview screen.
fn draw_preview(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::screens::preview::EntryKind;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style(app))
        .title(" Backup Preview ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    use crate::tui::task::LoadState;
    match &app.preview_screen.load_state {
        LoadState::Loading { .. } => lines.push(Line::from(Span::styled(
            "  Generating preview...",
            Style::default().fg(Color::Yellow),
        ))),
        LoadState::Stale { .. } => {
            lines.push(dim_line("  Preview is stale."));
        }
        LoadState::Failed { error, .. } => lines.push(Line::from(Span::styled(
            format!("  Preview failed: {error}"),
            Style::default().fg(Color::Red),
        ))),
        LoadState::NotLoaded | LoadState::Loaded(_) => {}
    }

    let Some(data) = app.preview_screen.load_state.data() else {
        lines.push(Line::from(""));
        let message = if app.preview_screen.load_state.is_loading() {
            "  Waiting for the backup planner."
        } else {
            "  Preview has not been generated."
        };
        lines.push(dim_line(message));
        lines.push(Line::from(""));
        lines.push(dim_line(
            "  This runs the backup planner without making changes.",
        ));
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    };

    // Summary line.
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!(
                "+{} ~{} -{} ○{} ⚠{}",
                data.additions, data.modifications, data.deletions, data.exclusions, data.warnings
            ),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "  ({} total operations)",
            data.additions + data.modifications + data.deletions
        )),
    ]));
    lines.push(Line::from(""));

    if data.entries.is_empty() {
        lines.push(dim_line("  No changes detected. Everything is up to date."));
    } else {
        // Display entries with scroll.
        let visible_height = inner.height.saturating_sub(6) as usize;
        let scroll = app
            .preview_screen
            .scroll
            .min(data.entries.len().saturating_sub(visible_height));

        for entry in data.entries.iter().skip(scroll).take(visible_height) {
            let (prefix_color, path_style) = match entry.kind {
                EntryKind::Addition => (Color::Green, Style::default().fg(Color::Green)),
                EntryKind::Modification => (Color::Yellow, Style::default().fg(Color::Yellow)),
                EntryKind::Deletion => (Color::Red, Style::default().fg(Color::Red)),
                EntryKind::Exclusion => (Color::DarkGray, Style::default().fg(Color::DarkGray)),
                EntryKind::Warning => (Color::Yellow, Style::default().fg(Color::Yellow)),
            };

            let mut spans = vec![
                Span::raw("  "),
                Span::styled(
                    format!("{} ", entry.kind.prefix()),
                    Style::default().fg(prefix_color),
                ),
                Span::styled(entry.path.clone(), path_style),
            ];

            if let Some(ref detail) = entry.detail {
                spans.push(Span::styled(
                    format!("  ({})", detail),
                    Style::default().fg(Color::DarkGray),
                ));
            }

            lines.push(Line::from(spans));
        }

        // Scroll indicator.
        if data.entries.len() > visible_height {
            lines.push(Line::from(""));
            lines.push(dim_line(format!(
                "  [{}-{} of {}]",
                scroll + 1,
                (scroll + visible_height).min(data.entries.len()),
                data.entries.len()
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the ignore rule editor screen.
fn draw_ignore(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::tui::screens::ignore::Mode;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style(app))
        .title(" Ignore Rules ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sources = app
        .config
        .as_ref()
        .map(|c| c.sources.as_slice())
        .unwrap_or(&[]);

    let mut lines: Vec<Line> = Vec::new();

    if sources.is_empty() {
        lines.push(Line::from(""));
        lines.push(dim_line("  No sources configured. Add sources first."));
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
        return;
    }

    // Source selector.
    let source_tabs: Vec<Span> = sources
        .iter()
        .enumerate()
        .flat_map(|(i, s)| {
            let style = if i == app.ignore_screen.source_idx {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            vec![Span::styled(format!(" {} ", s.path), style), Span::raw("|")]
        })
        .collect();

    if app.ignore_screen.mode == Mode::Preview {
        lines.push(Line::from(source_tabs));
        lines.push(Line::from(Span::styled(
            " File Preview:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        use crate::tui::task::LoadState;
        match &app.ignore_screen.preview_state {
            LoadState::Loading { .. } => lines.push(Line::from(Span::styled(
                "  Generating ignore preview...",
                Style::default().fg(Color::Yellow),
            ))),
            LoadState::Stale { .. } => lines.push(dim_line("  Preview is stale.")),
            LoadState::Failed { error, .. } => lines.push(Line::from(Span::styled(
                format!("  Preview failed: {error}"),
                Style::default().fg(Color::Red),
            ))),
            LoadState::NotLoaded | LoadState::Loaded(_) => {}
        }

        let available_rows = (inner.height as usize).saturating_sub(lines.len());
        let preview_len = app.ignore_screen.preview().map_or(0, <[_]>::len);

        if preview_len == 0 {
            app.ignore_screen.set_preview_viewport_height(0);
            let message = if app.ignore_screen.preview_state.is_loading() {
                "  Waiting for filesystem scan."
            } else {
                "  No files found."
            };
            lines.push(dim_line(message));
        } else {
            let show_range = preview_len > available_rows && available_rows > 1;
            let visible_rows = available_rows.saturating_sub(usize::from(show_range));
            app.ignore_screen.set_preview_viewport_height(visible_rows);
            let visible_range = app
                .ignore_screen
                .preview_viewport
                .visible_range(preview_len);
            let preview = app.ignore_screen.preview().unwrap_or_default();

            for entry in &preview[visible_range.clone()] {
                let mut spans = vec![Span::raw("  ")];

                if entry.ignored {
                    spans.push(Span::styled("✗ ", Style::default().fg(Color::Red)));
                    spans.push(Span::styled(
                        entry.path.clone(),
                        Style::default().fg(Color::DarkGray),
                    ));
                    if let Some(ref pat) = entry.matched_by {
                        spans.push(Span::styled(
                            format!("  ({pat})"),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                } else {
                    spans.push(Span::styled("✓ ", Style::default().fg(Color::Green)));
                    spans.push(Span::raw(entry.path.clone()));
                }

                if entry.secret_warning {
                    spans.push(Span::styled(
                        "  ⚠ secret",
                        Style::default().fg(Color::Yellow),
                    ));
                }

                lines.push(Line::from(spans));
            }
            if show_range {
                lines.push(dim_line(format!(
                    "  [{}-{} of {}]",
                    visible_range.start + 1,
                    visible_range.end,
                    preview_len
                )));
            }
        }

        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }

    lines.push(Line::from(""));
    lines.push(Line::from(source_tabs));
    lines.push(Line::from(""));

    // Current source's patterns.
    let current_source = &sources[app.ignore_screen.source_idx];
    if current_source.ignore.is_empty() {
        lines.push(dim_line("  No ignore patterns."));
    } else {
        lines.push(Line::from(Span::styled(
            "  Patterns:",
            Style::default().fg(Color::Cyan),
        )));
        for (i, pattern) in current_source.ignore.iter().enumerate() {
            let marker = if i == app.ignore_screen.pattern_idx {
                "▶ "
            } else {
                "  "
            };
            let style = if i == app.ignore_screen.pattern_idx {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                Span::styled(pattern.clone(), style),
            ]));
        }
    }

    // Input area in add mode.
    if app.ignore_screen.mode == Mode::AddInput {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  New pattern (gitignore syntax):",
            Style::default().fg(Color::Cyan),
        )));
        let input_display = format!("  > {}", app.ignore_screen.input);
        lines.push(Line::from(Span::raw(input_display)));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the sources management screen.
fn draw_sources(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::tui::screens::sources::Mode;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style(app))
        .title(" Sources ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // In Browse mode, show the filesystem picker.
    if app.sources_screen.mode == Mode::Browse {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),    // Browser
                Constraint::Length(1), // Selection summary
            ])
            .split(inner);

        if let Some(ref mut browser) = app.sources_screen.browser {
            let check_fn: Option<&dyn Fn(&std::path::Path) -> crate::tui::selection::CheckState> =
                match app.sources_screen.selection {
                    Some(ref sel) => Some(&|path: &std::path::Path| sel.is_selected(path)),
                    None => None,
                };
            crate::tui::picker::draw(frame, chunks[0], browser, check_fn);
        } else {
            let msg = Paragraph::new(Line::from(Span::styled(
                " Loading browser...",
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(msg, chunks[0]);
        }

        let summary = app.sources_screen.selection.as_ref().map_or_else(
            || " Selection not initialized".to_string(),
            |selection| {
                let (selected, excluded) = selection.summary();
                if excluded > 0 {
                    format!(" {selected} sources, {excluded} excluded")
                } else {
                    format!(" {selected} sources selected")
                }
            },
        );
        let paragraph = Paragraph::new(Line::from(Span::styled(
            summary,
            Style::default().fg(Color::Cyan),
        )));
        frame.render_widget(paragraph, chunks[1]);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    let sources = app
        .config
        .as_ref()
        .map(|c| c.sources.as_slice())
        .unwrap_or(&[]);

    if sources.is_empty() {
        lines.push(Line::from(""));
        lines.push(dim_line("  No sources configured."));
        lines.push(Line::from(""));
        lines.push(dim_line("  Add a source to begin backing up files."));
    } else {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Configured sources:",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        for (i, src) in sources.iter().enumerate() {
            let marker = if i == app.sources_screen.selected {
                "▶ "
            } else {
                "  "
            };
            let style = if i == app.sources_screen.selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(marker, Style::default().fg(Color::Cyan)),
                Span::styled(src.path.clone(), style),
                if !src.ignore.is_empty() {
                    Span::styled(
                        format!("  ({} ignore rules)", src.ignore.len()),
                        Style::default().fg(Color::DarkGray),
                    )
                } else {
                    Span::raw("")
                },
            ]));
        }
    }

    // Show input area in add mode.
    if app.sources_screen.mode == Mode::AddInput {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  New source (relative to $HOME):",
            Style::default().fg(Color::Cyan),
        )));
        let input_display = format!("  > {}", app.sources_screen.input);
        lines.push(Line::from(Span::raw(input_display)));
    }

    // Show confirm delete dialog.
    if app.sources_screen.mode == Mode::ConfirmDelete {
        let path = sources
            .get(app.sources_screen.selected)
            .map(|s| s.path.as_str())
            .unwrap_or("?");
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("Remove source '{path}'?"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Show the explicit pending-change choice before any apply occurs.
    if app.sources_screen.mode == Mode::PendingChanges {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Pending source changes require a decision.",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Show removal confirmation after the user explicitly chooses apply.
    if app.sources_screen.mode == Mode::ConfirmApply {
        lines.push(Line::from(""));
        if let Some(ref diff) = app.sources_screen.pending_diff {
            let mut parts: Vec<String> = Vec::new();
            if !diff.additions.is_empty() {
                parts.push(format!("add {}", diff.additions.len()));
            }
            if !diff.removals.is_empty() {
                parts.push(format!("remove {}", diff.removals.len()));
            }
            let rule_count: usize = diff.ignore_rules.values().map(|v| v.len()).sum();
            if rule_count > 0 {
                parts.push(format!("{rule_count} ignore rules"));
            }
            let summary = parts.join(", ");
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("Remove sources and apply changes ({summary})?"),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));

            // Show removals detail.
            if !diff.removals.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    "  Sources to remove:",
                    Style::default().fg(Color::Red),
                )));
                for removal in &diff.removals {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::styled(format!("- {removal}"), Style::default().fg(Color::Red)),
                    ]));
                }
            }
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Apply changes?",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the repository selection screen.
fn draw_repository(frame: &mut Frame, area: Rect, app: &mut App) {
    use crate::tui::screens::repository::RepoMode;
    use crate::tui::task::LoadState;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(content_border_style(app))
        .title(" Repository ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    match app.repo_screen.mode {
        RepoMode::Browser => {
            // Split: browser takes most space, status/validation at the bottom.
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),    // Browser
                    Constraint::Length(4), // Status/validation area
                ])
                .split(inner);

            // Draw the filesystem browser.
            if let Some(ref mut browser) = app.repo_screen.browser {
                crate::tui::picker::draw(frame, chunks[0], browser, None);
            } else {
                let msg = Paragraph::new(Line::from(Span::styled(
                    " Press Enter or ↓ to start browsing",
                    Style::default().fg(Color::DarkGray),
                )));
                frame.render_widget(msg, chunks[0]);
            }

            // Status/validation area.
            let mut lines: Vec<Line> = Vec::new();
            if let Some(ref config) = app.config {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("Namespace: ", Style::default().fg(Color::Cyan)),
                    Span::styled(config.namespace.clone(), Style::default().fg(Color::White)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("Namespace: ", Style::default().fg(Color::Cyan)),
                    Span::styled(
                        app.repo_screen.namespace_input.clone(),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }

            if let Some(ref err) = app.repo_screen.selection_error {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(format!("✗ {err}"), Style::default().fg(Color::Red)),
                ]));
            }

            match &app.repo_screen.validation {
                LoadState::Loaded(info) => {
                    lines.push(Line::from(vec![
                        Span::raw(" "),
                        Span::styled("✓ Valid repository", Style::default().fg(Color::Green)),
                        Span::raw(" — "),
                        Span::styled(&info.branch, Style::default().fg(Color::Cyan)),
                    ]));
                    draw_ownership_line(&info.ownership, &mut lines);
                }
                LoadState::Loading { .. } => lines.push(Line::from(Span::styled(
                    " Checking repository...",
                    Style::default().fg(Color::Yellow),
                ))),
                LoadState::Failed { error, .. } => lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(format!("✗ {error}"), Style::default().fg(Color::Red)),
                ])),
                LoadState::Stale { .. } => lines.push(dim_line(
                    " Repository validation is stale. Select or validate again.",
                )),
                LoadState::NotLoaded => {
                    lines.push(dim_line(" Select an existing Git worktree to validate it."))
                }
            }

            // Confirmation dialog overlay.
            draw_confirm_line(&app.repo_screen.confirm_state, &mut lines);

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, chunks[1]);
        }
        RepoMode::NamespaceInput => {
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(
                format!("  Namespace ({}):", app.repo_screen.namespace_action_name()),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(format!(
                "  > {}",
                app.repo_screen.namespace_input
            )));
            if let Some(ref config) = app.config {
                let home = app
                    .paths
                    .as_ref()
                    .map(|p| p.home())
                    .unwrap_or(std::path::Path::new("."));
                let namespace_path = if app.repo_screen.namespace_action
                    == crate::tui::screens::repository::NamespaceAction::Delete
                {
                    &app.repo_screen.namespace_origin
                } else {
                    &app.repo_screen.namespace_input
                };
                let root = config.repository_path(home).join(namespace_path);
                lines.push(dim_line(format!(
                    "  Affected: {}/home and {}/.dothoard-manifest.toml",
                    root.display(),
                    root.display()
                )));
            }
            if let Some(ref confirmation) = app.repo_screen.namespace_confirmation {
                lines.push(Line::from(Span::styled(
                    format!("  {confirmation}"),
                    Style::default().fg(Color::Yellow),
                )));
            }
            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }
        RepoMode::TextInput => {
            let mut lines: Vec<Line> = Vec::new();

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Repository path (Esc → browser):",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            // Input field with cursor indicator.
            let input_display = format!("  > {}", app.repo_screen.input);
            lines.push(Line::from(Span::raw(input_display)));

            // Cursor position indicator uses terminal cells rather than bytes.
            let cursor_width =
                text::width_before_cursor(&app.repo_screen.input, app.repo_screen.cursor);
            let cursor_line = format!("  {}^", " ".repeat(cursor_width + 1));
            lines.push(Line::from(Span::styled(
                cursor_line,
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));

            // Validation result.
            match &app.repo_screen.validation {
                LoadState::Loaded(info) => {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("✓ Valid repository", Style::default().fg(Color::Green)),
                    ]));
                    lines.push(field_line("    Branch", info.branch.clone()));
                    lines.push(field_line("    Path", info.path.display().to_string()));
                    lines.push(Line::from(""));
                    draw_ownership_lines(&info.ownership, &mut lines);
                }
                LoadState::Loading { .. } => lines.push(Line::from(Span::styled(
                    "  Checking repository...",
                    Style::default().fg(Color::Yellow),
                ))),
                LoadState::Failed { error, .. } => lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("✗ {error}"), Style::default().fg(Color::Red)),
                ])),
                LoadState::Stale { .. } => {
                    lines.push(dim_line("  Validation is stale. Press Enter to retry."));
                }
                LoadState::NotLoaded => {
                    lines.push(dim_line("  Press Enter to validate the path."));
                }
            }

            // Confirmation dialog.
            draw_confirm_line(&app.repo_screen.confirm_state, &mut lines);

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }
    }
}

/// Draw a one-line ownership summary.
fn draw_ownership_line(
    ownership: &crate::tui::screens::repository::OwnershipInfo,
    lines: &mut Vec<Line>,
) {
    use crate::tui::screens::repository::OwnershipInfo;
    match ownership {
        OwnershipInfo::New => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("New namespace", Style::default().fg(Color::Green)),
            ]));
        }
        OwnershipInfo::Owned { sources } => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("Existing manifest ({} sources)", sources.len()),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }
        OwnershipInfo::InvalidManifest(reason) => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("✗ Invalid manifest: {reason}"),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
        OwnershipInfo::Ambiguous(reason) => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("✗ Ambiguous: {reason}"),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
    }
}

/// Draw ownership info with detail lines (for text input mode).
fn draw_ownership_lines(
    ownership: &crate::tui::screens::repository::OwnershipInfo,
    lines: &mut Vec<Line>,
) {
    use crate::tui::screens::repository::OwnershipInfo;
    match ownership {
        OwnershipInfo::New => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "New namespace — no existing data.",
                    Style::default().fg(Color::Green),
                ),
            ]));
        }
        OwnershipInfo::Owned { sources } => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Existing manifest found.",
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            lines.push(dim_line(format!("    Sources: {}", sources.len())));
            for s in sources.iter().take(5) {
                lines.push(dim_line(format!("      • {s}")));
            }
        }
        OwnershipInfo::InvalidManifest(reason) => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("✗ Invalid manifest: {reason}"),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
        OwnershipInfo::Ambiguous(reason) => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("✗ Ambiguous: {reason}"),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }
    }
}

/// Draw confirmation dialog line.
fn draw_confirm_line(
    confirm_state: &crate::tui::screens::repository::ConfirmState,
    lines: &mut Vec<Line>,
) {
    use crate::tui::screens::repository::ConfirmState;
    match confirm_state {
        ConfirmState::AskInitialize => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Initialize this repository?",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        ConfirmState::AskAttach => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "Attach to this repository?",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        ConfirmState::Done => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    "✓ Repository configured.",
                    Style::default().fg(Color::Green),
                ),
            ]));
        }
        ConfirmState::None => {}
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::tui::task;

    /// Create a test App with a specific screen.
    fn app_on(screen: Screen) -> App {
        let mut app = App::new();
        app.active_screen = screen;
        app
    }

    /// Create an App with mock state for dashboard rendering tests.
    fn app_with_state() -> App {
        use chrono::Utc;

        let mut app = App::new();
        app.state = Some(crate::state::AppState {
            last_attempt: Some(Utc::now()),
            last_success: Some(Utc::now()),
            last_commit: Some("abc123def456".to_string()),
            last_push: Some(Utc::now()),
            pending_push: false,
            latest_warning: None,
            latest_error: None,
            history: Vec::new(),
        });
        app.config = Some(crate::config::Config::new(
            "~/dotfiles-repo",
            "test-machine",
        ));
        app
    }

    /// Verify that drawing does not panic for any screen.
    #[test]
    fn draw_all_screens_without_panic() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        for &screen in Screen::ALL {
            let mut app = app_on(screen);

            terminal
                .draw(|frame| draw(frame, &mut app))
                .expect("draw should not fail");
        }
    }

    /// Verify the tab bar highlights the active screen.
    #[test]
    fn tab_bar_renders_for_each_screen() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Automation);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");
    }

    /// Verify the dashboard renders with state data.
    #[test]
    fn dashboard_renders_with_state() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_with_state();

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Repository"));
        assert!(content.contains("Backup"));
        assert!(content.contains("Commit"));
    }

    #[test]
    fn dashboard_renders_unicode_commit_and_error_safely() {
        let backend = TestBackend::new(50, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_with_state();
        let state = app.state.as_mut().unwrap();
        state.last_commit = Some("界🙂éabcdef".to_string());
        state.latest_error = Some("界🙂e\u{301}".repeat(30));

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("Unicode values should render without slicing panics");
    }

    /// Verify dashboard renders without state (no config, no state).
    #[test]
    fn dashboard_renders_without_state() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Dashboard);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Dashboard"));
    }

    /// Verify dashboard shows pending push warning.
    #[test]
    fn dashboard_shows_pending_push() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_with_state();
        if let Some(ref mut state) = app.state {
            state.pending_push = true;
        }

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Pending"));
    }

    /// Verify dashboard shows error state.
    #[test]
    fn dashboard_shows_error() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_with_state();
        if let Some(ref mut state) = app.state {
            state.latest_error = Some("network timeout".to_string());
        }

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("network timeout"));
    }

    /// Verify running indicator shows during background task.
    #[test]
    fn dashboard_shows_running_indicator() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_with_state();
        app.tasks.active = Some(task::TaskKind::Backup);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Running backup"));
    }

    /// Status feedback and contextual help remain visible at the same time.
    #[test]
    fn status_message_coexists_with_help_footer() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Dashboard);
        app.focus = crate::tui::Focus::Content;
        app.status_message = Some(super::super::status::StatusMessage::warning(
            "Test status message",
        ));

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Warning: Test status message"));
        assert!(content.contains("backup"));
        assert!(content.contains("check"));
    }

    #[test]
    fn narrow_status_is_truncated_without_hiding_help() {
        let backend = TestBackend::new(30, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Dashboard);
        app.focus = crate::tui::Focus::Content;
        app.status_message = Some(super::super::status::StatusMessage::error(
            "a very long failure message that cannot fit",
        ));

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Error: a very long"));
        assert!(content.contains("Tab"));
    }

    #[test]
    fn ignore_footer_changes_for_input_and_preview_modes() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Ignore);
        app.focus = crate::tui::Focus::Content;

        app.ignore_screen.mode = crate::tui::screens::ignore::Mode::AddInput;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let input = buffer_text(terminal.backend());
        assert!(input.contains("Enter"));
        assert!(input.contains("cancel"));
        assert!(!input.contains("preview  "));

        app.ignore_screen.mode = crate::tui::screens::ignore::Mode::Preview;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let preview = buffer_text(terminal.backend());
        assert!(preview.contains("refresh"));
        assert!(preview.contains("scroll"));
        assert!(!preview.contains(" add  "));
    }

    /// Verify rendering in a very small terminal doesn't panic.
    #[test]
    fn renders_in_minimal_terminal() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("should handle small terminal");
    }

    // --- U11: Comprehensive rendering and interaction tests ---

    /// Verify repository screen renders with input and validation states.
    #[test]
    fn repository_screen_renders_input() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Repository);
        app.repo_screen = crate::tui::screens::repository::RepoScreen::with_path("~/my-repo");
        app.repo_screen.mode = crate::tui::screens::repository::RepoMode::TextInput;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Repository"));
        assert!(content.contains("my-repo"));
    }

    #[test]
    fn repository_input_cursor_uses_unicode_display_width() {
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Repository);
        app.repo_screen = crate::tui::screens::repository::RepoScreen::with_path("界e\u{301}");
        app.repo_screen.mode = crate::tui::screens::repository::RepoMode::TextInput;

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let width = terminal.backend().buffer().area.width as usize;
        let caret_column = terminal
            .backend()
            .buffer()
            .content()
            .chunks(width)
            .find_map(|row| row.iter().position(|cell| cell.symbol() == "^"))
            .expect("cursor indicator should be rendered");
        // One border cell plus the six-cell cursor offset inside the panel.
        assert_eq!(caret_column, 7);
    }

    #[test]
    fn source_and_ignore_screens_render_unicode_values_narrowly() {
        let backend = TestBackend::new(32, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_with_state();
        app.config.as_mut().unwrap().sources = vec![crate::config::SourceConfig {
            path: "配置/界e\u{301}".to_string(),
            ignore: vec!["*🙂界é*".to_string()],
        }];

        for screen in [Screen::Sources, Screen::Ignore] {
            app.active_screen = screen;
            terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        }
    }

    /// Verify repository screen shows validation error.
    #[test]
    fn repository_screen_shows_validation_error() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Repository);
        app.repo_screen.validation = crate::tui::task::LoadState::Failed {
            error: "Directory does not exist".to_string(),
            previous: None,
        };

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("does not exist"));
    }

    /// Verify repository screen shows confirmation dialog.
    #[test]
    fn repository_screen_shows_confirm_dialog() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Repository);
        app.repo_screen.confirm_state =
            crate::tui::screens::repository::ConfirmState::AskInitialize;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Initialize"));
    }

    /// Verify sources screen renders with configured sources.
    #[test]
    fn sources_screen_renders_list() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Sources);
        app.config = Some(crate::config::Config {
            version: crate::config::Config::CURRENT_VERSION,
            namespace: "test-machine".to_string(),
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
            sources: vec![
                crate::config::SourceConfig {
                    path: ".config/fish".to_string(),
                    ignore: vec![],
                },
                crate::config::SourceConfig {
                    path: ".bashrc".to_string(),
                    ignore: vec!["*.log".to_string()],
                },
            ],
        });

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains(".config/fish"));
        assert!(content.contains(".bashrc"));
    }

    /// Verify sources screen shows empty state.
    #[test]
    fn sources_screen_renders_empty() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Sources);
        // Clear config sources to test empty state
        if let Some(ref mut config) = app.config {
            config.sources.clear();
        }

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("No sources configured"));
    }

    /// Verify sources screen shows add input mode.
    #[test]
    fn sources_screen_renders_add_mode() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Sources);
        app.sources_screen.mode = crate::tui::screens::sources::Mode::AddInput;
        app.sources_screen.input = ".config/waybar".to_string();

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("waybar"));
    }

    /// Verify ignore screen renders with patterns.
    #[test]
    fn ignore_screen_renders_patterns() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Ignore);
        app.config = Some(crate::config::Config {
            version: crate::config::Config::CURRENT_VERSION,
            namespace: "test-machine".to_string(),
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
            sources: vec![crate::config::SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string(), "fish_variables".to_string()],
            }],
        });

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("*.log"));
        assert!(content.contains("fish_variables"));
    }

    /// Verify ignore screen shows preview mode.
    #[test]
    fn ignore_screen_renders_preview_mode() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Ignore);
        app.config = Some(crate::config::Config {
            version: crate::config::Config::CURRENT_VERSION,
            namespace: "test-machine".to_string(),
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
            sources: vec![crate::config::SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string()],
            }],
        });
        app.ignore_screen.mode = crate::tui::screens::ignore::Mode::Preview;
        app.ignore_screen.replace_preview(vec![
            crate::tui::screens::ignore::PreviewEntry {
                path: "config.fish".to_string(),
                ignored: false,
                matched_by: None,
                secret_warning: false,
            },
            crate::tui::screens::ignore::PreviewEntry {
                path: "fish_history".to_string(),
                ignored: true,
                matched_by: Some("*_history".to_string()),
                secret_warning: false,
            },
        ]);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Preview") || content.contains("config.fish"));
    }

    #[test]
    fn ignore_preview_renders_scrolled_range_from_actual_height() {
        let backend = TestBackend::new(80, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Ignore);
        app.config = Some(crate::config::Config {
            version: crate::config::Config::CURRENT_VERSION,
            namespace: "test-machine".to_string(),
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
            sources: vec![crate::config::SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec!["*.log".to_string()],
            }],
        });
        app.ignore_screen.mode = crate::tui::screens::ignore::Mode::Preview;
        app.ignore_screen.replace_preview(
            (0..30)
                .map(|i| crate::tui::screens::ignore::PreviewEntry {
                    path: format!("file-{i:02}"),
                    ignored: false,
                    matched_by: None,
                    secret_warning: false,
                })
                .collect(),
        );

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let first_page = buffer_text(terminal.backend());
        assert!(first_page.contains("file-00"));
        assert!(first_page.contains("of 30"));

        app.ignore_screen.preview_viewport.end(30);
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let content = buffer_text(terminal.backend());
        assert!(content.contains("file-29"));
        assert!(content.contains("of 30"));
        assert!(!content.contains("file-00"));
    }

    #[test]
    fn ignore_preview_clamps_on_resize() {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Ignore);
        app.config = Some(crate::config::Config::new("~/repo", "test-machine"));
        app.config.as_mut().unwrap().sources = vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }];
        app.ignore_screen.mode = crate::tui::screens::ignore::Mode::Preview;
        app.ignore_screen.replace_preview(
            (0..20)
                .map(|i| crate::tui::screens::ignore::PreviewEntry {
                    path: format!("item-{i:02}"),
                    ignored: false,
                    matched_by: None,
                    secret_warning: false,
                })
                .collect(),
        );

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        app.ignore_screen.preview_viewport.end(20);
        let active_row = app.ignore_screen.preview_viewport.visible_range(20).start;
        terminal.backend_mut().resize(60, 13);
        terminal.autoresize().unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let range = app.ignore_screen.preview_viewport.visible_range(20);
        assert_eq!(range.start, active_row);
        assert!(range.start < range.end);
        assert!(range.end <= 20);
    }

    #[test]
    fn slow_screen_states_render_loading_and_preserve_previous_data() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let request_id = crate::tui::task::RequestId::for_test(1);

        let mut repository = app_on(Screen::Repository);
        repository.repo_screen.mode = crate::tui::screens::repository::RepoMode::TextInput;
        repository.repo_screen.validation = crate::tui::task::LoadState::Loading {
            request_id,
            previous: None,
        };
        terminal.draw(|frame| draw(frame, &mut repository)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("Checking repository"));

        let mut preview = app_on(Screen::Preview);
        preview.preview_screen.load_state = crate::tui::task::LoadState::Loading {
            request_id,
            previous: Some(crate::tui::screens::preview::PreviewData {
                additions: 1,
                modifications: 0,
                deletions: 0,
                exclusions: 0,
                warnings: 0,
                entries: vec![crate::tui::screens::preview::PreviewEntry {
                    kind: crate::tui::screens::preview::EntryKind::Addition,
                    path: "previous-file".to_string(),
                    detail: None,
                }],
            }),
        };
        terminal.draw(|frame| draw(frame, &mut preview)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Generating preview"));
        assert!(content.contains("previous-file"));

        let mut ignore = app_on(Screen::Ignore);
        ignore.config = Some(crate::config::Config::new("~/repo", "test-machine"));
        ignore.config.as_mut().unwrap().sources = vec![crate::config::SourceConfig {
            path: ".config".to_string(),
            ignore: vec![],
        }];
        ignore.ignore_screen.mode = crate::tui::screens::ignore::Mode::Preview;
        ignore.ignore_screen.preview_state = crate::tui::task::LoadState::Loading {
            request_id,
            previous: Some(vec![crate::tui::screens::ignore::PreviewEntry {
                path: "previous-ignore-file".to_string(),
                ignored: false,
                matched_by: None,
                secret_warning: false,
            }]),
        };
        terminal.draw(|frame| draw(frame, &mut ignore)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Generating ignore preview"));
        assert!(content.contains("previous-ignore-file"));

        let mut automation = app_on(Screen::Automation);
        automation.automation_screen.status_state = crate::tui::task::LoadState::Loading {
            request_id,
            previous: Some("previous-active".to_string()),
        };
        terminal.draw(|frame| draw(frame, &mut automation)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Checking automation status"));
        assert!(content.contains("previous-active"));
    }

    /// Verify preview screen renders empty state.
    #[test]
    fn preview_screen_renders_stale() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Preview);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("not loaded") || content.contains("Preview"));
    }

    /// Verify preview screen renders with data.
    #[test]
    fn preview_screen_renders_with_data() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Preview);
        app.preview_screen.load_state =
            crate::tui::task::LoadState::Loaded(crate::tui::screens::preview::PreviewData {
                additions: 3,
                modifications: 1,
                deletions: 0,
                exclusions: 2,
                warnings: 0,
                entries: vec![
                    crate::tui::screens::preview::PreviewEntry {
                        kind: crate::tui::screens::preview::EntryKind::Addition,
                        path: ".config/fish/config.fish".to_string(),
                        detail: Some("regular file".to_string()),
                    },
                    crate::tui::screens::preview::PreviewEntry {
                        kind: crate::tui::screens::preview::EntryKind::Modification,
                        path: ".bashrc".to_string(),
                        detail: Some("content changed".to_string()),
                    },
                ],
            });

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("+3"));
        assert!(content.contains("config.fish"));
        assert!(content.contains(".bashrc"));
    }

    /// Verify automation screen renders.
    #[test]
    fn automation_screen_renders() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Automation);
        app.automation_screen.status_state =
            crate::tui::task::LoadState::Loaded("active".to_string());
        app.config = Some(crate::config::Config::new("~/repo", "test-machine"));

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Automation"));
        assert!(content.contains("active"));
    }

    /// Verify automation screen shows confirmation dialog.
    #[test]
    fn automation_screen_shows_confirm() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Automation);
        app.focus = crate::tui::Focus::Content;
        app.automation_screen.confirm = crate::tui::screens::automation::ConfirmAction::Install;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Install"));
        assert!(content.contains("confirm"));
        assert!(content.contains("cancel"));
    }

    /// Verify history screen renders with entries.
    #[test]
    fn history_screen_renders_with_entries() {
        use chrono::{TimeZone, Utc};

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::History);
        app.state = Some(crate::state::AppState {
            last_attempt: None,
            last_success: None,
            last_commit: None,
            last_push: None,
            pending_push: false,
            latest_warning: None,
            latest_error: None,
            history: vec![
                crate::state::RunRecord {
                    namespace: String::new(),
                    started_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 0, 0).unwrap(),
                    finished_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 0, 2).unwrap(),
                    outcome: crate::state::RunOutcome::Success,
                    commit: Some("abc123".to_string()),
                    message: None,
                    log_file: None,
                },
                crate::state::RunRecord {
                    namespace: String::new(),
                    started_at: Utc.with_ymd_and_hms(2026, 7, 21, 13, 0, 0).unwrap(),
                    finished_at: Utc.with_ymd_and_hms(2026, 7, 21, 13, 0, 5).unwrap(),
                    outcome: crate::state::RunOutcome::Failed,
                    commit: None,
                    message: Some("network timeout".to_string()),
                    log_file: None,
                },
            ],
        });

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("History"));
        assert!(content.contains("Success"));
    }

    #[test]
    fn history_long_list_keeps_last_selection_visible() {
        use chrono::{Duration, TimeZone, Utc};

        let backend = TestBackend::new(90, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::History);
        let started = Utc.with_ymd_and_hms(2026, 7, 21, 14, 0, 0).unwrap();
        let history = (0..20)
            .map(|i| crate::state::RunRecord {
                namespace: "desktop".to_string(),
                started_at: started - Duration::minutes(i),
                finished_at: started - Duration::minutes(i) + Duration::seconds(1),
                outcome: if i == 19 {
                    crate::state::RunOutcome::Failed
                } else {
                    crate::state::RunOutcome::Success
                },
                commit: None,
                message: None,
                log_file: None,
            })
            .collect();
        app.state = Some(crate::state::AppState {
            history,
            ..crate::state::AppState::default()
        });
        app.history_screen.selected = 19;

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Failed"));
        assert!(content.contains("of 20"));
        assert_eq!(app.history_screen.list_viewport.visible_range(20).end, 20);
    }

    #[test]
    fn history_viewport_recalculates_after_resize() {
        use chrono::{Duration, TimeZone, Utc};

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::History);
        let started = Utc.with_ymd_and_hms(2026, 7, 21, 14, 0, 0).unwrap();
        app.state = Some(crate::state::AppState {
            history: (0..20)
                .map(|i| crate::state::RunRecord {
                    namespace: String::new(),
                    started_at: started - Duration::minutes(i),
                    finished_at: started - Duration::minutes(i) + Duration::seconds(1),
                    outcome: crate::state::RunOutcome::Success,
                    commit: None,
                    message: None,
                    log_file: None,
                })
                .collect(),
            ..crate::state::AppState::default()
        });
        app.history_screen.selected = 19;

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let wide_height = app.history_screen.list_viewport.height();
        terminal.backend_mut().resize(60, 12);
        terminal.autoresize().unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        assert!(app.history_screen.list_viewport.height() < wide_height);
        assert_eq!(app.history_screen.list_viewport.visible_range(20).end, 20);
    }

    /// Verify history screen renders empty state.
    #[test]
    fn history_screen_renders_empty() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::History);
        // Clear state history to test empty state
        if let Some(ref mut state) = app.state {
            state.history.clear();
        }

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("No backup history available"));
    }

    /// Verify all screens render without panic at various terminal sizes.
    #[test]
    fn all_screens_render_at_various_sizes() {
        let sizes = [(40, 10), (80, 24), (120, 40), (200, 50)];

        for (w, h) in sizes {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();

            for &screen in Screen::ALL {
                let mut app = app_on(screen);
                terminal
                    .draw(|frame| draw(frame, &mut app))
                    .unwrap_or_else(|_| panic!("failed on screen {screen:?} at {w}x{h}"));
            }
        }
    }

    /// Verify navigation transitions update the tab bar correctly.
    #[test]
    fn navigation_transitions_render_correctly() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new();

        // Start on Dashboard with TabBar focus, use Right to navigate tabs.
        for expected in Screen::ALL.iter().skip(1) {
            app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
            assert_eq!(app.active_screen, *expected);

            terminal
                .draw(|frame| draw(frame, &mut app))
                .expect("draw after navigation should not fail");
        }
    }

    /// Verify backup result transition updates dashboard.
    #[test]
    fn backup_result_updates_dashboard() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_with_state();

        // Simulate a backup task completing.
        app.tasks.active = Some(task::TaskKind::Backup);
        app.tasks
            .sender
            .send(task::TaskResult::Backup(task::BackupResult {
                success: true,
                commit: Some("newcommit123".to_string()),
                pushed: true,
                copies: 5,
                deletions: 1,
                warnings: Vec::new(),
                error: None,
            }))
            .unwrap();
        app.poll_tasks();

        // Status message should reflect success.
        assert!(app.status_message.as_ref().unwrap().contains("success"));

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw after backup result should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("success"));
    }

    /// Helper to extract text content from a TestBackend buffer.
    fn buffer_text(backend: &TestBackend) -> String {
        backend
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect()
    }

    /// Verify help bar reflects tab-bar focus.
    #[test]
    fn help_bar_shows_tab_bar_hints_when_tab_focused() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new(); // Default: TabBar focus, Dashboard.
        assert_eq!(app.focus, crate::tui::Focus::TabBar);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        // Tab-bar help should mention arrow/tabs navigation and jump.
        assert!(content.contains("tabs"));
        assert!(content.contains("content"));
        assert!(content.contains("jump"));
    }

    /// Verify help bar reflects content focus.
    #[test]
    fn help_bar_shows_content_hints_when_content_focused() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Dashboard;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        // Content help for Dashboard should mention Tab to return and backup/check.
        assert!(content.contains("Tab"));
        assert!(content.contains("backup"));
        assert!(content.contains("check"));
    }

    /// Verify rendering works correctly for both focus states on every screen.
    #[test]
    fn all_screens_render_in_both_focus_states() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        for &screen in Screen::ALL {
            for focus in [crate::tui::Focus::TabBar, crate::tui::Focus::Content] {
                let mut app = App::new();
                app.active_screen = screen;
                app.focus = focus;

                terminal
                    .draw(|frame| draw(frame, &mut app))
                    .unwrap_or_else(|_| panic!("failed on screen {screen:?} with focus {focus:?}"));
            }
        }
    }

    /// Verify help bar shows browser hints for repository in browser mode.
    #[test]
    fn help_bar_repository_browser_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Repository;
        app.repo_screen.mode = crate::tui::screens::repository::RepoMode::Browser;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Space"), "should mention Space for select");
        assert!(
            content.contains("text input"),
            "should mention text input switch"
        );
    }

    /// Verify help bar shows text-entry hints for repository in text mode.
    #[test]
    fn help_bar_repository_text_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Repository;
        app.repo_screen.mode = crate::tui::screens::repository::RepoMode::TextInput;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("validate"), "should mention validate");
        assert!(content.contains("browser"), "should mention browser switch");
    }

    /// Verify help bar shows confirmation hints during repo confirm dialog.
    #[test]
    fn help_bar_repository_confirm_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Repository;
        app.repo_screen.confirm_state =
            crate::tui::screens::repository::ConfirmState::AskInitialize;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("confirm"), "should mention confirm");
        assert!(content.contains("cancel"), "should mention cancel");
    }

    /// Verify help bar shows browse hints for sources in browse mode.
    #[test]
    fn help_bar_sources_browse_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Sources;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::Browse;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Space"), "should mention Space for toggle");
        assert!(
            content.contains("review changes"),
            "should make Esc review rather than apply changes"
        );
    }

    /// Verify help bar shows list hints for sources in list mode.
    #[test]
    fn help_bar_sources_list_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Sources;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::List;
        let mut config = crate::config::Config::new("~/repo", "desktop");
        config.sources.push(crate::config::SourceConfig {
            path: ".config".to_string(),
            ignore: Vec::new(),
        });
        app.config = Some(config);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("add"), "should mention add");
        assert!(content.contains("delete"), "should mention delete");
    }

    /// Verify help bar shows confirm hints for sources in confirm delete mode.
    #[test]
    fn help_bar_sources_confirm_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Sources;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::ConfirmDelete;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("confirm"), "should mention confirm");
        assert!(content.contains("cancel"), "should mention cancel");
    }

    // --- MS07: Multi-select rendering tests ---

    /// Verify sources screen in Browse mode shows selection summary.
    #[test]
    fn sources_screen_browse_shows_selection_summary() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Sources);
        app.focus = crate::tui::Focus::Content;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::Browse;

        // Set up a selection with 2 sources, 1 excluded.
        let mut sel =
            crate::tui::selection::SourceSelection::new(std::path::Path::new("/home/user"));
        sel.toggle(std::path::Path::new("/home/user/.config/fish"), true);
        sel.toggle(std::path::Path::new("/home/user/.bashrc"), false);
        app.sources_screen.selection = Some(sel);

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(
            content.contains("2 sources"),
            "should show source count in status"
        );
    }

    /// Verify sources screen in ConfirmApply mode shows change summary.
    #[test]
    fn sources_screen_confirm_apply_shows_summary() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Sources);
        app.focus = crate::tui::Focus::Content;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::ConfirmApply;
        app.config = Some(crate::config::Config {
            version: crate::config::Config::CURRENT_VERSION,
            namespace: "test-machine".to_string(),
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
            sources: vec![crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            }],
        });

        let mut ignore_rules = std::collections::HashMap::new();
        ignore_rules.insert(".bashrc".to_string(), vec!["/secret".to_string()]);
        app.sources_screen.pending_diff = Some(crate::tui::selection::SelectionDiff {
            additions: vec![".config/fish".to_string()],
            removals: vec![".zshrc".to_string()],
            ignore_rules,
        });

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(
            content.contains("Remove sources and apply"),
            "should explain the removal confirmation"
        );
        assert!(
            content.contains("remove and apply"),
            "footer should show confirm action"
        );
        assert!(
            content.contains("back to choices"),
            "footer should show cancel action"
        );
        assert!(content.contains(".zshrc"), "should show removal detail");
    }

    /// Verify sources screen ConfirmApply renders at narrow terminal.
    #[test]
    fn sources_screen_confirm_apply_narrow() {
        let backend = TestBackend::new(40, 15);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Sources);
        app.focus = crate::tui::Focus::Content;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::ConfirmApply;
        app.config = Some(crate::config::Config {
            version: crate::config::Config::CURRENT_VERSION,
            namespace: "test-machine".to_string(),
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
            sources: vec![],
        });
        app.sources_screen.pending_diff = Some(crate::tui::selection::SelectionDiff {
            additions: vec![".bashrc".to_string()],
            removals: vec![],
            ignore_rules: std::collections::HashMap::new(),
        });

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not panic at narrow width");
    }

    /// Verify help bar shows apply hint for ConfirmApply mode.
    #[test]
    fn help_bar_sources_confirm_apply_mode() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = App::new();
        app.focus = crate::tui::Focus::Content;
        app.active_screen = Screen::Sources;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::ConfirmApply;

        terminal
            .draw(|frame| draw(frame, &mut app))
            .expect("draw should not fail");

        let content = buffer_text(terminal.backend());
        assert!(content.contains("remove and apply"), "should mention apply");
        assert!(
            content.contains("back to choices"),
            "should return to the pending-change choice"
        );
    }

    #[test]
    fn sources_pending_changes_renders_all_three_explicit_choices() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Sources);
        app.focus = crate::tui::Focus::Content;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::PendingChanges;

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let content = buffer_text(terminal.backend());
        assert!(content.contains("Pending source changes require a decision."));
        assert!(content.contains("apply"));
        assert!(content.contains("discard"));
        assert!(content.contains("continue editing"));
    }

    /// Verify sources browse renders at various sizes without panic.
    #[test]
    fn sources_browse_renders_at_various_sizes() {
        let sizes = [(40, 10), (80, 24), (120, 40)];

        for (w, h) in sizes {
            let backend = TestBackend::new(w, h);
            let mut terminal = Terminal::new(backend).unwrap();

            let mut app = app_on(Screen::Sources);
            app.focus = crate::tui::Focus::Content;
            app.sources_screen.mode = crate::tui::screens::sources::Mode::Browse;
            app.sources_screen.selection = Some(crate::tui::selection::SourceSelection::new(
                std::path::Path::new("/home/user"),
            ));

            terminal
                .draw(|frame| draw(frame, &mut app))
                .unwrap_or_else(|_| panic!("failed at {w}x{h}"));
        }
    }
}
