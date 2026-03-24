//! Runtime-mutable pager options.
//!
//! These options can be toggled interactively while the pager is running,
//! mirroring the behavior of GNU less's `-` command prefix.

use pgr_display::RawControlMode;

/// Search highlighting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiliteMode {
    /// Highlight all matches on screen (default).
    All,
    /// Highlight only the last match found (`-g`).
    LastOnly,
    /// Never highlight (`-G`).
    Never,
}

/// Errors that can occur when toggling or querying a runtime option.
#[derive(Debug, thiserror::Error)]
pub enum OptionError {
    /// The flag character does not correspond to any known option.
    #[error("unknown option: -{0}")]
    UnknownOption(char),
    /// The flag requires a value but none was provided.
    #[error("option -{0} requires a value")]
    RequiresValue(char),
    /// The value provided for the flag is invalid.
    #[error("invalid value for -{0}: {1}")]
    InvalidValue(char, String),
}

/// Runtime-mutable pager options.
///
/// This struct holds the current state of all options that can be toggled
/// at runtime. It is initialized from the command-line `Options` struct
/// (in `pgr-cli`) and can be mutated by `-<flag>` commands during the
/// pager session.
#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)] // Each bool maps to a distinct less flag
pub struct RuntimeOptions {
    // Boolean flags (toggleable)
    /// Smart case-insensitive search (`-i`).
    pub case_insensitive: bool,
    /// Always case-insensitive search (`-I`).
    pub case_insensitive_always: bool,
    /// Show line numbers in left margin (`-N`).
    pub line_numbers: bool,
    /// Chop (truncate) long lines instead of wrapping (`-S`).
    pub chop_long_lines: bool,
    /// Squeeze consecutive blank lines into one (`-s`).
    pub squeeze_blank_lines: bool,
    /// Raw control character mode (`-r` / `-R`).
    pub raw_control_mode: RawControlMode,
    /// Suppress bell (`-q`).
    pub quiet: bool,
    /// Quit at second end-of-file (`-e`).
    pub quit_at_eof: bool,
    /// Quit at first end-of-file (`-E`).
    pub quit_at_first_eof: bool,
    /// Quit if file fits on one screen (`-F`).
    pub quit_if_one_screen: bool,
    /// Don't use terminfo init/deinit (`-X`).
    pub no_init: bool,
    /// Search highlighting mode (`-g` / `-G`).
    pub hilite_search: HiliteMode,
    /// Highlight first new line after forward scroll (`-w`).
    pub hilite_unread: bool,
    /// Highlight first new line after any forward movement (`-W`).
    pub hilite_unread_all: bool,
    /// Don't show tilde for lines past EOF (`-~`).
    pub tilde: bool,
    /// Show status column on the left edge (`-J`).
    pub status_column: bool,

    // Value flags (prompted for a value)
    /// Tab stop width (`-x`).
    pub tab_width: usize,
    /// Target line for search results (`-j`).
    pub jump_target: usize,
    /// Horizontal scroll amount (`-#`).
    pub shift_amount: usize,
    /// Scroll window size override (`-z`).
    pub window_size: Option<usize>,
    /// Maximum backward scroll limit (`-h`).
    pub max_back_scroll: Option<usize>,
    /// Maximum forward scroll limit (`-y`).
    pub max_forw_scroll: Option<usize>,

    // String flags
    /// Custom prompt string (`-P`).
    pub prompt_string: Option<String>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            case_insensitive: false,
            case_insensitive_always: false,
            line_numbers: false,
            chop_long_lines: false,
            squeeze_blank_lines: false,
            raw_control_mode: RawControlMode::Off,
            quiet: false,
            quit_at_eof: false,
            quit_at_first_eof: false,
            quit_if_one_screen: false,
            no_init: false,
            hilite_search: HiliteMode::All,
            hilite_unread: false,
            hilite_unread_all: false,
            tilde: false,
            status_column: false,
            tab_width: 8,
            jump_target: 1,
            shift_amount: 0,
            window_size: None,
            max_back_scroll: None,
            max_forw_scroll: None,
            prompt_string: None,
        }
    }
}

