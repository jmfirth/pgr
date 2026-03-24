//! Ctags file parsing and tag navigation state.
//!
//! Supports the standard ctags format: `tagname\tfilename\tpattern`.
//! Patterns can be a line number or a `/pattern/` (or `?pattern?`) search.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::KeyError;

/// A single entry from a ctags file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagEntry {
    /// The tag name (identifier).
    pub tag: String,
    /// The file containing the tag.
    pub file: PathBuf,
    /// The ex-command pattern or line number used to locate the tag.
    pub pattern: String,
}

/// Find all entries matching `tag` in the given tags file.
///
/// Parses the standard ctags format where each line is:
/// `tagname<TAB>filename<TAB>pattern`
///
/// Lines starting with `!` are metadata and are skipped.
///
/// # Errors
///
/// Returns an error if the tags file cannot be read.
pub fn find_tag(tag: &str, tags_file: &Path) -> crate::error::Result<Vec<TagEntry>> {
    let content = fs::read_to_string(tags_file).map_err(|e| {
        KeyError::InvalidBinding(format!(
            "cannot read tags file {}: {e}",
            tags_file.display()
        ))
    })?;

    let mut results = Vec::new();
    for line in content.lines() {
        // Skip metadata lines.
        if line.starts_with('!') {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, '\t').collect();
        if parts.len() < 3 {
            continue;
        }

        if parts[0] == tag {
            // Strip trailing `;"<TAB>...` extended metadata from the pattern.
            // In extended ctags format, the pattern is terminated by `;"` followed
            // by a tab and extension fields (e.g., `/^fn main/;"\tkind:f`).
            let raw_pattern = parts[2];
            let pattern = if let Some(pos) = raw_pattern.find(";\"") {
                raw_pattern[..pos].to_string()
            } else {
                raw_pattern.to_string()
            };

            results.push(TagEntry {
                tag: parts[0].to_string(),
                file: PathBuf::from(parts[1]),
                pattern,
            });
        }
    }

    Ok(results)
}

/// Resolve a tag pattern to a 0-based line number.
///
/// Patterns can be:
/// - A decimal line number (e.g., `"42"`)
/// - A search pattern delimited by `/` or `?` (e.g., `"/^fn main/"`)
///
/// For search patterns, the file content is scanned line by line for a match.
/// Returns `None` if the pattern cannot be resolved.
#[must_use]
pub fn resolve_pattern(pattern: &str, file_content: &str) -> Option<usize> {
    // Try as a plain line number first.
    if let Ok(n) = pattern.parse::<usize>() {
        // ctags line numbers are 1-based.
        return Some(n.saturating_sub(1));
    }

    // Try as a /pattern/ or ?pattern? search.
    let stripped = pattern
        .strip_prefix('/')
        .and_then(|s| s.strip_suffix('/'))
        .or_else(|| pattern.strip_prefix('?').and_then(|s| s.strip_suffix('?')));

    if let Some(pat) = stripped {
        // ctags patterns are fixed strings, not regex. They use `^` for
        // start-of-line anchoring and may include backslash-escaped chars.
        let search = unescape_ctags_pattern(pat);
        for (i, line) in file_content.lines().enumerate() {
            if line.contains(&search) {
                return Some(i);
            }
        }
    }

    None
}

/// Unescape a ctags search pattern.
///
/// Ctags escapes `/` and `\` within patterns. This function reverses those
/// escapes and strips `^` / `$` anchors (we do a simple `contains` match).
fn unescape_ctags_pattern(pat: &str) -> String {
    let mut result = String::with_capacity(pat.len());
    let mut chars = pat.chars();
    // Strip leading `^` anchor.
    let first = chars.next();
    match first {
        Some('^') => {} // skip
        Some(c) => result.push(c),
        None => return result,
    }

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                result.push(next);
            }
        } else {
            result.push(c);
        }
    }

    // Strip trailing `$` anchor.
    if result.ends_with('$') {
        result.pop();
    }

    result
}

