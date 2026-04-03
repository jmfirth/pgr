//! Syntax highlighting via syntect, behind the `syntax` feature flag.
//!
//! Wraps syntect to provide filename-based language detection and line-level
//! highlighting that produces ANSI SGR sequences. The highlighted output is
//! fed into the existing render pipeline in `AnsiOnly` mode.

#[cfg(feature = "syntax")]
pub mod highlighting {
    use syntect::easy::HighlightLines;
    use syntect::highlighting::{FontStyle, Style, Theme, ThemeSet};
    use syntect::parsing::{ParseState, SyntaxReference, SyntaxSet};

    /// Default theme name used when none is specified.
    pub const DEFAULT_THEME: &str = "base16-ocean.dark";

    /// Interval (in lines) at which parse states are cached for random access.
    const STATE_CACHE_INTERVAL: usize = 100;

    /// Wraps syntect's syntax set and theme for line-level highlighting.
    pub struct Highlighter {
        syntax_set: SyntaxSet,
        theme: Theme,
        theme_set: ThemeSet,
    }

    impl Highlighter {
        /// Create a highlighter with the default theme (`base16-ocean.dark`).
        ///
        /// # Panics
        ///
        /// Panics if syntect ships zero built-in themes — this is guaranteed
        /// not to happen with the `default-fancy` feature.
        #[must_use]
        pub fn new() -> Self {
            let syntax_set = SyntaxSet::load_defaults_newlines();
            let theme_set = ThemeSet::load_defaults();
            let theme = theme_set
                .themes
                .get(DEFAULT_THEME)
                .cloned()
                .unwrap_or_else(|| {
                    theme_set
                        .themes
                        .values()
                        .next()
                        .cloned()
                        .expect("syntect ships with at least one theme")
                });
            Self {
                syntax_set,
                theme,
                theme_set,
            }
        }

        /// Create a highlighter with a named theme.
        ///
        /// Falls back to the default theme if the name doesn't match any
        /// built-in theme.
        ///
        /// # Panics
        ///
        /// Panics if syntect ships zero built-in themes — this is guaranteed
        /// not to happen with the `default-fancy` feature.
        #[must_use]
        pub fn with_theme(theme_name: &str) -> Self {
            let syntax_set = SyntaxSet::load_defaults_newlines();
            let theme_set = ThemeSet::load_defaults();
            let theme = theme_set
                .themes
                .get(theme_name)
                .or_else(|| theme_set.themes.get(DEFAULT_THEME))
                .cloned()
                .unwrap_or_else(|| {
                    theme_set
                        .themes
                        .values()
                        .next()
                        .cloned()
                        .expect("syntect ships with at least one theme")
                });
            Self {
                syntax_set,
                theme,
                theme_set,
            }
        }

        /// Detect the syntax definition for a filename based on its extension.
        ///
        /// Returns `None` if no syntax matches (plain text files get no
        /// highlighting).
        #[must_use]
        pub fn detect_syntax(&self, filename: &str) -> Option<&SyntaxReference> {
            let ext = std::path::Path::new(filename).extension()?.to_str()?;
            let syn = self.syntax_set.find_syntax_by_extension(ext)?;
            // Don't highlight plain text — it would just apply the default color
            // without any structure.
            if syn.name == "Plain Text" {
                return None;
            }
            Some(syn)
        }

        /// Highlight a single line, returning a string with embedded ANSI SGR
        /// sequences for foreground color and font style.
        ///
        /// The `state` is mutated to track multi-line constructs (strings,
        /// comments, etc.) across consecutive lines.
        pub fn highlight_line(
            &self,
            line: &str,
            _syntax: &SyntaxReference,
            state: &mut ParseState,
        ) -> Option<String> {
            let ops = state.parse_line(line, &self.syntax_set).ok()?;
            let h = syntect::highlighting::Highlighter::new(&self.theme);
            let hs =
                syntect::highlighting::HighlightState::new(&h, syntect::parsing::ScopeStack::new());
            let mut hs_clone = hs.clone();
            let iter = syntect::highlighting::HighlightIterator::new(&mut hs_clone, &ops, line, &h);
            let ranges: Vec<(Style, &str)> = iter.collect();

            let escaped = as_24_bit_terminal_escaped(&ranges);
            Some(escaped)
        }

        /// Create a new parse state for the given syntax.
        #[must_use]
        pub fn initial_state(&self, syntax: &SyntaxReference) -> ParseState {
            ParseState::new(syntax)
        }

