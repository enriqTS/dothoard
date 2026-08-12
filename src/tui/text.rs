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

/// Wrap text into character-boundary-safe lines no wider than `max_width` cells.
pub(crate) fn wrap(text: &str, max_width: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }

    let max_width = max_width.max(1);
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut width = 0;

    for value in text.chars() {
        if value == '\n' {
            lines.push(line);
            line = String::new();
            width = 0;
            continue;
        }

        let value_width = UnicodeWidthChar::width(value).unwrap_or(0);
        if value_width > 0 && !line.is_empty() && width + value_width > max_width {
            lines.push(line);
            line = String::new();
            width = 0;
        }
        line.push(value);
        width += value_width;
        // A wide character cannot fit in a narrower viewport without being
        // corrupted. Keep it intact on its own line and let Ratatui clip it.
        if width > max_width {
            lines.push(line);
            line = String::new();
            width = 0;
        }
    }

    if !line.is_empty() {
        lines.push(line);
    }
    lines
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
