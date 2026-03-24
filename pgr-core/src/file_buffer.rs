//! A file-backed [`Buffer`] implementation with automatic mmap for large files.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use memmap2::Mmap;

use crate::buffer::Buffer;
use crate::Result;

/// Files smaller than this threshold are read into memory; files at or above
/// it are memory-mapped.
const MMAP_THRESHOLD: u64 = 8 * 1024 * 1024;

/// Internal storage strategy.
enum Storage {
    /// In-memory byte vector (used for small files and empty files).
    Vec(Vec<u8>),
    /// Memory-mapped region (used for large files).
    Mmap(Mmap),
}

/// A read-only buffer backed by a file on disk.
///
/// Files smaller than 8 MiB are read entirely into a `Vec<u8>`. Larger files
/// are memory-mapped for efficient random access without consuming equivalent
/// RAM.
///
/// Empty files always use the `Vec` path because memory-mapping zero bytes is
/// undefined behavior on some platforms.
///
/// The buffer stores the file path so that [`Buffer::refresh`] can re-read the
/// file to detect new data appended in follow mode.
pub struct FileBuffer {
    storage: Storage,
    path: PathBuf,
}

impl FileBuffer {
    /// Opens a file and returns a buffer over its contents.
    ///
    /// The file at `path` is read into memory if smaller than 8 MiB, or
    /// memory-mapped otherwise. Empty files are handled via the in-memory path
    /// to avoid platform-specific mmap issues.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be opened or read.
    pub fn open(path: &Path) -> Result<Self> {
        let storage = Self::load_storage(path)?;
        Ok(Self {
            storage,
            path: path.to_path_buf(),
        })
    }

    /// Returns the file path this buffer was opened from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns a byte slice over the entire buffer contents.
    fn as_bytes(&self) -> &[u8] {
        match &self.storage {
            Storage::Vec(v) => v.as_slice(),
            Storage::Mmap(m) => m.as_ref(),
        }
    }

    /// Load storage from a file path (shared between `open` and `refresh`).
    fn load_storage(path: &Path) -> Result<Storage> {
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        let len = metadata.len();

        if len == 0 || len < MMAP_THRESHOLD {
            let mut file = file;
            let capacity = usize::try_from(len).map_err(|_| {
                crate::CoreError::Buffer(format!("file size {len} exceeds addressable range"))
            })?;
            let mut data = Vec::with_capacity(capacity);
            file.read_to_end(&mut data)?;
            Ok(Storage::Vec(data))
        } else {
            // SAFETY: The file is open and has a non-zero length. We hold no
            // mutable references to the mapped region. The mapping is read-only.
            let mmap = unsafe { Mmap::map(&file)? };
            Ok(Storage::Mmap(mmap))
        }
    }
}

