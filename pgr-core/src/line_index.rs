//! Lazy line index that maps line numbers to byte offsets in a [`Buffer`].

use crate::buffer::Buffer;
use crate::error::Result;

/// The number of bytes read per scan step when indexing newlines.
const SCAN_CHUNK_SIZE: usize = 64 * 1024;

/// A lazily-built index mapping line numbers to byte offsets within a buffer.
///
/// Lines are zero-indexed internally but the public API uses zero-based line
/// numbers (line 0 is the first line). The index records the byte offset where
/// each line begins and scans forward on demand, reading in 64 KiB chunks.
pub struct LineIndex {
    /// `offsets[n]` is the byte offset where line `n` starts.
    offsets: Vec<u64>,
    /// How far into the buffer we have scanned for newlines.
    scanned_to: u64,
    /// The total byte length of the buffer at last refresh.
    buffer_len: u64,
}

impl LineIndex {
    /// Creates a new line index for a buffer of the given byte length.
    ///
    /// If `buffer_len > 0`, the first line is assumed to start at offset 0.
    #[must_use]
    pub fn new(buffer_len: u64) -> Self {
        let mut offsets = Vec::new();
        if buffer_len > 0 {
            offsets.push(0);
        }
        Self {
            offsets,
            scanned_to: 0,
            buffer_len,
        }
    }

    /// Ensures the index has scanned far enough to know about `line_number`.
    ///
    /// Returns `Ok(true)` if the requested line exists, `Ok(false)` if the
    /// buffer does not contain that many lines.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer fails.
    pub fn ensure_line(&mut self, line_number: usize, buffer: &dyn Buffer) -> Result<bool> {
        while self.offsets.len() <= line_number && self.scanned_to < self.buffer_len {
            self.scan_chunk(buffer)?;
        }
        Ok(line_number < self.offsets.len())
    }

    /// Scans the entire buffer, indexing all line starts.
    ///
    /// After this call, [`lines_indexed`](Self::lines_indexed) returns the
    /// total number of lines.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer fails.
    pub fn index_all(&mut self, buffer: &dyn Buffer) -> Result<()> {
        while self.scanned_to < self.buffer_len {
            self.scan_chunk(buffer)?;
        }
        Ok(())
    }

    /// Returns the byte range `(start, end)` for a given line, where `end` is
    /// exclusive (one past the last byte of the line, including its terminator).
    ///
    /// Returns `None` if `line_number` is beyond the indexed lines. Note: this
    /// only checks already-indexed data; call [`ensure_line`](Self::ensure_line)
    /// first if the line might not be indexed yet.
    #[must_use]
    pub fn line_range(&self, line_number: usize) -> Option<(u64, u64)> {
        if line_number >= self.offsets.len() {
            return None;
        }
        let start = self.offsets[line_number];
        let end = if line_number + 1 < self.offsets.len() {
            self.offsets[line_number + 1]
        } else {
            // Last indexed line — extends to the end of scanned data.
            // If partially scanned, this is the current scan frontier;
            // if fully scanned, this equals buffer_len.
            if self.scanned_to >= self.buffer_len {
                self.buffer_len
            } else {
                self.scanned_to
            }
        };
        Some((start, end))
    }

    /// Reads the content of a line from the buffer, stripping trailing `\n`
    /// and `\r\n`.
    ///
    /// Returns `Ok(None)` if the line number is out of range.
    ///
    /// # Errors
    ///
    /// Returns an error if the line cannot be indexed or read from the buffer.
    pub fn get_line(&mut self, line_number: usize, buffer: &dyn Buffer) -> Result<Option<String>> {
        if !self.ensure_line(line_number, buffer)? {
            return Ok(None);
        }

        // Scan far enough to know the end of this line: either the next
        // line's start offset exists, or we've reached EOF.
        let _ = self.ensure_line(line_number + 1, buffer)?;

        let Some((start, end)) = self.line_range(line_number) else {
            return Ok(None);
        };

        let len = usize::try_from(end - start).unwrap_or(usize::MAX);
        let mut raw = vec![0u8; len];
        let offset = usize::try_from(start).unwrap_or(usize::MAX);
        let bytes_read = buffer.read_at(offset, &mut raw)?;
        raw.truncate(bytes_read);

        // Strip trailing newline variants.
        if raw.last() == Some(&b'\n') {
            raw.pop();
        }
        if raw.last() == Some(&b'\r') {
            raw.pop();
        }

        Ok(Some(String::from_utf8_lossy(&raw).into_owned()))
    }

