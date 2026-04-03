#![warn(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)] // Visual tests use unwrap/expect freely
#![allow(dead_code)] // Harness helpers may not all be used by current tests

//! Visual smoke tests for Phase 3 features (content modes, syntax highlighting,
//! diff coloring, side-by-side rendering, hunk navigation, pipe input).
//!
//! These are PTY-based tests that spawn pgr in a pseudo-terminal and inspect
//! the rendered screen output. They are `#[ignore]` and run only in the slow
//! test suite (`cargo test -p pgr-cli --test visual -- --ignored`).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use expectrl::process::NonBlocking;
use expectrl::session::OsSession;
use expectrl::Expect;

// ── Harness ─────────────────────────────────────────────────────────────────

static BUILD_ONCE: Once = Once::new();

/// Standard terminal dimensions for visual tests.
const TEST_ROWS: usize = 24;
/// Standard terminal width for visual tests.
const TEST_COLS: usize = 80;
/// Settle duration for initial render.
///
/// Generous timeout to handle parallel PTY test contention.
const SETTLE_INITIAL: Duration = Duration::from_millis(1000);
/// Settle duration after keystrokes.
const SETTLE_KEY: Duration = Duration::from_millis(500);

/// Ensure the pgr-cli binary is built before any test runs.
fn ensure_binary_built() {
    BUILD_ONCE.call_once(|| {
        let status = Command::new("cargo")
            .args(["build", "-p", "pgr-cli"])
            .status()
            .expect("failed to run cargo build");
        assert!(status.success(), "cargo build -p pgr-cli failed");
    });
}

/// Return the workspace root directory.
fn workspace_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // go from pgr-cli/ to workspace root
    path
}

/// Return the path to the built pgr-cli binary.
fn binary_path() -> PathBuf {
    let mut path = workspace_root();
    path.push("target");
    path.push("debug");
    path.push("pgr-cli");
    path
}

/// A PTY-based pager session for visual testing.
///
/// Wraps an expectrl session with a vt100 parser for screen capture,
/// including raw (SGR-preserving) output access.
struct VisualSession {
    /// The underlying PTY session.
    session: OsSession,
    /// Virtual terminal parser for screen state extraction.
    parser: vt100::Parser,
    /// Terminal rows.
    rows: u16,
    /// Terminal columns.
    cols: u16,
}

#[allow(clippy::cast_possible_truncation)] // Terminal dimensions are always < u16::MAX
impl VisualSession {
    /// Spawn pgr with the given arguments and input file.
    fn spawn_pgr(args: &[&str], input_file: &str) -> Self {
        ensure_binary_built();
        let rows = TEST_ROWS as u16;
        let cols = TEST_COLS as u16;

        let bin = binary_path();
        let mut cmd = Command::new(&bin);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);

