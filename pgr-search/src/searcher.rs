//! Forward and backward search across a buffer's lines.

use pgr_core::{Buffer, LineIndex};

use crate::{Result, SearchPattern};

/// Direction of search traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchDirection {
    /// Search from the current position toward the end of the buffer.
    Forward,
    /// Search from the current position toward the beginning of the buffer.
    Backward,
}

/// Configuration for wrap-around behavior when a search hits a boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    /// Stop at the boundary (beginning or end of buffer).
    NoWrap,
    /// Wrap around and continue searching from the opposite end.
    Wrap,
}

/// Drives search across a buffer's lines using a compiled pattern.
///
/// Holds a [`SearchPattern`] and search configuration (direction, wrap mode).
/// The `/`, `?`, `n`, and `N` commands will call through this struct.
pub struct Searcher {
    pattern: SearchPattern,
    direction: SearchDirection,
    wrap: WrapMode,
    /// When `true`, matches are inverted: lines that do NOT match the
    /// pattern are considered hits.
    inverted: bool,
}

impl Searcher {
    /// Create a new searcher with the given compiled pattern and direction.
    ///
    /// Wrap mode defaults to [`WrapMode::NoWrap`]; inverted defaults to `false`.
    #[must_use]
    pub fn new(pattern: SearchPattern, direction: SearchDirection) -> Self {
        Self {
            pattern,
            direction,
            wrap: WrapMode::NoWrap,
            inverted: false,
        }
    }

    /// Set whether searches wrap around at buffer boundaries.
    pub fn set_wrap(&mut self, wrap: WrapMode) {
        self.wrap = wrap;
    }

    /// Set whether the match is inverted (find lines that do NOT match).
    pub fn set_inverted(&mut self, inverted: bool) {
        self.inverted = inverted;
    }

    /// Return whether the searcher is in inverted mode.
    #[must_use]
    pub fn is_inverted(&self) -> bool {
        self.inverted
    }

    /// Return the current search direction.
    #[must_use]
    pub fn direction(&self) -> SearchDirection {
        self.direction
    }

    /// Return a reference to the compiled pattern.
    #[must_use]
    pub fn pattern(&self) -> &SearchPattern {
        &self.pattern
    }

    /// Search forward from `start_line` (exclusive — starts checking at `start_line + 1`).
    ///
    /// Returns the zero-based line number of the first matching line, or `None`
    /// if no match is found.
    ///
    /// Reads lines from `buffer` via `index`. If wrap mode is [`WrapMode::Wrap`]
    /// and no match is found before EOF, continues from line 0 up to (but not
    /// including) `start_line`.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer or indexing lines fails.
    pub fn search_forward(
        &self,
        start_line: usize,
        buffer: &dyn Buffer,
        index: &mut LineIndex,
    ) -> Result<Option<usize>> {
        index.index_all(buffer)?;
        let total = index.lines_indexed();

        if total == 0 {
            return Ok(None);
        }

        // Phase 1: search from start_line + 1 to end.
        for line in (start_line + 1)..total {
            if self.line_matches(line, buffer, index)? {
                return Ok(Some(line));
            }
        }

        // Phase 2: wrap around if enabled.
        if self.wrap == WrapMode::Wrap {
            let upper = start_line.min(total);
            for line in 0..upper {
                if self.line_matches(line, buffer, index)? {
                    return Ok(Some(line));
                }
            }
        }

        Ok(None)
    }

