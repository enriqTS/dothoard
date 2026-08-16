//! Layout and rendering for the TUI.
//!
//! Each screen has its own rendering function. The top-level `draw` function
//! renders the tab bar and delegates to the active screen's renderer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Tabs, Wrap};

use super::pointer::{ClickAction, ScrollAction};
use super::task::LoadState;
use super::{App, Screen, modal, text, theme};

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
    app.clear_pointer_map();
    if app.setup.is_some() {
        draw_setup(frame, app);
        return;
    }
    let area = frame.area();
    frame.render_widget(Block::default().style(theme::current().canvas()), area);
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
    app.register_click(chunks[1], ClickAction::FocusContent);
    if screen_scrolls(app) {
        app.register_scroll(chunks[1], ScrollAction::Vertical);
    }
    draw_screen(frame, chunks[1], app);
    draw_status_bar(frame, chunks[2], app);
    draw_help_bar(frame, chunks[3], app);
    if modal_owns_pointer(app) {
        app.clear_pointer_map();
        let line = help_bar_content_focus(app);
        register_help_clicks(chunks[3], app, &line);
    }
    draw_modal_overlay(frame, area, app);
    if app.theme_picker.is_some() {
        draw_theme_picker(frame, area, app);
    }
}

fn draw_setup(frame: &mut Frame, app: &mut App) {
    use super::setup::{CloneField, RepositoryMethod, RepositorySetupMode, SetupStep};

    let area = frame.area();
    frame.render_widget(Block::default().style(theme::current().canvas()), area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
    let step = app
        .setup
        .as_ref()
        .map(|setup| setup.step)
        .unwrap_or(SetupStep::Repository);
    let step_number = match step {
        SetupStep::Repository => 1,
        SetupStep::Namespace => 2,
        SetupStep::Automation => 3,
        SetupStep::Theme => 4,
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" dothoard setup ", theme::current().heading()),
            Span::styled(
                format!("Step {step_number} of 4 · "),
                theme::current().muted(),
            ),
            Span::styled(
                match step {
                    SetupStep::Repository => "Repository",
                    SetupStep::Namespace => "Namespace",
                    SetupStep::Automation => "Automation",
                    SetupStep::Theme => "Theme",
                },
                theme::current().label(),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(theme::current().border(true)),
        )
        .style(theme::current().chrome()),
        chunks[0],
    );

    match step {
        SetupStep::Repository => {
            let mode = app
                .setup
                .as_ref()
                .map(|setup| setup.repository_mode)
                .unwrap_or(RepositorySetupMode::Choose);
            match mode {
                RepositorySetupMode::Choose => {
                    let selected = app
                        .setup
                        .as_ref()
                        .map(|setup| setup.repository_method)
                        .unwrap_or(RepositoryMethod::Existing);
                    let option = |method, label: &'static str, detail: &'static str| {
                        Line::from(vec![
                            Span::styled(
                                if selected == method { "▶ " } else { "  " },
                                theme::current().focused(),
                            ),
                            Span::styled(
                                label,
                                if selected == method {
                                    theme::current().selected()
                                } else {
                                    Style::default()
                                },
                            ),
                            Span::styled(format!(" — {detail}"), theme::current().muted()),
                        ])
                    };
                    frame.render_widget(
                        Paragraph::new(vec![
                            Line::from(Span::styled(
                                "How should dothoard obtain the backup repository?",
                                theme::current().heading(),
                            )),
                            Line::from(""),
                            option(
                                RepositoryMethod::Existing,
                                "Use an existing clone",
                                "browse to a dedicated Git worktree",
                            ),
                            option(
                                RepositoryMethod::Clone,
                                "Clone from a Git URL",
                                "create a new local clone at a path you choose",
                            ),
                        ])
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(theme::current().border(true))
                                .title(" Repository setup "),
                        )
                        .wrap(Wrap { trim: false }),
                        chunks[1],
                    );
                }
                RepositorySetupMode::Existing => draw_repository(frame, chunks[1], app),
                RepositorySetupMode::Clone => {
                    let setup = app.setup.as_ref().expect("setup is active");
                    let focused = |field| {
                        if setup.clone_field == field {
                            theme::current().selected()
                        } else {
                            Style::default()
                        }
                    };
                    let state = if setup.cloning() {
                        ("Cloning repository…", theme::current().progress())
                    } else if let Some(error) = setup.clone_error() {
                        (error, theme::current().error())
                    } else if setup.clone_state.data().is_some() {
                        match &app.repo_screen.validation {
                            LoadState::Loading { .. } => (
                                "Clone complete. Validating repository…",
                                theme::current().progress(),
                            ),
                            LoadState::Failed { error, .. } => {
                                (error.as_str(), theme::current().error())
                            }
                            _ => (
                                "Clone complete. Press Enter to validate again.",
                                theme::current().success(),
                            ),
                        }
                    } else {
                        (
                            "The destination must be a new path whose parent already exists.",
                            theme::current().muted(),
                        )
                    };
                    frame.render_widget(
                        Paragraph::new(vec![
                            Line::from(Span::styled(
                                "Clone an existing remote repository",
                                theme::current().heading(),
                            )),
                            Line::from(""),
                            Line::from(vec![
                                Span::styled(
                                    if setup.clone_field == CloneField::Url {
                                        "▶ Git URL: "
                                    } else {
                                        "  Git URL: "
                                    },
                                    theme::current().label(),
                                ),
                                Span::styled(&setup.clone_url, focused(CloneField::Url)),
                            ]),
                            Line::from(vec![
                                Span::styled(
                                    if setup.clone_field == CloneField::Destination {
                                        "▶ Local path: "
                                    } else {
                                        "  Local path: "
                                    },
                                    theme::current().label(),
                                ),
                                Span::styled(
                                    &setup.clone_destination,
                                    focused(CloneField::Destination),
                                ),
                            ]),
                            Line::from(""),
                            Line::from(Span::styled(state.0, state.1)),
                        ])
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .border_style(theme::current().border(true))
                                .title(" Clone repository "),
                        )
                        .wrap(Wrap { trim: false }),
                        chunks[1],
                    );
                }
            }
        }
        SetupStep::Namespace => draw_repository(frame, chunks[1], app),
        SetupStep::Automation => {
            use super::setup::AutomationField;
            use crate::config::AutomationBackend;
            let setup = app.setup.as_ref().expect("setup is active");
            let backends = [
                (
                    AutomationBackend::Systemd,
                    "systemd",
                    "managed user timer; completion-relative scheduling",
                ),
                (
                    AutomationBackend::Cron,
                    "cron",
                    "managed crontab block; intervals 1–59 minutes",
                ),
                (
                    AutomationBackend::External,
                    "external",
                    "you own scheduling; dothoard prints the command",
                ),
            ];
            let mut lines = vec![
                Line::from(Span::styled(
                    "Choose how backups will be scheduled",
                    theme::current().heading(),
                )),
                Line::from(""),
            ];
            lines.extend(backends.into_iter().map(|(backend, label, detail)| {
                let selected = setup.automation_backend == backend;
                Line::from(vec![
                    Span::styled(
                        if selected { "▶ " } else { "  " },
                        theme::current().focused(),
                    ),
                    Span::styled(
                        format!("{label:<9}"),
                        if selected {
                            theme::current().selected()
                        } else {
                            Style::default()
                        },
                    ),
                    Span::styled(format!(" {detail}"), theme::current().muted()),
                ])
            }));
            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled(
                    if setup.automation_field == AutomationField::Interval {
                        "▶ Interval (minutes): "
                    } else {
                        "  Interval (minutes): "
                    },
                    theme::current().label(),
                ),
                Span::styled(
                    &setup.interval_input,
                    if setup.automation_field == AutomationField::Interval {
                        theme::current().selected()
                    } else {
                        Style::default()
                    },
                ),
            ]));
            if let Some(error) = setup.automation_error.as_deref() {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(error, theme::current().error())));
            }
            frame.render_widget(
                Paragraph::new(lines)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_style(theme::current().border(true))
                            .title(" Automation "),
                    )
                    .wrap(Wrap { trim: false }),
                chunks[1],
            );
        }
        SetupStep::Theme => {
            let setup = app.setup.as_ref().expect("setup is active");
            let mut lines = vec![Line::from(Span::styled(
                "Move through every theme to preview it live",
                theme::current().heading(),
            ))];
            lines.extend(theme::ThemeId::ALL.iter().map(|id| {
                let selected = *id == setup.theme_selected;
                Line::from(vec![
                    Span::styled(
                        if selected { "▶ " } else { "  " },
                        theme::current().focused(),
                    ),
                    Span::styled(
                        id.label(),
                        if selected {
                            theme::current().selected()
                        } else {
                            Style::default()
                        },
                    ),
                ])
            }));
            if let Some(error) = setup.theme_error.as_deref() {
                lines.push(Line::from(Span::styled(error, theme::current().error())));
            }
            frame.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(theme::current().border(true))
                        .title(" Theme preview "),
                ),
                chunks[1],
            );
        }
    }

    let help = match step {
        SetupStep::Repository => match app
            .setup
            .as_ref()
            .map(|setup| setup.repository_mode)
            .unwrap_or(RepositorySetupMode::Choose)
        {
            RepositorySetupMode::Choose => "←→/↑↓ choose  Enter continue  Esc quit",
            RepositorySetupMode::Existing => {
                "Space select repository  ↑↓←→ navigate  : type path  Esc back"
            }
            RepositorySetupMode::Clone => "Tab/↑↓ switch field  Enter clone  Esc back  Ctrl+C quit",
        },
        SetupStep::Namespace => "↑↓ choose  Enter use  n new  Esc back",
        SetupStep::Automation => {
            "←→ change backend  Tab interval  ↑↓ adjust  Enter continue  Esc back"
        }
        SetupStep::Theme => "↑↓/jk live preview  Enter finish setup  Esc back",
    };
    frame.render_widget(
        Paragraph::new(help)
            .style(theme::current().chrome())
            .block(Block::default().borders(Borders::TOP)),
        chunks[2],
    );

    if step == SetupStep::Namespace {
        draw_modal_overlay(frame, area, app);
    }
}

