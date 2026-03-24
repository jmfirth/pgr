//! OSC 8 hyperlink parsing and span tracking.
//!
//! Parses OSC 8 terminal hyperlink sequences and tracks the visible
//! column spans they cover. OSC 8 format:
//! - Opening: `\x1b]8;params;URI\x1b\\` or `\x1b]8;params;URI\x07`
//! - Closing: `\x1b]8;;\x1b\\` or `\x1b]8;;\x07`

use crate::ansi::{parse_ansi, Segment};
use crate::unicode;

/// A hyperlink span within a single line.
///
/// Tracks the visible column range and the associated URI. Column positions
/// are zero-based and refer to display columns (accounting for Unicode width).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HyperlinkSpan {
    /// Starting display column (inclusive, zero-based).
    pub start_col: usize,
    /// Ending display column (exclusive, zero-based).
    pub end_col: usize,
    /// The URI target of the hyperlink.
    pub uri: String,
    /// Optional parameters from the OSC 8 sequence (e.g., `id=...`).
    pub params: String,
}

/// Extract the URI from an OSC 8 opening escape sequence.
///
/// Returns `Some((params, uri))` if the escape is a valid OSC 8 opening
/// sequence with a non-empty URI. Returns `None` for closing sequences
/// or non-OSC-8 escapes.
fn parse_osc8_open(esc: &str) -> Option<(&str, &str)> {
    // Strip the OSC prefix: ESC ] 8 ;
    let body = esc.strip_prefix("\x1b]8;")?;

    // Strip the terminator: ESC \ or BEL
    let body = body
        .strip_suffix("\x1b\\")
        .or_else(|| body.strip_suffix('\x07'))?;

    // Split on the first ';' to separate params from URI
    let semicolon = body.find(';')?;
    let params = &body[..semicolon];
    let uri = &body[semicolon + 1..];

    // A non-empty URI means this is an opening sequence
    if uri.is_empty() {
        None
    } else {
        Some((params, uri))
    }
}

/// Check if an escape sequence is an OSC 8 closing sequence.
///
/// Closing sequences have the form `\x1b]8;;\x1b\\` or `\x1b]8;;\x07`
/// (empty URI).
fn is_osc8_close(esc: &str) -> bool {
    esc == "\x1b]8;;\x1b\\" || esc == "\x1b]8;;\x07"
}

/// Parse a line containing OSC 8 hyperlink sequences and return the spans.
///
/// Each span maps a range of visible display columns to a URI. The column
/// positions account for Unicode character widths and exclude escape
/// sequences from the column count.
///
/// Tab width is used for display-width calculations (passed through to
/// [`unicode::display_width`]).
#[must_use]
pub fn parse_osc8(input: &str, tab_width: usize) -> Vec<HyperlinkSpan> {
    let segments = parse_ansi(input);
    let mut spans = Vec::new();
    let mut col: usize = 0;

    // Active hyperlink state: (start_col, params, uri)
    let mut active: Option<(usize, String, String)> = None;

    for segment in segments {
        match segment {
            Segment::Text(text) => {
                let width = unicode::display_width(text, tab_width);
                col += width;
            }
            Segment::Escape(esc) => {
                if let Some((params, uri)) = parse_osc8_open(esc) {
                    // Close any previously active hyperlink (handles malformed input)
                    if let Some((start, prev_params, prev_uri)) = active.take() {
                        if col > start {
                            spans.push(HyperlinkSpan {
                                start_col: start,
                                end_col: col,
                                uri: prev_uri,
                                params: prev_params,
                            });
                        }
                    }
                    active = Some((col, params.to_string(), uri.to_string()));
                } else if is_osc8_close(esc) {
                    if let Some((start, params, uri)) = active.take() {
                        if col > start {
                            spans.push(HyperlinkSpan {
                                start_col: start,
                                end_col: col,
                                uri,
                                params,
                            });
                        }
                    }
                }
                // Non-OSC-8 escapes are ignored (zero display width)
            }
        }
    }

    // If a hyperlink was never closed, emit the span up to the current column
    if let Some((start, params, uri)) = active.take() {
        if col > start {
            spans.push(HyperlinkSpan {
                start_col: start,
                end_col: col,
                uri,
                params,
            });
        }
    }

    spans
}

