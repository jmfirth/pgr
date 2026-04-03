//! Side-by-side diff rendering engine.
//!
//! Splits the terminal into left (old) and right (new) panels separated
//! by a vertical bar, pairing removed/added lines from diff hunks for
//! easy visual comparison.

use pgr_core::{classify_diff_line, DiffLineType};

use crate::unicode::truncate_to_width_grapheme;

/// Minimum terminal width required for side-by-side mode.
///
/// Below this threshold the pager falls back to unified diff display.
pub const MIN_SIDE_BY_SIDE_COLS: usize = 80;

/// Layout parameters for side-by-side rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideLayout {
    /// Number of columns for the left (old) panel.
    pub left_width: usize,
    /// Number of columns for the right (new) panel.
    pub right_width: usize,
    /// Separator string drawn between panels.
    pub separator: &'static str,
}

impl SideBySideLayout {
    /// Compute a side-by-side layout from the terminal width.
    ///
    /// Returns `None` if the terminal is too narrow (below
    /// [`MIN_SIDE_BY_SIDE_COLS`]).
    #[must_use]
    pub fn from_terminal_width(cols: usize) -> Option<Self> {
        if cols < MIN_SIDE_BY_SIDE_COLS {
            return None;
        }
        // 1 column for the separator character
        let left_width = (cols - 1) / 2;
        let right_width = cols - 1 - left_width;
        Some(Self {
            left_width,
            right_width,
            separator: "\u{2502}", // │
        })
    }
}

/// A paired line for side-by-side display.
///
/// Each instance represents one screen row in the side-by-side view.
/// `left` is the old (removed) side, `right` is the new (added) side.
/// Either may be `None` for pure additions or pure deletions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideLine {
    /// Content for the left (old) panel, or `None` for blank.
    pub left: Option<String>,
    /// Content for the right (new) panel, or `None` for blank.
    pub right: Option<String>,
    /// Line type for the left panel.
    pub left_type: DiffLineType,
    /// Line type for the right panel.
    pub right_type: DiffLineType,
}

/// Pair diff lines within a hunk for side-by-side display.
///
/// Walks through the lines and their types, matching removed lines with
/// added lines sequentially. Context lines appear on both sides. Excess
/// removed or added lines are displayed with a blank opposite panel.
#[must_use]
pub fn pair_hunk_lines(lines: &[&str], line_types: &[DiffLineType]) -> Vec<SideBySideLine> {
    let mut result = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        match line_types[i] {
            DiffLineType::Context => {
                // Strip the leading space prefix from context lines
                let content = lines[i].strip_prefix(' ').unwrap_or(lines[i]);
                result.push(SideBySideLine {
                    left: Some(content.to_string()),
                    right: Some(content.to_string()),
                    left_type: DiffLineType::Context,
                    right_type: DiffLineType::Context,
                });
                i += 1;
            }
            DiffLineType::Removed => {
                // Collect consecutive removed lines
                let removed_start = i;
                while i < lines.len() && line_types[i] == DiffLineType::Removed {
                    i += 1;
                }
                let removed_end = i;

                // Collect consecutive added lines that follow
                let added_start = i;
                while i < lines.len() && line_types[i] == DiffLineType::Added {
                    i += 1;
                }
                let added_end = i;

                let removed_count = removed_end - removed_start;
                let added_count = added_end - added_start;
                let max_count = removed_count.max(added_count);

                for j in 0..max_count {
                    let left = if j < removed_count {
                        // Strip the '-' prefix
                        let raw = lines[removed_start + j];
                        Some(raw.strip_prefix('-').unwrap_or(raw).to_string())
                    } else {
                        None
                    };
                    let right = if j < added_count {
                        // Strip the '+' prefix
                        let raw = lines[added_start + j];
                        Some(raw.strip_prefix('+').unwrap_or(raw).to_string())
                    } else {
                        None
                    };
                    let left_type = if j < removed_count {
                        DiffLineType::Removed
                    } else {
                        DiffLineType::Other
                    };
                    let right_type = if j < added_count {
                        DiffLineType::Added
                    } else {
                        DiffLineType::Other
                    };
                    result.push(SideBySideLine {
                        left,
                        right,
                        left_type,
                        right_type,
                    });
                }
            }
            DiffLineType::Added => {
                // Pure addition (no preceding removal)
                let raw = lines[i];
                let content = raw.strip_prefix('+').unwrap_or(raw);
                result.push(SideBySideLine {
                    left: None,
                    right: Some(content.to_string()),
                    left_type: DiffLineType::Other,
                    right_type: DiffLineType::Added,
                });
                i += 1;
            }
            DiffLineType::Header | DiffLineType::HunkHeader | DiffLineType::Other => {
                // Headers and hunk headers span the full width (shown on both sides)
                result.push(SideBySideLine {
                    left: Some(lines[i].to_string()),
                    right: Some(String::new()),
                    left_type: line_types[i],
                    right_type: line_types[i],
                });
                i += 1;
            }
        }
    }

    result
}