        Self::spawn_command(cmd, rows, cols)
    }

    /// Spawn pgr reading from a shell pipe (e.g., `echo ... | pgr`).
    ///
    /// The `producer` command's stdout is piped into pgr's stdin via a shell.
    fn spawn_pgr_piped(producer: &str, pgr_args: &[&str]) -> Self {
        ensure_binary_built();
        let rows = TEST_ROWS as u16;
        let cols = TEST_COLS as u16;

        let bin = binary_path();
        let pgr_flags = pgr_args.join(" ");
        let shell_cmd = format!("{producer} | {bin:?} {pgr_flags}");

        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(&shell_cmd);

        Self::spawn_command(cmd, rows, cols)
    }

    /// Internal: spawn a command in a PTY with the given dimensions.
    fn spawn_command(mut cmd: Command, rows: u16, cols: u16) -> Self {
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");
        cmd.env_remove("LESSSECURE");
        cmd.env_remove("LESSKEY");
        cmd.env("LESSHISTFILE", "-");
        cmd.env("LESSHISTSIZE", "0");

        let mut session =
            expectrl::Session::spawn(cmd).unwrap_or_else(|e| panic!("failed to spawn pgr: {e}"));
        session.set_expect_timeout(Some(Duration::from_secs(5)));

        session
            .get_process_mut()
            .set_window_size(cols, rows)
            .unwrap_or_else(|e| panic!("failed to set window size: {e}"));

        let parser = vt100::Parser::new(rows, cols, 0);

        Self {
            session,
            parser,
            rows,
            cols,
        }
    }

    /// Send a sequence of keystrokes to the pager.
    fn send_keys(&mut self, keys: &str) {
        self.session
            .send(keys)
            .unwrap_or_else(|e| panic!("failed to send keys: {e}"));
    }

    /// Send raw bytes (for control characters, escape sequences).
    fn send_bytes(&mut self, bytes: &[u8]) {
        self.session
            .send(bytes)
            .unwrap_or_else(|e| panic!("failed to send bytes: {e}"));
    }

    /// Wait for the pager to settle by reading all available output.
    fn settle(&mut self, timeout: Duration) {
        let start = std::time::Instant::now();
        let mut buf = [0u8; 4096];

        self.session
            .get_stream_mut()
            .set_blocking(false)
            .expect("failed to set non-blocking");

        loop {
            match self.session.get_stream_mut().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.parser.process(&buf[..n]);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() > timeout {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
            if start.elapsed() > timeout {
                break;
            }
        }

        self.session
            .get_stream_mut()
            .set_blocking(true)
            .expect("failed to restore blocking");
    }

    /// Get the plain-text screen content (all rows joined with newlines).
    fn screen_text(&self) -> String {
        let screen = self.parser.screen();
        let rows: Vec<String> = screen.rows(0, self.cols).collect();
        rows.join("\n")
    }

    /// Get the prompt/status line text (last row).
    fn prompt_line(&self) -> String {
        let screen = self.parser.screen();
        let rows: Vec<String> = screen.rows(0, self.cols).collect();
        rows.last().cloned().unwrap_or_default()
    }

    /// Get content lines (all rows except the last/prompt row) as plain text.
    fn content_lines(&self) -> Vec<String> {
        let screen = self.parser.screen();
        let rows: Vec<String> = screen.rows(0, self.cols).collect();
        if rows.len() > 1 {
            rows[..rows.len() - 1].to_vec()
        } else {
            Vec::new()
        }
    }

    /// Get the raw screen output with SGR escape sequences preserved.
    ///
    /// Uses `rows_formatted` from vt100, which re-emits the SGR codes
    /// needed to reproduce the screen's colors and attributes.
    fn screen_formatted(&self) -> String {
        let screen = self.parser.screen();
        let rows: Vec<Vec<u8>> = screen.rows_formatted(0, self.cols).collect();
        let mut output = String::new();
        for row in &rows {
            output.push_str(&String::from_utf8_lossy(row));
            output.push('\n');
        }
        output
    }

    /// Check whether any cell in the content area has a non-default foreground color.
    ///
    /// Useful for detecting whether syntax highlighting is active.
    fn has_colored_content(&self) -> bool {
        let screen = self.parser.screen();
        let content_rows = self.rows - 1; // exclude prompt row
        for row in 0..content_rows {
            for col in 0..self.cols {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.fgcolor() != vt100::Color::Default {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check whether any cell in the content area has a specific background color.
    fn has_bg_color(&self, r: u8, g: u8, b: u8) -> bool {
        let screen = self.parser.screen();
        let content_rows = self.rows - 1;
        for row in 0..content_rows {
            for col in 0..self.cols {
                if let Some(cell) = screen.cell(row, col) {
                    if cell.bgcolor() == vt100::Color::Rgb(r, g, b) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Send quit command and wait for the process to exit.
    fn quit(&mut self) {
        let _ = self.session.send("q");
        std::thread::sleep(Duration::from_millis(200));
    }
}

impl Drop for VisualSession {
    fn drop(&mut self) {
        let _ = self.session.send("q");
    }
}

/// Generate a test file with specific content and a given extension.
///
/// Returns the `NamedTempFile` (file is deleted when dropped).
fn generate_file(content: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("failed to write to temp file");
    file.flush().expect("failed to flush temp file");
    file
}

/// Generate a test file with a specific suffix (extension).
fn generate_file_with_suffix(content: &str, suffix: &str) -> tempfile::NamedTempFile {
    let file = tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("failed to create temp file");
    std::fs::write(file.path(), content).expect("failed to write to temp file");
    file
}

// ── Content mode detection tests ────────────────────────────────────────────

/// Verify that pgr detects unified diff content and shows `[diff mode]` in the
/// status line on first render.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_content_mode_diff_detected() {
    let diff_content = "\
diff --git a/foo.rs b/foo.rs
index 1234567..abcdefg 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
     let x = 1;
 }
";
    let file = generate_file(diff_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[diff mode]"),
        "expected '[diff mode]' in prompt, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that pgr detects JSON content and shows `[JSON]` in the status line.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_content_mode_json_detected() {
    let json_content = r#"{
    "name": "pgr",
    "version": "0.1.0",
    "features": ["syntax", "diff"]
}
"#;
    let file = generate_file(json_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[JSON]"),
        "expected '[JSON]' in prompt, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that pgr detects man page content (backspace overprinting) and shows
/// `[man page]` in the status line.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_content_mode_man_detected() {
    // Man page bold encoding: each character is printed, backspaced, then printed again.
    // "Hello" in bold = H\x08He\x08el\x08ll\x08lo\x08o
    let man_content = "H\x08He\x08el\x08ll\x08lo\x08o\n\
                        This is a man page description.\n\
                        More text follows here.\n";
    let file = generate_file(man_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[man page]"),
        "expected '[man page]' in prompt, got: {prompt:?}"
    );

    session.quit();
}

// ── Syntax highlighting tests ───────────────────────────────────────────────

/// Verify that syntax highlighting produces 24-bit SGR sequences for a `.rs` file
/// when `--syntax` is enabled.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_syntax_highlighting_produces_sgr() {
    let rust_content = "fn main() {\n    println!(\"hello world\");\n}\n";
    let file = generate_file_with_suffix(rust_content, ".rs");
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&["--syntax"], path);
    session.settle(SETTLE_INITIAL);

    let formatted = session.screen_formatted();
    // 24-bit color SGR sequences use the form: ESC[38;2;R;G;B m
    assert!(
        formatted.contains("38;2;"),
        "expected 24-bit SGR sequence (38;2;) in formatted output, got:\n{formatted}"
    );

    session.quit();
}

/// Verify that ESC-S toggles syntax highlighting off and back on.
///
/// 1. Open a `.rs` file with `--syntax` => colors present
/// 2. Send ESC-S => colors absent (or only basic)
/// 3. Send ESC-S again => colors return
#[test]
#[ignore = "visual: slow PTY test"]
fn test_syntax_highlighting_toggle() {
    let rust_content = "fn main() {\n    let x = 42;\n    println!(\"{x}\");\n}\n";
    let file = generate_file_with_suffix(rust_content, ".rs");
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&["--syntax"], path);
    session.settle(SETTLE_INITIAL);

    // Step 1: Verify syntax highlighting is active (colored content).
    assert!(
        session.has_colored_content(),
        "expected colored content with syntax highlighting enabled"
    );

    // Step 2: Toggle syntax off with ESC-S.
    session.send_bytes(&[0x1B, b'S']);
    session.settle(SETTLE_KEY);

    assert!(
        !session.has_colored_content(),
        "expected no colored content after toggling syntax off"
    );

    // Step 3: Toggle syntax back on with ESC-S.
    session.send_bytes(&[0x1B, b'S']);
    session.settle(SETTLE_KEY);

    assert!(
        session.has_colored_content(),
        "expected colored content after toggling syntax back on"
    );

    session.quit();
}

// ── Diff coloring tests ─────────────────────────────────────────────────────

/// Verify that added lines in a diff get a green background tint (48;2;30;60;30).
#[test]
#[ignore = "visual: slow PTY test"]
fn test_diff_coloring_green_background() {
    let diff_content = "\
diff --git a/foo.rs b/foo.rs
index 1234567..abcdefg 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,5 @@
 fn main() {
+    println!(\"added line 1\");
+    println!(\"added line 2\");
     let x = 1;
 }
";
    let file = generate_file(diff_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Check for the green background (RGB 30,60,30) on added lines.
    assert!(
        session.has_bg_color(30, 60, 30),
        "expected green background (30,60,30) on added diff lines"
    );

    session.quit();
}

/// Verify that removed lines in a diff get a red background tint (48;2;60;30;30).
#[test]
#[ignore = "visual: slow PTY test"]
fn test_diff_coloring_red_background() {
    let diff_content = "\
diff --git a/foo.rs b/foo.rs
index 1234567..abcdefg 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,5 +1,3 @@
 fn main() {
-    println!(\"removed line 1\");
-    println!(\"removed line 2\");
     let x = 1;
 }
";
    let file = generate_file(diff_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Check for the red background (RGB 60,30,30) on removed lines.
    assert!(
        session.has_bg_color(60, 30, 30),
        "expected red background (60,30,30) on removed diff lines"
    );

    session.quit();
}

// ── Side-by-side diff test ──────────────────────────────────────────────────

/// Verify that toggling side-by-side mode (ESC-V) adds the `│` separator character.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_side_by_side_shows_separator() {
    let diff_content = "\
diff --git a/foo.rs b/foo.rs
index 1234567..abcdefg 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"hello\");
     let x = 1;
 }
";
    let file = generate_file(diff_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Toggle side-by-side mode with ESC-V.
    session.send_bytes(&[0x1B, b'V']);
    session.settle(SETTLE_KEY);

    let text = session.screen_text();
    assert!(
        text.contains('\u{2502}'),
        "expected Unicode box separator (U+2502) in side-by-side output, got:\n{text}"
    );

    session.quit();
}

// ── Pipe input test ─────────────────────────────────────────────────────────

/// Verify that pgr shows content when reading from a pipe (stdin).
#[test]
#[ignore = "visual: slow PTY test"]
fn test_pipe_input_shows_content() {
    let mut session =
        VisualSession::spawn_pgr_piped("echo 'hello from pipe\nsecond line\nthird line'", &[]);
    session.settle(SETTLE_INITIAL);

    let text = session.screen_text();
    assert!(
        text.contains("hello from pipe"),
        "expected piped content 'hello from pipe' on screen, got:\n{text}"
    );

    session.quit();
}

// ── Hunk navigation test ───────────────────────────────────────────────────

/// Verify that `]c` navigates to the next hunk header in a multi-hunk diff.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_hunk_navigation() {
    // Create a diff with two hunks separated by enough context that the second
    // hunk is not visible on the initial screen.
    let mut diff_content = String::from(
        "\
diff --git a/foo.rs b/foo.rs
index 1234567..abcdefg 100644
--- a/foo.rs
+++ b/foo.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!(\"first hunk\");
     let x = 1;
 }
",
    );
    // Add a large block of context lines, then a second hunk.
    for i in 0..40 {
        diff_content.push_str(&format!(" context line {i}\n"));
    }
    diff_content.push_str(
        "\
@@ -50,3 +51,4 @@
 fn other() {
+    println!(\"second hunk\");
     let y = 2;
 }
",
    );

    let file = generate_file(&diff_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Verify first hunk is visible initially.
    let initial_text = session.screen_text();
    assert!(
        initial_text.contains("first hunk"),
        "expected 'first hunk' visible initially, got:\n{initial_text}"
    );

    // Navigate to the next hunk with `]c`.
    // `]` is a bracket prefix — the dispatch reads `]` then waits for the next char.
    // The first `]c` may land on hunk 1's header (if it's after top_line=0).
    // Send `]c` twice to ensure we reach hunk 2.
    session.send_keys("]c");
    session.settle(SETTLE_KEY);
    session.send_keys("]c");
    session.settle(SETTLE_KEY);

    let after_nav = session.screen_text();
    assert!(
        after_nav.contains("second hunk"),
        "expected 'second hunk' visible after ]c navigation, got:\n{after_nav}"
    );

    session.quit();
}

// ── Search preserves syntax color test ──────────────────────────────────────

/// Verify that searching for a pattern preserves syntax highlighting on non-match text.
///
/// Opens a `.rs` file with syntax highlighting, searches for `fn`, then verifies
/// that colored (highlighted) content still exists on screen.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_search_preserves_syntax_color() {
    let rust_content = "\
fn main() {
    let x = 42;
    println!(\"{x}\");
    let y = x + 1;
    println!(\"{y}\");
}
";
    let file = generate_file_with_suffix(rust_content, ".rs");
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&["--syntax"], path);
    session.settle(SETTLE_INITIAL);

    // Verify syntax highlighting is active before search.
    assert!(
        session.has_colored_content(),
        "expected colored content before search"
    );

    // Search for "fn".
    session.send_keys("/fn\n");
    session.settle(SETTLE_KEY);

    // Syntax highlighting should still be present on non-match text (let, println, etc.).
    assert!(
        session.has_colored_content(),
        "expected syntax highlighting preserved after search"
    );

    session.quit();
}
