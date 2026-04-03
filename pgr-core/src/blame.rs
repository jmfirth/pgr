//! Git blame line parsing.
//!
//! Parses the output of `git blame` (both porcelain and non-porcelain formats)
//! into structured `BlameLine` values for recency-based coloring and optional
//! code extraction for syntax highlighting.

/// A parsed `git blame` line.
///
/// `git blame` output format:
/// ```text
/// <hash> (<author> <date> <line-no>) <code>
/// ```
/// where `<hash>` is 7–40 hex characters, `<date>` is `YYYY-MM-DD`, and the
/// parenthesized field may also include a time and timezone offset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    /// Abbreviated or full commit hash (7–40 hex characters).
    pub hash: String,
    /// Commit author name, trimmed.
    pub author: String,
    /// Commit date as `YYYY-MM-DD`.
    pub date: String,
    /// The source code portion after the closing `)`.
    pub code: String,
}

/// Parse a single `git blame` output line.
///
/// Returns `None` if the line doesn't match the expected format.
///
/// Supports both short and full hashes. The parenthesised metadata block may
/// contain optional time and timezone fields — only the author name (everything
/// before the first digit-started date field) and the date are extracted.
///
/// # Examples
///
/// ```
/// use pgr_core::blame::parse_blame_line;
///
/// let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
/// let bl = parse_blame_line(line).unwrap();
/// assert_eq!(bl.hash, "abcdef1");
/// assert_eq!(bl.author, "Alice");
/// assert_eq!(bl.date, "2024-03-15");
/// assert_eq!(bl.code, "fn main() {}");
/// ```
#[must_use]
pub fn parse_blame_line(line: &str) -> Option<BlameLine> {
    let bytes = line.as_bytes();

    // Extract hash: 7–40 hex characters at the start.
    let mut hash_end = 0;
    for &b in bytes {
        if b.is_ascii_hexdigit() {
            hash_end += 1;
            if hash_end > 40 {
                return None;
            }
        } else {
            break;
        }
    }
    if hash_end < 7 {
        return None;
    }
    let hash = &line[..hash_end];

    // Expect a space after the hash.
    let rest = line[hash_end..].trim_start_matches(' ');

    // Expect the opening parenthesis `(`.
    let rest = rest.strip_prefix('(')?;

    // Find the closing `)` of the metadata block (first occurrence).
    let paren_close = rest.find(')')?;
    let meta = &rest[..paren_close];
    // Git blame adds exactly one space between `)` and the code. Strip it, preserving
    // any additional indentation that is part of the code itself.
    let after_paren = &rest[paren_close + 1..];
    let code = after_paren.strip_prefix(' ').unwrap_or(after_paren);

    // Parse author and date from the metadata.
    // Meta format (no fixed separators): "<author> <YYYY-MM-DD> [time] [tz] <lineno>"
    // We find the date by scanning for a "YYYY-MM-DD" pattern.
    let (author, date) = extract_author_and_date(meta)?;

    Some(BlameLine {
        hash: hash.to_string(),
        author,
        date,
        code: code.to_string(),
    })
}

/// Extract the author name and `YYYY-MM-DD` date from the parenthesised metadata.
///
/// The author occupies everything before the date field. The date is the first
/// `NNNN-NN-NN` token (where all digits) within the metadata string.
fn extract_author_and_date(meta: &str) -> Option<(String, String)> {
    // Walk through space-separated tokens to find the date.
    let mut tokens = meta.splitn(usize::MAX, ' ');
    let mut author_parts: Vec<&str> = Vec::new();

    for token in tokens.by_ref() {
        if is_iso_date(token) {
            let author = author_parts
                .iter()
                .filter(|t| !t.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();
            return Some((author, token.to_string()));
        }
        author_parts.push(token);
    }

    None
}

/// Check whether a token looks like an ISO-8601 date (`YYYY-MM-DD`).
fn is_iso_date(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    // YYYY-MM-DD
    bytes[0].is_ascii_digit()
        && bytes[1].is_ascii_digit()
        && bytes[2].is_ascii_digit()
        && bytes[3].is_ascii_digit()
        && bytes[4] == b'-'
        && bytes[5].is_ascii_digit()
        && bytes[6].is_ascii_digit()
        && bytes[7] == b'-'
        && bytes[8].is_ascii_digit()
        && bytes[9].is_ascii_digit()
}

/// Extract the year component from a `YYYY-MM-DD` date string.
///
/// Returns `None` if the string is not a valid ISO date or the year is not
/// a 4-digit decimal.
#[must_use]
pub fn year_from_date(date: &str) -> Option<u32> {
    let bytes = date.as_bytes();
    if bytes.len() < 4 {
        return None;
    }
    let year_str = &date[..4];
    year_str.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_blame_line_full_hash_returns_blame_line() {
        let line =
            "abcdef1234567890abcdef1234567890abcdef12 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let bl = parse_blame_line(line).unwrap();
        assert_eq!(bl.hash, "abcdef1234567890abcdef1234567890abcdef12");
        assert_eq!(bl.author, "Alice");
        assert_eq!(bl.date, "2024-03-15");
        assert_eq!(bl.code, "fn main() {}");
    }

    #[test]
    fn test_parse_blame_line_short_hash_returns_blame_line() {
        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let bl = parse_blame_line(line).unwrap();
        assert_eq!(bl.hash, "abcdef1");
        assert_eq!(bl.author, "Alice");
        assert_eq!(bl.date, "2024-03-15");
        assert_eq!(bl.code, "fn main() {}");
    }

    #[test]
    fn test_parse_blame_line_multiword_author_returns_blame_line() {
        let line = "abcdef1 (Justin Firth  2023-06-20 09:15:00 -0700  42)     let x = 1;";
        let bl = parse_blame_line(line).unwrap();
        assert_eq!(bl.author, "Justin Firth");
        assert_eq!(bl.date, "2023-06-20");
        assert_eq!(bl.code, "    let x = 1;");
    }

    #[test]
    fn test_parse_blame_line_empty_code_returns_blame_line() {
        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) ";
        let bl = parse_blame_line(line).unwrap();
        assert_eq!(bl.code, "");
    }

    #[test]
    fn test_parse_blame_line_plain_text_returns_none() {
        assert!(parse_blame_line("this is not a blame line").is_none());
    }

    #[test]
    fn test_parse_blame_line_too_short_hash_returns_none() {
        assert!(parse_blame_line("abc (Author  2024-01-01 10:00:00 +0000  1) code").is_none());
    }

    #[test]
    fn test_parse_blame_line_no_paren_returns_none() {
        assert!(parse_blame_line("abcdef1 Author 2024-01-01 code").is_none());
    }

    #[test]
    fn test_year_from_date_valid_returns_year() {
        assert_eq!(year_from_date("2024-03-15"), Some(2024));
        assert_eq!(year_from_date("2020-01-01"), Some(2020));
    }

    #[test]
    fn test_year_from_date_invalid_returns_none() {
        assert_eq!(year_from_date("not-date"), None);
        assert_eq!(year_from_date("24-01-01"), None);
    }

    #[test]
    fn test_is_iso_date_valid() {
        assert!(is_iso_date("2024-01-15"));
        assert!(is_iso_date("2000-12-31"));
    }

    #[test]
    fn test_is_iso_date_invalid_length() {
        assert!(!is_iso_date("24-01-15"));
        assert!(!is_iso_date("2024-1-15"));
    }

    #[test]
    fn test_is_iso_date_no_dashes() {
        assert!(!is_iso_date("20240115ab"));
    }
}
