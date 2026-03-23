//! Color configuration and ANSI SGR generation for UI elements.
//!
//! Implements the GNU less `-D` flag color specification system, supporting
//! standard 4-bit colors, 8-bit extended colors, and text attributes.

use std::collections::HashMap;

use crate::error::DisplayError;

/// Selector characters identifying which UI element a color applies to.
///
/// Matches the GNU less `-D` flag selectors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorSelector {
    /// `B` — Binary characters.
    Binary,
    /// `C` — Control characters.
    Control,
    /// `E` — Error messages.
    Error,
    /// `H` — Header lines (--header).
    Header,
    /// `M` — Mark highlights.
    Mark,
    /// `N` — Line numbers (-N).
    LineNumber,
    /// `P` — Prompt.
    Prompt,
    /// `R` — Rscroll character.
    Rscroll,
    /// `S` — Search highlights.
    Search,
    /// `W` — Unread line highlights (-w/-W).
    Unread,
    /// `d` — Bold text.
    Bold,
    /// `k` — Blinking text.
    Blink,
    /// `s` — Standout text.
    Standout,
    /// `u` — Underlined text.
    Underline,
}

impl ColorSelector {
    /// Parse a selector character into the corresponding variant.
    ///
    /// # Errors
    ///
    /// Returns `DisplayError::InvalidColor` if the character is not a valid selector.
    pub fn from_char(c: char) -> crate::Result<Self> {
        match c {
            'B' => Ok(Self::Binary),
            'C' => Ok(Self::Control),
            'E' => Ok(Self::Error),
            'H' => Ok(Self::Header),
            'M' => Ok(Self::Mark),
            'N' => Ok(Self::LineNumber),
            'P' => Ok(Self::Prompt),
            'R' => Ok(Self::Rscroll),
            'S' => Ok(Self::Search),
            'W' => Ok(Self::Unread),
            'd' => Ok(Self::Bold),
            'k' => Ok(Self::Blink),
            's' => Ok(Self::Standout),
            'u' => Ok(Self::Underline),
            _ => Err(DisplayError::InvalidColor(format!(
                "unknown selector character: '{c}'"
            ))),
        }
    }
}

/// A terminal color value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    /// Standard 4-bit color (0-7 for normal, 8-15 for bright).
    Standard(u8),
    /// 8-bit color (0-255).
    Extended(u8),
}

/// A color specification for a single UI element.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // Each bool is an independent ANSI SGR attribute flag
pub struct ColorSpec {
    /// Foreground color (None = terminal default).
    pub fg: Option<Color>,
    /// Background color (None = terminal default).
    pub bg: Option<Color>,
    /// Bold attribute.
    pub bold: bool,
    /// Underline attribute.
    pub underline: bool,
    /// Dim/faint attribute.
    pub dim: bool,
    /// Reverse video attribute.
    pub reverse: bool,
    /// Blink attribute.
    pub blink: bool,
    /// Italic attribute.
    pub italic: bool,
}

impl ColorSpec {
    /// Create a default (empty) color spec with no colors or attributes.
    #[must_use]
    pub fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            bold: false,
            underline: false,
            dim: false,
            reverse: false,
            blink: false,
            italic: false,
        }
    }

    /// Generate the ANSI SGR escape sequence for this color spec.
    ///
    /// Returns the full sequence including `ESC[` prefix and `m` suffix.
    /// Returns an empty string if no attributes or colors are set.
    #[must_use]
    pub fn to_sgr(&self) -> String {
        let mut codes: Vec<String> = Vec::new();

        if self.bold {
            codes.push("1".to_string());
        }
        if self.dim {
            codes.push("2".to_string());
        }
        if self.italic {
            codes.push("3".to_string());
        }
        if self.underline {
            codes.push("4".to_string());
        }
        if self.blink {
            codes.push("5".to_string());
        }
        if self.reverse {
            codes.push("7".to_string());
        }

        if let Some(fg) = self.fg {
            match fg {
                Color::Standard(n) if n < 8 => codes.push(format!("{}", 30 + u32::from(n))),
                Color::Standard(n) => codes.push(format!("{}", 90 + u32::from(n) - 8)),
                Color::Extended(n) => codes.push(format!("38;5;{n}")),
            }
        }

        if let Some(bg) = self.bg {
            match bg {
                Color::Standard(n) if n < 8 => codes.push(format!("{}", 40 + u32::from(n))),
                Color::Standard(n) => codes.push(format!("{}", 100 + u32::from(n) - 8)),
                Color::Extended(n) => codes.push(format!("48;5;{n}")),
            }
        }

        if codes.is_empty() {
            String::new()
        } else {
            format!("\x1b[{}m", codes.join(";"))
        }
    }

    /// Generate the ANSI SGR reset sequence.
    ///
    /// Returns `ESC[0m`.
    #[must_use]
    pub fn reset_sgr() -> &'static str {
        "\x1b[0m"
    }
}

