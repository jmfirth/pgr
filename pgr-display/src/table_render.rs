//! SQL table rendering with sticky headers, column-snap horizontal scroll,
//! and frozen first column.
//!
//! Detects table layout from ASCII box-drawing patterns (psql, mysql, sqlite3
//! output) and provides column boundary information for enhanced rendering.

use crate::ansi::strip_ansi;
use crate::unicode::str_display_width;

/// Parsed layout of a SQL table: column boundaries and header row count.
///
/// Built by scanning rule lines (`+---+---+`) to find column edges.
/// Used by the pager to enable sticky headers and column-snap scrolling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlTableLayout {
    /// Display-column positions where column boundaries occur (the `+` positions
    /// in a rule line). Includes position 0 and the rightmost `+`.
    /// Example: `+------+------+` yields `[0, 7, 14]`.
    pub column_boundaries: Vec<usize>,
    /// Number of header rows (everything before the first data row).
    /// For a typical SQL table with one rule line, header row, and separator:
    /// ```text
    /// +------+------+   <- rule (row 0)
    /// | col1 | col2 |   <- header (row 1)
    /// +------+------+   <- separator (row 2)
    /// | data | data |   <- first data row (row 3)
    /// ```
    /// `header_rows` would be 3 (rows 0, 1, 2 are frozen as header).
    pub header_rows: usize,
}

/// Parse a SQL table layout from the content lines.
///
/// Scans for rule lines (`+[-+]+`) to determine column boundaries and
/// the number of header rows. Returns `None` if no valid table layout
/// is detected (e.g., the content is not actually a SQL table).
#[must_use]
pub fn parse_table_layout(lines: &[&str]) -> Option<SqlTableLayout> {
    // Find the first rule line to extract column boundaries.
    let mut first_rule_idx: Option<usize> = None;
    let mut second_rule_idx: Option<usize> = None;
    let mut boundaries: Option<Vec<usize>> = None;

    for (i, line) in lines.iter().enumerate() {
        let clean = strip_ansi(line);
        if is_rule_line(&clean) {
            if first_rule_idx.is_none() {
                first_rule_idx = Some(i);
                boundaries = Some(extract_column_boundaries(&clean));
            } else if second_rule_idx.is_none() {
                second_rule_idx = Some(i);
                break;
            }
        }
    }

    let boundaries = boundaries?;
    if boundaries.len() < 2 {
        return None;
    }

    // Header rows: everything up to and including the second rule line
    // (which separates header from data). If there's no second rule line,
    // use the first rule line + 1 header row + 1 separator = 3 rows,
    // or just up to and including the first rule if only one exists.
    let header_rows = if let Some(second) = second_rule_idx {
        second + 1
    } else {
        // Only one rule line found — treat it as the top border,
        // and assume 1 header row follows it. Header = 2 rows.
        first_rule_idx.map_or(0, |first| first + 2)
    };

    Some(SqlTableLayout {
        column_boundaries: boundaries,
        header_rows,
    })
}

/// Find the nearest column boundary at or after `current_offset` for
/// snapping horizontal scroll to column edges.
///
/// Returns the display-column position to scroll to. If `current_offset`
/// is already past all boundaries, returns the last boundary.
#[must_use]
pub fn snap_to_next_column(layout: &SqlTableLayout, current_offset: usize) -> usize {
    for &boundary in &layout.column_boundaries {
        if boundary > current_offset {
            return boundary;
        }
    }
    // Past all boundaries — stay at the last one.
    layout
        .column_boundaries
        .last()
        .copied()
        .unwrap_or(current_offset)
}

/// Find the nearest column boundary at or before `current_offset` for
/// snapping horizontal scroll leftward.
///
/// Returns the display-column position to scroll to. If `current_offset`
/// is before all boundaries, returns 0.
#[must_use]
pub fn snap_to_prev_column(layout: &SqlTableLayout, current_offset: usize) -> usize {
    let mut prev = 0;
    for &boundary in &layout.column_boundaries {
        if boundary >= current_offset {
            return prev;
        }
        prev = boundary;
    }
    prev
}

/// Width (in display columns) of the first table column, including the
/// leading `|` and trailing `|` characters.
///
/// Used to determine how many columns to freeze on the left during
/// horizontal scroll. Returns 0 if the layout has fewer than 2 boundaries.
#[must_use]
pub fn first_column_width(layout: &SqlTableLayout) -> usize {
    if layout.column_boundaries.len() < 2 {
        return 0;
    }
    // The first column spans from boundary[0] to boundary[1] (inclusive
    // of the separator character at boundary[1]).
    layout.column_boundaries[1].saturating_sub(layout.column_boundaries[0]) + 1
}

