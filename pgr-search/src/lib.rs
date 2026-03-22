#![warn(clippy::pedantic)]
//! Regex and literal search, highlighting, and filter mode.

pub mod error;
pub mod pattern;

pub use error::{Result, SearchError};
pub use pattern::{CaseMode, MatchRange, SearchPattern};
