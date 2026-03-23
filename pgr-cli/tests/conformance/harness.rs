/// PTY-based pager session for conformance testing.
///
/// Spawns pgr or GNU less in a PTY, sends keystrokes, and captures
/// screen state via a vt100 virtual terminal parser.
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use expectrl::session::OsSession;
use expectrl::Expect;

/// Error type for harness operations.
#[derive(Debug)]
pub struct HarnessError(pub String);

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "harness error: {}", self.0)
    }
}

impl std::error::Error for HarnessError {}

static BUILD_ONCE: Once = Once::new();

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
    path.pop();
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

/// A PTY-based pager session for testing.
pub struct PagerSession {
    /// The underlying PTY session.
    session: OsSession,
    /// Virtual terminal parser for screen state.
    parser: vt100::Parser,
}

impl PagerSession {
    /// Spawn pgr with the given arguments and input file.
    pub fn spawn_pgr(
        args: &[&str],
        input_file: &str,
        rows: u16,
        cols: u16,
    ) -> Result<Self, HarnessError> {
        ensure_binary_built();
        let bin = binary_path();
        let mut cmd = Command::new(&bin);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");

        Self::spawn_session(cmd, rows, cols)
    }

    /// Spawn GNU less with the given arguments and input file.
    pub fn spawn_less(
        args: &[&str],
        input_file: &str,
        rows: u16,
        cols: u16,
    ) -> Result<Self, HarnessError> {
        let mut cmd = Command::new("less");
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");

        Self::spawn_session(cmd, rows, cols)
    }

    /// Common session spawn logic.
    fn spawn_session(cmd: Command, rows: u16, cols: u16) -> Result<Self, HarnessError> {
        let mut session = expectrl::Session::spawn(cmd)
            .map_err(|e| HarnessError(format!("failed to spawn: {e}")))?;
        session.set_expect_timeout(Some(Duration::from_secs(5)));

        // Resize the PTY to the desired dimensions.
        session
            .get_process_mut()
            .set_window_size(cols, rows)
            .map_err(|e| HarnessError(format!("failed to set window size: {e}")))?;

        let parser = vt100::Parser::new(rows, cols, 0);

        Ok(Self { session, parser })
    }

    /// Send a sequence of keystrokes to the pager.
    pub fn send_keys(&mut self, keys: &str) -> Result<(), HarnessError> {
        self.session
            .send(keys)
            .map_err(|e| HarnessError(format!("failed to send keys: {e}")))?;
        Ok(())
    }

    /// Send raw bytes (for control characters, escape sequences).
    #[allow(dead_code)] // Used by other conformance suites (Tasks 126, 128, 129)
    pub fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), HarnessError> {
        self.session
            .write_all(bytes)
            .map_err(|e| HarnessError(format!("failed to send bytes: {e}")))?;
        Ok(())
    }

    /// Wait for output, reading from PTY and feeding to parser, with a sleep.
    ///
    /// Sleeps for the specified duration, then drains all available output
    /// from the PTY into the vt100 parser.
    pub fn wait_and_read(&mut self, wait: Duration) {
        std::thread::sleep(wait);
        self.drain_output();
    }

    /// Drain all currently available output from the PTY into the parser.
    fn drain_output(&mut self) {
        let mut buf = [0u8; 8192];
        // Use a short timeout to detect when no more data is available.
        self.session
            .set_expect_timeout(Some(Duration::from_millis(100)));

        loop {
            match self.session.read(&mut buf) {
                Ok(n) if n > 0 => {
                    self.parser.process(&buf[..n]);
                }
                _ => break,
            }
        }

        self.session
            .set_expect_timeout(Some(Duration::from_secs(5)));
    }

    /// Capture the current screen state from the vt100 parser.
    pub fn capture_screen(&self) -> ScreenCapture {
        let screen = self.parser.screen();
        let (rows, cols) = screen.size();

        // screen.rows(start_col, width) returns an iterator of row strings.
        let text_rows: Vec<String> = screen.rows(0, cols).collect();

        let cursor_pos = screen.cursor_position();

        ScreenCapture {
            rows: text_rows,
            terminal_rows: rows as usize,
            terminal_cols: cols as usize,
            cursor_row: cursor_pos.0 as usize,
            cursor_col: cursor_pos.1 as usize,
        }
    }

    /// Send quit command and wait for the process to exit.
    pub fn quit(&mut self) {
        let _ = self.send_keys("q");
        std::thread::sleep(Duration::from_millis(200));
    }
}

impl Drop for PagerSession {
    fn drop(&mut self) {
        let _ = self.send_keys("q");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// A captured screen state: a grid of text rows.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by other conformance suites (Tasks 126, 128, 129)
pub struct ScreenCapture {
    /// Text content for each row.
    pub rows: Vec<String>,
    /// Terminal row count at capture time.
    pub terminal_rows: usize,
    /// Terminal column count at capture time.
    pub terminal_cols: usize,
    /// Cursor row position.
    pub cursor_row: usize,
    /// Cursor column position.
    pub cursor_col: usize,
}

impl ScreenCapture {
    /// Extract content lines (all rows except the last, which is the prompt).
    pub fn content_lines(&self) -> &[String] {
        if self.rows.is_empty() {
            &[]
        } else {
            &self.rows[..self.rows.len() - 1]
        }
    }

    /// Extract the prompt/status line (last row).
    #[allow(dead_code)] // Used by other conformance suites (Tasks 126, 128, 129)
    pub fn prompt_line(&self) -> &str {
        self.rows.last().map_or("", String::as_str)
    }

    /// Get a specific row as text.
    #[allow(dead_code)] // Used by other conformance suites (Tasks 126, 128, 129)
    pub fn row_text(&self, row: usize) -> &str {
        self.rows.get(row).map_or("", String::as_str)
    }
}
