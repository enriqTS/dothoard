use super::*;
use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

/// Create a test directory structure.
fn setup_test_dir() -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    // Directories.
    std::fs::create_dir(root.join("alpha")).unwrap();
    std::fs::create_dir(root.join("beta")).unwrap();
    std::fs::create_dir(root.join(".hidden_dir")).unwrap();

    // Files.
    std::fs::write(root.join("file_a.txt"), "content a").unwrap();
    std::fs::write(root.join("file_b.rs"), "fn main() {}").unwrap();
    std::fs::write(root.join(".hidden_file"), "secret").unwrap();

    // Executable file.
    std::fs::write(root.join("script.sh"), "#!/bin/sh").unwrap();
    std::fs::set_permissions(
        root.join("script.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();

    // Symlink.
    std::os::unix::fs::symlink("alpha", root.join("link_to_alpha")).unwrap();

    // Subdirectory with content.
    std::fs::create_dir(root.join("alpha").join("sub")).unwrap();
    std::fs::write(root.join("alpha").join("sub").join("deep.txt"), "deep").unwrap();
    std::fs::write(root.join("alpha").join("inner.txt"), "inner").unwrap();

    tmp
}

#[test]
fn read_directory_returns_sorted_entries() {
    let tmp = setup_test_dir();
    let listing = read_directory(tmp.path());
    match listing {
        DirListing::Entries(entries) => {
            // Directories should come first.
            let dirs: Vec<_> = entries
                .iter()
                .filter(|e| e.kind == EntryKind::Directory)
                .collect();
            let non_dirs: Vec<_> = entries
                .iter()
                .filter(|e| e.kind != EntryKind::Directory)
                .collect();

            assert!(!dirs.is_empty());
            assert!(!non_dirs.is_empty());

            // First entries should all be directories.
            let first_non_dir_idx = entries
                .iter()
                .position(|e| e.kind != EntryKind::Directory)
                .unwrap();
            for entry in &entries[..first_non_dir_idx] {
                assert_eq!(entry.kind, EntryKind::Directory);
            }

            // Directories include .hidden_dir.
            assert!(dirs.iter().any(|e| e.display_name == ".hidden_dir"));
            assert!(dirs.iter().any(|e| e.display_name == "alpha"));
            assert!(dirs.iter().any(|e| e.display_name == "beta"));
        }
        DirListing::Error(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn read_directory_includes_hidden_entries_except_git_metadata() {
    let tmp = setup_test_dir();
    std::fs::create_dir(tmp.path().join(".git")).unwrap();
    let listing = read_directory(tmp.path());
    match listing {
        DirListing::Entries(entries) => {
            let hidden: Vec<_> = entries.iter().filter(|e| e.hidden).collect();
            assert!(hidden.len() >= 2); // .hidden_dir, .hidden_file
            assert!(hidden.iter().any(|e| e.display_name == ".hidden_dir"));
            assert!(hidden.iter().any(|e| e.display_name == ".hidden_file"));
            assert!(!hidden.iter().any(|e| e.display_name == ".git"));
        }
        DirListing::Error(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn read_directory_marks_git_repository_without_showing_its_metadata() {
    let tmp = setup_test_dir();
    let repository = tmp.path().join("repository");
    std::fs::create_dir_all(repository.join(".git")).unwrap();

    let listing = read_directory(tmp.path());
    let DirListing::Entries(entries) = listing else {
        panic!("expected directory entries");
    };
    let entry = entries
        .iter()
        .find(|entry| entry.display_name == "repository")
        .unwrap();
    assert!(entry.is_git_repository);

    let repository_listing = read_directory(&repository);
    let DirListing::Entries(entries) = repository_listing else {
        panic!("expected repository entries");
    };
    assert!(!entries.iter().any(|entry| entry.display_name == ".git"));
}

#[test]
fn read_directory_detects_symlinks() {
    let tmp = setup_test_dir();
    let listing = read_directory(tmp.path());
    match listing {
        DirListing::Entries(entries) => {
            let link = entries
                .iter()
                .find(|e| e.display_name == "link_to_alpha")
                .unwrap();
            assert_eq!(link.kind, EntryKind::Symlink);
            assert_eq!(link.link_target.as_deref(), Some("alpha"));
        }
        DirListing::Error(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn read_directory_detects_executable() {
    let tmp = setup_test_dir();
    let listing = read_directory(tmp.path());
    match listing {
        DirListing::Entries(entries) => {
            let script = entries
                .iter()
                .find(|e| e.display_name == "script.sh")
                .unwrap();
            assert_eq!(script.kind, EntryKind::File);
            assert!(script.executable);

            let regular = entries
                .iter()
                .find(|e| e.display_name == "file_a.txt")
                .unwrap();
            assert!(!regular.executable);
        }
        DirListing::Error(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn read_directory_reports_file_sizes() {
    let tmp = setup_test_dir();
    let listing = read_directory(tmp.path());
    match listing {
        DirListing::Entries(entries) => {
            let file = entries
                .iter()
                .find(|e| e.display_name == "file_a.txt")
                .unwrap();
            assert_eq!(file.size, Some(9)); // "content a" = 9 bytes
            let dir = entries.iter().find(|e| e.display_name == "alpha").unwrap();
            assert_eq!(dir.size, None);
        }
        DirListing::Error(e) => panic!("unexpected error: {e}"),
    }
}

#[test]
fn read_directory_error_for_nonexistent() {
    let listing = read_directory(Path::new("/nonexistent/path/that/does/not/exist"));
    assert!(matches!(listing, DirListing::Error(_)));
}

#[test]
fn sort_order_is_deterministic() {
    let mut entries = vec![
        Entry {
            name: OsString::from("z_file"),
            display_name: "z_file".to_string(),
            is_lossy: false,
            kind: EntryKind::File,
            hidden: false,
            is_git_repository: false,
            size: Some(10),
            executable: false,
            link_target: None,
        },
        Entry {
            name: OsString::from("a_dir"),
            display_name: "a_dir".to_string(),
            is_lossy: false,
            kind: EntryKind::Directory,
            hidden: false,
            is_git_repository: false,
            size: None,
            executable: false,
            link_target: None,
        },
        Entry {
            name: OsString::from("m_link"),
            display_name: "m_link".to_string(),
            is_lossy: false,
            kind: EntryKind::Symlink,
            hidden: false,
            is_git_repository: false,
            size: None,
            executable: false,
            link_target: Some("target".to_string()),
        },
        Entry {
            name: OsString::from("b_dir"),
            display_name: "b_dir".to_string(),
            is_lossy: false,
            kind: EntryKind::Directory,
            hidden: false,
            is_git_repository: false,
            size: None,
            executable: false,
            link_target: None,
        },
        Entry {
            name: OsString::from("a_file"),
            display_name: "a_file".to_string(),
            is_lossy: false,
            kind: EntryKind::File,
            hidden: false,
            is_git_repository: false,
            size: Some(5),
            executable: false,
            link_target: None,
        },
    ];

    sort_entries(&mut entries);

    // Directories first: a_dir, b_dir.
    assert_eq!(entries[0].display_name, "a_dir");
    assert_eq!(entries[1].display_name, "b_dir");
    // Then symlinks.
    assert_eq!(entries[2].display_name, "m_link");
    // Then files: a_file, z_file.
    assert_eq!(entries[3].display_name, "a_file");
    assert_eq!(entries[4].display_name, "z_file");
}

#[test]
fn sort_order_case_insensitive() {
    let mut entries = vec![
        Entry {
            name: OsString::from("Zebra"),
            display_name: "Zebra".to_string(),
            is_lossy: false,
            kind: EntryKind::File,
            hidden: false,
            is_git_repository: false,
            size: None,
            executable: false,
            link_target: None,
        },
        Entry {
            name: OsString::from("alpha"),
            display_name: "alpha".to_string(),
            is_lossy: false,
            kind: EntryKind::File,
            hidden: false,
            is_git_repository: false,
            size: None,
            executable: false,
            link_target: None,
        },
        Entry {
            name: OsString::from("Beta"),
            display_name: "Beta".to_string(),
            is_lossy: false,
            kind: EntryKind::File,
            hidden: false,
            is_git_repository: false,
            size: None,
            executable: false,
            link_target: None,
        },
    ];

    sort_entries(&mut entries);

    assert_eq!(entries[0].display_name, "alpha");
    assert_eq!(entries[1].display_name, "Beta");
    assert_eq!(entries[2].display_name, "Zebra");
}

#[test]
fn browser_starts_at_configured_start() {
    let tmp = setup_test_dir();
    let browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().join("alpha"),
    });
    assert_eq!(browser.current_dir(), tmp.path().join("alpha"));
}

#[test]
fn browser_clamps_start_to_root() {
    let tmp = setup_test_dir();
    // Start above root should be clamped.
    let browser = Browser::new(BrowserConfig {
        root: tmp.path().join("alpha"),
        start: tmp.path().to_path_buf(),
    });
    assert_eq!(browser.current_dir(), tmp.path().join("alpha"));
}

#[test]
fn browser_at_root_detection() {
    let tmp = setup_test_dir();
    let browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });
    assert!(browser.at_root());

    let browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().join("alpha"),
    });
    assert!(!browser.at_root());
}

#[test]
fn browser_move_up_down() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    assert_eq!(browser.selected(), 0);
    assert!(browser.move_down());
    assert_eq!(browser.selected(), 1);
    assert!(browser.move_up());
    assert_eq!(browser.selected(), 0);
    // Cannot go above 0.
    assert!(!browser.move_up());
    assert_eq!(browser.selected(), 0);
}

#[test]
fn browser_move_down_clamps_at_end() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("only_file"), "x").unwrap();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Only one entry.
    assert_eq!(browser.entry_count(), 1);
    assert!(!browser.move_down());
    assert_eq!(browser.selected(), 0);
}

#[test]
fn browser_enter_directory() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Find the "alpha" entry (should be first real directory after .hidden_dir).
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let alpha_idx = entries
            .iter()
            .position(|e| e.display_name == "alpha")
            .unwrap();
        browser.selected = alpha_idx;
        assert!(browser.enter_selected());
        assert_eq!(browser.current_dir(), tmp.path().join("alpha"));
        assert_eq!(browser.selected(), 0);
    }
}

#[test]
fn browser_cannot_enter_file() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Find a file entry.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let file_idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::File)
            .unwrap();
        browser.selected = file_idx;
        assert!(!browser.enter_selected());
    }
}

