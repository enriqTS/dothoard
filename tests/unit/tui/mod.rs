use super::*;

/// Helper to create a minimal App for testing navigation and keys.
fn test_app() -> App {
    App {
        focus: Focus::TabBar,
        active_screen: Screen::Dashboard,
        should_quit: false,
        tasks: task::TaskManager::new_controlled(),
        last_backup: None,
        last_check: None,
        paths: None,
        state: None,
        config: None,
        status_message: None,
        dashboard_screen: screens::dashboard::DashboardScreen::default(),
        repo_screen: screens::repository::RepoScreen::new(),
        sources_screen: screens::sources::SourcesScreen::new(),
        ignore_screen: screens::ignore::IgnoreScreen::new(),
        preview_screen: screens::preview::PreviewScreen::new(),
        automation_screen: screens::automation::AutomationScreen::new(),
        history_screen: screens::history::HistoryScreen::new(),
        setup: None,
        theme_picker: None,
        last_state_refresh: std::time::Instant::now(),
        pointer_map: std::cell::RefCell::new(pointer::PointerMap::default()),
    }
}

fn mouse(
    kind: crossterm::event::MouseEventKind,
    column: u16,
    row: u16,
) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: crossterm::event::KeyModifiers::NONE,
    }
}

#[test]
fn pointer_click_selects_tabs_and_focuses_the_tab_bar() {
    let mut app = test_app();
    app.focus = Focus::Content;
    app.register_click(
        ratatui::layout::Rect::new(10, 0, 8, 1),
        pointer::ClickAction::Tab(Screen::Sources),
    );

    app.handle_mouse(mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        12,
        0,
    ));

    assert_eq!(app.active_screen, Screen::Sources);
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn pointer_click_enters_content_with_keyboard_equivalent_initialization() {
    let (mut app, _temp) = configured_test_app();
    app.active_screen = Screen::Repository;
    app.focus = Focus::TabBar;
    app.repo_screen.browser = None;
    app.register_click(
        ratatui::layout::Rect::new(0, 1, 80, 20),
        pointer::ClickAction::FocusContent,
    );

    app.handle_mouse(mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        5,
        5,
    ));

    assert_eq!(app.focus, Focus::Content);
    assert!(app.repo_screen.browser.is_some());
}

#[test]
fn pointer_click_invokes_clickable_shortcut_action() {
    let mut app = test_app();
    app.active_screen = Screen::Sources;
    app.focus = Focus::Content;
    app.config = Some(crate::config::Config::new(
        "/repo".to_string(),
        "desktop".to_string(),
    ));
    app.register_click(
        ratatui::layout::Rect::new(0, 23, 3, 1),
        pointer::ClickAction::Key(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        ),
    );

    app.handle_mouse(mouse(
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        1,
        23,
    ));

    assert_eq!(app.sources_screen.mode, screens::sources::Mode::Browse);
}

#[test]
fn pointer_wheel_scrolls_the_control_under_the_pointer() {
    let mut app = test_app();
    app.active_screen = Screen::Sources;
    app.focus = Focus::TabBar;
    let mut config = crate::config::Config::new("/repo".to_string(), "desktop".to_string());
    config.sources = vec![
        crate::config::SourceConfig {
            path: "one".to_string(),
            ignore: Vec::new(),
        },
        crate::config::SourceConfig {
            path: "two".to_string(),
            ignore: Vec::new(),
        },
    ];
    app.config = Some(config);
    app.register_scroll(
        ratatui::layout::Rect::new(0, 1, 80, 20),
        pointer::ScrollAction::Vertical,
    );

    app.handle_mouse(mouse(crossterm::event::MouseEventKind::ScrollDown, 5, 5));

    assert_eq!(app.focus, Focus::Content);
    assert_eq!(app.sources_screen.selected, 1);
}

fn configured_test_app() -> (App, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path();
    let config_dir = home.join(".config/dothoard");
    let state_dir = home.join(".local/state/dothoard");
    let runtime_dir = home.join(".run");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let mut app = test_app();
    app.paths = Some(
        crate::paths::AppPaths::resolve(crate::paths::PathInputs {
            home: Some(home.to_path_buf()),
            config_dir: Some(config_dir),
            state_dir: Some(state_dir),
            runtime_dir: Some(runtime_dir),
            use_environment: false,
        })
        .unwrap(),
    );
    app.config = Some(crate::config::Config::new(
        home.join("repo").display().to_string(),
        "test-machine",
    ));
    (app, temp)
}

fn preview_data(path: &str) -> screens::preview::PreviewData {
    screens::preview::PreviewData {
        additions: 1,
        modifications: 0,
        deletions: 0,
        exclusions: 0,
        warnings: 0,
        entries: vec![screens::preview::PreviewEntry {
            kind: screens::preview::EntryKind::Addition,
            path: path.to_string(),
            detail: None,
        }],
    }
}

#[test]
fn app_tick_expires_transient_status_but_not_running_progress() {
    let mut app = test_app();
    app.success("saved");
    for _ in 0..16 {
        app.tick();
    }
    assert!(app.status_message.is_none());

    app.running("working");
    for _ in 0..100 {
        app.tick();
    }
    assert!(app.status_message.is_some());
}

#[test]
fn periodic_poll_refreshes_history_written_by_an_external_run() {
    let (mut app, _temp) = configured_test_app();
    app.state = Some(crate::state::AppState::new());
    let started_at = chrono::Utc::now();
    let mut external_state = crate::state::AppState::new();
    external_state.record_run(crate::state::RunRecord {
        namespace: "test-machine".to_string(),
        started_at,
        finished_at: started_at + chrono::Duration::seconds(1),
        outcome: crate::state::RunOutcome::NoChanges,
        commit: None,
        message: None,
        log_file: Some("external.log".to_string()),
    });
    external_state
        .save(app.paths.as_ref().unwrap().state_dir())
        .unwrap();

    app.poll_external_state();
    assert!(app.state.as_ref().unwrap().history.is_empty());

    app.last_state_refresh = std::time::Instant::now() - std::time::Duration::from_secs(1);
    app.poll_external_state();
    assert_eq!(app.state.as_ref().unwrap().history.len(), 1);
    assert_eq!(
        app.state.as_ref().unwrap().history[0].started_at,
        started_at
    );
}

#[test]
fn periodic_refresh_keeps_usable_state_after_a_transient_read_failure() {
    let (mut app, _temp) = configured_test_app();
    let started_at = chrono::Utc::now();
    let mut current = crate::state::AppState::new();
    current.record_run(crate::state::RunRecord {
        namespace: "test-machine".to_string(),
        started_at,
        finished_at: started_at,
        outcome: crate::state::RunOutcome::NoChanges,
        commit: None,
        message: None,
        log_file: None,
    });
    app.state = Some(current.clone());
    std::fs::write(
        crate::state::AppState::path_in(app.paths.as_ref().unwrap().state_dir()),
        "not valid JSON",
    )
    .unwrap();

    app.last_state_refresh = std::time::Instant::now() - std::time::Duration::from_secs(1);
    app.poll_external_state();

    assert_eq!(app.state, Some(current));
}

#[test]
fn automatic_history_refresh_preserves_an_older_selected_run() {
    let (mut app, _temp) = configured_test_app();
    let now = chrono::Utc::now();
    let record = |offset| crate::state::RunRecord {
        namespace: "test-machine".to_string(),
        started_at: now + chrono::Duration::seconds(offset),
        finished_at: now + chrono::Duration::seconds(offset + 1),
        outcome: crate::state::RunOutcome::Success,
        commit: Some(format!("commit-{offset}")),
        message: None,
        log_file: None,
    };
    let mut initial = crate::state::AppState::new();
    initial.record_run(record(1));
    initial.record_run(record(2));
    app.state = Some(initial.clone());
    app.history_screen.selected = 1;

    let mut external = initial;
    external.record_run(record(3));
    external
        .save(app.paths.as_ref().unwrap().state_dir())
        .unwrap();
    app.last_state_refresh = std::time::Instant::now() - std::time::Duration::from_secs(1);
    app.poll_external_state();

    assert_eq!(app.history_screen.selected, 2);
    assert_eq!(
        app.state.as_ref().unwrap().history[2].commit.as_deref(),
        Some("commit-1")
    );
}