/// Render a line with the first column frozen in place during horizontal scroll.
///
/// When `h_offset > 0` in SQL table mode, the first column stays visible
/// at the left edge while the remaining columns scroll. This creates a
/// "frozen column" effect similar to spreadsheet applications.
///
/// Returns the modified line with the frozen prefix prepended to the
/// horizontally-scrolled remainder. Returns the original line unchanged
/// if `h_offset` is 0 or the layout has insufficient column data.
#[must_use]
pub fn render_frozen_column(line: &str, layout: &SqlTableLayout, h_offset: usize) -> String {
    if h_offset == 0 || layout.column_boundaries.len() < 2 {
        return line.to_string();
    }

    let clean = strip_ansi(line);
    let freeze_width = first_column_width(layout);

    // Extract the frozen prefix (first column) from the original line.
    let frozen_prefix = take_display_columns(&clean, freeze_width);

    // Extract the portion after h_offset (the scrolled part).
    let scrolled = skip_display_columns(&clean, h_offset + freeze_width);

    if scrolled.is_empty() {
        frozen_prefix
    } else {
        format!("{frozen_prefix}{scrolled}")
    }
}

/// Check if a line is a SQL table horizontal rule: `+[-+]+`
///
/// Must start and end with `+`, containing only `-` and `+` characters,
/// with at least one `-`.
fn is_rule_line(line: &str) -> bool {
    let trimmed = line.trim_end();
    if trimmed.len() < 3 {
        return false;
    }
    let bytes = trimmed.as_bytes();
    if bytes[0] != b'+' || bytes[bytes.len() - 1] != b'+' {
        return false;
    }
    let has_dash = bytes.contains(&b'-');
    if !has_dash {
        return false;
    }
    bytes.iter().all(|&b| b == b'+' || b == b'-')
}

/// Extract column boundary positions from a rule line.
///
/// Each `+` character position (in display columns) is a boundary.
/// Example: `+------+------+` yields `[0, 7, 14]`.
fn extract_column_boundaries(rule_line: &str) -> Vec<usize> {
    let mut boundaries = Vec::new();
    let mut col: usize = 0;
    for ch in rule_line.chars() {
        if ch == '+' {
            boundaries.push(col);
        }
        col += str_display_width(&ch.to_string());
    }
    boundaries
}

/// Take the first `max_cols` display columns from a string.
fn take_display_columns(s: &str, max_cols: usize) -> String {
    let mut result = String::new();
    let mut col: usize = 0;
    for ch in s.chars() {
        let w = str_display_width(&ch.to_string());
        if col + w > max_cols {
            break;
        }
        result.push(ch);
        col += w;
    }
    result
}

