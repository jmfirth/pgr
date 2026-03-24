//! Command-line argument parsing for pgr.
//!
//! Supports the full set of less-compatible flags. Phase 2 flags (tags,
//! lesskey, mouse, etc.) are accepted and stored but not yet wired to
//! runtime behavior.

use std::path::PathBuf;

use clap::Parser;
use pgr_display::{PromptStyle, RawControlMode};

use crate::env::{read_less_env_split, split_flags_and_commands};

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

    /// Custom prompt string(s). Can be specified multiple times with per-slot
    /// prefixes: `-Ps` (short), `-Pm` (medium), `-PM` (long).
    #[arg(short = 'P', long = "prompt")]
    pub custom_prompts: Vec<String>,

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

    // ── Tag flags ─────────────────────────────────────────────────────
    /// Open the file containing the specified tag.
    #[arg(short = 't', long = "tag")]
    pub tag: Option<String>,

    /// Use the specified tags file (default: "tags").
    #[arg(short = 'T', long = "tag-file")]
    pub tag_file: Option<String>,

    /// Lesskey binary file (not supported; use --lesskey-src for source format).
    #[arg(short = 'k', long = "lesskey-file", hide = true)]
    pub lesskey_file: Option<String>,

    /// Lesskey source file to load custom key bindings from.
    #[arg(long = "lesskey-src")]
    pub lesskey_src: Option<String>,

    /// Inline lesskey content (Phase 2).
    #[arg(long = "lesskey-content", hide = true)]
    pub lesskey_content: Option<String>,

    /// Show status column on the left edge, indicating search matches and marks.
    #[arg(short = 'J', long = "status-column")]
    pub status_column: bool,

    /// Pin header lines at top of screen (format: L[,C[,N]]).
    #[arg(long = "header")]
    pub header: Option<String>,

    /// Enable mouse support (Phase 2).
    #[arg(long = "mouse", hide = true)]
    pub mouse: bool,

    /// Enable mouse wheel scrolling with reversed direction.
    #[arg(long = "MOUSE")]
    pub mouse_reversed: bool,

    /// Number of lines to scroll per mouse wheel tick (default: 3).
    #[arg(long = "wheel-lines")]
    pub wheel_lines: Option<usize>,

    /// Follow by name in follow mode: reopen file by path on rename/delete.
    #[arg(long = "follow-name")]
    pub follow_name: bool,

    /// Exit follow mode when the input pipe closes (EOF on stdin).
    #[arg(long = "exit-follow-on-close")]
    pub exit_follow_on_close: bool,

    // ── Meta flags ────────────────────────────────────────────────────
    /// Print version information and exit.
    #[arg(short = 'V', long = "version")]
    pub version: bool,

    /// Print help information and exit.
    #[arg(short = '?', long = "help")]
    pub help: bool,

    // ── Initial commands (not clap flags — populated by parse()) ─────
    /// Commands to execute after opening the first file (`+cmd` syntax).
    #[arg(skip)]
    pub initial_commands: Vec<String>,

    /// Commands to execute after opening every file (`++cmd` syntax).
    #[arg(skip)]
    pub every_file_commands: Vec<String>,
}

impl Options {
    /// Parse command-line arguments, prepending flags from the `LESS`
    /// environment variable so that explicit arguments override the env.
    ///
    /// Arguments starting with `+` are extracted as initial commands
    /// (`+cmd`) or every-file commands (`++cmd`) before passing the
    /// remaining flags to clap.
    pub fn parse() -> Self {
        let (env_flags, env_initial, env_every_file) = read_less_env_split();
        let real_args: Vec<String> = std::env::args().collect();

        // Separate `+cmd`/`++cmd` from regular args in the CLI arguments.
        let cli_tokens: Vec<String> = if real_args.len() > 1 {
            real_args[1..].to_vec()
        } else {
            Vec::new()
        };
        let (cli_flags, cli_initial, cli_every_file) = split_flags_and_commands(&cli_tokens);

        // Build merged argv: program name, env flags, then real flags.
        let mut merged = Vec::with_capacity(1 + env_flags.len() + cli_flags.len());
        if let Some(prog) = real_args.first() {
            merged.push(prog.clone());
        } else {
            merged.push(String::from("pgr"));
        }
        merged.extend(env_flags);
        merged.extend(cli_flags);

        let mut opts = <Self as Parser>::parse_from(merged);

        // Env commands first, then CLI commands (CLI takes precedence by running after).
        opts.initial_commands = env_initial;
        opts.initial_commands.extend(cli_initial);
        opts.every_file_commands = env_every_file;
        opts.every_file_commands.extend(cli_every_file);

        opts
    }

