//! Parser for lesskey source format files.
//!
//! Parses the human-readable lesskey source format used by GNU less 692+.
//! Only the `#command` section is currently supported; `#line-edit` and
//! `#env` sections are parsed but their contents are ignored.

use std::collections::HashMap;
use std::path::Path;

use crate::command::Command;
use crate::key::Key;

/// The section currently being parsed in a lesskey source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    /// `#command` section: key-to-command bindings.
    Command,
    /// `#line-edit` section: line-editing bindings (not yet implemented).
    LineEdit,
    /// `#env` section: environment variable overrides (not yet implemented).
    Env,
}

/// A single command binding parsed from a lesskey source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LesskeyBinding {
    /// The key that triggers this command.
    pub key: Key,
    /// The command to execute.
    pub command: Command,
}

/// Configuration parsed from a lesskey source file.
#[derive(Debug, Clone, Default)]
pub struct LesskeyConfig {
    /// Command section bindings (key -> command).
    pub command_bindings: Vec<LesskeyBinding>,
}

/// Parse a lesskey source file from the given path.
///
/// Returns `Ok(None)` if the file does not exist.
/// Returns `Ok(Some(config))` on successful parse.
/// Returns `Err` on I/O errors other than file-not-found.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read.
pub fn parse_lesskey_file(path: &Path) -> crate::error::Result<Option<LesskeyConfig>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    Ok(Some(parse_lesskey_source(&content)))
}

/// Parse lesskey source content from a string.
///
/// Lines starting with `#` are treated as section headers or comments.
/// The default section (before any header) is `#command`.
#[must_use]
pub fn parse_lesskey_source(content: &str) -> LesskeyConfig {
    let mut config = LesskeyConfig::default();
    let mut current_section = Section::Command;
    let action_map = build_action_map();

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip blank lines
        if trimmed.is_empty() {
            continue;
        }

        // Check for section headers
        if trimmed.starts_with('#') {
            match trimmed {
                "#command" => current_section = Section::Command,
                "#line-edit" => current_section = Section::LineEdit,
                "#env" => current_section = Section::Env,
                _ => {} // comment line — ignore
            }
            continue;
        }

        // Only process #command section lines
        if current_section != Section::Command {
            continue;
        }

        // Parse a command binding line: key-sequence  action-name  [extra-string]
        if let Some(binding) = parse_command_line(trimmed, &action_map) {
            config.command_bindings.push(binding);
        }
    }

    config
}

/// Parse a single command binding line.
///
/// Format: `key-sequence  action-name  [extra-string]`
/// The key sequence and action name are separated by whitespace.
fn parse_command_line(line: &str, action_map: &HashMap<&str, Command>) -> Option<LesskeyBinding> {
    // Strip inline comments: everything after an unescaped # is a comment
    let line = strip_inline_comment(line);
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Find the boundary between the key sequence and the action name.
    // The key sequence can contain escaped whitespace, so we parse it character by character.
    let (key_str, rest) = split_key_and_action(line)?;

    let key = parse_key_sequence(key_str)?;
    let action_name = rest.split_whitespace().next()?;

    let command = action_map.get(action_name)?;

    Some(LesskeyBinding {
        key,
        command: command.clone(),
    })
}

/// Strip an inline comment from a line.
///
/// A `#` that is not preceded by `\` starts a comment.
fn strip_inline_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Skip the escaped character
            i += 2;
            continue;
        }
        if bytes[i] == b'#' {
            return &line[..i];
        }
        i += 1;
    }
    line
}

/// Split a command line into the key-sequence part and the action part.
///
/// The key sequence may contain escape sequences like `\e`, `\t`, `\\`, `^X`, `\xNN`.
/// Returns `(key_part, action_part)`.
fn split_key_and_action(line: &str) -> Option<(&str, &str)> {
    let bytes = line.as_bytes();
    let mut i = 0;

    // Walk through the key sequence, handling escape sequences
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            // Skip the escape and next character(s)
            i += 1;
            if i < bytes.len() && bytes[i] == b'x' {
                // \xNN — skip two hex digits
                i += 1; // skip 'x'
                if i < bytes.len() && is_hex_digit(bytes[i]) {
                    i += 1;
                }
                if i < bytes.len() && is_hex_digit(bytes[i]) {
                    i += 1;
                }
            } else {
                i += 1; // skip escaped char
            }
            continue;
        }
        if bytes[i] == b'^' {
            // ^X — control character, skip two bytes
            i += 2;
            continue;
        }
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            // Found unescaped whitespace — this is the boundary
            let key_part = &line[..i];
            let rest = line[i..].trim_start();
            if rest.is_empty() {
                return None;
            }
            return Some((key_part, rest));
        }
        i += 1;
    }
    None
}

