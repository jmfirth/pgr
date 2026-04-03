//! Diff parsing — extracts structure (files, hunks, line types) from unified diffs.
//!
//! This module parses unified diff output (e.g., from `git diff`, `diff -u`, or
//! patch files) into a structured representation that enables hunk-level navigation
//! and diff-aware rendering.

/// A parsed file entry within a unified diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    /// Line number (0-indexed in buffer) where `diff --git` or `---` starts.
    pub start_line: usize,
    /// Old filename (from `--- a/path`).
    pub old_name: Option<String>,
    /// New filename (from `+++ b/path`).
    pub new_name: Option<String>,
    /// Hunks within this file.
    pub hunks: Vec<DiffHunk>,
}

/// A single hunk within a diff file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    /// Line number (0-indexed in buffer) of the `@@` header.
    pub header_line: usize,
    /// First content line (line after the `@@` header).
    pub content_start: usize,
    /// Last content line (inclusive).
    pub content_end: usize,
    /// Old file start line from the hunk header.
    pub old_start: usize,
    /// Old file line count from the hunk header.
    pub old_count: usize,
    /// New file start line from the hunk header.
    pub new_start: usize,
    /// New file line count from the hunk header.
    pub new_count: usize,
}

/// Line classification within a diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineType {
    /// File-level headers: `diff --git`, `index`, `---`, `+++`.
    Header,
    /// Hunk header: `@@ ... @@`.
    HunkHeader,
    /// Context line (space-prefixed, unchanged).
    Context,
    /// Added line (`+` prefixed).
    Added,
    /// Removed line (`-` prefixed).
    Removed,
    /// Anything else (binary notices, `\ No newline at end of file`, etc.).
    Other,
}

/// Information about the current diff position, for prompt rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPromptInfo {
    /// The current diff file name (from `+++ b/path`), if available.
    pub current_file: Option<String>,
    /// Current hunk position: `(current_1based, total)`.
    pub hunk_index: Option<(usize, usize)>,
    /// Current file position: `(current_1based, total)`.
    pub file_index: Option<(usize, usize)>,
}

/// Classify a single line within a diff.
///
/// Uses simple prefix matching to determine the line type.
#[must_use]
pub fn classify_diff_line(line: &str) -> DiffLineType {
    if line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
    {
        DiffLineType::Header
    } else if line.starts_with("@@ ") {
        DiffLineType::HunkHeader
    } else if line.starts_with('+') {
        DiffLineType::Added
    } else if line.starts_with('-') {
        DiffLineType::Removed
    } else if line.starts_with(' ') {
        DiffLineType::Context
    } else {
        DiffLineType::Other
    }
}

/// Parse a unified diff from buffer lines into structured `DiffFile`/`DiffHunk` data.
///
/// Walks through lines sequentially, identifying file boundaries (`diff --git` or
/// `---`/`+++` pairs), hunk headers (`@@`), and content lines. Returns an empty
/// `Vec` if no diff structure is found.
#[must_use]
pub fn parse_diff(lines: &[&str]) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Start of a new file: `diff --git a/... b/...`
        if line.starts_with("diff --git ") {
            let mut file = DiffFile {
                start_line: i,
                old_name: None,
                new_name: None,
                hunks: Vec::new(),
            };

            // Extract filenames from the `diff --git a/path b/path` line
            if let Some(names) = parse_git_diff_header(line) {
                file.old_name = Some(names.0);
                file.new_name = Some(names.1);
            }

            i += 1;

            // Skip metadata lines (index, old mode, new mode, etc.) until --- or @@
            while i < lines.len() {
                let cur = lines[i];
                if cur.starts_with("--- ") {
                    file.old_name = parse_diff_filename(cur, "--- ");
                    i += 1;
                    if i < lines.len() && lines[i].starts_with("+++ ") {
                        file.new_name = parse_diff_filename(lines[i], "+++ ");
                        i += 1;
                    }
                    break;
                } else if cur.starts_with("@@ ") {
                    // Binary diff or hunk without --- +++ lines
                    break;
                } else if cur.starts_with("diff --git ") {
                    // Next file without any hunks
                    break;
                }
                i += 1;
            }

            // Parse hunks for this file
            parse_hunks(lines, &mut i, &mut file);
            files.push(file);
        } else if line.starts_with("--- ")
            && i + 1 < lines.len()
            && lines[i + 1].starts_with("+++ ")
        {
            // Plain unified diff without `diff --git` header
            let mut file = DiffFile {
                start_line: i,
                old_name: parse_diff_filename(line, "--- "),
                new_name: None,
                hunks: Vec::new(),
            };
            i += 1;
            if i < lines.len() && lines[i].starts_with("+++ ") {
                file.new_name = parse_diff_filename(lines[i], "+++ ");
                i += 1;
            }

            parse_hunks(lines, &mut i, &mut file);
            files.push(file);
        } else {
            i += 1;
        }
    }

    files
}

