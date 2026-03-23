//! Terminal output primitives for screen painting.
//!
//! Provides functions to write rendered content to a terminal using
//! ANSI escape sequences for cursor positioning and screen clearing.

use std::io::Write;

use crate::line_numbers;
use crate::render::{self, RenderConfig};
use crate::screen::Screen;

/// A visible line paired with its actual buffer line number.
///
/// Used by [`paint_screen`] to display content with optional line numbers.
/// The `line_number` is 1-based (the buffer line index + 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenLine {
    /// The text content of the line, or `None` for beyond-EOF.
    pub content: Option<String>,
    /// The 1-based line number from the buffer. Used when `-N` is active.
    pub line_number: usize,
}

/// Options controlling how [`paint_screen`] renders content.
#[derive(Debug, Clone, Default)]
pub struct PaintOptions {
    /// Whether to display line numbers in the left margin (`-N` flag).
    pub show_line_numbers: bool,
    /// Total number of lines in the document, used to size the line number column.
    pub total_lines: usize,
    /// Minimum width for the line number column (`--line-num-width`). Default: 7.
    pub line_num_width: Option<usize>,
    /// Suppress tilde display for lines past EOF (`--tilde` / `-~` flag).
    /// When true, beyond-EOF rows are rendered as blank instead of `~`.
    pub suppress_tildes: bool,
}

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
    config: &RenderConfig,
) -> std::io::Result<()> {
    paint_screen_with_options(writer, screen, lines, config, &PaintOptions::default())
}

/// Paint the full screen content with extended options.
///
/// Like [`paint_screen`] but accepts [`PaintOptions`] for line numbers
/// and other display flags. When `options.show_line_numbers` is true,
/// each content line is prefixed with its 1-based line number. The line
/// number is derived from the position in the `lines` slice plus the
/// screen's `top_line`.
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
pub fn paint_screen_with_options<W: Write>(
    writer: &mut W,
    screen: &Screen,
    lines: &[Option<String>],
    config: &RenderConfig,
    options: &PaintOptions,
) -> std::io::Result<()> {
    let (_, cols) = screen.dimensions();
    let content_rows = screen.content_rows();
    let h_offset = screen.horizontal_offset();

    let ln_width = if options.show_line_numbers {
        if let Some(custom) = options.line_num_width {
            line_numbers::line_number_width_custom(options.total_lines, custom)
        } else {
            line_numbers::line_number_width(options.total_lines)
        }
    } else {
        0
    };

    let content_cols = cols.saturating_sub(ln_width);

    // Move cursor to top-left
    move_cursor(writer, 1, 1)?;

    for row in 0..content_rows {
        if row > 0 {
            // Move to the next row
            move_cursor(writer, row + 1, 1)?;
        }

        if let Some(Some(line_text)) = lines.get(row) {
            if options.show_line_numbers {
                // Line number is top_line + row + 1 (1-based)
                let line_num = screen.top_line() + row + 1;
                let formatted = line_numbers::format_line_number(line_num, ln_width);
                writer.write_all(formatted.as_bytes())?;
            }
            let (rendered, _) = render::render_line(line_text, h_offset, content_cols, config);
            writer.write_all(rendered.as_bytes())?;
        } else if !options.suppress_tildes {
            // Beyond EOF: display tilde (unless suppressed by --tilde)
            writer.write_all(b"~")?;
        }

        // Clear to end of line
        clear_to_eol(writer)?;
    }

    writer.flush()?;
    Ok(())
}