impl Default for ColorSpec {
    fn default() -> Self {
        Self::new()
    }
}

/// Full color configuration for all UI elements.
#[derive(Debug, Clone)]
pub struct ColorConfig {
    specs: HashMap<ColorSelector, ColorSpec>,
}

impl ColorConfig {
    /// Create a color config with the default less color scheme.
    ///
    /// Sets up the standard defaults: bold cyan line numbers, standout search
    /// highlights and prompt, standout+bold errors.
    #[must_use]
    pub fn default_less() -> Self {
        let mut specs = HashMap::new();

        // N (line numbers): bold cyan (*6 -> bold, fg=6)
        specs.insert(
            ColorSelector::LineNumber,
            ColorSpec {
                bold: true,
                fg: Some(Color::Standard(6)),
                ..ColorSpec::new()
            },
        );

        // S (search): standout (s -> reverse video)
        specs.insert(
            ColorSelector::Search,
            ColorSpec {
                reverse: true,
                ..ColorSpec::new()
            },
        );

        // P (prompt): standout (s -> reverse video)
        specs.insert(
            ColorSelector::Prompt,
            ColorSpec {
                reverse: true,
                ..ColorSpec::new()
            },
        );

        // E (error): standout bold (sd -> reverse + bold)
        specs.insert(
            ColorSelector::Error,
            ColorSpec {
                reverse: true,
                bold: true,
                ..ColorSpec::new()
            },
        );

        Self { specs }
    }

    /// Parse a `-D` flag value and apply it to the config.
    ///
    /// Format: `xcolor` where `x` is the selector character and
    /// `color` is the color specification string.
    ///
    /// Color format: `[attr][fg][.bg]`
    /// - Attributes: `s`/`~` (standout/reverse), `u`/`_` (underline),
    ///   `d`/`*` (bold), `k`/`&` (blink)
    /// - Colors: single digit 0-7 (standard), or multi-digit 0-255 (extended)
    ///
    /// # Errors
    ///
    /// Returns a `DisplayError::InvalidColor` if the selector or color spec is invalid.
    pub fn parse_and_apply(&mut self, spec: &str) -> crate::Result<()> {
        if spec.is_empty() {
            return Err(DisplayError::InvalidColor(
                "empty specification".to_string(),
            ));
        }

        let mut chars = spec.chars();
        let selector_char = chars
            .next()
            .ok_or_else(|| DisplayError::InvalidColor("empty specification".to_string()))?;
        let selector = ColorSelector::from_char(selector_char)?;

        let remainder: String = chars.collect();
        let color_spec = parse_color_spec(&remainder)?;
        self.specs.insert(selector, color_spec);

        Ok(())
    }

    /// Get the color spec for a selector, falling back to defaults.
    ///
    /// Returns a default empty `ColorSpec` if no spec has been set for the selector.
    #[must_use]
    pub fn get(&self, selector: ColorSelector) -> &ColorSpec {
        static DEFAULT: ColorSpec = ColorSpec {
            fg: None,
            bg: None,
            bold: false,
            underline: false,
            dim: false,
            reverse: false,
            blink: false,
            italic: false,
        };
        self.specs.get(&selector).unwrap_or(&DEFAULT)
    }

    /// Set the color spec for a selector directly.
    pub fn set(&mut self, selector: ColorSelector, spec: ColorSpec) {
        self.specs.insert(selector, spec);
    }

    /// Get the SGR escape sequence for a selector.
    ///
    /// Returns `Some(sgr_string)` if the selector has a non-empty color spec,
    /// or `None` if no spec is set (or the spec produces no SGR codes).
    /// The caller should fall back to reverse video when `None` is returned.
    #[must_use]
    pub fn get_sgr(&self, selector: ColorSelector) -> Option<String> {
        self.specs
            .get(&selector)
            .map(ColorSpec::to_sgr)
            .filter(|s| !s.is_empty())
    }
}

/// Auto-detection for terminal color support.
///
/// Determines whether the output terminal supports ANSI color sequences
/// based on environment variables and terminal state.
pub struct ColorAutoDetect;

