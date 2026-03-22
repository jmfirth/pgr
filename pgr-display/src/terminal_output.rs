//! Terminal output primitives for screen painting.
//!
//! Provides functions to write rendered content to a terminal using
//! ANSI escape sequences for cursor positioning and screen clearing.

use std::io::Write;

use crate::render::{self, RawControlMode};
use crate::screen::Screen;

/// Paint the full screen content to the terminal.
///
/// Moves the cursor to (1,1), renders each content row, and clears to
/// end-of-line after each rendered line. Lines beyond the end of the
/// document are shown as `~` (tilde on an otherwise blank line).
///
/// `lines` should contain `content_rows` entries; each entry is `Some(text)`
/// for a document line or `None` for beyond-EOF.
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
pub fn paint_screen<W: Write>(
    writer: &mut W,
    screen: &Screen,
    lines: &[Option<String>],
    raw_mode: RawControlMode,
    tab_width: usize,
) -> std::io::Result<()> {
    let (_, cols) = screen.dimensions();
    let content_rows = screen.content_rows();
    let h_offset = screen.horizontal_offset();

    // Move cursor to top-left
    move_cursor(writer, 1, 1)?;

    for row in 0..content_rows {
        if row > 0 {
            // Move to the next row
            move_cursor(writer, row + 1, 1)?;
        }

        if let Some(Some(line_text)) = lines.get(row) {
            let (rendered, _) = render::render_line(line_text, h_offset, cols, tab_width, raw_mode);
            writer.write_all(rendered.as_bytes())?;
        } else {
            // Beyond EOF: display tilde
            writer.write_all(b"~")?;
        }

        // Clear to end of line
        clear_to_eol(writer)?;
    }

    writer.flush()?;
    Ok(())
}

/// Clear the entire screen.
///
/// Emits the ANSI escape `ESC[2J` (erase entire display).
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
pub fn clear_screen<W: Write>(writer: &mut W) -> std::io::Result<()> {
    writer.write_all(b"\x1b[2J")?;
    writer.flush()
}

/// Move the cursor to the given row and column (1-indexed).
///
/// Emits `ESC[{row};{col}H`.
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
pub fn move_cursor<W: Write>(writer: &mut W, row: usize, col: usize) -> std::io::Result<()> {
    write!(writer, "\x1b[{row};{col}H")
}

/// Show or hide the terminal cursor.
///
/// Emits `ESC[?25h` (show) or `ESC[?25l` (hide).
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
pub fn set_cursor_visible<W: Write>(writer: &mut W, visible: bool) -> std::io::Result<()> {
    if visible {
        writer.write_all(b"\x1b[?25h")
    } else {
        writer.write_all(b"\x1b[?25l")
    }
}

/// Clear from the cursor position to the end of the current line.
///
/// Emits `ESC[K`.
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
fn clear_to_eol<W: Write>(writer: &mut W) -> std::io::Result<()> {
    writer.write_all(b"\x1b[K")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: capture terminal output into a `Vec<u8>`.
    fn capture_output<F>(f: F) -> Vec<u8>
    where
        F: FnOnce(&mut Vec<u8>) -> std::io::Result<()>,
    {
        let mut buf = Vec::new();
        f(&mut buf).expect("write should succeed in test");
        buf
    }

    #[test]
    fn test_clear_screen_emits_correct_escape() {
        let output = capture_output(|w| clear_screen(w));
        assert_eq!(output, b"\x1b[2J");
    }

    #[test]
    fn test_move_cursor_emits_correct_escape() {
        let output = capture_output(|w| move_cursor(w, 5, 10));
        assert_eq!(output, b"\x1b[5;10H");
    }

    #[test]
    fn test_move_cursor_top_left_is_one_one() {
        let output = capture_output(|w| move_cursor(w, 1, 1));
        assert_eq!(output, b"\x1b[1;1H");
    }

    #[test]
    fn test_set_cursor_visible_show_emits_correct_escape() {
        let output = capture_output(|w| set_cursor_visible(w, true));
        assert_eq!(output, b"\x1b[?25h");
    }

    #[test]
    fn test_set_cursor_visible_hide_emits_correct_escape() {
        let output = capture_output(|w| set_cursor_visible(w, false));
        assert_eq!(output, b"\x1b[?25l");
    }

    #[test]
    fn test_paint_screen_produces_output_with_cursor_positioning() {
        let screen = Screen::new(4, 80); // 3 content rows
        let lines: Vec<Option<String>> = vec![
            Some("line 1".to_string()),
            Some("line 2".to_string()),
            Some("line 3".to_string()),
        ];

        let output = capture_output(|w| paint_screen(w, &screen, &lines, RawControlMode::Off, 8));
        let output_str = String::from_utf8_lossy(&output);

        // Should contain cursor positioning
        assert!(output_str.contains("\x1b[1;1H"));
        // Should contain the line content
        assert!(output_str.contains("line 1"));
        assert!(output_str.contains("line 2"));
        assert!(output_str.contains("line 3"));
        // Should contain clear-to-eol sequences
        assert!(output_str.contains("\x1b[K"));
    }

    #[test]
    fn test_paint_screen_beyond_eof_shows_tildes() {
        let screen = Screen::new(4, 80); // 3 content rows
        let lines: Vec<Option<String>> = vec![Some("only line".to_string()), None, None];

        let output = capture_output(|w| paint_screen(w, &screen, &lines, RawControlMode::Off, 8));
        let output_str = String::from_utf8_lossy(&output);

        assert!(output_str.contains("only line"));
        // Count tildes in the output (should be 2 for the 2 beyond-EOF rows)
        let tilde_count = output_str.matches('~').count();
        assert_eq!(tilde_count, 2);
    }

    #[test]
    fn test_paint_screen_empty_lines_all_tildes() {
        let screen = Screen::new(3, 80); // 2 content rows
        let lines: Vec<Option<String>> = vec![None, None];

        let output = capture_output(|w| paint_screen(w, &screen, &lines, RawControlMode::Off, 8));
        let output_str = String::from_utf8_lossy(&output);

        let tilde_count = output_str.matches('~').count();
        assert_eq!(tilde_count, 2);
    }

    #[test]
    fn test_paint_screen_zero_content_rows_produces_minimal_output() {
        let screen = Screen::new(1, 80); // 0 content rows
        let lines: Vec<Option<String>> = vec![];

        let output = capture_output(|w| paint_screen(w, &screen, &lines, RawControlMode::Off, 8));
        let output_str = String::from_utf8_lossy(&output);

        // Should still have cursor positioning to (1,1) but no content
        assert!(output_str.contains("\x1b[1;1H"));
    }
}
