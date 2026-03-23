//! Search highlighting state and match computation.
//!
//! Provides [`HighlightState`] to track whether highlighting is enabled
//! (ESC-u toggle) and to compute match ranges for visible lines. Also
//! provides [`find_matches_in_line`] as a standalone utility.

use crate::{MatchRange, SearchPattern};

/// State for search highlighting across visible lines.
///
/// Tracks the ESC-u toggle and caches computed highlight ranges for the
/// currently visible set of lines. The cache should be cleared on scroll,
/// search change, or any other event that invalidates the visible content.
pub struct HighlightState {
    /// Whether highlighting is currently enabled (ESC-u toggle).
    enabled: bool,
    /// Cached highlights for the currently visible lines.
    /// Outer index corresponds to the visible line index (0 = first visible line).
    highlights: Vec<Vec<MatchRange>>,
}

impl HighlightState {
    /// Create a new highlight state with highlighting enabled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: true,
            highlights: Vec::new(),
        }
    }

    /// Toggle highlighting on/off. Returns the new state.
    pub fn toggle(&mut self) -> bool {
        self.enabled = !self.enabled;
        self.enabled
    }

    /// Return whether highlighting is currently enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Explicitly set the enabled state.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Compute highlight ranges for the given visible lines using the pattern.
    ///
    /// `visible_lines` is a slice of line contents (already fetched from the buffer).
    /// Each element is `Some(content)` for a valid line or `None` for lines beyond EOF.
    ///
    /// The result is cached internally and returned as a slice parallel to the input.
    /// If highlighting is disabled or no pattern is provided, returns empty vecs for all lines.
    pub fn compute_highlights(
        &mut self,
        visible_lines: &[Option<String>],
        pattern: Option<&SearchPattern>,
    ) -> &[Vec<MatchRange>] {
        self.highlights.clear();
        self.highlights.resize(visible_lines.len(), Vec::new());

        if !self.enabled {
            return &self.highlights;
        }

        let Some(pat) = pattern else {
            return &self.highlights;
        };

        for (i, line) in visible_lines.iter().enumerate() {
            if let Some(content) = line {
                self.highlights[i] = find_matches_in_line(content, pat);
            }
        }

        &self.highlights
    }

    /// Clear cached highlights (call after scroll, search change, etc.).
    pub fn clear(&mut self) {
        self.highlights.clear();
    }

    /// Return the cached highlights for visible lines.
    #[must_use]
    pub fn highlights(&self) -> &[Vec<MatchRange>] {
        &self.highlights
    }
}

impl Default for HighlightState {
    fn default() -> Self {
        Self::new()
    }
}