    /// Search backward from `start_line` (exclusive — starts checking at `start_line - 1`).
    ///
    /// Returns the zero-based line number of the first matching line, or `None`
    /// if no match is found.
    ///
    /// If wrap mode is [`WrapMode::Wrap`] and no match is found before line 0,
    /// continues from the last line down to (but not including) `start_line`.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer or indexing lines fails.
    pub fn search_backward(
        &self,
        start_line: usize,
        buffer: &dyn Buffer,
        index: &mut LineIndex,
    ) -> Result<Option<usize>> {
        index.index_all(buffer)?;
        let total = index.lines_indexed();

        if total == 0 || start_line == 0 {
            // If start_line is 0, there's nothing before it in phase 1.
            // But we may still wrap.
            if start_line == 0 && total > 0 && self.wrap == WrapMode::Wrap {
                // Wrap: search from last line down to (not including) start_line.
                for line in (1..total).rev() {
                    if self.line_matches(line, buffer, index)? {
                        return Ok(Some(line));
                    }
                }
            }
            return Ok(None);
        }

        // Phase 1: search from start_line - 1 down to 0.
        let first_check = (start_line - 1).min(total - 1);
        for line in (0..=first_check).rev() {
            if self.line_matches(line, buffer, index)? {
                return Ok(Some(line));
            }
        }

        // Phase 2: wrap around if enabled.
        if self.wrap == WrapMode::Wrap {
            let lower = (start_line + 1).min(total);
            for line in (lower..total).rev() {
                if self.line_matches(line, buffer, index)? {
                    return Ok(Some(line));
                }
            }
        }

        Ok(None)
    }

    /// Search for the N-th match in the configured direction from `start_line`.
    ///
    /// Returns the line number of the N-th match, or `None` if fewer than N
    /// matches exist.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer or indexing lines fails.
    pub fn search_nth(
        &self,
        start_line: usize,
        n: usize,
        buffer: &dyn Buffer,
        index: &mut LineIndex,
    ) -> Result<Option<usize>> {
        let mut current = start_line;
        for _ in 0..n {
            let result = match self.direction {
                SearchDirection::Forward => self.search_forward(current, buffer, index)?,
                SearchDirection::Backward => self.search_backward(current, buffer, index)?,
            };
            match result {
                Some(line) => current = line,
                None => return Ok(None),
            }
        }
        if n == 0 {
            return Ok(None);
        }
        Ok(Some(current))
    }

