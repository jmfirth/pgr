//! File information formatting for the `=` command.
//!
//! Produces the status line text matching GNU less's `=` output format.

use std::fmt::Write;

/// Format the file information string matching less's `=` command output.
///
/// The output matches less's format:
/// - For a named file: `filename lines X-Y/Z byte B/T P%`
/// - For a pipe: `(pipe) lines X-Y byte B`
/// - If the file list has multiple files: `filename (file N of M) lines X-Y/Z byte B/T P%`
///
/// Uses hardcoded format strings rather than the prompt mini-language.
#[allow(clippy::too_many_arguments)] // Mirrors the distinct fields of less's = output
#[must_use]
pub fn format_file_info(
    filename: Option<&str>,
    top_line: usize,
    bottom_line: usize,
    total_lines: Option<usize>,
    byte_offset: u64,
    total_bytes: u64,
    file_index: usize,
    file_count: usize,
    is_pipe: bool,
) -> String {
    let mut out = String::new();

    if is_pipe {
        out.push_str("(pipe)");
    } else {
        out.push_str(filename.unwrap_or("(standard input)"));
    }

    // Multi-file indicator
    if file_count > 1 {
        // Write cannot fail on String
        let _ = write!(out, " (file {} of {})", file_index + 1, file_count);
    }

    // Lines portion
    match total_lines {
        Some(total) => {
            let _ = write!(out, " lines {top_line}-{bottom_line}/{total}");
        }
        None => {
            let _ = write!(out, " lines {top_line}-{bottom_line}");
        }
    }

    // Byte portion
    if is_pipe {
        let _ = write!(out, " byte {byte_offset}");
    } else {
        let _ = write!(out, " byte {byte_offset}/{total_bytes}");

        // Percentage
        let pct = if total_bytes == 0 {
            0
        } else {
            byte_offset.saturating_mul(100) / total_bytes
        };
        let _ = write!(out, " {pct}%");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 5: format_file_info with a named file produces correct format
    #[test]
    fn test_format_file_info_named_file_produces_correct_format() {
        let result = format_file_info(Some("test.txt"), 1, 24, Some(500), 1234, 56789, 0, 1, false);
        assert_eq!(result, "test.txt lines 1-24/500 byte 1234/56789 2%");
    }

    // Test 6: format_file_info with a pipe produces pipe-specific format
    #[test]
    fn test_format_file_info_pipe_produces_pipe_format() {
        let result = format_file_info(None, 1, 24, None, 512, 0, 0, 1, true);
        assert_eq!(result, "(pipe) lines 1-24 byte 512");
    }

    // Test 7: format_file_info with multiple files includes "file N of M"
    #[test]
    fn test_format_file_info_multiple_files_includes_file_n_of_m() {
        let result = format_file_info(Some("test.txt"), 1, 24, Some(500), 1234, 56789, 0, 3, false);
        assert_eq!(
            result,
            "test.txt (file 1 of 3) lines 1-24/500 byte 1234/56789 2%"
        );
    }

    // Test 8: format_file_info with total lines known includes "/total"
    #[test]
    fn test_format_file_info_total_lines_known_includes_total() {
        let result = format_file_info(
            Some("data.log"),
            10,
            33,
            Some(1000),
            5000,
            100_000,
            0,
            1,
            false,
        );
        assert!(result.contains("lines 10-33/1000"));
        assert!(result.contains("byte 5000/100000"));
    }

    // Test 9: format_file_info with total lines unknown omits total
    #[test]
    fn test_format_file_info_total_lines_unknown_omits_total() {
        let result = format_file_info(Some("stream.log"), 1, 24, None, 0, 4096, 0, 1, false);
        assert!(result.contains("lines 1-24"));
        assert!(!result.contains("lines 1-24/"));
    }

    #[test]
    fn test_format_file_info_zero_total_bytes_shows_zero_percent() {
        let result = format_file_info(Some("empty.txt"), 0, 0, Some(0), 0, 0, 0, 1, false);
        assert!(result.contains("0%"));
    }

    #[test]
    fn test_format_file_info_no_filename_shows_standard_input() {
        let result = format_file_info(None, 1, 24, Some(100), 0, 5000, 0, 1, false);
        assert!(result.starts_with("(standard input)"));
    }
}
