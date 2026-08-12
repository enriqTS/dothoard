//! Three-pane filesystem picker widget.
//!
//! Renders the browser model as a ranger/yazi-style layout and translates
//! keyboard input into browser navigation actions. Designed to be embedded
//! in repository and source selection screens.

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use super::browser::{Browser, DirListing, EntryKind, Selection, SelectionError};
use super::selection::CheckState;
use super::{text, theme::THEME};

/// Type alias for the optional check-state callback.
///
/// When provided, the picker renders a checkbox prefix for each entry.
/// The function receives the absolute path of the entry and returns its state.
pub type CheckFn<'a> = &'a dyn Fn(&Path) -> CheckState;

/// Caller-specific presentation for the shared browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Presentation {
    /// Caller label identifying the selection context without duplicating footer help.
    pub label: &'static str,
    /// Render ASCII-only entry icons for limited terminals.
    pub ascii: bool,
}

impl Presentation {
    pub const REPOSITORY: Self = Self {
        label: "Repository",
        ascii: false,
    };
    pub const SOURCES: Self = Self {
        label: "Sources",
        ascii: false,
    };
    pub const DEFAULT: Self = Self {
        label: "Browser",
        ascii: false,
    };

    #[must_use]
    pub const fn ascii_safe(mut self) -> Self {
        self.ascii = true;
        self
    }
}

/// Actions that the picker can produce in response to key events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickerAction {
    /// Key was consumed by picker navigation (no external action needed).
    Consumed,
    /// User pressed Space to select the current entry.
    Select(Result<Selection, SelectionError>),
    /// User pressed Escape to cancel/close the picker.
    Cancel,
    /// Key was not consumed by the picker.
    NotConsumed,
}

/// Default viewport height when the actual height is unknown.
const DEFAULT_VIEWPORT: usize = 20;

/// Handle a key event for the browser. Returns the resulting action.
pub fn handle_key(browser: &mut Browser, key: KeyEvent, viewport_height: usize) -> PickerAction {
    let vh = if viewport_height == 0 {
        DEFAULT_VIEWPORT
    } else {
        viewport_height
    };

    match (key.modifiers, key.code) {
        // Navigation: Up/k
        (KeyModifiers::NONE, KeyCode::Up) | (KeyModifiers::NONE, KeyCode::Char('k')) => {
            if browser.move_up() {
                PickerAction::Consumed
            } else {
                // At top boundary — not consumed (allows parent to handle).
                PickerAction::NotConsumed
            }
        }
        // Navigation: Down/j
        (KeyModifiers::NONE, KeyCode::Down) | (KeyModifiers::NONE, KeyCode::Char('j')) => {
            browser.move_down();
            PickerAction::Consumed
        }
        // Navigate into directory: Right/l/Enter
        (KeyModifiers::NONE, KeyCode::Right)
        | (KeyModifiers::NONE, KeyCode::Char('l'))
        | (KeyModifiers::NONE, KeyCode::Enter) => {
            browser.enter_selected();
            PickerAction::Consumed
        }
        // Navigate to parent: Left/h
        (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h')) => {
            browser.go_parent();
            PickerAction::Consumed
        }
        // Select entry: Space
        (KeyModifiers::NONE, KeyCode::Char(' ')) => {
            let result = browser.try_select();
            PickerAction::Select(result)
        }
        // Home
        (KeyModifiers::NONE, KeyCode::Home) => {
            browser.move_home();
            PickerAction::Consumed
        }
        // End
        (KeyModifiers::NONE, KeyCode::End) => {
            browser.move_end();
            PickerAction::Consumed
        }
        // PageUp
        (KeyModifiers::NONE, KeyCode::PageUp) => {
            browser.page_up(vh);
            PickerAction::Consumed
        }
        // PageDown
        (KeyModifiers::NONE, KeyCode::PageDown) => {
            browser.page_down(vh);
            PickerAction::Consumed
        }
        // Refresh: Ctrl+R
        (KeyModifiers::CONTROL, KeyCode::Char('r')) => {
            browser.refresh_current();
            PickerAction::Consumed
        }
        // Cancel: Escape
        (KeyModifiers::NONE, KeyCode::Esc) => PickerAction::Cancel,
        _ => PickerAction::NotConsumed,
    }
}

