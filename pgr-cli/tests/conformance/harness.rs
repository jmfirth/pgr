use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

use expectrl::process::Healthcheck;
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

/// A captured screen state: rows of text from the virtual terminal.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by downstream conformance tests (Tasks 126-128)
pub struct ScreenCapture {
    /// Each row as a string of visible characters.
    pub rows: Vec<String>,
    /// Terminal dimensions at capture time.
    pub num_rows: usize,
    pub num_cols: usize,
}

#[allow(dead_code)] // Methods used by downstream conformance tests (Tasks 126-128)
impl ScreenCapture {
    /// Extract the prompt/status line (last row).
    pub fn prompt_line(&self) -> &str {
        self.rows.last().map_or("", String::as_str)
    }

    /// Extract content lines (all rows except the last).
    pub fn content_lines(&self) -> &[String] {
        if self.rows.len() > 1 {
            &self.rows[..self.rows.len() - 1]
        } else {
            &[]
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

/// A PTY-based pager session for conformance testing.
pub struct PagerSession {
    /// The underlying PTY session.
    session: OsSession,
    /// Virtual terminal parser for screen capture.
    parser: vt100::Parser,
    /// Terminal dimensions.
    rows: usize,
    cols: usize,
}

impl PagerSession {
    /// Spawn pgr with the given file arguments and pager flags.
    ///
    /// `args` are flags like `["-N", "-S"]`. `files` are file paths.
    pub fn spawn_pgr(
        args: &[&str],
        files: &[&str],
        rows: usize,
        cols: usize,
    ) -> Result<Self, HarnessError> {
        ensure_binary_built();
        let bin = binary_path();
        let mut cmd = Command::new(&bin);
        for arg in args {
            cmd.arg(arg);
        }
        for file in files {
            cmd.arg(file);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");

        let session = expectrl::Session::spawn(cmd)
            .map_err(|e| HarnessError(format!("failed to spawn pgr: {e}")))?;

        #[allow(clippy::cast_possible_truncation)] // Terminal dimensions always < u16::MAX
        let parser = vt100::Parser::new(rows as u16, cols as u16, 0);

        Ok(Self {
            session,
            parser,
            rows,
            cols,
        })
    }

    /// Spawn GNU less with the given file arguments and pager flags.
    pub fn spawn_less(
        args: &[&str],
        files: &[&str],
        rows: usize,
        cols: usize,
    ) -> Result<Self, HarnessError> {
        let mut cmd = Command::new("less");
        for arg in args {
            cmd.arg(arg);
        }
        for file in files {
            cmd.arg(file);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env_remove("LESS");
        cmd.env_remove("LESSOPEN");
        cmd.env_remove("LESSCLOSE");

        let session = expectrl::Session::spawn(cmd)
            .map_err(|e| HarnessError(format!("failed to spawn less: {e}")))?;

        #[allow(clippy::cast_possible_truncation)] // Terminal dimensions always < u16::MAX
        let parser = vt100::Parser::new(rows as u16, cols as u16, 0);

        Ok(Self {
            session,
            parser,
            rows,
            cols,
        })
    }

    /// Send a string of keystrokes to the pager.
    ///
    /// Each character is sent individually with a small inter-key delay
    /// for reliable terminal processing.
    pub fn send_keys(&mut self, keys: &str) -> Result<(), HarnessError> {
        self.session
            .send(keys)
            .map_err(|e| HarnessError(format!("failed to send keys: {e}")))?;
        Ok(())
    }

    /// Send raw bytes (for control characters, escape sequences).
    #[allow(dead_code)] // Used by downstream conformance tests (Tasks 126-128)
    pub fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), HarnessError> {
        self.session
            .get_stream_mut()
            .write_all(bytes)
            .map_err(|e| HarnessError(format!("failed to send bytes: {e}")))?;
        Ok(())
    }

    /// Wait for the pager to settle (process input and render).
    ///
    /// Reads all available output from the PTY and feeds it to the
    /// virtual terminal parser.
    pub fn settle(&mut self, timeout: Duration) {
        std::thread::sleep(timeout);
        self.drain_output();
    }

    /// Read all available output from the PTY into the vt100 parser.
    fn drain_output(&mut self) {
        let mut buf = [0u8; 4096];
        // Set non-blocking reads with a short timeout
        self.session
            .set_expect_timeout(Some(Duration::from_millis(100)));
        loop {
            match self.session.get_stream_mut().read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.parser.process(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(_) => break,
            }
        }
    }

    /// Capture the current screen state from the virtual terminal parser.
    pub fn capture_screen(&mut self) -> ScreenCapture {
        self.drain_output();
        let screen = self.parser.screen();
        #[allow(clippy::cast_possible_truncation)] // Terminal dimensions always < u16::MAX
        let rows_iter = screen.rows(0, self.cols as u16);
        let rows: Vec<String> = rows_iter.collect();

        ScreenCapture {
            rows,
            num_rows: self.rows,
            num_cols: self.cols,
        }
    }

    /// Send quit command and wait for the process to exit.
    pub fn quit(&mut self) {
        let _ = self.session.send("q");
        std::thread::sleep(Duration::from_millis(300));
    }

    /// Check if the underlying process is still alive.
    #[allow(dead_code)] // Used by downstream conformance tests (Tasks 126-128)
    pub fn is_alive(&self) -> bool {
        self.session.get_process().is_alive().unwrap_or(false)
    }
}

impl Drop for PagerSession {
    fn drop(&mut self) {
        // Try to quit gracefully, then kill if still alive.
        let _ = self.session.send("q");
        std::thread::sleep(Duration::from_millis(100));
    }
}
