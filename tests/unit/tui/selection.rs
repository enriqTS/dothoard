use super::*;

fn home() -> PathBuf {
    PathBuf::from("/home/user")
}

// --- CheckState and basic construction ---

#[test]
fn new_selection_is_empty() {
    let sel = SourceSelection::new(&home());
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config")),
        CheckState::Unchecked
    );
}

// --- load_from_config ---

#[test]
fn load_from_config_marks_sources_explicit() {
    let mut sel = SourceSelection::new(&home());
    let sources = vec![
        SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        },
        SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        },
    ];
    sel.load_from_config(&sources);

    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish")),
        CheckState::Explicit
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.bashrc")),
        CheckState::Explicit
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/waybar")),
        CheckState::Unchecked
    );
}

#[test]
fn load_from_config_clears_previous_state() {
    let mut sel = SourceSelection::new(&home());
    sel.selected.insert(PathBuf::from("/home/user/.zshrc"));

    let sources = vec![SourceConfig {
        path: ".bashrc".to_string(),
        ignore: vec![],
    }];
    sel.load_from_config(&sources);

    assert_eq!(
        sel.is_selected(Path::new("/home/user/.zshrc")),
        CheckState::Unchecked
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.bashrc")),
        CheckState::Explicit
    );
}

// --- is_selected with inheritance ---

#[test]
fn child_of_selected_folder_is_inherited() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/config.fish")),
        CheckState::Inherited
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/git.fish")),
        CheckState::Inherited
    );
}

#[test]
fn deselected_child_is_unchecked() {
    let mut sel = SourceSelection::new(&home());
    let source_path = PathBuf::from("/home/user/.config/fish");
    sel.selected.insert(source_path.clone());
    sel.deselected
        .entry(source_path)
        .or_default()
        .push("fish_variables".to_string());

    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/fish_variables")),
        CheckState::Unchecked
    );
    // Other children remain inherited.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/config.fish")),
        CheckState::Inherited
    );
}

#[test]
fn deeply_nested_child_inherits() {
    let mut sel = SourceSelection::new(&home());
    sel.selected.insert(PathBuf::from("/home/user/.config"));

    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/git.fish")),
        CheckState::Inherited
    );
}

#[test]
fn path_outside_any_source_is_unchecked() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/waybar/config")),
        CheckState::Unchecked
    );
}

// --- toggle ---

#[test]
fn toggle_unchecked_to_explicit() {
    let mut sel = SourceSelection::new(&home());
    let path = Path::new("/home/user/.config/fish");

    assert_eq!(sel.is_selected(path), CheckState::Unchecked);
    sel.toggle(path, false);
    assert_eq!(sel.is_selected(path), CheckState::Explicit);
}

#[test]
fn toggle_explicit_to_unchecked() {
    let mut sel = SourceSelection::new(&home());
    let path = Path::new("/home/user/.config/fish");
    sel.selected.insert(path.to_path_buf());

    sel.toggle(path, false);
    assert_eq!(sel.is_selected(path), CheckState::Unchecked);
}

#[test]
fn toggle_explicit_clears_deselected_list() {
    let mut sel = SourceSelection::new(&home());
    let source = PathBuf::from("/home/user/.config/fish");
    sel.selected.insert(source.clone());
    sel.deselected
        .entry(source.clone())
        .or_default()
        .push("fish_variables".to_string());

    sel.toggle(&source, false);
    assert!(!sel.deselected.contains_key(&source));
}

#[test]
fn toggle_inherited_to_deselected() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));
    let child = Path::new("/home/user/.config/fish/fish_variables");

    assert_eq!(sel.is_selected(child), CheckState::Inherited);
    sel.toggle(child, false);
    assert_eq!(sel.is_selected(child), CheckState::Unchecked);
}

#[test]
fn toggle_deselected_back_to_inherited() {
    let mut sel = SourceSelection::new(&home());
    let source = PathBuf::from("/home/user/.config/fish");
    sel.selected.insert(source.clone());
    sel.deselected
        .entry(source)
        .or_default()
        .push("fish_variables".to_string());

    let child = Path::new("/home/user/.config/fish/fish_variables");
    assert_eq!(sel.is_selected(child), CheckState::Unchecked);
    sel.toggle(child, false);
    assert_eq!(sel.is_selected(child), CheckState::Inherited);
}

#[test]
fn toggle_nested_deselection() {
    let mut sel = SourceSelection::new(&home());
    sel.selected.insert(PathBuf::from("/home/user/.config"));

    let nested = Path::new("/home/user/.config/fish/completions/git.fish");
    assert_eq!(sel.is_selected(nested), CheckState::Inherited);

    sel.toggle(nested, false);
    assert_eq!(sel.is_selected(nested), CheckState::Unchecked);

    // Verify the relative path is stored correctly.
    let deselected = sel.deselected.get(Path::new("/home/user/.config")).unwrap();
    assert_eq!(deselected, &["fish/completions/git.fish"]);
}

