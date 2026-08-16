//! Reusable three-pane filesystem browser model.
//!
//! Provides a ranger/yazi-style directory picker that:
//! - Shows parent context, current directory entries, and a preview pane.
//! - Sorts entries deterministically: directories first, then other types,
//!   alphabetical within each group (case-insensitive, locale-independent).
//! - Includes hidden entries (dotfiles are primary backup candidates).
//! - Respects a configurable root boundary.
//! - Maintains selection and scrolling state.
//! - Caches shallow directory listings per directory.
//! - Handles metadata errors gracefully without crashing.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_FILE_PREVIEW_BYTES: u64 = 256 * 1024;

/// Classification of a directory entry for display and sorting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// A real directory (not a symlink to a directory).
    Directory,
    /// A symbolic link (target not followed).
    Symlink,
    /// A regular file.
    File,
    /// Socket, device, FIFO, or other unsupported special file.
    Special,
    /// Entry whose metadata could not be read.
    Error,
}

/// A single entry in a directory listing.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The file name (not full path).
    pub name: OsString,
    /// Lossy UTF-8 representation for display and sorting.
    pub display_name: String,
    /// Whether the display name is a lossy conversion (contains replacement chars).
    pub is_lossy: bool,
    /// The kind of entry, determined without following symlinks.
    pub kind: EntryKind,
    /// Whether this entry is hidden (name starts with `.`).
    pub hidden: bool,
    /// Whether this directory contains no-follow `.git` metadata.
    pub is_git_repository: bool,
    /// Size in bytes for files, None for directories/errors.
    pub size: Option<u64>,
    /// Whether the entry has the executable bit set.
    pub executable: bool,
    /// Optional symlink target (raw, not resolved).
    pub link_target: Option<String>,
}

/// The result of reading a directory.
#[derive(Debug, Clone)]
pub enum DirListing {
    /// Successfully read entries (may be empty).
    Entries(Vec<Entry>),
    /// Failed to read the directory.
    Error(String),
}

/// Browser configuration.
#[derive(Debug, Clone)]
pub struct BrowserConfig {
    /// The root boundary; navigation cannot go above this.
    pub root: PathBuf,
    /// The starting directory (must be at or below root).
    pub start: PathBuf,
}

/// State for the three-pane filesystem browser.
#[derive(Debug)]
pub struct Browser {
    /// Configuration (root boundary).
    config: BrowserConfig,
    /// Current directory being browsed.
    current_dir: PathBuf,
    /// Selection index in the current directory listing.
    selected: usize,
    /// Scroll offset for the main pane viewport.
    scroll_offset: usize,
    /// Cache of directory listings by path.
    cache: HashMap<PathBuf, DirListing>,
    /// Cached `cat` output for regular-file previews.
    file_previews: HashMap<PathBuf, Result<FilePreview, String>>,
}

/// Text returned by `cat` for a regular-file preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilePreview {
    /// Display-safe file content.
    pub content: String,
    /// Whether output exceeded the preview limit and was clipped.
    pub truncated: bool,
}

impl Browser {
    /// Create a new browser with the given configuration.
    ///
    /// The start directory is clamped to the root if it is above it.
    pub fn new(config: BrowserConfig) -> Self {
        let current_dir = if config.start.starts_with(&config.root) {
            config.start.clone()
        } else {
            config.root.clone()
        };
        Self {
            config,
            current_dir,
            selected: 0,
            scroll_offset: 0,
            cache: HashMap::new(),
            file_previews: HashMap::new(),
        }
    }

    /// The current directory.
    pub fn current_dir(&self) -> &Path {
        &self.current_dir
    }

    /// The root boundary.
    pub fn root(&self) -> &Path {
        &self.config.root
    }

    /// Whether the browser is at the root boundary.
    pub fn at_root(&self) -> bool {
        self.current_dir == self.config.root
    }

    /// Current selection index.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Current scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Get the listing for the current directory, loading from cache or disk.
    pub fn current_listing(&mut self) -> &DirListing {
        let dir = self.current_dir.clone();
        self.ensure_cached(&dir);
        self.cache.get(&dir).unwrap()
    }

