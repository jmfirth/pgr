//! Content mode detection — sniffs the first lines of a file to determine
//! its type (diff, man page, git blame, git log, JSON, SQL table, or plain).

use std::fmt;

/// Detected content type for context-sensitive rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentMode {
    /// Plain text — no special rendering. Default/fallback.
    Plain,
    /// Unified diff (git diff, diff -u, patch files).
    Diff,
    /// Man page (groff/troff output with backspace overprinting).
    ManPage,
    /// Git blame output (abbreviated or full commit hash prefix on every line).
    GitBlame,
    /// Git log output (commit headers, author, date).
    GitLog,
    /// JSON content (starts with `{` or `[`).
    Json,
    /// SQL table output (ASCII box drawing or aligned columns).
    SqlTable,
}

impl fmt::Display for ContentMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plain => write!(f, "plain"),
            Self::Diff => write!(f, "diff mode"),
            Self::ManPage => write!(f, "man page"),
            Self::GitBlame => write!(f, "git blame"),
            Self::GitLog => write!(f, "git log"),
            Self::Json => write!(f, "JSON"),
            Self::SqlTable => write!(f, "SQL table"),
        }
    }
}

impl ContentMode {
    /// Returns the status label shown briefly on first paint, or `None` for plain text.
    #[must_use]
    pub fn status_label(self) -> Option<String> {
        if self == Self::Plain {
            None
        } else {
            Some(format!("[{self}]"))
        }
    }
}

/// Maximum number of lines examined by detectors.
const MAX_DETECT_LINES: usize = 50;

/// Sniff the first lines of content and detect the content mode.
///
/// Examines up to 50 lines and applies detectors in priority order
/// (Diff > `ManPage` > `GitBlame` > `GitLog` > JSON > `SqlTable`).
/// Returns [`ContentMode::Plain`] if no pattern matches.
#[must_use]
pub fn detect_content_mode(lines: &[&str]) -> ContentMode {
    let lines = if lines.len() > MAX_DETECT_LINES {
        &lines[..MAX_DETECT_LINES]
    } else {
        lines
    };

    if is_diff(lines) {
        ContentMode::Diff
    } else if is_man_page(lines) {
        ContentMode::ManPage
    } else if is_git_blame(lines) {
        ContentMode::GitBlame
    } else if is_git_log(lines) {
        ContentMode::GitLog
    } else if is_json(lines) {
        ContentMode::Json
    } else if is_sql_table(lines) {
        ContentMode::SqlTable
    } else {
        ContentMode::Plain
    }
}

/// Detect unified diff / patch content.
///
/// Matches:
/// - Line 1 starts with `diff --git `
/// - Line 1 starts with `--- ` AND line 2 starts with `+++ `
/// - Any of the first 10 lines contains a `@@ -<digits> +<digits> @@` hunk header
fn is_diff(lines: &[&str]) -> bool {
    if lines.is_empty() {
        return false;
    }

    // Scan all available lines (up to MAX_DETECT_LINES) for diff markers.
    // This handles `git log -p` where commit headers precede the diff,
    // and `git show` where the diff starts after the commit message.
    for (i, line) in lines.iter().enumerate() {
        // git diff header anywhere in the content
        if line.starts_with("diff --git ") {
            return true;
        }
        // unified diff header (two consecutive lines)
        if line.starts_with("--- ") && i + 1 < lines.len() && lines[i + 1].starts_with("+++ ") {
            return true;
        }
        // hunk header
        if is_hunk_header(line) {
            return true;
        }
    }

    false
}

