use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn key_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, modifiers)
}

#[test]
fn first_run_has_no_implicit_namespace() {
    let screen = RepoScreen::new();
    assert!(screen.namespace_input.is_empty());
    assert!(screen.namespace_origin.is_empty());
}

#[test]
fn discovery_without_an_active_namespace_does_not_invent_one() {
    let tmp = tempfile::tempdir().unwrap();
    let mut screen = RepoScreen::new();
    screen.refresh_namespaces(tmp.path(), "").unwrap();
    assert!(screen.namespaces.is_empty());
}

#[test]
fn discovers_active_sibling_and_unsafe_namespace_states() {
    use crate::backup::manifest::Manifest;

    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("desktop")).unwrap();
    Manifest::from_sources("desktop", &[])
        .save(&tmp.path().join("desktop"))
        .unwrap();
    std::fs::create_dir_all(tmp.path().join("notebook/home")).unwrap();
    std::fs::write(tmp.path().join("notebook/home/config"), "data").unwrap();
    std::fs::create_dir_all(tmp.path().join("broken")).unwrap();
    std::fs::write(
        tmp.path().join("broken/.dothoard-manifest.toml"),
        "not valid toml",
    )
    .unwrap();

    let mut screen = RepoScreen::new();
    screen.refresh_namespaces(tmp.path(), "desktop").unwrap();

    assert_eq!(screen.namespaces.len(), 3);
    assert!(screen.namespaces.iter().any(|item| item.active
        && item.name == "desktop"
        && matches!(item.ownership, OwnershipInfo::Owned { .. })));
    assert!(screen.namespaces.iter().any(
        |item| item.name == "notebook" && matches!(item.ownership, OwnershipInfo::Ambiguous(_))
    ));
    assert!(
        screen.namespaces.iter().any(|item| item.name == "broken"
            && matches!(item.ownership, OwnershipInfo::InvalidManifest(_)))
    );
}

#[test]
fn discovery_includes_missing_active_namespace_as_new() {
    let tmp = tempfile::tempdir().unwrap();
    let mut screen = RepoScreen::new();
    screen
        .refresh_namespaces(tmp.path(), "new-machine")
        .unwrap();

    assert_eq!(screen.namespaces.len(), 1);
    assert!(screen.namespaces[0].active);
    assert!(matches!(screen.namespaces[0].ownership, OwnershipInfo::New));
}

#[test]
fn namespace_list_selects_rows_and_only_edits_active_namespace() {
    let mut screen = RepoScreen::new();
    screen.namespaces = vec![
        NamespaceSummary {
            name: "desktop".to_string(),
            ownership: OwnershipInfo::Owned { sources: vec![] },
            active: true,
        },
        NamespaceSummary {
            name: "notebook".to_string(),
            ownership: OwnershipInfo::Owned { sources: vec![] },
            active: false,
        },
    ];
    screen.mode = RepoMode::Namespaces;
    screen.handle_key(key(KeyCode::Down));
    assert_eq!(screen.namespace_selected, 1);
    screen.handle_key(key(KeyCode::Char('r')));
    assert_eq!(screen.mode, RepoMode::Namespaces);
    screen.handle_key(key(KeyCode::Enter));
    assert_eq!(screen.mode, RepoMode::NamespaceInput);
    assert_eq!(screen.namespace_input, "notebook");
}

#[test]
fn namespace_list_does_not_offer_unsafe_entry_for_selection() {
    let mut screen = RepoScreen::new();
    screen.namespaces = vec![NamespaceSummary {
        name: "ambiguous".to_string(),
        ownership: OwnershipInfo::Ambiguous("missing manifest".to_string()),
        active: false,
    }];
    screen.mode = RepoMode::Namespaces;

    let result = screen.handle_key(key(KeyCode::Enter));

    assert_eq!(result, KeyResult::Consumed);
    assert_eq!(screen.mode, RepoMode::Namespaces);
}

#[test]
fn new_screen_is_empty() {
    let screen = RepoScreen::new();
    assert!(screen.input.is_empty());
    assert_eq!(screen.cursor, 0);
    assert!(matches!(screen.validation, LoadState::NotLoaded));
}

#[test]
fn with_path_prefills_input() {
    let screen = RepoScreen::with_path("~/dotfiles");
    assert_eq!(screen.input, "~/dotfiles");
    assert_eq!(screen.cursor, 10);
}

#[test]
fn typing_inserts_characters() {
    let mut screen = RepoScreen::new();
    screen.mode = RepoMode::TextInput;
    screen.handle_key(key(KeyCode::Char('/')));
    screen.handle_key(key(KeyCode::Char('t')));
    screen.handle_key(key(KeyCode::Char('m')));
    screen.handle_key(key(KeyCode::Char('p')));
    assert_eq!(screen.input, "/tmp");
    assert_eq!(screen.cursor, 4);
}