impl ColorAutoDetect {
    /// Detect whether color output should be enabled.
    ///
    /// Color is disabled when any of these conditions holds:
    /// - `NO_COLOR` environment variable is set (any value)
    /// - `TERM` is set to `dumb`
    /// - `force_disable` is `true` (from `--use-color=false`)
    ///
    /// When `force_enable` is `true` (from `--use-color=always`), color is
    /// always enabled regardless of other conditions.
    #[must_use]
    pub fn detect(force_enable: bool, force_disable: bool) -> bool {
        if force_disable {
            return false;
        }
        if force_enable {
            return true;
        }

        // NO_COLOR convention: https://no-color.org/
        if std::env::var_os("NO_COLOR").is_some() {
            return false;
        }

        // TERM=dumb means no color support
        if let Ok(term) = std::env::var("TERM") {
            if term == "dumb" {
                return false;
            }
        }

        true
    }
}

/// Parse the color/attribute portion of a `-D` spec (after the selector character).
///
/// Format: `[attrs...][fg_number][.bg_number]`
fn parse_color_spec(s: &str) -> crate::Result<ColorSpec> {
    let mut spec = ColorSpec::new();
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    // Parse attribute prefixes
    while pos < len {
        match bytes[pos] {
            b's' | b'~' => {
                spec.reverse = true;
                pos += 1;
            }
            b'u' | b'_' => {
                spec.underline = true;
                pos += 1;
            }
            b'd' | b'*' => {
                spec.bold = true;
                pos += 1;
            }
            b'k' | b'&' => {
                spec.blink = true;
                pos += 1;
            }
            _ => break,
        }
    }

    // Parse foreground color (digits before optional '.')
    let fg_start = pos;
    while pos < len && bytes[pos].is_ascii_digit() {
        pos += 1;
    }
    if pos > fg_start {
        let fg_str = &s[fg_start..pos];
        let fg_num: u16 = fg_str.parse().map_err(|_| {
            DisplayError::InvalidColor(format!("invalid foreground color: {fg_str}"))
        })?;
        if fg_num > 255 {
            return Err(DisplayError::InvalidColor(format!(
                "foreground color out of range: {fg_num}"
            )));
        }
        #[allow(clippy::cast_possible_truncation)] // Validated fg_num <= 255 above
        let fg_u8 = fg_num as u8;
        spec.fg = Some(if fg_num <= 7 {
            Color::Standard(fg_u8)
        } else {
            Color::Extended(fg_u8)
        });
    }

    // Parse optional background color after '.'
    if pos < len && bytes[pos] == b'.' {
        pos += 1; // skip the dot
        let bg_start = pos;
        while pos < len && bytes[pos].is_ascii_digit() {
            pos += 1;
        }
        if pos > bg_start {
            let bg_str = &s[bg_start..pos];
            let bg_num: u16 = bg_str.parse().map_err(|_| {
                DisplayError::InvalidColor(format!("invalid background color: {bg_str}"))
            })?;
            if bg_num > 255 {
                return Err(DisplayError::InvalidColor(format!(
                    "background color out of range: {bg_num}"
                )));
            }
            #[allow(clippy::cast_possible_truncation)] // Validated bg_num <= 255 above
            let bg_u8 = bg_num as u8;
            spec.bg = Some(if bg_num <= 7 {
                Color::Standard(bg_u8)
            } else {
                Color::Extended(bg_u8)
            });
        }
    }

    // If there are remaining unparsed characters, that's an error
    if pos < len {
        return Err(DisplayError::InvalidColor(format!(
            "unexpected characters in color spec: '{}'",
            &s[pos..]
        )));
    }

    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_color_spec_new_all_defaults() {
        let spec = ColorSpec::new();
        assert_eq!(spec.fg, None);
        assert_eq!(spec.bg, None);
        assert!(!spec.bold);
        assert!(!spec.underline);
        assert!(!spec.dim);
        assert!(!spec.reverse);
        assert!(!spec.blink);
        assert!(!spec.italic);
    }

    #[test]
    fn test_color_spec_to_sgr_empty_returns_empty() {
        let spec = ColorSpec::new();
        assert_eq!(spec.to_sgr(), "");
    }

    #[test]
    fn test_color_spec_to_sgr_bold_only() {
        let spec = ColorSpec {
            bold: true,
            ..ColorSpec::new()
        };
        assert_eq!(spec.to_sgr(), "\x1b[1m");
    }

    #[test]
    fn test_color_spec_to_sgr_fg_only() {
        let spec = ColorSpec {
            fg: Some(Color::Standard(1)),
            ..ColorSpec::new()
        };
        assert_eq!(spec.to_sgr(), "\x1b[31m");
    }

    #[test]
    fn test_color_spec_to_sgr_bg_only() {
        let spec = ColorSpec {
            bg: Some(Color::Standard(4)),
            ..ColorSpec::new()
        };
        assert_eq!(spec.to_sgr(), "\x1b[44m");
    }

    #[test]
    fn test_color_spec_to_sgr_fg_and_bg() {
        let spec = ColorSpec {
            fg: Some(Color::Standard(2)),
            bg: Some(Color::Standard(7)),
            ..ColorSpec::new()
        };
        assert_eq!(spec.to_sgr(), "\x1b[32;47m");
    }

    #[test]
    fn test_color_spec_to_sgr_all_attributes() {
        let spec = ColorSpec {
            bold: true,
            underline: true,
            reverse: true,
            fg: Some(Color::Standard(3)),
            bg: Some(Color::Standard(6)),
            ..ColorSpec::new()
        };
        let sgr = spec.to_sgr();
        assert_eq!(sgr, "\x1b[1;4;7;33;46m");
    }

    #[test]
    fn test_color_spec_to_sgr_extended_fg() {
        let spec = ColorSpec {
            fg: Some(Color::Extended(196)),
            ..ColorSpec::new()
        };
        assert_eq!(spec.to_sgr(), "\x1b[38;5;196m");
    }

    #[test]
    fn test_color_spec_to_sgr_extended_bg() {
        let spec = ColorSpec {
            bg: Some(Color::Extended(255)),
            ..ColorSpec::new()
        };
        assert_eq!(spec.to_sgr(), "\x1b[48;5;255m");
    }

    #[test]
    fn test_color_config_default_less_has_expected_selectors() {
        let config = ColorConfig::default_less();
        // Line numbers: bold + cyan
        let n = config.get(ColorSelector::LineNumber);
        assert!(n.bold);
        assert_eq!(n.fg, Some(Color::Standard(6)));

        // Search: reverse
        let s = config.get(ColorSelector::Search);
        assert!(s.reverse);

        // Prompt: reverse
        let p = config.get(ColorSelector::Prompt);
        assert!(p.reverse);

        // Error: reverse + bold
        let e = config.get(ColorSelector::Error);
        assert!(e.reverse);
        assert!(e.bold);
    }

    #[test]
    fn test_color_config_parse_and_apply_simple() {
        let mut config = ColorConfig::default_less();
        config.parse_and_apply("Ns5").unwrap();
        let n = config.get(ColorSelector::LineNumber);
        assert!(n.reverse); // 's' = standout/reverse
        assert_eq!(n.fg, Some(Color::Standard(5))); // 5 = magenta
    }

    #[test]
    fn test_color_config_parse_and_apply_fg_bg() {
        let mut config = ColorConfig::default_less();
        config.parse_and_apply("S2.3").unwrap();
        let s = config.get(ColorSelector::Search);
        assert_eq!(s.fg, Some(Color::Standard(2))); // green
        assert_eq!(s.bg, Some(Color::Standard(3))); // yellow
    }

    #[test]
    fn test_color_config_parse_and_apply_attribute_prefix() {
        let mut config = ColorConfig::default_less();
        config.parse_and_apply("P*_1").unwrap();
        let p = config.get(ColorSelector::Prompt);
        assert!(p.bold); // '*' = bold
        assert!(p.underline); // '_' = underline
        assert_eq!(p.fg, Some(Color::Standard(1))); // red
    }

    #[test]
    fn test_color_config_parse_and_apply_invalid_selector_returns_error() {
        let mut config = ColorConfig::default_less();
        let result = config.parse_and_apply("Z1");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("unknown selector character"));
    }

    #[test]
    fn test_color_config_parse_and_apply_empty_spec_returns_error() {
        let mut config = ColorConfig::default_less();
        let result = config.parse_and_apply("");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("empty specification"));
    }

    #[test]
    fn test_color_config_get_returns_set_value() {
        let mut config = ColorConfig::default_less();
        let spec = ColorSpec {
            bold: true,
            fg: Some(Color::Standard(3)),
            ..ColorSpec::new()
        };
        config.set(ColorSelector::Binary, spec.clone());
        assert_eq!(config.get(ColorSelector::Binary), &spec);
    }

    #[test]
    fn test_color_config_get_unset_returns_default() {
        let config = ColorConfig::default_less();
        let spec = config.get(ColorSelector::Binary);
        assert_eq!(spec, &ColorSpec::new());
    }

    #[test]
    fn test_color_spec_reset_sgr_returns_reset() {
        assert_eq!(ColorSpec::reset_sgr(), "\x1b[0m");
    }

    #[test]
    fn test_color_selector_from_char_all_valid() {
        let cases = [
            ('B', ColorSelector::Binary),
            ('C', ColorSelector::Control),
            ('E', ColorSelector::Error),
            ('H', ColorSelector::Header),
            ('M', ColorSelector::Mark),
            ('N', ColorSelector::LineNumber),
            ('P', ColorSelector::Prompt),
            ('R', ColorSelector::Rscroll),
            ('S', ColorSelector::Search),
            ('W', ColorSelector::Unread),
            ('d', ColorSelector::Bold),
            ('k', ColorSelector::Blink),
            ('s', ColorSelector::Standout),
            ('u', ColorSelector::Underline),
        ];
        for (ch, expected) in cases {
            let result = ColorSelector::from_char(ch).unwrap();
            assert_eq!(result, expected, "failed for selector char '{ch}'");
        }
    }

    #[test]
    fn test_color_config_parse_and_apply_eight_bit_color() {
        let mut config = ColorConfig::default_less();
        config.parse_and_apply("N128.200").unwrap();
        let n = config.get(ColorSelector::LineNumber);
        assert_eq!(n.fg, Some(Color::Extended(128)));
        assert_eq!(n.bg, Some(Color::Extended(200)));
    }

    // --- get_sgr tests ---

    #[test]
    fn test_color_config_get_sgr_returns_correct_string() {
        let config = ColorConfig::default_less();
        let sgr = config.get_sgr(ColorSelector::Search);
        assert_eq!(sgr, Some("\x1b[7m".to_string()));
    }

    #[test]
    fn test_color_config_get_sgr_unset_returns_none() {
        let config = ColorConfig::default_less();
        let sgr = config.get_sgr(ColorSelector::Binary);
        assert_eq!(sgr, None);
    }

    #[test]
    fn test_color_config_get_sgr_empty_spec_returns_none() {
        let mut config = ColorConfig::default_less();
        config.set(ColorSelector::Binary, ColorSpec::new());
        let sgr = config.get_sgr(ColorSelector::Binary);
        assert_eq!(sgr, None);
    }

    #[test]
    fn test_empty_color_config_uses_defaults_reverse_video() {
        // An empty ColorConfig (no -D flags) has no specs, so get_sgr returns None
        // for all selectors, which means callers fall back to reverse video.
        let config = ColorConfig {
            specs: HashMap::new(),
        };
        assert_eq!(config.get_sgr(ColorSelector::Search), None);
        assert_eq!(config.get_sgr(ColorSelector::Prompt), None);
        assert_eq!(config.get_sgr(ColorSelector::Error), None);
        assert_eq!(config.get_sgr(ColorSelector::LineNumber), None);
    }

    // --- ColorAutoDetect tests ---

    #[test]
    fn test_auto_detect_force_disable_returns_false() {
        assert!(!ColorAutoDetect::detect(false, true));
    }

    #[test]
    fn test_auto_detect_force_enable_returns_true() {
        assert!(ColorAutoDetect::detect(true, false));
    }

    #[test]
    fn test_auto_detect_force_disable_overrides_force_enable() {
        // force_disable is checked first
        assert!(!ColorAutoDetect::detect(true, true));
    }

    // NOTE: The following tests manipulate environment variables, which is
    // inherently not thread-safe. In the standard test runner these are safe
    // because each test function runs in isolation within the same thread.
    // They use force_enable=false, force_disable=false to exercise the
    // env-var detection path.

    #[test]
    fn test_auto_detect_no_color_env_disables_color() {
        // Save and set NO_COLOR
        let saved = std::env::var_os("NO_COLOR");
        std::env::set_var("NO_COLOR", "1");
        let result = ColorAutoDetect::detect(false, false);
        // Restore
        match saved {
            Some(v) => std::env::set_var("NO_COLOR", v),
            None => std::env::remove_var("NO_COLOR"),
        }
        assert!(!result);
    }

    #[test]
    fn test_auto_detect_term_dumb_disables_color() {
        // Save and set TERM
        let saved_term = std::env::var_os("TERM");
        let saved_no_color = std::env::var_os("NO_COLOR");
        // Ensure NO_COLOR is not set (it takes priority over TERM)
        std::env::remove_var("NO_COLOR");
        std::env::set_var("TERM", "dumb");
        let result = ColorAutoDetect::detect(false, false);
        // Restore
        match saved_term {
            Some(v) => std::env::set_var("TERM", v),
            None => std::env::remove_var("TERM"),
        }
        match saved_no_color {
            Some(v) => std::env::set_var("NO_COLOR", v),
            None => std::env::remove_var("NO_COLOR"),
        }
        assert!(!result);
    }
}
