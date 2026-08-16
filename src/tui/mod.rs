//! Ratatui application and screens.
//!
//! This module provides the terminal user interface for configuring and
//! monitoring dothoard. It depends on backend services but the backend
//! never depends on TUI code.

pub mod browser;
mod event;
mod modal;
pub mod picker;
mod pointer;
pub mod screens;
pub mod selection;
mod status;
pub mod task;
mod terminal;
mod text;
mod theme;
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
    /// Transient status displayed independently from contextual help.
    pub status_message: Option<status::StatusMessage>,
    /// Dashboard detail-view state.
    pub dashboard_screen: screens::dashboard::DashboardScreen,
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
    /// Theme picker overlay state, present only while it owns input.
    pub theme_picker: Option<ThemePickerState>,
    /// Interactive rectangles recorded during the most recent frame.
    pointer_map: std::cell::RefCell<pointer::PointerMap>,
}

/// State for the global theme picker overlay (Ctrl+T).
///
/// Moving the selection previews the highlighted theme immediately; Enter
/// persists it and Esc restores whatever was active before the picker
/// opened.
pub struct ThemePickerState {
    /// The theme that was active before the picker opened.
    previous: theme::ThemeId,
    /// The currently highlighted row.
    pub selected: theme::ThemeId,
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
        } else if let Some(ref app_paths) = paths {
            // First run starts directly in repository setup. Namespace selection
            // follows repository validation so existing manifests can be listed.
            repo_screen.ensure_browser(app_paths.home());
        }
        let first_run = config.is_none();

        let theme_id = paths
            .as_ref()
            .and_then(|p| theme::load_preference(p.config_dir()))
            .unwrap_or_default();
        theme::set_active(theme_id);

        Self {
            focus: if first_run {
                Focus::Content
            } else {
                Focus::TabBar
            },
            active_screen: if first_run {
                Screen::Repository
            } else {
                Screen::Dashboard
            },
            should_quit: false,
            tasks: task::TaskManager::new(),
            last_backup: None,
            last_check: None,
            paths,
            state,
            config,
            status_message: None,
            dashboard_screen: screens::dashboard::DashboardScreen::default(),
            repo_screen,
            sources_screen: screens::sources::SourcesScreen::new(),
            ignore_screen: screens::ignore::IgnoreScreen::new(),
            preview_screen: screens::preview::PreviewScreen::new(),
            automation_screen: screens::automation::AutomationScreen::new(),
            history_screen: screens::history::HistoryScreen::new(),
            theme_picker: None,
            pointer_map: std::cell::RefCell::new(pointer::PointerMap::default()),
        }
    }

    fn publish_status(&mut self, message: status::StatusMessage) {
        status::publish(&mut self.status_message, message);
    }

    fn success(&mut self, message: impl Into<String>) {
        self.publish_status(status::StatusMessage::success(message));
    }

    fn warning(&mut self, message: impl Into<String>) {
        self.publish_status(status::StatusMessage::warning(message));
    }

    fn error(&mut self, message: impl Into<String>) {
        self.publish_status(status::StatusMessage::error(message));
    }

    fn running(&mut self, message: impl Into<String>) {
        self.publish_status(status::StatusMessage::running(message));
    }

    /// Open the theme picker, remembering the active theme so Esc can
    /// restore it if the user backs out without confirming a choice.
    fn open_theme_picker(&mut self) {
        let previous = theme::active_id();
        self.theme_picker = Some(ThemePickerState {
            previous,
            selected: previous,
        });
    }

    /// Handle a key event while the theme picker owns input.
    fn handle_theme_picker_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        let Some(picker) = self.theme_picker.as_mut() else {
            return;
        };

        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
                picker.selected = picker.selected.prev();
                theme::set_active(picker.selected);
            }
            (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
                picker.selected = picker.selected.next();
                theme::set_active(picker.selected);
            }
            (KeyModifiers::NONE, KeyCode::Enter) => {
                let id = picker.selected;
                self.theme_picker = None;
                self.persist_theme(id);
            }
            (KeyModifiers::NONE, KeyCode::Esc) | (KeyModifiers::CONTROL, KeyCode::Char('t')) => {
                theme::set_active(picker.previous);
                self.theme_picker = None;
            }
            _ => {}
        }
    }

    /// Save the chosen theme to `theme.toml` so it survives restarts.
    /// Persistence failures only surface as a status message; the theme
    /// itself is already active regardless of whether the write succeeds.
    fn persist_theme(&mut self, id: theme::ThemeId) {
        let Some(paths) = self.paths.as_ref() else {
            self.success(format!("Theme set to {} (not saved)", id.label()));
            return;
        };
        match theme::save_preference(paths.config_dir(), id) {
            Ok(()) => self.success(format!("Theme set to {}", id.label())),
            Err(err) => self.warning(format!("Theme set to {} (save failed: {err})", id.label())),
        }
    }

    /// Advance transient UI state on the periodic event-loop tick.
    pub fn tick(&mut self) {
        if self
            .status_message
            .as_mut()
            .is_some_and(status::StatusMessage::tick)
        {
            self.status_message = None;
        }
    }

    /// Move screen-local feedback into the authoritative status region.
    fn promote_screen_messages(&mut self) {
        if let Some(message) = self.sources_screen.message.take() {
            let message = match message.kind {
                screens::sources::MessageKind::Info => status::StatusMessage::success(message.text),
                screens::sources::MessageKind::Warning => {
                    status::StatusMessage::warning(message.text)
                }
                screens::sources::MessageKind::Error => status::StatusMessage::error(message.text),
            };
            self.publish_status(message);
        }
        if let Some(message) = self.ignore_screen.message.take() {
            let message = match message.kind {
                screens::ignore::MessageKind::Success => {
                    status::StatusMessage::success(message.text)
                }
                screens::ignore::MessageKind::Error => status::StatusMessage::error(message.text),
            };
            self.publish_status(message);
        }
        if let Some(message) = self.automation_screen.message.take() {
            let message = if message.success {
                status::StatusMessage::success(message.text)
            } else {
                status::StatusMessage::error(message.text)
            };
            self.publish_status(message);
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
                    if r.success {
                        self.success("Backup completed successfully.");
                    } else {
                        self.error(format!(
                            "Backup failed: {}",
                            r.error.as_deref().unwrap_or("unknown error")
                        ));
                    }
                    self.last_backup = Some(r);
                    self.reload_state();
                    self.invalidate_backup_preview();
                }
                task::TaskResult::Check(r) => {
                    if r.healthy {
                        self.success("All checks passed.");
                    } else {
                        self.warning("Some checks reported issues.");
                    }
                    self.last_check = Some(r);
                }
                task::TaskResult::Push(r) => {
                    if r.success {
                        self.success("Push completed successfully.");
                    } else {
                        self.error(format!(
                            "Push failed: {}",
                            r.error.as_deref().unwrap_or("unknown error")
                        ));
                    }
                    self.reload_state();
                }
                task::TaskResult::RepositoryValidation { request_id, result } => {
                    if self.repo_screen.validation.finish(request_id, result) {
                        let validated = self.repo_screen.validation.data().cloned();
                        if let Some(info) = validated {
                            let active_namespace = self
                                .config
                                .as_ref()
                                .map(|config| config.namespace.clone())
                                .unwrap_or_default();
                            let _ = self
                                .repo_screen
                                .refresh_namespaces(&info.path, &active_namespace);
                            if self.config.is_none() {
                                // A fresh installation must choose or create a namespace after
                                // selecting the repository; no machine name is inferred.
                                self.repo_screen.confirm_state =
                                    screens::repository::ConfirmState::None;
                                self.repo_screen.mode = screens::repository::RepoMode::Namespaces;
                            } else if info.ownership.needs_confirmation() {
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
            self.ignore_screen.message = Some(screens::ignore::Message {
                text: format!("Added pattern '{pattern}'."),
                kind: screens::ignore::MessageKind::Success,
            });
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
                self.ignore_screen.message = Some(screens::ignore::Message {
                    text: format!("Removed pattern '{removed}'."),
                    kind: screens::ignore::MessageKind::Success,
                });
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
            Screen::Dashboard => self.dashboard_screen.detail.is_some(),
            Screen::Preview | Screen::History => false,
        }
    }

    pub(crate) fn clear_pointer_map(&self) {
        self.pointer_map.borrow_mut().clear();
    }

    pub(crate) fn register_click(&self, rect: ratatui::layout::Rect, action: pointer::ClickAction) {
        self.pointer_map.borrow_mut().click(rect, action);
    }

    pub(crate) fn register_scroll(
        &self,
        rect: ratatui::layout::Rect,
        action: pointer::ScrollAction,
    ) {
        self.pointer_map.borrow_mut().scroll(rect, action);
    }

    /// Handle a mouse or touchpad event using hit regions from the last frame.
    pub fn handle_mouse(&mut self, mouse: crossterm::event::MouseEvent) {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
        use pointer::{ClickAction, ScrollAction};

        let key = |code, modifiers| KeyEvent::new(code, modifiers);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(action) = self.pointer_map.borrow().click_at(mouse.column, mouse.row)
                else {
                    return;
                };
                match action {
                    ClickAction::Tab(screen) => {
                        self.active_screen = screen;
                        self.focus = Focus::TabBar;
                    }
                    ClickAction::FocusContent => {
                        if self.focus == Focus::TabBar {
                            self.handle_key(key(KeyCode::Down, KeyModifiers::NONE));
                        }
                    }
                    ClickAction::Key(code, modifiers) => self.handle_key(key(code, modifiers)),
                    ClickAction::PickerEntry(index) => {
                        self.focus = Focus::Content;
                        let browser = match self.active_screen {
                            Screen::Repository => self.repo_screen.browser.as_mut(),
                            Screen::Sources => self.sources_screen.browser.as_mut(),
                            _ => None,
                        };
                        if let Some(browser) = browser {
                            browser.select_index(index);
                        }
                    }
                    ClickAction::PickerToggle(index) => {
                        self.focus = Focus::Content;
                        if let Some(browser) = self.sources_screen.browser.as_mut() {
                            browser.select_index(index);
                        }
                        self.handle_key(key(KeyCode::Char(' '), KeyModifiers::NONE));
                    }
                    ClickAction::Source(index) => {
                        self.focus = Focus::Content;
                        self.sources_screen.selected = index;
                    }
                    ClickAction::IgnoreSource(index) => {
                        self.focus = Focus::Content;
                        self.ignore_screen.source_idx = index;
                        self.ignore_screen.pattern_idx = 0;
                        self.ignore_screen.list_focus = screens::ignore::ListFocus::SourceSelector;
                        self.invalidate_ignore_preview();
                    }
                    ClickAction::IgnorePattern(index) => {
                        self.focus = Focus::Content;
                        self.ignore_screen.pattern_idx = index;
                        self.ignore_screen.list_focus = screens::ignore::ListFocus::PatternList;
                    }
                    ClickAction::Namespace(index) => {
                        self.focus = Focus::Content;
                        self.repo_screen.namespace_selected = index;
                    }
                    ClickAction::History(index) => {
                        self.focus = Focus::Content;
                        self.history_screen.selected = index;
                        let len = self.state.as_ref().map_or(0, |state| state.history.len());
                        self.history_screen.clamp_history(len);
                    }
                    ClickAction::Theme(index) => {
                        if let Some(id) = theme::ThemeId::ALL.get(index).copied()
                            && let Some(picker) = self.theme_picker.as_mut()
                        {
                            picker.selected = id;
                            theme::set_active(id);
                        }
                    }
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if self.theme_picker.is_some() || self.content_owns_quit_shortcuts() {
                    return;
                }
                let Some(target) = self.pointer_map.borrow().scroll_at(mouse.column, mouse.row)
                else {
                    return;
                };
                self.focus = Focus::Content;
                let down = mouse.kind == MouseEventKind::ScrollDown;
                let (code, modifiers) = match target {
                    ScrollAction::Vertical | ScrollAction::PickerEntries => (
                        if down { KeyCode::Down } else { KeyCode::Up },
                        KeyModifiers::NONE,
                    ),
                    ScrollAction::PickerPreview => (
                        if down { KeyCode::Down } else { KeyCode::Up },
                        KeyModifiers::CONTROL,
                    ),
                };
                self.handle_key(key(code, modifiers));
                // Reaching the top of a pointer-scrolled list must not move
                // keyboard focus back to the tab bar.
                self.focus = Focus::Content;
            }
            _ => {}
        }
        self.promote_screen_messages();
    }

    /// Handle a key event and update application state.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::{KeyCode, KeyModifiers};

        // The theme picker is a global overlay: while open, it owns every
        // key regardless of which screen or mode was active beneath it.
        if self.theme_picker.is_some() {
            self.handle_theme_picker_key(key);
            return;
        }

        // Ctrl+T opens the theme picker from anywhere, mirroring Ctrl+C's
        // always-available precedent.
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('t') {
            self.open_theme_picker();
            return;
        }

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
        self.promote_screen_messages();
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
                } else if self.active_screen == Screen::Dashboard
                    && matches!(
                        self.automation_screen.status_state,
                        task::LoadState::NotLoaded | task::LoadState::Stale { .. }
                    )
                {
                    self.start_automation_inspection();
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

        if self.dashboard_screen.detail.is_some() {
            if key.code == KeyCode::Esc {
                self.dashboard_screen.close_detail();
            }
            return true;
        }

        match key.code {
            KeyCode::Char('b') if !self.tasks.is_busy() => {
                if self.paths.is_none() {
                    self.error("Cannot run backup: paths not resolved.");
                } else if self
                    .config
                    .as_ref()
                    .is_none_or(|config| config.sources.is_empty())
                {
                    self.warning(
                        "Configure a repository and at least one source before backing up.",
                    );
                } else if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_backup(paths.clone()) {
                        self.running("Backup in progress...");
                    }
                } else {
                    self.error("Cannot run backup: paths not resolved.");
                }
                true
            }
            KeyCode::Char('c') if !self.tasks.is_busy() => {
                if self.paths.is_none() {
                    self.error("Cannot run check: paths not resolved.");
                } else if self.config.is_none() {
                    self.warning("Select and validate a repository before running a check.");
                } else if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_check(paths.clone()) {
                        self.running("Check in progress...");
                    }
                } else {
                    self.error("Cannot run check: paths not resolved.");
                }
                true
            }
            KeyCode::Char('p') if !self.tasks.is_busy() => {
                if self.config.is_none() {
                    self.warning("Select and validate a repository before pushing.");
                } else if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_push(paths.clone()) {
                        self.running("Push in progress...");
                    }
                } else {
                    self.error("Cannot push: paths not resolved.");
                }
                true
            }
            KeyCode::Char('a') => {
                self.active_screen = Screen::Automation;
                if matches!(
                    self.automation_screen.status_state,
                    task::LoadState::NotLoaded | task::LoadState::Stale { .. }
                ) {
                    self.start_automation_inspection();
                }
                true
            }
            KeyCode::Char('r') => {
                self.active_screen = Screen::Repository;
                if let Some(ref paths) = self.paths {
                    self.repo_screen.ensure_browser(paths.home());
                }
                true
            }
            KeyCode::Char('d') => {
                if let Some(detail) = self.dashboard_detail() {
                    self.dashboard_screen.open_detail(detail.0, detail.1);
                } else {
                    self.warning("No error, warning, or check issue is available to show.");
                }
                true
            }
            _ => false,
        }
    }

    fn dashboard_detail(&self) -> Option<(String, String)> {
        if let Some(check) = &self.last_check
            && let Some(item) = check.results.iter().find(|item| {
                matches!(
                    item.status,
                    task::CheckItemStatus::Error | task::CheckItemStatus::Warning
                )
            })
        {
            return Some((
                format!("Check: {}", item.label),
                item.detail.clone().unwrap_or_else(|| item.label.clone()),
            ));
        }
        self.state.as_ref().and_then(|state| {
            state
                .latest_error
                .as_ref()
                .map(|error| ("Latest backup error".to_string(), error.clone()))
                .or_else(|| {
                    state
                        .latest_warning
                        .as_ref()
                        .map(|warning| ("Latest backup warning".to_string(), warning.clone()))
                })
        })
    }

    fn handle_namespace_action(&mut self) {
        let action = self.repo_screen.namespace_action;
        let requested = self.repo_screen.namespace_input.clone();
        let Some(paths) = self.paths.clone() else {
            self.error("Cannot manage namespace: paths not resolved.");
            return;
        };
        let mut config = if let Some(config) = self.config.clone() {
            config
        } else if let Some(info) = self.repo_screen.validation.data() {
            crate::config::Config::new(info.path.display().to_string(), requested.clone())
        } else {
            self.warning("Select a repository first, then choose its namespace.");
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
                let source_count = config.sources.len();
                self.config = Some(config);
                self.sources_screen.selection = None;
                self.sources_screen.selected = 0;
                self.ignore_screen.source_idx = 0;
                self.ignore_screen.pattern_idx = 0;
                self.repo_screen.set_namespace(&requested);
                let _ = self.repo_screen.refresh_namespaces(&repository, &requested);
                self.repo_screen.namespace_action = screens::repository::NamespaceAction::None;
                self.repo_screen.mode = screens::repository::RepoMode::Browser;
                self.repo_screen.lock_to_repository(&repository);
                self.invalidate_repository_validation();
                self.invalidate_dependent_previews();
                self.invalidate_automation_status();
                self.success(format!(
                    "Active namespace: {requested} ({source_count} sources active)"
                ));
            }
            Err(e) => self.error(format!("Namespace operation failed: {e}")),
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
                .unwrap_or("");
            match key.code {
                KeyCode::Char('c') if self.config.is_some() => {
                    if let Some(paths) = &self.paths {
                        self.repo_screen.browse_from_home(paths.home());
                    }
                    return true;
                }
                KeyCode::Char('m') => {
                    if let Some(config) = &self.config
                        && let Some(paths) = &self.paths
                    {
                        let repository = config.repository_path(paths.home());
                        if let Err(error) = self
                            .repo_screen
                            .refresh_namespaces(&repository, &config.namespace)
                        {
                            self.warning(error);
                        }
                    }
                    self.repo_screen.mode = screens::repository::RepoMode::Namespaces;
                    return true;
                }
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
                            self.success("Repository configured successfully.");
                        }
                        Err(e) => {
                            self.error(e.to_string());
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
        if self.sources_screen.mode == screens::sources::Mode::List
            && key.code == crossterm::event::KeyCode::Char('a')
            && self.config.is_none()
        {
            self.warning("Select and validate a repository before adding sources.");
            return true;
        }

        // Ensure browser is initialized when in browse mode.
        if self.sources_screen.mode == screens::sources::Mode::Browse
            && let Some(ref paths) = self.paths
        {
            self.sources_screen.ensure_browser(paths.home());
        }

        let source_count = self.config.as_ref().map(|c| c.sources.len()).unwrap_or(0);
        let action = self.sources_screen.handle_key(key, source_count);
        match action {
            screens::sources::Action::Consumed => {
                // If we just switched to Browse mode, ensure browser and selection exist.
                if self.sources_screen.mode == screens::sources::Mode::Browse
                    && let Some(ref paths) = self.paths
                {
                    self.sources_screen.ensure_browser(paths.home());
                    let sources = self
                        .config
                        .as_ref()
                        .map(|c| c.sources.as_slice())
                        .unwrap_or(&[]);
                    self.sources_screen.ensure_selection(sources, paths.home());
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
                if self
                    .config
                    .as_ref()
                    .is_none_or(|config| config.sources.is_empty())
                {
                    self.warning(
                        "Configure a repository and at least one source before backing up.",
                    );
                } else if self.tasks.is_busy() {
                    self.warning("A task is already running.");
                } else if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_backup(paths.clone()) {
                        self.running("Backup in progress...");
                    }
                } else {
                    self.error("Cannot run backup: paths not resolved.");
                }
                true
            }
            screens::preview::Action::Push => {
                if self.config.is_none() {
                    self.warning("Select and validate a repository before pushing.");
                } else if self.tasks.is_busy() {
                    self.warning("A task is already running.");
                } else if let Some(ref paths) = self.paths {
                    if self.tasks.spawn_push(paths.clone()) {
                        self.running("Push in progress...");
                    }
                } else {
                    self.error("Cannot push: paths not resolved.");
                }
                true
            }
            screens::preview::Action::NotConsumed => false,
        }
    }

    /// Automation content key handling.
    fn handle_automation_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        if matches!(
            key.code,
            crossterm::event::KeyCode::Char('i') | crossterm::event::KeyCode::Char('x')
        ) && self.automation_screen.confirm == screens::automation::ConfirmAction::None
            && (self.config.is_none() || self.paths.is_none())
        {
            self.warning("Select and validate a repository before changing automation.");
            return true;
        }
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
                    self.invalidate_automation_status();
                } else {
                    self.error("Cannot install automation without a validated repository.");
                }
                true
            }
            screens::automation::Action::Remove => {
                if let Some(ref paths) = self.paths {
                    self.automation_screen.remove(paths.home());
                    self.invalidate_automation_status();
                } else {
                    self.error("Cannot remove automation: application paths are unavailable.");
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
#[path = "../../tests/unit/tui/mod.rs"]
mod tests;
