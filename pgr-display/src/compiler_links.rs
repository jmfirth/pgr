//! OSC 8 hyperlink injection for compiler error output.
//!
//! Finds `file:line:col` and `file(line,col)` references in compiler output
//! lines and wraps them with OSC 8 terminal hyperlinks so they are clickable
//! in terminals that support the protocol (iTerm2, Kitty, Alacritty, ghostty,
//! Windows Terminal).

use regex::Regex;
use std::sync::OnceLock;

/// Regex for colon-separated file references: `path/file.ext:line[:col]`.
///
/// Capture groups:
/// 1. Full match prefix up to and including the location (used for replacement)
/// 2. File path (with extension)
/// 3. Line number
/// 4. Optional column number (may be empty)
fn colon_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // SAFETY: literal pattern, always valid.
        Regex::new(r"([^\s:]+\.[a-zA-Z][a-zA-Z0-9+]*)(:([0-9]+)(?::([0-9]+))?)").unwrap()
    })
}

/// Regex for paren-separated file references: `path/file.ext(line,col)`.
///
/// Capture groups:
/// 1. File path (with extension)
/// 2. Line number
/// 3. Optional column number (may be empty)
fn paren_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // SAFETY: literal pattern, always valid.
        Regex::new(r"([^\s(]+\.[a-zA-Z][a-zA-Z0-9+]*)\(([0-9]+)(?:,([0-9]+))?\)").unwrap()
    })
}

/// Wrap a file reference in an OSC 8 hyperlink.
///
/// Produces `ESC ] 8 ;; file:///<abs_path> BEL <display_text> ESC ] 8 ;; BEL`.
/// If `path` is already absolute it is used as-is; otherwise it is resolved
/// relative to the current working directory via [`std::env::current_dir`].
/// Falls back to a relative `file://` URI on any resolution error.
fn make_osc8_link(path: &str, display: &str) -> String {
    let abs = if std::path::Path::new(path).is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir().map_or_else(
            |_| path.to_owned(),
            |cwd| cwd.join(path).to_string_lossy().into_owned(),
        )
    };

    format!("\x1b]8;;file://{abs}\x07{display}\x1b]8;;\x07")
}

/// Find `file:line:col` and `file(line,col)` references in `line` and wrap
/// each one in an OSC 8 terminal hyperlink.
///
/// Lines with no file references pass through unchanged.
///
/// # Examples
///
/// ```
/// use pgr_display::linkify_compiler_output;
///
/// let out = linkify_compiler_output("src/main.rs:42:10: error");
/// assert!(out.contains("\x1b]8;;file://"));
/// assert!(out.contains("src/main.rs:42:10"));
/// ```
#[must_use]
pub fn linkify_compiler_output(line: &str) -> String {
    // We build the result by scanning for matches from both patterns and
    // applying them in left-to-right order, taking the first (leftmost) match
    // at each position to avoid overlapping replacements.

    let mut result = String::with_capacity(line.len() + 64);
    let mut pos = 0usize;

    while pos < line.len() {
        let remaining = &line[pos..];

        // Find the next match from either pattern, prefer the leftmost one.
        let colon_hit = colon_pattern().find(remaining);
        let paren_hit = paren_pattern().find(remaining);

        let best = match (colon_hit, paren_hit) {
            (None, None) => break,
            (Some(m), None) => (m.start(), m.end(), true),
            (None, Some(m)) => (m.start(), m.end(), false),
            (Some(cm), Some(pm)) => {
                if cm.start() <= pm.start() {
                    (cm.start(), cm.end(), true)
                } else {
                    (pm.start(), pm.end(), false)
                }
            }
        };

        let (rel_start, rel_end, is_colon) = best;

        // Append text before the match.
        result.push_str(&remaining[..rel_start]);

        let matched_text = &remaining[rel_start..rel_end];
        let hyperlink = if is_colon {
            build_colon_link(remaining, rel_start, rel_end)
        } else {
            build_paren_link(remaining, rel_start, rel_end)
        };

        match hyperlink {
            Some(osc8) => result.push_str(&osc8),
            None => result.push_str(matched_text),
        }

        pos += rel_end;
    }

    // Append any remaining text after the last match.
    if pos < line.len() {
        result.push_str(&line[pos..]);
    }

    result
}

/// Build an OSC 8 link for a colon-syntax match within `text`.
fn build_colon_link(text: &str, start: usize, end: usize) -> Option<String> {
    let caps = colon_pattern().captures(&text[start..end])?;
    let path = caps.get(1)?.as_str();
    let location = caps.get(2)?.as_str(); // e.g. ":42:10"
    let display = format!("{path}{location}");
    Some(make_osc8_link(path, &display))
}

/// Build an OSC 8 link for a paren-syntax match within `text`.
fn build_paren_link(text: &str, start: usize, end: usize) -> Option<String> {
    let caps = paren_pattern().captures(&text[start..end])?;
    let path = caps.get(1)?.as_str();
    let line_num = caps.get(2)?.as_str();
    let col = caps.get(3).map_or("", |m| m.as_str());
    let display = if col.is_empty() {
        format!("{path}({line_num})")
    } else {
        format!("{path}({line_num},{col})")
    };
    Some(make_osc8_link(path, &display))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linkify_compiler_output_colon_syntax_produces_osc8() {
        let out = linkify_compiler_output("src/main.rs:42:10: error");
        // Must contain the OSC 8 opener with a file:// URI
        assert!(out.contains("\x1b]8;;file://"), "expected OSC 8 in: {out}");
        // Must contain the visible display text
        assert!(
            out.contains("src/main.rs:42:10"),
            "expected display text in: {out}"
        );
        // Must contain the OSC 8 closer
        assert!(
            out.contains("\x1b]8;;\x07"),
            "expected OSC 8 closer in: {out}"
        );
    }

    #[test]
    fn test_linkify_compiler_output_no_reference_passes_through() {
        let line = "just a plain error message with no file reference";
        assert_eq!(linkify_compiler_output(line), line);
    }

    #[test]
    fn test_linkify_compiler_output_paren_syntax_produces_osc8() {
        let out = linkify_compiler_output("src/index.ts(10,5): error TS2304");
        assert!(out.contains("\x1b]8;;file://"), "expected OSC 8 in: {out}");
        assert!(
            out.contains("src/index.ts(10,5)"),
            "expected display text in: {out}"
        );
    }

    #[test]
    fn test_linkify_compiler_output_preserves_surrounding_text() {
        let out = linkify_compiler_output("src/main.rs:42:10: error: some message here");
        // Text after the link (the ": error: some message here" part) should still be present
        assert!(out.contains(": error: some message here"));
    }

    #[test]
    fn test_linkify_compiler_output_empty_line_returns_empty() {
        assert_eq!(linkify_compiler_output(""), "");
    }

    #[test]
    fn test_linkify_compiler_output_line_without_col_colon_syntax() {
        let out = linkify_compiler_output("main.py:42: SyntaxError: invalid syntax");
        assert!(out.contains("\x1b]8;;file://"), "expected OSC 8 in: {out}");
        assert!(
            out.contains("main.py:42"),
            "expected display text in: {out}"
        );
    }
}
