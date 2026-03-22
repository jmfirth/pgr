//! Prompt evaluation for the pager status line.
//!
//! Renders the three default prompt styles (short, medium, long) matching
//! the behavior of GNU less. The full prompt mini-language (`%` escapes,
//! conditionals, `-P` flag) is deferred to Phase 1.

use std::fmt::Write as FmtWrite;
use std::io::Write;

/// Prompt style, matching less's `-m` / `-M` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStyle {
    /// Default: short prompt (`:` or `(END)`).
    Short,
    /// `-m`: medium prompt (filename and percent).
    Medium,
    /// `-M`: long prompt (filename, line numbers, percent, bytes).
    Long,
}

/// Information available for prompt rendering.
#[derive(Debug)]
pub struct PromptContext<'a> {
    /// Filename being viewed, or `None` for pipes/stdin.
    pub filename: Option<&'a str>,
    /// First visible line number (1-based).
    pub top_line: usize,
    /// Last visible line number (1-based).
    pub bottom_line: usize,
    /// Total lines in file, or `None` if not fully indexed yet.
    pub total_lines: Option<usize>,
    /// Total bytes in the input.
    pub total_bytes: u64,
    /// Current byte offset of the top of the screen.
    pub byte_offset: u64,
    /// Zero-based index of current file in the file list.
    pub file_index: usize,
    /// Total number of files in the file list.
    pub file_count: usize,
    /// Whether the display is at the end of the file.
    pub at_eof: bool,
    /// Whether the input is a pipe (not a named file).
    pub is_pipe: bool,
}

/// Render the prompt string for the given style and context.
///
/// Returns the text content of the prompt without any terminal formatting.
/// Use [`paint_prompt`] to write it to the terminal with reverse video.
#[must_use]
pub fn render_prompt(style: PromptStyle, ctx: &PromptContext<'_>) -> String {
    match style {
        PromptStyle::Short => render_short(ctx),
        PromptStyle::Medium => render_medium(ctx),
        PromptStyle::Long => render_long(ctx),
    }
}

/// Render the prompt on the last row of the screen.
///
/// Clears the row first, then renders in standout (reverse video) mode,
/// matching less behavior. The prompt text is truncated to fit within
/// `screen_cols`.
///
/// # Errors
///
/// Returns an error if writing to the underlying writer fails.
pub fn paint_prompt<W: Write>(
    writer: &mut W,
    prompt_text: &str,
    screen_rows: usize,
    screen_cols: usize,
) -> std::io::Result<()> {
    // Move cursor to last row, column 1 (1-based ANSI coordinates)
    write!(writer, "\x1b[{screen_rows};1H")?;
    // Clear the entire line
    write!(writer, "\x1b[2K")?;
    // Truncate prompt to screen width
    let display_text: String = prompt_text.chars().take(screen_cols).collect();
    // Render in reverse video (standout mode)
    write!(writer, "\x1b[7m{display_text}\x1b[0m")?;
    writer.flush()
}

fn render_short(ctx: &PromptContext<'_>) -> String {
    if ctx.at_eof {
        String::from("(END)")
    } else {
        String::from(":")
    }
}

fn render_medium(ctx: &PromptContext<'_>) -> String {
    if ctx.at_eof {
        return String::from("(END)");
    }

    if ctx.is_pipe {
        let mut s = String::new();
        // Write cannot fail on String
        let _ = write!(s, "byte {}", ctx.byte_offset);
        return s;
    }

    let name = ctx.filename.unwrap_or("(standard input)");
    let percent = compute_percent(ctx);
    let mut s = String::new();
    let _ = write!(s, "{name} {percent}%");
    s
}

fn render_long(ctx: &PromptContext<'_>) -> String {
    if ctx.at_eof {
        return if ctx.is_pipe {
            String::from("(END)")
        } else {
            let name = ctx.filename.unwrap_or("(standard input)");
            let mut s = String::new();
            let _ = write!(s, "(END) - {name}");
            s
        };
    }

    if ctx.is_pipe {
        let mut s = String::new();
        let _ = write!(s, "byte {}", ctx.byte_offset);
        return s;
    }

    let name = ctx.filename.unwrap_or("(standard input)");
    let percent = compute_percent(ctx);

    let lines_part = match ctx.total_lines {
        Some(total) => format!("lines {}-{}/{total}", ctx.top_line, ctx.bottom_line),
        None => format!("lines {}-{}", ctx.top_line, ctx.bottom_line),
    };

    let mut s = String::new();
    let _ = write!(
        s,
        "{name} {lines_part} byte {}/{} {percent}%",
        ctx.byte_offset, ctx.total_bytes
    );
    s
}

