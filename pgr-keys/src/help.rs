//! Help screen content for the `h` command.
//!
//! Defines a static help text documenting key bindings, displayed when the
//! user presses `h` or `H`.

/// Help text documenting the current key bindings.
///
/// Displayed as a navigable document when the user presses `h` or `H`.
/// Covers Phase 0 + Phase 1 bindings.
pub const HELP_TEXT: &str = "\
                      SUMMARY OF LESS COMMANDS (pgr)

      Commands marked with * may be preceded by a number, N.

  h  H                 Display this help.
  q  :q  Q  :Q  ZZ     Exit.

          MOVING

  e  ^E  j  ^N  CR  *  Forward  one line (or N lines).
  ^Y  k  ^K  ^P     *  Backward one line (or N lines).
  f  ^F  ^V  SPACE  *  Forward  one window (or N lines).
  b  ^B  ESC-v      *  Backward one window (or N lines).
  z                 *  Forward  one window (and set window to N).
  w                 *  Backward one window (and set window to N).
  ESC-SPACE         *  Forward  one window, but don't stop at end-of-file.
  d  ^D             *  Forward  one half-window (and set half-window to N).
  u  ^U             *  Backward one half-window (and set half-window to N).
  ESC-j             *  Forward  one file-line.
  ESC-k             *  Backward one file-line.
  J                 *  Forward  one line (force; past end-of-file).
  K                 *  Backward one line (force; past beginning).
  F                    Scroll forward; like tail -f.
  r  ^R  ^L            Repaint screen.
  R                    Repaint screen, discarding buffered input.

          NAVIGATION

  g  <  ESC-<       *  Go to first line (or line N).
  G  >  ESC->       *  Go to last line (or line N).
  p  %              *  Go to N percent into file.
  P                 *  Go to byte offset N.
  RIGHT-ARROW  ESC-)   Scroll right N characters (default: half screen).
  LEFT-ARROW   ESC-(   Scroll left  N characters (default: half screen).
  Ctrl+RIGHT   ESC-}   Scroll right to end of longest visible line.
  Ctrl+LEFT    ESC-{   Scroll left  to first column.

          MARKS

  m<letter>            Set a mark at the current top line.
  M<letter>            Set a mark at the current bottom line.
  '<letter>            Go to a previously set mark.
  ''                   Return to previous position.
  '^                   Go to the beginning of the file.
  '$                   Go to the end of the file.
  ^X^X<letter>         Same as '.
  ESC-m<letter>        Clear a mark.

          FILE COMMANDS

  :n                *  Examine the next file.
  :p                *  Examine the previous file.
  :x                *  Examine the first file (or file N).
  :d                   Remove the current file from the list.

          CLIPBOARD

  y                    Yank (copy) current line to clipboard.
  Y                    Yank (copy) all visible lines to clipboard.

          MISCELLANEOUS

  =  ^G  :f            Display file information.
  V                    Print version number of pgr.
  h  H                 Display this help.
  q  :q  Q  ^C         Exit.

";

/// Version string for the `V` command.
///
/// Format: `pgr version X.Y.Z (Rust regex engine)`.
#[must_use]
pub fn version_string() -> String {
    format!(
        "pgr version {} (Rust regex engine)",
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 10: Help text constant is non-empty and contains key binding documentation
    #[test]
    fn test_help_text_is_nonempty_and_contains_key_bindings() {
        assert!(!HELP_TEXT.is_empty());
        assert!(HELP_TEXT.contains("SUMMARY OF LESS COMMANDS"));
        assert!(HELP_TEXT.contains("Forward  one line"));
        assert!(HELP_TEXT.contains("Backward one line"));
        assert!(HELP_TEXT.contains("Display this help"));
        assert!(HELP_TEXT.contains("Exit"));
    }

    #[test]
    fn test_version_string_contains_version_number() {
        let v = version_string();
        assert!(v.starts_with("pgr version "));
        assert!(v.contains("Rust regex engine"));
        // Should contain at least one dot (semver)
        assert!(v.contains('.'));
    }
}
