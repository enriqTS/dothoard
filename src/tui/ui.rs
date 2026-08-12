//! Layout and rendering for the TUI.
//!
//! Each screen has its own rendering function. The top-level `draw` function
//! renders the tab bar and delegates to the active screen's renderer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use super::{App, Screen, modal, text, theme::THEME};

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
    draw_modal_overlay(frame, frame.area(), app);
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
                Span::styled(num, THEME.key()),
                Span::raw(":"),
                Span::raw(screen.label()),
            ])
        })
        .collect();

    let selected = Screen::ALL
        .iter()
        .position(|&s| s == app.active_screen)
        .unwrap_or(0);

    let tab_focused = app.focus == Focus::TabBar;
    let title = if tab_focused {
        " ▶ TAB FOCUS · dothoard "
    } else {
        "   Tabs · dothoard "
    };
    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(THEME.border(tab_focused))
                .title(Line::from(Span::styled(
                    title,
                    if tab_focused {
                        THEME.focused()
                    } else {
                        THEME.muted()
                    },
                ))),
        )
        .select(selected)
        .style(Style::default())
        .highlight_style(if tab_focused {
            THEME.selected()
        } else {
            THEME.heading()
        });

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

/// Render the modal that currently owns input, if any.
///
/// Screen state machines retain all confirmation semantics; this function only
/// gives their ownership a consistent, centered presentation.
fn draw_modal_overlay(frame: &mut Frame, area: Rect, app: &App) {
    use crate::tui::screens::{automation, ignore, repository, sources};
    use crate::tui::task::LoadState;

    match app.active_screen {
        Screen::Dashboard if app.dashboard_screen.detail.is_some() => {
            let detail = app.dashboard_screen.detail.as_ref().expect("checked above");
            modal::draw(
                frame,
                area,
                modal::ModalSpec {
                    title: &detail.title,
                    affected: None,
                    consequence: &detail.value,
                    validation: None,
                    confirm: "d: details",
                    cancel: "Esc: close",
                },
            );
        }
        Screen::Repository => {
            if let Some(question) = app.repo_screen.namespace_confirmation.as_deref() {
                let affected = app.config.as_ref().map(|config| {
                    let home = app
                        .paths
                        .as_ref()
                        .map(|paths| paths.home())
                        .unwrap_or_else(|| std::path::Path::new("."));
                    let namespace = if app.repo_screen.namespace_action
                        == repository::NamespaceAction::Delete
                    {
                        &app.repo_screen.namespace_origin
                    } else {
                        &app.repo_screen.namespace_input
                    };
                    config
                        .repository_path(home)
                        .join(namespace)
                        .display()
                        .to_string()
                });
                modal::draw(
                    frame,
                    area,
                    modal::ModalSpec {
                        title: "Confirm namespace change",
                        affected: affected.as_deref(),
                        consequence: question,
                        validation: None,
                        confirm: "y: confirm",
                        cancel: "n / Esc: cancel",
                    },
                );
            } else if matches!(
                app.repo_screen.confirm_state,
                repository::ConfirmState::AskInitialize | repository::ConfirmState::AskAttach
            ) {
                let initialize =
                    app.repo_screen.confirm_state == repository::ConfirmState::AskInitialize;
                modal::draw(
                    frame,
                    area,
                    modal::ModalSpec {
                        title: if initialize {
                            "Initialize namespace"
                        } else {
                            "Attach namespace"
                        },
                        affected: if app.repo_screen.input.is_empty() {
                            None
                        } else {
                            Some(app.repo_screen.input.as_str())
                        },
                        consequence: if initialize {
                            "This creates dothoard's ownership manifest only in the selected namespace."
                        } else {
                            "This uses the existing matching ownership manifest; no sibling namespace is changed."
                        },
                        validation: None,
                        confirm: "y: confirm",
                        cancel: "n / Esc: cancel",
                    },
                );
            } else {
                match app.repo_screen.mode {
                    repository::RepoMode::TextInput => {
                        let validation = match &app.repo_screen.validation {
                            LoadState::Failed { error, .. } => {
                                Some((error.as_str(), THEME.error()))
                            }
                            LoadState::Loading { .. } => {
                                Some(("Checking repository…", THEME.progress()))
                            }
                            _ => app
                                .repo_screen
                                .selection_error
                                .as_deref()
                                .map(|error| (error, THEME.error())),
                        };
                        modal::draw_text_input(
                            frame,
                            area,
                            modal::TextInputSpec {
                                title: "Repository path",
                                label: "Existing Git worktree path",
                                text: &app.repo_screen.input,
                                cursor: app.repo_screen.cursor,
                                validation,
                                submit: "Enter: validate",
                                cancel: "Esc: browser",
                            },
                        );
                    }
                    repository::RepoMode::NamespaceInput => {
                        let affected = app.config.as_ref().map(|config| {
                            let home = app
                                .paths
                                .as_ref()
                                .map(|paths| paths.home())
                                .unwrap_or_else(|| std::path::Path::new("."));
                            config.repository_path(home).display().to_string()
                        });
                        modal::draw_text_input(
                            frame,
                            area,
                            modal::TextInputSpec {
                                title: "Namespace",
                                label: app.repo_screen.namespace_action_name(),
                                text: &app.repo_screen.namespace_input,
                                cursor: app.repo_screen.namespace_cursor,
                                validation: affected.as_deref().map(|path| (path, THEME.muted())),
                                submit: "Enter: review",
                                cancel: "Esc: cancel",
                            },
                        );
                    }
                    repository::RepoMode::Browser => {}
                }
            }
        }
        Screen::Sources => match app.sources_screen.mode {
            sources::Mode::AddInput => modal::draw_text_input(
                frame,
                area,
                modal::TextInputSpec {
                    title: "Add source",
                    label: "Path relative to $HOME",
                    text: &app.sources_screen.input,
                    cursor: app.sources_screen.cursor,
                    validation: app
                        .sources_screen
                        .message
                        .as_ref()
                        .map(|message| (message.text.as_str(), THEME.error())),
                    submit: "Enter: add",
                    cancel: "Esc: browser",
                },
            ),
            sources::Mode::ConfirmDelete => {
                let affected = app
                    .config
                    .as_ref()
                    .and_then(|config| config.sources.get(app.sources_screen.selected))
                    .map(|source| source.path.as_str());
                modal::draw(
                    frame,
                    area,
                    modal::ModalSpec {
                        title: "Remove source",
                        affected,
                        consequence: "This removes the source from configuration. It does not delete files from your home directory.",
                        validation: None,
                        confirm: "y: remove",
                        cancel: "n / Esc: cancel",
                    },
                );
            }
            sources::Mode::PendingChanges => modal::draw(
                frame,
                area,
                modal::ModalSpec {
                    title: "Review source changes",
                    affected: None,
                    consequence: "Source selection changed. Choose whether to apply it, discard it, or continue editing.",
                    validation: None,
                    confirm: "a: apply  d: discard",
                    cancel: "c / Esc: continue editing",
                },
            ),
            sources::Mode::ConfirmApply => {
                let summary = app.sources_screen.pending_diff.as_ref().map(|diff| {
                    format!(
                        "{} additions, {} removals",
                        diff.additions.len(),
                        diff.removals.len()
                    )
                });
                modal::draw(
                    frame,
                    area,
                    modal::ModalSpec {
                        title: "Apply source changes",
                        affected: summary.as_deref(),
                        consequence: "Removing sources changes configuration and applies generated ignore rules. Existing backup data is not deleted here.",
                        validation: None,
                        confirm: "y: remove and apply",
                        cancel: "n / Esc: back",
                    },
                );
            }
            _ => {}
        },
        Screen::Ignore if app.ignore_screen.mode == ignore::Mode::AddInput => {
            modal::draw_text_input(
                frame,
                area,
                modal::TextInputSpec {
                    title: "Add ignore pattern",
                    label: "Gitignore syntax",
                    text: &app.ignore_screen.input,
                    cursor: app.ignore_screen.cursor,
                    validation: app
                        .ignore_screen
                        .message
                        .as_ref()
                        .map(|message| (message.text.as_str(), THEME.error())),
                    submit: "Enter: add",
                    cancel: "Esc: cancel",
                },
            );
        }
        Screen::Automation if app.automation_screen.confirm != automation::ConfirmAction::None => {
            let removing = app.automation_screen.confirm == automation::ConfirmAction::Remove;
            modal::draw(
                frame,
                area,
                modal::ModalSpec {
                    title: if removing {
                        "Remove automation"
                    } else {
                        "Install automation"
                    },
                    affected: Some("dothoard-backup.service and dothoard-backup.timer"),
                    consequence: if removing {
                        "This disables and removes only dothoard's user timer units."
                    } else {
                        "This installs and enables dothoard's user timer using the current schedule."
                    },
                    validation: None,
                    confirm: if removing { "y: remove" } else { "y: install" },
                    cancel: "n / Esc: cancel",
                },
            );
        }
        _ => {}
    }
}

