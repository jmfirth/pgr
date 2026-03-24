//! Command-line argument parsing for pgr.
//!
//! Supports the full set of less-compatible flags. Phase 2 flags (tags,
//! lesskey, mouse, etc.) are accepted and stored but not yet wired to
//! runtime behavior.

use std::path::PathBuf;

use clap::Parser;
use pgr_display::{PromptStyle, RawControlMode};

use crate::env::read_less_env;

/// Search case sensitivity mode derived from `-i` / `-I` flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // Wired in Task 113 (search integration)
pub enum CaseMode {
    /// Default: case-sensitive search.
    Sensitive,
    /// `-i`: smart case (insensitive unless pattern has uppercase).
    Smart,
    /// `-I`: always case-insensitive.
    Insensitive,
}

/// A modern pager — drop-in replacement for less.
#[derive(Debug, Parser)]
#[command(
    name = "pgr",
    about,
    disable_help_flag = true,
    disable_version_flag = true
)]
#[allow(clippy::struct_excessive_bools)] // CLI options struct — each bool maps to a distinct less flag
pub struct Options {
    /// Files to view.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    // ── Navigation flags ──────────────────────────────────────────────
    /// Set scroll window size (default: screen height).
    #[arg(short = 'z', long = "window")]
    pub window_size: Option<i32>,

    /// Maximum backward scroll limit.
    #[arg(short = 'h', long = "max-back-scroll")]
    pub max_back_scroll: Option<usize>,

    /// Maximum forward scroll limit.
    #[arg(short = 'y', long = "max-forw-scroll")]
    pub max_forw_scroll: Option<usize>,

    /// Target line for search results and goto (default: 1).
    /// Negative values count from bottom of screen.
    #[arg(short = 'j', long = "jump-target")]
    pub jump_target: Option<i32>,

    /// Horizontal scroll amount (default: half screen width).
    /// Note: the `-#` short flag is not supported by clap; use `--shift`.
    #[arg(long = "shift")]
    pub horizontal_shift: Option<usize>,

    // ── Scrolling behavior flags ──────────────────────────────────────
    /// Repaint from top instead of scrolling.
    #[arg(short = 'c', long = "clear-screen")]
    pub clear_screen: bool,

    /// Like -c but clear entire screen first.
    #[arg(short = 'C', long = "CLEAR-SCREEN")]
    pub clear_screen_full: bool,

    /// Suppress error messages for dumb terminals.
    #[arg(short = 'd', long = "dumb")]
    pub dumb_terminal: bool,

    // ── Search flags ──────────────────────────────────────────────────
    /// Smart case-insensitive search.
    #[arg(short = 'i', long = "ignore-case")]
    pub ignore_case: bool,

    /// Always case-insensitive search.
    #[arg(short = 'I', long = "IGNORE-CASE")]
    pub ignore_case_always: bool,

    /// Highlight only last match found.
    #[arg(short = 'g', long = "hilite-search")]
    pub hilite_search: bool,

    /// No search highlighting at all.
    #[arg(short = 'G', long = "HILITE-SEARCH")]
    pub no_hilite_search: bool,

    /// Start search after last displayed line.
    #[arg(short = 'a', long = "search-skip-screen")]
    pub search_skip_screen: bool,

    /// Start search from target line.
    #[arg(short = 'A', long = "SEARCH-SKIP-SCREEN")]
    pub search_skip_target: bool,

    /// Start with a search pattern.
    #[arg(short = 'p', long = "pattern")]
    pub initial_pattern: Option<String>,

    // ── Display flags ─────────────────────────────────────────────────
    /// Suppress line number calculation (optimization for very large files).
    #[arg(short = 'n', long = "line-numbers")]
    pub suppress_line_numbers: bool,

    /// Show line numbers in left margin.
    #[arg(short = 'N', long = "LINE-NUMBERS")]
    pub line_numbers: bool,

    /// Squeeze consecutive blank lines into one.
    #[arg(short = 's', long = "squeeze-blank-lines")]
    pub squeeze_blank_lines: bool,

    /// Chop (truncate) long lines instead of wrapping.
    #[arg(short = 'S', long = "chop-long-lines")]
    pub chop_long_lines: bool,

