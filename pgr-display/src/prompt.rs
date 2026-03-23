//! Prompt evaluation for the pager status line.
//!
//! Renders the three default prompt styles (short, medium, long) matching
//! the behavior of GNU less, plus a `%` escape mini-language for custom
//! prompt templates via the `-P` flag.

use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::Path;

/// Prompt style, matching less's `-m` / `-M` flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptStyle {
    /// Default: short prompt (`:` or `(END)`).
    Short,
    /// `-m`: medium prompt (filename and percent).
    Medium,
    /// `-M`: long prompt (filename, line numbers, percent, bytes).
    Long,
    /// Custom prompt template string (from `-P` flag).
    Custom(String),
}

/// Information available for prompt rendering.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // Bools mirror distinct less prompt conditions (at_eof, is_pipe, search_active, etc.)
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
    /// Column number of the cursor (1-based), for `%c`.
    pub column: usize,
    /// Current page number, for `%d` (if applicable).
    pub page_number: Option<usize>,
    /// Number of input lines (for pipes), for `%i`.
    pub input_line: Option<usize>,
    /// Size of the pipe in bytes (if known), for `%s`/`%S`.
    pub pipe_size: Option<u64>,
    /// Whether a search pattern is active, for condition `?s`.
    pub search_active: bool,
    /// Whether line numbers are enabled, for condition `?n`.
    pub line_numbers_enabled: bool,
    /// Whether any marks are set, for condition `?m`.
    pub marks_set: bool,
}

/// Render the prompt string for the given style and context.
///
/// Returns the text content of the prompt without any terminal formatting.
/// Use [`paint_prompt`] to write it to the terminal with reverse video.
#[must_use]
pub fn render_prompt(style: &PromptStyle, ctx: &PromptContext<'_>) -> String {
    match style {
        PromptStyle::Short => render_short(ctx),
        PromptStyle::Medium => render_medium(ctx),
        PromptStyle::Long => render_long(ctx),
        PromptStyle::Custom(template) => eval_prompt(template, ctx),
    }
}

/// Evaluate a prompt template string with full support for `%` escapes
/// and `?x conditional .` expressions.
///
/// Conditionals: `?x text .` includes `text` if condition `x` is true,
/// otherwise the text up to `.` is skipped. Conditionals may nest.
///
/// The `.` delimiter ends the conditional section. If no matching `.`
/// is found, the conditional extends to the end of the template.
///
/// Unknown `%` escapes are passed through literally (e.g., `%Z` becomes `%Z`).
/// `%%` produces a literal `%`.
#[must_use]
pub fn eval_prompt(template: &str, ctx: &PromptContext<'_>) -> String {
    let chars: Vec<char> = template.chars().collect();
    let mut pos = 0;
    eval_recursive(&chars, &mut pos, ctx, 0)
}

/// Recursive-descent evaluator for the prompt mini-language.
///
/// Processes characters from `pos` onward, handling `%` escapes, `?x...`
/// conditionals, and literal text. `depth` tracks nesting level so that
/// `.` is only treated as a conditional terminator inside a conditional body.
fn eval_recursive(
    chars: &[char],
    pos: &mut usize,
    ctx: &PromptContext<'_>,
    depth: usize,
) -> String {
    let mut result = String::new();

    while *pos < chars.len() {
        match chars[*pos] {
            '%' => {
                *pos += 1;
                if *pos < chars.len() {
                    expand_escape(chars[*pos], ctx, &mut result);
                    *pos += 1;
                } else {
                    // Trailing `%` at end of string: pass through literally
                    result.push('%');
                }
            }
            '?' => {
                *pos += 1;
                if *pos < chars.len() {
                    let flag = chars[*pos];
                    *pos += 1;
                    if evaluate_condition(flag, ctx) {
                        // Condition true: recurse to evaluate the body
                        result.push_str(&eval_recursive(chars, pos, ctx, depth + 1));
                    } else {
                        // Condition false: skip to matching `.`
                        skip_conditional_body(chars, pos);
                    }
                }
                // Trailing `?` at end of string: silently ignore
            }
            '.' if depth > 0 => {
                // End of conditional section: consume the `.` and return
                *pos += 1;
                return result;
            }
            ch => {
                result.push(ch);
                *pos += 1;
            }
        }
    }

    result
}

