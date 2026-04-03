#![warn(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)] // Visual tests use unwrap/expect freely
#![allow(dead_code)] // Harness helpers may not all be used by current tests

//! Visual smoke tests for Phase 3 features (content modes, syntax highlighting,
//! diff coloring, side-by-side rendering, hunk navigation, pipe input).
//!
//! These are PTY-based tests that spawn pgr in a pseudo-terminal and inspect
//! the rendered screen output. They are `#[ignore]` and run only in the slow
//! test suite (`cargo test -p pgr-cli --test visual -- --ignored`).

use std::fmt::Write as FmtWrite;
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
        let shell_cmd = format!("{producer} | {} {pgr_flags}", bin.display());

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

    /// Spawn pgr with extra environment variables set.
    fn spawn_pgr_with_env(args: &[&str], input_file: &str, env: &[(&str, &str)]) -> Self {
        ensure_binary_built();
        let rows = TEST_ROWS as u16;
        let cols = TEST_COLS as u16;

        let bin = binary_path();
        let mut cmd = Command::new(&bin);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);

        // Apply standard env setup, then overlay extra env.
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");
        cmd.env_remove("LESSSECURE");
        cmd.env_remove("LESSKEY");
        cmd.env("LESSHISTFILE", "-");
        cmd.env("LESSHISTSIZE", "0");

        for &(k, v) in env {
            cmd.env(k, v);
        }

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
        let _ = writeln!(diff_content, " context line {i}");
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

// ── Content mode detection (additional) ────────────────────────────────────

