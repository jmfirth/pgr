//! Diff-aware line coloring and per-hunk syntax highlighting.
//!
//! Strips git's basic ANSI coloring from diff output and replaces it with
//! richer rendering: background tinting for added/removed lines and
//! syntax-highlighted foreground colors within hunks.

use crate::ansi::strip_ansi;
use pgr_core::DiffLineType;

/// Light green background for added lines (24-bit true color).
const ADDED_BG: &str = "\x1b[48;2;30;60;30m";
/// Light red background for removed lines (24-bit true color).
const REMOVED_BG: &str = "\x1b[48;2;60;30;30m";
/// Cyan foreground + dim for hunk headers.
const HUNK_HEADER_SGR: &str = "\x1b[36;2m";
/// Bold for file-level headers.
const FILE_HEADER_SGR: &str = "\x1b[1m";
/// Reset all attributes.
const RESET: &str = "\x1b[0m";

/// Apply diff-aware coloring to a single line based on its classification.
///
/// Strips any existing ANSI escape sequences (e.g., git's red/green) and
/// re-applies richer coloring with background tinting for added/removed lines,
/// cyan dim for hunk headers, and bold for file headers. Context lines and
/// other lines pass through with ANSI stripped.
#[must_use]
pub fn colorize_diff_line(line: &str, line_type: DiffLineType) -> String {
    let clean = strip_ansi(line);
    match line_type {
        DiffLineType::Added => format!("{ADDED_BG}{clean}{RESET}"),
        DiffLineType::Removed => format!("{REMOVED_BG}{clean}{RESET}"),
        DiffLineType::HunkHeader => format!("{HUNK_HEADER_SGR}{clean}{RESET}"),
        DiffLineType::Header => format!("{FILE_HEADER_SGR}{clean}{RESET}"),
        DiffLineType::Context | DiffLineType::Other => clean,
    }
}

/// Apply background tinting to already-stripped content (no `+`/`-` prefix).
///
/// Used by side-by-side rendering where prefixes have already been removed
/// during line pairing. Applies the same subtle 24-bit background tints
/// as the unified view.
#[must_use]
pub fn tint_content(text: &str, line_type: DiffLineType) -> String {
    match line_type {
        DiffLineType::Added => format!("{ADDED_BG}{text}{RESET}"),
        DiffLineType::Removed => format!("{REMOVED_BG}{text}{RESET}"),
        DiffLineType::HunkHeader => format!("{HUNK_HEADER_SGR}{text}{RESET}"),
        DiffLineType::Header => format!("{FILE_HEADER_SGR}{text}{RESET}"),
        DiffLineType::Context | DiffLineType::Other => text.to_string(),
    }
}

/// Apply syntax highlighting + background tinting to already-stripped content.
///
/// Like [`tint_content`] but also applies syntect foreground colors when the
/// `syntax` feature is enabled and a syntax is detected for the filename.
#[cfg(feature = "syntax")]
#[must_use]
pub fn highlight_content(
    text: &str,
    line_type: DiffLineType,
    highlighter: &crate::syntax::highlighting::Highlighter,
    hl: &mut syntect::easy::HighlightLines<'_>,
) -> String {
    use crate::syntax::highlighting::as_24_bit_terminal_escaped;

    match line_type {
        DiffLineType::Added | DiffLineType::Removed | DiffLineType::Context => {
            let code_nl = if text.ends_with('\n') {
                text.to_string()
            } else {
                format!("{text}\n")
            };

            let syntax_colored = hl
                .highlight_line(&code_nl, highlighter.syntax_set())
                .ok()
                .map(|ranges| {
                    let escaped = as_24_bit_terminal_escaped(&ranges);
                    escaped
                        .trim_end_matches('\n')
                        .strip_suffix(RESET)
                        .unwrap_or(escaped.trim_end_matches('\n'))
                        .to_string()
                });

            let syntax_text = syntax_colored.as_deref().unwrap_or(text);

            match line_type {
                DiffLineType::Added => format!("{ADDED_BG}{syntax_text}{RESET}"),
                DiffLineType::Removed => format!("{REMOVED_BG}{syntax_text}{RESET}"),
                DiffLineType::Context => format!("{syntax_text}{RESET}"),
                _ => unreachable!(),
            }
        }
        _ => tint_content(text, line_type),
    }
}