    /// Get the listing for the parent directory (for the left pane).
    /// Returns None if at the root boundary.
    pub fn parent_listing(&mut self) -> Option<&DirListing> {
        if self.at_root() {
            return None;
        }
        let parent = self.current_dir.parent()?.to_path_buf();
        if !parent.starts_with(&self.config.root) && parent != self.config.root {
            return None;
        }
        self.ensure_cached(&parent);
        self.cache.get(&parent)
    }

    /// Get the preview listing for the selected entry if it is a directory.
    /// Returns None if the selected entry is not a directory or listing is empty.
    pub fn preview_listing(&mut self) -> Option<&DirListing> {
        let entry = self.selected_entry_path()?;
        let kind = {
            let dir = self.current_dir.clone();
            self.ensure_cached(&dir);
            match self.cache.get(&dir) {
                Some(DirListing::Entries(entries)) => entries.get(self.selected).map(|e| e.kind),
                _ => None,
            }
        };
        if kind == Some(EntryKind::Directory) {
            self.ensure_cached(&entry);
            self.cache.get(&entry)
        } else {
            None
        }
    }

    /// Get the full path of the currently selected entry.
    pub fn selected_entry_path(&self) -> Option<PathBuf> {
        if let Some(DirListing::Entries(entries)) = self.cache.get(&self.current_dir) {
            entries
                .get(self.selected)
                .map(|e| self.current_dir.join(&e.name))
        } else {
            None
        }
    }

    /// Get a reference to the currently selected entry.
    pub fn selected_entry(&mut self) -> Option<&Entry> {
        let dir = self.current_dir.clone();
        self.ensure_cached(&dir);
        match self.cache.get(&dir) {
            Some(DirListing::Entries(entries)) => entries.get(self.selected),
            _ => None,
        }
    }

    /// Return cached `cat` output for the selected regular file.
    ///
    /// The command reads an already-opened, no-follow regular file through
    /// standard input, so a path race cannot make `cat` follow a symlink. Large
    /// files are refused to keep picker redraw responsive.
    pub fn selected_file_preview(&mut self) -> Option<&Result<FilePreview, String>> {
        let is_file = self
            .selected_entry()
            .is_some_and(|entry| entry.kind == EntryKind::File);
        if !is_file {
            return None;
        }
        let path = self.selected_entry_path()?;

        if !self.file_previews.contains_key(&path) {
            let preview = load_file_preview(&path);
            self.file_previews.insert(path.clone(), preview);
        }
        self.file_previews.get(&path)
    }

    /// Number of entries in the current listing.
    pub fn entry_count(&mut self) -> usize {
        let dir = self.current_dir.clone();
        self.ensure_cached(&dir);
        match self.cache.get(&dir) {
            Some(DirListing::Entries(entries)) => entries.len(),
            _ => 0,
        }
    }

    /// Move selection up. Returns true if moved.
    pub fn move_up(&mut self) -> bool {
        if self.selected > 0 {
            self.selected -= 1;
            self.adjust_scroll();
            true
        } else {
            false
        }
    }

    /// Move selection down. Returns true if moved.
    pub fn move_down(&mut self) -> bool {
        let count = self.entry_count();
        if count > 0 && self.selected < count - 1 {
            self.selected += 1;
            self.adjust_scroll();
            true
        } else {
            false
        }
    }

    /// Move to the first entry.
    pub fn move_home(&mut self) {
        self.selected = 0;
        self.scroll_offset = 0;
    }

    /// Move to the last entry.
    pub fn move_end(&mut self) {
        let count = self.entry_count();
        if count > 0 {
            self.selected = count - 1;
        }
        self.adjust_scroll();
    }

    /// Page up by a given viewport height. Returns true if selection changed.
    pub fn page_up(&mut self, viewport_height: usize) -> bool {
        if self.selected == 0 {
            return false;
        }
        self.selected = self.selected.saturating_sub(viewport_height);
        self.adjust_scroll();
        true
    }

    /// Page down by a given viewport height. Returns true if selection changed.
    pub fn page_down(&mut self, viewport_height: usize) -> bool {
        let count = self.entry_count();
        if count == 0 || self.selected >= count - 1 {
            return false;
        }
        self.selected = (self.selected + viewport_height).min(count - 1);
        self.adjust_scroll();
        true
    }

