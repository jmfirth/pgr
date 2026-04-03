//! Keymap: maps terminal key events to pager commands.

use crate::command::Command;
use crate::key::Key;
use crate::lesskey::LesskeyConfig;

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
            // ScrollBackward(1) — note: `y` is rebound to YankLine (pgr extension)
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
            // Follow mode stop on match (ESC-F)
            (Key::EscSeq('F'), Command::FollowModeStopOnMatch),
            // Repaint with refresh
            (Key::Char('R'), Command::RepaintRefresh),
            // File line navigation
            (Key::EscSeq('j'), Command::FileLineForward),
            (Key::EscSeq('k'), Command::FileLineBackward),
            // Force scroll (unclamped) — note: `Y` is rebound to YankScreen (pgr extension)
            (Key::Char('J'), Command::ScrollForwardForce(1)),
            (Key::Char('K'), Command::ScrollBackwardForce(1)),
            // Option toggling and query
            (Key::Char('-'), Command::ToggleOption),
            (Key::Char('_'), Command::QueryOption),
            // Filter mode — `&` is handled via PendingCommand::FilterPrefix
            // in dispatch.rs (supports `&+`, `&-`, `&l` sub-commands).
            // Shell and pipe commands
            (Key::Char('!'), Command::ShellCommand),
            (Key::Char('#'), Command::ShellCommandExpand),
            (Key::Char('|'), Command::PipeToCommand),
            (Key::Char('v'), Command::EditFile),
            (Key::Char('s'), Command::SavePipeInput),
            // Examine (open new file) — alternative binding
            (Key::Char('E'), Command::ExamineAlt),
            // Info and help
            (Key::Char('='), Command::FileInfo),
            (Key::Ctrl('g'), Command::FileInfo),
            (Key::Char('h'), Command::Help),
            (Key::Char('H'), Command::Help),
            (Key::Char('V'), Command::Version),
            // Search
            (Key::Char('/'), Command::SearchForward),
            (Key::Char('?'), Command::SearchBackward),
            (Key::Char('n'), Command::RepeatSearch),
            (Key::Char('N'), Command::RepeatSearchReverse),
            (Key::EscSeq('u'), Command::ToggleHighlight),
            (Key::EscSeq('U'), Command::ClearSearchPattern),
            // Bracket matching
            (Key::Char('{'), Command::FindCloseBracket('{', '}')),
            (Key::Char('}'), Command::FindOpenBracket('{', '}')),
            (Key::Char('('), Command::FindCloseBracket('(', ')')),
            (Key::Char(')'), Command::FindOpenBracket('(', ')')),
            // `[` and `]` are handled via PendingCommand::OpenBracketPrefix
            // and PendingCommand::CloseBracketPrefix in dispatch.rs. When followed
            // by `u` they trigger URL navigation; otherwise they resolve to
            // bracket matching (FindCloseBracket/FindOpenBracket).
            // Cross-file search repeat (ESC-n, ESC-N)
            (Key::EscSeq('n'), Command::SearchNextCrossFile),
            (Key::EscSeq('N'), Command::SearchPrevCrossFile),
            // Cross-file search with new pattern (ESC-/, ESC-?)
            (Key::EscSeq('/'), Command::SearchForwardCrossFile),
            (Key::EscSeq('?'), Command::SearchBackwardCrossFile),
            // Tag navigation
            (Key::Char('t'), Command::NextTag),
            (Key::Char('T'), Command::PrevTag),
            // Clipboard yank (pgr extension)
            (Key::Char('y'), Command::YankLine),
            (Key::Char('Y'), Command::YankScreen),
            // Syntax highlighting toggle
            (Key::EscSeq('S'), Command::ToggleSyntax),
            // Git gutter toggle
            (Key::EscSeq('G'), Command::ToggleGitGutter),
            // Side-by-side diff toggle
            (Key::EscSeq('V'), Command::ToggleSideBySide),
            // Mouse scroll (default 3 lines per wheel tick)
            (Key::ScrollUp, Command::ScrollBackward(3)),
            (Key::ScrollDown, Command::ScrollForward(3)),
        ];

        Self { bindings }
    }

    /// Update the scroll amount for mouse wheel bindings.
    ///
    /// Replaces the `ScrollBackward`/`ScrollForward` commands bound to
    /// `ScrollUp`/`ScrollDown` with the given line count.
    pub fn set_wheel_lines(&mut self, lines: usize) {
        for (key, command) in &mut self.bindings {
            match key {
                Key::ScrollUp => *command = Command::ScrollBackward(lines),
                Key::ScrollDown => *command = Command::ScrollForward(lines),
                _ => {}
            }
        }
    }

    /// Update the scroll direction for mouse wheel bindings (reversed mode).
    ///
    /// Swaps the direction of `ScrollUp` and `ScrollDown` bindings, so
    /// wheel up scrolls forward and wheel down scrolls backward.
    pub fn set_wheel_reversed(&mut self, lines: usize) {
        for (key, command) in &mut self.bindings {
            match key {
                Key::ScrollUp => *command = Command::ScrollForward(lines),
                Key::ScrollDown => *command = Command::ScrollBackward(lines),
                _ => {}
            }
        }
    }

    /// Apply lesskey configuration, overriding or extending the keymap.
    ///
    /// User bindings from the lesskey config take priority over defaults.
    /// If a key already has a binding, the existing binding is replaced.
    /// If the key is new, it is prepended so it takes priority.
    pub fn apply_lesskey(&mut self, config: &LesskeyConfig) {
        for binding in &config.command_bindings {
            let mut found = false;
            for (existing_key, existing_cmd) in &mut self.bindings {
                if *existing_key == binding.key {
                    *existing_cmd = binding.command.clone();
                    found = true;
                    break;
                }
            }
            if !found {
                // Prepend so user bindings are found first by lookup
                self.bindings
                    .insert(0, (binding.key.clone(), binding.command.clone()));
            }
        }
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

    // ── Task 330: Y is rebound to YankScreen (pgr extension) ──
    #[test]
    fn test_keymap_upper_y_maps_to_yank_screen() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('Y')), Command::YankScreen);
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

    // ── Test 14: & key is handled via PendingCommand, not keymap ───────
    #[test]
    fn test_keymap_ampersand_returns_noop_handled_via_pending() {
        let keymap = Keymap::default_less();
        // `&` is now intercepted by PendingCommand::FilterPrefix in dispatch.rs
        // before the keymap is consulted, so the keymap returns Noop.
        assert_eq!(keymap.lookup(&Key::Char('&')), Command::Noop);
    }

    // Test 1: ! key maps to ShellCommand
    #[test]
    fn test_keymap_bang_maps_to_shell_command() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('!')), Command::ShellCommand);
    }

    // Test: # key maps to ShellCommandExpand
    #[test]
    fn test_keymap_hash_maps_to_shell_command_expand() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('#')), Command::ShellCommandExpand);
    }

    // Test: | key maps to PipeToCommand
    #[test]
    fn test_keymap_pipe_maps_to_pipe_to_command() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('|')), Command::PipeToCommand);
    }

    // Test 2: v key maps to EditFile
    #[test]
    fn test_keymap_v_maps_to_edit_file() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('v')), Command::EditFile);
    }

    // Test 3: s key maps to SavePipeInput
    #[test]
    fn test_keymap_s_maps_to_save_pipe_input() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('s')), Command::SavePipeInput);
    }

    // ── Task 118: Info and help command key bindings ──

    // Test 1: `=` key maps to FileInfo command
    #[test]
    fn test_keymap_equals_maps_to_file_info() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('=')), Command::FileInfo);
    }

    // Test 2: `^G` key maps to FileInfo command
    #[test]
    fn test_keymap_ctrl_g_maps_to_file_info() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Ctrl('g')), Command::FileInfo);
    }

    // Test 3: `h` key maps to Help command
    #[test]
    fn test_keymap_h_maps_to_help() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('h')), Command::Help);
    }

    // Test 3b: `H` key maps to Help command
    #[test]
    fn test_keymap_upper_h_maps_to_help() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('H')), Command::Help);
    }

    // Test 4: `V` key maps to Version command
    #[test]
    fn test_keymap_upper_v_maps_to_version() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('V')), Command::Version);
    }

    // ── Task 113: Search command key bindings ──

    #[test]
    fn test_keymap_slash_maps_to_search_forward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('/')), Command::SearchForward);
    }

    #[test]
    fn test_keymap_question_maps_to_search_backward() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('?')), Command::SearchBackward);
    }

    #[test]
    fn test_keymap_n_maps_to_repeat_search() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('n')), Command::RepeatSearch);
    }

    #[test]
    fn test_keymap_upper_n_maps_to_repeat_search_reverse() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('N')), Command::RepeatSearchReverse);
    }

    #[test]
    fn test_keymap_esc_u_maps_to_toggle_highlight() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq('u')), Command::ToggleHighlight);
    }

    #[test]
    fn test_keymap_esc_n_maps_to_search_next_cross_file() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::EscSeq('n')),
            Command::SearchNextCrossFile
        );
    }

    #[test]
    fn test_keymap_esc_upper_n_maps_to_search_prev_cross_file() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::EscSeq('N')),
            Command::SearchPrevCrossFile
        );
    }

    #[test]
    fn test_keymap_esc_slash_maps_to_search_forward_cross_file() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::EscSeq('/')),
            Command::SearchForwardCrossFile
        );
    }

    #[test]
    fn test_keymap_esc_question_maps_to_search_backward_cross_file() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::EscSeq('?')),
            Command::SearchBackwardCrossFile
        );
    }

    // ── Task 247: ESC-U maps to ClearSearchPattern ──

    #[test]
    fn test_keymap_esc_upper_u_maps_to_clear_search_pattern() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::EscSeq('U')),
            Command::ClearSearchPattern
        );
    }

    // ── Task 210: Bracket matching key bindings ──

    #[test]
    fn test_keymap_open_brace_maps_to_find_close_bracket() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::Char('{')),
            Command::FindCloseBracket('{', '}')
        );
    }

    #[test]
    fn test_keymap_close_brace_maps_to_find_open_bracket() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::Char('}')),
            Command::FindOpenBracket('{', '}')
        );
    }

    #[test]
    fn test_keymap_open_paren_maps_to_find_close_bracket() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::Char('(')),
            Command::FindCloseBracket('(', ')')
        );
    }

    #[test]
    fn test_keymap_close_paren_maps_to_find_open_bracket() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::Char(')')),
            Command::FindOpenBracket('(', ')')
        );
    }

    // `[` and `]` are now handled via PendingCommand in dispatch.rs,
    // so the keymap returns Noop for them (they never reach keymap lookup).

    #[test]
    fn test_keymap_open_square_returns_noop_handled_via_pending() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('[')), Command::Noop);
    }

    #[test]
    fn test_keymap_close_square_returns_noop_handled_via_pending() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char(']')), Command::Noop);
    }

    // ── Task 215: Tag navigation key bindings ──

    #[test]
    fn test_keymap_t_maps_to_next_tag() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('t')), Command::NextTag);
    }

    #[test]
    fn test_keymap_upper_t_maps_to_prev_tag() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('T')), Command::PrevTag);
    }

    // ── Task 211: ESC-F maps to FollowModeStopOnMatch ──

    #[test]
    fn test_keymap_esc_f_maps_to_follow_mode_stop_on_match() {
        let keymap = Keymap::default_less();
        assert_eq!(
            keymap.lookup(&Key::EscSeq('F')),
            Command::FollowModeStopOnMatch
        );
    }

    // ── Task 221: Mouse scroll key bindings ──

    #[test]
    fn test_keymap_scroll_up_maps_to_scroll_backward_3() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::ScrollUp), Command::ScrollBackward(3));
    }

    #[test]
    fn test_keymap_scroll_down_maps_to_scroll_forward_3() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::ScrollDown), Command::ScrollForward(3));
    }

    #[test]
    fn test_keymap_set_wheel_lines_updates_scroll_amount() {
        let mut keymap = Keymap::default_less();
        keymap.set_wheel_lines(5);
        assert_eq!(keymap.lookup(&Key::ScrollUp), Command::ScrollBackward(5));
        assert_eq!(keymap.lookup(&Key::ScrollDown), Command::ScrollForward(5));
    }

    #[test]
    fn test_keymap_set_wheel_reversed_swaps_direction() {
        let mut keymap = Keymap::default_less();
        keymap.set_wheel_reversed(3);
        assert_eq!(keymap.lookup(&Key::ScrollUp), Command::ScrollForward(3));
        assert_eq!(keymap.lookup(&Key::ScrollDown), Command::ScrollBackward(3));
    }

    #[test]
    fn test_keymap_set_wheel_lines_does_not_affect_other_bindings() {
        let mut keymap = Keymap::default_less();
        keymap.set_wheel_lines(10);
        // Verify other scroll bindings are unchanged.
        assert_eq!(keymap.lookup(&Key::Down), Command::ScrollForward(1));
        assert_eq!(keymap.lookup(&Key::Up), Command::ScrollBackward(1));
    }

    // ── Task 330: Clipboard yank key bindings ──

    #[test]
    fn test_keymap_y_maps_to_yank_line() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('y')), Command::YankLine);
    }

    #[test]
    fn test_keymap_upper_y_maps_to_yank_screen_explicit() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('Y')), Command::YankScreen);
    }

    // ── Task 313: Syntax highlighting toggle ──

    #[test]
    fn test_keymap_esc_s_maps_to_toggle_syntax() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq('S')), Command::ToggleSyntax);
    }

    // ── Task 356: Git gutter toggle ──

    #[test]
    fn test_keymap_esc_g_maps_to_toggle_git_gutter() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq('G')), Command::ToggleGitGutter);
    }

    // ── Task 355: Side-by-side toggle ──

    #[test]
    fn test_keymap_esc_upper_v_maps_to_toggle_side_by_side() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::EscSeq('V')), Command::ToggleSideBySide);
    }
}