    /// Default backspace handling.
    #[arg(short = 'u', long = "underline-special")]
    pub underline_special: bool,

    /// Show control/backspace chars as-is.
    #[arg(short = 'U', long = "UNDERLINE-SPECIAL")]
    pub underline_special_all: bool,

    /// Highlight first new line after forward scroll.
    #[arg(short = 'w', long = "hilite-unread")]
    pub hilite_unread: bool,

    /// Highlight first new line after any forward movement.
    #[arg(short = 'W', long = "HILITE-UNREAD")]
    pub hilite_unread_all: bool,

    /// Set color for display element (can be specified multiple times).
    #[arg(short = 'D', long = "color")]
    pub color_specs: Vec<String>,

    /// Output raw ANSI color escape sequences (SGR only).
    #[arg(short = 'R', long = "RAW-CONTROL-CHARS")]
    pub raw_control_chars: bool,

    /// Output all control characters raw.
    #[arg(short = 'r', long = "raw-control-chars")]
    pub raw_all: bool,

    /// Don't show tilde (~) for lines past EOF.
    #[arg(long = "tilde")]
    pub tilde: bool,

    /// Character to use for right-scroll indicator.
    #[arg(long = "rscroll")]
    pub rscroll: Option<String>,

    /// Enable ANSI color output.
    #[arg(long = "use-color")]
    pub use_color: bool,

    /// Width of the line number column (default 7).
    #[arg(long = "line-num-width")]
    pub line_num_width: Option<usize>,

    // ── Prompt flags ──────────────────────────────────────────────────
    /// Use a medium prompt (filename and percent).
    #[arg(short = 'm', long = "long-prompt")]
    pub medium_prompt: bool,

    /// Use a long prompt (filename, lines, bytes, percent).
    #[arg(short = 'M', long = "LONG-PROMPT")]
    pub long_prompt: bool,

    /// Custom prompt string.
    #[arg(short = 'P', long = "prompt")]
    pub custom_prompt: Option<String>,

    // ── Quiet flags ───────────────────────────────────────────────────
    /// Suppress bell on first EOF.
    #[arg(short = 'q', long = "quiet", alias = "silent")]
    pub quiet: bool,

    /// Never ring bell.
    #[arg(short = 'Q', long = "QUIET", alias = "SILENT")]
    pub quiet_always: bool,

    // ── Exit behavior flags ───────────────────────────────────────────
    /// Quit at second end-of-file.
    #[arg(short = 'e', long = "quit-at-eof")]
    pub quit_at_eof: bool,

    /// Quit at first end-of-file.
    #[arg(short = 'E', long = "QUIT-AT-EOF")]
    pub quit_at_first_eof: bool,

    /// Quit if entire file fits on one screen.
    #[arg(short = 'F', long = "quit-if-one-screen")]
    pub quit_if_one_screen: bool,

    /// Exit on interrupt (Ctrl+C).
    #[arg(short = 'K', long = "quit-on-intr")]
    pub quit_on_intr: bool,

    // ── Terminal flags ────────────────────────────────────────────────
    /// Don't use terminfo init/deinit strings.
    #[arg(short = 'X', long = "no-init")]
    pub no_init: bool,

    /// Set tab stops (default 8).
    #[arg(short = 'x', long = "tabs", default_value = "8")]
    pub tab_width: usize,

    // ── Input flags ───────────────────────────────────────────────────
    /// Force open non-regular files.
    #[arg(short = 'f', long = "force")]
    pub force_open: bool,

    /// Ignore LESSOPEN.
    #[arg(short = 'L', long = "no-lessopen")]
    pub no_lessopen: bool,

    /// Buffer space (in KB) for each file.
    #[arg(short = 'b', long = "buffers")]
    pub buffer_size: Option<usize>,

    /// Don't allocate buffers for pipes automatically.
    #[arg(short = 'B', long = "auto-buffers")]
    pub auto_buffers: bool,

    // ── Output flags ──────────────────────────────────────────────────
    /// Log piped input to file.
    #[arg(short = 'o', long = "log-file")]
    pub log_file: Option<String>,

    /// Log piped input to file (overwrite).
    #[arg(short = 'O', long = "LOG-FILE")]
    pub log_file_overwrite: Option<String>,

