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
    /// Starting terminal row (1-based) for content rendering.
    /// Default 0 means start at row 1. Used to offset short files
    /// so content appears at the bottom of the screen (matching less).
    pub start_row: usize,
    /// Whether to display a 1-character status column on the left (`-J` flag).
    ///
    /// When active, a single column is reserved before line numbers (if both
    /// are active). Each line shows `*` for search matches, a mark letter for
    /// marked lines, or a space otherwise.
    pub show_status_column: bool,
    /// Per-line status column characters, parallel to the lines slice.
    ///
    /// Each entry is the character to display in the status column for that
    /// line index. Typically `' '` (space), `'*'` (search match), or a mark
    /// letter (`'a'`..`'z'`). Only used when `show_status_column` is `true`.
    pub status_column_chars: Vec<char>,
    /// Header lines to render before the scrollable content.
    ///
    /// These are always taken from the beginning of the file and rendered
    /// with reverse video. Only used when `--header=N` is active.
    pub header_line_contents: Vec<Option<String>>,
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
#[allow(clippy::too_many_lines)] // Rendering dispatch for chop/wrap modes with headers and status columns
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
    let chop_mode = screen.chop_mode();

    let status_col_width = usize::from(options.show_status_column);

    let ln_width = if options.show_line_numbers {
        if let Some(custom) = options.line_num_width {
            line_numbers::line_number_width_custom(options.total_lines, custom)
        } else {
            line_numbers::line_number_width(options.total_lines)
        }
    } else {
        0
    };

    let margin_width = status_col_width + ln_width;
    let content_cols = cols.saturating_sub(margin_width);

    // Move cursor to starting row (may be offset for short files).
    let first_row = if options.start_row > 0 {
        options.start_row
    } else {
        1
    };
    move_cursor(writer, first_row, 1)?;

    // Track the current terminal row (1-based) to account for wrapped lines.
    let mut screen_row: usize = first_row;

    // Render pinned header lines (reverse video) before scrollable content.
    let header_count = options.header_line_contents.len();
    screen_row = paint_header_lines(
        writer,
        screen,
        options,
        config,
        ln_width,
        content_cols,
        screen_row,
    )?;

    // Total rows available for scrollable content (accounts for header rows).
    let scrollable_rows = content_rows + header_count;
    let mut line_idx: usize = 0;

    while screen_row <= scrollable_rows {
        if screen_row > 1 {
            move_cursor(writer, screen_row, 1)?;
        }

        if let Some(Some(line_text)) = lines.get(line_idx) {
            if options.show_status_column {
                let ch = options
                    .status_column_chars
                    .get(line_idx)
                    .copied()
                    .unwrap_or(' ');
                let mut buf = [0u8; 4];
                let s = ch.encode_utf8(&mut buf);
                writer.write_all(s.as_bytes())?;
            }

            if options.show_line_numbers {
                let line_num = screen.top_line() + line_idx + 1;
                let formatted = line_numbers::format_line_number(line_num, ln_width);
                writer.write_all(formatted.as_bytes())?;
            }

            if chop_mode {
                // Chop mode: truncate at content_cols, apply markers
                let (rendered, width) =
                    render::render_line(line_text, h_offset, content_cols, config);
                if content_cols > 0 {
                    let full_width = render::line_display_width(line_text, config);
                    let truncated_right = full_width > h_offset + content_cols;
                    let (chopped, _) =
                        render::apply_chop_markers(&rendered, width, h_offset, truncated_right);
                    writer.write_all(chopped.as_bytes())?;
                } else {
                    writer.write_all(rendered.as_bytes())?;
                }
                clear_to_eol(writer)?;
                screen_row += 1;
            } else {
                // Wrap mode (default): render the full line and let the
                // terminal auto-wrap at the terminal width boundary.
                let render_width = if cols > 0 { usize::MAX / 2 } else { 0 };
                let (rendered, width) =
                    render::render_line(line_text, h_offset, render_width, config);
                writer.write_all(rendered.as_bytes())?;
                clear_to_eol(writer)?;

                // Calculate how many screen rows this line consumed.
                let rows_used = if cols == 0 {
                    1
                } else {
                    let total_display = margin_width + width;
                    if total_display <= cols {
                        1
                    } else {
                        // First row fills cols columns; each continuation
                        // row also fills cols columns.
                        let remaining = total_display.saturating_sub(cols);
                        1 + remaining.div_ceil(cols)
                    }
                };
                screen_row += rows_used;
            }
        } else {
            if options.show_status_column {
                writer.write_all(b" ")?;
            }
            if !options.suppress_tildes {
                writer.write_all(b"~")?;
            }
            clear_to_eol(writer)?;
            screen_row += 1;
        }

        line_idx += 1;
    }

    writer.flush()?;
    Ok(())
}

