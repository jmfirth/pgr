#![warn(clippy::pedantic)]
//! Buffer management, line indexing, marks, and filtering.

pub mod buffer;
pub mod error;
pub mod file_buffer;

pub use buffer::Buffer;
pub use error::{CoreError, Result};
pub use file_buffer::FileBuffer;