#[test]
fn browser_cannot_enter_symlink() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Find symlink entry.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let link_idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::Symlink)
            .unwrap();
        browser.selected = link_idx;
        assert!(!browser.enter_selected());
    }
}

#[test]
fn browser_go_parent() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().join("alpha"),
    });

    assert!(browser.go_parent());
    assert_eq!(browser.current_dir(), tmp.path());
    // Should have selected "alpha" in the parent listing.
    let selected = browser.selected();
    let listing = browser.current_listing();
    if let DirListing::Entries(entries) = listing
        && let Some(entry) = entries.get(selected)
    {
        assert_eq!(entry.display_name, "alpha");
    }
}

#[test]
fn browser_go_parent_blocked_at_root() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    assert!(!browser.go_parent());
    assert_eq!(browser.current_dir(), tmp.path());
}

#[test]
fn browser_caches_listings() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // First access loads.
    let _ = browser.current_listing();
    assert!(browser.cache.contains_key(tmp.path()));

    // Second access uses cache (no panic, same result).
    let _ = browser.current_listing();
    assert!(browser.cache.contains_key(tmp.path()));
}

#[test]
fn browser_invalidate_forces_reload() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let _ = browser.current_listing();
    assert!(browser.cache.contains_key(tmp.path()));

    browser.invalidate(tmp.path());
    assert!(!browser.cache.contains_key(tmp.path()));
}

