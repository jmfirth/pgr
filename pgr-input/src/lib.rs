#![warn(clippy::pedantic)]
//! File and pipe reading, LESSOPEN/LESSCLOSE, follow mode, and decompression.

pub mod error;
pub mod file_reader;
pub mod pipe_reader;
pub mod preproc;

pub use error::{InputError, Result};
pub use file_reader::LoadedFile;
pub use pipe_reader::{stdin_is_pipe, PipeBuffer};
pub use preproc::{LessOpenFormat, PreprocessResult, Preprocessor};