impl RuntimeOptions {
    /// Toggle a boolean flag by its short character.
    ///
    /// Returns a description of the new state, or an error if the flag
    /// is unknown or not toggleable.
    ///
    /// # Errors
    ///
    /// Returns [`OptionError::UnknownOption`] if `flag` is not a recognized
    /// option character, or [`OptionError::RequiresValue`] if the flag
    /// requires a value rather than toggling.
    #[allow(clippy::too_many_lines)] // Dispatch table for all toggleable flags
    pub fn toggle(&mut self, flag: char) -> Result<String, OptionError> {
        match flag {
            'i' => {
                self.case_insensitive = !self.case_insensitive;
                Ok(bool_description(
                    "Case-insensitive search",
                    self.case_insensitive,
                ))
            }
            'I' => {
                self.case_insensitive_always = !self.case_insensitive_always;
                Ok(bool_description(
                    "Case-insensitive search (always)",
                    self.case_insensitive_always,
                ))
            }
            'N' => {
                self.line_numbers = !self.line_numbers;
                Ok(bool_description("Line numbers", self.line_numbers))
            }
            'S' => {
                self.chop_long_lines = !self.chop_long_lines;
                Ok(bool_description("Chop long lines", self.chop_long_lines))
            }
            's' => {
                self.squeeze_blank_lines = !self.squeeze_blank_lines;
                Ok(bool_description(
                    "Squeeze blank lines",
                    self.squeeze_blank_lines,
                ))
            }
            'r' => {
                self.raw_control_mode = match self.raw_control_mode {
                    RawControlMode::All => RawControlMode::Off,
                    _ => RawControlMode::All,
                };
                Ok(format!(
                    "Raw control mode is {}",
                    raw_mode_description(self.raw_control_mode)
                ))
            }
            'R' => {
                self.raw_control_mode = match self.raw_control_mode {
                    RawControlMode::AnsiOnly => RawControlMode::Off,
                    _ => RawControlMode::AnsiOnly,
                };
                Ok(format!(
                    "Raw control mode is {}",
                    raw_mode_description(self.raw_control_mode)
                ))
            }
            'q' => {
                self.quiet = !self.quiet;
                Ok(bool_description("Quiet mode", self.quiet))
            }
            'e' => {
                self.quit_at_eof = !self.quit_at_eof;
                Ok(bool_description("Quit at EOF", self.quit_at_eof))
            }
            'E' => {
                self.quit_at_first_eof = !self.quit_at_first_eof;
                Ok(bool_description(
                    "Quit at first EOF",
                    self.quit_at_first_eof,
                ))
            }
            'F' => {
                self.quit_if_one_screen = !self.quit_if_one_screen;
                Ok(bool_description(
                    "Quit if one screen",
                    self.quit_if_one_screen,
                ))
            }
            'X' => {
                self.no_init = !self.no_init;
                Ok(bool_description("No init", self.no_init))
            }
            'g' => {
                self.hilite_search = match self.hilite_search {
                    HiliteMode::LastOnly => HiliteMode::All,
                    _ => HiliteMode::LastOnly,
                };
                Ok(format!(
                    "Search highlighting is {}",
                    hilite_description(self.hilite_search)
                ))
            }
            'G' => {
                self.hilite_search = match self.hilite_search {
                    HiliteMode::Never => HiliteMode::All,
                    _ => HiliteMode::Never,
                };
                Ok(format!(
                    "Search highlighting is {}",
                    hilite_description(self.hilite_search)
                ))
            }
            'w' => {
                self.hilite_unread = !self.hilite_unread;
                Ok(bool_description("Highlight unread", self.hilite_unread))
            }
            'W' => {
                self.hilite_unread_all = !self.hilite_unread_all;
                Ok(bool_description(
                    "Highlight unread (all)",
                    self.hilite_unread_all,
                ))
            }
            'J' => {
                self.status_column = !self.status_column;
                Ok(bool_description("Status column", self.status_column))
            }
            // Value flags cannot be toggled — they require a value.
            'x' | 'j' | '#' | 'z' | 'h' | 'y' => Err(OptionError::RequiresValue(flag)),
            _ => Err(OptionError::UnknownOption(flag)),
        }
    }

