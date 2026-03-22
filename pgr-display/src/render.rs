//! Line rendering for terminal display.
//!
//! Handles tab expansion, control character notation (`^X`), ANSI escape
//! passthrough, horizontal scrolling, and width truncation.

use crate::ansi::{self, OverstrikeMode, Segment};
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

/// Tab stop configuration.
///
/// A single value means regular stops at every N columns.
/// Multiple values define explicit column positions, with the last
/// interval repeating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabStops {
    stops: Vec<usize>,
}

impl TabStops {
    /// Create tab stops at regular intervals.
    #[must_use]
    pub fn regular(width: usize) -> Self {
        Self { stops: vec![width] }
    }

    /// Parse a comma-separated tab stop specification (from `-x` flag).
    ///
    /// Examples: "4" (every 4), "4,8,12" (explicit positions, then every 4 after).
    ///
    /// # Errors
    ///
    /// Returns an error string if the spec contains non-numeric values or
    /// tab stop positions are not strictly increasing.
    pub fn parse(spec: &str) -> Result<Self, String> {
        let trimmed = spec.trim();
        if trimmed.is_empty() {
            return Err("empty tab stop specification".to_string());
        }

        let parts: Result<Vec<usize>, _> = trimmed
            .split(',')
            .map(|s| {
                s.trim()
                    .parse::<usize>()
                    .map_err(|e| format!("invalid tab stop value '{s}': {e}"))
            })
            .collect();

        let values = parts?;

        if values.is_empty() {
            return Err("empty tab stop specification".to_string());
        }

        // Single value: treat as regular interval
        if values.len() == 1 {
            if values[0] == 0 {
                return Err("tab stop width must be greater than zero".to_string());
            }
            return Ok(Self::regular(values[0]));
        }

        // Multiple values: must be strictly increasing
        for window in values.windows(2) {
            if window[1] <= window[0] {
                return Err(format!(
                    "tab stops must be strictly increasing, got {} after {}",
                    window[1], window[0]
                ));
            }
        }

        if values[0] == 0 {
            return Err("tab stop positions must be greater than zero".to_string());
        }

        Ok(Self { stops: values })
    }

    /// Compute the next tab stop column after `current_col`.
    #[must_use]
    pub fn next_stop(&self, current_col: usize) -> usize {
        if self.stops.len() == 1 {
            // Regular intervals
            let width = self.stops[0];
            if width == 0 {
                return current_col;
            }
            ((current_col / width) + 1) * width
        } else {
            // Explicit stops: find first stop > current_col
            for &stop in &self.stops {
                if stop > current_col {
                    return stop;
                }
            }
            // Past all explicit stops: repeat the last interval
            let last = self.stops[self.stops.len() - 1];
            let prev = self.stops[self.stops.len() - 2];
            let interval = last - prev;
            let mut next = last + interval;
            while next <= current_col {
                next += interval;
            }
            next
        }
    }

    /// Compute the number of spaces to fill from `current_col` to the next stop.
    #[must_use]
    pub fn spaces_to_next(&self, current_col: usize) -> usize {
        self.next_stop(current_col) - current_col
    }
}

/// Configuration for line rendering.
///
/// Combines raw control mode, overstrike processing, and tab stop settings
/// into a single configuration passed to [`render_line`]. This struct is the
/// primary interface for downstream tasks (112, 124) that extend rendering.
#[derive(Debug, Clone)]
pub struct RenderConfig {
    /// How to handle raw control characters and ANSI escapes.
    pub raw_mode: RawControlMode,
    /// How to process backspace/overstrike sequences.
    pub overstrike_mode: OverstrikeMode,
    /// Tab stop positions for tab expansion.
    pub tab_stops: TabStops,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            raw_mode: RawControlMode::Off,
            overstrike_mode: OverstrikeMode::Interpret,
            tab_stops: TabStops::regular(8),
        }
    }
}

