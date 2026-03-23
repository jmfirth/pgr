//! Search pattern compilation and matching.

use regex::{Regex, RegexBuilder};

/// Byte-offset range of a match within a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchRange {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
}

/// Case sensitivity mode for pattern matching.
///
/// Mirrors the behavior of less's `-i` and `-I` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseMode {
    /// Always case-sensitive (default, no flag).
    Sensitive,
    /// Always case-insensitive (`-I` / `--IGNORE-CASE`).
    Insensitive,
    /// Smart case: insensitive if the pattern is all lowercase,
    /// sensitive if any uppercase character is present (`-i` / `--ignore-case`).
    Smart,
}

/// A compiled search pattern with case-mode-aware matching.
#[derive(Debug)]
pub struct SearchPattern {
    regex: Regex,
    /// The original pattern string (for display in prompts).
    pattern: String,
    /// The case mode used during compilation.
    case_mode: CaseMode,
}

/// Determine whether the pattern should be compiled case-insensitively.
fn should_be_insensitive(pattern: &str, case_mode: CaseMode) -> bool {
    match case_mode {
        CaseMode::Sensitive => false,
        CaseMode::Insensitive => true,
        CaseMode::Smart => !pattern.chars().any(|c| c.is_ascii_uppercase()),
    }
}

impl SearchPattern {
    /// Compile a search pattern with the specified case mode.
    ///
    /// For `CaseMode::Smart`, inspects the pattern string: if all
    /// characters are lowercase (no uppercase ASCII letters), the
    /// regex is compiled case-insensitively. If any uppercase letter
    /// is present, it is compiled case-sensitively.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::InvalidPattern` if the regex syntax is invalid.
    pub fn compile(pattern: &str, case_mode: CaseMode) -> crate::Result<Self> {
        let insensitive = should_be_insensitive(pattern, case_mode);
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(insensitive)
            .build()
            .map_err(|e| crate::SearchError::InvalidPattern(e.to_string()))?;

        Ok(Self {
            regex,
            pattern: pattern.to_string(),
            case_mode,
        })
    }

    /// Compile a literal (non-regex) pattern by escaping metacharacters.
    ///
    /// The `pattern` string is treated as literal text: all regex
    /// metacharacters are escaped before compilation.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::InvalidPattern` if compilation fails
    /// (should not happen with escaped input).
    pub fn compile_literal(pattern: &str, case_mode: CaseMode) -> crate::Result<Self> {
        let escaped = regex::escape(pattern);
        Self::compile(&escaped, case_mode)
    }

    /// Escape regex metacharacters in a pattern string.
    ///
    /// Useful when the `^R` (literal) modifier is active.
    #[must_use]
    pub fn escape(pattern: &str) -> String {
        regex::escape(pattern)
    }

    /// Find all non-overlapping matches in the given line.
    ///
    /// Returns byte-offset ranges into `line`. Returns an empty vec
    /// if there are no matches.
    #[must_use]
    pub fn find_in(&self, line: &str) -> Vec<MatchRange> {
        self.regex
            .find_iter(line)
            .map(|m| MatchRange {
                start: m.start(),
                end: m.end(),
            })
            .collect()
    }

    /// Returns `true` if the pattern matches anywhere in the line.
    #[must_use]
    pub fn is_match(&self, line: &str) -> bool {
        self.regex.is_match(line)
    }

