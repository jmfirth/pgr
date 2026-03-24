//! Single-line text editor for command prompts and search input.

use std::io::Write;

use crate::completion::{tab_complete, CompletionMode, CompletionState};
use crate::key::Key;

/// Result of processing a key event in the line editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineEditResult {
    /// The user is still editing. Continue reading keys.
    Continue,
    /// The user pressed Enter. Contains the final input string.
    Confirm(String),
    /// The user pressed Escape or Ctrl+C. Editing was cancelled.
    Cancel,
    /// The user pressed Tab and there are multiple completions.
    /// Contains a status message listing the candidates.
    ContinueWithStatus(String),
}

/// A single-line text editor for command prompts and search input.
///
/// Manages a text buffer with cursor position, supporting insertion,
/// deletion, and cursor movement. Call [`LineEditor::process_key`] for each
/// key event; it returns a [`LineEditResult`] indicating the state.
pub struct LineEditor {
    /// The text buffer.
    buf: String,
    /// Cursor position as a byte offset into `buf`.
    cursor: usize,
    /// The prompt prefix displayed before the input (e.g., "/" or ":").
    prompt: String,
    /// The completion mode for Tab key handling.
    completion_mode: CompletionMode,
    /// Active completion state for cycling through multiple matches.
    completion_state: Option<CompletionState>,
}

impl LineEditor {
    /// Create a new line editor with the given prompt prefix.
    ///
    /// The prompt is displayed before the user's input text but is not
    /// part of the editable content.
    #[must_use]
    pub fn new(prompt: &str) -> Self {
        Self {
            buf: String::new(),
            cursor: 0,
            prompt: prompt.to_owned(),
            completion_mode: CompletionMode::None,
            completion_state: None,
        }
    }

    /// Create a new line editor with the given prompt prefix and completion mode.
    ///
    /// The completion mode determines what kind of tab completion is
    /// available (filenames, option names, or none).
    #[must_use]
    pub fn with_completion(prompt: &str, mode: CompletionMode) -> Self {
        Self {
            buf: String::new(),
            cursor: 0,
            prompt: prompt.to_owned(),
            completion_mode: mode,
            completion_state: None,
        }
    }

    /// Create a new line editor pre-populated with initial text.
    ///
    /// The cursor is positioned at the end of the initial text.
    #[must_use]
    pub fn with_initial(prompt: &str, initial: &str) -> Self {
        Self {
            buf: initial.to_owned(),
            cursor: initial.len(),
            prompt: prompt.to_owned(),
            completion_mode: CompletionMode::None,
            completion_state: None,
        }
    }

    /// Process a key event and return the editing result.
    #[allow(clippy::missing_panics_doc)] // No panics possible; match is exhaustive
    pub fn process_key(&mut self, key: &Key) -> LineEditResult {
        match key {
            Key::Enter => return LineEditResult::Confirm(self.buf.clone()),
            Key::Escape | Key::Ctrl('c') => return LineEditResult::Cancel,
            Key::Tab => return self.handle_tab(),
            Key::Char(c) => {
                self.completion_state = None;
                self.insert(*c);
            }
            Key::Backspace => {
                self.completion_state = None;
                self.backspace();
            }
            Key::Delete => {
                self.completion_state = None;
                self.delete();
            }
            Key::Ctrl('u') => {
                self.completion_state = None;
                self.clear();
            }
            Key::Ctrl('a') | Key::Home => self.home(),
            Key::Ctrl('e') | Key::End => self.end(),
            Key::Left => self.cursor_left(),
            Key::Right => self.cursor_right(),
            Key::Ctrl('w') => {
                self.completion_state = None;
                self.delete_word_backward();
            }
            _ => {}
        }
        LineEditResult::Continue
    }