/// Parse hunks from the current position until the next file or end of input.
fn parse_hunks(lines: &[&str], i: &mut usize, file: &mut DiffFile) {
    while *i < lines.len() {
        let line = lines[*i];
        if line.starts_with("@@ ") {
            if let Some(hunk) = parse_hunk_header(line, *i) {
                let header_line = *i;
                *i += 1;

                // Find the end of this hunk's content
                let content_start = *i;
                let mut content_end = header_line; // Default if no content lines

                while *i < lines.len() {
                    let cur = lines[*i];
                    if cur.starts_with("@@ ")
                        || cur.starts_with("diff --git ")
                        || (cur.starts_with("--- ")
                            && *i + 1 < lines.len()
                            && lines[*i + 1].starts_with("+++ "))
                    {
                        break;
                    }
                    content_end = *i;
                    *i += 1;
                }

                let final_hunk = DiffHunk {
                    header_line: hunk.header_line,
                    content_start,
                    content_end,
                    old_start: hunk.old_start,
                    old_count: hunk.old_count,
                    new_start: hunk.new_start,
                    new_count: hunk.new_count,
                };
                file.hunks.push(final_hunk);
            } else {
                *i += 1;
            }
        } else if line.starts_with("diff --git ")
            || (line.starts_with("--- ")
                && *i + 1 < lines.len()
                && lines[*i + 1].starts_with("+++ "))
        {
            // Next file starts here
            break;
        } else {
            *i += 1;
        }
    }
}

