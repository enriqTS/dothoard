//! Reusable modal and text-input presentation.
//!
//! Dialog state and key handling remain owned by each screen. This module only
//! provides a single, size-safe visual treatment so every sensitive action has
//! the same input ownership language.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::{text, theme::THEME};

/// Content for a confirmation or review dialog.
pub(crate) struct ModalSpec<'a> {
    pub title: &'a str,
    pub affected: Option<&'a str>,
    pub consequence: &'a str,
    pub validation: Option<(&'a str, Style)>,
    pub confirm: &'a str,
    pub cancel: &'a str,
}

/// Content for a shared Unicode-safe text editor dialog.
pub(crate) struct TextInputSpec<'a> {
    pub title: &'a str,
    pub label: &'a str,
    pub text: &'a str,
    pub cursor: usize,
    pub validation: Option<(&'a str, Style)>,
    pub submit: &'a str,
    pub cancel: &'a str,
}

/// Render a centered confirmation dialog above a de-emphasized background.
pub(crate) fn draw(frame: &mut Frame, area: Rect, spec: ModalSpec<'_>) {
    let width = area.width.saturating_sub(4).clamp(24, 76);
    let body_width = usize::from(width.saturating_sub(4)).max(1);
    let mut lines = Vec::new();
    if let Some(affected) = spec.affected {
        lines.push(Line::from(Span::styled("Affected: ", THEME.label())));
        lines.extend(
            text::wrap(affected, body_width)
                .into_iter()
                .map(|line| Line::from(Span::raw(line))),
        );
    }
    lines.push(Line::from(""));
    lines.extend(
        text::wrap(spec.consequence, body_width)
            .into_iter()
            .map(|line| Line::from(Span::raw(line))),
    );
    if let Some((message, style)) = spec.validation {
        lines.push(Line::from(""));
        lines.extend(
            text::wrap(message, body_width)
                .into_iter()
                .map(|line| Line::from(Span::styled(line, style))),
        );
    }
    lines.push(Line::from(""));
    lines.push(actions_line(spec.confirm, spec.cancel));
    if area.height <= 5 {
        fit_lines_to_dialog(&mut lines, area.height.saturating_sub(2) as usize);
    }

    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);
    let dialog = dialog_area(area, width, height);
    draw_backdrop(frame, area, dialog, spec.title);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        dialog_inner(dialog),
    );
}

/// Render a centered text editor with a visible terminal-cell cursor.
pub(crate) fn draw_text_input(frame: &mut Frame, area: Rect, spec: TextInputSpec<'_>) {
    let width = area.width.saturating_sub(4).clamp(24, 76);
    let body_width = usize::from(width.saturating_sub(4)).max(1);
    let mut lines = vec![Line::from(Span::styled(spec.label, THEME.label()))];
    let input_width = body_width.saturating_sub(2).max(1);
    let (display, cursor_cells) = input_window(spec.text, spec.cursor, input_width);
    lines.push(Line::from(vec![
        Span::styled("> ", THEME.focused()),
        Span::styled(display, THEME.selected()),
    ]));
    lines.push(Line::from(Span::styled(
        format!("  {}^", " ".repeat(cursor_cells)),
        THEME.focused(),
    )));
    if let Some((message, style)) = spec.validation {
        lines.push(Line::from(""));
        lines.extend(
            text::wrap(message, body_width)
                .into_iter()
                .map(|line| Line::from(Span::styled(line, style))),
        );
    }
    lines.push(Line::from(""));
    lines.push(actions_line(spec.submit, spec.cancel));
    if area.height <= 5 {
        fit_lines_to_dialog(&mut lines, area.height.saturating_sub(2) as usize);
    }

    let height = u16::try_from(lines.len().saturating_add(2)).unwrap_or(u16::MAX);
    let dialog = dialog_area(area, width, height);
    draw_backdrop(frame, area, dialog, spec.title);
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        dialog_inner(dialog),
    );
}

/// Drop secondary body content on short terminals while retaining the modal's
/// explicit action row. The complete value remains available after resize.
fn fit_lines_to_dialog(lines: &mut Vec<Line<'_>>, max_lines: usize) {
    if lines.len() <= max_lines {
        return;
    }
    let actions = lines.pop().expect("modal lines always include actions");
    if max_lines == 0 {
        return;
    }
    lines.truncate(max_lines.saturating_sub(2));
    if max_lines > 1 {
        lines.push(Line::from(Span::styled("…", THEME.muted())));
    }
    lines.push(actions);
}

fn draw_backdrop(frame: &mut Frame, area: Rect, dialog: Rect, title: &str) {
    frame.render_widget(Block::default().style(THEME.backdrop()), area);
    frame.render_widget(Clear, dialog);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.dialog())
            .title(Line::from(Span::styled(
                format!(" {title} "),
                THEME.heading(),
            ))),
        dialog,
    );
}

fn dialog_inner(dialog: Rect) -> Rect {
    Block::default().borders(Borders::ALL).inner(dialog)
}

fn actions_line(confirm: &str, cancel: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(confirm.to_string(), THEME.focused()),
        Span::raw(" · "),
        Span::styled(cancel.to_string(), THEME.focused()),
    ])
}

/// Return a centered rectangle clamped to the available terminal area.
pub(crate) fn dialog_area(area: Rect, desired_width: u16, desired_height: u16) -> Rect {
    let width = desired_width.min(area.width);
    let height = desired_height.min(area.height);
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

fn input_window(text_value: &str, cursor: usize, width: usize) -> (String, usize) {
    let mut cursor = cursor.min(text_value.len());
    while !text_value.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let cursor_width = text::width_before_cursor(text_value, cursor);
    if unicode_width::UnicodeWidthStr::width(text_value) <= width {
        return (text_value.to_string(), cursor_width);
    }

    // Keep the cursor visible by retaining the widest suffix ending at it that
    // fits. The original string is never sliced at a non-character boundary.
    let before = &text_value[..cursor];
    let mut start = before.len();
    while start > 0 {
        let previous = before[..start].char_indices().next_back().map(|(i, _)| i);
        let Some(previous) = previous else { break };
        let candidate = &text_value[previous..cursor];
        if unicode_width::UnicodeWidthStr::width(candidate) > width {
            break;
        }
        start = previous;
    }
    let display = text::prefix(&text_value[start..], width);
    let visible_cursor = text::width_before_cursor(&text_value[start..], cursor - start).min(width);
    (display, visible_cursor)
}

#[cfg(test)]
#[path = "../../tests/unit/tui/modal.rs"]
mod tests;