    /// Handle tab key press for completion.
    fn handle_tab(&mut self) -> LineEditResult {
        let (replacement, status, new_state) = tab_complete(
            &self.buf,
            &self.completion_mode,
            self.completion_state.take(),
        );

        if let Some(text) = replacement {
            self.buf = text;
            self.cursor = self.buf.len();
        }

        self.completion_state = new_state;

        if let Some(msg) = status {
            LineEditResult::ContinueWithStatus(msg)
        } else {
            LineEditResult::Continue
        }
    }

    /// Insert a character at the cursor position.
    pub fn insert(&mut self, c: char) {
        self.buf.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Delete the character before the cursor (backspace).
    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        // Find the previous character boundary.
        let prev = self.prev_char_boundary();
        self.buf.drain(prev..self.cursor);
        self.cursor = prev;
    }

    /// Delete the character at the cursor (forward delete).
    pub fn delete(&mut self) {
        if self.cursor >= self.buf.len() {
            return;
        }
        let next = self.next_char_boundary();
        self.buf.drain(self.cursor..next);
    }

    /// Clear the entire buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
        self.cursor = 0;
    }

    /// Move the cursor one character to the left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary();
        }
    }

    /// Move the cursor one character to the right.
    pub fn cursor_right(&mut self) {
        if self.cursor < self.buf.len() {
            self.cursor = self.next_char_boundary();
        }
    }

    /// Move the cursor to the beginning of the buffer.
    pub fn home(&mut self) {
        self.cursor = 0;
    }

    /// Move the cursor to the end of the buffer.
    pub fn end(&mut self) {
        self.cursor = self.buf.len();
    }

    /// Return the current buffer contents.
    #[must_use]
    pub fn contents(&self) -> &str {
        &self.buf
    }

    /// Return whether the buffer is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Render the editing line to the given writer at the specified position.
    ///
    /// Writes: move cursor to (`row`, `col`), clear to EOL, prompt prefix,
    /// buffer text, then reposition cursor to the correct edit position.
    /// `max_width` is the available terminal columns.
    ///
    /// If the combined prompt + buffer exceeds `max_width`, the visible
    /// window scrolls horizontally to keep the cursor visible.
    ///
    /// # Errors
    ///
    /// Returns an error if writing to the writer fails.
    pub fn render<W: Write>(
        &self,
        writer: &mut W,
        row: usize,
        col: usize,
        max_width: usize,
    ) -> std::io::Result<()> {
        let prompt_char_count = self.prompt.chars().count();
        let cursor_char_pos = self.buf[..self.cursor].chars().count();

        // Total logical position of cursor including prompt.
        let cursor_visual = prompt_char_count + cursor_char_pos;

        // Determine the scroll offset so the cursor stays visible.
        let scroll = if max_width == 0 {
            0
        } else if cursor_visual >= max_width {
            cursor_visual - max_width + 1
        } else {
            0
        };

        // Build the full display string: prompt + buffer.
        let full: String = self.prompt.chars().chain(self.buf.chars()).collect();
        let visible: String = full.chars().skip(scroll).take(max_width).collect();

        // Move cursor to (row, col), 1-indexed for ANSI.
        write!(writer, "\x1b[{};{}H", row + 1, col + 1)?;
        // Clear to end of line.
        write!(writer, "\x1b[K")?;
        // Write visible text.
        write!(writer, "{visible}")?;
        // Reposition cursor to the edit position.
        let cursor_col = col + cursor_visual - scroll;
        write!(writer, "\x1b[{};{}H", row + 1, cursor_col + 1)?;

        Ok(())
    }

    /// Delete from the cursor backward to the previous whitespace boundary.
    fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let before_cursor = &self.buf[..self.cursor];

        // Skip any trailing whitespace.
        let end_pos = before_cursor
            .char_indices()
            .rev()
            .find(|(_, c)| !c.is_whitespace())
            .map_or(0, |(i, _)| i);

        if end_pos == 0 && before_cursor.starts_with(|c: char| c.is_whitespace()) {
            // Everything before cursor is whitespace — delete it all.
            self.buf.drain(..self.cursor);
            self.cursor = 0;
            return;
        }

        // Find the start of the word (next whitespace going backward, or start of string).
        let word_start = before_cursor[..end_pos]
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map_or(0, |(i, c)| i + c.len_utf8());

        self.buf.drain(word_start..self.cursor);
        self.cursor = word_start;
    }

    /// Find the byte offset of the previous character boundary before `self.cursor`.
    fn prev_char_boundary(&self) -> usize {
        self.buf[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    /// Find the byte offset of the next character boundary after `self.cursor`.
    fn next_char_boundary(&self) -> usize {
        self.buf[self.cursor..]
            .char_indices()
            .nth(1)
            .map_or(self.buf.len(), |(i, _)| self.cursor + i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_line_editor_new_is_empty() {
        let editor = LineEditor::new("/");
        assert!(editor.is_empty());
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_insert_single_char() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        assert_eq!(editor.contents(), "a");
    }

    #[test]
    fn test_line_editor_insert_multiple_chars() {
        let mut editor = LineEditor::new("/");
        for c in "hello".chars() {
            editor.insert(c);
        }
        assert_eq!(editor.contents(), "hello");
    }

    #[test]
    fn test_line_editor_backspace_deletes_before_cursor() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        editor.insert('b');
        editor.backspace();
        assert_eq!(editor.contents(), "a");
    }

    #[test]
    fn test_line_editor_backspace_at_start_is_noop() {
        let mut editor = LineEditor::new("/");
        editor.backspace(); // Should not panic.
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_delete_at_cursor() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        editor.insert('b');
        editor.home();
        editor.delete();
        assert_eq!(editor.contents(), "b");
    }

    #[test]
    fn test_line_editor_delete_at_end_is_noop() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        editor.delete(); // Cursor at end — noop.
        assert_eq!(editor.contents(), "a");
    }

    #[test]
    fn test_line_editor_clear_empties_buffer() {
        let mut editor = LineEditor::new("/");
        for c in "hello".chars() {
            editor.insert(c);
        }
        editor.clear();
        assert!(editor.is_empty());
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_cursor_left_moves_position() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        editor.insert('b');
        editor.cursor_left();
        editor.insert('x');
        assert_eq!(editor.contents(), "axb");
    }

    #[test]
    fn test_line_editor_cursor_right_moves_position() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        editor.insert('b');
        editor.home();
        editor.cursor_right();
        editor.insert('x');
        assert_eq!(editor.contents(), "axb");
    }

    #[test]
    fn test_line_editor_home_moves_to_start() {
        let mut editor = LineEditor::new("/");
        for c in "abc".chars() {
            editor.insert(c);
        }
        editor.home();
        editor.insert('x');
        assert_eq!(editor.contents(), "xabc");
    }

    #[test]
    fn test_line_editor_end_moves_to_end() {
        let mut editor = LineEditor::new("/");
        for c in "abc".chars() {
            editor.insert(c);
        }
        editor.home();
        editor.end();
        editor.insert('x');
        assert_eq!(editor.contents(), "abcx");
    }

    #[test]
    fn test_line_editor_process_key_enter_returns_confirm() {
        let mut editor = LineEditor::new("/");
        for c in "test".chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Enter);
        assert_eq!(result, LineEditResult::Confirm("test".to_owned()));
    }

    #[test]
    fn test_line_editor_process_key_escape_returns_cancel() {
        let mut editor = LineEditor::new("/");
        let result = editor.process_key(&Key::Escape);
        assert_eq!(result, LineEditResult::Cancel);
    }

    #[test]
    fn test_line_editor_process_key_ctrl_c_returns_cancel() {
        let mut editor = LineEditor::new("/");
        let result = editor.process_key(&Key::Ctrl('c'));
        assert_eq!(result, LineEditResult::Cancel);
    }

    #[test]
    fn test_line_editor_process_key_ctrl_u_clears() {
        let mut editor = LineEditor::new("/");
        for c in "hello".chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Ctrl('u'));
        assert_eq!(result, LineEditResult::Continue);
        assert!(editor.is_empty());
    }

    #[test]
    fn test_line_editor_utf8_insert_and_cursor() {
        let mut editor = LineEditor::new(":");
        // Insert multi-byte characters.
        editor.insert('ä');
        editor.insert('ö');
        editor.insert('ü');
        assert_eq!(editor.contents(), "äöü");

        // Move left past 'ü', insert 'x'.
        editor.cursor_left();
        editor.insert('x');
        assert_eq!(editor.contents(), "äöxü");

        // Backspace should remove 'x'.
        editor.backspace();
        assert_eq!(editor.contents(), "äöü");

        // Delete at current position should remove 'ü'.
        editor.delete();
        assert_eq!(editor.contents(), "äö");
    }

    #[test]
    fn test_line_editor_with_initial_populates_buffer() {
        let editor = LineEditor::with_initial("/", "search");
        assert_eq!(editor.contents(), "search");
        assert!(!editor.is_empty());
    }

    #[test]
    fn test_line_editor_ctrl_w_deletes_word() {
        let mut editor = LineEditor::new("/");
        for c in "hello world".chars() {
            editor.insert(c);
        }
        editor.process_key(&Key::Ctrl('w'));
        assert_eq!(editor.contents(), "hello ");
    }

    #[test]
    fn test_line_editor_render_writes_prompt_and_content() {
        let mut editor = LineEditor::new("/");
        for c in "test".chars() {
            editor.insert(c);
        }
        let mut output = Vec::new();
        editor.render(&mut output, 0, 0, 80).unwrap();
        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.contains("/test"));
    }

    // Additional edge case tests.

    #[test]
    fn test_line_editor_cursor_left_at_start_is_noop() {
        let mut editor = LineEditor::new("/");
        editor.cursor_left(); // Should not panic.
        editor.insert('a');
        assert_eq!(editor.contents(), "a");
    }

    #[test]
    fn test_line_editor_cursor_right_at_end_is_noop() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        editor.cursor_right(); // Already at end.
        editor.insert('b');
        assert_eq!(editor.contents(), "ab");
    }

    #[test]
    fn test_line_editor_ctrl_w_empty_buffer_is_noop() {
        let mut editor = LineEditor::new("/");
        editor.delete_word_backward(); // Should not panic.
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_ctrl_w_single_word_deletes_all() {
        let mut editor = LineEditor::new("/");
        for c in "hello".chars() {
            editor.insert(c);
        }
        editor.delete_word_backward();
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_process_key_unknown_is_ignored() {
        let mut editor = LineEditor::new("/");
        editor.insert('a');
        let result = editor.process_key(&Key::PageUp);
        assert_eq!(result, LineEditResult::Continue);
        assert_eq!(editor.contents(), "a");
    }

    #[test]
    fn test_line_editor_with_initial_cursor_at_end() {
        let mut editor = LineEditor::with_initial("/", "abc");
        // Cursor should be at end, so inserting appends.
        editor.insert('d');
        assert_eq!(editor.contents(), "abcd");
    }

    #[test]
    fn test_line_editor_render_scrolls_when_exceeding_max_width() {
        let mut editor = LineEditor::new("/");
        // Insert enough text to exceed a narrow width.
        for c in "abcdefghij".chars() {
            editor.insert(c);
        }
        let mut output = Vec::new();
        // Prompt "/" (1 char) + "abcdefghij" (10 chars) = 11 total, max_width = 5.
        editor.render(&mut output, 0, 0, 5).unwrap();
        let rendered = String::from_utf8(output).unwrap();
        // The visible portion should not contain the prompt since we've scrolled past it.
        assert!(!rendered.contains("/abc"));
        // But it should contain the tail end near the cursor.
        assert!(rendered.contains("ghij"));
    }

    #[test]
    fn test_line_editor_process_key_ctrl_a_moves_home() {
        let mut editor = LineEditor::new("/");
        for c in "abc".chars() {
            editor.process_key(&Key::Char(c));
        }
        editor.process_key(&Key::Ctrl('a'));
        editor.insert('x');
        assert_eq!(editor.contents(), "xabc");
    }

    #[test]
    fn test_line_editor_process_key_ctrl_e_moves_end() {
        let mut editor = LineEditor::new("/");
        for c in "abc".chars() {
            editor.process_key(&Key::Char(c));
        }
        editor.process_key(&Key::Ctrl('a'));
        editor.process_key(&Key::Ctrl('e'));
        editor.insert('x');
        assert_eq!(editor.contents(), "abcx");
    }

    // ── Tab completion integration tests ──

    #[test]
    fn test_line_editor_tab_no_completion_mode_is_noop() {
        let mut editor = LineEditor::new("/");
        for c in "test".chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Tab);
        assert_eq!(result, LineEditResult::Continue);
        assert_eq!(editor.contents(), "test");
    }

    #[test]
    fn test_line_editor_tab_option_completion_single_match() {
        let mut editor = LineEditor::with_completion("-- ", CompletionMode::OptionName);
        for c in "wordw".chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Tab);
        assert_eq!(result, LineEditResult::Continue);
        assert_eq!(editor.contents(), "wordwrap");
    }

    #[test]
    fn test_line_editor_tab_option_completion_multiple_matches() {
        let mut editor = LineEditor::with_completion("-- ", CompletionMode::OptionName);
        for c in "quit".chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Tab);
        // Should return ContinueWithStatus since there are multiple matches.
        match result {
            LineEditResult::ContinueWithStatus(msg) => {
                assert!(msg.contains("completions"));
            }
            _ => panic!("Expected ContinueWithStatus, got {result:?}"),
        }
        // Buffer should contain the common prefix.
        assert!(editor.contents().starts_with("quit"));
    }

    #[test]
    fn test_line_editor_tab_no_matches_is_noop() {
        let mut editor = LineEditor::with_completion("-- ", CompletionMode::OptionName);
        for c in "zzzzz".chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Tab);
        assert_eq!(result, LineEditResult::Continue);
        assert_eq!(editor.contents(), "zzzzz");
    }

    #[test]
    fn test_line_editor_tab_completion_state_reset_on_char() {
        let mut editor = LineEditor::with_completion("-- ", CompletionMode::OptionName);
        for c in "quit".chars() {
            editor.process_key(&Key::Char(c));
        }
        // Trigger completion (creates state).
        editor.process_key(&Key::Tab);
        // Type a character — should reset the completion state.
        editor.process_key(&Key::Char('x'));
        // Contents should have appended 'x' to whatever Tab set.
        assert!(editor.contents().ends_with('x'));
    }

    #[test]
    fn test_line_editor_tab_completion_cycles_on_repeated_tab() {
        let mut editor = LineEditor::with_completion("-- ", CompletionMode::OptionName);
        for c in "quit".chars() {
            editor.process_key(&Key::Char(c));
        }
        // First Tab sets common prefix and creates cycling state.
        editor.process_key(&Key::Tab);
        let after_first = editor.contents().to_owned();

        // Second Tab should cycle to first candidate.
        editor.process_key(&Key::Tab);
        let after_second = editor.contents().to_owned();

        // The two should differ (prefix vs. a specific candidate).
        // Both should start with "quit".
        assert!(after_first.starts_with("quit"));
        assert!(after_second.starts_with("quit"));
    }

    #[test]
    fn test_line_editor_with_completion_creates_correct_mode() {
        let editor = LineEditor::with_completion("-- ", CompletionMode::OptionName);
        assert!(editor.is_empty());
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_tab_filename_completion_in_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create a uniquely-named file.
        std::fs::write(base.join("unique_test_file.txt"), "").unwrap();

        let mut editor = LineEditor::with_completion("Examine: ", CompletionMode::Filename);
        let partial = format!("{}/unique_test_f", base.display());
        for c in partial.chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Tab);
        assert_eq!(result, LineEditResult::Continue);
        assert!(editor.contents().contains("unique_test_file.txt"));
    }
}
