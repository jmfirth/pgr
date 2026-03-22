//! Command dispatch loop — the main pager event loop.
//!
//! Reads keys, translates them to commands via the keymap, and executes
//! those commands by mutating the screen state and repainting.

use std::io::{Read, Write};

use pgr_core::{Buffer, LineIndex};
use pgr_display::{
    paint_prompt, paint_screen, render_prompt, PromptContext, PromptStyle, RawControlMode, Screen,
};

use crate::error::Result;
use crate::key::Key;
use crate::key_reader::KeyReader;
use crate::keymap::Keymap;
use crate::Command;

/// The main pager state, tying together all subsystems.
pub struct Pager<R: Read, W: Write> {
    reader: KeyReader<R>,
    writer: W,
    keymap: Keymap,
    screen: Screen,
    buffer: Box<dyn Buffer>,
    index: LineIndex,
    raw_mode: RawControlMode,
    tab_width: usize,
    filename: Option<String>,
    prompt_style: PromptStyle,
    /// Numeric prefix accumulator.
    pending_count: Option<usize>,
    /// Whether we should quit.
    should_quit: bool,
    /// Sticky half-page scroll size. Set by `d`/`u` with a count.
    sticky_half_page: Option<usize>,
    /// Custom window size. Set by `z`/`w` with a count.
    custom_window_size: Option<usize>,
}

impl<R: Read, W: Write> Pager<R, W> {
    /// Create a new pager with the given components.
    ///
    /// Uses the default `less` keymap, a 24x80 screen, tab width 8,
    /// and `RawControlMode::Off`.
    #[must_use]
    pub fn new(
        reader: KeyReader<R>,
        writer: W,
        buffer: Box<dyn Buffer>,
        index: LineIndex,
        filename: Option<String>,
    ) -> Self {
        Self {
            reader,
            writer,
            keymap: Keymap::default_less(),
            screen: Screen::new(24, 80),
            buffer,
            index,
            raw_mode: RawControlMode::Off,
            tab_width: 8,
            filename,
            prompt_style: PromptStyle::Short,
            pending_count: None,
            should_quit: false,
            sticky_half_page: None,
            custom_window_size: None,
        }
    }

