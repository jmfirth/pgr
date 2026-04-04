#![warn(clippy::pedantic)]
//! Terminal rendering, prompt evaluation, ANSI handling, color, and Unicode width.

pub mod ansi;
pub mod blame_render;
pub mod charset;
pub mod color;
pub mod compiler_links;
pub mod diff_render;
pub mod error;
pub mod hyperlink;
pub mod line_numbers;
pub mod prompt;
pub mod render;
pub mod screen;
pub mod side_by_side;
pub mod squeeze;
#[cfg(feature = "syntax")]
pub mod syntax;
pub mod table_render;
pub mod termcap;
pub mod terminal_output;
pub mod unicode;
pub mod url;

pub use ansi::{AnsiState, OverstrikeMode};
pub use blame_render::colorize_blame_line;
#[cfg(feature = "syntax")]
pub use blame_render::colorize_blame_line_syntax;
pub use charset::{CharType, Charset};
pub use color::{Color, ColorAutoDetect, ColorConfig, ColorSelector, ColorSpec};
pub use compiler_links::linkify_compiler_output;
pub use diff_render::colorize_diff_line;
#[cfg(feature = "syntax")]
pub use diff_render::highlight_content;
#[cfg(feature = "syntax")]
pub use diff_render::highlight_diff_hunk;
pub use diff_render::tint_content;
pub use diff_render::{apply_word_emphasis, DiffSide};
pub use error::{DisplayError, Result};
pub use hyperlink::{parse_osc8, strip_osc8, HyperlinkSpan};
pub use line_numbers::{
    format_line_number, format_line_number_colored, line_number_width, line_number_width_custom,
};
pub use prompt::{
    eval_prompt, paint_info_line, paint_prompt, render_prompt, PromptContext, PromptStyle,
    DEFAULT_LONG_PROMPT, DEFAULT_MEDIUM_PROMPT, DEFAULT_SHORT_PROMPT,
};
pub use render::{
    render_line, render_line_highlighted, render_line_marked, render_line_multi_highlighted,
    BinFmt, BinFmtSegment, ColoredRange, RawControlMode, RenderConfig, TabStops,
};
pub use screen::Screen;
pub use side_by_side::{
    build_side_by_side_lines, pair_hunk_lines, render_side_by_side, rendered_line_width,
    SideBySideLayout, SideBySideLine, MIN_SIDE_BY_SIDE_COLS,
};
pub use squeeze::{is_blank_line, squeeze_visible_lines};
pub use table_render::{
    colorize_table_lines, first_column_width, parse_table_layout, render_frozen_column,
    snap_to_next_column, snap_to_prev_column, SqlTableLayout,
};
pub use termcap::TermcapOverrides;
pub use terminal_output::{
    clear_screen, compute_line_screen_rows, paint_error_message, paint_screen, paint_screen_mapped,
    paint_screen_with_options, wordwrap_segments, PaintOptions, ScreenLine,
};
pub use url::{find_urls, UrlMatch};
