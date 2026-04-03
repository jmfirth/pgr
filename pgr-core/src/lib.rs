#![warn(clippy::pedantic)]
//! Buffer management, line indexing, marks, and filtering.

pub mod buffer;
pub mod content_mode;
pub mod diff;
pub mod error;
pub mod file_buffer;
pub mod line_index;
pub mod marks;

pub use buffer::Buffer;
pub use content_mode::{detect_content_mode, ContentMode};
pub use diff::{
    classify_diff_line, compute_diff_prompt_info, next_file_line, next_hunk_line, parse_diff,
    prev_file_line, prev_hunk_line, DiffFile, DiffHunk, DiffLineType, DiffPromptInfo,
};
pub use error::{CoreError, Result};
pub use file_buffer::FileBuffer;
pub use line_index::LineIndex;
pub use marks::{Mark, MarkStore};

#[cfg(test)]
pub(crate) mod test_helpers;