// --- diff_against_config ---

#[test]
fn diff_detects_additions() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));
    sel.selected.insert(PathBuf::from("/home/user/.bashrc"));

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let diff = sel.diff_against_config(&existing);
    assert_eq!(diff.additions, vec![".bashrc"]);
    assert!(diff.removals.is_empty());
    assert!(diff.ignore_rules.is_empty());
}

#[test]
fn diff_detects_removals() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let existing = vec![
        SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        },
        SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        },
    ];

    let diff = sel.diff_against_config(&existing);
    assert!(diff.additions.is_empty());
    assert_eq!(diff.removals, vec![".bashrc"]);
    assert!(diff.ignore_rules.is_empty());
}

#[test]
fn diff_detects_ignore_rules() {
    let mut sel = SourceSelection::new(&home());
    let source = PathBuf::from("/home/user/.config/fish");
    sel.selected.insert(source.clone());
    sel.deselected
        .entry(source)
        .or_default()
        .push("fish_variables".to_string());

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let diff = sel.diff_against_config(&existing);
    assert!(diff.additions.is_empty());
    assert!(diff.removals.is_empty());
    let rules = diff.ignore_rules.get(".config/fish").unwrap();
    assert_eq!(rules, &["/fish_variables"]);
}

#[test]
fn diff_does_not_duplicate_existing_ignore_rules() {
    let mut sel = SourceSelection::new(&home());
    let source = PathBuf::from("/home/user/.config/fish");
    sel.selected.insert(source.clone());
    sel.deselected
        .entry(source)
        .or_default()
        .push("fish_variables".to_string());

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec!["/fish_variables".to_string()],
    }];

    let diff = sel.diff_against_config(&existing);
    // The rule already exists, so it should not appear in the diff.
    assert!(diff.ignore_rules.is_empty());
}

#[test]
fn diff_combined_changes() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));
    sel.selected
        .insert(PathBuf::from("/home/user/.config/waybar"));
    let fish_path = PathBuf::from("/home/user/.config/fish");
    sel.deselected
        .entry(fish_path)
        .or_default()
        .push("completions/git.fish".to_string());

    let existing = vec![
        SourceConfig {
            path: ".config/fish".to_string(),
            ignore: vec![],
        },
        SourceConfig {
            path: ".bashrc".to_string(),
            ignore: vec![],
        },
    ];

    let diff = sel.diff_against_config(&existing);
    assert_eq!(diff.additions, vec![".config/waybar"]);
    assert_eq!(diff.removals, vec![".bashrc"]);
    let rules = diff.ignore_rules.get(".config/fish").unwrap();
    assert_eq!(rules, &["/completions/git.fish"]);
}

#[test]
fn diff_no_changes() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let diff = sel.diff_against_config(&existing);
    assert!(diff.additions.is_empty());
    assert!(diff.removals.is_empty());
    assert!(diff.ignore_rules.is_empty());
}

#[test]
fn has_changes_false_when_no_diff() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    assert!(!sel.has_changes(&existing));
}

#[test]
fn has_changes_true_when_additions() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));
    sel.selected.insert(PathBuf::from("/home/user/.bashrc"));

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    assert!(sel.has_changes(&existing));
}

#[test]
fn diff_ignores_deselected_for_removed_sources() {
    // If a source is unchecked (removed), its deselected list should not
    // generate ignore rules since the source itself is being removed.
    let mut sel = SourceSelection::new(&home());
    // Don't add .config/fish to selected — it's being removed.
    let source = PathBuf::from("/home/user/.config/fish");
    sel.deselected
        .entry(source)
        .or_default()
        .push("fish_variables".to_string());

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let diff = sel.diff_against_config(&existing);
    assert_eq!(diff.removals, vec![".config/fish"]);
    // No ignore rules because the source is being removed.
    assert!(diff.ignore_rules.is_empty());
}

// --- Directory deselection format ---

#[test]
fn toggle_directory_inside_source_stores_with_trailing_slash() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let dir = Path::new("/home/user/.config/fish/completions");
    sel.toggle(dir, true);

    let deselected = sel
        .deselected
        .get(Path::new("/home/user/.config/fish"))
        .unwrap();
    assert_eq!(deselected, &["completions/"]);
}

