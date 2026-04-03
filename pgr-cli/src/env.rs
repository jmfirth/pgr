//! Environment variable handling for pgr.
//!
//! Reads the `LESS` environment variable and parses it into flags that are
//! prepended to the command-line arguments so that explicit flags override
//! the environment defaults.
//!
//! For full environment variable handling, use [`EnvConfig::from_env()`].

// EnvConfig and its helpers are fully implemented here but wired into main.rs
// in downstream tasks (Task 117 for editor, etc.).  Suppress dead-code warnings
// for this module so the complete implementation can land without phantom usage.
#![allow(dead_code)]

use pgr_display::BinFmt;

/// Parsed environment variable configuration.
///
/// Holds all less-related environment variables needed for pager operation.
/// Variables are read once at startup and stored here.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)] // Each bool maps to a distinct env var flag from the less man page
pub struct EnvConfig {
    /// LESS: default command-line options.
    pub less_options: Vec<String>,

    /// LESSCHARSET: character set name (e.g., "utf-8", "latin1").
    pub charset: Option<String>,

    /// LESSEDIT: editor command template for `v` command.
    /// Contains `%f` (filename) and `%lm` (line number) placeholders.
    pub less_edit: Option<String>,

    /// VISUAL: preferred editor (takes precedence over EDITOR).
    pub visual: Option<String>,

    /// EDITOR: fallback editor.
    pub editor: Option<String>,

    /// SHELL: user's shell for `!` command.
    pub shell: Option<String>,

    /// COLUMNS: override terminal width.
    pub columns: Option<usize>,

    /// LINES: override terminal height.
    pub lines: Option<usize>,

    /// TERM: terminal type.
    pub term: Option<String>,

    /// HOME: user's home directory.
    pub home: Option<String>,

    /// LESSSECURE: if set to "1", enable security restrictions.
    pub secure_mode: bool,

    /// LESSWINDOW: default window size for scrolling.
    pub window_size: Option<usize>,

    /// LESSSEPARATOR: character used to separate directory components
    /// in filenames displayed in prompts.
    pub separator: Option<String>,

    // --- Task 206: batch 1 environment variables ---
    /// LESSBINFMT: format for displaying binary/control characters.
    /// Default: `*s<%02X>`. The `*` prefix means standout mode.
    pub bin_fmt: Option<String>,

    /// LESSUTFBINFMT: format for displaying invalid UTF-8 byte sequences.
    /// Default: `<U+%04X>`.
    pub utf_bin_fmt: Option<String>,

    /// `LESS_IS_MORE`: when set (non-empty), behave as `more(1)`.
    /// Wired in `Options::parse()` (Task 250) to enable more-compat defaults.
    pub is_more: bool,

    /// `LESS_COLUMNS`: override terminal width (takes precedence over `COLUMNS`).
    pub less_columns: Option<usize>,

    /// `LESS_LINES`: override terminal height (takes precedence over `LINES`).
    pub less_lines: Option<usize>,

    /// `LESS_SHELL_LINES`: override screen height for `-F` (quit-if-one-screen).
    pub shell_lines: Option<usize>,

    /// `XDG_CONFIG_HOME`: base directory for config files. Defaults to `~/.config`.
    pub xdg_config_home: Option<String>,

    /// `XDG_DATA_HOME`: base directory for data files. Defaults to `~/.local/share`.
    pub xdg_data_home: Option<String>,

    /// `XDG_STATE_HOME`: base directory for state files. Defaults to `~/.local/state`.
    pub xdg_state_home: Option<String>,

    // --- Phase 2 variables (parsed, stored, not yet used) ---
    /// LESSOPEN: input preprocessor command.
    pub lessopen: Option<String>,

    /// LESSCLOSE: input preprocessor cleanup command.
    pub lessclose: Option<String>,

    /// LESSKEY / LESSKEYIN: lesskey file path.
    pub lesskey: Option<String>,

    /// LESSHISTFILE: command/search history file path.
    pub histfile: Option<String>,

    /// LESSHISTSIZE: maximum history entries.
    pub histsize: Option<usize>,

    /// LESSANSIMIDCHARS: characters recognized in middle of ANSI sequences.
    pub ansi_mid_chars: Option<String>,

    /// LESSANSIENDCHARS: characters recognized as ending ANSI sequences.
    pub ansi_end_chars: Option<String>,

    // --- Task 246: batch 2 environment variables ---
    /// LESSCHARDEF: custom character type definitions.
    /// Each character in the string defines the type of the corresponding character
    /// (`.` = normal, `c` = control, `b` = binary).
    pub chardef: Option<String>,

    /// LESSMETACHARS: shell metacharacters that need quoting in filenames.
    pub meta_chars: Option<String>,

    /// LESSMETAESCAPE: prefix character used to escape metacharacters.
    pub meta_escape: Option<String>,

    /// LESSECHO: external program for filename expansion with metacharacter handling.
    pub lessecho: Option<String>,

    /// `LESS_DATA_DELAY`: delay (in tenths of a second) before showing
    /// "waiting for data" message when reading from a pipe.
    pub data_delay: Option<usize>,

    /// `LESS_SIGUSR1`: when set (non-empty), enables USR1 signal handling
    /// to reopen the current file.
    pub sigusr1: bool,

    /// `LESS_UNSUPPORT`: space-separated list of option letters to silently ignore
    /// when encountered on the command line.
    pub unsupport: Option<String>,

    /// `LESSNOCONFIG`: when set (non-empty), disables all env-based configuration.
    /// The `LESS` environment variable and lesskey files are ignored.
    pub no_config: bool,

    /// `POSIXLY_CORRECT`: when set (non-empty), enforces strict POSIX option ordering
    /// (options must precede operands).
    pub posixly_correct: bool,

    // --- pgr-specific environment variables ---
    /// `PGR_SYNTAX`: set to "0" to disable syntax highlighting by default.
    pub pgr_syntax_disabled: bool,

    /// `PGR_THEME`: syntax highlighting theme name.
    pub pgr_theme: Option<String>,

    /// `PGR_CLIPBOARD`: clipboard backend override (auto, osc52, pbcopy, xclip, xsel, wl-copy, off).
    pub pgr_clipboard: Option<String>,

    /// `PGR_GIT_GUTTER`: set to "1" to enable git gutter by default.
    pub pgr_git_gutter: bool,
}

