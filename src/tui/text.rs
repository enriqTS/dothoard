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
mod tests {
    use super::*;

    #[test]
    fn editing_moves_and_deletes_at_utf8_boundaries() {
        let mut text = "aé界🙂z".to_string();
        let mut cursor = text.len();

        move_left(&text, &mut cursor);
        move_left(&text, &mut cursor);
        assert_eq!(&text[cursor..], "🙂z");

        assert!(delete(&mut text, &mut cursor));
        assert_eq!(text, "aé界z");
        assert!(backspace(&mut text, &mut cursor));
        assert_eq!(text, "aéz");

        insert_char(&mut text, &mut cursor, 'ø');
        assert_eq!(text, "aéøz");
        assert!(text.is_char_boundary(cursor));
    }

    #[test]
    fn invalid_cursor_is_safely_normalized() {
        let mut text = "éx".to_string();
        let mut cursor = 1;

        insert_char(&mut text, &mut cursor, 'a');

        assert_eq!(text, "aéx");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn terminal_width_handles_wide_and_combining_characters() {
        let text = "e\u{301}界x";
        let cursor = "e\u{301}界".len();

        assert_eq!(width_before_cursor(text, cursor), 3);
        assert_eq!(prefix(text, 3), "e\u{301}界");
        assert_eq!(truncate("界界界", 5), "界...");
        assert_eq!(UnicodeWidthStr::width(truncate("界界界", 5).as_str()), 5);
    }

    #[test]
    fn narrow_truncation_never_slices_utf8() {
        assert_eq!(truncate("界a", 0), "");
        assert_eq!(truncate("界a", 1), "");
        assert_eq!(truncate("界a", 2), "界");
        assert_eq!(truncate("éclair", 4), "é...");
    }

    #[test]
    fn wrapping_preserves_unicode_without_replacement_characters() {
        let lines = wrap("ab界cde\u{301}f", 4);

        assert_eq!(lines, vec!["ab界", "cde\u{301}f"]);
        assert!(
            lines
                .iter()
                .all(|line| UnicodeWidthStr::width(line.as_str()) <= 4)
        );
    }

    #[test]
    fn wrapping_respects_newlines_and_keeps_too_wide_characters_intact() {
        assert_eq!(wrap("a\n界b", 1), vec!["a", "界", "b"]);
    }
}
