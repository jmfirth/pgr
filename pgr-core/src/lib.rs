#![warn(clippy::pedantic)]
//! Buffer management, line indexing, marks, and filtering.

pub mod buffer;
pub mod content_mode;
pub mod error;
pub mod file_buffer;
pub mod line_index;
pub mod marks;

pub use buffer::Buffer;
pub use content_mode::{detect_content_mode, ContentMode};
pub use error::{CoreError, Result};
pub use file_buffer::FileBuffer;
pub use line_index::LineIndex;
pub use marks::{Mark, MarkStore};

#[cfg(test)]
pub(crate) mod test_helpers;