#[test]
fn browser_refresh_current_clamps_selection() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Move to a high index.
    let count = browser.entry_count();
    browser.selected = count - 1;

    // Remove most entries on disk.
    std::fs::remove_dir_all(tmp.path().join("alpha")).unwrap();
    std::fs::remove_dir_all(tmp.path().join("beta")).unwrap();
    std::fs::remove_dir_all(tmp.path().join(".hidden_dir")).unwrap();
    std::fs::remove_file(tmp.path().join("file_a.txt")).unwrap();
    std::fs::remove_file(tmp.path().join("file_b.rs")).unwrap();
    std::fs::remove_file(tmp.path().join(".hidden_file")).unwrap();
    std::fs::remove_file(tmp.path().join("script.sh")).unwrap();
    // Only link_to_alpha remains.

    browser.refresh_current();
    let new_count = browser.entry_count();
    assert!(browser.selected() < new_count || new_count == 0);
}

#[test]
fn browser_move_home_end() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let count = browser.entry_count();
    assert!(count > 1);

    browser.move_end();
    assert_eq!(browser.selected(), count - 1);

    browser.move_home();
    assert_eq!(browser.selected(), 0);
}

#[test]
fn browser_page_up_down() {
    let tmp = TempDir::new().unwrap();
    // Create many files for paging.
    for i in 0..50 {
        std::fs::write(tmp.path().join(format!("file_{i:03}.txt")), "x").unwrap();
    }

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    assert_eq!(browser.entry_count(), 50);
    assert!(browser.page_down(10));
    assert_eq!(browser.selected(), 10);
    assert!(browser.page_down(10));
    assert_eq!(browser.selected(), 20);
    assert!(browser.page_up(10));
    assert_eq!(browser.selected(), 10);
    assert!(browser.page_up(10));
    assert_eq!(browser.selected(), 0);
    // At 0, page_up returns false.
    assert!(!browser.page_up(10));
}

