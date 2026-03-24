//! LESSOPEN/LESSCLOSE preprocessor pipeline.
//!
//! GNU less supports a preprocessor pipeline via the `LESSOPEN` and `LESSCLOSE`
//! environment variables. When `LESSOPEN` is set, each filename is passed
//! through a preprocessor command before display. This is how `lesspipe` works.

use std::path::PathBuf;
use std::process::Command;

use crate::InputError;

/// The format of the LESSOPEN command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LessOpenFormat {
    /// `| command %s` — command's stdout replaces the file content.
    Pipe,
    /// `command %s` — command prints a replacement filename to stdout.
    Standard,
    /// `|| command %s` — like pipe, but also handles stdin (no filename).
    TwoPipe,
}

/// The result of running the LESSOPEN preprocessor.
#[derive(Debug)]
pub enum PreprocessResult {
    /// The preprocessor produced data to display instead of the file.
    PipeData(Vec<u8>),
    /// The preprocessor returned a replacement filename.
    ReplacementFile(PathBuf),
    /// The preprocessor did not produce output; use the original file.
    Unchanged,
}

/// Parsed LESSOPEN command template.
#[derive(Debug, Clone)]
struct ParsedOpen {
    /// The format determined by the prefix.
    format: LessOpenFormat,
    /// The command template with `%s` placeholder.
    template: String,
}

/// Preprocessor that implements the LESSOPEN/LESSCLOSE pipeline.
///
/// Reads `LESSOPEN` and `LESSCLOSE` from the provided values (typically
/// sourced from environment variables) and executes the preprocessing
/// commands for each file.
#[derive(Debug)]
pub struct Preprocessor {
    parsed: ParsedOpen,
    lessclose: Option<String>,
    shell: String,
}

impl Preprocessor {
    /// Creates a new preprocessor from the given LESSOPEN value.
    ///
    /// Returns `None` if `lessopen` is empty after stripping the format prefix.
    ///
    /// # Arguments
    ///
    /// * `lessopen` — The LESSOPEN environment variable value.
    /// * `lessclose` — The LESSCLOSE environment variable value, if set.
    /// * `shell` — The shell to use for command execution (e.g., `/bin/sh`).
    #[must_use]
    pub fn new(lessopen: &str, lessclose: Option<&str>, shell: &str) -> Option<Self> {
        let parsed = parse_lessopen(lessopen)?;
        Some(Self {
            parsed,
            lessclose: lessclose.map(String::from),
            shell: shell.to_string(),
        })
    }

    /// Returns the parsed LESSOPEN format.
    #[must_use]
    pub fn format(&self) -> LessOpenFormat {
        self.parsed.format
    }

    /// Returns whether this preprocessor handles stdin (two-pipe format).
    #[must_use]
    pub fn handles_stdin(&self) -> bool {
        self.parsed.format == LessOpenFormat::TwoPipe
    }

    /// Run the preprocessor on the given filename.
    ///
    /// # Errors
    ///
    /// Returns an error if the shell command fails to execute.
    pub fn preprocess(&self, filename: &str) -> crate::Result<PreprocessResult> {
        let cmd = build_command(&self.parsed.template, filename);
        let output = Command::new(&self.shell)
            .arg("-c")
            .arg(&cmd)
            .output()
            .map_err(|e| InputError::Message(format!("failed to run LESSOPEN command: {e}")))?;

        if !output.status.success() {
            return Ok(PreprocessResult::Unchanged);
        }

        match self.parsed.format {
            LessOpenFormat::Pipe | LessOpenFormat::TwoPipe => {
                if output.stdout.is_empty() {
                    Ok(PreprocessResult::Unchanged)
                } else {
                    Ok(PreprocessResult::PipeData(output.stdout))
                }
            }
            LessOpenFormat::Standard => {
                let replacement = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if replacement.is_empty() {
                    Ok(PreprocessResult::Unchanged)
                } else {
                    Ok(PreprocessResult::ReplacementFile(PathBuf::from(
                        replacement,
                    )))
                }
            }
        }
    }

    /// Run the LESSCLOSE cleanup command, if configured.
    ///
    /// # Arguments
    ///
    /// * `original` — The original filename.
    /// * `replacement` — The replacement filename (from standard format), or
    ///   `"-"` if the preprocessor produced pipe data.
    ///
    /// # Errors
    ///
    /// Returns an error if the shell command fails to execute.
    pub fn close(&self, original: &str, replacement: &str) -> crate::Result<()> {
        let Some(ref template) = self.lessclose else {
            return Ok(());
        };

        let cmd = build_close_command(template, original, replacement);
        Command::new(&self.shell)
            .arg("-c")
            .arg(&cmd)
            .status()
            .map_err(|e| InputError::Message(format!("failed to run LESSCLOSE command: {e}")))?;

        Ok(())
    }
}

