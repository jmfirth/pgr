//! Line rendering for terminal display.
//!
//! Handles tab expansion, control character notation (`^X`), ANSI escape
//! passthrough, horizontal scrolling, and width truncation.

use crate::ansi::{self, Segment};
use unicode_width::UnicodeWidthChar;

/// Controls how raw control characters and ANSI escapes are handled during rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawControlMode {
    /// Default: ANSI escapes are stripped, control characters render as `^X`.
    Off,
    /// `-R` flag: ANSI SGR (color/style) sequences are passed through,
    /// other control characters render as `^X`.
    AnsiOnly,
    /// `-r` flag: everything is passed through raw, no interpretation.
    All,
}

/// Render a single line for terminal display.
///
/// Applies horizontal offset (skipping leading display columns), tab
/// expansion, control character notation, and ANSI escape handling
/// according to `raw_mode`. The output is truncated to `max_width`
/// display columns.
///
/// Returns `(rendered_string, display_width)`.
#[must_use]
pub fn render_line(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_width: usize,
    raw_mode: RawControlMode,
) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }

    match raw_mode {
        RawControlMode::Off => render_off(line, horizontal_offset, max_width, tab_width),
        RawControlMode::AnsiOnly => render_ansi_only(line, horizontal_offset, max_width, tab_width),
        RawControlMode::All => render_all(line, horizontal_offset, max_width),
    }
}

/// Render with all ANSI escapes stripped and control chars as `^X`.
fn render_off(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_width: usize,
) -> (String, usize) {
    let stripped = ansi::strip_ansi(line);
    let chars: Vec<char> = stripped.chars().collect();
    render_chars(&chars, horizontal_offset, max_width, tab_width, false)
}

