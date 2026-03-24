//! Single-line text editor for command prompts and search input.

use std::io::Write;

use crate::key::Key;

/// Default maximum number of entries stored in a history list.
const DEFAULT_HISTORY_MAX: usize = 100;

/// Result of processing a key event in the line editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineEditResult {
    /// The user is still editing. Continue reading keys.
    Continue,
    /// The user pressed Enter. Contains the final input string.
    Confirm(String),
    /// The user pressed Escape or Ctrl+C. Editing was cancelled.
    Cancel,
}

/// In-memory history of previous command/search inputs.
///
/// Stores entries in chronological order (oldest first). Supports
/// prefix-filtered backward/forward search for up/down arrow recall.
pub struct History {
    /// The stored entries, oldest first.
    entries: Vec<String>,
    /// Maximum number of entries to retain.
    max_size: usize,
}

impl History {
    /// Create a new empty history with the default maximum size.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_size: DEFAULT_HISTORY_MAX,
        }
    }

    /// Create a new empty history with the given maximum size.
    #[must_use]
    pub fn with_max_size(max_size: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_size,
        }
    }

    /// Add an entry to the history.
    ///
    /// Empty strings are not added. Duplicate consecutive entries are
    /// deduplicated. If the history exceeds the maximum size, the oldest
    /// entry is removed.
    pub fn push(&mut self, entry: String) {
        if entry.is_empty() {
            return;
        }
        // Deduplicate consecutive entries.
        if self.entries.last().is_some_and(|last| *last == entry) {
            return;
        }
        self.entries.push(entry);
        if self.entries.len() > self.max_size {
            self.entries.remove(0);
        }
    }

    /// Return the number of history entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether the history is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get an entry by index (0 = oldest).
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(String::as_str)
    }

    /// Search backward from `from` for an entry starting with `prefix`.
    ///
    /// `from` is an exclusive upper bound: searching starts at `from - 1`.
    /// Returns the index and text of the matching entry, or `None`.
    #[must_use]
    pub fn search_backward(&self, prefix: &str, from: usize) -> Option<(usize, &str)> {
        let start = from.min(self.entries.len());
        self.entries[..start]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, e)| e.starts_with(prefix))
            .map(|(i, e)| (i, e.as_str()))
    }

    /// Search forward from `from` for an entry starting with `prefix`.
    ///
    /// `from` is an inclusive lower bound: searching starts at `from`.
    /// Returns the index and text of the matching entry, or `None`.
    #[must_use]
    pub fn search_forward(&self, prefix: &str, from: usize) -> Option<(usize, &str)> {
        if from >= self.entries.len() {
            return None;
        }
        self.entries[from..]
            .iter()
            .enumerate()
            .find(|(_, e)| e.starts_with(prefix))
            .map(|(i, e)| (from + i, e.as_str()))
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
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
    /// The user's original input before history navigation began.
    saved_input: Option<String>,
    /// Current position in history during navigation (`None` = not navigating).
    history_pos: Option<usize>,
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
            saved_input: None,
            history_pos: None,
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
            saved_input: None,
            history_pos: None,
        }
    }

    /// Process a key event and return the editing result.
    #[allow(clippy::missing_panics_doc)] // No panics possible; match is exhaustive
    pub fn process_key(&mut self, key: &Key) -> LineEditResult {
        match key {
            Key::Enter => return LineEditResult::Confirm(self.buf.clone()),
            Key::Escape | Key::Ctrl('c' | 'g') => return LineEditResult::Cancel,
            Key::Char(c) => {
                self.reset_history_nav();
                self.insert(*c);
            }
            Key::Backspace => {
                self.reset_history_nav();
                self.backspace();
            }
            Key::Delete => {
                self.reset_history_nav();
                self.delete();
            }
            Key::Ctrl('u') => {
                self.reset_history_nav();
                self.clear();
            }
            Key::Ctrl('a') | Key::Home => self.home(),
            Key::Ctrl('e') | Key::End => self.end(),
            Key::Left => self.cursor_left(),
            Key::Right => self.cursor_right(),
            Key::Ctrl('w') => {
                self.reset_history_nav();
                self.delete_word_backward();
            }
            _ => {}
        }
        LineEditResult::Continue
    }

    /// Process a key event with history support.
    ///
    /// Up/Down arrow keys navigate the history with prefix filtering
    /// based on the text typed before the first Up press. All other keys
    /// are delegated to [`process_key`](Self::process_key).
    #[allow(clippy::missing_panics_doc)] // No panics possible
    pub fn process_key_with_history(&mut self, key: &Key, history: &History) -> LineEditResult {
        match key {
            Key::Up => {
                self.history_up(history);
                LineEditResult::Continue
            }
            Key::Down => {
                self.history_down(history);
                LineEditResult::Continue
            }
            _ => self.process_key(key),
        }
    }

    /// Navigate backward (older) in history with prefix filtering.
    fn history_up(&mut self, history: &History) {
        if history.is_empty() {
            return;
        }

        // On the first Up press, save the current input as the prefix.
        if self.saved_input.is_none() {
            self.saved_input = Some(self.buf.clone());
        }

        let prefix = self.saved_input.as_deref().unwrap_or("");
        let from = self.history_pos.unwrap_or(history.len());

        if let Some((idx, entry)) = history.search_backward(prefix, from) {
            self.history_pos = Some(idx);
            self.buf = entry.to_owned();
            self.cursor = self.buf.len();
        }
    }

    /// Navigate forward (newer) in history with prefix filtering.
    fn history_down(&mut self, history: &History) {
        // Only meaningful if we are navigating history.
        let Some(pos) = self.history_pos else {
            return;
        };

        let prefix = self.saved_input.as_deref().unwrap_or("");

        if let Some((idx, entry)) = history.search_forward(prefix, pos + 1) {
            self.history_pos = Some(idx);
            self.buf = entry.to_owned();
            self.cursor = self.buf.len();
        } else {
            // Past the newest entry — restore the original input.
            self.buf = self.saved_input.clone().unwrap_or_default();
            self.cursor = self.buf.len();
            self.history_pos = None;
            self.saved_input = None;
        }
    }

    /// Reset history navigation state (called when the user types/edits).
    fn reset_history_nav(&mut self) {
        self.saved_input = None;
        self.history_pos = None;
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

    // ── History tests ───────────────────────────────────────────────

    #[test]
    fn test_history_new_is_empty() {
        let history = History::new();
        assert!(history.is_empty());
        assert_eq!(history.len(), 0);
    }

    #[test]
    fn test_history_push_adds_entry() {
        let mut history = History::new();
        history.push("foo".to_owned());
        assert_eq!(history.len(), 1);
        assert_eq!(history.get(0), Some("foo"));
    }

    #[test]
    fn test_history_push_ignores_empty_string() {
        let mut history = History::new();
        history.push(String::new());
        assert!(history.is_empty());
    }

    #[test]
    fn test_history_push_deduplicates_consecutive() {
        let mut history = History::new();
        history.push("foo".to_owned());
        history.push("foo".to_owned());
        assert_eq!(history.len(), 1);
    }

    #[test]
    fn test_history_push_allows_non_consecutive_duplicates() {
        let mut history = History::new();
        history.push("foo".to_owned());
        history.push("bar".to_owned());
        history.push("foo".to_owned());
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_history_push_evicts_oldest_when_full() {
        let mut history = History::with_max_size(2);
        history.push("a".to_owned());
        history.push("b".to_owned());
        history.push("c".to_owned());
        assert_eq!(history.len(), 2);
        assert_eq!(history.get(0), Some("b"));
        assert_eq!(history.get(1), Some("c"));
    }

    #[test]
    fn test_history_search_backward_finds_match() {
        let mut history = History::new();
        history.push("alpha".to_owned());
        history.push("beta".to_owned());
        history.push("alpha2".to_owned());
        let result = history.search_backward("alpha", 3);
        assert_eq!(result, Some((2, "alpha2")));
    }

    #[test]
    fn test_history_search_backward_skips_non_matching() {
        let mut history = History::new();
        history.push("alpha".to_owned());
        history.push("beta".to_owned());
        history.push("gamma".to_owned());
        let result = history.search_backward("alpha", 3);
        assert_eq!(result, Some((0, "alpha")));
    }

    #[test]
    fn test_history_search_backward_from_position() {
        let mut history = History::new();
        history.push("alpha1".to_owned());
        history.push("alpha2".to_owned());
        history.push("alpha3".to_owned());
        // From position 2 (exclusive), should find index 1.
        let result = history.search_backward("alpha", 2);
        assert_eq!(result, Some((1, "alpha2")));
    }

    #[test]
    fn test_history_search_backward_no_match_returns_none() {
        let mut history = History::new();
        history.push("alpha".to_owned());
        let result = history.search_backward("beta", 1);
        assert_eq!(result, None);
    }

    #[test]
    fn test_history_search_backward_empty_prefix_matches_all() {
        let mut history = History::new();
        history.push("alpha".to_owned());
        history.push("beta".to_owned());
        let result = history.search_backward("", 2);
        assert_eq!(result, Some((1, "beta")));
    }

    #[test]
    fn test_history_search_forward_finds_match() {
        let mut history = History::new();
        history.push("alpha".to_owned());
        history.push("beta".to_owned());
        history.push("alpha2".to_owned());
        let result = history.search_forward("alpha", 1);
        assert_eq!(result, Some((2, "alpha2")));
    }

    #[test]
    fn test_history_search_forward_no_match_returns_none() {
        let mut history = History::new();
        history.push("alpha".to_owned());
        history.push("beta".to_owned());
        let result = history.search_forward("gamma", 0);
        assert_eq!(result, None);
    }

    #[test]
    fn test_history_search_forward_from_beyond_end_returns_none() {
        let mut history = History::new();
        history.push("alpha".to_owned());
        let result = history.search_forward("alpha", 5);
        assert_eq!(result, None);
    }

    #[test]
    fn test_history_get_out_of_bounds_returns_none() {
        let history = History::new();
        assert_eq!(history.get(0), None);
    }

    #[test]
    fn test_history_default_is_empty() {
        let history = History::default();
        assert!(history.is_empty());
    }

    // ── LineEditor basic tests ──────────────────────────────────────

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
    fn test_line_editor_process_key_ctrl_g_returns_cancel() {
        let mut editor = LineEditor::new("/");
        for c in "test".chars() {
            editor.process_key(&Key::Char(c));
        }
        let result = editor.process_key(&Key::Ctrl('g'));
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

    // ── History navigation tests ────────────────────────────────────

    #[test]
    fn test_line_editor_up_arrow_recalls_previous_search() {
        let mut history = History::new();
        history.push("pattern1".to_owned());
        history.push("pattern2".to_owned());

        let mut editor = LineEditor::new("/");
        let result = editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(result, LineEditResult::Continue);
        assert_eq!(editor.contents(), "pattern2");
    }

    #[test]
    fn test_line_editor_down_arrow_returns_to_newer_entry() {
        let mut history = History::new();
        history.push("pattern1".to_owned());
        history.push("pattern2".to_owned());

        let mut editor = LineEditor::new("/");
        // Go back twice.
        editor.process_key_with_history(&Key::Up, &history);
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "pattern1");
        // Go forward once.
        editor.process_key_with_history(&Key::Down, &history);
        assert_eq!(editor.contents(), "pattern2");
    }

    #[test]
    fn test_line_editor_down_past_newest_restores_original_input() {
        let mut history = History::new();
        history.push("typed_old".to_owned());

        let mut editor = LineEditor::new("/");
        // Type a prefix that matches the history entry.
        for c in "typed".chars() {
            editor.process_key_with_history(&Key::Char(c), &history);
        }
        // Navigate up — "typed_old" matches prefix "typed".
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "typed_old");
        // Navigate down past history — should restore original "typed".
        editor.process_key_with_history(&Key::Down, &history);
        assert_eq!(editor.contents(), "typed");
    }

    #[test]
    fn test_line_editor_prefix_filtering_works() {
        let mut history = History::new();
        history.push("alpha1".to_owned());
        history.push("beta1".to_owned());
        history.push("alpha2".to_owned());
        history.push("beta2".to_owned());

        let mut editor = LineEditor::new("/");
        // Type a prefix.
        for c in "alpha".chars() {
            editor.process_key_with_history(&Key::Char(c), &history);
        }
        // Up should find "alpha2" (most recent matching entry).
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "alpha2");
        // Up again should find "alpha1".
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "alpha1");
        // Up again — no older match, should stay at "alpha1".
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "alpha1");
    }

    #[test]
    fn test_line_editor_up_on_empty_history_is_noop() {
        let history = History::new();
        let mut editor = LineEditor::new("/");
        for c in "typed".chars() {
            editor.insert(c);
        }
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "typed");
    }

    #[test]
    fn test_line_editor_down_without_navigating_is_noop() {
        let mut history = History::new();
        history.push("pattern".to_owned());
        let mut editor = LineEditor::new("/");
        editor.process_key_with_history(&Key::Down, &history);
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_typing_resets_history_navigation() {
        let mut history = History::new();
        history.push("pattern1".to_owned());
        history.push("pattern2".to_owned());

        let mut editor = LineEditor::new("/");
        // Navigate into history.
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "pattern2");
        // Type a character — should reset history state.
        editor.process_key_with_history(&Key::Char('x'), &history);
        assert_eq!(editor.contents(), "pattern2x");
        // Up should now use "pattern2x" as prefix (no match).
        editor.process_key_with_history(&Key::Up, &history);
        // No history entry starts with "pattern2x", so buffer unchanged.
        assert_eq!(editor.contents(), "pattern2x");
    }

    #[test]
    fn test_line_editor_history_with_empty_prefix_navigates_all() {
        let mut history = History::new();
        history.push("aaa".to_owned());
        history.push("bbb".to_owned());
        history.push("ccc".to_owned());

        let mut editor = LineEditor::new("/");
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "ccc");
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "bbb");
        editor.process_key_with_history(&Key::Up, &history);
        assert_eq!(editor.contents(), "aaa");
        editor.process_key_with_history(&Key::Down, &history);
        assert_eq!(editor.contents(), "bbb");
        editor.process_key_with_history(&Key::Down, &history);
        assert_eq!(editor.contents(), "ccc");
        editor.process_key_with_history(&Key::Down, &history);
        // Restored to original empty input.
        assert_eq!(editor.contents(), "");
    }

    #[test]
    fn test_line_editor_non_history_keys_delegate_to_process_key() {
        let history = History::new();
        let mut editor = LineEditor::new("/");
        let result = editor.process_key_with_history(&Key::Enter, &history);
        assert_eq!(result, LineEditResult::Confirm(String::new()));
    }
}
