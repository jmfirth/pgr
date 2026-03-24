//! Prompt evaluation for the pager status line.
//!
//! Renders the three default prompt styles (short, medium, long) matching
//! the behavior of GNU less, plus a `%` escape mini-language for custom
//! prompt templates via the `-P` flag.
//!
//! Default prompts are expressed as mini-language templates rather than
//! hardcoded logic, so `eval_prompt` is the single rendering path for all
//! prompt styles including custom `-P` strings.

use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::Path;

/// Short prompt template: shows `(END)` at EOF, filename when known, otherwise `:`.
///
/// Matches the default less short prompt: at EOF show `(END)`, otherwise show
/// the filename if known, otherwise show `:`.
pub const DEFAULT_SHORT_PROMPT: &str = "?e(END):?f%f:\\:.";

/// Medium prompt template (`-m`): filename and percent, `(END)` at EOF.
///
/// Matches less's default medium prompt: filename (if known), then `(END)` at
/// EOF or byte-based percentage otherwise.
pub const DEFAULT_MEDIUM_PROMPT: &str = "?f%f .?e(END) :?pB%pB\\%..";

/// Long prompt template (`-M`): filename, line numbers, byte offset, percent.
///
/// Matches less's default long prompt: filename, multi-file indicator, line
/// range, byte offset and size, then `(END)` at EOF or percentage.
pub const DEFAULT_LONG_PROMPT: &str =
    "?f%f .?n?m(file %i of %m) ..?ltlines %lt-%lb?L/%L. :byte %bB?s/%s. .?e(END) :?pB%pB\\%..";

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
    /// Whether a search pattern is active, for condition `?a`.
    pub search_active: bool,
    /// The current search pattern string (for future `%S` escape).
    pub search_pattern: Option<&'a str>,
    /// Whether line numbers are enabled.
    pub line_numbers_enabled: bool,
    /// Whether any marks are set.
    pub marks_set: bool,
    /// Whether a filter is active, for condition `?u` (unfiltered input available).
    pub filter_active: bool,
    /// The current filter pattern string.
    pub filter_pattern: Option<&'a str>,
    /// Whether the input has been fully read (for pipe/stdin `?x` conditional).
    pub input_complete: bool,
}