/// Compute all match ranges for a single line against a pattern.
///
/// Returns an empty vec if the line is empty or the pattern doesn't match.
/// Uses the pattern's `find_in` method which returns non-overlapping matches.
#[must_use]
pub fn find_matches_in_line(line: &str, pattern: &SearchPattern) -> Vec<MatchRange> {
    if line.is_empty() {
        return Vec::new();
    }
    pattern.find_in(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaseMode;

    fn pattern(pat: &str) -> SearchPattern {
        SearchPattern::compile(pat, CaseMode::Sensitive).expect("test pattern should compile")
    }

    // ── Test 1: find_matches_in_line with a simple pattern returns correct byte ranges
    #[test]
    fn test_find_matches_in_line_simple_pattern_returns_correct_ranges() {
        let pat = pattern("world");
        let matches = find_matches_in_line("hello world", &pat);
        assert_eq!(matches, vec![MatchRange { start: 6, end: 11 }]);
    }

    // ── Test 2: find_matches_in_line with multiple matches on one line returns all ranges
    #[test]
    fn test_find_matches_in_line_multiple_matches_returns_all() {
        let pat = pattern("ab");
        let matches = find_matches_in_line("ab cd ab ef ab", &pat);
        assert_eq!(
            matches,
            vec![
                MatchRange { start: 0, end: 2 },
                MatchRange { start: 6, end: 8 },
                MatchRange { start: 12, end: 14 },
            ]
        );
    }

    // ── Test 3: find_matches_in_line with no match returns empty vec
    #[test]
    fn test_find_matches_in_line_no_match_returns_empty() {
        let pat = pattern("xyz");
        let matches = find_matches_in_line("hello world", &pat);
        assert!(matches.is_empty());
    }

    // ── Test 4: find_matches_in_line with overlapping regex matches returns non-overlapping ranges
    #[test]
    fn test_find_matches_in_line_overlapping_regex_returns_non_overlapping() {
        // "aaa" with pattern "aa" should match non-overlapping: [0,2) only
        let pat = pattern("aa");
        let matches = find_matches_in_line("aaa", &pat);
        assert_eq!(matches, vec![MatchRange { start: 0, end: 2 }]);
    }

    // ── Test 5: find_matches_in_line on empty string returns empty vec
    #[test]
    fn test_find_matches_in_line_empty_string_returns_empty() {
        let pat = pattern("hello");
        let matches = find_matches_in_line("", &pat);
        assert!(matches.is_empty());
    }

    // ── Test 6: HighlightState::new starts with highlighting enabled
    #[test]
    fn test_highlight_state_new_starts_enabled() {
        let state = HighlightState::new();
        assert!(state.is_enabled());
    }

    // ── Test 7: HighlightState::toggle flips the state and returns the new value
    #[test]
    fn test_highlight_state_toggle_flips_and_returns_new_value() {
        let mut state = HighlightState::new();
        assert!(state.is_enabled());

        let new_state = state.toggle();
        assert!(!new_state);
        assert!(!state.is_enabled());

        let new_state = state.toggle();
        assert!(new_state);
        assert!(state.is_enabled());
    }

    // ── Test 8: compute_highlights with highlighting disabled returns empty vecs
    #[test]
    fn test_compute_highlights_disabled_returns_empty_vecs() {
        let mut state = HighlightState::new();
        state.set_enabled(false);
        let pat = pattern("hello");
        let lines = vec![
            Some("hello world".to_string()),
            Some("hello again".to_string()),
        ];
        let highlights = state.compute_highlights(&lines, Some(&pat));
        assert_eq!(highlights.len(), 2);
        assert!(highlights[0].is_empty());
        assert!(highlights[1].is_empty());
    }

    // ── Test 9: compute_highlights with a pattern produces correct ranges for each visible line
    #[test]
    fn test_compute_highlights_with_pattern_produces_correct_ranges() {
        let mut state = HighlightState::new();
        let pat = pattern("hello");
        let lines = vec![
            Some("hello world".to_string()),
            Some("no match here".to_string()),
            Some("say hello".to_string()),
        ];
        let highlights = state.compute_highlights(&lines, Some(&pat));
        assert_eq!(highlights.len(), 3);
        assert_eq!(highlights[0], vec![MatchRange { start: 0, end: 5 }]);
        assert!(highlights[1].is_empty());
        assert_eq!(highlights[2], vec![MatchRange { start: 4, end: 9 }]);
    }

    // ── Test 10: compute_highlights with None pattern returns empty vecs
    #[test]
    fn test_compute_highlights_none_pattern_returns_empty_vecs() {
        let mut state = HighlightState::new();
        let lines = vec![Some("hello".to_string()), Some("world".to_string())];
        let highlights = state.compute_highlights(&lines, None);
        assert_eq!(highlights.len(), 2);
        assert!(highlights[0].is_empty());
        assert!(highlights[1].is_empty());
    }

    // ── Test 11: clear empties the cached highlights
    #[test]
    fn test_clear_empties_cached_highlights() {
        let mut state = HighlightState::new();
        let pat = pattern("hello");
        let lines = vec![Some("hello".to_string())];
        state.compute_highlights(&lines, Some(&pat));
        assert!(!state.highlights().is_empty());

        state.clear();
        assert!(state.highlights().is_empty());
    }
}
