//! Shared viewport state for bounded TUI lists and previews.

use std::ops::Range;

/// A viewport offset and its most recently rendered height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Viewport {
    offset: usize,
    height: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: 0,
            height: 1,
        }
    }
}

impl Viewport {
    #[cfg(test)]
    pub(crate) fn offset(self) -> usize {
        self.offset
    }

    #[cfg(test)]
    pub(crate) fn height(self) -> usize {
        self.height
    }

    /// Record the actual rendered height and clamp the offset to the data.
    pub(crate) fn set_height(&mut self, height: usize, total: usize) {
        self.height = height;
        self.clamp(total);
    }

    /// Keep an active row visible and clamp all state to the current data.
    pub(crate) fn ensure_visible(&mut self, active: usize, total: usize) {
        self.clamp(total);
        if total == 0 {
            return;
        }

        let active = active.min(total - 1);
        let effective_height = self.height.max(1);
        if active < self.offset {
            self.offset = active;
        } else if active >= self.offset.saturating_add(effective_height) {
            self.offset = active + 1 - effective_height;
        }
        self.clamp(total);
    }

    pub(crate) fn scroll_up(&mut self, amount: usize) {
        self.offset = self.offset.saturating_sub(amount);
    }

    pub(crate) fn scroll_down(&mut self, amount: usize, total: usize) {
        self.offset = self.offset.saturating_add(amount);
        self.clamp(total);
    }

    pub(crate) fn home(&mut self) {
        self.offset = 0;
    }

    pub(crate) fn end(&mut self, total: usize) {
        self.offset = self.max_offset(total);
    }

    pub(crate) fn page_size(self) -> usize {
        self.height.max(1)
    }

    pub(crate) fn visible_range(self, total: usize) -> Range<usize> {
        let start = self.offset.min(total);
        let end = start.saturating_add(self.height).min(total);
        start..end
    }

    pub(crate) fn clamp(&mut self, total: usize) {
        self.offset = self.offset.min(self.max_offset(total));
    }

    fn max_offset(self, total: usize) -> usize {
        if total == 0 {
            0
        } else {
            total.saturating_sub(self.height.max(1))
        }
    }
}

#[cfg(test)]
mod tests {
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
}
