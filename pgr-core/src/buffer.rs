//! The core `Buffer` trait for abstracting over data sources.

use crate::Result;

/// A readable byte buffer that may or may not grow over time.
///
/// Implementors provide random-access reads into a contiguous byte sequence.
/// The buffer may be backed by an in-memory `Vec`, a memory-mapped file, a
/// pipe, or any other byte source.
///
/// All implementations must be `Send` so buffers can be transferred between
/// threads.
pub trait Buffer: Send {
    /// Returns the current number of bytes available in the buffer.
    fn len(&self) -> usize;

    /// Returns `true` if the buffer contains no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Reads bytes starting at `offset` into `buf`.
    ///
    /// Returns the number of bytes actually read. If `offset` is at or beyond
    /// the end of the buffer, returns `Ok(0)` rather than an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying storage cannot be read.
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize>;

    /// Returns `true` if this buffer can grow (e.g. a pipe or follow-mode file).
    ///
    /// A non-growable buffer has a fixed size that will not change after opening.
    fn is_growable(&self) -> bool;

    /// Re-checks the data source and returns the current length.
    ///
    /// For growable buffers this may pull in new data. For fixed-size buffers
    /// this is a no-op that returns the existing length.
    ///
    /// # Errors
    ///
    /// Returns an error if refreshing the underlying source fails.
    fn refresh(&mut self) -> Result<usize>;
}
