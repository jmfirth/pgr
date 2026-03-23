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
    /// Create the default keymap matching the `less` pager keybindings.
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
            // Horizontal scroll
            (Key::Right, Command::ScrollRight),
            (Key::EscSeq(')'), Command::ScrollRight),
            (Key::Left, Command::ScrollLeft),
            (Key::EscSeq('('), Command::ScrollLeft),
            (Key::CtrlRight, Command::ScrollRightEnd),
            (Key::EscSeq('}'), Command::ScrollRightEnd),
            (Key::CtrlLeft, Command::ScrollLeftHome),
            (Key::EscSeq('{'), Command::ScrollLeftHome),
            // Percent and byte navigation
            (Key::Char('p'), Command::GotoPercent),
            (Key::Char('%'), Command::GotoPercent),
            (Key::Char('P'), Command::GotoByteOffset),
            // Force scroll past boundaries
            (Key::EscSeq(' '), Command::ForwardForceEof),
            (Key::EscSeq('b'), Command::BackwardForceBeginning),
            // Window sizing
            (Key::Char('z'), Command::WindowForward),
            (Key::Char('w'), Command::WindowBackward),
            // Follow mode
            (Key::Char('F'), Command::FollowMode),
            // Repaint with refresh
            (Key::Char('R'), Command::RepaintRefresh),
            // File line navigation
            (Key::EscSeq('j'), Command::FileLineForward),
            (Key::EscSeq('k'), Command::FileLineBackward),
            // Force scroll (unclamped)
            (Key::Char('J'), Command::ScrollForwardForce(1)),
            (Key::Char('K'), Command::ScrollBackwardForce(1)),
            (Key::Char('Y'), Command::ScrollBackwardForce(1)),
            // Option toggling and query
            (Key::Char('-'), Command::ToggleOption),
            (Key::Char('_'), Command::QueryOption),
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
        assert_eq!(keymap.lookup(&Key::Char('x')), Command::Noop);
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

    #[test]
    fn test_keymap_right_arrow_maps_to_scroll_right() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Right), Command::ScrollRight);
    }

    #[test]
    fn test_keymap_left_arrow_maps_to_scroll_left() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Left), Command::ScrollLeft);
    }

    #[test]
    fn test_keymap_percent_maps_to_goto_percent() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('%')), Command::GotoPercent);
    }

    #[test]
    fn test_keymap_upper_f_maps_to_follow_mode() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('F')), Command::FollowMode);
    }

    #[test]
    fn test_keymap_upper_r_maps_to_repaint_refresh() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('R')), Command::RepaintRefresh);
    }

    #[test]
    fn test_keymap_p_maps_to_goto_percent() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('p')), Command::GotoPercent);
    }

    #[test]
    fn test_keymap_upper_p_maps_to_goto_byte_offset() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('P')), Command::GotoByteOffset);
    }

    #[test]
    fn test_keymap_z_maps_to_window_forward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('z')), Command::WindowForward);
    }

    #[test]
    fn test_keymap_w_maps_to_window_backward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('w')), Command::WindowBackward);
    }

    #[test]
    fn test_keymap_upper_j_maps_to_scroll_forward_force() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::Char('J')),
            Command::ScrollForwardForce(1)
        );
    }

    #[test]
    fn test_keymap_upper_k_maps_to_scroll_backward_force() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::Char('K')),
            Command::ScrollBackwardForce(1)
        );
    }

    #[test]
    fn test_keymap_upper_y_maps_to_scroll_backward_force() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::Char('Y')),
            Command::ScrollBackwardForce(1)
        );
    }

    #[test]
    fn test_keymap_ctrl_right_maps_to_scroll_right_end() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::CtrlRight), Command::ScrollRightEnd);
    }

    #[test]
    fn test_keymap_ctrl_left_maps_to_scroll_left_home() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::CtrlLeft), Command::ScrollLeftHome);
    }

    #[test]
    fn test_keymap_esc_space_maps_to_forward_force_eof() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq(' ')), Command::ForwardForceEof);
    }

    #[test]
    fn test_keymap_esc_j_maps_to_file_line_forward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq('j')), Command::FileLineForward);
    }

    #[test]
    fn test_keymap_esc_k_maps_to_file_line_backward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq('k')), Command::FileLineBackward);
    }

    // ── Task 119: `-` key maps to ToggleOption command ──

    #[test]
    fn test_keymap_dash_maps_to_toggle_option() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('-')), Command::ToggleOption);
    }

    // ── Task 119: `_` key maps to QueryOption command ──

    #[test]
    fn test_keymap_underscore_maps_to_query_option() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('_')), Command::QueryOption);
    }
}
