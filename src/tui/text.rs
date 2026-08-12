//! Unicode-safe text editing and terminal-width display helpers.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Insert one character at the cursor and advance to the next UTF-8 boundary.
pub(crate) fn insert_char(text: &mut String, cursor: &mut usize, value: char) {
    normalize_cursor(text, cursor);
    text.insert(*cursor, value);
    *cursor += value.len_utf8();
}

/// Delete the character before the cursor.
pub(crate) fn backspace(text: &mut String, cursor: &mut usize) -> bool {
    normalize_cursor(text, cursor);
    let Some(previous) = previous_boundary(text, *cursor) else {
        return false;
    };
    text.replace_range(previous..*cursor, "");
    *cursor = previous;
    true
}

/// Delete the character at the cursor.
pub(crate) fn delete(text: &mut String, cursor: &mut usize) -> bool {
    normalize_cursor(text, cursor);
    let Some(next) = next_boundary(text, *cursor) else {
        return false;
    };
    text.replace_range(*cursor..next, "");
    true
}

/// Move the cursor one Unicode scalar value to the left.
pub(crate) fn move_left(text: &str, cursor: &mut usize) {
    normalize_cursor(text, cursor);
    if let Some(previous) = previous_boundary(text, *cursor) {
        *cursor = previous;
    }
}

/// Move the cursor one Unicode scalar value to the right.
pub(crate) fn move_right(text: &str, cursor: &mut usize) {
    normalize_cursor(text, cursor);
    if let Some(next) = next_boundary(text, *cursor) {
        *cursor = next;
    }
}

/// Return the terminal-cell width of the text before the cursor.
pub(crate) fn width_before_cursor(text: &str, cursor: usize) -> usize {
    let mut boundary = cursor;
    normalize_cursor(text, &mut boundary);
    UnicodeWidthStr::width(&text[..boundary])
}

/// Return the terminal-cell width of complete text.
pub(crate) fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

/// Truncate text to terminal cells, adding `...` when it does not fit.
pub(crate) fn truncate(text: &str, max_width: usize) -> String {
    if UnicodeWidthStr::width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 3 {
        return prefix(text, max_width);
    }

    let mut result = prefix(text, max_width - 3);
    result.push_str("...");
    result
}

/// Return the longest character-boundary-safe prefix within `max_width` cells.
pub(crate) fn prefix(text: &str, max_width: usize) -> String {
    let mut result = String::new();
    let mut width = 0;

    for value in text.chars() {
        let value_width = UnicodeWidthChar::width(value).unwrap_or(0);
        if value_width > 0 && width + value_width > max_width {
            break;
        }
        result.push(value);
        width += value_width;
    }

    result
}

/// Wrap text into display-width-safe lines no wider than `max_width` cells.
///
/// Breaks preferentially fall on whitespace, so prose reads as words rather
/// than fragments split mid-word. A run with no whitespace at all — a path,
/// hash, or any single word wider than `max_width` — falls back to a
/// character-level break so content is never lost or corrupted.
pub(crate) fn wrap(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let max_width = max_width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        wrap_paragraph(paragraph, max_width, &mut lines);
    }
    lines
}

fn wrap_paragraph(text: &str, max_width: usize, lines: &mut Vec<String>) {
    if text.is_empty() {
        lines.push(String::new());
        return;
    }

    let mut line = String::new();
    let mut width = 0;
    // Byte offset and display width of `line` at its most recent whitespace
    // character, i.e. the last point where breaking would land between
    // words rather than inside one.
    let mut last_break: Option<usize> = None;

    for value in text.chars() {
        let value_width = UnicodeWidthChar::width(value).unwrap_or(0);

        if value_width > 0 && !line.is_empty() && width + value_width > max_width {
            if let Some(break_at) = last_break {
                let rest = line[break_at..].trim_start().to_string();
                line.truncate(break_at);
                lines.push(line.trim_end().to_string());
                width = UnicodeWidthStr::width(rest.as_str());
                line = rest;
            } else {
                lines.push(std::mem::take(&mut line));
                width = 0;
            }
            last_break = None;
        }

        if value.is_whitespace() {
            last_break = Some(line.len());
        }

        line.push(value);
        width += value_width;
        // A wide character (or an unbroken run with no earlier whitespace)
        // cannot fit in a narrower viewport without being corrupted. Keep it
        // intact on its own line and let Ratatui clip it.
        if width > max_width {
            lines.push(std::mem::take(&mut line));
            width = 0;
            last_break = None;
        }
    }

    if !line.is_empty() {
        lines.push(line.trim_end().to_string());
    }
}

fn normalize_cursor(text: &str, cursor: &mut usize) {
    *cursor = (*cursor).min(text.len());
    while !text.is_char_boundary(*cursor) {
        *cursor -= 1;
    }
}

fn previous_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[..cursor]
        .char_indices()
        .next_back()
        .map(|(index, _)| index)
}

fn next_boundary(text: &str, cursor: usize) -> Option<usize> {
    text[cursor..]
        .chars()
        .next()
        .map(|value| cursor + value.len_utf8())
}

#[cfg(test)]
#[path = "../../tests/unit/tui/text.rs"]
mod tests;