fn is_hex_digit(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

/// Parse a key sequence string into a `Key`.
///
/// Supports: `\e` (ESC), `\n` (newline/Enter), `\t` (tab), `\b` (backspace),
/// `\\` (backslash), `\xNN` (hex byte), `^X` (control char), plain chars.
fn parse_key_sequence(s: &str) -> Option<Key> {
    let bytes = s.as_bytes();

    if bytes.is_empty() {
        return None;
    }

    // Handle \e prefix (ESC followed by a character)
    if bytes.len() >= 3 && bytes[0] == b'\\' && bytes[1] == b'e' {
        // ESC + remaining character(s)
        let rest = &s[2..];
        if rest.len() == 1 {
            let ch = rest.chars().next()?;
            return Some(Key::EscSeq(ch));
        }
        // \e followed by a control char: \e^X
        if rest.len() == 2 && rest.starts_with('^') {
            let ctrl_char = rest.as_bytes()[1];
            let _ctrl_val = ctrl_char_value(ctrl_char)?;
            // ESC + Ctrl is complex; for now treat as EscSeq of the control char
            return None;
        }
        return None;
    }

    // Single escape sequences
    if bytes.len() == 2 && bytes[0] == b'\\' {
        return match bytes[1] {
            b'n' => Some(Key::Enter),
            b't' => Some(Key::Tab),
            b'b' => Some(Key::Backspace),
            b'\\' => Some(Key::Char('\\')),
            b'e' => Some(Key::Escape),
            _ => None,
        };
    }

    // \xNN hex byte
    if bytes.len() >= 4 && bytes[0] == b'\\' && bytes[1] == b'x' {
        let hex_str = &s[2..];
        if let Ok(byte_val) = u8::from_str_radix(hex_str, 16) {
            return key_from_byte(byte_val);
        }
        return None;
    }

    // ^X control character
    if bytes.len() == 2 && bytes[0] == b'^' {
        let ctrl_char = bytes[1];
        if let Some(val) = ctrl_char_value(ctrl_char) {
            return key_from_byte(val);
        }
        return None;
    }

    // Single plain character
    if s.chars().count() == 1 {
        let ch = s.chars().next()?;
        return Some(Key::Char(ch));
    }

    None
}

/// Convert a control character letter to its byte value.
/// `^A` = 1, `^Z` = 26, `^[` = 27 (ESC), etc.
fn ctrl_char_value(c: u8) -> Option<u8> {
    match c {
        b'@' => Some(0),
        b'A'..=b'Z' => Some(c - b'A' + 1),
        b'a'..=b'z' => Some(c - b'a' + 1),
        b'[' => Some(27),
        b'\\' => Some(28),
        b']' => Some(29),
        b'^' => Some(30),
        b'_' => Some(31),
        b'?' => Some(127),
        _ => None,
    }
}

/// Convert a raw byte value to a `Key`.
fn key_from_byte(val: u8) -> Option<Key> {
    match val {
        1..=26 => Some(Key::Ctrl((b'a' + val - 1) as char)),
        27 => Some(Key::Escape),
        127 => Some(Key::Backspace),
        _ if val.is_ascii_graphic() || val == b' ' => Some(Key::Char(val as char)),
        _ => None,
    }
}

/// Build the mapping from less action names to `Command` variants.
///
/// This covers the ~60 action names documented in the GNU less man page
/// under the LESSKEY section.
#[must_use]
fn build_action_map() -> HashMap<&'static str, Command> {
    let mut m = HashMap::new();

    // Navigation: forward/backward
    m.insert("forw-line", Command::ScrollForward(1));
    m.insert("back-line", Command::ScrollBackward(1));
    m.insert("forw-scroll", Command::PageForward);
    m.insert("back-scroll", Command::PageBackward);
    m.insert("forw-screen", Command::PageForward);
    m.insert("back-screen", Command::PageBackward);
    m.insert("forw-window", Command::WindowForward);
    m.insert("back-window", Command::WindowBackward);
    m.insert("forw-line-force", Command::ScrollForwardForce(1));
    m.insert("back-line-force", Command::ScrollBackwardForce(1));
    m.insert("forw-forever", Command::FollowMode);
    m.insert("forw-until-hilite", Command::FollowModeStopOnMatch);

    // Half-page navigation
    m.insert("forw-half-screen", Command::HalfPageForward);
    m.insert("back-half-screen", Command::HalfPageBackward);

    // Horizontal scrolling
    m.insert("right-scroll", Command::ScrollRight);
    m.insert("left-scroll", Command::ScrollLeft);

    // Goto navigation
    m.insert("goto-line", Command::GotoBeginning(None));
    m.insert("goto-end", Command::GotoEnd(None));
    m.insert("goto-end-buffered", Command::GotoEnd(None));
    m.insert("percent", Command::GotoPercent);

    // Repaint
    m.insert("repaint", Command::Repaint);
    m.insert("repaint-flush", Command::RepaintRefresh);

    // Search
    m.insert("forw-search", Command::SearchForward);
    m.insert("back-search", Command::SearchBackward);
    m.insert("repeat-search", Command::RepeatSearch);
    m.insert("reverse-search", Command::RepeatSearchReverse);
    m.insert("repeat-search-all", Command::RepeatSearch);
    m.insert("reverse-search-all", Command::RepeatSearchReverse);
    m.insert("undo-hilite", Command::ToggleHighlight);
    m.insert("filter", Command::Filter);

    // File management
    m.insert("examine", Command::Examine);
    m.insert("next-file", Command::NextFile);
    m.insert("prev-file", Command::PreviousFile);
    m.insert("first-file", Command::FirstFile);
    m.insert("remove-file", Command::RemoveFile);

    // Info and help
    m.insert("status", Command::FileInfo);
    m.insert("version", Command::Version);
    m.insert("help", Command::Help);

    // Quit
    m.insert("quit", Command::Quit);
    m.insert("quit-at-eof", Command::Quit);
    m.insert("quit-if-one-screen", Command::Quit);
    m.insert("abort", Command::Quit);

    // Options
    m.insert("toggle-option", Command::ToggleOption);
    m.insert("set-option", Command::ToggleOption);
    m.insert("toggle-option-string", Command::ToggleOption);
    m.insert("query-option", Command::QueryOption);

    // Shell and external
    m.insert("shell", Command::ShellCommand);
    m.insert("popen", Command::ShellCommandExpand);
    m.insert("pipe", Command::PipeToCommand);
    m.insert("visual", Command::EditFile);
    m.insert("edit", Command::EditFile);

    // Marks (map to Noop for now since pgr doesn't have mark commands yet)
    m.insert("set-mark", Command::Noop);
    m.insert("goto-mark", Command::Noop);
    m.insert("set-mark-bottom", Command::Noop);
    m.insert("clear-mark", Command::Noop);

    // Miscellaneous
    m.insert("noaction", Command::Noop);
    m.insert("invalid", Command::Noop);
    m.insert("debug", Command::Noop);

    // Force forward/backward
    m.insert("forw-screen-force", Command::ForwardForceEof);
    m.insert("back-screen-force", Command::BackwardForceBeginning);

    // Byte offset
    m.insert("goto-byte", Command::GotoByteOffset);

    // Clipboard yank (pgr extension)
    m.insert("yank-line", Command::YankLine);
    m.insert("yank-screen", Command::YankScreen);

    m
}

