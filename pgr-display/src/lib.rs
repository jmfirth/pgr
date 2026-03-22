#![warn(clippy::pedantic)]
//! Terminal rendering, prompt evaluation, ANSI handling, color, and Unicode width.

pub mod ansi;
pub mod error;
pub mod render;
pub mod screen;
pub mod terminal_output;
pub mod unicode;

pub use error::{DisplayError, Result};
pub use render::{render_line, RawControlMode};
pub use screen::Screen;
pub use terminal_output::paint_screen;
