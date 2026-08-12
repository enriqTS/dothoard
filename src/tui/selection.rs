//! Multi-select state model for source browser.
//!
//! Tracks which filesystem paths are selected as sources, which are inherited
//! from a parent folder source, and which have been explicitly deselected
//! (creating ignore rule candidates). Operates on absolute paths internally
//! and converts to/from home-relative config paths at the boundaries.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::SourceConfig;

/// Visual and logical state of a path in the multi-select browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    /// The path is explicitly selected as a source.
    Explicit,
    /// The path is inside a selected folder source (auto-selected).
    Inherited,
    /// The path is not selected.
    Unchecked,
}

/// The result of diffing the current selection against the persisted config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectionDiff {
    /// New source paths to add (home-relative).
    pub additions: Vec<String>,
    /// Existing source paths to remove (home-relative).
    pub removals: Vec<String>,
    /// New ignore rules to append, keyed by the source path (home-relative)
    /// they belong to. Values are anchored relative paths (e.g., `/subfolder/`).
    pub ignore_rules: HashMap<String, Vec<String>>,
}

/// Multi-selection state for the source browser.
///
/// All paths stored internally are absolute. Conversion to/from home-relative
/// strings happens at load and diff boundaries.
#[derive(Debug, Clone)]
pub struct SourceSelection {
    /// Paths explicitly selected as sources.
    selected: HashSet<PathBuf>,
    /// For each selected folder source, the relative paths (within that source)
    /// that have been explicitly deselected — these become ignore rules.
    deselected: HashMap<PathBuf, Vec<String>>,
    /// The home directory root used for path resolution.
    home: PathBuf,
}

impl SourceSelection {
    /// Create an empty selection rooted at the given home directory.
    pub fn new(home: &Path) -> Self {
        Self {
            selected: HashSet::new(),
            deselected: HashMap::new(),
            home: home.to_path_buf(),
        }
    }

    /// Load existing source configuration into the selection state.
    ///
    /// Each configured source becomes an explicitly selected entry.
    pub fn load_from_config(&mut self, sources: &[SourceConfig]) {
        self.selected.clear();
        self.deselected.clear();
        for source in sources {
            let abs = self.home.join(&source.path);
            self.selected.insert(abs);
        }
    }

    /// Query the check state of an absolute path.
    ///
    /// Walks up the path hierarchy to detect inheritance from a selected
    /// ancestor. A path that appears in the deselected list of its ancestor
    /// is reported as `Unchecked`. If a parent directory of the path is
    /// deselected (directory rule with trailing `/`), all children beneath
    /// it are also `Unchecked`.
    pub fn is_selected(&self, path: &Path) -> CheckState {
        // Check if explicitly selected.
        if self.selected.contains(path) {
            return CheckState::Explicit;
        }

        // Walk ancestors to find a selected parent folder.
        if let Some((ancestor, relative)) = self.find_selected_ancestor(path) {
            // Check if explicitly deselected within that ancestor.
            if let Some(deselected_list) = self.deselected.get(&ancestor) {
                // Match the exact path (with and without trailing slash).
                let rel_dir = format!("{relative}/");
                if deselected_list.contains(&relative) || deselected_list.contains(&rel_dir) {
                    return CheckState::Unchecked;
                }

                // Check if any deselected directory is a parent of this path.
                // E.g., if "completions/" is deselected and we're querying
                // "completions/git.fish", the child is also unchecked.
                for deselected in deselected_list {
                    if deselected.ends_with('/') && relative.starts_with(deselected.as_str()) {
                        return CheckState::Unchecked;
                    }
                }
            }
            return CheckState::Inherited;
        }

        CheckState::Unchecked
    }

    /// Toggle the selection state of an absolute path.
    ///
    /// - `Unchecked` → `Explicit` (add to selected set)
    /// - `Explicit` → `Unchecked` (remove from selected set, clear its deselected list)
    /// - `Inherited` → `Unchecked` (add to ancestor's deselected list)
    /// - `Unchecked` but in deselected list → `Inherited` (remove from deselected list)
    ///
    /// When `is_dir` is true and the path is inherited, the deselected entry
    /// gets a trailing `/` to generate a directory-matching ignore rule.
    pub fn toggle(&mut self, path: &Path, is_dir: bool) {
        match self.is_selected(path) {
            CheckState::Explicit => {
                self.selected.remove(path);
                self.deselected.remove(path);
            }
            CheckState::Inherited => {
                // Add to the ancestor's deselected list.
                if let Some((ancestor, mut relative)) = self.find_selected_ancestor(path) {
                    if is_dir && !relative.ends_with('/') {
                        relative.push('/');
                    }
                    self.deselected.entry(ancestor).or_default().push(relative);
                }
            }
            CheckState::Unchecked => {
                // Check if it's in a deselected list (toggle back to inherited).
                let restored = self.try_restore_from_deselected(path, is_dir);
                if !restored {
                    // Not inherited at all — add as explicit selection.
                    self.selected.insert(path.to_path_buf());
                }
            }
        }
    }