/// Border style for the active screen block, based on focus.
fn content_border_style(app: &App) -> Style {
    THEME.border(app.focus == super::Focus::Content)
}

/// Title text names both the screen and the interaction mode that owns input.
fn screen_title(app: &App, screen: &'static str) -> Line<'static> {
    use crate::tui::screens::{automation::ConfirmAction, history, ignore, repository, sources};

    let mode = match app.active_screen {
        Screen::Dashboard if app.tasks.is_busy() => "Running",
        Screen::Dashboard => "Overview",
        Screen::Repository
            if app.repo_screen.confirm_state == repository::ConfirmState::AskInitialize
                || app.repo_screen.confirm_state == repository::ConfirmState::AskAttach
                || app.repo_screen.namespace_confirmation.is_some() =>
        {
            "Confirming"
        }
        Screen::Repository => match app.repo_screen.mode {
            repository::RepoMode::Browser => "Browsing",
            repository::RepoMode::TextInput | repository::RepoMode::NamespaceInput => "Editing",
        },
        Screen::Sources => match app.sources_screen.mode {
            sources::Mode::List => "Browsing",
            sources::Mode::Browse => "Selecting",
            sources::Mode::AddInput => "Editing",
            sources::Mode::PendingChanges => "Reviewing",
            sources::Mode::ConfirmDelete | sources::Mode::ConfirmApply => "Confirming",
        },
        Screen::Ignore => match app.ignore_screen.mode {
            ignore::Mode::List => "Browsing",
            ignore::Mode::AddInput => "Editing",
            ignore::Mode::Preview => "Previewing",
        },
        Screen::Preview if app.preview_screen.load_state.is_loading() => "Running",
        Screen::Preview => "Previewing",
        Screen::Automation if app.automation_screen.confirm != ConfirmAction::None => "Confirming",
        Screen::Automation if app.automation_screen.status_state.is_loading() => "Running",
        Screen::Automation => "Inspecting",
        Screen::History if app.history_screen.mode == history::Mode::LogView => "Previewing",
        Screen::History => "Browsing",
    };
    let owns_focus = app.focus == super::Focus::Content;
    let marker = if owns_focus {
        "▶ CONTENT"
    } else {
        "  Content"
    };
    Line::from(vec![
        Span::styled(
            format!(" {marker} · {screen} "),
            if owns_focus {
                THEME.focused()
            } else {
                THEME.muted()
            },
        ),
        Span::styled(format!("[{mode}] "), THEME.label()),
    ])
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
        let rendered = format!(" {}: {}", message.kind.label(), message.text);
        let rendered = text::truncate(&rendered, area.width as usize);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                rendered,
                THEME.status(message.kind),
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
        Screen::Dashboard if app.dashboard_screen.detail.is_some() => Line::from(vec![
            Span::styled("Tab", THEME.key()),
            Span::raw(" tabs  "),
            Span::styled("Esc", THEME.key()),
            Span::raw(" close details"),
        ]),
        Screen::Dashboard => {
            let mut spans = vec![Span::styled("Tab", THEME.key()), Span::raw(" tabs  ")];
            if !app.tasks.is_busy() {
                spans.extend([
                    Span::styled("b", THEME.key()),
                    Span::raw(" backup  "),
                    Span::styled("c", THEME.key()),
                    Span::raw(" check  "),
                    Span::styled("p", THEME.key()),
                    Span::raw(" push  "),
                ]);
            }
            spans.extend([
                Span::styled("a", THEME.key()),
                Span::raw(" automation  "),
                Span::styled("r", THEME.key()),
                Span::raw(" repository  "),
                Span::styled("d", THEME.key()),
                Span::raw(" details  "),
                Span::styled("q", THEME.key()),
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
        .title(screen_title(app, "Dashboard"));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Primary health is always drawn first. Narrow terminals stack secondary
    // configuration below it so health and the next action remain visible.
    if inner.width < 72 {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(inner);
        draw_dashboard_status(frame, rows[0], app);
        draw_dashboard_info(frame, rows[1], app);
    } else {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(inner);
        draw_dashboard_status(frame, columns[0], app);
        draw_dashboard_info(frame, columns[1], app);
    }
}

/// Primary dashboard summaries: health, last success, synchronization,
/// automation, and the one action that best moves the user forward.
fn draw_dashboard_status(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![section_header("Backup Health")];
    let (health, health_style) = dashboard_health(app);
    lines.push(Line::from(Span::styled(
        format!("  {health}"),
        health_style,
    )));
    lines.push(field_line(
        "  Last successful backup",
        app.state
            .as_ref()
            .and_then(|state| state.last_success.as_ref())
            .map(format_time)
            .unwrap_or_else(|| "Never".to_string()),
    ));

    lines.push(section_header("Remote Synchronization"));
    let pending_push = app.state.as_ref().is_some_and(|state| state.pending_push);
    lines.push(Line::from(Span::styled(
        if pending_push {
            "  Pending commits need push"
        } else {
            "  No pending commits"
        },
        if pending_push {
            THEME.warning()
        } else {
            THEME.success()
        },
    )));
    if let Some(last_push) = app
        .state
        .as_ref()
        .and_then(|state| state.last_push.as_ref())
    {
        lines.push(field_line("  Last push", format_time(last_push)));
    }

    lines.push(section_header("Automation Health"));
    let (automation, automation_style) = dashboard_automation(app);
    lines.push(Line::from(Span::styled(
        format!("  {automation}"),
        automation_style,
    )));

    lines.push(section_header("Recommended Next Action"));
    lines.push(Line::from(Span::styled(
        format!("  {}", dashboard_action(app)),
        THEME.focused(),
    )));

    frame.render_widget(Paragraph::new(lines), area);
}

/// Secondary configuration and diagnostics. All long values are wrapped by
/// display cells and their full check/error values remain available with `d`.
fn draw_dashboard_info(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![section_header("Latest Check")];
    let (check, check_style) = dashboard_check(app);
    lines.extend(wrapped_styled_lines(
        "  ",
        &check,
        area.width.saturating_sub(2),
        check_style,
    ));
    lines.push(Line::from(""));

    lines.push(section_header("Repository Details"));
    if let Some(config) = &app.config {
        lines.extend(wrapped_field_lines(
            "  Path",
            &config.repository,
            area.width,
        ));
        lines.push(field_line("  Namespace", config.namespace.clone()));
        lines.push(field_line("  Sources", config.sources.len().to_string()));
        lines.push(field_line(
            "  Schedule",
            format!("{} min", config.interval_minutes),
        ));
        lines.push(field_line(
            "  Timeout",
            format!("{}s", config.network_timeout_seconds),
        ));
    } else {
        lines.push(dim_line("  No repository configured. Press r to begin."));
    }
    lines.push(Line::from(""));

    lines.push(section_header("Latest Issue"));
    if let Some((issue, style)) = dashboard_issue(app) {
        lines.extend(wrapped_styled_lines(
            "  ",
            &issue,
            area.width.saturating_sub(2),
            style,
        ));
        lines.push(dim_line("  Press d for complete details."));
    } else {
        lines.push(Line::from(Span::styled(
            "  No issues reported",
            THEME.success(),
        )));
    }

    frame.render_widget(Paragraph::new(lines), area);
}

fn dashboard_health(app: &App) -> (&'static str, Style) {
    if app.config.is_none() {
        return ("UNCONFIGURED — repository setup required", THEME.warning());
    }
    if app.tasks.active_task() == Some(super::task::TaskKind::Backup) {
        return ("RUNNING — backup in progress", THEME.progress());
    }
    if app
        .state
        .as_ref()
        .is_some_and(|state| state.latest_error.is_some())
        || app.last_check.as_ref().is_some_and(|check| !check.healthy)
    {
        return ("NEEDS ATTENTION — review the latest issue", THEME.error());
    }
    if app
        .state
        .as_ref()
        .and_then(|state| state.last_success.as_ref())
        .is_none()
    {
        return ("NO SUCCESSFUL BACKUP YET", THEME.warning());
    }
    ("HEALTHY", THEME.success())
}

fn dashboard_automation(app: &App) -> (String, Style) {
    use crate::tui::task::LoadState;
    match &app.automation_screen.status_state {
        LoadState::Loaded(status) => {
            let style = if status == "active" {
                THEME.success()
            } else {
                THEME.warning()
            };
            (status.clone(), style)
        }
        LoadState::Loading { .. } => ("Checking automation status…".to_string(), THEME.progress()),
        LoadState::Stale {
            previous: Some(status),
        } => (format!("Stale: {status}"), THEME.warning()),
        LoadState::Stale { previous: None } | LoadState::NotLoaded => (
            "Unavailable — not inspected yet (press a)".to_string(),
            THEME.warning(),
        ),
        LoadState::Failed { error, previous } => (
            previous.as_ref().map_or_else(
                || format!("Unavailable: {error}"),
                |status| format!("Unavailable: {error}; previous {status}"),
            ),
            THEME.error(),
        ),
    }
}

fn dashboard_check(app: &App) -> (String, Style) {
    if app.tasks.active_task() == Some(super::task::TaskKind::Check) {
        return ("Checking repository…".to_string(), THEME.progress());
    }
    let Some(check) = &app.last_check else {
        return if app.config.is_some() {
            ("Unavailable — run check (c)".to_string(), THEME.warning())
        } else {
            (
                "Unavailable — configure repository first".to_string(),
                THEME.warning(),
            )
        };
    };
    if check.healthy {
        ("All checks passed".to_string(), THEME.success())
    } else if let Some(item) = first_check_issue(check) {
        (
            format!(
                "{}: {}",
                item.label,
                item.detail.as_deref().unwrap_or("needs attention")
            ),
            THEME.error(),
        )
    } else {
        (
            "Checks reported an unspecified issue".to_string(),
            THEME.error(),
        )
    }
}

fn dashboard_issue(app: &App) -> Option<(String, Style)> {
    if let Some(check) = &app.last_check
        && let Some(item) = first_check_issue(check)
    {
        return Some((
            format!(
                "{}: {}",
                item.label,
                item.detail.as_deref().unwrap_or("needs attention")
            ),
            THEME.error(),
        ));
    }
    app.state.as_ref().and_then(|state| {
        state
            .latest_error
            .as_ref()
            .map(|error| (error.clone(), THEME.error()))
            .or_else(|| {
                state
                    .latest_warning
                    .as_ref()
                    .map(|warning| (warning.clone(), THEME.warning()))
            })
    })
}

fn first_check_issue(check: &super::task::CheckResult) -> Option<&super::task::CheckItem> {
    check
        .results
        .iter()
        .find(|item| item.status == super::task::CheckItemStatus::Error)
        .or_else(|| {
            check
                .results
                .iter()
                .find(|item| item.status == super::task::CheckItemStatus::Warning)
        })
}

fn dashboard_action(app: &App) -> &'static str {
    if app.config.is_none() {
        "Configure repository (r)"
    } else if app
        .state
        .as_ref()
        .is_some_and(|state| state.latest_error.is_some())
        || app.last_check.as_ref().is_some_and(|check| !check.healthy)
    {
        "Run a check (c)"
    } else if app.state.as_ref().is_some_and(|state| state.pending_push) {
        "Push pending commits (p)"
    } else if !matches!(app.automation_screen.status_state, super::task::LoadState::Loaded(ref status) if status == "active")
    {
        "Review automation (a)"
    } else {
        "No action needed — run backup when ready (b)"
    }
}

fn wrapped_styled_lines(prefix: &str, value: &str, width: u16, style: Style) -> Vec<Line<'static>> {
    let width = usize::from(width).saturating_sub(prefix.len()).max(1);
    text::wrap(value, width)
        .into_iter()
        .map(|line| {
            Line::from(vec![
                Span::raw(prefix.to_string()),
                Span::styled(line, style),
            ])
        })
        .collect()
}

fn wrapped_field_lines(label: &'static str, value: &str, width: u16) -> Vec<Line<'static>> {
    let available = usize::from(width).saturating_sub(label.len() + 4).max(1);
    text::wrap(value, available)
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                field_line(label, line)
            } else {
                Line::from(Span::raw(format!("    {line}")))
            }
        })
        .collect()
}