    /// Explicitly set a flag on (`-+` prefix).
    ///
    /// For boolean flags, sets the value to `true`. For mode flags, sets
    /// to the "on" variant.
    ///
    /// # Errors
    ///
    /// Returns [`OptionError::UnknownOption`] if `flag` is not recognized,
    /// or [`OptionError::RequiresValue`] for value-typed flags.
    pub fn set_on(&mut self, flag: char) -> Result<String, OptionError> {
        match flag {
            'i' => {
                self.case_insensitive = true;
                Ok(bool_description("Case-insensitive search", true))
            }
            'I' => {
                self.case_insensitive_always = true;
                Ok(bool_description("Case-insensitive search (always)", true))
            }
            'N' => {
                self.line_numbers = true;
                Ok(bool_description("Line numbers", true))
            }
            'S' => {
                self.chop_long_lines = true;
                Ok(bool_description("Chop long lines", true))
            }
            's' => {
                self.squeeze_blank_lines = true;
                Ok(bool_description("Squeeze blank lines", true))
            }
            'r' => {
                self.raw_control_mode = RawControlMode::All;
                Ok(format!(
                    "Raw control mode is {}",
                    raw_mode_description(RawControlMode::All)
                ))
            }
            'R' => {
                self.raw_control_mode = RawControlMode::AnsiOnly;
                Ok(format!(
                    "Raw control mode is {}",
                    raw_mode_description(RawControlMode::AnsiOnly)
                ))
            }
            'q' => {
                self.quiet = true;
                Ok(bool_description("Quiet mode", true))
            }
            'e' => {
                self.quit_at_eof = true;
                Ok(bool_description("Quit at EOF", true))
            }
            'E' => {
                self.quit_at_first_eof = true;
                Ok(bool_description("Quit at first EOF", true))
            }
            'F' => {
                self.quit_if_one_screen = true;
                Ok(bool_description("Quit if one screen", true))
            }
            'X' => {
                self.no_init = true;
                Ok(bool_description("No init", true))
            }
            'g' => {
                self.hilite_search = HiliteMode::LastOnly;
                Ok(format!(
                    "Search highlighting is {}",
                    hilite_description(HiliteMode::LastOnly)
                ))
            }
            'G' => {
                self.hilite_search = HiliteMode::Never;
                Ok(format!(
                    "Search highlighting is {}",
                    hilite_description(HiliteMode::Never)
                ))
            }
            'w' => {
                self.hilite_unread = true;
                Ok(bool_description("Highlight unread", true))
            }
            'W' => {
                self.hilite_unread_all = true;
                Ok(bool_description("Highlight unread (all)", true))
            }
            'J' => {
                self.status_column = true;
                Ok(bool_description("Status column", true))
            }
            'x' | 'j' | '#' | 'z' | 'h' | 'y' => Err(OptionError::RequiresValue(flag)),
            _ => Err(OptionError::UnknownOption(flag)),
        }
    }

