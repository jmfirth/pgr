//! ANSI escape sequence parser for terminal output.
//!
//! Parses ANSI/ECMA-48 escape sequences into zero-copy segments,
//! supporting CSI, OSC, and simple escape sequences. Provides
//! utilities for stripping escapes and measuring visible display width.

use crate::unicode;

/// A segment of parsed terminal output, borrowing from the input string.
///
/// Each segment is either visible text or an escape sequence. Escape
/// sequences include CSI (e.g., SGR color codes), OSC (e.g., hyperlinks),
/// and simple two-byte ESC sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment<'a> {
    /// Visible text content.
    Text(&'a str),
    /// An ANSI escape sequence (CSI, OSC, or simple ESC).
    Escape(&'a str),
}

/// Parser states for the ANSI escape sequence state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Normal text processing.
    Ground,
    /// Seen ESC (0x1B), waiting for next byte to determine sequence type.
    Escape,
    /// Inside a CSI sequence (ESC [), consuming parameter/intermediate/final bytes.
    Csi,
    /// Inside an OSC sequence (ESC ]), consuming until ST or BEL.
    Osc,
    /// Inside an OSC sequence, seen ESC that might start ST (ESC \).
    OscEscape,
}

/// Parse a string into segments of text and ANSI escape sequences.
///
/// Segments borrow from the input string (zero-copy). The parser recognizes:
/// - **CSI sequences**: `ESC [` followed by parameter bytes (0x30-0x3F),
///   intermediate bytes (0x20-0x2F), and a final byte (0x40-0x7E).
/// - **OSC sequences**: `ESC ]` followed by arbitrary bytes until ST
///   (`ESC \`) or BEL (0x07).
/// - **Simple escape sequences**: `ESC` followed by a single byte in 0x40-0x5F
///   (excluding `[` and `]` which start CSI/OSC).
///
/// Unterminated sequences at end-of-input are emitted as escape segments.
#[must_use]
pub fn parse_ansi(input: &str) -> Vec<Segment<'_>> {
    if input.is_empty() {
        return Vec::new();
    }

    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut segments: Vec<Segment<'_>> = Vec::new();
    let mut state = State::Ground;
    let mut seg_start: usize = 0;
    let mut i: usize = 0;

    while i < len {
        let b = bytes[i];

        match state {
            State::Ground => {
                if b == 0x1B {
                    // Flush any accumulated text
                    if i > seg_start {
                        segments.push(Segment::Text(&input[seg_start..i]));
                    }
                    seg_start = i;
                    state = State::Escape;
                }
            }
            State::Escape => {
                match b {
                    b'[' => {
                        state = State::Csi;
                    }
                    b']' => {
                        state = State::Osc;
                    }
                    // Simple escape: ESC + byte in 0x40-0x5F (C1 controls)
                    0x40..=0x5F => {
                        segments.push(Segment::Escape(&input[seg_start..=i]));
                        seg_start = i + 1;
                        state = State::Ground;
                    }
                    // Unexpected byte after ESC: emit ESC as escape, reprocess byte
                    _ => {
                        segments.push(Segment::Escape(&input[seg_start..i]));
                        seg_start = i;
                        state = State::Ground;
                        continue; // Reprocess current byte in Ground state
                    }
                }
            }
            State::Csi => {
                match b {
                    // Parameter bytes (0x30-0x3F) and intermediate bytes (0x20-0x2F)
                    0x20..=0x3F => {}
                    // Final byte: terminates the CSI sequence
                    0x40..=0x7E => {
                        segments.push(Segment::Escape(&input[seg_start..=i]));
                        seg_start = i + 1;
                        state = State::Ground;
                    }
                    // Invalid byte in CSI: emit what we have as escape, reprocess
                    _ => {
                        segments.push(Segment::Escape(&input[seg_start..i]));
                        seg_start = i;
                        state = State::Ground;
                        continue;
                    }
                }
            }
            State::Osc => {
                match b {
                    // BEL terminates OSC
                    0x07 => {
                        segments.push(Segment::Escape(&input[seg_start..=i]));
                        seg_start = i + 1;
                        state = State::Ground;
                    }
                    // ESC might start ST (ESC \)
                    0x1B => {
                        state = State::OscEscape;
                    }
                    // All other bytes are part of the OSC payload
                    _ => {}
                }
            }
            State::OscEscape => {
                if b == b'\\' {
                    // ST found: ESC \ terminates the OSC sequence
                    segments.push(Segment::Escape(&input[seg_start..=i]));
                    seg_start = i + 1;
                    state = State::Ground;
                } else {
                    // Not ST, the ESC was part of the OSC payload; continue OSC
                    state = State::Osc;
                    continue; // Reprocess current byte in Osc state
                }
            }
        }
        i += 1;
    }

    // Flush remaining content
    if seg_start < len {
        match state {
            State::Ground => {
                segments.push(Segment::Text(&input[seg_start..]));
            }
            // Unterminated escape sequences are emitted as escapes (lenient)
            State::Escape | State::Csi | State::Osc | State::OscEscape => {
                segments.push(Segment::Escape(&input[seg_start..]));
            }
        }
    }

    segments
}