/// Render a single line for terminal display.
///
/// Applies horizontal offset (skipping leading display columns), tab
/// expansion, control character notation, overstrike processing, and
/// ANSI escape handling according to the [`RenderConfig`]. The output
/// is truncated to `max_width` display columns.
///
/// When `overstrike_mode` is [`OverstrikeMode::Interpret`], overstrike
/// sequences are converted to ANSI equivalents before rendering.
///
/// Returns `(rendered_string, display_width)`.
#[must_use]
pub fn render_line(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    config: &RenderConfig,
) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }

    // Pre-process overstrikes if in interpret mode
    let processed;
    let effective_line = if config.overstrike_mode == OverstrikeMode::Interpret {
        processed = ansi::process_overstrikes(line, OverstrikeMode::Interpret);
        &processed
    } else if config.overstrike_mode == OverstrikeMode::Show {
        processed = ansi::process_overstrikes(line, OverstrikeMode::Show);
        &processed
    } else {
        line
    };

    match config.raw_mode {
        RawControlMode::Off => render_off(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
        ),
        RawControlMode::AnsiOnly => render_ansi_only(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
        ),
        RawControlMode::All => render_all(effective_line, horizontal_offset, max_width),
    }
}

/// Render with all ANSI escapes stripped and control chars as `^X`.
fn render_off(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
) -> (String, usize) {
    let stripped = ansi::strip_ansi(line);
    let chars: Vec<char> = stripped.chars().collect();
    render_chars(&chars, horizontal_offset, max_width, tab_stops, false)
}

