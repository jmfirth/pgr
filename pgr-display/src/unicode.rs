//! Unicode display width calculation for terminal rendering.
//!
//! Handles UAX #11 East Asian Width, tab expansion, control character
//! display notation (`^X`), and column-to-byte-index mapping.

use unicode_width::UnicodeWidthChar;

/// Calculate the display width of a single character in terminal cells.
///
/// - Normal characters use UAX #11 width (1 for most, 2 for CJK fullwidth).
/// - Tab (`\t`) is not meaningful without column context; returns 0.
///   Use [`display_width_from`] for proper tab expansion.
/// - Newline (`\n`) returns 0 (line terminator, not displayed).
/// - Control characters (0x00-0x1F except `\t` and `\n`) display as `^X` (2 cells).
/// - DEL (0x7F) displays as `^?` (2 cells).
#[must_use]
pub fn char_width(c: char) -> usize {
    match c {
        '\t' | '\n' => 0,
        // Control chars (0x00..=0x1F except tab/newline) and DEL (0x7F): display as ^X
        c if c.is_ascii_control() => 2,
        c => UnicodeWidthChar::width(c).unwrap_or(0),
    }
}

/// Calculate the display width of a string in terminal cells.
///
/// Tabs are expanded to the next tab stop based on `tab_width`.
/// Control characters display as `^X` (2 cells). Newlines are width 0.
#[must_use]
pub fn display_width(s: &str, tab_width: usize) -> usize {
    display_width_from(s, 0, tab_width)
}

/// Calculate display width with a starting column position.
///
/// This is needed when a string begins mid-line, since tab expansion
/// depends on the current column. Returns the total number of terminal
/// cells the string occupies.
#[must_use]
pub fn display_width_from(s: &str, start_column: usize, tab_width: usize) -> usize {
    let mut col = start_column;
    for c in s.chars() {
        col += char_display_width(c, col, tab_width);
    }
    col - start_column
}

/// Truncate a string to fit within `max_width` terminal cells.
///
/// Returns `(truncated_str, actual_width)`. Multi-cell characters that
/// would straddle the boundary are not split; the truncation stops before
/// them. Tabs are expanded relative to column 0.
#[must_use]
pub fn truncate_to_width(s: &str, max_width: usize, tab_width: usize) -> (&str, usize) {
    let mut col: usize = 0;
    for (byte_idx, c) in s.char_indices() {
        let w = char_display_width(c, col, tab_width);
        if col + w > max_width {
            return (&s[..byte_idx], col);
        }
        col += w;
    }
    (s, col)
}

/// Find the byte index corresponding to a display column.
///
/// Returns `None` if `column` is beyond the string's display width.
/// Tabs are expanded relative to column 0.
#[must_use]
pub fn byte_index_at_column(s: &str, column: usize, tab_width: usize) -> Option<usize> {
    let mut col: usize = 0;
    for (byte_idx, c) in s.char_indices() {
        if col >= column {
            return Some(byte_idx);
        }
        col += char_display_width(c, col, tab_width);
    }
    if col >= column {
        Some(s.len())
    } else {
        None
    }
}