/// Render with ANSI escapes preserved, control chars as `^X`.
fn render_ansi_only(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_width: usize,
) -> (String, usize) {
    let segments = ansi::parse_ansi(line);
    let mut output = String::with_capacity(line.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;

    for segment in segments {
        match segment {
            Segment::Escape(esc) => {
                // Always pass through ANSI escapes (zero display width)
                if skipped >= horizontal_offset {
                    output.push_str(esc);
                }
            }
            Segment::Text(text) => {
                for c in text.chars() {
                    if visible_width >= max_width {
                        break;
                    }

                    let char_w = expanded_width(c, col, tab_width);

                    // Handle horizontal offset: skip leading columns
                    if skipped < horizontal_offset {
                        let remaining_to_skip = horizontal_offset - skipped;
                        if char_w <= remaining_to_skip {
                            skipped += char_w;
                            col += char_w;
                            continue;
                        }
                        // Partial skip for wide chars: skip entirely
                        skipped += char_w;
                        col += char_w;
                        continue;
                    }

                    // Truncate if this char would exceed max_width
                    if visible_width + char_w > max_width {
                        break;
                    }

                    let expansion = expand_char(c, col, tab_width);
                    output.push_str(&expansion);
                    visible_width += char_w;
                    col += char_w;
                }
            }
        }
    }

    (output, visible_width)
}

/// Render in raw passthrough mode: everything goes through as-is.
fn render_all(line: &str, horizontal_offset: usize, max_width: usize) -> (String, usize) {
    // In raw mode we can't accurately measure display width since we don't
    // interpret escapes or control chars. We do a best-effort byte slice.
    // For `less -r` compatibility, we pass through the entire line and let
    // the terminal sort it out.
    let mut output = String::with_capacity(line.len());
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;

    for c in line.chars() {
        let w = raw_char_width(c);

        if skipped < horizontal_offset {
            skipped += w;
            continue;
        }

        if visible_width + w > max_width && w > 0 {
            break;
        }

        output.push(c);
        visible_width += w;
    }

    (output, visible_width)
}

/// Compute the display width of a character after expansion.
///
/// Returns the number of terminal cells the expanded form occupies.
/// This must stay in sync with [`expand_char`].
fn expanded_width(c: char, current_col: usize, tab_width: usize) -> usize {
    match c {
        '\t' => {
            if tab_width == 0 {
                0
            } else {
                tab_width - (current_col % tab_width)
            }
        }
        '\n' | '\r' => 0,
        '\x7f' => 2,
        c if c.is_ascii_control() => 2,
        c => UnicodeWidthChar::width(c).unwrap_or(0),
    }
}

/// Expand a character into its display representation.
///
/// - Tabs expand to spaces based on current column and tab width.
/// - Control characters become `^X` notation.
/// - Newlines and carriage returns are ignored (stripped).
/// - Normal characters pass through.
fn expand_char(c: char, current_col: usize, tab_width: usize) -> String {
    match c {
        '\t' => {
            if tab_width == 0 {
                return String::new();
            }
            let spaces = tab_width - (current_col % tab_width);
            " ".repeat(spaces)
        }
        '\n' | '\r' => String::new(),
        '\x7f' => "^?".to_string(),
        c if c.is_ascii_control() => {
            let mut s = String::with_capacity(2);
            s.push('^');
            // Control chars 0x00-0x1F map to ^@, ^A, ..., ^_
            #[allow(clippy::cast_possible_truncation)] // c is in 0x00..=0x1F, always fits u8
            let display = (c as u8 + b'@') as char;
            s.push(display);
            s
        }
        _ => {
            let mut s = String::with_capacity(c.len_utf8());
            s.push(c);
            s
        }
    }
}

/// Get the display width of a character in raw passthrough mode.
///
/// In raw mode, escape characters and control characters still occupy
/// their natural terminal behavior, but we estimate width for offset logic.
fn raw_char_width(c: char) -> usize {
    match c {
        '\x1b' | '\n' | '\r' => 0,
        c if c.is_ascii_control() => 0,
        c => UnicodeWidthChar::width(c).unwrap_or(0),
    }
}

/// Render a character slice with offset, width limit, and tab/control handling.
fn render_chars(
    chars: &[char],
    horizontal_offset: usize,
    max_width: usize,
    tab_width: usize,
    _pass_ansi: bool,
) -> (String, usize) {
    let mut output = String::with_capacity(chars.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;

    for &c in chars {
        if visible_width >= max_width {
            break;
        }

        let char_w = expanded_width(c, col, tab_width);

        // Handle horizontal offset
        if skipped < horizontal_offset {
            let remaining = horizontal_offset - skipped;
            if char_w <= remaining {
                skipped += char_w;
                col += char_w;
                continue;
            }
            // For partial skips (e.g., tab partially visible), skip entirely
            skipped += char_w;
            col += char_w;
            continue;
        }

        if visible_width + char_w > max_width {
            break;
        }

        let expansion = expand_char(c, col, tab_width);
        output.push_str(&expansion);
        visible_width += char_w;
        col += char_w;
    }

    (output, visible_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Plain ASCII rendering ---

    #[test]
    fn test_render_line_plain_ascii_renders_as_is() {
        let (rendered, width) = render_line("hello world", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "hello world");
        assert_eq!(width, 11);
    }

    // --- Tab expansion ---

    #[test]
    fn test_render_line_tab_expands_correctly() {
        let (rendered, width) = render_line("\thello", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "        hello");
        assert_eq!(width, 13);
    }

    #[test]
    fn test_render_line_tab_mid_line_expands_to_next_stop() {
        // "ab\tc": 'a'=col0, 'b'=col1, tab at col2 -> 6 spaces to col8, 'c'=col8
        let (rendered, width) = render_line("ab\tc", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "ab      c");
        assert_eq!(width, 9);
    }

    // --- Control characters in Off mode ---

    #[test]
    fn test_render_line_control_chars_off_mode_renders_caret() {
        let (rendered, width) = render_line("a\x01b", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "a^Ab");
        assert_eq!(width, 4);
    }

    #[test]
    fn test_render_line_del_renders_as_caret_question() {
        let (rendered, _) = render_line("a\x7fb", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "a^?b");
    }

    // --- ANSI escapes in Off mode ---

    #[test]
    fn test_render_line_ansi_off_mode_strips_escapes() {
        let (rendered, width) = render_line("\x1b[31mred\x1b[0m", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "red");
        assert_eq!(width, 3);
    }

    // --- ANSI escapes in AnsiOnly mode ---

    #[test]
    fn test_render_line_ansi_only_mode_preserves_escapes() {
        let (rendered, width) =
            render_line("\x1b[31mred\x1b[0m", 0, 80, 8, RawControlMode::AnsiOnly);
        assert_eq!(rendered, "\x1b[31mred\x1b[0m");
        assert_eq!(width, 3);
    }

    #[test]
    fn test_render_line_ansi_only_control_chars_rendered_as_caret() {
        let (rendered, _) = render_line("\x1b[31m\x01\x1b[0m", 0, 80, 8, RawControlMode::AnsiOnly);
        assert_eq!(rendered, "\x1b[31m^A\x1b[0m");
    }

    // --- Horizontal offset ---

    #[test]
    fn test_render_line_horizontal_offset_skips_columns() {
        let (rendered, width) = render_line("hello world", 6, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "world");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_render_line_horizontal_offset_beyond_line_returns_empty() {
        let (rendered, width) = render_line("hello", 20, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "");
        assert_eq!(width, 0);
    }

    // --- Width truncation ---

    #[test]
    fn test_render_line_truncates_at_max_width() {
        let (rendered, width) = render_line("hello world", 0, 5, 8, RawControlMode::Off);
        assert_eq!(rendered, "hello");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_render_line_zero_max_width_returns_empty() {
        let (rendered, width) = render_line("hello", 0, 0, 8, RawControlMode::Off);
        assert_eq!(rendered, "");
        assert_eq!(width, 0);
    }

    // --- CJK characters ---

    #[test]
    fn test_render_line_cjk_correct_width() {
        // '中' = 2 cells, '文' = 2 cells
        let (rendered, width) = render_line("\u{4e2d}\u{6587}", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "\u{4e2d}\u{6587}");
        assert_eq!(width, 4);
    }

    #[test]
    fn test_render_line_cjk_truncation_no_split() {
        // '中' = 2 cells. Max width 3: fits '中' (2), next '文' (2) won't fit.
        let (rendered, width) = render_line("\u{4e2d}\u{6587}", 0, 3, 8, RawControlMode::Off);
        assert_eq!(rendered, "\u{4e2d}");
        assert_eq!(width, 2);
    }

    // --- All mode (raw passthrough) ---

    #[test]
    fn test_render_line_all_mode_passes_everything() {
        let input = "\x1b[31m\x01raw\x1b[0m";
        let (rendered, _) = render_line(input, 0, 80, 8, RawControlMode::All);
        // In All mode, everything passes through including escapes and control chars
        assert!(rendered.contains("\x1b[31m"));
        assert!(rendered.contains("\x01"));
        assert!(rendered.contains("raw"));
    }

    // --- Empty input ---

    #[test]
    fn test_render_line_empty_input_returns_empty() {
        let (rendered, width) = render_line("", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "");
        assert_eq!(width, 0);
    }

    // --- Newlines stripped ---

    #[test]
    fn test_render_line_newline_stripped() {
        let (rendered, width) = render_line("hello\n", 0, 80, 8, RawControlMode::Off);
        assert_eq!(rendered, "hello");
        assert_eq!(width, 5);
    }

    // --- expand_char unit tests ---

    #[test]
    fn test_expand_char_tab_at_col_zero_gives_full_width() {
        let result = expand_char('\t', 0, 8);
        assert_eq!(result, "        ");
    }

    #[test]
    fn test_expand_char_control_a_gives_caret_a() {
        let result = expand_char('\x01', 0, 8);
        assert_eq!(result, "^A");
    }

    #[test]
    fn test_expand_char_null_gives_caret_at() {
        let result = expand_char('\x00', 0, 8);
        assert_eq!(result, "^@");
    }

    #[test]
    fn test_expand_char_normal_char_passes_through() {
        let result = expand_char('a', 0, 8);
        assert_eq!(result, "a");
    }
}
