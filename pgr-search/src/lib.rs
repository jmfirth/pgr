#![warn(clippy::pedantic)]
//! Regex and literal search, highlighting, and filter mode.

pub mod error;
pub mod filter;
pub mod filtered_index;
pub mod pattern;
pub mod searcher;

pub use error::{Result, SearchError};
pub use filter::FilterState;
pub use filtered_index::FilteredLines;
pub use pattern::{CaseMode, MatchRange, SearchPattern};
pub use searcher::{SearchDirection, Searcher, WrapMode};
