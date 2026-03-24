//! Search modifier parsing for control-character prefixes.
//!
//! Less supports modifier prefixes at the beginning of a search pattern
//! that alter search behavior. For example, `^N` inverts the match,
//! `^R` forces literal (non-regex) matching, and `^W` enables wrap-around.

/// Parsed search modifiers extracted from the beginning of a pattern string.
///
/// Each flag corresponds to a `less` search modifier prefix. Modifiers
/// are parsed and stripped from the pattern before compilation.
#[allow(clippy::struct_excessive_bools)] // Six independent on/off modifier flags from the less spec
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchModifiers {
    /// Invert the match -- find lines that do NOT contain the pattern.
    pub invert: bool,
    /// Search across all files in the file list.
    pub multi_file: bool,
    /// Start search from the first/last file (depending on direction).
    pub from_first: bool,
    /// Highlight matches but do not move the viewport.
    pub keep_position: bool,
    /// Treat the pattern as a literal string, not a regex.
    pub literal: bool,
    /// Wrap around at buffer boundaries.
    pub wrap: bool,
}

impl SearchModifiers {
    /// Create default modifiers (all false).
    #[must_use]
    pub fn new() -> Self {
        Self {
            invert: false,
            multi_file: false,
            from_first: false,
            keep_position: false,
            literal: false,
            wrap: false,
        }
    }

    /// Parse modifier prefixes from the beginning of a pattern string.
    ///
    /// Returns the parsed modifiers and the remaining pattern string
    /// (with modifier characters stripped).
    ///
    /// Modifiers are consumed left-to-right from the start of the string.
    /// Parsing stops at the first non-modifier character.
    ///
    /// Control characters and their alternate forms:
    /// - `\x0e` (^N) or `!`: invert match
    /// - `\x05` (^E) or `*`: multi-file search
    /// - `\x06` (^F) or `@`: start from first/last file
    /// - `\x0b` (^K): keep position
    /// - `\x12` (^R): literal (non-regex)
    /// - `\x17` (^W): wrap around
    #[must_use]
    pub fn parse(input: &str) -> (Self, &str) {
        let mut mods = Self::new();
        let mut chars = input.char_indices();

        loop {
            let Some((idx, c)) = chars.next() else {
                // Consumed the entire string as modifiers.
                return (mods, "");
            };

            match c {
                '\x0e' | '!' => mods.invert = true,
                '\x05' | '*' => mods.multi_file = true,
                '\x06' | '@' => mods.from_first = true,
                '\x0b' => mods.keep_position = true,
                '\x12' => mods.literal = true,
                '\x17' => mods.wrap = true,
                _ => {
                    // First non-modifier character; the rest is the pattern.
                    return (mods, &input[idx..]);
                }
            }
        }
    }

    /// Merge default modifiers into this instance.
    ///
    /// For each flag in `defaults` that is `true`, sets the corresponding
    /// flag in `self` to `true`. Existing `true` flags in `self` are not
    /// cleared. This allows `--search-options` defaults to be overridden
    /// by per-search control-character modifiers.
    pub fn apply_defaults(&mut self, defaults: &Self) {
        if defaults.invert {
            self.invert = true;
        }
        if defaults.multi_file {
            self.multi_file = true;
        }
        if defaults.from_first {
            self.from_first = true;
        }
        if defaults.keep_position {
            self.keep_position = true;
        }
        if defaults.literal {
            self.literal = true;
        }
        if defaults.wrap {
            self.wrap = true;
        }
    }

    /// Return `true` if no modifiers are set.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.invert
            && !self.multi_file
            && !self.from_first
            && !self.keep_position
            && !self.literal
            && !self.wrap
    }

    /// Build a display prefix showing active modifiers for the search prompt.
    ///
    /// Returns a string like `"!*"` for invert + multi-file, or an empty
    /// string if no modifiers are active.
    #[must_use]
    pub fn display_prefix(&self) -> String {
        let mut prefix = String::new();
        if self.invert {
            prefix.push('!');
        }
        if self.multi_file {
            prefix.push('*');
        }
        if self.from_first {
            prefix.push('@');
        }
        if self.keep_position {
            prefix.push('^');
        }
        if self.literal {
            prefix.push('R');
        }
        if self.wrap {
            prefix.push('W');
        }
        prefix
    }
}

