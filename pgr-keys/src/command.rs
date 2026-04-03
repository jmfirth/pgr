//! Command types representing pager actions triggered by key bindings.

/// A pager command that can be executed in response to user input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Scroll forward (down) by the given number of lines.
    ScrollForward(usize),
    /// Scroll backward (up) by the given number of lines.
    ScrollBackward(usize),
    /// Move forward one full page (window size).
    PageForward,
    /// Move backward one full page (window size).
    PageBackward,
    /// Move forward half a page.
    HalfPageForward,
    /// Move backward half a page.
    HalfPageBackward,
    /// Go to the beginning of the file, optionally to a specific line number.
    GotoBeginning(Option<usize>),
    /// Go to the end of the file, optionally to a specific line number.
    GotoEnd(Option<usize>),
    /// Repaint the screen.
    Repaint,
    /// Quit the pager.
    Quit,
    /// No operation; the key has no binding.
    Noop,
    /// Scroll right N characters (default: half screen width).
    /// ESC-) or RIGHT arrow.
    ScrollRight,
    /// Scroll left N characters (default: half screen width).
    /// ESC-( or LEFT arrow.
    ScrollLeft,
    /// Scroll right to end of longest displayed line. ESC-} or Ctrl+RIGHT.
    ScrollRightEnd,
    /// Scroll left to first column. ESC-{ or Ctrl+LEFT.
    ScrollLeftHome,
    /// Go to N percent through file. `p` or `%`.
    GotoPercent,
    /// Go to byte offset N. `P`.
    GotoByteOffset,
    /// Like `PageForward` but works even at EOF. ESC-SPACE.
    ForwardForceEof,
    /// Like `PageBackward` but works even at beginning. ESC-b.
    BackwardForceBeginning,
    /// Set window size to N and scroll forward. `z`.
    WindowForward,
    /// Set window size to N and scroll backward. `w`.
    WindowBackward,
    /// Enter follow mode (tail -f). `F`.
    FollowMode,
    /// Enter follow mode that stops when a search pattern matches new data. `ESC-F`.
    FollowModeStopOnMatch,
    /// Repaint with buffer refresh (reload). `R`.
    RepaintRefresh,
    /// Scroll forward N file lines (ignoring long wrapped lines). ESC-j.
    FileLineForward,
    /// Scroll backward N file lines. ESC-k.
    FileLineBackward,
    /// Like `ScrollForward` but works beyond EOF. `J`.
    ScrollForwardForce(usize),
    /// Like `ScrollBackward` but works beyond beginning. `K` or `Y`.
    ScrollBackwardForce(usize),
    /// Switch to the next file in the file list. `:n`.
    NextFile,
    /// Switch to the previous file in the file list. `:p`.
    PreviousFile,
    /// Switch to the first file (or N-th with numeric prefix). `:x`.
    FirstFile,
    /// Remove the current file from the list. `:d`.
    RemoveFile,
    /// Toggle or set an option at runtime (`-` prefix).
    ToggleOption,
    /// Display current option value (`_` prefix).
    QueryOption,
    /// Enter filter mode: prompt for pattern (`&` command).
    Filter,
    /// Execute a shell command (`!command`).
    ShellCommand,
    /// Execute a shell command with prompt-style expansion (`#command`).
    ShellCommandExpand,
    /// Pipe lines from a mark to the current screen position to a command (`|mark command`).
    PipeToCommand,
    /// Open the current file in the editor (`$VISUAL` or `$EDITOR`).
    EditFile,
    /// Save pipe input to a file (`s filename`).
    SavePipeInput,
    /// Examine (open) a new file. `:e [filename]`.
    Examine,
    /// Same as Examine (alternative bindings: `^X^V`, `E`).
    ExamineAlt,
    /// Display file information (`=`, `^G`, `:f`).
    FileInfo,
    /// Display the help screen (`h`, `H`).
    Help,
    /// Display version information (`V`).
    Version,
    /// Enter forward search mode: prompt for pattern, search forward.
    SearchForward,
    /// Enter backward search mode: prompt for pattern, search backward.
    SearchBackward,
    /// Repeat last search in the same direction.
    RepeatSearch,
    /// Repeat last search in the opposite direction.
    RepeatSearchReverse,
    /// Toggle search highlighting (ESC-u).
    ToggleHighlight,
    /// Find matching close bracket by searching forward from top line.
    /// The tuple fields are `(open_char, close_char)`, e.g., `('{', '}')`.
    FindCloseBracket(char, char),
    /// Find matching open bracket by searching backward from bottom line.
    /// The tuple fields are `(open_char, close_char)`, e.g., `('{', '}')`.
    FindOpenBracket(char, char),
    /// Repeat last search forward, crossing file boundaries. ESC-n.
    SearchNextCrossFile,
    /// Repeat last search backward, crossing file boundaries. ESC-N.
    SearchPrevCrossFile,
    /// Enter forward search mode with cross-file behavior. ESC-/.
    SearchForwardCrossFile,
    /// Enter backward search mode with cross-file behavior. ESC-?.
    SearchBackwardCrossFile,
    /// Go to the next tag match in the tag list. `t`.
    NextTag,
    /// Go to the previous tag match in the tag list. `T`.
    PrevTag,
    /// Clear search highlighting and the saved pattern. ESC-U.
    ClearSearchPattern,
    /// Navigate to the next hyperlink on screen. `^O^N`.
    HyperlinkNext,
    /// Navigate to the previous hyperlink on screen. `^O^P`.
    HyperlinkPrev,
    /// Jump to a hyperlink by number. `^O^L`.
    HyperlinkJump,
    /// Open the current hyperlink's URI in a browser. `^O^O`.
    HyperlinkOpen,
    /// Toggle syntax highlighting on/off. `ESC-S`.
    ToggleSyntax,
    /// Add an extra highlight pattern with a distinct color. `&+`.
    AddHighlight,
    /// Remove an extra highlight pattern by name. `&-`.
    RemoveHighlight,
    /// List all active highlight patterns. `&l`.
    ListHighlights,
    /// Yank (copy) the current top-of-screen line to the clipboard. `y`.
    YankLine,
    /// Yank (copy) all visible lines to the clipboard. `Y`.
    YankScreen,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_clone_produces_equal_value() {
        let cmd = Command::ScrollForward(5);
        assert_eq!(cmd.clone(), cmd);
    }

    #[test]
    fn test_command_debug_format_is_nonempty() {
        let debug = format!("{:?}", Command::Quit);
        assert!(!debug.is_empty());
    }

    #[test]
    fn test_command_equality_same_variant_returns_true() {
        assert_eq!(Command::PageForward, Command::PageForward);
    }

    #[test]
    fn test_command_equality_different_variant_returns_false() {
        assert_ne!(Command::PageForward, Command::PageBackward);
    }

    #[test]
    fn test_command_goto_beginning_none_equals_none() {
        assert_eq!(Command::GotoBeginning(None), Command::GotoBeginning(None));
    }

    #[test]
    fn test_command_goto_beginning_some_differs_from_none() {
        assert_ne!(
            Command::GotoBeginning(Some(1)),
            Command::GotoBeginning(None)
        );
    }

    #[test]
    fn test_command_find_close_bracket_equality() {
        assert_eq!(
            Command::FindCloseBracket('{', '}'),
            Command::FindCloseBracket('{', '}')
        );
    }

    #[test]
    fn test_command_find_open_bracket_equality() {
        assert_eq!(
            Command::FindOpenBracket('{', '}'),
            Command::FindOpenBracket('{', '}')
        );
    }

    #[test]
    fn test_command_find_bracket_different_types_not_equal() {
        assert_ne!(
            Command::FindCloseBracket('{', '}'),
            Command::FindOpenBracket('{', '}')
        );
    }

    #[test]
    fn test_command_next_tag_equality() {
        assert_eq!(Command::NextTag, Command::NextTag);
    }

    #[test]
    fn test_command_prev_tag_equality() {
        assert_eq!(Command::PrevTag, Command::PrevTag);
    }

    #[test]
    fn test_command_next_tag_differs_from_prev_tag() {
        assert_ne!(Command::NextTag, Command::PrevTag);
    }

    #[test]
    fn test_command_follow_mode_stop_on_match_differs_from_follow_mode() {
        assert_ne!(Command::FollowMode, Command::FollowModeStopOnMatch);
    }

    #[test]
    fn test_command_follow_mode_stop_on_match_equality() {
        assert_eq!(
            Command::FollowModeStopOnMatch,
            Command::FollowModeStopOnMatch
        );
    }

    #[test]
    fn test_command_clear_search_pattern_equality() {
        assert_eq!(Command::ClearSearchPattern, Command::ClearSearchPattern);
    }

    #[test]
    fn test_command_clear_search_pattern_differs_from_toggle_highlight() {
        assert_ne!(Command::ClearSearchPattern, Command::ToggleHighlight);
    }

    #[test]
    fn test_command_hyperlink_next_equality() {
        assert_eq!(Command::HyperlinkNext, Command::HyperlinkNext);
    }

    #[test]
    fn test_command_hyperlink_prev_equality() {
        assert_eq!(Command::HyperlinkPrev, Command::HyperlinkPrev);
    }

    #[test]
    fn test_command_hyperlink_jump_equality() {
        assert_eq!(Command::HyperlinkJump, Command::HyperlinkJump);
    }

    #[test]
    fn test_command_hyperlink_open_equality() {
        assert_eq!(Command::HyperlinkOpen, Command::HyperlinkOpen);
    }

    #[test]
    fn test_command_hyperlink_next_differs_from_prev() {
        assert_ne!(Command::HyperlinkNext, Command::HyperlinkPrev);
    }

    #[test]
    fn test_command_hyperlink_jump_differs_from_open() {
        assert_ne!(Command::HyperlinkJump, Command::HyperlinkOpen);
    }

    #[test]
    fn test_command_hyperlink_next_differs_from_noop() {
        assert_ne!(Command::HyperlinkNext, Command::Noop);
    }

    // ── Task 321: Multi-pattern highlighting commands ──

    #[test]
    fn test_command_add_highlight_equality() {
        assert_eq!(Command::AddHighlight, Command::AddHighlight);
    }

    #[test]
    fn test_command_remove_highlight_equality() {
        assert_eq!(Command::RemoveHighlight, Command::RemoveHighlight);
    }

    #[test]
    fn test_command_list_highlights_equality() {
        assert_eq!(Command::ListHighlights, Command::ListHighlights);
    }

    #[test]
    fn test_command_add_highlight_differs_from_remove() {
        assert_ne!(Command::AddHighlight, Command::RemoveHighlight);
    }

    #[test]
    fn test_command_list_highlights_differs_from_filter() {
        assert_ne!(Command::ListHighlights, Command::Filter);
    }

    // ── Task 330: Clipboard yank commands ──

    #[test]
    fn test_command_yank_line_equality() {
        assert_eq!(Command::YankLine, Command::YankLine);
    }

    #[test]
    fn test_command_yank_screen_equality() {
        assert_eq!(Command::YankScreen, Command::YankScreen);
    }

    #[test]
    fn test_command_yank_line_differs_from_yank_screen() {
        assert_ne!(Command::YankLine, Command::YankScreen);
    }

    #[test]
    fn test_command_yank_line_differs_from_noop() {
        assert_ne!(Command::YankLine, Command::Noop);
    }
}
