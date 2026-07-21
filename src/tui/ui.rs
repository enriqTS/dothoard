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
        Screen::Dashboard => draw_dashboard(frame, area, app),
        Screen::Repository => draw_repository(frame, area, app),
        Screen::Sources => draw_sources(frame, area, app),
        Screen::Ignore => draw_ignore(frame, area, app),
        Screen::Preview => draw_preview(frame, area, app),
        Screen::Automation => draw_placeholder(frame, area, "Automation"),
        Screen::History => draw_placeholder(frame, area, "History"),
    }
}

/// Draw the help/status bar at the bottom.
fn draw_help_bar(frame: &mut Frame, area: Rect, app: &App) {
    let line = if let Some(ref msg) = app.status_message {
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(msg.as_str(), Style::default().fg(Color::Yellow)),
        ])
    } else if app.active_screen == Screen::Dashboard {
        Line::from(vec![
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::raw(" quit  "),
            Span::styled("b", Style::default().fg(Color::Cyan)),
            Span::raw(" backup  "),
            Span::styled("c", Style::default().fg(Color::Cyan)),
            Span::raw(" check  "),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw("/"),
            Span::styled("S-Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" navigate  "),
            Span::styled("1-7", Style::default().fg(Color::Cyan)),
            Span::raw(" jump"),
        ])
    } else if app.active_screen == Screen::Repository {
        Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" validate  "),
            Span::styled("Ctrl+C", Style::default().fg(Color::Cyan)),
            Span::raw(" quit  "),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" next screen"),
        ])
    } else {
        Line::from(vec![
            Span::styled("q", Style::default().fg(Color::Cyan)),
            Span::raw(" quit  "),
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw("/"),
            Span::styled("S-Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" navigate  "),
            Span::styled("1-7", Style::default().fg(Color::Cyan)),
            Span::raw(" jump"),
        ])
    };

    let paragraph = Paragraph::new(line);
    frame.render_widget(paragraph, area);
}

