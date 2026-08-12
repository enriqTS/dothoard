use super::*;

#[test]
fn reduced_palette_keeps_focus_and_selection_visible_without_color() {
    let focused = Theme::REDUCED.focused();
    let selected = Theme::REDUCED.selected();

    assert_eq!(focused.fg, None);
    assert!(focused.add_modifier.contains(Modifier::UNDERLINED));
    assert_eq!(selected.fg, None);
    assert!(selected.add_modifier.contains(Modifier::REVERSED));
}

#[test]
fn standard_palette_does_not_force_a_background() {
    for style in [
        Theme::STANDARD.heading(),
        Theme::STANDARD.label(),
        Theme::STANDARD.success(),
        Theme::STANDARD.warning(),
        Theme::STANDARD.error(),
    ] {
        assert_eq!(style.bg, None);
    }
}

#[test]
fn semantic_states_have_non_color_emphasis() {
    assert!(THEME.success().add_modifier.contains(Modifier::BOLD));
    assert!(THEME.warning().add_modifier.contains(Modifier::BOLD));
    assert!(THEME.error().add_modifier.contains(Modifier::BOLD));
    assert!(THEME.disabled().add_modifier.contains(Modifier::DIM));
}
