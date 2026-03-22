//! Command-line argument parsing for pgr.
//!
//! Phase 0 supports a minimal subset of less-compatible flags. The full
//! flag set (~60 flags) is deferred to Phase 1.

use std::path::PathBuf;

use clap::Parser;
use pgr_display::{PromptStyle, RawControlMode};

use crate::env::read_less_env;

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

    /// Show line numbers.
    #[arg(short = 'N', long = "LINE-NUMBERS")]
    pub line_numbers: bool,

    /// Chop (truncate) long lines instead of wrapping.
    #[arg(short = 'S', long = "chop-long-lines")]
    pub chop_long_lines: bool,

    /// Output raw ANSI color escape sequences (SGR only).
    #[arg(short = 'R', long = "RAW-CONTROL-CHARS")]
    pub raw_control_chars: bool,

    /// Output all control characters raw.
    #[arg(short = 'r', long = "raw-control-chars")]
    pub raw_all: bool,

    /// Use a medium prompt (filename and percent).
    #[arg(short = 'm', long = "long-prompt")]
    pub medium_prompt: bool,

    /// Use a long prompt (filename, lines, bytes, percent).
    #[arg(short = 'M', long = "LONG-PROMPT")]
    pub long_prompt: bool,

    /// Quiet — suppress terminal bell.
    #[arg(short = 'q', long = "quiet", alias = "silent")]
    pub quiet: bool,

    /// Quit at second end-of-file.
    #[arg(short = 'e', long = "quit-at-eof")]
    pub quit_at_eof: bool,

    /// Quit at first end-of-file.
    #[arg(short = 'E', long = "QUIT-AT-EOF")]
    pub quit_at_first_eof: bool,

    /// Quit if entire file fits on one screen.
    #[arg(short = 'F', long = "quit-if-one-screen")]
    pub quit_if_one_screen: bool,

    /// Don't clear the screen on init/exit.
    #[arg(short = 'X', long = "no-init")]
    pub no_init: bool,

    /// Set tab stops (default 8).
    #[arg(short = 'x', long = "tabs", default_value = "8")]
    pub tab_width: usize,

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

    /// Derive the prompt style from the `-m` / `-M` flags.
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
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 1: pgr file.txt — files = ["file.txt"], all flags default
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

    // Test 2: pgr -R file.txt — raw_control_chars = true
    #[test]
    fn test_options_dash_r_upper_sets_raw_control_chars() {
        let opts = Options::parse_from(["pgr", "-R", "file.txt"]);
        assert!(opts.raw_control_chars);
        assert_eq!(opts.raw_mode(), RawControlMode::AnsiOnly);
    }

    // Test 3: pgr -S file.txt — chop_long_lines = true
    #[test]
    fn test_options_dash_s_upper_sets_chop_long_lines() {
        let opts = Options::parse_from(["pgr", "-S", "file.txt"]);
        assert!(opts.chop_long_lines);
    }

    // Test 4: pgr -M file.txt — long_prompt = true
    #[test]
    fn test_options_dash_m_upper_sets_long_prompt() {
        let opts = Options::parse_from(["pgr", "-M", "file.txt"]);
        assert!(opts.long_prompt);
        assert_eq!(opts.prompt_style(), PromptStyle::Long);
    }

    // Test 5: pgr -RS file.txt — combined flags work
    #[test]
    fn test_options_combined_flags_rs_both_set() {
        let opts = Options::parse_from(["pgr", "-RS", "file.txt"]);
        assert!(opts.raw_control_chars);
        assert!(opts.chop_long_lines);
    }

    // Test 6: pgr -x4 file.txt — tab_width = 4
    #[test]
    fn test_options_dash_x_sets_tab_width() {
        let opts = Options::parse_from(["pgr", "-x4", "file.txt"]);
        assert_eq!(opts.tab_width, 4);
    }

    // Test 7: pgr -V — version = true
    #[test]
    fn test_options_dash_v_upper_sets_version() {
        let opts = Options::parse_from(["pgr", "-V"]);
        assert!(opts.version);
    }

    // Test 8: No files and no version/help — files is empty
    #[test]
    fn test_options_no_files_results_in_empty_vec() {
        let opts = Options::parse_from(["pgr"]);
        assert!(opts.files.is_empty());
    }

    // Additional: prompt_style defaults to Short
    #[test]
    fn test_options_prompt_style_defaults_to_short() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.prompt_style(), PromptStyle::Short);
    }

    // Additional: -m sets medium prompt
    #[test]
    fn test_options_dash_m_lower_sets_medium_prompt() {
        let opts = Options::parse_from(["pgr", "-m", "file.txt"]);
        assert!(opts.medium_prompt);
        assert_eq!(opts.prompt_style(), PromptStyle::Medium);
    }

    // Additional: raw_mode defaults to Off
    #[test]
    fn test_options_raw_mode_defaults_to_off() {
        let opts = Options::parse_from(["pgr", "file.txt"]);
        assert_eq!(opts.raw_mode(), RawControlMode::Off);
    }

    // Additional: -r sets All mode
    #[test]
    fn test_options_dash_r_lower_sets_raw_all() {
        let opts = Options::parse_from(["pgr", "-r", "file.txt"]);
        assert!(opts.raw_all);
        assert_eq!(opts.raw_mode(), RawControlMode::All);
    }
}