/// Render the prompt string for the given style and context.
///
/// All prompt styles—including the three defaults—are rendered through
/// [`eval_prompt`] using template strings, so there is a single rendering
/// path for built-in and custom (`-P`) prompts.
///
/// Returns the text content of the prompt without any terminal formatting.
/// Use [`paint_prompt`] to write it to the terminal with reverse video.
#[must_use]
pub fn render_prompt(style: &PromptStyle, ctx: &PromptContext<'_>) -> String {
    let template = match style {
        PromptStyle::Short => DEFAULT_SHORT_PROMPT,
        PromptStyle::Medium => DEFAULT_MEDIUM_PROMPT,
        PromptStyle::Long => DEFAULT_LONG_PROMPT,
        PromptStyle::Custom(t) => t.as_str(),
    };
    eval_prompt(template, ctx)
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
/// conditionals (with optional `:` else branches), and literal text.
/// `depth` tracks nesting level so that `.` and `:` are only treated as
/// structural delimiters inside a conditional body.
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
                    expand_escape(chars, pos, ctx, &mut result);
                } else {
                    // Trailing `%` at end of string: pass through literally
                    result.push('%');
                }
            }
            '\\' => {
                // Backslash escaping: \% → literal %, \. → literal ., etc.
                *pos += 1;
                if *pos < chars.len() {
                    result.push(chars[*pos]);
                    *pos += 1;
                }
            }
            '?' => {
                *pos += 1;
                if *pos < chars.len() {
                    if evaluate_condition(chars, pos, ctx) {
                        // Condition true: recurse to evaluate the true branch.
                        let text = eval_recursive(chars, pos, ctx, depth + 1);
                        result.push_str(&text);
                    } else {
                        // Condition false: skip the true branch. If a `:`
                        // else separator is found, evaluate the else branch;
                        // otherwise the `.` terminator was consumed and we're
                        // done with this conditional.
                        let found_else = skip_to_colon_or_dot(chars, pos);
                        if found_else {
                            // Evaluate the else branch at depth+1 so the
                            // closing `.` terminates it.
                            let text = eval_recursive(chars, pos, ctx, depth + 1);
                            result.push_str(&text);
                        }
                    }
                }
                // Trailing `?` at end of string: silently ignore
            }
            ':' if depth > 0 => {
                // Else separator within a conditional. When the true branch
                // was evaluated and we hit `:`, skip the false branch and
                // consume the closing `.`.
                *pos += 1;
                skip_to_dot(chars, pos);
                return result;
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
/// Reads the flag character at `chars[*pos]` and advances `pos` past it
/// (and past any modifier characters, e.g. `B` after `p`). Returns `true`
/// if the condition is met, `false` otherwise. Unknown flags always
/// evaluate to `false`.
///
/// The conditional flags mirror GNU less:
///
/// | Flag | True when |
/// |------|-----------|
/// | `a` | Search pattern is active |
/// | `b` | Byte offset is known (always true) |
/// | `e` | At end of file |
/// | `f` | A filename is known (not a pipe) |
/// | `l` | Line number known (bottom_line > 0) |
/// | `L` | Total line count is known |
/// | `m` | More than one file open |
/// | `n` | Not the only file (same as `m`) |
/// | `pB` | Percent by byte is known (total_bytes > 0) |
/// | `s`/`S` | File size is known |
/// | `t` | Tab stops set (always true for now) |
/// | `u` | Filter (un-filter) is active |
/// | `x` | First file in the file list |
/// | `B` | Total bytes known (always true) |
fn evaluate_condition(chars: &[char], pos: &mut usize, ctx: &PromptContext<'_>) -> bool {
    let flag = chars[*pos];
    *pos += 1;

    match flag {
        'a' => ctx.search_active,
        'b' | 'B' | 't' => true, // Byte offset / total bytes / tab stops always known
        'e' => ctx.at_eof,
        'f' => !ctx.is_pipe,
        'l' => {
            // Consume optional modifier: `t` (top line) or `b` (bottom line)
            if *pos < chars.len() && (chars[*pos] == 't' || chars[*pos] == 'b') {
                *pos += 1;
            }
            ctx.bottom_line > 0 // Line numbers are known when we have lines
        }
        'L' => ctx.total_lines.is_some(),
        'm' | 'n' => ctx.file_count > 1,
        'p' | 'P' => {
            // Consume optional `B` modifier (byte-based percent)
            if *pos < chars.len() && chars[*pos] == 'B' {
                *pos += 1;
            }
            ctx.total_bytes > 0
        }
        's' | 'S' => !ctx.is_pipe || ctx.pipe_size.is_some(),
        'u' => ctx.filter_active,
        'x' => ctx.file_index == 0,
        _ => false,
    }
}

/// Skip a condition flag character and any optional modifier.
///
/// Mirrors the flag consumption in [`evaluate_condition`] so that the skip
/// functions advance past the same number of characters as evaluation would.
/// This is critical for multi-character conditions like `?lt`, `?lb`, `?pB`.
fn skip_condition_flag(chars: &[char], pos: &mut usize) {
    if *pos >= chars.len() {
        return;
    }
    let flag = chars[*pos];
    *pos += 1;
    match flag {
        'l' => {
            // Optional modifier: `t` (top line) or `b` (bottom line)
            if *pos < chars.len() && (chars[*pos] == 't' || chars[*pos] == 'b') {
                *pos += 1;
            }
        }
        'p' | 'P' => {
            // Optional `B` modifier (byte-based percent)
            if *pos < chars.len() && chars[*pos] == 'B' {
                *pos += 1;
            }
        }
        _ => {} // Single-character flags: a, b, B, e, f, L, m, n, s, S, t, u, x
    }
}

/// Skip past a conditional's true branch to either a `:` (else) or `.` (end).
///
/// When the condition was false, we need to skip the true branch. If we
/// find a `:` at the current nesting level, we stop there and return
/// `true` so the caller can evaluate the else branch. If we find a `.`
/// first, the entire conditional is over (no else branch) and we return
/// `false`.
///
/// Respects nested `?`/`.` pairs so that inner conditionals' `:` and `.`
/// delimiters are not confused with the outer one.
fn skip_to_colon_or_dot(chars: &[char], pos: &mut usize) -> bool {
    let mut nesting: usize = 1;
    while *pos < chars.len() {
        match chars[*pos] {
            '?' => {
                *pos += 1;
                skip_condition_flag(chars, pos);
                nesting += 1;
            }
            ':' if nesting == 1 => {
                // Found the else separator at our level. Consume it and
                // let the caller evaluate the else branch.
                *pos += 1;
                return true;
            }
            '.' => {
                *pos += 1;
                nesting -= 1;
                if nesting == 0 {
                    return false;
                }
            }
            '%' | '\\' => {
                // Skip the character after `%` or `\`
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
    false
}

/// Skip to the closing `.` at the current nesting level.
///
/// Used to discard the false branch of a conditional after the true branch
/// was evaluated and the `:` separator was encountered.
fn skip_to_dot(chars: &[char], pos: &mut usize) {
    let mut nesting: usize = 1;
    while *pos < chars.len() {
        match chars[*pos] {
            '?' => {
                *pos += 1;
                skip_condition_flag(chars, pos);
                nesting += 1;
            }
            '.' => {
                *pos += 1;
                nesting -= 1;
                if nesting == 0 {
                    return;
                }
            }
            '%' | '\\' => {
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

/// Expand a `%` escape starting at `chars[*pos]` into the result buffer.
///
/// Advances `*pos` past all characters consumed by the escape. Handles
/// both single-character escapes (`%f`, `%B`) and the multi-character
/// `%l` family (`%lt` = top line, `%lb` = bottom line).
#[allow(clippy::too_many_lines)] // Escape dispatch table is inherently large
fn expand_escape(chars: &[char], pos: &mut usize, ctx: &PromptContext<'_>, out: &mut String) {
    let escape = chars[*pos];
    *pos += 1;

    match escape {
        '%' => out.push('%'),
        // %b and %o both expand to byte offset of the top of screen.
        // An optional `B` modifier is consumed for compatibility with the
        // default long prompt template (`%bB`).
        'b' | 'o' => {
            if *pos < chars.len() && chars[*pos] == 'B' {
                *pos += 1;
            }
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
            // Multi-character escape: %lt = top line, %lb = bottom line.
            // Plain %l (no modifier) defaults to top line.
            if *pos < chars.len() && chars[*pos] == 't' {
                *pos += 1;
                let _ = write!(out, "{}", ctx.top_line);
            } else if *pos < chars.len() && chars[*pos] == 'b' {
                *pos += 1;
                let _ = write!(out, "{}", ctx.bottom_line);
            } else {
                let _ = write!(out, "{}", ctx.top_line);
            }
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
        // %p and %P compute percent through file. An optional modifier `B`
        // explicitly selects byte-based percent (the only mode currently
        // supported), so `%pB` and `%p` produce the same result.
        'p' | 'P' => {
            // Consume optional `B` modifier
            if *pos < chars.len() && chars[*pos] == 'B' {
                *pos += 1;
            }
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
    // Truncate prompt to screen width using left-truncation (matching less
    // behavior). When the prompt is too long, leading characters are removed
    // so that the right side (line info, percentage) remains visible.
    let char_count = prompt_text.chars().count();
    let display_text: String = if char_count > screen_cols {
        prompt_text.chars().skip(char_count - screen_cols).collect()
    } else {
        prompt_text.to_string()
    };
    // Render with configured color or fallback to reverse video
    let sgr = prompt_sgr.unwrap_or("\x1b[7m");
    write!(writer, "{sgr}{display_text}\x1b[0m")?;
    writer.flush()
}

/// Render an info line (e.g. the `=` command output) at a given row.
///
/// Unlike [`paint_prompt`], which left-truncates long text to keep the
/// right-side info visible, this function **right-truncates** to match
/// GNU less's behavior for the `=` info display.
///
/// # Errors
///
/// Returns an error if writing to the underlying writer fails.
pub fn paint_info_line<W: Write>(
    writer: &mut W,
    text: &str,
    row: usize,
    screen_cols: usize,
    sgr: Option<&str>,
) -> std::io::Result<()> {
    // Move cursor to the specified row, column 1 (1-based ANSI coordinates)
    write!(writer, "\x1b[{row};1H")?;
    // Clear the entire line
    write!(writer, "\x1b[2K")?;
    // Right-truncate: keep leading characters, drop trailing overflow.
    let display_text: String = text.chars().take(screen_cols).collect();
    // Render with configured color or fallback to reverse video
    let sgr_code = sgr.unwrap_or("\x1b[7m");
    write!(writer, "{sgr_code}{display_text}\x1b[0m")?;
    writer.flush()
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
            search_pattern: None,
            line_numbers_enabled: false,
            marks_set: false,
            filter_active: false,
            filter_pattern: None,
            input_complete: true,
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
            search_pattern: None,
            line_numbers_enabled: false,
            marks_set: false,
            filter_active: false,
            filter_pattern: None,
            input_complete: true,
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
            search_pattern: None,
            line_numbers_enabled: false,
            marks_set: false,
            filter_active: false,
            filter_pattern: None,
            input_complete: true,
        }
    }

    // ===== render_prompt tests (template-based) =====

    /// Task 132 test 1: Short prompt renders filename when not at EOF.
    #[test]
    fn test_render_prompt_short_not_eof_returns_filename() {
        let ctx = file_ctx("test.txt", false, 1, 24, Some(100), 0, 5000);
        assert_eq!(render_prompt(&PromptStyle::Short, &ctx), "test.txt");
    }

    /// Task 132 test 2: Short prompt renders "(END)" at EOF.
    #[test]
    fn test_render_prompt_short_at_eof_returns_end() {
        let ctx = file_ctx("test.txt", true, 77, 100, Some(100), 5000, 5000);
        assert_eq!(render_prompt(&PromptStyle::Short, &ctx), "(END)");
    }

    /// Task 121 test 3: Medium prompt includes filename and percent.
    #[test]
    fn test_render_prompt_medium_file_shows_name_and_percent() {
        let ctx = file_ctx("filename", false, 1, 24, Some(100), 2100, 5000);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "filename 42%");
    }

    /// Medium prompt for pipe shows percent (not byte offset).
    #[test]
    fn test_render_prompt_medium_pipe_shows_percent() {
        let ctx = pipe_ctx(false, 1234, 5000);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "24%");
    }

    /// Medium prompt at EOF shows filename and (END).
    #[test]
    fn test_render_prompt_medium_at_eof_returns_end() {
        let ctx = file_ctx("test.txt", true, 77, 100, Some(100), 5000, 5000);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "test.txt (END) ");
    }

    /// Task 121 test 4: Long prompt includes filename, lines, bytes, percent.
    #[test]
    fn test_render_prompt_long_file_known_total_shows_full_format() {
        let ctx = file_ctx("data.log", false, 10, 33, Some(200), 1500, 10000);
        assert_eq!(
            render_prompt(&PromptStyle::Long, &ctx),
            "data.log lines 10-33/200 15%"
        );
    }

    /// Long prompt with unknown total omits the total line count.
    /// Line info is still shown (lines are known), byte section is skipped.
    #[test]
    fn test_render_prompt_long_file_unknown_total_omits_total() {
        let ctx = file_ctx("data.log", false, 10, 33, None, 1500, 10000);
        assert_eq!(
            render_prompt(&PromptStyle::Long, &ctx),
            "data.log lines 10-33 15%"
        );
    }

    /// Long prompt for pipe shows lines and percent (no byte info when
    /// line numbers are known). Matches less 692 behavior.
    #[test]
    fn test_render_prompt_long_pipe_shows_lines_and_percent() {
        let ctx = pipe_ctx(false, 4096, 8192);
        assert_eq!(render_prompt(&PromptStyle::Long, &ctx), "lines 1-24 50%");
    }

    /// Long prompt at EOF shows line info plus (END), no byte section.
    #[test]
    fn test_render_prompt_long_at_eof_shows_end_with_filename() {
        let ctx = file_ctx("readme.txt", true, 90, 100, Some(100), 5000, 5000);
        assert_eq!(
            render_prompt(&PromptStyle::Long, &ctx),
            "readme.txt lines 90-100/100 (END) "
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

    /// Long prompt for pipe at EOF shows lines and (END).
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
        assert_eq!(render_prompt(&PromptStyle::Long, &ctx), "lines 1-24 (END) ");
    }

    /// Medium prompt with zero-byte file shows filename only.
    #[test]
    fn test_render_prompt_medium_zero_bytes_shows_filename_only() {
        let ctx = file_ctx("empty.txt", false, 1, 1, Some(1), 0, 0);
        assert_eq!(render_prompt(&PromptStyle::Medium, &ctx), "empty.txt ");
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
        // `?a` is false because search_active is false
        assert_eq!(eval_prompt("?a search ?e eof . stuff .", &ctx), "");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    #[test]
    fn test_eval_prompt_nested_conditional_inner_false_shows_outer() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        // `?f` is true (not pipe), `?a` is false (no search active)
        assert_eq!(
            eval_prompt("?f file ?a search . rest .", &ctx),
            " file  rest "
        );
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    /// `?b` = byte offset is known (always true)
    #[test]
    fn test_eval_prompt_condition_b_byte_offset_known() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?b has bytes .", &ctx), " has bytes ");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    /// `?l` = line numbers are known (bottom_line > 0)
    #[test]
    fn test_eval_prompt_condition_l_line_numbers_known() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?l lines known .", &ctx), " lines known ");
    }

    /// Spec ref: SPECIFICATION.md 5.9 — conditional expressions
    /// `?m` = more than one file open
    #[test]
    fn test_eval_prompt_condition_m_multiple_files() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.file_index = 1;
        ctx.file_count = 3;
        assert_eq!(eval_prompt("?m (%i of %m) .", &ctx), " (2 of 3) ");
    }

    /// `?m` is false when only one file is open.
    #[test]
    fn test_eval_prompt_condition_m_single_file_false() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?m multi .", &ctx), "");
    }

    /// Task 121 test 8: `?n` shows text when multiple files are open.
    #[test]
    fn test_eval_prompt_condition_n_multiple_files() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.file_count = 3;
        assert_eq!(eval_prompt("?n multi files .", &ctx), " multi files ");
    }

    /// `?n` is false when only one file.
    #[test]
    fn test_eval_prompt_condition_n_single_file_false() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?n multi .", &ctx), "");
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

    // ===== Task 121: integrated prompt rendering tests =====

    /// Task 121 test 5: Custom `-P` prompt template renders correctly.
    #[test]
    fn test_custom_prompt_template_renders_correctly() {
        let ctx = file_ctx("myfile.log", false, 50, 73, Some(500), 2500, 10000);
        let style = PromptStyle::Custom(String::from("Viewing %f (%pB%%)"));
        assert_eq!(render_prompt(&style, &ctx), "Viewing myfile.log (25%)");
    }

    /// Task 121 test 6: `?e` conditional shows text only at EOF.
    #[test]
    fn test_conditional_e_only_at_eof() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?eEOF.more", &ctx), "more");
        ctx.at_eof = true;
        assert_eq!(eval_prompt("?eEOF.more", &ctx), "EOFmore");
    }

    /// Task 121 test 7: `?f` conditional shows text only when filename known.
    #[test]
    fn test_conditional_f_only_when_filename_known() {
        let file_ctx = eval_ctx(Some("data.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?f%f.", &file_ctx), "data.txt");

        let mut pipe = eval_ctx(None, 1, 24, None, 0, 5000);
        pipe.is_pipe = true;
        assert_eq!(eval_prompt("?f%f.", &pipe), "");
    }

    /// Task 121 test 9: `?a` (search-active) correctly reflects search state.
    #[test]
    fn test_conditional_a_search_active_reflects_state() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?a[searching].", &ctx), "");

        ctx.search_active = true;
        ctx.search_pattern = Some("pattern");
        assert_eq!(eval_prompt("?a[searching].", &ctx), "[searching]");
    }

    /// Task 121 test 10: `?u` (filter-active) correctly reflects filter state.
    #[test]
    fn test_conditional_u_filter_active_reflects_state() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt("?u[filtered].", &ctx), "");

        ctx.filter_active = true;
        ctx.filter_pattern = Some("filter.*");
        assert_eq!(eval_prompt("?u[filtered].", &ctx), "[filtered]");
    }

    /// Task 121 test 12: Prompt is truncated to screen width.
    #[test]
    fn test_paint_prompt_truncates_to_screen_width() {
        let mut buf: Vec<u8> = Vec::new();
        let long_prompt = "A".repeat(100);
        paint_prompt(&mut buf, &long_prompt, 24, 10, None).unwrap();
        let output = String::from_utf8(buf).unwrap();
        // The prompt should be truncated to 10 chars
        assert!(output.contains(&"A".repeat(10)));
        assert!(!output.contains(&"A".repeat(11)));
    }

    /// Conditional with `:` else branch works.
    #[test]
    fn test_eval_prompt_colon_else_branch() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        // ?e false → else branch: "middle"
        assert_eq!(eval_prompt("?eend:middle.", &ctx), "middle");
        // ?e true → true branch: "end"
        ctx.at_eof = true;
        assert_eq!(eval_prompt("?eend:middle.", &ctx), "end");
    }

    /// Default short prompt template constant works through eval_prompt.
    #[test]
    fn test_default_short_prompt_template_via_eval() {
        let ctx = file_ctx("test.txt", false, 1, 24, Some(100), 0, 5000);
        assert_eq!(eval_prompt(DEFAULT_SHORT_PROMPT, &ctx), "test.txt");

        let mut eof_ctx = file_ctx("test.txt", true, 77, 100, Some(100), 5000, 5000);
        eof_ctx.at_eof = true;
        assert_eq!(eval_prompt(DEFAULT_SHORT_PROMPT, &eof_ctx), "(END)");
    }

    /// Default medium prompt template constant works through eval_prompt.
    #[test]
    fn test_default_medium_prompt_template_via_eval() {
        let ctx = file_ctx("notes.txt", false, 1, 24, Some(100), 2500, 5000);
        assert_eq!(eval_prompt(DEFAULT_MEDIUM_PROMPT, &ctx), "notes.txt 50%");
    }

    /// Default long prompt template constant works through eval_prompt.
    #[test]
    fn test_default_long_prompt_template_via_eval() {
        let ctx = file_ctx("data.log", false, 10, 33, Some(200), 1500, 10000);
        assert_eq!(
            eval_prompt(DEFAULT_LONG_PROMPT, &ctx),
            "data.log lines 10-33/200 15%"
        );
    }

    /// `%lt` expands to top line and `%lb` expands to bottom line.
    #[test]
    fn test_eval_prompt_percent_lt_and_lb_line_modifiers() {
        let ctx = eval_ctx(Some("test.txt"), 15, 38, Some(200), 0, 5000);
        assert_eq!(eval_prompt("lines %lt-%lb", &ctx), "lines 15-38");
    }

    /// `%pB` expands to byte percent and consumes the `B` modifier.
    #[test]
    fn test_eval_prompt_percent_p_b_modifier() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 500, 2000);
        assert_eq!(eval_prompt("%pB\\%%", &ctx), "25%%");
    }

    /// `?pB` condition consumes the `B` modifier.
    #[test]
    fn test_eval_prompt_conditional_p_b_modifier() {
        let ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 500, 2000);
        assert_eq!(eval_prompt("?pB%pB\\%.", &ctx), "25%");
    }

    /// `?x` conditional is true for the first file.
    #[test]
    fn test_eval_prompt_condition_x_first_file() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.file_index = 0;
        ctx.file_count = 3;
        assert_eq!(eval_prompt("?xfirst.", &ctx), "first");
    }

    /// `?x` conditional is false when not first file.
    #[test]
    fn test_eval_prompt_condition_x_not_first_file() {
        let mut ctx = eval_ctx(Some("test.txt"), 1, 24, Some(100), 0, 5000);
        ctx.file_index = 1;
        ctx.file_count = 3;
        assert_eq!(eval_prompt("?xfirst.", &ctx), "");
    }
}