/// Evaluate a condition flag against the current prompt context.
///
/// Returns `true` if the condition is met, `false` otherwise.
/// Unknown flags always evaluate to `false`.
fn evaluate_condition(flag: char, ctx: &PromptContext<'_>) -> bool {
    match flag {
        'a' | 's' => ctx.search_active,
        'b' => ctx.top_line == 1,
        'e' => ctx.at_eof,
        'f' => !ctx.is_pipe,
        'l' => ctx.file_count > 1,
        'm' => ctx.marks_set,
        'n' => ctx.line_numbers_enabled,
        'p' => ctx.is_pipe,
        'B' => true, // Byte offset is always known
        'L' => ctx.total_lines.is_some(),
        'S' => !ctx.is_pipe || ctx.pipe_size.is_some(),
        _ => false,
    }
}

/// Skip past a conditional body when its condition is false.
///
/// Advances `pos` past the matching `.`, respecting nested `?`/`.` pairs.
/// If no matching `.` is found, advances to the end of the input.
fn skip_conditional_body(chars: &[char], pos: &mut usize) {
    let mut nesting: usize = 1;
    while *pos < chars.len() {
        match chars[*pos] {
            '?' => {
                *pos += 1;
                // Skip the flag character after `?`
                if *pos < chars.len() {
                    *pos += 1;
                }
                nesting += 1;
            }
            '.' => {
                *pos += 1;
                nesting -= 1;
                if nesting == 0 {
                    return;
                }
            }
            '%' => {
                // Skip the escape character after `%`
                *pos += 1;
                if *pos < chars.len() {
                    *pos += 1;
                }
            }
            _ => {
                *pos += 1;
            }
        }
    }
}

/// Expand a single `%` escape character into the result buffer.
fn expand_escape(escape: char, ctx: &PromptContext<'_>, out: &mut String) {
    match escape {
        '%' => out.push('%'),
        // %b and %o both expand to byte offset of the top of screen
        'b' | 'o' => {
            let _ = write!(out, "{}", ctx.byte_offset);
        }
        'B' => {
            let _ = write!(out, "{}", ctx.total_bytes);
        }
        'c' => {
            let _ = write!(out, "{}", ctx.column);
        }
        'd' => {
            let _ = write!(out, "{}", ctx.page_number.unwrap_or(0));
        }
        'D' => {
            // Number of pages: not directly available, stub as 0.
            // A full implementation would compute from total_lines and screen height,
            // but screen height is not in the prompt context.
            out.push('0');
        }
        'E' => {
            if !ctx.at_eof {
                out.push(' ');
            }
        }
        'f' => {
            out.push_str(ctx.filename.unwrap_or("(standard input)"));
        }
        'F' => {
            let name = ctx.filename.unwrap_or("(standard input)");
            let basename = Path::new(name)
                .file_name()
                .map_or(name, |os| os.to_str().unwrap_or(name));
            out.push_str(basename);
        }
        'i' => {
            let _ = write!(out, "{}", ctx.file_index + 1);
        }
        'l' => {
            let _ = write!(out, "{}", ctx.top_line);
        }
        'L' => match ctx.total_lines {
            Some(n) => {
                let _ = write!(out, "{n}");
            }
            None => out.push('?'),
        },
        'm' => {
            let _ = write!(out, "{}", ctx.file_count);
        }
        'M' => {
            if ctx.top_line == 1 && !ctx.at_eof {
                out.push_str("TOP");
            } else if ctx.at_eof {
                out.push_str("END");
            } else {
                let pct = compute_line_percent(ctx);
                let _ = write!(out, "{pct}%");
            }
        }
        // %p and %P both use byte-based percent (we approximate %P with the same
        // byte_offset since a separate bottom-byte-offset field is not yet available)
        'p' | 'P' => {
            let pct = compute_byte_percent(ctx);
            let _ = write!(out, "{pct}");
        }
        's' => {
            if ctx.is_pipe {
                if let Some(size) = ctx.pipe_size {
                    let _ = write!(out, "{size}");
                } else {
                    let _ = write!(out, "{}", ctx.total_bytes);
                }
            } else {
                let _ = write!(out, "{}", ctx.total_bytes);
            }
        }
        'S' => {
            if ctx.is_pipe && ctx.pipe_size.is_none() {
                out.push('?');
            } else if ctx.is_pipe {
                if let Some(size) = ctx.pipe_size {
                    let _ = write!(out, "{size}");
                }
            } else {
                let _ = write!(out, "{}", ctx.total_bytes);
            }
        }
        't' | 'T' | 'x' => {
            // Stubs: tags and next-file are deferred to later phases
        }
        _ => {
            // Unknown escape: pass through literally
            out.push('%');
            out.push(escape);
        }
    }
}

