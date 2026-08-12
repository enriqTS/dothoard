//! Ignore pattern matching for backup sources.
//!
//! Implements `.gitignore`-style matching semantics rooted at the configured
//! source directory. Key behaviors:
//!
//! - Rules are evaluated in order; the last matching rule wins.
//! - Leading `/` anchors a pattern to the source root.
//! - Trailing `/` restricts a pattern to directories only.
//! - `!` prefix negates a pattern (re-includes a previously excluded path).
//! - `\` escapes special characters (`!`, `#`, leading spaces).
//! - A child cannot be re-included while its parent directory remains excluded.
//! - Nested `.git` directories and unsupported special files are hard
//!   exclusions that cannot be negated by user patterns.
//!
//! Only rules from the application configuration are evaluated. `.gitignore`
//! files found inside a source are treated as ordinary files.

use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// Result of matching a path against ignore rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchResult {
    /// The path is not matched by any rule (included in backup).
    None,

    /// The path is excluded by a user-configured pattern.
    Ignored {
        /// The pattern that caused the exclusion.
        pattern: String,
    },

    /// The path was excluded but then re-included by a negation pattern.
    /// This is tracked for informational purposes.
    Whitelisted {
        /// The negation pattern that re-included the path.
        pattern: String,
    },
}

impl MatchResult {
    /// Returns `true` if the path should be excluded from backup.
    pub fn is_ignored(&self) -> bool {
        matches!(self, Self::Ignored { .. })
    }

    /// Returns `true` if the path is included (either not matched or whitelisted).
    pub fn is_included(&self) -> bool {
        !self.is_ignored()
    }
}

/// A compiled set of ignore patterns for one source directory.
///
/// Wraps the `ignore` crate's gitignore matching with our specific semantics:
/// ordered evaluation, last-match-wins, and hard exclusion awareness.
#[derive(Debug)]
pub struct IgnoreMatcher {
    /// The compiled gitignore rules.
    gitignore: Gitignore,

    /// The original patterns (kept for diagnostic messages).
    patterns: Vec<String>,
}

/// Errors from building an ignore matcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnorePatternError {
    /// The pattern that failed to parse.
    pub pattern: String,
    /// The line number (0-indexed position in the pattern list).
    pub line: usize,
    /// Human-readable error description.
    pub message: String,
}

impl std::fmt::Display for IgnorePatternError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid ignore pattern at line {}: \"{}\": {}",
            self.line, self.pattern, self.message
        )
    }
}

impl std::error::Error for IgnorePatternError {}

impl IgnoreMatcher {
    /// Build a matcher from a list of patterns, rooted at the given source directory.
    ///
    /// The `source_root` is the absolute path to the source directory. Patterns
    /// with a leading `/` are anchored relative to this root.
    ///
    /// Returns errors for patterns that cannot be parsed but still builds a
    /// matcher from the valid patterns.
    pub fn new(source_root: &Path, patterns: &[String]) -> (Self, Vec<IgnorePatternError>) {
        let mut builder = GitignoreBuilder::new(source_root);
        let mut errors = Vec::new();

        for (line, pattern) in patterns.iter().enumerate() {
            if let Err(err) = builder.add_line(None, pattern) {
                errors.push(IgnorePatternError {
                    pattern: pattern.clone(),
                    line,
                    message: err.to_string(),
                });
            }
        }

        let gitignore = builder.build().unwrap_or_else(|_| {
            // Fallback: empty matcher if build somehow fails.
            GitignoreBuilder::new(source_root).build().unwrap()
        });

        let matcher = Self {
            gitignore,
            patterns: patterns.to_vec(),
        };

        (matcher, errors)
    }

    /// Match a path against the ignore rules.
    ///
    /// The `path` should be relative to the source root. The `is_dir` flag
    /// indicates whether the path is a directory (needed for trailing-slash rules).
    ///
    /// Enforces the Git rule that a child cannot be re-included while its
    /// parent directory remains excluded.
    pub fn matches(&self, path: &Path, is_dir: bool) -> MatchResult {
        // First check if any parent directory is excluded.
        // If a parent is excluded (and not whitelisted), the child cannot be
        // re-included regardless of negation patterns.
        let mut current = PathBuf::new();
        for component in path.parent().iter().flat_map(|p| p.components()) {
            current.push(component);
            let parent_match = self.gitignore.matched_path_or_any_parents(&current, true);
            if let ignore::Match::Ignore(glob) = parent_match {
                return MatchResult::Ignored {
                    pattern: glob.original().to_string(),
                };
            }
        }

        // No parent is excluded — check the path itself.
        let matched = self.gitignore.matched_path_or_any_parents(path, is_dir);

        match matched {
            ignore::Match::None => MatchResult::None,
            ignore::Match::Ignore(glob) => MatchResult::Ignored {
                pattern: glob.original().to_string(),
            },
            ignore::Match::Whitelist(glob) => MatchResult::Whitelisted {
                pattern: glob.original().to_string(),
            },
        }
    }

    /// Check if a path should be excluded from backup.
    ///
    /// Convenience method that returns `true` for excluded paths.
    pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        self.matches(path, is_dir).is_ignored()
    }

    /// Returns the original pattern list.
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// Returns `true` if no patterns are configured.
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }
}

/// Check if a path component represents a nested `.git` directory.
///
/// This is a hard exclusion that cannot be negated by user patterns.
pub fn is_hard_excluded_git(relative_path: &Path) -> bool {
    relative_path.components().any(|c| c.as_os_str() == ".git")
}

/// Check if a walk entry represents an unsupported special file.
///
/// This is a hard exclusion that cannot be negated by user patterns.
pub fn is_hard_excluded_special(kind: &super::walker::WalkEntryKind) -> bool {
    matches!(kind, super::walker::WalkEntryKind::SpecialFile { .. })
}

#[cfg(test)]
#[path = "../../tests/unit/backup/ignore.rs"]
mod tests;
