//! Keymap: maps terminal key events to pager commands.

use crate::command::Command;
use crate::key::Key;

/// A mapping from key events to pager commands.
///
/// Bindings are stored in order; the first matching binding wins.
/// Keys with no binding resolve to [`Command::Noop`].
pub struct Keymap {
    bindings: Vec<(Key, Command)>,
}

impl Keymap {
    /// Create the default keymap matching the `less` pager's Phase 0 keybindings.
    #[must_use]
    pub fn default_less() -> Self {
        let bindings = vec![
            // Quit
            (Key::Char('q'), Command::Quit),
            (Key::Char('Q'), Command::Quit),
            (Key::Ctrl('c'), Command::Quit),
            // ScrollForward(1)
            (Key::Enter, Command::ScrollForward(1)),
            (Key::Char('e'), Command::ScrollForward(1)),
            (Key::Char('j'), Command::ScrollForward(1)),
            (Key::Ctrl('e'), Command::ScrollForward(1)),
            (Key::Ctrl('n'), Command::ScrollForward(1)),
            (Key::Down, Command::ScrollForward(1)),
            // ScrollBackward(1)
            (Key::Char('y'), Command::ScrollBackward(1)),
            (Key::Char('k'), Command::ScrollBackward(1)),
            (Key::Ctrl('y'), Command::ScrollBackward(1)),
            (Key::Ctrl('k'), Command::ScrollBackward(1)),
            (Key::Ctrl('p'), Command::ScrollBackward(1)),
            (Key::Up, Command::ScrollBackward(1)),
            // PageForward
            (Key::Char(' '), Command::PageForward),
            (Key::Char('f'), Command::PageForward),
            (Key::Ctrl('f'), Command::PageForward),
            (Key::Ctrl('v'), Command::PageForward),
            (Key::PageDown, Command::PageForward),
            // PageBackward
            (Key::Char('b'), Command::PageBackward),
            (Key::Ctrl('b'), Command::PageBackward),
            (Key::EscSeq('v'), Command::PageBackward),
            (Key::PageUp, Command::PageBackward),
            // HalfPageForward
            (Key::Char('d'), Command::HalfPageForward),
            (Key::Ctrl('d'), Command::HalfPageForward),
            // HalfPageBackward
            (Key::Char('u'), Command::HalfPageBackward),
            (Key::Ctrl('u'), Command::HalfPageBackward),
            // GotoBeginning
            (Key::Char('g'), Command::GotoBeginning(None)),
            (Key::Home, Command::GotoBeginning(None)),
            (Key::EscSeq('<'), Command::GotoBeginning(None)),
            // GotoEnd
            (Key::Char('G'), Command::GotoEnd(None)),
            (Key::End, Command::GotoEnd(None)),
            (Key::EscSeq('>'), Command::GotoEnd(None)),
            // Repaint
            (Key::Char('r'), Command::Repaint),
            (Key::Ctrl('r'), Command::Repaint),
            (Key::Ctrl('l'), Command::Repaint),
        ];

        Self { bindings }
    }

    /// Look up the command bound to the given key.
    ///
    /// Returns the [`Command`] for the first matching binding,
    /// or [`Command::Noop`] if no binding matches.
    #[must_use]
    pub fn lookup(&self, key: &Key) -> Command {
        for (bound_key, command) in &self.bindings {
            if bound_key == key {
                return command.clone();
            }
        }
        Command::Noop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_less_creates_keymap_without_panic() {
        let _keymap = Keymap::default_less();
    }

    #[test]
    fn test_lookup_q_returns_quit() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('q')), Command::Quit);
    }

    #[test]
    fn test_lookup_space_returns_page_forward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char(' ')), Command::PageForward);
    }

    #[test]
    fn test_lookup_j_returns_scroll_forward_1() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('j')), Command::ScrollForward(1));
    }

    #[test]
    fn test_lookup_k_returns_scroll_backward_1() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('k')), Command::ScrollBackward(1));
    }

    #[test]
    fn test_lookup_g_returns_goto_beginning_none() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('g')), Command::GotoBeginning(None));
    }

    #[test]
    fn test_lookup_upper_g_returns_goto_end_none() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('G')), Command::GotoEnd(None));
    }

    #[test]
    fn test_lookup_unbound_key_returns_noop() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('z')), Command::Noop);
    }

    #[test]
    fn test_lookup_down_returns_scroll_forward_1() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Down), Command::ScrollForward(1));
    }

    #[test]
    fn test_lookup_up_returns_scroll_backward_1() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Up), Command::ScrollBackward(1));
    }

    #[test]
    fn test_lookup_ctrl_f_returns_page_forward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Ctrl('f')), Command::PageForward);
    }

    #[test]
    fn test_lookup_ctrl_b_returns_page_backward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Ctrl('b')), Command::PageBackward);
    }

    #[test]
    fn test_lookup_escseq_v_returns_page_backward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq('v')), Command::PageBackward);
    }

    #[test]
    fn test_lookup_escseq_less_returns_goto_beginning_none() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::EscSeq('<')),
            Command::GotoBeginning(None)
        );
    }
}