#[test]
fn repository_text_input_handles_multibyte_characters() {
    let mut screen = RepoScreen::new();
    screen.mode = RepoMode::TextInput;
    screen.handle_key(key(KeyCode::Char('界')));
    screen.handle_key(key(KeyCode::Char('é')));
    screen.handle_key(key(KeyCode::Left));
    screen.handle_key(key(KeyCode::Backspace));
    assert_eq!(screen.input, "é");
    assert_eq!(screen.cursor, 0);
    screen.handle_key(key(KeyCode::Delete));
    assert!(screen.input.is_empty());
    assert_eq!(screen.cursor, 0);
}

#[test]
fn backspace_deletes_before_cursor() {
    let mut screen = RepoScreen::with_path("/tmp");
    screen.mode = RepoMode::TextInput;
    screen.handle_key(key(KeyCode::Backspace));
    assert_eq!(screen.input, "/tm");
    assert_eq!(screen.cursor, 3);
}

#[test]
fn left_right_moves_cursor() {
    let mut screen = RepoScreen::with_path("/tmp");
    screen.mode = RepoMode::TextInput;
    screen.handle_key(key(KeyCode::Left));
    assert_eq!(screen.cursor, 3);
    screen.handle_key(key(KeyCode::Left));
    assert_eq!(screen.cursor, 2);
    screen.handle_key(key(KeyCode::Right));
    assert_eq!(screen.cursor, 3);
}

#[test]
fn home_end_jump_cursor() {
    let mut screen = RepoScreen::with_path("/home/user/repo");
    screen.mode = RepoMode::TextInput;
    screen.handle_key(key(KeyCode::Home));
    assert_eq!(screen.cursor, 0);
    screen.handle_key(key(KeyCode::End));
    assert_eq!(screen.cursor, 15);
}

#[test]
fn ctrl_u_clears_input() {
    let mut screen = RepoScreen::with_path("/some/path");
    screen.mode = RepoMode::TextInput;
    screen.handle_key(key_mod(KeyCode::Char('u'), KeyModifiers::CONTROL));
    assert!(screen.input.is_empty());
    assert_eq!(screen.cursor, 0);
}

#[test]
fn enter_returns_validate() {
    let mut screen = RepoScreen::with_path("/tmp");
    screen.mode = RepoMode::TextInput;
    let result = screen.handle_key(key(KeyCode::Enter));
    assert_eq!(result, KeyResult::Validate);
}

#[test]
fn validate_rejects_empty_path() {
    let error = RepoScreen::validate_path("", Path::new("/home/test"), "desktop", "origin", 120)
        .unwrap_err();
    assert!(error.contains("empty"));
}

#[test]
fn validate_rejects_relative_path() {
    let error = RepoScreen::validate_path(
        "relative/path",
        Path::new("/home/test"),
        "desktop",
        "origin",
        120,
    )
    .unwrap_err();
    assert!(error.contains("absolute"));
}

#[test]
fn validate_rejects_nonexistent_path() {
    let error = RepoScreen::validate_path(
        "/nonexistent/path/12345",
        Path::new("/home/test"),
        "desktop",
        "origin",
        120,
    )
    .unwrap_err();
    assert!(error.contains("does not exist"));
}

#[test]
fn validate_rejects_non_repo_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let error = RepoScreen::validate_path(
        tmp.path().to_str().unwrap(),
        Path::new("/home/test"),
        "desktop",
        "origin",
        120,
    )
    .unwrap_err();
    assert!(
        error.contains("repository") || error.contains("git"),
        "unexpected message: {error}"
    );
}

#[test]
fn validate_accepts_valid_repo() {
    // Create a minimal git repo.
    let tmp = tempfile::tempdir().unwrap();
    let repo = tmp.path();
    std::process::Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(repo)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args(["remote", "add", "origin", "/dev/null"])
        .current_dir(repo)
        .output()
        .unwrap();
    // Make an initial commit so HEAD exists.
    std::process::Command::new("git")
        .args(["commit", "--allow-empty", "-m", "init"])
        .current_dir(repo)
        .output()
        .unwrap();

    let info = RepoScreen::validate_path(
        repo.to_str().unwrap(),
        Path::new("/home/test"),
        "desktop",
        "origin",
        120,
    )
    .unwrap();
    assert_eq!(info.branch, "main");
    assert!(matches!(info.ownership, OwnershipInfo::New));
}

#[test]
fn expand_tilde_expands_home() {
    let home = Path::new("/home/user");
    assert_eq!(
        expand_tilde("~/repo", home),
        PathBuf::from("/home/user/repo")
    );
    assert_eq!(expand_tilde("~", home), PathBuf::from("/home/user"));
    assert_eq!(expand_tilde("/abs/path", home), PathBuf::from("/abs/path"));
}

#[test]
fn confirm_rejects_preserved_but_non_current_validation() {
    let mut screen = RepoScreen::new();
    screen.validation = LoadState::Stale {
        previous: Some(RepoInfo {
            path: PathBuf::from("/repo"),
            branch: "main".to_string(),
            ownership: OwnershipInfo::New,
        }),
    };

    let error = screen
        .confirm(Path::new("/home/test"), "desktop")
        .unwrap_err();

    assert!(error.contains("current"));
}