    /// Returns the number of lines indexed so far.
    ///
    /// This may be less than the total number of lines if the buffer has not
    /// been fully scanned. Call [`index_all`](Self::index_all) first for a
    /// complete count.
    #[must_use]
    pub fn lines_indexed(&self) -> usize {
        self.offsets.len()
    }

    /// Scans the entire buffer (if needed) and returns the total number of
    /// lines.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer fails.
    pub fn total_lines(&mut self, buffer: &dyn Buffer) -> Result<usize> {
        self.index_all(buffer)?;
        Ok(self.offsets.len())
    }

    /// Updates the known buffer length so that new data can be scanned.
    ///
    /// Call this after [`Buffer::refresh`] reports a larger size. The index
    /// retains all previously scanned offsets and will scan the new region
    /// lazily on the next [`ensure_line`](Self::ensure_line) or
    /// [`index_all`](Self::index_all) call.
    ///
    /// If the buffer was previously empty and is now non-empty, the first
    /// line start (offset 0) is automatically recorded.
    pub fn update_buffer_len(&mut self, new_len: u64) {
        if new_len > self.buffer_len {
            // If the index was empty (buffer was zero-length before) and now has
            // data, seed the first line.
            if self.offsets.is_empty() && new_len > 0 {
                self.offsets.push(0);
            }
            // If the scan had completed to the old boundary, back up by one byte
            // so the last byte is re-processed. This handles the case where a
            // newline was at the end of the old buffer and its following line
            // start was suppressed by the `< buffer_len` guard in `scan_chunk`.
            if self.scanned_to == self.buffer_len && self.scanned_to > 0 {
                self.scanned_to -= 1;
            }
            self.buffer_len = new_len;
        }
    }

    /// Returns the zero-based line number containing the given byte offset.
    ///
    /// Scans forward as needed to find the line. Returns `Ok(None)` if the
    /// offset is at or beyond the buffer length.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer fails.
    pub fn line_at_offset(&mut self, offset: u64, buffer: &dyn Buffer) -> Result<Option<usize>> {
        if offset >= self.buffer_len {
            return Ok(None);
        }

        // Scan forward until we have enough indexed lines to cover `offset`.
        while self.scanned_to <= offset && self.scanned_to < self.buffer_len {
            self.scan_chunk(buffer)?;
        }

        // Binary search: find the last line whose start offset <= `offset`.
        let line = match self.offsets.binary_search(&offset) {
            Ok(exact) => exact,
            Err(insert_pos) => insert_pos.saturating_sub(1),
        };

        Ok(Some(line))
    }

    /// Reads and scans one chunk from the buffer, recording newline positions.
    fn scan_chunk(&mut self, buffer: &dyn Buffer) -> Result<()> {
        let mut chunk = vec![0u8; SCAN_CHUNK_SIZE];
        let offset = usize::try_from(self.scanned_to).unwrap_or(usize::MAX);
        let bytes_read = buffer.read_at(offset, &mut chunk)?;
        if bytes_read == 0 {
            // No more data; mark as fully scanned.
            self.scanned_to = self.buffer_len;
            return Ok(());
        }

        for (i, &byte) in chunk[..bytes_read].iter().enumerate() {
            if byte == b'\n' {
                let next_line_offset = self.scanned_to + (i as u64) + 1;
                // Only record a new line start if it's within the buffer.
                if next_line_offset < self.buffer_len {
                    self.offsets.push(next_line_offset);
                }
            }
        }

        self.scanned_to += bytes_read as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::SliceBuffer;

    // ── Empty buffer ───────────────────────────────────────────────────

    #[test]
    fn test_line_index_empty_buffer_total_lines_returns_zero() {
        let buf = SliceBuffer::new(b"");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 0);
    }