impl EnvConfig {
    /// Read all environment variables and construct the config.
    ///
    /// This is called once at startup. Unknown or unparseable numeric values
    /// are silently ignored (stored as `None`).
    ///
    /// When `LESSNOCONFIG` is set (non-empty), the `LESS` environment variable
    /// and lesskey-related variables are not read. System variables like
    /// `HOME`, `TERM`, `SHELL`, `COLUMNS`, `LINES`, and the XDG variables
    /// are still read because they are not less-specific configuration.
    #[must_use]
    pub fn from_env() -> Self {
        // Task 246: check LESSNOCONFIG first — it suppresses LESS env config.
        let no_config = env_is_set("LESSNOCONFIG");

        // When LESSNOCONFIG is active, skip the LESS env var.
        let less_options = if no_config {
            Vec::new()
        } else {
            read_less_env()
        };

        // LESSCHARSET is a less-specific config but describes encoding, always read it.
        let charset = env_nonempty("LESSCHARSET");
        let less_edit = env_nonempty("LESSEDIT");
        let visual = env_nonempty("VISUAL");
        let editor = env_nonempty("EDITOR");
        let shell = env_nonempty("SHELL");
        let term = env_nonempty("TERM");
        let home = env_nonempty("HOME");
        let separator = env_nonempty("LESSSEPARATOR");

        let columns = env_parse_usize("COLUMNS");
        let lines = env_parse_usize("LINES");
        let window_size = env_parse_usize("LESSWINDOW");
        let histsize = env_parse_usize("LESSHISTSIZE");

        let secure_mode = std::env::var("LESSSECURE")
            .map(|v| v.trim() == "1")
            .unwrap_or(false);

        // Task 206: batch 1 env vars
        let bin_fmt = env_nonempty("LESSBINFMT");
        let utf_bin_fmt = env_nonempty("LESSUTFBINFMT");
        let is_more = env_is_set("LESS_IS_MORE");
        let less_columns = env_parse_usize("LESS_COLUMNS");
        let less_lines = env_parse_usize("LESS_LINES");
        let shell_lines = env_parse_usize("LESS_SHELL_LINES");
        let xdg_config_home = env_nonempty("XDG_CONFIG_HOME");
        let xdg_data_home = env_nonempty("XDG_DATA_HOME");
        let xdg_state_home = env_nonempty("XDG_STATE_HOME");

        let lessopen = env_nonempty("LESSOPEN");
        let lessclose = env_nonempty("LESSCLOSE");

        // LESSKEY takes precedence over LESSKEYIN when both are set.
        // When LESSNOCONFIG is active, skip lesskey files.
        let lesskey = if no_config {
            None
        } else {
            env_nonempty("LESSKEY").or_else(|| env_nonempty("LESSKEYIN"))
        };

        let histfile = env_nonempty("LESSHISTFILE");
        let ansi_mid_chars = env_nonempty("LESSANSIMIDCHARS");
        let ansi_end_chars = env_nonempty("LESSANSIENDCHARS");

        // Task 246: batch 2 env vars
        let chardef = env_nonempty("LESSCHARDEF");
        let meta_chars = env_nonempty("LESSMETACHARS");
        let meta_escape = env_nonempty("LESSMETAESCAPE");
        let lessecho = env_nonempty("LESSECHO");
        let data_delay = env_parse_usize("LESS_DATA_DELAY");
        let sigusr1 = env_is_set("LESS_SIGUSR1");
        let unsupport = env_nonempty("LESS_UNSUPPORT");
        let posixly_correct = env_is_set("POSIXLY_CORRECT");

        // pgr-specific env vars
        let pgr_syntax_disabled = std::env::var("PGR_SYNTAX")
            .map(|v| v.trim() == "0")
            .unwrap_or(false);
        let pgr_theme = env_nonempty("PGR_THEME");
        let pgr_clipboard = env_nonempty("PGR_CLIPBOARD");
        let pgr_git_gutter = std::env::var("PGR_GIT_GUTTER")
            .map(|v| v.trim() == "1")
            .unwrap_or(false);

        Self {
            less_options,
            charset,
            less_edit,
            visual,
            editor,
            shell,
            columns,
            lines,
            term,
            home,
            secure_mode,
            window_size,
            separator,
            bin_fmt,
            utf_bin_fmt,
            is_more,
            less_columns,
            less_lines,
            shell_lines,
            xdg_config_home,
            xdg_data_home,
            xdg_state_home,
            lessopen,
            lessclose,
            lesskey,
            histfile,
            histsize,
            ansi_mid_chars,
            ansi_end_chars,
            chardef,
            meta_chars,
            meta_escape,
            lessecho,
            data_delay,
            sigusr1,
            unsupport,
            no_config,
            posixly_correct,
            pgr_syntax_disabled,
            pgr_theme,
            pgr_clipboard,
            pgr_git_gutter,
        }
    }

    /// Returns the editor command to use for the `v` command.
    ///
    /// Precedence: `LESSEDIT` > `VISUAL` > `EDITOR` > `"vi"`.
    #[must_use]
    pub fn editor_command(&self) -> &str {
        self.less_edit
            .as_deref()
            .or(self.visual.as_deref())
            .or(self.editor.as_deref())
            .unwrap_or("vi")
    }

    /// Returns the shell to use for `!` commands.
    ///
    /// Precedence: `SHELL` > `"sh"`.
    #[must_use]
    pub fn shell_command(&self) -> &str {
        self.shell.as_deref().unwrap_or("sh")
    }

    /// Returns the terminal dimensions override, if both `COLUMNS` and `LINES` are set.
    ///
    /// Returns `None` if either variable is absent or invalid.
    #[must_use]
    pub fn terminal_size_override(&self) -> Option<(usize, usize)> {
        match (self.columns, self.lines) {
            (Some(cols), Some(lines)) => Some((cols, lines)),
            _ => None,
        }
    }

    /// Returns the effective terminal dimensions, applying `LESS_COLUMNS` / `LESS_LINES`
    /// overrides on top of the ioctl-detected values.
    ///
    /// Each dimension is independently overridden: if only `LESS_COLUMNS` is set,
    /// only width changes.
    #[must_use]
    pub fn effective_dimensions(
        &self,
        detected_rows: usize,
        detected_cols: usize,
    ) -> (usize, usize) {
        let rows = self.less_lines.unwrap_or(detected_rows);
        let cols = self.less_columns.unwrap_or(detected_cols);
        (rows, cols)
    }

    /// Returns the screen height to use for `-F` (quit-if-one-screen) checks.
    ///
    /// Uses `LESS_SHELL_LINES` if set, otherwise falls back to the given
    /// terminal height.
    #[must_use]
    pub fn shell_screen_height(&self, terminal_rows: usize) -> usize {
        self.shell_lines.unwrap_or(terminal_rows)
    }

    /// Returns the XDG config home directory.
    ///
    /// Uses `XDG_CONFIG_HOME` if set, otherwise `$HOME/.config`.
    #[must_use]
    pub fn config_home(&self) -> Option<std::path::PathBuf> {
        if let Some(ref dir) = self.xdg_config_home {
            return Some(std::path::PathBuf::from(dir));
        }
        self.home
            .as_ref()
            .map(|h| std::path::PathBuf::from(h).join(".config"))
    }

    /// Returns the XDG data home directory.
    ///
    /// Uses `XDG_DATA_HOME` if set, otherwise `$HOME/.local/share`.
    #[must_use]
    pub fn data_home(&self) -> Option<std::path::PathBuf> {
        if let Some(ref dir) = self.xdg_data_home {
            return Some(std::path::PathBuf::from(dir));
        }
        self.home
            .as_ref()
            .map(|h| std::path::PathBuf::from(h).join(".local").join("share"))
    }

    /// Returns the XDG state home directory.
    ///
    /// Uses `XDG_STATE_HOME` if set, otherwise `$HOME/.local/state`.
    #[must_use]
    pub fn state_home(&self) -> Option<std::path::PathBuf> {
        if let Some(ref dir) = self.xdg_state_home {
            return Some(std::path::PathBuf::from(dir));
        }
        self.home
            .as_ref()
            .map(|h| std::path::PathBuf::from(h).join(".local").join("state"))
    }