    /// Returns the original pattern string.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Returns the case mode used for this pattern.
    #[must_use]
    pub fn case_mode(&self) -> CaseMode {
        self.case_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_simple_literal_succeeds() {
        let pat = SearchPattern::compile("hello", CaseMode::Sensitive);
        assert!(pat.is_ok());
    }

    #[test]
    fn test_compile_regex_pattern_succeeds() {
        let pat = SearchPattern::compile("err(or|ors)", CaseMode::Sensitive);
        assert!(pat.is_ok());
    }

    #[test]
    fn test_compile_invalid_regex_returns_error() {
        let pat = SearchPattern::compile("(unclosed", CaseMode::Sensitive);
        assert!(pat.is_err());
        let err = pat.unwrap_err();
        assert!(matches!(err, crate::SearchError::InvalidPattern(_)));
    }

    #[test]
    fn test_find_in_single_match_returns_correct_range() {
        let pat = SearchPattern::compile("world", CaseMode::Sensitive).unwrap();
        let matches = pat.find_in("hello world");
        assert_eq!(matches, vec![MatchRange { start: 6, end: 11 }]);
    }

    #[test]
    fn test_find_in_multiple_matches_returns_all() {
        let pat = SearchPattern::compile("ab", CaseMode::Sensitive).unwrap();
        let matches = pat.find_in("abab");
        assert_eq!(
            matches,
            vec![
                MatchRange { start: 0, end: 2 },
                MatchRange { start: 2, end: 4 },
            ]
        );
    }

    #[test]
    fn test_find_in_no_match_returns_empty() {
        let pat = SearchPattern::compile("xyz", CaseMode::Sensitive).unwrap();
        let matches = pat.find_in("hello");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_in_regex_groups_returns_full_match_range() {
        let pat = SearchPattern::compile(r"error\d+", CaseMode::Sensitive).unwrap();
        let matches = pat.find_in("error123");
        assert_eq!(matches, vec![MatchRange { start: 0, end: 8 }]);
    }

    #[test]
    fn test_is_match_true_when_pattern_found() {
        let pat = SearchPattern::compile("ell", CaseMode::Sensitive).unwrap();
        assert!(pat.is_match("hello"));
    }

    #[test]
    fn test_is_match_false_when_no_match() {
        let pat = SearchPattern::compile("xyz", CaseMode::Sensitive).unwrap();
        assert!(!pat.is_match("hello"));
    }

    #[test]
    fn test_case_sensitive_does_not_match_wrong_case() {
        let pat = SearchPattern::compile("hello", CaseMode::Sensitive).unwrap();
        assert!(!pat.is_match("Hello"));
    }

    #[test]
    fn test_case_insensitive_matches_any_case() {
        let pat = SearchPattern::compile("hello", CaseMode::Insensitive).unwrap();
        assert!(pat.is_match("Hello"));
    }

    #[test]
    fn test_smart_case_lowercase_pattern_is_insensitive() {
        let pat = SearchPattern::compile("hello", CaseMode::Smart).unwrap();
        assert!(pat.is_match("Hello"));
    }

    #[test]
    fn test_smart_case_uppercase_pattern_is_sensitive() {
        let pat = SearchPattern::compile("Hello", CaseMode::Smart).unwrap();
        assert!(!pat.is_match("hello"));
    }

    #[test]
    fn test_smart_case_mixed_pattern_is_sensitive() {
        let pat = SearchPattern::compile("heLLo", CaseMode::Smart).unwrap();
        assert!(!pat.is_match("hello"));
    }

    #[test]
    fn test_pattern_returns_original_string() {
        let pat = SearchPattern::compile("test.*pattern", CaseMode::Sensitive).unwrap();
        assert_eq!(pat.pattern(), "test.*pattern");
    }

    #[test]
    fn test_case_mode_returns_compile_mode() {
        let pat_s = SearchPattern::compile("a", CaseMode::Sensitive).unwrap();
        assert_eq!(pat_s.case_mode(), CaseMode::Sensitive);

        let pat_i = SearchPattern::compile("a", CaseMode::Insensitive).unwrap();
        assert_eq!(pat_i.case_mode(), CaseMode::Insensitive);

        let pat_m = SearchPattern::compile("a", CaseMode::Smart).unwrap();
        assert_eq!(pat_m.case_mode(), CaseMode::Smart);
    }

    #[test]
    fn test_find_in_empty_line_returns_empty() {
        let pat = SearchPattern::compile("hello", CaseMode::Sensitive).unwrap();
        let matches = pat.find_in("");
        assert!(matches.is_empty());
    }

    #[test]
    fn test_find_in_empty_pattern_matches_everywhere() {
        let pat = SearchPattern::compile("", CaseMode::Sensitive).unwrap();
        let matches = pat.find_in("abc");
        // Empty pattern matches at every position: before a, before b, before c, and at end
        assert_eq!(matches.len(), 4);
        for m in &matches {
            assert_eq!(m.start, m.end); // Zero-width matches
        }
    }

    #[test]
    fn test_compile_special_regex_chars_work() {
        // Escaped dot
        let pat = SearchPattern::compile(r"\.", CaseMode::Sensitive).unwrap();
        assert!(pat.is_match("file.txt"));
        assert!(!pat.is_match("filetxt"));

        // Digit class
        let pat = SearchPattern::compile(r"\d", CaseMode::Sensitive).unwrap();
        assert!(pat.is_match("abc123"));
        assert!(!pat.is_match("abc"));

        // Character class
        let pat = SearchPattern::compile("[a-z]", CaseMode::Sensitive).unwrap();
        assert!(pat.is_match("hello"));
        assert!(!pat.is_match("123"));

        // Anchors
        let pat = SearchPattern::compile("^hello$", CaseMode::Sensitive).unwrap();
        assert!(pat.is_match("hello"));
        assert!(!pat.is_match("say hello"));
    }

    #[test]
    fn test_find_in_utf8_content_correct_byte_offsets() {
        // "café" is 5 bytes: c(1) a(1) f(1) é(2)
        let line = "café match";
        let pat = SearchPattern::compile("match", CaseMode::Sensitive).unwrap();
        let matches = pat.find_in(line);
        assert_eq!(matches.len(), 1);
        let m = matches[0];
        // Verify the byte offsets slice back to the correct substring
        assert_eq!(&line[m.start..m.end], "match");
    }

    #[test]
    fn test_compile_literal_escapes_metacharacters() {
        // "foo.*bar" as a literal should only match the literal string "foo.*bar"
        let pat = SearchPattern::compile_literal("foo.*bar", CaseMode::Sensitive).unwrap();
        assert!(pat.is_match("foo.*bar"));
        assert!(!pat.is_match("fooXbar"));
    }

    #[test]
    fn test_escape_returns_escaped_string() {
        assert_eq!(SearchPattern::escape("foo.*bar"), r"foo\.\*bar");
        assert_eq!(SearchPattern::escape("hello"), "hello");
        assert_eq!(SearchPattern::escape("[a-z]+"), r"\[a\-z\]\+");
    }
}
