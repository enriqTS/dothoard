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
