//! File list management for multi-file navigation.
//!
//! Provides [`FileList`] and [`FileEntry`] for tracking multiple open files
//! in the pager, supporting `:n`, `:p`, `:x`, and `:d` commands.

use std::path::PathBuf;

use pgr_core::{Buffer, LineIndex, MarkStore};

/// Metadata and state for a single file in the file list.
pub struct FileEntry {
    /// The file path, or `None` for stdin/pipe.
    pub path: Option<PathBuf>,
    /// Display name (filename or "standard input").
    pub display_name: String,
    /// The buffer for this file's content.
    pub buffer: Box<dyn Buffer>,
    /// The line index for this file.
    pub index: LineIndex,
    /// Per-file marks.
    pub marks: MarkStore,
    /// Saved viewport position (top line) for restoring when switching back.
    pub saved_top_line: usize,
    /// Saved horizontal offset.
    pub saved_horizontal_offset: usize,
}

/// An ordered list of files with a current-file cursor.
pub struct FileList {
    entries: Vec<FileEntry>,
    current: usize,
}

impl FileList {
    /// Create a new file list from the initial entry.
    #[must_use]
    pub fn new(entry: FileEntry) -> Self {
        Self {
            entries: vec![entry],
            current: 0,
        }
    }

    /// Add a file to the end of the list.
    pub fn push(&mut self, entry: FileEntry) {
        self.entries.push(entry);
    }

    /// Return the current file entry.
    #[must_use]
    pub fn current(&self) -> &FileEntry {
        &self.entries[self.current]
    }

    /// Return a mutable reference to the current file entry.
    pub fn current_mut(&mut self) -> &mut FileEntry {
        &mut self.entries[self.current]
    }

    /// Return the zero-based index of the current file.
    #[must_use]
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// Return the total number of files.
    #[must_use]
    pub fn file_count(&self) -> usize {
        self.entries.len()
    }

    /// Switch to the next file.
    ///
    /// # Errors
    ///
    /// Returns [`FileListError::NoNextFile`] if already at the last file.
    #[allow(clippy::should_implement_trait)] // Not an Iterator; "next" is the natural name for file navigation.
    pub fn next(&mut self) -> Result<(), FileListError> {
        if self.current + 1 >= self.entries.len() {
            return Err(FileListError::NoNextFile);
        }
        self.current += 1;
        Ok(())
    }

    /// Switch to the previous file.
    ///
    /// # Errors
    ///
    /// Returns [`FileListError::NoPreviousFile`] if already at the first file.
    pub fn prev(&mut self) -> Result<(), FileListError> {
        if self.current == 0 {
            return Err(FileListError::NoPreviousFile);
        }
        self.current -= 1;
        Ok(())
    }

    /// Switch to the N-th file (0-based).
    ///
    /// # Errors
    ///
    /// Returns [`FileListError::IndexOutOfRange`] if the index is out of range.
    pub fn goto(&mut self, index: usize) -> Result<(), FileListError> {
        if index >= self.entries.len() {
            return Err(FileListError::IndexOutOfRange(index));
        }
        self.current = index;
        Ok(())
    }

    /// Remove the current file from the list.
    ///
    /// Moves to the next file, or the previous file if at the end.
    ///
    /// # Errors
    ///
    /// Returns [`FileListError::CannotRemoveOnly`] if this is the only file.
    pub fn remove_current(&mut self) -> Result<(), FileListError> {
        if self.entries.len() <= 1 {
            return Err(FileListError::CannotRemoveOnly);
        }
        self.entries.remove(self.current);
        // If we removed the last entry in the vec, move cursor back.
        if self.current >= self.entries.len() {
            self.current = self.entries.len() - 1;
        }
        Ok(())
    }

    /// Save the current viewport state before switching files.
    pub fn save_viewport(&mut self, top_line: usize, horizontal_offset: usize) {
        let entry = &mut self.entries[self.current];
        entry.saved_top_line = top_line;
        entry.saved_horizontal_offset = horizontal_offset;
    }

    /// Return the saved viewport state for the current file.
    #[must_use]
    pub fn saved_viewport(&self) -> (usize, usize) {
        let entry = &self.entries[self.current];
        (entry.saved_top_line, entry.saved_horizontal_offset)
    }
}

/// Errors from file list operations.
#[derive(Debug, thiserror::Error)]
pub enum FileListError {
    /// No next file available.
    #[error("no next file")]
    NoNextFile,
    /// No previous file available.
    #[error("no previous file")]
    NoPreviousFile,
    /// File index is out of range.
    #[error("file index out of range: {0}")]
    IndexOutOfRange(usize),
    /// Cannot remove the only file in the list.
    #[error("cannot remove the only file")]
    CannotRemoveOnly,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple test buffer implementing `Buffer` over a `Vec<u8>`.
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

    fn make_entry(name: &str) -> FileEntry {
        let data = format!("{name} content\n").into_bytes();
        let buf_len = data.len() as u64;
        FileEntry {
            path: Some(PathBuf::from(name)),
            display_name: name.to_string(),
            buffer: Box::new(TestBuffer::new(&data)),
            index: LineIndex::new(buf_len),
            marks: MarkStore::new(),
            saved_top_line: 0,
            saved_horizontal_offset: 0,
        }
    }

    // Test 1: FileList::new creates a list with one entry, current index 0
    #[test]
    fn test_file_list_new_creates_single_entry_at_index_zero() {
        let list = FileList::new(make_entry("file1"));
        assert_eq!(list.file_count(), 1);
        assert_eq!(list.current_index(), 0);
        assert_eq!(list.current().display_name, "file1");
    }

