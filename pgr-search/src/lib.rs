#![warn(clippy::pedantic)]
//! Regex and literal search, highlighting, and filter mode.

pub mod error;
pub mod filter;
pub mod filtered_index;
pub mod highlight;
pub mod modifiers;
pub mod pattern;
pub mod searcher;

pub use error::{Result, SearchError};
pub use filter::FilterState;
pub use filtered_index::FilteredLines;
pub use highlight::{find_matches_in_line, HighlightState};
pub use modifiers::SearchModifiers;
pub use pattern::{CaseMode, MatchRange, SearchPattern};
pub use searcher::{count_matches, find_match_index, SearchDirection, Searcher, WrapMode};