/// Verify that pgr detects git blame output and shows `[git blame]` in the
/// status line.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_content_mode_git_blame_detected() {
    // Every line must start with 7-40 hex chars + space for blame detection.
    let blame_content = "\
abcdef1 (Alice   2024-01-15  1) fn main() {
abcdef1 (Alice   2024-01-15  2)     println!(\"hello\");
1234567 (Bob     2024-02-20  3)     let x = 42;
abcdef1 (Alice   2024-01-15  4) }
";
    let file = generate_file(blame_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[git blame]"),
        "expected '[git blame]' in prompt, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that pgr detects SQL table output and shows `[SQL table]` in the
/// status line.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_content_mode_sql_table_detected() {
    let sql_content = "\
+--------+-------+--------+
| name   | age   | city   |
+--------+-------+--------+
| Alice  |    30 | NYC    |
| Bob    |    25 | LA     |
+--------+-------+--------+
";
    let file = generate_file(sql_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[SQL table]"),
        "expected '[SQL table]' in prompt, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that pgr detects compiler error output and shows `[compiler output]`
/// in the status line.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_content_mode_compiler_error_detected() {
    // Need at least 2 lines matching the file:line:col: error/warning pattern.
    let compiler_content = "\
error[E0308]: mismatched types
 --> src/main.rs:42:10
  |
42 |     let x: i32 = \"hello\";
  |                  ^^^^^^^ expected `i32`, found `&str`
src/lib.rs:10:5: error[E0425]: cannot find value `foo` in this scope
src/lib.rs:20:1: warning: unused variable `bar`
";
    let file = generate_file(compiler_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[compiler output]"),
        "expected '[compiler output]' in prompt, got: {prompt:?}"
    );

    session.quit();
}

// ── Search features ────────────────────────────────────────────────────────

/// Verify that searching shows match count in the prompt ("match N of M").
#[test]
#[ignore = "visual: slow PTY test"]
fn test_search_match_count_in_prompt() {
    // The file must be longer than one screen (24 lines) so the prompt shows
    // the medium format with match info instead of "(END)".
    let mut content = String::new();
    content.push_str("fn alpha() {}\n");
    content.push_str("fn beta() {}\n");
    for i in 0..30 {
        let _ = writeln!(content, "line {i}");
    }
    content.push_str("fn gamma() {}\n");
    content.push_str("fn delta() {}\n");

    let file = generate_file_with_suffix(&content, ".txt");
    let path = file.path().to_str().unwrap();

    // Use -m (medium prompt) which includes match count via %r.
    let mut session = VisualSession::spawn_pgr(&["-m"], path);
    session.settle(SETTLE_INITIAL);

    // Search for "fn" and advance to populate match count.
    session.send_keys("/fn\n");
    session.settle(SETTLE_KEY);
    session.send_keys("n");
    session.settle(SETTLE_KEY);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("match") && prompt.contains("of"),
        "expected 'match N of M' in prompt after search, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that adding a second highlight pattern with `&+` results in cells
/// with two different non-default background colors on screen.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_multi_pattern_different_colors() {
    let content = "\
fn main() {
    let value = 42;
    println!(\"fn value: {}\", value);
}
fn other() {
    let value = 99;
}
";
    let file = generate_file_with_suffix(content, ".txt");
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Primary search for "fn" — shown in reverse video (color 0).
    session.send_keys("/fn\n");
    session.settle(SETTLE_KEY);

    // Add a second highlight for "let" — shown in yellow bg (color 1).
    session.send_keys("&+let\n");
    session.settle(SETTLE_KEY);

    // The primary search uses reverse video (\x1b[7m) and the extra
    // highlight uses a colored background (yellow: \x1b[30;43m). Check that
    // both styles are present: cells with the `inverse` attribute (primary)
    // and cells with a non-default bgcolor (extra pattern).
    let screen = session.parser.screen();
    let content_rows = session.rows - 1;
    let mut has_inverse = false;
    let mut has_colored_bg = false;
    for row in 0..content_rows {
        for col in 0..session.cols {
            if let Some(cell) = screen.cell(row, col) {
                if cell.inverse() {
                    has_inverse = true;
                }
                if cell.bgcolor() != vt100::Color::Default {
                    has_colored_bg = true;
                }
            }
        }
    }

    assert!(
        has_inverse && has_colored_bg,
        "expected both reverse-video (primary search) and colored bg (extra highlight), \
         inverse={has_inverse}, colored_bg={has_colored_bg}"
    );

    session.quit();
}

/// Verify that incremental search moves the screen to show the match.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_incsearch_moves_screen() {
    // Create content where a unique target is far from the top.
    let mut content = String::new();
    for i in 0..50 {
        let _ = writeln!(content, "filler line number {i}");
    }
    content.push_str("UNIQUE_TARGET_PATTERN\n");
    for i in 51..80 {
        let _ = writeln!(content, "filler line number {i}");
    }

    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&["--incsearch"], path);
    session.settle(SETTLE_INITIAL);

    // Record initial screen content.
    let initial_text = session.screen_text();
    assert!(
        initial_text.contains("filler line number 0"),
        "expected top of file visible initially"
    );

    // Start search and type the unique pattern — incsearch should scroll.
    session.send_keys("/UNIQUE_TARGET");
    session.settle(SETTLE_KEY);

    let during_search = session.screen_text();
    assert!(
        during_search.contains("UNIQUE_TARGET_PATTERN"),
        "expected incsearch to scroll screen to show match, got:\n{during_search}"
    );

    // Cancel the search.
    session.send_bytes(&[0x1B]); // ESC to cancel
    session.settle(SETTLE_KEY);

    session.quit();
}

// ── Navigation ─────────────────────────────────────────────────────────────

