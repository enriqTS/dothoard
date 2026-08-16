//! Pointer hit testing for mouse and touchpad input.
//!
//! Rendering records the rectangles of interactive controls. Mouse handling
//! then uses those exact frame coordinates instead of duplicating responsive
//! layout calculations.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::layout::Rect;

use super::Screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClickAction {
    Tab(Screen),
    FocusContent,
    Key(KeyCode, KeyModifiers),
    PickerEntry(usize),
    PickerToggle(usize),
    Source(usize),
    IgnoreSource(usize),
    IgnorePattern(usize),
    Namespace(usize),
    History(usize),
    Theme(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollAction {
    Vertical,
    PickerEntries,
    PickerPreview,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Region {
    rect: Rect,
    click: Option<ClickAction>,
    scroll: Option<ScrollAction>,
}

#[derive(Debug, Default)]
pub(crate) struct PointerMap {
    regions: Vec<Region>,
}

impl PointerMap {
    pub(crate) fn clear(&mut self) {
        self.regions.clear();
    }

    pub(crate) fn click(&mut self, rect: Rect, action: ClickAction) {
        if rect.width > 0 && rect.height > 0 {
            self.regions.push(Region {
                rect,
                click: Some(action),
                scroll: None,
            });
        }
    }

    pub(crate) fn scroll(&mut self, rect: Rect, action: ScrollAction) {
        if rect.width > 0 && rect.height > 0 {
            self.regions.push(Region {
                rect,
                click: None,
                scroll: Some(action),
            });
        }
    }

    pub(crate) fn click_at(&self, column: u16, row: u16) -> Option<ClickAction> {
        self.regions
            .iter()
            .rev()
            .find(|region| region.click.is_some() && contains(region.rect, column, row))
            .and_then(|region| region.click)
    }

    pub(crate) fn scroll_at(&self, column: u16, row: u16) -> Option<ScrollAction> {
        self.regions
            .iter()
            .rev()
            .find(|region| region.scroll.is_some() && contains(region.rect, column, row))
            .and_then(|region| region.scroll)
    }
}

fn contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}