    /// Compute the diff between the current selection state and the given config.
    ///
    /// Returns additions (new sources), removals (unchecked existing sources),
    /// and new ignore rules to append per source.
    pub fn diff_against_config(&self, sources: &[SourceConfig]) -> SelectionDiff {
        let existing: HashSet<PathBuf> = sources.iter().map(|s| self.home.join(&s.path)).collect();

        // Additions: in selection but not in config.
        let additions: Vec<String> = self
            .selected
            .iter()
            .filter(|p| !existing.contains(*p))
            .filter_map(|p| self.to_home_relative(p))
            .collect();

        // Removals: in config but not in selection.
        let removals: Vec<String> = sources
            .iter()
            .filter(|s| !self.selected.contains(&self.home.join(&s.path)))
            .map(|s| s.path.clone())
            .collect();

        // Ignore rules: deselected entries for sources that exist in selection.
        let mut ignore_rules: HashMap<String, Vec<String>> = HashMap::new();
        for (ancestor_abs, deselected_paths) in &self.deselected {
            if let Some(rel) = self.to_home_relative(ancestor_abs) {
                // Only generate rules for sources that will remain in the config.
                if self.selected.contains(ancestor_abs) {
                    // Filter out rules that already exist in the config.
                    let existing_ignores: HashSet<&str> = sources
                        .iter()
                        .find(|s| s.path == rel)
                        .map(|s| s.ignore.iter().map(|i| i.as_str()).collect())
                        .unwrap_or_default();

                    let new_rules: Vec<String> = deselected_paths
                        .iter()
                        .map(|p| format!("/{p}"))
                        .filter(|rule| !existing_ignores.contains(rule.as_str()))
                        .collect();

                    if !new_rules.is_empty() {
                        ignore_rules.insert(rel, new_rules);
                    }
                }
            }
        }

        SelectionDiff {
            additions,
            removals,
            ignore_rules,
        }
    }

    /// Check if the diff has any changes.
    pub fn has_changes(&self, sources: &[SourceConfig]) -> bool {
        let diff = self.diff_against_config(sources);
        !diff.additions.is_empty() || !diff.removals.is_empty() || !diff.ignore_rules.is_empty()
    }

    /// Return a summary of the current selection state for display.
    ///
    /// Returns (selected_count, excluded_count) where selected_count is the
    /// number of explicitly selected sources and excluded_count is the total
    /// number of deselected paths across all sources.
    pub fn summary(&self) -> (usize, usize) {
        let selected = self.selected.len();
        let excluded: usize = self.deselected.values().map(|v| v.len()).sum();
        (selected, excluded)
    }

    /// Get the home directory.
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Find the nearest selected ancestor of the given path and return it
    /// along with the relative path from that ancestor to the given path.
    fn find_selected_ancestor(&self, path: &Path) -> Option<(PathBuf, String)> {
        let mut current = path.parent();
        while let Some(ancestor) = current {
            if self.selected.contains(ancestor) {
                // Compute relative path from ancestor to the original path.
                if let Ok(relative) = path.strip_prefix(ancestor) {
                    let rel_str = relative.to_string_lossy().to_string();
                    return Some((ancestor.to_path_buf(), rel_str));
                }
            }
            // Stop at home boundary.
            if ancestor == self.home {
                break;
            }
            current = ancestor.parent();
        }
        None
    }

    /// Try to remove a path from an ancestor's deselected list (restoring
    /// it to inherited state). Returns true if the path was found and removed.
    fn try_restore_from_deselected(&mut self, path: &Path, is_dir: bool) -> bool {
        // Walk ancestors to find one that has this path in its deselected list.
        let mut current = path.parent();
        while let Some(ancestor) = current {
            if self.selected.contains(ancestor) {
                if let Ok(relative) = path.strip_prefix(ancestor) {
                    let rel_str = relative.to_string_lossy().to_string();
                    // Try both with and without trailing slash.
                    let rel_dir = format!("{rel_str}/");
                    if let Some(list) = self.deselected.get_mut(ancestor) {
                        let search = if is_dir { &rel_dir } else { &rel_str };
                        // Also try the other variant for robustness.
                        if let Some(pos) = list
                            .iter()
                            .position(|p| p == search || p == &rel_str || p == &rel_dir)
                        {
                            list.remove(pos);
                            if list.is_empty() {
                                self.deselected.remove(ancestor);
                            }
                            return true;
                        }
                    }
                }
                break;
            }
            if ancestor == self.home {
                break;
            }
            current = ancestor.parent();
        }
        false
    }

    /// Convert an absolute path to a home-relative string.
    fn to_home_relative(&self, path: &Path) -> Option<String> {
        path.strip_prefix(&self.home)
            .ok()
            .map(|p| p.to_string_lossy().to_string())
    }
}

#[cfg(test)]
#[path = "../../tests/unit/tui/selection.rs"]
mod tests;
