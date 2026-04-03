//! Search highlighting state and match computation.
//!
//! Provides [`HighlightState`] to track whether highlighting is enabled
//! (ESC-u toggle) and to compute match ranges for visible lines. Also
//! provides [`find_matches_in_line`] as a standalone utility.
//!
//! Supports multiple concurrent highlight patterns, each with a distinct
//! color index. The primary search pattern (from `/`) is always index 0.
//! Additional patterns added via `&+` use indices 1..7. The color palette
//! rotates through 8 SGR highlight colors.

use crate::{CaseMode, MatchRange, SearchPattern};

/// Maximum number of highlight patterns (primary + extras).
pub const MAX_HIGHLIGHT_PATTERNS: usize = 8;

/// SGR color palette for multi-pattern highlighting.
///
/// Index 0 is reverse video (the default for primary search).
/// Indices 1..7 use distinct background colors for visual distinction.
pub const HIGHLIGHT_COLORS: [&str; MAX_HIGHLIGHT_PATTERNS] = [
    "\x1b[7m",     // 0: reverse video (primary search)
    "\x1b[30;43m", // 1: black on yellow
    "\x1b[30;46m", // 2: black on cyan
    "\x1b[30;42m", // 3: black on green
    "\x1b[30;45m", // 4: black on magenta
    "\x1b[97;44m", // 5: white on blue
    "\x1b[30;41m", // 6: black on red
    "\x1b[30;47m", // 7: black on white
];

/// A byte-offset range with an associated color index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColoredHighlight {
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// Index into [`HIGHLIGHT_COLORS`].
    pub color_index: u8,
}

/// An extra highlight pattern with its assigned color index.
struct ExtraPattern {
    pattern: SearchPattern,
    color_index: u8,
}

/// State for search highlighting across visible lines.
///
/// Tracks the ESC-u toggle and caches computed highlight ranges for the
/// currently visible set of lines. The cache should be cleared on scroll,
/// search change, or any other event that invalidates the visible content.
///
/// Supports multiple concurrent patterns: the primary search pattern
/// (from `/`) is always color index 0, and extra patterns added via `&+`
/// use indices 1..7.
pub struct HighlightState {
    /// Whether highlighting is currently enabled (ESC-u toggle).
    enabled: bool,
    /// Cached highlights for the currently visible lines.
    /// Outer index corresponds to the visible line index (0 = first visible line).
    highlights: Vec<Vec<MatchRange>>,
    /// Cached multi-colored highlights for the currently visible lines.
    colored_highlights: Vec<Vec<ColoredHighlight>>,
    /// Extra highlight patterns (beyond the primary search pattern).
    extra_patterns: Vec<ExtraPattern>,
    /// Counter for assigning the next color index to new patterns.
    next_color: u8,
}

impl HighlightState {
    /// Create a new highlight state with highlighting enabled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            enabled: true,
            highlights: Vec::new(),
            colored_highlights: Vec::new(),
            extra_patterns: Vec::new(),
            next_color: 1,
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

    /// Add an extra highlight pattern with an automatically assigned color.
    ///
    /// Returns the color index assigned to the pattern, or `None` if the
    /// maximum number of extra patterns has been reached.
    ///
    /// # Errors
    ///
    /// Returns `SearchError::InvalidPattern` if the regex is invalid.
    pub fn add_pattern(
        &mut self,
        pattern_str: &str,
        case_mode: CaseMode,
    ) -> crate::Result<Option<u8>> {
        if self.extra_patterns.len() >= MAX_HIGHLIGHT_PATTERNS - 1 {
            return Ok(None);
        }
        let compiled = SearchPattern::compile(pattern_str, case_mode)?;
        let color_index = self.next_color;
        self.extra_patterns.push(ExtraPattern {
            pattern: compiled,
            color_index,
        });
        // MAX_HIGHLIGHT_PATTERNS is a small constant (8), so this cast is safe.
        #[allow(clippy::cast_possible_truncation)]
        let max_extra: u8 = MAX_HIGHLIGHT_PATTERNS as u8 - 1;
        self.next_color = (self.next_color % max_extra) + 1;
        self.clear();
        Ok(Some(color_index))
    }

