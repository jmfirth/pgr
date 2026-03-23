//! Squeeze blank lines for `-s` display mode.
//!
//! When squeeze mode is active, consecutive blank lines are collapsed
//! so that only the first blank line in each run is displayed.

/// Check if a line is blank (empty or whitespace-only).
#[must_use]
pub fn is_blank_line(line: &str) -> bool {
    line.trim().is_empty()
}

/// Determine which lines should be visible after squeezing blank lines.
///
/// Takes a starting line index and walks forward through the document,
/// collecting up to `max_lines` actual line numbers to display. Consecutive
/// blank lines are collapsed so only the first in each run appears.
///
/// `get_line` provides the content of a given line index, returning `None`
/// if the index is past the end of the document.
#[must_use]
pub fn squeeze_visible_lines<F>(
    start_line: usize,
    max_lines: usize,
    total_lines: usize,
    get_line: F,
) -> Vec<usize>
where
    F: Fn(usize) -> Option<String>,
{
    if max_lines == 0 || total_lines == 0 {
        return Vec::new();
    }

    let mut result = Vec::with_capacity(max_lines);
    let mut line_idx = start_line;
    let mut prev_was_blank = false;

    // Check if the line immediately before start_line was blank, so we know
    // if we're already in a blank run.
    if start_line > 0 {
        if let Some(prev_line) = get_line(start_line - 1) {
            prev_was_blank = is_blank_line(&prev_line);
        }
    }

    while result.len() < max_lines && line_idx < total_lines {
        let Some(line_content) = get_line(line_idx) else {
            break;
        };

        let blank = is_blank_line(&line_content);

        if blank && prev_was_blank {
            // Skip this blank line (it's a continuation of a blank run)
            line_idx += 1;
            continue;
        }

        result.push(line_idx);
        prev_was_blank = blank;
        line_idx += 1;
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_blank_line_empty_string_returns_true() {
        assert!(is_blank_line(""));
    }

    #[test]
    fn test_is_blank_line_whitespace_only_returns_true() {
        assert!(is_blank_line("   "));
        assert!(is_blank_line("\t"));
        assert!(is_blank_line("  \t  "));
    }

    #[test]
    fn test_is_blank_line_content_returns_false() {
        assert!(!is_blank_line("hello"));
        assert!(!is_blank_line("  hello  "));
        assert!(!is_blank_line("\thello"));
    }

    #[test]
    fn test_squeeze_visible_lines_no_blanks_returns_all() {
        let lines = vec![
            "line 1".to_string(),
            "line 2".to_string(),
            "line 3".to_string(),
            "line 4".to_string(),
        ];
        let result = squeeze_visible_lines(0, 4, lines.len(), |i| lines.get(i).cloned());
        assert_eq!(result, vec![0, 1, 2, 3]);
    }

    #[test]
    fn test_squeeze_visible_lines_collapses_three_consecutive_blanks() {
        let lines = vec![
            "content".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "more content".to_string(),
        ];
        let result = squeeze_visible_lines(0, 5, lines.len(), |i| lines.get(i).cloned());
        // Should show: line 0 (content), line 1 (first blank), line 4 (more content)
        assert_eq!(result, vec![0, 1, 4]);
    }

    #[test]
    fn test_squeeze_visible_lines_preserves_single_blank_between_content() {
        let lines = vec![
            "line 1".to_string(),
            "".to_string(),
            "line 3".to_string(),
            "".to_string(),
            "line 5".to_string(),
        ];
        let result = squeeze_visible_lines(0, 5, lines.len(), |i| lines.get(i).cloned());
        assert_eq!(result, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_squeeze_visible_lines_leading_blanks_squeezed() {
        let lines = vec![
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "content".to_string(),
        ];
        let result = squeeze_visible_lines(0, 4, lines.len(), |i| lines.get(i).cloned());
        // First blank at line 0 is kept, lines 1 and 2 are squeezed
        assert_eq!(result, vec![0, 3]);
    }

    #[test]
    fn test_squeeze_visible_lines_max_lines_limits_output() {
        let lines = vec![
            "a".to_string(),
            "b".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        let result = squeeze_visible_lines(0, 2, lines.len(), |i| lines.get(i).cloned());
        assert_eq!(result, vec![0, 1]);
    }

    #[test]
    fn test_squeeze_visible_lines_start_mid_document() {
        let lines = vec![
            "a".to_string(),
            "".to_string(),
            "".to_string(),
            "b".to_string(),
        ];
        // Start at line 1 (first blank). prev line (0) is not blank, so line 1 is kept.
        // Line 2 is consecutive blank, skipped.
        let result = squeeze_visible_lines(1, 4, lines.len(), |i| lines.get(i).cloned());
        assert_eq!(result, vec![1, 3]);
    }

    #[test]
    fn test_squeeze_visible_lines_start_in_middle_of_blank_run() {
        let lines = vec![
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "content".to_string(),
        ];
        // Start at line 2. Prev line (1) is blank, so line 2 (also blank) should be skipped.
        let result = squeeze_visible_lines(2, 4, lines.len(), |i| lines.get(i).cloned());
        assert_eq!(result, vec![3]);
    }

    #[test]
    fn test_squeeze_visible_lines_empty_document() {
        let result = squeeze_visible_lines(0, 10, 0, |_: usize| -> Option<String> { None });
        assert!(result.is_empty());
    }

    #[test]
    fn test_squeeze_visible_lines_zero_max_lines() {
        let lines = vec!["content".to_string()];
        let result = squeeze_visible_lines(0, 0, lines.len(), |i| lines.get(i).cloned());
        assert!(result.is_empty());
    }

    #[test]
    fn test_squeeze_visible_lines_whitespace_blanks_squeezed() {
        let lines = vec![
            "content".to_string(),
            "   ".to_string(),
            "\t".to_string(),
            "  \t  ".to_string(),
            "more".to_string(),
        ];
        let result = squeeze_visible_lines(0, 5, lines.len(), |i| lines.get(i).cloned());
        // All three whitespace-only lines form a blank run, only first kept
        assert_eq!(result, vec![0, 1, 4]);
    }

    #[test]
    fn test_squeeze_visible_lines_scrolling_skips_squeezed() {
        // Simulates scrolling: start_line jumps into the document
        let lines = vec![
            "a".to_string(),
            "b".to_string(),
            "".to_string(),
            "".to_string(),
            "".to_string(),
            "c".to_string(),
            "d".to_string(),
        ];
        // Start at line 2 (first blank). Prev (line 1) is not blank.
        let result = squeeze_visible_lines(2, 3, lines.len(), |i| lines.get(i).cloned());
        // Line 2 = first blank (kept), lines 3,4 squeezed, line 5 = "c", line 6 = "d"
        assert_eq!(result, vec![2, 5, 6]);
    }
}
