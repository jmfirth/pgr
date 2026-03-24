//! A growable [`Buffer`] implementation backed by a streaming `Read` source.

use std::io::Read;

use pgr_core::buffer::Buffer;

use crate::log_file::LogWriter;

/// Default read chunk size for pulling data from the pipe.
const PIPE_READ_CHUNK: usize = 64 * 1024;

/// Default buffer size when auto-allocation is disabled (`-B`).
///
/// This provides enough space for a single screen of content (roughly 64 KiB),
/// matching the read chunk size.
const DEFAULT_DISABLED_AUTO_ALLOC_SIZE: usize = 64 * 1024;

/// A growable buffer backed by a `Read` source (pipe, stdin, or any stream).
///
/// Data is read incrementally via [`Buffer::refresh`]. The buffer stores all
/// data read so far in a `Vec<u8>`, enabling random-access reads for
/// backward scrolling.
pub struct PipeBuffer<R: Read> {
    data: Vec<u8>,
    source: R,
    /// Whether the source has reached EOF.
    eof: bool,
    /// Optional log writer for tee-ing data to a file (`-o` / `-O`).
    log_writer: Option<LogWriter>,
    /// Maximum buffer size in bytes. `None` means unlimited growth.
    max_buffer_bytes: Option<usize>,
    /// Total number of bytes discarded from the front of the buffer
    /// when the buffer limit was exceeded. Used to translate logical
    /// offsets (from the consumer's perspective) to physical indices
    /// into `data`.
    bytes_discarded: usize,
}

impl<R: Read> PipeBuffer<R> {
    /// Creates a new pipe buffer reading from the given source.
    #[must_use]
    pub fn new(source: R) -> Self {
        Self {
            data: Vec::new(),
            source,
            eof: false,
            log_writer: None,
            max_buffer_bytes: None,
            bytes_discarded: 0,
        }
    }

    /// Returns whether the underlying source has reached EOF.
    #[must_use]
    pub fn is_eof(&self) -> bool {
        self.eof
    }

    /// Attaches a log writer that will receive a copy of all data read from
    /// the source. Used to implement `-o` / `-O` flags.
    pub fn set_log_writer(&mut self, writer: LogWriter) {
        self.log_writer = Some(writer);
    }

    /// Sets the maximum buffer size in kilobytes (`-b N`).
    ///
    /// When the buffer grows beyond this limit, the oldest data is discarded
    /// from the front. This controls memory usage for large pipe inputs.
    pub fn set_buffer_limit(&mut self, kb: usize) {
        self.max_buffer_bytes = Some(kb * 1024);
    }

    /// Disables automatic buffer allocation beyond a minimal amount (`-B`).
    ///
    /// Limits the buffer to [`DEFAULT_DISABLED_AUTO_ALLOC_SIZE`] bytes,
    /// discarding oldest data when the limit is exceeded.
    pub fn disable_auto_alloc(&mut self) {
        self.max_buffer_bytes = Some(DEFAULT_DISABLED_AUTO_ALLOC_SIZE);
    }

    /// Returns the number of bytes that have been discarded from the front
    /// of the buffer due to size limits.
    #[must_use]
    pub fn bytes_discarded(&self) -> usize {
        self.bytes_discarded
    }

    /// Enforces the buffer size limit by discarding oldest data from the front.
    fn enforce_limit(&mut self) {
        if let Some(max) = self.max_buffer_bytes {
            if self.data.len() > max {
                let excess = self.data.len() - max;
                self.data.drain(..excess);
                self.bytes_discarded += excess;
            }
        }
    }
}

impl<R: Read + Send> Buffer for PipeBuffer<R> {
    fn len(&self) -> usize {
        self.bytes_discarded + self.data.len()
    }

    fn is_empty(&self) -> bool {
        self.bytes_discarded == 0 && self.data.is_empty()
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> pgr_core::Result<usize> {
        // Offset is logical (from the consumer's perspective, counting from
        // byte 0 of the original stream). Translate to a physical index into
        // `self.data`, which may have had its front truncated.
        if offset < self.bytes_discarded {
            // The requested region starts in data that has been discarded.
            // Calculate how many bytes of the request fall in discarded range.
            let skip = self.bytes_discarded - offset;
            if skip >= buf.len() {
                // Entire request is in the discarded region.
                return Ok(0);
            }
            // Partial overlap: read what we still have from physical index 0.
            let available = &self.data[..];
            let remaining_buf = &mut buf[skip..];
            let to_copy = available.len().min(remaining_buf.len());
            remaining_buf[..to_copy].copy_from_slice(&available[..to_copy]);
            // Zero out the leading bytes that were in the discarded region.
            buf[..skip].fill(0);
            return Ok(skip + to_copy);
        }
        let physical = offset - self.bytes_discarded;
        if physical >= self.data.len() {
            return Ok(0);
        }
        let available = &self.data[physical..];
        let to_copy = available.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&available[..to_copy]);
        Ok(to_copy)
    }

