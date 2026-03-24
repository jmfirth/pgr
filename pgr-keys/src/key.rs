//! Key representation for terminal input events.

/// A parsed key event from terminal input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// A regular printable character (including multi-byte UTF-8).
    Char(char),
    /// A control key combination: Ctrl+A through Ctrl+Z. The char is always lowercase.
    Ctrl(char),
    /// Standalone Escape key (no following sequence).
    Escape,
    /// Escape followed by a single character (Alt/Meta key in some terminals).
    EscSeq(char),
    /// Up arrow.
    Up,
    /// Down arrow.
    Down,
    /// Left arrow.
    Left,
    /// Right arrow.
    Right,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page Up key.
    PageUp,
    /// Page Down key.
    PageDown,
    /// Backspace key.
    Backspace,
    /// Delete key.
    Delete,
    /// Tab key.
    Tab,
    /// Enter/Return key.
    Enter,
    /// Ctrl+Left arrow.
    CtrlLeft,
    /// Ctrl+Right arrow.
    CtrlRight,
    /// Mouse scroll wheel up.
    ScrollUp,
    /// Mouse scroll wheel down.
    ScrollDown,
    /// An unrecognized byte sequence.
    Unknown(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_char_equality_same_char_returns_true() {
        assert_eq!(Key::Char('a'), Key::Char('a'));
    }

    #[test]
    fn test_key_char_equality_different_char_returns_false() {
        assert_ne!(Key::Char('a'), Key::Char('b'));
    }

    #[test]
    fn test_key_ctrl_equality_same_char_returns_true() {
        assert_eq!(Key::Ctrl('a'), Key::Ctrl('a'));
    }

    #[test]
    fn test_key_clone_produces_equal_value() {
        let key = Key::Unknown(vec![0x1b, 0x5b, 0x99]);
        assert_eq!(key.clone(), key);
    }

    #[test]
    fn test_key_scroll_up_equality() {
        assert_eq!(Key::ScrollUp, Key::ScrollUp);
    }

    #[test]
    fn test_key_scroll_down_equality() {
        assert_eq!(Key::ScrollDown, Key::ScrollDown);
    }

    #[test]
    fn test_key_scroll_up_differs_from_scroll_down() {
        assert_ne!(Key::ScrollUp, Key::ScrollDown);
    }

    #[test]
    fn test_key_debug_format_is_nonempty() {
        let debug = format!("{:?}", Key::Up);
        assert!(!debug.is_empty());
    }
}
