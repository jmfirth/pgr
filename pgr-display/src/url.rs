//! URL detection via regex for plain-text content.
//!
//! Scans line content for URLs matching common schemes (http, https, ftp, ftps,
//! file, mailto) and returns their byte positions and extracted text. Handles
//! trailing punctuation stripping and parenthesized URLs (e.g., Wikipedia links).

use regex::Regex;
use std::sync::OnceLock;

/// A URL match found within a line of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrlMatch {
    /// Byte offset of the URL start within the line (inclusive).
    pub start: usize,
    /// Byte offset of the URL end within the line (exclusive).
    pub end: usize,
    /// The extracted URL text (after trailing punctuation cleanup).
    pub url: String,
}

/// Compile the URL detection regex once and cache it.
fn url_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // Match common URL schemes followed by non-whitespace characters.
        // We do post-processing to strip trailing punctuation and balance parens.
        Regex::new(r"(?i)(?:https?|ftps?|file)://[^\s<>\x00-\x1f]+|mailto:[^\s<>\x00-\x1f]+")
            .expect("URL regex is valid")
    })
}

/// Strip ANSI escape sequences from a string for URL detection.
///
/// Returns the plain text with ANSI escapes removed, and a mapping from
/// plain-text byte offsets back to original byte offsets.
fn strip_ansi_for_detection(input: &str) -> (String, Vec<usize>) {
    let mut plain = String::with_capacity(input.len());
    let mut offset_map: Vec<usize> = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        if bytes[i] == 0x1b {
            // ESC sequence: skip until end of sequence
            i += 1;
            if i < len {
                match bytes[i] {
                    b'[' => {
                        // CSI sequence: skip until final byte (0x40-0x7E)
                        i += 1;
                        while i < len && !(0x40..=0x7E).contains(&bytes[i]) {
                            i += 1;
                        }
                        if i < len {
                            i += 1; // skip final byte
                        }
                    }
                    b']' => {
                        // OSC sequence: skip until ST (ESC \) or BEL
                        i += 1;
                        while i < len {
                            if bytes[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // Other ESC sequences: skip one more byte
                        i += 1;
                    }
                }
            }
        } else {
            let ch_start = i;
            // Get the full UTF-8 character
            let ch = &input[i..];
            if let Some(c) = ch.chars().next() {
                let c_len = c.len_utf8();
                for byte_idx in 0..c_len {
                    offset_map.push(ch_start + byte_idx);
                }
                plain.push(c);
                i += c_len;
            } else {
                // Invalid UTF-8, skip byte
                offset_map.push(ch_start);
                plain.push(char::REPLACEMENT_CHARACTER);
                i += 1;
            }
        }
    }

    (plain, offset_map)
}

/// Strip trailing punctuation from a URL match, respecting balanced parentheses.
///
/// Characters like `.`, `,`, `;`, `:`, `!`, `?`, `'`, `"` are stripped from
/// the end. Closing parentheses `)` are stripped only if there is no matching
/// open parenthesis within the URL.
fn clean_url_trailing(url: &str) -> &str {
    let mut end = url.len();
    let bytes = url.as_bytes();

    while end > 0 {
        let last = bytes[end - 1];
        match last {
            b'.' | b',' | b';' | b':' | b'!' | b'?' | b'\'' | b'"' => {
                end -= 1;
            }
            b')' => {
                // Count open and close parens in the URL up to this point.
                // Use a running depth counter to avoid naive bytecount.
                let mut depth: i32 = 0;
                for &b in &bytes[..end] {
                    if b == b'(' {
                        depth += 1;
                    } else if b == b')' {
                        depth -= 1;
                    }
                }
                if depth < 0 {
                    // More close parens than open: strip the trailing one
                    end -= 1;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }

    &url[..end]
}

/// Find all URLs in a line of text.
///
/// Handles ANSI escape sequences transparently — escape sequences within or
/// around URLs are ignored for matching purposes, and the returned byte
/// offsets refer to positions in the original (unstripped) string.
///
/// Returns an empty Vec if no URLs are found.
#[must_use]
pub fn find_urls(line: &str) -> Vec<UrlMatch> {
    if line.is_empty() {
        return Vec::new();
    }

    let (plain, offset_map) = strip_ansi_for_detection(line);
    let re = url_regex();
    let mut results = Vec::new();

    for m in re.find_iter(&plain) {
        let matched_text = m.as_str();
        let cleaned = clean_url_trailing(matched_text);
        if cleaned.is_empty() {
            continue;
        }

        let plain_start = m.start();
        let plain_end = plain_start + cleaned.len();

        // Map back to original string offsets
        if plain_start < offset_map.len() && plain_end > 0 && plain_end <= offset_map.len() {
            let orig_start = offset_map[plain_start];
            // end is exclusive: the byte after the last character.
            // When plain_end is at the map boundary, compute end from the
            // last mapped byte plus its UTF-8 character length, rather than
            // using line.len() which would include trailing escapes.
            let orig_end = if plain_end < offset_map.len() {
                offset_map[plain_end]
            } else {
                let last_orig = offset_map[plain_end - 1];
                // Advance past the last character (could be multi-byte)
                let remaining = &line[last_orig..];
                last_orig + remaining.chars().next().map_or(1, char::len_utf8)
            };

            results.push(UrlMatch {
                start: orig_start,
                end: orig_end,
                url: cleaned.to_string(),
            });
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── find_urls basic tests ──

    #[test]
    fn test_find_urls_empty_input_returns_empty() {
        assert!(find_urls("").is_empty());
    }

    #[test]
    fn test_find_urls_no_urls_returns_empty() {
        assert!(find_urls("just some plain text").is_empty());
    }

    #[test]
    fn test_find_urls_single_http_url() {
        let matches = find_urls("Visit https://example.com for info");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    #[test]
    fn test_find_urls_http_without_tls() {
        let matches = find_urls("See http://example.com/page");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "http://example.com/page");
    }

    #[test]
    fn test_find_urls_ftp_scheme() {
        let matches = find_urls("Download from ftp://files.example.com/pub");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "ftp://files.example.com/pub");
    }

    #[test]
    fn test_find_urls_file_scheme() {
        let matches = find_urls("Open file:///home/user/doc.txt");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "file:///home/user/doc.txt");
    }

    #[test]
    fn test_find_urls_mailto_scheme() {
        let matches = find_urls("Email mailto:user@example.com");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "mailto:user@example.com");
    }

    #[test]
    fn test_find_urls_multiple_urls_in_line() {
        let matches = find_urls("A https://a.com B https://b.com C");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].url, "https://a.com");
        assert_eq!(matches[1].url, "https://b.com");
    }

    #[test]
    fn test_find_urls_url_at_start_of_line() {
        let matches = find_urls("https://example.com is a URL");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start, 0);
    }

    #[test]
    fn test_find_urls_url_at_end_of_line() {
        let matches = find_urls("Visit https://example.com");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    // ── Trailing punctuation stripping ──

    #[test]
    fn test_find_urls_strips_trailing_period() {
        let matches = find_urls("See https://example.com.");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    #[test]
    fn test_find_urls_strips_trailing_comma() {
        let matches = find_urls("Visit https://example.com, then continue");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    #[test]
    fn test_find_urls_strips_trailing_semicolon() {
        let matches = find_urls("See https://example.com;");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    #[test]
    fn test_find_urls_strips_trailing_question_mark() {
        let matches = find_urls("Is it at https://example.com?");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    #[test]
    fn test_find_urls_preserves_query_string_question_mark() {
        let matches = find_urls("See https://example.com?q=test for results");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com?q=test");
    }

    // ── Parenthesized URLs ──

    #[test]
    fn test_find_urls_parenthesized_url_balanced() {
        let matches = find_urls("(https://en.wikipedia.org/wiki/Rust_(programming_language))");
        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].url,
            "https://en.wikipedia.org/wiki/Rust_(programming_language)"
        );
    }

    #[test]
    fn test_find_urls_unbalanced_trailing_paren_stripped() {
        let matches = find_urls("(see https://example.com)");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    // ── ANSI escape handling ──

    #[test]
    fn test_find_urls_with_sgr_escapes_detected() {
        let matches = find_urls("\x1b[34mhttps://example.com\x1b[0m");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "https://example.com");
    }

    #[test]
    fn test_find_urls_with_ansi_offsets_map_to_original() {
        let input = "\x1b[34mhttps://example.com\x1b[0m";
        let matches = find_urls(input);
        assert_eq!(matches.len(), 1);
        // The URL in the original string starts after the SGR escape \x1b[34m (5 bytes)
        assert_eq!(matches[0].start, 5);
        assert_eq!(
            &input[matches[0].start..matches[0].end],
            "https://example.com"
        );
    }

    #[test]
    fn test_find_urls_with_osc8_hyperlink_escapes() {
        let input = "\x1b]8;;https://target.com\x1b\\visible text\x1b]8;;\x1b\\";
        // The OSC8 sequences are stripped; "visible text" has no URL scheme
        let matches = find_urls(input);
        assert!(matches.is_empty());
    }

    // ── Byte offset correctness ──

    #[test]
    fn test_find_urls_byte_offsets_correct_for_plain_text() {
        let input = "Go to https://example.com now";
        let matches = find_urls(input);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            &input[matches[0].start..matches[0].end],
            "https://example.com"
        );
    }

    #[test]
    fn test_find_urls_byte_offsets_with_unicode_prefix() {
        let input = "\u{1F600} https://example.com";
        let matches = find_urls(input);
        assert_eq!(matches.len(), 1);
        assert_eq!(
            &input[matches[0].start..matches[0].end],
            "https://example.com"
        );
    }

    // ── clean_url_trailing unit tests ──

    #[test]
    fn test_clean_url_trailing_no_trailing_returns_same() {
        assert_eq!(
            clean_url_trailing("https://example.com"),
            "https://example.com"
        );
    }

    #[test]
    fn test_clean_url_trailing_strips_multiple_punctuation() {
        assert_eq!(
            clean_url_trailing("https://example.com.,;"),
            "https://example.com"
        );
    }

    #[test]
    fn test_clean_url_trailing_preserves_path_dot() {
        // A dot within a path segment should not be stripped
        assert_eq!(
            clean_url_trailing("https://example.com/file.txt"),
            "https://example.com/file.txt"
        );
    }

    // ── Case insensitive scheme ──

    #[test]
    fn test_find_urls_case_insensitive_scheme() {
        let matches = find_urls("Visit HTTPS://EXAMPLE.COM for info");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].url, "HTTPS://EXAMPLE.COM");
    }

    // ── strip_ansi_for_detection ──

    #[test]
    fn test_strip_ansi_plain_text_unchanged() {
        let (plain, map) = strip_ansi_for_detection("hello");
        assert_eq!(plain, "hello");
        assert_eq!(map, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_strip_ansi_removes_sgr() {
        let (plain, _) = strip_ansi_for_detection("\x1b[31mred\x1b[0m");
        assert_eq!(plain, "red");
    }

    #[test]
    fn test_strip_ansi_removes_osc() {
        let (plain, _) = strip_ansi_for_detection("\x1b]8;;url\x1b\\text\x1b]8;;\x1b\\");
        assert_eq!(plain, "text");
    }
}
