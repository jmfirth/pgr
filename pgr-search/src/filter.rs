//! Filter state for the `&` command (show only matching/non-matching lines).

use crate::SearchPattern;

/// Filter state determining which lines are visible.
///
/// When a filter is active, only lines matching (or not matching, if inverted)
/// the pattern are displayed. This implements the `&` command from `less`.
pub struct FilterState {
    /// The active filter pattern, or `None` if no filter is active.
    pattern: Option<SearchPattern>,
    /// Whether the filter is inverted (show non-matching lines).
    inverted: bool,
}

impl FilterState {
    /// Create a new inactive filter state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pattern: None,
            inverted: false,
        }
    }

    /// Set the filter pattern. Pass `None` to clear the filter.
    pub fn set_pattern(&mut self, pattern: Option<SearchPattern>) {
        self.pattern = pattern;
    }

    /// Set whether the filter is inverted.
    pub fn set_inverted(&mut self, inverted: bool) {
        self.inverted = inverted;
    }

    /// Return the current filter pattern, if any.
    #[must_use]
    pub fn pattern(&self) -> Option<&SearchPattern> {
        self.pattern.as_ref()
    }

    /// Return whether filtering is currently active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.pattern.is_some()
    }

    /// Return whether the filter is inverted.
    #[must_use]
    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// Test whether a given line should be visible under the current filter.
    ///
    /// Returns `true` if the line should be shown:
    /// - No filter active: always returns `true`.
    /// - Normal filter: returns `true` if the line matches the pattern.
    /// - Inverted filter: returns `true` if the line does NOT match the pattern.
    #[must_use]
    pub fn is_visible(&self, line_content: &str) -> bool {
        let Some(pat) = &self.pattern else {
            return true;
        };
        let matches = pat.is_match(line_content);
        if self.inverted {
            !matches
        } else {
            matches
        }
    }

    /// Clear the filter (remove pattern, reset inversion).
    pub fn clear(&mut self) {
        self.pattern = None;
        self.inverted = false;
    }
}

impl Default for FilterState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaseMode;

    fn compile(pat: &str) -> SearchPattern {
        SearchPattern::compile(pat, CaseMode::Sensitive).expect("test pattern should compile")
    }

    // ── Test 1: FilterState::new() starts with no active filter ─────
    #[test]
    fn test_filter_state_new_starts_inactive() {
        let state = FilterState::new();
        assert!(!state.is_active());
        assert!(state.pattern().is_none());
        assert!(!state.is_inverted());
    }

    // ── Test 2: is_visible with no filter returns true for any line ─
    #[test]
    fn test_filter_state_no_filter_is_visible_returns_true() {
        let state = FilterState::new();
        assert!(state.is_visible("anything at all"));
        assert!(state.is_visible(""));
        assert!(state.is_visible("error: something failed"));
    }

    // ── Test 3: is_visible with a pattern returns true for matching ─
    #[test]
    fn test_filter_state_pattern_is_visible_true_for_matching() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("error")));
        assert!(state.is_visible("error: something failed"));
        assert!(state.is_visible("an error occurred"));
    }

    // ── Test 4: is_visible with a pattern returns false for non-matching
    #[test]
    fn test_filter_state_pattern_is_visible_false_for_non_matching() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("error")));
        assert!(!state.is_visible("info: all good"));
        assert!(!state.is_visible("warning: check this"));
    }

    // ── Test 5: is_visible inverted returns true for non-matching ───
    #[test]
    fn test_filter_state_inverted_is_visible_true_for_non_matching() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("error")));
        state.set_inverted(true);
        assert!(state.is_visible("info: all good"));
        assert!(state.is_visible("warning: check this"));
    }

    // ── Test 6: is_visible inverted returns false for matching ──────
    #[test]
    fn test_filter_state_inverted_is_visible_false_for_matching() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("error")));
        state.set_inverted(true);
        assert!(!state.is_visible("error: something failed"));
        assert!(!state.is_visible("an error occurred"));
    }

    // ── Test 7: clear removes the filter and resets inversion ───────
    #[test]
    fn test_filter_state_clear_removes_filter_and_resets_inversion() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("error")));
        state.set_inverted(true);
        assert!(state.is_active());
        assert!(state.is_inverted());

        state.clear();
        assert!(!state.is_active());
        assert!(!state.is_inverted());
        assert!(state.pattern().is_none());
        // After clear, all lines are visible again.
        assert!(state.is_visible("anything"));
    }

    // ── Test 15: Empty filter pattern clears an active filter ───────
    #[test]
    fn test_filter_state_set_pattern_none_clears_filter() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("error")));
        assert!(state.is_active());

        state.set_pattern(None);
        assert!(!state.is_active());
        assert!(state.is_visible("anything"));
    }

    // ── Default impl ────────────────────────────────────────────────
    #[test]
    fn test_filter_state_default_is_inactive() {
        let state = FilterState::default();
        assert!(!state.is_active());
    }

    // ── is_active returns true when pattern is set ──────────────────
    #[test]
    fn test_filter_state_is_active_with_pattern() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("test")));
        assert!(state.is_active());
    }

    // ── pattern() returns reference to active pattern ───────────────
    #[test]
    fn test_filter_state_pattern_returns_active_pattern() {
        let mut state = FilterState::new();
        state.set_pattern(Some(compile("hello")));
        let pat = state.pattern().expect("should have pattern");
        assert_eq!(pat.pattern(), "hello");
    }
}