#[test]
fn browser_navigate_to_respects_root() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().join("alpha"),
        start: tmp.path().join("alpha"),
    });

    // Cannot navigate above root.
    assert!(!browser.navigate_to(tmp.path()));
    assert_eq!(browser.current_dir(), tmp.path().join("alpha"));

    // Can navigate to subdirectory.
    assert!(browser.navigate_to(&tmp.path().join("alpha").join("sub")));
    assert_eq!(browser.current_dir(), tmp.path().join("alpha").join("sub"));
}

#[test]
fn browser_selected_entry_path() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Load the cache first.
    let _ = browser.current_listing();
    let path = browser.selected_entry_path();
    assert!(path.is_some());
    let path = path.unwrap();
    assert!(path.starts_with(tmp.path()));
}

#[test]
fn browser_preview_listing_for_directory() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Find "alpha" directory and select it.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let alpha_idx = entries
            .iter()
            .position(|e| e.display_name == "alpha")
            .unwrap();
        browser.selected = alpha_idx;
    }

    // Preview should show contents of alpha.
    let preview = browser.preview_listing();
    assert!(preview.is_some());
    if let Some(DirListing::Entries(entries)) = preview {
        // alpha contains: sub/ and inner.txt
        assert!(entries.iter().any(|e| e.display_name == "sub"));
        assert!(entries.iter().any(|e| e.display_name == "inner.txt"));
    }
}

#[test]
fn browser_regular_file_preview_uses_cat_and_refreshes_cached_content() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("preview.conf");
    std::fs::write(&path, "first value").unwrap();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let first = browser.selected_file_preview().unwrap().as_ref().unwrap();
    assert_eq!(first.content, "first value");

    std::fs::write(&path, "second value").unwrap();
    assert_eq!(
        browser
            .selected_file_preview()
            .unwrap()
            .as_ref()
            .unwrap()
            .content,
        "first value"
    );

    browser.refresh_current();
    assert_eq!(
        browser
            .selected_file_preview()
            .unwrap()
            .as_ref()
            .unwrap()
            .content,
        "second value"
    );
}

#[test]
fn browser_preview_listing_for_file_is_none() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Find a file entry.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let file_idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::File)
            .unwrap();
        browser.selected = file_idx;
    }

    assert!(browser.preview_listing().is_none());
}

#[test]
fn browser_empty_directory() {
    let tmp = TempDir::new().unwrap();
    let empty = tmp.path().join("empty");
    std::fs::create_dir(&empty).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: empty.clone(),
    });

    assert_eq!(browser.entry_count(), 0);
    assert!(!browser.move_down());
    assert!(!browser.move_up());
    assert_eq!(browser.selected_entry_path(), None);
}

#[test]
fn browser_deep_navigation_and_return() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Navigate into alpha.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let alpha_idx = entries
            .iter()
            .position(|e| e.display_name == "alpha")
            .unwrap();
        browser.selected = alpha_idx;
    }
    assert!(browser.enter_selected());
    assert_eq!(browser.current_dir(), tmp.path().join("alpha"));

    // Navigate into sub.
    let listing = browser.current_listing().clone();
    if let DirListing::Entries(entries) = &listing {
        let sub_idx = entries
            .iter()
            .position(|e| e.display_name == "sub")
            .unwrap();
        browser.selected = sub_idx;
    }
    assert!(browser.enter_selected());
    assert_eq!(browser.current_dir(), tmp.path().join("alpha").join("sub"));

    // Go back up twice.
    assert!(browser.go_parent());
    assert_eq!(browser.current_dir(), tmp.path().join("alpha"));
    assert!(browser.go_parent());
    assert_eq!(browser.current_dir(), tmp.path());
    assert!(!browser.go_parent()); // at root
}

