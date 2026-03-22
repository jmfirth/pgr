//! Screen state tracking for the pager viewport.
//!
//! Manages the visible window into a document: which line is at the top,
//! how many rows and columns are available, and scrolling/navigation logic.

/// The pager viewport state, tracking position and dimensions.
///
/// The screen reserves the last row for the prompt/status line, so
/// `content_rows` is always `rows - 1` (or 0 if `rows` is 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    top_line: usize,
    rows: usize,
    cols: usize,
    content_rows: usize,
    horizontal_offset: usize,
    chop_mode: bool,
}

impl Screen {
    /// Create a new screen with the given terminal dimensions.
    ///
    /// The last row is reserved for the prompt, so `content_rows = rows - 1`.
    #[must_use]
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            top_line: 0,
            rows,
            cols,
            content_rows: rows.saturating_sub(1),
            horizontal_offset: 0,
            chop_mode: false,
        }
    }

    /// Return the range of visible lines as `(top_line, top_line + content_rows)`.
    ///
    /// The end of the range is exclusive and may exceed the document length.
    #[must_use]
    pub fn visible_range(&self) -> (usize, usize) {
        (self.top_line, self.top_line + self.content_rows)
    }

    /// Scroll forward (down) by `n` lines, clamping so `top_line` does not
    /// exceed `total_lines - 1`.
    ///
    /// Returns the new `top_line` value.
    pub fn scroll_forward(&mut self, n: usize, total_lines: usize) -> usize {
        let max_top = total_lines.saturating_sub(1);
        self.top_line = (self.top_line + n).min(max_top);
        self.top_line
    }

    /// Scroll backward (up) by `n` lines, clamping at line 0.
    ///
    /// Returns the new `top_line` value.
    pub fn scroll_backward(&mut self, n: usize) -> usize {
        self.top_line = self.top_line.saturating_sub(n);
        self.top_line
    }

    /// Jump directly to a line number, clamped to the valid range `[0, total_lines - 1]`.
    ///
    /// Returns the new `top_line` value.
    pub fn goto_line(&mut self, line: usize, total_lines: usize) -> usize {
        let max_top = total_lines.saturating_sub(1);
        self.top_line = line.min(max_top);
        self.top_line
    }

    /// Update the terminal dimensions (e.g., after a `SIGWINCH` resize).
    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.rows = rows;
        self.cols = cols;
        self.content_rows = rows.saturating_sub(1);
    }

    /// Return the terminal dimensions as `(rows, cols)`.
    #[must_use]
    pub fn dimensions(&self) -> (usize, usize) {
        (self.rows, self.cols)
    }

    /// Return the current top line index.
    #[must_use]
    pub fn top_line(&self) -> usize {
        self.top_line
    }

    /// Return the number of content rows (total rows minus the prompt row).
    #[must_use]
    pub fn content_rows(&self) -> usize {
        self.content_rows
    }

    /// Return the current horizontal scroll offset in columns.
    #[must_use]
    pub fn horizontal_offset(&self) -> usize {
        self.horizontal_offset
    }

    /// Set the horizontal scroll offset.
    pub fn set_horizontal_offset(&mut self, offset: usize) {
        self.horizontal_offset = offset;
    }

    /// Return whether chop mode (line truncation) is active.
    #[must_use]
    pub fn chop_mode(&self) -> bool {
        self.chop_mode
    }

    /// Set chop mode on or off.
    pub fn set_chop_mode(&mut self, chop: bool) {
        self.chop_mode = chop;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screen_new_visible_range_starts_at_zero() {
        let screen = Screen::new(25, 80);
        assert_eq!(screen.visible_range(), (0, 24));
    }

    #[test]
    fn test_screen_scroll_forward_by_one_increments_top_line() {
        let mut screen = Screen::new(25, 80);
        let top = screen.scroll_forward(1, 1000);
        assert_eq!(top, 1);
        assert_eq!(screen.visible_range(), (1, 25));
    }

    #[test]
    fn test_screen_scroll_forward_clamps_at_total_lines_minus_one() {
        let mut screen = Screen::new(25, 80);
        let top = screen.scroll_forward(200, 100);
        assert_eq!(top, 99);
    }

    #[test]
    fn test_screen_scroll_backward_by_one_decrements_top_line() {
        let mut screen = Screen::new(25, 80);
        screen.scroll_forward(10, 1000);
        let top = screen.scroll_backward(1);
        assert_eq!(top, 9);
    }

    #[test]
    fn test_screen_scroll_backward_clamps_at_zero() {
        let mut screen = Screen::new(25, 80);
        screen.scroll_forward(3, 1000);
        let top = screen.scroll_backward(100);
        assert_eq!(top, 0);
    }

    #[test]
    fn test_screen_goto_line_sets_correct_top_line() {
        let mut screen = Screen::new(25, 80);
        let top = screen.goto_line(42, 1000);
        assert_eq!(top, 42);
        assert_eq!(screen.top_line(), 42);
    }

    #[test]
    fn test_screen_goto_line_clamps_to_valid_range() {
        let mut screen = Screen::new(25, 80);
        let top = screen.goto_line(500, 100);
        assert_eq!(top, 99);
    }

    #[test]
    fn test_screen_resize_updates_dimensions_and_content_rows() {
        let mut screen = Screen::new(25, 80);
        screen.resize(40, 120);
        assert_eq!(screen.dimensions(), (40, 120));
        assert_eq!(screen.content_rows(), 39);
    }

    #[test]
    fn test_screen_content_rows_is_rows_minus_one() {
        let screen = Screen::new(10, 80);
        assert_eq!(screen.content_rows(), 9);
    }

    #[test]
    fn test_screen_zero_rows_content_rows_is_zero() {
        let screen = Screen::new(0, 80);
        assert_eq!(screen.content_rows(), 0);
        assert_eq!(screen.visible_range(), (0, 0));
    }

    #[test]
    fn test_screen_one_row_content_rows_is_zero() {
        let screen = Screen::new(1, 80);
        assert_eq!(screen.content_rows(), 0);
    }

    #[test]
    fn test_screen_scroll_forward_zero_total_lines_stays_at_zero() {
        let mut screen = Screen::new(25, 80);
        let top = screen.scroll_forward(10, 0);
        assert_eq!(top, 0);
    }

    #[test]
    fn test_screen_horizontal_offset_default_is_zero() {
        let screen = Screen::new(25, 80);
        assert_eq!(screen.horizontal_offset(), 0);
    }

    #[test]
    fn test_screen_set_horizontal_offset_stores_value() {
        let mut screen = Screen::new(25, 80);
        screen.set_horizontal_offset(20);
        assert_eq!(screen.horizontal_offset(), 20);
    }

    #[test]
    fn test_screen_chop_mode_default_is_false() {
        let screen = Screen::new(25, 80);
        assert!(!screen.chop_mode());
    }

    #[test]
    fn test_screen_set_chop_mode_stores_value() {
        let mut screen = Screen::new(25, 80);
        screen.set_chop_mode(true);
        assert!(screen.chop_mode());
    }
}