    fn is_growable(&self) -> bool {
        true
    }

    fn refresh(&mut self) -> pgr_core::Result<usize> {
        if self.eof {
            return Ok(self.bytes_discarded + self.data.len());
        }

        let mut chunk = vec![0u8; PIPE_READ_CHUNK];
        let n = self.source.read(&mut chunk)?;
        if n == 0 {
            self.eof = true;
        } else {
            if let Some(ref mut writer) = self.log_writer {
                // Best-effort write to log file; ignore errors to avoid
                // disrupting the pager's primary function.
                let _ = writer.write_chunk(&chunk[..n]);
            }
            self.data.extend_from_slice(&chunk[..n]);
            self.enforce_limit();
        }

        Ok(self.bytes_discarded + self.data.len())
    }
}

/// Returns `true` if file descriptor 0 (stdin) is a pipe/file rather than a TTY.
///
/// Used at startup to decide whether to read from stdin or require a filename
/// argument. Returns `false` when stdin is connected to a terminal.
#[must_use]
pub fn stdin_is_pipe() -> bool {
    // SAFETY: `isatty` is a POSIX function that inspects the file descriptor
    // without modifying any state. fd 0 (stdin) is always valid.
    unsafe { libc::isatty(0) == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // ── 1. test_pipe_buffer_new_is_empty ─────────────────────────────

    #[test]
    fn test_pipe_buffer_new_is_empty() {
        let source = Cursor::new(vec![1u8, 2, 3]);
        let buf = PipeBuffer::new(source);
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    // ── 2. test_pipe_buffer_refresh_reads_data ───────────────────────

    #[test]
    fn test_pipe_buffer_refresh_reads_data() {
        let data = b"hello world";
        let source = Cursor::new(data.to_vec());
        let mut buf = PipeBuffer::new(source);

        let len = buf.refresh().expect("refresh failed");
        assert_eq!(len, data.len());
        assert_eq!(buf.len(), data.len());
    }

    // ── 3. test_pipe_buffer_read_at_zero_returns_full_content ────────

    #[test]
    fn test_pipe_buffer_read_at_zero_returns_full_content() {
        let data = b"hello world";
        let source = Cursor::new(data.to_vec());
        let mut buf = PipeBuffer::new(source);
        buf.refresh().expect("refresh failed");

        let mut out = vec![0u8; data.len()];
        let n = buf.read_at(0, &mut out).expect("read_at failed");
        assert_eq!(n, data.len());
        assert_eq!(&out, data);
    }

    // ── 4. test_pipe_buffer_read_at_partway_returns_correct_slice ────

    #[test]
    fn test_pipe_buffer_read_at_partway_returns_correct_slice() {
        let data = b"hello world";
        let source = Cursor::new(data.to_vec());
        let mut buf = PipeBuffer::new(source);
        buf.refresh().expect("refresh failed");

        let mut out = vec![0u8; 5];
        let n = buf.read_at(6, &mut out).expect("read_at failed");
        assert_eq!(n, 5);
        assert_eq!(&out[..n], b"world");
    }

    // ── 5. test_pipe_buffer_read_at_beyond_end_returns_zero ──────────

    #[test]
    fn test_pipe_buffer_read_at_beyond_end_returns_zero() {
        let source = Cursor::new(b"short".to_vec());
        let mut buf = PipeBuffer::new(source);
        buf.refresh().expect("refresh failed");

        let mut out = vec![0u8; 10];
        let n = buf.read_at(100, &mut out).expect("read_at failed");
        assert_eq!(n, 0);
    }

    // ── 6. test_pipe_buffer_is_growable_returns_true ──────────────────

    #[test]
    fn test_pipe_buffer_is_growable_returns_true() {
        let source = Cursor::new(Vec::<u8>::new());
        let buf = PipeBuffer::new(source);
        assert!(buf.is_growable());
    }

    // ── 7. test_pipe_buffer_multiple_refreshes_accumulate_data ───────

    /// A reader that yields data in fixed-size chunks, simulating a pipe
    /// that delivers data incrementally.
    struct ChunkedReader {
        data: Vec<u8>,
        pos: usize,
        chunk_size: usize,
    }

    impl ChunkedReader {
        fn new(data: Vec<u8>, chunk_size: usize) -> Self {
            Self {
                data,
                pos: 0,
                chunk_size,
            }
        }
    }

    impl Read for ChunkedReader {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.pos >= self.data.len() {
                return Ok(0);
            }
            let remaining = &self.data[self.pos..];
            let to_read = remaining.len().min(self.chunk_size).min(buf.len());
            buf[..to_read].copy_from_slice(&remaining[..to_read]);
            self.pos += to_read;
            Ok(to_read)
        }
    }

    #[test]
    fn test_pipe_buffer_multiple_refreshes_accumulate_data() {
        let data = b"aabbcc".to_vec();
        let source = ChunkedReader::new(data.clone(), 2);
        let mut buf = PipeBuffer::new(source);

        let len1 = buf.refresh().expect("refresh 1 failed");
        assert_eq!(len1, 2);

        let len2 = buf.refresh().expect("refresh 2 failed");
        assert_eq!(len2, 4);

        let len3 = buf.refresh().expect("refresh 3 failed");
        assert_eq!(len3, 6);

        let mut out = vec![0u8; 6];
        let n = buf.read_at(0, &mut out).expect("read_at failed");
        assert_eq!(n, 6);
        assert_eq!(&out, b"aabbcc");
    }

    // ── 8. test_pipe_buffer_refresh_after_eof_is_noop ────────────────

    #[test]
    fn test_pipe_buffer_refresh_after_eof_is_noop() {
        let data = b"data";
        let source = Cursor::new(data.to_vec());
        let mut buf = PipeBuffer::new(source);

        // First refresh reads all data
        buf.refresh().expect("refresh 1 failed");
        // Second refresh hits EOF
        buf.refresh().expect("refresh 2 failed");
        assert!(buf.is_eof());

        let len_before = buf.len();
        let len_after = buf.refresh().expect("refresh 3 failed");
        assert_eq!(len_before, len_after);
        assert_eq!(len_after, data.len());
    }

    // ── 9. test_pipe_buffer_is_eof_false_initially ───────────────────

    #[test]
    fn test_pipe_buffer_is_eof_false_initially() {
        let source = Cursor::new(b"stuff".to_vec());
        let buf = PipeBuffer::new(source);
        assert!(!buf.is_eof());
    }

    // ── 10. test_pipe_buffer_is_eof_true_after_full_read ─────────────

    #[test]
    fn test_pipe_buffer_is_eof_true_after_full_read() {
        let data = b"small";
        let source = Cursor::new(data.to_vec());
        let mut buf = PipeBuffer::new(source);

        // First refresh reads all the data
        buf.refresh().expect("refresh 1 failed");
        // Second refresh reads 0 bytes, setting EOF
        buf.refresh().expect("refresh 2 failed");
        assert!(buf.is_eof());
    }

    // ── 11. test_pipe_buffer_empty_source_eof_immediately ────────────

    #[test]
    fn test_pipe_buffer_empty_source_eof_immediately() {
        let source = Cursor::new(Vec::<u8>::new());
        let mut buf = PipeBuffer::new(source);

        let len = buf.refresh().expect("refresh failed");
        assert_eq!(len, 0);
        assert!(buf.is_eof());
        assert!(buf.is_empty());
    }

    // ── 12. test_stdin_is_pipe_returns_bool ───────────────────────────

    #[test]
    fn test_stdin_is_pipe_returns_bool() {
        // Smoke test: function runs without panicking and returns a bool.
        // In a test runner, stdin is typically not a TTY, but the exact
        // value depends on the test environment.
        let result: bool = stdin_is_pipe();
        // Just verify it's a bool by using it
        let _ = result;
    }

    // ── 13. test_pipe_buffer_set_buffer_limit_enforces_max ──────────

    #[test]
    fn test_pipe_buffer_set_buffer_limit_enforces_max() {
        // 10 bytes of data, limit of 1 KB (1024 bytes) — no truncation.
        let data = b"0123456789";
        let source = Cursor::new(data.to_vec());
        let mut buf = PipeBuffer::new(source);
        buf.set_buffer_limit(1); // 1 KB
        buf.refresh().expect("refresh failed");

        assert_eq!(buf.data.len(), 10);
        assert_eq!(buf.bytes_discarded, 0);
    }

    // ── 14. test_pipe_buffer_limit_discards_oldest_data ─────────────

    #[test]
    fn test_pipe_buffer_limit_discards_oldest_data() {
        // Use a chunked reader that delivers 5 bytes at a time.
        // Total data: 20 bytes, limit: very small (we set bytes directly).
        let data: Vec<u8> = (0..20).collect();
        let source = ChunkedReader::new(data, 5);
        let mut buf = PipeBuffer::new(source);
        // Set a limit of 10 bytes via direct field access in test.
        buf.max_buffer_bytes = Some(10);

        // First refresh: 5 bytes, under limit.
        buf.refresh().expect("refresh 1 failed");
        assert_eq!(buf.data.len(), 5);
        assert_eq!(buf.bytes_discarded, 0);

        // Second refresh: 10 bytes total, at limit.
        buf.refresh().expect("refresh 2 failed");
        assert_eq!(buf.data.len(), 10);
        assert_eq!(buf.bytes_discarded, 0);

        // Third refresh: 15 bytes read, limit is 10 → 5 discarded.
        buf.refresh().expect("refresh 3 failed");
        assert_eq!(buf.data.len(), 10);
        assert_eq!(buf.bytes_discarded, 5);

        // Fourth refresh: 20 bytes read, limit is 10 → 10 discarded.
        buf.refresh().expect("refresh 4 failed");
        assert_eq!(buf.data.len(), 10);
        assert_eq!(buf.bytes_discarded, 10);

        // The remaining data should be bytes 10..20.
        let mut out = vec![0u8; 10];
        let n = buf.read_at(10, &mut out).expect("read_at failed");
        assert_eq!(n, 10);
        let expected: Vec<u8> = (10..20).collect();
        assert_eq!(&out[..n], &expected);
    }

    // ── 15. test_pipe_buffer_read_at_discarded_region_returns_zero ───

    #[test]
    fn test_pipe_buffer_read_at_discarded_region_returns_zero() {
        let data: Vec<u8> = (0..20).collect();
        let source = Cursor::new(data);
        let mut buf = PipeBuffer::new(source);
        buf.max_buffer_bytes = Some(10);

        // Read all data — only last 10 bytes remain.
        buf.refresh().expect("refresh failed");
        assert_eq!(buf.bytes_discarded, 10);

        // Reading from the fully-discarded region with a small buffer
        // should return 0 when entirely in discarded range.
        let mut out = vec![0u8; 5];
        let n = buf.read_at(0, &mut out).expect("read_at failed");
        assert_eq!(n, 0);
    }

    // ── 16. test_pipe_buffer_len_includes_discarded ─────────────────

    #[test]
    fn test_pipe_buffer_len_includes_discarded() {
        let data: Vec<u8> = (0..20).collect();
        let source = Cursor::new(data);
        let mut buf = PipeBuffer::new(source);
        buf.max_buffer_bytes = Some(10);

        buf.refresh().expect("refresh failed");
        // len() reports the total logical length (discarded + retained).
        assert_eq!(buf.len(), 20);
        assert_eq!(buf.bytes_discarded(), 10);
    }

    // ── 17. test_pipe_buffer_disable_auto_alloc ─────────────────────

    #[test]
    fn test_pipe_buffer_disable_auto_alloc() {
        let source = Cursor::new(Vec::<u8>::new());
        let mut buf = PipeBuffer::new(source);
        buf.disable_auto_alloc();
        assert_eq!(buf.max_buffer_bytes, Some(DEFAULT_DISABLED_AUTO_ALLOC_SIZE));
    }

    // ── 18. test_pipe_buffer_set_buffer_limit_converts_kb ───────────

    #[test]
    fn test_pipe_buffer_set_buffer_limit_converts_kb() {
        let source = Cursor::new(Vec::<u8>::new());
        let mut buf = PipeBuffer::new(source);
        buf.set_buffer_limit(64);
        assert_eq!(buf.max_buffer_bytes, Some(64 * 1024));
    }

    // ── 19. test_pipe_buffer_no_limit_by_default ────────────────────

    #[test]
    fn test_pipe_buffer_no_limit_by_default() {
        let source = Cursor::new(Vec::<u8>::new());
        let buf = PipeBuffer::new(source);
        assert!(buf.max_buffer_bytes.is_none());
        assert_eq!(buf.bytes_discarded(), 0);
    }
}