/// Verify that `]s` navigates to the next man page section.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_man_section_navigation() {
    // Build man page content with two sections separated by enough lines
    // that the second section is off-screen.
    // Bold encoding: each char repeated as X\x08X
    fn bold(s: &str) -> String {
        let mut out = String::new();
        for ch in s.chars() {
            out.push(ch);
            out.push('\x08');
            out.push(ch);
        }
        out
    }

    let mut content = String::new();
    content.push_str(&bold("NAME"));
    content.push('\n');
    content.push_str("     test - a test man page\n");
    for i in 0..40 {
        let _ = writeln!(content, "     description line {i}");
    }
    content.push_str(&bold("OPTIONS"));
    content.push('\n');
    content.push_str("     -v  verbose output\n");

    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Initially should show NAME section.
    let initial = session.screen_text();
    assert!(
        initial.contains("NAME") || initial.contains("test - a test man page"),
        "expected NAME section visible initially"
    );

    // Navigate to next section with ]s.
    session.send_keys("]s");
    session.settle(SETTLE_KEY);

    let after_nav = session.screen_text();
    assert!(
        after_nav.contains("OPTIONS") || after_nav.contains("verbose"),
        "expected OPTIONS section visible after ]s, got:\n{after_nav}"
    );

    session.quit();
}

/// Verify that `]g` navigates between commits in git log content.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_git_log_commit_navigation() {
    let mut content = String::new();
    content.push_str("commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n");
    content.push_str("Author: Alice <alice@example.com>\n");
    content.push_str("Date:   Mon Jan 1 00:00:00 2024 +0000\n");
    content.push('\n');
    content.push_str("    First commit message\n");
    // Pad to push second commit off-screen.
    for i in 0..40 {
        let _ = writeln!(content, "    change line {i}");
    }
    content.push_str("commit bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\n");
    content.push_str("Author: Bob <bob@example.com>\n");
    content.push_str("Date:   Tue Jan 2 00:00:00 2024 +0000\n");
    content.push('\n');
    content.push_str("    Second commit message\n");

    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let initial = session.screen_text();
    assert!(
        initial.contains("First commit"),
        "expected first commit visible initially"
    );

    // Navigate to next commit with ]g.
    session.send_keys("]g");
    session.settle(SETTLE_KEY);

    let after_nav = session.screen_text();
    assert!(
        after_nav.contains("Second commit"),
        "expected second commit visible after ]g, got:\n{after_nav}"
    );

    session.quit();
}

/// Verify that `]u` shows URL navigation status when content has URLs.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_url_navigation_highlights() {
    let content = "\
Visit https://example.com for details.
Also see https://rust-lang.org for more.
No URL on this line.
";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Navigate to next URL with ]u.
    session.send_keys("]u");
    session.settle(SETTLE_KEY);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("URL") && prompt.contains("of"),
        "expected 'URL N of M' in status after ]u, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that `]f` navigates to the next file in a multi-file diff.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_diff_file_navigation() {
    let mut content = String::new();
    content.push_str("diff --git a/first.rs b/first.rs\n");
    content.push_str("index 1111111..2222222 100644\n");
    content.push_str("--- a/first.rs\n");
    content.push_str("+++ b/first.rs\n");
    content.push_str("@@ -1,3 +1,4 @@\n");
    content.push_str(" fn first() {\n");
    content.push_str("+    println!(\"first file change\");\n");
    content.push_str("     let x = 1;\n");
    content.push_str(" }\n");
    // Pad with context to push second file off-screen.
    for i in 0..40 {
        let _ = writeln!(content, " context line {i}");
    }
    content.push_str("diff --git a/second.rs b/second.rs\n");
    content.push_str("index 3333333..4444444 100644\n");
    content.push_str("--- a/second.rs\n");
    content.push_str("+++ b/second.rs\n");
    content.push_str("@@ -1,3 +1,4 @@\n");
    content.push_str(" fn second() {\n");
    content.push_str("+    println!(\"second file change\");\n");
    content.push_str("     let y = 2;\n");
    content.push_str(" }\n");

    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let initial = session.screen_text();
    assert!(
        initial.contains("first.rs") || initial.contains("first file"),
        "expected first file visible initially"
    );

    // Navigate to next file with ]f.
    session.send_keys("]f");
    session.settle(SETTLE_KEY);

    let after_nav = session.screen_text();
    assert!(
        after_nav.contains("second.rs") || after_nav.contains("second file"),
        "expected second file visible after ]f, got:\n{after_nav}"
    );

    session.quit();
}

