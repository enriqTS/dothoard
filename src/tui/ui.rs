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

/// Supported responsive layout classes. Width determines pane arrangement;
/// short terminals prioritize the active control, status, and shortcut footer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LayoutClass {
    Wide,
    Medium,
    Narrow,
    Short,
}

fn layout_class(area: Rect) -> LayoutClass {
    if area.height < 12 {
        LayoutClass::Short
    } else if area.width < 40 {
        LayoutClass::Narrow
    } else if area.width < 80 {
        LayoutClass::Medium
    } else {
        LayoutClass::Wide
    }
}

/// Draw the complete UI for one frame.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let compact_shell = !matches!(layout_class(area), LayoutClass::Wide);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if compact_shell { 1 } else { 3 }), // Tab bar
            Constraint::Min(0),                                    // Screen content
            Constraint::Length(1),                                 // Transient status/progress
            Constraint::Length(1),                                 // Contextual help
        ])
        .split(frame.area());

    draw_tabs(frame, chunks[0], app);
    draw_screen(frame, chunks[1], app);
    draw_status_bar(frame, chunks[2], app);
    draw_help_bar(frame, chunks[3], app);
    draw_modal_overlay(frame, area, app);
}

/// Draw the tab bar at the top.
fn draw_tabs(frame: &mut Frame, area: Rect, app: &App) {
    use super::Focus;

    let compact = area.height <= 1;
    if compact {
        let selected = Screen::ALL
            .iter()
            .position(|&screen| screen == app.active_screen)
            .unwrap_or(0);
        let line = if area.width >= 21 {
            Line::from(
                Screen::ALL
                    .iter()
                    .enumerate()
                    .flat_map(|(index, _)| {
                        let style = if index == selected {
                            THEME.selected()
                        } else {
                            THEME.key()
                        };
                        [
                            Span::styled(format!(" {} ", index + 1), style),
                            Span::raw(" "),
                        ]
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            Line::from(vec![
                Span::styled("▶ ", THEME.focused()),
                Span::styled(format!("{}/7 ", selected + 1), THEME.selected()),
                Span::raw(text::truncate(
                    app.active_screen.label(),
                    area.width.saturating_sub(7) as usize,
                )),
            ])
        };
        frame.render_widget(Paragraph::new(line), area);
        return;
    }
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
    let mut tabs = Tabs::new(titles);
    if !compact {
        tabs = tabs.block(
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
        );
    }
    let tabs = tabs
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
                    repository::RepoMode::Browser | repository::RepoMode::Namespaces => {}
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
            repository::RepoMode::Namespaces => "Managing namespaces",
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
            if !app.tasks.is_busy() && app.config.is_some() {
                spans.extend([
                    Span::styled("c", THEME.key()),
                    Span::raw(" check  "),
                    Span::styled("p", THEME.key()),
                    Span::raw(" push  "),
                ]);
                if app
                    .config
                    .as_ref()
                    .is_some_and(|config| !config.sources.is_empty())
                {
                    spans.extend([Span::styled("b", THEME.key()), Span::raw(" backup  ")]);
                }
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
        Screen::Preview => {
            let mut spans = vec![
                Span::styled("Tab", THEME.key()),
                Span::raw(" tabs  "),
                Span::styled("r", THEME.key()),
                Span::raw(" refresh  "),
            ];
            if app.config.is_some() {
                spans.extend([Span::styled("p", THEME.key()), Span::raw(" push  ")]);
                if app
                    .config
                    .as_ref()
                    .is_some_and(|config| !config.sources.is_empty())
                {
                    spans.extend([Span::styled("b", THEME.key()), Span::raw(" backup  ")]);
                }
            }
            spans.extend([Span::styled("↑↓/jk", THEME.key()), Span::raw(" scroll")]);
            Line::from(spans)
        }
        Screen::Automation => {
            use crate::tui::screens::automation::ConfirmAction;
            if app.automation_screen.confirm == ConfirmAction::None {
                let mut spans = vec![
                    Span::styled("Tab", THEME.key()),
                    Span::raw(" tabs  "),
                    Span::styled("r", THEME.key()),
                    Span::raw(" refresh"),
                ];
                if app.config.is_some() && app.paths.is_some() {
                    spans.extend([
                        Span::raw("  "),
                        Span::styled("i", THEME.key()),
                        Span::raw(" install  "),
                        Span::styled("x", THEME.key()),
                        Span::raw(" remove"),
                    ]);
                }
                Line::from(spans)
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
                Span::styled("m", Style::default().fg(Color::Cyan)),
                Span::raw(" namespaces  "),
                Span::styled("n", Style::default().fg(Color::Cyan)),
                Span::raw(" create  "),
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
        RepoMode::Namespaces => Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(" tabs  "),
            Span::styled("↑↓/jk", Style::default().fg(Color::Cyan)),
            Span::raw(" select  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(" use/create  "),
            Span::styled("n/r/d", Style::default().fg(Color::Cyan)),
            Span::raw(" create/rename/delete  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(" back"),
        ]),
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

    // Primary health is always drawn first. Short terminals reserve their
    // scarce rows for it; medium and narrow terminals stack secondary details.
    match layout_class(inner) {
        LayoutClass::Short => draw_dashboard_status(frame, inner, app),
        LayoutClass::Narrow | LayoutClass::Medium => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(inner);
            draw_dashboard_status(frame, rows[0], app);
            draw_dashboard_info(frame, rows[1], app);
        }
        LayoutClass::Wide => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
                .split(inner);
            draw_dashboard_status(frame, columns[0], app);
            draw_dashboard_info(frame, columns[1], app);
        }
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
        return ("UNCONFIGURED — setup required", THEME.warning());
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
    let width = usize::from(width)
        .saturating_sub(text::display_width(prefix))
        .max(1);
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
    let available = usize::from(width)
        .saturating_sub(text::display_width(label).saturating_add(4))
        .max(1);
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

    // Keep the selected run and its detail usable on narrow terminals by
    // stacking rather than squeezing both panes into unusable columns.
    let panes = history_panes(inner);
    let list_area = panes[0];
    let detail_area = panes[1];

    // List of runs.
    let mut list_lines: Vec<Line> = Vec::new();
    list_lines.push(Line::from(Span::styled(
        " Recent runs:",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    list_lines.push(Line::from(""));

    let available_rows = list_area.height.saturating_sub(2) as usize;
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
            Span::styled(
                entry
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "unknown namespace".to_string()),
                THEME.label(),
            ),
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
    frame.render_widget(list_paragraph, list_area);

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
        detail_lines.push(field_line(
            " Namespace",
            entry
                .namespace
                .unwrap_or_else(|| "Unknown (legacy run)".to_string()),
        ));
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
            let max_width = detail_area.width.saturating_sub(3) as usize;
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
    frame.render_widget(detail_paragraph, detail_area);
}

/// Return list/detail rectangles, stacking below the medium-width breakpoint.
fn history_panes(area: Rect) -> std::rc::Rc<[Rect]> {
    if area.width < 80 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(area)
    }
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
    if let Some(namespace) = &app.history_screen.log_namespace {
        lines.push(field_line(" Namespace", namespace.clone()));
    } else {
        lines.push(dim_line(" Namespace: unknown (legacy run)"));
    }
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
                "  Status not loaded. Press r to check automation status.",
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
            lines.push(dim_line(
                "  Automation status is stale. Press r to refresh.",
            ));
            if let Some(status) = previous {
                lines.push(field_line("  Previous", status.clone()));
            }
        }
        LoadState::Failed { error, previous } => {
            lines.push(Line::from(Span::styled(
                format!("  Status check failed: {error}. Press r to retry."),
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
            lines.push(dim_line("  Preview is stale. Press r to refresh."));
        }
        LoadState::Failed { error, .. } => lines.push(Line::from(Span::styled(
            format!("  Preview failed: {error}. Press r to retry."),
            Style::default().fg(Color::Red),
        ))),
        LoadState::NotLoaded | LoadState::Loaded(_) => {}
    }

    let Some(data) = app.preview_screen.load_state.data() else {
        lines.push(Line::from(""));
        let message = if app.preview_screen.load_state.is_loading() {
            "  Waiting for the backup planner."
        } else {
            "  Preview has not been generated. Press r to create one."
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
                "Added: {}  Changed: {}  Deleted: {}  Ignored: {}  Warning: {}",
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
            "▶ Source selector [FOCUS]: "
        } else {
            "  Source selector: "
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
        let current_source = &sources[app.ignore_screen.source_idx];
        lines.push(field_line(" Source", current_source.path.clone()));
        let active_rule = current_source
            .ignore
            .get(app.ignore_screen.pattern_idx)
            .map_or("No selected rule", String::as_str);
        lines.push(field_line(" Active rule context", active_rule));
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
            LoadState::Stale { .. } => {
                lines.push(dim_line("  Preview is stale. Press r to refresh."))
            }
            LoadState::Failed { error, .. } => lines.push(Line::from(Span::styled(
                format!("  Preview failed: {error}. Press r to retry."),
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
            } else if matches!(app.ignore_screen.preview_state, LoadState::NotLoaded) {
                "  No preview yet. Press p to scan this source."
            } else {
                "  No matching files found for this source."
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
                    spans.push(Span::styled("[ignored] ", Style::default().fg(Color::Red)));
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
                "▶ Pattern list [FOCUS]:"
            } else {
                "  Pattern list:"
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
            crate::tui::picker::draw_with_presentation(
                frame,
                chunks[0],
                browser,
                check_fn,
                crate::tui::picker::Presentation::SOURCES,
            );
        } else {
            let msg = Paragraph::new(Line::from(Span::styled(
                " Browser is not ready. Press Esc, then a to try again.",
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
        let next_step = if app.config.is_some() {
            "  Press a to add a source and begin backing up files."
        } else {
            "  Select and validate a repository before adding sources."
        };
        lines.push(dim_line(next_step));
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

    // Show every pending path and generated rule before the user chooses apply.
    if app.sources_screen.mode == Mode::PendingChanges {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                "Pending source changes require a decision.",
                THEME.warning(),
            ),
        ]));
        if let Some(diff) = &app.sources_screen.pending_diff {
            append_source_diff_lines(&mut lines, diff, inner.width);
        }
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

            append_source_diff_lines(&mut lines, diff, inner.width);
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

/// Append the complete reviewed source selection diff before an apply action.
fn append_source_diff_lines(
    lines: &mut Vec<Line<'static>>,
    diff: &crate::tui::selection::SelectionDiff,
    width: u16,
) {
    let path_width = width.saturating_sub(8) as usize;
    if !diff.additions.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Sources to add:",
            THEME.success(),
        )));
        for path in &diff.additions {
            lines.push(Line::from(Span::styled(
                format!("    + {}", text::truncate(path, path_width)),
                THEME.success(),
            )));
        }
    }
    if !diff.removals.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Sources to remove:",
            THEME.error(),
        )));
        for path in &diff.removals {
            lines.push(Line::from(Span::styled(
                format!("    - {}", text::truncate(path, path_width)),
                THEME.error(),
            )));
        }
    }
    if !diff.ignore_rules.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Generated ignore rules:",
            THEME.warning(),
        )));
        let mut sources: Vec<_> = diff.ignore_rules.iter().collect();
        sources.sort_by(|left, right| left.0.cmp(right.0));
        for (source, rules) in sources {
            for rule in rules {
                lines.push(Line::from(Span::styled(
                    format!("    {source}: {}", text::truncate(rule, path_width)),
                    THEME.warning(),
                )));
            }
        }
    }
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
                crate::tui::picker::draw_with_presentation(
                    frame,
                    chunks[0],
                    browser,
                    None,
                    crate::tui::picker::Presentation::REPOSITORY,
                );
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
                    Span::styled(
                        format!("✗ {error}. Select a directory to retry."),
                        Style::default().fg(Color::Red),
                    ),
                ])),
                LoadState::Stale { .. } => lines.push(dim_line(
                    " Repository validation is stale. Select a directory to validate again.",
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
        RepoMode::Namespaces => {
            let mut lines = vec![section_header("Namespaces")];
            lines.push(dim_line(
                "  Active and discovered direct repository namespaces",
            ));
            lines.push(Line::from(""));
            if app.repo_screen.namespaces.is_empty() {
                lines.push(dim_line(
                    "  No repository selected. Create a namespace after selecting a repository.",
                ));
            }
            for (index, item) in app.repo_screen.namespaces.iter().enumerate() {
                let selected = index == app.repo_screen.namespace_selected;
                let marker = if selected { "▶" } else { " " };
                let active = if item.active { " active" } else { " sibling" };
                let state_style = match item.ownership {
                    crate::tui::screens::repository::OwnershipInfo::New => THEME.warning(),
                    crate::tui::screens::repository::OwnershipInfo::Owned { .. } => THEME.success(),
                    crate::tui::screens::repository::OwnershipInfo::InvalidManifest(_) => {
                        THEME.error()
                    }
                    crate::tui::screens::repository::OwnershipInfo::Ambiguous(_) => THEME.warning(),
                };
                let row_style = if selected {
                    THEME.selected()
                } else {
                    Style::default()
                };
                lines.push(Line::from(vec![
                    Span::styled(format!(" {marker} "), row_style),
                    Span::styled(item.name.clone(), row_style),
                    Span::raw(active),
                    Span::raw(" — "),
                    Span::styled(item.ownership.label(), state_style),
                ]));
                if selected {
                    match &item.ownership {
                        crate::tui::screens::repository::OwnershipInfo::InvalidManifest(reason)
                        | crate::tui::screens::repository::OwnershipInfo::Ambiguous(reason) => {
                            for line in text::wrap(reason, inner.width.saturating_sub(4) as usize) {
                                lines.push(dim_line(format!("     {line}")));
                            }
                        }
                        _ => {}
                    }
                }
            }
            lines.push(Line::from(""));
            lines.push(field_line("  Create", "n (choose a new name)"));
            lines.push(field_line(
                "  Select",
                "Enter (new and owned namespaces only)",
            ));
            lines.push(field_line("  Rename", "r (active namespace only)"));
            lines.push(field_line("  Delete", "d (active; type a replacement)"));
            frame.render_widget(Paragraph::new(lines), inner);
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
                    Span::styled(
                        format!("✗ {error}. Press Enter to retry."),
                        Style::default().fg(Color::Red),
                    ),
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
#[path = "../../tests/unit/tui/ui.rs"]
mod tests;