/// Render the three-pane browser into the given area.
///
/// If `check_fn` is provided, a checkbox indicator is rendered before each
/// entry in the current pane: `[●]` for Explicit, `[◉]` for Inherited,
/// `[ ]` for Unchecked. Pass `None` for browsers that don't need checkboxes
/// (e.g., repository browser).
pub fn draw(frame: &mut Frame, area: Rect, browser: &mut Browser, check_fn: Option<CheckFn>) {
    draw_with_presentation(frame, area, browser, check_fn, Presentation::DEFAULT);
}

/// Render the picker with caller-specific context and icon mode.
pub fn draw_with_presentation(
    frame: &mut Frame,
    area: Rect,
    browser: &mut Browser,
    check_fn: Option<CheckFn>,
    presentation: Presentation,
) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Breadcrumb
            Constraint::Min(3),    // Panes
        ])
        .split(area);

    draw_breadcrumb(frame, outer[0], browser);
    draw_panes(
        frame,
        outer[1],
        browser,
        check_fn,
        presentation.ascii,
        presentation.label,
    );
}

/// Draw the breadcrumb/path header.
fn draw_breadcrumb(frame: &mut Frame, area: Rect, browser: &Browser) {
    let path_str = browser.current_dir().to_string_lossy();
    let path_display = text::truncate(&path_str, area.width.saturating_sub(1) as usize);
    let line = Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(path_display, THEME.heading()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// Draw the three panes: parent (left), current (center), preview (right).
fn draw_panes(
    frame: &mut Frame,
    area: Rect,
    browser: &mut Browser,
    check_fn: Option<CheckFn>,
    ascii: bool,
    label: &'static str,
) {
    // Adaptive layout: if terminal is narrow, collapse parent/preview.
    let constraints = if area.width >= 80 {
        vec![
            Constraint::Percentage(20), // Parent
            Constraint::Percentage(50), // Current
            Constraint::Percentage(30), // Preview
        ]
    } else if area.width >= 40 {
        vec![
            Constraint::Percentage(0),  // No parent
            Constraint::Percentage(60), // Current
            Constraint::Percentage(40), // Preview
        ]
    } else {
        vec![
            Constraint::Percentage(0),   // No parent
            Constraint::Percentage(100), // Current only
            Constraint::Percentage(0),   // No preview
        ]
    };

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Parent pane (left).
    if panes[0].width > 0 {
        draw_parent_pane(frame, panes[0], browser);
    }

    // Current directory pane (center).
    let viewport_height = panes[1].height.saturating_sub(2) as usize; // subtract borders
    browser.adjust_scroll_with_height(viewport_height);
    draw_current_pane(frame, panes[1], browser, check_fn, ascii, label);

    // Preview pane (right).
    if panes[2].width > 0 {
        draw_preview_pane(frame, panes[2], browser, ascii);
    }
}

/// Draw the parent directory pane.
fn draw_parent_pane(frame: &mut Frame, area: Rect, browser: &mut Browser) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(THEME.border(false))
        .title(Line::from(Span::styled(" Parent ", THEME.label())));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let listing = browser.parent_listing();
    let items: Vec<ListItem> = match listing {
        Some(DirListing::Entries(entries)) => entries
            .iter()
            .map(|e| {
                let style = entry_style(e.kind, false);
                ListItem::new(Line::from(Span::styled(
                    truncate_name(&e.display_name, inner.width as usize),
                    style,
                )))
            })
            .collect(),
        Some(DirListing::Error(_)) => {
            vec![ListItem::new(Line::from(Span::styled(
                "<error>",
                THEME.error(),
            )))]
        }
        None => Vec::new(),
    };

    let list = List::new(items);
    frame.render_widget(list, inner);
}

/// Draw the current directory pane with selection highlight.
fn draw_current_pane(
    frame: &mut Frame,
    area: Rect,
    browser: &mut Browser,
    check_fn: Option<CheckFn>,
    ascii: bool,
    label: &'static str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(THEME.border(true))
        .title(Line::from(Span::styled(
            format!(" ▶ Files [ACTIVE: {label}] "),
            THEME.focused(),
        )));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let listing = browser.current_listing().clone();
    let current_dir = browser.current_dir().to_path_buf();
    match listing {
        DirListing::Entries(entries) if entries.is_empty() => {
            let msg = Paragraph::new(Line::from(Span::styled(" (empty)", THEME.muted())));
            frame.render_widget(msg, inner);
        }
        DirListing::Entries(entries) => {
            let selected = browser.selected();
            let scroll = browser.scroll_offset();
            let viewport = inner.height as usize;

            // Selection marker plus optional checkbox prefix.
            let checkbox_width: u16 = if check_fn.is_some() { 4 } else { 0 };
            let marker_width: u16 = 2;

            let items: Vec<ListItem> = entries
                .iter()
                .enumerate()
                .skip(scroll)
                .take(viewport)
                .map(|(i, e)| {
                    let is_selected = i == selected;
                    let style = if is_selected {
                        entry_style(e.kind, true)
                    } else {
                        entry_style(e.kind, false)
                    };

                    let mut spans: Vec<Span> = vec![Span::styled(
                        if is_selected { "▶ " } else { "  " },
                        if is_selected {
                            THEME.focused()
                        } else {
                            THEME.muted()
                        },
                    )];

                    // Checkbox prefix.
                    if let Some(ref check) = check_fn {
                        let entry_path = current_dir.join(&e.name);
                        let state = check(&entry_path);
                        let (indicator, ind_style) = match state {
                            CheckState::Explicit => ("[●]", Style::default().fg(Color::Cyan)),
                            CheckState::Inherited => ("[◉]", Style::default().fg(Color::DarkGray)),
                            CheckState::Unchecked => ("[ ]", Style::default().fg(Color::DarkGray)),
                        };
                        spans.push(Span::styled(
                            format!("{indicator} "),
                            if is_selected {
                                ind_style.patch(THEME.selected())
                            } else {
                                ind_style
                            },
                        ));
                    }

                    let icon = entry_icon(e, ascii);
                    let name_width = inner
                        .width
                        .saturating_sub(3 + checkbox_width + marker_width)
                        as usize;
                    let name = truncate_name(&e.display_name, name_width);
                    spans.push(Span::styled(format!("{icon} "), style));
                    spans.push(Span::styled(name, style));

                    ListItem::new(Line::from(spans))
                })
                .collect();

            let list = List::new(items);
            frame.render_widget(list, inner);
        }
        DirListing::Error(e) => {
            let msg = Paragraph::new(Line::from(Span::styled(
                format!(" Error: {e}"),
                THEME.error(),
            )));
            frame.render_widget(msg, inner);
        }
    }
}

/// Draw the preview pane (directory contents or file metadata).
fn draw_preview_pane(frame: &mut Frame, area: Rect, browser: &mut Browser, ascii: bool) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(THEME.border(false))
        .title(Line::from(Span::styled(" Preview ", THEME.label())));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Get selected entry info.
    let selected_entry = browser.selected_entry().cloned();

    match selected_entry {
        Some(entry) if entry.kind == EntryKind::Directory => {
            // Show directory preview.
            let preview = browser.preview_listing();
            match preview {
                Some(DirListing::Entries(entries)) if entries.is_empty() => {
                    let msg = Paragraph::new(Line::from(Span::styled(" (empty)", THEME.muted())));
                    frame.render_widget(msg, inner);
                }
                Some(DirListing::Entries(entries)) => {
                    let items: Vec<ListItem> = entries
                        .iter()
                        .take(inner.height as usize)
                        .map(|e| {
                            let icon = entry_icon(e, ascii);
                            let name = truncate_name(
                                &e.display_name,
                                inner.width.saturating_sub(3) as usize,
                            );
                            ListItem::new(Line::from(vec![
                                Span::styled(format!(" {icon} "), entry_style(e.kind, false)),
                                Span::styled(name, entry_style(e.kind, false)),
                            ]))
                        })
                        .collect();
                    let list = List::new(items);
                    frame.render_widget(list, inner);
                }
                Some(DirListing::Error(e)) => {
                    let msg =
                        Paragraph::new(Line::from(Span::styled(format!(" {e}"), THEME.error())));
                    frame.render_widget(msg, inner);
                }
                None => {}
            }
        }
        Some(entry) => {
            // Show file/symlink metadata.
            let mut lines: Vec<Line> = Vec::new();
            lines.push(Line::from(Span::styled(
                format!(" {}", entry.display_name),
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));

            let kind_str = match entry.kind {
                EntryKind::File => "Regular file",
                EntryKind::Symlink => "Symbolic link",
                EntryKind::Special => "Special file",
                EntryKind::Error => "Unreadable",
                EntryKind::Directory => "Directory",
            };
            lines.push(Line::from(vec![
                Span::styled(" Type: ", THEME.label()),
                Span::raw(kind_str),
            ]));

            if let Some(size) = entry.size {
                lines.push(Line::from(vec![
                    Span::styled(" Size: ", THEME.label()),
                    Span::raw(format_size(size)),
                ]));
            }

            if entry.executable {
                lines.push(Line::from(vec![
                    Span::styled(" Exec: ", THEME.label()),
                    Span::styled("yes", Style::default().fg(Color::Green)),
                ]));
            }

            if let Some(ref target) = entry.link_target {
                lines.push(Line::from(vec![
                    Span::styled(" Target: ", THEME.label()),
                    Span::styled(target.clone(), Style::default().fg(Color::Magenta)),
                ]));
            }

            if entry.is_lossy {
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    " ⚠ Non-UTF-8 name (cannot select)",
                    THEME.warning(),
                )));
            }

            let paragraph = Paragraph::new(lines);
            frame.render_widget(paragraph, inner);
        }
        None => {
            let msg = Paragraph::new(Line::from(Span::styled(" (no selection)", THEME.muted())));
            frame.render_widget(msg, inner);
        }
    }
}

