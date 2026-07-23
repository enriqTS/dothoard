//! Ratatui application and screens.
//!
//! This module provides the terminal user interface for configuring and
//! monitoring dothoard. It depends on backend services but the backend
//! never depends on TUI code.

pub mod browser;
mod event;
pub mod picker;
pub mod screens;
pub mod task;
mod terminal;
mod ui;

pub use terminal::run;

/// Which top-level element currently receives keyboard input.
///
/// The application starts with focus on the tab bar. Down/Enter/Tab enters
/// the active tab's content. Tab/Shift+Tab from content returns to the tab
/// bar. Up/k at the uppermost content boundary also returns to tab-bar focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The tab bar receives navigation keys.
    TabBar,
    /// The active screen's content receives keyboard input.
    Content,
}

/// The screens available in the TUI, corresponding to tab navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Dashboard,
    Repository,
    Sources,
    Ignore,
    Preview,
    Automation,
    History,
}

impl Screen {
    /// All screens in tab order.
    pub const ALL: &'static [Screen] = &[
        Screen::Dashboard,
        Screen::Repository,
        Screen::Sources,
        Screen::Ignore,
        Screen::Preview,
        Screen::Automation,
        Screen::History,
    ];

    /// Human-readable label for the tab bar.
    pub fn label(self) -> &'static str {
        match self {
            Screen::Dashboard => "Dashboard",
            Screen::Repository => "Repository",
            Screen::Sources => "Sources",
            Screen::Ignore => "Ignore",
            Screen::Preview => "Preview",
            Screen::Automation => "Automation",
            Screen::History => "History",
        }
    }

    /// Move to the next screen (wraps around).
    pub fn next(self) -> Screen {
        let all = Self::ALL;
        let idx = all.iter().position(|&s| s == self).unwrap_or(0);
        all[(idx + 1) % all.len()]
    }

    /// Move to the previous screen (wraps around).
    pub fn prev(self) -> Screen {
        let all = Self::ALL;
        let idx = all.iter().position(|&s| s == self).unwrap_or(0);
        all[(idx + all.len() - 1) % all.len()]
    }
}

/// Top-level TUI application state.
pub struct App {
    /// Which level currently receives keyboard input (tab bar or content).
    pub focus: Focus,
    /// The currently active screen/tab.
    pub active_screen: Screen,
    /// Whether the user has requested to quit.
    pub should_quit: bool,
    /// Background task manager for nonblocking backend operations.
    pub tasks: task::TaskManager,
    /// Last backup result received from the background thread.
    pub last_backup: Option<task::BackupResult>,
    /// Last check result received from the background thread.
    pub last_check: Option<task::CheckResult>,
    /// Resolved application paths (populated on startup if available).
    pub paths: Option<crate::paths::AppPaths>,
    /// Loaded application state (last backup, commit, push, etc.).
    pub state: Option<crate::state::AppState>,
    /// Loaded configuration.
    pub config: Option<crate::config::Config>,
    /// Status message displayed temporarily in the help bar.
    pub status_message: Option<String>,
    /// Repository selection screen state.
    pub repo_screen: screens::repository::RepoScreen,
    /// Sources management screen state.
    pub sources_screen: screens::sources::SourcesScreen,
    /// Ignore editor screen state.
    pub ignore_screen: screens::ignore::IgnoreScreen,
    /// Backup preview screen state.
    pub preview_screen: screens::preview::PreviewScreen,
    /// Automation controls screen state.
    pub automation_screen: screens::automation::AutomationScreen,
    /// History screen state.
    pub history_screen: screens::history::HistoryScreen,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let paths = crate::paths::AppPaths::from_environment().ok();

        // Load persistent state and config if paths are available.
        let state = paths
            .as_ref()
            .and_then(|p| crate::state::AppState::load(p.state_dir()).ok());
        let config = paths
            .as_ref()
            .and_then(|p| crate::config::Config::load(p.config_file()).ok());

        let repo_screen = if let Some(ref c) = config {
            screens::repository::RepoScreen::with_path(&c.repository)
        } else {
            screens::repository::RepoScreen::new()
        };

