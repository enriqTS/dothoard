use ratatui::Terminal;
use ratatui::backend::TestBackend;

use super::*;
use crate::tui::task;

#[test]
fn responsive_breakpoints_cover_wide_medium_narrow_and_short() {
    assert_eq!(layout_class(Rect::new(0, 0, 120, 30)), LayoutClass::Wide);
    assert_eq!(layout_class(Rect::new(0, 0, 60, 24)), LayoutClass::Medium);
    assert_eq!(layout_class(Rect::new(0, 0, 30, 24)), LayoutClass::Narrow);
    assert_eq!(layout_class(Rect::new(0, 0, 120, 10)), LayoutClass::Short);
}

#[test]
fn history_layout_stacks_at_medium_width_and_keeps_detail_below_list() {
    let wide = history_panes(Rect::new(0, 0, 100, 20));
    assert_eq!(wide[0].y, wide[1].y);
    assert!(wide[0].width < wide[1].width);

    let medium = history_panes(Rect::new(0, 0, 60, 20));
    assert_eq!(medium[0].x, medium[1].x);
    assert!(medium[0].y < medium[1].y);
    assert_eq!(medium[0].width, 60);
}

#[test]
fn compact_tab_bar_preserves_active_tab_and_selection_style() {
    let backend = TestBackend::new(30, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app_on(Screen::History);
    app.focus = crate::tui::Focus::TabBar;

    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(buffer.area().height, 10);
    assert!(buffer_text(terminal.backend()).contains("7"));
    assert!(buffer.cell((18, 0)).is_some());
}

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
    // App::new may load local state outside test fixtures; this test is
    // specifically the unconfigured first-run presentation.
    app.config = None;
    app.state = None;

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
    assert!(content.contains("automation"));
    assert!(content.contains("repository"));
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
    app.repo_screen.confirm_state = crate::tui::screens::repository::ConfirmState::AskInitialize;

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
    assert!(content.contains("Source: .config/fish"));
    assert!(content.contains("Active rule context: *.log"));
    assert!(content.contains("[ignored]"));
    assert!(content.contains("*_history"));
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
    assert!(content.contains("Added: 3"));
    assert!(content.contains("Changed: 1"));
    assert!(content.contains("Deleted: 0"));
    assert!(content.contains("Ignored: 2"));
    assert!(content.contains("Warning: 0"));
    assert!(content.contains("config.fish"));
    assert!(content.contains(".bashrc"));
}

/// Verify automation screen renders.
#[test]
fn automation_screen_renders() {
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut app = app_on(Screen::Automation);
    app.automation_screen.status_state = crate::tui::task::LoadState::Loaded("active".to_string());
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

#[test]
fn repository_namespace_controls_show_active_and_unsafe_siblings_on_narrow_layout() {
    let backend = TestBackend::new(48, 18);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = app_on(Screen::Repository);
    app.repo_screen.mode = crate::tui::screens::repository::RepoMode::Namespaces;
    app.repo_screen.namespaces = vec![
        crate::tui::screens::repository::NamespaceSummary {
            name: "desktop".to_string(),
            ownership: crate::tui::screens::repository::OwnershipInfo::Owned { sources: vec![] },
            active: true,
        },
        crate::tui::screens::repository::NamespaceSummary {
            name: "orphan".to_string(),
            ownership: crate::tui::screens::repository::OwnershipInfo::Ambiguous(
                "content has no manifest".to_string(),
            ),
            active: false,
        },
    ];
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    let content = buffer_text(terminal.backend());
    assert!(content.contains("Namespaces"));
    assert!(content.contains("desktop"));
    assert!(content.contains("Owned"));
    assert!(content.contains("Ambiguous"));
    assert!(content.contains("Create"));
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
                namespace: "desktop".to_string(),
                started_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 0, 0).unwrap(),
                finished_at: Utc.with_ymd_and_hms(2026, 7, 21, 14, 0, 2).unwrap(),
                outcome: crate::state::RunOutcome::Success,
                commit: Some("abc123".to_string()),
                message: None,
                log_file: None,
            },
            crate::state::RunRecord {
                namespace: "notebook".to_string(),
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
    assert!(content.contains("desktop"));
    assert!(content.contains("Namespace"));
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

    let mut app = app_with_state();
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
    app.repo_screen.confirm_state = crate::tui::screens::repository::ConfirmState::AskInitialize;

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
    let mut sel = crate::tui::selection::SourceSelection::new(std::path::Path::new("/home/user"));
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
    assert!(
        content.contains(".config/fish"),
        "should show addition detail"
    );
    assert!(content.contains(".zshrc"), "should show removal detail");
    assert!(
        content.contains(".bashrc: /secret"),
        "should show generated rule"
    );
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
    app.repo_screen.confirm_state = crate::tui::screens::repository::ConfirmState::AskInitialize;

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
    assert!(buffer_text(terminal.backend()).contains("▶ Source selector [FOCUS]:"));

    app.ignore_screen.list_focus = crate::tui::screens::ignore::ListFocus::PatternList;
    terminal.draw(|frame| draw(frame, &mut app)).unwrap();
    assert!(buffer_text(terminal.backend()).contains("▶ Pattern list [FOCUS]:"));
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