#[test]
fn toggle_file_inside_source_stores_without_trailing_slash() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let file = Path::new("/home/user/.config/fish/config.fish");
    sel.toggle(file, false);

    let deselected = sel
        .deselected
        .get(Path::new("/home/user/.config/fish"))
        .unwrap();
    assert_eq!(deselected, &["config.fish"]);
}

#[test]
fn diff_generates_anchored_directory_rule() {
    let mut sel = SourceSelection::new(&home());
    let source = PathBuf::from("/home/user/.config/fish");
    sel.selected.insert(source.clone());
    sel.deselected
        .entry(source)
        .or_default()
        .push("completions/".to_string());

    let existing = vec![SourceConfig {
        path: ".config/fish".to_string(),
        ignore: vec![],
    }];

    let diff = sel.diff_against_config(&existing);
    let rules = diff.ignore_rules.get(".config/fish").unwrap();
    assert_eq!(rules, &["/completions/"]);
}

#[test]
fn toggle_directory_deselect_then_reselect() {
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let dir = Path::new("/home/user/.config/fish/completions");

    // Deselect directory.
    sel.toggle(dir, true);
    assert_eq!(sel.is_selected(dir), CheckState::Unchecked);

    // Re-select directory.
    sel.toggle(dir, true);
    assert_eq!(sel.is_selected(dir), CheckState::Inherited);
    // Deselected list should be cleared.
    assert!(
        !sel.deselected
            .contains_key(Path::new("/home/user/.config/fish"))
    );
}

// --- MS06: Multi-level inheritance and deselected directory children ---

#[test]
fn multi_level_inheritance_three_deep() {
    // Source: .config → child: .config/fish → grandchild: .config/fish/conf.d
    // → great-grandchild: .config/fish/conf.d/aliases.fish
    let mut sel = SourceSelection::new(&home());
    sel.selected.insert(PathBuf::from("/home/user/.config"));

    // All descendants at various depths should be Inherited.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish")),
        CheckState::Inherited
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/conf.d")),
        CheckState::Inherited
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/conf.d/aliases.fish")),
        CheckState::Inherited
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/waybar/config.jsonc")),
        CheckState::Inherited
    );
}

#[test]
fn deselect_at_various_depths() {
    // Source is .config. Deselect entries at depth 1, 2, and 3.
    let mut sel = SourceSelection::new(&home());
    sel.selected.insert(PathBuf::from("/home/user/.config"));

    // Deselect depth-1 file.
    let depth1 = Path::new("/home/user/.config/mimeapps.list");
    sel.toggle(depth1, false);
    assert_eq!(sel.is_selected(depth1), CheckState::Unchecked);

    // Deselect depth-2 file.
    let depth2 = Path::new("/home/user/.config/fish/fish_variables");
    sel.toggle(depth2, false);
    assert_eq!(sel.is_selected(depth2), CheckState::Unchecked);

    // Deselect depth-3 file.
    let depth3 = Path::new("/home/user/.config/fish/completions/git.fish");
    sel.toggle(depth3, false);
    assert_eq!(sel.is_selected(depth3), CheckState::Unchecked);

    // Verify stored relative paths are correct.
    let deselected = sel.deselected.get(Path::new("/home/user/.config")).unwrap();
    assert!(deselected.contains(&"mimeapps.list".to_string()));
    assert!(deselected.contains(&"fish/fish_variables".to_string()));
    assert!(deselected.contains(&"fish/completions/git.fish".to_string()));
}

#[test]
fn deselected_directory_blocks_children() {
    // Source is .config/fish. Deselect "completions/" directory.
    // Children inside completions/ should also show as Unchecked.
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let completions = Path::new("/home/user/.config/fish/completions");
    sel.toggle(completions, true); // is_dir=true → stores "completions/"

    // The directory itself is unchecked.
    assert_eq!(sel.is_selected(completions), CheckState::Unchecked);

    // Children inside the deselected directory are also unchecked.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/git.fish")),
        CheckState::Unchecked
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/docker.fish")),
        CheckState::Unchecked
    );
    assert_eq!(
        sel.is_selected(Path::new(
            "/home/user/.config/fish/completions/subdir/nested.fish"
        )),
        CheckState::Unchecked
    );

    // Siblings of the deselected directory remain inherited.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/config.fish")),
        CheckState::Inherited
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/conf.d/aliases.fish")),
        CheckState::Inherited
    );
}