    // ── Phase 2 flags (accepted, not yet wired) ──────────────────────
    /// Start at tag (Phase 2).
    #[arg(short = 't', long = "tag", hide = true)]
    pub tag: Option<String>,

    /// Tag file path (Phase 2).
    #[arg(short = 'T', long = "tag-file", hide = true)]
    pub tag_file: Option<String>,

    /// Lesskey file (Phase 2).
    #[arg(short = 'k', long = "lesskey-file", hide = true)]
    pub lesskey_file: Option<String>,

    /// Lesskey source file (Phase 2).
    #[arg(long = "lesskey-src", hide = true)]
    pub lesskey_src: Option<String>,

    /// Inline lesskey content (Phase 2).
    #[arg(long = "lesskey-content", hide = true)]
    pub lesskey_content: Option<String>,

    /// Show status column on the left edge, indicating search matches and marks.
    #[arg(short = 'J', long = "status-column")]
    pub status_column: bool,

    /// Enable mouse support (Phase 2).
    #[arg(long = "mouse", hide = true)]
    pub mouse: bool,

    /// Follow by name in follow mode (Phase 2).
    #[arg(long = "follow-name", hide = true)]
    pub follow_name: bool,

    // ── Meta flags ────────────────────────────────────────────────────
    /// Print version information and exit.
    #[arg(short = 'V', long = "version")]
    pub version: bool,

    /// Print help information and exit.
    #[arg(short = '?', long = "help")]
    pub help: bool,
}

impl Options {
    /// Parse command-line arguments, prepending flags from the `LESS`
    /// environment variable so that explicit arguments override the env.
    pub fn parse() -> Self {
        let env_args = read_less_env();
        let real_args: Vec<String> = std::env::args().collect();

        // Build merged argv: program name, env flags, then real flags (skip argv[0]).
        let mut merged = Vec::with_capacity(1 + env_args.len() + real_args.len());
        if let Some(prog) = real_args.first() {
            merged.push(prog.clone());
        } else {
            merged.push(String::from("pgr"));
        }
        merged.extend(env_args);
        if real_args.len() > 1 {
            merged.extend_from_slice(&real_args[1..]);
        }

        <Self as Parser>::parse_from(merged)
    }

    /// Parse from an explicit argument list (for testing).
    #[cfg(test)]
    pub fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        <Self as Parser>::parse_from(args)
    }

    /// Derive the prompt style from the `-m` / `-M` / `-P` flags.
    ///
    /// When `-P` is specified, the custom prompt template takes priority.
    /// GNU less supports an optional prefix on `-P` values (`s` for short,
    /// `m` for medium, `M` for long) which selects which prompt to customize.
    /// We strip the prefix and use the rest as a custom prompt template.
    /// Otherwise falls back to `-M` (long), `-m` (medium), or short.
    #[must_use]
    pub fn prompt_style(&self) -> PromptStyle {
        if let Some(ref raw) = self.custom_prompt {
            let template = strip_prompt_prefix(raw);
            PromptStyle::Custom(template.to_string())
        } else if self.long_prompt {
            PromptStyle::Long
        } else if self.medium_prompt {
            PromptStyle::Medium
        } else {
            PromptStyle::Short
        }
    }

    /// Derive the raw control mode from the `-r` / `-R` flags.
    #[must_use]
    pub fn raw_mode(&self) -> RawControlMode {
        if self.raw_all {
            RawControlMode::All
        } else if self.raw_control_chars {
            RawControlMode::AnsiOnly
        } else {
            RawControlMode::Off
        }
    }

    /// Derive the search case mode from `-i` / `-I` flags.
    #[must_use]
    #[allow(dead_code)] // Wired in Task 113 (search integration)
    pub fn case_mode(&self) -> CaseMode {
        if self.ignore_case_always {
            CaseMode::Insensitive
        } else if self.ignore_case {
            CaseMode::Smart
        } else {
            CaseMode::Sensitive
        }
    }
}