/// Skip the first `skip_cols` display columns and return the rest.
fn skip_display_columns(s: &str, skip_cols: usize) -> String {
    let mut col: usize = 0;
    for (byte_idx, ch) in s.char_indices() {
        if col >= skip_cols {
            return s[byte_idx..].to_string();
        }
        col += str_display_width(&ch.to_string());
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── is_rule_line tests ────────────────────────────────────────────

    #[test]
    fn test_is_rule_line_basic_returns_true() {
        assert!(is_rule_line("+---+---+"));
    }

    #[test]
    fn test_is_rule_line_long_returns_true() {
        assert!(is_rule_line("+------+------+------+"));
    }

    #[test]
    fn test_is_rule_line_trailing_whitespace_returns_true() {
        assert!(is_rule_line("+---+---+  "));
    }

    #[test]
    fn test_is_rule_line_pipe_delimited_returns_false() {
        assert!(!is_rule_line("| col1 | col2 |"));
    }

    #[test]
    fn test_is_rule_line_too_short_returns_false() {
        assert!(!is_rule_line("++"));
    }

    #[test]
    fn test_is_rule_line_no_dashes_returns_false() {
        assert!(!is_rule_line("+++"));
    }

    #[test]
    fn test_is_rule_line_plain_text_returns_false() {
        assert!(!is_rule_line("hello world"));
    }

    // ── extract_column_boundaries tests ───────────────────────────────

    #[test]
    fn test_extract_boundaries_basic_two_columns() {
        let boundaries = extract_column_boundaries("+------+------+");
        assert_eq!(boundaries, vec![0, 7, 14]);
    }

    #[test]
    fn test_extract_boundaries_three_columns() {
        let boundaries = extract_column_boundaries("+---+---+---+");
        assert_eq!(boundaries, vec![0, 4, 8, 12]);
    }

    #[test]
    fn test_extract_boundaries_single_column() {
        let boundaries = extract_column_boundaries("+------+");
        assert_eq!(boundaries, vec![0, 7]);
    }

    // ── parse_table_layout tests ──────────────────────────────────────

    #[test]
    fn test_parse_layout_standard_mysql_table() {
        let lines = vec![
            "+------+------+",
            "| col1 | col2 |",
            "+------+------+",
            "| a    | b    |",
            "| c    | d    |",
            "+------+------+",
        ];
        let layout = parse_table_layout(&lines).unwrap();
        assert_eq!(layout.column_boundaries, vec![0, 7, 14]);
        assert_eq!(layout.header_rows, 3); // rule + header + rule
    }

    #[test]
    fn test_parse_layout_single_rule_line() {
        let lines = vec!["+------+------+", "| col1 | col2 |", "| a    | b    |"];
        let layout = parse_table_layout(&lines).unwrap();
        assert_eq!(layout.column_boundaries, vec![0, 7, 14]);
        assert_eq!(layout.header_rows, 2); // rule + header row
    }

    #[test]
    fn test_parse_layout_no_rule_lines_returns_none() {
        let lines = vec!["hello", "world"];
        assert!(parse_table_layout(&lines).is_none());
    }

    #[test]
    fn test_parse_layout_empty_returns_none() {
        let lines: Vec<&str> = vec![];
        assert!(parse_table_layout(&lines).is_none());
    }

    #[test]
    fn test_parse_layout_psql_style_three_columns() {
        let lines = vec![
            "+----+------+-----+",
            "| id | name | age |",
            "+----+------+-----+",
            "|  1 | foo  |  42 |",
            "+----+------+-----+",
        ];
        let layout = parse_table_layout(&lines).unwrap();
        assert_eq!(layout.column_boundaries, vec![0, 5, 12, 18]);
        assert_eq!(layout.header_rows, 3);
    }

    // ── snap_to_next_column tests ─────────────────────────────────────

    #[test]
    fn test_snap_next_from_zero_goes_to_first_inner_boundary() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        assert_eq!(snap_to_next_column(&layout, 0), 7);
    }

    #[test]
    fn test_snap_next_from_mid_column_goes_to_next_boundary() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14, 21],
            header_rows: 3,
        };
        assert_eq!(snap_to_next_column(&layout, 5), 7);
    }

    #[test]
    fn test_snap_next_from_boundary_goes_to_following() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        assert_eq!(snap_to_next_column(&layout, 7), 14);
    }

    #[test]
    fn test_snap_next_past_all_returns_last() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        assert_eq!(snap_to_next_column(&layout, 20), 14);
    }

    // ── snap_to_prev_column tests ─────────────────────────────────────

    #[test]
    fn test_snap_prev_from_end_goes_to_previous_boundary() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        assert_eq!(snap_to_prev_column(&layout, 14), 7);
    }

    #[test]
    fn test_snap_prev_from_mid_column_goes_to_start_of_column() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        assert_eq!(snap_to_prev_column(&layout, 10), 7);
    }

    #[test]
    fn test_snap_prev_from_first_boundary_returns_zero() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        assert_eq!(snap_to_prev_column(&layout, 7), 0);
    }

    #[test]
    fn test_snap_prev_from_zero_returns_zero() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        assert_eq!(snap_to_prev_column(&layout, 0), 0);
    }

    // ── first_column_width tests ──────────────────────────────────────

    #[test]
    fn test_first_column_width_basic() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        // First column: boundary[0]=0 to boundary[1]=7, width = 7 - 0 + 1 = 8
        assert_eq!(first_column_width(&layout), 8);
    }

    #[test]
    fn test_first_column_width_insufficient_boundaries() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0],
            header_rows: 1,
        };
        assert_eq!(first_column_width(&layout), 0);
    }

    // ── render_frozen_column tests ────────────────────────────────────

    #[test]
    fn test_render_frozen_no_offset_returns_original() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        let line = "| col1 | col2 |";
        assert_eq!(render_frozen_column(line, &layout, 0), line);
    }

    #[test]
    fn test_render_frozen_with_offset_preserves_first_column() {
        let layout = SqlTableLayout {
            column_boundaries: vec![0, 7, 14],
            header_rows: 3,
        };
        let line = "| col1 | col2 | col3 |";
        let result = render_frozen_column(line, &layout, 7);
        // First column width is 8 chars (| col1 |+space)
        // Should start with the frozen first column
        assert!(result.starts_with("| col1 |"));
    }

    // ── take_display_columns / skip_display_columns tests ─────────────

    #[test]
    fn test_take_display_columns_ascii() {
        assert_eq!(take_display_columns("hello world", 5), "hello");
    }

    #[test]
    fn test_take_display_columns_exact() {
        assert_eq!(take_display_columns("abc", 3), "abc");
    }

    #[test]
    fn test_take_display_columns_over_length() {
        assert_eq!(take_display_columns("hi", 10), "hi");
    }

    #[test]
    fn test_skip_display_columns_ascii() {
        assert_eq!(skip_display_columns("hello world", 6), "world");
    }

    #[test]
    fn test_skip_display_columns_past_end_returns_empty() {
        assert_eq!(skip_display_columns("hi", 10), "");
    }

    #[test]
    fn test_skip_display_columns_zero_returns_full() {
        assert_eq!(skip_display_columns("hello", 0), "hello");
    }
}