/// Render the global theme picker (Ctrl+T). Each row previews its own
/// theme's palette with a swatch strip, independent of whichever theme is
/// currently active, so every option can be compared at a glance; moving
/// the selection also live-applies that theme to the rest of the interface
/// behind the dialog.
fn modal_owns_pointer(app: &App) -> bool {
    use crate::tui::screens::{automation, ignore, repository, sources};
    if app.theme_picker.is_some() {
        return true;
    }
    match app.active_screen {
        Screen::Dashboard => app.dashboard_screen.detail.is_some(),
        Screen::Repository => {
            app.repo_screen.namespace_confirmation.is_some()
                || matches!(
                    app.repo_screen.confirm_state,
                    repository::ConfirmState::AskInitialize | repository::ConfirmState::AskAttach
                )
                || matches!(
                    app.repo_screen.mode,
                    repository::RepoMode::TextInput | repository::RepoMode::NamespaceInput
                )
        }
        Screen::Sources => matches!(
            app.sources_screen.mode,
            sources::Mode::AddInput
                | sources::Mode::ConfirmDelete
                | sources::Mode::PendingChanges
                | sources::Mode::ConfirmApply
        ),
        Screen::Ignore => app.ignore_screen.mode == ignore::Mode::AddInput,
        Screen::Automation => app.automation_screen.confirm != automation::ConfirmAction::None,
        Screen::Preview | Screen::History => false,
    }
}

fn screen_scrolls(app: &App) -> bool {
    matches!(
        app.active_screen,
        Screen::Repository | Screen::Sources | Screen::Ignore | Screen::Preview | Screen::History
    )
}

fn draw_theme_picker(frame: &mut Frame, area: Rect, app: &mut App) {
    app.clear_pointer_map();
    let picker = app.theme_picker.as_ref().expect("theme picker is open");
    use super::theme::ThemeId;

    let width = area.width.saturating_sub(6).clamp(30, 56);
    let height = u16::try_from(ThemeId::ALL.len() + 4)
        .unwrap_or(u16::MAX)
        .min(area.height);
    let dialog = modal::dialog_area(area, width, height);

    frame.render_widget(Block::default().style(theme::current().backdrop()), area);
    frame.render_widget(Clear, dialog);
    let block = Block::default()
        .style(theme::current().surface())
        .borders(Borders::ALL)
        .border_style(theme::current().dialog())
        .title(Line::from(Span::styled(
            " Select Theme ",
            theme::current().heading(),
        )));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let rows: Vec<Line> = ThemeId::ALL
        .iter()
        .map(|&id| {
            let palette = id.palette();
            let swatch = [
                palette.accent,
                palette.secondary,
                palette.success,
                palette.warning,
                palette.error,
            ];
            let marker = if id == picker.selected { "▶ " } else { "  " };
            let name_style = if id == picker.selected {
                theme::current().selected()
            } else {
                Style::default()
            };
            let mut spans = vec![Span::styled(marker, theme::current().focused())];
            spans.extend(
                swatch
                    .into_iter()
                    .map(|color| Span::styled("██", Style::default().fg(color))),
            );
            spans.push(Span::raw(" "));
            spans.push(Span::styled(format!("{:<18}", id.label()), name_style));
            Line::from(spans)
        })
        .collect();

    let mut lines = vec![Line::from("")];
    lines.extend(rows);
    for (index, _) in ThemeId::ALL.iter().enumerate() {
        app.register_click(
            Rect::new(
                inner.x,
                inner.y.saturating_add(1 + index as u16),
                inner.width,
                1,
            ),
            ClickAction::Theme(index),
        );
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("↑↓/jk", theme::current().key()),
        Span::raw(" preview  "),
        Span::styled("Enter", theme::current().key()),
        Span::raw(" save  "),
        Span::styled("Esc", theme::current().key()),
        Span::raw(" cancel"),
    ]));
    let action_y = inner.y.saturating_add(lines.len().saturating_sub(1) as u16);
    app.register_click(
        Rect::new(inner.x.saturating_add(15), action_y, 5, 1),
        ClickAction::Key(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ),
    );
    app.register_click(
        Rect::new(inner.x.saturating_add(27), action_y, 3, 1),
        ClickAction::Key(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    frame.render_widget(Paragraph::new(lines), inner);
}

/// Draw the tab bar at the top.
fn draw_tabs(frame: &mut Frame, area: Rect, app: &mut App) {
    use super::Focus;

    frame.render_widget(Block::default().style(theme::current().chrome()), area);

    register_tab_regions(area, app);
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
                            theme::current().selected()
                        } else {
                            theme::current().key()
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
                Span::styled("▶ ", theme::current().focused()),
                Span::styled(format!("{}/7 ", selected + 1), theme::current().selected()),
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
                Span::styled(num, theme::current().key()),
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
    let marker = if tab_focused {
        " ▶ TAB FOCUS · dothoard "
    } else {
        "   Tabs · dothoard "
    };
    let marker_style = if tab_focused {
        theme::current().focused()
    } else {
        theme::current().muted()
    };
    let mut title_spans = vec![Span::styled(marker, marker_style)];
    if let Some(config) = &app.config {
        title_spans.push(Span::styled("· namespace ", theme::current().muted()));
        title_spans.push(Span::styled(
            config.namespace.clone(),
            theme::current().label(),
        ));
        title_spans.push(Span::raw(" "));
    }
    let mut tabs = Tabs::new(titles);
    if !compact {
        tabs = tabs.block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(theme::current().border(tab_focused))
                .title(Line::from(title_spans)),
        );
    }
    let tabs = tabs
        .select(selected)
        .style(theme::current().chrome())
        .highlight_style(if tab_focused {
            theme::current().selected()
        } else {
            theme::current().heading()
        });

    frame.render_widget(tabs, area);
}