    /// Run the main loop. Blocks until the user quits or input is exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error if key reading, buffer access, or terminal output fails.
    pub fn run(&mut self) -> Result<()> {
        self.repaint()?;

        loop {
            match self.reader.read_key() {
                Ok(key) => {
                    if !self.process_key(&key)? {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(())
    }

    /// Process a single key event. Returns `Ok(true)` if the pager should
    /// continue, `Ok(false)` if it should quit.
    fn process_key(&mut self, key: &Key) -> Result<bool> {
        // Digit accumulation for numeric prefixes.
        if let Key::Char(c) = *key {
            if c.is_ascii_digit() {
                let digit = u32::from(c) - u32::from('0');
                #[allow(clippy::cast_possible_truncation)] // digit is 0..=9
                let digit = digit as usize;
                self.pending_count = Some(
                    self.pending_count
                        .unwrap_or(0)
                        .saturating_mul(10)
                        .saturating_add(digit),
                );
                return Ok(true);
            }
        }

        let command = self.keymap.lookup(key);
        let count = self.pending_count.take();
        self.execute(&command, count)?;

        Ok(!self.should_quit)
    }

    /// Execute a command with the given numeric count prefix.
    #[allow(clippy::too_many_lines)] // dispatch table is inherently large
    fn execute(&mut self, command: &Command, count: Option<usize>) -> Result<()> {
        let total = self.index.total_lines(&*self.buffer)?;

        match *command {
            Command::ScrollForward(n) => {
                self.screen.scroll_forward(count.unwrap_or(n), total);
                self.repaint()?;
            }
            Command::ScrollBackward(n) => {
                self.screen.scroll_backward(count.unwrap_or(n));
                self.repaint()?;
            }
            Command::PageForward => {
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_forward(count.unwrap_or(window), total);
                self.repaint()?;
            }
            Command::PageBackward => {
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_backward(count.unwrap_or(window));
                self.repaint()?;
            }
            Command::HalfPageForward => {
                if let Some(c) = count {
                    self.sticky_half_page = Some(c);
                }
                let amount = self
                    .sticky_half_page
                    .unwrap_or(self.screen.content_rows() / 2);
                self.screen.scroll_forward(amount, total);
                self.repaint()?;
            }
            Command::HalfPageBackward => {
                if let Some(c) = count {
                    self.sticky_half_page = Some(c);
                }
                let amount = self
                    .sticky_half_page
                    .unwrap_or(self.screen.content_rows() / 2);
                self.screen.scroll_backward(amount);
                self.repaint()?;
            }
            Command::GotoBeginning(n) => {
                self.screen.goto_line(count.or(n).unwrap_or(0), total);
                self.repaint()?;
            }
            Command::GotoEnd(n) => {
                let default = total.saturating_sub(self.screen.content_rows());
                self.screen.goto_line(count.or(n).unwrap_or(default), total);
                self.repaint()?;
            }
            Command::Repaint => {
                self.repaint()?;
            }
            Command::Quit => {
                self.should_quit = true;
            }
            Command::Noop => {}
            Command::ScrollRight => {
                let cols = self.screen.cols();
                let amount = count.unwrap_or(cols / 2);
                let h = self.screen.horizontal_offset();
                self.screen.set_horizontal_offset(h.saturating_add(amount));
                self.repaint()?;
            }
            Command::ScrollLeft => {
                let cols = self.screen.cols();
                let amount = count.unwrap_or(cols / 2);
                let h = self.screen.horizontal_offset();
                self.screen.set_horizontal_offset(h.saturating_sub(amount));
                self.repaint()?;
            }
            Command::ScrollRightEnd => {
                // Find max line width among visible lines and set offset to show rightmost content.
                let (start, end) = self.screen.visible_range();
                let cols = self.screen.cols();
                let mut max_width: usize = 0;
                for line_num in start..end.min(total) {
                    if let Some(content) = self.index.get_line(line_num, &*self.buffer)? {
                        max_width = max_width.max(content.len());
                    }
                }
                let new_offset = max_width.saturating_sub(cols);
                self.screen.set_horizontal_offset(new_offset);
                self.repaint()?;
            }
            Command::ScrollLeftHome => {
                self.screen.set_horizontal_offset(0);
                self.repaint()?;
            }
            Command::GotoPercent => {
                let pct = count.unwrap_or(0).min(100);
                let target = if total == 0 {
                    0
                } else {
                    pct.saturating_mul(total) / 100
                };
                self.screen.goto_line(target, total);
                self.repaint()?;
            }
            Command::GotoByteOffset => {
                let byte_offset = count.unwrap_or(0) as u64;
                let line = self
                    .index
                    .line_at_offset(byte_offset, &*self.buffer)?
                    .unwrap_or(total.saturating_sub(1));
                self.screen.goto_line(line, total);
                self.repaint()?;
            }
            Command::ForwardForceEof => {
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen
                    .scroll_forward_unclamped(count.unwrap_or(window));
                self.repaint()?;
            }
            Command::BackwardForceBeginning => {
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                // scroll_backward already clamps at 0, which is the correct behavior
                self.screen.scroll_backward(count.unwrap_or(window));
                self.repaint()?;
            }
            Command::WindowForward => {
                if let Some(c) = count {
                    self.custom_window_size = Some(c);
                }
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_forward(window, total);
                self.repaint()?;
            }
            Command::WindowBackward => {
                if let Some(c) = count {
                    self.custom_window_size = Some(c);
                }
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_backward(window);
                self.repaint()?;
            }
            Command::FollowMode => {
                self.follow_mode()?;
            }
            Command::RepaintRefresh => {
                self.buffer.refresh()?;
                let new_len = self.buffer.len() as u64;
                self.index = LineIndex::new(new_len);
                self.repaint()?;
            }
            Command::FileLineForward => {
                // Equivalent to ScrollForward for now; differentiation comes with word-wrap.
                self.screen.scroll_forward(count.unwrap_or(1), total);
                self.repaint()?;
            }
            Command::FileLineBackward => {
                // Equivalent to ScrollBackward for now; differentiation comes with word-wrap.
                self.screen.scroll_backward(count.unwrap_or(1));
                self.repaint()?;
            }
            Command::ScrollForwardForce(n) => {
                self.screen.scroll_forward_unclamped(count.unwrap_or(n));
                self.repaint()?;
            }
            Command::ScrollBackwardForce(n) => {
                // scroll_backward already clamps at 0
                self.screen.scroll_backward(count.unwrap_or(n));
                self.repaint()?;
            }
        }

        Ok(())
    }

    /// Enter basic follow mode: scroll to end and exit immediately.
    ///
    /// A full follow mode with `inotify`/`kqueue` polling and non-blocking key
    /// reading is deferred to Phase 2. This stub scrolls to the end of the
    /// buffer and returns, which satisfies the basic "F scrolls to bottom"
    /// contract.
    fn follow_mode(&mut self) -> Result<()> {
        self.buffer.refresh()?;
        let new_len = self.buffer.len() as u64;
        self.index = LineIndex::new(new_len);
        self.index.index_all(&*self.buffer)?;
        let total = self.index.lines_indexed();
        let default = total.saturating_sub(self.screen.content_rows());
        self.screen.goto_line(default, total);
        self.repaint()?;
        Ok(())
    }

    /// Fetch visible lines from the buffer/index and repaint the screen.
    fn repaint(&mut self) -> Result<()> {
        self.index.index_all(&*self.buffer)?;
        let total = self.index.lines_indexed();
        let (start, end) = self.screen.visible_range();

        let mut lines: Vec<Option<String>> = Vec::with_capacity(self.screen.content_rows());
        for line_num in start..end {
            if line_num < total {
                let content = self.index.get_line(line_num, &*self.buffer)?;
                lines.push(content);
            } else {
                lines.push(None);
            }
        }

        paint_screen(
            &mut self.writer,
            &self.screen,
            &lines,
            self.raw_mode,
            self.tab_width,
        )?;

        // Write the prompt on the last row.
        self.paint_status_prompt(total)?;

        Ok(())
    }

    /// Render and paint the status prompt on the last row.
    fn paint_status_prompt(&mut self, total_lines: usize) -> Result<()> {
        let (rows, cols) = self.screen.dimensions();
        if rows == 0 {
            return Ok(());
        }

        let at_eof = if total_lines == 0 {
            true
        } else {
            let (_, end) = self.screen.visible_range();
            end >= total_lines
        };

        let (start, end) = self.screen.visible_range();
        let bottom_display = end.min(total_lines);

        let ctx = PromptContext {
            filename: self.filename.as_deref(),
            top_line: start.saturating_add(1),
            bottom_line: bottom_display,
            total_lines: Some(total_lines),
            total_bytes: self.buffer.len() as u64,
            byte_offset: 0,
            file_index: 0,
            file_count: 1,
            at_eof,
            is_pipe: false,
        };

        let text = render_prompt(self.prompt_style, &ctx);
        paint_prompt(&mut self.writer, &text, rows, cols)?;

        Ok(())
    }

    /// Set the raw control mode for rendering.
    pub fn set_raw_mode(&mut self, mode: RawControlMode) {
        self.raw_mode = mode;
    }

    /// Set the tab stop width.
    pub fn set_tab_width(&mut self, width: usize) {
        self.tab_width = width;
    }

    /// Set the terminal dimensions, delegating to the internal screen state.
    pub fn set_dimensions(&mut self, rows: usize, cols: usize) {
        self.screen.resize(rows, cols);
    }

    /// Set the prompt style used for the status line.
    pub fn set_prompt_style(&mut self, style: PromptStyle) {
        self.prompt_style = style;
    }

    /// Access the screen state (for testing).
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Return the sticky half-page size, if set by a counted `d`/`u` command.
    #[must_use]
    pub fn sticky_half_page(&self) -> Option<usize> {
        self.sticky_half_page
    }

    /// Return the custom window size, if set by a counted `z`/`w` command.
    #[must_use]
    pub fn custom_window_size(&self) -> Option<usize> {
        self.custom_window_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A simple test buffer implementing `Buffer` over a `Vec<u8>`.
    struct TestBuffer {
        data: Vec<u8>,
    }

    impl TestBuffer {
        fn new(data: &[u8]) -> Self {
            Self {
                data: data.to_vec(),
            }
        }
    }

    impl Buffer for TestBuffer {
        fn len(&self) -> usize {
            self.data.len()
        }

        fn read_at(&self, offset: usize, buf: &mut [u8]) -> pgr_core::Result<usize> {
            if offset >= self.data.len() {
                return Ok(0);
            }
            let available = &self.data[offset..];
            let to_copy = available.len().min(buf.len());
            buf[..to_copy].copy_from_slice(&available[..to_copy]);
            Ok(to_copy)
        }

        fn is_growable(&self) -> bool {
            false
        }

        fn refresh(&mut self) -> pgr_core::Result<usize> {
            Ok(self.data.len())
        }
    }

    /// Build a multiline test buffer with numbered lines.
    fn make_test_content(line_count: usize) -> Vec<u8> {
        let mut data = Vec::new();
        for i in 0..line_count {
            data.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        data
    }

    /// Create a pager with the given input bytes and buffer content,
    /// run it, and return the pager for inspection.
    fn run_pager(keys: &[u8], content: &[u8]) -> Pager<Cursor<Vec<u8>>, Vec<u8>> {
        let reader = KeyReader::new(Cursor::new(keys.to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        // Ignore errors from run — they happen when input is exhausted.
        let _ = pager.run();
        pager
    }

    #[test]
    fn test_dispatch_q_causes_quit() {
        let content = make_test_content(50);
        let pager = run_pager(b"q", &content);
        assert!(pager.should_quit);
    }

    #[test]
    fn test_dispatch_j_scrolls_forward_one_line() {
        let content = make_test_content(50);
        let pager = run_pager(b"jq", &content);
        assert_eq!(pager.screen().top_line(), 1);
    }

    #[test]
    fn test_dispatch_k_scrolls_backward_one_line() {
        // Start by scrolling forward, then backward.
        let content = make_test_content(50);
        let pager = run_pager(b"jjjkq", &content);
        // 3 forward, 1 backward = top_line 2
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_space_scrolls_forward_one_page() {
        let content = make_test_content(100);
        let pager = run_pager(b" q", &content);
        // Default screen is 24 rows, content_rows = 23. Space scrolls 23 lines.
        assert_eq!(pager.screen().top_line(), 23);
    }

    #[test]
    fn test_dispatch_b_scrolls_backward_one_page() {
        let content = make_test_content(100);
        // Scroll forward two pages, then back one.
        let pager = run_pager(b"  bq", &content);
        // 23 + 23 = 46, then back 23 = 23.
        assert_eq!(pager.screen().top_line(), 23);
    }

    #[test]
    fn test_dispatch_g_goes_to_beginning() {
        let content = make_test_content(100);
        // Scroll forward, then go to beginning.
        let pager = run_pager(b"   gq", &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_upper_g_goes_to_end() {
        let content = make_test_content(100);
        let pager = run_pager(b"Gq", &content);
        // GotoEnd default: total - content_rows = 100 - 23 = 77
        assert_eq!(pager.screen().top_line(), 77);
    }

    #[test]
    fn test_dispatch_numeric_prefix_5j_scrolls_forward_5() {
        let content = make_test_content(50);
        let pager = run_pager(b"5jq", &content);
        assert_eq!(pager.screen().top_line(), 5);
    }

    #[test]
    fn test_dispatch_numeric_prefix_10_upper_g_goes_to_line_10() {
        let content = make_test_content(100);
        let pager = run_pager(b"10Gq", &content);
        // 10G: goto_line(10, 100) = min(10, 99) = 10
        assert_eq!(pager.screen().top_line(), 10);
    }

    #[test]
    fn test_dispatch_multiple_digits_123j_scrolls_forward_123_clamped() {
        let content = make_test_content(50);
        let pager = run_pager(b"123jq", &content);
        // 123 lines forward, but total is 50, so clamped to 49.
        assert_eq!(pager.screen().top_line(), 49);
    }

    #[test]
    fn test_dispatch_r_triggers_repaint_without_changing_position() {
        let content = make_test_content(50);
        let pager = run_pager(b"jjrq", &content);
        // Two j's move to line 2, r repaints without moving.
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_screen_accessor_returns_reference() {
        let content = make_test_content(10);
        let pager = run_pager(b"q", &content);
        assert_eq!(pager.screen().content_rows(), 23);
    }

    #[test]
    fn test_dispatch_empty_buffer_shows_end() {
        let pager = run_pager(b"q", b"");
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_input_exhausted_exits_gracefully() {
        let content = make_test_content(10);
        // No 'q' — just run out of input.
        let pager = run_pager(b"jj", &content);
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_numeric_prefix_resets_after_command() {
        let content = make_test_content(50);
        // 5j (go to 5), then j (go to 6) — prefix should not carry over.
        let pager = run_pager(b"5jjq", &content);
        assert_eq!(pager.screen().top_line(), 6);
    }

    #[test]
    fn test_dispatch_noop_key_does_not_change_position() {
        let content = make_test_content(50);
        // 'x' is unbound (Noop), should not change position.
        let pager = run_pager(b"jxq", &content);
        assert_eq!(pager.screen().top_line(), 1);
    }

    // ── Horizontal scrolling ─────────────────────────────────────────

    #[test]
    fn test_dispatch_scroll_right_increases_horizontal_offset() {
        let content = make_test_content(50);
        // RIGHT arrow is ESC [ C
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right arrow
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Default scroll: cols/2 = 80/2 = 40
        assert_eq!(pager.screen().horizontal_offset(), 40);
    }

    #[test]
    fn test_dispatch_scroll_left_decreases_horizontal_offset() {
        let content = make_test_content(50);
        // Two rights, then one left
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.extend_from_slice(&[0x1B, b'[', b'D']); // Left
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // 40 + 40 - 40 = 40
        assert_eq!(pager.screen().horizontal_offset(), 40);
    }

    #[test]
    fn test_dispatch_scroll_left_clamps_at_zero() {
        let content = make_test_content(50);
        // Left arrow at offset 0 should stay at 0
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'D']); // Left
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().horizontal_offset(), 0);
    }

    #[test]
    fn test_dispatch_scroll_left_home_resets_to_zero() {
        let content = make_test_content(50);
        // Right, then CtrlLeft (ESC [ 1 ; 5 D)
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.extend_from_slice(&[0x1B, b'[', b'1', b';', b'5', b'D']); // CtrlLeft
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().horizontal_offset(), 0);
    }

    #[test]
    fn test_dispatch_scroll_right_with_count() {
        let content = make_test_content(50);
        // "20" then RIGHT arrow -> scroll right 20
        let mut keys: Vec<u8> = b"20".to_vec();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().horizontal_offset(), 20);
    }

    // ── Percent and byte navigation ──────────────────────────────────

    #[test]
    fn test_dispatch_goto_percent_50_goes_to_middle() {
        let content = make_test_content(100);
        // "50p" -> goto 50% of 100 lines = line 50
        let pager = run_pager(b"50pq", &content);
        assert_eq!(pager.screen().top_line(), 50);
    }

    #[test]
    fn test_dispatch_goto_percent_0_goes_to_beginning() {
        let content = make_test_content(100);
        // Scroll forward first, then "0p" -> goto beginning
        let pager = run_pager(b"  0pq", &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_goto_percent_100_goes_to_end() {
        let content = make_test_content(100);
        let pager = run_pager(b"100pq", &content);
        // 100 * 100 / 100 = 100, clamped to 99 (total_lines - 1)
        assert_eq!(pager.screen().top_line(), 99);
    }

    #[test]
    fn test_dispatch_goto_byte_offset_finds_correct_line() {
        // "line 0\n" is 7 bytes, "line 1\n" is 7 bytes, etc.
        // Byte offset 7 is start of line 1.
        let content = make_test_content(50);
        let pager = run_pager(b"7Pq", &content);
        assert_eq!(pager.screen().top_line(), 1);
    }

    // ── Sticky half-page ─────────────────────────────────────────────

    #[test]
    fn test_dispatch_half_page_forward_with_count_sets_sticky() {
        let content = make_test_content(100);
        // "10d" sets sticky to 10 and scrolls 10. Then "d" uses sticky 10.
        let pager = run_pager(b"10ddq", &content);
        // 10 + 10 = 20
        assert_eq!(pager.screen().top_line(), 20);
        assert_eq!(pager.sticky_half_page(), Some(10));
    }

    #[test]
    fn test_dispatch_half_page_backward_with_count_sets_sticky() {
        let content = make_test_content(100);
        // Scroll forward by 30 first, then "5u" sets sticky to 5 and scrolls back 5
        let pager = run_pager(b"30j5uq", &content);
        assert_eq!(pager.screen().top_line(), 25);
        assert_eq!(pager.sticky_half_page(), Some(5));
    }

    // ── Window sizing ────────────────────────────────────────────────

    #[test]
    fn test_dispatch_z_with_count_sets_window_and_scrolls() {
        let content = make_test_content(100);
        // "15z" sets window to 15 and scrolls forward 15
        let pager = run_pager(b"15zq", &content);
        assert_eq!(pager.screen().top_line(), 15);
        assert_eq!(pager.custom_window_size(), Some(15));
    }

    #[test]
    fn test_dispatch_w_with_count_sets_window_and_scrolls_back() {
        let content = make_test_content(100);
        // Scroll forward 30, then "10w" sets window to 10 and scrolls back 10
        let pager = run_pager(b"30j10wq", &content);
        assert_eq!(pager.screen().top_line(), 20);
        assert_eq!(pager.custom_window_size(), Some(10));
    }

    // ── Force-scroll commands ────────────────────────────────────────

    #[test]
    fn test_dispatch_esc_space_scrolls_forward_even_at_eof() {
        let content = make_test_content(100);
        // Navigate to end with G, then ESC-SPACE scrolls forward unclamped.
        // G -> total(100) - content_rows(23) = 77. Then ESC-SPACE scrolls 23 more -> 100.
        let mut keys = Vec::new();
        keys.push(b'G');
        keys.extend_from_slice(&[0x1B, b' ']); // ESC-SPACE
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // G -> 77, ESC-SPACE -> 77 + 23 = 100 (beyond total_lines - 1 = 99)
        assert_eq!(pager.screen().top_line(), 100);
    }

    #[test]
    fn test_dispatch_upper_j_scrolls_forward_beyond_eof() {
        let content = make_test_content(100);
        // Navigate to end with G, then J scrolls 1 line beyond.
        // G -> 77 (total 100 - content_rows 23), then J -> 78... but that's clamped.
        // Actually J is unclamped, so from 77 it goes to 78.
        // Let's scroll to the very last line first, then J.
        let pager = run_pager(b"99jJq", &content);
        // 99j -> scroll_forward clamped at 99. J -> unclamped 100.
        assert_eq!(pager.screen().top_line(), 100);
    }

    // ── Follow mode ──────────────────────────────────────────────────

    #[test]
    fn test_dispatch_follow_mode_scrolls_to_end() {
        let content = make_test_content(100);
        let pager = run_pager(b"Fq", &content);
        // Follow mode scrolls to end: total(100) - content_rows(23) = 77
        assert_eq!(pager.screen().top_line(), 77);
    }

    // ── Repaint refresh ──────────────────────────────────────────────

    #[test]
    fn test_dispatch_upper_r_refreshes_buffer() {
        let content = make_test_content(50);
        // R refreshes and repaints without moving
        let pager = run_pager(b"jjRq", &content);
        // Position should remain at line 2 after refresh + repaint
        assert_eq!(pager.screen().top_line(), 2);
    }

    // ── Window forward/backward affects page commands ────────────────

    #[test]
    fn test_dispatch_window_size_affects_subsequent_page_forward() {
        let content = make_test_content(100);
        // "10z" sets window to 10, then SPACE uses that window
        let pager = run_pager(b"10z q", &content);
        // 10z -> scrolls 10, SPACE -> scrolls 10 more = 20
        assert_eq!(pager.screen().top_line(), 20);
    }
}
