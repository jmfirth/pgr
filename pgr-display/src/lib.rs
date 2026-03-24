#![warn(clippy::pedantic)]
//! Terminal rendering, prompt evaluation, ANSI handling, color, and Unicode width.

pub mod ansi;
pub mod charset;
pub mod color;
pub mod error;
pub mod line_numbers;
pub mod prompt;
pub mod render;
pub mod screen;
pub mod squeeze;
pub mod terminal_output;
pub mod unicode;

pub use ansi::{AnsiState, OverstrikeMode};
pub use charset::{CharType, Charset};
pub use color::{Color, ColorAutoDetect, ColorConfig, ColorSelector, ColorSpec};
pub use error::{DisplayError, Result};
pub use line_numbers::{
    format_line_number, format_line_number_colored, line_number_width, line_number_width_custom,
};
pub use prompt::{
    eval_prompt, paint_info_line, paint_prompt, render_prompt, PromptContext, PromptStyle,
    DEFAULT_LONG_PROMPT, DEFAULT_MEDIUM_PROMPT, DEFAULT_SHORT_PROMPT,
};
pub use render::{
    render_line, render_line_highlighted, render_line_marked, BinFmt, BinFmtSegment,
    RawControlMode, RenderConfig, TabStops,
};
pub use screen::Screen;
pub use squeeze::{is_blank_line, squeeze_visible_lines};
pub use terminal_output::{
    clear_screen, paint_error_message, paint_screen, paint_screen_mapped,
    paint_screen_with_options, PaintOptions, ScreenLine,
};
