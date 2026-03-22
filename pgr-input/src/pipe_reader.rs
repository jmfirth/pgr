//! A growable [`Buffer`] implementation backed by a streaming `Read` source.

use std::io::Read;

use pgr_core::buffer::Buffer;

/// Default read chunk size for pulling data from the pipe.
const PIPE_READ_CHUNK: usize = 64 * 1024;

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
}

impl<R: Read> PipeBuffer<R> {
    /// Creates a new pipe buffer reading from the given source.
    #[must_use]
    pub fn new(source: R) -> Self {
        Self {
            data: Vec::new(),
            source,
            eof: false,
        }
    }

    /// Returns whether the underlying source has reached EOF.
    #[must_use]
    pub fn is_eof(&self) -> bool {
        self.eof
    }
}

impl<R: Read + Send> Buffer for PipeBuffer<R> {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn is_empty(&self) -> bool {
        self.data.is_empty()
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
        true
    }

    fn refresh(&mut self) -> pgr_core::Result<usize> {
        if self.eof {
            return Ok(self.data.len());
        }

        let mut chunk = vec![0u8; PIPE_READ_CHUNK];
        let n = self.source.read(&mut chunk)?;
        if n == 0 {
            self.eof = true;
        } else {
            self.data.extend_from_slice(&chunk[..n]);
        }

        Ok(self.data.len())
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
}