    /// Parse from an explicit argument list (for testing).
    ///
    /// Extracts `+cmd` and `++cmd` arguments before passing the rest to clap,
    /// matching the behavior of the real `parse()` method.
    #[cfg(test)]
    pub fn parse_from<I, T>(args: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let all_args: Vec<String> = args
            .into_iter()
            .map(|a| a.into().to_string_lossy().into_owned())
            .collect();

        // First element is program name.
        let (prog, rest) = if all_args.is_empty() {
            (String::from("pgr"), Vec::new())
        } else {
            (all_args[0].clone(), all_args[1..].to_vec())
        };

        let (flags, initial, every_file) = split_flags_and_commands(&rest);

        let mut clap_args = Vec::with_capacity(1 + flags.len());
        clap_args.push(prog);
        clap_args.extend(flags);

        let mut opts = <Self as Parser>::parse_from(clap_args);
        opts.initial_commands = initial;
        opts.every_file_commands = every_file;
        opts
    }

    /// Derive the prompt style from the `-m` / `-M` flags.
    ///
    /// The `-P` flag provides per-slot template overrides (via
    /// [`custom_prompt_overrides`]) but does not change the base prompt style.
    /// Falls back to `-M` (long), `-m` (medium), or short.
    #[must_use]
    pub fn prompt_style(&self) -> PromptStyle {
        if self.long_prompt {
            PromptStyle::Long
        } else if self.medium_prompt {
            PromptStyle::Medium
        } else {
            PromptStyle::Short
        }
    }

