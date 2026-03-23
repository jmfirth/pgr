//! Line number formatting for `-N` display mode.
//!
//! Computes line number column width and formats individual line numbers
//! for display in the left margin.

use std::fmt::Write;

/// Minimum width of the line number column (matches `less` default).
///
/// GNU less uses `LINENUM_WIDTH = 7` for the number field, then appends a
/// trailing space separator, totalling 8 visible columns. Our width
/// constant includes the separator, hence 8.
const MIN_LINE_NUMBER_WIDTH: usize = 8;

/// Calculate the width of the line number column.
///
/// Returns the number of columns needed for line numbers, including
/// the trailing space separator. The width is the larger of
/// `digits(total_lines) + 1` and [`MIN_LINE_NUMBER_WIDTH`].
#[must_use]
pub fn line_number_width(total_lines: usize) -> usize {
    let digits = if total_lines == 0 {
        1
    } else {
        digit_count(total_lines)
    };
    (digits + 1).max(MIN_LINE_NUMBER_WIDTH)
}

/// Calculate the width of the line number column with a custom minimum.
///
/// Like [`line_number_width`] but uses `min_width` instead of the default
/// minimum of 7. `min_width` is clamped to the range 1..=30.
#[must_use]
pub fn line_number_width_custom(total_lines: usize, min_width: usize) -> usize {
    let clamped_min = min_width.clamp(1, 30);
    let digits = if total_lines == 0 {
        1
    } else {
        digit_count(total_lines)
    };
    (digits + 1).max(clamped_min)
}

/// Format a line number for display.
///
/// Right-aligned within `width` columns, followed by a space.
/// Line numbers are 1-based: the caller is responsible for converting
/// 0-based buffer indices before calling.
#[must_use]
pub fn format_line_number(line_number: usize, width: usize) -> String {
    // width includes the trailing space, so the number occupies width-1 chars
    let num_width = width.saturating_sub(1);
    let mut result = String::with_capacity(width);
    // Write can't fail on a String, but we avoid unwrap in library code
    // by using write! which returns fmt::Error. We handle it gracefully.
    let _ = write!(result, "{line_number:>num_width$} ");
    result
}

/// Format a line number with the configured color.
///
/// Like [`format_line_number`] but wraps the number in the provided SGR
/// sequence when `number_sgr` is `Some`. The reset sequence follows
/// the number (before the trailing space) to prevent color bleed.
/// When `number_sgr` is `None`, behaves identically to [`format_line_number`].
#[must_use]
pub fn format_line_number_colored(
    line_number: usize,
    width: usize,
    number_sgr: Option<&str>,
) -> String {
    match number_sgr {
        Some(sgr) if !sgr.is_empty() => {
            let num_width = width.saturating_sub(1);
            let mut result = String::with_capacity(width + sgr.len() + 4);
            result.push_str(sgr);
            let _ = write!(result, "{line_number:>num_width$}");
            result.push_str("\x1b[0m");
            result.push(' ');
            result
        }
        _ => format_line_number(line_number, width),
    }
}