/// Render the prompt on the last row of the screen with the configured color.
///
/// Clears the row first, then renders using the provided SGR sequence.
/// If `prompt_sgr` is `None`, falls back to reverse video (standout mode),
/// matching less default behavior. The prompt text is truncated to fit
/// within `screen_cols`.
///
/// # Errors
///
/// Returns an error if writing to the underlying writer fails.
pub fn paint_prompt<W: Write>(
    writer: &mut W,
    prompt_text: &str,
    screen_rows: usize,
    screen_cols: usize,
    prompt_sgr: Option<&str>,
) -> std::io::Result<()> {
    // Move cursor to last row, column 1 (1-based ANSI coordinates)
    write!(writer, "\x1b[{screen_rows};1H")?;
    // Clear the entire line
    write!(writer, "\x1b[2K")?;
    // Truncate prompt to screen width
    let display_text: String = prompt_text.chars().take(screen_cols).collect();
    // Render with configured color or fallback to reverse video
    let sgr = prompt_sgr.unwrap_or("\x1b[7m");
    write!(writer, "{sgr}{display_text}\x1b[0m")?;
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
    let percent = compute_byte_percent(ctx);
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
    let percent = compute_byte_percent(ctx);

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
fn compute_byte_percent(ctx: &PromptContext<'_>) -> u64 {
    if ctx.total_bytes == 0 {
        return 0;
    }
    // Saturate at 100
    let raw = ctx.byte_offset.saturating_mul(100) / ctx.total_bytes;
    raw.min(100)
}

/// Compute the percentage through the file based on line numbers.
fn compute_line_percent(ctx: &PromptContext<'_>) -> u64 {
    match ctx.total_lines {
        Some(total) if total > 0 => {
            #[allow(clippy::cast_possible_truncation)] // Line counts fit in u64
            let raw = (ctx.bottom_line as u64).saturating_mul(100) / total as u64;
            raw.min(100)
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a basic file context with all fields.
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
            column: 1,
            page_number: None,
            input_line: None,
            pipe_size: None,
            search_active: false,
            line_numbers_enabled: false,
            marks_set: false,
        }
    }

    /// Helper to create a pipe context with all fields.
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
            column: 1,
            page_number: None,
            input_line: None,
            pipe_size: None,
            search_active: false,
            line_numbers_enabled: false,
            marks_set: false,
        }
    }

    /// Helper to create a full-featured context for eval_prompt tests.
    fn eval_ctx<'a>(
        filename: Option<&'a str>,
        top_line: usize,
        bottom_line: usize,
        total_lines: Option<usize>,
        byte_offset: u64,
        total_bytes: u64,
    ) -> PromptContext<'a> {
        PromptContext {
            filename,
            top_line,
            bottom_line,
            total_lines,
            total_bytes,
            byte_offset,
            file_index: 0,
            file_count: 1,
            at_eof: false,
            is_pipe: false,
            column: 1,
            page_number: None,
            input_line: None,
            pipe_size: None,
            search_active: false,
            line_numbers_enabled: false,
            marks_set: false,
        }
    }

    // ===== Existing render_prompt tests =====

    #[test]
    fn test_render_prompt_short_not_eof_returns_colon() {
        let ctx = file_ctx("test.txt", false, 1, 24, Some(100), 0, 5000);
        assert_eq!(render_prompt(&PromptStyle::Short, &ctx), ":");
    }

    #[test]
    fn test_render_prompt_short_at_eof_returns_end() {
        let ctx = file_ctx("test.txt", true, 77, 100, Some(100), 5000, 5000);
        assert_eq!(render_prompt(&PromptStyle::Short, &ctx), "(END)");
    }

    #[test]
    fn test_render_prompt_medium_file_shows_name_and_percent() {
        let ctx = file_ctx("filename", false, 1, 24, Some(100), 2100, 5000);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "filename 42%");
    }

    #[test]
    fn test_render_prompt_medium_pipe_shows_byte_offset() {
        let ctx = pipe_ctx(false, 1234, 5000);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "byte 1234");
    }

    #[test]
    fn test_render_prompt_medium_at_eof_returns_end() {
        let ctx = file_ctx("test.txt", true, 77, 100, Some(100), 5000, 5000);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "(END)");
    }

    #[test]
    fn test_render_prompt_long_file_known_total_shows_full_format() {
        let ctx = file_ctx("data.log", false, 10, 33, Some(200), 1500, 10000);
        assert_eq!(
            render_prompt(&PromptStyle::Long, &ctx),
            "data.log lines 10-33/200 byte 1500/10000 15%"
        );
    }

    #[test]
    fn test_render_prompt_long_file_unknown_total_omits_total() {
        let ctx = file_ctx("data.log", false, 10, 33, None, 1500, 10000);
        assert_eq!(
            render_prompt(&PromptStyle::Long, &ctx),
            "data.log lines 10-33 byte 1500/10000 15%"
        );
    }

    #[test]
    fn test_render_prompt_long_pipe_shows_byte_offset() {
        let ctx = pipe_ctx(false, 4096, 8192);
        assert_eq!(render_prompt(&PromptStyle::Long, &ctx), "byte 4096");
    }

    #[test]
    fn test_render_prompt_long_at_eof_shows_end_with_filename() {
        let ctx = file_ctx("readme.txt", true, 90, 100, Some(100), 5000, 5000);
        assert_eq!(
            render_prompt(&PromptStyle::Long, &ctx),
            "(END) - readme.txt"
        );
    }

    #[test]
    fn test_paint_prompt_output_includes_reverse_video_and_cursor() {
        let mut buf: Vec<u8> = Vec::new();
        paint_prompt(&mut buf, "test prompt", 24, 80, None).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(
            output.contains("\x1b[24;1H"),
            "missing cursor move: {output}"
        );
        assert!(output.contains("\x1b[2K"), "missing clear line: {output}");
        assert!(
            output.contains("\x1b[7m"),
            "missing reverse video start: {output}"
        );
        assert!(output.contains("\x1b[0m"), "missing reset: {output}");
        assert!(
            output.contains("test prompt"),
            "missing prompt text: {output}"
        );
    }

    #[test]
    fn test_paint_prompt_with_custom_sgr_uses_provided_color() {
        let mut buf: Vec<u8> = Vec::new();
        let custom_sgr = "\x1b[1;33m"; // bold yellow
        paint_prompt(&mut buf, "colored prompt", 24, 80, Some(custom_sgr)).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.contains(custom_sgr), "missing custom SGR: {output}");
        assert!(
            !output.contains("\x1b[7m"),
            "should not contain reverse video when custom SGR provided: {output}"
        );
        assert!(output.contains("\x1b[0m"), "missing reset: {output}");
    }

    #[test]
    fn test_paint_prompt_none_sgr_falls_back_to_reverse_video() {
        let mut buf: Vec<u8> = Vec::new();
        paint_prompt(&mut buf, "prompt", 24, 80, None).unwrap();
        let output = String::from_utf8(buf).unwrap();
        assert!(
            output.contains("\x1b[7m"),
            "should use reverse video when sgr is None: {output}"
        );
    }

    #[test]
    fn test_render_prompt_long_pipe_at_eof_returns_end() {
        let ctx = pipe_ctx(true, 8192, 8192);
        assert_eq!(render_prompt(&PromptStyle::Long, &ctx), "(END)");
    }

    #[test]
    fn test_render_prompt_medium_zero_bytes_shows_zero_percent() {
        let ctx = file_ctx("empty.txt", false, 1, 1, Some(1), 0, 0);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "empty.txt 0%");
    }

    // ===== Task 103: eval_prompt tests =====

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_literal_text_passes_through() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("hello", &ctx), "hello");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_percent_produces_literal_percent() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%%", &ctx), "%");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_f_expands_filename() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%f", &ctx), "test.txt");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_f_pipe_shows_standard_input() {
        let ctx = eval_ctx(None, 1, 24, None, 0, 5000);
        assert_eq!(eval_prompt("%f", &ctx), "(standard input)");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_f_shows_basename() {
        let ctx = eval_ctx(Some("/path/to/file.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%F", &ctx), "file.txt");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_b_shows_byte_offset() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 4096, 5000);
        assert_eq!(eval_prompt("%b", &ctx), "4096");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_b_shows_total_bytes() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%B", &ctx), "5000");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_l_shows_top_line() {
        let ctx = eval_ctx(Some("test.txt"), 42, 65, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%l", &ctx), "42");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_l_shows_total_lines() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(200), 0, 5000);
        assert_eq!(eval_prompt("%L", &ctx), "200");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_l_unknown_total_shows_question() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, None, 0, 5000);
        assert_eq!(eval_prompt("%L", &ctx), "?");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_m_shows_file_count() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 50, Some(100), 0, 5000);
        ctx.file_count = 3;
        assert_eq!(eval_prompt("%m", &ctx), "3");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_m_at_top_shows_top() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%M", &ctx), "TOP");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_m_at_eof_shows_end() {
        let mut ctx = eval_ctx(Some("test.txt"), 77, 100, Some(100), 5000, 5000);
        ctx.at_eof = true;
        assert_eq!(eval_prompt("%M", &ctx), "END");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_m_middle_shows_percent() {
        let ctx = eval_ctx(Some("test.txt"), 25, 50, Some(100), 2500, 5000);
        assert_eq!(eval_prompt("%M", &ctx), "50%");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_p_shows_byte_percent() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 250, 5000);
        assert_eq!(eval_prompt("%p", &ctx), "5");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_i_shows_file_index_one_based() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.file_index = 2;
        ctx.file_count = 5;
        assert_eq!(eval_prompt("%i", &ctx), "3");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_e_not_at_eof_returns_space() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%E", &ctx), " ");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_upper_e_at_eof_returns_empty() {
        let mut ctx = eval_ctx(Some("test.txt"), 77, 100, Some(100), 5000, 5000);
        ctx.at_eof = true;
        assert_eq!(eval_prompt("%E", &ctx), "");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_s_shows_size() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%s", &ctx), "5000");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_unknown_escape_passes_through() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%Z", &ctx), "%Z");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_at_end_of_string() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("hello%", &ctx), "hello%");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_complex_template() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 250, 5000);
        assert_eq!(
            eval_prompt("%f lines %l-%L %p%%", &ctx),
            "test.txt lines 1-100 5%"
        );
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_c_shows_column() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.column = 42;
        assert_eq!(eval_prompt("%c", &ctx), "42");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_eval_prompt_percent_t_stub_returns_empty() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("%t", &ctx), "");
    }

    /// Spec ref: SPECIFICATION.md 5.9
    #[test]
    fn test_render_prompt_custom_style_uses_eval_prompt() {
        let ctx = eval_ctx(Some("data.txt"), 10, 50, Some(200), 1000, 4000);
        let style = PromptStyle::Custom(String::from("File: %f Line: %l"));
        assert_eq!(render_prompt(&style, &ctx), "File: data.txt Line: 10");
    }

    // ===== Task 104: conditional expression tests =====

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_simple_conditional_true_includes_text() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.at_eof = true;
        assert_eq!(eval_prompt("?e (END) .", &ctx), " (END) ");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_simple_conditional_false_excludes_text() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?e (END) .", &ctx), "");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_conditional_with_percent_escape() {
        let ctx = eval_ctx(Some("readme.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?f %f .", &ctx), " readme.txt ");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_nested_conditional_both_true() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.at_eof = true;
        assert_eq!(
            eval_prompt("?f outer ?e inner . rest .", &ctx),
            " outer  inner  rest "
        );
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_nested_conditional_outer_false_skips_all() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        // `?p` is false because is_pipe is false
        assert_eq!(eval_prompt("?p pipe ?e eof . stuff .", &ctx), "");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_nested_conditional_inner_false_shows_outer() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        // `?f` is true (not pipe), `?p` is false (not pipe)
        assert_eq!(
            eval_prompt("?f file ?p pipe . rest .", &ctx),
            " file  rest "
        );
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_condition_b_at_beginning() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?b TOP .", &ctx), " TOP ");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_condition_b_not_at_beginning() {
        let ctx = eval_ctx(Some("test.txt"), 5, 28, Some(100), 500, 5000);
        assert_eq!(eval_prompt("?b TOP .", &ctx), "");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_condition_l_multiple_files() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.file_index = 1;
        ctx.file_count = 3;
        assert_eq!(eval_prompt("?l (%i of %m) .", &ctx), " (2 of 3) ");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_condition_n_line_numbers() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.line_numbers_enabled = true;
        assert_eq!(eval_prompt("?n lines on .", &ctx), " lines on ");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_unknown_condition_evaluates_false() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?Z text .", &ctx), "");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_unclosed_conditional_extends_to_end() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.at_eof = true;
        assert_eq!(eval_prompt("?e hello", &ctx), " hello");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_dot_outside_conditional_is_literal() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("hello.world", &ctx), "hello.world");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_mixed_escapes_and_conditionals() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.at_eof = true;
        // %f expands to "test.txt", ?e is true so " (END)" included, ?b is true (top_line==1) so " (TOP)" included
        assert_eq!(
            eval_prompt("%f?e (END).?b (TOP).", &ctx),
            "test.txt (END) (TOP)"
        );
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_empty_conditional_body() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.at_eof = true;
        assert_eq!(eval_prompt("?e.", &ctx), "");
    }
}
