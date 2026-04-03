//! Git blame line coloring — recency-based hash/author/date gutter and
//! optional syntax highlighting of the code column.
//!
//! Each blame line is rendered as:
//!   `<hash-gutter>  <author-gutter>  <date-gutter>  <code>`
//!
//! The gutter colors shift from dim grey (old commits) through warm orange to
//! bright yellow for very recent commits, giving an instant visual "heat map"
//! of code age. The current year is passed in so rendering stays deterministic
//! in tests.

use pgr_core::blame::{parse_blame_line, year_from_date};

/// Width of the hash gutter column (characters). Abbreviated hashes are
/// typically 7–12 chars; we reserve enough for the common case.
const HASH_WIDTH: usize = 8;

/// Width of the author gutter column (characters). Truncated or padded to fit.
const AUTHOR_WIDTH: usize = 12;

/// Reset all SGR attributes.
const RESET: &str = "\x1b[0m";

/// Dim grey — commits older than 3 years.
const COLOR_OLD: &str = "\x1b[38;2;100;100;100m";
/// Muted blue-grey — commits 1–3 years ago.
const COLOR_MEDIUM: &str = "\x1b[38;2;140;160;180m";
/// Warm orange — commits within the past year.
const COLOR_RECENT: &str = "\x1b[38;2;220;160;80m";
/// Bright yellow — commits within the past 6 months.
const COLOR_HOT: &str = "\x1b[38;2;255;220;80m";

/// Choose a gutter color based on commit age in years.
fn age_color(commit_year: u32, current_year: u32) -> &'static str {
    let age = current_year.saturating_sub(commit_year);
    match age {
        0 => COLOR_HOT,
        1..=2 => COLOR_RECENT,
        3..=5 => COLOR_MEDIUM,
        _ => COLOR_OLD,
    }
}

/// Truncate or right-pad a string to exactly `width` bytes (ASCII-safe).
///
/// Non-ASCII characters are not counted by byte length so this is only
/// guaranteed correct for ASCII content (author names and hex hashes).
fn fixed_width(s: &str, width: usize) -> String {
    if s.len() >= width {
        s[..width].to_string()
    } else {
        format!("{s:<width$}")
    }
}

/// Colorize a single `git blame` line without syntax highlighting.
///
/// Returns the line with a colored gutter (hash + author + date) prepended.
/// Lines that cannot be parsed as blame output are returned unchanged.
///
/// `current_year` is used to compute commit age. Pass the actual calendar
/// year (e.g., 2026) for production use, or a fixed value in tests.
#[must_use]
pub fn colorize_blame_line(line: &str, current_year: u32) -> String {
    let Some(bl) = parse_blame_line(line) else {
        return line.to_string();
    };

    let color = if let Some(year) = year_from_date(&bl.date) {
        age_color(year, current_year)
    } else {
        COLOR_OLD
    };

    let hash_col = fixed_width(&bl.hash, HASH_WIDTH);
    let author_col = fixed_width(&bl.author, AUTHOR_WIDTH);
    let date_col = &bl.date;

    format!(
        "{color}{hash_col}  {author_col}  {date_col}{RESET}  {}",
        bl.code
    )
}