#[test]
fn confirm_dialog_y_confirms() {
    let mut screen = RepoScreen::new();
    screen.confirm_state = ConfirmState::AskInitialize;
    let result = screen.handle_key(key(KeyCode::Char('y')));
    assert_eq!(result, KeyResult::Confirm);
}

#[test]
fn confirm_dialog_n_cancels() {
    let mut screen = RepoScreen::new();
    screen.confirm_state = ConfirmState::AskAttach;
    let result = screen.handle_key(key(KeyCode::Char('n')));
    assert_eq!(result, KeyResult::Consumed);
    assert_eq!(screen.confirm_state, ConfirmState::None);
}

#[test]
fn confirm_dialog_esc_cancels() {
    let mut screen = RepoScreen::new();
    screen.confirm_state = ConfirmState::AskInitialize;
    let result = screen.handle_key(key(KeyCode::Esc));
    assert_eq!(result, KeyResult::Consumed);
    assert_eq!(screen.confirm_state, ConfirmState::None);
}

// --- Browser mode tests ---

#[test]
fn namespace_input_submits_only_after_confirmation() {
    let mut screen = RepoScreen::new();
    screen.begin_namespace(NamespaceAction::Rename, "desktop");
    assert_eq!(screen.handle_key(key(KeyCode::Enter)), KeyResult::Consumed);
    assert!(screen.namespace_confirmation.is_some());
    assert_eq!(
        screen.handle_key(key(KeyCode::Char('n'))),
        KeyResult::Consumed
    );
    assert!(screen.namespace_confirmation.is_none());
}

#[test]
fn namespace_input_handles_multibyte_characters_without_panicking() {
    let mut screen = RepoScreen::new();
    screen.begin_namespace(NamespaceAction::SelectOrCreate, "é界");
    screen.handle_key(key(KeyCode::Left));
    screen.handle_key(key(KeyCode::Backspace));
    assert_eq!(screen.namespace_input, "界");
    assert_eq!(screen.namespace_cursor, 0);
    screen.handle_key(key(KeyCode::Delete));
    assert!(screen.namespace_input.is_empty());
}

#[test]
fn namespace_input_yields_namespace_action() {
    let mut screen = RepoScreen::new();
    screen.begin_namespace(NamespaceAction::SelectOrCreate, "desktop");
    assert_eq!(screen.handle_key(key(KeyCode::Enter)), KeyResult::Consumed);
    assert_eq!(
        screen.handle_key(key(KeyCode::Char('y'))),
        KeyResult::Namespace
    );
}

#[test]
fn new_screen_defaults_to_browser_mode() {
    let screen = RepoScreen::new();
    assert_eq!(screen.mode, RepoMode::Browser);
}

#[test]
fn colon_switches_to_text_input() {
    let mut screen = RepoScreen::new();
    screen.ensure_browser(Path::new("/tmp"));
    let result = screen.handle_key(key(KeyCode::Char(':')));
    assert_eq!(result, KeyResult::Consumed);
    assert_eq!(screen.mode, RepoMode::TextInput);
}

#[test]
fn slash_switches_to_text_input() {
    let mut screen = RepoScreen::new();
    screen.ensure_browser(Path::new("/tmp"));
    let result = screen.handle_key(key(KeyCode::Char('/')));
    assert_eq!(result, KeyResult::Consumed);
    assert_eq!(screen.mode, RepoMode::TextInput);
}

#[test]
fn esc_in_text_input_returns_to_browser() {
    let mut screen = RepoScreen::new();
    screen.mode = RepoMode::TextInput;
    let result = screen.handle_key(key(KeyCode::Esc));
    assert_eq!(result, KeyResult::Consumed);
    assert_eq!(screen.mode, RepoMode::Browser);
}

#[test]
fn browser_ensure_creates_browser() {
    let mut screen = RepoScreen::new();
    assert!(screen.browser.is_none());
    screen.ensure_browser(Path::new("/tmp"));
    assert!(screen.browser.is_some());
}

#[test]
fn browser_space_on_directory_validates() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir(tmp.path().join("subdir")).unwrap();

    let mut screen = RepoScreen::new();
    screen.ensure_browser(tmp.path());
    // Navigate browser to start at the temp dir.
    if let Some(ref mut browser) = screen.browser {
        browser.navigate_to(tmp.path());
        let _ = browser.current_listing();
        // Select the subdir (should be index 0 since it's the only entry).
    }

    let result = screen.handle_key(key(KeyCode::Char(' ')));
    // Should try to validate a directory.
    assert_eq!(result, KeyResult::Validate);
}

#[test]
fn browser_rejects_file_selection() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "x").unwrap();

    let mut screen = RepoScreen::new();
    screen.ensure_browser(tmp.path());
    if let Some(ref mut browser) = screen.browser {
        browser.navigate_to(tmp.path());
        let _ = browser.current_listing();
    }

    let result = screen.handle_key(key(KeyCode::Char(' ')));
    // Should not validate — only dirs allowed.
    assert_eq!(result, KeyResult::Consumed);
    assert!(screen.selection_error.is_some());
}
