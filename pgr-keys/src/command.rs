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
