//! Git gutter: show +/-/~ markers in the left margin for uncommitted changes.
//!
//! Runs `git diff --no-color -U0 HEAD -- <path>` and parses the unified diff
//! hunk headers to determine which lines are added, removed, or modified.
//! No external git crate is used — only `std::process::Command`.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// The type of change for a single line in the git gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterMark {
    /// Line was added (exists in working copy but not in HEAD).
    Added,
    /// Line immediately follows a deletion (line exists in HEAD but was removed).
    Removed,
    /// Line was modified (both added and removed at the same position).
    Modified,
}

impl GutterMark {
    /// ANSI escape sequence to colorize the gutter mark.
    ///
    /// Green for added, red for removed, yellow for modified.
    #[must_use]
    pub fn ansi_color(self) -> &'static str {
        match self {
            Self::Added => "\x1b[32m",
            Self::Removed => "\x1b[31m",
            Self::Modified => "\x1b[33m",
        }
    }

    /// The single character displayed in the gutter column.
    #[must_use]
    pub fn symbol(self) -> char {
        match self {
            Self::Added => '+',
            Self::Removed => '-',
            Self::Modified => '~',
        }
    }
}

/// Cached git gutter state for a single file.
#[derive(Debug, Clone)]
pub struct GutterState {
    /// Map from 1-based line number to the gutter mark for that line.
    marks: HashMap<usize, GutterMark>,
}

impl GutterState {
    /// Create a new empty gutter state (no marks).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            marks: HashMap::new(),
        }
    }

    /// Create gutter state by running `git diff` against the given file path.
    ///
    /// Returns `None` if the file is not in a git repository, git is not
    /// available, or the diff command fails for any reason.
    #[must_use]
    pub fn from_file(path: &Path) -> Option<Self> {
        let output = Command::new("git")
            .args(["diff", "--no-color", "-U0", "HEAD", "--"])
            .arg(path)
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Some(Self::from_diff_output(&stdout))
    }

    /// Parse unified diff output (with `-U0`) into gutter marks.
    ///
    /// Hunk headers have the form `@@ -old_start[,old_count] +new_start[,new_count] @@`.
    /// With `-U0`, there is no context, so each hunk represents exactly one change region.
    #[must_use]
    pub fn from_diff_output(diff: &str) -> Self {
        let mut marks = HashMap::new();

        for line in diff.lines() {
            if let Some(hunk) = line.strip_prefix("@@ ") {
                if let Some((old_count, new_start, new_count)) = parse_hunk_header(hunk) {
                    if old_count == 0 && new_count > 0 {
                        // Pure addition: mark all new lines as Added.
                        for i in 0..new_count {
                            marks.insert(new_start + i, GutterMark::Added);
                        }
                    } else if old_count > 0 && new_count == 0 {
                        // Pure deletion: mark the line after the deletion point as Removed.
                        // new_start points to the line after which the deletion occurred.
                        marks.entry(new_start).or_insert(GutterMark::Removed);
                    } else if old_count > 0 && new_count > 0 {
                        // Modification: old lines replaced by new lines.
                        // Mark the overlapping region as Modified, any extra as Added.
                        let modified_count = old_count.min(new_count);
                        for i in 0..modified_count {
                            marks.insert(new_start + i, GutterMark::Modified);
                        }
                        for i in modified_count..new_count {
                            marks.insert(new_start + i, GutterMark::Added);
                        }
                    }
                }
            }
        }

        Self { marks }
    }

    /// Look up the gutter mark for a 1-based line number.
    #[must_use]
    pub fn mark_for_line(&self, line_number: usize) -> Option<GutterMark> {
        self.marks.get(&line_number).copied()
    }

    /// Whether this gutter state has any marks at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }

    /// The total number of marked lines.
    #[must_use]
    pub fn mark_count(&self) -> usize {
        self.marks.len()
    }
}

/// Parse a hunk header like `-old_start[,old_count] +new_start[,new_count] @@`.
///
/// Returns `(old_count, new_start, new_count)` on success.
fn parse_hunk_header(header: &str) -> Option<(usize, usize, usize)> {
    // Format: "-old_start[,old_count] +new_start[,new_count] @@[ optional section heading]"
    let parts: Vec<&str> = header.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }

    let old_part = parts[0].strip_prefix('-')?;
    let new_part = parts[1].strip_prefix('+')?;

    let old_count = parse_range_count(old_part);
    let (new_start, new_count) = parse_range(new_part)?;

    Some((old_count, new_start, new_count))
}

/// Parse a range like `start,count` or just `start` (count defaults to 1).
///
/// Returns `(start, count)`.
fn parse_range(range: &str) -> Option<(usize, usize)> {
    if let Some((start_s, count_s)) = range.split_once(',') {
        let start = start_s.parse::<usize>().ok()?;
        let count = count_s.parse::<usize>().ok()?;
        Some((start, count))
    } else {
        let start = range.parse::<usize>().ok()?;
        Some((start, 1))
    }
}