/// Colorize a `git blame` line and syntax-highlight the code column.
///
/// When the `syntax` feature is enabled and a `Highlighter` is supplied with a
/// detected syntax, the code portion of the line is highlighted using syntect.
/// A fresh `HighlightLines` state is created per call — blame lines are
/// syntactically independent (each has its own hash/code), so per-line state
/// is correct.  Lines that cannot be parsed as blame output are returned
/// unchanged.
///
/// `current_year` drives recency coloring.
#[cfg(feature = "syntax")]
#[must_use]
pub fn colorize_blame_line_syntax(
    line: &str,
    current_year: u32,
    highlighter: &crate::syntax::highlighting::Highlighter,
    filename: &str,
) -> String {
    let Some(bl) = parse_blame_line(line) else {
        return line.to_string();
    };

    let color = if let Some(year) = year_from_date(&bl.date) {
        age_color(year, current_year)
    } else {
        COLOR_OLD
    };

    let hash_col = fixed_width(&bl.hash, HASH_WIDTH);
    let author_col = fixed_width(&bl.author, AUTHOR_WIDTH);
    let date_col = &bl.date;

    // Highlight the code column with syntect.
    let highlighted_code = if let Some(syntax) = highlighter.detect_syntax(filename) {
        let mut hl = highlighter.highlight_lines(syntax);
        let code_nl = if bl.code.ends_with('\n') {
            bl.code.clone()
        } else {
            format!("{}\n", bl.code)
        };
        highlighter
            .highlight_line_easy(&code_nl, &mut hl)
            .map_or_else(|| bl.code.clone(), |s| s.trim_end_matches('\n').to_string())
    } else {
        bl.code.clone()
    };

    format!("{color}{hash_col}  {author_col}  {date_col}{RESET}  {highlighted_code}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURRENT_YEAR: u32 = 2026;

    #[test]
    fn test_colorize_blame_line_contains_hash_gutter() {
        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(
            result.contains("abcdef1 "),
            "expected hash in gutter: {result:?}"
        );
    }

    #[test]
    fn test_colorize_blame_line_contains_author_gutter() {
        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(
            result.contains("Alice"),
            "expected author in gutter: {result:?}"
        );
    }

    #[test]
    fn test_colorize_blame_line_contains_date_gutter() {
        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(
            result.contains("2024-03-15"),
            "expected date in gutter: {result:?}"
        );
    }

    #[test]
    fn test_colorize_blame_line_contains_code() {
        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(
            result.contains("fn main() {}"),
            "expected code in output: {result:?}"
        );
    }

    #[test]
    fn test_colorize_blame_line_contains_sgr_reset() {
        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(result.contains("\x1b[0m"), "expected SGR reset: {result:?}");
    }

    #[test]
    fn test_colorize_blame_line_plain_text_passthrough() {
        let line = "this is not a blame line";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert_eq!(result, line);
    }

    #[test]
    fn test_colorize_blame_line_hot_color_current_year() {
        // Commit from current year — should use hot color.
        let line = "abcdef1 (Alice  2026-03-15 10:00:00 +0000  1) code";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(
            result.contains(COLOR_HOT),
            "expected hot color for current-year commit: {result:?}"
        );
    }

    #[test]
    fn test_colorize_blame_line_old_color_ancient_commit() {
        // Commit from 2010 — should use old color.
        let line = "abcdef1 (Alice  2010-01-01 10:00:00 +0000  1) old code";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(
            result.contains(COLOR_OLD),
            "expected old color for ancient commit: {result:?}"
        );
    }

    #[test]
    fn test_colorize_blame_line_recent_color_last_year() {
        // Commit from one year ago — should use recent color.
        let line = "abcdef1 (Alice  2025-06-01 10:00:00 +0000  1) recent code";
        let result = colorize_blame_line(line, CURRENT_YEAR);
        assert!(
            result.contains(COLOR_RECENT),
            "expected recent color for commit 1 year ago: {result:?}"
        );
    }

    #[test]
    fn test_fixed_width_truncates_long_string() {
        assert_eq!(fixed_width("abcdefghij", 8), "abcdefgh");
    }

    #[test]
    fn test_fixed_width_pads_short_string() {
        let s = fixed_width("abc", 8);
        assert_eq!(s.len(), 8);
        assert!(s.starts_with("abc"));
    }

    #[test]
    fn test_age_color_zero_returns_hot() {
        assert_eq!(age_color(2026, 2026), COLOR_HOT);
    }

    #[test]
    fn test_age_color_two_years_returns_recent() {
        assert_eq!(age_color(2024, 2026), COLOR_RECENT);
    }

    #[test]
    fn test_age_color_four_years_returns_medium() {
        assert_eq!(age_color(2022, 2026), COLOR_MEDIUM);
    }

    #[test]
    fn test_age_color_ten_years_returns_old() {
        assert_eq!(age_color(2016, 2026), COLOR_OLD);
    }

    #[cfg(feature = "syntax")]
    #[test]
    fn test_colorize_blame_line_syntax_contains_sgr_for_rust() {
        use crate::syntax::highlighting::Highlighter;
        let hl_instance = Highlighter::new();

        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) fn main() {}";
        let result = colorize_blame_line_syntax(line, CURRENT_YEAR, &hl_instance, "main.rs");
        // Should contain SGR from gutter coloring at minimum.
        assert!(
            result.contains("\x1b["),
            "expected SGR sequences: {result:?}"
        );
        // Code should still appear in the output.
        assert!(
            result.contains("fn") || result.contains("main"),
            "expected code token: {result:?}"
        );
    }

    #[cfg(feature = "syntax")]
    #[test]
    fn test_colorize_blame_line_syntax_plain_passthrough() {
        use crate::syntax::highlighting::Highlighter;
        let hl_instance = Highlighter::new();

        let line = "not a blame line at all";
        let result = colorize_blame_line_syntax(line, CURRENT_YEAR, &hl_instance, "main.rs");
        assert_eq!(result, line);
    }

    #[cfg(feature = "syntax")]
    #[test]
    fn test_colorize_blame_line_syntax_unknown_extension_falls_back() {
        use crate::syntax::highlighting::Highlighter;
        let hl_instance = Highlighter::new();

        let line = "abcdef1 (Alice  2024-03-15 10:00:00 +0000  1) some code here";
        // Unknown file extension: should fall back to plain colorize_blame_line behavior.
        let result = colorize_blame_line_syntax(line, CURRENT_YEAR, &hl_instance, "file.xyz123");
        assert!(
            result.contains("some code here"),
            "expected code in output: {result:?}"
        );
        assert!(
            result.contains("2024-03-15"),
            "expected date in output: {result:?}"
        );
    }
}