    /// Explicitly set a flag off (`-!` prefix).
    ///
    /// For boolean flags, sets the value to `false`. For mode flags, sets
    /// to the "off" or default variant.
    ///
    /// # Errors
    ///
    /// Returns [`OptionError::UnknownOption`] if `flag` is not recognized,
    /// or [`OptionError::RequiresValue`] for value-typed flags.
    pub fn set_off(&mut self, flag: char) -> Result<String, OptionError> {
        match flag {
            'i' => {
                self.case_insensitive = false;
                Ok(bool_description("Case-insensitive search", false))
            }
            'I' => {
                self.case_insensitive_always = false;
                Ok(bool_description("Case-insensitive search (always)", false))
            }
            'N' => {
                self.line_numbers = false;
                Ok(bool_description("Line numbers", false))
            }
            'S' => {
                self.chop_long_lines = false;
                Ok(bool_description("Chop long lines", false))
            }
            's' => {
                self.squeeze_blank_lines = false;
                Ok(bool_description("Squeeze blank lines", false))
            }
            'r' | 'R' => {
                self.raw_control_mode = RawControlMode::Off;
                Ok(format!(
                    "Raw control mode is {}",
                    raw_mode_description(RawControlMode::Off)
                ))
            }
            'q' => {
                self.quiet = false;
                Ok(bool_description("Quiet mode", false))
            }
            'e' => {
                self.quit_at_eof = false;
                Ok(bool_description("Quit at EOF", false))
            }
            'E' => {
                self.quit_at_first_eof = false;
                Ok(bool_description("Quit at first EOF", false))
            }
            'F' => {
                self.quit_if_one_screen = false;
                Ok(bool_description("Quit if one screen", false))
            }
            'X' => {
                self.no_init = false;
                Ok(bool_description("No init", false))
            }
            'g' | 'G' => {
                self.hilite_search = HiliteMode::All;
                Ok(format!(
                    "Search highlighting is {}",
                    hilite_description(HiliteMode::All)
                ))
            }
            'w' => {
                self.hilite_unread = false;
                Ok(bool_description("Highlight unread", false))
            }
            'W' => {
                self.hilite_unread_all = false;
                Ok(bool_description("Highlight unread (all)", false))
            }
            'J' => {
                self.status_column = false;
                Ok(bool_description("Status column", false))
            }
            'x' | 'j' | '#' | 'z' | 'h' | 'y' => Err(OptionError::RequiresValue(flag)),
            _ => Err(OptionError::UnknownOption(flag)),
        }
    }

    /// Set a value flag (prompted input).
    ///
    /// Parses `value` as appropriate for the flag type and stores it.
    ///
    /// # Errors
    ///
    /// Returns [`OptionError::UnknownOption`] if the flag is not a value-type
    /// option, or [`OptionError::InvalidValue`] if parsing fails.
    pub fn set_value(&mut self, flag: char, value: &str) -> Result<String, OptionError> {
        match flag {
            'x' => {
                let n = parse_usize(flag, value)?;
                self.tab_width = n;
                Ok(format!("Tab stops at {n}"))
            }
            'j' => {
                let n = parse_usize(flag, value)?;
                self.jump_target = n;
                Ok(format!("Jump target at {n}"))
            }
            '#' => {
                let n = parse_usize(flag, value)?;
                self.shift_amount = n;
                Ok(format!("Horizontal shift is {n}"))
            }
            'z' => {
                let n = parse_usize(flag, value)?;
                self.window_size = Some(n);
                Ok(format!("Window size is {n}"))
            }
            'h' => {
                let n = parse_usize(flag, value)?;
                self.max_back_scroll = Some(n);
                Ok(format!("Max backward scroll is {n}"))
            }
            'y' => {
                let n = parse_usize(flag, value)?;
                self.max_forw_scroll = Some(n);
                Ok(format!("Max forward scroll is {n}"))
            }
            'P' => {
                self.prompt_string = Some(value.to_owned());
                Ok(format!("Prompt string is \"{value}\""))
            }
            _ => Err(OptionError::UnknownOption(flag)),
        }
    }