/// Parse just the count from a range like `start,count` or `start` (count defaults to 1).
fn parse_range_count(range: &str) -> usize {
    if let Some((_, count_s)) = range.split_once(',') {
        count_s.parse::<usize>().unwrap_or(1)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gutter_mark_symbol_added() {
        assert_eq!(GutterMark::Added.symbol(), '+');
    }

    #[test]
    fn test_gutter_mark_symbol_removed() {
        assert_eq!(GutterMark::Removed.symbol(), '-');
    }

    #[test]
    fn test_gutter_mark_symbol_modified() {
        assert_eq!(GutterMark::Modified.symbol(), '~');
    }

    #[test]
    fn test_gutter_mark_ansi_color_added_is_green() {
        assert_eq!(GutterMark::Added.ansi_color(), "\x1b[32m");
    }

    #[test]
    fn test_gutter_mark_ansi_color_removed_is_red() {
        assert_eq!(GutterMark::Removed.ansi_color(), "\x1b[31m");
    }

    #[test]
    fn test_gutter_mark_ansi_color_modified_is_yellow() {
        assert_eq!(GutterMark::Modified.ansi_color(), "\x1b[33m");
    }

    #[test]
    fn test_gutter_state_empty_has_no_marks() {
        let state = GutterState::empty();
        assert!(state.is_empty());
        assert_eq!(state.mark_count(), 0);
        assert_eq!(state.mark_for_line(1), None);
    }

    #[test]
    fn test_parse_hunk_header_simple_addition() {
        // @@ -10,0 +11,3 @@ => 0 old lines removed, 3 new lines added starting at line 11
        let result = parse_hunk_header("-10,0 +11,3 @@");
        assert_eq!(result, Some((0, 11, 3)));
    }

    #[test]
    fn test_parse_hunk_header_simple_deletion() {
        // @@ -5,2 +4,0 @@ => 2 old lines removed, 0 new lines at position 4
        let result = parse_hunk_header("-5,2 +4,0 @@");
        assert_eq!(result, Some((2, 4, 0)));
    }

    #[test]
    fn test_parse_hunk_header_modification() {
        // @@ -3,2 +3,4 @@ => 2 old lines replaced by 4 new lines
        let result = parse_hunk_header("-3,2 +3,4 @@");
        assert_eq!(result, Some((2, 3, 4)));
    }

    #[test]
    fn test_parse_hunk_header_single_line_no_count() {
        // @@ -1 +1 @@ => single line change (count defaults to 1)
        let result = parse_hunk_header("-1 +1 @@");
        assert_eq!(result, Some((1, 1, 1)));
    }

    #[test]
    fn test_parse_hunk_header_with_section_heading() {
        let result = parse_hunk_header("-10,0 +11,2 @@ fn some_function()");
        assert_eq!(result, Some((0, 11, 2)));
    }

    #[test]
    fn test_from_diff_output_pure_addition() {
        let diff = "\
diff --git a/file.rs b/file.rs
index abc..def 100644
--- a/file.rs
+++ b/file.rs
@@ -10,0 +11,3 @@
+line1
+line2
+line3
";
        let state = GutterState::from_diff_output(diff);
        assert_eq!(state.mark_for_line(11), Some(GutterMark::Added));
        assert_eq!(state.mark_for_line(12), Some(GutterMark::Added));
        assert_eq!(state.mark_for_line(13), Some(GutterMark::Added));
        assert_eq!(state.mark_for_line(10), None);
        assert_eq!(state.mark_for_line(14), None);
    }

    #[test]
    fn test_from_diff_output_pure_deletion() {
        let diff = "\
diff --git a/file.rs b/file.rs
@@ -5,2 +4,0 @@
-old line 1
-old line 2
";
        let state = GutterState::from_diff_output(diff);
        // Deletion marker placed at the line after the deletion point.
        assert_eq!(state.mark_for_line(4), Some(GutterMark::Removed));
        assert_eq!(state.mark_for_line(5), None);
    }

    #[test]
    fn test_from_diff_output_modification() {
        let diff = "\
diff --git a/file.rs b/file.rs
@@ -3,2 +3,2 @@
-old1
-old2
+new1
+new2
";
        let state = GutterState::from_diff_output(diff);
        assert_eq!(state.mark_for_line(3), Some(GutterMark::Modified));
        assert_eq!(state.mark_for_line(4), Some(GutterMark::Modified));
    }

    #[test]
    fn test_from_diff_output_modification_with_extra_adds() {
        let diff = "\
diff --git a/file.rs b/file.rs
@@ -3,1 +3,3 @@
-old
+new1
+new2
+new3
";
        let state = GutterState::from_diff_output(diff);
        assert_eq!(state.mark_for_line(3), Some(GutterMark::Modified));
        assert_eq!(state.mark_for_line(4), Some(GutterMark::Added));
        assert_eq!(state.mark_for_line(5), Some(GutterMark::Added));
    }

    #[test]
    fn test_from_diff_output_empty_diff() {
        let state = GutterState::from_diff_output("");
        assert!(state.is_empty());
    }

    #[test]
    fn test_from_diff_output_multiple_hunks() {
        let diff = "\
diff --git a/file.rs b/file.rs
@@ -1,0 +1,1 @@
+new first line
@@ -10,1 +11,1 @@
-old
+modified
";
        let state = GutterState::from_diff_output(diff);
        assert_eq!(state.mark_for_line(1), Some(GutterMark::Added));
        assert_eq!(state.mark_for_line(11), Some(GutterMark::Modified));
        assert_eq!(state.mark_count(), 2);
    }

    #[test]
    fn test_from_file_nonexistent_returns_none() {
        let result = GutterState::from_file(Path::new("/nonexistent/path/to/file.txt"));
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_range_with_comma() {
        assert_eq!(parse_range("10,3"), Some((10, 3)));
    }

    #[test]
    fn test_parse_range_without_comma() {
        assert_eq!(parse_range("42"), Some((42, 1)));
    }

    #[test]
    fn test_parse_range_count_with_comma() {
        assert_eq!(parse_range_count("10,5"), 5);
    }

    #[test]
    fn test_parse_range_count_without_comma() {
        assert_eq!(parse_range_count("10"), 1);
    }
}