fn register_tab_regions(area: Rect, app: &mut App) {
    if area.width < 21 {
        app.register_click(area, ClickAction::Tab(app.active_screen));
        return;
    }
    let mut x = area.x;
    for (index, screen) in Screen::ALL.iter().copied().enumerate() {
        let width = if area.height <= 1 {
            4
        } else {
            u16::try_from(screen.label().len() + 5).unwrap_or(u16::MAX)
        };
        let width = width.min(area.right().saturating_sub(x));
        app.register_click(
            Rect::new(x, area.y, width, area.height),
            ClickAction::Tab(screen),
        );
        x = x.saturating_add(width);
        if index + 1 == Screen::ALL.len() || x >= area.right() {
            break;
        }
    }
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
                                Some((error.as_str(), theme::current().error()))
                            }
                            LoadState::Loading { .. } => {
                                Some(("Checking repository…", theme::current().progress()))
                            }
                            _ => app
                                .repo_screen
                                .selection_error
                                .as_deref()
                                .map(|error| (error, theme::current().error())),
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
                                validation: affected
                                    .as_deref()
                                    .map(|path| (path, theme::current().muted())),
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
                        .map(|message| (message.text.as_str(), theme::current().error())),
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
                    consequence: "Pending source changes require a decision. Apply them, discard them, or continue editing.",
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
                        .map(|message| (message.text.as_str(), theme::current().error())),
                    submit: "Enter: add",
                    cancel: "Esc: cancel",
                },
            );
        }
        Screen::Automation if app.automation_screen.confirm != automation::ConfirmAction::None => {
            let removing = app.automation_screen.confirm == automation::ConfirmAction::Remove;
            let backend = app
                .config
                .as_ref()
                .map(crate::automation::selected_backend)
                .unwrap_or_default();
            modal::draw(
                frame,
                area,
                modal::ModalSpec {
                    title: if removing {
                        "Remove automation"
                    } else {
                        "Install automation"
                    },
                    affected: Some(backend.description()),
                    consequence: if removing {
                        "This disables and removes only dothoard's managed automation."
                    } else {
                        "This installs and enables the selected backend using the current schedule."
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
    theme::current().border(app.focus == super::Focus::Content)
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
                theme::current().focused()
            } else {
                theme::current().muted()
            },
        ),
        Span::styled(format!("[{mode}] "), theme::current().label()),
    ])
}

/// Draw transient feedback and progress independently from keyboard help.
fn draw_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    use super::status::{StatusKind, StatusMessage};

    frame.render_widget(Block::default().style(theme::current().chrome()), area);

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
                theme::current().status(message.kind),
            ))),
            area,
        );
    }
}

/// Draw the authoritative mode-aware shortcut footer.
fn draw_help_bar(frame: &mut Frame, area: Rect, app: &App) {
    use super::Focus;

    frame.render_widget(Block::default().style(theme::current().chrome()), area);

    let line = match app.focus {
        Focus::TabBar => help_bar_tab_focus(),
        Focus::Content => help_bar_content_focus(app),
    };
    register_help_clicks(area, app, &line);
    frame.render_widget(Paragraph::new(line), area);
}

fn register_help_clicks(area: Rect, app: &App, line: &Line<'_>) {
    use crossterm::event::{KeyCode, KeyModifiers};

    let mut x = area.x;
    for span in &line.spans {
        let width = u16::try_from(unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
            .unwrap_or(u16::MAX)
            .min(area.right().saturating_sub(x));
        if span.content.as_ref() == "n/r/d" {
            for (offset, key) in [(0, 'n'), (2, 'r'), (4, 'd')] {
                app.register_click(
                    Rect::new(x.saturating_add(offset), area.y, 1, area.height),
                    ClickAction::Key(KeyCode::Char(key), KeyModifiers::NONE),
                );
            }
            x = x.saturating_add(width);
            continue;
        }
        let key = match span.content.as_ref() {
            "Tab" => Some((KeyCode::Tab, KeyModifiers::NONE)),
            "Enter" => Some((KeyCode::Enter, KeyModifiers::NONE)),
            "Space" => Some((KeyCode::Char(' '), KeyModifiers::NONE)),
            "Esc" | "n/Esc" | "c/Esc" => Some((KeyCode::Esc, KeyModifiers::NONE)),
            "Ctrl+T" => Some((KeyCode::Char('t'), KeyModifiers::CONTROL)),
            "Ctrl+U" => Some((KeyCode::Char('u'), KeyModifiers::CONTROL)),
            ":/" => Some((KeyCode::Char(':'), KeyModifiers::NONE)),
            key if key.len() == 1 => key
                .chars()
                .next()
                .map(|key| (KeyCode::Char(key), KeyModifiers::NONE)),
            _ => None,
        };
        if let Some((code, modifiers)) = key {
            app.register_click(
                Rect::new(x, area.y, width, area.height),
                ClickAction::Key(code, modifiers),
            );
        }
        x = x.saturating_add(width);
        if x >= area.right() {
            break;
        }
    }
}

/// Help bar when the tab bar has focus.
fn help_bar_tab_focus() -> Line<'static> {
    Line::from(vec![
        Span::styled("←→/hl", theme::current().key()),
        Span::raw(" tabs  "),
        Span::styled("↓/j/Enter", theme::current().key()),
        Span::raw(" content  "),
        Span::styled("1-7", theme::current().key()),
        Span::raw(" jump  "),
        Span::styled("Ctrl+T", theme::current().key()),
        Span::raw(" theme  "),
        Span::styled("q", theme::current().key()),
        Span::raw(" quit"),
    ])
}