/// Compute the display width of a character given the current column.
///
/// Tab width depends on the current column (next tab stop). All other
/// characters delegate to [`char_width`] or specific control-char rules.
fn char_display_width(c: char, current_col: usize, tab_width: usize) -> usize {
    if c == '\t' {
        if tab_width == 0 {
            return 0;
        }
        tab_width - (current_col % tab_width)
    } else {
        char_width(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- char_width tests ---

    #[test]
    fn test_char_width_ascii_returns_one() {
        assert_eq!(char_width('a'), 1);
        assert_eq!(char_width('Z'), 1);
        assert_eq!(char_width('0'), 1);
        assert_eq!(char_width(' '), 1);
    }

    #[test]
    fn test_char_width_cjk_returns_two() {
        assert_eq!(char_width('\u{4e2d}'), 2); // '中'
        assert_eq!(char_width('\u{3042}'), 2); // 'あ' hiragana
    }

    #[test]
    fn test_char_width_combining_mark_returns_zero() {
        assert_eq!(char_width('\u{0301}'), 0); // combining acute accent
    }

    #[test]
    fn test_char_width_tab_returns_zero() {
        assert_eq!(char_width('\t'), 0);
    }

    #[test]
    fn test_char_width_newline_returns_zero() {
        assert_eq!(char_width('\n'), 0);
    }

    #[test]
    fn test_char_width_control_char_returns_two() {
        assert_eq!(char_width('\x01'), 2); // ^A
        assert_eq!(char_width('\x02'), 2); // ^B
        assert_eq!(char_width('\x1f'), 2); // ^_
    }

    #[test]
    fn test_char_width_del_returns_two() {
        assert_eq!(char_width('\x7f'), 2); // ^?
    }

    // --- display_width tests ---

    #[test]
    fn test_display_width_ascii_string_returns_length() {
        assert_eq!(display_width("hello", 8), 5);
    }

    #[test]
    fn test_display_width_tab_expansion_correct() {
        // "hello\tworld": "hello" = 5 cols, tab at col 5 -> next stop at 8 = 3 cells, "world" = 5
        assert_eq!(display_width("hello\tworld", 8), 13);
    }

    #[test]
    fn test_display_width_mixed_ascii_cjk_correct_sum() {
        // "a中b" = 1 + 2 + 1 = 4
        assert_eq!(display_width("a\u{4e2d}b", 8), 4);
    }

    #[test]
    fn test_display_width_empty_string_returns_zero() {
        assert_eq!(display_width("", 8), 0);
    }

    // --- display_width_from tests ---

    #[test]
    fn test_display_width_from_tab_at_col_zero_width_eight() {
        // Tab at col 0 with tab_width 8 -> 8 cells
        assert_eq!(display_width_from("\t", 0, 8), 8);
    }

    #[test]
    fn test_display_width_from_tab_at_col_three_width_five() {
        // Tab at col 3 with tab_width 8 -> next stop at 8 = 5 cells
        assert_eq!(display_width_from("\t", 3, 8), 5);
    }

    #[test]
    fn test_display_width_from_mid_line_ascii() {
        assert_eq!(display_width_from("abc", 10, 8), 3);
    }

    // --- truncate_to_width tests ---

    #[test]
    fn test_truncate_to_width_ascii_truncates_correctly() {
        let (s, w) = truncate_to_width("hello world", 5, 8);
        assert_eq!(s, "hello");
        assert_eq!(w, 5);
    }

    #[test]
    fn test_truncate_to_width_cjk_no_split() {
        // "a中b" widths: a=1, 中=2, b=1. Max 2 -> only 'a' fits (width 1),
        // '中' would need 2 more but 1+2=3 > 2.
        let (s, w) = truncate_to_width("a\u{4e2d}b", 2, 8);
        assert_eq!(s, "a");
        assert_eq!(w, 1);
    }

    #[test]
    fn test_truncate_to_width_cjk_exact_fit() {
        // "a中b" widths: a=1, 中=2, b=1. Max 3 -> 'a' + '中' fits (width 3).
        let (s, w) = truncate_to_width("a\u{4e2d}b", 3, 8);
        assert_eq!(s, "a\u{4e2d}");
        assert_eq!(w, 3);
    }

    #[test]
    fn test_truncate_to_width_empty_string_returns_empty() {
        let (s, w) = truncate_to_width("", 10, 8);
        assert_eq!(s, "");
        assert_eq!(w, 0);
    }

    #[test]
    fn test_truncate_to_width_string_fits_entirely() {
        let (s, w) = truncate_to_width("hi", 10, 8);
        assert_eq!(s, "hi");
        assert_eq!(w, 2);
    }

    // --- byte_index_at_column tests ---

    #[test]
    fn test_byte_index_at_column_ascii_returns_correct_index() {
        // "hello" -> col 3 is byte 3
        assert_eq!(byte_index_at_column("hello", 3, 8), Some(3));
    }

    #[test]
    fn test_byte_index_at_column_multibyte_utf8_correct() {
        // "a中b": 'a' = byte 0 (width 1), '中' = bytes 1..4 (width 2), 'b' = byte 4 (width 1)
        // Column 1 -> byte 1 (start of '中')
        assert_eq!(byte_index_at_column("a\u{4e2d}b", 1, 8), Some(1));
        // Column 3 -> byte 4 (start of 'b')
        assert_eq!(byte_index_at_column("a\u{4e2d}b", 3, 8), Some(4));
    }

    #[test]
    fn test_byte_index_at_column_beyond_string_returns_none() {
        assert_eq!(byte_index_at_column("hi", 10, 8), None);
    }

    #[test]
    fn test_byte_index_at_column_zero_returns_zero() {
        assert_eq!(byte_index_at_column("hello", 0, 8), Some(0));
    }

    #[test]
    fn test_byte_index_at_column_at_end_returns_len() {
        // "hi" = 2 cols wide. Column 2 -> byte index 2 (= s.len())
        assert_eq!(byte_index_at_column("hi", 2, 8), Some(2));
    }

    #[test]
    fn test_byte_index_at_column_empty_string_zero_returns_some() {
        assert_eq!(byte_index_at_column("", 0, 8), Some(0));
    }

    #[test]
    fn test_byte_index_at_column_empty_string_nonzero_returns_none() {
        assert_eq!(byte_index_at_column("", 1, 8), None);
    }
}