/// Paint the screen with explicit line-number-to-content mappings.
///
/// Each entry in `screen_lines` pairs optional content with the actual
/// buffer line number. This is used when squeeze mode is active and
/// the displayed lines don't map sequentially from `top_line`.
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
pub fn paint_screen_mapped<W: Write>(
    writer: &mut W,
    screen: &Screen,
    screen_lines: &[ScreenLine],
    config: &RenderConfig,
    options: &PaintOptions,
) -> std::io::Result<()> {
    let (_, cols) = screen.dimensions();
    let content_rows = screen.content_rows();
    let h_offset = screen.horizontal_offset();

    let ln_width = if options.show_line_numbers {
        if let Some(custom) = options.line_num_width {
            line_numbers::line_number_width_custom(options.total_lines, custom)
        } else {
            line_numbers::line_number_width(options.total_lines)
        }
    } else {
        0
    };

    let content_cols = cols.saturating_sub(ln_width);

    // Move cursor to top-left
    move_cursor(writer, 1, 1)?;

    for row in 0..content_rows {
        if row > 0 {
            move_cursor(writer, row + 1, 1)?;
        }

        if let Some(sl) = screen_lines.get(row) {
            if let Some(ref line_text) = sl.content {
                if options.show_line_numbers {
                    let formatted = line_numbers::format_line_number(sl.line_number, ln_width);
                    writer.write_all(formatted.as_bytes())?;
                }
                let (rendered, _) = render::render_line(line_text, h_offset, content_cols, config);
                writer.write_all(rendered.as_bytes())?;
            } else if !options.suppress_tildes {
                // Beyond EOF: display tilde (unless suppressed)
                writer.write_all(b"~")?;
            }
        } else if !options.suppress_tildes {
            writer.write_all(b"~")?;
        }

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

/// Paint an error message on the status line with error color.
///
/// Displays the message on the last row of the screen using the provided
/// `error_sgr` sequence. If `error_sgr` is `None`, falls back to bold
/// reverse video (matching less default error styling).
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
pub fn paint_error_message<W: Write>(
    writer: &mut W,
    message: &str,
    screen_rows: usize,
    screen_cols: usize,
    error_sgr: Option<&str>,
) -> std::io::Result<()> {
    // Move cursor to last row, column 1
    write!(writer, "\x1b[{screen_rows};1H")?;
    // Clear the entire line
    write!(writer, "\x1b[2K")?;
    // Truncate message to screen width
    let display_text: String = message.chars().take(screen_cols).collect();
    // Render with configured color or fallback to bold reverse video
    let sgr = error_sgr.unwrap_or("\x1b[1;7m");
    write!(writer, "{sgr}{display_text}\x1b[0m")?;
    writer.flush()
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

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
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

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
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

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        let tilde_count = output_str.matches('~').count();
        assert_eq!(tilde_count, 2);
    }

    #[test]
    fn test_paint_screen_zero_content_rows_produces_minimal_output() {
        let screen = Screen::new(1, 80); // 0 content rows
        let lines: Vec<Option<String>> = vec![];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        // Should still have cursor positioning to (1,1) but no content
        assert!(output_str.contains("\x1b[1;1H"));
    }

    // --- Line number rendering tests ---

    #[test]
    fn test_paint_screen_with_line_numbers_shows_numbers() {
        let screen = Screen::new(4, 80); // 3 content rows
        let lines: Vec<Option<String>> = vec![
            Some("alpha".to_string()),
            Some("beta".to_string()),
            Some("gamma".to_string()),
        ];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_line_numbers: true,
            total_lines: 100,
            line_num_width: None,
            suppress_tildes: false,
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        // Should contain line numbers 1, 2, 3 (right-aligned in 8-wide column)
        assert!(output_str.contains("      1 "));
        assert!(output_str.contains("      2 "));
        assert!(output_str.contains("      3 "));
        // Should also contain the content
        assert!(output_str.contains("alpha"));
        assert!(output_str.contains("beta"));
        assert!(output_str.contains("gamma"));
    }

    #[test]
    fn test_paint_screen_line_numbers_reduce_content_width() {
        // 20 columns total, line number column takes 8 -> 12 for content
        let screen = Screen::new(2, 20); // 1 content row
        let lines: Vec<Option<String>> = vec![Some(
            "this is a longer line that should be truncated".to_string(),
        )];

        let config = RenderConfig::default();

        // Without line numbers: 20 cols of content
        let output_no_ln = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let str_no_ln = String::from_utf8_lossy(&output_no_ln);

        // With line numbers: 12 cols of content
        let options = PaintOptions {
            show_line_numbers: true,
            total_lines: 50,
            line_num_width: None,
            suppress_tildes: false,
        };
        let output_ln =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let str_ln = String::from_utf8_lossy(&output_ln);

        // The version with line numbers should have the line number prefix
        assert!(str_ln.contains("      1 "));
        // And should contain less content text (truncated earlier)
        // "this is a longer" fits in 20 cols; with line nums only 12 cols
        assert!(str_no_ln.contains("this is a longer lin"));
        assert!(str_ln.contains("this is a lo"));
        assert!(!str_ln.contains("this is a longer"));
    }

    #[test]
    fn test_paint_screen_line_numbers_disabled_no_prefix() {
        let screen = Screen::new(2, 80);
        let lines: Vec<Option<String>> = vec![Some("content".to_string())];

        let config = RenderConfig::default();
        let options = PaintOptions::default();
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        // Should NOT contain any line number formatting
        assert!(!output_str.contains("      1 "));
        assert!(output_str.contains("content"));
    }

    // --- Mapped paint (squeeze + line numbers) ---

    #[test]
    fn test_paint_screen_mapped_with_squeeze_line_numbers() {
        let screen = Screen::new(4, 80); // 3 content rows
        let screen_lines = vec![
            ScreenLine {
                content: Some("first".to_string()),
                line_number: 1,
            },
            ScreenLine {
                content: Some("".to_string()),
                line_number: 2,
            },
            // Lines 3, 4 were squeezed; line 5 is next
            ScreenLine {
                content: Some("fifth".to_string()),
                line_number: 5,
            },
        ];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_line_numbers: true,
            total_lines: 10,
            line_num_width: None,
            suppress_tildes: false,
        };
        let output =
            capture_output(|w| paint_screen_mapped(w, &screen, &screen_lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        // Should show line numbers 1, 2, 5 (skipping squeezed 3, 4)
        assert!(output_str.contains("      1 "));
        assert!(output_str.contains("      2 "));
        assert!(output_str.contains("      5 "));
        assert!(!output_str.contains("      3 "));
        assert!(!output_str.contains("      4 "));
    }

    #[test]
    fn test_paint_screen_mapped_beyond_eof_shows_tildes() {
        let screen = Screen::new(4, 80); // 3 content rows
        let screen_lines = vec![
            ScreenLine {
                content: Some("only line".to_string()),
                line_number: 1,
            },
            ScreenLine {
                content: None,
                line_number: 0,
            },
        ];

        let config = RenderConfig::default();
        let options = PaintOptions::default();
        let output =
            capture_output(|w| paint_screen_mapped(w, &screen, &screen_lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        assert!(output_str.contains("only line"));
        // 2 tildes: one from screen_lines[1] (None content), one from missing row 2
        let tilde_count = output_str.matches('~').count();
        assert_eq!(tilde_count, 2);
    }

    // --- Error message rendering tests ---

    #[test]
    fn test_paint_error_message_with_custom_sgr_uses_provided_color() {
        let custom_sgr = "\x1b[1;31m"; // bold red
        let output = capture_output(|w| {
            paint_error_message(w, "Pattern not found", 24, 80, Some(custom_sgr))
        });
        let output_str = String::from_utf8_lossy(&output);

        assert!(
            output_str.contains(custom_sgr),
            "missing custom SGR: {output_str}"
        );
        assert!(
            output_str.contains("Pattern not found"),
            "missing error message: {output_str}"
        );
        assert!(
            output_str.contains("\x1b[0m"),
            "missing reset: {output_str}"
        );
    }

    #[test]
    fn test_paint_error_message_none_sgr_falls_back_to_bold_reverse() {
        let output = capture_output(|w| paint_error_message(w, "error msg", 24, 80, None));
        let output_str = String::from_utf8_lossy(&output);

        assert!(
            output_str.contains("\x1b[1;7m"),
            "should use bold reverse video when sgr is None: {output_str}"
        );
        assert!(
            output_str.contains("error msg"),
            "missing error text: {output_str}"
        );
        assert!(
            output_str.contains("\x1b[0m"),
            "missing reset: {output_str}"
        );
    }

    #[test]
    fn test_paint_error_message_positions_cursor_on_last_row() {
        let output = capture_output(|w| paint_error_message(w, "test", 24, 80, None));
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("\x1b[24;1H"),
            "missing cursor positioning: {output_str}"
        );
    }

    #[test]
    fn test_paint_error_message_clears_line() {
        let output = capture_output(|w| paint_error_message(w, "test", 24, 80, None));
        let output_str = String::from_utf8_lossy(&output);
        assert!(
            output_str.contains("\x1b[2K"),
            "missing line clear: {output_str}"
        );
    }
}