/// Compute the percentage through the file based on byte offset.
fn compute_percent(ctx: &PromptContext<'_>) -> u64 {
    if ctx.total_bytes == 0 {
        return 0;
    }
    // Saturate at 100
    let raw = ctx.byte_offset.saturating_mul(100) / ctx.total_bytes;
    raw.min(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a basic file context.
    fn file_ctx<'a>(
        filename: &'a str,
        at_eof: bool,
        top_line: usize,
        bottom_line: usize,
        total_lines: Option<usize>,
        byte_offset: u64,
        total_bytes: u64,
    ) -> PromptContext<'a> {
        PromptContext {
            filename: Some(filename),
            top_line,
            bottom_line,
            total_lines,
            total_bytes,
            byte_offset,
            file_index: 0,
            file_count: 1,
            at_eof,
            is_pipe: false,
        }
    }

    /// Helper to create a pipe context.
    fn pipe_ctx(at_eof: bool, byte_offset: u64, total_bytes: u64) -> PromptContext<'static> {
        PromptContext {
            filename: None,
            top_line: 1,
            bottom_line: 24,
            total_lines: None,
            total_bytes,
            byte_offset,
            file_index: 0,
            file_count: 1,
            at_eof,
            is_pipe: true,
        }
    }

    // Test 1: Short prompt not at EOF
    #[test]
    fn test_render_prompt_short_not_eof_returns_colon() {
        let ctx = file_ctx("test.txt", false, 1, 24, Some(100), 0, 5000);
        assert_eq!(render_prompt(PromptStyle::Short, &ctx), ":");
    }

    // Test 2: Short prompt at EOF
    #[test]
    fn test_render_prompt_short_at_eof_returns_end() {
        let ctx = file_ctx("test.txt", true, 77, 100, Some(100), 5000, 5000);
        assert_eq!(render_prompt(PromptStyle::Short, &ctx), "(END)");
    }

    // Test 3: Medium prompt basic file
    #[test]
    fn test_render_prompt_medium_file_shows_name_and_percent() {
        let ctx = file_ctx("filename", false, 1, 24, Some(100), 2100, 5000);
        assert_eq!(render_prompt(PromptStyle::Medium, &ctx), "filename 42%");
    }

    // Test 4: Medium prompt pipe
    #[test]
    fn test_render_prompt_medium_pipe_shows_byte_offset() {
        let ctx = pipe_ctx(false, 1234, 5000);
        assert_eq!(render_prompt(PromptStyle::Medium, &ctx), "byte 1234");
    }

    // Test 5: Medium prompt at EOF
    #[test]
    fn test_render_prompt_medium_at_eof_returns_end() {
        let ctx = file_ctx("test.txt", true, 77, 100, Some(100), 5000, 5000);
        assert_eq!(render_prompt(PromptStyle::Medium, &ctx), "(END)");
    }

    // Test 6: Long prompt basic file with known total
    #[test]
    fn test_render_prompt_long_file_known_total_shows_full_format() {
        let ctx = file_ctx("data.log", false, 10, 33, Some(200), 1500, 10000);
        assert_eq!(
            render_prompt(PromptStyle::Long, &ctx),
            "data.log lines 10-33/200 byte 1500/10000 15%"
        );
    }

    // Test 7: Long prompt unknown total lines
    #[test]
    fn test_render_prompt_long_file_unknown_total_omits_total() {
        let ctx = file_ctx("data.log", false, 10, 33, None, 1500, 10000);
        assert_eq!(
            render_prompt(PromptStyle::Long, &ctx),
            "data.log lines 10-33 byte 1500/10000 15%"
        );
    }

    // Test 8: Long prompt pipe
    #[test]
    fn test_render_prompt_long_pipe_shows_byte_offset() {
        let ctx = pipe_ctx(false, 4096, 8192);
        assert_eq!(render_prompt(PromptStyle::Long, &ctx), "byte 4096");
    }

    // Test 9: Long prompt at EOF with filename
    #[test]
    fn test_render_prompt_long_at_eof_shows_end_with_filename() {
        let ctx = file_ctx("readme.txt", true, 90, 100, Some(100), 5000, 5000);
        assert_eq!(render_prompt(PromptStyle::Long, &ctx), "(END) - readme.txt");
    }

    // Test 10: paint_prompt output includes reverse video and cursor position
    #[test]
    fn test_paint_prompt_output_includes_reverse_video_and_cursor() {
        let mut buf: Vec<u8> = Vec::new();
        paint_prompt(&mut buf, "test prompt", 24, 80).unwrap();
        let output = String::from_utf8(buf).unwrap();

        // Should move cursor to row 24, col 1
        assert!(
            output.contains("\x1b[24;1H"),
            "missing cursor move: {output}"
        );
        // Should clear the line
        assert!(output.contains("\x1b[2K"), "missing clear line: {output}");
        // Should contain reverse video start
        assert!(
            output.contains("\x1b[7m"),
            "missing reverse video start: {output}"
        );
        // Should contain reset
        assert!(output.contains("\x1b[0m"), "missing reset: {output}");
        // Should contain the prompt text
        assert!(
            output.contains("test prompt"),
            "missing prompt text: {output}"
        );
    }

    // Additional edge case: Long prompt at EOF for pipe
    #[test]
    fn test_render_prompt_long_pipe_at_eof_returns_end() {
        let ctx = pipe_ctx(true, 8192, 8192);
        assert_eq!(render_prompt(PromptStyle::Long, &ctx), "(END)");
    }

    // Edge case: zero total bytes
    #[test]
    fn test_render_prompt_medium_zero_bytes_shows_zero_percent() {
        let ctx = file_ctx("empty.txt", false, 1, 1, Some(1), 0, 0);
        assert_eq!(render_prompt(PromptStyle::Medium, &ctx), "empty.txt 0%");
    }
}
