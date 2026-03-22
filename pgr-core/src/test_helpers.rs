//! Test helpers for `pgr-core` unit tests.

use crate::buffer::Buffer;
use crate::error::Result;

/// A [`Buffer`] backed by an in-memory byte slice, for use in unit tests.
pub struct SliceBuffer {
    data: Vec<u8>,
}

impl SliceBuffer {
    /// Creates a new `SliceBuffer` containing a copy of `data`.
    pub fn new(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
        }
    }
}

impl Buffer for SliceBuffer {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> Result<usize> {
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

    fn refresh(&mut self) -> Result<usize> {
        Ok(self.data.len())
    }
}