impl Default for SearchModifiers {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Test 1: parse with no modifiers returns defaults and full pattern
    #[test]
    fn test_parse_no_modifiers_returns_defaults_and_full_pattern() {
        let (mods, rest) = SearchModifiers::parse("hello");
        assert_eq!(mods, SearchModifiers::new());
        assert_eq!(rest, "hello");
    }

    // ── Test 2: parse with ^N prefix sets invert and strips prefix
    #[test]
    fn test_parse_ctrl_n_sets_invert_and_strips_prefix() {
        let (mods, rest) = SearchModifiers::parse("\x0ehello");
        assert!(mods.invert);
        assert_eq!(rest, "hello");
    }

    // ── Test 3: parse with `!` prefix sets invert
    #[test]
    fn test_parse_bang_prefix_sets_invert() {
        let (mods, rest) = SearchModifiers::parse("!hello");
        assert!(mods.invert);
        assert_eq!(rest, "hello");
    }

    // ── Test 4: parse with ^R prefix sets literal
    #[test]
    fn test_parse_ctrl_r_sets_literal() {
        let (mods, rest) = SearchModifiers::parse("\x12hello");
        assert!(mods.literal);
        assert_eq!(rest, "hello");
    }

    // ── Test 5: parse with ^W prefix sets wrap
    #[test]
    fn test_parse_ctrl_w_sets_wrap() {
        let (mods, rest) = SearchModifiers::parse("\x17hello");
        assert!(mods.wrap);
        assert_eq!(rest, "hello");
    }

    // ── Test 6: parse with ^K prefix sets keep_position
    #[test]
    fn test_parse_ctrl_k_sets_keep_position() {
        let (mods, rest) = SearchModifiers::parse("\x0bhello");
        assert!(mods.keep_position);
        assert_eq!(rest, "hello");
    }

    // ── Test 7: parse with ^E prefix sets multi_file
    #[test]
    fn test_parse_ctrl_e_sets_multi_file() {
        let (mods, rest) = SearchModifiers::parse("\x05hello");
        assert!(mods.multi_file);
        assert_eq!(rest, "hello");
    }

    // ── Test 8: parse with ^F prefix sets from_first
    #[test]
    fn test_parse_ctrl_f_sets_from_first() {
        let (mods, rest) = SearchModifiers::parse("\x06hello");
        assert!(mods.from_first);
        assert_eq!(rest, "hello");
    }

    // ── Test 9: parse with multiple modifiers (^N^R) sets both and strips both
    #[test]
    fn test_parse_multiple_modifiers_sets_both_and_strips_both() {
        let (mods, rest) = SearchModifiers::parse("\x0e\x12pattern");
        assert!(mods.invert);
        assert!(mods.literal);
        assert!(!mods.wrap);
        assert_eq!(rest, "pattern");
    }

    // ── Test 10: parse with `!` followed by pattern: `!error` -> invert=true, pattern="error"
    #[test]
    fn test_parse_bang_followed_by_pattern() {
        let (mods, rest) = SearchModifiers::parse("!error");
        assert!(mods.invert);
        assert_eq!(rest, "error");
    }

    // ── Test 11: literal modifier (tested via integration in dispatch — here just parse)
    #[test]
    fn test_parse_literal_modifier_strips_prefix_leaving_raw_pattern() {
        let (mods, rest) = SearchModifiers::parse("\x12foo.*bar");
        assert!(mods.literal);
        assert_eq!(rest, "foo.*bar");
    }