/// Help bar when content has focus.
fn help_bar_content_focus(app: &App) -> Line<'static> {
    match app.active_screen {
        Screen::Dashboard if app.dashboard_screen.detail.is_some() => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" close details"),
        ]),
        Screen::Dashboard => {
            let mut spans = vec![
                Span::styled("Tab", theme::current().key()),
                Span::raw(" tabs  "),
            ];
            if !app.tasks.is_busy() && app.config.is_some() {
                spans.extend([
                    Span::styled("c", theme::current().key()),
                    Span::raw(" check  "),
                    Span::styled("p", theme::current().key()),
                    Span::raw(" push  "),
                ]);
                if app
                    .config
                    .as_ref()
                    .is_some_and(|config| !config.sources.is_empty())
                {
                    spans.extend([
                        Span::styled("b", theme::current().key()),
                        Span::raw(" backup  "),
                    ]);
                }
            }
            spans.extend([
                Span::styled("a", theme::current().key()),
                Span::raw(" automation  "),
                Span::styled("r", theme::current().key()),
                Span::raw(" repository  "),
                Span::styled("d", theme::current().key()),
                Span::raw(" details  "),
                Span::styled("q", theme::current().key()),
                Span::raw(" quit"),
            ]);
            Line::from(spans)
        }
        Screen::Repository => help_bar_repository(app),
        Screen::Sources => help_bar_sources(app),
        Screen::Ignore => help_bar_ignore(app),
        Screen::Preview => {
            let mut spans = vec![
                Span::styled("Tab", theme::current().key()),
                Span::raw(" tabs  "),
                Span::styled("r", theme::current().key()),
                Span::raw(" refresh  "),
            ];
            if app.config.is_some() {
                spans.extend([
                    Span::styled("p", theme::current().key()),
                    Span::raw(" push  "),
                ]);
                if app
                    .config
                    .as_ref()
                    .is_some_and(|config| !config.sources.is_empty())
                {
                    spans.extend([
                        Span::styled("b", theme::current().key()),
                        Span::raw(" backup  "),
                    ]);
                }
            }
            spans.extend([
                Span::styled("↑↓/jk", theme::current().key()),
                Span::raw(" scroll"),
            ]);
            Line::from(spans)
        }
        Screen::Automation => {
            use crate::tui::screens::automation::ConfirmAction;
            if app.automation_screen.confirm == ConfirmAction::None {
                let mut spans = vec![
                    Span::styled("Tab", theme::current().key()),
                    Span::raw(" tabs  "),
                    Span::styled("r", theme::current().key()),
                    Span::raw(" refresh"),
                ];
                if app.config.is_some() && app.paths.is_some() {
                    spans.extend([
                        Span::raw("  "),
                        Span::styled("b", theme::current().key()),
                        Span::raw(" backend"),
                    ]);
                    if app.config.as_ref().is_some_and(|config| {
                        crate::automation::selected_backend(config)
                            != crate::automation::Backend::External
                    }) {
                        spans.extend([
                            Span::raw("  "),
                            Span::styled("i", theme::current().key()),
                            Span::raw(" install  "),
                            Span::styled("x", theme::current().key()),
                            Span::raw(" remove"),
                        ]);
                    }
                }
                Line::from(spans)
            } else {
                Line::from(vec![
                    Span::styled("Tab", theme::current().key()),
                    Span::raw(" tabs  "),
                    Span::styled("y", theme::current().key()),
                    Span::raw(" confirm  "),
                    Span::styled("n/Esc", theme::current().key()),
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
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("y", theme::current().key()),
            Span::raw(" confirm  "),
            Span::styled("n/Esc", theme::current().key()),
            Span::raw(" cancel"),
        ]);
    }

    match app.repo_screen.mode {
        RepoMode::Browser => {
            let mut spans = vec![
                Span::styled("Tab", theme::current().key()),
                Span::raw(" tabs  "),
            ];
            if app.repo_screen.repository_locked {
                spans.extend([
                    Span::styled("c", theme::current().key()),
                    Span::raw(" change repository  "),
                ]);
            } else {
                spans.extend([
                    Span::styled("Space", theme::current().key()),
                    Span::raw(" select  "),
                ]);
            }
            spans.extend([
                Span::styled("m", theme::current().key()),
                Span::raw(" namespaces  "),
                Span::styled("n", theme::current().key()),
                Span::raw(" create  "),
            ]);
            if app.config.is_some() {
                spans.extend([
                    Span::styled("r", theme::current().key()),
                    Span::raw(" rename  "),
                    Span::styled("d", theme::current().key()),
                    Span::raw(" delete  "),
                ]);
            }
            spans.extend([
                Span::styled("↑↓←→", theme::current().key()),
                Span::raw(" navigate  "),
                Span::styled("Ctrl+↑↓/jk", theme::current().key()),
                Span::raw(" content  "),
            ]);
            if !app.repo_screen.repository_locked {
                spans.extend([
                    Span::styled(":/", theme::current().key()),
                    Span::raw(" text input"),
                ]);
            }
            Line::from(spans)
        }
        RepoMode::Namespaces => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("↑↓/jk", theme::current().key()),
            Span::raw(" select  "),
            Span::styled("Enter", theme::current().key()),
            Span::raw(" use/create  "),
            Span::styled("n/r/d", theme::current().key()),
            Span::raw(" create/rename/delete  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" back"),
        ]),
        RepoMode::NamespaceInput if app.repo_screen.namespace_confirmation.is_some() => {
            Line::from(vec![
                Span::styled("Tab", theme::current().key()),
                Span::raw(" tabs  "),
                Span::styled("y", theme::current().key()),
                Span::raw(" confirm  "),
                Span::styled("n/Esc", theme::current().key()),
                Span::raw(" cancel"),
            ])
        }
        RepoMode::NamespaceInput => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Enter", theme::current().key()),
            Span::raw(" review  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" cancel  "),
            Span::styled("Ctrl+U", theme::current().key()),
            Span::raw(" clear"),
        ]),
        RepoMode::TextInput => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Enter", theme::current().key()),
            Span::raw(" validate  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" browser  "),
            Span::styled("Ctrl+U", theme::current().key()),
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
                Span::styled("Tab", theme::current().key()),
                Span::raw(" tabs  "),
                Span::styled("a", theme::current().key()),
                Span::raw(" add"),
            ];
            if has_sources {
                spans.extend([
                    Span::raw("  "),
                    Span::styled("d", theme::current().key()),
                    Span::raw(" delete  "),
                    Span::styled("↑↓/jk", theme::current().key()),
                    Span::raw(" navigate"),
                ]);
            }
            Line::from(spans)
        }
        Mode::Browse => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Space", theme::current().key()),
            Span::raw(" toggle  "),
            Span::styled("↑↓←→", theme::current().key()),
            Span::raw(" navigate  "),
            Span::styled("Ctrl+↑↓/jk", theme::current().key()),
            Span::raw(" content  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" review changes  "),
            Span::styled(":/", theme::current().key()),
            Span::raw(" text"),
        ]),
        Mode::AddInput => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Enter", theme::current().key()),
            Span::raw(" add  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" browser  "),
            Span::styled("Ctrl+U", theme::current().key()),
            Span::raw(" clear"),
        ]),
        Mode::ConfirmDelete => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("y", theme::current().key()),
            Span::raw(" confirm  "),
            Span::styled("n/Esc", theme::current().key()),
            Span::raw(" cancel"),
        ]),
        Mode::PendingChanges => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("a", theme::current().key()),
            Span::raw(" apply  "),
            Span::styled("d", theme::current().key()),
            Span::raw(" discard  "),
            Span::styled("c/Esc", theme::current().key()),
            Span::raw(" continue editing"),
        ]),
        Mode::ConfirmApply => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("y", theme::current().key()),
            Span::raw(" remove and apply  "),
            Span::styled("n/Esc", theme::current().key()),
            Span::raw(" back to choices"),
        ]),
    }
}

