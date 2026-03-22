#![warn(clippy::pedantic)]
//! File and pipe reading, LESSOPEN/LESSCLOSE, follow mode, and decompression.

pub mod error;
pub mod file_reader;

pub use error::{InputError, Result};
pub use file_reader::LoadedFile;