#[test]
fn browser_scroll_offset_adjusts() {
    let tmp = TempDir::new().unwrap();
    for i in 0..100 {
        std::fs::write(tmp.path().join(format!("file_{i:03}.txt")), "x").unwrap();
    }

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    assert_eq!(browser.scroll_offset(), 0);

    // Move to a position beyond the default viewport.
    for _ in 0..25 {
        browser.move_down();
    }
    // Scroll offset should have adjusted.
    assert!(browser.scroll_offset() > 0);
    assert!(browser.selected() >= browser.scroll_offset());
}

#[test]
fn parent_listing_returns_none_at_root() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    assert!(browser.parent_listing().is_none());
}

#[test]
fn parent_listing_returns_entries_when_not_at_root() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().join("alpha"),
    });

    let parent = browser.parent_listing();
    assert!(parent.is_some());
}

// --- UX04: Safety tests ---

#[test]
fn select_regular_file_succeeds() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Find a regular file.
    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let file_idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::File)
            .unwrap();
        browser.selected = file_idx;
    }

    let result = browser.try_select();
    assert!(result.is_ok());
    let sel = result.unwrap();
    assert_eq!(sel.kind, EntryKind::File);
    assert!(!sel.is_symlink);
}

#[test]
fn select_directory_succeeds() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let dir_idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::Directory && e.display_name == "alpha")
            .unwrap();
        browser.selected = dir_idx;
    }

    let result = browser.try_select();
    assert!(result.is_ok());
    let sel = result.unwrap();
    assert_eq!(sel.kind, EntryKind::Directory);
    assert!(!sel.is_symlink);
}

#[test]
fn select_symlink_succeeds_with_metadata() {
    let tmp = setup_test_dir();
    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let link_idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::Symlink)
            .unwrap();
        browser.selected = link_idx;
    }

    let result = browser.try_select();
    assert!(result.is_ok());
    let sel = result.unwrap();
    assert_eq!(sel.kind, EntryKind::Symlink);
    assert!(sel.is_symlink);
    assert_eq!(sel.link_target.as_deref(), Some("alpha"));
}

#[test]
fn select_special_file_rejected() {
    let tmp = TempDir::new().unwrap();
    // Create a FIFO (named pipe).
    let fifo_path = tmp.path().join("test_fifo");
    unsafe {
        let c_path = std::ffi::CString::new(fifo_path.to_str().unwrap()).unwrap();
        libc::mkfifo(c_path.as_ptr(), 0o644);
    }

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let special_idx = entries
            .iter()
            .position(|e| e.kind == EntryKind::Special)
            .unwrap();
        browser.selected = special_idx;
    }

    let result = browser.try_select();
    assert!(matches!(result, Err(SelectionError::SpecialFile(_))));
}

#[test]
fn select_non_utf8_entry_rejected() {
    use std::os::unix::ffi::OsStrExt;
    let tmp = TempDir::new().unwrap();
    // Create a file with a non-UTF-8 name.
    let invalid_name = std::ffi::OsStr::from_bytes(b"invalid\xff\xfename.txt");
    let invalid_path = tmp.path().join(invalid_name);
    std::fs::write(&invalid_path, "content").unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let lossy_idx = entries.iter().position(|e| e.is_lossy).unwrap();
        browser.selected = lossy_idx;
    }

    let result = browser.try_select();
    assert!(matches!(result, Err(SelectionError::NonUtf8(_))));
}

#[test]
fn select_disappeared_entry_detected() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("ephemeral.txt"), "here now").unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // Load the listing (entry visible).
    let _ = browser.current_listing();
    browser.selected = 0;

    // Remove the file between listing and selection.
    std::fs::remove_file(tmp.path().join("ephemeral.txt")).unwrap();

    let result = browser.try_select();
    assert!(matches!(result, Err(SelectionError::Disappeared(_))));
}

#[test]
fn select_empty_directory_returns_no_entry() {
    let tmp = TempDir::new().unwrap();
    let empty = tmp.path().join("empty");
    std::fs::create_dir(&empty).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: empty,
    });

    let _ = browser.current_listing();
    let result = browser.try_select();
    assert!(matches!(result, Err(SelectionError::NoEntry)));
}

#[test]
fn cannot_enter_symlink_to_directory() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("real_dir")).unwrap();
    std::fs::write(tmp.path().join("real_dir").join("secret.txt"), "hidden").unwrap();
    std::os::unix::fs::symlink("real_dir", tmp.path().join("link_dir")).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let link_idx = entries
            .iter()
            .position(|e| e.display_name == "link_dir")
            .unwrap();
        browser.selected = link_idx;
        // Confirm it's classified as symlink, not directory.
        assert_eq!(entries[link_idx].kind, EntryKind::Symlink);
    }

    // Cannot enter through symlink.
    assert!(!browser.enter_selected());
}