#[test]
fn deselected_nested_directory_blocks_deeply_nested_children() {
    // Source is .config. Deselect "fish/completions/" (nested directory).
    // Children at any depth within that directory should be Unchecked.
    let mut sel = SourceSelection::new(&home());
    sel.selected.insert(PathBuf::from("/home/user/.config"));

    let completions = Path::new("/home/user/.config/fish/completions");
    sel.toggle(completions, true); // stores "fish/completions/"

    // The directory itself.
    assert_eq!(sel.is_selected(completions), CheckState::Unchecked);

    // Direct child.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/git.fish")),
        CheckState::Unchecked
    );

    // Nested child inside the deselected directory.
    assert_eq!(
        sel.is_selected(Path::new(
            "/home/user/.config/fish/completions/vendor/extra.fish"
        )),
        CheckState::Unchecked
    );

    // Unrelated paths remain inherited.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/config.fish")),
        CheckState::Inherited
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/waybar/config")),
        CheckState::Inherited
    );
}

#[test]
fn reselect_file_inside_deselected_directory_restores_inherited() {
    // Source is .config/fish. Deselect "completions/" directory.
    // Then toggle a specific file inside → it should become Explicit
    // (since we can't partially un-deselect a directory rule).
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let completions = Path::new("/home/user/.config/fish/completions");
    sel.toggle(completions, true);
    assert_eq!(sel.is_selected(completions), CheckState::Unchecked);

    // Toggle a child inside the deselected directory.
    let child = Path::new("/home/user/.config/fish/completions/git.fish");
    assert_eq!(sel.is_selected(child), CheckState::Unchecked);
    sel.toggle(child, false);
    // The child becomes Explicit since there's no way to partially
    // restore within a deselected directory — it's added to selected set.
    assert_eq!(sel.is_selected(child), CheckState::Explicit);
}

#[test]
fn reselect_deselected_directory_restores_all_children() {
    // Deselect completions/ then reselect it → all children go back to Inherited.
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let completions = Path::new("/home/user/.config/fish/completions");
    sel.toggle(completions, true);
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/git.fish")),
        CheckState::Unchecked
    );

    // Reselect the directory.
    sel.toggle(completions, true);
    assert_eq!(sel.is_selected(completions), CheckState::Inherited);
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/git.fish")),
        CheckState::Inherited
    );
}

#[test]
fn multiple_deselected_directories_independent() {
    // Deselect two sibling directories; each blocks only its own children.
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let completions = Path::new("/home/user/.config/fish/completions");
    let conf_d = Path::new("/home/user/.config/fish/conf.d");
    sel.toggle(completions, true);
    sel.toggle(conf_d, true);

    // Both directories and their children are unchecked.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/completions/git.fish")),
        CheckState::Unchecked
    );
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/conf.d/aliases.fish")),
        CheckState::Unchecked
    );

    // Other children of the source remain inherited.
    assert_eq!(
        sel.is_selected(Path::new("/home/user/.config/fish/config.fish")),
        CheckState::Inherited
    );
}

#[test]
fn diff_generates_rules_for_deeply_nested_deselection() {
    // Source is .config, deselect fish/completions/ and fish/fish_variables.
    let mut sel = SourceSelection::new(&home());
    let source = PathBuf::from("/home/user/.config");
    sel.selected.insert(source.clone());
    sel.deselected.entry(source).or_default().extend(vec![
        "fish/completions/".to_string(),
        "fish/fish_variables".to_string(),
    ]);

    let existing = vec![SourceConfig {
        path: ".config".to_string(),
        ignore: vec![],
    }];

    let diff = sel.diff_against_config(&existing);
    let rules = diff.ignore_rules.get(".config").unwrap();
    assert!(rules.contains(&"/fish/completions/".to_string()));
    assert!(rules.contains(&"/fish/fish_variables".to_string()));
}

#[test]
fn toggle_via_api_multi_level_roundtrip() {
    // Full workflow: select .config/fish, deselect completions/ dir,
    // verify child state, re-select completions/, verify restoration.
    let mut sel = SourceSelection::new(&home());
    sel.selected
        .insert(PathBuf::from("/home/user/.config/fish"));

    let completions = Path::new("/home/user/.config/fish/completions");
    let git_fish = Path::new("/home/user/.config/fish/completions/git.fish");

    // Initially inherited.
    assert_eq!(sel.is_selected(completions), CheckState::Inherited);
    assert_eq!(sel.is_selected(git_fish), CheckState::Inherited);

    // Deselect completions/.
    sel.toggle(completions, true);
    assert_eq!(sel.is_selected(completions), CheckState::Unchecked);
    assert_eq!(sel.is_selected(git_fish), CheckState::Unchecked);

    // Re-select completions/.
    sel.toggle(completions, true);
    assert_eq!(sel.is_selected(completions), CheckState::Inherited);
    assert_eq!(sel.is_selected(git_fish), CheckState::Inherited);

    // Deselected list should be empty now.
    assert!(
        !sel.deselected
            .contains_key(Path::new("/home/user/.config/fish"))
    );
}
