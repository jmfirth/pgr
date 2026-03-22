//! Command dispatch loop — the main pager event loop.
//!
//! Reads keys, translates them to commands via the keymap, and executes
//! those commands by mutating the screen state and repainting.

use std::io::{Read, Write};

use pgr_core::{Buffer, LineIndex};
use pgr_display::{paint_screen, RawControlMode, Screen};

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
    #[allow(dead_code)] // Used by Task 014 prompt integration
    filename: Option<String>,
    /// Numeric prefix accumulator.
    pending_count: Option<usize>,
    /// Whether we should quit.
    should_quit: bool,
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
            pending_count: None,
            should_quit: false,
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
                self.screen
                    .scroll_forward(count.unwrap_or(self.screen.content_rows()), total);
                self.repaint()?;
            }
            Command::PageBackward => {
                self.screen
                    .scroll_backward(count.unwrap_or(self.screen.content_rows()));
                self.repaint()?;
            }
            Command::HalfPageForward => {
                self.screen
                    .scroll_forward(count.unwrap_or(self.screen.content_rows() / 2), total);
                self.repaint()?;
            }
            Command::HalfPageBackward => {
                self.screen
                    .scroll_backward(count.unwrap_or(self.screen.content_rows() / 2));
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
        }

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

        // Write a minimal prompt on the last row.
        self.paint_minimal_prompt(total)?;

        Ok(())
    }

    /// Write a minimal hardcoded prompt on the last row.
    ///
    /// Shows `:` normally, or `(END)` if at/past the last line, in reverse video.
    fn paint_minimal_prompt(&mut self, total_lines: usize) -> Result<()> {
        let (rows, _) = self.screen.dimensions();
        if rows == 0 {
            return Ok(());
        }

        // Move cursor to the last row, column 1.
        write!(self.writer, "\x1b[{rows};1H")?;

        let at_eof = if total_lines == 0 {
            true
        } else {
            let (_, end) = self.screen.visible_range();
            end >= total_lines
        };

        let prompt_text = if at_eof { "(END)" } else { ":" };

        // Reverse video on, write prompt, reverse video off, clear rest of line.
        write!(self.writer, "\x1b[7m{prompt_text}\x1b[0m\x1b[K")?;
        self.writer.flush()?;

        Ok(())
    }

    /// Access the screen state (for testing).
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
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
        // 'z' is unbound (Noop), should not change position.
        let pager = run_pager(b"jzq", &content);
        assert_eq!(pager.screen().top_line(), 1);
    }
}
