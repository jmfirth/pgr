/// PTY-based pager session for conformance testing.
///
/// Spawns a pager (pgr or GNU less) in a PTY, sends keystrokes, and
/// captures the resulting screen state via a vt100 virtual terminal parser.
use std::io::Write;
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use expectrl::session::OsSession;

use super::compare::ScreenCapture;

/// Error type for harness operations.
#[derive(Debug)]
pub struct HarnessError {
    pub message: String,
}

impl std::fmt::Display for HarnessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "harness error: {}", self.message)
    }
}

impl std::error::Error for HarnessError {}

impl HarnessError {
    fn new(msg: impl Into<String>) -> Self {
        Self {
            message: msg.into(),
        }
    }
}

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
fn workspace_root() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // go from pgr-cli/ to workspace root
    path
}

/// Return the path to the built pgr-cli binary.
fn binary_path() -> std::path::PathBuf {
    let mut path = workspace_root();
    path.push("target");
    path.push("debug");
    path.push("pgr-cli");
    path
}

/// A PTY-based pager session for conformance testing.
pub struct PagerSession {
    /// The underlying PTY session.
    session: OsSession,
    /// Virtual terminal parser for interpreting raw output.
    parser: vt100::Parser,
    /// Terminal row count.
    rows: u16,
    /// Terminal column count.
    cols: u16,
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
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);

        Self::spawn_with_cmd(cmd, rows, cols)
    }

    /// Spawn GNU less with the given arguments and input file.
    pub fn spawn_less(
        args: &[&str],
        input_file: &str,
        rows: u16,
        cols: u16,
    ) -> Result<Self, HarnessError> {
        let mut cmd = Command::new("less");
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");
        // Disable lesskey file interference.
        cmd.env_remove("LESSKEY");
        for arg in args {
            cmd.arg(arg);
        }
        cmd.arg(input_file);

        Self::spawn_with_cmd(cmd, rows, cols)
    }

    /// Spawn a process from the given command in a PTY.
    fn spawn_with_cmd(cmd: Command, rows: u16, cols: u16) -> Result<Self, HarnessError> {
        let mut session = expectrl::Session::spawn(cmd)
            .map_err(|e| HarnessError::new(format!("failed to spawn process: {e}")))?;
        session.set_expect_timeout(Some(Duration::from_secs(5)));

        // Set PTY size. UnixProcess derefs to ptyprocess::PtyProcess which
        // has set_window_size(cols, rows).
        session
            .get_process_mut()
            .set_window_size(cols, rows)
            .map_err(|e| HarnessError::new(format!("failed to set window size: {e}")))?;

        let parser = vt100::Parser::new(rows, cols, 0);
        Ok(Self {
            session,
            parser,
            rows,
            cols,
        })
    }

    /// Send a sequence of keystrokes to the pager.
    pub fn send_keys(&mut self, keys: &str) -> Result<(), HarnessError> {
        self.session
            .write_all(keys.as_bytes())
            .map_err(|e| HarnessError::new(format!("failed to send keys: {e}")))?;
        self.session
            .flush()
            .map_err(|e| HarnessError::new(format!("failed to flush: {e}")))?;
        Ok(())
    }

    /// Send raw bytes (for control characters, escape sequences).
    pub fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), HarnessError> {
        self.session
            .write_all(bytes)
            .map_err(|e| HarnessError::new(format!("failed to send bytes: {e}")))?;
        self.session
            .flush()
            .map_err(|e| HarnessError::new(format!("failed to flush: {e}")))?;
        Ok(())
    }

    /// Wait for the pager to settle by draining output until quiescent.
    ///
    /// Reads all available output and feeds it to the vt100 parser,
    /// then waits for a quiet period where no new output appears.
    pub fn settle(&mut self, timeout: Duration) -> Result<(), HarnessError> {
        let start = std::time::Instant::now();
        let mut buf = [0u8; 4096];

        // Read in a loop until we hit a quiet period or timeout.
        loop {
            if start.elapsed() > timeout {
                break;
            }

            match self.session.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.parser.process(&buf[..n]);
                    // Keep reading if data is flowing.
                    continue;
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No data available — wait a bit and try again.
                    std::thread::sleep(Duration::from_millis(50));
                    // Try one more read to see if we're truly settled.
                    match self.session.try_read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            self.parser.process(&buf[..n]);
                            continue;
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                        Err(e) => {
                            return Err(HarnessError::new(format!("read error: {e}")));
                        }
                    }
                }
                Err(e) => {
                    return Err(HarnessError::new(format!("read error: {e}")));
                }
            }
        }
        Ok(())
    }

    /// Capture the current screen state from the vt100 parser.
    pub fn capture_screen(&self) -> ScreenCapture {
        let screen = self.parser.screen();
        let rows: Vec<String> = screen.rows(0, self.cols).collect();
        let (cursor_row, cursor_col) = screen.cursor_position();
        ScreenCapture {
            rows,
            terminal_rows: self.rows as usize,
            terminal_cols: self.cols as usize,
            cursor_row: cursor_row as usize,
            cursor_col: cursor_col as usize,
        }
    }

    /// Send quit command and wait for the process to exit.
    pub fn quit(&mut self) -> Result<(), HarnessError> {
        let _ = self.send_keys("q");
        std::thread::sleep(Duration::from_millis(200));
        Ok(())
    }
}

impl Drop for PagerSession {
    fn drop(&mut self) {
        // Try to quit gracefully, then let expectrl handle cleanup.
        let _ = self.session.write_all(b"q");
        std::thread::sleep(Duration::from_millis(100));
    }
}
