#![warn(clippy::pedantic)]
//! Buffer management, line indexing, marks, and filtering.

pub mod buffer;
pub mod error;
pub mod file_buffer;
pub mod line_index;

pub use buffer::Buffer;
pub use error::{CoreError, Result};
pub use file_buffer::FileBuffer;
pub use line_index::LineIndex;

#[cfg(test)]
pub(crate) mod test_helpers;