/// Strip all ANSI escape sequences from a string, returning only visible text.
///
/// This is a convenience wrapper over [`parse_ansi`] that concatenates
/// all `Text` segments.
#[must_use]
pub fn strip_ansi(input: &str) -> String {
    let segments = parse_ansi(input);
    let mut result = String::with_capacity(input.len());
    for seg in segments {
        if let Segment::Text(text) = seg {
            result.push_str(text);
        }
    }
    result
}

/// Calculate the display width of a string containing ANSI escape sequences.
///
/// Escape sequences contribute zero display width. Visible text is measured
/// using [`unicode::display_width`] with the given `tab_width`.
#[must_use]
pub fn display_width_ansi(input: &str, tab_width: usize) -> usize {
    let stripped = strip_ansi(input);
    unicode::display_width(&stripped, tab_width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ansi_plain_text_returns_single_text_segment() {
        let segments = parse_ansi("hello world");
        assert_eq!(segments, vec![Segment::Text("hello world")]);
    }

    #[test]
    fn test_parse_ansi_simple_sgr_returns_escape_text_escape() {
        let input = "\x1b[31mred\x1b[0m";
        let segments = parse_ansi(input);
        assert_eq!(
            segments,
            vec![
                Segment::Escape("\x1b[31m"),
                Segment::Text("red"),
                Segment::Escape("\x1b[0m"),
            ]
        );
    }

    #[test]
    fn test_parse_ansi_multiple_sgr_correct_segmentation() {
        let input = "\x1b[1m\x1b[31mbold red\x1b[0m";
        let segments = parse_ansi(input);
        assert_eq!(
            segments,
            vec![
                Segment::Escape("\x1b[1m"),
                Segment::Escape("\x1b[31m"),
                Segment::Text("bold red"),
                Segment::Escape("\x1b[0m"),
            ]
        );
    }

    #[test]
    fn test_parse_ansi_osc8_hyperlink_correct_segments() {
        // OSC 8 hyperlink: ESC]8;;url ESC\ visible_text ESC]8;; ESC\
        let input = "\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\";
        let segments = parse_ansi(input);
        assert_eq!(
            segments,
            vec![
                Segment::Escape("\x1b]8;;https://example.com\x1b\\"),
                Segment::Text("link"),
                Segment::Escape("\x1b]8;;\x1b\\"),
            ]
        );
    }

    #[test]
    fn test_parse_ansi_osc_terminated_by_bel_single_escape() {
        let input = "\x1b]0;title\x07";
        let segments = parse_ansi(input);
        assert_eq!(segments, vec![Segment::Escape("\x1b]0;title\x07")]);
    }

    #[test]
    fn test_parse_ansi_adjacent_escapes_no_text_between() {
        let input = "\x1b[1m\x1b[31m\x1b[4m";
        let segments = parse_ansi(input);
        assert_eq!(
            segments,
            vec![
                Segment::Escape("\x1b[1m"),
                Segment::Escape("\x1b[31m"),
                Segment::Escape("\x1b[4m"),
            ]
        );
    }

    #[test]
    fn test_parse_ansi_unterminated_csi_at_end_treated_as_escape() {
        let input = "text\x1b[31";
        let segments = parse_ansi(input);
        assert_eq!(
            segments,
            vec![Segment::Text("text"), Segment::Escape("\x1b[31"),]
        );
    }

    #[test]
    fn test_strip_ansi_removes_all_escapes() {
        let input = "\x1b[1m\x1b[31mbold red\x1b[0m normal";
        assert_eq!(strip_ansi(input), "bold red normal");
    }

    #[test]
    fn test_strip_ansi_plain_text_returns_same_string() {
        let input = "hello world";
        assert_eq!(strip_ansi(input), "hello world");
    }

    #[test]
    fn test_display_width_ansi_colored_text_same_width_as_plain() {
        let plain = "hello";
        let colored = "\x1b[31mhello\x1b[0m";
        assert_eq!(
            display_width_ansi(colored, 8),
            unicode::display_width(plain, 8)
        );
    }

    #[test]
    fn test_display_width_ansi_cjk_with_color_correct_width() {
        // 中 = 2 cells, 文 = 2 cells => total 4
        let input = "\x1b[32m\u{4e2d}\u{6587}\x1b[0m";
        assert_eq!(display_width_ansi(input, 8), 4);
    }

    #[test]
    fn test_parse_ansi_empty_string_returns_empty_segments() {
        let segments = parse_ansi("");
        assert!(segments.is_empty());
    }
}
