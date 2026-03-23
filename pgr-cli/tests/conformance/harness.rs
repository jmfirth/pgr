use std::io::Read;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use expectrl::process::NonBlocking;
use expectrl::session::OsSession;
use expectrl::Expect;

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

/// A PTY-based pager session for conformance testing.
///
/// Wraps an expectrl session with a vt100 parser for screen capture.
pub struct PagerSession {
    /// The name of the pager ("pgr" or "less").
    pub name: String,
    /// The underlying PTY session.
    session: OsSession,
    /// Virtual terminal parser for screen state extraction.
    parser: vt100::Parser,
    /// Terminal rows.
    rows: u16,
    /// Terminal columns.
    cols: u16,
}

impl PagerSession {
    /// Spawn pgr with the given arguments and input file.
    #[allow(clippy::cast_possible_truncation)] // Terminal dimensions are always < u16::MAX
    pub fn spawn_pgr(args: &[&str], input_file: &str, rows: usize, cols: usize) -> Self {
        ensure_binary_built();
        let rows = rows as u16;
        let cols = cols as u16;

        let bin = binary_path();
        let mut cmd = Command::new(&bin);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);

        Self::spawn_command("pgr", cmd, rows, cols)
    }

    /// Spawn GNU less with the given arguments and input file.
    #[allow(clippy::cast_possible_truncation)] // Terminal dimensions are always < u16::MAX
    pub fn spawn_less(args: &[&str], input_file: &str, rows: usize, cols: usize) -> Self {
        let rows = rows as u16;
        let cols = cols as u16;

        let less_bin = std::env::var("LESS_BIN").unwrap_or_else(|_| "less".to_string());
        let mut cmd = Command::new(&less_bin);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);

        Self::spawn_command("less", cmd, rows, cols)
    }

    /// Spawn pgr with multiple input files.
    #[allow(clippy::cast_possible_truncation)] // Terminal dimensions are always < u16::MAX
    pub fn spawn_pgr_files(args: &[&str], input_files: &[&str], rows: usize, cols: usize) -> Self {
        ensure_binary_built();
        let rows = rows as u16;
        let cols = cols as u16;

        let bin = binary_path();
        let mut cmd = Command::new(&bin);
        for arg in args {
            cmd.arg(arg);
        }
        for file in input_files {
            cmd.arg(file);
        }

        Self::spawn_command("pgr", cmd, rows, cols)
    }

    /// Spawn GNU less with multiple input files.
    #[allow(clippy::cast_possible_truncation)] // Terminal dimensions are always < u16::MAX
    pub fn spawn_less_files(args: &[&str], input_files: &[&str], rows: usize, cols: usize) -> Self {
        let rows = rows as u16;
        let cols = cols as u16;

        let less_bin = std::env::var("LESS_BIN").unwrap_or_else(|_| "less".to_string());
        let mut cmd = Command::new(&less_bin);
        for arg in args {
            cmd.arg(arg);
        }
        for file in input_files {
            cmd.arg(file);
        }

        Self::spawn_command("less", cmd, rows, cols)
    }

    /// Internal: spawn a command in a PTY with the given dimensions.
    fn spawn_command(name: &str, mut cmd: Command, rows: u16, cols: u16) -> Self {
        // Set TERM so the pager knows it has a terminal.
        cmd.env("TERM", "xterm-256color");
        // Clear env vars that might interfere with conformance comparison.
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");
        cmd.env_remove("LESSSECURE");
        cmd.env_remove("LESSKEY");

        let mut session =
            expectrl::Session::spawn(cmd).unwrap_or_else(|e| panic!("failed to spawn {name}: {e}"));
        session.set_expect_timeout(Some(Duration::from_secs(5)));

        // Set terminal size via the PTY.
        session
            .get_process_mut()
            .set_window_size(cols, rows)
            .unwrap_or_else(|e| panic!("failed to set {name} window size: {e}"));

        let parser = vt100::Parser::new(rows, cols, 0);

        Self {
            name: name.to_string(),
            session,
            parser,
            rows,
            cols,
        }
    }

    /// Send a sequence of keystrokes to the pager.
    pub fn send_keys(&mut self, keys: &str) {
        self.session
            .send(keys)
            .unwrap_or_else(|e| panic!("failed to send keys to {}: {e}", self.name));
    }

    /// Send raw bytes (for control characters, escape sequences).
    pub fn send_bytes(&mut self, bytes: &[u8]) {
        self.session
            .send(bytes)
            .unwrap_or_else(|e| panic!("failed to send bytes to {}: {e}", self.name));
    }

    /// Wait for the pager to settle by reading all available output into
    /// the vt100 parser, with a timeout.
    pub fn settle(&mut self, timeout: Duration) {
        let start = std::time::Instant::now();
        let mut buf = [0u8; 4096];

        // Set non-blocking mode for reading.
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
                    // No more data available right now.
                    if start.elapsed() > timeout {
                        break;
                    }
                    // Sleep briefly then retry — pager may still be rendering.
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => break,
            }
            if start.elapsed() > timeout {
                break;
            }
        }

        // Restore blocking mode.
        self.session
            .get_stream_mut()
            .set_blocking(true)
            .expect("failed to restore blocking");
    }

    /// Wait and read output for the given duration (alias for settle).
    pub fn wait_and_read(&mut self, timeout: Duration) {
        self.settle(timeout);
    }

    /// Capture the current screen state from the vt100 parser.
    pub fn capture_screen(&self) -> ScreenCapture {
        let screen = self.parser.screen();
        let mut rows_text = Vec::new();

        for row_str in screen.rows(0, self.cols) {
            rows_text.push(row_str);
        }

        let (cursor_row, cursor_col) = screen.cursor_position();

        ScreenCapture {
            rows: rows_text,
            num_rows: usize::from(self.rows),
            num_cols: usize::from(self.cols),
            cursor_row: usize::from(cursor_row),
            cursor_col: usize::from(cursor_col),
        }
    }

    /// Send quit command and wait for the process to exit.
    pub fn quit(&mut self) {
        let _ = self.session.send("q");
        std::thread::sleep(Duration::from_millis(200));
    }
}

impl Drop for PagerSession {
    fn drop(&mut self) {
        // Send quit in case the process is still running.
        let _ = self.session.send("q");
    }
}

/// A captured screen state: rows of text with terminal dimensions.
#[derive(Debug, Clone)]
pub struct ScreenCapture {
    /// Text content of each row, with trailing whitespace preserved.
    pub rows: Vec<String>,
    /// Number of terminal rows.
    pub num_rows: usize,
    /// Number of terminal columns.
    pub num_cols: usize,
    /// Cursor row position.
    pub cursor_row: usize,
    /// Cursor column position.
    pub cursor_col: usize,
}

impl ScreenCapture {
    /// Extract the prompt/status line (last row).
    pub fn prompt_line(&self) -> &str {
        self.rows.last().map_or("", String::as_str)
    }

    /// Extract content lines (all rows except the last).
    pub fn content_lines(&self) -> &[String] {
        if self.rows.is_empty() {
            &[]
        } else {
            &self.rows[..self.rows.len() - 1]
        }
    }

    /// Extract a specific row as text.
    pub fn row_text(&self, row: usize) -> &str {
        self.rows.get(row).map_or("", String::as_str)
    }
}