/// Check if a line looks like a unified diff hunk header: `@@ -N,N +N,N @@`
///
/// Uses byte-level parsing instead of regex since pgr-core has no regex dependency.
fn is_hunk_header(line: &str) -> bool {
    let bytes = line.as_bytes();

    // Must start with "@@ -"
    if !bytes.starts_with(b"@@ -") {
        return false;
    }

    let mut i = 4; // skip "@@ -"

    // Expect at least one digit
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return false;
    }
    // Skip digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    // Optional comma + digits
    if i < bytes.len() && bytes[i] == b',' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }

    // Expect " +"
    if i + 1 >= bytes.len() || bytes[i] != b' ' || bytes[i + 1] != b'+' {
        return false;
    }
    i += 2;

    // Expect at least one digit
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return false;
    }
    // Skip digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    // Optional comma + digits
    if i < bytes.len() && bytes[i] == b',' {
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
    }

    // Expect " @@"
    i + 2 < bytes.len() && bytes[i] == b' ' && bytes[i + 1] == b'@' && bytes[i + 2] == b'@'
}

/// Detect man page output via backspace overprinting.
///
/// Troff/groff encodes bold as `X\x08X` and underline as `_\x08X`.
/// Checks the first 20 lines for this byte pattern.
fn is_man_page(lines: &[&str]) -> bool {
    let check_count = lines.len().min(20);
    for line in &lines[..check_count] {
        let bytes = line.as_bytes();
        // Look for the pattern: any byte, then 0x08 (backspace), then any byte
        if bytes.len() >= 3 {
            for window in bytes.windows(3) {
                if window[1] == 0x08 {
                    return true;
                }
            }
        }
    }
    false
}

/// Detect git blame output.
///
/// Every non-empty line in the first 10 lines must start with 7-40 hex
/// characters followed by a space.
fn is_git_blame(lines: &[&str]) -> bool {
    let check_count = lines.len().min(10);
    let non_empty: Vec<&&str> = lines[..check_count]
        .iter()
        .filter(|l| !l.is_empty())
        .collect();

    // Need at least one non-empty line
    if non_empty.is_empty() {
        return false;
    }

    non_empty.iter().all(|line| starts_with_hex_hash(line))
}

/// Check if a line starts with 7-40 hex characters followed by a space.
fn starts_with_hex_hash(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut hex_count = 0;

    for &b in bytes {
        if b.is_ascii_hexdigit() {
            hex_count += 1;
            if hex_count > 40 {
                return false;
            }
        } else {
            return b == b' ' && hex_count >= 7;
        }
    }

    false
}

/// Detect git log output.
///
/// First non-empty line matches `commit <40-hex-chars>` (optionally with
/// more text after), and within the first 5 lines there is an `Author: `
/// or `author ` line.
fn is_git_log(lines: &[&str]) -> bool {
    let first_non_empty = lines.iter().find(|l| !l.is_empty());
    let Some(first) = first_non_empty else {
        return false;
    };

    // First non-empty line must start with "commit " followed by 40 hex chars
    if !first.starts_with("commit ") {
        return false;
    }
    let after_commit = &first[7..];
    if after_commit.len() < 40 {
        return false;
    }
    let hash_part = &after_commit[..40];
    if !hash_part.bytes().all(|b| b.is_ascii_hexdigit()) {
        return false;
    }

    // Within first 5 lines, look for Author: or author
    let check_count = lines.len().min(5);
    for line in &lines[..check_count] {
        let trimmed = line.trim_start();
        if trimmed.starts_with("Author: ") || trimmed.starts_with("author ") {
            return true;
        }
    }

    false
}

/// Detect JSON content.
///
/// The first non-whitespace character in the first 5 lines must be `{` or `[`.
fn is_json(lines: &[&str]) -> bool {
    let check_count = lines.len().min(5);
    for line in &lines[..check_count] {
        for ch in line.chars() {
            if ch.is_whitespace() {
                continue;
            }
            return ch == '{' || ch == '[';
        }
    }
    false
}

