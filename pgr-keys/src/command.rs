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
}