/// Render pinned header lines with reverse video.
///
/// Writes each header line in reverse video at the current `screen_row`,
/// incrementing it for each line rendered. Returns the updated `screen_row`.
///
/// # Errors
///
/// Returns an I/O error if writing to `writer` fails.
fn paint_header_lines<W: Write>(
    writer: &mut W,
    screen: &Screen,
    options: &PaintOptions,
    config: &RenderConfig,
    ln_width: usize,
    content_cols: usize,
    mut screen_row: usize,
) -> std::io::Result<usize> {
    let (_, cols) = screen.dimensions();
    let h_offset = screen.horizontal_offset();
    let chop_mode = screen.chop_mode();

    for (i, header_line) in options.header_line_contents.iter().enumerate() {
        if screen_row > 1 {
            move_cursor(writer, screen_row, 1)?;
        }
        writer.write_all(b"\x1b[7m")?;
        if let Some(text) = header_line {
            if options.show_status_column {
                writer.write_all(b" ")?;
            }
            if options.show_line_numbers {
                let line_num = i + 1;
                let formatted = line_numbers::format_line_number(line_num, ln_width);
                writer.write_all(formatted.as_bytes())?;
            }
            if chop_mode {
                let (rendered, width) = render::render_line(text, h_offset, content_cols, config);
                if content_cols > 0 {
                    let full_width = render::line_display_width(text, config);
                    let truncated_right = full_width > h_offset + content_cols;
                    let (chopped, _) =
                        render::apply_chop_markers(&rendered, width, h_offset, truncated_right);
                    writer.write_all(chopped.as_bytes())?;
                } else {
                    writer.write_all(rendered.as_bytes())?;
                }
            } else {
                let render_width = if cols > 0 { usize::MAX / 2 } else { 0 };
                let (rendered, _) = render::render_line(text, h_offset, render_width, config);
                writer.write_all(rendered.as_bytes())?;
            }
        }
        clear_to_eol(writer)?;
        writer.write_all(b"\x1b[0m")?;
        screen_row += 1;
    }
    Ok(screen_row)
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
#[allow(clippy::too_many_lines)] // Status column adds branches to each rendering path
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
    let chop_mode = screen.chop_mode();

    let status_col_width = usize::from(options.show_status_column);

    let ln_width = if options.show_line_numbers {
        if let Some(custom) = options.line_num_width {
            line_numbers::line_number_width_custom(options.total_lines, custom)
        } else {
            line_numbers::line_number_width(options.total_lines)
        }
    } else {
        0
    };

    let margin_width = status_col_width + ln_width;
    let content_cols = cols.saturating_sub(margin_width);

    // Move cursor to starting row (may be offset for short files).
    let first_row = if options.start_row > 0 {
        options.start_row
    } else {
        1
    };
    move_cursor(writer, first_row, 1)?;

    let mut screen_row: usize = first_row;

    // Render pinned header lines (reverse video) before scrollable content.
    let header_count = options.header_line_contents.len();
    screen_row = paint_header_lines(
        writer,
        screen,
        options,
        config,
        ln_width,
        content_cols,
        screen_row,
    )?;

    let scrollable_rows = content_rows + header_count;
    let mut line_idx: usize = 0;

    while screen_row <= scrollable_rows {
        if screen_row > 1 {
            move_cursor(writer, screen_row, 1)?;
        }

        if let Some(sl) = screen_lines.get(line_idx) {
            if let Some(ref line_text) = sl.content {
                if options.show_status_column {
                    let ch = options
                        .status_column_chars
                        .get(line_idx)
                        .copied()
                        .unwrap_or(' ');
                    let mut buf = [0u8; 4];
                    let s = ch.encode_utf8(&mut buf);
                    writer.write_all(s.as_bytes())?;
                }

                if options.show_line_numbers {
                    let formatted = line_numbers::format_line_number(sl.line_number, ln_width);
                    writer.write_all(formatted.as_bytes())?;
                }

                if chop_mode {
                    let (rendered, width) =
                        render::render_line(line_text, h_offset, content_cols, config);
                    if content_cols > 0 {
                        let full_width = render::line_display_width(line_text, config);
                        let truncated_right = full_width > h_offset + content_cols;
                        let (chopped, _) =
                            render::apply_chop_markers(&rendered, width, h_offset, truncated_right);
                        writer.write_all(chopped.as_bytes())?;
                    } else {
                        writer.write_all(rendered.as_bytes())?;
                    }
                    clear_to_eol(writer)?;
                    screen_row += 1;
                } else {
                    let render_width = if cols > 0 { usize::MAX / 2 } else { 0 };
                    let (rendered, width) =
                        render::render_line(line_text, h_offset, render_width, config);
                    writer.write_all(rendered.as_bytes())?;
                    clear_to_eol(writer)?;

                    let rows_used = if cols == 0 {
                        1
                    } else {
                        let total_display = margin_width + width;
                        if total_display <= cols {
                            1
                        } else {
                            let remaining = total_display.saturating_sub(cols);
                            1 + remaining.div_ceil(cols)
                        }
                    };
                    screen_row += rows_used;
                }
            } else {
                if options.show_status_column {
                    writer.write_all(b" ")?;
                }
                if !options.suppress_tildes {
                    writer.write_all(b"~")?;
                }
                clear_to_eol(writer)?;
                screen_row += 1;
            }
        } else {
            if options.show_status_column {
                writer.write_all(b" ")?;
            }
            if !options.suppress_tildes {
                writer.write_all(b"~")?;
            }
            clear_to_eol(writer)?;
            screen_row += 1;
        }

        line_idx += 1;
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
    writer.write_all(b"\x1b[2J\x1b[H")?;
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
        assert_eq!(output, b"\x1b[2J\x1b[H");
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
            start_row: 0,
            ..PaintOptions::default()
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
        // 20 columns total, line number column takes 8 -> 12 for content.
        // Chop mode is required to see truncation (wrap mode renders full lines).
        let mut screen = Screen::new(2, 20); // 1 content row
        screen.set_chop_mode(true);
        let lines: Vec<Option<String>> = vec![Some(
            "this is a longer line that should be truncated".to_string(),
        )];

        let config = RenderConfig::default();

        // Without line numbers: 20 cols of content (chop marker at col 20)
        let output_no_ln = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let str_no_ln = String::from_utf8_lossy(&output_no_ln);

        // With line numbers: 12 cols of content (chop marker at col 12)
        let options = PaintOptions {
            show_line_numbers: true,
            total_lines: 50,
            line_num_width: None,
            suppress_tildes: false,
            start_row: 0,
            ..PaintOptions::default()
        };
        let output_ln =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let str_ln = String::from_utf8_lossy(&output_ln);

        // The version with line numbers should have the line number prefix
        assert!(str_ln.contains("      1 "));
        // And should contain less content text (truncated earlier).
        // Chop marker replaces last char: "this is a longer li>" vs "this is a l>"
        assert!(
            str_no_ln.contains("this is a longer li>"),
            "expected chopped line without line nums: {str_no_ln}"
        );
        assert!(
            str_ln.contains("this is a l>"),
            "expected chopped line with line nums: {str_ln}"
        );
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
            start_row: 0,
            ..PaintOptions::default()
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

    // --- Chop mode truncation marker tests ---

    #[test]
    fn test_paint_screen_chop_mode_adds_right_marker() {
        let mut screen = Screen::new(2, 10); // 1 content row, 10 cols
        screen.set_chop_mode(true);
        // Line longer than 10 cols
        let lines: Vec<Option<String>> = vec![Some("abcdefghijklmno".to_string())];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        // Should contain `>` at the right edge
        assert!(
            output_str.contains('>'),
            "missing right truncation marker: {output_str}"
        );
        // The rendered text should be 10 chars with `>` as the last one
        assert!(
            output_str.contains("abcdefghi>"),
            "expected 'abcdefghi>' in output: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_chop_mode_no_marker_for_short_line() {
        let mut screen = Screen::new(2, 80); // 1 content row, 80 cols
        screen.set_chop_mode(true);
        let lines: Vec<Option<String>> = vec![Some("short line".to_string())];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        // No markers needed
        assert!(
            !output_str.contains('>'),
            "unexpected right marker: {output_str}"
        );
        assert!(
            !output_str.contains('<'),
            "unexpected left marker: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_chop_mode_no_left_marker_when_scrolled() {
        let mut screen = Screen::new(2, 10); // 1 content row, 10 cols
        screen.set_chop_mode(true);
        screen.set_horizontal_offset(5);
        // Line: "abcdefghijklmno" (15 chars). At h_offset=5, shows "fghijklmno"
        // full_width=15, h_offset+cols=15, so not truncated right.
        // GNU less does not display a left-side marker, so no `<` should appear.
        let lines: Vec<Option<String>> = vec![Some("abcdefghijklmno".to_string())];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        assert!(
            !output_str.contains('<'),
            "unexpected left marker (GNU less has none): {output_str}"
        );
        // Content should show "fghijklmno" (positions 5-14)
        assert!(
            output_str.contains("fghijklmno"),
            "missing expected content: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_chop_mode_right_marker_only_when_scrolled() {
        let mut screen = Screen::new(2, 10); // 1 content row, 10 cols
        screen.set_chop_mode(true);
        screen.set_horizontal_offset(5);
        // "abcdefghijklmnopqrst" (20 chars). h_offset=5, shows cols 5-14.
        // full_width=20 > 5+10=15, so truncated right too.
        // GNU less only shows > on the right, no < on the left.
        let lines: Vec<Option<String>> = vec![Some("abcdefghijklmnopqrst".to_string())];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        assert!(
            !output_str.contains('<'),
            "unexpected left marker (GNU less has none): {output_str}"
        );
        assert!(
            output_str.contains('>'),
            "missing right marker: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_no_chop_mode_no_markers() {
        let screen = Screen::new(2, 10); // chop mode is OFF by default
        let lines: Vec<Option<String>> = vec![Some("abcdefghijklmno".to_string())];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        // With chop mode off, the full line is rendered (terminal auto-wraps)
        // and no truncation markers are added.
        assert!(
            !output_str.contains('>'),
            "unexpected right marker when chop off: {output_str}"
        );
    }

    // --- Line wrapping tests ---

    #[test]
    fn test_paint_screen_wrap_mode_renders_full_line() {
        // 10-col screen, chop mode OFF (default). A 15-char line should
        // be rendered in full, letting the terminal auto-wrap.
        let screen = Screen::new(4, 10); // 3 content rows
        let lines: Vec<Option<String>> = vec![Some("abcdefghijklmno".to_string())];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        // The full line content should be present (not truncated at 10)
        assert!(
            output_str.contains("abcdefghijklmno"),
            "full line should be rendered in wrap mode: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_wrap_mode_accounts_for_wrapped_rows() {
        // 10-col screen with 3 content rows. A 25-char line consumes 3
        // screen rows (10+10+5), leaving no room for additional lines.
        let screen = Screen::new(4, 10); // 3 content rows
        let lines: Vec<Option<String>> = vec![
            Some("abcdefghijklmnopqrstuvwxy".to_string()), // 25 chars = 3 rows
            Some("second line".to_string()),
            Some("third line".to_string()),
        ];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        // The first line should be fully rendered
        assert!(
            output_str.contains("abcdefghijklmnopqrstuvwxy"),
            "first line should be fully rendered: {output_str}"
        );
        // The second line should NOT appear because the first line
        // consumed all 3 content rows.
        assert!(
            !output_str.contains("second line"),
            "second line should not fit when first line wraps to 3 rows: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_wrap_mode_short_lines_no_wrap() {
        // Lines shorter than terminal width behave identically to before.
        let screen = Screen::new(4, 80); // 3 content rows
        let lines: Vec<Option<String>> = vec![
            Some("short 1".to_string()),
            Some("short 2".to_string()),
            Some("short 3".to_string()),
        ];

        let config = RenderConfig::default();
        let output = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let output_str = String::from_utf8_lossy(&output);

        assert!(output_str.contains("short 1"));
        assert!(output_str.contains("short 2"));
        assert!(output_str.contains("short 3"));
    }

    // --- Status column rendering tests ---

    #[test]
    fn test_paint_screen_status_column_shows_chars() {
        let screen = Screen::new(4, 80); // 3 content rows
        let lines: Vec<Option<String>> = vec![
            Some("alpha".to_string()),
            Some("beta".to_string()),
            Some("gamma".to_string()),
        ];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_status_column: true,
            status_column_chars: vec!['*', 'a', ' '],
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        // The status chars should appear before each line's content
        assert!(
            output_str.contains("*alpha"),
            "expected '*' before 'alpha': {output_str}"
        );
        assert!(
            output_str.contains("abeta"),
            "expected 'a' before 'beta': {output_str}"
        );
        assert!(
            output_str.contains(" gamma"),
            "expected space before 'gamma': {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_status_column_search_match_indicator() {
        let screen = Screen::new(3, 80); // 2 content rows
        let lines: Vec<Option<String>> =
            vec![Some("match line".to_string()), Some("no match".to_string())];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_status_column: true,
            status_column_chars: vec!['*', ' '],
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        assert!(
            output_str.contains("*match line"),
            "expected '*' before match line: {output_str}"
        );
        assert!(
            output_str.contains(" no match"),
            "expected space before non-match line: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_status_column_with_line_numbers() {
        let screen = Screen::new(3, 80); // 2 content rows
        let lines: Vec<Option<String>> = vec![Some("hello".to_string()), Some("world".to_string())];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_status_column: true,
            status_column_chars: vec!['*', 'b'],
            show_line_numbers: true,
            total_lines: 10,
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        // Status column char comes before line number
        assert!(
            output_str.contains("*      1 hello"),
            "expected status char before line number: {output_str}"
        );
        assert!(
            output_str.contains("b      2 world"),
            "expected mark 'b' before line number: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_status_column_reduces_content_width() {
        // 20 columns total. Status column takes 1 -> 19 for content.
        let mut screen = Screen::new(2, 20); // 1 content row
        screen.set_chop_mode(true);
        let lines: Vec<Option<String>> = vec![Some(
            "this is a longer line that should be truncated".to_string(),
        )];

        let config = RenderConfig::default();

        // Without status column: 20 cols of content
        let output_no_status = capture_output(|w| paint_screen(w, &screen, &lines, &config));
        let str_no_status = String::from_utf8_lossy(&output_no_status);

        // With status column: 19 cols of content
        let options = PaintOptions {
            show_status_column: true,
            status_column_chars: vec![' '],
            ..PaintOptions::default()
        };
        let output_status =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let str_status = String::from_utf8_lossy(&output_status);

        // Without status: "this is a longer li>" (19 chars + ">")
        assert!(
            str_no_status.contains("this is a longer li>"),
            "expected chopped line without status: {str_no_status}"
        );
        // With status: " this is a longer l>" (space + 18 chars + ">")
        assert!(
            str_status.contains("this is a longer l>"),
            "expected chopped line with status: {str_status}"
        );
        // The status version should not contain the longer substring
        assert!(
            !str_status.contains("this is a longer li>"),
            "status column should reduce content width: {str_status}"
        );
    }

    #[test]
    fn test_paint_screen_status_column_beyond_eof_shows_space() {
        let screen = Screen::new(4, 80); // 3 content rows
        let lines: Vec<Option<String>> = vec![Some("only line".to_string()), None, None];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_status_column: true,
            status_column_chars: vec!['*'],
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        // First line has status char
        assert!(
            output_str.contains("*only line"),
            "expected '*' before content: {output_str}"
        );
        // Beyond-EOF lines should have space before tilde
        assert!(
            output_str.contains(" ~"),
            "expected space before tilde for beyond-EOF lines: {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_mapped_status_column_shows_chars() {
        let screen = Screen::new(4, 80); // 3 content rows
        let screen_lines = vec![
            ScreenLine {
                content: Some("first".to_string()),
                line_number: 1,
            },
            ScreenLine {
                content: Some("second".to_string()),
                line_number: 2,
            },
            ScreenLine {
                content: None,
                line_number: 0,
            },
        ];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_status_column: true,
            status_column_chars: vec!['*', 'a', ' '],
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_mapped(w, &screen, &screen_lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        assert!(
            output_str.contains("*first"),
            "expected '*' before 'first': {output_str}"
        );
        assert!(
            output_str.contains("asecond"),
            "expected 'a' before 'second': {output_str}"
        );
    }

    #[test]
    fn test_paint_screen_no_status_column_no_prefix() {
        let screen = Screen::new(3, 80);
        let lines: Vec<Option<String>> = vec![Some("content".to_string()), None];

        let config = RenderConfig::default();
        let options = PaintOptions {
            show_status_column: false,
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);

        // No status column prefix — content starts immediately after cursor positioning
        assert!(output_str.contains("content"));
        // Tilde should not be preceded by a space (no status column)
        // The tilde follows a cursor-move escape, not a space
        assert!(
            !output_str.contains(" ~"),
            "should not have space before tilde without status column: {output_str}"
        );
    }

    // ── Header lines rendering tests ─────────────────────────────────

    #[test]
    fn test_paint_screen_header_lines_rendered_with_reverse_video() {
        let mut screen = Screen::new(10, 80);
        screen.set_header_lines(2);
        let config = RenderConfig::default();
        let lines = vec![
            Some("scrollable line 1".to_string()),
            Some("scrollable line 2".to_string()),
        ];
        let options = PaintOptions {
            header_line_contents: vec![Some("header 1".to_string()), Some("header 2".to_string())],
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);
        // Reverse video escape \x1b[7m should precede header content
        assert!(output_str.contains("\x1b[7m"), "expected reverse video");
        assert!(output_str.contains("header 1"), "expected header 1");
        assert!(output_str.contains("header 2"), "expected header 2");
        // Reset escape \x1b[0m should follow each header line
        assert!(output_str.contains("\x1b[0m"), "expected SGR reset");
        // Scrollable content should also appear
        assert!(
            output_str.contains("scrollable line 1"),
            "expected scrollable content"
        );
    }

    #[test]
    fn test_paint_screen_no_headers_by_default() {
        let screen = Screen::new(10, 80);
        let config = RenderConfig::default();
        let lines = vec![Some("line 1".to_string())];
        let options = PaintOptions::default();
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);
        // No reverse video when no headers
        assert!(
            !output_str.contains("\x1b[7m"),
            "should not have reverse video without headers"
        );
    }

    #[test]
    fn test_paint_screen_header_with_line_numbers() {
        let mut screen = Screen::new(10, 80);
        screen.set_header_lines(1);
        let config = RenderConfig::default();
        let lines = vec![Some("content".to_string())];
        let options = PaintOptions {
            show_line_numbers: true,
            total_lines: 50,
            header_line_contents: vec![Some("header".to_string())],
            ..PaintOptions::default()
        };
        let output =
            capture_output(|w| paint_screen_with_options(w, &screen, &lines, &config, &options));
        let output_str = String::from_utf8_lossy(&output);
        // Header line number should be 1 (first line of file)
        assert!(
            output_str.contains("1"),
            "expected line number 1 for header: {output_str}"
        );
    }
}