    /// Query the current value of a flag (`_` prefix).
    ///
    /// Returns a human-readable description of the flag's current state.
    ///
    /// # Errors
    ///
    /// Returns [`OptionError::UnknownOption`] if `flag` is not recognized.
    pub fn query(&self, flag: char) -> Result<String, OptionError> {
        match flag {
            'i' => Ok(bool_description(
                "Case-insensitive search",
                self.case_insensitive,
            )),
            'I' => Ok(bool_description(
                "Case-insensitive search (always)",
                self.case_insensitive_always,
            )),
            'N' => Ok(bool_description("Line numbers", self.line_numbers)),
            'S' => Ok(bool_description("Chop long lines", self.chop_long_lines)),
            's' => Ok(bool_description(
                "Squeeze blank lines",
                self.squeeze_blank_lines,
            )),
            'r' | 'R' => Ok(format!(
                "Raw control mode is {}",
                raw_mode_description(self.raw_control_mode)
            )),
            'q' => Ok(bool_description("Quiet mode", self.quiet)),
            'e' => Ok(bool_description("Quit at EOF", self.quit_at_eof)),
            'E' => Ok(bool_description(
                "Quit at first EOF",
                self.quit_at_first_eof,
            )),
            'F' => Ok(bool_description(
                "Quit if one screen",
                self.quit_if_one_screen,
            )),
            'X' => Ok(bool_description("No init", self.no_init)),
            'g' | 'G' => Ok(format!(
                "Search highlighting is {}",
                hilite_description(self.hilite_search)
            )),
            'w' => Ok(bool_description("Highlight unread", self.hilite_unread)),
            'W' => Ok(bool_description(
                "Highlight unread (all)",
                self.hilite_unread_all,
            )),
            'J' => Ok(bool_description("Status column", self.status_column)),
            'x' => Ok(format!("Tab stops at {}", self.tab_width)),
            'j' => Ok(format!("Jump target at {}", self.jump_target)),
            '#' => Ok(format!("Horizontal shift is {}", self.shift_amount)),
            'z' => match self.window_size {
                Some(n) => Ok(format!("Window size is {n}")),
                None => Ok("Window size is default".to_owned()),
            },
            'h' => match self.max_back_scroll {
                Some(n) => Ok(format!("Max backward scroll is {n}")),
                None => Ok("Max backward scroll is unlimited".to_owned()),
            },
            'y' => match self.max_forw_scroll {
                Some(n) => Ok(format!("Max forward scroll is {n}")),
                None => Ok("Max forward scroll is unlimited".to_owned()),
            },
            'P' => match &self.prompt_string {
                Some(s) => Ok(format!("Prompt string is \"{s}\"")),
                None => Ok("Prompt string is default".to_owned()),
            },
            _ => Err(OptionError::UnknownOption(flag)),
        }
    }

    /// Returns `true` if toggling the given flag should cause a screen repaint.
    #[must_use]
    pub fn needs_repaint(flag: char) -> bool {
        matches!(flag, 'N' | 'S' | 's' | 'r' | 'R' | 'x' | 'J')
    }
}

/// Format a boolean option description.
fn bool_description(name: &str, value: bool) -> String {
    let state = if value { "ON" } else { "OFF" };
    format!("{name} is {state}")
}

/// Format a `RawControlMode` for display.
fn raw_mode_description(mode: RawControlMode) -> &'static str {
    match mode {
        RawControlMode::Off => "OFF",
        RawControlMode::AnsiOnly => "ANSI only",
        RawControlMode::All => "ALL",
    }
}

/// Format a `HiliteMode` for display.
fn hilite_description(mode: HiliteMode) -> &'static str {
    match mode {
        HiliteMode::All => "all matches",
        HiliteMode::LastOnly => "last match only",
        HiliteMode::Never => "OFF",
    }
}

