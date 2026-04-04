//! Diff-aware line coloring and per-hunk syntax highlighting.
//!
//! Strips git's basic ANSI coloring from diff output and replaces it with
//! richer rendering: background tinting for added/removed lines and
//! syntax-highlighted foreground colors within hunks.

use crate::ansi::strip_ansi;
use pgr_core::DiffLineType;

/// Green-tinted background for added lines (24-bit true color).
/// Clearly green on dark terminals while keeping syntax colors readable.
const ADDED_BG: &str = "\x1b[48;2;20;60;20m";
/// Red-tinted background for removed lines (24-bit true color).
const REMOVED_BG: &str = "\x1b[48;2;60;20;20m";
/// Intense green background for word-level emphasis on added lines.
const ADDED_EMPHASIS_BG: &str = "\x1b[48;2;40;120;40m";
/// Intense red background for word-level emphasis on removed lines.
const REMOVED_EMPHASIS_BG: &str = "\x1b[48;2;120;40;40m";
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

/// Which side of a diff pair a line belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffSide {
    /// Old/removed side.
    Old,
    /// New/added side.
    New,
}

/// Apply word-level emphasis to diff content.
///
/// Unchanged portions get the normal dim tint (`ADDED_BG`/`REMOVED_BG`),
/// while changed byte spans get an intense highlight. The `changes` are
/// byte ranges from [`pgr_core::word_diff::compute_word_diff`].
///
/// `side` determines which span offsets (`old_*` or `new_*`) and which
/// colors to use.
#[must_use]
pub fn apply_word_emphasis(
    text: &str,
    side: DiffSide,
    changes: &[pgr_core::word_diff::WordChange],
) -> String {
    if changes.is_empty() {
        // No word-level data — fall back to whole-line tinting.
        let lt = match side {
            DiffSide::Old => DiffLineType::Removed,
            DiffSide::New => DiffLineType::Added,
        };
        return tint_content(text, lt);
    }

    let (dim_bg, emphasis_bg) = match side {
        DiffSide::Old => (REMOVED_BG, REMOVED_EMPHASIS_BG),
        DiffSide::New => (ADDED_BG, ADDED_EMPHASIS_BG),
    };

    let mut out = String::with_capacity(text.len() + changes.len() * 30);
    let mut pos = 0;

    for change in changes {
        let (start, end) = match side {
            DiffSide::Old => (change.old_start, change.old_end),
            DiffSide::New => (change.new_start, change.new_end),
        };

        // Emit unchanged portion before this change (dim tint).
        if start > pos {
            out.push_str(dim_bg);
            out.push_str(&text[pos..start]);
        }

        // Emit the changed span (intense emphasis).
        if end > start {
            out.push_str(emphasis_bg);
            out.push_str(&text[start..end]);
        }

        pos = end;
    }

    // Trailing unchanged portion.
    if pos < text.len() {
        out.push_str(dim_bg);
        out.push_str(&text[pos..]);
    }

    out.push_str(RESET);
    out
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
                    // Strip RESET first, then trim the \n that was added for syntect.
                    // The \n sits between the last token and the trailing RESET, so
                    // trim_end_matches('\n') must run after strip_suffix(RESET).
                    escaped
                        .strip_suffix(RESET)
                        .unwrap_or(&escaped)
                        .trim_end_matches('\n')
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
                            // Strip RESET first, then trim the \n added for syntect.
                            escaped
                                .strip_suffix(RESET)
                                .unwrap_or(&escaped)
                                .trim_end_matches('\n')
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
            result.contains("\x1b[48;2;20;60;20m"),
            "expected green background in: {result:?}"
        );
    }

    #[test]
    fn test_colorize_removed_line_contains_red_background() {
        let result = colorize_diff_line("-removed line", DiffLineType::Removed);
        assert!(
            result.contains("\x1b[48;2;60;20;20m"),
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
        assert!(result.contains("\x1b[48;2;20;60;20m"));
        assert!(result.contains("+added line"));
    }

    #[test]
    fn test_strip_git_color_from_removed_line() {
        let git_colored = "\x1b[31m-removed line\x1b[0m";
        let result = colorize_diff_line(git_colored, DiffLineType::Removed);
        assert!(!result.contains("\x1b[31m"));
        assert!(result.contains("\x1b[48;2;60;20;20m"));
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
            result[1].contains("\x1b[48;2;20;60;20m"),
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
        assert!(result[0].contains("\x1b[48;2;20;60;20m"));
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

    /// Regression: highlight_content must not leave a trailing `\n` in its output.
    ///
    /// syntect requires `\n`-terminated input, but the added `\n` must be stripped
    /// before returning. A stale `\n` causes `truncate_to_width_ansi` to overcount
    /// width by 1 (unicode-width 0.2 reports `\n` as width 1), misaligning the
    /// side-by-side separator.
    #[cfg(feature = "syntax")]
    #[test]
    fn test_highlight_content_no_trailing_newline() {
        let highlighter = crate::syntax::highlighting::Highlighter::new();
        let syntax = highlighter.detect_syntax("test.rs").unwrap();
        let mut hl = highlighter.highlight_lines(syntax);

        let text = "let x = 42;";
        let result = highlight_content(text, DiffLineType::Added, &highlighter, &mut hl);

        assert!(
            !result.contains('\n'),
            "highlight_content must strip trailing newline, got: {result:?}"
        );
    }

    /// Regression: highlight_content separator alignment through full render path.
    ///
    /// When highlight_content output is fed through side-by-side rendering,
    /// the separator column must be identical across all lines — headers,
    /// hunk headers, and syntax-highlighted code lines.
    #[cfg(feature = "syntax")]
    #[test]
    fn test_highlight_content_sbs_separator_aligned() {
        use crate::ansi::strip_ansi;
        use crate::side_by_side::{render_side_by_side, SideBySideLayout, SideBySideLine};

        let highlighter = crate::syntax::highlighting::Highlighter::new();
        let syntax = highlighter.detect_syntax("test.rs").unwrap();
        let mut hl_left = highlighter.highlight_lines(syntax);
        let mut hl_right = highlighter.highlight_lines(syntax);

        let layout = SideBySideLayout::from_terminal_width(100).unwrap();

        let mut lines = vec![
            SideBySideLine {
                left: Some("a/foo.rs".to_string()),
                right: Some("b/foo.rs".to_string()),
                left_type: DiffLineType::Header,
                right_type: DiffLineType::Header,
            },
            SideBySideLine {
                left: Some("@@ -1,3 +1,3 @@".to_string()),
                right: Some("@@ -1,3 +1,3 @@".to_string()),
                left_type: DiffLineType::HunkHeader,
                right_type: DiffLineType::HunkHeader,
            },
            SideBySideLine {
                left: Some("fn main() {".to_string()),
                right: Some("fn main() {".to_string()),
                left_type: DiffLineType::Context,
                right_type: DiffLineType::Context,
            },
            SideBySideLine {
                left: Some("    let x = 1;".to_string()),
                right: Some("    let x = 2;".to_string()),
                left_type: DiffLineType::Removed,
                right_type: DiffLineType::Added,
            },
        ];

        // Apply highlight_content (syntect path) to each side.
        for line in &mut lines {
            if let Some(ref text) = line.left {
                line.left = Some(highlight_content(
                    text,
                    line.left_type,
                    &highlighter,
                    &mut hl_left,
                ));
            }
            if let Some(ref text) = line.right {
                line.right = Some(highlight_content(
                    text,
                    line.right_type,
                    &highlighter,
                    &mut hl_right,
                ));
            }
        }

        let rendered = render_side_by_side(&lines, &layout);
        let sep_cols: Vec<usize> = rendered
            .iter()
            .filter_map(|l| {
                let stripped = strip_ansi(l);
                stripped.find('\u{2502}')
            })
            .collect();

        assert_eq!(
            sep_cols.len(),
            rendered.len(),
            "separator missing on some lines"
        );
        let first = sep_cols[0];
        for (i, &col) in sep_cols.iter().enumerate() {
            assert_eq!(
                col, first,
                "line {} separator at column {}, expected {} (line 0). Likely a stale \\n in highlight_content output.",
                i, col, first
            );
        }
    }

    #[test]
    fn test_apply_word_emphasis_changed_spans() {
        use pgr_core::word_diff::WordChange;

        let text = "hello world";
        let changes = vec![WordChange {
            old_start: 6,
            old_end: 11,
            new_start: 6,
            new_end: 11,
        }];

        let result = apply_word_emphasis(text, DiffSide::Old, &changes);
        // "hello " should get dim red, "world" should get intense red.
        assert!(
            result.contains("\x1b[48;2;60;20;20m"),
            "expected dim red bg: {result:?}"
        );
        assert!(
            result.contains("\x1b[48;2;120;40;40m"),
            "expected intense red bg: {result:?}"
        );
        assert!(result.contains("hello"), "missing 'hello': {result:?}");
        assert!(result.contains("world"), "missing 'world': {result:?}");
    }

    #[test]
    fn test_apply_word_emphasis_empty_changes_falls_back() {
        let result = apply_word_emphasis("some text", DiffSide::New, &[]);
        // Should fall back to flat tint (added green bg).
        assert!(
            result.contains("\x1b[48;2;20;60;20m"),
            "expected green bg fallback: {result:?}"
        );
    }
}