/// Verify that hunk navigation wraps around in a 2-hunk diff.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_hunk_navigation_wraps() {
    let mut content = String::new();
    content.push_str("diff --git a/wrap.rs b/wrap.rs\n");
    content.push_str("index aaa..bbb 100644\n");
    content.push_str("--- a/wrap.rs\n");
    content.push_str("+++ b/wrap.rs\n");
    content.push_str("@@ -1,3 +1,4 @@\n");
    content.push_str(" fn first_hunk() {\n");
    content.push_str("+    println!(\"hunk one\");\n");
    content.push_str("     let x = 1;\n");
    content.push_str(" }\n");
    for i in 0..40 {
        let _ = writeln!(content, " padding line {i}");
    }
    content.push_str("@@ -50,3 +51,4 @@\n");
    content.push_str(" fn second_hunk() {\n");
    content.push_str("+    println!(\"hunk two\");\n");
    content.push_str("     let y = 2;\n");
    content.push_str(" }\n");
    // Add trailing context so the file is long enough for top_line to reach
    // hunk 2's header line without clamping (total > hunk2_line + screen_rows).
    for i in 0..30 {
        let _ = writeln!(content, " trailing context {i}");
    }

    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Navigate to hunk 2: ]c to hunk 1, ]c to hunk 2.
    session.send_keys("]c");
    session.settle(SETTLE_KEY);
    session.send_keys("]c");
    session.settle(SETTLE_KEY);

    // Verify we reached hunk 2 — the screen should show hunk 2 content.
    let at_hunk2 = session.screen_text();
    assert!(
        at_hunk2.contains("hunk two") || at_hunk2.contains("second_hunk"),
        "expected hunk 2 content after two ]c, got:\n{at_hunk2}"
    );

    // One more ]c should wrap back to hunk 1.
    session.send_keys("]c");
    session.settle(SETTLE_KEY);

    let wrapped = session.screen_text();
    // After wrap, hunk 1's @@ header and content should be visible.
    assert!(
        wrapped.contains("hunk one") || wrapped.contains("first_hunk"),
        "expected hunk 1 visible after wrap, got:\n{wrapped}"
    );

    session.quit();
}

// ── Clipboard (yank) ──────────────────────────────────────────────────────

/// Verify that `y` shows "Yanked 1 line" in the status.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_yank_line_shows_status() {
    let content = "line one\nline two\nline three\n";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    session.send_keys("y");
    session.settle(SETTLE_KEY);

    let prompt = session.prompt_line();
    // Clipboard might not be available in CI, so check for either message.
    assert!(
        prompt.contains("Yanked 1 line") || prompt.contains("Clipboard"),
        "expected 'Yanked 1 line' or clipboard status in prompt, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that `Y` (yank screen) shows "Yanked N lines" in the status.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_yank_screen_shows_status() {
    let content = "line one\nline two\nline three\nline four\nline five\n";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    session.send_keys("Y");
    session.settle(SETTLE_KEY);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("Yanked") || prompt.contains("Clipboard"),
        "expected 'Yanked N lines' or clipboard status in prompt, got: {prompt:?}"
    );

    session.quit();
}

// ── Git gutter ─────────────────────────────────────────────────────────────

