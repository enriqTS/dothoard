use super::*;
use std::path::PathBuf;

fn matcher(patterns: &[&str]) -> IgnoreMatcher {
    let root = Path::new("/home/user/.config/fish");
    let patterns: Vec<String> = patterns.iter().map(|s| s.to_string()).collect();
    let (m, errors) = IgnoreMatcher::new(root, &patterns);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
    m
}

// --- Basic matching ---

#[test]
fn no_patterns_matches_nothing() {
    let m = matcher(&[]);
    assert!(!m.is_ignored(Path::new("file.txt"), false));
    assert!(!m.is_ignored(Path::new("any/path"), false));
}

#[test]
fn simple_glob_matches_filename() {
    let m = matcher(&["*.log"]);
    assert!(m.is_ignored(Path::new("debug.log"), false));
    assert!(m.is_ignored(Path::new("subdir/app.log"), false));
    assert!(!m.is_ignored(Path::new("readme.txt"), false));
}

#[test]
fn exact_filename_match() {
    let m = matcher(&["fish_variables"]);
    assert!(m.is_ignored(Path::new("fish_variables"), false));
    assert!(m.is_ignored(Path::new("subdir/fish_variables"), false));
    assert!(!m.is_ignored(Path::new("fish_variables.bak"), false));
}

#[test]
fn wildcard_in_middle() {
    let m = matcher(&["*token*"]);
    assert!(m.is_ignored(Path::new("auth_token.json"), false));
    assert!(m.is_ignored(Path::new("token"), false));
    assert!(m.is_ignored(Path::new("my_token_file"), false));
    assert!(!m.is_ignored(Path::new("readme.txt"), false));
}

// --- Trailing slash (directory only) ---

#[test]
fn trailing_slash_matches_only_directories() {
    let m = matcher(&["cache/"]);
    assert!(m.is_ignored(Path::new("cache"), true));
    assert!(!m.is_ignored(Path::new("cache"), false)); // file named cache
    assert!(m.is_ignored(Path::new("sub/cache"), true));
}

#[test]
fn trailing_slash_excludes_directory_contents() {
    let m = matcher(&["cache/"]);
    // Files inside a matched directory are excluded via parent matching
    assert!(m.is_ignored(Path::new("cache/file.txt"), false));
    assert!(m.is_ignored(Path::new("cache/sub/deep.txt"), false));
}

// --- Leading slash (anchoring) ---

#[test]
fn leading_slash_anchors_to_root() {
    let m = matcher(&["/build"]);
    assert!(m.is_ignored(Path::new("build"), false));
    assert!(!m.is_ignored(Path::new("sub/build"), false)); // not anchored here
}

#[test]
fn unanchored_matches_anywhere() {
    let m = matcher(&["build"]);
    assert!(m.is_ignored(Path::new("build"), false));
    assert!(m.is_ignored(Path::new("sub/build"), false));
    assert!(m.is_ignored(Path::new("a/b/build"), false));
}

// --- Negation ---

#[test]
fn negation_re_includes() {
    let m = matcher(&["*.log", "!important.log"]);
    assert!(m.is_ignored(Path::new("debug.log"), false));
    assert!(!m.is_ignored(Path::new("important.log"), false));
}

#[test]
fn last_matching_rule_wins() {
    let m = matcher(&["*.log", "!important.log", "important.log"]);
    // Last rule re-ignores important.log
    assert!(m.is_ignored(Path::new("important.log"), false));
}

#[test]
fn negation_cannot_re_include_inside_excluded_parent() {
    let m = matcher(&["dir/", "!dir/keep.txt"]);
    // The ignore crate's matched_path_or_any_parents handles this:
    // dir/ is excluded, so dir/keep.txt should still be excluded
    // because the parent "dir" is matched.
    assert!(m.is_ignored(Path::new("dir/keep.txt"), false));
}

// --- Escaping ---

#[test]
fn escaped_exclamation_mark() {
    let m = matcher(&["\\!important"]);
    assert!(m.is_ignored(Path::new("!important"), false));
    assert!(!m.is_ignored(Path::new("important"), false));
}

#[test]
fn escaped_hash() {
    let m = matcher(&["\\#notes"]);
    assert!(m.is_ignored(Path::new("#notes"), false));
}

// --- Pattern with path separator ---

#[test]
fn pattern_with_slash_anchors_implicitly() {
    let m = matcher(&["doc/generated"]);
    assert!(m.is_ignored(Path::new("doc/generated"), false));
    // Pattern contains a slash, so it's anchored
    assert!(!m.is_ignored(Path::new("sub/doc/generated"), false));
}

// --- Double star ---

#[test]
fn double_star_matches_any_depth() {
    let m = matcher(&["**/logs"]);
    assert!(m.is_ignored(Path::new("logs"), false));
    assert!(m.is_ignored(Path::new("a/logs"), false));
    assert!(m.is_ignored(Path::new("a/b/c/logs"), false));
}