#[test]
fn ignore_validation_failure_promotes_error_status() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Ignore;
    app.ignore_screen.mode = screens::ignore::Mode::AddInput;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let status = app.status_message.expect("validation status");
    assert_eq!(status.kind, status::StatusKind::Error);
    assert!(status.text.contains("cannot be empty"));
    assert!(app.ignore_screen.message.is_none());
}

#[test]
fn screen_next_wraps_around() {
    assert_eq!(Screen::Dashboard.next(), Screen::Repository);
    assert_eq!(Screen::History.next(), Screen::Dashboard);
}

#[test]
fn screen_prev_wraps_around() {
    assert_eq!(Screen::Dashboard.prev(), Screen::History);
    assert_eq!(Screen::Repository.prev(), Screen::Dashboard);
}

#[test]
fn all_screens_have_labels() {
    for screen in Screen::ALL {
        assert!(!screen.label().is_empty());
    }
}

#[test]
fn app_starts_on_dashboard() {
    let app = test_app();
    assert_eq!(app.active_screen, Screen::Dashboard);
    assert_eq!(app.focus, Focus::TabBar);
    assert!(!app.should_quit);
}

#[test]
fn quit_on_q() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(app.should_quit);
}

#[test]
fn quit_on_ctrl_c() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn quit_on_esc() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.should_quit);
}

#[test]
fn tab_bar_right_navigates_forward() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Repository);
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn tab_bar_left_navigates_backward() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::History);
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn tab_bar_tab_key_navigates_forward() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Repository);
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn shift_tab_navigates_backward() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.active_screen, Screen::History);
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn enter_content_from_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
    assert_eq!(app.active_screen, Screen::Dashboard);
}

#[test]
fn down_enters_content_from_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
}

#[test]
fn j_enters_content_from_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
}

#[test]
fn tab_from_content_returns_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
    // Active screen is not changed.
    assert_eq!(app.active_screen, Screen::Dashboard);
}

#[test]
fn shift_tab_from_content_returns_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.focus, Focus::TabBar);
    assert_eq!(app.active_screen, Screen::Dashboard);
}

#[test]
fn up_at_boundary_returns_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    // Dashboard has no items, so Up is not consumed -> returns to tab bar.
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn number_keys_select_screens() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();

    app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Sources);

    app.handle_key(KeyEvent::new(KeyCode::Char('7'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::History);

    app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Dashboard);
}

#[test]
fn focus_preserved_when_switching_tabs() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    // Enter content on Dashboard.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
    // Tab returns to tab bar.
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
    // Navigate to next tab.
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Repository);
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn backup_key_sets_status_when_no_paths() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    // Enter content focus first (b is a content-level key on Dashboard).
    app.focus = Focus::Content;
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert!(
        app.status_message
            .as_ref()
            .unwrap()
            .contains("not resolved")
    );
}

#[test]
fn check_key_sets_status_when_no_paths() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
    assert!(
        app.status_message
            .as_ref()
            .unwrap()
            .contains("not resolved")
    );
}

#[test]
fn poll_tasks_updates_last_backup() {
    let mut app = test_app();
    app.tasks.active = Some(task::TaskKind::Backup);

    // Send a result directly on the channel.
    app.tasks
        .sender
        .send(task::TaskResult::Backup(task::BackupResult {
            success: true,
            commit: Some("deadbeef".to_string()),
            pushed: true,
            copies: 3,
            deletions: 0,
            warnings: Vec::new(),
            error: None,
        }))
        .unwrap();

    app.poll_tasks();

    assert!(app.last_backup.is_some());
    let result = app.last_backup.as_ref().unwrap();
    assert!(result.success);
    assert_eq!(result.commit.as_deref(), Some("deadbeef"));
    let status = app.status_message.as_ref().unwrap();
    assert!(status.contains("success"));
    assert_eq!(status.kind, status::StatusKind::Success);
}

#[test]
fn poll_tasks_updates_last_check() {
    let mut app = test_app();
    app.tasks.active = Some(task::TaskKind::Check);

    app.tasks
        .sender
        .send(task::TaskResult::Check(task::CheckResult {
            healthy: false,
            results: vec![task::CheckItem {
                label: "config".to_string(),
                status: task::CheckItemStatus::Error,
                detail: Some("missing".to_string()),
            }],
        }))
        .unwrap();

    app.poll_tasks();

    assert!(app.last_check.is_some());
    let result = app.last_check.as_ref().unwrap();
    assert!(!result.healthy);
    assert!(app.status_message.as_ref().unwrap().contains("issues"));
}

// --- Focus model interaction tests ---

#[test]
fn h_l_are_tab_bar_aliases_for_left_right() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();

    app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Repository);
    assert_eq!(app.focus, Focus::TabBar);

    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Dashboard);
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn ctrl_c_exits_from_content_focus() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn ctrl_c_exits_from_tab_bar_focus() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.should_quit);
}

#[test]
fn number_keys_do_not_switch_tabs_in_content_focus() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Dashboard;
    // '3' in content focus should not switch to Sources.
    app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Dashboard);
}

#[test]
fn left_right_do_not_switch_tabs_in_content_focus() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Dashboard;
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    // Right in content is delegated to the screen, not consumed -> doesn't switch tab.
    assert_eq!(app.active_screen, Screen::Dashboard);
}

#[test]
fn k_at_boundary_returns_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Dashboard;
    // 'k' is vim alias for Up, also returns to tab bar at boundary.
    app.handle_key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn q_from_content_quits() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Dashboard;
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(app.should_quit);
}

#[test]
fn esc_from_top_level_content_returns_to_tab_bar_without_quitting() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Dashboard;
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
    assert!(!app.should_quit);
}

#[test]
fn repository_browser_esc_returns_to_tab_bar_without_quitting() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Repository;

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert_eq!(app.focus, Focus::TabBar);
    assert_eq!(app.repo_screen.mode, screens::repository::RepoMode::Browser);
    assert!(!app.should_quit);
}

#[test]
fn ctrl_c_is_owned_by_text_input_and_confirmation_modes() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);

    let mut cases = Vec::new();

    let mut repository_text = test_app();
    repository_text.active_screen = Screen::Repository;
    repository_text.repo_screen.mode = screens::repository::RepoMode::TextInput;
    cases.push(repository_text);

    let mut repository_confirm = test_app();
    repository_confirm.active_screen = Screen::Repository;
    repository_confirm.repo_screen.confirm_state = screens::repository::ConfirmState::AskInitialize;
    cases.push(repository_confirm);

    let mut source_input = test_app();
    source_input.active_screen = Screen::Sources;
    source_input.sources_screen.mode = screens::sources::Mode::AddInput;
    cases.push(source_input);

    let mut source_pending = test_app();
    source_pending.active_screen = Screen::Sources;
    source_pending.sources_screen.mode = screens::sources::Mode::PendingChanges;
    cases.push(source_pending);

    let mut ignore_input = test_app();
    ignore_input.active_screen = Screen::Ignore;
    ignore_input.ignore_screen.mode = screens::ignore::Mode::AddInput;
    cases.push(ignore_input);

    let mut automation_confirm = test_app();
    automation_confirm.active_screen = Screen::Automation;
    automation_confirm.automation_screen.confirm = screens::automation::ConfirmAction::Install;
    cases.push(automation_confirm);

    for mut app in cases {
        app.focus = Focus::Content;
        app.handle_key(ctrl_c);
        assert!(!app.should_quit);
    }
}