    /// Remove an extra highlight pattern by its pattern string.
    ///
    /// Returns `true` if a pattern was found and removed.
    pub fn remove_pattern(&mut self, pattern_str: &str) -> bool {
        let before = self.extra_patterns.len();
        self.extra_patterns
            .retain(|ep| ep.pattern.pattern() != pattern_str);
        let removed = self.extra_patterns.len() < before;
        if removed {
            self.clear();
        }
        removed
    }

    /// List all extra highlight patterns with their color indices.
    ///
    /// Returns a vec of `(pattern_string, color_index)` pairs.
    #[must_use]
    pub fn list_patterns(&self) -> Vec<(&str, u8)> {
        self.extra_patterns
            .iter()
            .map(|ep| (ep.pattern.pattern(), ep.color_index))
            .collect()
    }

    /// Return the number of extra highlight patterns currently active.
    #[must_use]
    pub fn extra_pattern_count(&self) -> usize {
        self.extra_patterns.len()
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
        self.colored_highlights.clear();
        self.colored_highlights
            .resize(visible_lines.len(), Vec::new());

        if !self.enabled {
            return &self.highlights;
        }

        let has_primary = pattern.is_some();
        let has_extras = !self.extra_patterns.is_empty();

        if !has_primary && !has_extras {
            return &self.highlights;
        }

        for (i, line) in visible_lines.iter().enumerate() {
            if let Some(content) = line {
                // Primary pattern (color index 0).
                if let Some(pat) = pattern {
                    let matches = find_matches_in_line(content, pat);
                    for m in &matches {
                        self.colored_highlights[i].push(ColoredHighlight {
                            start: m.start,
                            end: m.end,
                            color_index: 0,
                        });
                    }
                    self.highlights[i] = matches;
                }

                // Extra patterns — first pattern in list wins on overlap.
                for ep in &self.extra_patterns {
                    let matches = find_matches_in_line(content, &ep.pattern);
                    for m in &matches {
                        if !overlaps_existing(&self.colored_highlights[i], m.start, m.end) {
                            self.colored_highlights[i].push(ColoredHighlight {
                                start: m.start,
                                end: m.end,
                                color_index: ep.color_index,
                            });
                        }
                    }
                }

                // Sort colored highlights by start position for stable rendering.
                self.colored_highlights[i].sort_by_key(|h| h.start);
            }
        }

        &self.highlights
    }

    /// Clear cached highlights (call after scroll, search change, etc.).
    pub fn clear(&mut self) {
        self.highlights.clear();
        self.colored_highlights.clear();
    }

    /// Return the cached highlights for visible lines.
    #[must_use]
    pub fn highlights(&self) -> &[Vec<MatchRange>] {
        &self.highlights
    }

    /// Return the cached multi-colored highlights for visible lines.
    ///
    /// Each inner vec contains sorted, non-overlapping highlight ranges
    /// with color indices. Empty if highlighting is disabled or no
    /// patterns are active.
    #[must_use]
    pub fn colored_highlights(&self) -> &[Vec<ColoredHighlight>] {
        &self.colored_highlights
    }

    /// Return whether any extra highlight patterns are active.
    #[must_use]
    pub fn has_extra_patterns(&self) -> bool {
        !self.extra_patterns.is_empty()
    }
}