/// Tracks the current position within a list of tag matches for `t`/`T` navigation.
#[derive(Debug, Clone)]
pub struct TagState {
    /// All matching entries for the current tag.
    entries: Vec<TagEntry>,
    /// Index of the currently active entry within `entries`.
    current: usize,
}

impl TagState {
    /// Create a new tag state from a list of matching entries.
    ///
    /// The initial position is at the first entry.
    #[must_use]
    pub fn new(entries: Vec<TagEntry>) -> Self {
        Self {
            entries,
            current: 0,
        }
    }

    /// The current tag entry, if any.
    #[must_use]
    pub fn current_entry(&self) -> Option<&TagEntry> {
        self.entries.get(self.current)
    }

    /// Advance to the next tag match. Returns the new current entry, or `None`
    /// if already at the last match.
    #[must_use]
    pub fn advance(&mut self) -> Option<&TagEntry> {
        if self.current + 1 < self.entries.len() {
            self.current += 1;
            self.entries.get(self.current)
        } else {
            None
        }
    }

    /// Go back to the previous tag match. Returns the new current entry, or
    /// `None` if already at the first match.
    #[must_use]
    pub fn go_back(&mut self) -> Option<&TagEntry> {
        if self.current > 0 {
            self.current -= 1;
            self.entries.get(self.current)
        } else {
            None
        }
    }

    /// The number of tag matches.
    #[must_use]
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// The 0-based index of the current match.
    #[must_use]
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// Whether the tag state has any entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper to create a temp tags file with given content.
    fn make_tags_file(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("failed to create temp file");
        f.write_all(content.as_bytes())
            .expect("failed to write tags");
        f.flush().expect("failed to flush");
        f
    }

    // ── find_tag tests ──────────────────────────────────────────────