/// Count the number of decimal digits in a positive integer.
fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    let mut count = 0;
    let mut value = n;
    while value > 0 {
        count += 1;
        value /= 10;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_number_width_99_lines_returns_minimum() {
        // 99 lines = 2 digits + 1 space = 3, but min is 8
        assert_eq!(line_number_width(99), MIN_LINE_NUMBER_WIDTH);
    }

    #[test]
    fn test_line_number_width_1000_lines_returns_5_clamped_to_min() {
        // 1000 lines = 4 digits + 1 space = 5, but min is 8
        assert_eq!(line_number_width(1000), MIN_LINE_NUMBER_WIDTH);
    }

    #[test]
    fn test_line_number_width_large_file_exceeds_minimum() {
        // 10_000_000 lines = 8 digits + 1 space = 9 > 8
        assert_eq!(line_number_width(10_000_000), 9);
    }

    #[test]
    fn test_line_number_width_zero_lines() {
        // 0 lines = 1 digit + 1 space = 2, but min is 8
        assert_eq!(line_number_width(0), MIN_LINE_NUMBER_WIDTH);
    }

    #[test]
    fn test_line_number_width_custom_small_min() {
        // 99 lines with min_width 3 -> digits(99)+1 = 3 >= 3
        assert_eq!(line_number_width_custom(99, 3), 3);
    }

    #[test]
    fn test_line_number_width_custom_clamped_to_one() {
        // min_width 0 gets clamped to 1
        assert_eq!(line_number_width_custom(99, 0), 3);
    }

    #[test]
    fn test_line_number_width_custom_clamped_to_thirty() {
        // min_width 50 gets clamped to 30
        assert_eq!(line_number_width_custom(99, 50), 30);
    }

    #[test]
    fn test_format_line_number_right_aligned() {
        let formatted = format_line_number(42, 7);
        // width=7, num_width=6: "    42 " (right-aligned in 6 chars + trailing space)
        assert_eq!(formatted, "    42 ");
        assert_eq!(formatted.len(), 7);
    }

    #[test]
    fn test_format_line_number_single_digit() {
        let formatted = format_line_number(1, 7);
        assert_eq!(formatted, "     1 ");
        assert_eq!(formatted.len(), 7);
    }

    #[test]
    fn test_format_line_number_fills_width() {
        let formatted = format_line_number(999_999, 7);
        assert_eq!(formatted, "999999 ");
        assert_eq!(formatted.len(), 7);
    }

    #[test]
    fn test_format_line_number_exceeds_width_still_formats() {
        // If the number exceeds the allotted space, it overflows (no truncation)
        let formatted = format_line_number(12_345_678, 7);
        assert_eq!(formatted, "12345678 ");
    }

    #[test]
    fn test_format_line_number_width_one() {
        // width=1: num_width=0, but the number still renders
        let formatted = format_line_number(5, 1);
        assert_eq!(formatted, "5 ");
    }

    #[test]
    fn test_line_number_reduces_content_width() {
        // Simulates what paint_screen does: total_cols - line_number_width
        let total_cols = 80;
        let ln_width = line_number_width(500);
        let content_width = total_cols - ln_width;
        assert_eq!(ln_width, MIN_LINE_NUMBER_WIDTH);
        assert_eq!(content_width, 72);
    }

    #[test]
    fn test_digit_count_values() {
        assert_eq!(digit_count(0), 1);
        assert_eq!(digit_count(1), 1);
        assert_eq!(digit_count(9), 1);
        assert_eq!(digit_count(10), 2);
        assert_eq!(digit_count(99), 2);
        assert_eq!(digit_count(100), 3);
        assert_eq!(digit_count(999), 3);
        assert_eq!(digit_count(1000), 4);
    }

    // --- Colored line number tests ---

    #[test]
    fn test_format_line_number_colored_with_sgr_wraps_number() {
        let sgr = "\x1b[1;36m"; // bold cyan
        let formatted = format_line_number_colored(42, 7, Some(sgr));
        assert_eq!(formatted, "\x1b[1;36m    42\x1b[0m ");
    }

    #[test]
    fn test_format_line_number_colored_none_produces_plain() {
        let colored = format_line_number_colored(42, 7, None);
        let plain = format_line_number(42, 7);
        assert_eq!(colored, plain);
    }

    #[test]
    fn test_format_line_number_colored_empty_sgr_produces_plain() {
        let colored = format_line_number_colored(42, 7, Some(""));
        let plain = format_line_number(42, 7);
        assert_eq!(colored, plain);
    }

    #[test]
    fn test_format_line_number_colored_reset_before_trailing_space() {
        let sgr = "\x1b[32m"; // green
        let formatted = format_line_number_colored(1, 7, Some(sgr));
        // Reset should come before the trailing space
        assert!(formatted.ends_with("\x1b[0m "));
    }
}