impl Buffer for FileBuffer {
    fn len(&self) -> usize {
        self.as_bytes().len()
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
        let data = self.as_bytes();
        if offset >= data.len() {
            return Ok(0);
        }
        let available = &data[offset..];
        let to_copy = available.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&available[..to_copy]);
        Ok(to_copy)
    }

    fn is_growable(&self) -> bool {
        false
    }

    fn refresh(&mut self) -> Result<usize> {
        self.storage = Self::load_storage(&self.path)?;
        Ok(self.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn make_temp_file(content: &[u8]) -> NamedTempFile {
        let mut f = NamedTempFile::new().expect("failed to create temp file");
        f.write_all(content).expect("failed to write to temp file");
        f.flush().expect("failed to flush temp file");
        f
    }

    // ── Small-file tests ────────────────────────────────────────────────

    #[test]
    fn test_open_small_file_len_matches_content() {
        let content = b"hello world";
        let f = make_temp_file(content);
        let buf = FileBuffer::open(f.path()).expect("open failed");
        assert_eq!(buf.len(), content.len());
    }

    #[test]
    fn test_read_at_zero_returns_full_content() {
        let content = b"hello world";
        let f = make_temp_file(content);
        let buf = FileBuffer::open(f.path()).expect("open failed");
        let mut out = vec![0u8; content.len()];
        let n = buf.read_at(0, &mut out).expect("read_at failed");
        assert_eq!(n, content.len());
        assert_eq!(&out, content);
    }

    #[test]
    fn test_read_at_partway_returns_correct_slice() {
        let content = b"hello world";
        let f = make_temp_file(content);
        let buf = FileBuffer::open(f.path()).expect("open failed");
        let mut out = vec![0u8; 5];
        let n = buf.read_at(6, &mut out).expect("read_at failed");
        assert_eq!(n, 5);
        assert_eq!(&out[..n], b"world");
    }

    #[test]
    fn test_read_at_beyond_end_returns_zero() {
        let content = b"hello";
        let f = make_temp_file(content);
        let buf = FileBuffer::open(f.path()).expect("open failed");
        let mut out = vec![0u8; 10];
        let n = buf.read_at(100, &mut out).expect("read_at failed");
        assert_eq!(n, 0);
    }

    #[test]
    fn test_read_at_buffer_larger_than_remaining_reads_available() {
        let content = b"hello";
        let f = make_temp_file(content);
        let buf = FileBuffer::open(f.path()).expect("open failed");
        let mut out = vec![0u8; 100];
        let n = buf.read_at(3, &mut out).expect("read_at failed");
        assert_eq!(n, 2);
        assert_eq!(&out[..n], b"lo");
    }

    // ── Empty file ──────────────────────────────────────────────────────

    #[test]
    fn test_empty_file_len_is_zero_and_is_empty() {
        let f = make_temp_file(b"");
        let buf = FileBuffer::open(f.path()).expect("open failed");
        assert_eq!(buf.len(), 0);
        assert!(buf.is_empty());
    }

    // ── Trait behaviour ─────────────────────────────────────────────────

    #[test]
    fn test_is_growable_returns_false() {
        let f = make_temp_file(b"data");
        let buf = FileBuffer::open(f.path()).expect("open failed");
        assert!(!buf.is_growable());
    }

    #[test]
    fn test_refresh_returns_same_len() {
        let content = b"some bytes";
        let f = make_temp_file(content);
        let mut buf = FileBuffer::open(f.path()).expect("open failed");
        let len = buf.refresh().expect("refresh failed");
        assert_eq!(len, content.len());
    }

    // ── Large (mmap) file tests ─────────────────────────────────────────

    #[test]
    fn test_large_file_opens_and_reports_correct_len() {
        let size = (MMAP_THRESHOLD as usize) + 1024;
        let data = vec![0xABu8; size];
        let f = make_temp_file(&data);
        let buf = FileBuffer::open(f.path()).expect("open failed");
        assert_eq!(buf.len(), size);
    }

    #[test]
    fn test_large_file_read_at_returns_correct_bytes() {
        let size = (MMAP_THRESHOLD as usize) + 1024;
        let mut data = vec![0u8; size];
        // Write a recognizable pattern near the end.
        let marker = b"MARKER";
        let marker_offset = size - marker.len();
        data[marker_offset..].copy_from_slice(marker);

        let f = make_temp_file(&data);
        let buf = FileBuffer::open(f.path()).expect("open failed");

        let mut out = vec![0u8; marker.len()];
        let n = buf
            .read_at(marker_offset, &mut out)
            .expect("read_at failed");
        assert_eq!(n, marker.len());
        assert_eq!(&out, marker);
    }

    // ── Error paths ─────────────────────────────────────────────────────

    #[test]
    fn test_open_nonexistent_file_returns_error() {
        let result = FileBuffer::open(Path::new("/tmp/pgr_nonexistent_file_does_not_exist"));
        assert!(result.is_err());
    }

    #[test]
    fn test_open_directory_returns_error() {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let result = FileBuffer::open(dir.path());
        assert!(result.is_err());
    }

    // ── Path accessor ──────────────────────────────────────────────────

    #[test]
    fn test_path_returns_opened_path() {
        let f = make_temp_file(b"data");
        let buf = FileBuffer::open(f.path()).expect("open failed");
        assert_eq!(buf.path(), f.path());
    }

    // ── Refresh detects new data ───────────────────────────────────────

    #[test]
    fn test_refresh_detects_appended_data() {
        let f = make_temp_file(b"hello\n");
        let mut buf = FileBuffer::open(f.path()).expect("open failed");
        assert_eq!(buf.len(), 6);

        // Append data to the file.
        {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .append(true)
                .open(f.path())
                .expect("open for append");
            file.write_all(b"world\n").expect("append write");
            file.flush().expect("flush append");
        }

        let new_len = buf.refresh().expect("refresh failed");
        assert_eq!(new_len, 12);
        assert_eq!(buf.len(), 12);

        // Verify the appended content is readable.
        let mut out = vec![0u8; 6];
        let n = buf.read_at(6, &mut out).expect("read_at failed");
        assert_eq!(n, 6);
        assert_eq!(&out, b"world\n");
    }

    #[test]
    fn test_refresh_unchanged_file_returns_same_len() {
        let content = b"static content\n";
        let f = make_temp_file(content);
        let mut buf = FileBuffer::open(f.path()).expect("open failed");
        let len1 = buf.refresh().expect("refresh 1 failed");
        let len2 = buf.refresh().expect("refresh 2 failed");
        assert_eq!(len1, len2);
        assert_eq!(len1, content.len());
    }
}
