//! Man page section detection — finds section headers in man page output.
//!
//! Man pages rendered by troff/groff use backspace overprinting to encode
//! bold and underline. This module strips those sequences before checking
//! whether a line is a section header.

/// A detected section header in a man page buffer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManSection {
    /// Line number (0-indexed in buffer) of this section header.
    pub line: usize,
    /// Section name after stripping overprint formatting (e.g., `"OPTIONS"`).
    pub name: String,
}

/// Strip backspace overprint sequences from a line.
///
/// Troff encodes bold as `X\x08X` (char, backspace, same char) and underline
/// as `_\x08X`. This function removes both patterns, returning the printable
/// text only. The result is used for section header detection.
#[must_use]
pub fn strip_overstrike(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if i + 2 < bytes.len() && bytes[i + 1] == 0x08 {
            // Pattern: char, BS, char — skip the first char and the backspace,
            // keep the second char (the visible character).
            i += 2; // skip `char` + BS; next iteration picks up the following char
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    String::from_utf8_lossy(&out).into_owned()
}

/// Parse section headers from man page buffer lines.
///
/// A line is considered a section header when, after stripping overprint
/// formatting, it consists entirely of ASCII uppercase letters, spaces,
/// and hyphens — and is at least two characters long. Leading whitespace
/// is also stripped before the check (some man pages indent sub-sections
/// with a single space).
///
/// Returns the sections in document order. The `line` field is the
/// 0-indexed buffer line number of the header.
#[must_use]
pub fn find_sections(lines: &[&str]) -> Vec<ManSection> {
    let mut sections = Vec::new();

    for (i, &raw) in lines.iter().enumerate() {
        let stripped = strip_overstrike(raw);
        let candidate = stripped.trim_start();

        if is_section_header(candidate) {
            sections.push(ManSection {
                line: i,
                name: candidate.trim_end().to_owned(),
            });
        }
    }

    sections
}

/// Return true if `text` (already stripped of overprinting and leading
/// whitespace) looks like a man page section header.
///
/// Criteria:
/// - At least 2 characters long.
/// - Every character is ASCII uppercase, ASCII digit, space, or hyphen.
/// - Contains at least one ASCII uppercase letter.
/// - Does not begin with a digit (avoids matching lines like "42 items").
fn is_section_header(text: &str) -> bool {
    if text.len() < 2 {
        return false;
    }

    // Must not start with a digit
    if text.starts_with(|c: char| c.is_ascii_digit()) {
        return false;
    }

    let mut has_upper = false;
    for ch in text.chars() {
        match ch {
            'A'..='Z' => {
                has_upper = true;
            }
            '0'..='9' | ' ' | '-' | '_' => {}
            _ => return false,
        }
    }

    has_upper
}

/// Find the next section header line number after `current_line`.
///
/// If `wrap` is true and no section follows, wraps around to the first
/// section. Returns `None` if there are no sections at all.
#[must_use]
pub fn next_section_line(
    sections: &[ManSection],
    current_line: usize,
    wrap: bool,
) -> Option<usize> {
    for sec in sections {
        if sec.line > current_line {
            return Some(sec.line);
        }
    }

    if wrap {
        sections.first().map(|s| s.line)
    } else {
        None
    }
}

/// Find the previous section header line number before `current_line`.
///
/// If `wrap` is true and no section precedes, wraps around to the last
/// section. Returns `None` if there are no sections at all.
#[must_use]
pub fn prev_section_line(
    sections: &[ManSection],
    current_line: usize,
    wrap: bool,
) -> Option<usize> {
    let mut best: Option<usize> = None;

    for sec in sections {
        if sec.line < current_line {
            best = Some(sec.line);
        }
    }

    if best.is_some() {
        return best;
    }

    if wrap {
        sections.last().map(|s| s.line)
    } else {
        None
    }
}

/// Build a status message for the given section position.
///
/// Returns `"Section N of T: NAME"` where N is 1-based index and T is total.
/// Returns an empty string if `sections` is empty or `target_line` is not found.
#[must_use]
pub fn section_status(sections: &[ManSection], target_line: usize) -> String {
    let total = sections.len();
    for (idx, sec) in sections.iter().enumerate() {
        if sec.line == target_line {
            return format!("Section {} of {}: {}", idx + 1, total, sec.name);
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_overstrike ──────────────────────────────────────────────────────

    #[test]
    fn test_strip_overstrike_plain_text_unchanged() {
        assert_eq!(strip_overstrike("hello"), "hello");
    }

    #[test]
    fn test_strip_overstrike_bold_sequence_stripped() {
        // Bold 'O' is O\x08O — should reduce to single O
        let input = "O\x08OP\x08PT\x08TI\x08IO\x08ON\x08NS\x08S";
        assert_eq!(strip_overstrike(input), "OPTIONS");
    }

    #[test]
    fn test_strip_overstrike_underline_sequence_stripped() {
        // Underline 'N' is _\x08N
        let input = "_\x08N_\x08A_\x08M_\x08E";
        assert_eq!(strip_overstrike(input), "NAME");
    }

    #[test]
    fn test_strip_overstrike_mixed_bold_and_plain() {
        // "B\x08Bo" — bold B followed by plain 'o'
        let input = "B\x08Bo";
        assert_eq!(strip_overstrike(input), "Bo");
    }

    #[test]
    fn test_strip_overstrike_empty_string_returns_empty() {
        assert_eq!(strip_overstrike(""), "");
    }

    // ── is_section_header ─────────────────────────────────────────────────────

    #[test]
    fn test_is_section_header_single_word_all_caps_returns_true() {
        assert!(is_section_header("OPTIONS"));
    }

    #[test]
    fn test_is_section_header_multi_word_all_caps_returns_true() {
        assert!(is_section_header("EXIT STATUS"));
    }

    #[test]
    fn test_is_section_header_with_hyphen_returns_true() {
        assert!(is_section_header("SEE-ALSO"));
    }

    #[test]
    fn test_is_section_header_lowercase_returns_false() {
        assert!(!is_section_header("options"));
    }

    #[test]
    fn test_is_section_header_mixed_case_returns_false() {
        assert!(!is_section_header("Options"));
    }

    #[test]
    fn test_is_section_header_too_short_returns_false() {
        assert!(!is_section_header("A"));
    }

    #[test]
    fn test_is_section_header_starts_with_digit_returns_false() {
        assert!(!is_section_header("42 THINGS"));
    }

    #[test]
    fn test_is_section_header_empty_returns_false() {
        assert!(!is_section_header(""));
    }

    #[test]
    fn test_is_section_header_only_spaces_returns_false() {
        assert!(!is_section_header("   "));
    }

    #[test]
    fn test_is_section_header_contains_punctuation_returns_false() {
        assert!(!is_section_header("OPTIONS:"));
    }

    // ── find_sections ─────────────────────────────────────────────────────────

    #[test]
    fn test_find_sections_plain_lines_returns_empty() {
        let lines = vec!["hello world", "  indented text", "more text"];
        assert_eq!(find_sections(&lines), vec![]);
    }

    #[test]
    fn test_find_sections_typical_man_page_detects_headers() {
        let lines = vec![
            "N\x08NA\x08AM\x08ME\x08E", // bold NAME
            "     program - short description",
            "S\x08SY\x08YN\x08NO\x08OP\x08PS\x08SI\x08IS\x08S", // bold SYNOPSIS
            "     program [options]",
            "D\x08DE\x08ES\x08SC\x08CR\x08RI\x08IP\x08PT\x08TI\x08IO\x08ON\x08N", // bold DESCRIPTION
        ];
        let sections = find_sections(&lines);
        assert_eq!(sections.len(), 3);
        assert_eq!(
            sections[0],
            ManSection {
                line: 0,
                name: "NAME".to_string()
            }
        );
        assert_eq!(
            sections[1],
            ManSection {
                line: 2,
                name: "SYNOPSIS".to_string()
            }
        );
        assert_eq!(
            sections[2],
            ManSection {
                line: 4,
                name: "DESCRIPTION".to_string()
            }
        );
    }

    #[test]
    fn test_find_sections_plain_caps_headers_detected() {
        let lines = vec![
            "NAME",
            "  program description",
            "OPTIONS",
            "  -h  show help",
        ];
        let sections = find_sections(&lines);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].line, 0);
        assert_eq!(sections[0].name, "NAME");
        assert_eq!(sections[1].line, 2);
        assert_eq!(sections[1].name, "OPTIONS");
    }

    #[test]
    fn test_find_sections_indented_header_detected() {
        // Some man pages indent section headers with leading spaces
        let lines = vec!["  OPTIONS", "     -h help"];
        let sections = find_sections(&lines);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "OPTIONS");
    }

    #[test]
    fn test_find_sections_empty_input_returns_empty() {
        let lines: Vec<&str> = vec![];
        assert_eq!(find_sections(&lines), vec![]);
    }

    #[test]
    fn test_find_sections_multi_word_section_detected() {
        let lines = vec!["EXIT STATUS", "  0  success"];
        let sections = find_sections(&lines);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].name, "EXIT STATUS");
    }

    // ── next_section_line ─────────────────────────────────────────────────────

    #[test]
    fn test_next_section_line_returns_next_after_current() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
            ManSection {
                line: 20,
                name: "SEE ALSO".to_string(),
            },
        ];
        assert_eq!(next_section_line(&sections, 0, false), Some(10));
    }

    #[test]
    fn test_next_section_line_at_last_no_wrap_returns_none() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
        ];
        assert_eq!(next_section_line(&sections, 10, false), None);
    }

    #[test]
    fn test_next_section_line_at_last_with_wrap_returns_first() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
        ];
        assert_eq!(next_section_line(&sections, 10, true), Some(0));
    }

    #[test]
    fn test_next_section_line_empty_sections_returns_none() {
        assert_eq!(next_section_line(&[], 5, true), None);
    }

    // ── prev_section_line ─────────────────────────────────────────────────────

    #[test]
    fn test_prev_section_line_returns_prev_before_current() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
            ManSection {
                line: 20,
                name: "SEE ALSO".to_string(),
            },
        ];
        assert_eq!(prev_section_line(&sections, 20, false), Some(10));
    }

    #[test]
    fn test_prev_section_line_at_first_no_wrap_returns_none() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
        ];
        assert_eq!(prev_section_line(&sections, 0, false), None);
    }

    #[test]
    fn test_prev_section_line_at_first_with_wrap_returns_last() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
        ];
        assert_eq!(prev_section_line(&sections, 0, true), Some(10));
    }

    #[test]
    fn test_prev_section_line_empty_sections_returns_none() {
        assert_eq!(prev_section_line(&[], 5, true), None);
    }

    // ── section_status ────────────────────────────────────────────────────────

    #[test]
    fn test_section_status_found_returns_formatted_string() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
            ManSection {
                line: 20,
                name: "SEE ALSO".to_string(),
            },
        ];
        assert_eq!(section_status(&sections, 10), "Section 2 of 3: OPTIONS");
    }

    #[test]
    fn test_section_status_first_section_returns_correct_index() {
        let sections = vec![
            ManSection {
                line: 0,
                name: "NAME".to_string(),
            },
            ManSection {
                line: 10,
                name: "OPTIONS".to_string(),
            },
        ];
        assert_eq!(section_status(&sections, 0), "Section 1 of 2: NAME");
    }

    #[test]
    fn test_section_status_line_not_found_returns_empty() {
        let sections = vec![ManSection {
            line: 5,
            name: "NAME".to_string(),
        }];
        assert_eq!(section_status(&sections, 99), "");
    }

    #[test]
    fn test_section_status_empty_sections_returns_empty() {
        assert_eq!(section_status(&[], 0), "");
    }
}