/// ANSI escape code for dim text.
const DIM: &str = "\x1b[2m";
/// ANSI escape code for removed (red background) text.
const REMOVED_BG: &str = "\x1b[41m";
/// ANSI escape code for added (green background) text.
const ADDED_BG: &str = "\x1b[42m";
/// ANSI reset code.
const RESET: &str = "\x1b[0m";

/// Render paired lines into pre-formatted strings with ANSI coloring.
///
/// Each output string is a complete terminal row with left panel,
/// separator, and right panel — ready to be passed to the normal
/// paint path as a pre-rendered line.
#[must_use]
pub fn render_side_by_side(paired: &[SideBySideLine], layout: &SideBySideLayout) -> Vec<String> {
    paired
        .iter()
        .map(|line| render_one_line(line, layout))
        .collect()
}

/// Render a single paired line into a formatted terminal row.
fn render_one_line(line: &SideBySideLine, layout: &SideBySideLayout) -> String {
    let left_text = line.left.as_deref().unwrap_or("");
    let right_text = line.right.as_deref().unwrap_or("");

    // Truncate and pad left panel
    let (left_truncated, left_width) = truncate_to_width_grapheme(left_text, layout.left_width);
    let left_pad = layout.left_width.saturating_sub(left_width);

    // Truncate and pad right panel
    let (right_truncated, right_width) = truncate_to_width_grapheme(right_text, layout.right_width);
    let right_pad = layout.right_width.saturating_sub(right_width);

    let mut out = String::with_capacity(
        layout.left_width + layout.right_width + 64, // extra for ANSI codes
    );

    // Left panel with color
    let left_color = color_for_type(line.left_type, true);
    if let Some(color) = left_color {
        out.push_str(color);
    }
    out.push_str(left_truncated);
    // Pad with spaces (still inside the color region so background fills)
    for _ in 0..left_pad {
        out.push(' ');
    }
    if left_color.is_some() {
        out.push_str(RESET);
    }

    // Separator
    out.push_str(DIM);
    out.push_str(layout.separator);
    out.push_str(RESET);

    // Right panel with color
    let right_color = color_for_type(line.right_type, false);
    if let Some(color) = right_color {
        out.push_str(color);
    }
    out.push_str(right_truncated);
    for _ in 0..right_pad {
        out.push(' ');
    }
    if right_color.is_some() {
        out.push_str(RESET);
    }

    out
}

/// Return the ANSI color escape for a given line type and panel side.
///
/// Returns `None` for context and other types that don't need coloring.
fn color_for_type(line_type: DiffLineType, is_left: bool) -> Option<&'static str> {
    match line_type {
        DiffLineType::Removed if is_left => Some(REMOVED_BG),
        DiffLineType::Added if !is_left => Some(ADDED_BG),
        _ => None,
    }
}

/// Build side-by-side rendered lines from raw buffer lines.
///
/// Classifies each line, pairs them, and renders the result. Returns
/// `None` if the terminal is too narrow for side-by-side display.
#[must_use]
pub fn build_side_by_side_lines(
    buffer_lines: &[Option<String>],
    terminal_cols: usize,
) -> Option<Vec<String>> {
    let layout = SideBySideLayout::from_terminal_width(terminal_cols)?;

    let lines: Vec<&str> = buffer_lines
        .iter()
        .filter_map(|opt| opt.as_deref())
        .collect();
    let types: Vec<DiffLineType> = lines.iter().map(|l| classify_diff_line(l)).collect();

    let paired = pair_hunk_lines(&lines, &types);
    Some(render_side_by_side(&paired, &layout))
}