/// Render with ANSI escapes preserved, control chars as `^X`.
fn render_ansi_only(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
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

                    let char_w = expanded_width(c, col, tab_stops);

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

                    let expansion = expand_char(c, col, tab_stops);
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
fn expanded_width(c: char, current_col: usize, tab_stops: &TabStops) -> usize {
    match c {
        '\t' => tab_stops.spaces_to_next(current_col),
        '\n' | '\r' => 0,
        '\x7f' => 2,
        c if c.is_ascii_control() => 2,
        c => UnicodeWidthChar::width(c).unwrap_or(0),
    }
}

/// Expand a character into its display representation.
///
/// - Tabs expand to spaces based on current column and tab stops.
/// - Control characters become `^X` notation.
/// - Newlines and carriage returns are ignored (stripped).
/// - Normal characters pass through.
fn expand_char(c: char, current_col: usize, tab_stops: &TabStops) -> String {
    match c {
        '\t' => {
            let spaces = tab_stops.spaces_to_next(current_col);
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
    tab_stops: &TabStops,
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

        let char_w = expanded_width(c, col, tab_stops);

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

        let expansion = expand_char(c, col, tab_stops);
        output.push_str(&expansion);
        visible_width += char_w;
        col += char_w;
    }

    (output, visible_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to create a default config for backward-compatible tests.
    fn default_config() -> RenderConfig {
        RenderConfig::default()
    }

    /// Helper to create a config with a specific raw mode.
    fn config_with_mode(mode: RawControlMode) -> RenderConfig {
        RenderConfig {
            raw_mode: mode,
            ..RenderConfig::default()
        }
    }

    // --- Plain ASCII rendering ---

    #[test]
    fn test_render_line_plain_ascii_renders_as_is() {
        let (rendered, width) = render_line("hello world", 0, 80, &default_config());
        assert_eq!(rendered, "hello world");
        assert_eq!(width, 11);
    }

    // --- Tab expansion ---

    #[test]
    fn test_render_line_tab_expands_correctly() {
        let (rendered, width) = render_line("\thello", 0, 80, &default_config());
        assert_eq!(rendered, "        hello");
        assert_eq!(width, 13);
    }

    #[test]
    fn test_render_line_tab_mid_line_expands_to_next_stop() {
        // "ab\tc": 'a'=col0, 'b'=col1, tab at col2 -> 6 spaces to col8, 'c'=col8
        let (rendered, width) = render_line("ab\tc", 0, 80, &default_config());
        assert_eq!(rendered, "ab      c");
        assert_eq!(width, 9);
    }

    // --- Control characters in Off mode ---

    #[test]
    fn test_render_line_control_chars_off_mode_renders_caret() {
        let (rendered, width) = render_line("a\x01b", 0, 80, &default_config());
        assert_eq!(rendered, "a^Ab");
        assert_eq!(width, 4);
    }

    #[test]
    fn test_render_line_del_renders_as_caret_question() {
        let (rendered, _) = render_line("a\x7fb", 0, 80, &default_config());
        assert_eq!(rendered, "a^?b");
    }

    // --- ANSI escapes in Off mode ---

    #[test]
    fn test_render_line_ansi_off_mode_strips_escapes() {
        let (rendered, width) = render_line("\x1b[31mred\x1b[0m", 0, 80, &default_config());
        assert_eq!(rendered, "red");
        assert_eq!(width, 3);
    }

    // --- ANSI escapes in AnsiOnly mode ---

    #[test]
    fn test_render_line_ansi_only_mode_preserves_escapes() {
        let (rendered, width) = render_line(
            "\x1b[31mred\x1b[0m",
            0,
            80,
            &config_with_mode(RawControlMode::AnsiOnly),
        );
        assert_eq!(rendered, "\x1b[31mred\x1b[0m");
        assert_eq!(width, 3);
    }

    #[test]
    fn test_render_line_ansi_only_control_chars_rendered_as_caret() {
        let (rendered, _) = render_line(
            "\x1b[31m\x01\x1b[0m",
            0,
            80,
            &config_with_mode(RawControlMode::AnsiOnly),
        );
        assert_eq!(rendered, "\x1b[31m^A\x1b[0m");
    }

    // --- Horizontal offset ---

    #[test]
    fn test_render_line_horizontal_offset_skips_columns() {
        let (rendered, width) = render_line("hello world", 6, 80, &default_config());
        assert_eq!(rendered, "world");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_render_line_horizontal_offset_beyond_line_returns_empty() {
        let (rendered, width) = render_line("hello", 20, 80, &default_config());
        assert_eq!(rendered, "");
        assert_eq!(width, 0);
    }

    // --- Width truncation ---

    #[test]
    fn test_render_line_truncates_at_max_width() {
        let (rendered, width) = render_line("hello world", 0, 5, &default_config());
        assert_eq!(rendered, "hello");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_render_line_zero_max_width_returns_empty() {
        let (rendered, width) = render_line("hello", 0, 0, &default_config());
        assert_eq!(rendered, "");
        assert_eq!(width, 0);
    }

    // --- CJK characters ---

    #[test]
    fn test_render_line_cjk_correct_width() {
        // '中' = 2 cells, '文' = 2 cells
        let (rendered, width) = render_line("\u{4e2d}\u{6587}", 0, 80, &default_config());
        assert_eq!(rendered, "\u{4e2d}\u{6587}");
        assert_eq!(width, 4);
    }

    #[test]
    fn test_render_line_cjk_truncation_no_split() {
        // '中' = 2 cells. Max width 3: fits '中' (2), next '文' (2) won't fit.
        let (rendered, width) = render_line("\u{4e2d}\u{6587}", 0, 3, &default_config());
        assert_eq!(rendered, "\u{4e2d}");
        assert_eq!(width, 2);
    }

    // --- All mode (raw passthrough) ---

    #[test]
    fn test_render_line_all_mode_passes_everything() {
        let input = "\x1b[31m\x01raw\x1b[0m";
        let (rendered, _) = render_line(input, 0, 80, &config_with_mode(RawControlMode::All));
        // In All mode, everything passes through including escapes and control chars
        assert!(rendered.contains("\x1b[31m"));
        assert!(rendered.contains("\x01"));
        assert!(rendered.contains("raw"));
    }

    // --- Empty input ---

    #[test]
    fn test_render_line_empty_input_returns_empty() {
        let (rendered, width) = render_line("", 0, 80, &default_config());
        assert_eq!(rendered, "");
        assert_eq!(width, 0);
    }

    // --- Newlines stripped ---

    #[test]
    fn test_render_line_newline_stripped() {
        let (rendered, width) = render_line("hello\n", 0, 80, &default_config());
        assert_eq!(rendered, "hello");
        assert_eq!(width, 5);
    }

    // --- expand_char unit tests ---

    #[test]
    fn test_expand_char_tab_at_col_zero_gives_full_width() {
        let tabs = TabStops::regular(8);
        let result = expand_char('\t', 0, &tabs);
        assert_eq!(result, "        ");
    }

    #[test]
    fn test_expand_char_control_a_gives_caret_a() {
        let tabs = TabStops::regular(8);
        let result = expand_char('\x01', 0, &tabs);
        assert_eq!(result, "^A");
    }

    #[test]
    fn test_expand_char_null_gives_caret_at() {
        let tabs = TabStops::regular(8);
        let result = expand_char('\x00', 0, &tabs);
        assert_eq!(result, "^@");
    }

    #[test]
    fn test_expand_char_normal_char_passes_through() {
        let tabs = TabStops::regular(8);
        let result = expand_char('a', 0, &tabs);
        assert_eq!(result, "a");
    }

    // --- Tab stop tests ---

    #[test]
    fn test_tab_stops_regular_next_stop() {
        let stops = TabStops::regular(8);
        assert_eq!(stops.next_stop(0), 8);
        assert_eq!(stops.next_stop(5), 8);
        assert_eq!(stops.next_stop(8), 16);
    }

    #[test]
    fn test_tab_stops_regular_spaces_to_next() {
        let stops = TabStops::regular(8);
        assert_eq!(stops.spaces_to_next(0), 8);
        assert_eq!(stops.spaces_to_next(3), 5);
    }

    #[test]
    fn test_tab_stops_parse_single_value() {
        let stops = TabStops::parse("4").unwrap();
        assert_eq!(stops, TabStops::regular(4));
    }

    #[test]
    fn test_tab_stops_parse_multiple_values() {
        let stops = TabStops::parse("4,8,12").unwrap();
        // Explicit stops at 4, 8, 12, then every 4 after
        assert_eq!(stops.next_stop(0), 4);
        assert_eq!(stops.next_stop(4), 8);
        assert_eq!(stops.next_stop(8), 12);
        assert_eq!(stops.next_stop(12), 16);
        assert_eq!(stops.next_stop(16), 20);
    }

    #[test]
    fn test_tab_stops_parse_invalid_returns_error() {
        assert!(TabStops::parse("abc").is_err());
    }

    #[test]
    fn test_render_line_with_tab_stops_custom() {
        let config = RenderConfig {
            raw_mode: RawControlMode::Off,
            overstrike_mode: OverstrikeMode::Interpret,
            tab_stops: TabStops::regular(4),
        };
        // Tab at col 0 with tab_width 4 -> 4 spaces
        let (rendered, width) = render_line("\thello", 0, 80, &config);
        assert_eq!(rendered, "    hello");
        assert_eq!(width, 9);
    }

    // --- Render mode refinement tests ---

    #[test]
    fn test_render_line_ansi_only_control_char_shows_caret() {
        // 0x01 in AnsiOnly mode renders as ^A
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line("a\x01b", 0, 80, &config);
        assert_eq!(rendered, "a^Ab");
    }

    #[test]
    fn test_render_line_ansi_only_preserves_sgr() {
        // SGR sequences pass through in AnsiOnly mode
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, width) = render_line("\x1b[31mred\x1b[0m", 0, 80, &config);
        assert_eq!(rendered, "\x1b[31mred\x1b[0m");
        assert_eq!(width, 3);
    }

    #[test]
    fn test_render_line_all_mode_passes_control_chars() {
        // Control chars pass through raw in All mode
        let config = config_with_mode(RawControlMode::All);
        let (rendered, _) = render_line("a\x01b", 0, 80, &config);
        assert!(rendered.contains('\x01'));
    }

    // --- Integration test ---

    #[test]
    fn test_render_config_combines_all_settings() {
        let config = RenderConfig {
            raw_mode: RawControlMode::AnsiOnly,
            overstrike_mode: OverstrikeMode::Interpret,
            tab_stops: TabStops::regular(4),
        };
        // Line with overstrike bold, a tab, and an ANSI color
        let input = "a\x08a\t\x1b[31mred\x1b[0m";
        let (rendered, _) = render_line(input, 0, 80, &config);
        // Overstrike should become ANSI bold
        assert!(rendered.contains("\x1b[1m"));
        // SGR should be preserved
        assert!(rendered.contains("\x1b[31m"));
        // Tab should be expanded (4 spaces from col after bold 'a')
        assert!(rendered.contains("   "));
    }
}