/// Strip all OSC 8 hyperlink escape sequences from a string.
///
/// Returns the visible text with OSC 8 opening and closing sequences
/// removed. Other ANSI escape sequences (SGR, CSI, etc.) are preserved.
#[must_use]
pub fn strip_osc8(input: &str) -> String {
    let segments = parse_ansi(input);
    let mut result = String::with_capacity(input.len());

    for segment in segments {
        match segment {
            Segment::Text(text) => {
                result.push_str(text);
            }
            Segment::Escape(esc) => {
                // Strip OSC 8 sequences, preserve everything else
                if parse_osc8_open(esc).is_some() || is_osc8_close(esc) {
                    // Skip OSC 8 sequences
                } else {
                    result.push_str(esc);
                }
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_osc8_open tests ──

    #[test]
    fn test_parse_osc8_open_st_terminator_returns_params_and_uri() {
        let esc = "\x1b]8;;https://example.com\x1b\\";
        let result = parse_osc8_open(esc);
        assert_eq!(result, Some(("", "https://example.com")));
    }

    #[test]
    fn test_parse_osc8_open_bel_terminator_returns_params_and_uri() {
        let esc = "\x1b]8;;https://example.com\x07";
        let result = parse_osc8_open(esc);
        assert_eq!(result, Some(("", "https://example.com")));
    }

    #[test]
    fn test_parse_osc8_open_with_params_returns_params() {
        let esc = "\x1b]8;id=link1;https://example.com\x1b\\";
        let result = parse_osc8_open(esc);
        assert_eq!(result, Some(("id=link1", "https://example.com")));
    }

    #[test]
    fn test_parse_osc8_open_close_sequence_returns_none() {
        let esc = "\x1b]8;;\x1b\\";
        let result = parse_osc8_open(esc);
        assert_eq!(result, None);
    }

    #[test]
    fn test_parse_osc8_open_non_osc8_returns_none() {
        let esc = "\x1b[31m";
        let result = parse_osc8_open(esc);
        assert_eq!(result, None);
    }

    // ── is_osc8_close tests ──

    #[test]
    fn test_is_osc8_close_st_terminator_returns_true() {
        assert!(is_osc8_close("\x1b]8;;\x1b\\"));
    }

    #[test]
    fn test_is_osc8_close_bel_terminator_returns_true() {
        assert!(is_osc8_close("\x1b]8;;\x07"));
    }

    #[test]
    fn test_is_osc8_close_opening_sequence_returns_false() {
        assert!(!is_osc8_close("\x1b]8;;https://example.com\x1b\\"));
    }

    #[test]
    fn test_is_osc8_close_sgr_sequence_returns_false() {
        assert!(!is_osc8_close("\x1b[31m"));
    }

    // ── parse_osc8 tests ──

    #[test]
    fn test_parse_osc8_single_link_returns_correct_span() {
        let input = "\x1b]8;;https://example.com\x1b\\click here\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 10); // "click here" = 10 columns
        assert_eq!(spans[0].uri, "https://example.com");
        assert_eq!(spans[0].params, "");
    }

    #[test]
    fn test_parse_osc8_link_with_preceding_text_correct_columns() {
        let input = "Hello \x1b]8;;https://example.com\x1b\\world\x1b]8;;\x1b\\!";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_col, 6); // "Hello " = 6 columns
        assert_eq!(spans[0].end_col, 11); // "world" = 5 columns
        assert_eq!(spans[0].uri, "https://example.com");
    }

    #[test]
    fn test_parse_osc8_multiple_links_returns_all_spans() {
        let input =
            "\x1b]8;;https://a.com\x1b\\A\x1b]8;;\x1b\\ \x1b]8;;https://b.com\x1b\\B\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 1);
        assert_eq!(spans[0].uri, "https://a.com");
        assert_eq!(spans[1].start_col, 2); // "A " = 2 columns
        assert_eq!(spans[1].end_col, 3);
        assert_eq!(spans[1].uri, "https://b.com");
    }

    #[test]
    fn test_parse_osc8_bel_terminator_returns_correct_span() {
        let input = "\x1b]8;;https://example.com\x07link\x1b]8;;\x07";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 4); // "link" = 4 columns
        assert_eq!(spans[0].uri, "https://example.com");
    }

    #[test]
    fn test_parse_osc8_with_params_preserves_params() {
        let input = "\x1b]8;id=foo;https://example.com\x1b\\text\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].params, "id=foo");
        assert_eq!(spans[0].uri, "https://example.com");
    }

    #[test]
    fn test_parse_osc8_no_links_returns_empty() {
        let input = "plain text with no links";
        let spans = parse_osc8(input, 8);
        assert!(spans.is_empty());
    }

    #[test]
    fn test_parse_osc8_empty_input_returns_empty() {
        let spans = parse_osc8("", 8);
        assert!(spans.is_empty());
    }

    #[test]
    fn test_parse_osc8_unclosed_link_emits_span_to_end() {
        let input = "\x1b]8;;https://example.com\x1b\\orphaned text";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 13); // "orphaned text" = 13 columns
        assert_eq!(spans[0].uri, "https://example.com");
    }

    #[test]
    fn test_parse_osc8_adjacent_links_no_gap() {
        let input =
            "\x1b]8;;https://a.com\x1b\\A\x1b]8;;\x1b\\\x1b]8;;https://b.com\x1b\\B\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].end_col, 1);
        assert_eq!(spans[1].start_col, 1);
    }

    #[test]
    fn test_parse_osc8_link_with_sgr_inside_correct_columns() {
        // Link text has SGR coloring inside
        let input = "\x1b]8;;https://example.com\x1b\\\x1b[31mred link\x1b[0m\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 8); // "red link" = 8 columns
    }

    #[test]
    fn test_parse_osc8_link_with_cjk_text_correct_width() {
        // CJK characters are 2 columns wide each
        let input = "\x1b]8;;https://example.com\x1b\\\u{4e2d}\u{6587}\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 4); // 2 CJK chars = 4 columns
    }

    #[test]
    fn test_parse_osc8_empty_text_link_produces_no_span() {
        // Link with no visible text between open and close
        let input = "\x1b]8;;https://example.com\x1b\\\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert!(spans.is_empty());
    }

    #[test]
    fn test_parse_osc8_nested_open_closes_previous() {
        // A new open before a close should finalize the previous span
        let input =
            "\x1b]8;;https://a.com\x1b\\first\x1b]8;;https://b.com\x1b\\second\x1b]8;;\x1b\\";
        let spans = parse_osc8(input, 8);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].uri, "https://a.com");
        assert_eq!(spans[0].start_col, 0);
        assert_eq!(spans[0].end_col, 5); // "first"
        assert_eq!(spans[1].uri, "https://b.com");
        assert_eq!(spans[1].start_col, 5);
        assert_eq!(spans[1].end_col, 11); // "second"
    }

    // ── strip_osc8 tests ──

    #[test]
    fn test_strip_osc8_removes_hyperlink_sequences() {
        let input = "\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\";
        assert_eq!(strip_osc8(input), "link");
    }

    #[test]
    fn test_strip_osc8_preserves_sgr_sequences() {
        let input = "\x1b[31m\x1b]8;;https://example.com\x1b\\red link\x1b]8;;\x1b\\\x1b[0m";
        assert_eq!(strip_osc8(input), "\x1b[31mred link\x1b[0m");
    }

    #[test]
    fn test_strip_osc8_plain_text_returns_same() {
        let input = "no links here";
        assert_eq!(strip_osc8(input), "no links here");
    }

    #[test]
    fn test_strip_osc8_empty_returns_empty() {
        assert_eq!(strip_osc8(""), "");
    }

    #[test]
    fn test_strip_osc8_bel_terminated_sequences_removed() {
        let input = "\x1b]8;;https://example.com\x07link\x1b]8;;\x07";
        assert_eq!(strip_osc8(input), "link");
    }

    #[test]
    fn test_strip_osc8_multiple_links_all_removed() {
        let input = "\x1b]8;;https://a.com\x1b\\A\x1b]8;;\x1b\\ and \x1b]8;;https://b.com\x1b\\B\x1b]8;;\x1b\\";
        assert_eq!(strip_osc8(input), "A and B");
    }

    #[test]
    fn test_strip_osc8_preserves_other_osc_sequences() {
        // OSC 0 (set title) should be preserved
        let input = "\x1b]0;title\x07hello";
        assert_eq!(strip_osc8(input), "\x1b]0;title\x07hello");
    }
}
