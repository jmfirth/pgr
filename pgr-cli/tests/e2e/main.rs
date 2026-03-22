#![warn(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)] // Integration tests use unwrap/expect freely

mod basic_navigation;
mod display;
mod startup;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::Duration;

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

/// Return the path to a fixture file.
fn fixture_path(name: &str) -> PathBuf {
    let mut path = workspace_root();
    path.push("fixtures");
    path.push(name);
    path
}

/// Return the fixtures directory.
fn fixtures_dir() -> PathBuf {
    let mut path = workspace_root();
    path.push("fixtures");
    path
}

/// Spawn the pgr binary in a PTY with the given arguments.
/// Returns a session configured with a reasonable timeout.
fn spawn_pgr(args: &[&str]) -> OsSession {
    spawn_pgr_in(args, None)
}

/// Spawn the pgr binary in a PTY with the given arguments and optional
/// working directory. Returns a session configured with a reasonable timeout.
fn spawn_pgr_in(args: &[&str], cwd: Option<&Path>) -> OsSession {
    ensure_binary_built();

    let bin = binary_path();
    let mut cmd = Command::new(&bin);
    for arg in args {
        cmd.arg(arg);
    }

    // Set TERM so the pager knows it has a terminal.
    cmd.env("TERM", "xterm-256color");
    // Clear LESS env var so it doesn't interfere.
    cmd.env_remove("LESS");

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let mut session =
        expectrl::Session::spawn(cmd).unwrap_or_else(|e| panic!("failed to spawn pgr: {e}"));
    session.set_expect_timeout(Some(Duration::from_secs(5)));

    // The PTY defaults to 80x24 (set by ptyprocess), which matches the
    // pager's default Screen dimensions. No need to override.

    session
}

/// Send a keystroke and wait briefly for the pager to process it.
fn send_key(session: &mut OsSession, key: &str) {
    session.send(key).expect("failed to send key");
    std::thread::sleep(Duration::from_millis(100));
}

/// Wait for output containing a specific string pattern.
fn expect_str(session: &mut OsSession, pattern: &str) {
    session
        .expect(pattern)
        .unwrap_or_else(|e| panic!("expected '{pattern}' but got error: {e}"));
}

/// Quit the pager by sending 'q'.
fn quit_pager(session: &mut OsSession) {
    let _ = session.send("q");
    std::thread::sleep(Duration::from_millis(200));
}