/// Context-sensitive help for the Ignore screen.
fn help_bar_ignore(app: &App) -> Line<'static> {
    use crate::tui::screens::ignore::Mode;

    match app.ignore_screen.mode {
        Mode::AddInput => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Enter", theme::current().key()),
            Span::raw(" add  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" cancel  "),
            Span::styled("Ctrl+U", theme::current().key()),
            Span::raw(" clear"),
        ]),
        Mode::Preview => Line::from(vec![
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" back  "),
            Span::styled("r", theme::current().key()),
            Span::raw(" refresh  "),
            Span::styled("↑↓/jk", theme::current().key()),
            Span::raw(" scroll  "),
            Span::styled("PgUp/PgDn", theme::current().key()),
            Span::raw(" page"),
        ]),
        Mode::List => {
            let has_sources = app
                .config
                .as_ref()
                .is_some_and(|config| !config.sources.is_empty());
            if !has_sources {
                return Line::from(vec![
                    Span::styled("Tab", theme::current().key()),
                    Span::raw(" tabs"),
                ]);
            }
            let has_patterns = app
                .config
                .as_ref()
                .and_then(|config| config.sources.get(app.ignore_screen.source_idx))
                .is_some_and(|source| !source.ignore.is_empty());
            let mut spans = vec![
                Span::styled("Tab", theme::current().key()),
                Span::raw(" tabs  "),
                Span::styled("a", theme::current().key()),
                Span::raw(" add  "),
            ];
            if has_patterns {
                spans.extend([
                    Span::styled("d", theme::current().key()),
                    Span::raw(" delete  "),
                ]);
            }
            spans.extend([
                Span::styled("p", theme::current().key()),
                Span::raw(" preview  "),
                Span::styled("←→/hl", theme::current().key()),
                Span::raw(" source  "),
                Span::styled("↑↓/jk", theme::current().key()),
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
            Span::styled("Tab", theme::current().key()),
            Span::raw(" tabs  "),
            Span::styled("Esc", theme::current().key()),
            Span::raw(" back  "),
            Span::styled("↑↓/jk", theme::current().key()),
            Span::raw(" scroll  "),
            Span::styled("PgUp/PgDn", theme::current().key()),
            Span::raw(" page"),
        ]),
        Mode::History => {
            let has_history = app
                .state
                .as_ref()
                .is_some_and(|state| !state.history.is_empty());
            let mut spans = vec![
                Span::styled("Tab", theme::current().key()),
                Span::raw(" tabs  "),
            ];
            if has_history {
                spans.extend([
                    Span::styled("↑↓/jk", theme::current().key()),
                    Span::raw(" navigate  "),
                    Span::styled("Enter", theme::current().key()),
                    Span::raw(" view logs  "),
                ]);
            }
            spans.extend([
                Span::styled("q", theme::current().key()),
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
    // Wide terminals with room to spare arrange each section as its own card
    // so a tall terminal fills with framed panels instead of a short block of
    // text trailing off into empty space.
    const CARD_MIN_HEIGHT: u16 = 20;
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
        LayoutClass::Wide if inner.height >= CARD_MIN_HEIGHT => {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
                .split(inner);
            draw_dashboard_status_cards(frame, columns[0], app);
            draw_dashboard_info_cards(frame, columns[1], app);
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

fn backup_health_lines(app: &App) -> Vec<Line<'static>> {
    let (health, health_style) = dashboard_health(app);
    vec![
        Line::from(Span::styled(format!("  {health}"), health_style)),
        field_line(
            "  Last successful backup",
            app.state
                .as_ref()
                .and_then(|state| state.last_success.as_ref())
                .map(format_time)
                .unwrap_or_else(|| "Never".to_string()),
        ),
    ]
}

fn remote_sync_lines(app: &App) -> Vec<Line<'static>> {
    let pending_push = app.state.as_ref().is_some_and(|state| state.pending_push);
    let mut lines = vec![Line::from(Span::styled(
        if pending_push {
            "  Pending commits need push"
        } else {
            "  No pending commits"
        },
        if pending_push {
            theme::current().warning()
        } else {
            theme::current().success()
        },
    ))];
    if let Some(last_push) = app
        .state
        .as_ref()
        .and_then(|state| state.last_push.as_ref())
    {
        lines.push(field_line("  Last push", format_time(last_push)));
    }
    lines
}

fn automation_health_lines(app: &App) -> Vec<Line<'static>> {
    let (automation, automation_style) = dashboard_automation(app);
    vec![Line::from(Span::styled(
        format!("  {automation}"),
        automation_style,
    ))]
}

fn next_action_lines(app: &App) -> Vec<Line<'static>> {
    vec![Line::from(Span::styled(
        format!("  {}", dashboard_action(app)),
        theme::current().accent(),
    ))]
}

fn latest_check_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let (check, check_style) = dashboard_check(app);
    wrapped_styled_lines("  ", &check, width.saturating_sub(2), check_style)
}

fn repository_details_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    let Some(config) = &app.config else {
        return vec![dim_line("  No repository configured. Press r to begin.")];
    };
    let mut lines = wrapped_field_lines("  Path", &config.repository, width);
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
    lines
}

fn latest_issue_lines(app: &App, width: u16) -> Vec<Line<'static>> {
    if let Some((issue, style)) = dashboard_issue(app) {
        let mut lines = wrapped_styled_lines("  ", &issue, width.saturating_sub(2), style);
        lines.push(dim_line("  Press d for complete details."));
        lines
    } else {
        vec![Line::from(Span::styled(
            "  No issues reported",
            theme::current().success(),
        ))]
    }
}