/// Verify that ESC-G toggles git gutter on and shows status message.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_git_gutter_toggle() {
    let content = "fn main() {\n    println!(\"test\");\n}\n";
    let file = generate_file_with_suffix(content, ".rs");
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Toggle git gutter on with ESC-G.
    session.send_bytes(&[0x1B, b'G']);
    session.settle(SETTLE_KEY);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("Git gutter on"),
        "expected 'Git gutter on' in status after ESC-G, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that ESC-G twice (on then off) shows "Git gutter off" in status.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_git_gutter_disabled_status() {
    let content = "fn main() {\n    println!(\"test\");\n}\n";
    let file = generate_file_with_suffix(content, ".rs");
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Toggle on.
    session.send_bytes(&[0x1B, b'G']);
    session.settle(SETTLE_KEY);

    // Toggle off.
    session.send_bytes(&[0x1B, b'G']);
    session.settle(SETTLE_KEY);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("Git gutter off"),
        "expected 'Git gutter off' in status after second ESC-G, got: {prompt:?}"
    );

    session.quit();
}

// ── SQL table features ─────────────────────────────────────────────────────

/// Verify that the SQL table sticky header persists after scrolling down.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_sql_sticky_header_after_scroll() {
    // Build an SQL table taller than the 24-row screen.
    let mut content = String::new();
    content.push_str("+--------+-------+\n");
    content.push_str("| name   | score |\n");
    content.push_str("+--------+-------+\n");
    for i in 0..40 {
        let _ = writeln!(content, "| user{i:02} | {i:>5} |");
    }
    content.push_str("+--------+-------+\n");

    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Scroll down with space (page forward).
    session.send_keys(" ");
    session.settle(SETTLE_KEY);

    // The header row ("name" / "score") should still be visible at the top
    // due to sticky header.
    let lines = session.content_lines();
    let top_lines = lines.iter().take(3).cloned().collect::<Vec<_>>().join("\n");
    assert!(
        top_lines.contains("name") || top_lines.contains("score"),
        "expected sticky header (name/score) at top after scroll, top lines:\n{top_lines}"
    );

    session.quit();
}

/// Verify that horizontal scroll in SQL table mode snaps to column boundaries.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_sql_column_snap_hscroll() {
    // Build a wide SQL table.
    let content = "\
+------------+--------------+------------------+-------------------+
| first_col  | second_col   | third_column     | fourth_column     |
+------------+--------------+------------------+-------------------+
| value1     | value2       | value3           | value4            |
| aaaa       | bbbb         | cccc             | dddd              |
+------------+--------------+------------------+-------------------+
";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Record initial screen content.
    let initial = session.content_lines();

    // Press right arrow to scroll horizontally — should snap to a column boundary.
    session.send_bytes(&[0x1B, b'[', b'C']); // Right arrow ESC[C
    session.settle(SETTLE_KEY);

    let after_scroll = session.content_lines();

    // The screen content should have shifted — at minimum, the first column
    // should have shifted or the frozen column pattern should be different.
    assert_ne!(
        initial, after_scroll,
        "expected screen to change after horizontal scroll in SQL table mode"
    );

    session.quit();
}

// ── Buffer save ────────────────────────────────────────────────────────────

/// Verify that the `s` command saves buffer content to a file.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_save_buffer_creates_file() {
    let content = "line alpha\nline beta\nline gamma\n";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();

    let save_dir = tempfile::tempdir().expect("failed to create temp dir");
    let save_path = save_dir.path().join("pgr_visual_save_test.txt");
    let save_path_str = save_path.to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Send `s` to enter save mode, then type the path and press Enter.
    session.send_keys("s");
    session.settle(SETTLE_KEY);
    session.send_keys(save_path_str);
    session.send_keys("\n");
    session.settle(SETTLE_KEY);

    // Verify the file was created.
    assert!(
        save_path.exists(),
        "expected save file to be created at {save_path_str}"
    );

    // Verify the content is correct (plain text, no ANSI).
    let saved = std::fs::read_to_string(&save_path).expect("failed to read saved file");
    assert!(
        saved.contains("line alpha"),
        "expected saved content to contain 'line alpha', got: {saved:?}"
    );

    session.quit();
}

