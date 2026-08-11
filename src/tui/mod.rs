//! Ratatui application and screens.
//!
//! This module provides the terminal user interface for configuring and
//! monitoring dothoard. It depends on backend services but the backend
//! never depends on TUI code.

pub mod browser;
mod event;
pub mod picker;
pub mod screens;
pub mod selection;
pub mod task;
mod terminal;
mod text;
mod ui;
mod viewport;

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

        let mut repo_screen = if let Some(ref c) = config {
            screens::repository::RepoScreen::with_path(&c.repository)
        } else {
            screens::repository::RepoScreen::new()
        };
        if let Some(ref c) = config {
            repo_screen.set_namespace(&c.namespace);
        }

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
            let history_len = self.state.as_ref().map_or(0, |state| state.history.len());
            self.history_screen.clamp_history(history_len);
        }
    }

    /// Poll for all completed background tasks and update state.
    pub fn poll_tasks(&mut self) {
        while let Some(result) = self.tasks.poll() {
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
                    self.reload_state();
                    self.invalidate_backup_preview();
                }
                task::TaskResult::Check(r) => {
                    self.status_message = if r.healthy {
                        Some("All checks passed.".to_string())
                    } else {
                        Some("Some checks reported issues.".to_string())
                    };
                    self.last_check = Some(r);
                }
                task::TaskResult::Push(r) => {
                    self.status_message = if r.success {
                        Some("Push completed successfully.".to_string())
                    } else {
                        Some(format!(
                            "Push failed: {}",
                            r.error.as_deref().unwrap_or("unknown error")
                        ))
                    };
                    self.reload_state();
                }
                task::TaskResult::RepositoryValidation { request_id, result } => {
                    if self.repo_screen.validation.finish(request_id, result)
                        && let Some(info) = self.repo_screen.validation.data()
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
                }
                task::TaskResult::BackupPreview { request_id, result } => {
                    if self.preview_screen.load_state.finish(request_id, result) {
                        self.preview_screen.scroll = 0;
                    }
                }
                task::TaskResult::IgnorePreview {
                    request_id,
                    source_idx,
                    result,
                } => {
                    if source_idx == self.ignore_screen.source_idx
                        && self.ignore_screen.preview_state.finish(request_id, result)
                    {
                        let len = self.ignore_screen.preview().map_or(0, <[_]>::len);
                        self.ignore_screen.preview_viewport.clamp(len);
                    }
                }
                task::TaskResult::AutomationInspection { request_id, result } => {
                    self.automation_screen
                        .status_state
                        .finish(request_id, result);
                }
            }
        }
    }

    fn invalidate_repository_validation(&mut self) {
        self.tasks
            .invalidate_load(task::LoadTaskKind::RepositoryValidation);
        self.repo_screen.validation.invalidate();
        self.repo_screen.confirm_state = screens::repository::ConfirmState::None;
    }

    fn invalidate_backup_preview(&mut self) {
        self.tasks
            .invalidate_load(task::LoadTaskKind::BackupPreview);
        self.preview_screen.load_state.invalidate();
    }

    fn invalidate_ignore_preview(&mut self) {
        self.tasks
            .invalidate_load(task::LoadTaskKind::IgnorePreview);
        self.ignore_screen.mark_preview_stale();
    }

    fn invalidate_dependent_previews(&mut self) {
        self.invalidate_backup_preview();
        self.invalidate_ignore_preview();
    }

    fn invalidate_automation_status(&mut self) {
        self.tasks
            .invalidate_load(task::LoadTaskKind::AutomationInspection);
        self.automation_screen.status_state.invalidate();
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
                    self.invalidate_dependent_previews();
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
        let mut removed_source = false;
        if let Some(ref mut config) = self.config
            && idx < config.sources.len()
        {
            removed_source = true;
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
            // Adjust source list selection.
            if self.sources_screen.selected >= config.sources.len() && !config.sources.is_empty() {
                self.sources_screen.selected = config.sources.len() - 1;
            }
            // Clamp ignore screen's source index.
            if config.sources.is_empty() {
                self.ignore_screen.source_idx = 0;
                self.ignore_screen.pattern_idx = 0;
            } else if self.ignore_screen.source_idx >= config.sources.len() {
                self.ignore_screen.source_idx = config.sources.len() - 1;
                self.ignore_screen.pattern_idx = 0;
            }
        }
        if removed_source {
            self.invalidate_dependent_previews();
        }
        self.sources_screen.mode = screens::sources::Mode::List;
    }

    /// Review changes when leaving the source browser.
    ///
    /// An unchanged session closes immediately. Changed sessions always stop
    /// at an explicit apply/discard/continue choice before configuration can
    /// be mutated.
    fn handle_apply_selection(&mut self) {
        let sources = self
            .config
            .as_ref()
            .map(|c| c.sources.as_slice())
            .unwrap_or(&[]);

        if let Some(ref sel) = self.sources_screen.selection {
            let diff = sel.diff_against_config(sources);
            if diff.additions.is_empty() && diff.removals.is_empty() && diff.ignore_rules.is_empty()
            {
                self.sources_screen.mode = screens::sources::Mode::List;
                self.sources_screen.selection = None;
                self.sources_screen.pending_diff = None;
                self.sources_screen.message = None;
            } else {
                self.sources_screen.pending_diff = Some(diff);
                self.sources_screen.mode = screens::sources::Mode::PendingChanges;
                self.sources_screen.message = None;
            }
        } else {
            self.sources_screen.mode = screens::sources::Mode::List;
        }
    }

    /// Continue an explicit apply request, preserving removal confirmation.
    fn handle_choose_apply(&mut self) {
        match self.sources_screen.pending_diff.as_ref() {
            Some(diff) if !diff.removals.is_empty() => {
                self.sources_screen.mode = screens::sources::Mode::ConfirmApply;
            }
            Some(_) => self.execute_selection_diff(),
            None => self.sources_screen.mode = screens::sources::Mode::Browse,
        }
    }

    /// Discard all edits from the current source-browser session.
    fn handle_discard_selection(&mut self) {
        self.sources_screen.selection = None;
        self.sources_screen.pending_diff = None;
        self.sources_screen.mode = screens::sources::Mode::List;
        self.sources_screen.message = Some(screens::sources::Message {
            text: "Source changes discarded.".to_string(),
            kind: screens::sources::MessageKind::Info,
        });
    }

    /// Handle ConfirmApply action (user pressed 'y' in the removal dialog).
    fn handle_confirm_apply(&mut self) {
        self.execute_selection_diff();
    }

    /// Execute the pending selection diff: add sources, remove sources, add ignore rules.
    fn execute_selection_diff(&mut self) {
        let diff = match self.sources_screen.pending_diff.take() {
            Some(d) => d,
            None => {
                self.sources_screen.mode = screens::sources::Mode::List;
                return;
            }
        };

        let mut added = 0usize;
        let mut removed = 0usize;
        let mut ignored = 0usize;

        if let Some(ref mut config) = self.config {
            // Remove sources.
            config.sources.retain(|s| !diff.removals.contains(&s.path));
            removed = diff.removals.len();

            // Add new sources.
            for path in &diff.additions {
                config.sources.push(crate::config::SourceConfig {
                    path: path.clone(),
                    ignore: Vec::new(),
                });
                added += 1;
            }

            // Add ignore rules.
            for (source_path, rules) in &diff.ignore_rules {
                if let Some(source) = config.sources.iter_mut().find(|s| &s.path == source_path) {
                    for rule in rules {
                        source.ignore.push(rule.clone());
                        ignored += 1;
                    }
                }
            }

            // Save config.
            if let Some(ref paths) = self.paths {
                let _ = config.save(paths.config_file());
            }

            // Clamp selections.
            if self.sources_screen.selected >= config.sources.len() && !config.sources.is_empty() {
                self.sources_screen.selected = config.sources.len() - 1;
            }
            if config.sources.is_empty() {
                self.ignore_screen.source_idx = 0;
                self.ignore_screen.pattern_idx = 0;
            } else if self.ignore_screen.source_idx >= config.sources.len() {
                self.ignore_screen.source_idx = config.sources.len() - 1;
                self.ignore_screen.pattern_idx = 0;
            }
        }

        // Build feedback message.
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(format!("Added {added}"));
        }
        if removed > 0 {
            parts.push(format!("removed {removed}"));
        }
        if ignored > 0 {
            parts.push(format!("{ignored} ignore rules"));
        }
        let msg = if parts.is_empty() {
            "No changes applied.".to_string()
        } else {
            format!("{}.", parts.join(", "))
        };

        self.sources_screen.mode = screens::sources::Mode::List;
        self.sources_screen.message = Some(screens::sources::Message {
            text: msg,
            kind: screens::sources::MessageKind::Info,
        });

        // Reset selection so next Browse entry reloads from config.
        self.sources_screen.selection = None;

        self.invalidate_dependent_previews();
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
            self.ignore_screen.message = Some(format!("Added pattern '{pattern}'."));
        }
        self.invalidate_dependent_previews();
    }

    /// Remove a pattern from the source.
    fn handle_remove_pattern(&mut self, src_idx: usize, pat_idx: usize) {
        if let Some(ref mut config) = self.config {
            if let Some(source) = config.sources.get_mut(src_idx)
                && pat_idx < source.ignore.len()
            {
                let removed = source.ignore.remove(pat_idx);
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
        self.invalidate_dependent_previews();
    }

    fn start_ignore_preview(&mut self, src_idx: usize) {
        if self.ignore_screen.preview_state.is_loading() {
            return;
        }
        let Some(config) = self.config.as_ref() else {
            self.ignore_screen
                .preview_state
                .fail("No configuration loaded.".to_string(), true);
            return;
        };
        let Some(source) = config.sources.get(src_idx) else {
            self.ignore_screen
                .preview_state
                .fail("No source selected.".to_string(), true);
            return;
        };
        let Some(paths) = self.paths.as_ref() else {
            self.ignore_screen
                .preview_state
                .fail("Application paths are unavailable.".to_string(), true);
            return;
        };
        if let Some(request_id) = self.tasks.spawn_ignore_preview(
            src_idx,
            source.path.clone(),
            source.ignore.clone(),
            paths.home().to_path_buf(),
        ) {
            self.ignore_screen.preview_state.begin(request_id, true);
        }
    }

    fn start_backup_preview(&mut self) {
        if self.preview_screen.load_state.is_loading() {
            return;
        }
        let Some(config) = self.config.clone() else {
            self.preview_screen
                .load_state
                .fail("No configuration loaded.".to_string(), true);
            return;
        };
        let Some(paths) = self.paths.as_ref() else {
            self.preview_screen
                .load_state
                .fail("Application paths are unavailable.".to_string(), true);
            return;
        };
        let home = paths.home().to_path_buf();
        let repository = config.repository_path(&home);
        if let Some(request_id) = self.tasks.spawn_backup_preview(config, home, repository) {
            self.preview_screen.load_state.begin(request_id, true);
        }
    }

    fn start_automation_inspection(&mut self) {
        if self.automation_screen.status_state.is_loading() {
            return;
        }
        let Some(config) = self.config.clone() else {
            self.automation_screen
                .status_state
                .fail("No configuration loaded.".to_string(), true);
            return;
        };
        let Some(paths) = self.paths.as_ref() else {
            self.automation_screen
                .status_state
                .fail("Application paths are unavailable.".to_string(), true);
            return;
        };
        if let Some(request_id) = self
            .tasks
            .spawn_automation_inspection(config, paths.home().to_path_buf())
        {
            self.automation_screen.status_state.begin(request_id, true);
        }
    }

    fn start_repository_validation(&mut self) {
        if self.repo_screen.validation.is_loading() {
            return;
        }
        let Some(paths) = self.paths.as_ref() else {
            self.repo_screen
                .validation
                .fail("Application paths are unavailable.".to_string(), false);
            return;
        };
        let (namespace, remote, timeout_seconds) = self.config.as_ref().map_or_else(
            || {
                (
                    self.repo_screen.namespace_input.clone(),
                    "origin".to_string(),
                    120,
                )
            },
            |config| {
                (
                    config.namespace.clone(),
                    config.remote.clone(),
                    config.network_timeout_seconds,
                )
            },
        );
        if let Some(request_id) = self.tasks.spawn_repository_validation(
            self.repo_screen.input.clone(),
            paths.home().to_path_buf(),
            namespace,
            remote,
            timeout_seconds,
        ) {
            self.repo_screen.validation.begin(request_id, false);
            self.repo_screen.confirm_state = screens::repository::ConfirmState::None;
        }
    }

    /// Whether the current content mode owns text and confirmation shortcuts.
    fn content_owns_quit_shortcuts(&self) -> bool {
        use screens::automation::ConfirmAction;
        use screens::repository::{ConfirmState, RepoMode};
        use screens::sources::Mode as SourcesMode;

        match self.active_screen {
            Screen::Repository => {
                self.repo_screen.mode != RepoMode::Browser
                    || self.repo_screen.confirm_state == ConfirmState::AskInitialize
                    || self.repo_screen.confirm_state == ConfirmState::AskAttach
                    || self.repo_screen.namespace_confirmation.is_some()
            }
            Screen::Sources => matches!(
                self.sources_screen.mode,
                SourcesMode::AddInput
                    | SourcesMode::ConfirmDelete
                    | SourcesMode::PendingChanges
                    | SourcesMode::ConfirmApply
            ),
            Screen::Ignore => self.ignore_screen.mode == screens::ignore::Mode::AddInput,
            Screen::Automation => self.automation_screen.confirm != ConfirmAction::None,
            Screen::Dashboard | Screen::Preview | Screen::History => false,
        }
    }

    /// Handle a key event and update application state.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // Ctrl+C quits only when no text entry or confirmation owns input.
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('c') {
            if self.focus == Focus::TabBar || !self.content_owns_quit_shortcuts() {
                self.should_quit = true;
            }
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
                // Initialize repository browser when entering Repository screen content.
                if self.active_screen == Screen::Repository {
                    if let Some(ref paths) = self.paths {
                        self.repo_screen.ensure_browser(paths.home());
                    }
                } else if self.active_screen == Screen::Preview
                    && matches!(
                        self.preview_screen.load_state,
                        task::LoadState::NotLoaded | task::LoadState::Stale { .. }
                    )
                {
                    self.start_backup_preview();
                } else if self.active_screen == Screen::Automation
                    && matches!(
                        self.automation_screen.status_state,
                        task::LoadState::NotLoaded | task::LoadState::Stale { .. }
                    )
                {
                    self.start_automation_inspection();
                }
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

        // If the screen did not consume the key, Up/k and Esc back out to
        // the tab bar. Only q is an explicit content-level quit action.
        if !consumed {
            match key.code {
                KeyCode::Up | KeyCode::Char('k') | KeyCode::Esc => {
                    self.focus = Focus::TabBar;
                }
                KeyCode::Char('q') => {
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

    fn handle_namespace_action(&mut self) {
        let action = self.repo_screen.namespace_action;
        let requested = self.repo_screen.namespace_input.clone();
        let Some(paths) = self.paths.clone() else {
            self.status_message = Some("Cannot manage namespace: paths not resolved.".into());
            return;
        };
        let Some(mut config) = self.config.clone() else {
            self.status_message =
                Some("Select a repository first, then choose its namespace.".into());
            self.repo_screen.mode = screens::repository::RepoMode::Browser;
            return;
        };
        let repository = config.repository_path(paths.home());
        let result = match action {
            screens::repository::NamespaceAction::SelectOrCreate => {
                let state = crate::git::classify_ownership(&repository, &requested);
                match state {
                    Ok(crate::git::OwnershipState::New) => {
                        crate::namespace::create(&repository, &requested, true).and_then(|_| {
                            crate::namespace::select(
                                paths.config_file(),
                                &mut config,
                                &repository,
                                &requested,
                            )
                            .map(|_| ())
                        })
                    }
                    Ok(_) => crate::namespace::select(
                        paths.config_file(),
                        &mut config,
                        &repository,
                        &requested,
                    )
                    .map(|_| ()),
                    Err(e) => Err(crate::namespace::NamespaceError::OwnershipInspect(e)),
                }
            }
            screens::repository::NamespaceAction::Rename => crate::namespace::rename(
                paths.config_file(),
                &mut config,
                &repository,
                &requested,
                true,
            )
            .map(|_| ()),
            screens::repository::NamespaceAction::Delete => crate::namespace::delete(
                paths.config_file(),
                &mut config,
                &repository,
                &requested,
                true,
            ),
            screens::repository::NamespaceAction::None => return,
        };
        match result {
            Ok(_) => {
                self.config = Some(config);
                self.repo_screen.set_namespace(&requested);
                self.repo_screen.namespace_action = screens::repository::NamespaceAction::None;
                self.repo_screen.mode = screens::repository::RepoMode::Browser;
                self.invalidate_repository_validation();
                self.invalidate_dependent_previews();
                self.invalidate_automation_status();
                self.status_message = Some(format!("Active namespace: {requested}"));
            }
            Err(e) => self.status_message = Some(format!("Namespace operation failed: {e}")),
        }
    }

    /// Repository content key handling.
    fn handle_repository_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::{KeyCode, KeyModifiers};
        use screens::repository::NamespaceAction;
        if self.repo_screen.mode == screens::repository::RepoMode::Browser
            && key.modifiers == KeyModifiers::NONE
        {
            let current = self
                .config
                .as_ref()
                .map(|c| c.namespace.as_str())
                .unwrap_or("desktop");
            match key.code {
                KeyCode::Char('n') => {
                    self.repo_screen
                        .begin_namespace(NamespaceAction::SelectOrCreate, current);
                    return true;
                }
                KeyCode::Char('r') if self.config.is_some() => {
                    self.repo_screen
                        .begin_namespace(NamespaceAction::Rename, current);
                    return true;
                }
                KeyCode::Char('d') if self.config.is_some() => {
                    self.repo_screen
                        .begin_namespace(NamespaceAction::Delete, current);
                    return true;
                }
                _ => {}
            }
        }
        // Ensure the browser is initialized when entering this screen.
        if let Some(ref paths) = self.paths {
            self.repo_screen.ensure_browser(paths.home());
        }

        let previous_input = self.repo_screen.input.clone();
        let result = self.repo_screen.handle_key(key);
        if self.repo_screen.input != previous_input {
            self.tasks
                .invalidate_load(task::LoadTaskKind::RepositoryValidation);
            self.repo_screen.validation.reset();
            self.repo_screen.confirm_state = screens::repository::ConfirmState::None;
        }
        match result {
            screens::repository::KeyResult::Consumed => true,
            screens::repository::KeyResult::Namespace => {
                self.handle_namespace_action();
                true
            }
            screens::repository::KeyResult::Validate => {
                self.start_repository_validation();
                true
            }
            screens::repository::KeyResult::Confirm => {
                if let Some(ref paths) = self.paths {
                    let namespace = self
                        .config
                        .as_ref()
                        .map(|config| config.namespace.clone())
                        .unwrap_or_else(|| self.repo_screen.namespace_input.clone());
                    match self.repo_screen.confirm(paths.home(), &namespace) {
                        Ok(repo_path) => {
                            let repo_str = repo_path.to_str().unwrap_or_default().to_string();
                            if let Some(ref mut config) = self.config {
                                config.repository = repo_str;
                            } else {
                                self.config = Some(crate::config::Config::new(repo_str, namespace));
                            }
                            if let Some(ref paths) = self.paths
                                && let Some(ref config) = self.config
                            {
                                let _ = config.save(paths.config_file());
                            }
                            self.invalidate_dependent_previews();
                            self.invalidate_automation_status();
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
                // If we just switched to Browse mode, ensure browser and selection exist.
                if self.sources_screen.mode == screens::sources::Mode::Browse {
                    if let Some(ref paths) = self.paths {
                        self.sources_screen.ensure_browser(paths.home());
                        let sources = self
                            .config
                            .as_ref()
                            .map(|c| c.sources.as_slice())
                            .unwrap_or(&[]);
                        self.sources_screen.ensure_selection(sources, paths.home());
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
            screens::sources::Action::ApplySelection => {
                self.handle_apply_selection();
                true
            }
            screens::sources::Action::ChooseApply => {
                self.handle_choose_apply();
                true
            }
            screens::sources::Action::DiscardSelection => {
                self.handle_discard_selection();
                true
            }
            screens::sources::Action::ConfirmApply => {
                self.handle_confirm_apply();
                true
            }
            screens::sources::Action::NotConsumed => false,
        }
    }

    /// Ignore content key handling.
    fn handle_ignore_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        let previous_source_idx = self.ignore_screen.source_idx;
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
        if self.ignore_screen.source_idx != previous_source_idx {
            self.tasks
                .invalidate_load(task::LoadTaskKind::IgnorePreview);
        }
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
                self.start_ignore_preview(src_idx);
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
                self.start_backup_preview();
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
            screens::preview::Action::Push => {
                if self.tasks.is_busy() {
                    self.status_message = Some("A task is already running.".to_string());
                } else if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_push(paths.clone()) {
                        self.status_message = Some("Pushing to remote...".to_string());
                    }
                } else {
                    self.status_message = Some("Cannot push: paths not resolved.".to_string());
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
                self.start_automation_inspection();
                true
            }
            screens::automation::Action::Install => {
                if let Some(ref config) = self.config
                    && let Some(ref paths) = self.paths
                {
                    self.automation_screen.install(config, paths.home());
                }
                self.invalidate_automation_status();
                true
            }
            screens::automation::Action::Remove => {
                if let Some(ref paths) = self.paths {
                    self.automation_screen.remove(paths.home());
                }
                self.invalidate_automation_status();
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
            screens::history::Action::ViewLogs => {
                // Enter log view mode for the selected entry.
                if let Some(ref state) = self.state
                    && let Some(ref paths) = self.paths
                    && let Some(record) = state.history.get(self.history_screen.selected)
                {
                    self.history_screen
                        .enter_log_view(record, paths.state_dir());
                }
                true
            }
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
            tasks: task::TaskManager::new_controlled(),
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
        repository_confirm.repo_screen.confirm_state =
            screens::repository::ConfirmState::AskInitialize;
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
            network_timeout_seconds: 120,
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
}
