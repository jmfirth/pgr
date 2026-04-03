#![warn(clippy::pedantic)]
//! Buffer management, line indexing, marks, and filtering.

pub mod buffer;
pub mod content_mode;
pub mod diff;
pub mod error;
pub mod file_buffer;
pub mod git_log;
pub mod line_index;
pub mod man_sections;
pub mod marks;
pub mod word_diff;

pub use buffer::Buffer;
pub use content_mode::{detect_content_mode, ContentMode};
pub use diff::{
    classify_diff_line, compute_diff_prompt_info, next_file_line, next_hunk_line, parse_diff,
    prev_file_line, prev_hunk_line, DiffFile, DiffHunk, DiffLineType, DiffPromptInfo,
};
pub use error::{CoreError, Result};
pub use file_buffer::FileBuffer;
pub use git_log::{next_commit_line, parse_git_log, prev_commit_line, GitCommit};
pub use line_index::LineIndex;
pub use man_sections::{
    find_sections, next_section_line, prev_section_line, section_status, ManSection,
};
pub use marks::{Mark, MarkStore};
pub use word_diff::{compute_word_diff, pair_changed_lines, WordChange};

#[cfg(test)]
pub(crate) mod test_helpers;
