#![warn(clippy::pedantic)]
//! Terminal rendering, prompt evaluation, ANSI handling, color, and Unicode width.

pub mod ansi;
pub mod error;
pub mod prompt;
pub mod render;
pub mod screen;
pub mod terminal_output;
pub mod unicode;

pub use ansi::{AnsiState, OverstrikeMode};
pub use error::{DisplayError, Result};
pub use prompt::{paint_prompt, render_prompt, PromptContext, PromptStyle};
pub use render::{render_line, RawControlMode, RenderConfig, TabStops};
pub use screen::Screen;
pub use terminal_output::paint_screen;
