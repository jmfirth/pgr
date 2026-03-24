//! Character set classification based on LESSCHARSET and LESSCHARDEF.
//!
//! GNU less uses LESSCHARSET to select a named character set and LESSCHARDEF
//! for per-byte-value customization. This module provides the same classification
//! logic: each byte value (0-255) maps to a `CharType` indicating whether the
//! byte represents a normal printable character, a control character, or a binary
//! (non-displayable) byte.

/// Classification of a single byte value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharType {
    /// Normal printable character — displayed as-is.
    Normal,
    /// Control character — displayed in caret notation (e.g., `^A`).
    Control,
    /// Binary / non-printable — displayed in hex notation or triggers binary warning.
    Binary,
}

/// Maps every byte value (0-255) to a `CharType`.
///
/// Constructed from a named charset (`from_name`) or a LESSCHARDEF string
/// (`from_chardef`). When neither is specified, `auto_detect` inspects locale
/// environment variables to pick a sensible default.
#[derive(Debug, Clone)]
pub struct Charset {
    table: [CharType; 256],
    name: String,
}

impl Charset {
    /// Look up a built-in charset by name.
    ///
    /// Names are matched case-insensitively. Returns `None` for unknown names.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        let lower = name.to_ascii_lowercase();
        let table = match lower.as_str() {
            "ascii" => build_ascii(),
            "latin1" | "latin-1" => build_latin1(),
            "utf-8" | "utf8" => build_utf8(),
            "iso8859" => build_iso8859(),
            "dos" => build_dos(),
            "ebcdic" => build_ebcdic(),
            "koi8-r" | "koi8r" => build_koi8r(),
            _ => return None,
        };
        Some(Self { table, name: lower })
    }

    /// Parse a LESSCHARDEF string into a `Charset`.
    ///
    /// Each character in the definition string sets the type for the corresponding
    /// byte value: `.` = normal, `c` = control, `b` = binary. A numeric digit
    /// repeats the *previous* type that many times (as in GNU less). Unspecified
    /// trailing byte values default to `Normal`.
    #[must_use]
    pub fn from_chardef(def: &str) -> Self {
        let mut table = [CharType::Normal; 256];
        let mut pos: usize = 0;
        let mut last_type = CharType::Normal;

        for ch in def.chars() {
            if pos >= 256 {
                break;
            }
            match ch {
                '.' => {
                    table[pos] = CharType::Normal;
                    last_type = CharType::Normal;
                    pos += 1;
                }
                'c' => {
                    table[pos] = CharType::Control;
                    last_type = CharType::Control;
                    pos += 1;
                }
                'b' => {
                    table[pos] = CharType::Binary;
                    last_type = CharType::Binary;
                    pos += 1;
                }
                d if d.is_ascii_digit() => {
                    #[allow(clippy::cast_possible_truncation)] // single ASCII digit, max 9
                    let count = (d as u8 - b'0') as usize;
                    for _ in 0..count {
                        if pos >= 256 {
                            break;
                        }
                        table[pos] = last_type;
                        pos += 1;
                    }
                }
                // Unknown characters are ignored, matching GNU less tolerance.
                _ => {}
            }
        }

        Self {
            table,
            name: String::from("chardef"),
        }
    }

    /// Auto-detect charset from locale environment variables.
    ///
    /// Checks `LC_ALL`, `LC_CTYPE`, and `LANG` (in that order) for UTF-8
    /// indicators. Falls back to `latin1` if no UTF-8 locale is detected.
    #[must_use]
    pub fn auto_detect() -> Self {
        if locale_is_utf8() {
            Self::from_name("utf-8").unwrap_or_else(Self::fallback)
        } else {
            Self::from_name("latin1").unwrap_or_else(Self::fallback)
        }
    }

    /// Classify a single byte value.
    #[must_use]
    pub fn classify_byte(&self, byte: u8) -> CharType {
        self.table[byte as usize]
    }

    /// The name of this charset (lowercase).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Fallback charset equivalent to ascii — should never be reached via
    /// `from_name` since "ascii" and "latin1" are always valid, but provides
    /// a safe default without panicking.
    fn fallback() -> Self {
        Self {
            table: build_ascii(),
            name: String::from("ascii"),
        }
    }
}

