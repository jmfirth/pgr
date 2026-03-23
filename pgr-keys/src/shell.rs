//! Shell command execution and command string expansion.
//!
//! Provides utilities for executing external commands from within the pager,
//! including prompt-style `%` escape expansion used by the `#` command.

use std::io::Write;
use std::process::Command as ProcessCommand;

/// Execute a shell command, returning its exit status.
///
/// Uses the given shell path (typically from the `SHELL` environment variable)
/// to run the command string via `shell -c command`.
///
/// # Errors
///
/// Returns an I/O error if the command fails to spawn or wait.
pub fn execute_shell_command(
    command: &str,
    shell: &str,
) -> std::io::Result<std::process::ExitStatus> {
    ProcessCommand::new(shell).args(["-c", command]).status()
}

/// Pipe the given content to a shell command's stdin and return its exit status.
///
/// Uses the given shell path to run `shell -c command`, writing `content`
/// to the child process's standard input.
///
/// # Errors
///
/// Returns an I/O error if spawning, writing, or waiting fails.
pub fn pipe_to_command(
    command: &str,
    shell: &str,
    content: &str,
) -> std::io::Result<std::process::ExitStatus> {
    let mut child = ProcessCommand::new(shell)
        .args(["-c", command])
        .stdin(std::process::Stdio::piped())
        .spawn()?;

    if let Some(ref mut stdin) = child.stdin {
        // Best-effort write; if the child closes stdin early we still wait.
        let _ = stdin.write_all(content.as_bytes());
    }
    // Drop stdin so the child sees EOF.
    drop(child.stdin.take());

    child.wait()
}

/// Expand prompt-style `%` escapes in a command string.
///
/// Supports a simplified subset of the `less` prompt mini-language:
///
/// - `%f` — current filename (or `"(stdin)"` if none)
/// - `%l` — current line number (1-indexed)
/// - `%L` — total line count
/// - `%b` — byte offset of the top-of-screen line
/// - `%%` — literal `%`
///
/// Unknown `%` sequences are passed through unchanged.
#[must_use]
pub fn expand_command_string(
    template: &str,
    filename: Option<&str>,
    line_number: usize,
    total_lines: usize,
    byte_offset: u64,
) -> String {
    let mut result = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('f') => {
                    chars.next();
                    result.push_str(filename.unwrap_or("(stdin)"));
                }
                Some('l') => {
                    chars.next();
                    result.push_str(&line_number.to_string());
                }
                Some('L') => {
                    chars.next();
                    result.push_str(&total_lines.to_string());
                }
                Some('b') => {
                    chars.next();
                    result.push_str(&byte_offset.to_string());
                }
                Some('%') => {
                    chars.next();
                    result.push('%');
                }
                _ => {
                    // Unknown escape: pass through unchanged.
                    result.push('%');
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Build the editor invocation command line.
///
/// If the editor string contains `%f` or `%lm` placeholders (the `LESSEDIT`
/// convention), they are expanded. Otherwise the editor is invoked as
/// `editor +line_number filename`.
#[must_use]
pub fn build_editor_command(editor: &str, filename: &str, line_number: usize) -> String {
    if editor.contains("%f") || editor.contains("%lm") {
        editor
            .replace("%f", filename)
            .replace("%lm", &line_number.to_string())
    } else {
        format!("{editor} +{line_number} {filename}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 4: execute_shell_command runs a simple command and returns success
    #[test]
    fn test_execute_shell_command_runs_true_returns_success() {
        let status = execute_shell_command("true", "sh").unwrap();
        assert!(status.success());
    }

    // Test 5: execute_shell_command uses the specified shell
    #[test]
    fn test_execute_shell_command_uses_specified_shell() {
        // Use /bin/sh explicitly; "echo ok" should succeed.
        let status = execute_shell_command("echo ok", "/bin/sh").unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_execute_shell_command_returns_failure_for_false() {
        let status = execute_shell_command("false", "sh").unwrap();
        assert!(!status.success());
    }

    // Test 6: expand_command_string replaces %f with filename
    #[test]
    fn test_expand_command_string_replaces_pct_f_with_filename() {
        let result = expand_command_string("cat %f", Some("test.txt"), 1, 100, 0);
        assert_eq!(result, "cat test.txt");
    }

    #[test]
    fn test_expand_command_string_pct_f_no_filename_uses_stdin() {
        let result = expand_command_string("cat %f", None, 1, 100, 0);
        assert_eq!(result, "cat (stdin)");
    }

    // Test 7: expand_command_string replaces %l with line number
    #[test]
    fn test_expand_command_string_replaces_pct_l_with_line_number() {
        let result = expand_command_string("goto %l", Some("f.txt"), 42, 100, 0);
        assert_eq!(result, "goto 42");
    }

    #[test]
    fn test_expand_command_string_replaces_pct_upper_l_with_total_lines() {
        let result = expand_command_string("total %L", Some("f.txt"), 1, 500, 0);
        assert_eq!(result, "total 500");
    }

    #[test]
    fn test_expand_command_string_replaces_pct_b_with_byte_offset() {
        let result = expand_command_string("offset %b", Some("f.txt"), 1, 100, 1024);
        assert_eq!(result, "offset 1024");
    }

    #[test]
    fn test_expand_command_string_double_percent_becomes_literal() {
        let result = expand_command_string("100%%", Some("f.txt"), 1, 100, 0);
        assert_eq!(result, "100%");
    }

    #[test]
    fn test_expand_command_string_unknown_escape_passed_through() {
        let result = expand_command_string("val %z end", Some("f.txt"), 1, 100, 0);
        assert_eq!(result, "val %z end");
    }

    #[test]
    fn test_expand_command_string_multiple_escapes() {
        let result = expand_command_string("%f:%l/%L", Some("data.log"), 10, 200, 0);
        assert_eq!(result, "data.log:10/200");
    }

    #[test]
    fn test_expand_command_string_no_escapes_returns_unchanged() {
        let result = expand_command_string("ls -la", Some("f.txt"), 1, 100, 0);
        assert_eq!(result, "ls -la");
    }

    #[test]
    fn test_expand_command_string_trailing_percent_passed_through() {
        let result = expand_command_string("end%", Some("f.txt"), 1, 100, 0);
        assert_eq!(result, "end%");
    }

    #[test]
    fn test_pipe_to_command_sends_content_to_stdin() {
        // Use `wc -l` to count lines piped in.
        let status = pipe_to_command("wc -l > /dev/null", "sh", "line1\nline2\n").unwrap();
        assert!(status.success());
    }

    #[test]
    fn test_build_editor_command_simple_editor() {
        let cmd = build_editor_command("vim", "test.txt", 42);
        assert_eq!(cmd, "vim +42 test.txt");
    }

    #[test]
    fn test_build_editor_command_with_lessedit_template() {
        let cmd = build_editor_command("vim +%lm %f", "test.txt", 42);
        assert_eq!(cmd, "vim +42 test.txt");
    }

    #[test]
    fn test_build_editor_command_vi_fallback() {
        let cmd = build_editor_command("vi", "readme.md", 1);
        assert_eq!(cmd, "vi +1 readme.md");
    }
}
