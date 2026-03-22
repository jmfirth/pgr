//! High-level file reader that combines a [`FileBuffer`] with a [`LineIndex`].

use std::path::{Path, PathBuf};

use pgr_core::{Buffer, FileBuffer, LineIndex};

/// A file opened for paging, with lazy line indexing.
///
/// `LoadedFile` owns both the underlying byte buffer and the line index,
/// providing a convenient API for reading individual lines by number.
pub struct LoadedFile {
    path: PathBuf,
    buffer: FileBuffer,
    index: LineIndex,
}

impl LoadedFile {
    /// Opens the file at `path` and prepares it for line-oriented reading.
    ///
    /// The file contents are loaded into a [`FileBuffer`] (memory-mapped for
    /// large files) and a [`LineIndex`] is initialised but not yet scanned.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    pub fn open(path: &Path) -> crate::Result<Self> {
        let buffer = FileBuffer::open(path)?;
        let index = LineIndex::new(buffer.len() as u64);
        Ok(Self {
            path: path.to_path_buf(),
            buffer,
            index,
        })
    }

    /// Returns the path this file was opened from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a reference to the underlying buffer.
    #[must_use]
    pub fn buffer(&self) -> &dyn Buffer {
        &self.buffer
    }

    /// Returns a mutable reference to the line index.
    pub fn index_mut(&mut self) -> &mut LineIndex {
        &mut self.index
    }

    /// Reads the content of a single line, stripping the trailing newline.
    ///
    /// Returns `Ok(None)` if `line_number` is beyond the end of the file.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer fails.
    pub fn get_line(&mut self, line_number: usize) -> crate::Result<Option<String>> {
        Ok(self.index.get_line(line_number, &self.buffer)?)
    }

    /// Ensures the index has scanned far enough to know about `up_to`.
    ///
    /// Returns `Ok(true)` if the line exists, `Ok(false)` if the file does not
    /// contain that many lines.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer fails.
    pub fn ensure_lines(&mut self, up_to: usize) -> crate::Result<bool> {
        Ok(self.index.ensure_line(up_to, &self.buffer)?)
    }

    /// Consume this `LoadedFile` and return the buffer and line index separately.
    ///
    /// This is useful when handing ownership to a `Pager`, which needs the
    /// buffer and index as independent values.
    #[must_use]
    pub fn into_parts(self) -> (Box<dyn Buffer>, LineIndex) {
        (Box::new(self.buffer) as Box<dyn Buffer>, self.index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Helper: write `content` to a temporary file and return the handle.
    fn make_temp_file(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("failed to create temp file");
        f.write_all(content).expect("failed to write");
        f.flush().expect("failed to flush");
        f
    }

    // ── 1. Open valid text file, read lines, verify content ──────────

    #[test]
    fn test_open_and_read_lines_returns_correct_content() {
        let f = make_temp_file(b"hello\nworld\n");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        assert_eq!(loaded.get_line(0).unwrap().as_deref(), Some("hello"));
        assert_eq!(loaded.get_line(1).unwrap().as_deref(), Some("world"));
    }

    // ── 2. Open file with known line count, verify total_lines ───────

    #[test]
    fn test_total_lines_matches_expected_count() {
        let f = make_temp_file(b"a\nb\nc\n");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        let total = loaded
            .index
            .total_lines(&loaded.buffer)
            .expect("total_lines failed");
        assert_eq!(total, 3);
    }

    // ── 3. get_line returns correct content for various line numbers ──

    #[test]
    fn test_get_line_various_numbers() {
        let f = make_temp_file(b"alpha\nbeta\ngamma\ndelta\n");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        assert_eq!(loaded.get_line(0).unwrap().as_deref(), Some("alpha"));
        assert_eq!(loaded.get_line(1).unwrap().as_deref(), Some("beta"));
        assert_eq!(loaded.get_line(2).unwrap().as_deref(), Some("gamma"));
        assert_eq!(loaded.get_line(3).unwrap().as_deref(), Some("delta"));
    }

    // ── 4. get_line beyond EOF returns None ───────────────────────────

    #[test]
    fn test_get_line_beyond_eof_returns_none() {
        let f = make_temp_file(b"only\n");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        assert_eq!(loaded.get_line(100).unwrap(), None);
    }

    // ── 5. Open nonexistent path returns InputError ──────────────────

    #[test]
    fn test_open_nonexistent_returns_error() {
        let result = LoadedFile::open(Path::new("/tmp/pgr_does_not_exist_ever"));
        assert!(result.is_err());
    }

    // ── 6. Open empty file succeeds, total_lines is 0 ────────────────

    #[test]
    fn test_empty_file_total_lines_zero() {
        let f = make_temp_file(b"");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        let total = loaded
            .index
            .total_lines(&loaded.buffer)
            .expect("total_lines failed");
        assert_eq!(total, 0);
    }

    // ── 7. File with no trailing newline has correct last line ────────

    #[test]
    fn test_no_trailing_newline_last_line_correct() {
        let f = make_temp_file(b"first\nsecond");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        assert_eq!(loaded.get_line(0).unwrap().as_deref(), Some("first"));
        assert_eq!(loaded.get_line(1).unwrap().as_deref(), Some("second"));
        assert_eq!(loaded.get_line(2).unwrap(), None);
    }

    // ── Additional: path() returns the correct path ──────────────────

    #[test]
    fn test_path_returns_opened_path() {
        let f = make_temp_file(b"data");
        let loaded = LoadedFile::open(f.path()).expect("open failed");
        assert_eq!(loaded.path(), f.path());
    }

    // ── Additional: ensure_lines correctness ─────────────────────────

    #[test]
    fn test_ensure_lines_returns_true_for_existing_line() {
        let f = make_temp_file(b"a\nb\nc\n");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        assert!(loaded.ensure_lines(2).unwrap());
    }

    #[test]
    fn test_ensure_lines_returns_false_for_nonexistent_line() {
        let f = make_temp_file(b"a\nb\n");
        let mut loaded = LoadedFile::open(f.path()).expect("open failed");
        assert!(!loaded.ensure_lines(10).unwrap());
    }

    // ── into_parts returns usable buffer and index ──────────────────

    #[test]
    fn test_into_parts_returns_functional_buffer_and_index() {
        let f = make_temp_file(b"hello\nworld\n");
        let loaded = LoadedFile::open(f.path()).expect("open failed");
        let (buffer, mut index) = loaded.into_parts();
        let total = index.total_lines(&*buffer).expect("total_lines failed");
        assert_eq!(total, 2);
        let line = index.get_line(0, &*buffer).expect("get_line failed");
        assert_eq!(line.as_deref(), Some("hello"));
    }
}
