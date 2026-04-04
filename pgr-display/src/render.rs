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

impl std::fmt::Display for TabStops {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut first = true;
        for stop in &self.stops {
            if first {
                first = false;
            } else {
                f.write_str(",")?;
            }
            write!(f, "{stop}")?;
        }
        Ok(())
    }
}

/// Parsed binary character format specification.
///
/// Represents the parsed form of `LESSBINFMT` or `LESSUTFBINFMT`.
/// The format string supports:
/// - `*` prefix: enable standout (reverse video) mode
/// - `s` after `*`: insert a literal `*` character
/// - literal characters: inserted directly
/// - `%x`, `%X`, `%02X`, `%04X`, etc.: hex format of the byte value
/// - `%o`: octal format
/// - `%d`: decimal format
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinFmt {
    /// Whether to wrap the output in standout (reverse video) mode.
    pub standout: bool,
    /// The format segments that produce the display string.
    pub segments: Vec<BinFmtSegment>,
}

/// A segment of a binary format specification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinFmtSegment {
    /// A literal string to insert.
    Literal(String),
    /// A printf-style format specifier for the byte value.
    /// Stores the full specifier (e.g., `%02X`, `%04X`, `%o`, `%d`).
    Format(String),
}

impl BinFmt {
    /// Parse a `LESSBINFMT`-style format string.
    ///
    /// The `*` prefix enables standout mode. `s` immediately after `*`
    /// inserts a literal `*`. Otherwise characters are literal unless a `%`
    /// introduces a printf-style format specifier.
    #[must_use]
    pub fn parse(spec: &str) -> Self {
        let mut chars = spec.chars().peekable();
        let mut standout = false;
        let mut segments = Vec::new();

        // Check for `*` prefix (standout mode)
        if chars.peek() == Some(&'*') {
            standout = true;
            chars.next();

            // After `*`, if the next char is `s`, it means literal `*`
            if chars.peek() == Some(&'s') {
                chars.next();
                segments.push(BinFmtSegment::Literal("*".to_string()));
            }
        }

        let mut literal_buf = String::new();

        while let Some(c) = chars.next() {
            if c == '%' {
                // Flush accumulated literal
                if !literal_buf.is_empty() {
                    segments.push(BinFmtSegment::Literal(literal_buf.clone()));
                    literal_buf.clear();
                }

                // Collect the format specifier
                let mut fmt = String::from('%');
                // Collect flags and width digits
                while let Some(&fc) = chars.peek() {
                    if fc.is_ascii_digit() || fc == '-' || fc == '+' || fc == '0' || fc == ' ' {
                        fmt.push(fc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                // Collect the conversion character
                if let Some(&conv) = chars.peek() {
                    fmt.push(conv);
                    chars.next();
                }
                segments.push(BinFmtSegment::Format(fmt));
            } else {
                literal_buf.push(c);
            }
        }

        if !literal_buf.is_empty() {
            segments.push(BinFmtSegment::Literal(literal_buf));
        }

        Self { standout, segments }
    }

    /// Format a byte value using this binary format specification.
    #[must_use]
    pub fn format_byte(&self, byte_val: u32) -> String {
        let mut result = String::new();
        for seg in &self.segments {
            match seg {
                BinFmtSegment::Literal(s) => result.push_str(s),
                BinFmtSegment::Format(fmt) => {
                    result.push_str(&apply_printf_format(fmt, byte_val));
                }
            }
        }
        result
    }

    /// Compute the display width of a formatted byte value.
    ///
    /// This must match the length of the string returned by `format_byte`.
    #[must_use]
    pub fn display_width(&self, byte_val: u32) -> usize {
        self.format_byte(byte_val).len()
    }
}

/// Apply a printf-style format specifier to a value.
///
/// Supports `%x`, `%X`, `%o`, `%d`, and width/zero-padding modifiers.
fn apply_printf_format(fmt: &str, value: u32) -> String {
    let bytes = fmt.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'%' {
        return fmt.to_string();
    }

    let conv = bytes[bytes.len() - 1];
    let middle = &fmt[1..fmt.len() - 1];

    let zero_pad = middle.starts_with('0');
    let width: usize = middle
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap_or(0);

    let raw = match conv {
        b'x' => format!("{value:x}"),
        b'X' => format!("{value:X}"),
        b'o' => format!("{value:o}"),
        b'd' => format!("{value}"),
        _ => return fmt.to_string(),
    };

    if width > raw.len() {
        let pad_char = if zero_pad { '0' } else { ' ' };
        let padding = pad_char.to_string().repeat(width - raw.len());
        format!("{padding}{raw}")
    } else {
        raw
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
    /// Optional binary character format (from LESSBINFMT).
    /// When `Some`, control/binary characters use this format instead of `^X`.
    pub bin_fmt: Option<BinFmt>,
    /// Optional UTF-8 binary format (from LESSUTFBINFMT).
    /// When `Some`, invalid UTF-8 sequences use this format.
    pub utf_bin_fmt: Option<BinFmt>,
}

impl Default for RenderConfig {
    fn default() -> Self {
        Self {
            raw_mode: RawControlMode::Off,
            overstrike_mode: OverstrikeMode::Interpret,
            tab_stops: TabStops::regular(8),
            bin_fmt: None,
            utf_bin_fmt: None,
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

    let bin_fmt = config.bin_fmt.as_ref();
    match config.raw_mode {
        RawControlMode::Off => render_off(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
            bin_fmt,
        ),
        RawControlMode::AnsiOnly => render_ansi_only(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
            bin_fmt,
        ),
        RawControlMode::All => render_all(effective_line, horizontal_offset, max_width),
    }
}

/// Render with control chars displayed as visible notation.
///
/// ESC (0x1b) is displayed as "ESC" and other control characters as `^X`,
/// matching GNU less default mode. ANSI escape sequences are NOT stripped;
/// instead the ESC byte becomes "ESC" and the remaining sequence characters
/// (e.g. `[31m`) pass through as visible printable text.
fn render_off(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    let chars: Vec<char> = line.chars().collect();
    render_chars(
        &chars,
        horizontal_offset,
        max_width,
        tab_stops,
        false,
        bin_fmt,
    )
}

/// Render with ANSI escapes preserved, control chars as `^X`.
fn render_ansi_only(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    let segments = ansi::parse_ansi(line);
    let mut output = String::with_capacity(line.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;

    for segment in segments {
        match segment {
            Segment::Escape(esc) => {
                // Only pass through SGR sequences (color/style); strip all others.
                // This matches GNU less -R behavior: CSI SGR (ESC [ ... m)
                // passes through, other CSI/OSC/simple escapes are silently dropped.
                if ansi::is_sgr_sequence(esc) && skipped >= horizontal_offset {
                    output.push_str(esc);
                }
            }
            Segment::Text(text) => {
                for c in text.chars() {
                    if visible_width >= max_width {
                        break;
                    }

                    let char_w = expanded_width(c, col, tab_stops, bin_fmt);

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

                    let expansion = expand_char(c, col, tab_stops, bin_fmt);
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
///
/// When `bin_fmt` is `Some`, binary/control characters use the LESSBINFMT
/// format instead of the default `^X` notation.
fn expanded_width(
    c: char,
    current_col: usize,
    tab_stops: &TabStops,
    bin_fmt: Option<&BinFmt>,
) -> usize {
    match c {
        '\t' => tab_stops.spaces_to_next(current_col),
        '\n' | '\r' => 0,
        '\x1b' => {
            if let Some(fmt) = bin_fmt {
                fmt.display_width(u32::from(c as u8))
            } else {
                3 // ESC displays as "ESC" (matching GNU less)
            }
        }
        '\x7f' => {
            if let Some(fmt) = bin_fmt {
                fmt.display_width(0x7F)
            } else {
                2
            }
        }
        c if c.is_ascii_control() => {
            if let Some(fmt) = bin_fmt {
                fmt.display_width(u32::from(c as u8))
            } else {
                2
            }
        }
        c => UnicodeWidthChar::width(c).unwrap_or(0),
    }
}

/// Expand a character into its display representation.
///
/// - Tabs expand to spaces based on current column and tab stops.
/// - Control characters become `^X` notation (or LESSBINFMT format if set).
/// - Newlines and carriage returns are ignored (stripped).
/// - Normal characters pass through.
///
/// When `bin_fmt` is `Some`, binary/control characters use the LESSBINFMT
/// format instead of the default `^X` notation.
fn expand_char(
    c: char,
    current_col: usize,
    tab_stops: &TabStops,
    bin_fmt: Option<&BinFmt>,
) -> String {
    match c {
        '\t' => {
            let spaces = tab_stops.spaces_to_next(current_col);
            " ".repeat(spaces)
        }
        '\n' | '\r' => String::new(),
        '\x1b' => {
            if let Some(fmt) = bin_fmt {
                fmt.format_byte(u32::from(c as u8))
            } else {
                // ESC displays as "ESC" (matching GNU less, which special-cases 0x1b
                // to avoid the confusing ^[ caret notation that resembles a real
                // escape sequence).
                "ESC".to_string()
            }
        }
        '\x7f' => {
            if let Some(fmt) = bin_fmt {
                fmt.format_byte(0x7F)
            } else {
                "^?".to_string()
            }
        }
        c if c.is_ascii_control() => {
            if let Some(fmt) = bin_fmt {
                fmt.format_byte(u32::from(c as u8))
            } else {
                let mut s = String::with_capacity(2);
                s.push('^');
                // Control chars 0x00-0x1F map to ^@, ^A, ..., ^_
                #[allow(clippy::cast_possible_truncation)] // c is in 0x00..=0x1F, always fits u8
                let display = (c as u8 + b'@') as char;
                s.push(display);
                s
            }
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

/// Compute the full display width of a line after tab expansion and control
/// character notation, using the same rules as [`render_line`].
///
/// ANSI escape sequences are stripped (zero width). Overstrikes are processed
/// if `config.overstrike_mode` is set. This is used to determine whether a
/// line extends beyond the visible viewport for chop mode truncation markers.
#[must_use]
pub fn line_display_width(line: &str, config: &RenderConfig) -> usize {
    // Pre-process overstrikes to match render_line behavior
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

    let bin_fmt = config.bin_fmt.as_ref();
    match config.raw_mode {
        RawControlMode::Off => {
            // In Off mode, ESC is displayed as "ESC" and the rest of ANSI
            // sequences are visible text — don't strip.
            compute_display_width(effective_line, &config.tab_stops, bin_fmt)
        }
        RawControlMode::AnsiOnly => {
            // In AnsiOnly mode, SGR sequences pass through with zero display
            // width — strip them for width calculation.
            let stripped = ansi::strip_ansi(effective_line);
            compute_display_width(&stripped, &config.tab_stops, bin_fmt)
        }
        RawControlMode::All => {
            // In raw mode, best-effort: count printable char widths
            let mut width: usize = 0;
            for c in effective_line.chars() {
                width += raw_char_width(c);
            }
            width
        }
    }
}

/// Compute display width of text (no ANSI) with tab expansion and control char notation.
fn compute_display_width(text: &str, tab_stops: &TabStops, bin_fmt: Option<&BinFmt>) -> usize {
    let mut col: usize = 0;
    for c in text.chars() {
        col += expanded_width(c, col, tab_stops, bin_fmt);
    }
    col
}

/// Apply chop mode truncation markers to a rendered line.
///
/// In `less -S` (chop) mode, if the line extends beyond the visible area
/// (right-truncated), the last visible character is replaced with `>`.
/// This matches GNU less behavior controlled by the `--rscroll` option
/// (default: `>` in standout mode).
///
/// GNU less does not display a left-side marker when content extends to the
/// left of the viewport, so this function does not add one either.
///
/// The `rendered` string may contain ANSI escape sequences; this function
/// correctly skips them when locating the last visible character.
///
/// The `h_offset` parameter is accepted for API compatibility but does not
/// affect the output (GNU less has no left-shift marker).
///
/// Returns the modified rendered string and its display width.
#[must_use]
pub fn apply_chop_markers(
    rendered: &str,
    display_width: usize,
    _h_offset: usize,
    truncated_right: bool,
) -> (String, usize) {
    if display_width == 0 {
        return (rendered.to_string(), 0);
    }

    let needs_right = truncated_right && display_width > 0;

    if !needs_right {
        return (rendered.to_string(), display_width);
    }

    // Build a new string with the right marker applied.
    // We track visible character positions to know when to substitute.
    let mut result = String::with_capacity(rendered.len());
    let mut visible_col: usize = 0;
    let bytes = rendered.as_bytes();
    let len = bytes.len();
    let mut i: usize = 0;

    // The `>` marker replaces the last visible column (display_width - 1).
    let right_marker_col = display_width.saturating_sub(1);

    while i < len {
        // Check for ANSI escape sequence
        if bytes[i] == b'\x1b' && i + 1 < len && bytes[i + 1] == b'[' {
            // Find the end of the escape sequence
            let start = i;
            i += 2; // skip ESC [
            while i < len && !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'm') {
                i += 1;
            }
            if i < len {
                i += 1; // skip the terminator
            }
            // Pass through the escape sequence
            if visible_col < display_width {
                result.push_str(&rendered[start..i]);
            }
            continue;
        }

        // This is a visible character
        let c = rendered[i..].chars().next().unwrap_or(' ');
        let char_len = c.len_utf8();
        let char_w = UnicodeWidthChar::width(c).unwrap_or(0);
        if char_w == 0 {
            // Zero-width character (combining mark): emit without advancing column
            result.push(c);
            i += char_len;
            continue;
        }

        if visible_col == right_marker_col {
            // Replace last visible character with `>`
            result.push('>');
            i += char_len;
            // Collect any trailing ANSI resets
            while i < len {
                if bytes[i] == b'\x1b' && i + 1 < len && bytes[i + 1] == b'[' {
                    let start = i;
                    i += 2;
                    while i < len && !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'm') {
                        i += 1;
                    }
                    if i < len {
                        i += 1;
                    }
                    result.push_str(&rendered[start..i]);
                } else {
                    break;
                }
            }
            break;
        }

        result.push(c);
        visible_col += char_w;
        i += char_len;
    }

    (result, display_width)
}

/// ANSI escape sequence for standout (reverse video) mode.
const STANDOUT_ON: &str = "\x1b[7m";
/// ANSI escape sequence to reset all attributes.
const STANDOUT_OFF: &str = "\x1b[0m";

/// Render a single line with optional search highlighting.
///
/// Behaves identically to [`render_line`] but additionally wraps matched
/// byte ranges in highlight escape sequences. Each element of `highlights`
/// is a `(start, end)` byte-offset pair into `line`.
///
/// `highlight_sgr` is the SGR sequence for search matches (from `ColorConfig`
/// selector `'S'`). If `None`, falls back to reverse video (`\x1b[7m`).
///
/// Highlights that fall outside the visible horizontal window are clipped.
/// Multiple highlights on one line are rendered independently.
///
/// Returns `(rendered_string, display_width)`.
#[must_use]
pub fn render_line_highlighted(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    config: &RenderConfig,
    highlights: &[(usize, usize)],
    highlight_sgr: Option<&str>,
) -> (String, usize) {
    if highlights.is_empty() {
        return render_line(line, horizontal_offset, max_width, config);
    }

    if max_width == 0 {
        return (String::new(), 0);
    }

    // Pre-process overstrikes
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

    let hl_on = highlight_sgr.unwrap_or(STANDOUT_ON);
    let hl_off = STANDOUT_OFF;

    let bin_fmt = config.bin_fmt.as_ref();
    match config.raw_mode {
        RawControlMode::Off => render_off_highlighted(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
            highlights,
            hl_on,
            hl_off,
            bin_fmt,
        ),
        RawControlMode::AnsiOnly => render_ansi_only_highlighted(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
            highlights,
            hl_on,
            hl_off,
            bin_fmt,
        ),
        // In raw passthrough mode, we don't apply highlights since we can't
        // reliably track character positions. Fall back to un-highlighted.
        RawControlMode::All => render_all(effective_line, horizontal_offset, max_width),
    }
}

/// Render a single line with mark color applied to the entire line.
///
/// When a line has a mark set, the entire rendered line is wrapped in
/// the mark SGR sequence. If `mark_sgr` is `None`, the line is rendered
/// without any mark coloring.
///
/// Returns `(rendered_string, display_width)`.
#[must_use]
pub fn render_line_marked(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    config: &RenderConfig,
    mark_sgr: Option<&str>,
) -> (String, usize) {
    let (rendered, width) = render_line(line, horizontal_offset, max_width, config);
    match mark_sgr {
        Some(sgr) if !sgr.is_empty() => {
            let mut result = String::with_capacity(sgr.len() + rendered.len() + STANDOUT_OFF.len());
            result.push_str(sgr);
            result.push_str(&rendered);
            result.push_str(STANDOUT_OFF);
            (result, width)
        }
        _ => (rendered, width),
    }
}

/// A highlight range with an associated SGR color sequence.
///
/// Used by [`render_line_multi_highlighted`] to apply per-pattern colors
/// to different match ranges on the same line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColoredRange<'a> {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// SGR escape sequence to activate this highlight color.
    pub sgr: &'a str,
}

/// Render a single line with multiple colored highlight ranges.
///
/// Like [`render_line_highlighted`] but each range can have a different
/// SGR color. `colored_ranges` must be sorted by `start` and non-overlapping.
///
/// Returns `(rendered_string, display_width)`.
#[must_use]
pub fn render_line_multi_highlighted(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    config: &RenderConfig,
    colored_ranges: &[ColoredRange<'_>],
) -> (String, usize) {
    if colored_ranges.is_empty() {
        return render_line(line, horizontal_offset, max_width, config);
    }

    if max_width == 0 {
        return (String::new(), 0);
    }

    // Pre-process overstrikes
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

    let hl_off = STANDOUT_OFF;
    let bin_fmt = config.bin_fmt.as_ref();

    match config.raw_mode {
        RawControlMode::Off => render_off_multi_highlighted(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
            colored_ranges,
            hl_off,
            bin_fmt,
        ),
        RawControlMode::AnsiOnly => render_ansi_only_multi_highlighted(
            effective_line,
            horizontal_offset,
            max_width,
            &config.tab_stops,
            colored_ranges,
            hl_off,
            bin_fmt,
        ),
        RawControlMode::All => render_all(effective_line, horizontal_offset, max_width),
    }
}

/// Check if a byte offset is inside any of the highlight ranges.
fn is_highlighted(byte_offset: usize, highlights: &[(usize, usize)]) -> bool {
    highlights
        .iter()
        .any(|&(start, end)| byte_offset >= start && byte_offset < end)
}

/// Find the SGR for a byte offset in sorted, non-overlapping colored ranges.
fn find_colored_highlight<'a>(
    byte_offset: usize,
    colored_ranges: &[ColoredRange<'a>],
) -> Option<&'a str> {
    colored_ranges
        .iter()
        .find(|r| byte_offset >= r.start && byte_offset < r.end)
        .map(|r| r.sgr)
}

/// Render with ANSI stripping and search highlighting.
#[allow(clippy::too_many_arguments)] // Internal fn; 8th arg is bin_fmt for LESSBINFMT support
fn render_off_highlighted(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    highlights: &[(usize, usize)],
    hl_on: &str,
    hl_off: &str,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    // In Off mode, ESC is displayed as "ESC" and the remaining sequence
    // characters are visible — don't strip ANSI.
    render_chars_highlighted(
        line,
        horizontal_offset,
        max_width,
        tab_stops,
        highlights,
        hl_on,
        hl_off,
        bin_fmt,
    )
}

/// Render with ANSI passthrough and search highlighting.
#[allow(clippy::too_many_arguments)] // Internal fn; 8th arg is bin_fmt for LESSBINFMT support
fn render_ansi_only_highlighted(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    highlights: &[(usize, usize)],
    hl_on: &str,
    _hl_off: &str,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    let segments = ansi::parse_ansi(line);
    let mut output = String::with_capacity(line.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;
    // Text-only byte offset for highlight lookups — highlights are computed
    // on raw text (no SGR), so we skip escape sequence bytes.
    let mut text_byte_offset: usize = 0;
    let mut in_standout = false;
    // Track the last SGR sequence from the input so we can restore it
    // after a search highlight ends, instead of hard-resetting with \x1b[0m
    // which would kill syntax/diff coloring.
    let mut last_sgr = String::new();

    for segment in segments {
        match segment {
            Segment::Escape(esc) => {
                // Only pass through SGR sequences; strip non-SGR (matches -R behavior).
                if ansi::is_sgr_sequence(esc) {
                    if skipped >= horizontal_offset {
                        // Don't emit input SGR while inside a highlight — we'll
                        // restore it when the highlight ends.
                        if !in_standout {
                            output.push_str(esc);
                        }
                    }
                    // Always track the latest SGR so we can restore after highlights.
                    last_sgr.clear();
                    last_sgr.push_str(esc);
                }
                // Escapes are not in the raw text — don't advance text_byte_offset.
            }
            Segment::Text(text) => {
                for c in text.chars() {
                    if visible_width >= max_width {
                        break;
                    }

                    let char_w = expanded_width(c, col, tab_stops, bin_fmt);

                    if skipped < horizontal_offset {
                        let remaining_to_skip = horizontal_offset - skipped;
                        if char_w <= remaining_to_skip {
                            skipped += char_w;
                            col += char_w;
                            text_byte_offset += c.len_utf8();
                            continue;
                        }
                        skipped += char_w;
                        col += char_w;
                        text_byte_offset += c.len_utf8();
                        continue;
                    }

                    if visible_width + char_w > max_width {
                        break;
                    }

                    let should_highlight = is_highlighted(text_byte_offset, highlights);
                    if should_highlight && !in_standout {
                        output.push_str(hl_on);
                        in_standout = true;
                    } else if !should_highlight && in_standout {
                        // Restore the previous SGR state instead of hard-resetting.
                        // This preserves syntax/diff coloring around search matches.
                        output.push_str(STANDOUT_OFF);
                        if !last_sgr.is_empty() {
                            output.push_str(&last_sgr);
                        }
                        in_standout = false;
                    }

                    let expansion = expand_char(c, col, tab_stops, bin_fmt);
                    output.push_str(&expansion);
                    visible_width += char_w;
                    col += char_w;
                    text_byte_offset += c.len_utf8();
                }
            }
        }
    }

    if in_standout {
        output.push_str(STANDOUT_OFF);
    }

    (output, visible_width)
}

/// Render characters with highlight tracking.
#[allow(clippy::too_many_arguments)] // Internal fn; 8th arg is bin_fmt for LESSBINFMT support
fn render_chars_highlighted(
    text: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    highlights: &[(usize, usize)],
    hl_on: &str,
    hl_off: &str,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    let mut output = String::with_capacity(text.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;
    let mut in_standout = false;

    for (byte_offset, c) in text.char_indices() {
        if visible_width >= max_width {
            break;
        }

        let char_w = expanded_width(c, col, tab_stops, bin_fmt);

        if skipped < horizontal_offset {
            let remaining = horizontal_offset - skipped;
            if char_w <= remaining {
                skipped += char_w;
                col += char_w;
                continue;
            }
            skipped += char_w;
            col += char_w;
            continue;
        }

        if visible_width + char_w > max_width {
            break;
        }

        let should_highlight = is_highlighted(byte_offset, highlights);
        if should_highlight && !in_standout {
            output.push_str(hl_on);
            in_standout = true;
        } else if !should_highlight && in_standout {
            output.push_str(hl_off);
            in_standout = false;
        }

        let expansion = expand_char(c, col, tab_stops, bin_fmt);
        output.push_str(&expansion);
        visible_width += char_w;
        col += char_w;
    }

    if in_standout {
        output.push_str(hl_off);
    }

    (output, visible_width)
}

/// Render with ANSI stripping and multi-colored search highlighting.
#[allow(clippy::too_many_arguments)] // Internal fn; 7th arg is bin_fmt for LESSBINFMT support
fn render_off_multi_highlighted(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    colored_ranges: &[ColoredRange<'_>],
    hl_off: &str,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    render_chars_multi_highlighted(
        line,
        horizontal_offset,
        max_width,
        tab_stops,
        colored_ranges,
        hl_off,
        bin_fmt,
    )
}

/// Render with ANSI passthrough and multi-colored search highlighting.
#[allow(clippy::too_many_arguments)] // Internal fn; 7th arg is bin_fmt for LESSBINFMT support
fn render_ansi_only_multi_highlighted(
    line: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    colored_ranges: &[ColoredRange<'_>],
    _hl_off: &str,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    let segments = ansi::parse_ansi(line);
    let mut output = String::with_capacity(line.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;
    // Text-only byte offset for highlight lookups — highlights are computed
    // on raw text (no SGR), so we skip escape sequence bytes.
    let mut text_byte_offset: usize = 0;
    let mut current_sgr: Option<&str> = None;
    // Track the last input SGR to restore after highlight ends.
    let mut last_input_sgr = String::new();

    for segment in segments {
        match segment {
            Segment::Escape(esc) => {
                if ansi::is_sgr_sequence(esc) {
                    if skipped >= horizontal_offset && current_sgr.is_none() {
                        output.push_str(esc);
                    }
                    last_input_sgr.clear();
                    last_input_sgr.push_str(esc);
                }
                // Escapes are not in the raw text — don't advance text_byte_offset.
            }
            Segment::Text(text) => {
                for c in text.chars() {
                    if visible_width >= max_width {
                        break;
                    }

                    let char_w = expanded_width(c, col, tab_stops, bin_fmt);

                    if skipped < horizontal_offset {
                        let remaining_to_skip = horizontal_offset - skipped;
                        if char_w <= remaining_to_skip {
                            skipped += char_w;
                            col += char_w;
                            text_byte_offset += c.len_utf8();
                            continue;
                        }
                        skipped += char_w;
                        col += char_w;
                        text_byte_offset += c.len_utf8();
                        continue;
                    }

                    if visible_width + char_w > max_width {
                        break;
                    }

                    let new_sgr = find_colored_highlight(text_byte_offset, colored_ranges);
                    if new_sgr != current_sgr {
                        if current_sgr.is_some() {
                            // Restore input SGR instead of hard reset.
                            output.push_str(STANDOUT_OFF);
                            if !last_input_sgr.is_empty() {
                                output.push_str(&last_input_sgr);
                            }
                        }
                        if let Some(sgr) = new_sgr {
                            output.push_str(sgr);
                        }
                        current_sgr = new_sgr;
                    }

                    let expansion = expand_char(c, col, tab_stops, bin_fmt);
                    output.push_str(&expansion);
                    visible_width += char_w;
                    col += char_w;
                    text_byte_offset += c.len_utf8();
                }
            }
        }
    }

    if current_sgr.is_some() {
        output.push_str(STANDOUT_OFF);
    }

    (output, visible_width)
}

/// Render characters with multi-colored highlight tracking.
#[allow(clippy::too_many_arguments)] // Internal fn; 7th arg is bin_fmt for LESSBINFMT support
fn render_chars_multi_highlighted(
    text: &str,
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    colored_ranges: &[ColoredRange<'_>],
    hl_off: &str,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    let mut output = String::with_capacity(text.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;
    let mut current_sgr: Option<&str> = None;

    for (byte_offset, c) in text.char_indices() {
        if visible_width >= max_width {
            break;
        }

        let char_w = expanded_width(c, col, tab_stops, bin_fmt);

        if skipped < horizontal_offset {
            let remaining = horizontal_offset - skipped;
            if char_w <= remaining {
                skipped += char_w;
                col += char_w;
                continue;
            }
            skipped += char_w;
            col += char_w;
            continue;
        }

        if visible_width + char_w > max_width {
            break;
        }

        let new_sgr = find_colored_highlight(byte_offset, colored_ranges);
        if new_sgr != current_sgr {
            if current_sgr.is_some() {
                output.push_str(hl_off);
            }
            if let Some(sgr) = new_sgr {
                output.push_str(sgr);
            }
            current_sgr = new_sgr;
        }

        let expansion = expand_char(c, col, tab_stops, bin_fmt);
        output.push_str(&expansion);
        visible_width += char_w;
        col += char_w;
    }

    if current_sgr.is_some() {
        output.push_str(hl_off);
    }

    (output, visible_width)
}

/// Render a character slice with offset, width limit, and tab/control handling.
fn render_chars(
    chars: &[char],
    horizontal_offset: usize,
    max_width: usize,
    tab_stops: &TabStops,
    _pass_ansi: bool,
    bin_fmt: Option<&BinFmt>,
) -> (String, usize) {
    let mut output = String::with_capacity(chars.len());
    let mut col: usize = 0;
    let mut skipped: usize = 0;
    let mut visible_width: usize = 0;

    for &c in chars {
        if visible_width >= max_width {
            break;
        }

        let char_w = expanded_width(c, col, tab_stops, bin_fmt);

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

        let expansion = expand_char(c, col, tab_stops, bin_fmt);
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
    fn test_render_line_ansi_off_mode_shows_esc_notation() {
        // In Off mode, ESC is displayed as "ESC" (matching GNU less) and the
        // remaining sequence characters pass through as visible text.
        let (rendered, width) = render_line("\x1b[31mred\x1b[0m", 0, 80, &default_config());
        assert_eq!(rendered, "ESC[31mredESC[0m");
        // "ESC"(3) + "[31m"(4) + "red"(3) + "ESC"(3) + "[0m"(3) = 16
        assert_eq!(width, 16);
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
        let result = expand_char('\t', 0, &tabs, None);
        assert_eq!(result, "        ");
    }

    #[test]
    fn test_expand_char_control_a_gives_caret_a() {
        let tabs = TabStops::regular(8);
        let result = expand_char('\x01', 0, &tabs, None);
        assert_eq!(result, "^A");
    }

    #[test]
    fn test_expand_char_null_gives_caret_at() {
        let tabs = TabStops::regular(8);
        let result = expand_char('\x00', 0, &tabs, None);
        assert_eq!(result, "^@");
    }

    #[test]
    fn test_expand_char_normal_char_passes_through() {
        let tabs = TabStops::regular(8);
        let result = expand_char('a', 0, &tabs, None);
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
            tab_stops: TabStops::regular(4),
            ..RenderConfig::default()
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
    fn test_render_line_ansi_only_strips_non_sgr_csi() {
        // Non-SGR CSI sequences (e.g., cursor movement) are stripped in -R mode
        let config = config_with_mode(RawControlMode::AnsiOnly);
        // ESC[2J = clear screen, ESC[H = cursor home — both non-SGR
        let (rendered, width) = render_line("\x1b[2JScreen clear\x1b[HHome", 0, 80, &config);
        assert_eq!(rendered, "Screen clearHome");
        assert_eq!(width, 16);
    }

    #[test]
    fn test_render_line_ansi_only_mixed_sgr_and_non_sgr() {
        // SGR passes through, non-SGR stripped, text preserved
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let input = "\x1b[31mRed SGR\x1b[0m\x1b[2JScreen clear\x1b[32mGreen SGR\x1b[0m";
        let (rendered, width) = render_line(input, 0, 80, &config);
        assert_eq!(
            rendered,
            "\x1b[31mRed SGR\x1b[0mScreen clear\x1b[32mGreen SGR\x1b[0m"
        );
        // "Red SGR" (7) + "Screen clear" (12) + "Green SGR" (9) = 28
        assert_eq!(width, 28);
    }

    #[test]
    fn test_render_line_ansi_only_strips_cursor_movement() {
        // ESC[5A = cursor up 5 — non-SGR, should be stripped
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, width) = render_line("before\x1b[5Aafter", 0, 80, &config);
        assert_eq!(rendered, "beforeafter");
        assert_eq!(width, 11);
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
            tab_stops: TabStops::regular(4),
            ..RenderConfig::default()
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

    // --- Highlighted rendering tests ---

    // ── Test 12: render_line_highlighted with no highlights produces identical output to render_line
    #[test]
    fn test_render_line_highlighted_no_highlights_matches_render_line() {
        let config = default_config();
        let line = "hello world";
        let (normal, normal_w) = render_line(line, 0, 80, &config);
        let (highlighted, highlighted_w) = render_line_highlighted(line, 0, 80, &config, &[], None);
        assert_eq!(normal, highlighted);
        assert_eq!(normal_w, highlighted_w);
    }

    // ── Test 13: render_line_highlighted with one match wraps matched text in reverse video
    #[test]
    fn test_render_line_highlighted_one_match_wraps_in_reverse_video() {
        let config = default_config();
        // "hello world" — highlight "world" at bytes 6..11
        let (rendered, width) =
            render_line_highlighted("hello world", 0, 80, &config, &[(6, 11)], None);
        assert_eq!(rendered, format!("hello {STANDOUT_ON}world{STANDOUT_OFF}"));
        assert_eq!(width, 11);
    }

    // ── Test 14: render_line_highlighted with multiple matches highlights each independently
    #[test]
    fn test_render_line_highlighted_multiple_matches_each_highlighted() {
        let config = default_config();
        // "ab cd ab" — highlight "ab" at bytes 0..2 and 6..8
        let (rendered, _) =
            render_line_highlighted("ab cd ab", 0, 80, &config, &[(0, 2), (6, 8)], None);
        assert_eq!(
            rendered,
            format!("{STANDOUT_ON}ab{STANDOUT_OFF} cd {STANDOUT_ON}ab{STANDOUT_OFF}")
        );
    }

    // ── Test 15: render_line_highlighted clips highlights to the visible horizontal window
    #[test]
    fn test_render_line_highlighted_clips_at_horizontal_offset() {
        let config = default_config();
        // "hello world" with offset 8, max_width 80 — only "rld" is visible (bytes 8..11)
        // Highlight is on "world" (bytes 6..11), so the visible portion "rld" should be highlighted
        let (rendered, width) =
            render_line_highlighted("hello world", 8, 80, &config, &[(6, 11)], None);
        assert_eq!(rendered, format!("{STANDOUT_ON}rld{STANDOUT_OFF}"));
        assert_eq!(width, 3);
    }

    #[test]
    fn test_render_line_highlighted_highlight_entirely_before_viewport_not_shown() {
        let config = default_config();
        // "hello world" with offset 6 — "world" visible
        // Highlight is on "hello" (bytes 0..5) — entirely before viewport
        let (rendered, width) =
            render_line_highlighted("hello world", 6, 80, &config, &[(0, 5)], None);
        assert_eq!(rendered, "world");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_render_line_highlighted_highlight_at_start_of_line() {
        let config = default_config();
        let (rendered, _) = render_line_highlighted("hello world", 0, 80, &config, &[(0, 5)], None);
        assert_eq!(rendered, format!("{STANDOUT_ON}hello{STANDOUT_OFF} world"));
    }

    // --- Color integration tests ---

    #[test]
    fn test_render_line_highlighted_custom_sgr_uses_provided_sequence() {
        let config = default_config();
        let custom_sgr = "\x1b[32;44m"; // green on blue
        let (rendered, _) =
            render_line_highlighted("hello world", 0, 80, &config, &[(6, 11)], Some(custom_sgr));
        assert_eq!(rendered, format!("hello {custom_sgr}world{STANDOUT_OFF}"));
    }

    #[test]
    fn test_render_line_highlighted_none_sgr_falls_back_to_reverse_video() {
        let config = default_config();
        let (rendered, _) =
            render_line_highlighted("hello world", 0, 80, &config, &[(6, 11)], None);
        assert_eq!(rendered, format!("hello {STANDOUT_ON}world{STANDOUT_OFF}"));
    }

    #[test]
    fn test_render_line_highlighted_multiple_highlights_use_custom_color() {
        let config = default_config();
        let custom_sgr = "\x1b[1;33m"; // bold yellow
        let (rendered, _) = render_line_highlighted(
            "ab cd ab",
            0,
            80,
            &config,
            &[(0, 2), (6, 8)],
            Some(custom_sgr),
        );
        assert_eq!(
            rendered,
            format!("{custom_sgr}ab{STANDOUT_OFF} cd {custom_sgr}ab{STANDOUT_OFF}")
        );
    }

    #[test]
    fn test_render_line_highlighted_reset_follows_every_colored_region() {
        let config = default_config();
        let custom_sgr = "\x1b[31m";
        let (rendered, _) = render_line_highlighted(
            "ab cd ab",
            0,
            80,
            &config,
            &[(0, 2), (6, 8)],
            Some(custom_sgr),
        );
        // Count reset sequences — should be 2 (one per highlighted region)
        let reset_count = rendered.matches(STANDOUT_OFF).count();
        assert_eq!(reset_count, 2);
    }

    #[test]
    fn test_render_line_highlighted_color_does_not_bleed_across_lines() {
        let config = default_config();
        let custom_sgr = "\x1b[31m";
        let (rendered, _) =
            render_line_highlighted("hello world", 0, 80, &config, &[(6, 11)], Some(custom_sgr));
        // Must end with reset, not with custom_sgr active
        assert!(rendered.ends_with(STANDOUT_OFF));
    }

    // --- Mark rendering tests ---

    #[test]
    fn test_render_line_marked_with_sgr_wraps_line() {
        let config = default_config();
        let mark_sgr = "\x1b[43m"; // yellow background
        let (rendered, width) = render_line_marked("hello", 0, 80, &config, Some(mark_sgr));
        assert_eq!(rendered, format!("{mark_sgr}hello{STANDOUT_OFF}"));
        assert_eq!(width, 5);
    }

    #[test]
    fn test_render_line_marked_none_sgr_renders_plain() {
        let config = default_config();
        let (rendered, width) = render_line_marked("hello", 0, 80, &config, None);
        assert_eq!(rendered, "hello");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_render_line_marked_empty_sgr_renders_plain() {
        let config = default_config();
        let (rendered, _) = render_line_marked("hello", 0, 80, &config, Some(""));
        assert_eq!(rendered, "hello");
    }

    // --- line_display_width tests ---

    #[test]
    fn test_line_display_width_plain_ascii() {
        let config = default_config();
        assert_eq!(line_display_width("hello world", &config), 11);
    }

    #[test]
    fn test_line_display_width_with_tabs() {
        let config = default_config();
        // Tab at col 0 -> 8 spaces, then "hi" = 2
        assert_eq!(line_display_width("\thi", &config), 10);
    }

    #[test]
    fn test_line_display_width_with_ansi_off_mode() {
        let config = default_config();
        // In Off mode, ESC is displayed as "ESC" (3 chars) and the rest of
        // the sequence is visible text: "ESC[31mredESC[0m" = 16 cols
        assert_eq!(line_display_width("\x1b[31mred\x1b[0m", &config), 16);
    }

    #[test]
    fn test_line_display_width_with_ansi_ansi_only_mode() {
        let config = config_with_mode(RawControlMode::AnsiOnly);
        // In AnsiOnly mode, SGR sequences have zero display width
        assert_eq!(line_display_width("\x1b[31mred\x1b[0m", &config), 3);
    }

    #[test]
    fn test_line_display_width_with_control_chars() {
        let config = default_config();
        // ^A = 2 display cols
        assert_eq!(line_display_width("a\x01b", &config), 4);
    }

    #[test]
    fn test_line_display_width_empty() {
        let config = default_config();
        assert_eq!(line_display_width("", &config), 0);
    }

    // --- apply_chop_markers tests ---

    #[test]
    fn test_chop_markers_no_markers_needed() {
        let (result, width) = apply_chop_markers("hello", 5, 0, false);
        assert_eq!(result, "hello");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_chop_markers_right_truncation_adds_gt() {
        // "hello worl" rendered at 10 cols, truncated on right
        let (result, width) = apply_chop_markers("hello worl", 10, 0, true);
        assert_eq!(result, "hello wor>");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_chop_markers_no_left_marker_when_scrolled_right() {
        // Scrolled right (h_offset=5), showing "world" (5 cols, not truncated right).
        // GNU less does not add a left-side marker, so no `<` should appear.
        let (result, width) = apply_chop_markers("world", 5, 5, false);
        assert_eq!(result, "world");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_chop_markers_right_only_when_scrolled_right() {
        // Scrolled right and truncated: only > on right (no < on left per GNU less)
        let (result, width) = apply_chop_markers("ello worl", 9, 5, true);
        assert_eq!(result, "ello wor>");
        assert_eq!(width, 9);
    }

    #[test]
    fn test_chop_markers_empty_string_no_crash() {
        let (result, width) = apply_chop_markers("", 0, 0, false);
        assert_eq!(result, "");
        assert_eq!(width, 0);
    }

    #[test]
    fn test_chop_markers_single_char_right_truncation() {
        let (result, width) = apply_chop_markers("a", 1, 0, true);
        assert_eq!(result, ">");
        assert_eq!(width, 1);
    }

    #[test]
    fn test_chop_markers_single_char_no_left_marker() {
        // GNU less has no left-side marker, so scrolled right still shows the char
        let (result, width) = apply_chop_markers("a", 1, 1, false);
        assert_eq!(result, "a");
        assert_eq!(width, 1);
    }

    #[test]
    fn test_chop_markers_with_ansi_escapes_right_truncation() {
        // Rendered text with ANSI color: "\x1b[31mhello\x1b[0m" (5 visible cols)
        let input = "\x1b[31mhello\x1b[0m";
        let (result, width) = apply_chop_markers(input, 5, 0, true);
        // The last visible char 'o' should be replaced with '>'
        assert!(result.contains('>'));
        assert_eq!(width, 5);
        // ANSI escapes should be preserved before the marker
        assert!(result.contains("\x1b[31m"));
    }

    #[test]
    fn test_chop_markers_with_ansi_escapes_no_left_marker() {
        let input = "\x1b[31mworld\x1b[0m";
        let (result, width) = apply_chop_markers(input, 5, 5, false);
        // GNU less has no left-side marker, so the text is unchanged
        assert_eq!(result, input);
        assert_eq!(width, 5);
    }

    // --- BinFmt parsing and rendering tests ---

    #[test]
    fn test_binfmt_parse_default_lessbinfmt() {
        let fmt = BinFmt::parse("*s<%02X>");
        assert!(fmt.standout);
        assert_eq!(fmt.segments.len(), 4);
        assert_eq!(fmt.segments[0], BinFmtSegment::Literal("*".to_string()));
        assert_eq!(fmt.segments[1], BinFmtSegment::Literal("<".to_string()));
        assert_eq!(fmt.segments[2], BinFmtSegment::Format("%02X".to_string()));
        assert_eq!(fmt.segments[3], BinFmtSegment::Literal(">".to_string()));
    }

    #[test]
    fn test_binfmt_format_byte_hex() {
        let fmt = BinFmt::parse("*s<%02X>");
        assert_eq!(fmt.format_byte(0x01), "*<01>");
        assert_eq!(fmt.format_byte(0xFF), "*<FF>");
    }

    #[test]
    fn test_binfmt_parse_utf_default() {
        let fmt = BinFmt::parse("<U+%04X>");
        assert!(!fmt.standout);
        assert_eq!(fmt.format_byte(0xFFFD), "<U+FFFD>");
    }

    #[test]
    fn test_binfmt_display_width() {
        let fmt = BinFmt::parse("*s<%02X>");
        // "*<01>" is 5 chars
        assert_eq!(fmt.display_width(0x01), 5);
    }

    #[test]
    fn test_render_line_with_binfmt_control_char() {
        let config = RenderConfig {
            bin_fmt: Some(BinFmt::parse("[%02X]")),
            ..RenderConfig::default()
        };
        // \x01 normally renders as "^A" (2 chars), with binfmt should be "[01]"
        let (rendered, width) = render_line("a\x01b", 0, 80, &config);
        assert_eq!(rendered, "a[01]b");
        assert_eq!(width, 6);
    }

    #[test]
    fn test_render_line_without_binfmt_control_char_is_caret() {
        let config = RenderConfig::default();
        let (rendered, width) = render_line("a\x01b", 0, 80, &config);
        assert_eq!(rendered, "a^Ab");
        assert_eq!(width, 4);
    }

    #[test]
    fn test_render_line_with_binfmt_esc_char() {
        let config = RenderConfig {
            bin_fmt: Some(BinFmt::parse("[%02X]")),
            ..RenderConfig::default()
        };
        // \x1b normally renders as "ESC" (3 chars), with binfmt "[1B]"
        let (rendered, width) = render_line("a\x1bb", 0, 80, &config);
        assert_eq!(rendered, "a[1B]b");
        assert_eq!(width, 6);
    }

    #[test]
    fn test_render_line_with_binfmt_del_char() {
        let config = RenderConfig {
            bin_fmt: Some(BinFmt::parse("[%02X]")),
            ..RenderConfig::default()
        };
        // \x7f normally renders as "^?" (2 chars), with binfmt "[7F]"
        let (rendered, width) = render_line("a\x7fb", 0, 80, &config);
        assert_eq!(rendered, "a[7F]b");
        assert_eq!(width, 6);
    }

    #[test]
    fn test_expand_char_with_binfmt() {
        let tabs = TabStops::regular(8);
        let fmt = BinFmt::parse("[%02X]");
        let result = expand_char('\x01', 0, &tabs, Some(&fmt));
        assert_eq!(result, "[01]");
    }

    #[test]
    fn test_expand_char_tab_ignores_binfmt() {
        let tabs = TabStops::regular(8);
        let fmt = BinFmt::parse("[%02X]");
        // Tab should still expand to spaces, not use binfmt
        let result = expand_char('\t', 0, &tabs, Some(&fmt));
        assert_eq!(result, "        ");
    }

    #[test]
    fn test_line_display_width_with_binfmt() {
        let config = RenderConfig {
            bin_fmt: Some(BinFmt::parse("[%02X]")),
            ..RenderConfig::default()
        };
        // "a\x01b": a=1, [01]=4, b=1 = 6
        let width = line_display_width("a\x01b", &config);
        assert_eq!(width, 6);
    }

    // --- Chop marker combining mark tests (Task 301) ---

    #[test]
    fn test_apply_chop_markers_combining_marks_correct_position() {
        // "e\u{0301}f" rendered in 10 cols with right truncation.
        // "e\u{0301}" = 1 display col, "f" = 1 display col => total 2.
        // With display_width=2 and truncated_right, marker col = 1.
        // The "f" at visible_col=1 should be replaced with ">".
        let (result, width) = apply_chop_markers("e\u{0301}f", 2, 0, true);
        assert_eq!(width, 2);
        // The combining mark should stay with its base character 'e',
        // and 'f' is replaced by '>'
        assert!(
            result.contains("e\u{0301}"),
            "combining mark should stay with base char: {result}"
        );
        assert!(result.contains('>'), "should have right marker: {result}");
    }

    #[test]
    fn test_apply_chop_markers_cjk_correct_position() {
        // CJK char (width 2) + ASCII char at display width 3.
        // "\u{4e2d}a" = 2+1 = 3 display cols.
        // With display_width=3 and truncated, marker_col=2.
        // 'a' at visible_col=2 should be replaced with '>'.
        let (result, width) = apply_chop_markers("\u{4e2d}a", 3, 0, true);
        assert_eq!(width, 3);
        assert!(
            result.contains('\u{4e2d}'),
            "CJK char should be present: {result}"
        );
        assert!(result.contains('>'), "should have right marker: {result}");
        assert!(
            !result.contains('a'),
            "ASCII 'a' should be replaced by marker: {result}"
        );
    }

    // --- ANSI extended color passthrough tests (task 310) ---

    #[test]
    fn test_render_line_ansi_only_256_color_fg_passes_through() {
        // Spec §310.1: 256-color foreground sequence must appear in output
        let input = "\x1b[38;5;196mred text\x1b[0m";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line(input, 0, 80, &config);
        assert!(
            rendered.contains("\x1b[38;5;196m"),
            "256-color fg SGR should be passed through: {rendered:?}"
        );
        assert!(
            rendered.contains("red text"),
            "visible text should be present: {rendered:?}"
        );
    }

    #[test]
    fn test_render_line_ansi_only_256_color_bg_passes_through() {
        // Spec §310.2: 256-color background sequence must be preserved
        let input = "\x1b[48;5;21mblue bg\x1b[0m";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line(input, 0, 80, &config);
        assert!(
            rendered.contains("\x1b[48;5;21m"),
            "256-color bg SGR should be passed through: {rendered:?}"
        );
        assert!(
            rendered.contains("blue bg"),
            "visible text should be present: {rendered:?}"
        );
    }

    #[test]
    fn test_render_line_ansi_only_24bit_fg_passes_through() {
        // Spec §310.3: 24-bit true color foreground must be preserved
        let input = "\x1b[38;2;255;128;0morange\x1b[0m";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line(input, 0, 80, &config);
        assert!(
            rendered.contains("\x1b[38;2;255;128;0m"),
            "24-bit fg SGR should be passed through: {rendered:?}"
        );
        assert!(
            rendered.contains("orange"),
            "visible text should be present: {rendered:?}"
        );
    }

    #[test]
    fn test_render_line_ansi_only_24bit_bg_passes_through() {
        // Spec §310.4: 24-bit true color background must be preserved
        let input = "\x1b[48;2;0;128;255msky bg\x1b[0m";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line(input, 0, 80, &config);
        assert!(
            rendered.contains("\x1b[48;2;0;128;255m"),
            "24-bit bg SGR should be passed through: {rendered:?}"
        );
        assert!(
            rendered.contains("sky bg"),
            "visible text should be present: {rendered:?}"
        );
    }

    #[test]
    fn test_render_line_ansi_only_mixed_truecolor_and_bold_passes_through() {
        // Spec §310.5: combined bold + 24-bit color params must both be preserved
        let input = "\x1b[1;38;2;255;0;0mbold red\x1b[0m";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line(input, 0, 80, &config);
        assert!(
            rendered.contains("\x1b[1;38;2;255;0;0m"),
            "combined bold+24-bit SGR should be passed through: {rendered:?}"
        );
        assert!(
            rendered.contains("bold red"),
            "visible text should be present: {rendered:?}"
        );
    }

    #[test]
    fn test_render_line_ansi_only_multiple_color_sequences_all_preserved() {
        // Spec §310.6: multiple color changes in one line must all be preserved
        let input =
            "\x1b[38;2;255;0;0mred\x1b[0m \x1b[38;2;0;255;0mgreen\x1b[0m \x1b[38;2;0;0;255mblue\x1b[0m";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line(input, 0, 80, &config);
        assert!(
            rendered.contains("\x1b[38;2;255;0;0m"),
            "red SGR should be present: {rendered:?}"
        );
        assert!(
            rendered.contains("\x1b[38;2;0;255;0m"),
            "green SGR should be present: {rendered:?}"
        );
        assert!(
            rendered.contains("\x1b[38;2;0;0;255m"),
            "blue SGR should be present: {rendered:?}"
        );
        assert!(
            rendered.contains("red") && rendered.contains("green") && rendered.contains("blue"),
            "all visible text should be present: {rendered:?}"
        );
    }

    #[test]
    fn test_render_line_ansi_only_non_sgr_csi_stripped() {
        // Spec §310.7: non-SGR CSI (e.g., erase) must be stripped; only text remains
        let input = "\x1b[2Jclear";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (rendered, _) = render_line(input, 0, 80, &config);
        assert_eq!(
            rendered, "clear",
            "non-SGR CSI should be stripped, leaving only text"
        );
    }

    #[test]
    fn test_render_line_ansi_only_truecolor_width_equals_visible_text_only() {
        // Spec §310.8: width returned must equal visible text width, excluding escapes
        let input = "\x1b[38;2;255;128;0morange\x1b[0m";
        let config = config_with_mode(RawControlMode::AnsiOnly);
        let (_, width) = render_line(input, 0, 80, &config);
        // "orange" is 6 characters, all ASCII (width 1 each)
        assert_eq!(width, 6, "width should equal visible text width only");
    }

    // ── Multi-highlight render tests ──

    #[test]
    fn test_render_line_multi_highlighted_no_ranges_matches_render_line() {
        let config = RenderConfig::default();
        let line = "hello world";
        let (plain, plain_w) = render_line(line, 0, 80, &config);
        let (multi, multi_w) = render_line_multi_highlighted(line, 0, 80, &config, &[]);
        assert_eq!(plain, multi);
        assert_eq!(plain_w, multi_w);
    }

    #[test]
    fn test_render_line_multi_highlighted_single_range_uses_its_sgr() {
        let config = RenderConfig::default();
        let ranges = [ColoredRange {
            start: 6,
            end: 11,
            sgr: "\x1b[30;43m",
        }];
        let (rendered, _) = render_line_multi_highlighted("hello world", 0, 80, &config, &ranges);
        assert_eq!(rendered, format!("hello \x1b[30;43mworld{STANDOUT_OFF}"));
    }

    #[test]
    fn test_render_line_multi_highlighted_two_ranges_different_colors() {
        let config = RenderConfig::default();
        let ranges = [
            ColoredRange {
                start: 0,
                end: 5,
                sgr: "\x1b[7m",
            },
            ColoredRange {
                start: 6,
                end: 11,
                sgr: "\x1b[30;43m",
            },
        ];
        let (rendered, _) = render_line_multi_highlighted("hello world", 0, 80, &config, &ranges);
        assert_eq!(
            rendered,
            format!("\x1b[7mhello{STANDOUT_OFF} \x1b[30;43mworld{STANDOUT_OFF}")
        );
    }

    #[test]
    fn test_render_line_multi_highlighted_clips_at_horizontal_offset() {
        let config = RenderConfig::default();
        let ranges = [ColoredRange {
            start: 6,
            end: 11,
            sgr: "\x1b[7m",
        }];
        let (rendered, _) = render_line_multi_highlighted("hello world", 8, 80, &config, &ranges);
        // Offset 8 means we start at "rld", which is still in the highlight range
        assert_eq!(rendered, format!("\x1b[7mrld{STANDOUT_OFF}"));
    }

    #[test]
    fn test_render_line_multi_highlighted_reset_between_adjacent_colors() {
        let config = RenderConfig::default();
        // Two adjacent ranges with different colors
        let ranges = [
            ColoredRange {
                start: 0,
                end: 5,
                sgr: "\x1b[7m",
            },
            ColoredRange {
                start: 5,
                end: 10,
                sgr: "\x1b[30;43m",
            },
        ];
        let (rendered, _) = render_line_multi_highlighted("helloworld!", 0, 80, &config, &ranges);
        // Should reset between them
        assert!(rendered.contains(&format!("{STANDOUT_OFF}\x1b[30;43m")));
    }

    /// Regression: search highlights on SGR-colored text must use text-only byte offsets.
    ///
    /// Highlights are computed on raw text (no SGR), but the renderer processes
    /// text with injected SGR escape sequences. The highlight lookup must use a
    /// text-only byte counter that skips SGR bytes, otherwise highlights land on
    /// wrong characters or suppress coloring entirely.
    #[test]
    fn test_highlight_on_sgr_colored_text_uses_text_offsets() {
        let config = RenderConfig {
            raw_mode: RawControlMode::AnsiOnly,
            ..RenderConfig::default()
        };
        // Input: "hello" with SGR foreground wrapping it.
        // Raw text would be "hello" (5 bytes). SGR adds ~8 bytes before and 4 after.
        let colored_line = "\x1b[32mhello\x1b[0m";
        // Highlight bytes 0..5 of the RAW text ("hello").
        let highlights = [(0usize, 5usize)];
        let hl_on = "\x1b[7m";

        let (rendered, _) =
            render_line_highlighted(colored_line, 0, 80, &config, &highlights, Some(hl_on));

        // The highlight SGR should appear in the output.
        assert!(
            rendered.contains(hl_on),
            "highlight not applied to SGR-colored text: {rendered:?}"
        );
        // "hello" should still be present.
        assert!(
            rendered.contains("hello"),
            "text content missing: {rendered:?}"
        );
    }

    /// Regression: search highlight on diff-colored line preserves diff coloring.
    ///
    /// When searching in diff mode, the diff background tinting (e.g., green for
    /// added lines) must survive the highlight overlay. The highlight should only
    /// affect the matched region; surrounding text keeps its diff coloring.
    #[test]
    fn test_highlight_preserves_surrounding_sgr() {
        let config = RenderConfig {
            raw_mode: RawControlMode::AnsiOnly,
            ..RenderConfig::default()
        };
        // Simulate a diff-colored line: green bg wrapping "hello world end".
        // Highlight "world" in the MIDDLE so there's text after it that needs
        // the green bg restored.
        let green_bg = "\x1b[48;2;20;60;20m";
        let reset = "\x1b[0m";
        let colored_line = format!("{green_bg}hello world end{reset}");
        // Highlight "world" (bytes 6..11 in raw text "hello world end").
        let highlights = [(6usize, 11usize)];
        let hl_on = "\x1b[7m";

        let (rendered, _) =
            render_line_highlighted(&colored_line, 0, 80, &config, &highlights, Some(hl_on));

        // The highlight should appear.
        assert!(rendered.contains(hl_on), "highlight missing: {rendered:?}");
        // The green background should be present (for the non-highlighted portion).
        assert!(
            rendered.contains(green_bg),
            "diff coloring lost: {rendered:?}"
        );
        // After the highlight ends, the green bg should be restored for " end".
        assert!(
            rendered.contains(&format!("{STANDOUT_OFF}{green_bg}")),
            "diff coloring not restored after highlight: {rendered:?}"
        );
    }
}