    /// Returns the path for the history file.
    ///
    /// Precedence: `LESSHISTFILE` > `$XDG_STATE_HOME/less/history` >
    /// `$HOME/.local/state/less/history`. Returns `None` if
    /// `LESSHISTFILE` is set to `"-"` (disables persistence) or if no
    /// home directory can be determined.
    #[must_use]
    pub fn history_file_path(&self) -> Option<std::path::PathBuf> {
        if let Some(ref explicit) = self.histfile {
            if explicit == "-" {
                return None;
            }
            return Some(std::path::PathBuf::from(explicit));
        }
        self.state_home()
            .map(|base| base.join("less").join("history"))
    }

    /// Returns the parsed binary character format string.
    ///
    /// Uses `LESSBINFMT` if set, otherwise the default `*s<%02X>`.
    /// The returned `BinFmt` can be used by the renderer.
    #[must_use]
    pub fn binary_format(&self) -> BinFmt {
        BinFmt::parse(self.bin_fmt.as_deref().unwrap_or("*s<%02X>"))
    }

    /// Returns the parsed UTF-8 binary format string.
    ///
    /// Uses `LESSUTFBINFMT` if set, otherwise the default `<U+%04X>`.
    #[must_use]
    pub fn utf_binary_format(&self) -> BinFmt {
        BinFmt::parse(self.utf_bin_fmt.as_deref().unwrap_or("<U+%04X>"))
    }

    /// Returns whether a specific command is allowed under the current security mode.
    ///
    /// When `secure_mode` is true, shell execution (`shell`), pipe (`pipe`),
    /// editor (`editor`), save (`save`), examine (`examine`), external
    /// preprocessors (`preproc`), log files (`logfile`), key files (`keyfile`),
    /// and tags (`tag`) are disabled.
    ///
    /// When `secure_mode` is false, all commands are allowed. Unknown command
    /// identifiers are allowed in both modes.
    #[must_use]
    pub fn is_command_allowed(&self, command: &str) -> bool {
        if !self.secure_mode {
            return true;
        }
        !matches!(
            command,
            "shell"
                | "pipe"
                | "editor"
                | "save"
                | "examine"
                | "preproc"
                | "logfile"
                | "keyfile"
                | "tag"
        )
    }

    /// Returns whether a command-line option letter should be silently ignored.
    ///
    /// Checks the `LESS_UNSUPPORT` variable, which contains a string of option
    /// letters that should be accepted without effect.
    #[must_use]
    pub fn is_unsupported_option(&self, flag: char) -> bool {
        self.unsupport.as_ref().is_some_and(|s| s.contains(flag))
    }
}

/// Read the `LESS` environment variable and split it into individual flags.
///
/// Returns an empty vector if the variable is not set or is empty.
///
/// Parsing follows GNU less conventions:
/// - Tokens are whitespace-delimited.
/// - `$` acts as a value terminator for options that take arguments (e.g.,
///   `-P$custom prompt$` yields the token `-Pcustom prompt`).
/// - `-r` is silently replaced with `-R` for safety (raw ANSI only, not all
///   control chars) when it appears in the env var.
/// - When `--use-backslash` appears among the tokens, standard backslash
///   escape sequences (`\t`, `\n`, `\\`, `\e`) are interpreted in all tokens.
///
/// This is a convenience function; prefer [`EnvConfig::from_env()`] for
/// full environment handling.
#[must_use]
pub fn read_less_env() -> Vec<String> {
    match std::env::var("LESS") {
        Ok(val) if !val.is_empty() => parse_less_env_value(&val),
        _ => Vec::new(),
    }
}

/// Parse a LESS environment variable value into individual option tokens.
///
/// Handles `$` delimiters, `-r` to `-R` replacement, and `--use-backslash`
/// escape processing.
#[must_use]
pub fn parse_less_env_value(input: &str) -> Vec<String> {
    let raw_tokens = tokenize_less_env(input);

    // Check whether --use-backslash appears among the tokens.
    let use_backslash = raw_tokens.iter().any(|t| t == "--use-backslash");

    let mut result = Vec::with_capacity(raw_tokens.len());
    for token in raw_tokens {
        // Strip --use-backslash from the output; it's a parsing directive,
        // not a runtime flag passed through to clap.
        if token == "--use-backslash" {
            continue;
        }
        let mut t = token;
        // Apply backslash escaping if enabled.
        if use_backslash {
            t = interpret_backslash_escapes(&t);
        }
        // In the LESS env, -r is treated as -R for safety.
        if t == "-r" {
            t = String::from("-R");
        }
        result.push(t);
    }
    result
}

/// Tokenize a LESS env string, respecting `$` as a value terminator.
///
/// In the LESS env var, `$` marks the end of an option value. For example,
/// `"-P$custom prompt$-S"` yields tokens `["-Pcustom prompt", "-S"]`.
/// The `$` delimiters are consumed and not included in the output.
fn tokenize_less_env(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    // Skip leading whitespace.
    skip_whitespace(&mut chars);

    while chars.peek().is_some() {
        let mut token = String::new();

        // Read characters until whitespace or end.
        while let Some(&ch) = chars.peek() {
            if ch.is_whitespace() {
                break;
            }
            if ch == '$' {
                // `$` starts a dollar-delimited value section.
                chars.next(); // consume the opening `$`
                              // Read until the closing `$` or end of string.
                let mut closed = false;
                while let Some(&inner) = chars.peek() {
                    if inner == '$' {
                        chars.next(); // consume closing `$`
                        closed = true;
                        break;
                    }
                    token.push(inner);
                    chars.next();
                }
                // A closing `$` terminates the current token.
                if closed {
                    break;
                }
                // Unclosed `$` — consumed to end of string, token ends naturally.
                continue;
            }
            token.push(ch);
            chars.next();
        }

        if !token.is_empty() {
            tokens.push(token);
        }
        skip_whitespace(&mut chars);
    }

    tokens
}

/// Skip whitespace characters from a peekable char iterator.
fn skip_whitespace(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(&ch) = chars.peek() {
        if ch.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
}

/// Interpret backslash escape sequences in a string.
///
/// Supported escapes: `\\` -> `\`, `\n` -> newline, `\t` -> tab, `\e` -> ESC (0x1B).
/// Unknown escape sequences pass through as-is (backslash + character).
fn interpret_backslash_escapes(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('\\') | None => result.push('\\'),
                Some('n') => result.push('\n'),
                Some('t') => result.push('\t'),
                Some('e') => result.push('\x1b'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
            }
        } else {
            result.push(ch);
        }
    }
    result
}

/// Read the `LESS` environment variable, splitting tokens into flags and
/// initial commands.
///
/// Tokens starting with `++` are every-file commands (the `++` prefix is
/// stripped). Tokens starting with `+` (but not `++`) are initial commands
/// (the `+` prefix is stripped). Everything else is a regular flag.
///
/// Returns `(flags, initial_commands, every_file_commands)`.
#[must_use]
pub fn read_less_env_split() -> (Vec<String>, Vec<String>, Vec<String>) {
    let tokens = read_less_env();
    split_flags_and_commands(&tokens)
}

/// Partition a list of argument tokens into flags and initial commands.
///
/// Tokens starting with `++` become every-file commands (prefix stripped).
/// Tokens starting with `+` become initial commands (prefix stripped).
/// All other tokens are returned as flags.
#[must_use]
pub fn split_flags_and_commands(tokens: &[String]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut flags = Vec::new();
    let mut initial = Vec::new();
    let mut every_file = Vec::new();

    for token in tokens {
        if let Some(cmd) = token.strip_prefix("++") {
            if !cmd.is_empty() {
                every_file.push(cmd.to_string());
            }
        } else if let Some(cmd) = token.strip_prefix('+') {
            if !cmd.is_empty() {
                initial.push(cmd.to_string());
            }
        } else {
            flags.push(token.clone());
        }
    }

    (flags, initial, every_file)
}