/// Check whether the current locale indicates UTF-8 encoding.
fn locale_is_utf8() -> bool {
    for var in &["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(val) = std::env::var(var) {
            let lower = val.to_ascii_lowercase();
            if lower.contains("utf-8") || lower.contains("utf8") {
                return true;
            }
            // If the variable is set but doesn't mention UTF-8, stop checking
            // lower-priority variables (mirrors GNU less precedence).
            if !val.is_empty() {
                return false;
            }
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Built-in charset tables
//
// These mirror the tables in GNU less charset.c. The general pattern:
//   0x00-0x1F: control (except some charsets treat a few as normal)
//   0x20-0x7E: normal (printable ASCII)
//   0x7F:      control (DEL)
//   0x80-0xFF: varies by charset
// ---------------------------------------------------------------------------

/// Build the ASCII charset table.
///
/// 0x00-0x1F control, 0x20-0x7E normal, 0x7F control, 0x80-0xFF binary.
fn build_ascii() -> [CharType; 256] {
    let mut t = [CharType::Binary; 256];
    // Control characters 0x00-0x1F
    for slot in &mut t[0x00..=0x1F] {
        *slot = CharType::Control;
    }
    // Printable ASCII 0x20-0x7E
    for slot in &mut t[0x20..=0x7E] {
        *slot = CharType::Normal;
    }
    // DEL
    t[0x7F] = CharType::Control;
    // 0x80-0xFF remain Binary
    t
}

/// Build the latin1 (ISO 8859-1) charset table.
///
/// Same as ASCII for 0x00-0x7F. 0x80-0x9F control, 0xA0-0xFF normal.
fn build_latin1() -> [CharType; 256] {
    let mut t = build_ascii();
    // C1 control codes
    for slot in &mut t[0x80..=0x9F] {
        *slot = CharType::Control;
    }
    // Latin-1 supplement printable
    for slot in &mut t[0xA0..] {
        *slot = CharType::Normal;
    }
    t
}

/// Build the UTF-8 charset table.
///
/// Like latin1 but 0x80-0xFF are all normal because they appear as continuation
/// bytes in valid UTF-8 multibyte sequences.
fn build_utf8() -> [CharType; 256] {
    let mut t = build_ascii();
    // All high bytes are normal — they are part of multibyte UTF-8 sequences.
    for slot in &mut t[0x80..] {
        *slot = CharType::Normal;
    }
    t
}

/// Build the iso8859 charset table.
///
/// Same as latin1 — 0x80-0x9F control, 0xA0-0xFF normal.
fn build_iso8859() -> [CharType; 256] {
    build_latin1()
}

/// Build the DOS charset table.
///
/// DOS code pages treat most bytes 0x00-0xFF as printable except the standard
/// ASCII control range. 0x80-0xFF are all normal (box-drawing, accented chars).
fn build_dos() -> [CharType; 256] {
    let mut t = build_ascii();
    for slot in &mut t[0x80..] {
        *slot = CharType::Normal;
    }
    t
}

/// Build the EBCDIC charset table.
///
/// EBCDIC has a different layout from ASCII. Control characters are scattered
/// across the low range. This table covers the most common EBCDIC code page
/// (037/500).
fn build_ebcdic() -> [CharType; 256] {
    let mut t = [CharType::Normal; 256];

    // EBCDIC control characters — common positions in code page 037.
    // 0x00-0x3F: mostly control
    for slot in &mut t[0x00..=0x3F] {
        *slot = CharType::Control;
    }
    // 0x40 is space (normal) — already set by the Normal default.
    // 0x41-0xFE are normal — already set by the Normal default.
    // 0xFF is control.
    t[0xFF] = CharType::Control;

    t
}

/// Build the KOI8-R charset table.
///
/// KOI8-R is an 8-bit Russian character set. 0x80-0xFF are all normal
/// (Cyrillic letters and box-drawing characters).
fn build_koi8r() -> [CharType; 256] {
    let mut t = build_ascii();
    for slot in &mut t[0x80..] {
        *slot = CharType::Normal;
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Named charset lookup
    // -----------------------------------------------------------------------

    #[test]
    fn test_from_name_ascii_returns_some() {
        assert!(Charset::from_name("ascii").is_some());
    }

    #[test]
    fn test_from_name_latin1_returns_some() {
        assert!(Charset::from_name("latin1").is_some());
    }

    #[test]
    fn test_from_name_latin1_alias_returns_some() {
        assert!(Charset::from_name("latin-1").is_some());
    }

    #[test]
    fn test_from_name_utf8_returns_some() {
        assert!(Charset::from_name("utf-8").is_some());
    }

    #[test]
    fn test_from_name_utf8_no_dash_returns_some() {
        assert!(Charset::from_name("utf8").is_some());
    }

    #[test]
    fn test_from_name_iso8859_returns_some() {
        assert!(Charset::from_name("iso8859").is_some());
    }

    #[test]
    fn test_from_name_dos_returns_some() {
        assert!(Charset::from_name("dos").is_some());
    }

    #[test]
    fn test_from_name_ebcdic_returns_some() {
        assert!(Charset::from_name("ebcdic").is_some());
    }

    #[test]
    fn test_from_name_koi8r_returns_some() {
        assert!(Charset::from_name("koi8-r").is_some());
    }

    #[test]
    fn test_from_name_koi8r_no_dash_returns_some() {
        assert!(Charset::from_name("koi8r").is_some());
    }

    #[test]
    fn test_from_name_case_insensitive() {
        assert!(Charset::from_name("UTF-8").is_some());
        assert!(Charset::from_name("Ascii").is_some());
        assert!(Charset::from_name("LATIN1").is_some());
    }

    #[test]
    fn test_from_name_unknown_returns_none() {
        assert!(Charset::from_name("bogus").is_none());
        assert!(Charset::from_name("").is_none());
        assert!(Charset::from_name("windows-1252").is_none());
    }

    // -----------------------------------------------------------------------
    // ASCII charset classification
    // -----------------------------------------------------------------------

    #[test]
    fn test_ascii_nul_is_control() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(0x00), CharType::Control);
    }

    #[test]
    fn test_ascii_tab_is_control() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(0x09), CharType::Control);
    }

    #[test]
    fn test_ascii_newline_is_control() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(0x0A), CharType::Control);
    }

    #[test]
    fn test_ascii_space_is_normal() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(0x20), CharType::Normal);
    }

    #[test]
    fn test_ascii_printable_a_is_normal() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(b'A'), CharType::Normal);
    }

    #[test]
    fn test_ascii_tilde_is_normal() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(0x7E), CharType::Normal);
    }

    #[test]
    fn test_ascii_del_is_control() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(0x7F), CharType::Control);
    }

    #[test]
    fn test_ascii_high_bytes_are_binary() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.classify_byte(0x80), CharType::Binary);
        assert_eq!(cs.classify_byte(0xFF), CharType::Binary);
        assert_eq!(cs.classify_byte(0xA0), CharType::Binary);
    }

    // -----------------------------------------------------------------------
    // Latin1 charset classification
    // -----------------------------------------------------------------------

    #[test]
    fn test_latin1_c1_controls_are_control() {
        let cs = Charset::from_name("latin1").unwrap();
        for byte in 0x80..=0x9F {
            assert_eq!(
                cs.classify_byte(byte),
                CharType::Control,
                "byte 0x{byte:02X} should be control in latin1"
            );
        }
    }

    #[test]
    fn test_latin1_high_printable_are_normal() {
        let cs = Charset::from_name("latin1").unwrap();
        for byte in 0xA0..=0xFF {
            assert_eq!(
                cs.classify_byte(byte),
                CharType::Normal,
                "byte 0x{byte:02X} should be normal in latin1"
            );
        }
    }

    // -----------------------------------------------------------------------
    // UTF-8 charset classification
    // -----------------------------------------------------------------------

    #[test]
    fn test_utf8_low_ascii_matches_ascii() {
        let cs = Charset::from_name("utf-8").unwrap();
        assert_eq!(cs.classify_byte(0x00), CharType::Control);
        assert_eq!(cs.classify_byte(0x20), CharType::Normal);
        assert_eq!(cs.classify_byte(0x7E), CharType::Normal);
        assert_eq!(cs.classify_byte(0x7F), CharType::Control);
    }

    #[test]
    fn test_utf8_high_bytes_all_normal() {
        let cs = Charset::from_name("utf-8").unwrap();
        for byte in 0x80..=0xFF {
            assert_eq!(
                cs.classify_byte(byte),
                CharType::Normal,
                "byte 0x{byte:02X} should be normal in utf-8"
            );
        }
    }

    // -----------------------------------------------------------------------
    // ISO8859 is identical to latin1
    // -----------------------------------------------------------------------

    #[test]
    fn test_iso8859_matches_latin1() {
        let iso = Charset::from_name("iso8859").unwrap();
        let latin = Charset::from_name("latin1").unwrap();
        for byte in 0..=255u8 {
            assert_eq!(
                iso.classify_byte(byte),
                latin.classify_byte(byte),
                "iso8859 and latin1 differ at byte 0x{byte:02X}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // DOS charset classification
    // -----------------------------------------------------------------------

    #[test]
    fn test_dos_high_bytes_all_normal() {
        let cs = Charset::from_name("dos").unwrap();
        for byte in 0x80..=0xFF {
            assert_eq!(
                cs.classify_byte(byte),
                CharType::Normal,
                "byte 0x{byte:02X} should be normal in dos"
            );
        }
    }

    #[test]
    fn test_dos_ascii_control_range() {
        let cs = Charset::from_name("dos").unwrap();
        assert_eq!(cs.classify_byte(0x01), CharType::Control);
        assert_eq!(cs.classify_byte(0x1F), CharType::Control);
    }

    // -----------------------------------------------------------------------
    // EBCDIC charset classification
    // -----------------------------------------------------------------------

    #[test]
    fn test_ebcdic_low_range_control() {
        let cs = Charset::from_name("ebcdic").unwrap();
        for byte in 0x00..=0x3F {
            assert_eq!(
                cs.classify_byte(byte),
                CharType::Control,
                "byte 0x{byte:02X} should be control in ebcdic"
            );
        }
    }

    #[test]
    fn test_ebcdic_space_is_normal() {
        let cs = Charset::from_name("ebcdic").unwrap();
        assert_eq!(cs.classify_byte(0x40), CharType::Normal);
    }

    #[test]
    fn test_ebcdic_printable_range_normal() {
        let cs = Charset::from_name("ebcdic").unwrap();
        // Spot check some EBCDIC printable positions
        assert_eq!(cs.classify_byte(0xC1), CharType::Normal); // 'A' in EBCDIC
        assert_eq!(cs.classify_byte(0xF0), CharType::Normal); // '0' in EBCDIC
    }

    #[test]
    fn test_ebcdic_0xff_is_control() {
        let cs = Charset::from_name("ebcdic").unwrap();
        assert_eq!(cs.classify_byte(0xFF), CharType::Control);
    }

    // -----------------------------------------------------------------------
    // KOI8-R charset classification
    // -----------------------------------------------------------------------

    #[test]
    fn test_koi8r_high_bytes_all_normal() {
        let cs = Charset::from_name("koi8-r").unwrap();
        for byte in 0x80..=0xFF {
            assert_eq!(
                cs.classify_byte(byte),
                CharType::Normal,
                "byte 0x{byte:02X} should be normal in koi8-r"
            );
        }
    }

    // -----------------------------------------------------------------------
    // LESSCHARDEF parsing
    // -----------------------------------------------------------------------

    #[test]
    fn test_chardef_simple_dot_c_b() {
        let cs = Charset::from_chardef("..cb");
        assert_eq!(cs.classify_byte(0), CharType::Normal);
        assert_eq!(cs.classify_byte(1), CharType::Normal);
        assert_eq!(cs.classify_byte(2), CharType::Control);
        assert_eq!(cs.classify_byte(3), CharType::Binary);
    }

    #[test]
    fn test_chardef_digit_repeats_previous() {
        // "c3." means: c at pos 0, then repeat 'c' 3 more times (pos 1,2,3), then '.' at pos 4
        let cs = Charset::from_chardef("c3.");
        assert_eq!(cs.classify_byte(0), CharType::Control);
        assert_eq!(cs.classify_byte(1), CharType::Control);
        assert_eq!(cs.classify_byte(2), CharType::Control);
        assert_eq!(cs.classify_byte(3), CharType::Control);
        assert_eq!(cs.classify_byte(4), CharType::Normal);
    }

    #[test]
    fn test_chardef_trailing_bytes_default_to_normal() {
        let cs = Charset::from_chardef("cb");
        assert_eq!(cs.classify_byte(0), CharType::Control);
        assert_eq!(cs.classify_byte(1), CharType::Binary);
        // Everything else defaults to Normal
        assert_eq!(cs.classify_byte(2), CharType::Normal);
        assert_eq!(cs.classify_byte(255), CharType::Normal);
    }

    #[test]
    fn test_chardef_empty_string_all_normal() {
        let cs = Charset::from_chardef("");
        for byte in 0..=255u8 {
            assert_eq!(cs.classify_byte(byte), CharType::Normal);
        }
    }

    #[test]
    fn test_chardef_unknown_chars_ignored() {
        let cs = Charset::from_chardef(".x.c");
        // 'x' is ignored, so positions are: 0='.', 1='.', 2='c'
        assert_eq!(cs.classify_byte(0), CharType::Normal);
        assert_eq!(cs.classify_byte(1), CharType::Normal);
        assert_eq!(cs.classify_byte(2), CharType::Control);
    }

    #[test]
    fn test_chardef_long_string_stops_at_256() {
        // Build a chardef with more than 256 entries
        let def: String = std::iter::repeat_n('b', 300).collect();
        let cs = Charset::from_chardef(&def);
        // All 256 entries should be binary
        for byte in 0..=255u8 {
            assert_eq!(cs.classify_byte(byte), CharType::Binary);
        }
    }

    #[test]
    fn test_chardef_name_is_chardef() {
        let cs = Charset::from_chardef(".");
        assert_eq!(cs.name(), "chardef");
    }

    // -----------------------------------------------------------------------
    // Auto-detection
    // -----------------------------------------------------------------------

    #[test]
    fn test_auto_detect_returns_a_charset() {
        // Just verify it doesn't panic and returns something sensible.
        let cs = Charset::auto_detect();
        // Should be either utf-8 or latin1 depending on test environment.
        assert!(
            cs.name() == "utf-8" || cs.name() == "latin1",
            "auto_detect returned unexpected charset: {}",
            cs.name()
        );
    }

    // -----------------------------------------------------------------------
    // Name accessor
    // -----------------------------------------------------------------------

    #[test]
    fn test_name_returns_lowercase() {
        let cs = Charset::from_name("UTF-8").unwrap();
        assert_eq!(cs.name(), "utf-8");
    }

    #[test]
    fn test_name_ascii() {
        let cs = Charset::from_name("ascii").unwrap();
        assert_eq!(cs.name(), "ascii");
    }

    // -----------------------------------------------------------------------
    // Charset is Clone
    // -----------------------------------------------------------------------

    #[test]
    fn test_charset_clone_produces_identical_classification() {
        let original = Charset::from_name("utf-8").unwrap();
        let cloned = original.clone();
        for byte in 0..=255u8 {
            assert_eq!(original.classify_byte(byte), cloned.classify_byte(byte));
        }
    }

    // -----------------------------------------------------------------------
    // Edge cases for classify_byte
    // -----------------------------------------------------------------------

    #[test]
    fn test_classify_byte_boundary_values() {
        let cs = Charset::from_name("ascii").unwrap();
        // Verify the exact boundaries
        assert_eq!(cs.classify_byte(0x1F), CharType::Control); // last control
        assert_eq!(cs.classify_byte(0x20), CharType::Normal); // first normal
        assert_eq!(cs.classify_byte(0x7E), CharType::Normal); // last normal
        assert_eq!(cs.classify_byte(0x7F), CharType::Control); // DEL
    }

    // -----------------------------------------------------------------------
    // Chardef digit 0 edge case
    // -----------------------------------------------------------------------

    #[test]
    fn test_chardef_digit_zero_repeats_nothing() {
        // "c0." means: c at pos 0, repeat 0 times (no-op), then '.' at pos 1
        let cs = Charset::from_chardef("c0.");
        assert_eq!(cs.classify_byte(0), CharType::Control);
        assert_eq!(cs.classify_byte(1), CharType::Normal);
    }
}