/// Parse a hunk header line: `@@ -old_start,old_count +new_start,new_count @@ ...`
///
/// Returns a partially-filled `DiffHunk` with header info. Content start/end are
/// set to placeholder values; the caller fills them in after scanning content lines.
fn parse_hunk_header(line: &str, line_num: usize) -> Option<DiffHunk> {
    let bytes = line.as_bytes();

    // Must start with "@@ -"
    if !bytes.starts_with(b"@@ -") {
        return None;
    }

    let mut pos = 4; // Skip "@@ -"

    let old_start = parse_number(bytes, &mut pos)?;
    let old_count = if pos < bytes.len() && bytes[pos] == b',' {
        pos += 1;
        parse_number(bytes, &mut pos)?
    } else {
        1 // Default count when omitted
    };

    // Expect " +"
    if pos + 1 >= bytes.len() || bytes[pos] != b' ' || bytes[pos + 1] != b'+' {
        return None;
    }
    pos += 2;

    let new_start = parse_number(bytes, &mut pos)?;
    let new_count = if pos < bytes.len() && bytes[pos] == b',' {
        pos += 1;
        parse_number(bytes, &mut pos)?
    } else {
        1 // Default count when omitted
    };

    // Expect " @@"
    if pos + 2 >= bytes.len()
        || bytes[pos] != b' '
        || bytes[pos + 1] != b'@'
        || bytes[pos + 2] != b'@'
    {
        return None;
    }

    Some(DiffHunk {
        header_line: line_num,
        content_start: line_num + 1,
        content_end: line_num,
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

/// Parse a decimal number from bytes, advancing the position.
fn parse_number(bytes: &[u8], pos: &mut usize) -> Option<usize> {
    if *pos >= bytes.len() || !bytes[*pos].is_ascii_digit() {
        return None;
    }
    let mut n: usize = 0;
    while *pos < bytes.len() && bytes[*pos].is_ascii_digit() {
        n = n
            .checked_mul(10)?
            .checked_add(usize::from(bytes[*pos] - b'0'))?;
        *pos += 1;
    }
    Some(n)
}

/// Extract filenames from a `diff --git a/path b/path` line.
///
/// Returns `(old_path, new_path)`. Strips the `a/` and `b/` prefixes.
fn parse_git_diff_header(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("diff --git ")?;
    // Format: "a/path b/path" — split on " b/" working from the right
    // to handle filenames containing spaces.
    let b_pos = rest.rfind(" b/")?;
    let a_part = &rest[..b_pos];
    let b_part = &rest[b_pos + 1..];
    let old = a_part.strip_prefix("a/").unwrap_or(a_part);
    let new = b_part.strip_prefix("b/").unwrap_or(b_part);
    Some((old.to_string(), new.to_string()))
}

/// Extract a filename from a `---` or `+++` line.
///
/// Strips the prefix and the `a/` or `b/` path prefix. Returns `None` for
/// `/dev/null` (new files or deleted files).
fn parse_diff_filename(line: &str, prefix: &str) -> Option<String> {
    let rest = line.strip_prefix(prefix)?;
    // Strip tab and anything after (timestamps in plain diff -u)
    let name = rest.split('\t').next().unwrap_or(rest);
    if name == "/dev/null" {
        return None;
    }
    let stripped = name
        .strip_prefix("a/")
        .or_else(|| name.strip_prefix("b/"))
        .unwrap_or(name);
    Some(stripped.to_string())
}

/// Compute diff prompt info for a given viewport top line.
///
/// Determines which file and hunk the user is currently viewing based on
/// the `top_line` (0-indexed buffer line). Returns `None` if diff state is empty.
#[must_use]
pub fn compute_diff_prompt_info(files: &[DiffFile], top_line: usize) -> Option<DiffPromptInfo> {
    if files.is_empty() {
        return None;
    }

    let total_files = files.len();
    let total_hunks: usize = files.iter().map(|f| f.hunks.len()).sum();

    // Find which file we're in
    let mut current_file_idx = 0;
    for (idx, file) in files.iter().enumerate() {
        if file.start_line <= top_line {
            current_file_idx = idx;
        } else {
            break;
        }
    }

    let file = &files[current_file_idx];
    let file_name = file.new_name.as_ref().or(file.old_name.as_ref()).cloned();

    // Find which hunk we're in (global index across all files)
    let mut global_hunk_idx: Option<usize> = None;
    let mut hunk_counter = 0;
    for f in files {
        for hunk in &f.hunks {
            if hunk.header_line <= top_line {
                global_hunk_idx = Some(hunk_counter);
            }
            hunk_counter += 1;
        }
    }

    Some(DiffPromptInfo {
        current_file: file_name,
        hunk_index: global_hunk_idx.map(|idx| (idx + 1, total_hunks)),
        file_index: Some((current_file_idx + 1, total_files)),
    })
}

/// Find the next hunk header line after `current_line`.
///
/// Returns `None` if there are no hunks after the current line. If `wrap`
/// is true, wraps around to the first hunk.
#[must_use]
pub fn next_hunk_line(files: &[DiffFile], current_line: usize, wrap: bool) -> Option<usize> {
    // Find the first hunk header after current_line
    for file in files {
        for hunk in &file.hunks {
            if hunk.header_line > current_line {
                return Some(hunk.header_line);
            }
        }
    }

    // Wrap to first hunk
    if wrap {
        for file in files {
            if let Some(hunk) = file.hunks.first() {
                return Some(hunk.header_line);
            }
        }
    }

    None
}

/// Find the previous hunk header line before `current_line`.
///
/// Returns `None` if there are no hunks before the current line. If `wrap`
/// is true, wraps around to the last hunk.
#[must_use]
pub fn prev_hunk_line(files: &[DiffFile], current_line: usize, wrap: bool) -> Option<usize> {
    let mut best: Option<usize> = None;

    for file in files {
        for hunk in &file.hunks {
            if hunk.header_line < current_line {
                best = Some(hunk.header_line);
            }
        }
    }

    if best.is_some() {
        return best;
    }

    // Wrap to last hunk
    if wrap {
        for file in files.iter().rev() {
            if let Some(hunk) = file.hunks.last() {
                return Some(hunk.header_line);
            }
        }
    }

    None
}

/// Find the next file start line after `current_line`.
///
/// Returns `None` if there are no files after the current line. If `wrap`
/// is true, wraps around to the first file.
#[must_use]
pub fn next_file_line(files: &[DiffFile], current_line: usize, wrap: bool) -> Option<usize> {
    for file in files {
        if file.start_line > current_line {
            return Some(file.start_line);
        }
    }

    if wrap {
        files.first().map(|f| f.start_line)
    } else {
        None
    }
}

/// Find the previous file start line before `current_line`.
///
/// Returns `None` if there are no files before the current line. If `wrap`
/// is true, wraps around to the last file.
#[must_use]
pub fn prev_file_line(files: &[DiffFile], current_line: usize, wrap: bool) -> Option<usize> {
    let mut best: Option<usize> = None;

    for file in files {
        if file.start_line < current_line {
            best = Some(file.start_line);
        }
    }

    if best.is_some() {
        return best;
    }

    if wrap {
        files.last().map(|f| f.start_line)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── classify_diff_line tests ──

    #[test]
    fn test_classify_diff_line_diff_header() {
        assert_eq!(
            classify_diff_line("diff --git a/foo.rs b/foo.rs"),
            DiffLineType::Header
        );
    }

    #[test]
    fn test_classify_diff_line_index_header() {
        assert_eq!(
            classify_diff_line("index 1234567..abcdefg 100644"),
            DiffLineType::Header
        );
    }

    #[test]
    fn test_classify_diff_line_old_file_header() {
        assert_eq!(classify_diff_line("--- a/foo.rs"), DiffLineType::Header);
    }

    #[test]
    fn test_classify_diff_line_new_file_header() {
        assert_eq!(classify_diff_line("+++ b/foo.rs"), DiffLineType::Header);
    }

    #[test]
    fn test_classify_diff_line_hunk_header() {
        assert_eq!(
            classify_diff_line("@@ -1,3 +1,4 @@"),
            DiffLineType::HunkHeader
        );
    }

    #[test]
    fn test_classify_diff_line_hunk_header_with_context() {
        assert_eq!(
            classify_diff_line("@@ -10,3 +15,7 @@ fn foo()"),
            DiffLineType::HunkHeader
        );
    }

    #[test]
    fn test_classify_diff_line_context() {
        assert_eq!(classify_diff_line(" unchanged line"), DiffLineType::Context);
    }

    #[test]
    fn test_classify_diff_line_added() {
        assert_eq!(classify_diff_line("+new line"), DiffLineType::Added);
    }

    #[test]
    fn test_classify_diff_line_removed() {
        assert_eq!(classify_diff_line("-old line"), DiffLineType::Removed);
    }

    #[test]
    fn test_classify_diff_line_other_no_newline() {
        assert_eq!(
            classify_diff_line("\\ No newline at end of file"),
            DiffLineType::Other
        );
    }

    #[test]
    fn test_classify_diff_line_other_empty() {
        assert_eq!(classify_diff_line(""), DiffLineType::Other);
    }

    // ── parse_diff tests ──

    #[test]
    fn test_parse_diff_simple_one_file_two_hunks() {
        let lines = vec![
            "diff --git a/foo.rs b/foo.rs",
            "index 1234567..abcdefg 100644",
            "--- a/foo.rs",
            "+++ b/foo.rs",
            "@@ -1,3 +1,4 @@",
            " line 1",
            "+added line",
            " line 2",
            " line 3",
            "@@ -10,2 +11,3 @@ fn bar()",
            " line 10",
            "+added line 2",
            " line 11",
        ];

        let files = parse_diff(&lines);
        assert_eq!(files.len(), 1);

        let file = &files[0];
        assert_eq!(file.start_line, 0);
        assert_eq!(file.old_name.as_deref(), Some("foo.rs"));
        assert_eq!(file.new_name.as_deref(), Some("foo.rs"));
        assert_eq!(file.hunks.len(), 2);

        let h0 = &file.hunks[0];
        assert_eq!(h0.header_line, 4);
        assert_eq!(h0.content_start, 5);
        assert_eq!(h0.content_end, 8);
        assert_eq!(h0.old_start, 1);
        assert_eq!(h0.old_count, 3);
        assert_eq!(h0.new_start, 1);
        assert_eq!(h0.new_count, 4);

        let h1 = &file.hunks[1];
        assert_eq!(h1.header_line, 9);
        assert_eq!(h1.content_start, 10);
        assert_eq!(h1.content_end, 12);
        assert_eq!(h1.old_start, 10);
        assert_eq!(h1.old_count, 2);
        assert_eq!(h1.new_start, 11);
        assert_eq!(h1.new_count, 3);
    }

    #[test]
    fn test_parse_diff_multi_file() {
        let lines = vec![
            "diff --git a/src/main.rs b/src/main.rs",
            "--- a/src/main.rs",
            "+++ b/src/main.rs",
            "@@ -1,2 +1,3 @@",
            " fn main() {",
            "+    println!(\"hello\");",
            " }",
            "diff --git a/src/lib.rs b/src/lib.rs",
            "--- a/src/lib.rs",
            "+++ b/src/lib.rs",
            "@@ -5,3 +5,4 @@",
            " pub fn foo() {",
            "+    // comment",
            " }",
            "diff --git a/Cargo.toml b/Cargo.toml",
            "--- a/Cargo.toml",
            "+++ b/Cargo.toml",
            "@@ -1,1 +1,2 @@",
            " [package]",
            "+name = \"test\"",
        ];

        let files = parse_diff(&lines);
        assert_eq!(files.len(), 3);

        assert_eq!(files[0].new_name.as_deref(), Some("src/main.rs"));
        assert_eq!(files[0].hunks.len(), 1);

        assert_eq!(files[1].new_name.as_deref(), Some("src/lib.rs"));
        assert_eq!(files[1].hunks.len(), 1);

        assert_eq!(files[2].new_name.as_deref(), Some("Cargo.toml"));
        assert_eq!(files[2].hunks.len(), 1);
    }

    #[test]
    fn test_parse_diff_no_newline_at_end() {
        let lines = vec![
            "diff --git a/foo.rs b/foo.rs",
            "--- a/foo.rs",
            "+++ b/foo.rs",
            "@@ -1,2 +1,2 @@",
            "-old line",
            "+new line",
            "\\ No newline at end of file",
        ];

        let files = parse_diff(&lines);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[0].hunks[0].content_end, 6);
    }

    #[test]
    fn test_parse_diff_hunk_header_no_comma() {
        let lines = vec![
            "--- a/foo.rs",
            "+++ b/foo.rs",
            "@@ -1 +1 @@",
            "-old",
            "+new",
        ];

        let files = parse_diff(&lines);
        assert_eq!(files.len(), 1);
        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.old_start, 1);
        assert_eq!(hunk.old_count, 1);
        assert_eq!(hunk.new_start, 1);
        assert_eq!(hunk.new_count, 1);
    }

    #[test]
    fn test_parse_diff_hunk_header_with_function_context() {
        let lines = vec![
            "diff --git a/foo.rs b/foo.rs",
            "--- a/foo.rs",
            "+++ b/foo.rs",
            "@@ -10,3 +15,7 @@ fn foo()",
            " line",
        ];

        let files = parse_diff(&lines);
        assert_eq!(files.len(), 1);
        let hunk = &files[0].hunks[0];
        assert_eq!(hunk.old_start, 10);
        assert_eq!(hunk.old_count, 3);
        assert_eq!(hunk.new_start, 15);
        assert_eq!(hunk.new_count, 7);
    }

    #[test]
    fn test_parse_diff_empty_input() {
        let files = parse_diff(&[]);
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_diff_no_diff_content() {
        let lines = vec!["hello world", "just some text"];
        let files = parse_diff(&lines);
        assert!(files.is_empty());
    }

    #[test]
    fn test_parse_diff_plain_unified_diff() {
        let lines = vec![
            "--- a/foo.txt",
            "+++ b/foo.txt",
            "@@ -1,3 +1,4 @@",
            " line 1",
            "+added",
            " line 2",
            " line 3",
        ];

        let files = parse_diff(&lines);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].start_line, 0);
        assert_eq!(files[0].old_name.as_deref(), Some("foo.txt"));
        assert_eq!(files[0].new_name.as_deref(), Some("foo.txt"));
    }

    // ── navigation tests ──

    #[test]
    fn test_next_hunk_line_finds_next() {
        let files = vec![DiffFile {
            start_line: 0,
            old_name: None,
            new_name: None,
            hunks: vec![
                DiffHunk {
                    header_line: 4,
                    content_start: 5,
                    content_end: 8,
                    old_start: 1,
                    old_count: 3,
                    new_start: 1,
                    new_count: 4,
                },
                DiffHunk {
                    header_line: 20,
                    content_start: 21,
                    content_end: 25,
                    old_start: 10,
                    old_count: 3,
                    new_start: 10,
                    new_count: 5,
                },
            ],
        }];

        assert_eq!(next_hunk_line(&files, 5, false), Some(20));
    }

    #[test]
    fn test_prev_hunk_line_finds_previous() {
        let files = vec![DiffFile {
            start_line: 0,
            old_name: None,
            new_name: None,
            hunks: vec![
                DiffHunk {
                    header_line: 4,
                    content_start: 5,
                    content_end: 8,
                    old_start: 1,
                    old_count: 3,
                    new_start: 1,
                    new_count: 4,
                },
                DiffHunk {
                    header_line: 20,
                    content_start: 21,
                    content_end: 25,
                    old_start: 10,
                    old_count: 3,
                    new_start: 10,
                    new_count: 5,
                },
            ],
        }];

        assert_eq!(prev_hunk_line(&files, 20, false), Some(4));
    }

    #[test]
    fn test_next_hunk_line_wraps() {
        let files = vec![DiffFile {
            start_line: 0,
            old_name: None,
            new_name: None,
            hunks: vec![DiffHunk {
                header_line: 4,
                content_start: 5,
                content_end: 8,
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 4,
            }],
        }];

        // Past the last hunk, should wrap
        assert_eq!(next_hunk_line(&files, 10, true), Some(4));
        // Past the last hunk, no wrap
        assert_eq!(next_hunk_line(&files, 10, false), None);
    }

    #[test]
    fn test_prev_hunk_line_wraps() {
        let files = vec![DiffFile {
            start_line: 0,
            old_name: None,
            new_name: None,
            hunks: vec![
                DiffHunk {
                    header_line: 4,
                    content_start: 5,
                    content_end: 8,
                    old_start: 1,
                    old_count: 3,
                    new_start: 1,
                    new_count: 4,
                },
                DiffHunk {
                    header_line: 20,
                    content_start: 21,
                    content_end: 25,
                    old_start: 10,
                    old_count: 3,
                    new_start: 10,
                    new_count: 5,
                },
            ],
        }];

        // Before the first hunk, should wrap to last
        assert_eq!(prev_hunk_line(&files, 2, true), Some(20));
        // Before the first hunk, no wrap
        assert_eq!(prev_hunk_line(&files, 2, false), None);
    }

    #[test]
    fn test_next_file_line_finds_next() {
        let files = vec![
            DiffFile {
                start_line: 0,
                old_name: None,
                new_name: None,
                hunks: vec![],
            },
            DiffFile {
                start_line: 50,
                old_name: None,
                new_name: None,
                hunks: vec![],
            },
        ];

        assert_eq!(next_file_line(&files, 10, false), Some(50));
    }

    #[test]
    fn test_prev_file_line_finds_previous() {
        let files = vec![
            DiffFile {
                start_line: 0,
                old_name: None,
                new_name: None,
                hunks: vec![],
            },
            DiffFile {
                start_line: 50,
                old_name: None,
                new_name: None,
                hunks: vec![],
            },
        ];

        assert_eq!(prev_file_line(&files, 50, false), Some(0));
    }

    #[test]
    fn test_next_file_line_wraps() {
        let files = vec![DiffFile {
            start_line: 0,
            old_name: None,
            new_name: None,
            hunks: vec![],
        }];

        assert_eq!(next_file_line(&files, 10, true), Some(0));
        assert_eq!(next_file_line(&files, 10, false), None);
    }

    #[test]
    fn test_empty_diff_no_hunks() {
        let files: Vec<DiffFile> = vec![];
        assert_eq!(next_hunk_line(&files, 0, false), None);
        assert_eq!(prev_hunk_line(&files, 0, false), None);
        assert_eq!(next_file_line(&files, 0, false), None);
        assert_eq!(prev_file_line(&files, 0, false), None);
    }

    // ── diff prompt info tests ──

    #[test]
    fn test_compute_diff_prompt_info_empty() {
        assert_eq!(compute_diff_prompt_info(&[], 0), None);
    }

    #[test]
    fn test_compute_diff_prompt_info_basic() {
        let files = vec![
            DiffFile {
                start_line: 0,
                old_name: Some("old.rs".to_string()),
                new_name: Some("src/main.rs".to_string()),
                hunks: vec![
                    DiffHunk {
                        header_line: 4,
                        content_start: 5,
                        content_end: 8,
                        old_start: 1,
                        old_count: 3,
                        new_start: 1,
                        new_count: 4,
                    },
                    DiffHunk {
                        header_line: 20,
                        content_start: 21,
                        content_end: 25,
                        old_start: 10,
                        old_count: 3,
                        new_start: 10,
                        new_count: 5,
                    },
                ],
            },
            DiffFile {
                start_line: 30,
                old_name: None,
                new_name: Some("src/lib.rs".to_string()),
                hunks: vec![DiffHunk {
                    header_line: 35,
                    content_start: 36,
                    content_end: 40,
                    old_start: 1,
                    old_count: 3,
                    new_start: 1,
                    new_count: 4,
                }],
            },
        ];

        // At line 5 (in first file, after first hunk header)
        let info = compute_diff_prompt_info(&files, 5).unwrap();
        assert_eq!(info.current_file.as_deref(), Some("src/main.rs"));
        assert_eq!(info.hunk_index, Some((1, 3)));
        assert_eq!(info.file_index, Some((1, 2)));

        // At line 22 (in first file, after second hunk header)
        let info = compute_diff_prompt_info(&files, 22).unwrap();
        assert_eq!(info.current_file.as_deref(), Some("src/main.rs"));
        assert_eq!(info.hunk_index, Some((2, 3)));
        assert_eq!(info.file_index, Some((1, 2)));

        // At line 36 (in second file, after its hunk header)
        let info = compute_diff_prompt_info(&files, 36).unwrap();
        assert_eq!(info.current_file.as_deref(), Some("src/lib.rs"));
        assert_eq!(info.hunk_index, Some((3, 3)));
        assert_eq!(info.file_index, Some((2, 2)));
    }

    #[test]
    fn test_parse_diff_dev_null_old_name() {
        let lines = vec![
            "diff --git a/new_file.rs b/new_file.rs",
            "--- /dev/null",
            "+++ b/new_file.rs",
            "@@ -0,0 +1,3 @@",
            "+line 1",
            "+line 2",
            "+line 3",
        ];

        let files = parse_diff(&lines);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_name, None);
        assert_eq!(files[0].new_name.as_deref(), Some("new_file.rs"));
    }
}