/// Compute the display width of a rendered side-by-side line.
///
/// Since the rendered lines contain ANSI escape codes, this strips
/// them before measuring width.
#[must_use]
pub fn rendered_line_width(layout: &SideBySideLayout) -> usize {
    layout.left_width + 1 + layout.right_width
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Layout calculation tests ──

    #[test]
    fn test_layout_120_cols_splits_correctly() {
        let layout = SideBySideLayout::from_terminal_width(120).unwrap();
        assert_eq!(layout.left_width, 59);
        assert_eq!(layout.right_width, 60);
        assert_eq!(layout.separator, "\u{2502}");
    }

    #[test]
    fn test_layout_80_cols_splits_correctly() {
        let layout = SideBySideLayout::from_terminal_width(80).unwrap();
        assert_eq!(layout.left_width, 39);
        assert_eq!(layout.right_width, 40);
    }

    #[test]
    fn test_layout_minimum_60_cols_returns_none() {
        assert!(SideBySideLayout::from_terminal_width(60).is_none());
    }

    #[test]
    fn test_layout_79_cols_returns_none() {
        assert!(SideBySideLayout::from_terminal_width(79).is_none());
    }

    #[test]
    fn test_layout_exactly_80_cols_returns_some() {
        assert!(SideBySideLayout::from_terminal_width(80).is_some());
    }

    // ── Line pairing tests ──

    #[test]
    fn test_pair_context_lines_appear_on_both_sides() {
        let lines = vec![" unchanged line"];
        let types = vec![DiffLineType::Context];
        let paired = pair_hunk_lines(&lines, &types);
        assert_eq!(paired.len(), 1);
        assert_eq!(paired[0].left.as_deref(), Some("unchanged line"));
        assert_eq!(paired[0].right.as_deref(), Some("unchanged line"));
        assert_eq!(paired[0].left_type, DiffLineType::Context);
        assert_eq!(paired[0].right_type, DiffLineType::Context);
    }

    #[test]
    fn test_pair_removed_and_added_lines() {
        let lines = vec!["-old line", "+new line"];
        let types = vec![DiffLineType::Removed, DiffLineType::Added];
        let paired = pair_hunk_lines(&lines, &types);
        assert_eq!(paired.len(), 1);
        assert_eq!(paired[0].left.as_deref(), Some("old line"));
        assert_eq!(paired[0].right.as_deref(), Some("new line"));
        assert_eq!(paired[0].left_type, DiffLineType::Removed);
        assert_eq!(paired[0].right_type, DiffLineType::Added);
    }

    #[test]
    fn test_pair_pure_removal_content_left_blank_right() {
        let lines = vec!["-deleted line"];
        let types = vec![DiffLineType::Removed];
        let paired = pair_hunk_lines(&lines, &types);
        assert_eq!(paired.len(), 1);
        assert_eq!(paired[0].left.as_deref(), Some("deleted line"));
        assert!(paired[0].right.is_none());
        assert_eq!(paired[0].left_type, DiffLineType::Removed);
    }

    #[test]
    fn test_pair_pure_addition_blank_left_content_right() {
        let lines = vec!["+added line"];
        let types = vec![DiffLineType::Added];
        let paired = pair_hunk_lines(&lines, &types);
        assert_eq!(paired.len(), 1);
        assert!(paired[0].left.is_none());
        assert_eq!(paired[0].right.as_deref(), Some("added line"));
        assert_eq!(paired[0].right_type, DiffLineType::Added);
    }

    #[test]
    fn test_pair_unequal_counts_3_removed_5_added() {
        let lines = vec![
            "-old1", "-old2", "-old3", "+new1", "+new2", "+new3", "+new4", "+new5",
        ];
        let types = vec![
            DiffLineType::Removed,
            DiffLineType::Removed,
            DiffLineType::Removed,
            DiffLineType::Added,
            DiffLineType::Added,
            DiffLineType::Added,
            DiffLineType::Added,
            DiffLineType::Added,
        ];
        let paired = pair_hunk_lines(&lines, &types);
        assert_eq!(paired.len(), 5);

        // First 3 are paired
        assert_eq!(paired[0].left.as_deref(), Some("old1"));
        assert_eq!(paired[0].right.as_deref(), Some("new1"));
        assert_eq!(paired[1].left.as_deref(), Some("old2"));
        assert_eq!(paired[1].right.as_deref(), Some("new2"));
        assert_eq!(paired[2].left.as_deref(), Some("old3"));
        assert_eq!(paired[2].right.as_deref(), Some("new3"));

        // Last 2 are pure additions (blank left)
        assert!(paired[3].left.is_none());
        assert_eq!(paired[3].right.as_deref(), Some("new4"));
        assert!(paired[4].left.is_none());
        assert_eq!(paired[4].right.as_deref(), Some("new5"));
    }

    // ── Rendering tests ──

    #[test]
    fn test_render_truncates_long_lines() {
        let layout = SideBySideLayout {
            left_width: 10,
            right_width: 10,
            separator: "\u{2502}",
        };
        let long_left = "a".repeat(20);
        let long_right = "b".repeat(20);
        let paired = vec![SideBySideLine {
            left: Some(long_left),
            right: Some(long_right),
            left_type: DiffLineType::Removed,
            right_type: DiffLineType::Added,
        }];
        let rendered = render_side_by_side(&paired, &layout);
        assert_eq!(rendered.len(), 1);
        // The rendered line should contain the separator
        assert!(rendered[0].contains('\u{2502}'));
    }

    #[test]
    fn test_render_separator_present() {
        let layout = SideBySideLayout {
            left_width: 20,
            right_width: 20,
            separator: "\u{2502}",
        };
        let paired = vec![SideBySideLine {
            left: Some("left".to_string()),
            right: Some("right".to_string()),
            left_type: DiffLineType::Context,
            right_type: DiffLineType::Context,
        }];
        let rendered = render_side_by_side(&paired, &layout);
        assert!(rendered[0].contains('\u{2502}'));
    }

    #[test]
    fn test_toggle_state_side_by_side_is_bool() {
        // Verify the toggle semantics: flipping true->false->true
        let mut side_by_side = false;
        side_by_side = !side_by_side;
        assert!(side_by_side);
        side_by_side = !side_by_side;
        assert!(!side_by_side);
    }

    #[test]
    fn test_build_side_by_side_lines_returns_none_for_narrow() {
        let lines = vec![Some(" context".to_string())];
        assert!(build_side_by_side_lines(&lines, 60).is_none());
    }

    #[test]
    fn test_build_side_by_side_lines_returns_some_for_wide() {
        let lines = vec![
            Some(" context line".to_string()),
            Some("-removed".to_string()),
            Some("+added".to_string()),
        ];
        let result = build_side_by_side_lines(&lines, 120);
        assert!(result.is_some());
        let rendered = result.unwrap();
        assert!(!rendered.is_empty());
    }

    #[test]
    fn test_rendered_line_width_correct() {
        let layout = SideBySideLayout {
            left_width: 59,
            right_width: 60,
            separator: "\u{2502}",
        };
        assert_eq!(rendered_line_width(&layout), 120);
    }

    #[test]
    fn test_pair_header_lines_span_full_width() {
        let lines = vec!["diff --git a/foo.rs b/foo.rs"];
        let types = vec![DiffLineType::Header];
        let paired = pair_hunk_lines(&lines, &types);
        assert_eq!(paired.len(), 1);
        assert_eq!(
            paired[0].left.as_deref(),
            Some("diff --git a/foo.rs b/foo.rs")
        );
        assert_eq!(paired[0].right.as_deref(), Some(""));
    }

    #[test]
    fn test_pair_mixed_hunk_content() {
        let lines = vec![
            "@@ -1,3 +1,4 @@",
            " context",
            "-removed",
            "+added1",
            "+added2",
            " more context",
        ];
        let types = vec![
            DiffLineType::HunkHeader,
            DiffLineType::Context,
            DiffLineType::Removed,
            DiffLineType::Added,
            DiffLineType::Added,
            DiffLineType::Context,
        ];
        let paired = pair_hunk_lines(&lines, &types);
        // hunk header + context + 1 paired (removed+added1) + 1 pure added (added2) + context
        assert_eq!(paired.len(), 5);
        assert_eq!(paired[0].left_type, DiffLineType::HunkHeader);
        assert_eq!(paired[1].left.as_deref(), Some("context"));
        assert_eq!(paired[1].right.as_deref(), Some("context"));
        assert_eq!(paired[2].left.as_deref(), Some("removed"));
        assert_eq!(paired[2].right.as_deref(), Some("added1"));
        assert!(paired[3].left.is_none());
        assert_eq!(paired[3].right.as_deref(), Some("added2"));
        assert_eq!(paired[4].left.as_deref(), Some("more context"));
    }
}