    // ── Test 12: parse with all modifiers set
    #[test]
    fn test_parse_all_modifiers_set() {
        let (mods, rest) = SearchModifiers::parse("\x0e\x05\x06\x0b\x12\x17pattern");
        assert!(mods.invert);
        assert!(mods.multi_file);
        assert!(mods.from_first);
        assert!(mods.keep_position);
        assert!(mods.literal);
        assert!(mods.wrap);
        assert_eq!(rest, "pattern");
    }

    // ── Test: parse with alternate forms `*` and `@`
    #[test]
    fn test_parse_alternate_forms_star_and_at() {
        let (mods, rest) = SearchModifiers::parse("*hello");
        assert!(mods.multi_file);
        assert_eq!(rest, "hello");

        let (mods, rest) = SearchModifiers::parse("@hello");
        assert!(mods.from_first);
        assert_eq!(rest, "hello");
    }

    // ── Test: empty input returns defaults and empty pattern
    #[test]
    fn test_parse_empty_input_returns_defaults_and_empty_pattern() {
        let (mods, rest) = SearchModifiers::parse("");
        assert_eq!(mods, SearchModifiers::new());
        assert_eq!(rest, "");
    }

    // ── Test: only modifiers, no pattern text
    #[test]
    fn test_parse_only_modifiers_no_pattern_text() {
        let (mods, rest) = SearchModifiers::parse("\x0e\x12");
        assert!(mods.invert);
        assert!(mods.literal);
        assert_eq!(rest, "");
    }

    // ── Test: is_empty returns true for default, false when any flag set
    #[test]
    fn test_is_empty_default_true_with_flag_false() {
        let mods = SearchModifiers::new();
        assert!(mods.is_empty());

        let (mods, _) = SearchModifiers::parse("!hello");
        assert!(!mods.is_empty());
    }

    // ── Test: display_prefix shows active modifiers
    #[test]
    fn test_display_prefix_shows_active_modifiers() {
        let mut mods = SearchModifiers::new();
        assert_eq!(mods.display_prefix(), "");

        mods.invert = true;
        assert_eq!(mods.display_prefix(), "!");

        mods.wrap = true;
        assert_eq!(mods.display_prefix(), "!W");

        mods.literal = true;
        assert_eq!(mods.display_prefix(), "!RW");
    }

    // ── Test: Default trait implementation matches new()
    #[test]
    fn test_default_matches_new() {
        assert_eq!(SearchModifiers::default(), SearchModifiers::new());
    }

    // ── Tests for apply_defaults ──

    #[test]
    fn test_apply_defaults_merges_true_flags() {
        let mut mods = SearchModifiers::new();
        let mut defaults = SearchModifiers::new();
        defaults.wrap = true;
        defaults.from_first = true;

        mods.apply_defaults(&defaults);
        assert!(mods.wrap);
        assert!(mods.from_first);
        assert!(!mods.invert);
    }

    #[test]
    fn test_apply_defaults_does_not_clear_existing_flags() {
        let mut mods = SearchModifiers::new();
        mods.invert = true;

        let mut defaults = SearchModifiers::new();
        defaults.wrap = true;

        mods.apply_defaults(&defaults);
        assert!(mods.invert); // existing flag preserved
        assert!(mods.wrap); // default applied
    }

    #[test]
    fn test_apply_defaults_empty_defaults_no_change() {
        let mut mods = SearchModifiers::new();
        mods.literal = true;

        let defaults = SearchModifiers::new();
        mods.apply_defaults(&defaults);
        assert!(mods.literal);
        assert!(!mods.wrap);
    }

    #[test]
    fn test_apply_defaults_all_flags() {
        let mut mods = SearchModifiers::new();
        let mut defaults = SearchModifiers::new();
        defaults.invert = true;
        defaults.multi_file = true;
        defaults.from_first = true;
        defaults.keep_position = true;
        defaults.literal = true;
        defaults.wrap = true;

        mods.apply_defaults(&defaults);
        assert!(mods.invert);
        assert!(mods.multi_file);
        assert!(mods.from_first);
        assert!(mods.keep_position);
        assert!(mods.literal);
        assert!(mods.wrap);
    }
}
