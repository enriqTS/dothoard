use super::*;

#[test]
fn keeps_active_row_visible_at_both_edges() {
    let mut viewport = Viewport::default();
    viewport.set_height(3, 10);
    viewport.ensure_visible(4, 10);
    assert_eq!(viewport.visible_range(10), 2..5);

    viewport.ensure_visible(1, 10);
    assert_eq!(viewport.visible_range(10), 1..4);
}

#[test]
fn clamps_after_data_and_viewport_shrinkage() {
    let mut viewport = Viewport::default();
    viewport.set_height(4, 20);
    viewport.end(20);
    assert_eq!(viewport.offset(), 16);

    viewport.set_height(2, 5);
    assert_eq!(viewport.offset(), 3);
    viewport.set_height(0, 0);
    assert_eq!(viewport.offset(), 0);
    assert_eq!(viewport.visible_range(0), 0..0);
}

#[test]
fn empty_and_single_row_ranges_are_stable() {
    let mut viewport = Viewport::default();
    viewport.set_height(0, 0);
    assert_eq!(viewport.visible_range(0), 0..0);

    viewport.set_height(1, 1);
    viewport.ensure_visible(0, 1);
    assert_eq!(viewport.visible_range(1), 0..1);
}

#[test]
fn scroll_and_page_size_use_actual_height() {
    let mut viewport = Viewport::default();
    viewport.set_height(5, 20);
    viewport.scroll_down(viewport.page_size(), 20);
    assert_eq!(viewport.offset(), 5);
    viewport.scroll_up(2);
    assert_eq!(viewport.offset(), 3);
}
