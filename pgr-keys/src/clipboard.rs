//! Clipboard trait and platform-specific backends for yank commands.
//!
//! Supports OSC 52 (universal terminal clipboard), pbcopy (macOS),
//! xclip/xsel (X11), and wl-copy (Wayland). OSC 52 is the default
//! fallback since it requires no external tools and works over SSH.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::Result;
use crate::KeyError;

/// A clipboard backend that can copy text to the system clipboard.
pub trait Clipboard: Send {
    /// Copy the given text to the system clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error if the clipboard operation fails (e.g., the
    /// external tool is not available or the pipe breaks).
    fn copy(&self, text: &str) -> Result<()>;

    /// Returns the human-readable name of this clipboard backend.
    fn name(&self) -> &'static str;
}

/// The configured clipboard backend choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardBackend {
    /// Auto-detect the best available backend.
    Auto,
    /// OSC 52 terminal escape sequence (universal).
    Osc52,
    /// macOS `pbcopy` command.
    Pbcopy,
    /// X11 `xclip` command.
    Xclip,
    /// X11 `xsel` command.
    Xsel,
    /// Wayland `wl-copy` command.
    WlCopy,
    /// Clipboard disabled entirely.
    Off,
}

impl ClipboardBackend {
    /// Parse a backend name from a string (CLI flag or env var value).
    ///
    /// Returns `None` if the string is not a recognized backend name.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "osc52" => Some(Self::Osc52),
            "pbcopy" => Some(Self::Pbcopy),
            "xclip" => Some(Self::Xclip),
            "xsel" => Some(Self::Xsel),
            "wl-copy" | "wlcopy" => Some(Self::WlCopy),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// A disabled clipboard that does nothing.
pub struct DisabledClipboard;

impl Clipboard for DisabledClipboard {
    fn copy(&self, _text: &str) -> Result<()> {
        Ok(())
    }

    fn name(&self) -> &'static str {
        "disabled"
    }
}

/// OSC 52 clipboard backend — writes escape sequences to stdout.
///
/// This is the universal fallback that works in any terminal supporting
/// OSC 52, including over SSH sessions. The terminal intercepts the
/// sequence and copies to the system clipboard.
pub struct Osc52Clipboard;

impl Clipboard for Osc52Clipboard {
    fn copy(&self, text: &str) -> Result<()> {
        let encoded = base64_encode(text.as_bytes());
        let sequence = format!("\x1b]52;c;{encoded}\x07");
        let mut stdout = std::io::stdout().lock();
        stdout
            .write_all(sequence.as_bytes())
            .map_err(KeyError::Io)?;
        stdout.flush().map_err(KeyError::Io)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "osc52"
    }
}

/// Clipboard backend that pipes to an external command.
struct ExternalClipboard {
    command: &'static str,
    args: &'static [&'static str],
    display_name: &'static str,
}