/// Detect SQL table output (psql, mysql, sqlite3).
///
/// Looks for ASCII box-drawing patterns in the first 5 lines:
/// - A line matching `+[-+]+` (the horizontal rule)
/// - Or a line matching `|...|` combined with a horizontal rule line
fn is_sql_table(lines: &[&str]) -> bool {
    let check_count = lines.len().min(5);
    let subset = &lines[..check_count];

    let has_rule = subset.iter().any(|l| is_sql_rule_line(l));
    if has_rule {
        return true;
    }

    // Check for pipe-delimited rows alongside a rule line (already checked above,
    // but kept explicit for clarity — if we have a rule line we're already returning true)
    false
}

/// Check if a line is a SQL table horizontal rule: `+[-+]+`
///
/// Must start and end with `+`, containing only `-` and `+` characters.
fn is_sql_rule_line(line: &str) -> bool {
    let trimmed = line.trim_end();
    if trimmed.len() < 3 {
        return false;
    }
    let bytes = trimmed.as_bytes();
    if bytes[0] != b'+' || bytes[bytes.len() - 1] != b'+' {
        return false;
    }
    // Must contain at least one '-'
    let has_dash = bytes.contains(&b'-');
    if !has_dash {
        return false;
    }
    // All characters must be '+' or '-'
    bytes.iter().all(|&b| b == b'+' || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_git_diff_format_returns_diff() {
        let lines = vec![
            "diff --git a/foo.rs b/foo.rs",
            "index 1234567..abcdefg 100644",
            "--- a/foo.rs",
            "+++ b/foo.rs",
            "@@ -1,3 +1,4 @@",
            " line 1",
            "+added line",
            " line 2",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::Diff);
    }

    #[test]
    fn test_detect_unified_diff_returns_diff() {
        let lines = vec![
            "--- a/foo.rs",
            "+++ b/foo.rs",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::Diff);
    }

    #[test]
    fn test_detect_patch_with_hunk_headers_returns_diff() {
        let lines = vec![
            "Some preamble text",
            "More context",
            "@@ -10,5 +10,7 @@ fn main() {",
            "     code here",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::Diff);
    }

    #[test]
    fn test_detect_man_page_overprinting_returns_manpage() {
        // Bold 'H' is encoded as H\x08H
        let lines = vec!["N\x08NA\x08AM\x08ME\x08E", "     some description"];
        assert_eq!(detect_content_mode(&lines), ContentMode::ManPage);
    }

    #[test]
    fn test_detect_git_blame_returns_gitblame() {
        let lines = vec![
            "abcdef1234567890abcdef1234567890abcdef12 (Author 2024-01-01  1) fn main() {",
            "abcdef1234567890abcdef1234567890abcdef12 (Author 2024-01-01  2)     println!(\"hello\");",
            "abcdef1234567890abcdef1234567890abcdef12 (Author 2024-01-01  3) }",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::GitBlame);
    }

    #[test]
    fn test_detect_git_blame_partial_returns_plain() {
        // Some lines start with hash, some don't — should NOT be GitBlame
        let lines = vec![
            "abcdef1234567890abcdef1234567890abcdef12 (Author 2024-01-01  1) fn main() {",
            "this is not a blame line",
            "abcdef1234567890abcdef1234567890abcdef12 (Author 2024-01-01  3) }",
        ];
        assert_ne!(detect_content_mode(&lines), ContentMode::GitBlame);
    }

    #[test]
    fn test_detect_git_log_returns_gitlog() {
        let lines = vec![
            "commit abcdef1234567890abcdef1234567890abcdef12",
            "Author: Test User <test@example.com>",
            "Date:   Mon Jan 1 00:00:00 2024 +0000",
            "",
            "    Initial commit",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::GitLog);
    }

    #[test]
    fn test_detect_json_object_returns_json() {
        let lines = vec!["{", "  \"key\": \"value\"", "}"];
        assert_eq!(detect_content_mode(&lines), ContentMode::Json);
    }

    #[test]
    fn test_detect_json_array_returns_json() {
        let lines = vec!["[", "  1, 2, 3", "]"];
        assert_eq!(detect_content_mode(&lines), ContentMode::Json);
    }

    #[test]
    fn test_detect_json_with_leading_whitespace_returns_json() {
        let lines = vec!["  {", "    \"key\": \"value\"", "  }"];
        assert_eq!(detect_content_mode(&lines), ContentMode::Json);
    }

    #[test]
    fn test_detect_sql_table_returns_sqltable() {
        let lines = vec![
            "+------+------+",
            "| col1 | col2 |",
            "+------+------+",
            "| a    | b    |",
            "+------+------+",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::SqlTable);
    }

    #[test]
    fn test_detect_plain_text_returns_plain() {
        let lines = vec!["hello world", "foo bar", "baz qux"];
        assert_eq!(detect_content_mode(&lines), ContentMode::Plain);
    }

    #[test]
    fn test_detect_empty_input_returns_plain() {
        let lines: Vec<&str> = vec![];
        assert_eq!(detect_content_mode(&lines), ContentMode::Plain);
    }

    #[test]
    fn test_priority_diff_over_gitlog() {
        // Content that looks like both git log and a diff — diff should win
        let lines = vec![
            "diff --git a/foo.rs b/foo.rs",
            "commit abcdef1234567890abcdef1234567890abcdef12",
            "Author: Test <test@example.com>",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::Diff);
    }

    #[test]
    fn test_hunk_header_basic() {
        assert!(is_hunk_header("@@ -1,3 +1,4 @@"));
    }

    #[test]
    fn test_hunk_header_no_comma() {
        assert!(is_hunk_header("@@ -1 +1 @@"));
    }

    #[test]
    fn test_hunk_header_with_context() {
        assert!(is_hunk_header("@@ -10,5 +10,7 @@ fn main() {"));
    }

    #[test]
    fn test_hunk_header_not_matching() {
        assert!(!is_hunk_header("not a hunk header"));
        assert!(!is_hunk_header("@@ garbage @@"));
        assert!(!is_hunk_header("@@"));
    }

    #[test]
    fn test_sql_rule_line() {
        assert!(is_sql_rule_line("+---+---+"));
        assert!(is_sql_rule_line("+------+------+"));
        assert!(!is_sql_rule_line("| a | b |"));
        assert!(!is_sql_rule_line("++"));
        assert!(!is_sql_rule_line("not a rule"));
    }

    #[test]
    fn test_git_blame_abbreviated_hash() {
        let lines = vec![
            "abcdef1 (Author 2024-01-01  1) fn main() {",
            "abcdef1 (Author 2024-01-01  2)     code",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::GitBlame);
    }

    #[test]
    fn test_content_mode_display() {
        assert_eq!(ContentMode::Diff.to_string(), "diff mode");
        assert_eq!(ContentMode::ManPage.to_string(), "man page");
        assert_eq!(ContentMode::GitBlame.to_string(), "git blame");
        assert_eq!(ContentMode::GitLog.to_string(), "git log");
        assert_eq!(ContentMode::Json.to_string(), "JSON");
        assert_eq!(ContentMode::SqlTable.to_string(), "SQL table");
        assert_eq!(ContentMode::Plain.to_string(), "plain");
    }

    #[test]
    fn test_content_mode_status_label() {
        assert_eq!(ContentMode::Plain.status_label(), None);
        assert_eq!(
            ContentMode::Diff.status_label(),
            Some("[diff mode]".to_string())
        );
        assert_eq!(
            ContentMode::ManPage.status_label(),
            Some("[man page]".to_string())
        );
    }

    #[test]
    fn test_detect_man_page_underline() {
        // Underline 'N' is encoded as _\x08N
        let lines = vec!["_\x08N_\x08A_\x08M_\x08E"];
        assert_eq!(detect_content_mode(&lines), ContentMode::ManPage);
    }

    #[test]
    fn test_git_log_with_author_lowercase() {
        let lines = vec![
            "commit abcdef1234567890abcdef1234567890abcdef12",
            "author Test User <test@example.com>",
        ];
        assert_eq!(detect_content_mode(&lines), ContentMode::GitLog);
    }
}