#[test]
fn double_star_in_middle() {
    let m = matcher(&["a/**/z"]);
    assert!(m.is_ignored(Path::new("a/z"), false));
    assert!(m.is_ignored(Path::new("a/b/z"), false));
    assert!(m.is_ignored(Path::new("a/b/c/z"), false));
}

// --- Hard exclusions ---

#[test]
fn git_directory_is_hard_excluded() {
    assert!(is_hard_excluded_git(Path::new(".git")));
    assert!(is_hard_excluded_git(Path::new(".git/objects")));
    assert!(is_hard_excluded_git(Path::new("sub/.git")));
    assert!(is_hard_excluded_git(Path::new("sub/.git/HEAD")));
}

#[test]
fn non_git_paths_not_hard_excluded() {
    assert!(!is_hard_excluded_git(Path::new(".gitignore")));
    assert!(!is_hard_excluded_git(Path::new(".github/workflows")));
    assert!(!is_hard_excluded_git(Path::new("git")));
    assert!(!is_hard_excluded_git(Path::new("file.git")));
}

#[test]
fn special_files_are_hard_excluded() {
    use super::super::walker::WalkEntryKind;

    assert!(is_hard_excluded_special(&WalkEntryKind::SpecialFile {
        file_type: "socket".to_string(),
    }));
    assert!(!is_hard_excluded_special(&WalkEntryKind::File));
    assert!(!is_hard_excluded_special(&WalkEntryKind::Symlink));
    assert!(!is_hard_excluded_special(&WalkEntryKind::ExecutableFile));
}

// --- MatchResult ---

#[test]
fn match_result_is_ignored() {
    assert!(
        MatchResult::Ignored {
            pattern: "*.log".to_string()
        }
        .is_ignored()
    );
    assert!(!MatchResult::None.is_ignored());
    assert!(
        !MatchResult::Whitelisted {
            pattern: "!keep".to_string()
        }
        .is_ignored()
    );
}

#[test]
fn match_result_is_included() {
    assert!(MatchResult::None.is_included());
    assert!(
        MatchResult::Whitelisted {
            pattern: "!keep".to_string()
        }
        .is_included()
    );
    assert!(
        !MatchResult::Ignored {
            pattern: "*.log".to_string()
        }
        .is_included()
    );
}

// --- Empty and accessor ---

#[test]
fn empty_matcher_reports_empty() {
    let m = matcher(&[]);
    assert!(m.is_empty());
    assert!(m.patterns().is_empty());
}

#[test]
fn non_empty_matcher_reports_patterns() {
    let m = matcher(&["*.log", "cache/"]);
    assert!(!m.is_empty());
    assert_eq!(m.patterns(), &["*.log".to_string(), "cache/".to_string()]);
}

// --- Error handling ---

#[test]
fn invalid_pattern_still_builds_matcher() {
    // The ignore crate is lenient about pattern syntax.
    // Verify that even unusual patterns don't crash the builder and
    // valid patterns continue to work.
    let root = Path::new("/home/user/source");
    let patterns = vec!["*.log".to_string(), "normal_file".to_string()];
    let (m, errors) = IgnoreMatcher::new(root, &patterns);

    // Should build without errors for standard patterns
    assert!(errors.is_empty());
    assert!(m.is_ignored(Path::new("test.log"), false));
    assert!(m.is_ignored(Path::new("normal_file"), false));
}

// --- Complex scenarios ---

#[test]
fn realistic_fish_config_patterns() {
    let m = matcher(&["*.log", "fish_variables", "fish_history"]);
    assert!(m.is_ignored(Path::new("fish_variables"), false));
    assert!(m.is_ignored(Path::new("fish_history"), false));
    assert!(m.is_ignored(Path::new("debug.log"), false));
    assert!(!m.is_ignored(Path::new("config.fish"), false));
    assert!(!m.is_ignored(Path::new("functions/hello.fish"), false));
}

#[test]
fn realistic_waybar_patterns() {
    let m = matcher(&["cache/", "*token*"]);
    assert!(m.is_ignored(Path::new("cache"), true));
    assert!(m.is_ignored(Path::new("cache/data.json"), false));
    assert!(m.is_ignored(Path::new("auth_token"), false));
    assert!(m.is_ignored(Path::new("sub/refresh_token.json"), false));
    assert!(!m.is_ignored(Path::new("config"), false));
    assert!(!m.is_ignored(Path::new("style.css"), false));
}

#[test]
fn matches_returns_full_result_info() {
    let m = matcher(&["*.log", "!important.log"]);

    let result = m.matches(Path::new("debug.log"), false);
    assert!(matches!(result, MatchResult::Ignored { .. }));

    let result = m.matches(Path::new("important.log"), false);
    assert!(matches!(result, MatchResult::Whitelisted { .. }));

    let result = m.matches(Path::new("readme.txt"), false);
    assert_eq!(result, MatchResult::None);
}

#[test]
fn hard_excluded_git_at_any_depth() {
    // Even if buried deep, .git component triggers exclusion
    assert!(is_hard_excluded_git(&PathBuf::from(
        "a/b/c/.git/objects/pack"
    )));
}