#[test]
fn dashboard_detail_owns_input_and_closes_with_escape() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    app.focus = Focus::Content;
    app.last_check = Some(task::CheckResult {
        healthy: false,
        results: vec![task::CheckItem {
            label: "Repository".to_string(),
            status: task::CheckItemStatus::Error,
            detail: Some("a complete diagnostic with a long path /one/two/three".to_string()),
        }],
    });

    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    assert_eq!(
        app.dashboard_screen
            .detail
            .as_ref()
            .map(|detail| detail.value.as_str()),
        Some("a complete diagnostic with a long path /one/two/three")
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(!app.should_quit);
    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.dashboard_screen.detail.is_none());
}

#[test]
fn q_is_literal_in_text_input_consumed_by_modals_and_quits_elsewhere() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);

    let mut input = test_app();
    input.focus = Focus::Content;
    input.active_screen = Screen::Repository;
    input.repo_screen.mode = screens::repository::RepoMode::TextInput;
    input.handle_key(q);
    assert_eq!(input.repo_screen.input, "q");
    assert!(!input.should_quit);

    let mut modal = test_app();
    modal.focus = Focus::Content;
    modal.active_screen = Screen::Sources;
    modal.sources_screen.mode = screens::sources::Mode::PendingChanges;
    modal.handle_key(q);
    assert!(!modal.should_quit);
    assert_eq!(
        modal.sources_screen.mode,
        screens::sources::Mode::PendingChanges
    );

    let mut preview = test_app();
    preview.focus = Focus::Content;
    preview.active_screen = Screen::Ignore;
    preview.ignore_screen.mode = screens::ignore::Mode::Preview;
    preview.handle_key(q);
    assert!(preview.should_quit);
}

#[test]
fn tab_and_shift_tab_leave_pending_choice_without_resolving_it() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    for key in [
        KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
    ] {
        let mut app = test_app();
        app.focus = Focus::Content;
        app.active_screen = Screen::Sources;
        app.sources_screen.mode = screens::sources::Mode::PendingChanges;
        app.sources_screen.pending_diff = Some(crate::tui::selection::SelectionDiff {
            additions: vec![".bashrc".to_string()],
            removals: vec![],
            ignore_rules: std::collections::HashMap::new(),
        });

        app.handle_key(key);

        assert_eq!(app.focus, Focus::TabBar);
        assert_eq!(
            app.sources_screen.mode,
            screens::sources::Mode::PendingChanges
        );
        assert!(app.sources_screen.pending_diff.is_some());
    }
}

#[test]
fn history_up_stays_in_content_when_items_exist() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::History;
    // Give history some entries so Up is consumed at non-boundary position.
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
                started_at: chrono::Utc::now(),
                finished_at: chrono::Utc::now(),
                outcome: crate::state::RunOutcome::Success,
                commit: None,
                message: None,
                log_file: None,
            },
            crate::state::RunRecord {
                namespace: String::new(),
                started_at: chrono::Utc::now(),
                finished_at: chrono::Utc::now(),
                outcome: crate::state::RunOutcome::Success,
                commit: None,
                message: None,
                log_file: None,
            },
        ],
    });
    // Move to second item first.
    app.history_screen.selected = 1;
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    // Should stay in content (moved from 1 to 0).
    assert_eq!(app.focus, Focus::Content);
    assert_eq!(app.history_screen.selected, 0);
}

#[test]
fn history_up_at_top_returns_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::History;
    app.state = Some(crate::state::AppState {
        last_attempt: None,
        last_success: None,
        last_commit: None,
        last_push: None,
        pending_push: false,
        latest_warning: None,
        latest_error: None,
        history: vec![crate::state::RunRecord {
            namespace: String::new(),
            started_at: chrono::Utc::now(),
            finished_at: chrono::Utc::now(),
            outcome: crate::state::RunOutcome::Success,
            commit: None,
            message: None,
            log_file: None,
        }],
    });
    app.history_screen.selected = 0;
    // Up at the first item: screen reports NotConsumed, so the parent
    // content handler detects the boundary and returns to tab bar.
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn complete_focus_cycle() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();

    // 1. Start at tab bar, on Dashboard.
    assert_eq!(app.focus, Focus::TabBar);
    assert_eq!(app.active_screen, Screen::Dashboard);

    // 2. Navigate to Sources tab.
    app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Sources);
    assert_eq!(app.focus, Focus::TabBar);

    // 3. Enter content.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
    assert_eq!(app.active_screen, Screen::Sources);

    // 4. Tab returns to tab bar, screen unchanged.
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
    assert_eq!(app.active_screen, Screen::Sources);

    // 5. Shift+Tab selects previous tab.
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.active_screen, Screen::Repository);
    assert_eq!(app.focus, Focus::TabBar);

    // 6. Enter content again.
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
    assert_eq!(app.active_screen, Screen::Repository);

    // 7. Shift+Tab from content returns to tab bar.
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.focus, Focus::TabBar);
    assert_eq!(app.active_screen, Screen::Repository);
}

#[test]
fn tab_bar_wraps_with_all_navigation_methods() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();

    // Right wraps from History -> Dashboard.
    app.active_screen = Screen::History;
    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Dashboard);

    // Left wraps from Dashboard -> History.
    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::History);

    // 'l' wraps the same.
    app.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Dashboard);

    // 'h' wraps the same.
    app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::History);
}

#[test]
fn screen_state_preserved_across_focus_transitions() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".config/fish".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
        ],
    });

    // Navigate to Sources, enter content, move selection.
    app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.sources_screen.selected, 1);

    // Return to tab bar.
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);

    // Go to a different tab and come back.
    app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Dashboard);
    app.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
    assert_eq!(app.active_screen, Screen::Sources);

    // Re-enter content: selection is preserved.
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.sources_screen.selected, 1);
}

// --- UX02: Screen boundary and modal Tab pass-through tests ---

#[test]
fn sources_up_at_top_returns_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }],
    });
    app.sources_screen.selected = 0;
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn sources_down_stays_in_content() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".zshrc".to_string(),
                ignore: vec![],
            },
        ],
    });
    app.sources_screen.selected = 0;
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
    assert_eq!(app.sources_screen.selected, 1);
}

#[test]
fn preview_up_at_scroll_zero_returns_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Preview;
    app.preview_screen.scroll = 0;
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn preview_up_with_scroll_stays_in_content() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Preview;
    app.preview_screen.scroll = 3;
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
    assert_eq!(app.preview_screen.scroll, 2);
}

#[test]
fn repository_tab_escapes_text_input() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Repository;
    // Type something in the repo input.
    app.repo_screen.input = "~/some-repo".to_string();
    app.repo_screen.cursor = 11;
    // Tab from text input returns to tab bar.
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
    // Input state preserved.
    assert_eq!(app.repo_screen.input, "~/some-repo");
}

#[test]
fn repository_shift_tab_escapes_text_input() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Repository;
    app.repo_screen.input = "~/path".to_string();
    app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn sources_tab_escapes_add_input_mode() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;
    app.sources_screen.mode = screens::sources::Mode::AddInput;
    app.sources_screen.input = ".config/partial".to_string();
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
    // Input is preserved, mode unchanged (screen state not reset).
    assert_eq!(app.sources_screen.input, ".config/partial");
}

#[test]
fn sources_tab_escapes_confirm_delete() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;
    app.sources_screen.mode = screens::sources::Mode::ConfirmDelete;
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn ignore_tab_escapes_add_input_mode() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Ignore;
    app.ignore_screen.mode = screens::ignore::Mode::AddInput;
    app.ignore_screen.input = "*.log".to_string();
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn ignore_tab_escapes_preview_mode() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Ignore;
    app.ignore_screen.mode = screens::ignore::Mode::Preview;
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn ignore_nested_boundary_source_to_tab_bar() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Ignore;
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string()],
        }],
    });
    // Start at SourceSelector (default).
    app.ignore_screen.list_focus = screens::ignore::ListFocus::SourceSelector;
    // Up from SourceSelector returns to tab bar.
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
}

