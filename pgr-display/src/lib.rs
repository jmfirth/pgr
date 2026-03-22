#![warn(clippy::pedantic)]
//! Terminal rendering, prompt evaluation, ANSI handling, color, and Unicode width.

pub mod ansi;
pub mod color;
pub mod error;
pub mod prompt;
pub mod render;
pub mod screen;
pub mod terminal_output;
pub mod unicode;

pub use color::{Color, ColorConfig, ColorSelector, ColorSpec};
pub use error::{DisplayError, Result};
pub use prompt::{eval_prompt, paint_prompt, render_prompt, PromptContext, PromptStyle};
pub use render::{render_line, RawControlMode};
pub use screen::Screen;
pub use terminal_output::paint_screen;