/// Strip an optional GNU less prompt prefix from a `-P` value.
///
/// GNU less allows the `-P` value to start with `s` (short), `m` (medium),
/// `M` (long), `h` (help), `=` (status), or `w` (waiting/EOF message) to
/// select which prompt to customize. The prefix is stripped and the remainder
/// is the template string.
///
/// If no recognized prefix is present, the entire string is the template.
fn strip_prompt_prefix(raw: &str) -> &str {
    let mut chars = raw.chars();
    match chars.next() {
        Some('s' | 'm' | 'M' | 'h' | '=' | 'w') => chars.as_str(),
        _ => raw,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Phase 0 backward compatibility ────────────────────────────────

    #[test]
    fn test_options_default_values_unchanged() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.files, vec![PathBuf::from("file.txt")]);
        assert!(!opts.line_numbers);
        assert!(!opts.chop_long_lines);
        assert!(!opts.raw_control_chars);
        assert!(!opts.raw_all);
        assert!(!opts.medium_prompt);
        assert!(!opts.long_prompt);
        assert!(!opts.quiet);
        assert!(!opts.quit_at_eof);
        assert!(!opts.quit_at_first_eof);
        assert!(!opts.quit_if_one_screen);
        assert!(!opts.no_init);
        assert_eq!(opts.tab_width, 8);
        assert!(!opts.version);
        assert!(!opts.help);
        // Phase 1 defaults
        assert!(!opts.ignore_case);
        assert!(!opts.ignore_case_always);
        assert!(!opts.hilite_search);
        assert!(!opts.no_hilite_search);
        assert!(!opts.squeeze_blank_lines);
        assert!(!opts.suppress_line_numbers);
        assert!(!opts.hilite_unread);
        assert!(!opts.hilite_unread_all);
        assert!(!opts.dumb_terminal);
        assert!(!opts.clear_screen);
        assert!(!opts.clear_screen_full);
        assert!(!opts.force_open);
        assert!(!opts.quit_on_intr);
        assert!(!opts.search_skip_screen);
        assert!(!opts.search_skip_target);
        assert!(!opts.auto_buffers);
        assert!(!opts.no_lessopen);
        assert!(!opts.quiet_always);
        assert!(!opts.tilde);
        assert!(!opts.use_color);
        assert!(opts.custom_prompt.is_none());
        assert!(opts.initial_pattern.is_none());
        assert!(opts.window_size.is_none());
        assert!(opts.jump_target.is_none());
        assert!(opts.log_file.is_none());
        assert!(opts.buffer_size.is_none());
        assert!(opts.rscroll.is_none());
        assert!(opts.line_num_width.is_none());
        assert!(opts.color_specs.is_empty());
    }

    #[test]
    fn test_options_file_only_sets_defaults() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.files, vec![PathBuf::from("file.txt")]);
        assert!(!opts.line_numbers);
        assert!(!opts.chop_long_lines);
        assert!(!opts.raw_control_chars);
        assert!(!opts.raw_all);
        assert!(!opts.medium_prompt);
        assert!(!opts.long_prompt);
        assert!(!opts.quiet);
        assert!(!opts.quit_at_eof);
        assert!(!opts.quit_at_first_eof);
        assert!(!opts.quit_if_one_screen);
        assert!(!opts.no_init);
        assert_eq!(opts.tab_width, 8);
        assert!(!opts.version);
        assert!(!opts.help);
    }

    #[test]
    fn test_options_dash_r_upper_sets_raw_control_chars() {
        let opts = Options::parse_from(["pgr", "-R", "file.txt"]);
        assert!(opts.raw_control_chars);
        assert_eq!(opts.raw_mode(), RawControlMode::AnsiOnly);
    }

    #[test]
    fn test_options_dash_s_upper_sets_chop_long_lines() {
        let opts = Options::parse_from(["pgr", "-S", "file.txt"]);
        assert!(opts.chop_long_lines);
    }

    #[test]
    fn test_options_dash_m_upper_sets_long_prompt() {
        let opts = Options::parse_from(["pgr", "-M", "file.txt"]);
        assert!(opts.long_prompt);
        assert_eq!(opts.prompt_style(), PromptStyle::Long);
    }

    #[test]
    fn test_options_combined_flags_rs_both_set() {
        let opts = Options::parse_from(["pgr", "-RS", "file.txt"]);
        assert!(opts.raw_control_chars);
        assert!(opts.chop_long_lines);
    }

    #[test]
    fn test_options_dash_x_sets_tab_width() {
        let opts = Options::parse_from(["pgr", "-x4", "file.txt"]);
        assert_eq!(opts.tab_width, 4);
    }

    #[test]
    fn test_options_dash_v_upper_sets_version() {
        let opts = Options::parse_from(["pgr", "-V"]);
        assert!(opts.version);
    }

    #[test]
    fn test_options_no_files_results_in_empty_vec() {
        let opts = Options::parse_from(["pgr"]);
        assert!(opts.files.is_empty());
    }

    #[test]
    fn test_options_prompt_style_defaults_to_short() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.prompt_style(), PromptStyle::Short);
    }

    #[test]
    fn test_options_dash_m_lower_sets_medium_prompt() {
        let opts = Options::parse_from(["pgr", "-m", "file.txt"]);
        assert!(opts.medium_prompt);
        assert_eq!(opts.prompt_style(), PromptStyle::Medium);
    }

    #[test]
    fn test_options_raw_mode_defaults_to_off() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.raw_mode(), RawControlMode::Off);
    }

    #[test]
    fn test_options_dash_r_lower_sets_raw_all() {
        let opts = Options::parse_from(["pgr", "-r", "file.txt"]);
        assert!(opts.raw_all);
        assert_eq!(opts.raw_mode(), RawControlMode::All);
    }

    // ── Phase 1 flag tests ────────────────────────────────────────────

    #[test]
    fn test_options_dash_i_sets_ignore_case() {
        let opts = Options::parse_from(["pgr", "-i", "file.txt"]);
        assert!(opts.ignore_case);
    }

    #[test]
    fn test_options_dash_upper_i_sets_ignore_case_always() {
        let opts = Options::parse_from(["pgr", "-I", "file.txt"]);
        assert!(opts.ignore_case_always);
    }

    #[test]
    fn test_options_dash_g_sets_hilite_search() {
        let opts = Options::parse_from(["pgr", "-g", "file.txt"]);
        assert!(opts.hilite_search);
    }

    #[test]
    fn test_options_dash_upper_g_sets_no_hilite_search() {
        let opts = Options::parse_from(["pgr", "-G", "file.txt"]);
        assert!(opts.no_hilite_search);
    }

    #[test]
    fn test_options_dash_s_lower_sets_squeeze() {
        let opts = Options::parse_from(["pgr", "-s", "file.txt"]);
        assert!(opts.squeeze_blank_lines);
    }

    #[test]
    fn test_options_dash_n_lower_sets_suppress_line_numbers() {
        let opts = Options::parse_from(["pgr", "-n", "file.txt"]);
        assert!(opts.suppress_line_numbers);
    }

    #[test]
    fn test_options_dash_upper_n_sets_line_numbers() {
        let opts = Options::parse_from(["pgr", "-N", "file.txt"]);
        assert!(opts.line_numbers);
    }

    #[test]
    fn test_options_dash_w_sets_hilite_unread() {
        let opts = Options::parse_from(["pgr", "-w", "file.txt"]);
        assert!(opts.hilite_unread);
    }

    #[test]
    fn test_options_dash_upper_w_sets_hilite_unread_all() {
        let opts = Options::parse_from(["pgr", "-W", "file.txt"]);
        assert!(opts.hilite_unread_all);
    }

    #[test]
    fn test_options_dash_d_sets_dumb() {
        let opts = Options::parse_from(["pgr", "-d", "file.txt"]);
        assert!(opts.dumb_terminal);
    }

    #[test]
    fn test_options_dash_c_sets_clear_screen() {
        let opts = Options::parse_from(["pgr", "-c", "file.txt"]);
        assert!(opts.clear_screen);
    }

    #[test]
    fn test_options_dash_f_sets_force() {
        let opts = Options::parse_from(["pgr", "-f", "file.txt"]);
        assert!(opts.force_open);
    }

    #[test]
    fn test_options_dash_upper_k_sets_quit_on_intr() {
        let opts = Options::parse_from(["pgr", "-K", "file.txt"]);
        assert!(opts.quit_on_intr);
    }

    #[test]
    fn test_options_dash_upper_p_sets_custom_prompt() {
        let opts = Options::parse_from(["pgr", "-P", "%f", "file.txt"]);
        assert_eq!(opts.custom_prompt.as_deref(), Some("%f"));
    }

    #[test]
    fn test_options_dash_p_sets_initial_pattern() {
        let opts = Options::parse_from(["pgr", "-p", "error", "file.txt"]);
        assert_eq!(opts.initial_pattern.as_deref(), Some("error"));
    }

    #[test]
    fn test_options_dash_z_sets_window_size() {
        let opts = Options::parse_from(["pgr", "-z", "20", "file.txt"]);
        assert_eq!(opts.window_size, Some(20));
    }

    #[test]
    fn test_options_dash_j_sets_jump_target() {
        let opts = Options::parse_from(["pgr", "-j", "5", "file.txt"]);
        assert_eq!(opts.jump_target, Some(5));
    }

    #[test]
    fn test_options_dash_o_sets_log_file() {
        let opts = Options::parse_from(["pgr", "-o", "file.log", "file.txt"]);
        assert_eq!(opts.log_file.as_deref(), Some("file.log"));
    }

    #[test]
    fn test_options_dash_upper_d_collects_color_specs() {
        let opts = Options::parse_from(["pgr", "-D", "S1", "file.txt"]);
        assert_eq!(opts.color_specs, vec!["S1"]);
    }

    #[test]
    fn test_options_multiple_color_specs() {
        let opts = Options::parse_from(["pgr", "-D", "S1", "-D", "N2", "file.txt"]);
        assert_eq!(opts.color_specs, vec!["S1", "N2"]);
    }

    #[test]
    fn test_options_prompt_style_custom_with_dash_p() {
        let opts = Options::parse_from(["pgr", "-P", "%f:%l", "file.txt"]);
        assert!(opts.custom_prompt.is_some());
        assert_eq!(opts.custom_prompt.as_deref(), Some("%f:%l"));
        assert_eq!(
            opts.prompt_style(),
            PromptStyle::Custom(String::from("%f:%l"))
        );
    }

    #[test]
    fn test_options_prompt_style_custom_with_prefix_strip() {
        // `-Ps"page %d"` — the `s` prefix selects the short prompt and is stripped.
        let opts = Options::parse_from(["pgr", "-P", "s\"page %d\"", "file.txt"]);
        assert_eq!(
            opts.prompt_style(),
            PromptStyle::Custom(String::from("\"page %d\""))
        );
    }

    #[test]
    fn test_strip_prompt_prefix_known_prefixes() {
        assert_eq!(strip_prompt_prefix("s%f"), "%f");
        assert_eq!(strip_prompt_prefix("m%f:%l"), "%f:%l");
        assert_eq!(strip_prompt_prefix("M%f lines"), "%f lines");
        assert_eq!(strip_prompt_prefix("h(help)"), "(help)");
        assert_eq!(strip_prompt_prefix("=status"), "status");
        assert_eq!(strip_prompt_prefix("wwaiting"), "waiting");
    }

    #[test]
    fn test_strip_prompt_prefix_no_prefix() {
        assert_eq!(strip_prompt_prefix("%f:%l"), "%f:%l");
        assert_eq!(strip_prompt_prefix(""), "");
        assert_eq!(strip_prompt_prefix("page %d"), "page %d");
    }

    #[test]
    fn test_options_prompt_style_default_without_dash_p() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.prompt_style(), PromptStyle::Short);
        let opts = Options::parse_from(["pgr", "-m", "file.txt"]);
        assert_eq!(opts.prompt_style(), PromptStyle::Medium);
        let opts = Options::parse_from(["pgr", "-M", "file.txt"]);
        assert_eq!(opts.prompt_style(), PromptStyle::Long);
    }

    #[test]
    fn test_options_phase2_flags_accepted() {
        let opts = Options::parse_from([
            "pgr", "-t", "mytag", "-T", "tags", "-k", "keys.bin", "file.txt",
        ]);
        assert_eq!(opts.tag.as_deref(), Some("mytag"));
        assert_eq!(opts.tag_file.as_deref(), Some("tags"));
        assert_eq!(opts.lesskey_file.as_deref(), Some("keys.bin"));
    }

    #[test]
    fn test_options_combined_search_flags() {
        let opts = Options::parse_from(["pgr", "-iR", "file.txt"]);
        assert!(opts.ignore_case);
        assert!(opts.raw_control_chars);
    }

    #[test]
    fn test_options_dash_a_sets_search_skip() {
        let opts = Options::parse_from(["pgr", "-a", "file.txt"]);
        assert!(opts.search_skip_screen);
    }

    #[test]
    fn test_options_dash_b_sets_buffer_size() {
        let opts = Options::parse_from(["pgr", "-b", "64", "file.txt"]);
        assert_eq!(opts.buffer_size, Some(64));
    }

    #[test]
    fn test_options_dash_upper_b_sets_auto_buffers() {
        let opts = Options::parse_from(["pgr", "-B", "file.txt"]);
        assert!(opts.auto_buffers);
    }

    #[test]
    fn test_options_dash_upper_l_sets_no_lessopen() {
        let opts = Options::parse_from(["pgr", "-L", "file.txt"]);
        assert!(opts.no_lessopen);
    }

    #[test]
    fn test_options_case_mode_derivation() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.case_mode(), CaseMode::Sensitive);

        let opts = Options::parse_from(["pgr", "-i", "file.txt"]);
        assert_eq!(opts.case_mode(), CaseMode::Smart);

        let opts = Options::parse_from(["pgr", "-I", "file.txt"]);
        assert_eq!(opts.case_mode(), CaseMode::Insensitive);
    }

    // ── Review-required flags ─────────────────────────────────────────

    #[test]
    fn test_options_dash_q_sets_quiet() {
        let opts = Options::parse_from(["pgr", "-q", "file.txt"]);
        assert!(opts.quiet);
    }

    #[test]
    fn test_options_dash_upper_q_sets_quiet_always() {
        let opts = Options::parse_from(["pgr", "-Q", "file.txt"]);
        assert!(opts.quiet_always);
    }

    #[test]
    fn test_options_quiet_long_aliases() {
        let opts = Options::parse_from(["pgr", "--silent", "file.txt"]);
        assert!(opts.quiet);
        let opts = Options::parse_from(["pgr", "--SILENT", "file.txt"]);
        assert!(opts.quiet_always);
    }

    #[test]
    fn test_options_dash_upper_s_sets_chop() {
        let opts = Options::parse_from(["pgr", "-S", "file.txt"]);
        assert!(opts.chop_long_lines);
    }

    #[test]
    fn test_options_dash_upper_x_sets_no_init() {
        let opts = Options::parse_from(["pgr", "-X", "file.txt"]);
        assert!(opts.no_init);
    }

    #[test]
    fn test_options_tilde_flag() {
        let opts = Options::parse_from(["pgr", "--tilde", "file.txt"]);
        assert!(opts.tilde);
    }

    #[test]
    fn test_options_rscroll_flag() {
        let opts = Options::parse_from(["pgr", "--rscroll", ">", "file.txt"]);
        assert_eq!(opts.rscroll.as_deref(), Some(">"));
    }

    #[test]
    fn test_options_use_color_flag() {
        let opts = Options::parse_from(["pgr", "--use-color", "file.txt"]);
        assert!(opts.use_color);
    }

    #[test]
    fn test_options_line_num_width_flag() {
        let opts = Options::parse_from(["pgr", "--line-num-width", "10", "file.txt"]);
        assert_eq!(opts.line_num_width, Some(10));
    }

    // ── Additional Phase 2 coverage ───────────────────────────────────

    #[test]
    fn test_options_phase2_bool_flags_accepted() {
        let opts = Options::parse_from(["pgr", "-J", "--mouse", "--follow-name", "file.txt"]);
        assert!(opts.status_column);
        assert!(opts.mouse);
        assert!(opts.follow_name);
    }

    #[test]
    fn test_options_phase2_lesskey_flags_accepted() {
        let opts = Options::parse_from([
            "pgr",
            "--lesskey-src",
            "src.txt",
            "--lesskey-content",
            "inline",
            "file.txt",
        ]);
        assert_eq!(opts.lesskey_src.as_deref(), Some("src.txt"));
        assert_eq!(opts.lesskey_content.as_deref(), Some("inline"));
    }
}