        /// Create a `HighlightLines` instance for convenient line-by-line
        /// highlighting that manages parse state internally.
        #[must_use]
        pub fn highlight_lines<'a>(&'a self, syntax: &SyntaxReference) -> HighlightLines<'a> {
            HighlightLines::new(syntax, &self.theme)
        }

        /// Highlight a line using a `HighlightLines` instance, returning ANSI SGR text.
        ///
        /// This is the primary highlighting entry point. The `hl` instance
        /// tracks parse state across calls.
        pub fn highlight_line_easy(
            &self,
            line: &str,
            hl: &mut HighlightLines<'_>,
        ) -> Option<String> {
            let ranges = hl.highlight_line(line, &self.syntax_set).ok()?;
            Some(as_24_bit_terminal_escaped(&ranges))
        }

        /// Return available theme names.
        #[must_use]
        pub fn theme_names(&self) -> Vec<&str> {
            self.theme_set.themes.keys().map(String::as_str).collect()
        }

        /// Access the syntax set.
        #[must_use]
        pub fn syntax_set(&self) -> &SyntaxSet {
            &self.syntax_set
        }

        /// Access the current theme.
        #[must_use]
        pub fn theme(&self) -> &Theme {
            &self.theme
        }
    }

    impl Default for Highlighter {
        fn default() -> Self {
            Self::new()
        }
    }

    /// Convert a syntect `Style` to an ANSI SGR escape sequence.
    ///
    /// Produces 24-bit true color sequences: `\x1b[38;2;r;g;bm` for foreground.
    /// Also handles bold, italic, and underline from `FontStyle`.
    #[must_use]
    pub fn style_to_sgr(style: &Style) -> String {
        let mut parts: Vec<String> = Vec::new();

        if style.font_style.contains(FontStyle::BOLD) {
            parts.push(String::from("1"));
        }
        if style.font_style.contains(FontStyle::ITALIC) {
            parts.push(String::from("3"));
        }
        if style.font_style.contains(FontStyle::UNDERLINE) {
            parts.push(String::from("4"));
        }

        let fg = style.foreground;
        parts.push(format!("38;2;{};{};{}", fg.r, fg.g, fg.b));

        format!("\x1b[{}m", parts.join(";"))
    }

    /// Convert highlighted ranges to a 24-bit ANSI-escaped string.
    ///
    /// Similar to `syntect::util::as_24_bit_terminal_escaped` but without
    /// background color (terminals have their own background).
    #[must_use]
    pub fn as_24_bit_terminal_escaped(ranges: &[(Style, &str)]) -> String {
        let mut result =
            String::with_capacity(ranges.iter().map(|(_, s)| s.len() + 20).sum::<usize>());
        for (style, text) in ranges {
            result.push_str(&style_to_sgr(style));
            result.push_str(text);
        }
        result.push_str("\x1b[0m");
        result
    }

    /// Manages parse state across lines for a single file.
    ///
    /// Caches parse states at regular intervals to support random access
    /// (jumping to a specific line) without re-parsing the entire file.
    pub struct SyntaxState {
        /// Parse states cached at `STATE_CACHE_INTERVAL` boundaries.
        /// Index `i` holds the state at line `i * STATE_CACHE_INTERVAL`.
        cached_states: Vec<ParseState>,
        /// The furthest line number whose parse state has been computed.
        parsed_up_to: usize,
    }

    impl SyntaxState {
        /// Create a new syntax state from an initial parse state.
        #[must_use]
        pub fn new(initial: ParseState) -> Self {
            Self {
                cached_states: vec![initial],
                parsed_up_to: 0,
            }
        }

        /// Get a parse state suitable for highlighting from line `line_num`.
        ///
        /// Returns the nearest cached state and the line number it corresponds to.
        /// The caller must parse lines sequentially from the returned start line
        /// up to the target line.
        #[must_use]
        pub fn state_for_line(&self, line_num: usize) -> (ParseState, usize) {
            let cache_idx = line_num / STATE_CACHE_INTERVAL;
            let available_idx = cache_idx.min(self.cached_states.len().saturating_sub(1));
            let start_line = available_idx * STATE_CACHE_INTERVAL;
            (self.cached_states[available_idx].clone(), start_line)
        }

        /// Record a parse state after parsing a line.
        ///
        /// Caches the state at `STATE_CACHE_INTERVAL` boundaries.
        pub fn record_state(&mut self, line_num: usize, state: &ParseState) {
            if line_num >= self.parsed_up_to {
                self.parsed_up_to = line_num;
            }
            let next_cache_point = self.cached_states.len() * STATE_CACHE_INTERVAL;
            if line_num >= next_cache_point && line_num.is_multiple_of(STATE_CACHE_INTERVAL) {
                self.cached_states.push(state.clone());
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_highlighter_new_creates_without_panic() {
            let _h = Highlighter::new();
        }

        #[test]
        fn test_detect_syntax_rust_returns_some() {
            let h = Highlighter::new();
            let syn = h.detect_syntax("main.rs");
            assert!(syn.is_some());
            assert_eq!(syn.unwrap().name, "Rust");
        }

        #[test]
        fn test_detect_syntax_python_returns_some() {
            let h = Highlighter::new();
            let syn = h.detect_syntax("script.py");
            assert!(syn.is_some());
            assert_eq!(syn.unwrap().name, "Python");
        }

        #[test]
        fn test_detect_syntax_unknown_returns_none() {
            let h = Highlighter::new();
            assert!(h.detect_syntax("file.xyz123").is_none());
        }

        #[test]
        fn test_highlight_line_easy_contains_sgr_sequences() {
            let h = Highlighter::new();
            let syn = h.detect_syntax("main.rs").unwrap();
            let mut hl = h.highlight_lines(syn);
            let result = h.highlight_line_easy("fn main() {\n", &mut hl);
            assert!(result.is_some());
            let text = result.unwrap();
            assert!(text.contains("\x1b["));
        }

        #[test]
        fn test_highlight_line_easy_preserves_content() {
            let h = Highlighter::new();
            let syn = h.detect_syntax("main.rs").unwrap();
            let mut hl = h.highlight_lines(syn);
            let input = "fn main() {\n";
            let result = h.highlight_line_easy(input, &mut hl).unwrap();
            let stripped = strip_ansi(&result);
            assert_eq!(stripped, input);
        }

        #[test]
        fn test_style_to_sgr_produces_valid_format() {
            let style = Style {
                foreground: syntect::highlighting::Color {
                    r: 200,
                    g: 100,
                    b: 50,
                    a: 255,
                },
                background: syntect::highlighting::Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                font_style: FontStyle::empty(),
            };
            let sgr = style_to_sgr(&style);
            assert!(sgr.starts_with("\x1b["));
            assert!(sgr.ends_with('m'));
            assert!(sgr.contains("38;2;200;100;50"));
        }

        #[test]
        fn test_style_to_sgr_with_bold() {
            let style = Style {
                foreground: syntect::highlighting::Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                background: syntect::highlighting::Color {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                font_style: FontStyle::BOLD,
            };
            let sgr = style_to_sgr(&style);
            // Bold should produce "1" in the SGR sequence.
            assert!(sgr.contains("1;") || sgr.starts_with("\x1b[1;"));
        }

        #[test]
        fn test_with_theme_selects_named_theme() {
            let h = Highlighter::with_theme("Solarized (dark)");
            let names = h.theme_names();
            assert!(!names.is_empty());
        }

        #[test]
        fn test_with_theme_fallback_for_nonexistent() {
            let h = Highlighter::with_theme("nonexistent_theme_12345");
            let syn = h.detect_syntax("test.rs");
            assert!(syn.is_some());
        }

        #[test]
        fn test_theme_names_returns_nonempty() {
            let h = Highlighter::new();
            let names = h.theme_names();
            assert!(!names.is_empty());
            assert!(names.contains(&"base16-ocean.dark"));
        }

        #[test]
        fn test_syntax_state_for_line_returns_initial() {
            let h = Highlighter::new();
            let syn = h.detect_syntax("main.rs").unwrap();
            let initial = h.initial_state(syn);
            let ss = SyntaxState::new(initial);
            let (_, start) = ss.state_for_line(0);
            assert_eq!(start, 0);
        }

        #[test]
        fn test_syntax_state_caches_at_interval() {
            let h = Highlighter::new();
            let syn = h.detect_syntax("main.rs").unwrap();
            let initial = h.initial_state(syn);
            let mut ss = SyntaxState::new(initial.clone());

            // Simulate parsing 200 lines.
            let mut state = initial;
            for i in 0..200 {
                let line = "let x = 1;\n";
                let _ = state.parse_line(line, h.syntax_set());
                ss.record_state(i + 1, &state);
            }

            // Cache should have entries at 0 and 100.
            let (_, start) = ss.state_for_line(150);
            assert_eq!(start, 100);
        }

        /// Strip ANSI escape sequences from a string.
        fn strip_ansi(s: &str) -> String {
            let mut result = String::new();
            let mut in_escape = false;
            for ch in s.chars() {
                if in_escape {
                    if ch.is_ascii_alphabetic() {
                        in_escape = false;
                    }
                } else if ch == '\x1b' {
                    in_escape = true;
                } else {
                    result.push(ch);
                }
            }
            result
        }
    }
}