/// Primary dashboard summaries: health, last success, synchronization,
/// automation, and the one action that best moves the user forward.
fn draw_dashboard_status(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![section_header("Backup Health")];
    lines.extend(backup_health_lines(app));
    lines.push(section_header("Remote Synchronization"));
    lines.extend(remote_sync_lines(app));
    lines.push(section_header("Automation Health"));
    lines.extend(automation_health_lines(app));
    lines.push(section_header("Recommended Next Action"));
    lines.extend(next_action_lines(app));

    frame.render_widget(Paragraph::new(lines), area);
}

/// Secondary configuration and diagnostics. All long values are wrapped by
/// display cells and their full check/error values remain available with `d`.
fn draw_dashboard_info(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![section_header("Latest Check")];
    lines.extend(latest_check_lines(app, area.width));
    lines.push(Line::from(""));

    lines.push(section_header("Repository Details"));
    lines.extend(repository_details_lines(app, area.width));
    lines.push(Line::from(""));

    lines.push(section_header("Latest Issue"));
    lines.extend(latest_issue_lines(app, area.width));

    frame.render_widget(Paragraph::new(lines), area);
}

/// Render one bordered dashboard card with its section name as the title.
fn draw_card(frame: &mut Frame, area: Rect, title: &'static str, lines: Vec<Line<'static>>) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::current().border(false))
        .title(Line::from(Span::styled(
            format!(" {title} "),
            theme::current().heading(),
        )));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

/// Card layout for `draw_dashboard_status` on tall, wide terminals: each
/// section fills an equal share of the column instead of leaving the rest of
/// a tall terminal empty below a short block of text.
fn draw_dashboard_status_cards(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 4); 4])
        .split(area);
    draw_card(frame, rows[0], "Backup Health", backup_health_lines(app));
    draw_card(
        frame,
        rows[1],
        "Remote Synchronization",
        remote_sync_lines(app),
    );
    draw_card(
        frame,
        rows[2],
        "Automation Health",
        automation_health_lines(app),
    );
    draw_card(
        frame,
        rows[3],
        "Recommended Next Action",
        next_action_lines(app),
    );
}

/// Card layout for `draw_dashboard_info`, matching `draw_dashboard_status_cards`.
fn draw_dashboard_info_cards(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Ratio(1, 3); 3])
        .split(area);
    let card_width = rows[0].width.saturating_sub(2);
    draw_card(
        frame,
        rows[0],
        "Latest Check",
        latest_check_lines(app, card_width),
    );
    draw_card(
        frame,
        rows[1],
        "Repository Details",
        repository_details_lines(app, card_width),
    );
    draw_card(
        frame,
        rows[2],
        "Latest Issue",
        latest_issue_lines(app, card_width),
    );
}

fn dashboard_health(app: &App) -> (&'static str, Style) {
    if app.config.is_none() {
        return ("UNCONFIGURED — setup required", theme::current().warning());
    }
    if app.tasks.active_task() == Some(super::task::TaskKind::Backup) {
        return ("RUNNING — backup in progress", theme::current().progress());
    }
    if app
        .state
        .as_ref()
        .is_some_and(|state| state.latest_error.is_some())
        || app.last_check.as_ref().is_some_and(|check| !check.healthy)
    {
        return (
            "NEEDS ATTENTION — review the latest issue",
            theme::current().error(),
        );
    }
    if app
        .state
        .as_ref()
        .and_then(|state| state.last_success.as_ref())
        .is_none()
    {
        return ("NO SUCCESSFUL BACKUP YET", theme::current().warning());
    }
    ("HEALTHY", theme::current().success())
}

fn dashboard_automation(app: &App) -> (String, Style) {
    use crate::tui::task::LoadState;
    match &app.automation_screen.status_state {
        LoadState::Loaded(status) => {
            let style = if status == "active" {
                theme::current().success()
            } else {
                theme::current().warning()
            };
            (status.clone(), style)
        }
        LoadState::Loading { .. } => (
            "Checking automation status…".to_string(),
            theme::current().progress(),
        ),
        LoadState::Stale {
            previous: Some(status),
        } => (format!("Stale: {status}"), theme::current().warning()),
        LoadState::Stale { previous: None } | LoadState::NotLoaded => (
            "Unavailable — not inspected yet (press a)".to_string(),
            theme::current().warning(),
        ),
        LoadState::Failed { error, previous } => (
            previous.as_ref().map_or_else(
                || format!("Unavailable: {error}"),
                |status| format!("Unavailable: {error}; previous {status}"),
            ),
            theme::current().error(),
        ),
    }
}