    /// Navigate into the selected directory. Returns true if successful.
    ///
    /// Only real directories can be entered (not symlinks).
    pub fn enter_selected(&mut self) -> bool {
        let dir = self.current_dir.clone();
        self.ensure_cached(&dir);
        let can_enter = match self.cache.get(&dir) {
            Some(DirListing::Entries(entries)) => entries
                .get(self.selected)
                .is_some_and(|e| e.kind == EntryKind::Directory),
            _ => false,
        };
        if can_enter {
            let new_dir = match self.cache.get(&dir) {
                Some(DirListing::Entries(entries)) => {
                    Some(self.current_dir.join(&entries[self.selected].name))
                }
                _ => None,
            };
            if let Some(new_dir) = new_dir {
                self.current_dir = new_dir;
                self.selected = 0;
                self.scroll_offset = 0;
                return true;
            }
        }
        false
    }

    /// Navigate to the parent directory. Returns true if moved.
    ///
    /// Respects the root boundary.
    pub fn go_parent(&mut self) -> bool {
        if self.at_root() {
            return false;
        }
        if let Some(parent) = self.current_dir.parent() {
            let parent = parent.to_path_buf();
            if parent.starts_with(&self.config.root) || parent == self.config.root {
                // Try to select the directory we came from in the parent.
                let old_name = self.current_dir.file_name().map(|n| n.to_os_string());
                self.current_dir = parent;
                self.selected = 0;
                self.scroll_offset = 0;

                // Select the entry we came from if possible.
                if let Some(old_name) = old_name {
                    let dir = self.current_dir.clone();
                    self.ensure_cached(&dir);
                    if let Some(DirListing::Entries(entries)) = self.cache.get(&dir)
                        && let Some(idx) = entries.iter().position(|e| e.name == old_name)
                    {
                        self.selected = idx;
                        self.adjust_scroll();
                    }
                }
                return true;
            }
        }
        false
    }

    /// Invalidate the cache for a specific directory.
    pub fn invalidate(&mut self, path: &Path) {
        self.cache.remove(path);
        self.file_previews
            .retain(|entry, _| !entry.starts_with(path));
    }

    /// Invalidate the entire cache.
    pub fn invalidate_all(&mut self) {
        self.cache.clear();
        self.file_previews.clear();
    }

    /// Refresh the current directory listing from disk.
    pub fn refresh_current(&mut self) {
        self.cache.remove(&self.current_dir.clone());
        self.file_previews
            .retain(|entry, _| entry.parent() != Some(self.current_dir.as_path()));
        let dir = self.current_dir.clone();
        self.ensure_cached(&dir);
        // Clamp selection.
        let count = match self.cache.get(&dir) {
            Some(DirListing::Entries(entries)) => entries.len(),
            _ => 0,
        };
        if self.selected >= count && count > 0 {
            self.selected = count - 1;
        } else if count == 0 {
            self.selected = 0;
        }
        self.adjust_scroll();
    }

    /// Set the viewport height for scroll adjustment calculations.
    /// This is stored implicitly via adjust_scroll calls with a default.
    /// For now, we use a reasonable default of 20 lines.
    const DEFAULT_VIEWPORT: usize = 20;

    /// Adjust scroll offset to keep selection visible.
    fn adjust_scroll(&mut self) {
        self.adjust_scroll_with_height(Self::DEFAULT_VIEWPORT);
    }