/// Create a section header line.
fn section_header(title: &'static str) -> Line<'static> {
    Line::from(Span::styled(title, THEME.heading()))
}

/// Create a "label: value" field line with owned strings.
fn field_line(label: &'static str, value: impl Into<String>) -> Line<'static> {
    let val: String = value.into();
    Line::from(vec![
        Span::styled(label, THEME.label()),
        Span::raw(": "),
        Span::raw(val),
    ])
}

/// Create a dim informational line with an owned string.
fn dim_line(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(text.into(), THEME.muted()))
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
        .title(screen_title(app, "History"));

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
            THEME.selected()
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
        .title(screen_title(app, "Automation"));

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
        .title(screen_title(app, "Backup Preview"));

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
        .title(screen_title(app, "Ignore Rules"));

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

    // Source selector. Focus is reinforced with a marker and underline, not color alone.
    let source_focused = app.ignore_screen.mode == Mode::List
        && app.ignore_screen.list_focus == crate::tui::screens::ignore::ListFocus::SourceSelector;
    let mut source_tabs: Vec<Span> = vec![Span::styled(
        if source_focused {
            "▶ Sources: "
        } else {
            "  Sources: "
        },
        if source_focused {
            THEME.focused()
        } else {
            THEME.label()
        },
    )];
    for (i, source) in sources.iter().enumerate() {
        let style = if i == app.ignore_screen.source_idx {
            THEME.selected()
        } else {
            THEME.muted()
        };
        source_tabs.push(Span::styled(format!(" {} ", source.path), style));
        source_tabs.push(Span::raw("|"));
    }

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
        let patterns_focused =
            app.ignore_screen.list_focus == crate::tui::screens::ignore::ListFocus::PatternList;
        lines.push(Line::from(Span::styled(
            if patterns_focused {
                "▶ Patterns:"
            } else {
                "  Patterns:"
            },
            if patterns_focused {
                THEME.focused()
            } else {
                THEME.label()
            },
        )));
        for (i, pattern) in current_source.ignore.iter().enumerate() {
            let marker = if i == app.ignore_screen.pattern_idx {
                "▶ "
            } else {
                "  "
            };
            let style = if i == app.ignore_screen.pattern_idx {
                THEME.selected()
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
        .title(screen_title(app, "Sources"));

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
        lines.push(Line::from(Span::styled(
            "  No sources configured.",
            THEME.disabled(),
        )));
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
                THEME.selected()
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
        .title(screen_title(app, "Repository"));

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
        assert!(content.contains("Backup Health"));
        assert!(content.contains("Remote Synchronization"));
        assert!(content.contains("Automation Health"));
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
        assert!(content.contains("RUNNING — backup in progress"));
    }

    #[test]
    fn dashboard_prioritizes_health_check_automation_and_next_action() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_with_state();
        app.state.as_mut().unwrap().pending_push = true;
        app.last_check = Some(task::CheckResult {
            healthy: false,
            results: vec![
                task::CheckItem {
                    label: "Remote".to_string(),
                    status: task::CheckItemStatus::Warning,
                    detail: Some("remote is slow".to_string()),
                },
                task::CheckItem {
                    label: "Repository".to_string(),
                    status: task::CheckItemStatus::Error,
                    detail: Some("repository needs repair".to_string()),
                },
            ],
        });
        app.automation_screen.status_state = task::LoadState::Loaded("active".to_string());

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Backup Health"));
        assert!(content.contains("Last successful backup"));
        assert!(content.contains("Pending commits need push"));
        assert!(content.contains("Automation Health"));
        assert!(content.contains("Repository: repository needs repair"));
        assert!(content.contains("Run a check (c)"));
    }

    #[test]
    fn dashboard_unconfigured_and_narrow_layout_keep_primary_action_visible() {
        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Dashboard);

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Backup Health"));
        assert!(content.contains("UNCONFIGURED"));
        assert!(content.contains("Configure repository (r)"));
    }

    #[test]
    fn dashboard_distinguishes_check_and_automation_unavailable_states() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_with_state();
        app.automation_screen.status_state = task::LoadState::Failed {
            error: "user manager unavailable".to_string(),
            previous: None,
        };
        app.tasks.active = Some(task::TaskKind::Check);

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Checking repository"));
        assert!(content.contains("Unavailable: user manager unavailable"));
    }

    #[test]
    fn dashboard_detail_modal_keeps_complete_unicode_issue_available() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Dashboard);
        app.dashboard_screen.open_detail(
            "Check: repository",
            "Complete issue: /very/long/界/path remains available without truncation.",
        );

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Check: repository"));
        assert!(content.contains("Complete issue:"));
        assert!(content.contains("very/long"));
        assert!(content.contains("path remains available"));
        assert!(content.contains("Esc: close"));
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
        // Centered input border plus the six-cell display-width cursor offset.
        assert_eq!(caret_column, 8);
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

    #[test]
    fn focus_and_selection_have_visible_non_color_language() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Sources);
        app.config = Some(crate::config::Config {
            sources: vec![crate::config::SourceConfig {
                path: ".config/fish".to_string(),
                ignore: Vec::new(),
            }],
            ..crate::config::Config::new("~/repo", "desktop")
        });

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let tab_text = buffer_text(terminal.backend());
        assert!(tab_text.contains("▶ TAB FOCUS"));
        assert!(!tab_text.contains("▶ CONTENT"));

        app.focus = crate::tui::Focus::Content;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let content_text = buffer_text(terminal.backend());
        assert!(content_text.contains("▶ CONTENT"));
        assert!(content_text.contains("[Browsing]"));
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.modifier.contains(Modifier::REVERSED))
        );
    }

    #[test]
    fn centered_confirmation_modal_takes_precedence_and_dims_background() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Repository);
        app.focus = crate::tui::Focus::Content;
        app.repo_screen.mode = crate::tui::screens::repository::RepoMode::TextInput;
        app.repo_screen.input = "/a/very/long/repository/path".to_string();
        app.repo_screen.confirm_state =
            crate::tui::screens::repository::ConfirmState::AskInitialize;

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();

        let buffer = terminal.backend().buffer();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Initialize namespace"));
        assert!(content.contains("y: confirm"));
        assert!(
            buffer
                .content()
                .iter()
                .any(|cell| cell.modifier.contains(Modifier::DIM))
        );
    }

    #[test]
    fn shared_input_dialogs_render_labels_validation_and_cursors() {
        let backend = TestBackend::new(60, 16);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Sources);
        app.focus = crate::tui::Focus::Content;
        app.sources_screen.mode = crate::tui::screens::sources::Mode::AddInput;
        app.sources_screen.input = "配置/界".to_string();
        app.sources_screen.cursor = app.sources_screen.input.len();
        app.sources_screen.message = Some(crate::tui::screens::sources::Message {
            text: "Source is invalid".to_string(),
            kind: crate::tui::screens::sources::MessageKind::Error,
        });

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let content = buffer_text(terminal.backend());
        assert!(content.contains("Add source"));
        assert!(content.contains("Source is invalid"));
        assert!(content.contains("^"));

        app.active_screen = Screen::Ignore;
        app.ignore_screen.mode = crate::tui::screens::ignore::Mode::AddInput;
        app.ignore_screen.input = "*界*".to_string();
        app.ignore_screen.cursor = app.ignore_screen.input.len();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("Add ignore pattern"));
    }

    #[test]
    fn screen_titles_name_edit_preview_confirm_and_running_modes() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Sources);
        app.focus = crate::tui::Focus::Content;

        app.sources_screen.mode = crate::tui::screens::sources::Mode::AddInput;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("[Editing]"));

        app.sources_screen.mode = crate::tui::screens::sources::Mode::ConfirmDelete;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("[Confirming]"));

        app.active_screen = Screen::Ignore;
        app.ignore_screen.mode = crate::tui::screens::ignore::Mode::Preview;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("[Previewing]"));

        app.active_screen = Screen::Preview;
        app.preview_screen.load_state = task::LoadState::Loading {
            request_id: task::RequestId::for_test(7),
            previous: None,
        };
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("[Running]"));
    }

    #[test]
    fn ignore_nested_focus_uses_markers_and_distinct_styles() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = app_on(Screen::Ignore);
        app.focus = crate::tui::Focus::Content;
        let mut config = crate::config::Config::new("~/repo", "desktop");
        config.sources.push(crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        });
        app.config = Some(config);

        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("▶ Sources:"));

        app.ignore_screen.list_focus = crate::tui::screens::ignore::ListFocus::PatternList;
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(buffer_text(terminal.backend()).contains("▶ Patterns:"));
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|cell| cell.modifier.contains(Modifier::UNDERLINED))
        );
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