/// Parse a `usize` from a string, returning an [`OptionError`] on failure.
fn parse_usize(flag: char, value: &str) -> Result<usize, OptionError> {
    value
        .parse::<usize>()
        .map_err(|_| OptionError::InvalidValue(flag, value.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1: toggle('i') flips case_insensitive and returns description ──

    #[test]
    fn test_toggle_i_flips_case_insensitive_returns_description() {
        let mut opts = RuntimeOptions::default();
        assert!(!opts.case_insensitive);

        let msg = opts.toggle('i').unwrap();
        assert!(opts.case_insensitive);
        assert!(msg.contains("ON"), "expected ON in: {msg}");

        let msg = opts.toggle('i').unwrap();
        assert!(!opts.case_insensitive);
        assert!(msg.contains("OFF"), "expected OFF in: {msg}");
    }

    // ── Test 4: toggle('N') flips line_numbers ──

    #[test]
    fn test_toggle_n_upper_flips_line_numbers() {
        let mut opts = RuntimeOptions::default();
        assert!(!opts.line_numbers);

        let msg = opts.toggle('N').unwrap();
        assert!(opts.line_numbers);
        assert!(msg.contains("ON"));
    }

    // ── Test 5: toggle('S') flips chop_long_lines ──

    #[test]
    fn test_toggle_s_upper_flips_chop_long_lines() {
        let mut opts = RuntimeOptions::default();
        assert!(!opts.chop_long_lines);

        let msg = opts.toggle('S').unwrap();
        assert!(opts.chop_long_lines);
        assert!(msg.contains("ON"));
    }

    // ── Test 6: set_on('i') sets case_insensitive to true ──

    #[test]
    fn test_set_on_i_sets_case_insensitive_true() {
        let mut opts = RuntimeOptions::default();
        let msg = opts.set_on('i').unwrap();
        assert!(opts.case_insensitive);
        assert!(msg.contains("ON"));

        // Setting on when already on is idempotent.
        let msg = opts.set_on('i').unwrap();
        assert!(opts.case_insensitive);
        assert!(msg.contains("ON"));
    }

    // ── Test 7: set_off('i') sets case_insensitive to false ──

    #[test]
    fn test_set_off_i_sets_case_insensitive_false() {
        let mut opts = RuntimeOptions::default();
        opts.case_insensitive = true;

        let msg = opts.set_off('i').unwrap();
        assert!(!opts.case_insensitive);
        assert!(msg.contains("OFF"));
    }

    // ── Test 8: query('i') returns current state description ──

    #[test]
    fn test_query_i_returns_current_state_description() {
        let opts = RuntimeOptions::default();
        let msg = opts.query('i').unwrap();
        assert!(msg.contains("OFF"));

        let mut opts2 = RuntimeOptions::default();
        opts2.case_insensitive = true;
        let msg = opts2.query('i').unwrap();
        assert!(msg.contains("ON"));
    }

    // ── Test 9: toggle with unknown flag returns UnknownOption error ──

    #[test]
    fn test_toggle_unknown_flag_returns_unknown_option_error() {
        let mut opts = RuntimeOptions::default();
        let err = opts.toggle('Z').unwrap_err();
        assert!(matches!(err, OptionError::UnknownOption('Z')));
    }

    // ── Test 10: set_value('x', "4") sets tab width to 4 ──

    #[test]
    fn test_set_value_x_sets_tab_width() {
        let mut opts = RuntimeOptions::default();
        let msg = opts.set_value('x', "4").unwrap();
        assert_eq!(opts.tab_width, 4);
        assert!(msg.contains('4'));
    }

    // ── Test 11: set_value('x', "abc") returns InvalidValue error ──

    #[test]
    fn test_set_value_x_invalid_returns_error() {
        let mut opts = RuntimeOptions::default();
        let err = opts.set_value('x', "abc").unwrap_err();
        assert!(matches!(err, OptionError::InvalidValue('x', _)));
    }

    // ── Test 14: toggling -s updates squeeze blank lines state ──

    #[test]
    fn test_toggle_s_lower_updates_squeeze_blank_lines() {
        let mut opts = RuntimeOptions::default();
        assert!(!opts.squeeze_blank_lines);

        let msg = opts.toggle('s').unwrap();
        assert!(opts.squeeze_blank_lines);
        assert!(msg.contains("ON"));

        let msg = opts.toggle('s').unwrap();
        assert!(!opts.squeeze_blank_lines);
        assert!(msg.contains("OFF"));
    }

    // ── Test 15: query for value flags returns the numeric value ──

    #[test]
    fn test_query_value_flags_returns_numeric_value() {
        let mut opts = RuntimeOptions::default();
        opts.tab_width = 4;
        let msg = opts.query('x').unwrap();
        assert!(msg.contains('4'), "expected 4 in: {msg}");

        opts.jump_target = 10;
        let msg = opts.query('j').unwrap();
        assert!(msg.contains("10"), "expected 10 in: {msg}");
    }

    // ── Additional coverage: value flags cannot be toggled ──

    #[test]
    fn test_toggle_value_flag_returns_requires_value() {
        let mut opts = RuntimeOptions::default();
        let err = opts.toggle('x').unwrap_err();
        assert!(matches!(err, OptionError::RequiresValue('x')));
    }

    // ── Additional coverage: set_on / set_off with unknown flag ──

    #[test]
    fn test_set_on_unknown_flag_returns_error() {
        let mut opts = RuntimeOptions::default();
        let err = opts.set_on('Z').unwrap_err();
        assert!(matches!(err, OptionError::UnknownOption('Z')));
    }

    #[test]
    fn test_set_off_unknown_flag_returns_error() {
        let mut opts = RuntimeOptions::default();
        let err = opts.set_off('Z').unwrap_err();
        assert!(matches!(err, OptionError::UnknownOption('Z')));
    }

    // ── Additional coverage: set_value with unknown flag ──

    #[test]
    fn test_set_value_unknown_flag_returns_error() {
        let mut opts = RuntimeOptions::default();
        let err = opts.set_value('Z', "1").unwrap_err();
        assert!(matches!(err, OptionError::UnknownOption('Z')));
    }

    // ── Additional coverage: needs_repaint ──

    #[test]
    fn test_needs_repaint_for_display_flags() {
        assert!(RuntimeOptions::needs_repaint('N'));
        assert!(RuntimeOptions::needs_repaint('S'));
        assert!(RuntimeOptions::needs_repaint('s'));
        assert!(RuntimeOptions::needs_repaint('r'));
        assert!(RuntimeOptions::needs_repaint('R'));
        assert!(RuntimeOptions::needs_repaint('x'));
        assert!(!RuntimeOptions::needs_repaint('i'));
        assert!(!RuntimeOptions::needs_repaint('q'));
    }

    // ── Additional coverage: hilite mode toggling ──

    #[test]
    fn test_toggle_g_cycles_hilite_mode() {
        let mut opts = RuntimeOptions::default();
        assert_eq!(opts.hilite_search, HiliteMode::All);

        opts.toggle('g').unwrap();
        assert_eq!(opts.hilite_search, HiliteMode::LastOnly);

        opts.toggle('g').unwrap();
        assert_eq!(opts.hilite_search, HiliteMode::All);
    }

    #[test]
    fn test_toggle_upper_g_cycles_hilite_mode() {
        let mut opts = RuntimeOptions::default();
        opts.toggle('G').unwrap();
        assert_eq!(opts.hilite_search, HiliteMode::Never);

        opts.toggle('G').unwrap();
        assert_eq!(opts.hilite_search, HiliteMode::All);
    }

    // ── Additional coverage: raw mode toggling ──

    #[test]
    fn test_toggle_r_cycles_raw_mode() {
        let mut opts = RuntimeOptions::default();
        opts.toggle('r').unwrap();
        assert_eq!(opts.raw_control_mode, RawControlMode::All);

        opts.toggle('r').unwrap();
        assert_eq!(opts.raw_control_mode, RawControlMode::Off);
    }

    #[test]
    fn test_toggle_upper_r_cycles_raw_mode() {
        let mut opts = RuntimeOptions::default();
        opts.toggle('R').unwrap();
        assert_eq!(opts.raw_control_mode, RawControlMode::AnsiOnly);

        opts.toggle('R').unwrap();
        assert_eq!(opts.raw_control_mode, RawControlMode::Off);
    }

    // ── Status column (-J) flag ──

    #[test]
    fn test_toggle_upper_j_flips_status_column() {
        let mut opts = RuntimeOptions::default();
        assert!(!opts.status_column);

        let msg = opts.toggle('J').unwrap();
        assert!(opts.status_column);
        assert!(msg.contains("ON"), "expected ON in: {msg}");

        let msg = opts.toggle('J').unwrap();
        assert!(!opts.status_column);
        assert!(msg.contains("OFF"), "expected OFF in: {msg}");
    }

    #[test]
    fn test_set_on_upper_j_sets_status_column_true() {
        let mut opts = RuntimeOptions::default();
        let msg = opts.set_on('J').unwrap();
        assert!(opts.status_column);
        assert!(msg.contains("ON"));
    }

    #[test]
    fn test_set_off_upper_j_sets_status_column_false() {
        let mut opts = RuntimeOptions::default();
        opts.status_column = true;
        let msg = opts.set_off('J').unwrap();
        assert!(!opts.status_column);
        assert!(msg.contains("OFF"));
    }

    #[test]
    fn test_query_upper_j_returns_current_state() {
        let opts = RuntimeOptions::default();
        let msg = opts.query('J').unwrap();
        assert!(msg.contains("OFF"), "expected OFF in: {msg}");

        let mut opts2 = RuntimeOptions::default();
        opts2.status_column = true;
        let msg = opts2.query('J').unwrap();
        assert!(msg.contains("ON"), "expected ON in: {msg}");
    }

    #[test]
    fn test_needs_repaint_includes_j() {
        assert!(RuntimeOptions::needs_repaint('J'));
    }

    // ── Additional coverage: set_value for all value flags ──

    #[test]
    fn test_set_value_z_sets_window_size() {
        let mut opts = RuntimeOptions::default();
        let msg = opts.set_value('z', "20").unwrap();
        assert_eq!(opts.window_size, Some(20));
        assert!(msg.contains("20"));
    }

    #[test]
    fn test_set_value_j_sets_jump_target() {
        let mut opts = RuntimeOptions::default();
        let msg = opts.set_value('j', "5").unwrap();
        assert_eq!(opts.jump_target, 5);
        assert!(msg.contains('5'));
    }

    #[test]
    fn test_set_value_hash_sets_shift_amount() {
        let mut opts = RuntimeOptions::default();
        let msg = opts.set_value('#', "16").unwrap();
        assert_eq!(opts.shift_amount, 16);
        assert!(msg.contains("16"));
    }

    #[test]
    fn test_set_value_p_upper_sets_prompt_string() {
        let mut opts = RuntimeOptions::default();
        let msg = opts.set_value('P', "%f:%l").unwrap();
        assert_eq!(opts.prompt_string.as_deref(), Some("%f:%l"));
        assert!(msg.contains("%f:%l"));
    }

    // ── Additional coverage: query optional values ──

    #[test]
    fn test_query_z_default_returns_default() {
        let opts = RuntimeOptions::default();
        let msg = opts.query('z').unwrap();
        assert!(msg.contains("default"));
    }

    #[test]
    fn test_query_p_upper_default_returns_default() {
        let opts = RuntimeOptions::default();
        let msg = opts.query('P').unwrap();
        assert!(msg.contains("default"));
    }

    // ── Error display ──

    #[test]
    fn test_option_error_display_unknown() {
        let err = OptionError::UnknownOption('Z');
        assert_eq!(err.to_string(), "unknown option: -Z");
    }

    #[test]
    fn test_option_error_display_requires_value() {
        let err = OptionError::RequiresValue('x');
        assert_eq!(err.to_string(), "option -x requires a value");
    }

    #[test]
    fn test_option_error_display_invalid_value() {
        let err = OptionError::InvalidValue('x', "abc".to_owned());
        assert_eq!(err.to_string(), "invalid value for -x: abc");
    }
}