#[test]
fn ignore_nested_boundary_pattern_to_source() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Ignore;
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["*.log".to_string(), "*.tmp".to_string()],
        }],
    });
    app.ignore_screen.list_focus = screens::ignore::ListFocus::PatternList;
    app.ignore_screen.pattern_idx = 0;
    // Up at pattern_idx 0 moves to SourceSelector, stays in content.
    app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Content);
    assert_eq!(
        app.ignore_screen.list_focus,
        screens::ignore::ListFocus::SourceSelector
    );
}

#[test]
fn repository_tab_escapes_confirmation_dialog() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Repository;
    app.repo_screen.confirm_state = screens::repository::ConfirmState::AskInitialize;
    app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::TabBar);
    // Confirmation state is preserved.
    assert_eq!(
        app.repo_screen.confirm_state,
        screens::repository::ConfirmState::AskInitialize
    );
}

// --- UX09: Dependent state synchronization tests ---

#[test]
fn add_source_marks_preview_stale() {
    let mut app = test_app();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();

    // Setup: create a source directory and configure paths.
    std::fs::create_dir(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/share/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".config/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".run")).unwrap();
    app.paths = Some(
        crate::paths::AppPaths::resolve(crate::paths::PathInputs {
            home: Some(home.to_path_buf()),
            config_dir: Some(home.join(".config/dothoard")),
            state_dir: Some(home.join(".local/share/dothoard")),
            runtime_dir: Some(home.join(".run")),
            use_environment: false,
        })
        .unwrap(),
    );
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: Vec::new(),
    });
    // Add a source.
    app.handle_add_source(".config".to_string());

    assert!(matches!(
        app.preview_screen.load_state,
        task::LoadState::Stale { .. }
    ));
    assert!(matches!(
        app.ignore_screen.preview_state,
        task::LoadState::Stale { .. }
    ));
}

#[test]
fn remove_source_marks_preview_stale() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".zshrc".to_string(),
                ignore: vec![],
            },
        ],
    });
    app.handle_remove_source(0);

    assert!(matches!(
        app.preview_screen.load_state,
        task::LoadState::Stale { .. }
    ));
    assert!(matches!(
        app.ignore_screen.preview_state,
        task::LoadState::Stale { .. }
    ));
}

#[test]
fn remove_source_clamps_sources_selection() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".zshrc".to_string(),
                ignore: vec![],
            },
        ],
    });
    app.sources_screen.selected = 1; // pointing at last item

    app.handle_remove_source(1);

    // Selection should be clamped to 0 (only 1 item left).
    assert_eq!(app.sources_screen.selected, 0);
}

#[test]
fn remove_source_clamps_ignore_source_idx() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec!["*.log".to_string()],
            },
            crate::config::SourceConfig {
                path: ".zshrc".to_string(),
                ignore: vec![],
            },
        ],
    });
    app.ignore_screen.source_idx = 1;
    app.ignore_screen.pattern_idx = 0;

    app.handle_remove_source(1);

    // Ignore screen source index should be clamped.
    assert_eq!(app.ignore_screen.source_idx, 0);
}

#[test]
fn remove_all_sources_resets_ignore_indices() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec!["*.tmp".to_string()],
        }],
    });
    app.ignore_screen.source_idx = 0;
    app.ignore_screen.pattern_idx = 0;

    app.handle_remove_source(0);

    assert_eq!(app.ignore_screen.source_idx, 0);
    assert_eq!(app.ignore_screen.pattern_idx, 0);
}

#[test]
fn browser_state_preserved_across_tab_switches() {
    let mut app = test_app();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::create_dir(home.join("dir_a")).unwrap();
    std::fs::create_dir(home.join("dir_b")).unwrap();
    std::fs::write(home.join("file.txt"), "x").unwrap();

    app.repo_screen.ensure_browser(home);

    // Move down in the browser.
    if let Some(ref mut browser) = app.repo_screen.browser {
        browser.move_down();
        browser.move_down();
    }
    let saved_selected = app.repo_screen.browser.as_ref().unwrap().selected();
    assert!(saved_selected > 0);

    // Switch to another tab and back.
    app.focus = Focus::TabBar;
    app.active_screen = Screen::Dashboard;
    app.active_screen = Screen::Repository;
    app.focus = Focus::Content;

    // Browser selection should be preserved.
    assert_eq!(
        app.repo_screen.browser.as_ref().unwrap().selected(),
        saved_selected
    );
}

#[test]
fn source_add_failure_keeps_error_message() {
    let mut app = test_app();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    std::fs::create_dir(home.join(".config")).unwrap();
    std::fs::create_dir_all(home.join(".local/share/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".config/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".run")).unwrap();

    app.paths = Some(
        crate::paths::AppPaths::resolve(crate::paths::PathInputs {
            home: Some(home.to_path_buf()),
            config_dir: Some(home.join(".config/dothoard")),
            state_dir: Some(home.join(".local/share/dothoard")),
            runtime_dir: Some(home.join(".run")),
            use_environment: false,
        })
        .unwrap(),
    );
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".config".to_string(),
            ignore: vec![],
        }],
    });

    // Try to add a duplicate source (will fail validation).
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.handle_add_source(".config".to_string());

    // On failure, the message should indicate the error.
    assert!(app.sources_screen.message.is_some());
    assert_eq!(
        app.sources_screen.message.as_ref().unwrap().kind,
        screens::sources::MessageKind::Error,
    );
}

// --- TU04: slow work must not complete inline ---

#[test]
fn first_run_repository_validation_opens_namespace_selection() {
    let (mut app, temp) = configured_test_app();
    app.config = None;
    app.setup = Some(setup::SetupState::new());
    app.repo_screen.namespace_input.clear();
    let request_id = app
        .tasks
        .spawn_repository_validation(
            String::new(),
            temp.path().to_path_buf(),
            String::new(),
            "origin".to_string(),
            120,
        )
        .unwrap();
    app.repo_screen.validation.begin(request_id, false);
    app.tasks
        .sender
        .send(task::TaskResult::RepositoryValidation {
            request_id,
            result: Ok(screens::repository::RepoInfo {
                path: temp.path().join("repo"),
                branch: "main".to_string(),
                ownership: screens::repository::OwnershipInfo::New,
            }),
        })
        .unwrap();

    std::fs::create_dir_all(temp.path().join("repo")).unwrap();
    app.poll_tasks();

    assert_eq!(
        app.repo_screen.mode,
        screens::repository::RepoMode::Namespaces
    );
    assert_eq!(
        app.repo_screen.confirm_state,
        screens::repository::ConfirmState::None
    );
    assert_eq!(
        app.setup.as_ref().unwrap().step,
        setup::SetupStep::Namespace
    );
}

#[test]
fn first_run_clone_starts_in_background_and_surfaces_failure() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, temp) = configured_test_app();
    app.config = None;
    app.setup = Some(setup::SetupState::new());
    let setup = app.setup.as_mut().unwrap();
    setup.repository_mode = setup::RepositorySetupMode::Clone;
    setup.clone_url = "https://example.invalid/repository.git".to_string();
    setup.clone_url_cursor = setup.clone_url.len();
    setup.clone_destination = temp.path().join("clone").display().to_string();
    setup.clone_destination_cursor = setup.clone_destination.len();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let request_id = app
        .setup
        .as_ref()
        .unwrap()
        .clone_state
        .loading_id()
        .expect("background clone request");
    app.tasks
        .sender
        .send(task::TaskResult::RepositoryClone {
            request_id,
            result: Err("authentication failed".to_string()),
        })
        .unwrap();
    app.poll_tasks();

    assert_eq!(
        app.setup.as_ref().unwrap().clone_error(),
        Some("authentication failed")
    );
    assert!(app.config.is_none());
}