    /// Adjust scroll offset to keep selection visible with a specific viewport height.
    pub fn adjust_scroll_with_height(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.selected - viewport_height + 1;
        }
    }

    /// Ensure a directory is in the cache.
    fn ensure_cached(&mut self, path: &Path) {
        if !self.cache.contains_key(path) {
            let listing = read_directory(path);
            self.cache.insert(path.to_path_buf(), listing);
        }
    }

    /// Navigate to a specific directory. Respects root boundary.
    /// Returns true if navigation succeeded.
    pub fn navigate_to(&mut self, target: &Path) -> bool {
        if !target.starts_with(&self.config.root) && target != &*self.config.root {
            return false;
        }
        self.current_dir = target.to_path_buf();
        self.selected = 0;
        self.scroll_offset = 0;
        true
    }

    /// Attempt to select the current entry for use as a path.
    ///
    /// This validates the selection against safety rules:
    /// - Rejects non-UTF-8 file names (cannot be stored in config).
    /// - Rejects special files (sockets, devices, FIFOs).
    /// - Rejects error entries (disappeared or unreadable).
    /// - Returns metadata about the selection (kind, whether it's a symlink).
    ///
    /// The caller decides which kinds are acceptable (e.g., source picker
    /// accepts files, directories, and source-root symlinks; repository
    /// picker accepts only directories).
    pub fn try_select(&mut self) -> Result<Selection, SelectionError> {
        let dir = self.current_dir.clone();
        self.ensure_cached(&dir);

        let entry = match self.cache.get(&dir) {
            Some(DirListing::Entries(entries)) => match entries.get(self.selected) {
                Some(e) => e.clone(),
                None => return Err(SelectionError::NoEntry),
            },
            Some(DirListing::Error(e)) => {
                return Err(SelectionError::DirectoryError(e.clone()));
            }
            None => return Err(SelectionError::NoEntry),
        };

        // Reject non-UTF-8 names.
        if entry.is_lossy {
            return Err(SelectionError::NonUtf8(entry.display_name.clone()));
        }

        // Reject special files.
        if entry.kind == EntryKind::Special {
            return Err(SelectionError::SpecialFile(entry.display_name.clone()));
        }

        // Reject error entries (disappeared or unreadable).
        if entry.kind == EntryKind::Error {
            return Err(SelectionError::Disappeared(entry.display_name.clone()));
        }

        // Re-validate that the entry still exists on disk (handle races).
        let full_path = self.current_dir.join(&entry.name);
        match std::fs::symlink_metadata(&full_path) {
            Ok(meta) => {
                let ft = meta.file_type();
                let actual_kind = if ft.is_dir() {
                    EntryKind::Directory
                } else if ft.is_symlink() {
                    EntryKind::Symlink
                } else if ft.is_file() {
                    EntryKind::File
                } else {
                    return Err(SelectionError::SpecialFile(entry.display_name.clone()));
                };

                Ok(Selection {
                    path: full_path,
                    kind: actual_kind,
                    is_symlink: actual_kind == EntryKind::Symlink,
                    link_target: entry.link_target.clone(),
                })
            }
            Err(_) => {
                // Entry disappeared between listing and selection.
                Err(SelectionError::Disappeared(entry.display_name.clone()))
            }
        }
    }
}

/// Result of a successful entry selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Full path of the selected entry.
    pub path: PathBuf,
    /// Kind of the entry at selection time.
    pub kind: EntryKind,
    /// Whether the entry is a symbolic link.
    pub is_symlink: bool,
    /// Raw link target if it is a symlink.
    pub link_target: Option<String>,
}

/// Reasons a selection may be rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectionError {
    /// No entry at the current selection index.
    NoEntry,
    /// The file name is not valid UTF-8 and cannot be stored in configuration.
    NonUtf8(String),
    /// The entry is a special file (socket, device, FIFO) and cannot be selected.
    SpecialFile(String),
    /// The entry disappeared or became unreadable since listing.
    Disappeared(String),
    /// The directory listing itself failed.
    DirectoryError(String),
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEntry => write!(f, "No entry selected"),
            Self::NonUtf8(name) => write!(
                f,
                "'{name}' contains non-UTF-8 characters and cannot be used"
            ),
            Self::SpecialFile(name) => {
                write!(f, "'{name}' is a special file and cannot be selected")
            }
            Self::Disappeared(name) => {
                write!(f, "'{name}' no longer exists or is unreadable")
            }
            Self::DirectoryError(e) => write!(f, "Directory error: {e}"),
        }
    }
}