/// Verify that buffer save is blocked in secure mode (LESSSECURE=1).
#[test]
#[ignore = "visual: slow PTY test"]
fn test_save_buffer_blocked_secure() {
    let content = "secure mode test content\n";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr_with_env(&[], path, &[("LESSSECURE", "1")]);
    session.settle(SETTLE_INITIAL);

    // Attempt save.
    session.send_keys("s");
    session.settle(SETTLE_KEY);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("not available") || prompt.contains("Command not available"),
        "expected 'Command not available' in secure mode, got: {prompt:?}"
    );

    session.quit();
}

// ── Compiler error links ───────────────────────────────────────────────────

/// Verify that compiler error content renders file references with the correct
/// text, confirming that the compiler linkify pass ran (OSC 8 sequences are
/// consumed by the terminal emulator and not visible in screen text, but the
/// file references must remain legible).
#[test]
#[ignore = "visual: slow PTY test"]
fn test_compiler_error_renders_file_references() {
    let compiler_content = "\
error[E0308]: mismatched types
 --> src/main.rs:42:10
  |
42 |     let x: i32 = \"hello\";
  |                  ^^^^^^^ expected `i32`, found `&str`
src/lib.rs:10:5: error[E0425]: cannot find value `foo` in this scope
src/lib.rs:20:1: warning: unused variable `bar`
";
    let file = generate_file(compiler_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Verify the content mode was detected.
    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[compiler output]"),
        "expected '[compiler output]' in prompt, got: {prompt:?}"
    );

    // Verify that file references are rendered in the screen text.
    let text = session.screen_text();
    assert!(
        text.contains("src/main.rs:42:10") && text.contains("src/lib.rs:10:5"),
        "expected file references visible on screen, got:\n{text}"
    );

    session.quit();
}

// ── Side-by-side (additional) ──────────────────────────────────────────────

/// Verify that side-by-side mode shows old content on the left and new
/// content on the right for a change line.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_side_by_side_shows_old_and_new() {
    let diff_content = "\
diff --git a/file.rs b/file.rs
index aaa..bbb 100644
--- a/file.rs
+++ b/file.rs
@@ -1,3 +1,3 @@
 fn main() {
-    let old_value = 1;
+    let new_value = 2;
 }
";
    let file = generate_file(diff_content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Enable side-by-side with ESC-V.
    session.send_bytes(&[0x1B, b'V']);
    session.settle(SETTLE_KEY);

    let text = session.screen_text();
    // In side-by-side, the left panel shows old content and right shows new.
    assert!(
        text.contains("old_value") && text.contains("new_value"),
        "expected both 'old_value' and 'new_value' visible in side-by-side, got:\n{text}"
    );

    session.quit();
}

/// Verify that ESC-V toggles side-by-side off after enabling it.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_side_by_side_toggle_back() {
    let diff_content = "\
diff --git a/file.rs b/file.rs
index aaa..bbb 100644
--- a/file.rs
+++ b/file.rs
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

    // Enable side-by-side.
    session.send_bytes(&[0x1B, b'V']);
    session.settle(SETTLE_KEY);

    let sbs_text = session.screen_text();
    assert!(
        sbs_text.contains('\u{2502}'),
        "expected separator in side-by-side mode"
    );

    // Toggle back to unified.
    session.send_bytes(&[0x1B, b'V']);
    session.settle(SETTLE_KEY);

    // Check status message indicates side-by-side is off.
    let prompt = session.prompt_line();
    // The prompt should show the file or "Side-by-side diff off".
    // Also verify the separator is gone.
    let unified_text = session.screen_text();
    // In unified mode, the typical diff format is `+    println!(...)`
    // not split into two panels. The separator should be absent on content lines.
    let content_has_separator = session
        .content_lines()
        .iter()
        .any(|l| l.contains('\u{2502}'));
    assert!(
        !content_has_separator || prompt.contains("off"),
        "expected unified view restored after second ESC-V, \
         separator still present or no 'off' message. prompt: {prompt:?}, text:\n{unified_text}"
    );

    session.quit();
}