#[test]
fn first_run_namespace_selection_restores_sources_from_manifest() {
    let (mut app, temp) = configured_test_app();
    app.config = None;
    app.setup = Some(setup::SetupState::new());
    let repository = temp.path().join("repo");
    let namespace = repository.join("notebook");
    std::fs::create_dir_all(namespace.join("home")).unwrap();
    crate::backup::manifest::Manifest::from_sources(
        "notebook",
        &[crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec!["fish_variables".to_string()],
        }],
    )
    .save(&namespace)
    .unwrap();
    app.repo_screen.validation = task::LoadState::Loaded(screens::repository::RepoInfo {
        path: repository,
        branch: "main".to_string(),
        ownership: screens::repository::OwnershipInfo::New,
    });
    app.repo_screen.namespace_action = screens::repository::NamespaceAction::SelectOrCreate;
    app.repo_screen.namespace_input = "notebook".to_string();

    app.handle_namespace_action();

    let config = app.config.as_ref().expect("first-run configuration");
    assert_eq!(config.namespace, "notebook");
    assert_eq!(config.sources.len(), 1);
    assert_eq!(config.sources[0].path, ".config/fish");
    assert_eq!(config.sources[0].ignore, vec!["fish_variables"]);
    assert_eq!(
        app.setup.as_ref().unwrap().step,
        setup::SetupStep::Automation
    );
}

#[test]
fn first_run_automation_validates_persists_and_advances_to_theme() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, _temp) = configured_test_app();
    app.setup = Some(setup::SetupState::resume(
        app.config.as_ref().unwrap(),
        theme::ThemeId::System,
    ));
    let setup = app.setup.as_mut().unwrap();
    setup.automation_backend = crate::config::AutomationBackend::Cron;
    setup.interval_input = "90".to_string();
    setup.interval_cursor = 2;

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        app.setup.as_ref().unwrap().step,
        setup::SetupStep::Automation
    );
    assert!(
        app.setup
            .as_ref()
            .unwrap()
            .automation_error
            .as_deref()
            .is_some_and(|error| error.contains("1 through 59"))
    );

    let setup = app.setup.as_mut().unwrap();
    setup.interval_input = "15".to_string();
    setup.interval_cursor = 2;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(app.setup.as_ref().unwrap().step, setup::SetupStep::Theme);
    let saved = crate::config::Config::load(app.paths.as_ref().unwrap().config_file()).unwrap();
    assert_eq!(
        saved.automation_backend,
        crate::config::AutomationBackend::Cron
    );
    assert_eq!(saved.interval_minutes, 15);
}

#[test]
fn first_run_theme_navigation_live_previews_and_finish_opens_main_tabs() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, _temp) = configured_test_app();
    setup::mark_incomplete(app.paths.as_ref().unwrap().config_dir()).unwrap();
    let mut setup = setup::SetupState::resume(app.config.as_ref().unwrap(), theme::ThemeId::System);
    setup.step = setup::SetupStep::Theme;
    app.setup = Some(setup);
    theme::set_active(theme::ThemeId::System);

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_eq!(theme::active_id(), theme::ThemeId::CatppuccinMocha);
    assert_eq!(
        app.setup.as_ref().unwrap().theme_selected,
        theme::ThemeId::CatppuccinMocha
    );

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.setup.is_none());
    assert_eq!(app.active_screen, Screen::Dashboard);
    assert_eq!(app.focus, Focus::TabBar);
    assert!(!setup::is_incomplete(
        app.paths.as_ref().unwrap().config_dir()
    ));
    assert_eq!(
        theme::load_preference(app.paths.as_ref().unwrap().config_dir()),
        Some(theme::ThemeId::CatppuccinMocha)
    );
    theme::set_active(theme::ThemeId::default());
}

#[test]
fn repository_validation_does_not_run_inline() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let config_dir = home.join(".config/dothoard");
    let state_dir = home.join(".local/state/dothoard");
    let runtime_dir = home.join(".run");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::create_dir_all(&state_dir).unwrap();
    std::fs::create_dir_all(&runtime_dir).unwrap();

    let mut app = test_app();
    app.paths = Some(
        crate::paths::AppPaths::resolve(crate::paths::PathInputs {
            home: Some(home.to_path_buf()),
            config_dir: Some(config_dir),
            state_dir: Some(state_dir),
            runtime_dir: Some(runtime_dir),
            use_environment: false,
        })
        .unwrap(),
    );
    app.focus = Focus::Content;
    app.active_screen = Screen::Repository;
    app.repo_screen.mode = screens::repository::RepoMode::TextInput;
    app.repo_screen.input = home.join("missing").display().to_string();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(
        app.repo_screen.validation.is_loading(),
        "validation must be handed to a background worker"
    );
    let request_id = app.repo_screen.validation.loading_id().unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::RepositoryValidation {
            request_id,
            result: Err("not a repository".to_string()),
        })
        .unwrap();
    app.poll_tasks();
    assert_eq!(app.repo_screen.validation.error(), Some("not a repository"));
}

#[test]
fn screen_loads_complete_and_fail_from_controlled_results() {
    let (mut app, _temp) = configured_test_app();

    app.preview_screen.load_state = task::LoadState::Loaded(preview_data("old"));
    app.start_backup_preview();
    let preview_request = app.preview_screen.load_state.loading_id().unwrap();
    assert_eq!(
        app.preview_screen.load_state.data().unwrap().entries[0].path,
        "old"
    );
    app.tasks
        .sender
        .send(task::TaskResult::BackupPreview {
            request_id: preview_request,
            result: Err("planner failed".to_string()),
        })
        .unwrap();
    app.poll_tasks();
    assert_eq!(
        app.preview_screen.load_state.error(),
        Some("planner failed")
    );
    assert_eq!(
        app.preview_screen.load_state.data().unwrap().entries[0].path,
        "old"
    );

    app.config.as_mut().unwrap().sources = vec![crate::config::SourceConfig {
        path: ".config".to_string(),
        ignore: vec![],
    }];
    app.start_ignore_preview(0);
    let ignore_request = app.ignore_screen.preview_state.loading_id().unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::IgnorePreview {
            request_id: ignore_request,
            source_idx: 0,
            result: Ok(vec![screens::ignore::PreviewEntry {
                path: "file".to_string(),
                ignored: false,
                matched_by: None,
                secret_warning: false,
            }]),
        })
        .unwrap();
    app.poll_tasks();
    assert_eq!(app.ignore_screen.preview().unwrap()[0].path, "file");
}

#[test]
fn backup_preview_starts_on_first_content_entry() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _temp) = configured_test_app();
    app.active_screen = Screen::Preview;

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.preview_screen.load_state.is_loading());
    assert!(app.tasks.is_load_active(task::LoadTaskKind::BackupPreview));
}

#[test]
fn dashboard_starts_automation_inspection_on_first_content_entry() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _temp) = configured_test_app();
    app.active_screen = Screen::Dashboard;

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.automation_screen.status_state.is_loading());
    assert!(
        app.tasks
            .is_load_active(task::LoadTaskKind::AutomationInspection)
    );
}

#[test]
fn automation_inspection_starts_on_first_content_entry_and_suppresses_duplicates() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _temp) = configured_test_app();
    app.active_screen = Screen::Automation;

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let request_id = app
        .automation_screen
        .status_state
        .loading_id()
        .expect("initial inspection should start");

    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    assert_eq!(
        app.automation_screen.status_state.loading_id(),
        Some(request_id)
    );

    app.tasks
        .sender
        .send(task::TaskResult::AutomationInspection {
            request_id,
            result: Ok("active".to_string()),
        })
        .unwrap();
    app.poll_tasks();
    assert_eq!(
        app.automation_screen
            .status_state
            .data()
            .map(String::as_str),
        Some("active")
    );
}

