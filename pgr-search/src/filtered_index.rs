//! Filtered line index mapping visible lines to actual buffer lines.

use pgr_core::{Buffer, LineIndex};

use crate::FilterState;

/// An iterator-like helper that yields only visible line numbers
/// according to the active filter.
///
/// Maintains a mapping from "filtered line N" to "actual buffer line M",
/// enabling the display layer to iterate over only visible lines while
/// preserving the ability to translate between coordinate systems.
pub struct FilteredLines {
    /// Map from "filtered line N" to "actual buffer line M".
    visible_lines: Vec<usize>,
}

impl FilteredLines {
    /// Build the filtered line mapping from the buffer and filter state.
    ///
    /// Scans all lines in the buffer and retains only those passing the filter.
    /// When no filter is active, all lines are included.
    ///
    /// # Errors
    ///
    /// Returns an error if indexing or reading lines from the buffer fails.
    pub fn build(
        buffer: &dyn Buffer,
        index: &mut LineIndex,
        filter: &FilterState,
    ) -> crate::Result<Self> {
        index.index_all(buffer)?;
        let total = index.lines_indexed();
        let mut visible_lines = Vec::with_capacity(total);

        for line_num in 0..total {
            let content = index.get_line(line_num, buffer)?;
            let text = content.as_deref().unwrap_or("");
            if filter.is_visible(text) {
                visible_lines.push(line_num);
            }
        }

        Ok(Self { visible_lines })
    }

    /// Return the actual buffer line number for the N-th visible line.
    ///
    /// Returns `None` if `filtered_index` is out of range.
    #[must_use]
    pub fn actual_line(&self, filtered_index: usize) -> Option<usize> {
        self.visible_lines.get(filtered_index).copied()
    }

    /// Return the total number of visible lines.
    #[must_use]
    pub fn visible_count(&self) -> usize {
        self.visible_lines.len()
    }

    /// Return the filtered index for a given actual line number, or `None`
    /// if that line is filtered out.
    #[must_use]
    pub fn filtered_index(&self, actual_line: usize) -> Option<usize> {
        self.visible_lines
            .iter()
            .position(|&line| line == actual_line)
    }

    /// Return all visible line numbers.
    #[must_use]
    pub fn visible_lines(&self) -> &[usize] {
        &self.visible_lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CaseMode, SearchPattern};

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

    fn compile(pat: &str) -> SearchPattern {
        SearchPattern::compile(pat, CaseMode::Sensitive).expect("test pattern should compile")
    }

    // ── Test 8: build with no filter includes all lines ─────────────
    #[test]
    fn test_filtered_lines_build_no_filter_includes_all() {
        let (buf, mut idx) = make_buffer(&["alpha", "beta", "gamma"]);
        let filter = FilterState::new();
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();
        assert_eq!(filtered.visible_count(), 3);
        assert_eq!(filtered.visible_lines(), &[0, 1, 2]);
    }

    // ── Test 9: build with a pattern includes only matching lines ───
    #[test]
    fn test_filtered_lines_build_with_pattern_includes_matching() {
        let (buf, mut idx) =
            make_buffer(&["error: bad", "info: ok", "error: worse", "debug: fine"]);
        let mut filter = FilterState::new();
        filter.set_pattern(Some(compile("error")));
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();
        assert_eq!(filtered.visible_count(), 2);
        assert_eq!(filtered.visible_lines(), &[0, 2]);
    }

    // ── Test 10: build with inverted filter includes only non-matching
    #[test]
    fn test_filtered_lines_build_inverted_includes_non_matching() {
        let (buf, mut idx) =
            make_buffer(&["error: bad", "info: ok", "error: worse", "debug: fine"]);
        let mut filter = FilterState::new();
        filter.set_pattern(Some(compile("error")));
        filter.set_inverted(true);
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();
        assert_eq!(filtered.visible_count(), 2);
        assert_eq!(filtered.visible_lines(), &[1, 3]);
    }

    // ── Test 11: actual_line maps filtered index to correct buffer line
    #[test]
    fn test_filtered_lines_actual_line_maps_correctly() {
        let (buf, mut idx) =
            make_buffer(&["error: bad", "info: ok", "error: worse", "debug: fine"]);
        let mut filter = FilterState::new();
        filter.set_pattern(Some(compile("error")));
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();

        assert_eq!(filtered.actual_line(0), Some(0));
        assert_eq!(filtered.actual_line(1), Some(2));
        assert_eq!(filtered.actual_line(2), None);
    }

    // ── Test 12: filtered_index maps buffer line to correct filtered index
    #[test]
    fn test_filtered_lines_filtered_index_maps_correctly() {
        let (buf, mut idx) =
            make_buffer(&["error: bad", "info: ok", "error: worse", "debug: fine"]);
        let mut filter = FilterState::new();
        filter.set_pattern(Some(compile("error")));
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();

        assert_eq!(filtered.filtered_index(0), Some(0));
        assert_eq!(filtered.filtered_index(2), Some(1));
    }

    // ── Test 13: filtered_index returns None for filtered-out lines ──
    #[test]
    fn test_filtered_lines_filtered_index_returns_none_for_hidden() {
        let (buf, mut idx) =
            make_buffer(&["error: bad", "info: ok", "error: worse", "debug: fine"]);
        let mut filter = FilterState::new();
        filter.set_pattern(Some(compile("error")));
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();

        assert_eq!(filtered.filtered_index(1), None);
        assert_eq!(filtered.filtered_index(3), None);
    }

    // ── Empty buffer ────────────────────────────────────────────────
    #[test]
    fn test_filtered_lines_build_empty_buffer_returns_empty() {
        let buf = TestBuffer::new(b"");
        let mut idx = LineIndex::new(0);
        let filter = FilterState::new();
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();
        assert_eq!(filtered.visible_count(), 0);
        assert!(filtered.visible_lines().is_empty());
    }

    // ── All lines filtered out ──────────────────────────────────────
    #[test]
    fn test_filtered_lines_build_all_filtered_out_returns_empty() {
        let (buf, mut idx) = make_buffer(&["info: ok", "debug: fine"]);
        let mut filter = FilterState::new();
        filter.set_pattern(Some(compile("error")));
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();
        assert_eq!(filtered.visible_count(), 0);
        assert!(filtered.visible_lines().is_empty());
    }

    // ── All lines match ─────────────────────────────────────────────
    #[test]
    fn test_filtered_lines_build_all_match_includes_all() {
        let (buf, mut idx) = make_buffer(&["error: one", "error: two", "error: three"]);
        let mut filter = FilterState::new();
        filter.set_pattern(Some(compile("error")));
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();
        assert_eq!(filtered.visible_count(), 3);
        assert_eq!(filtered.visible_lines(), &[0, 1, 2]);
    }

    // ── actual_line out of bounds ───────────────────────────────────
    #[test]
    fn test_filtered_lines_actual_line_out_of_bounds_returns_none() {
        let (buf, mut idx) = make_buffer(&["alpha", "beta"]);
        let filter = FilterState::new();
        let filtered = FilteredLines::build(&buf, &mut idx, &filter).unwrap();
        assert_eq!(filtered.actual_line(5), None);
    }
}