/// Parse a LESSOPEN value into its format and command template.
///
/// Returns `None` if the command template is empty after stripping the prefix.
fn parse_lessopen(value: &str) -> Option<ParsedOpen> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (format, template) = if let Some(rest) = trimmed.strip_prefix("||") {
        (LessOpenFormat::TwoPipe, rest.trim())
    } else if let Some(rest) = trimmed.strip_prefix('|') {
        (LessOpenFormat::Pipe, rest.trim())
    } else {
        (LessOpenFormat::Standard, trimmed)
    };

    if template.is_empty() {
        return None;
    }

    Some(ParsedOpen {
        format,
        template: template.to_string(),
    })
}

/// Build the shell command string by substituting the filename into the template.
///
/// If the template contains `%s`, it is replaced with the shell-quoted filename.
/// If no `%s` is present, the quoted filename is appended after a space.
fn build_command(template: &str, filename: &str) -> String {
    let quoted = shell_quote(filename);
    if template.contains("%s") {
        template.replace("%s", &quoted)
    } else {
        format!("{template} {quoted}")
    }
}

/// Build the LESSCLOSE command string.
///
/// Replaces the first `%s` with the original filename and the second with the
/// replacement filename. If fewer than two `%s` markers exist, remaining
/// filenames are appended.
fn build_close_command(template: &str, original: &str, replacement: &str) -> String {
    let quoted_orig = shell_quote(original);
    let quoted_repl = shell_quote(replacement);

    // Replace first %s with original, second with replacement.
    let mut result = String::with_capacity(template.len() + quoted_orig.len() + quoted_repl.len());
    let mut remaining = template;
    let mut replaced_first = false;
    let mut replaced_second = false;

    while let Some(pos) = remaining.find("%s") {
        result.push_str(&remaining[..pos]);
        if !replaced_first {
            result.push_str(&quoted_orig);
            replaced_first = true;
        } else if !replaced_second {
            result.push_str(&quoted_repl);
            replaced_second = true;
        } else {
            result.push_str("%s");
        }
        remaining = &remaining[pos + 2..];
    }
    result.push_str(remaining);

    // If not enough %s placeholders were found, append missing args.
    if !replaced_first {
        result.push(' ');
        result.push_str(&quoted_orig);
    }
    if !replaced_second {
        result.push(' ');
        result.push_str(&quoted_repl);
    }

    result
}