    /// Check if a single line matches the pattern.
    ///
    /// When `self.inverted` is `true`, returns `true` for lines that do
    /// NOT match the pattern.
    fn line_matches(
        &self,
        line: usize,
        buffer: &dyn Buffer,
        index: &mut LineIndex,
    ) -> Result<bool> {
        let content = index.get_line(line, buffer)?;
        let matched = content.is_some_and(|text| self.pattern.is_match(&text));
        Ok(if self.inverted { !matched } else { matched })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CaseMode;

    /// A simple in-memory buffer for tests.
    struct TestBuffer {
        data: Vec<u8>,
    }

    impl TestBuffer {
        fn new(data: &[u8]) -> Self {
            Self {
                data: data.to_vec(),
            }
        }
    }

    impl Buffer for TestBuffer {
        fn len(&self) -> usize {
            self.data.len()
        }

        fn read_at(&self, offset: usize, buf: &mut [u8]) -> pgr_core::Result<usize> {
            if offset >= self.data.len() {
                return Ok(0);
            }
            let available = &self.data[offset..];
            let to_copy = available.len().min(buf.len());
            buf[..to_copy].copy_from_slice(&available[..to_copy]);
            Ok(to_copy)
        }

        fn is_growable(&self) -> bool {
            false
        }

        fn refresh(&mut self) -> pgr_core::Result<usize> {
            Ok(self.data.len())
        }
    }

    /// Helper: create a buffer and index from lines of text.
    fn make_buffer(lines: &[&str]) -> (TestBuffer, LineIndex) {
        let mut data = Vec::new();
        for line in lines {
            data.extend_from_slice(line.as_bytes());
            data.push(b'\n');
        }
        let len = data.len() as u64;
        let buf = TestBuffer::new(&data);
        let idx = LineIndex::new(len);
        (buf, idx)
    }

    fn pattern(pat: &str) -> SearchPattern {
        SearchPattern::compile(pat, CaseMode::Sensitive).expect("test pattern should compile")
    }

    // ── Test 1: search_forward with one match returns correct line ────
    #[test]
    fn test_search_forward_one_match_returns_correct_line() {
        let (buf, mut idx) = make_buffer(&["alpha", "beta", "gamma"]);
        let searcher = Searcher::new(pattern("gamma"), SearchDirection::Forward);
        let result = searcher.search_forward(0, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(2));
    }

    // ── Test 2: search_forward with no matches returns None ───────────
    #[test]
    fn test_search_forward_no_match_returns_none() {
        let (buf, mut idx) = make_buffer(&["alpha", "beta", "gamma"]);
        let searcher = Searcher::new(pattern("delta"), SearchDirection::Forward);
        let result = searcher.search_forward(0, &buf, &mut idx).unwrap();
        assert_eq!(result, None);
    }

    // ── Test 3: search_forward starts after start_line (exclusive) ────
    #[test]
    fn test_search_forward_exclusive_start_does_not_match_start_line() {
        let (buf, mut idx) = make_buffer(&["match", "no", "no"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        // Start at line 0 which matches, but search should start at line 1.
        let result = searcher.search_forward(0, &buf, &mut idx).unwrap();
        assert_eq!(result, None);
    }

    // ── Test 4: search_forward with multiple matches returns first ────
    #[test]
    fn test_search_forward_multiple_matches_returns_first_after_start() {
        let (buf, mut idx) = make_buffer(&["a", "match", "b", "match", "c"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        let result = searcher.search_forward(0, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(1));
    }

    // ── Test 5: search_backward with one match returns correct line ───
    #[test]
    fn test_search_backward_one_match_returns_correct_line() {
        let (buf, mut idx) = make_buffer(&["alpha", "beta", "gamma"]);
        let searcher = Searcher::new(pattern("alpha"), SearchDirection::Backward);
        let result = searcher.search_backward(2, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(0));
    }

    // ── Test 6: search_backward with no matches returns None ──────────
    #[test]
    fn test_search_backward_no_match_returns_none() {
        let (buf, mut idx) = make_buffer(&["alpha", "beta", "gamma"]);
        let searcher = Searcher::new(pattern("delta"), SearchDirection::Backward);
        let result = searcher.search_backward(2, &buf, &mut idx).unwrap();
        assert_eq!(result, None);
    }

    // ── Test 7: search_backward starts before start_line (exclusive) ──
    #[test]
    fn test_search_backward_exclusive_start_does_not_match_start_line() {
        let (buf, mut idx) = make_buffer(&["no", "no", "match"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Backward);
        // Start at line 2 which matches, but search should start at line 1.
        let result = searcher.search_backward(2, &buf, &mut idx).unwrap();
        assert_eq!(result, None);
    }

    // ── Test 8: search_backward with multiple matches returns closest ─
    #[test]
    fn test_search_backward_multiple_matches_returns_closest_before_start() {
        let (buf, mut idx) = make_buffer(&["match", "a", "match", "b", "c"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Backward);
        let result = searcher.search_backward(4, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(2));
    }

    // ── Test 9: search_forward with Wrap wraps from EOF to line 0 ─────
    #[test]
    fn test_search_forward_wrap_wraps_from_eof_to_beginning() {
        let (buf, mut idx) = make_buffer(&["match", "a", "b"]);
        let mut searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        searcher.set_wrap(WrapMode::Wrap);
        // Start at line 1; forward search checks 2, then wraps to 0.
        let result = searcher.search_forward(1, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(0));
    }

    // ── Test 10: search_backward with Wrap wraps from line 0 to EOF ───
    #[test]
    fn test_search_backward_wrap_wraps_from_beginning_to_end() {
        let (buf, mut idx) = make_buffer(&["a", "b", "match"]);
        let mut searcher = Searcher::new(pattern("match"), SearchDirection::Backward);
        searcher.set_wrap(WrapMode::Wrap);
        // Start at line 1; backward search checks 0, then wraps to end.
        let result = searcher.search_backward(1, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(2));
    }

    // ── Test 11: search_forward with NoWrap does not wrap ─────────────
    #[test]
    fn test_search_forward_nowrap_does_not_wrap() {
        let (buf, mut idx) = make_buffer(&["match", "a", "b"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        // match is at line 0, start at line 1 — forward can't find it without wrap.
        let result = searcher.search_forward(1, &buf, &mut idx).unwrap();
        assert_eq!(result, None);
    }

    // ── Test 12: search_nth with N=1 behaves like single search ───────
    #[test]
    fn test_search_nth_one_behaves_like_single_search() {
        let (buf, mut idx) = make_buffer(&["a", "match", "b"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        let result = searcher.search_nth(0, 1, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(1));
    }

    // ── Test 13: search_nth with N=3 returns third match ──────────────
    #[test]
    fn test_search_nth_three_returns_third_match() {
        let (buf, mut idx) = make_buffer(&["a", "match1", "b", "match2", "c", "match3", "d"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        let result = searcher.search_nth(0, 3, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(5));
    }

    // ── Test 14: search_nth with N > total matches returns None ───────
    #[test]
    fn test_search_nth_exceeds_total_matches_returns_none() {
        let (buf, mut idx) = make_buffer(&["a", "match", "b"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        let result = searcher.search_nth(0, 5, &buf, &mut idx).unwrap();
        assert_eq!(result, None);
    }

    // ── Test 15: search on empty buffer returns None ──────────────────
    #[test]
    fn test_search_empty_buffer_returns_none() {
        let buf = TestBuffer::new(b"");
        let mut idx = LineIndex::new(0);
        let searcher = Searcher::new(pattern("anything"), SearchDirection::Forward);

        let fwd = searcher.search_forward(0, &buf, &mut idx).unwrap();
        assert_eq!(fwd, None);

        let bwd = searcher.search_backward(0, &buf, &mut idx).unwrap();
        assert_eq!(bwd, None);
    }

    // ── Additional: accessor tests ────────────────────────────────────

    #[test]
    fn test_searcher_direction_returns_configured_direction() {
        let searcher = Searcher::new(pattern("x"), SearchDirection::Forward);
        assert_eq!(searcher.direction(), SearchDirection::Forward);

        let searcher = Searcher::new(pattern("x"), SearchDirection::Backward);
        assert_eq!(searcher.direction(), SearchDirection::Backward);
    }

    #[test]
    fn test_searcher_pattern_returns_pattern_reference() {
        let searcher = Searcher::new(pattern("hello"), SearchDirection::Forward);
        assert_eq!(searcher.pattern().pattern(), "hello");
    }

    #[test]
    fn test_searcher_set_wrap_changes_wrap_mode() {
        let (buf, mut idx) = make_buffer(&["match", "a", "b"]);
        let mut searcher = Searcher::new(pattern("match"), SearchDirection::Forward);

        // Default is NoWrap — should not find match at line 0 when starting at 1.
        let result = searcher.search_forward(1, &buf, &mut idx).unwrap();
        assert_eq!(result, None);

        // After setting Wrap, should find it.
        searcher.set_wrap(WrapMode::Wrap);
        let result = searcher.search_forward(1, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(0));
    }

    // ── search_nth backward ───────────────────────────────────────────

    #[test]
    fn test_search_nth_backward_returns_correct_match() {
        let (buf, mut idx) = make_buffer(&["match1", "a", "match2", "b", "match3", "c"]);
        let searcher = Searcher::new(pattern("match"), SearchDirection::Backward);
        let result = searcher.search_nth(5, 2, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(2));
    }

    // ── Inverted search ──────────────────────────────────────────────

    #[test]
    fn test_search_forward_inverted_finds_non_matching_lines() {
        let (buf, mut idx) = make_buffer(&["match", "match", "other", "match"]);
        let mut searcher = Searcher::new(pattern("match"), SearchDirection::Forward);
        searcher.set_inverted(true);
        // Starting at line 0, inverted search should find line 2 ("other").
        let result = searcher.search_forward(0, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(2));
    }

    #[test]
    fn test_search_backward_inverted_finds_non_matching_lines() {
        let (buf, mut idx) = make_buffer(&["other", "match", "match", "match"]);
        let mut searcher = Searcher::new(pattern("match"), SearchDirection::Backward);
        searcher.set_inverted(true);
        let result = searcher.search_backward(3, &buf, &mut idx).unwrap();
        assert_eq!(result, Some(0));
    }

    #[test]
    fn test_searcher_is_inverted_default_false() {
        let searcher = Searcher::new(pattern("x"), SearchDirection::Forward);
        assert!(!searcher.is_inverted());
    }

    #[test]
    fn test_searcher_set_inverted_changes_flag() {
        let mut searcher = Searcher::new(pattern("x"), SearchDirection::Forward);
        searcher.set_inverted(true);
        assert!(searcher.is_inverted());
    }
}