#[test]
fn switching_ignore_source_invalidates_loaded_or_loading_preview() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _temp) = configured_test_app();
    app.config.as_mut().unwrap().sources = vec![
        crate::config::SourceConfig {
            path: "first".to_string(),
            ignore: vec![],
        },
        crate::config::SourceConfig {
            path: "second".to_string(),
            ignore: vec![],
        },
    ];
    app.active_screen = Screen::Ignore;
    app.focus = Focus::Content;
    app.start_ignore_preview(0);
    let old_request = app.ignore_screen.preview_state.loading_id().unwrap();

    app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

    assert_eq!(app.ignore_screen.source_idx, 1);
    assert!(matches!(
        app.ignore_screen.preview_state,
        task::LoadState::Stale { .. }
    ));
    assert!(!app.tasks.is_load_active(task::LoadTaskKind::IgnorePreview));

    app.start_ignore_preview(1);
    assert_ne!(
        app.ignore_screen.preview_state.loading_id(),
        Some(old_request)
    );
}

#[test]
fn invalidation_ignores_old_preview_result_and_accepts_replacement() {
    let (mut app, _temp) = configured_test_app();
    app.preview_screen.load_state = task::LoadState::Loaded(preview_data("baseline"));
    app.start_backup_preview();
    let old_request = app.preview_screen.load_state.loading_id().unwrap();

    app.invalidate_backup_preview();
    app.start_backup_preview();
    let replacement = app.preview_screen.load_state.loading_id().unwrap();
    assert_ne!(old_request, replacement);

    app.tasks
        .sender
        .send(task::TaskResult::BackupPreview {
            request_id: old_request,
            result: Ok(preview_data("obsolete")),
        })
        .unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::BackupPreview {
            request_id: replacement,
            result: Ok(preview_data("current")),
        })
        .unwrap();
    app.poll_tasks();

    assert_eq!(
        app.preview_screen.load_state.data().unwrap().entries[0].path,
        "current"
    );
}

#[test]
fn input_remains_responsive_while_screen_data_loads() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut app, _temp) = configured_test_app();
    app.active_screen = Screen::Preview;
    app.focus = Focus::Content;
    app.start_backup_preview();
    assert!(app.preview_screen.load_state.is_loading());

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    assert_eq!(app.preview_screen.scroll, 1);
    assert!(app.preview_screen.load_state.is_loading());
}

// --- F02: Repository browser initialization test ---

#[test]
fn repository_browser_initializes_on_focus_entry() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();

    // Create necessary directories.
    std::fs::create_dir_all(home.join(".config/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".local/share/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".run")).unwrap();

    // Setup paths.
    app.paths = Some(
        crate::paths::AppPaths::resolve(crate::paths::PathInputs {
            home: Some(home.to_path_buf()),
            config_dir: Some(home.join(".config/dothoard")),
            state_dir: Some(home.join(".local/share/dothoard")),
            runtime_dir: Some(home.join(".run")),
            use_environment: false,
        })
        .unwrap(),
    );

    // Navigate to Repository tab.
    app.active_screen = Screen::Repository;
    app.focus = Focus::TabBar;

    // Browser should be None initially.
    assert!(app.repo_screen.browser.is_none());

    // Press Down to enter content focus on Repository screen.
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    // Focus should be Content now.
    assert_eq!(app.focus, Focus::Content);

    // Browser should be initialized (Some).
    assert!(app.repo_screen.browser.is_some());
}

#[test]
fn configured_repository_change_key_restarts_browser_at_home() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = test_app();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path();
    let repository = home.join("repo");
    std::fs::create_dir(&repository).unwrap();
    std::fs::create_dir_all(home.join(".config/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".local/share/dothoard")).unwrap();
    std::fs::create_dir_all(home.join(".run")).unwrap();
    app.paths = Some(
        crate::paths::AppPaths::resolve(crate::paths::PathInputs {
            home: Some(home.to_path_buf()),
            config_dir: Some(home.join(".config/dothoard")),
            state_dir: Some(home.join(".local/share/dothoard")),
            runtime_dir: Some(home.join(".run")),
            use_environment: false,
        })
        .unwrap(),
    );
    app.config = Some(crate::config::Config::new(
        repository.display().to_string(),
        "desktop".to_string(),
    ));
    app.repo_screen = screens::repository::RepoScreen::with_path(repository.to_str().unwrap());
    app.repo_screen.ensure_browser(home);
    app.active_screen = Screen::Repository;
    app.focus = Focus::Content;

    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

    assert!(!app.repo_screen.repository_locked);
    assert_eq!(
        app.repo_screen.browser.as_ref().unwrap().current_dir(),
        home
    );
}

// --- TU03: explicit source apply/discard integration tests ---

#[test]
fn apply_selection_no_changes_returns_to_list() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }],
    });

    // Set up selection matching the config (no diff).
    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    app.handle_apply_selection();

    // No changes → returns to list immediately.
    assert_eq!(app.sources_screen.mode, screens::sources::Mode::List);
    assert!(app.sources_screen.pending_diff.is_none());
}

#[test]
fn escaping_changed_source_browser_does_not_apply_additions() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![],
    });
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(&[], std::path::Path::new("/home/user"));
    app.sources_screen
        .selection
        .as_mut()
        .unwrap()
        .toggle(std::path::Path::new("/home/user/.bashrc"), false);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(app.config.as_ref().unwrap().sources.is_empty());
    assert!(!app.should_quit);
}

#[test]
fn pending_source_changes_can_continue_or_discard_without_mutating_config() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".config".to_string(),
            ignore: vec![],
        }],
    });
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen.ensure_selection(
        app.config.as_ref().unwrap().sources.as_slice(),
        std::path::Path::new("/home/user"),
    );
    let selection = app.sources_screen.selection.as_mut().unwrap();
    selection.toggle(std::path::Path::new("/home/user/.bashrc"), false);
    selection.toggle(std::path::Path::new("/home/user/.config/secrets"), true);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(
        app.sources_screen.mode,
        screens::sources::Mode::PendingChanges
    );
    let diff = app.sources_screen.pending_diff.as_ref().unwrap();
    assert_eq!(diff.additions, vec![".bashrc"]);
    assert_eq!(
        diff.ignore_rules.get(".config").unwrap(),
        &vec!["/secrets/".to_string()]
    );

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.sources_screen.mode, screens::sources::Mode::Browse);
    assert!(app.sources_screen.selection.is_some());
    assert_eq!(app.config.as_ref().unwrap().sources.len(), 1);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
    assert_eq!(app.sources_screen.mode, screens::sources::Mode::List);
    assert!(app.sources_screen.selection.is_none());
    assert_eq!(app.config.as_ref().unwrap().sources.len(), 1);
    assert!(app.config.as_ref().unwrap().sources[0].ignore.is_empty());
}

#[test]
fn apply_selection_additions_require_explicit_choice() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }],
    });

    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    // Add a new selection.
    app.sources_screen
        .selection
        .as_mut()
        .unwrap()
        .toggle(std::path::Path::new("/home/user/.zshrc"), false);

    app.handle_apply_selection();

    assert_eq!(
        app.sources_screen.mode,
        screens::sources::Mode::PendingChanges
    );
    assert_eq!(app.config.as_ref().unwrap().sources.len(), 1);

    app.handle_choose_apply();
    assert_eq!(app.sources_screen.mode, screens::sources::Mode::List);
    let sources = &app.config.as_ref().unwrap().sources;
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().any(|s| s.path == ".zshrc"));
    assert!(matches!(
        app.preview_screen.load_state,
        task::LoadState::Stale { .. }
    ));
}

#[test]
fn apply_selection_with_removals_requires_choice_then_confirmation() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".zshrc".to_string(),
                ignore: vec![],
            },
        ],
    });

    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    // Remove .zshrc from selection.
    app.sources_screen
        .selection
        .as_mut()
        .unwrap()
        .toggle(std::path::Path::new("/home/user/.zshrc"), false);

    app.handle_apply_selection();

    assert_eq!(
        app.sources_screen.mode,
        screens::sources::Mode::PendingChanges
    );
    assert!(app.sources_screen.pending_diff.is_some());
    app.handle_choose_apply();
    assert_eq!(
        app.sources_screen.mode,
        screens::sources::Mode::ConfirmApply
    );
    let diff = app.sources_screen.pending_diff.as_ref().unwrap();
    assert_eq!(diff.removals, vec![".zshrc"]);
}