    #[test]
    fn test_find_tag_standard_format_returns_matches() {
        let tags = make_tags_file("main\tsrc/main.rs\t/^fn main/\nhelper\tsrc/lib.rs\t42\n");
        let result = find_tag("main", tags.path()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].tag, "main");
        assert_eq!(result[0].file, PathBuf::from("src/main.rs"));
        assert_eq!(result[0].pattern, "/^fn main/");
    }

    #[test]
    fn test_find_tag_multiple_matches_returns_all() {
        let content = "foo\ta.rs\t/^fn foo/\nfoo\tb.rs\t/^fn foo/\nbar\tc.rs\t10\n";
        let tags = make_tags_file(content);
        let result = find_tag("foo", tags.path()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].file, PathBuf::from("a.rs"));
        assert_eq!(result[1].file, PathBuf::from("b.rs"));
    }

    #[test]
    fn test_find_tag_no_match_returns_empty() {
        let tags = make_tags_file("main\tsrc/main.rs\t/^fn main/\n");
        let result = find_tag("nonexistent", tags.path()).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_find_tag_skips_metadata_lines() {
        let content = "!_TAG_FILE_FORMAT\t2\n!_TAG_FILE_SORTED\t1\nmain\tsrc/main.rs\t1\n";
        let tags = make_tags_file(content);
        let result = find_tag("main", tags.path()).unwrap();
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_find_tag_missing_file_returns_error() {
        let result = find_tag("main", Path::new("/nonexistent/tags"));
        assert!(result.is_err());
    }

    #[test]
    fn test_find_tag_strips_trailing_metadata() {
        let content = "main\tsrc/main.rs\t/^fn main/;\"\tf\n";
        let tags = make_tags_file(content);
        let result = find_tag("main", tags.path()).unwrap();
        assert_eq!(result[0].pattern, "/^fn main/");
    }

    #[test]
    fn test_find_tag_malformed_line_skipped() {
        let content = "malformed_no_tabs\nmain\tsrc/main.rs\t1\n";
        let tags = make_tags_file(content);
        let result = find_tag("main", tags.path()).unwrap();
        assert_eq!(result.len(), 1);
    }

    // ── resolve_pattern tests ───────────────────────────────────────

    #[test]
    fn test_resolve_pattern_line_number_returns_zero_based() {
        assert_eq!(resolve_pattern("42", ""), Some(41));
    }

    #[test]
    fn test_resolve_pattern_line_number_one_returns_zero() {
        assert_eq!(resolve_pattern("1", ""), Some(0));
    }

    #[test]
    fn test_resolve_pattern_search_pattern_finds_line() {
        let content = "first line\nfn main() {\n    // body\n}\n";
        assert_eq!(resolve_pattern("/^fn main/", content), Some(1));
    }

    #[test]
    fn test_resolve_pattern_question_mark_delimiter() {
        let content = "alpha\nbeta\ngamma\n";
        assert_eq!(resolve_pattern("?beta?", content), Some(1));
    }

    #[test]
    fn test_resolve_pattern_no_match_returns_none() {
        let content = "alpha\nbeta\n";
        assert_eq!(resolve_pattern("/nonexistent/", content), None);
    }

    #[test]
    fn test_resolve_pattern_unrecognized_returns_none() {
        assert_eq!(
            resolve_pattern("not_a_number_or_pattern", "content\n"),
            None
        );
    }

    // ── unescape_ctags_pattern tests ────────────────────────────────

    #[test]
    fn test_unescape_strips_caret_and_dollar() {
        assert_eq!(unescape_ctags_pattern("^fn main()$"), "fn main()");
    }

    #[test]
    fn test_unescape_handles_backslash_escapes() {
        assert_eq!(unescape_ctags_pattern("foo\\/bar"), "foo/bar");
    }

    #[test]
    fn test_unescape_empty_string() {
        assert_eq!(unescape_ctags_pattern(""), "");
    }

    // ── TagState tests ──────────────────────────────────────────────

    #[test]
    fn test_tag_state_new_starts_at_first_entry() {
        let entries = vec![
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("a.rs"),
                pattern: "1".into(),
            },
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("b.rs"),
                pattern: "2".into(),
            },
        ];
        let state = TagState::new(entries);
        assert_eq!(state.current_index(), 0);
        assert_eq!(state.count(), 2);
        assert!(!state.is_empty());
    }

    #[test]
    fn test_tag_state_next_advances() {
        let entries = vec![
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("a.rs"),
                pattern: "1".into(),
            },
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("b.rs"),
                pattern: "2".into(),
            },
        ];
        let mut state = TagState::new(entries);
        let next = state.advance();
        assert!(next.is_some());
        assert_eq!(state.current_index(), 1);
    }

    #[test]
    fn test_tag_state_next_at_end_returns_none() {
        let entries = vec![TagEntry {
            tag: "foo".into(),
            file: PathBuf::from("a.rs"),
            pattern: "1".into(),
        }];
        let mut state = TagState::new(entries);
        assert!(state.advance().is_none());
        assert_eq!(state.current_index(), 0);
    }

    #[test]
    fn test_tag_state_prev_goes_back() {
        let entries = vec![
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("a.rs"),
                pattern: "1".into(),
            },
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("b.rs"),
                pattern: "2".into(),
            },
        ];
        let mut state = TagState::new(entries);
        let _ = state.advance();
        let prev = state.go_back();
        assert!(prev.is_some());
        assert_eq!(state.current_index(), 0);
    }

    #[test]
    fn test_tag_state_prev_at_start_returns_none() {
        let entries = vec![TagEntry {
            tag: "foo".into(),
            file: PathBuf::from("a.rs"),
            pattern: "1".into(),
        }];
        let mut state = TagState::new(entries);
        assert!(state.go_back().is_none());
        assert_eq!(state.current_index(), 0);
    }

    #[test]
    fn test_tag_state_empty_entries() {
        let state = TagState::new(Vec::new());
        assert!(state.is_empty());
        assert_eq!(state.count(), 0);
        assert!(state.current_entry().is_none());
    }

    #[test]
    fn test_tag_state_current_entry_returns_correct() {
        let entries = vec![
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("a.rs"),
                pattern: "1".into(),
            },
            TagEntry {
                tag: "foo".into(),
                file: PathBuf::from("b.rs"),
                pattern: "2".into(),
            },
        ];
        let state = TagState::new(entries);
        let entry = state.current_entry().unwrap();
        assert_eq!(entry.file, PathBuf::from("a.rs"));
    }
}
