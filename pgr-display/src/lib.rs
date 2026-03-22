#![warn(clippy::pedantic)]
//! Terminal rendering, prompt evaluation, ANSI handling, color, and Unicode width.

pub mod ansi;
pub mod error;
pub mod unicode;
pub use error::{DisplayError, Result};