/// Return the number of action names in the action map.
///
/// Useful for tests to verify coverage.
#[must_use]
pub fn action_name_count() -> usize {
    build_action_map().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Key sequence parsing ──

    #[test]
    fn test_parse_key_sequence_plain_char() {
        assert_eq!(parse_key_sequence("j"), Some(Key::Char('j')));
    }

    #[test]
    fn test_parse_key_sequence_space() {
        // Space is a valid single character — it parses as Key::Char(' ').
        // In practice, space is a delimiter so it won't reach parse_key_sequence
        // in normal flow, but the function itself handles it correctly.
        assert_eq!(parse_key_sequence(" "), Some(Key::Char(' ')));
    }

    #[test]
    fn test_parse_key_sequence_escape_e() {
        assert_eq!(parse_key_sequence("\\e"), Some(Key::Escape));
    }

    #[test]
    fn test_parse_key_sequence_esc_char() {
        assert_eq!(parse_key_sequence("\\ek"), Some(Key::EscSeq('k')));
    }

    #[test]
    fn test_parse_key_sequence_esc_bracket() {
        assert_eq!(parse_key_sequence("\\e<"), Some(Key::EscSeq('<')));
    }

    #[test]
    fn test_parse_key_sequence_newline() {
        assert_eq!(parse_key_sequence("\\n"), Some(Key::Enter));
    }

    #[test]
    fn test_parse_key_sequence_tab() {
        assert_eq!(parse_key_sequence("\\t"), Some(Key::Tab));
    }

    #[test]
    fn test_parse_key_sequence_backspace() {
        assert_eq!(parse_key_sequence("\\b"), Some(Key::Backspace));
    }

    #[test]
    fn test_parse_key_sequence_backslash() {
        assert_eq!(parse_key_sequence("\\\\"), Some(Key::Char('\\')));
    }

    #[test]
    fn test_parse_key_sequence_hex_byte_printable() {
        // 0x6A = 'j'
        assert_eq!(parse_key_sequence("\\x6A"), Some(Key::Char('j')));
    }

    #[test]
    fn test_parse_key_sequence_hex_byte_ctrl() {
        // 0x01 = Ctrl-A
        assert_eq!(parse_key_sequence("\\x01"), Some(Key::Ctrl('a')));
    }

    #[test]
    fn test_parse_key_sequence_ctrl_char_upper() {
        assert_eq!(parse_key_sequence("^A"), Some(Key::Ctrl('a')));
    }

    #[test]
    fn test_parse_key_sequence_ctrl_char_lower() {
        assert_eq!(parse_key_sequence("^a"), Some(Key::Ctrl('a')));
    }

    #[test]
    fn test_parse_key_sequence_ctrl_z() {
        assert_eq!(parse_key_sequence("^Z"), Some(Key::Ctrl('z')));
    }

    #[test]
    fn test_parse_key_sequence_ctrl_bracket_is_escape() {
        assert_eq!(parse_key_sequence("^["), Some(Key::Escape));
    }

    #[test]
    fn test_parse_key_sequence_ctrl_question_is_backspace() {
        assert_eq!(parse_key_sequence("^?"), Some(Key::Backspace));
    }

    #[test]
    fn test_parse_key_sequence_empty_returns_none() {
        assert_eq!(parse_key_sequence(""), None);
    }

    #[test]
    fn test_parse_key_sequence_invalid_returns_none() {
        assert_eq!(parse_key_sequence("\\z"), None);
    }

    // ── ctrl_char_value ──

    #[test]
    fn test_ctrl_char_value_at_sign() {
        assert_eq!(ctrl_char_value(b'@'), Some(0));
    }

    #[test]
    fn test_ctrl_char_value_a_returns_1() {
        assert_eq!(ctrl_char_value(b'A'), Some(1));
    }

    #[test]
    fn test_ctrl_char_value_z_returns_26() {
        assert_eq!(ctrl_char_value(b'Z'), Some(26));
    }

    #[test]
    fn test_ctrl_char_value_lower_a_returns_1() {
        assert_eq!(ctrl_char_value(b'a'), Some(1));
    }

    #[test]
    fn test_ctrl_char_value_invalid_returns_none() {
        assert_eq!(ctrl_char_value(b'!'), None);
    }

    // ── key_from_byte ──

    #[test]
    fn test_key_from_byte_zero_returns_none() {
        assert_eq!(key_from_byte(0), None);
    }

    #[test]
    fn test_key_from_byte_1_returns_ctrl_a() {
        assert_eq!(key_from_byte(1), Some(Key::Ctrl('a')));
    }

    #[test]
    fn test_key_from_byte_27_returns_escape() {
        assert_eq!(key_from_byte(27), Some(Key::Escape));
    }

    #[test]
    fn test_key_from_byte_127_returns_backspace() {
        assert_eq!(key_from_byte(127), Some(Key::Backspace));
    }

    #[test]
    fn test_key_from_byte_printable_ascii() {
        assert_eq!(key_from_byte(b'A'), Some(Key::Char('A')));
    }

    #[test]
    fn test_key_from_byte_space() {
        assert_eq!(key_from_byte(b' '), Some(Key::Char(' ')));
    }

    #[test]
    fn test_key_from_byte_high_non_ascii_returns_none() {
        assert_eq!(key_from_byte(200), None);
    }

    // ── strip_inline_comment ──

    #[test]
    fn test_strip_inline_comment_removes_comment() {
        assert_eq!(
            strip_inline_comment("j  forw-line  # forward"),
            "j  forw-line  "
        );
    }

    #[test]
    fn test_strip_inline_comment_no_comment() {
        assert_eq!(strip_inline_comment("j  forw-line"), "j  forw-line");
    }

    #[test]
    fn test_strip_inline_comment_escaped_hash() {
        assert_eq!(strip_inline_comment("\\# forw-line"), "\\# forw-line");
    }

    // ── split_key_and_action ──

    #[test]
    fn test_split_key_and_action_simple() {
        let (key, action) = split_key_and_action("j forw-line").unwrap();
        assert_eq!(key, "j");
        assert_eq!(action, "forw-line");
    }

    #[test]
    fn test_split_key_and_action_esc_sequence() {
        let (key, action) = split_key_and_action("\\ek next-file").unwrap();
        assert_eq!(key, "\\ek");
        assert_eq!(action, "next-file");
    }

    #[test]
    fn test_split_key_and_action_ctrl_sequence() {
        let (key, action) = split_key_and_action("^A forw-line").unwrap();
        assert_eq!(key, "^A");
        assert_eq!(action, "forw-line");
    }

    #[test]
    fn test_split_key_and_action_hex_sequence() {
        let (key, action) = split_key_and_action("\\x6A forw-line").unwrap();
        assert_eq!(key, "\\x6A");
        assert_eq!(action, "forw-line");
    }

    #[test]
    fn test_split_key_and_action_tab_separator() {
        let (key, action) = split_key_and_action("j\tforw-line").unwrap();
        assert_eq!(key, "j");
        assert_eq!(action, "forw-line");
    }

    #[test]
    fn test_split_key_and_action_no_action_returns_none() {
        assert!(split_key_and_action("j").is_none());
    }

    // ── parse_command_line ──

    #[test]
    fn test_parse_command_line_simple_binding() {
        let action_map = build_action_map();
        let binding = parse_command_line("j  forw-line", &action_map).unwrap();
        assert_eq!(binding.key, Key::Char('j'));
        assert_eq!(binding.command, Command::ScrollForward(1));
    }

    #[test]
    fn test_parse_command_line_esc_binding() {
        let action_map = build_action_map();
        let binding = parse_command_line("\\ek  next-file", &action_map).unwrap();
        assert_eq!(binding.key, Key::EscSeq('k'));
        assert_eq!(binding.command, Command::NextFile);
    }

    #[test]
    fn test_parse_command_line_with_inline_comment() {
        let action_map = build_action_map();
        let binding = parse_command_line("q  quit  # exit pager", &action_map).unwrap();
        assert_eq!(binding.key, Key::Char('q'));
        assert_eq!(binding.command, Command::Quit);
    }

    #[test]
    fn test_parse_command_line_unknown_action_returns_none() {
        let action_map = build_action_map();
        assert!(parse_command_line("j  nonexistent-action", &action_map).is_none());
    }

    #[test]
    fn test_parse_command_line_ctrl_binding() {
        let action_map = build_action_map();
        let binding = parse_command_line("^F  forw-screen", &action_map).unwrap();
        assert_eq!(binding.key, Key::Ctrl('f'));
        assert_eq!(binding.command, Command::PageForward);
    }

    // ── parse_lesskey_source ──

    #[test]
    fn test_parse_lesskey_source_basic_command_section() {
        let content = "\
#command
j  forw-line
k  back-line
q  quit
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 3);
        assert_eq!(config.command_bindings[0].key, Key::Char('j'));
        assert_eq!(
            config.command_bindings[0].command,
            Command::ScrollForward(1)
        );
        assert_eq!(config.command_bindings[1].key, Key::Char('k'));
        assert_eq!(
            config.command_bindings[1].command,
            Command::ScrollBackward(1)
        );
        assert_eq!(config.command_bindings[2].key, Key::Char('q'));
        assert_eq!(config.command_bindings[2].command, Command::Quit);
    }

    #[test]
    fn test_parse_lesskey_source_default_section_is_command() {
        // Without a #command header, lines are still treated as command bindings
        let content = "j  forw-line\nk  back-line\n";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 2);
    }

    #[test]
    fn test_parse_lesskey_source_ignores_line_edit_section() {
        let content = "\
#command
j  forw-line
#line-edit
\\t  forw-complete
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 1);
    }

    #[test]
    fn test_parse_lesskey_source_ignores_env_section() {
        let content = "\
#command
j  forw-line
#env
LESS = -R
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 1);
    }

    #[test]
    fn test_parse_lesskey_source_resumes_command_section() {
        let content = "\
#command
j  forw-line
#env
LESS = -R
#command
k  back-line
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 2);
    }

    #[test]
    fn test_parse_lesskey_source_comments_and_blank_lines() {
        let content = "\
# This is a comment
#command

j  forw-line

# Another comment
k  back-line
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 2);
    }

    #[test]
    fn test_parse_lesskey_source_empty_content() {
        let config = parse_lesskey_source("");
        assert!(config.command_bindings.is_empty());
    }

    #[test]
    fn test_parse_lesskey_source_only_comments() {
        let content = "# just a comment\n# another comment\n";
        let config = parse_lesskey_source(content);
        assert!(config.command_bindings.is_empty());
    }

    #[test]
    fn test_parse_lesskey_source_esc_bindings() {
        let content = "\
#command
\\ek  next-file
\\e<  goto-line
\\e>  goto-end
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 3);
        assert_eq!(config.command_bindings[0].key, Key::EscSeq('k'));
        assert_eq!(config.command_bindings[0].command, Command::NextFile);
        assert_eq!(config.command_bindings[1].key, Key::EscSeq('<'));
        assert_eq!(
            config.command_bindings[1].command,
            Command::GotoBeginning(None)
        );
        assert_eq!(config.command_bindings[2].key, Key::EscSeq('>'));
        assert_eq!(config.command_bindings[2].command, Command::GotoEnd(None));
    }

    #[test]
    fn test_parse_lesskey_source_ctrl_bindings() {
        let content = "\
#command
^F  forw-screen
^B  back-screen
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 2);
        assert_eq!(config.command_bindings[0].key, Key::Ctrl('f'));
        assert_eq!(config.command_bindings[0].command, Command::PageForward);
        assert_eq!(config.command_bindings[1].key, Key::Ctrl('b'));
        assert_eq!(config.command_bindings[1].command, Command::PageBackward);
    }

    #[test]
    fn test_parse_lesskey_source_hex_bindings() {
        let content = "\
#command
\\x71  quit
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 1);
        // 0x71 = 'q'
        assert_eq!(config.command_bindings[0].key, Key::Char('q'));
        assert_eq!(config.command_bindings[0].command, Command::Quit);
    }

    #[test]
    fn test_parse_lesskey_source_backslash_n_binding() {
        let content = "\
#command
\\n  forw-line
";
        let config = parse_lesskey_source(content);
        assert_eq!(config.command_bindings.len(), 1);
        assert_eq!(config.command_bindings[0].key, Key::Enter);
        assert_eq!(
            config.command_bindings[0].command,
            Command::ScrollForward(1)
        );
    }

    // ── parse_lesskey_file ──

    #[test]
    fn test_parse_lesskey_file_missing_returns_none() {
        let result = parse_lesskey_file(Path::new("/nonexistent/path/lesskey"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_parse_lesskey_file_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lesskey");
        std::fs::write(&path, "#command\nj  forw-line\n").unwrap();
        let result = parse_lesskey_file(&path).unwrap();
        assert!(result.is_some());
        let config = result.unwrap();
        assert_eq!(config.command_bindings.len(), 1);
        assert_eq!(config.command_bindings[0].key, Key::Char('j'));
    }

    // ── action_name_count ──

    #[test]
    fn test_action_name_count_at_least_30() {
        assert!(
            action_name_count() >= 30,
            "Expected at least 30 action names, got {}",
            action_name_count()
        );
    }

    // ── Comprehensive action name mapping tests ──

    #[test]
    fn test_action_map_forw_line() {
        let m = build_action_map();
        assert_eq!(m.get("forw-line"), Some(&Command::ScrollForward(1)));
    }

    #[test]
    fn test_action_map_back_line() {
        let m = build_action_map();
        assert_eq!(m.get("back-line"), Some(&Command::ScrollBackward(1)));
    }

    #[test]
    fn test_action_map_forw_scroll() {
        let m = build_action_map();
        assert_eq!(m.get("forw-scroll"), Some(&Command::PageForward));
    }

    #[test]
    fn test_action_map_back_scroll() {
        let m = build_action_map();
        assert_eq!(m.get("back-scroll"), Some(&Command::PageBackward));
    }

    #[test]
    fn test_action_map_forw_screen() {
        let m = build_action_map();
        assert_eq!(m.get("forw-screen"), Some(&Command::PageForward));
    }

    #[test]
    fn test_action_map_back_screen() {
        let m = build_action_map();
        assert_eq!(m.get("back-screen"), Some(&Command::PageBackward));
    }

    #[test]
    fn test_action_map_forw_window() {
        let m = build_action_map();
        assert_eq!(m.get("forw-window"), Some(&Command::WindowForward));
    }

    #[test]
    fn test_action_map_back_window() {
        let m = build_action_map();
        assert_eq!(m.get("back-window"), Some(&Command::WindowBackward));
    }

    #[test]
    fn test_action_map_forw_half_screen() {
        let m = build_action_map();
        assert_eq!(m.get("forw-half-screen"), Some(&Command::HalfPageForward));
    }

    #[test]
    fn test_action_map_back_half_screen() {
        let m = build_action_map();
        assert_eq!(m.get("back-half-screen"), Some(&Command::HalfPageBackward));
    }

    #[test]
    fn test_action_map_goto_line() {
        let m = build_action_map();
        assert_eq!(m.get("goto-line"), Some(&Command::GotoBeginning(None)));
    }

    #[test]
    fn test_action_map_goto_end() {
        let m = build_action_map();
        assert_eq!(m.get("goto-end"), Some(&Command::GotoEnd(None)));
    }

    #[test]
    fn test_action_map_percent() {
        let m = build_action_map();
        assert_eq!(m.get("percent"), Some(&Command::GotoPercent));
    }

    #[test]
    fn test_action_map_quit() {
        let m = build_action_map();
        assert_eq!(m.get("quit"), Some(&Command::Quit));
    }

    #[test]
    fn test_action_map_forw_search() {
        let m = build_action_map();
        assert_eq!(m.get("forw-search"), Some(&Command::SearchForward));
    }

    #[test]
    fn test_action_map_back_search() {
        let m = build_action_map();
        assert_eq!(m.get("back-search"), Some(&Command::SearchBackward));
    }

    #[test]
    fn test_action_map_repeat_search() {
        let m = build_action_map();
        assert_eq!(m.get("repeat-search"), Some(&Command::RepeatSearch));
    }

    #[test]
    fn test_action_map_reverse_search() {
        let m = build_action_map();
        assert_eq!(m.get("reverse-search"), Some(&Command::RepeatSearchReverse));
    }

    #[test]
    fn test_action_map_help() {
        let m = build_action_map();
        assert_eq!(m.get("help"), Some(&Command::Help));
    }

    #[test]
    fn test_action_map_status() {
        let m = build_action_map();
        assert_eq!(m.get("status"), Some(&Command::FileInfo));
    }

    #[test]
    fn test_action_map_version() {
        let m = build_action_map();
        assert_eq!(m.get("version"), Some(&Command::Version));
    }

    #[test]
    fn test_action_map_next_file() {
        let m = build_action_map();
        assert_eq!(m.get("next-file"), Some(&Command::NextFile));
    }

    #[test]
    fn test_action_map_prev_file() {
        let m = build_action_map();
        assert_eq!(m.get("prev-file"), Some(&Command::PreviousFile));
    }

    #[test]
    fn test_action_map_examine() {
        let m = build_action_map();
        assert_eq!(m.get("examine"), Some(&Command::Examine));
    }

    #[test]
    fn test_action_map_filter() {
        let m = build_action_map();
        assert_eq!(m.get("filter"), Some(&Command::Filter));
    }

    #[test]
    fn test_action_map_shell() {
        let m = build_action_map();
        assert_eq!(m.get("shell"), Some(&Command::ShellCommand));
    }

    #[test]
    fn test_action_map_pipe() {
        let m = build_action_map();
        assert_eq!(m.get("pipe"), Some(&Command::PipeToCommand));
    }

    #[test]
    fn test_action_map_visual() {
        let m = build_action_map();
        assert_eq!(m.get("visual"), Some(&Command::EditFile));
    }

    #[test]
    fn test_action_map_toggle_option() {
        let m = build_action_map();
        assert_eq!(m.get("toggle-option"), Some(&Command::ToggleOption));
    }

    #[test]
    fn test_action_map_repaint() {
        let m = build_action_map();
        assert_eq!(m.get("repaint"), Some(&Command::Repaint));
    }

    #[test]
    fn test_action_map_forw_forever() {
        let m = build_action_map();
        assert_eq!(m.get("forw-forever"), Some(&Command::FollowMode));
    }

    #[test]
    fn test_action_map_forw_until_hilite() {
        let m = build_action_map();
        assert_eq!(
            m.get("forw-until-hilite"),
            Some(&Command::FollowModeStopOnMatch)
        );
    }

    #[test]
    fn test_action_map_forw_screen_force() {
        let m = build_action_map();
        assert_eq!(m.get("forw-screen-force"), Some(&Command::ForwardForceEof));
    }

    #[test]
    fn test_action_map_back_screen_force() {
        let m = build_action_map();
        assert_eq!(
            m.get("back-screen-force"),
            Some(&Command::BackwardForceBeginning)
        );
    }

    #[test]
    fn test_action_map_goto_byte() {
        let m = build_action_map();
        assert_eq!(m.get("goto-byte"), Some(&Command::GotoByteOffset));
    }

    #[test]
    fn test_action_map_undo_hilite() {
        let m = build_action_map();
        assert_eq!(m.get("undo-hilite"), Some(&Command::ToggleHighlight));
    }

    #[test]
    fn test_action_map_right_scroll() {
        let m = build_action_map();
        assert_eq!(m.get("right-scroll"), Some(&Command::ScrollRight));
    }

    #[test]
    fn test_action_map_left_scroll() {
        let m = build_action_map();
        assert_eq!(m.get("left-scroll"), Some(&Command::ScrollLeft));
    }

    #[test]
    fn test_action_map_noaction() {
        let m = build_action_map();
        assert_eq!(m.get("noaction"), Some(&Command::Noop));
    }

    // ── Integration: Keymap::apply_lesskey ──

    #[test]
    fn test_apply_lesskey_overrides_default_binding() {
        use crate::keymap::Keymap;

        let mut keymap = Keymap::default_less();
        // By default 'j' is ScrollForward(1)
        assert_eq!(keymap.lookup(&Key::Char('j')), Command::ScrollForward(1));

        // Override 'j' to quit
        let config = parse_lesskey_source("#command\nj  quit\n");
        keymap.apply_lesskey(&config);

        assert_eq!(keymap.lookup(&Key::Char('j')), Command::Quit);
    }

    #[test]
    fn test_apply_lesskey_adds_new_binding() {
        use crate::keymap::Keymap;

        let mut keymap = Keymap::default_less();
        // 'x' is unbound by default
        assert_eq!(keymap.lookup(&Key::Char('x')), Command::Noop);

        let config = parse_lesskey_source("#command\nx  quit\n");
        keymap.apply_lesskey(&config);

        assert_eq!(keymap.lookup(&Key::Char('x')), Command::Quit);
    }

    #[test]
    fn test_apply_lesskey_preserves_unaffected_bindings() {
        use crate::keymap::Keymap;

        let mut keymap = Keymap::default_less();
        let config = parse_lesskey_source("#command\nx  quit\n");
        keymap.apply_lesskey(&config);

        // Original 'q' binding still works
        assert_eq!(keymap.lookup(&Key::Char('q')), Command::Quit);
        // Original 'j' binding still works
        assert_eq!(keymap.lookup(&Key::Char('j')), Command::ScrollForward(1));
    }

    #[test]
    fn test_apply_lesskey_empty_config_changes_nothing() {
        use crate::keymap::Keymap;

        let mut keymap = Keymap::default_less();
        let config = LesskeyConfig::default();
        keymap.apply_lesskey(&config);

        assert_eq!(keymap.lookup(&Key::Char('q')), Command::Quit);
        assert_eq!(keymap.lookup(&Key::Char('j')), Command::ScrollForward(1));
    }

    #[test]
    fn test_apply_lesskey_multiple_overrides() {
        use crate::keymap::Keymap;

        let mut keymap = Keymap::default_less();
        let content = "\
#command
j  back-line
k  forw-line
";
        let config = parse_lesskey_source(content);
        keymap.apply_lesskey(&config);

        // j and k are now swapped
        assert_eq!(keymap.lookup(&Key::Char('j')), Command::ScrollBackward(1));
        assert_eq!(keymap.lookup(&Key::Char('k')), Command::ScrollForward(1));
    }
}
