use super::*;

#[test]
fn default_theme_is_catppuccin_mocha() {
    assert_eq!(ThemeId::default(), ThemeId::CatppuccinMocha);
    assert_eq!(ThemeId::ALL[0], ThemeId::CatppuccinMocha);
}

#[test]
fn every_theme_has_a_label_and_a_unique_slug() {
    let mut slugs = std::collections::HashSet::new();
    for &id in ThemeId::ALL {
        assert!(!id.label().is_empty());
        assert!(slugs.insert(id.slug()), "duplicate slug: {}", id.slug());
    }
}

#[test]
fn slugs_round_trip_through_from_slug() {
    for &id in ThemeId::ALL {
        assert_eq!(ThemeId::from_slug(id.slug()), Some(id));
    }
}

#[test]
fn from_slug_rejects_unknown_values() {
    assert_eq!(ThemeId::from_slug("not-a-real-theme"), None);
    assert_eq!(ThemeId::from_slug(""), None);
}

#[test]
fn next_and_prev_wrap_around_and_are_inverses() {
    assert_eq!(ThemeId::CatppuccinMocha.prev(), ThemeId::Kanagawa);
    assert_eq!(ThemeId::Kanagawa.next(), ThemeId::CatppuccinMocha);
    for &id in ThemeId::ALL {
        assert_eq!(id.next().prev(), id);
    }
}

#[test]
fn every_palette_role_is_distinguishable_from_the_background() {
    // A role that resolved to the same color as the canvas background would
    // make that element invisible. This does not check contrast ratios, only
    // that no theme accidentally aliases a foreground role to its own
    // background.
    for &id in ThemeId::ALL {
        let p = id.palette();
        for role in [
            p.foreground,
            p.accent,
            p.secondary,
            p.success,
            p.warning,
            p.error,
        ] {
            assert_ne!(
                role,
                p.background,
                "{}: a foreground role matches the background",
                id.label()
            );
        }
    }
}

#[test]
fn set_active_changes_current_and_persists_across_calls() {
    let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    set_active(ThemeId::Dracula);
    assert_eq!(active_id(), ThemeId::Dracula);
    assert_eq!(current().palette(), ThemeId::Dracula.palette());

    set_active(ThemeId::default());
    assert_eq!(active_id(), ThemeId::default());
}

#[test]
fn preference_round_trips_through_disk() {
    let _guard = TEST_GUARD.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();

    assert_eq!(load_preference(dir.path()), None);

    save_preference(dir.path(), ThemeId::Nord).unwrap();
    assert_eq!(load_preference(dir.path()), Some(ThemeId::Nord));

    save_preference(dir.path(), ThemeId::GruvboxDark).unwrap();
    assert_eq!(load_preference(dir.path()), Some(ThemeId::GruvboxDark));
}

#[test]
fn corrupt_preference_file_is_ignored_rather_than_failing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("theme.toml"), "not valid toml{{{").unwrap();
    assert_eq!(load_preference(dir.path()), None);

    std::fs::write(dir.path().join("theme.toml"), "theme = \"not-a-theme\"").unwrap();
    assert_eq!(load_preference(dir.path()), None);
}

#[test]
fn reduced_palette_keeps_focus_and_selection_legible_without_color() {
    let focused = Theme::REDUCED.focused();
    let selected = Theme::REDUCED.selected();

    assert_eq!(focused.fg, None);
    assert!(focused.add_modifier.contains(Modifier::BOLD));
    assert_eq!(selected.bg, None);
    assert!(selected.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn semantic_states_have_non_color_emphasis() {
    let theme = Theme::current();
    assert!(theme.success().add_modifier.contains(Modifier::BOLD));
    assert!(theme.warning().add_modifier.contains(Modifier::BOLD));
    assert!(theme.error().add_modifier.contains(Modifier::BOLD));
    assert!(theme.disabled().add_modifier.contains(Modifier::DIM));
}

#[test]
fn canvas_chrome_and_surface_paint_explicit_backgrounds() {
    let theme = Theme::current();
    assert!(theme.canvas().bg.is_some());
    assert!(theme.chrome().bg.is_some());
    assert!(theme.surface().bg.is_some());
}