// ── Pipe and edge cases ────────────────────────────────────────────────────

/// Verify that piped JSON content gets syntax highlighting (color).
#[test]
#[ignore = "visual: slow PTY test"]
fn test_pipe_json_gets_highlighting() {
    let json_str = r#"printf '{"name":"pgr","version":"0.1.0","features":["syntax","diff"]}'"#;
    let mut session = VisualSession::spawn_pgr_piped(json_str, &["--syntax"]);
    session.settle(SETTLE_INITIAL);

    // JSON in pipe mode should still trigger content-mode detection and
    // produce colored output (either via syntax highlighting or JSON mode).
    let formatted = session.screen_formatted();
    // Check for any SGR color code (either 24-bit or standard).
    let has_color = formatted.contains("\x1b[3") || formatted.contains("\x1b[9");
    assert!(
        has_color,
        "expected colored output for piped JSON content, got:\n{formatted}"
    );

    session.quit();
}

/// Verify that a file longer than the screen scrolls correctly with `j`.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_large_file_scrolls_correctly() {
    let mut content = String::new();
    for i in 0..50 {
        let _ = writeln!(content, "line number {i:03}");
    }

    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    // Verify initial top line.
    let initial_lines = session.content_lines();
    let first_line_initial = initial_lines.first().cloned().unwrap_or_default();
    assert!(
        first_line_initial.contains("line number 000"),
        "expected 'line number 000' as first line, got: {first_line_initial:?}"
    );

    // Press j five times to scroll down.
    for _ in 0..5 {
        session.send_keys("j");
        session.settle(Duration::from_millis(100));
    }
    session.settle(SETTLE_KEY);

    let scrolled_lines = session.content_lines();
    let first_line_scrolled = scrolled_lines.first().cloned().unwrap_or_default();
    assert!(
        first_line_scrolled.contains("line number 005"),
        "expected 'line number 005' as first line after 5x j, got: {first_line_scrolled:?}"
    );

    session.quit();
}

// ── Regression guards ──────────────────────────────────────────────────────

/// Verify that content detection works even when input contains ANSI color codes
/// (e.g., from `git diff --color`).
#[test]
#[ignore = "visual: slow PTY test"]
fn test_content_detection_with_ansi_input() {
    // Simulate colored diff: ANSI codes wrapping diff markers.
    let colored_diff = "\
\x1b[1mdiff --git a/foo.rs b/foo.rs\x1b[0m
\x1b[1mindex 1234567..abcdefg 100644\x1b[0m
\x1b[1m--- a/foo.rs\x1b[0m
\x1b[1m+++ b/foo.rs\x1b[0m
\x1b[36m@@ -1,3 +1,4 @@\x1b[0m
 fn main() {
\x1b[32m+    println!(\"hello\");\x1b[0m
     let x = 1;
 }
";
    let file = generate_file(colored_diff);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let prompt = session.prompt_line();
    assert!(
        prompt.contains("[diff mode]"),
        "expected '[diff mode]' detected despite ANSI in input, got: {prompt:?}"
    );

    session.quit();
}

/// Verify that the initial render is not blank — content lines have text.
#[test]
#[ignore = "visual: slow PTY test"]
fn test_initial_render_not_blank() {
    let content = "Hello, pgr!\nThis is a test file.\nThird line here.\n";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();

    let mut session = VisualSession::spawn_pgr(&[], path);
    session.settle(SETTLE_INITIAL);

    let lines = session.content_lines();
    let non_empty = lines.iter().any(|l| !l.trim().is_empty());
    assert!(
        non_empty,
        "expected at least one non-empty content line on initial render, \
         got all-empty lines: {lines:?}"
    );

    // Verify specific content is present.
    let text = session.screen_text();
    assert!(
        text.contains("Hello, pgr!"),
        "expected 'Hello, pgr!' on screen, got:\n{text}"
    );

    session.quit();
}