impl Clipboard for ExternalClipboard {
    fn copy(&self, text: &str) -> Result<()> {
        let mut child = Command::new(self.command)
            .args(self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(KeyError::Io)?;

        if let Some(ref mut stdin) = child.stdin {
            stdin.write_all(text.as_bytes()).map_err(KeyError::Io)?;
        }

        child.wait().map_err(KeyError::Io)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        self.display_name
    }
}

/// Check whether an external command is available on `PATH`.
fn command_exists(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Detect the best available clipboard backend.
///
/// Tries backends in priority order: pbcopy (macOS), xclip (X11),
/// xsel (X11), wl-copy (Wayland), then falls back to OSC 52 which
/// is always available.
#[must_use]
pub fn detect_clipboard() -> Box<dyn Clipboard> {
    if command_exists("pbcopy") {
        return Box::new(ExternalClipboard {
            command: "pbcopy",
            args: &[],
            display_name: "pbcopy",
        });
    }
    if command_exists("xclip") {
        return Box::new(ExternalClipboard {
            command: "xclip",
            args: &["-selection", "clipboard"],
            display_name: "xclip",
        });
    }
    if command_exists("xsel") {
        return Box::new(ExternalClipboard {
            command: "xsel",
            args: &["--clipboard", "--input"],
            display_name: "xsel",
        });
    }
    if command_exists("wl-copy") {
        return Box::new(ExternalClipboard {
            command: "wl-copy",
            args: &[],
            display_name: "wl-copy",
        });
    }
    Box::new(Osc52Clipboard)
}

/// Create a clipboard backend from a specific backend choice.
///
/// For `Auto`, runs detection. For `Off`, returns a disabled clipboard.
/// For a specific backend, returns that backend without checking availability.
#[must_use]
pub fn create_clipboard(backend: ClipboardBackend) -> Box<dyn Clipboard> {
    match backend {
        ClipboardBackend::Auto => detect_clipboard(),
        ClipboardBackend::Osc52 => Box::new(Osc52Clipboard),
        ClipboardBackend::Pbcopy => Box::new(ExternalClipboard {
            command: "pbcopy",
            args: &[],
            display_name: "pbcopy",
        }),
        ClipboardBackend::Xclip => Box::new(ExternalClipboard {
            command: "xclip",
            args: &["-selection", "clipboard"],
            display_name: "xclip",
        }),
        ClipboardBackend::Xsel => Box::new(ExternalClipboard {
            command: "xsel",
            args: &["--clipboard", "--input"],
            display_name: "xsel",
        }),
        ClipboardBackend::WlCopy => Box::new(ExternalClipboard {
            command: "wl-copy",
            args: &[],
            display_name: "wl-copy",
        }),
        ClipboardBackend::Off => Box::new(DisabledClipboard),
    }
}

// ── Base64 encoding ──────────────────────────────────────────────────

/// Standard base64 alphabet.
const BASE64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode a byte slice as standard base64 with `=` padding.
#[must_use]
pub fn base64_encode(input: &[u8]) -> String {
    let mut result = String::with_capacity(input.len().div_ceil(3) * 4);
    let chunks = input.chunks(3);

    for chunk in chunks {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let n = u32::from(b0) << 16 | u32::from(b1) << 8 | u32::from(b2);

        result.push(BASE64_CHARS[(n >> 18 & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[(n >> 12 & 0x3F) as usize] as char);

        if chunk.len() > 1 {
            result.push(BASE64_CHARS[(n >> 6 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(BASE64_CHARS[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }

    result
}

/// Format text as an OSC 52 clipboard escape sequence.
#[must_use]
pub fn osc52_sequence(text: &str) -> String {
    let encoded = base64_encode(text.as_bytes());
    format!("\x1b]52;c;{encoded}\x07")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Base64 encoding tests ────────────────────────────────────────

    #[test]
    fn test_base64_encode_hello() {
        assert_eq!(base64_encode(b"hello"), "aGVsbG8=");
    }

    #[test]
    fn test_base64_encode_empty() {
        assert_eq!(base64_encode(b""), "");
    }

    #[test]
    fn test_base64_encode_single_byte() {
        assert_eq!(base64_encode(b"a"), "YQ==");
    }

    #[test]
    fn test_base64_encode_two_bytes() {
        assert_eq!(base64_encode(b"ab"), "YWI=");
    }

    #[test]
    fn test_base64_encode_three_bytes() {
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn test_base64_encode_longer_text() {
        assert_eq!(base64_encode(b"Hello, World!"), "SGVsbG8sIFdvcmxkIQ==");
    }

    #[test]
    fn test_base64_encode_all_zeros() {
        assert_eq!(base64_encode(&[0, 0, 0]), "AAAA");
    }

    #[test]
    fn test_base64_encode_all_ones() {
        assert_eq!(base64_encode(&[0xFF, 0xFF, 0xFF]), "////");
    }

    #[test]
    fn test_base64_encode_binary_data() {
        assert_eq!(base64_encode(&[0x00, 0x01, 0x02, 0x03]), "AAECAw==");
    }

    // ── OSC 52 format tests ──────────────────────────────────────────

    #[test]
    fn test_osc52_sequence_format() {
        let seq = osc52_sequence("hello");
        assert_eq!(seq, "\x1b]52;c;aGVsbG8=\x07");
    }

    #[test]
    fn test_osc52_sequence_empty() {
        let seq = osc52_sequence("");
        assert_eq!(seq, "\x1b]52;c;\x07");
    }

    // ── Clipboard detection tests ────────────────────────────────────

    #[test]
    fn test_detect_clipboard_returns_a_backend() {
        let clipboard = detect_clipboard();
        // On any platform, we should get at least OSC 52 as fallback.
        assert!(!clipboard.name().is_empty());
    }

    #[test]
    fn test_disabled_clipboard_name() {
        let clipboard = DisabledClipboard;
        assert_eq!(clipboard.name(), "disabled");
    }

    #[test]
    fn test_disabled_clipboard_copy_succeeds() {
        let clipboard = DisabledClipboard;
        assert!(clipboard.copy("test").is_ok());
    }

    #[test]
    fn test_osc52_clipboard_name() {
        let clipboard = Osc52Clipboard;
        assert_eq!(clipboard.name(), "osc52");
    }

    #[test]
    fn test_create_clipboard_off_returns_disabled() {
        let clipboard = create_clipboard(ClipboardBackend::Off);
        assert_eq!(clipboard.name(), "disabled");
    }

    #[test]
    fn test_create_clipboard_osc52_returns_osc52() {
        let clipboard = create_clipboard(ClipboardBackend::Osc52);
        assert_eq!(clipboard.name(), "osc52");
    }

    #[test]
    fn test_create_clipboard_auto_returns_something() {
        let clipboard = create_clipboard(ClipboardBackend::Auto);
        assert!(!clipboard.name().is_empty());
    }

    // ── ClipboardBackend::from_str tests ─────────────────────────────

    #[test]
    fn test_clipboard_backend_parse_auto() {
        assert_eq!(
            ClipboardBackend::parse("auto"),
            Some(ClipboardBackend::Auto)
        );
    }

    #[test]
    fn test_clipboard_backend_parse_osc52() {
        assert_eq!(
            ClipboardBackend::parse("osc52"),
            Some(ClipboardBackend::Osc52)
        );
    }

    #[test]
    fn test_clipboard_backend_parse_pbcopy() {
        assert_eq!(
            ClipboardBackend::parse("pbcopy"),
            Some(ClipboardBackend::Pbcopy)
        );
    }

    #[test]
    fn test_clipboard_backend_parse_xclip() {
        assert_eq!(
            ClipboardBackend::parse("xclip"),
            Some(ClipboardBackend::Xclip)
        );
    }

    #[test]
    fn test_clipboard_backend_parse_xsel() {
        assert_eq!(
            ClipboardBackend::parse("xsel"),
            Some(ClipboardBackend::Xsel)
        );
    }

    #[test]
    fn test_clipboard_backend_parse_wl_copy() {
        assert_eq!(
            ClipboardBackend::parse("wl-copy"),
            Some(ClipboardBackend::WlCopy)
        );
    }

    #[test]
    fn test_clipboard_backend_parse_wlcopy() {
        assert_eq!(
            ClipboardBackend::parse("wlcopy"),
            Some(ClipboardBackend::WlCopy)
        );
    }

    #[test]
    fn test_clipboard_backend_parse_off() {
        assert_eq!(ClipboardBackend::parse("off"), Some(ClipboardBackend::Off));
    }

    #[test]
    fn test_clipboard_backend_parse_invalid() {
        assert_eq!(ClipboardBackend::parse("invalid"), None);
    }

    #[test]
    fn test_clipboard_backend_parse_case_insensitive() {
        assert_eq!(
            ClipboardBackend::parse("OSC52"),
            Some(ClipboardBackend::Osc52)
        );
        assert_eq!(
            ClipboardBackend::parse("PBCOPY"),
            Some(ClipboardBackend::Pbcopy)
        );
        assert_eq!(ClipboardBackend::parse("Off"), Some(ClipboardBackend::Off));
    }
}