    /// Extract per-slot custom prompt template overrides from `-P` flags.
    ///
    /// GNU less supports an optional prefix on `-P` values (`s` for short,
    /// `m` for medium, `M` for long) to select which prompt to customize.
    /// Unprefixed values default to the short prompt slot. Multiple `-P`
    /// flags can each override a different slot.
    ///
    /// Returns `(short, medium, long)` overrides.
    #[must_use]
    pub fn custom_prompt_overrides(&self) -> (Option<String>, Option<String>, Option<String>) {
        let mut short = None;
        let mut medium = None;
        let mut long = None;

        for raw in &self.custom_prompts {
            let (prefix, template) = strip_prompt_prefix(raw);
            match prefix {
                Some('m') => medium = Some(template.to_owned()),
                Some('M') => long = Some(template.to_owned()),
                Some('s') | None => short = Some(template.to_owned()),
                // h, =, w prefixes are lower priority — ignore for now
                _ => {}
            }
        }

        (short, medium, long)
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

    /// Parse the `--header` flag into `(lines, cols, gap)`.
    ///
    /// Accepts formats: `L`, `L,C`, or `L,C,N`. Defaults to 0 for
    /// omitted components. Returns `(0, 0, 0)` if no header flag is set.
    #[must_use]
    pub fn header_params(&self) -> (usize, usize, usize) {
        let Some(ref raw) = self.header else {
            return (0, 0, 0);
        };
        let parts: Vec<&str> = raw.split(',').collect();
        let lines = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let cols = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let gap = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        (lines, cols, gap)
    }
}

/// Strip an optional GNU less prompt prefix from a `-P` value.
///
/// GNU less allows the `-P` value to start with `s` (short), `m` (medium),
/// `M` (long), `h` (help), `=` (status), or `w` (waiting/EOF message) to
/// select which prompt to customize. Returns the prefix character (if any)
/// and the template string.
///
/// If no recognized prefix is present, `None` is returned for the prefix
/// and the entire string is the template.
fn strip_prompt_prefix(raw: &str) -> (Option<char>, &str) {
    let mut chars = raw.chars();
    match chars.next() {
        Some(c @ ('s' | 'm' | 'M' | 'h' | '=' | 'w')) => (Some(c), chars.as_str()),
        _ => (None, raw),
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
        assert!(opts.custom_prompts.is_empty());
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
        assert_eq!(opts.custom_prompts, vec!["%f"]);
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
    fn test_options_dash_p_unprefixed_overrides_short_slot() {
        let opts = Options::parse_from(["pgr", "-P", "%f:%l", "file.txt"]);
        assert_eq!(opts.custom_prompts, vec!["%f:%l"]);
        // Unprefixed -P does not change prompt style
        assert_eq!(opts.prompt_style(), PromptStyle::Short);
        let (short, medium, long) = opts.custom_prompt_overrides();
        assert_eq!(short.as_deref(), Some("%f:%l"));
        assert!(medium.is_none());
        assert!(long.is_none());
    }

    #[test]
    fn test_options_dash_ps_overrides_short_slot() {
        let opts = Options::parse_from(["pgr", "-P", "spage %d", "file.txt"]);
        let (short, medium, long) = opts.custom_prompt_overrides();
        assert_eq!(short.as_deref(), Some("page %d"));
        assert!(medium.is_none());
        assert!(long.is_none());
    }

    #[test]
    fn test_options_dash_pm_overrides_medium_slot() {
        let opts = Options::parse_from(["pgr", "-P", "m%f %pB%%", "file.txt"]);
        let (short, medium, long) = opts.custom_prompt_overrides();
        assert!(short.is_none());
        assert_eq!(medium.as_deref(), Some("%f %pB%%"));
        assert!(long.is_none());
    }

    #[test]
    fn test_options_dash_p_upper_m_overrides_long_slot() {
        let opts = Options::parse_from(["pgr", "-P", "M%f lines %lt-%lb", "file.txt"]);
        let (short, medium, long) = opts.custom_prompt_overrides();
        assert!(short.is_none());
        assert!(medium.is_none());
        assert_eq!(long.as_deref(), Some("%f lines %lt-%lb"));
    }

    #[test]
    fn test_options_multiple_dash_p_flags_each_override_slot() {
        let opts = Options::parse_from([
            "pgr",
            "-P",
            "sshort custom",
            "-P",
            "mmedium custom",
            "-P",
            "Mlong custom",
            "file.txt",
        ]);
        let (short, medium, long) = opts.custom_prompt_overrides();
        assert_eq!(short.as_deref(), Some("short custom"));
        assert_eq!(medium.as_deref(), Some("medium custom"));
        assert_eq!(long.as_deref(), Some("long custom"));
    }

    #[test]
    fn test_strip_prompt_prefix_known_prefixes() {
        assert_eq!(strip_prompt_prefix("s%f"), (Some('s'), "%f"));
        assert_eq!(strip_prompt_prefix("m%f:%l"), (Some('m'), "%f:%l"));
        assert_eq!(strip_prompt_prefix("M%f lines"), (Some('M'), "%f lines"));
        assert_eq!(strip_prompt_prefix("h(help)"), (Some('h'), "(help)"));
        assert_eq!(strip_prompt_prefix("=status"), (Some('='), "status"));
        assert_eq!(strip_prompt_prefix("wwaiting"), (Some('w'), "waiting"));
    }

    #[test]
    fn test_strip_prompt_prefix_no_prefix() {
        assert_eq!(strip_prompt_prefix("%f:%l"), (None, "%f:%l"));
        assert_eq!(strip_prompt_prefix(""), (None, ""));
        assert_eq!(strip_prompt_prefix("page %d"), (None, "page %d"));
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
    fn test_options_exit_follow_on_close_flag_accepted() {
        let opts = Options::parse_from(["pgr", "--exit-follow-on-close", "file.txt"]);
        assert!(opts.exit_follow_on_close);
    }

    #[test]
    fn test_options_follow_name_and_exit_follow_on_close_combined() {
        let opts =
            Options::parse_from(["pgr", "--follow-name", "--exit-follow-on-close", "file.txt"]);
        assert!(opts.follow_name);
        assert!(opts.exit_follow_on_close);
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

    // ── Initial command (+cmd / ++cmd) tests ─────────────────────────

    #[test]
    fn test_options_plus_g_parsed_as_initial_command() {
        let opts = Options::parse_from(["pgr", "+G", "file.txt"]);
        assert_eq!(opts.initial_commands, vec!["G"]);
        assert_eq!(opts.files, vec![PathBuf::from("file.txt")]);
    }

    #[test]
    fn test_options_plus_plus_g_parsed_as_every_file_command() {
        let opts = Options::parse_from(["pgr", "++G", "file.txt"]);
        assert_eq!(opts.every_file_commands, vec!["G"]);
        assert!(opts.initial_commands.is_empty());
    }

    #[test]
    fn test_options_plus_search_parsed_as_initial_command() {
        let opts = Options::parse_from(["pgr", "+/pattern", "file.txt"]);
        assert_eq!(opts.initial_commands, vec!["/pattern"]);
    }

    #[test]
    fn test_options_multiple_plus_commands_preserve_order() {
        let opts = Options::parse_from(["pgr", "+G", "+g", "file.txt"]);
        assert_eq!(opts.initial_commands, vec!["G", "g"]);
    }

    #[test]
    fn test_options_mixed_flags_and_plus_commands() {
        let opts = Options::parse_from(["pgr", "-R", "+G", "-S", "file.txt"]);
        assert!(opts.raw_control_chars);
        assert!(opts.chop_long_lines);
        assert_eq!(opts.initial_commands, vec!["G"]);
        assert_eq!(opts.files, vec![PathBuf::from("file.txt")]);
    }

    #[test]
    fn test_options_plus_number_g_parsed_as_initial_command() {
        let opts = Options::parse_from(["pgr", "+100g", "file.txt"]);
        assert_eq!(opts.initial_commands, vec!["100g"]);
    }

    #[test]
    fn test_options_plus_gg_parsed_as_initial_command() {
        let opts = Options::parse_from(["pgr", "+Gg", "file.txt"]);
        assert_eq!(opts.initial_commands, vec!["Gg"]);
    }

    #[test]
    fn test_options_no_plus_args_yields_empty_commands() {
        let opts = Options::parse_from(["pgr", "-R", "file.txt"]);
        assert!(opts.initial_commands.is_empty());
        assert!(opts.every_file_commands.is_empty());
    }

    #[test]
    fn test_options_bare_plus_ignored() {
        let opts = Options::parse_from(["pgr", "+", "file.txt"]);
        assert!(opts.initial_commands.is_empty());
    }

    #[test]
    fn test_options_bare_plus_plus_ignored() {
        let opts = Options::parse_from(["pgr", "++", "file.txt"]);
        assert!(opts.every_file_commands.is_empty());
    }

    // ── Task 215: Tag flags ─────────────────────────────────────────

    #[test]
    fn test_options_dash_t_sets_tag() {
        let opts = Options::parse_from(["pgr", "-t", "main"]);
        assert_eq!(opts.tag.as_deref(), Some("main"));
    }

    #[test]
    fn test_options_dash_upper_t_sets_tag_file() {
        let opts = Options::parse_from(["pgr", "-T", "TAGS", "file.txt"]);
        assert_eq!(opts.tag_file.as_deref(), Some("TAGS"));
    }

    #[test]
    fn test_options_tag_default_is_none() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert!(opts.tag.is_none());
        assert!(opts.tag_file.is_none());
    }

    // ── Header flag tests ────────────────────────────────────────────

    #[test]
    fn test_options_header_default_is_none() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert!(opts.header.is_none());
        assert_eq!(opts.header_params(), (0, 0, 0));
    }

    #[test]
    fn test_options_header_lines_only() {
        let opts = Options::parse_from(["pgr", "--header=3", "file.txt"]);
        assert_eq!(opts.header_params(), (3, 0, 0));
    }

    #[test]
    fn test_options_header_lines_and_cols() {
        let opts = Options::parse_from(["pgr", "--header=3,2", "file.txt"]);
        assert_eq!(opts.header_params(), (3, 2, 0));
    }

    #[test]
    fn test_options_header_lines_cols_and_gap() {
        let opts = Options::parse_from(["pgr", "--header=3,2,1", "file.txt"]);
        assert_eq!(opts.header_params(), (3, 2, 1));
    }

    #[test]
    fn test_options_header_invalid_values_default_to_zero() {
        let opts = Options::parse_from(["pgr", "--header=abc", "file.txt"]);
        assert_eq!(opts.header_params(), (0, 0, 0));
    }
}