#[test]
fn confirm_apply_executes_diff() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".zshrc".to_string(),
                ignore: vec![],
            },
        ],
    });

    // Simulate: pending diff with removal of .zshrc.
    app.sources_screen.pending_diff = Some(crate::tui::selection::SelectionDiff {
        additions: vec![".config/fish".to_string()],
        removals: vec![".zshrc".to_string()],
        ignore_rules: std::collections::HashMap::new(),
    });
    app.sources_screen.mode = screens::sources::Mode::ConfirmApply;

    app.handle_confirm_apply();

    // Should return to list mode.
    assert_eq!(app.sources_screen.mode, screens::sources::Mode::List);
    // Config should have .bashrc and .config/fish (not .zshrc).
    let sources = &app.config.as_ref().unwrap().sources;
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().any(|s| s.path == ".bashrc"));
    assert!(sources.iter().any(|s| s.path == ".config/fish"));
    assert!(!sources.iter().any(|s| s.path == ".zshrc"));
    // Selection reset.
    assert!(app.sources_screen.selection.is_none());
    assert!(matches!(
        app.preview_screen.load_state,
        task::LoadState::Stale { .. }
    ));
}

#[test]
fn confirm_apply_adds_ignore_rules() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }],
    });

    let mut ignore_rules = std::collections::HashMap::new();
    ignore_rules.insert(
        ".config/fish".to_string(),
        vec!["/fish_variables".to_string()],
    );

    app.sources_screen.pending_diff = Some(crate::tui::selection::SelectionDiff {
        additions: vec![],
        removals: vec![],
        ignore_rules,
    });
    app.sources_screen.mode = screens::sources::Mode::ConfirmApply;

    app.handle_confirm_apply();

    // Source should now have the ignore rule.
    let source = &app.config.as_ref().unwrap().sources[0];
    assert_eq!(source.ignore, vec!["/fish_variables"]);
}

// --- TU14: End-to-end TUI acceptance flows ---

#[test]
fn e2e_preview_automation_failure_recovery_and_history_logs() {
    use chrono::{Duration, Utc};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, temp) = configured_test_app();
    app.config
        .as_mut()
        .unwrap()
        .sources
        .push(crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        });

    // First-entry preview loads, then recovers from a stale failure via r.
    app.active_screen = Screen::Preview;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let preview_request = app.preview_screen.load_state.loading_id().unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::BackupPreview {
            request_id: preview_request,
            result: Ok(preview_data("test-machine/home/.config/fish/config.fish")),
        })
        .unwrap();
    app.poll_tasks();
    assert!(app.preview_screen.load_state.data().is_some());
    app.invalidate_backup_preview();
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    let retry_request = app.preview_screen.load_state.loading_id().unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::BackupPreview {
            request_id: retry_request,
            result: Err("source temporarily unavailable".to_string()),
        })
        .unwrap();
    app.poll_tasks();
    assert!(matches!(
        app.preview_screen.load_state,
        task::LoadState::Failed { .. }
    ));
    assert!(app.preview_screen.load_state.data().is_some());
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    let recovery_request = app.preview_screen.load_state.loading_id().unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::BackupPreview {
            request_id: recovery_request,
            result: Ok(preview_data(
                "test-machine/home/.config/fish/recovered.fish",
            )),
        })
        .unwrap();
    app.poll_tasks();
    assert!(matches!(
        app.preview_screen.load_state,
        task::LoadState::Loaded(_)
    ));

    // Automation likewise retries a failed inspection without blocking input.
    app.active_screen = Screen::Automation;
    app.focus = Focus::TabBar;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    let automation_request = app.automation_screen.status_state.loading_id().unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::AutomationInspection {
            request_id: automation_request,
            result: Err("user manager unavailable".to_string()),
        })
        .unwrap();
    app.poll_tasks();
    assert!(matches!(
        app.automation_screen.status_state,
        task::LoadState::Failed { .. }
    ));
    app.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    let automation_retry = app.automation_screen.status_state.loading_id().unwrap();
    app.tasks
        .sender
        .send(task::TaskResult::AutomationInspection {
            request_id: automation_retry,
            result: Ok("active".to_string()),
        })
        .unwrap();
    app.poll_tasks();
    assert_eq!(
        app.automation_screen
            .status_state
            .data()
            .map(String::as_str),
        Some("active")
    );

    // A selected history entry opens its namespace-aware per-run log.
    let log_name = "accepted-run.log";
    let logs = crate::diagnostics::log_dir(temp.path().join(".local/state/dothoard").as_path());
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join(log_name), "backup completed\n").unwrap();
    app.state = Some(crate::state::AppState {
        history: vec![crate::state::RunRecord {
            namespace: "test-machine".to_string(),
            started_at: Utc::now(),
            finished_at: Utc::now() + Duration::seconds(1),
            outcome: crate::state::RunOutcome::Success,
            commit: None,
            message: None,
            log_file: Some(log_name.to_string()),
        }],
        ..crate::state::AppState::default()
    });
    app.active_screen = Screen::History;
    app.focus = Focus::Content;
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.history_screen.mode, screens::history::Mode::LogView);
    assert_eq!(
        app.history_screen.log_namespace.as_deref(),
        Some("test-machine")
    );
    assert_eq!(app.history_screen.log_lines, vec!["backup completed"]);
}

// --- MS08: Integration testing and edge cases ---

#[test]
fn e2e_inherited_deselection_produces_anchored_ignore_rules() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        }],
    });

    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    // Deselect a file inside the source (inherited → unchecked).
    app.sources_screen.selection.as_mut().unwrap().toggle(
        std::path::Path::new("/home/user/.config/fish/fish_variables"),
        false,
    );
    // Deselect a directory inside the source.
    app.sources_screen.selection.as_mut().unwrap().toggle(
        std::path::Path::new("/home/user/.config/fish/completions"),
        true,
    );

    // Review and explicitly apply.
    app.handle_apply_selection();
    app.handle_choose_apply();

    assert_eq!(app.sources_screen.mode, screens::sources::Mode::List);
    let source = &app.config.as_ref().unwrap().sources[0];
    assert_eq!(source.path, ".config/fish");
    // File gets plain anchored rule, directory gets trailing slash.
    assert!(source.ignore.contains(&"/fish_variables".to_string()));
    assert!(source.ignore.contains(&"/completions/".to_string()));
}

#[test]
fn e2e_uncheck_existing_source_with_confirmation() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![
            crate::config::SourceConfig {
                path: ".bashrc".to_string(),
                ignore: vec![],
            },
            crate::config::SourceConfig {
                path: ".zshrc".to_string(),
                ignore: vec![],
            },
        ],
    });

    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    // Uncheck .zshrc (explicit → unchecked).
    app.sources_screen
        .selection
        .as_mut()
        .unwrap()
        .toggle(std::path::Path::new("/home/user/.zshrc"), false);

    // Choose apply, then confirm because there are removals.
    app.handle_apply_selection();
    assert_eq!(
        app.sources_screen.mode,
        screens::sources::Mode::PendingChanges
    );
    app.handle_choose_apply();
    assert_eq!(
        app.sources_screen.mode,
        screens::sources::Mode::ConfirmApply
    );

    // Confirm.
    app.handle_confirm_apply();
    assert_eq!(app.sources_screen.mode, screens::sources::Mode::List);

    // Config should only have .bashrc.
    let sources = &app.config.as_ref().unwrap().sources;
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].path, ".bashrc");
}

#[test]
fn e2e_re_entering_browser_reflects_applied_config() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }],
    });

    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    // Add .zshrc and explicitly apply.
    app.sources_screen
        .selection
        .as_mut()
        .unwrap()
        .toggle(std::path::Path::new("/home/user/.zshrc"), false);
    app.handle_apply_selection();
    app.handle_choose_apply();

    // Selection is reset after apply.
    assert!(app.sources_screen.selection.is_none());

    // Re-enter browse mode: ensure_selection reloads from config.
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    let sel = app.sources_screen.selection.as_ref().unwrap();
    // Both .bashrc and .zshrc should now be explicitly selected.
    assert_eq!(
        sel.is_selected(std::path::Path::new("/home/user/.bashrc")),
        crate::tui::selection::CheckState::Explicit
    );
    assert_eq!(
        sel.is_selected(std::path::Path::new("/home/user/.zshrc")),
        crate::tui::selection::CheckState::Explicit
    );
}