fn dashboard_check(app: &App) -> (String, Style) {
    if app.tasks.active_task() == Some(super::task::TaskKind::Check) {
        return (
            "Checking repository…".to_string(),
            theme::current().progress(),
        );
    }
    let Some(check) = &app.last_check else {
        return if app.config.is_some() {
            (
                "Unavailable — run check (c)".to_string(),
                theme::current().warning(),
            )
        } else {
            (
                "Unavailable — configure repository first".to_string(),
                theme::current().warning(),
            )
        };
    };
    if check.healthy {
        ("All checks passed".to_string(), theme::current().success())
    } else if let Some(item) = first_check_issue(check) {
        (
            format!(
                "{}: {}",
                item.label,
                item.detail.as_deref().unwrap_or("needs attention")
            ),
            theme::current().error(),
        )
    } else {
        (
            "Checks reported an unspecified issue".to_string(),
            theme::current().error(),
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
            theme::current().error(),
        ));
    }
    app.state.as_ref().and_then(|state| {
        state
            .latest_error
            .as_ref()
            .map(|error| (error.clone(), theme::current().error()))
            .or_else(|| {
                state
                    .latest_warning
                    .as_ref()
                    .map(|warning| (warning.clone(), theme::current().warning()))
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
    Line::from(Span::styled(title, theme::current().heading()))
}

/// Create a "label: value" field line with owned strings.
fn field_line(label: &'static str, value: impl Into<String>) -> Line<'static> {
    let val: String = value.into();
    Line::from(vec![
        Span::styled(label, theme::current().label()),
        Span::raw(": "),
        Span::raw(val),
    ])
}

/// Create a dim informational line with an owned string.
fn dim_line(text: impl Into<String>) -> Line<'static> {
    Line::from(Span::styled(text.into(), theme::current().muted()))
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
        theme::current().heading(),
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
    for (row, i) in visible_range.clone().enumerate() {
        app.register_click(
            Rect::new(
                list_area.x,
                list_area.y.saturating_add(2 + row as u16),
                list_area.width,
                1,
            ),
            ClickAction::History(i),
        );
        let record = &history[i];
        let entry = HistoryScreen::format_entry(record);
        let marker = if i == app.history_screen.selected {
            "▶ "
        } else {
            "  "
        };

        let outcome_color = if entry.is_error {
            theme::current().palette().error
        } else if entry.is_warning {
            theme::current().palette().warning
        } else {
            theme::current().palette().success
        };

        let style = if i == app.history_screen.selected {
            theme::current().selected()
        } else {
            Style::default()
        };

        list_lines.push(Line::from(vec![
            Span::styled(marker, theme::current().focused()),
            Span::styled(entry.time.clone(), style),
            Span::raw(" "),
            Span::styled(
                entry
                    .namespace
                    .clone()
                    .unwrap_or_else(|| "unknown namespace".to_string()),
                theme::current().label(),
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
            theme::current().heading(),
        )));
        detail_lines.push(Line::from(""));

        let outcome_color = if entry.is_error {
            theme::current().palette().error
        } else if entry.is_warning {
            theme::current().palette().warning
        } else {
            theme::current().palette().success
        };

        detail_lines.push(Line::from(vec![
            Span::styled(" Outcome: ", theme::current().muted()),
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
                theme::current().muted(),
            )));
            // Wrap long messages without splitting UTF-8 characters.
            let max_width = detail_area.width.saturating_sub(3) as usize;
            for message_line in text::wrap(msg, max_width) {
                detail_lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        message_line,
                        if entry.is_error {
                            theme::current().error()
                        } else {
                            theme::current().warning()
                        },
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
        theme::current().heading(),
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

    let backend = app
        .config
        .as_ref()
        .map(crate::automation::selected_backend)
        .unwrap_or_default();
    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Backup Automation",
            theme::current().heading(),
        )),
        Line::from(""),
        field_line("  Backend", backend.description()),
    ];

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
                theme::current().warning(),
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
                theme::current().error(),
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
                Span::styled("Install and enable automation?", theme::current().warning()),
            ]));
        }
        ConfirmAction::Remove => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Remove managed automation?", theme::current().warning()),
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
            theme::current().warning(),
        ))),
        LoadState::Stale { .. } => {
            lines.push(dim_line("  Preview is stale. Press r to refresh."));
        }
        LoadState::Failed { error, .. } => lines.push(Line::from(Span::styled(
            format!("  Preview failed: {error}. Press r to retry."),
            theme::current().error(),
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
            theme::current().heading(),
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
            let entry_style = match entry.kind {
                EntryKind::Addition => theme::current().success(),
                EntryKind::Modification | EntryKind::Warning => theme::current().warning(),
                EntryKind::Deletion => theme::current().error(),
                EntryKind::Exclusion => theme::current().muted(),
            };

            let mut spans = vec![
                Span::raw("  "),
                Span::styled(format!("{} ", entry.kind.prefix()), entry_style),
                Span::styled(entry.path.clone(), entry_style),
            ];

            if let Some(ref detail) = entry.detail {
                spans.push(Span::styled(
                    format!("  ({})", detail),
                    theme::current().muted(),
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
            theme::current().focused()
        } else {
            theme::current().label()
        },
    )];
    let source_row = if app.ignore_screen.mode == Mode::Preview {
        inner.y
    } else {
        inner.y.saturating_add(1)
    };
    let mut source_x = inner.x.saturating_add(if source_focused { 27 } else { 19 });
    for (i, source) in sources.iter().enumerate() {
        let source_width =
            u16::try_from(unicode_width::UnicodeWidthStr::width(source.path.as_str()) + 3)
                .unwrap_or(u16::MAX)
                .min(inner.right().saturating_sub(source_x));
        app.register_click(
            Rect::new(source_x, source_row, source_width, 1),
            ClickAction::IgnoreSource(i),
        );
        source_x = source_x.saturating_add(source_width);
        let style = if i == app.ignore_screen.source_idx {
            theme::current().selected()
        } else {
            theme::current().muted()
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
            theme::current().heading(),
        )));

        use crate::tui::task::LoadState;
        match &app.ignore_screen.preview_state {
            LoadState::Loading { .. } => lines.push(Line::from(Span::styled(
                "  Generating ignore preview...",
                theme::current().warning(),
            ))),
            LoadState::Stale { .. } => {
                lines.push(dim_line("  Preview is stale. Press r to refresh."))
            }
            LoadState::Failed { error, .. } => lines.push(Line::from(Span::styled(
                format!("  Preview failed: {error}. Press r to retry."),
                theme::current().error(),
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
                    spans.push(Span::styled("[ignored] ", theme::current().error()));
                    spans.push(Span::styled(entry.path.clone(), theme::current().muted()));
                    if let Some(ref pat) = entry.matched_by {
                        spans.push(Span::styled(format!("  ({pat})"), theme::current().muted()));
                    }
                } else {
                    spans.push(Span::styled("✓ ", theme::current().success()));
                    spans.push(Span::raw(entry.path.clone()));
                }

                if entry.secret_warning {
                    spans.push(Span::styled("  ⚠ secret", theme::current().warning()));
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
                theme::current().focused()
            } else {
                theme::current().label()
            },
        )));
        for (i, pattern) in current_source.ignore.iter().enumerate() {
            app.register_click(
                Rect::new(
                    inner.x,
                    inner.y.saturating_add(lines.len() as u16),
                    inner.width,
                    1,
                ),
                ClickAction::IgnorePattern(i),
            );
            let marker = if i == app.ignore_screen.pattern_idx {
                "▶ "
            } else {
                "  "
            };
            let style = if i == app.ignore_screen.pattern_idx {
                theme::current().selected()
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(marker, theme::current().focused()),
                Span::styled(pattern.clone(), style),
            ]));
        }
    }

    // Input area in add mode.
    if app.ignore_screen.mode == Mode::AddInput {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  New pattern (gitignore syntax):",
            theme::current().label(),
        )));
        let input_display = format!("  > {}", app.ignore_screen.input);
        lines.push(Line::from(Span::raw(input_display)));
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn register_picker_pointer(app: &mut App, area: Rect, toggles: bool) {
    let browser = if toggles {
        app.sources_screen.browser.as_mut()
    } else {
        app.repo_screen.browser.as_mut()
    };
    let Some(browser) = browser else { return };
    let (current, preview, rows) = crate::tui::picker::pointer_areas(area, browser);
    app.register_scroll(current, ScrollAction::PickerEntries);
    if let Some(preview) = preview {
        app.register_scroll(preview, ScrollAction::PickerPreview);
    }
    for (index, row) in rows {
        if toggles {
            let toggle_width = row.width.min(4);
            app.register_click(
                Rect::new(row.x, row.y, toggle_width, 1),
                ClickAction::PickerToggle(index),
            );
            app.register_click(
                Rect::new(
                    row.x.saturating_add(toggle_width),
                    row.y,
                    row.width.saturating_sub(toggle_width),
                    1,
                ),
                ClickAction::PickerEntry(index),
            );
        } else {
            app.register_click(row, ClickAction::PickerEntry(index));
        }
    }
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
            register_picker_pointer(app, chunks[0], true);
        } else {
            let msg = Paragraph::new(Line::from(Span::styled(
                " Browser is not ready. Press Esc, then a to try again.",
                theme::current().muted(),
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
        let paragraph =
            Paragraph::new(Line::from(Span::styled(summary, theme::current().accent())));
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
            theme::current().disabled(),
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
            theme::current().heading(),
        )));
        lines.push(Line::from(""));

        for (i, src) in sources.iter().enumerate() {
            app.register_click(
                Rect::new(
                    inner.x,
                    inner.y.saturating_add(lines.len() as u16),
                    inner.width,
                    1,
                ),
                ClickAction::Source(i),
            );
            let marker = if i == app.sources_screen.selected {
                "▶ "
            } else {
                "  "
            };
            let style = if i == app.sources_screen.selected {
                theme::current().selected()
            } else {
                Style::default()
            };
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(marker, theme::current().focused()),
                Span::styled(src.path.clone(), style),
                if !src.ignore.is_empty() {
                    Span::styled(
                        format!("  ({} ignore rules)", src.ignore.len()),
                        theme::current().muted(),
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
            theme::current().label(),
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
                theme::current().warning(),
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
                theme::current().warning(),
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
                    theme::current().warning(),
                ),
            ]));

            append_source_diff_lines(&mut lines, diff, inner.width);
        } else {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Apply changes?", theme::current().warning()),
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
            theme::current().success(),
        )));
        for path in &diff.additions {
            lines.push(Line::from(Span::styled(
                format!("    + {}", text::truncate(path, path_width)),
                theme::current().success(),
            )));
        }
    }
    if !diff.removals.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Sources to remove:",
            theme::current().error(),
        )));
        for path in &diff.removals {
            lines.push(Line::from(Span::styled(
                format!("    - {}", text::truncate(path, path_width)),
                theme::current().error(),
            )));
        }
    }
    if !diff.ignore_rules.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Generated ignore rules:",
            theme::current().warning(),
        )));
        let mut sources: Vec<_> = diff.ignore_rules.iter().collect();
        sources.sort_by(|left, right| left.0.cmp(right.0));
        for (source, rules) in sources {
            for rule in rules {
                lines.push(Line::from(Span::styled(
                    format!("    {source}: {}", text::truncate(rule, path_width)),
                    theme::current().warning(),
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

            // A configured repository remains browsable, but its root boundary
            // prevents exposing or navigating to the parent directory.
            if let Some(ref mut browser) = app.repo_screen.browser {
                crate::tui::picker::draw_with_presentation(
                    frame,
                    chunks[0],
                    browser,
                    None,
                    crate::tui::picker::Presentation::REPOSITORY,
                );
                register_picker_pointer(app, chunks[0], false);
            } else {
                let msg = Paragraph::new(Line::from(Span::styled(
                    " Press Enter or ↓ to start browsing",
                    theme::current().muted(),
                )));
                frame.render_widget(msg, chunks[0]);
            }

            // Status/validation area.
            let mut lines: Vec<Line> = Vec::new();
            if let Some(ref config) = app.config {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("Namespace: ", theme::current().label()),
                    Span::styled(config.namespace.clone(), theme::current().emphasis()),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled("Namespace: ", theme::current().label()),
                    Span::styled(
                        app.repo_screen.namespace_input.clone(),
                        theme::current().emphasis(),
                    ),
                ]));
            }

            if let Some(ref err) = app.repo_screen.selection_error {
                lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(format!("✗ {err}"), theme::current().error()),
                ]));
            }

            match &app.repo_screen.validation {
                LoadState::Loaded(info) => {
                    lines.push(Line::from(vec![
                        Span::raw(" "),
                        Span::styled("✓ Valid repository", theme::current().success()),
                        Span::raw(" — "),
                        Span::styled(&info.branch, theme::current().accent()),
                    ]));
                    draw_ownership_line(&info.ownership, &mut lines);
                }
                LoadState::Loading { .. } => lines.push(Line::from(Span::styled(
                    " Checking repository...",
                    theme::current().warning(),
                ))),
                LoadState::Failed { error, .. } => lines.push(Line::from(vec![
                    Span::raw(" "),
                    Span::styled(
                        format!("✗ {error}. Select a directory to retry."),
                        theme::current().error(),
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
                    "  No namespaces found. Press n to create the first one.",
                ));
            }
            for (index, item) in app.repo_screen.namespaces.iter().enumerate() {
                app.register_click(
                    Rect::new(
                        inner.x,
                        inner.y.saturating_add(lines.len() as u16),
                        inner.width,
                        1,
                    ),
                    ClickAction::Namespace(index),
                );
                let selected = index == app.repo_screen.namespace_selected;
                let marker = if selected { "▶" } else { " " };
                let active = if item.active {
                    " active"
                } else if app.config.is_some() {
                    " sibling"
                } else {
                    " available"
                };
                let state_style = match item.ownership {
                    crate::tui::screens::repository::OwnershipInfo::New => {
                        theme::current().warning()
                    }
                    crate::tui::screens::repository::OwnershipInfo::Owned { .. } => {
                        theme::current().success()
                    }
                    crate::tui::screens::repository::OwnershipInfo::InvalidManifest(_) => {
                        theme::current().error()
                    }
                    crate::tui::screens::repository::OwnershipInfo::Ambiguous(_) => {
                        theme::current().warning()
                    }
                };
                let row_style = if selected {
                    theme::current().selected()
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
                theme::current().heading(),
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
                    theme::current().warning(),
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
                theme::current().heading(),
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
                theme::current().muted(),
            )));
            lines.push(Line::from(""));

            // Validation result.
            match &app.repo_screen.validation {
                LoadState::Loaded(info) => {
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled("✓ Valid repository", theme::current().success()),
                    ]));
                    lines.push(field_line("    Branch", info.branch.clone()));
                    lines.push(field_line("    Path", info.path.display().to_string()));
                    lines.push(Line::from(""));
                    draw_ownership_lines(&info.ownership, &mut lines);
                }
                LoadState::Loading { .. } => lines.push(Line::from(Span::styled(
                    "  Checking repository...",
                    theme::current().warning(),
                ))),
                LoadState::Failed { error, .. } => lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(
                        format!("✗ {error}. Press Enter to retry."),
                        theme::current().error(),
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
                Span::styled("New namespace", theme::current().success()),
            ]));
        }
        OwnershipInfo::Owned { sources } => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("Existing manifest ({} sources)", sources.len()),
                    theme::current().warning(),
                ),
            ]));
        }
        OwnershipInfo::InvalidManifest(reason) => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(
                    format!("✗ Invalid manifest: {reason}"),
                    theme::current().error(),
                ),
            ]));
        }
        OwnershipInfo::Ambiguous(reason) => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled(format!("✗ Ambiguous: {reason}"), theme::current().error()),
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
                    theme::current().success(),
                ),
            ]));
        }
        OwnershipInfo::Owned { sources } => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("Existing manifest found.", theme::current().warning()),
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
                    theme::current().error(),
                ),
            ]));
        }
        OwnershipInfo::Ambiguous(reason) => {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("✗ Ambiguous: {reason}"), theme::current().error()),
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
                Span::styled("Initialize this repository?", theme::current().warning()),
            ]));
        }
        ConfirmState::AskAttach => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("Attach to this repository?", theme::current().warning()),
            ]));
        }
        ConfirmState::Done => {
            lines.push(Line::from(vec![
                Span::raw(" "),
                Span::styled("✓ Repository configured.", theme::current().success()),
            ]));
        }
        ConfirmState::None => {}
    }
}

#[cfg(test)]
#[path = "../../tests/unit/tui/ui.rs"]
mod tests;