/// Read a regular file through `cat` for display in the picker preview.
fn load_file_preview(path: &Path) -> Result<FilePreview, String> {
    // O_NOFOLLOW preserves picker traversal safety if the selected path is
    // replaced by a symlink, while O_NONBLOCK prevents a raced special file
    // from blocking during open. Validate the opened object before giving its
    // descriptor to `cat`.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|error| format!("Cannot open file: {error}"))?;
    let metadata = file
        .metadata()
        .map_err(|error| format!("Cannot inspect file: {error}"))?;
    if !metadata.file_type().is_file() {
        return Err("Entry is no longer a regular file".to_string());
    }
    if metadata.len() > MAX_FILE_PREVIEW_BYTES {
        return Err(format!(
            "Content preview unavailable: file exceeds {} KB",
            MAX_FILE_PREVIEW_BYTES / 1024
        ));
    }

    let output = Command::new("cat")
        .arg("--")
        .stdin(Stdio::from(file))
        .output()
        .map_err(|error| format!("Cannot run cat: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(if detail.is_empty() {
            format!("cat exited with {}", output.status)
        } else {
            format!("cat failed: {detail}")
        });
    }

    let limit = MAX_FILE_PREVIEW_BYTES as usize;
    let truncated = output.stdout.len() > limit;
    let bytes = &output.stdout[..output.stdout.len().min(limit)];
    let content = String::from_utf8_lossy(bytes)
        .chars()
        .map(|character| {
            if character == '\n' || character == '\t' || !character.is_control() {
                character
            } else {
                '�'
            }
        })
        .collect();

    Ok(FilePreview { content, truncated })
}

/// Read a directory and return a sorted listing.
///
/// Uses `symlink_metadata` to avoid following symlinks.
/// Sorts: directories first, then symlinks, then files, then special/error.
/// Within each group, sorts alphabetically case-insensitive.
pub fn read_directory(path: &Path) -> DirListing {
    let read_dir = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(e) => return DirListing::Error(format!("Cannot read directory: {e}")),
    };

    let mut entries = Vec::new();
    for result in read_dir {
        let dir_entry = match result {
            Ok(de) => de,
            Err(e) => {
                // Individual entry errors are included as Error entries.
                entries.push(Entry {
                    name: OsString::from(format!("<error: {e}>")),
                    display_name: format!("<error: {e}>"),
                    is_lossy: false,
                    kind: EntryKind::Error,
                    hidden: false,
                    is_git_repository: false,
                    size: None,
                    executable: false,
                    link_target: None,
                });
                continue;
            }
        };

        let name = dir_entry.file_name();
        // Git metadata is never a useful picker target and exposing its internals
        // makes a selected repository look like an ordinary directory tree.
        if name == ".git" {
            continue;
        }
        let display_name = name.to_string_lossy().to_string();
        let is_lossy = name.to_str().is_none();
        let hidden = display_name.starts_with('.');

        // Use symlink_metadata (no follow).
        let (kind, size, executable, link_target) =
            match std::fs::symlink_metadata(path.join(&name)) {
                Ok(meta) => {
                    let ft = meta.file_type();
                    let kind = if ft.is_dir() {
                        EntryKind::Directory
                    } else if ft.is_symlink() {
                        EntryKind::Symlink
                    } else if ft.is_file() {
                        EntryKind::File
                    } else {
                        EntryKind::Special
                    };

                    let size = if ft.is_file() { Some(meta.len()) } else { None };

                    #[cfg(unix)]
                    let executable = {
                        use std::os::unix::fs::PermissionsExt;
                        ft.is_file() && (meta.permissions().mode() & 0o111) != 0
                    };
                    #[cfg(not(unix))]
                    let executable = false;

                    let link_target = if ft.is_symlink() {
                        std::fs::read_link(path.join(&name))
                            .ok()
                            .map(|t| t.to_string_lossy().to_string())
                    } else {
                        None
                    };

                    (kind, size, executable, link_target)
                }
                Err(_) => (EntryKind::Error, None, false, None),
            };

        let is_git_repository = kind == EntryKind::Directory
            && std::fs::symlink_metadata(path.join(&name).join(".git")).is_ok_and(|metadata| {
                let file_type = metadata.file_type();
                file_type.is_dir() || file_type.is_file()
            });

        entries.push(Entry {
            name,
            display_name,
            is_lossy,
            kind,
            hidden,
            is_git_repository,
            size,
            executable,
            link_target,
        });
    }

    sort_entries(&mut entries);
    DirListing::Entries(entries)
}

/// Sort entries deterministically: directories first, then symlinks, then files,
/// then special, then errors. Within each group, sort case-insensitive alphabetically.
fn sort_entries(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        a.kind
            .cmp(&b.kind)
            .then_with(|| {
                a.display_name
                    .to_lowercase()
                    .cmp(&b.display_name.to_lowercase())
            })
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
}

#[cfg(test)]
#[path = "../../tests/unit/tui/browser.rs"]
mod tests;