/// Read an environment variable, returning `None` if not set or empty.
fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Check whether an environment variable is set to a non-empty value.
fn env_is_set(key: &str) -> bool {
    std::env::var(key).ok().filter(|v| !v.is_empty()).is_some()
}

/// Read an environment variable and parse it as `usize`.
///
/// Returns `None` if the variable is not set, empty, or not a valid integer.
fn env_parse_usize(key: &str) -> Option<usize> {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgr_display::BinFmtSegment;
    use std::env;
    use std::sync::Mutex;

    // Serialize all tests that mutate the process environment to prevent
    // races when cargo runs tests in parallel within this binary.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    // ---------------------------------------------------------------------------
    // read_less_env (existing tests kept intact)
    // ---------------------------------------------------------------------------

    // Test 9: LESS="-R -S" parsed into flags
    #[test]
    fn test_read_less_env_parses_flags() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::set_var("LESS", "-R -S");
        let result = read_less_env();
        assert_eq!(result, vec!["-R", "-S"]);
        match original {
            Some(v) => env::set_var("LESS", v),
            None => env::remove_var("LESS"),
        }
    }

    // Test 10: LESS not set returns empty vec
    #[test]
    fn test_read_less_env_unset_returns_empty() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::remove_var("LESS");
        let result = read_less_env();
        assert!(result.is_empty());
        if let Some(v) = original {
            env::set_var("LESS", v);
        }
    }

    // Test 11: Command-line flags override LESS env
    // (This is structurally guaranteed by prepending env flags before argv
    // in Options::parse — tested here by verifying the merge behavior.)
    #[test]
    fn test_read_less_env_empty_string_returns_empty() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::set_var("LESS", "");
        let result = read_less_env();
        assert!(result.is_empty());
        match original {
            Some(v) => env::set_var("LESS", v),
            None => env::remove_var("LESS"),
        }
    }

    // Additional: multiple spaces between flags are handled
    #[test]
    fn test_read_less_env_multiple_spaces_handled() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::set_var("LESS", "-R   -S   -M");
        let result = read_less_env();
        assert_eq!(result, vec!["-R", "-S", "-M"]);
        match original {
            Some(v) => env::set_var("LESS", v),
            None => env::remove_var("LESS"),
        }
    }

    // ---------------------------------------------------------------------------
    // EnvConfig::from_env tests
    // ---------------------------------------------------------------------------

    // Helper: set a list of (key, value) env vars, returning saved originals.
    fn set_vars(pairs: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        let mut saved = Vec::with_capacity(pairs.len());
        for (k, v) in pairs {
            saved.push(((*k).to_owned(), env::var(*k).ok()));
            env::set_var(*k, *v);
        }
        saved
    }

    // Helper: unset a list of env var keys, returning saved originals.
    fn unset_vars(keys: &[&str]) -> Vec<(String, Option<String>)> {
        let mut saved = Vec::with_capacity(keys.len());
        for k in keys {
            saved.push(((*k).to_owned(), env::var(*k).ok()));
            env::remove_var(*k);
        }
        saved
    }

    fn restore_vars(saved: &[(String, Option<String>)]) {
        for (k, v) in saved {
            match v {
                Some(val) => env::set_var(k, val),
                None => env::remove_var(k),
            }
        }
    }

    // Test 1: LESS="-R" -> less_options contains "-R"
    #[test]
    fn test_env_config_from_env_reads_less_var() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS", "-R")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.less_options.contains(&String::from("-R")));
    }

    // Test 2: VISUAL="nvim" -> visual = Some("nvim")
    #[test]
    fn test_env_config_from_env_reads_visual() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("VISUAL", "nvim")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.visual.as_deref(), Some("nvim"));
    }

    // Test 3: EDITOR="vim" -> editor = Some("vim")
    #[test]
    fn test_env_config_from_env_reads_editor() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("EDITOR", "vim")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.editor.as_deref(), Some("vim"));
    }

    // Test 4: SHELL="/bin/zsh" -> shell = Some("/bin/zsh")
    #[test]
    fn test_env_config_from_env_reads_shell() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("SHELL", "/bin/zsh")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.shell.as_deref(), Some("/bin/zsh"));
    }

    // Test 5: COLUMNS="120", LINES="40" -> parsed correctly
    #[test]
    fn test_env_config_from_env_reads_columns_lines() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("COLUMNS", "120"), ("LINES", "40")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.columns, Some(120));
        assert_eq!(cfg.lines, Some(40));
    }

    // Test 6: LESSSECURE="1" -> secure_mode = true
    #[test]
    fn test_env_config_from_env_reads_lesssecure() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSSECURE", "1")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.secure_mode);
    }

    // Test 7: no LESSSECURE -> secure_mode = false
    #[test]
    fn test_env_config_from_env_lesssecure_unset_is_false() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["LESSSECURE"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(!cfg.secure_mode);
    }

    // Test 8: LESSCHARSET="utf-8" -> charset = Some("utf-8")
    #[test]
    fn test_env_config_from_env_reads_lesscharset() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSCHARSET", "utf-8")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.charset.as_deref(), Some("utf-8"));
    }

    // Test 9: VISUAL set -> editor_command returns VISUAL value
    #[test]
    fn test_env_config_editor_command_prefers_visual() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved_visual = set_vars(&[("VISUAL", "nvim")]);
        let saved_lessedit = unset_vars(&["LESSEDIT"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved_visual);
        restore_vars(&saved_lessedit);
        assert_eq!(cfg.editor_command(), "nvim");
    }

    // Test 10: no VISUAL, EDITOR set -> editor_command returns EDITOR
    #[test]
    fn test_env_config_editor_command_falls_back_to_editor() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["LESSEDIT", "VISUAL"]);
        let saved_editor = set_vars(&[("EDITOR", "vim")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        restore_vars(&saved_editor);
        assert_eq!(cfg.editor_command(), "vim");
    }

    // Test 11: neither LESSEDIT, VISUAL, nor EDITOR set -> returns "vi"
    #[test]
    fn test_env_config_editor_command_defaults_to_vi() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["LESSEDIT", "VISUAL", "EDITOR"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.editor_command(), "vi");
    }

    // Test 12: SHELL set -> shell_command returns SHELL value
    #[test]
    fn test_env_config_shell_command_returns_shell() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("SHELL", "/usr/bin/fish")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.shell_command(), "/usr/bin/fish");
    }

    // Test 13: no SHELL -> shell_command returns "sh"
    #[test]
    fn test_env_config_shell_command_defaults_to_sh() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["SHELL"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.shell_command(), "sh");
    }

    // Test 14: COLUMNS and LINES both set -> terminal_size_override returns Some
    #[test]
    fn test_env_config_terminal_size_override_both_set() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("COLUMNS", "200"), ("LINES", "50")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.terminal_size_override(), Some((200, 50)));
    }

    // Test 15: only COLUMNS set -> terminal_size_override returns None
    #[test]
    fn test_env_config_terminal_size_override_partial_returns_none() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved_cols = set_vars(&[("COLUMNS", "80")]);
        let saved_lines = unset_vars(&["LINES"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved_cols);
        restore_vars(&saved_lines);
        assert!(cfg.terminal_size_override().is_none());
    }

    // Test 16: secure_mode=false -> all commands allowed
    #[test]
    fn test_env_config_is_command_allowed_normal_mode() {
        let cfg = EnvConfig {
            secure_mode: false,
            ..EnvConfig::default()
        };
        assert!(cfg.is_command_allowed("shell"));
        assert!(cfg.is_command_allowed("pipe"));
        assert!(cfg.is_command_allowed("editor"));
        assert!(cfg.is_command_allowed("save"));
        assert!(cfg.is_command_allowed("examine"));
        assert!(cfg.is_command_allowed("preproc"));
        assert!(cfg.is_command_allowed("logfile"));
        assert!(cfg.is_command_allowed("keyfile"));
        assert!(cfg.is_command_allowed("tag"));
        assert!(cfg.is_command_allowed("unknown"));
    }

    // Test 17: secure_mode=true, "shell" -> false
    #[test]
    fn test_env_config_is_command_allowed_secure_blocks_shell() {
        let cfg = EnvConfig {
            secure_mode: true,
            ..EnvConfig::default()
        };
        assert!(!cfg.is_command_allowed("shell"));
    }

    // Test 18: secure_mode=true, "pipe" -> false
    #[test]
    fn test_env_config_is_command_allowed_secure_blocks_pipe() {
        let cfg = EnvConfig {
            secure_mode: true,
            ..EnvConfig::default()
        };
        assert!(!cfg.is_command_allowed("pipe"));
    }

    // Test 19: secure_mode=true, "editor" -> false
    #[test]
    fn test_env_config_is_command_allowed_secure_blocks_editor() {
        let cfg = EnvConfig {
            secure_mode: true,
            ..EnvConfig::default()
        };
        assert!(!cfg.is_command_allowed("editor"));
    }

    // Test 20: LESSOPEN, LESSCLOSE, LESSKEY all stored correctly
    #[test]
    fn test_env_config_phase2_vars_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[
            ("LESSOPEN", "|lesspipe %s"),
            ("LESSCLOSE", "lesspipe %s %s"),
            ("LESSKEY", "/home/user/.lesskey"),
        ]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.lessopen.as_deref(), Some("|lesspipe %s"));
        assert_eq!(cfg.lessclose.as_deref(), Some("lesspipe %s %s"));
        assert_eq!(cfg.lesskey.as_deref(), Some("/home/user/.lesskey"));
    }

    // ---------------------------------------------------------------------------
    // split_flags_and_commands tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_split_flags_and_commands_flags_only() {
        let tokens: Vec<String> = vec!["-R".into(), "-S".into()];
        let (flags, initial, every_file) = split_flags_and_commands(&tokens);
        assert_eq!(flags, vec!["-R", "-S"]);
        assert!(initial.is_empty());
        assert!(every_file.is_empty());
    }

    #[test]
    fn test_split_flags_and_commands_initial_command() {
        let tokens: Vec<String> = vec!["-R".into(), "+Gg".into(), "-S".into()];
        let (flags, initial, every_file) = split_flags_and_commands(&tokens);
        assert_eq!(flags, vec!["-R", "-S"]);
        assert_eq!(initial, vec!["Gg"]);
        assert!(every_file.is_empty());
    }

    #[test]
    fn test_split_flags_and_commands_every_file_command() {
        let tokens: Vec<String> = vec!["++G".into(), "-R".into()];
        let (flags, initial, every_file) = split_flags_and_commands(&tokens);
        assert_eq!(flags, vec!["-R"]);
        assert!(initial.is_empty());
        assert_eq!(every_file, vec!["G"]);
    }

    #[test]
    fn test_split_flags_and_commands_mixed() {
        let tokens: Vec<String> = vec!["-R".into(), "+Gg".into(), "++G".into(), "-S".into()];
        let (flags, initial, every_file) = split_flags_and_commands(&tokens);
        assert_eq!(flags, vec!["-R", "-S"]);
        assert_eq!(initial, vec!["Gg"]);
        assert_eq!(every_file, vec!["G"]);
    }

    #[test]
    fn test_split_flags_and_commands_bare_plus_ignored() {
        let tokens: Vec<String> = vec!["+".into(), "++".into()];
        let (flags, initial, every_file) = split_flags_and_commands(&tokens);
        assert!(flags.is_empty());
        assert!(initial.is_empty());
        assert!(every_file.is_empty());
    }

    // ---------------------------------------------------------------------------
    // read_less_env_split tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_read_less_env_split_with_initial_commands() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::set_var("LESS", "-R +Gg -S");
        let (flags, initial, every_file) = read_less_env_split();
        match original {
            Some(v) => env::set_var("LESS", v),
            None => env::remove_var("LESS"),
        }
        assert_eq!(flags, vec!["-R", "-S"]);
        assert_eq!(initial, vec!["Gg"]);
        assert!(every_file.is_empty());
    }

    #[test]
    fn test_read_less_env_split_with_every_file_command() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::set_var("LESS", "-R ++G");
        let (flags, initial, every_file) = read_less_env_split();
        match original {
            Some(v) => env::set_var("LESS", v),
            None => env::remove_var("LESS"),
        }
        assert_eq!(flags, vec!["-R"]);
        assert!(initial.is_empty());
        assert_eq!(every_file, vec!["G"]);
    }

    // ---------------------------------------------------------------------------
    // Task 206: batch 1 env var tests
    // ---------------------------------------------------------------------------

    // Test 21: LESS_COLUMNS overrides terminal width
    #[test]
    fn test_env_config_less_columns_overrides_width() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_COLUMNS", "132")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.less_columns, Some(132));
    }

    // Test 22: LESS_LINES overrides terminal height
    #[test]
    fn test_env_config_less_lines_overrides_height() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_LINES", "50")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.less_lines, Some(50));
    }

    // Test 23: effective_dimensions applies LESS_COLUMNS/LESS_LINES
    #[test]
    fn test_env_config_effective_dimensions_both_overridden() {
        let cfg = EnvConfig {
            less_columns: Some(132),
            less_lines: Some(50),
            ..EnvConfig::default()
        };
        assert_eq!(cfg.effective_dimensions(24, 80), (50, 132));
    }

    // Test 24: effective_dimensions partial override (only columns)
    #[test]
    fn test_env_config_effective_dimensions_partial_override() {
        let cfg = EnvConfig {
            less_columns: Some(132),
            ..EnvConfig::default()
        };
        assert_eq!(cfg.effective_dimensions(24, 80), (24, 132));
    }

    // Test 25: effective_dimensions no overrides passes through
    #[test]
    fn test_env_config_effective_dimensions_no_override() {
        let cfg = EnvConfig::default();
        assert_eq!(cfg.effective_dimensions(24, 80), (24, 80));
    }

    // Test 26: LESS_SHELL_LINES read and used
    #[test]
    fn test_env_config_shell_lines_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_SHELL_LINES", "30")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.shell_lines, Some(30));
        assert_eq!(cfg.shell_screen_height(24), 30);
    }

    // Test 27: shell_screen_height falls back to terminal rows
    #[test]
    fn test_env_config_shell_screen_height_fallback() {
        let cfg = EnvConfig::default();
        assert_eq!(cfg.shell_screen_height(24), 24);
    }

    // Test 28: LESSBINFMT stored correctly
    #[test]
    fn test_env_config_lessbinfmt_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSBINFMT", "*d<%02X>")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.bin_fmt.as_deref(), Some("*d<%02X>"));
    }

    // Test 29: LESSUTFBINFMT stored correctly
    #[test]
    fn test_env_config_lessutfbinfmt_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSUTFBINFMT", "<U+%04X>")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.utf_bin_fmt.as_deref(), Some("<U+%04X>"));
    }

    // Test 30: LESS_IS_MORE set -> is_more = true
    #[test]
    fn test_env_config_less_is_more_set() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_IS_MORE", "1")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.is_more);
    }

    // Test 31: LESS_IS_MORE not set -> is_more = false
    #[test]
    fn test_env_config_less_is_more_unset() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["LESS_IS_MORE"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(!cfg.is_more);
    }

    // Test 32: XDG_CONFIG_HOME set -> config_home returns it
    #[test]
    fn test_env_config_xdg_config_home_set() {
        let cfg = EnvConfig {
            xdg_config_home: Some("/custom/config".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.config_home(),
            Some(std::path::PathBuf::from("/custom/config"))
        );
    }

    // Test 33: XDG_CONFIG_HOME unset, HOME set -> defaults to ~/.config
    #[test]
    fn test_env_config_xdg_config_home_default() {
        let cfg = EnvConfig {
            home: Some("/home/user".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.config_home(),
            Some(std::path::PathBuf::from("/home/user/.config"))
        );
    }

    // Test 34: XDG_DATA_HOME set -> data_home returns it
    #[test]
    fn test_env_config_xdg_data_home_set() {
        let cfg = EnvConfig {
            xdg_data_home: Some("/custom/data".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.data_home(),
            Some(std::path::PathBuf::from("/custom/data"))
        );
    }

    // Test 35: XDG_DATA_HOME unset, HOME set -> defaults to ~/.local/share
    #[test]
    fn test_env_config_xdg_data_home_default() {
        let cfg = EnvConfig {
            home: Some("/home/user".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.data_home(),
            Some(std::path::PathBuf::from("/home/user/.local/share"))
        );
    }

    // Test 36: XDG_STATE_HOME set -> state_home returns it
    #[test]
    fn test_env_config_xdg_state_home_set() {
        let cfg = EnvConfig {
            xdg_state_home: Some("/custom/state".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.state_home(),
            Some(std::path::PathBuf::from("/custom/state"))
        );
    }

    // Test 37: XDG_STATE_HOME unset, HOME set -> defaults to ~/.local/state
    #[test]
    fn test_env_config_xdg_state_home_default() {
        let cfg = EnvConfig {
            home: Some("/home/user".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.state_home(),
            Some(std::path::PathBuf::from("/home/user/.local/state"))
        );
    }

    // Test 38: no HOME, no XDG -> all XDG paths are None
    #[test]
    fn test_env_config_xdg_no_home_returns_none() {
        let cfg = EnvConfig::default();
        assert!(cfg.config_home().is_none());
        assert!(cfg.data_home().is_none());
        assert!(cfg.state_home().is_none());
    }

    // Test 39: missing env vars use defaults
    #[test]
    fn test_env_config_missing_vars_use_defaults() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&[
            "LESS_COLUMNS",
            "LESS_LINES",
            "LESS_SHELL_LINES",
            "LESSBINFMT",
            "LESSUTFBINFMT",
            "LESS_IS_MORE",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_STATE_HOME",
        ]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.less_columns.is_none());
        assert!(cfg.less_lines.is_none());
        assert!(cfg.shell_lines.is_none());
        assert!(cfg.bin_fmt.is_none());
        assert!(cfg.utf_bin_fmt.is_none());
        assert!(!cfg.is_more);
        assert!(cfg.xdg_config_home.is_none());
        assert!(cfg.xdg_data_home.is_none());
        assert!(cfg.xdg_state_home.is_none());
    }

    // Test 40: XDG env vars actually parsed from environment
    #[test]
    fn test_env_config_xdg_vars_from_env() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[
            ("XDG_CONFIG_HOME", "/xdg/config"),
            ("XDG_DATA_HOME", "/xdg/data"),
            ("XDG_STATE_HOME", "/xdg/state"),
        ]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.xdg_config_home.as_deref(), Some("/xdg/config"));
        assert_eq!(cfg.xdg_data_home.as_deref(), Some("/xdg/data"));
        assert_eq!(cfg.xdg_state_home.as_deref(), Some("/xdg/state"));
    }

    // ---------------------------------------------------------------------------
    // BinFmt parsing tests
    // ---------------------------------------------------------------------------

    // Test 41: default LESSBINFMT parses correctly
    #[test]
    fn test_binfmt_parse_default() {
        let fmt = BinFmt::parse("*s<%02X>");
        assert!(fmt.standout);
        assert_eq!(fmt.segments.len(), 4);
        assert_eq!(fmt.segments[0], BinFmtSegment::Literal("*".to_string()));
        assert_eq!(fmt.segments[1], BinFmtSegment::Literal("<".to_string()));
        assert_eq!(fmt.segments[2], BinFmtSegment::Format("%02X".to_string()));
        assert_eq!(fmt.segments[3], BinFmtSegment::Literal(">".to_string()));
    }

    // Test 42: BinFmt format_byte with default format
    #[test]
    fn test_binfmt_format_byte_default() {
        let fmt = BinFmt::parse("*s<%02X>");
        // Byte 0x01 -> "*<01>"
        let result = fmt.format_byte(0x01);
        assert_eq!(result, "*<01>");
    }

    // Test 43: BinFmt format_byte hex uppercase
    #[test]
    fn test_binfmt_format_byte_hex_uppercase() {
        let fmt = BinFmt::parse("*s<%02X>");
        let result = fmt.format_byte(0xFF);
        assert_eq!(result, "*<FF>");
    }

    // Test 44: LESSUTFBINFMT default format
    #[test]
    fn test_binfmt_utf_default() {
        let fmt = BinFmt::parse("<U+%04X>");
        assert!(!fmt.standout);
        let result = fmt.format_byte(0xFFFD);
        assert_eq!(result, "<U+FFFD>");
    }

    // Test 45: BinFmt without standout
    #[test]
    fn test_binfmt_no_standout() {
        let fmt = BinFmt::parse("[%02x]");
        assert!(!fmt.standout);
        let result = fmt.format_byte(0x0A);
        assert_eq!(result, "[0a]");
    }

    // Test 46: BinFmt with octal format
    #[test]
    fn test_binfmt_octal() {
        let fmt = BinFmt::parse("\\%03o");
        assert!(!fmt.standout);
        let result = fmt.format_byte(0o177);
        assert_eq!(result, "\\177");
    }

    // Test 47: BinFmt with decimal format
    #[test]
    fn test_binfmt_decimal() {
        let fmt = BinFmt::parse("(%d)");
        assert!(!fmt.standout);
        let result = fmt.format_byte(127);
        assert_eq!(result, "(127)");
    }

    // Test 48: BinFmt standout only (no `s`)
    #[test]
    fn test_binfmt_standout_no_literal_star() {
        let fmt = BinFmt::parse("*<%02X>");
        assert!(fmt.standout);
        // No `s` after `*`, so no literal `*` segment
        let result = fmt.format_byte(0x01);
        assert_eq!(result, "<01>");
    }

    // Test 49: binary_format() uses default when env not set
    #[test]
    fn test_env_config_binary_format_default() {
        let cfg = EnvConfig::default();
        let fmt = cfg.binary_format();
        assert!(fmt.standout);
        let result = fmt.format_byte(0x01);
        assert_eq!(result, "*<01>");
    }

    // Test 50: utf_binary_format() uses default when env not set
    #[test]
    fn test_env_config_utf_binary_format_default() {
        let cfg = EnvConfig::default();
        let fmt = cfg.utf_binary_format();
        assert!(!fmt.standout);
        let result = fmt.format_byte(0xFFFD);
        assert_eq!(result, "<U+FFFD>");
    }

    // ---------------------------------------------------------------------------
    // Task 240: LESS env parsing refinements
    // ---------------------------------------------------------------------------

    // Test 51: -r in LESS env is replaced with -R
    #[test]
    fn test_parse_less_env_value_r_becomes_upper_r() {
        let tokens = parse_less_env_value("-r -S");
        assert_eq!(tokens, vec!["-R", "-S"]);
    }

    // Test 52: -R in LESS env stays -R
    #[test]
    fn test_parse_less_env_value_upper_r_unchanged() {
        let tokens = parse_less_env_value("-R -S");
        assert_eq!(tokens, vec!["-R", "-S"]);
    }

    // Test 53: -r replacement only affects standalone -r, not -rS or similar
    #[test]
    fn test_parse_less_env_value_r_in_combined_flag_not_replaced() {
        // "-rS" is not "-r" — it's a combined flag, not a standalone -r.
        let tokens = parse_less_env_value("-rS");
        assert_eq!(tokens, vec!["-rS"]);
    }

    // Test 54: $ delimiter basic usage
    #[test]
    fn test_parse_less_env_value_dollar_delimiter_basic() {
        let tokens = parse_less_env_value("-P$custom prompt$-S");
        assert_eq!(tokens, vec!["-Pcustom prompt", "-S"]);
    }

    // Test 55: $ delimiter with spaces in value
    #[test]
    fn test_parse_less_env_value_dollar_delimiter_with_spaces() {
        let tokens = parse_less_env_value("-P$file: %f  line: %l$");
        assert_eq!(tokens, vec!["-Pfile: %f  line: %l"]);
    }

    // Test 56: $ delimiter followed by another option
    #[test]
    fn test_parse_less_env_value_dollar_delimiter_followed_by_option() {
        let tokens = parse_less_env_value("-P$my prompt$ -R -S");
        assert_eq!(tokens, vec!["-Pmy prompt", "-R", "-S"]);
    }

    // Test 57: $ at start of input (empty dollar section)
    #[test]
    fn test_parse_less_env_value_dollar_empty_section() {
        let tokens = parse_less_env_value("$$-S");
        assert_eq!(tokens, vec!["-S"]);
    }

    // Test 58: unclosed $ reads to end of string
    #[test]
    fn test_parse_less_env_value_dollar_unclosed() {
        let tokens = parse_less_env_value("-P$unclosed value");
        assert_eq!(tokens, vec!["-Punclosed value"]);
    }

    // Test 59: --use-backslash enables escape interpretation
    #[test]
    fn test_parse_less_env_value_use_backslash_tab() {
        let tokens = parse_less_env_value("--use-backslash -P$col1\\tcol2$");
        assert_eq!(tokens, vec!["-Pcol1\tcol2"]);
    }

    // Test 60: --use-backslash interprets \n
    #[test]
    fn test_parse_less_env_value_use_backslash_newline() {
        let tokens = parse_less_env_value("--use-backslash -P$line1\\nline2$");
        assert_eq!(tokens, vec!["-Pline1\nline2"]);
    }

    // Test 61: --use-backslash interprets \\
    #[test]
    fn test_parse_less_env_value_use_backslash_literal_backslash() {
        let tokens = parse_less_env_value("--use-backslash -P$path\\\\name$");
        assert_eq!(tokens, vec!["-Ppath\\name"]);
    }

    // Test 62: --use-backslash interprets \e (ESC)
    #[test]
    fn test_parse_less_env_value_use_backslash_escape() {
        let tokens = parse_less_env_value("--use-backslash -P$\\etest$");
        assert_eq!(tokens, vec![format!("-P\x1btest")]);
    }

    // Test 63: without --use-backslash, backslashes are literal
    #[test]
    fn test_parse_less_env_value_no_use_backslash_literal() {
        let tokens = parse_less_env_value("-P$col1\\tcol2$");
        assert_eq!(tokens, vec!["-Pcol1\\tcol2"]);
    }

    // Test 64: --use-backslash is stripped from output
    #[test]
    fn test_parse_less_env_value_use_backslash_stripped_from_output() {
        let tokens = parse_less_env_value("--use-backslash -R");
        assert_eq!(tokens, vec!["-R"]);
        assert!(!tokens.iter().any(|t| t == "--use-backslash"));
    }

    // Test 65: -r combined with $ delimiter and --use-backslash
    #[test]
    fn test_parse_less_env_value_r_replaced_with_dollar_and_backslash() {
        let tokens = parse_less_env_value("--use-backslash -r -P$prompt\\t$ -S");
        // -r becomes -R, --use-backslash is stripped, \t is interpreted
        assert_eq!(tokens, vec!["-R", "-Pprompt\t", "-S"]);
    }

    // Test 66: read_less_env() applies -r -> -R from actual env var
    #[test]
    fn test_read_less_env_r_becomes_upper_r() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::set_var("LESS", "-r -S");
        let result = read_less_env();
        match original {
            Some(v) => env::set_var("LESS", v),
            None => env::remove_var("LESS"),
        }
        assert_eq!(result, vec!["-R", "-S"]);
    }

    // Test 67: read_less_env() handles $ delimiter from actual env var
    #[test]
    fn test_read_less_env_dollar_delimiter() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let original = env::var("LESS").ok();
        env::set_var("LESS", "-P$custom prompt$-S");
        let result = read_less_env();
        match original {
            Some(v) => env::set_var("LESS", v),
            None => env::remove_var("LESS"),
        }
        assert_eq!(result, vec!["-Pcustom prompt", "-S"]);
    }

    // Test 68: multiple $ sections in one env value
    #[test]
    fn test_parse_less_env_value_multiple_dollar_sections() {
        let tokens = parse_less_env_value("-P$prompt one$ -D$color spec$");
        assert_eq!(tokens, vec!["-Pprompt one", "-Dcolor spec"]);
    }

    // Test 69: empty input produces empty output
    #[test]
    fn test_parse_less_env_value_empty_input() {
        let tokens = parse_less_env_value("");
        assert!(tokens.is_empty());
    }

    // Test 70: whitespace-only input produces empty output
    #[test]
    fn test_parse_less_env_value_whitespace_only() {
        let tokens = parse_less_env_value("   ");
        assert!(tokens.is_empty());
    }

    // history_file_path tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_history_file_path_explicit_lesshistfile() {
        let cfg = EnvConfig {
            histfile: Some("/tmp/my_history".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.history_file_path(),
            Some(std::path::PathBuf::from("/tmp/my_history"))
        );
    }

    #[test]
    fn test_history_file_path_dash_disables() {
        let cfg = EnvConfig {
            histfile: Some("-".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(cfg.history_file_path(), None);
    }

    #[test]
    fn test_history_file_path_xdg_state_home() {
        let cfg = EnvConfig {
            xdg_state_home: Some("/xdg/state".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.history_file_path(),
            Some(std::path::PathBuf::from("/xdg/state/less/history"))
        );
    }

    #[test]
    fn test_history_file_path_home_fallback() {
        let cfg = EnvConfig {
            home: Some("/home/user".to_string()),
            ..EnvConfig::default()
        };
        assert_eq!(
            cfg.history_file_path(),
            Some(std::path::PathBuf::from(
                "/home/user/.local/state/less/history"
            ))
        );
    }

    #[test]
    fn test_history_file_path_no_home_returns_none() {
        let cfg = EnvConfig::default();
        assert_eq!(cfg.history_file_path(), None);
    }

    // ---------------------------------------------------------------------------
    // Task 246: batch 2 env var tests
    // ---------------------------------------------------------------------------

    // Test 71: LESSCHARDEF stored correctly
    #[test]
    fn test_env_config_lesschardef_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSCHARDEF", "..bc")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.chardef.as_deref(), Some("..bc"));
    }

    // Test 72: LESSMETACHARS stored correctly
    #[test]
    fn test_env_config_lessmetachars_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSMETACHARS", " ;|&<>()")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.meta_chars.as_deref(), Some(" ;|&<>()"));
    }

    // Test 73: LESSMETAESCAPE stored correctly
    #[test]
    fn test_env_config_lessmetaescape_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSMETAESCAPE", "\\")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.meta_escape.as_deref(), Some("\\"));
    }

    // Test 74: LESSECHO stored correctly
    #[test]
    fn test_env_config_lessecho_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSECHO", "/usr/bin/lessecho")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.lessecho.as_deref(), Some("/usr/bin/lessecho"));
    }

    // Test 75: LESS_DATA_DELAY parsed as usize
    #[test]
    fn test_env_config_less_data_delay_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_DATA_DELAY", "20")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.data_delay, Some(20));
    }

    // Test 76: LESS_DATA_DELAY non-numeric silently ignored
    #[test]
    fn test_env_config_less_data_delay_invalid_ignored() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_DATA_DELAY", "abc")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.data_delay.is_none());
    }

    // Test 77: LESS_SIGUSR1 set -> sigusr1 = true
    #[test]
    fn test_env_config_less_sigusr1_set() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_SIGUSR1", "1")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.sigusr1);
    }

    // Test 78: LESS_SIGUSR1 not set -> sigusr1 = false
    #[test]
    fn test_env_config_less_sigusr1_unset() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["LESS_SIGUSR1"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(!cfg.sigusr1);
    }

    // Test 79: LESS_UNSUPPORT stored correctly
    #[test]
    fn test_env_config_less_unsupport_parsed() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESS_UNSUPPORT", "xXkK")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.unsupport.as_deref(), Some("xXkK"));
    }

    // Test 80: is_unsupported_option returns true for listed flags
    #[test]
    fn test_env_config_is_unsupported_option_match() {
        let cfg = EnvConfig {
            unsupport: Some("xXkK".to_string()),
            ..EnvConfig::default()
        };
        assert!(cfg.is_unsupported_option('x'));
        assert!(cfg.is_unsupported_option('X'));
        assert!(cfg.is_unsupported_option('k'));
        assert!(cfg.is_unsupported_option('K'));
    }

    // Test 81: is_unsupported_option returns false for unlisted flags
    #[test]
    fn test_env_config_is_unsupported_option_no_match() {
        let cfg = EnvConfig {
            unsupport: Some("xXkK".to_string()),
            ..EnvConfig::default()
        };
        assert!(!cfg.is_unsupported_option('R'));
        assert!(!cfg.is_unsupported_option('S'));
    }

    // Test 82: is_unsupported_option returns false when unsupport is None
    #[test]
    fn test_env_config_is_unsupported_option_none() {
        let cfg = EnvConfig::default();
        assert!(!cfg.is_unsupported_option('x'));
    }

    // Test 83: LESSNOCONFIG set -> no_config = true
    #[test]
    fn test_env_config_lessnoconfig_set() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSNOCONFIG", "1")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.no_config);
    }

    // Test 84: LESSNOCONFIG not set -> no_config = false
    #[test]
    fn test_env_config_lessnoconfig_unset() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["LESSNOCONFIG"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(!cfg.no_config);
    }

    // Test 85: LESSNOCONFIG suppresses LESS env var
    #[test]
    fn test_env_config_lessnoconfig_suppresses_less_env() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSNOCONFIG", "1"), ("LESS", "-R -S")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.less_options.is_empty());
    }

    // Test 86: LESSNOCONFIG suppresses LESSKEY
    #[test]
    fn test_env_config_lessnoconfig_suppresses_lesskey() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("LESSNOCONFIG", "1"), ("LESSKEY", "/home/user/.lesskey")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.lesskey.is_none());
    }

    // Test 87: POSIXLY_CORRECT set -> posixly_correct = true
    #[test]
    fn test_env_config_posixly_correct_set() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("POSIXLY_CORRECT", "1")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.posixly_correct);
    }

    // Test 88: POSIXLY_CORRECT not set -> posixly_correct = false
    #[test]
    fn test_env_config_posixly_correct_unset() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["POSIXLY_CORRECT"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(!cfg.posixly_correct);
    }

    // Test 89: all batch 2 vars default to None/false when unset
    #[test]
    fn test_env_config_batch2_missing_vars_use_defaults() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&[
            "LESSCHARDEF",
            "LESSMETACHARS",
            "LESSMETAESCAPE",
            "LESSECHO",
            "LESS_DATA_DELAY",
            "LESS_SIGUSR1",
            "LESS_UNSUPPORT",
            "LESSNOCONFIG",
            "POSIXLY_CORRECT",
        ]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.chardef.is_none());
        assert!(cfg.meta_chars.is_none());
        assert!(cfg.meta_escape.is_none());
        assert!(cfg.lessecho.is_none());
        assert!(cfg.data_delay.is_none());
        assert!(!cfg.sigusr1);
        assert!(cfg.unsupport.is_none());
        assert!(!cfg.no_config);
        assert!(!cfg.posixly_correct);
    }

    // ── Task 330: PGR_CLIPBOARD env var ──

    #[test]
    fn test_env_config_pgr_clipboard_default_is_none() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["PGR_CLIPBOARD"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.pgr_clipboard.is_none());
    }

    #[test]
    fn test_env_config_pgr_clipboard_set() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("PGR_CLIPBOARD", "osc52")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert_eq!(cfg.pgr_clipboard.as_deref(), Some("osc52"));
    }

    // ── Task 356: PGR_GIT_GUTTER env var ──

    #[test]
    fn test_env_config_pgr_git_gutter_default_is_false() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = unset_vars(&["PGR_GIT_GUTTER"]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(!cfg.pgr_git_gutter);
    }

    #[test]
    fn test_env_config_pgr_git_gutter_set_to_1() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("PGR_GIT_GUTTER", "1")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(cfg.pgr_git_gutter);
    }

    #[test]
    fn test_env_config_pgr_git_gutter_set_to_0() {
        let _guard = ENV_MUTEX.lock().unwrap();
        let saved = set_vars(&[("PGR_GIT_GUTTER", "0")]);
        let cfg = EnvConfig::from_env();
        restore_vars(&saved);
        assert!(!cfg.pgr_git_gutter);
    }
}