/// Check whether a new range `[start, end)` overlaps any existing highlight.
fn overlaps_existing(existing: &[ColoredHighlight], start: usize, end: usize) -> bool {
    existing.iter().any(|h| start < h.end && end > h.start)
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

    // ── Test 12: add_pattern creates extra highlight with assigned color index
    #[test]
    fn test_add_pattern_assigns_color_index() {
        let mut state = HighlightState::new();
        let idx = state.add_pattern("test", CaseMode::Sensitive).unwrap();
        assert_eq!(idx, Some(1));
        assert_eq!(state.extra_pattern_count(), 1);
    }

    // ── Test 13: add_pattern increments color index for each new pattern
    #[test]
    fn test_add_pattern_increments_color_index() {
        let mut state = HighlightState::new();
        let idx1 = state.add_pattern("pat1", CaseMode::Sensitive).unwrap();
        let idx2 = state.add_pattern("pat2", CaseMode::Sensitive).unwrap();
        assert_eq!(idx1, Some(1));
        assert_eq!(idx2, Some(2));
    }

    // ── Test 14: add_pattern returns None when max patterns reached
    #[test]
    fn test_add_pattern_max_returns_none() {
        let mut state = HighlightState::new();
        for i in 0..MAX_HIGHLIGHT_PATTERNS - 1 {
            let result = state
                .add_pattern(&format!("pat{i}"), CaseMode::Sensitive)
                .unwrap();
            assert!(result.is_some());
        }
        let result = state.add_pattern("overflow", CaseMode::Sensitive).unwrap();
        assert!(result.is_none());
    }

    // ── Test 15: remove_pattern removes by pattern string
    #[test]
    fn test_remove_pattern_by_string() {
        let mut state = HighlightState::new();
        state.add_pattern("hello", CaseMode::Sensitive).unwrap();
        state.add_pattern("world", CaseMode::Sensitive).unwrap();
        assert_eq!(state.extra_pattern_count(), 2);
        assert!(state.remove_pattern("hello"));
        assert_eq!(state.extra_pattern_count(), 1);
        assert!(!state.remove_pattern("nonexistent"));
    }

    // ── Test 16: list_patterns returns pattern strings and color indices
    #[test]
    fn test_list_patterns_returns_strings_and_indices() {
        let mut state = HighlightState::new();
        state.add_pattern("alpha", CaseMode::Sensitive).unwrap();
        state.add_pattern("beta", CaseMode::Sensitive).unwrap();
        let list = state.list_patterns();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].0, "alpha");
        assert_eq!(list[0].1, 1);
        assert_eq!(list[1].0, "beta");
        assert_eq!(list[1].1, 2);
    }

    // ── Test 17: compute_highlights with extra patterns produces colored highlights
    #[test]
    fn test_compute_highlights_with_extras_produces_colored() {
        let mut state = HighlightState::new();
        let primary = pattern("hello");
        state.add_pattern("world", CaseMode::Sensitive).unwrap();
        let lines = vec![Some("hello world".to_string())];
        state.compute_highlights(&lines, Some(&primary));
        let colored = state.colored_highlights();
        assert_eq!(colored.len(), 1);
        assert_eq!(colored[0].len(), 2);
        // Primary: "hello" at [0,5) with color 0
        assert_eq!(
            colored[0][0],
            ColoredHighlight {
                start: 0,
                end: 5,
                color_index: 0
            }
        );
        // Extra: "world" at [6,11) with color 1
        assert_eq!(
            colored[0][1],
            ColoredHighlight {
                start: 6,
                end: 11,
                color_index: 1
            }
        );
    }

    // ── Test 18: overlapping extra pattern does not overwrite primary
    #[test]
    fn test_colored_highlights_primary_wins_on_overlap() {
        let mut state = HighlightState::new();
        let primary = pattern("hello");
        // This extra pattern overlaps the primary.
        state.add_pattern("hell", CaseMode::Sensitive).unwrap();
        let lines = vec![Some("hello world".to_string())];
        state.compute_highlights(&lines, Some(&primary));
        let colored = state.colored_highlights();
        // Only the primary should appear for the overlapping region.
        assert_eq!(colored[0].len(), 1);
        assert_eq!(colored[0][0].color_index, 0);
    }

    // ── Test 19: extra patterns work without a primary pattern
    #[test]
    fn test_extra_patterns_without_primary() {
        let mut state = HighlightState::new();
        state.add_pattern("world", CaseMode::Sensitive).unwrap();
        let lines = vec![Some("hello world".to_string())];
        state.compute_highlights(&lines, None);
        let colored = state.colored_highlights();
        assert_eq!(colored[0].len(), 1);
        assert_eq!(
            colored[0][0],
            ColoredHighlight {
                start: 6,
                end: 11,
                color_index: 1
            }
        );
    }

    // ── Test 20: colored_highlights empty when disabled
    #[test]
    fn test_colored_highlights_empty_when_disabled() {
        let mut state = HighlightState::new();
        state.set_enabled(false);
        state.add_pattern("world", CaseMode::Sensitive).unwrap();
        let lines = vec![Some("hello world".to_string())];
        state.compute_highlights(&lines, None);
        let colored = state.colored_highlights();
        assert_eq!(colored.len(), 1);
        assert!(colored[0].is_empty());
    }

    // ── Test 21: has_extra_patterns returns correct state
    #[test]
    fn test_has_extra_patterns() {
        let mut state = HighlightState::new();
        assert!(!state.has_extra_patterns());
        state.add_pattern("test", CaseMode::Sensitive).unwrap();
        assert!(state.has_extra_patterns());
        state.remove_pattern("test");
        assert!(!state.has_extra_patterns());
    }
}