#[test]
fn source_root_symlink_selectable() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("real_config")).unwrap();
    std::os::unix::fs::symlink("real_config", tmp.path().join(".config")).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let link_idx = entries
            .iter()
            .position(|e| e.display_name == ".config")
            .unwrap();
        browser.selected = link_idx;
    }

    // try_select succeeds for a symlink (caller decides policy).
    let result = browser.try_select();
    assert!(result.is_ok());
    let sel = result.unwrap();
    assert!(sel.is_symlink);
    assert_eq!(sel.kind, EntryKind::Symlink);
    assert_eq!(sel.link_target.as_deref(), Some("real_config"));
}

#[test]
fn unreadable_directory_produces_error_listing() {
    let tmp = TempDir::new().unwrap();
    let restricted = tmp.path().join("restricted");
    std::fs::create_dir(&restricted).unwrap();
    std::fs::write(restricted.join("secret.txt"), "x").unwrap();
    // Remove read permission.
    std::fs::set_permissions(&restricted, std::fs::Permissions::from_mode(0o000)).unwrap();

    let listing = read_directory(&restricted);
    // Restore permissions for cleanup.
    std::fs::set_permissions(&restricted, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(matches!(listing, DirListing::Error(_)));
}

#[test]
fn selection_error_display() {
    assert_eq!(SelectionError::NoEntry.to_string(), "No entry selected");
    assert_eq!(
        SelectionError::NonUtf8("bad\u{fffd}name".to_string()).to_string(),
        "'bad\u{fffd}name' contains non-UTF-8 characters and cannot be used"
    );
    assert_eq!(
        SelectionError::SpecialFile("my_socket".to_string()).to_string(),
        "'my_socket' is a special file and cannot be selected"
    );
    assert_eq!(
        SelectionError::Disappeared("gone.txt".to_string()).to_string(),
        "'gone.txt' no longer exists or is unreadable"
    );
}

#[test]
fn refresh_tolerates_disappeared_current_dir() {
    let tmp = TempDir::new().unwrap();
    let sub = tmp.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    std::fs::write(sub.join("file.txt"), "x").unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: sub.clone(),
    });

    let _ = browser.current_listing();
    assert_eq!(browser.entry_count(), 1);

    // Remove the current directory.
    std::fs::remove_dir_all(&sub).unwrap();
    browser.refresh_current();

    // Should produce an error listing, not crash.
    let listing = browser.current_listing();
    assert!(matches!(listing, DirListing::Error(_)));
}

#[test]
fn navigate_does_not_follow_symlinks_in_path() {
    let tmp = TempDir::new().unwrap();
    let real = tmp.path().join("real");
    std::fs::create_dir(&real).unwrap();
    std::fs::create_dir(real.join("inner")).unwrap();
    std::os::unix::fs::symlink("real", tmp.path().join("link")).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: tmp.path().to_path_buf(),
        start: tmp.path().to_path_buf(),
    });

    // The symlink "link" is listed but cannot be entered.
    let _ = browser.current_listing();
    let listing = browser.cache.get(tmp.path()).unwrap().clone();
    if let DirListing::Entries(entries) = &listing {
        let link_idx = entries
            .iter()
            .position(|e| e.display_name == "link")
            .unwrap();
        browser.selected = link_idx;
        assert_eq!(entries[link_idx].kind, EntryKind::Symlink);
    }

    // enter_selected refuses symlinks.
    assert!(!browser.enter_selected());
    assert_eq!(browser.current_dir(), tmp.path());
}

#[test]
fn boundary_prevents_navigation_above_root() {
    let tmp = TempDir::new().unwrap();
    let inner = tmp.path().join("inner");
    std::fs::create_dir(&inner).unwrap();
    std::fs::create_dir(inner.join("deep")).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: inner.clone(),
        start: inner.join("deep"),
    });

    // Can go up to root.
    assert!(browser.go_parent());
    assert_eq!(browser.current_dir(), &inner);

    // Cannot go above root.
    assert!(!browser.go_parent());
    assert_eq!(browser.current_dir(), &inner);
}

#[test]
fn navigate_to_rejects_paths_outside_root() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("root");
    let outside = tmp.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();

    let mut browser = Browser::new(BrowserConfig {
        root: root.clone(),
        start: root.clone(),
    });

    assert!(!browser.navigate_to(&outside));
    assert_eq!(browser.current_dir(), &root);
}
