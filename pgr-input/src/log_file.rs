//! Log file writer for tee-ing pipe input to a file.
//!
//! Implements the `-o` / `-O` flags from GNU less: when stdin is piped,
//! data can be simultaneously written to a log file while being displayed.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::error::InputError;

/// Writes pipe data to a log file as it is read.
///
/// Used with `-o` / `-O` flags to save stdin input to a file while paging.
pub struct LogWriter {
    file: File,
}

impl LogWriter {
    /// Opens a log file for writing, creating or truncating it.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or opened.
    pub fn create(path: &Path) -> crate::Result<Self> {
        let file = File::create(path).map_err(InputError::Io)?;
        Ok(Self { file })
    }

    /// Appends a chunk of data to the log file.
    ///
    /// # Errors
    ///
    /// Returns an error if the write fails.
    pub fn write_chunk(&mut self, data: &[u8]) -> crate::Result<()> {
        self.file.write_all(data).map_err(InputError::Io)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 1. test_log_writer_create_creates_file ──────────────────────────

    #[test]
    fn test_log_writer_create_creates_file() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.log");
        let _writer = LogWriter::create(&path).expect("create failed");
        assert!(path.exists());
    }

    // ── 2. test_log_writer_write_chunk_writes_data ──────────────────────

    #[test]
    fn test_log_writer_write_chunk_writes_data() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.log");
        let mut writer = LogWriter::create(&path).expect("create failed");
        writer.write_chunk(b"hello world").expect("write failed");
        drop(writer);
        let contents = std::fs::read(&path).expect("read failed");
        assert_eq!(contents, b"hello world");
    }

    // ── 3. test_log_writer_write_multiple_chunks_appends ────────────────

    #[test]
    fn test_log_writer_write_multiple_chunks_appends() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.log");
        let mut writer = LogWriter::create(&path).expect("create failed");
        writer.write_chunk(b"first ").expect("write 1 failed");
        writer.write_chunk(b"second").expect("write 2 failed");
        drop(writer);
        let contents = std::fs::read(&path).expect("read failed");
        assert_eq!(contents, b"first second");
    }

    // ── 4. test_log_writer_create_truncates_existing ────────────────────

    #[test]
    fn test_log_writer_create_truncates_existing() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.log");
        std::fs::write(&path, b"old data").expect("pre-write failed");
        let mut writer = LogWriter::create(&path).expect("create failed");
        writer.write_chunk(b"new").expect("write failed");
        drop(writer);
        let contents = std::fs::read(&path).expect("read failed");
        assert_eq!(contents, b"new");
    }

    // ── 5. test_log_writer_create_invalid_path_returns_error ────────────

    #[test]
    fn test_log_writer_create_invalid_path_returns_error() {
        let result = LogWriter::create(Path::new("/nonexistent/dir/file.log"));
        assert!(result.is_err());
    }

    // ── 6. test_log_writer_write_chunk_empty_is_ok ──────────────────────

    #[test]
    fn test_log_writer_write_chunk_empty_is_ok() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let path = dir.path().join("test.log");
        let mut writer = LogWriter::create(&path).expect("create failed");
        writer.write_chunk(b"").expect("empty write failed");
        drop(writer);
        let contents = std::fs::read(&path).expect("read failed");
        assert!(contents.is_empty());
    }
}