#[test]
fn e2e_empty_selection_esc_is_noop() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![crate::config::SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        }],
    });

    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    // Don't change anything, just press Esc.
    app.handle_apply_selection();

    // Should silently return to list with no changes.
    assert_eq!(app.sources_screen.mode, screens::sources::Mode::List);
    assert_eq!(app.config.as_ref().unwrap().sources.len(), 1);
    assert_eq!(app.config.as_ref().unwrap().sources[0].path, ".bashrc");
}

#[test]
fn e2e_selection_reset_prevents_stale_state() {
    let mut app = test_app();
    app.config = Some(crate::config::Config {
        version: crate::config::Config::CURRENT_VERSION,
        namespace: "test-machine".to_string(),
        repository: "~/repo".to_string(),
        remote: "origin".to_string(),
        interval_minutes: 5,
        automation_backend: crate::config::AutomationBackend::Systemd,
        network_timeout_seconds: 120,
        log_retention: Default::default(),
        sources: vec![],
    });

    let home = std::path::Path::new("/home/user");
    app.sources_screen.mode = screens::sources::Mode::Browse;
    app.sources_screen
        .ensure_selection(app.config.as_ref().unwrap().sources.as_slice(), home);

    // Select a new source.
    app.sources_screen
        .selection
        .as_mut()
        .unwrap()
        .toggle(std::path::Path::new("/home/user/.config"), true);

    app.handle_apply_selection();
    app.handle_choose_apply();

    // After apply, selection is None (reset for next session entry).
    assert!(app.sources_screen.selection.is_none());
    // Config has the new source.
    assert_eq!(app.config.as_ref().unwrap().sources.len(), 1);
    assert_eq!(app.config.as_ref().unwrap().sources[0].path, ".config");
}

#[test]
fn dashboard_backup_requires_configured_source() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, _temp) = configured_test_app();
    app.focus = Focus::Content;
    app.handle_key_content(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));

    assert!(
        app.status_message
            .as_ref()
            .is_some_and(|message| message.text.contains("repository and at least one source"))
    );
}

#[test]
fn automation_backend_selection_is_explicit_and_persisted() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, _temp) = configured_test_app();
    app.active_screen = Screen::Automation;
    app.focus = Focus::Content;
    app.handle_key_content(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));

    assert_eq!(
        app.config.as_ref().unwrap().automation_backend,
        crate::config::AutomationBackend::Cron,
        "backend selection status: {:?}",
        app.status_message
    );
    let saved = crate::config::Config::load(app.paths.as_ref().unwrap().config_file()).unwrap();
    assert_eq!(
        saved.automation_backend,
        crate::config::AutomationBackend::Cron
    );
    assert!(
        app.status_message
            .as_ref()
            .is_some_and(|message| { message.text.contains("Selected cron automation") })
    );
}

#[test]
fn external_automation_refuses_managed_lifecycle_actions() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let (mut app, _temp) = configured_test_app();
    app.active_screen = Screen::Automation;
    app.focus = Focus::Content;
    app.config.as_mut().unwrap().automation_backend = crate::config::AutomationBackend::External;

    app.handle_key_content(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    assert_eq!(
        app.automation_screen.confirm,
        screens::automation::ConfirmAction::None
    );
    assert!(
        app.status_message
            .as_ref()
            .is_some_and(|message| message.text.contains("service print-command"))
    );
}

#[test]
fn automation_changes_require_configured_repository() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    app.active_screen = Screen::Automation;
    app.focus = Focus::Content;
    app.handle_key_content(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));

    assert_eq!(
        app.automation_screen.confirm,
        screens::automation::ConfirmAction::None
    );
    assert!(
        app.status_message
            .as_ref()
            .is_some_and(|message| message.text.contains("Select and validate a repository"))
    );
}

#[test]
fn repository_browser_has_no_checkboxes() {
    // Repository screen uses picker::draw with None check_fn.
    // This test verifies that the repository screen renders without panic
    // and does not show checkbox indicators.
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    let tmp = tempfile::tempdir().unwrap();
    let mut app = test_app();
    app.active_screen = Screen::Repository;
    app.focus = Focus::Content;
    app.repo_screen.ensure_browser(tmp.path());

    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &mut app))
        .expect("draw should not fail");

    let content: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
        .collect();
    // No multi-select checkbox indicators in repository browser.
    assert!(
        !content.contains("[●]"),
        "repo browser should not have explicit checkbox"
    );
    assert!(
        !content.contains("[◉]"),
        "repo browser should not have inherited checkbox"
    );
}

// --- Theme picker (Ctrl+T) ---

#[test]
fn ctrl_t_opens_theme_picker_from_anywhere() {
    let _guard = theme::TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    app.focus = Focus::Content;
    app.active_screen = Screen::Sources;

    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    assert!(app.theme_picker.is_some());
    theme::set_active(theme::ThemeId::default());
}

#[test]
fn theme_picker_owns_every_key_while_open() {
    let _guard = theme::TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert!(app.theme_picker.is_some());

    // 'q' would normally quit; while the picker owns input it must not.
    app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
    assert!(!app.should_quit);
    assert!(app.theme_picker.is_some());

    theme::set_active(theme::ThemeId::default());
}

#[test]
fn theme_picker_down_previews_the_next_theme_live() {
    let _guard = theme::TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    theme::set_active(theme::ThemeId::default());
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));

    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));

    let expected = theme::ThemeId::default().next();
    assert_eq!(theme::active_id(), expected);
    assert_eq!(app.theme_picker.as_ref().unwrap().selected, expected);

    theme::set_active(theme::ThemeId::default());
}

#[test]
fn theme_picker_esc_restores_the_theme_active_before_it_opened() {
    let _guard = theme::TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    theme::set_active(theme::ThemeId::Nord);
    let mut app = test_app();
    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert_ne!(theme::active_id(), theme::ThemeId::Nord);

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));

    assert!(app.theme_picker.is_none());
    assert_eq!(theme::active_id(), theme::ThemeId::Nord);

    theme::set_active(theme::ThemeId::default());
}

#[test]
fn theme_picker_enter_confirms_and_persists_to_disk() {
    let _guard = theme::TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    theme::set_active(theme::ThemeId::default());
    let (mut app, _temp) = configured_test_app();
    let config_dir = app.paths.as_ref().unwrap().config_dir().to_path_buf();

    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    let expected = theme::ThemeId::default().next();

    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert!(app.theme_picker.is_none());
    assert_eq!(theme::active_id(), expected);
    assert_eq!(theme::load_preference(&config_dir), Some(expected));
    assert!(app.status_message.is_some());

    theme::set_active(theme::ThemeId::default());
}

#[test]
fn theme_picker_renders_every_theme_name_and_paints_the_active_palette() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let _guard = theme::TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    theme::set_active(theme::ThemeId::default());
    let backend = TestBackend::new(100, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    let mut app = test_app();
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('t'),
        crossterm::event::KeyModifiers::CONTROL,
    ));

    terminal
        .draw(|frame| crate::tui::ui::draw(frame, &mut app))
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content: String = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol().chars().next().unwrap_or(' '))
        .collect();
    for &id in theme::ThemeId::ALL {
        assert!(content.contains(id.label()), "missing {}", id.label());
    }
    assert!(content.contains("Select Theme"));

    // The default theme leaves the canvas on the terminal-configured background.
    let system_background = theme::ThemeId::System.palette().background;
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.bg == system_background)
    );

    theme::set_active(theme::ThemeId::default());
}