    #[test]
    fn test_line_index_empty_buffer_lines_indexed_returns_zero() {
        let buf = SliceBuffer::new(b"");
        let idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.lines_indexed(), 0);
    }

    // ── Single line, no trailing newline ───────────────────────────────

    #[test]
    fn test_line_index_single_line_no_newline_total_lines_returns_one() {
        let buf = SliceBuffer::new(b"hello");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 1);
    }

    #[test]
    fn test_line_index_single_line_no_newline_get_line_returns_content() {
        let buf = SliceBuffer::new(b"hello");
        let mut idx = LineIndex::new(buf.len() as u64);
        let line = idx.get_line(0, &buf).unwrap();
        assert_eq!(line.as_deref(), Some("hello"));
    }

    // ── Single line with trailing newline ──────────────────────────────

    #[test]
    fn test_line_index_single_line_with_newline_total_lines_returns_one() {
        let buf = SliceBuffer::new(b"hello\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 1);
    }

    #[test]
    fn test_line_index_single_line_with_newline_get_line_strips_newline() {
        let buf = SliceBuffer::new(b"hello\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        let line = idx.get_line(0, &buf).unwrap();
        assert_eq!(line.as_deref(), Some("hello"));
    }

    // ── Multiple lines ────────────────────────────────────────────────

    #[test]
    fn test_line_index_multiple_lines_total_lines_correct() {
        let buf = SliceBuffer::new(b"a\nb\nc\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 3);
    }

    #[test]
    fn test_line_index_multiple_lines_get_line_returns_correct_content() {
        let buf = SliceBuffer::new(b"a\nb\nc\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.get_line(0, &buf).unwrap().as_deref(), Some("a"));
        assert_eq!(idx.get_line(1, &buf).unwrap().as_deref(), Some("b"));
        assert_eq!(idx.get_line(2, &buf).unwrap().as_deref(), Some("c"));
    }

    // ── Windows line endings ──────────────────────────────────────────

    #[test]
    fn test_line_index_windows_line_endings_total_lines_correct() {
        let buf = SliceBuffer::new(b"a\r\nb\r\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 2);
    }

    #[test]
    fn test_line_index_windows_line_endings_get_line_strips_crlf() {
        let buf = SliceBuffer::new(b"a\r\nb\r\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.get_line(0, &buf).unwrap().as_deref(), Some("a"));
        assert_eq!(idx.get_line(1, &buf).unwrap().as_deref(), Some("b"));
    }

    // ── Mixed line endings ────────────────────────────────────────────

    #[test]
    fn test_line_index_mixed_line_endings_total_lines_correct() {
        let buf = SliceBuffer::new(b"a\nb\r\nc\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 3);
    }

    #[test]
    fn test_line_index_mixed_line_endings_get_line_returns_correct_content() {
        let buf = SliceBuffer::new(b"a\nb\r\nc\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.get_line(0, &buf).unwrap().as_deref(), Some("a"));
        assert_eq!(idx.get_line(1, &buf).unwrap().as_deref(), Some("b"));
        assert_eq!(idx.get_line(2, &buf).unwrap().as_deref(), Some("c"));
    }

    // ── File not ending in newline ────────────────────────────────────

    #[test]
    fn test_line_index_no_trailing_newline_total_lines_correct() {
        let buf = SliceBuffer::new(b"a\nb");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 2);
    }

    #[test]
    fn test_line_index_no_trailing_newline_get_line_returns_last_line() {
        let buf = SliceBuffer::new(b"a\nb");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.get_line(0, &buf).unwrap().as_deref(), Some("a"));
        assert_eq!(idx.get_line(1, &buf).unwrap().as_deref(), Some("b"));
    }

    // ── line_range ────────────────────────────────────────────────────

    #[test]
    fn test_line_index_line_range_returns_correct_byte_ranges() {
        // "a\nb\nc\n" → offsets: [0, 2, 4], buffer_len=6
        let buf = SliceBuffer::new(b"a\nb\nc\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        idx.index_all(&buf).unwrap();

        assert_eq!(idx.line_range(0), Some((0, 2)));
        assert_eq!(idx.line_range(1), Some((2, 4)));
        assert_eq!(idx.line_range(2), Some((4, 6)));
    }

    #[test]
    fn test_line_index_line_range_no_trailing_newline_last_line_extends_to_eof() {
        // "a\nb" → offsets: [0, 2], buffer_len=3
        let buf = SliceBuffer::new(b"a\nb");
        let mut idx = LineIndex::new(buf.len() as u64);
        idx.index_all(&buf).unwrap();

        assert_eq!(idx.line_range(0), Some((0, 2)));
        assert_eq!(idx.line_range(1), Some((2, 3)));
    }

    #[test]
    fn test_line_index_line_range_out_of_bounds_returns_none() {
        let buf = SliceBuffer::new(b"a\nb\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        idx.index_all(&buf).unwrap();
        assert_eq!(idx.line_range(5), None);
    }

    // ── line_at_offset ────────────────────────────────────────────────

    #[test]
    fn test_line_index_line_at_offset_identifies_correct_line() {
        // "a\nb\nc\n" → line 0: [0,2), line 1: [2,4), line 2: [4,6)
        let buf = SliceBuffer::new(b"a\nb\nc\n");
        let mut idx = LineIndex::new(buf.len() as u64);

        assert_eq!(idx.line_at_offset(0, &buf).unwrap(), Some(0));
        assert_eq!(idx.line_at_offset(1, &buf).unwrap(), Some(0)); // the \n belongs to line 0
        assert_eq!(idx.line_at_offset(2, &buf).unwrap(), Some(1));
        assert_eq!(idx.line_at_offset(4, &buf).unwrap(), Some(2));
        assert_eq!(idx.line_at_offset(5, &buf).unwrap(), Some(2));
    }

    #[test]
    fn test_line_index_line_at_offset_beyond_buffer_returns_none() {
        let buf = SliceBuffer::new(b"a\nb\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.line_at_offset(100, &buf).unwrap(), None);
    }

    // ── Lazy scanning ─────────────────────────────────────────────────

    #[test]
    fn test_line_index_ensure_line_scans_lazily() {
        // Build a 100-line buffer.
        let mut data = Vec::new();
        for i in 0..100 {
            data.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        let buf = SliceBuffer::new(&data);
        let mut idx = LineIndex::new(buf.len() as u64);

        // Ask for line 5 — should not index everything.
        assert!(idx.ensure_line(5, &buf).unwrap());
        assert!(idx.lines_indexed() >= 6); // at least lines 0..=5

        // But the full buffer has 100 lines, so we shouldn't have indexed all of them
        // (unless the buffer is smaller than SCAN_CHUNK_SIZE, which it is for 100 short lines).
        // For a small buffer, one chunk scans everything. That's correct behavior.
        // The real laziness test uses a large buffer — see the large buffer test below.
    }

    // ── index_all ─────────────────────────────────────────────────────

    #[test]
    fn test_line_index_index_all_indexes_everything() {
        let mut data = Vec::new();
        for i in 0..50 {
            data.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        let buf = SliceBuffer::new(&data);
        let mut idx = LineIndex::new(buf.len() as u64);
        idx.index_all(&buf).unwrap();
        assert_eq!(idx.lines_indexed(), 50);
    }

    // ── Out-of-bounds ─────────────────────────────────────────────────

    #[test]
    fn test_line_index_ensure_line_out_of_bounds_returns_false() {
        let buf = SliceBuffer::new(b"a\nb\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert!(!idx.ensure_line(10, &buf).unwrap());
    }

    #[test]
    fn test_line_index_get_line_out_of_bounds_returns_none() {
        let buf = SliceBuffer::new(b"a\nb\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.get_line(10, &buf).unwrap(), None);
    }

    // ── Large buffer (crosses chunk boundary) ─────────────────────────

    #[test]
    fn test_line_index_large_buffer_scans_across_chunk_boundaries() {
        // Build a buffer larger than SCAN_CHUNK_SIZE (64 KiB).
        // Each line is ~80 bytes, so ~1000 lines ≈ 80 KiB > 64 KiB.
        let mut data = Vec::new();
        let line_count = 1200;
        for i in 0..line_count {
            // Pad to ensure we're well over the chunk boundary.
            let line = format!(
                "line {:06} — padding to make lines longer for chunk boundary test\n",
                i
            );
            data.extend_from_slice(line.as_bytes());
        }
        assert!(
            data.len() > SCAN_CHUNK_SIZE,
            "test data must exceed chunk size"
        );

        let buf = SliceBuffer::new(&data);
        let mut idx = LineIndex::new(buf.len() as u64);

        // Index everything and verify.
        assert_eq!(idx.total_lines(&buf).unwrap(), line_count);

        // Verify a line near the chunk boundary.
        let mid = line_count / 2;
        let content = idx.get_line(mid, &buf).unwrap().unwrap();
        let expected = format!(
            "line {:06} — padding to make lines longer for chunk boundary test",
            mid
        );
        assert_eq!(content, expected);

        // Verify the last line.
        let last = idx.get_line(line_count - 1, &buf).unwrap().unwrap();
        let expected_last = format!(
            "line {:06} — padding to make lines longer for chunk boundary test",
            line_count - 1
        );
        assert_eq!(last, expected_last);
    }

    #[test]
    fn test_line_index_large_buffer_ensure_line_does_not_index_everything() {
        // Build a buffer much larger than SCAN_CHUNK_SIZE.
        let mut data = Vec::new();
        let line_count = 1200;
        for i in 0..line_count {
            let line = format!(
                "line {:06} — padding to make lines longer for chunk boundary test\n",
                i
            );
            data.extend_from_slice(line.as_bytes());
        }
        assert!(data.len() > SCAN_CHUNK_SIZE);

        let buf = SliceBuffer::new(&data);
        let mut idx = LineIndex::new(buf.len() as u64);

        // Ask for line 5 — should NOT scan the whole buffer.
        assert!(idx.ensure_line(5, &buf).unwrap());
        assert!(idx.lines_indexed() < line_count);
    }

    // ── Edge cases ────────────────────────────────────────────────────

    #[test]
    fn test_line_index_only_newlines_returns_correct_count() {
        // "\n\n\n" → 3 lines (each containing just a newline).
        let buf = SliceBuffer::new(b"\n\n\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 3);
    }

    #[test]
    fn test_line_index_only_newlines_get_line_returns_empty_strings() {
        let buf = SliceBuffer::new(b"\n\n\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.get_line(0, &buf).unwrap().as_deref(), Some(""));
        assert_eq!(idx.get_line(1, &buf).unwrap().as_deref(), Some(""));
        assert_eq!(idx.get_line(2, &buf).unwrap().as_deref(), Some(""));
    }

    #[test]
    fn test_line_index_single_newline_returns_one_line() {
        let buf = SliceBuffer::new(b"\n");
        let mut idx = LineIndex::new(buf.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 1);
        assert_eq!(idx.get_line(0, &buf).unwrap().as_deref(), Some(""));
    }

    // ── update_buffer_len ────────────────────────────────────────────

    #[test]
    fn test_line_index_update_buffer_len_extends_scannable_range() {
        // Start with "a\nb\n" (4 bytes, 2 lines).
        let initial = b"a\nb\n";
        let buf = SliceBuffer::new(initial);
        let mut idx = LineIndex::new(initial.len() as u64);
        assert_eq!(idx.total_lines(&buf).unwrap(), 2);

        // Simulate buffer growing to "a\nb\nc\n" (6 bytes).
        let grown = b"a\nb\nc\n";
        let buf2 = SliceBuffer::new(grown);
        idx.update_buffer_len(grown.len() as u64);
        assert_eq!(idx.total_lines(&buf2).unwrap(), 3);
    }

    #[test]
    fn test_line_index_update_buffer_len_no_change_is_noop() {
        let data = b"a\nb\n";
        let buf = SliceBuffer::new(data);
        let mut idx = LineIndex::new(data.len() as u64);
        idx.index_all(&buf).unwrap();
        let lines_before = idx.lines_indexed();

        idx.update_buffer_len(data.len() as u64);
        assert_eq!(idx.lines_indexed(), lines_before);
    }

    #[test]
    fn test_line_index_update_buffer_len_from_empty_seeds_first_line() {
        let mut idx = LineIndex::new(0);
        assert_eq!(idx.lines_indexed(), 0);

        // Grow from empty to non-empty.
        idx.update_buffer_len(5);
        assert_eq!(idx.lines_indexed(), 1); // line 0 seeded
    }

    #[test]
    fn test_line_index_update_buffer_len_smaller_is_ignored() {
        let data = b"a\nb\nc\n";
        let buf = SliceBuffer::new(data);
        let mut idx = LineIndex::new(data.len() as u64);
        idx.index_all(&buf).unwrap();
        let lines_before = idx.lines_indexed();

        // Trying to shrink should be ignored.
        idx.update_buffer_len(2);
        assert_eq!(idx.lines_indexed(), lines_before);
    }
}
