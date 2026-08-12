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
#[path = "../../tests/unit/tui/viewport.rs"]
mod tests;