/// Highlight code within diff hunks using the detected file's syntax.
///
/// For each line, strips git's ANSI coloring, applies syntax highlighting
/// (foreground colors from syntect), then overlays the diff background tint.
/// Context and other non-code lines get diff coloring without syntax highlighting.
///
/// The highlighter state is reset at the start of each call (fresh parse).
/// Most hunks begin at statement boundaries, so this approximation is acceptable.
#[cfg(feature = "syntax")]
#[must_use]
pub fn highlight_diff_hunk(
    lines: &[&str],
    line_types: &[DiffLineType],
    highlighter: &crate::syntax::highlighting::Highlighter,
    filename: &str,
) -> Vec<String> {
    use crate::syntax::highlighting::as_24_bit_terminal_escaped;

    let Some(syntax) = highlighter.detect_syntax(filename) else {
        // No recognized syntax — fall back to diff coloring only.
        return lines
            .iter()
            .zip(line_types.iter())
            .map(|(line, lt)| colorize_diff_line(line, *lt))
            .collect();
    };

    let mut hl = highlighter.highlight_lines(syntax);

    lines
        .iter()
        .zip(line_types.iter())
        .map(|(line, lt)| {
            let clean = strip_ansi(line);

            match lt {
                DiffLineType::Added | DiffLineType::Removed | DiffLineType::Context => {
                    // Strip the diff prefix character ('+', '-', or ' ') for highlighting,
                    // then reattach it with the appropriate background.
                    let (prefix, code) = if clean.is_empty() {
                        ("", clean.as_str())
                    } else {
                        clean.split_at(1)
                    };

                    // Highlight the code portion.
                    let code_nl = if code.ends_with('\n') {
                        code.to_string()
                    } else {
                        format!("{code}\n")
                    };

                    let syntax_colored = hl
                        .highlight_line(&code_nl, highlighter.syntax_set())
                        .ok()
                        .map(|ranges| {
                            let escaped = as_24_bit_terminal_escaped(&ranges);
                            // Remove trailing newline and the trailing reset from
                            // as_24_bit_terminal_escaped so we can wrap with background.
                            escaped
                                .trim_end_matches('\n')
                                .strip_suffix(RESET)
                                .unwrap_or(escaped.trim_end_matches('\n'))
                                .to_string()
                        });

                    let syntax_text = syntax_colored.as_deref().unwrap_or(code);

                    match lt {
                        DiffLineType::Added => {
                            format!("{ADDED_BG}{prefix}{syntax_text}{RESET}")
                        }
                        DiffLineType::Removed => {
                            format!("{REMOVED_BG}{prefix}{syntax_text}{RESET}")
                        }
                        DiffLineType::Context => {
                            // Context lines: syntax foreground, no background tint.
                            format!("{prefix}{syntax_text}{RESET}")
                        }
                        _ => unreachable!(),
                    }
                }
                DiffLineType::HunkHeader => {
                    // Feed a blank line through the highlighter to keep state
                    // somewhat aligned, but render with hunk header styling.
                    let _ = hl.highlight_line("\n", highlighter.syntax_set());
                    format!("{HUNK_HEADER_SGR}{clean}{RESET}")
                }
                DiffLineType::Header => {
                    format!("{FILE_HEADER_SGR}{clean}{RESET}")
                }
                DiffLineType::Other => clean,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pgr_core::{classify_diff_line, DiffLineType};

    #[test]
    fn test_classify_added_line_returns_added() {
        assert_eq!(classify_diff_line("+foo"), DiffLineType::Added);
    }

    #[test]
    fn test_classify_removed_line_returns_removed() {
        assert_eq!(classify_diff_line("-foo"), DiffLineType::Removed);
    }

    #[test]
    fn test_classify_context_line_returns_context() {
        assert_eq!(classify_diff_line(" foo"), DiffLineType::Context);
    }

    #[test]
    fn test_colorize_added_line_contains_green_background() {
        let result = colorize_diff_line("+added line", DiffLineType::Added);
        assert!(
            result.contains("\x1b[48;2;30;60;30m"),
            "expected green background in: {result:?}"
        );
    }

    #[test]
    fn test_colorize_removed_line_contains_red_background() {
        let result = colorize_diff_line("-removed line", DiffLineType::Removed);
        assert!(
            result.contains("\x1b[48;2;60;30;30m"),
            "expected red background in: {result:?}"
        );
    }

    #[test]
    fn test_colorize_hunk_header_contains_cyan() {
        let result = colorize_diff_line("@@ -1,3 +1,4 @@", DiffLineType::HunkHeader);
        assert!(
            result.contains("\x1b[36;2m"),
            "expected cyan+dim in: {result:?}"
        );
    }

    #[test]
    fn test_colorize_file_header_contains_bold() {
        let result = colorize_diff_line("diff --git a/foo b/foo", DiffLineType::Header);
        assert!(result.contains("\x1b[1m"), "expected bold in: {result:?}");
    }

    #[test]
    fn test_colorize_context_line_no_background() {
        let result = colorize_diff_line(" context line", DiffLineType::Context);
        // Context lines should have no background escape.
        assert!(!result.contains("\x1b[48;2;"));
    }

    #[test]
    fn test_strip_git_color_from_added_line() {
        let git_colored = "\x1b[32m+added line\x1b[0m";
        let result = colorize_diff_line(git_colored, DiffLineType::Added);
        // Should not contain git's green foreground; should contain our green background.
        assert!(!result.contains("\x1b[32m"));
        assert!(result.contains("\x1b[48;2;30;60;30m"));
        assert!(result.contains("+added line"));
    }

    #[test]
    fn test_strip_git_color_from_removed_line() {
        let git_colored = "\x1b[31m-removed line\x1b[0m";
        let result = colorize_diff_line(git_colored, DiffLineType::Removed);
        assert!(!result.contains("\x1b[31m"));
        assert!(result.contains("\x1b[48;2;60;30;30m"));
        assert!(result.contains("-removed line"));
    }

    #[cfg(feature = "syntax")]
    #[test]
    fn test_highlight_diff_hunk_produces_sgr_for_rust() {
        let highlighter = crate::syntax::highlighting::Highlighter::new();
        let lines = [" fn main() {", "+    println!(\"hello\");", " }"];
        let types = [
            DiffLineType::Context,
            DiffLineType::Added,
            DiffLineType::Context,
        ];
        let result = highlight_diff_hunk(&lines, &types, &highlighter, "test.rs");
        assert_eq!(result.len(), 3);
        // The added line should contain both the green background and foreground SGR.
        assert!(
            result[1].contains("\x1b[48;2;30;60;30m"),
            "expected green bg in added line: {:?}",
            result[1]
        );
        assert!(
            result[1].contains("\x1b["),
            "expected SGR in highlighted line"
        );
    }

    #[cfg(feature = "syntax")]
    #[test]
    fn test_highlight_diff_hunk_unknown_syntax_falls_back_to_colorize() {
        let highlighter = crate::syntax::highlighting::Highlighter::new();
        let lines = ["+added line"];
        let types = [DiffLineType::Added];
        let result = highlight_diff_hunk(&lines, &types, &highlighter, "unknown.xyz123");
        assert_eq!(result.len(), 1);
        // Should still have the green background from colorize_diff_line.
        assert!(result[0].contains("\x1b[48;2;30;60;30m"));
    }

    #[test]
    fn test_colorize_other_line_passes_through() {
        let result = colorize_diff_line("\\ No newline at end of file", DiffLineType::Other);
        assert_eq!(result, "\\ No newline at end of file");
    }

    #[test]
    fn test_colorize_empty_line() {
        let result = colorize_diff_line("", DiffLineType::Context);
        assert_eq!(result, "");
    }
}
