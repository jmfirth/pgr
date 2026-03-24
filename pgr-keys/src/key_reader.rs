//! Key reader that parses raw terminal bytes into [`Key`] events.

use std::io::Read;

use crate::key::Key;

/// Reads raw bytes from a terminal (or any [`Read`] source) and parses them
/// into structured [`Key`] events, handling escape sequences and UTF-8.
pub struct KeyReader<R: Read> {
    reader: R,
    buf: Vec<u8>,
}

impl<R: Read> KeyReader<R> {
    /// Create a new key reader wrapping the given byte source.
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            buf: Vec::with_capacity(32),
        }
    }

    /// Read and parse the next key event.
    ///
    /// Blocks until at least one byte is available. Escape sequences are parsed
    /// eagerly: if bytes are available after an ESC, they are consumed as part
    /// of the sequence. When reading from a non-blocking or finite source (e.g.,
    /// a byte slice in tests), a standalone ESC may not be distinguishable from
    /// an incomplete sequence.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the underlying reader fails.
    pub fn read_key(&mut self) -> std::io::Result<Key> {
        let first = self.read_byte()?;

        match first {
            0x1B => self.parse_escape(),
            0x09 => Ok(Key::Tab),
            0x0A | 0x0D => Ok(Key::Enter),
            0x7F => Ok(Key::Backspace),
            b @ 0x01..=0x1A => {
                // Ctrl+A through Ctrl+Z (0x01 = 'a', 0x1A = 'z')
                let ch = (b - 1 + b'a') as char;
                Ok(Key::Ctrl(ch))
            }
            b if b & 0x80 != 0 => self.parse_utf8(b),
            b => Ok(Key::Char(b as char)),
        }
    }

    /// Parse an escape sequence after the initial ESC byte has been consumed.
    fn parse_escape(&mut self) -> std::io::Result<Key> {
        match self.try_read_byte()? {
            None => Ok(Key::Escape),
            Some(b'[') => self.parse_csi(),
            Some(b'O') => self.parse_ss3(),
            Some(b) => {
                let ch = b as char;
                Ok(Key::EscSeq(ch))
            }
        }
    }

    /// Parse a CSI sequence (ESC [ ...).
    fn parse_csi(&mut self) -> std::io::Result<Key> {
        self.buf.clear();

        // Read parameter bytes (digits and semicolons) and the final byte.
        loop {
            let b = self.read_byte()?;
            if b == b'M' && self.buf.is_empty() {
                // X11 mouse tracking: ESC[M followed by 3 raw bytes (button, x, y).
                return self.parse_x11_mouse();
            }
            if b == b'<' && self.buf.is_empty() {
                // SGR mouse tracking: ESC[< followed by params and M/m final byte.
                return self.parse_sgr_mouse();
            }
            if (0x40..=0x7E).contains(&b) {
                // Final byte of CSI sequence.
                return Ok(self.map_csi_sequence(b));
            }
            self.buf.push(b);
        }
    }

    /// Parse X11 mouse tracking sequence: ESC[M cb cx cy.
    ///
    /// Button byte (cb) has 32 added. Scroll wheel up = 96 (64+32), down = 97 (65+32).
    fn parse_x11_mouse(&mut self) -> std::io::Result<Key> {
        let cb = self.read_byte()?;
        let cx = self.read_byte()?;
        let cy = self.read_byte()?;

        // cb has 32 added to it. Button 64 = scroll up, 65 = scroll down.
        let button = cb.wrapping_sub(32);
        match button {
            64 => Ok(Key::ScrollUp),
            65 => Ok(Key::ScrollDown),
            _ => {
                // Non-wheel mouse event; report as unknown.
                Ok(Key::Unknown(vec![0x1B, b'[', b'M', cb, cx, cy]))
            }
        }
    }

    /// Parse SGR mouse tracking sequence: ESC[< params M or ESC[< params m.
    ///
    /// Format: ESC[< button;x;y M (press) or ESC[< button;x;y m (release).
    /// Scroll wheel up = button 64, down = button 65.
    fn parse_sgr_mouse(&mut self) -> std::io::Result<Key> {
        self.buf.clear();

        // Read until we hit 'M' (press) or 'm' (release).
        let final_byte = loop {
            let b = self.read_byte()?;
            if b == b'M' || b == b'm' {
                break b;
            }
            self.buf.push(b);
        };

        // Parse button from the first parameter (before the first ';').
        let button = self.parse_sgr_button();

        match button {
            Some(64) => Ok(Key::ScrollUp),
            Some(65) => Ok(Key::ScrollDown),
            _ => {
                // Non-wheel mouse event; report as unknown.
                let mut raw = vec![0x1B, b'[', b'<'];
                raw.extend_from_slice(&self.buf);
                raw.push(final_byte);
                Ok(Key::Unknown(raw))
            }
        }
    }

    /// Extract the button number from the first parameter in `self.buf`.
    ///
    /// The buffer contains `button;x;y` as ASCII digits and semicolons.
    fn parse_sgr_button(&self) -> Option<u16> {
        let params = &self.buf;
        let end = params
            .iter()
            .position(|&b| b == b';')
            .unwrap_or(params.len());
        let digits = &params[..end];
        if digits.is_empty() {
            return None;
        }
        let mut value: u16 = 0;
        for &d in digits {
            if !d.is_ascii_digit() {
                return None;
            }
            value = value.checked_mul(10)?.checked_add(u16::from(d - b'0'))?;
        }
        Some(value)
    }

    /// Map a completed CSI sequence (params in `self.buf`, final byte given) to a `Key`.
    fn map_csi_sequence(&self, final_byte: u8) -> Key {
        let params = &self.buf;

        match final_byte {
            b'A' => Key::Up,
            b'B' => Key::Down,
            b'C' => {
                if params == b"1;5" {
                    Key::CtrlRight
                } else {
                    Key::Right
                }
            }
            b'D' => {
                if params == b"1;5" {
                    Key::CtrlLeft
                } else {
                    Key::Left
                }
            }
            b'H' => Key::Home,
            b'F' => Key::End,
            b'~' => self.map_tilde_sequence(),
            _ => {
                let mut raw = vec![0x1B, b'['];
                raw.extend_from_slice(params);
                raw.push(final_byte);
                Key::Unknown(raw)
            }
        }
    }

    /// Map a tilde-terminated CSI sequence to a key based on the numeric parameter.
    fn map_tilde_sequence(&self) -> Key {
        match self.buf.as_slice() {
            b"3" => Key::Delete,
            b"5" => Key::PageUp,
            b"6" => Key::PageDown,
            _ => {
                let mut raw = vec![0x1B, b'['];
                raw.extend_from_slice(&self.buf);
                raw.push(b'~');
                Key::Unknown(raw)
            }
        }
    }

    /// Parse an SS3 sequence (ESC O ...).
    fn parse_ss3(&mut self) -> std::io::Result<Key> {
        let b = self.read_byte()?;
        let key = match b {
            b'A' => Key::Up,
            b'B' => Key::Down,
            b'C' => Key::Right,
            b'D' => Key::Left,
            b'H' => Key::Home,
            b'F' => Key::End,
            _ => Key::Unknown(vec![0x1B, b'O', b]),
        };
        Ok(key)
    }

    /// Parse a multi-byte UTF-8 character given the first byte.
    fn parse_utf8(&mut self, first: u8) -> std::io::Result<Key> {
        let width = utf8_char_width(first);
        if width == 0 {
            return Ok(Key::Unknown(vec![first]));
        }

        let mut utf8_buf = vec![first];
        for _ in 1..width {
            let b = self.read_byte()?;
            utf8_buf.push(b);
        }

        match std::str::from_utf8(&utf8_buf) {
            Ok(s) => {
                // We know the string has exactly one character.
                match s.chars().next() {
                    Some(ch) => Ok(Key::Char(ch)),
                    None => Ok(Key::Unknown(utf8_buf)),
                }
            }
            Err(_) => Ok(Key::Unknown(utf8_buf)),
        }
    }

    /// Read exactly one byte from the reader, blocking.
    fn read_byte(&mut self) -> std::io::Result<u8> {
        let mut byte = [0u8; 1];
        self.reader.read_exact(&mut byte)?;
        Ok(byte[0])
    }

    /// Try to read one byte. Returns `None` at EOF (zero-length read or `UnexpectedEof`).
    fn try_read_byte(&mut self) -> std::io::Result<Option<u8>> {
        let mut byte = [0u8; 1];
        match self.reader.read(&mut byte) {
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(byte[0])),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
            Err(e) => Err(e),
        }
    }
}