    // Test 2: push adds an entry; file_count increases
    #[test]
    fn test_file_list_push_increases_file_count() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));
        assert_eq!(list.file_count(), 2);
    }

    // Test 3: next advances to the next file
    #[test]
    fn test_file_list_next_advances_to_next_file() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));
        list.next().unwrap();
        assert_eq!(list.current_index(), 1);
        assert_eq!(list.current().display_name, "file2");
    }

    // Test 4: next at the last file returns NoNextFile error
    #[test]
    fn test_file_list_next_at_last_file_returns_error() {
        let mut list = FileList::new(make_entry("file1"));
        let err = list.next().unwrap_err();
        assert!(matches!(err, FileListError::NoNextFile));
    }

    // Test 5: prev moves to the previous file
    #[test]
    fn test_file_list_prev_moves_to_previous_file() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));
        list.next().unwrap();
        list.prev().unwrap();
        assert_eq!(list.current_index(), 0);
        assert_eq!(list.current().display_name, "file1");
    }

    // Test 6: prev at the first file returns NoPreviousFile error
    #[test]
    fn test_file_list_prev_at_first_file_returns_error() {
        let mut list = FileList::new(make_entry("file1"));
        let err = list.prev().unwrap_err();
        assert!(matches!(err, FileListError::NoPreviousFile));
    }

    // Test 7: goto(0) switches to the first file
    #[test]
    fn test_file_list_goto_zero_switches_to_first_file() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));
        list.push(make_entry("file3"));
        list.next().unwrap();
        list.next().unwrap();
        list.goto(0).unwrap();
        assert_eq!(list.current_index(), 0);
        assert_eq!(list.current().display_name, "file1");
    }

    // Test 8: goto with out-of-range index returns error
    #[test]
    fn test_file_list_goto_out_of_range_returns_error() {
        let mut list = FileList::new(make_entry("file1"));
        let err = list.goto(5).unwrap_err();
        assert!(matches!(err, FileListError::IndexOutOfRange(5)));
    }

    // Test 9: remove_current with multiple files removes and advances
    #[test]
    fn test_file_list_remove_current_with_multiple_files_advances() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));
        list.push(make_entry("file3"));
        // Remove file1 (index 0), should advance to file2 (now index 0).
        list.remove_current().unwrap();
        assert_eq!(list.file_count(), 2);
        assert_eq!(list.current_index(), 0);
        assert_eq!(list.current().display_name, "file2");
    }

    // Test 9b: remove_current at the end falls back to previous
    #[test]
    fn test_file_list_remove_current_at_end_falls_back() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));
        list.next().unwrap(); // current = 1 (file2)
        list.remove_current().unwrap();
        assert_eq!(list.file_count(), 1);
        assert_eq!(list.current_index(), 0);
        assert_eq!(list.current().display_name, "file1");
    }

    // Test 10: remove_current with one file returns CannotRemoveOnly error
    #[test]
    fn test_file_list_remove_current_single_file_returns_error() {
        let mut list = FileList::new(make_entry("file1"));
        let err = list.remove_current().unwrap_err();
        assert!(matches!(err, FileListError::CannotRemoveOnly));
    }

    // Test 11: save_viewport and saved_viewport round-trip correctly
    #[test]
    fn test_file_list_save_viewport_round_trips() {
        let mut list = FileList::new(make_entry("file1"));
        list.save_viewport(42, 10);
        assert_eq!(list.saved_viewport(), (42, 10));
    }

    // Test 11b: viewport state is per-file
    #[test]
    fn test_file_list_viewport_state_is_per_file() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));

        // Save viewport for file1
        list.save_viewport(10, 5);

        // Switch to file2 and save different viewport
        list.next().unwrap();
        list.save_viewport(20, 15);

        // Verify file2's viewport
        assert_eq!(list.saved_viewport(), (20, 15));

        // Switch back to file1 and verify
        list.prev().unwrap();
        assert_eq!(list.saved_viewport(), (10, 5));
    }

    #[test]
    fn test_file_list_current_mut_allows_modification() {
        let mut list = FileList::new(make_entry("file1"));
        list.current_mut().display_name = "modified".to_string();
        assert_eq!(list.current().display_name, "modified");
    }

    #[test]
    fn test_file_list_error_display_no_next_file() {
        let err = FileListError::NoNextFile;
        assert_eq!(err.to_string(), "no next file");
    }

    #[test]
    fn test_file_list_error_display_no_previous_file() {
        let err = FileListError::NoPreviousFile;
        assert_eq!(err.to_string(), "no previous file");
    }

    #[test]
    fn test_file_list_error_display_index_out_of_range() {
        let err = FileListError::IndexOutOfRange(7);
        assert_eq!(err.to_string(), "file index out of range: 7");
    }

    #[test]
    fn test_file_list_error_display_cannot_remove_only() {
        let err = FileListError::CannotRemoveOnly;
        assert_eq!(err.to_string(), "cannot remove the only file");
    }

    // Test: remove_current from the middle keeps cursor at same index
    #[test]
    fn test_file_list_remove_current_from_middle_stays_at_index() {
        let mut list = FileList::new(make_entry("file1"));
        list.push(make_entry("file2"));
        list.push(make_entry("file3"));
        list.next().unwrap(); // current = 1 (file2)
        list.remove_current().unwrap();
        // After removing file2 at index 1, file3 slides to index 1.
        assert_eq!(list.file_count(), 2);
        assert_eq!(list.current_index(), 1);
        assert_eq!(list.current().display_name, "file3");
    }
}