/// Get a short icon character for an entry.
fn entry_icon(entry: &super::browser::Entry, ascii: bool) -> &'static str {
    if ascii {
        match entry.kind {
            EntryKind::Directory => "D",
            EntryKind::Symlink => "L",
            EntryKind::File if entry.executable => "*",
            EntryKind::File => "F",
            EntryKind::Special => "!",
            EntryKind::Error => "x",
        }
    } else {
        match entry.kind {
            EntryKind::Directory => "▸",
            EntryKind::Symlink => "↪",
            EntryKind::File if entry.executable => "*",
            EntryKind::File => "·",
            EntryKind::Special => "!",
            EntryKind::Error => "x",
        }
    }
}

/// Get a style for an entry based on kind and selection state.
fn entry_style(kind: EntryKind, selected: bool) -> Style {
    let base = match kind {
        EntryKind::Directory => Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD),
        EntryKind::Symlink => Style::default().fg(Color::Magenta),
        EntryKind::File => Style::default().fg(Color::White),
        EntryKind::Special => Style::default().fg(Color::Yellow),
        EntryKind::Error => Style::default().fg(Color::Red),
    };
    if selected {
        base.patch(THEME.selected())
    } else {
        base
    }
}

/// Truncate a name to fit within a given width.
fn truncate_name(name: &str, max_width: usize) -> String {
    text::truncate(name, max_width)
}

/// Format a file size in human-readable form.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/tui/picker.rs"]
mod tests;