/// Determine the expected byte width of a UTF-8 character from its leading byte.
/// Returns 0 for invalid leading bytes.
const fn utf8_char_width(first: u8) -> usize {
    match first {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xF7 => 4,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// Helper to parse a single key from a byte sequence.
    fn parse_key(bytes: &[u8]) -> Key {
        let cursor = Cursor::new(bytes.to_vec());
        let mut reader = KeyReader::new(cursor);
        reader.read_key().expect("should parse a key")
    }

    #[test]
    fn test_key_reader_single_ascii_char_returns_char() {
        assert_eq!(parse_key(b"a"), Key::Char('a'));
    }

    #[test]
    fn test_key_reader_ctrl_a_returns_ctrl_a() {
        assert_eq!(parse_key(&[0x01]), Key::Ctrl('a'));
    }

    #[test]
    fn test_key_reader_ctrl_c_returns_ctrl_c() {
        assert_eq!(parse_key(&[0x03]), Key::Ctrl('c'));
    }

    #[test]
    fn test_key_reader_ctrl_z_returns_ctrl_z() {
        assert_eq!(parse_key(&[0x1A]), Key::Ctrl('z'));
    }

    #[test]
    fn test_key_reader_carriage_return_returns_enter() {
        assert_eq!(parse_key(&[0x0D]), Key::Enter);
    }

    #[test]
    fn test_key_reader_newline_returns_enter() {
        assert_eq!(parse_key(&[0x0A]), Key::Enter);
    }

    #[test]
    fn test_key_reader_tab_returns_tab() {
        assert_eq!(parse_key(&[0x09]), Key::Tab);
    }

    #[test]
    fn test_key_reader_backspace_returns_backspace() {
        assert_eq!(parse_key(&[0x7F]), Key::Backspace);
    }

    #[test]
    fn test_key_reader_esc_bracket_a_returns_up() {
        assert_eq!(parse_key(&[0x1B, b'[', b'A']), Key::Up);
    }

    #[test]
    fn test_key_reader_esc_bracket_b_returns_down() {
        assert_eq!(parse_key(&[0x1B, b'[', b'B']), Key::Down);
    }

    #[test]
    fn test_key_reader_esc_bracket_c_returns_right() {
        assert_eq!(parse_key(&[0x1B, b'[', b'C']), Key::Right);
    }

    #[test]
    fn test_key_reader_esc_bracket_d_returns_left() {
        assert_eq!(parse_key(&[0x1B, b'[', b'D']), Key::Left);
    }

    #[test]
    fn test_key_reader_esc_bracket_5_tilde_returns_page_up() {
        assert_eq!(parse_key(&[0x1B, b'[', b'5', b'~']), Key::PageUp);
    }

    #[test]
    fn test_key_reader_esc_bracket_6_tilde_returns_page_down() {
        assert_eq!(parse_key(&[0x1B, b'[', b'6', b'~']), Key::PageDown);
    }

    #[test]
    fn test_key_reader_esc_bracket_h_returns_home() {
        assert_eq!(parse_key(&[0x1B, b'[', b'H']), Key::Home);
    }

    #[test]
    fn test_key_reader_esc_bracket_f_returns_end() {
        assert_eq!(parse_key(&[0x1B, b'[', b'F']), Key::End);
    }

    #[test]
    fn test_key_reader_esc_bracket_3_tilde_returns_delete() {
        assert_eq!(parse_key(&[0x1B, b'[', b'3', b'~']), Key::Delete);
    }

    #[test]
    fn test_key_reader_esc_bracket_1_5c_returns_ctrl_right() {
        assert_eq!(
            parse_key(&[0x1B, b'[', b'1', b';', b'5', b'C']),
            Key::CtrlRight
        );
    }

    #[test]
    fn test_key_reader_esc_bracket_1_5d_returns_ctrl_left() {
        assert_eq!(
            parse_key(&[0x1B, b'[', b'1', b';', b'5', b'D']),
            Key::CtrlLeft
        );
    }

    #[test]
    fn test_key_reader_esc_then_char_returns_esc_seq() {
        assert_eq!(parse_key(&[0x1B, b'b']), Key::EscSeq('b'));
    }

    #[test]
    fn test_key_reader_unknown_csi_returns_unknown() {
        // ESC[99~ — unrecognized tilde param
        let key = parse_key(&[0x1B, b'[', b'9', b'9', b'~']);
        match key {
            Key::Unknown(bytes) => {
                assert_eq!(bytes[0], 0x1B);
                assert_eq!(bytes[1], b'[');
            }
            other => panic!("expected Key::Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_key_reader_unknown_csi_final_byte_returns_unknown() {
        // ESC[Z — unrecognized final byte
        let key = parse_key(&[0x1B, b'[', b'Z']);
        match key {
            Key::Unknown(bytes) => {
                assert_eq!(bytes, vec![0x1B, b'[', b'Z']);
            }
            other => panic!("expected Key::Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_key_reader_utf8_cjk_char_returns_char() {
        // '中' is U+4E2D, encoded as [0xE4, 0xB8, 0xAD]
        assert_eq!(parse_key(&[0xE4, 0xB8, 0xAD]), Key::Char('中'));
    }

    #[test]
    fn test_key_reader_utf8_emoji_returns_char() {
        // '🦀' is U+1F980, encoded as [0xF0, 0x9F, 0xA6, 0x80]
        assert_eq!(parse_key(&[0xF0, 0x9F, 0xA6, 0x80]), Key::Char('🦀'));
    }

    #[test]
    fn test_key_reader_ss3_up_returns_up() {
        assert_eq!(parse_key(&[0x1B, b'O', b'A']), Key::Up);
    }

    #[test]
    fn test_key_reader_ss3_down_returns_down() {
        assert_eq!(parse_key(&[0x1B, b'O', b'B']), Key::Down);
    }

    #[test]
    fn test_key_reader_ss3_right_returns_right() {
        assert_eq!(parse_key(&[0x1B, b'O', b'C']), Key::Right);
    }

    #[test]
    fn test_key_reader_ss3_left_returns_left() {
        assert_eq!(parse_key(&[0x1B, b'O', b'D']), Key::Left);
    }

    #[test]
    fn test_key_reader_ss3_home_returns_home() {
        assert_eq!(parse_key(&[0x1B, b'O', b'H']), Key::Home);
    }

    #[test]
    fn test_key_reader_ss3_end_returns_end() {
        assert_eq!(parse_key(&[0x1B, b'O', b'F']), Key::End);
    }

    #[test]
    fn test_key_reader_ss3_unknown_returns_unknown() {
        let key = parse_key(&[0x1B, b'O', b'Z']);
        assert_eq!(key, Key::Unknown(vec![0x1B, b'O', b'Z']));
    }

    #[test]
    fn test_key_reader_standalone_esc_at_eof_returns_escape() {
        // With a cursor, read() after ESC returns 0 bytes (EOF), so we get standalone Escape.
        assert_eq!(parse_key(&[0x1B]), Key::Escape);
    }

    #[test]
    fn test_key_reader_multiple_keys_reads_sequentially() {
        let bytes: Vec<u8> = vec![b'a', b'b', 0x1B, b'[', b'A'];
        let cursor = Cursor::new(bytes);
        let mut reader = KeyReader::new(cursor);

        assert_eq!(reader.read_key().expect("key 1"), Key::Char('a'));
        assert_eq!(reader.read_key().expect("key 2"), Key::Char('b'));
        assert_eq!(reader.read_key().expect("key 3"), Key::Up);
    }

    #[test]
    fn test_key_reader_space_returns_space_char() {
        assert_eq!(parse_key(b" "), Key::Char(' '));
    }

    #[test]
    fn test_key_reader_printable_ascii_returns_char() {
        // Test a range of printable ASCII
        for b in 0x20..=0x7E_u8 {
            let key = parse_key(&[b]);
            assert_eq!(key, Key::Char(b as char), "failed for byte {b:#04x}");
        }
    }

    #[test]
    fn test_key_reader_invalid_utf8_leading_byte_returns_unknown() {
        // 0xFF is not a valid UTF-8 leading byte
        let key = parse_key(&[0xFF]);
        assert_eq!(key, Key::Unknown(vec![0xFF]));
    }

    // ── Mouse input tests ──────────────────────────────────────────────

    #[test]
    fn test_key_reader_x11_mouse_scroll_up_returns_scroll_up() {
        // ESC[M followed by button=96 (64+32), x=33, y=33
        assert_eq!(parse_key(&[0x1B, b'[', b'M', 96, 33, 33]), Key::ScrollUp);
    }

    #[test]
    fn test_key_reader_x11_mouse_scroll_down_returns_scroll_down() {
        // ESC[M followed by button=97 (65+32), x=33, y=33
        assert_eq!(parse_key(&[0x1B, b'[', b'M', 97, 33, 33]), Key::ScrollDown);
    }

    #[test]
    fn test_key_reader_x11_mouse_click_returns_unknown() {
        // ESC[M followed by button=32 (0+32=left click), x=33, y=33
        let key = parse_key(&[0x1B, b'[', b'M', 32, 33, 33]);
        match key {
            Key::Unknown(bytes) => {
                assert_eq!(bytes[0], 0x1B);
                assert_eq!(bytes[2], b'M');
            }
            other => panic!("expected Key::Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_key_reader_sgr_mouse_scroll_up_returns_scroll_up() {
        // ESC[<64;10;20M — SGR scroll wheel up
        assert_eq!(
            parse_key(&[0x1B, b'[', b'<', b'6', b'4', b';', b'1', b'0', b';', b'2', b'0', b'M']),
            Key::ScrollUp
        );
    }

    #[test]
    fn test_key_reader_sgr_mouse_scroll_down_returns_scroll_down() {
        // ESC[<65;10;20M — SGR scroll wheel down
        assert_eq!(
            parse_key(&[0x1B, b'[', b'<', b'6', b'5', b';', b'1', b'0', b';', b'2', b'0', b'M']),
            Key::ScrollDown
        );
    }

    #[test]
    fn test_key_reader_sgr_mouse_click_returns_unknown() {
        // ESC[<0;10;20M — SGR left click
        let key = parse_key(&[
            0x1B, b'[', b'<', b'0', b';', b'1', b'0', b';', b'2', b'0', b'M',
        ]);
        match key {
            Key::Unknown(bytes) => {
                assert_eq!(bytes[0], 0x1B);
            }
            other => panic!("expected Key::Unknown, got {other:?}"),
        }
    }

    #[test]
    fn test_key_reader_sgr_mouse_release_scroll_up_returns_scroll_up() {
        // ESC[<64;10;20m — SGR scroll wheel up (release variant)
        assert_eq!(
            parse_key(&[0x1B, b'[', b'<', b'6', b'4', b';', b'1', b'0', b';', b'2', b'0', b'm']),
            Key::ScrollUp
        );
    }
}