        Self {
            focus: Focus::TabBar,
            active_screen: Screen::Dashboard,
            should_quit: false,
            tasks: task::TaskManager::new(),
            last_backup: None,
            last_check: None,
            paths,
            state,
            config,
            status_message: None,
            repo_screen,
            sources_screen: screens::sources::SourcesScreen::new(),
            ignore_screen: screens::ignore::IgnoreScreen::new(),
            preview_screen: screens::preview::PreviewScreen::new(),
            automation_screen: screens::automation::AutomationScreen::new(),
            history_screen: screens::history::HistoryScreen::new(),
        }
    }

    /// Reload persistent state from disk (called after backup completes).
    pub fn reload_state(&mut self) {
        if let Some(ref paths) = self.paths {
            self.state = crate::state::AppState::load(paths.state_dir()).ok();
        }
    }

    /// Poll for completed background tasks and update state.
    pub fn poll_tasks(&mut self) {
        if let Some(result) = self.tasks.poll() {
            match result {
                task::TaskResult::Backup(r) => {
                    self.status_message = if r.success {
                        Some("Backup completed successfully.".to_string())
                    } else {
                        Some(format!(
                            "Backup failed: {}",
                            r.error.as_deref().unwrap_or("unknown error")
                        ))
                    };
                    self.last_backup = Some(r);
                    // Reload persistent state to reflect the new backup outcome.
                    self.reload_state();
                    // Mark preview stale since files may have changed.
                    self.preview_screen.stale = true;
                }
                task::TaskResult::Check(r) => {
                    self.status_message = if r.healthy {
                        Some("All checks passed.".to_string())
                    } else {
                        Some("Some checks reported issues.".to_string())
                    };
                    self.last_check = Some(r);
                }
            }
        }
    }

    /// Add a new source path to the configuration.
    fn handle_add_source(&mut self, path: String) {
        let home = self.paths.as_ref().map(|p| p.home());
        let repo_path = self
            .config
            .as_ref()
            .and_then(|c| home.map(|h| c.repository_path(h)));

        let existing = self
            .config
            .as_ref()
            .map(|c| c.sources.as_slice())
            .unwrap_or(&[]);

        if let Some(home) = home {
            match screens::sources::SourcesScreen::validate_source(
                &path,
                existing,
                home,
                repo_path.as_deref(),
            ) {
                Ok(info) => {
                    // Add the source to config.
                    if let Some(ref mut config) = self.config {
                        config.sources.push(crate::config::SourceConfig {
                            path: info.path.clone(),
                            ignore: Vec::new(),
                        });
                        // Save config.
                        if let Some(ref paths) = self.paths {
                            let _ = config.save(paths.config_file());
                        }
                    }
                    self.sources_screen.mode = screens::sources::Mode::List;
                    let msg = if let Some(ref warning) = info.warning {
                        format!("Added '{}'. {}", info.path, warning)
                    } else {
                        format!("Added '{}'.", info.path)
                    };
                    self.sources_screen.message = Some(screens::sources::Message {
                        text: msg,
                        kind: if info.warning.is_some() {
                            screens::sources::MessageKind::Warning
                        } else {
                            screens::sources::MessageKind::Info
                        },
                    });
                }
                Err(e) => {
                    self.sources_screen.message = Some(screens::sources::Message {
                        text: e,
                        kind: screens::sources::MessageKind::Error,
                    });
                }
            }
        } else {
            self.sources_screen.message = Some(screens::sources::Message {
                text: "Cannot add source: paths not resolved.".to_string(),
                kind: screens::sources::MessageKind::Error,
            });
        }
    }

    /// Remove the source at the given index from configuration.
    fn handle_remove_source(&mut self, idx: usize) {
        if let Some(ref mut config) = self.config
            && idx < config.sources.len()
        {
            let removed = config.sources.remove(idx);
            // Save config.
            if let Some(ref paths) = self.paths {
                let _ = config.save(paths.config_file());
            }
            self.sources_screen.mode = screens::sources::Mode::List;
            self.sources_screen.message = Some(screens::sources::Message {
                text: format!("Removed '{}'.", removed.path),
                kind: screens::sources::MessageKind::Info,
            });
            // Adjust selection if needed.
            if self.sources_screen.selected >= config.sources.len() && !config.sources.is_empty() {
                self.sources_screen.selected = config.sources.len() - 1;
            }
        }
        self.sources_screen.mode = screens::sources::Mode::List;
    }

    /// Add a pattern to the source at the given index.
    fn handle_add_pattern(&mut self, src_idx: usize, pattern: String) {
        if let Some(ref mut config) = self.config
            && let Some(source) = config.sources.get_mut(src_idx)
        {
            source.ignore.push(pattern.clone());
            if let Some(ref paths) = self.paths {
                let _ = config.save(paths.config_file());
            }
            self.ignore_screen.mode = screens::ignore::Mode::List;
            self.ignore_screen.preview_stale = true;
            self.ignore_screen.message = Some(format!("Added pattern '{pattern}'."));
        }
    }

    /// Remove a pattern from the source.
    fn handle_remove_pattern(&mut self, src_idx: usize, pat_idx: usize) {
        if let Some(ref mut config) = self.config {
            if let Some(source) = config.sources.get_mut(src_idx)
                && pat_idx < source.ignore.len()
            {
                let removed = source.ignore.remove(pat_idx);
                self.ignore_screen.preview_stale = true;
                self.ignore_screen.message = Some(format!("Removed pattern '{removed}'."));
                // Adjust selection.
                if self.ignore_screen.pattern_idx >= source.ignore.len()
                    && !source.ignore.is_empty()
                {
                    self.ignore_screen.pattern_idx = source.ignore.len() - 1;
                }
            }
            // Save config after mutation is complete.
            if let Some(ref paths) = self.paths {
                let _ = config.save(paths.config_file());
            }
        }
    }

    /// Refresh the ignore preview for a source.
    fn handle_refresh_preview(&mut self, src_idx: usize) {
        if let Some(ref config) = self.config
            && let Some(source) = config.sources.get(src_idx)
            && let Some(ref paths) = self.paths
        {
            self.ignore_screen.preview = screens::ignore::IgnoreScreen::generate_preview(
                &source.path,
                &source.ignore,
                paths.home(),
            );
            self.ignore_screen.preview_stale = false;
        }
    }

    /// Refresh the backup preview (dry-run planner).
    fn refresh_preview(&mut self) {
        if let Some(ref config) = self.config {
            if let Some(ref paths) = self.paths {
                let repo_path = config.repository_path(paths.home());
                match screens::preview::PreviewScreen::generate(config, paths.home(), &repo_path) {
                    Ok(data) => {
                        self.preview_screen.preview = Some(data);
                        self.preview_screen.error = None;
                        self.preview_screen.stale = false;
                        self.preview_screen.scroll = 0;
                    }
                    Err(e) => {
                        self.preview_screen.error = Some(e);
                        self.preview_screen.preview = None;
                        self.preview_screen.stale = false;
                    }
                }
            }
        } else {
            self.preview_screen.error = Some("No configuration loaded.".to_string());
        }
    }

    /// Handle a key event and update application state.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Ctrl+C is always a global exit regardless of focus.
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match self.focus {
            Focus::TabBar => self.handle_key_tab_bar(key),
            Focus::Content => self.handle_key_content(key),
        }
    }

    /// Handle keys when the tab bar has focus.
    fn handle_key_tab_bar(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        match (key.modifiers, key.code) {
            // Select next tab.
            (_, KeyCode::Right) | (_, KeyCode::Char('l')) | (KeyModifiers::NONE, KeyCode::Tab) => {
                self.active_screen = self.active_screen.next();
            }
            // Select previous tab.
            (_, KeyCode::Left) | (_, KeyCode::Char('h')) => {
                self.active_screen = self.active_screen.prev();
            }
            // Shift+Tab selects previous tab without entering content.
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.active_screen = self.active_screen.prev();
            }
            // Enter content focus.
            (_, KeyCode::Down) | (_, KeyCode::Char('j')) | (_, KeyCode::Enter) => {
                self.focus = Focus::Content;
            }
            // Direct tab selection via number keys.
            (_, KeyCode::Char(c @ '1'..='7')) => {
                let idx = (c as usize) - ('1' as usize);
                if let Some(&screen) = Screen::ALL.get(idx) {
                    self.active_screen = screen;
                }
            }
            // Quit.
            (_, KeyCode::Char('q')) | (_, KeyCode::Esc) => {
                self.should_quit = true;
            }
            _ => {}
        }
    }

    /// Handle keys when a screen's content has focus.
    fn handle_key_content(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Tab and Shift+Tab always return to tab-bar focus from content.
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Tab) | (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.focus = Focus::TabBar;
                return;
            }
            _ => {}
        }

        // Delegate to screen-specific handlers.
        let consumed = self.dispatch_to_screen(key);

        // If the screen did not consume the key, check for content-level
        // boundary escape: Up/k at the top returns to tab bar.
        if !consumed {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    self.focus = Focus::TabBar;
                }
                KeyCode::Char('q') | KeyCode::Esc => {
                    self.should_quit = true;
                }
                _ => {}
            }
        }
    }

    /// Dispatch a key to the active screen. Returns true if consumed.
    fn dispatch_to_screen(&mut self, key: crossterm::event::KeyEvent) -> bool {
        match self.active_screen {
            Screen::Dashboard => self.handle_dashboard_key(key),
            Screen::Repository => self.handle_repository_key(key),
            Screen::Sources => self.handle_sources_key(key),
            Screen::Ignore => self.handle_ignore_key(key),
            Screen::Preview => self.handle_preview_key(key),
            Screen::Automation => self.handle_automation_key(key),
            Screen::History => self.handle_history_key(key),
        }
    }

    /// Dashboard content key handling.
    fn handle_dashboard_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        match key.code {
            KeyCode::Char('b') if !self.tasks.is_busy() => {
                if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_backup(paths.clone()) {
                        self.status_message = Some("Running backup...".to_string());
                    }
                } else {
                    self.status_message =
                        Some("Cannot run backup: paths not resolved.".to_string());
                }
                true
            }
            KeyCode::Char('c') if !self.tasks.is_busy() => {
                if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_check(paths.clone()) {
                        self.status_message = Some("Running check...".to_string());
                    }
                } else {
                    self.status_message = Some("Cannot run check: paths not resolved.".to_string());
                }
                true
            }
            _ => false,
        }
    }

    /// Repository content key handling.
    fn handle_repository_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        // Ensure the browser is initialized when entering this screen.
        if let Some(ref paths) = self.paths {
            self.repo_screen.ensure_browser(paths.home());
        }

        let result = self.repo_screen.handle_key(key);
        match result {
            screens::repository::KeyResult::Consumed => true,
            screens::repository::KeyResult::Validate => {
                if let Some(ref paths) = self.paths {
                    self.repo_screen.validate(paths.home());
                    if let Some(screens::repository::ValidationResult::Valid(ref info)) =
                        self.repo_screen.validation
                        && info.ownership.needs_confirmation()
                    {
                        self.repo_screen.confirm_state = match info.ownership {
                            screens::repository::OwnershipInfo::New => {
                                screens::repository::ConfirmState::AskInitialize
                            }
                            screens::repository::OwnershipInfo::Owned { .. } => {
                                screens::repository::ConfirmState::AskAttach
                            }
                            _ => screens::repository::ConfirmState::None,
                        };
                    }
                } else {
                    self.status_message = Some("Cannot validate: paths not resolved.".to_string());
                }
                true
            }
            screens::repository::KeyResult::Confirm => {
                if let Some(ref paths) = self.paths {
                    match self.repo_screen.confirm(paths.home()) {
                        Ok(repo_path) => {
                            let repo_str = repo_path.to_str().unwrap_or_default().to_string();
                            if let Some(ref mut config) = self.config {
                                config.repository = repo_str;
                            } else {
                                self.config = Some(crate::config::Config::new(repo_str));
                            }
                            if let Some(ref paths) = self.paths
                                && let Some(ref config) = self.config
                            {
                                let _ = config.save(paths.config_file());
                            }
                            self.status_message =
                                Some("Repository configured successfully.".to_string());
                        }
                        Err(e) => {
                            self.status_message = Some(format!("Error: {e}"));
                            self.repo_screen.confirm_state =
                                screens::repository::ConfirmState::None;
                        }
                    }
                }
                true
            }
            screens::repository::KeyResult::NotConsumed => false,
        }
    }

    /// Sources content key handling.
    fn handle_sources_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        // Ensure browser is initialized when in browse mode.
        if self.sources_screen.mode == screens::sources::Mode::Browse {
            if let Some(ref paths) = self.paths {
                self.sources_screen.ensure_browser(paths.home());
            }
        }

        let source_count = self.config.as_ref().map(|c| c.sources.len()).unwrap_or(0);
        let action = self.sources_screen.handle_key(key, source_count);
        match action {
            screens::sources::Action::Consumed => {
                // If we just switched to Browse mode, ensure browser exists.
                if self.sources_screen.mode == screens::sources::Mode::Browse {
                    if let Some(ref paths) = self.paths {
                        self.sources_screen.ensure_browser(paths.home());
                    }
                }
                true
            }
            screens::sources::Action::AddSource(path) => {
                self.handle_add_source(path);
                true
            }
            screens::sources::Action::RemoveSource(idx) => {
                self.handle_remove_source(idx);
                true
            }
            screens::sources::Action::NotConsumed => false,
        }
    }

    /// Ignore content key handling.
    fn handle_ignore_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let source_count = self.config.as_ref().map(|c| c.sources.len()).unwrap_or(0);
        let pattern_count = self
            .config
            .as_ref()
            .and_then(|c| c.sources.get(self.ignore_screen.source_idx))
            .map(|s| s.ignore.len())
            .unwrap_or(0);
        let action = self
            .ignore_screen
            .handle_key(key, pattern_count, source_count);
        match action {
            screens::ignore::Action::Consumed => true,
            screens::ignore::Action::AddPattern(src_idx, pattern) => {
                self.handle_add_pattern(src_idx, pattern);
                true
            }
            screens::ignore::Action::RemovePattern(src_idx, pat_idx) => {
                self.handle_remove_pattern(src_idx, pat_idx);
                true
            }
            screens::ignore::Action::RefreshPreview(src_idx) => {
                self.handle_refresh_preview(src_idx);
                true
            }
            screens::ignore::Action::NotConsumed => false,
        }
    }

    /// Preview content key handling.
    fn handle_preview_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let action = self.preview_screen.handle_key(key);
        match action {
            screens::preview::Action::Consumed => true,
            screens::preview::Action::Refresh => {
                self.refresh_preview();
                true
            }
            screens::preview::Action::RunBackup => {
                if self.tasks.is_busy() {
                    self.status_message = Some("A task is already running.".to_string());
                } else if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_backup(paths.clone()) {
                        self.status_message = Some("Running backup...".to_string());
                    }
                } else {
                    self.status_message =
                        Some("Cannot run backup: paths not resolved.".to_string());
                }
                true
            }
            screens::preview::Action::NotConsumed => false,
        }
    }

    /// Automation content key handling.
    fn handle_automation_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let action = self.automation_screen.handle_key(key);
        match action {
            screens::automation::Action::Consumed => true,
            screens::automation::Action::RefreshStatus => {
                if let Some(ref config) = self.config
                    && let Some(ref paths) = self.paths
                {
                    self.automation_screen.refresh_status(config, paths.home());
                }
                true
            }
            screens::automation::Action::Install => {
                if let Some(ref config) = self.config
                    && let Some(ref paths) = self.paths
                {
                    self.automation_screen.install(config, paths.home());
                }
                true
            }
            screens::automation::Action::Remove => {
                if let Some(ref paths) = self.paths {
                    self.automation_screen.remove(paths.home());
                }
                true
            }
            screens::automation::Action::NotConsumed => false,
        }
    }

    /// History content key handling.
    fn handle_history_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let history_len = self.state.as_ref().map(|s| s.history.len()).unwrap_or(0);
        let action = self.history_screen.handle_key(key, history_len);
        match action {
            screens::history::Action::Consumed => true,
            screens::history::Action::NotConsumed => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a minimal App for testing navigation and keys.
    fn test_app() -> App {
        App {
            focus: Focus::TabBar,
            active_screen: Screen::Dashboard,
            should_quit: false,
            tasks: task::TaskManager::new(),
            last_backup: None,
            last_check: None,
            paths: None,
            state: None,
            config: None,
            status_message: None,
            repo_screen: screens::repository::RepoScreen::new(),
            sources_screen: screens::sources::SourcesScreen::new(),
            ignore_screen: screens::ignore::IgnoreScreen::new(),
            preview_screen: screens::preview::PreviewScreen::new(),
            automation_screen: screens::automation::AutomationScreen::new(),
            history_screen: screens::history::HistoryScreen::new(),
        }
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
        assert!(app.status_message.as_ref().unwrap().contains("success"));
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
    fn esc_from_content_quits_when_not_consumed() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = test_app();
        app.focus = Focus::Content;
        app.active_screen = Screen::Dashboard;
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.should_quit);
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
                    started_at: chrono::Utc::now(),
                    finished_at: chrono::Utc::now(),
                    outcome: crate::state::RunOutcome::Success,
                    commit: None,
                    message: None,
                },
                crate::state::RunRecord {
                    started_at: chrono::Utc::now(),
                    finished_at: chrono::Utc::now(),
                    outcome: crate::state::RunOutcome::Success,
                    commit: None,
                    message: None,
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
                started_at: chrono::Utc::now(),
                finished_at: chrono::Utc::now(),
                outcome: crate::state::RunOutcome::Success,
                commit: None,
                message: None,
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
            version: 1,
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
            version: 1,
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
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
            version: 1,
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
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
            version: 1,
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
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
            version: 1,
            repository: "~/repo".to_string(),
            remote: "origin".to_string(),
            interval_minutes: 5,
            network_timeout_seconds: 120,
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
}