/// Shell-quote a filename to prevent injection.
///
/// Wraps the filename in single quotes and escapes any embedded single quotes
/// using the `'\''` idiom (end quote, escaped quote, start quote).
fn shell_quote(s: &str) -> String {
    let mut quoted = String::with_capacity(s.len() + 2);
    quoted.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            // End the current single-quoted string, insert an escaped quote,
            // then start a new single-quoted string.
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── shell_quote tests ───────────────────────────────────────────────

    #[test]
    fn test_shell_quote_simple_filename() {
        assert_eq!(shell_quote("file.txt"), "'file.txt'");
    }

    #[test]
    fn test_shell_quote_filename_with_spaces() {
        assert_eq!(shell_quote("my file.txt"), "'my file.txt'");
    }

    #[test]
    fn test_shell_quote_filename_with_single_quotes() {
        assert_eq!(shell_quote("it's a file"), "'it'\\''s a file'");
    }

    #[test]
    fn test_shell_quote_filename_with_special_chars() {
        assert_eq!(shell_quote("file;rm -rf /"), "'file;rm -rf /'");
    }

    #[test]
    fn test_shell_quote_empty_string() {
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn test_shell_quote_dollar_backtick() {
        assert_eq!(shell_quote("$HOME/`cmd`"), "'$HOME/`cmd`'");
    }

    // ── parse_lessopen tests ────────────────────────────────────────────

    #[test]
    fn test_parse_lessopen_pipe_format() {
        let parsed = parse_lessopen("| lesspipe %s").unwrap();
        assert_eq!(parsed.format, LessOpenFormat::Pipe);
        assert_eq!(parsed.template, "lesspipe %s");
    }

    #[test]
    fn test_parse_lessopen_pipe_format_no_space() {
        let parsed = parse_lessopen("|lesspipe %s").unwrap();
        assert_eq!(parsed.format, LessOpenFormat::Pipe);
        assert_eq!(parsed.template, "lesspipe %s");
    }

    #[test]
    fn test_parse_lessopen_standard_format() {
        let parsed = parse_lessopen("lesspipe %s").unwrap();
        assert_eq!(parsed.format, LessOpenFormat::Standard);
        assert_eq!(parsed.template, "lesspipe %s");
    }

    #[test]
    fn test_parse_lessopen_two_pipe_format() {
        let parsed = parse_lessopen("|| lesspipe %s").unwrap();
        assert_eq!(parsed.format, LessOpenFormat::TwoPipe);
        assert_eq!(parsed.template, "lesspipe %s");
    }

    #[test]
    fn test_parse_lessopen_two_pipe_format_no_space() {
        let parsed = parse_lessopen("||lesspipe %s").unwrap();
        assert_eq!(parsed.format, LessOpenFormat::TwoPipe);
        assert_eq!(parsed.template, "lesspipe %s");
    }

    #[test]
    fn test_parse_lessopen_empty_returns_none() {
        assert!(parse_lessopen("").is_none());
    }

    #[test]
    fn test_parse_lessopen_only_pipe_returns_none() {
        assert!(parse_lessopen("|").is_none());
    }

    #[test]
    fn test_parse_lessopen_only_two_pipe_returns_none() {
        assert!(parse_lessopen("||").is_none());
    }

    #[test]
    fn test_parse_lessopen_whitespace_only_returns_none() {
        assert!(parse_lessopen("   ").is_none());
    }

    // ── build_command tests ─────────────────────────────────────────────

    #[test]
    fn test_build_command_with_percent_s() {
        let cmd = build_command("lesspipe %s", "file.txt");
        assert_eq!(cmd, "lesspipe 'file.txt'");
    }

    #[test]
    fn test_build_command_without_percent_s_appends() {
        let cmd = build_command("lesspipe", "file.txt");
        assert_eq!(cmd, "lesspipe 'file.txt'");
    }

    #[test]
    fn test_build_command_with_spaces_in_filename() {
        let cmd = build_command("cat %s", "my file.txt");
        assert_eq!(cmd, "cat 'my file.txt'");
    }

    #[test]
    fn test_build_command_with_path() {
        let cmd = build_command("/usr/bin/lesspipe %s", "/tmp/data.gz");
        assert_eq!(cmd, "/usr/bin/lesspipe '/tmp/data.gz'");
    }

    // ── build_close_command tests ───────────────────────────────────────

    #[test]
    fn test_build_close_command_two_percent_s() {
        let cmd = build_close_command("lesspipe %s %s", "orig.txt", "repl.txt");
        assert_eq!(cmd, "lesspipe 'orig.txt' 'repl.txt'");
    }

    #[test]
    fn test_build_close_command_one_percent_s() {
        let cmd = build_close_command("cleanup %s", "orig.txt", "repl.txt");
        assert_eq!(cmd, "cleanup 'orig.txt' 'repl.txt'");
    }

    #[test]
    fn test_build_close_command_no_percent_s() {
        let cmd = build_close_command("cleanup", "orig.txt", "repl.txt");
        assert_eq!(cmd, "cleanup 'orig.txt' 'repl.txt'");
    }

    // ── Preprocessor::new tests ─────────────────────────────────────────

    #[test]
    fn test_preprocessor_new_pipe_format() {
        let p = Preprocessor::new("| cat %s", None, "/bin/sh").unwrap();
        assert_eq!(p.format(), LessOpenFormat::Pipe);
        assert!(!p.handles_stdin());
    }

    #[test]
    fn test_preprocessor_new_two_pipe_format() {
        let p = Preprocessor::new("|| cat %s", None, "/bin/sh").unwrap();
        assert_eq!(p.format(), LessOpenFormat::TwoPipe);
        assert!(p.handles_stdin());
    }

    #[test]
    fn test_preprocessor_new_standard_format() {
        let p = Preprocessor::new("echo %s", None, "/bin/sh").unwrap();
        assert_eq!(p.format(), LessOpenFormat::Standard);
        assert!(!p.handles_stdin());
    }

    #[test]
    fn test_preprocessor_new_empty_returns_none() {
        assert!(Preprocessor::new("", None, "/bin/sh").is_none());
    }

    #[test]
    fn test_preprocessor_new_pipe_only_returns_none() {
        assert!(Preprocessor::new("|", None, "/bin/sh").is_none());
    }

    // ── Preprocessor::preprocess integration tests ──────────────────────

    /// Helper: write `content` to a temporary file and return the handle.
    fn make_temp_file(content: &[u8]) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().expect("failed to create temp file");
        f.write_all(content).expect("failed to write");
        f.flush().expect("failed to flush");
        f
    }

    #[test]
    fn test_preprocess_pipe_format_returns_pipe_data() {
        let tmp = make_temp_file(b"hello world\n");
        let path = tmp.path().to_str().unwrap();

        let p = Preprocessor::new("| cat %s", None, "/bin/sh").unwrap();
        let result = p.preprocess(path).unwrap();

        match result {
            PreprocessResult::PipeData(data) => {
                assert_eq!(data, b"hello world\n");
            }
            other => panic!("expected PipeData, got {other:?}"),
        }
    }

    #[test]
    fn test_preprocess_pipe_format_empty_output_returns_unchanged() {
        let p = Preprocessor::new("| true", None, "/bin/sh").unwrap();
        let result = p.preprocess("nonexistent_file").unwrap();
        assert!(matches!(result, PreprocessResult::Unchanged));
    }

    #[test]
    fn test_preprocess_standard_format_returns_replacement_file() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap();

        // echo outputs the filename back — simulates a preprocessor that
        // returns a replacement filename.
        let p = Preprocessor::new("echo %s", None, "/bin/sh").unwrap();
        let result = p.preprocess(path).unwrap();

        match result {
            PreprocessResult::ReplacementFile(repl) => {
                assert_eq!(repl, PathBuf::from(path));
            }
            other => panic!("expected ReplacementFile, got {other:?}"),
        }
    }

    #[test]
    fn test_preprocess_standard_format_empty_returns_unchanged() {
        let p = Preprocessor::new("true", None, "/bin/sh").unwrap();
        let result = p.preprocess("some_file").unwrap();
        assert!(matches!(result, PreprocessResult::Unchanged));
    }

    #[test]
    fn test_preprocess_failed_command_returns_unchanged() {
        let p = Preprocessor::new("| false", None, "/bin/sh").unwrap();
        let result = p.preprocess("some_file").unwrap();
        assert!(matches!(result, PreprocessResult::Unchanged));
    }

    #[test]
    fn test_preprocess_filename_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("my file.txt");
        std::fs::write(&file_path, b"spaced content\n").unwrap();

        let p = Preprocessor::new("| cat %s", None, "/bin/sh").unwrap();
        let result = p.preprocess(file_path.to_str().unwrap()).unwrap();

        match result {
            PreprocessResult::PipeData(data) => {
                assert_eq!(data, b"spaced content\n");
            }
            other => panic!("expected PipeData, got {other:?}"),
        }
    }

    #[test]
    fn test_preprocess_filename_with_special_chars() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("file;echo pwned");
        std::fs::write(&file_path, b"safe\n").unwrap();

        let p = Preprocessor::new("| cat %s", None, "/bin/sh").unwrap();
        let result = p.preprocess(file_path.to_str().unwrap()).unwrap();

        match result {
            PreprocessResult::PipeData(data) => {
                assert_eq!(data, b"safe\n");
            }
            other => panic!("expected PipeData, got {other:?}"),
        }
    }

    // ── Preprocessor::close tests ───────────────────────────────────────

    #[test]
    fn test_close_no_lessclose_is_noop() {
        let p = Preprocessor::new("| cat %s", None, "/bin/sh").unwrap();
        // Should not error.
        p.close("orig.txt", "-").unwrap();
    }

    #[test]
    fn test_close_with_lessclose_runs_command() {
        let p = Preprocessor::new("| cat %s", Some("true %s %s"), "/bin/sh").unwrap();
        // Should succeed — `true` always exits 0.
        p.close("orig.txt", "repl.txt").unwrap();
    }

    // ── LESSOPEN without %s ─────────────────────────────────────────────

    #[test]
    fn test_preprocess_no_percent_s_appends_filename() {
        let tmp = make_temp_file(b"data\n");
        let path = tmp.path().to_str().unwrap();

        let p = Preprocessor::new("| cat", None, "/bin/sh").unwrap();
        let result = p.preprocess(path).unwrap();

        match result {
            PreprocessResult::PipeData(data) => {
                assert_eq!(data, b"data\n");
            }
            other => panic!("expected PipeData, got {other:?}"),
        }
    }
}