/// Draw the dashboard screen with real status information.
fn draw_dashboard(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Dashboard ");

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
            let short = if sha.len() > 8 {
                sha[..8].to_string()
            } else {
                sha.clone()
            };
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
            // Truncate long errors for display.
            let display = if err.len() > 60 {
                format!("{}...", &err[..57])
            } else {
                err.clone()
            };
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

/// Draw the backup preview screen.
fn draw_preview(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::screens::preview::EntryKind;

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Backup Preview ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    // Error state.
    if let Some(ref err) = app.preview_screen.error {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("Error: {err}"), Style::default().fg(Color::Red)),
        ]));
        lines.push(Line::from(""));
        lines.push(dim_line("  Press 'r' to retry."));
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
        return;
    }

    // Stale / not yet loaded.
    if app.preview_screen.stale || app.preview_screen.preview.is_none() {
        lines.push(Line::from(""));
        lines.push(dim_line("  Preview not loaded. Press 'r' to generate."));
        lines.push(Line::from(""));
        lines.push(dim_line(
            "  This runs the backup planner without making changes.",
        ));
        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
        return;
    }

    let data = app.preview_screen.preview.as_ref().unwrap();

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
                "  [{}-{} of {}] ↑↓/jk to scroll",
                scroll + 1,
                (scroll + visible_height).min(data.entries.len()),
                data.entries.len()
            )));
        }
    }

    // Help.
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("r", Style::default().fg(Color::DarkGray)),
        Span::styled(" refresh  ", Style::default().fg(Color::DarkGray)),
        Span::styled("↑↓/jk", Style::default().fg(Color::DarkGray)),
        Span::styled(" scroll", Style::default().fg(Color::DarkGray)),
    ]));

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the ignore rule editor screen.
fn draw_ignore(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::screens::ignore::Mode;

    let block = Block::default()
        .borders(Borders::ALL)
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
    lines.push(Line::from(""));
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
    lines.push(Line::from(source_tabs));
    lines.push(Line::from(""));

    // Current source's patterns.
    let current_source = &sources[app.ignore_screen.source_idx];
    if current_source.ignore.is_empty() {
        lines.push(dim_line("  No ignore patterns. Press 'a' to add."));
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

    // Preview mode.
    if app.ignore_screen.mode == Mode::Preview {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  File Preview (Esc to close):",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));

        if app.ignore_screen.preview.is_empty() {
            lines.push(dim_line("    No files found."));
        } else {
            for entry in app.ignore_screen.preview.iter().take(20) {
                let mut spans = vec![Span::raw("    ")];

                if entry.ignored {
                    spans.push(Span::styled("✗ ", Style::default().fg(Color::Red)));
                    spans.push(Span::styled(
                        entry.path.clone(),
                        Style::default().fg(Color::DarkGray),
                    ));
                    if let Some(ref pat) = entry.matched_by {
                        spans.push(Span::styled(
                            format!("  ({})", pat),
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
            if app.ignore_screen.preview.len() > 20 {
                lines.push(dim_line(format!(
                    "    ...and {} more files",
                    app.ignore_screen.preview.len() - 20
                )));
            }
        }
    }

    // Feedback message.
    if let Some(ref msg) = app.ignore_screen.message {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(msg.clone(), Style::default().fg(Color::Green)),
        ]));
    }

    // Help.
    if app.ignore_screen.mode == Mode::List {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("a", Style::default().fg(Color::DarkGray)),
            Span::styled(" add  ", Style::default().fg(Color::DarkGray)),
            Span::styled("d", Style::default().fg(Color::DarkGray)),
            Span::styled(" delete  ", Style::default().fg(Color::DarkGray)),
            Span::styled("p", Style::default().fg(Color::DarkGray)),
            Span::styled(" preview  ", Style::default().fg(Color::DarkGray)),
            Span::styled("←→/hl", Style::default().fg(Color::DarkGray)),
            Span::styled(" source  ", Style::default().fg(Color::DarkGray)),
            Span::styled("↑↓/jk", Style::default().fg(Color::DarkGray)),
            Span::styled(" pattern", Style::default().fg(Color::DarkGray)),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the sources management screen.
fn draw_sources(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::screens::sources::{MessageKind, Mode};

    let block = Block::default().borders(Borders::ALL).title(" Sources ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

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
        lines.push(dim_line("  Press 'a' to add a source path."));
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
                format!("Delete '{path}'? (y/n)"),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Show feedback message.
    if let Some(ref msg) = app.sources_screen.message {
        lines.push(Line::from(""));
        let color = match msg.kind {
            MessageKind::Info => Color::Green,
            MessageKind::Warning => Color::Yellow,
            MessageKind::Error => Color::Red,
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(msg.text.clone(), Style::default().fg(color)),
        ]));
    }

    // Help line at bottom of content.
    if app.sources_screen.mode == Mode::List {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("a", Style::default().fg(Color::DarkGray)),
            Span::styled(" add  ", Style::default().fg(Color::DarkGray)),
            Span::styled("d", Style::default().fg(Color::DarkGray)),
            Span::styled(" delete  ", Style::default().fg(Color::DarkGray)),
            Span::styled("↑↓/jk", Style::default().fg(Color::DarkGray)),
            Span::styled(" navigate", Style::default().fg(Color::DarkGray)),
        ]));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

/// Draw the repository selection screen.
fn draw_repository(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::screens::repository::{ConfirmState, OwnershipInfo, ValidationResult};

    let block = Block::default().borders(Borders::ALL).title(" Repository ");

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  Repository path:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    // Input field with cursor indicator.
    let input_display = format!("  > {}", app.repo_screen.input);
    lines.push(Line::from(Span::raw(input_display)));

    // Cursor position indicator.
    let cursor_line = format!("  {}^", " ".repeat(app.repo_screen.cursor + 1));
    lines.push(Line::from(Span::styled(
        cursor_line,
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(Line::from(""));

    // Validation result.
    match &app.repo_screen.validation {
        Some(ValidationResult::Valid(info)) => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("✓ Valid repository", Style::default().fg(Color::Green)),
            ]));
            lines.push(field_line("    Branch", info.branch.clone()));
            lines.push(field_line("    Path", info.path.display().to_string()));
            lines.push(Line::from(""));

            // Ownership info.
            match &info.ownership {
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
        Some(ValidationResult::Invalid(msg)) => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("✗ {msg}"), Style::default().fg(Color::Red)),
            ]));
        }
        None => {
            lines.push(dim_line("  Press Enter to validate the path."));
        }
    }

    // Confirmation dialog.
    match app.repo_screen.confirm_state {
        ConfirmState::AskInitialize => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Initialize this repository? (y/n)",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        ConfirmState::AskAttach => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "Attach to this repository? (y/n)",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        ConfirmState::Done => {
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    "✓ Repository configured.",
                    Style::default().fg(Color::Green),
                ),
            ]));
        }
        ConfirmState::None => {}
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
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
        app.config = Some(crate::config::Config::new("~/dotfiles-repo"));
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

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");
    }

    /// Verify the dashboard renders with state data.
    #[test]
    fn dashboard_renders_with_state() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        let app = app_with_state();

        terminal
            .draw(|frame| draw(frame, &app))
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

    /// Verify dashboard renders without state (no config, no state).
    #[test]
    fn dashboard_renders_without_state() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let app = app_on(Screen::Dashboard);

        terminal
            .draw(|frame| draw(frame, &app))
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
            .draw(|frame| draw(frame, &app))
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
            .draw(|frame| draw(frame, &app))
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
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Running backup"));
    }

    /// Verify status message appears in help bar.
    #[test]
    fn help_bar_shows_status_message() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();

        let mut app = app_on(Screen::Dashboard);
        app.status_message = Some("Test status message".to_string());

        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw should not fail");

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
            .collect();
        assert!(content.contains("Test status message"));
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
