/// Test helpers for conformance tests.
///
/// Provides fixture generation, GNU less detection, and convenience
/// assertion functions that spawn both pagers and compare their output.
use std::io::Write;
use std::process::Command;
use std::time::Duration;

use super::compare::{compare_content, compare_screens};
use super::harness::PagerSession;

/// Standard terminal rows for conformance tests.
pub const TEST_ROWS: u16 = 24;

/// Standard terminal columns for conformance tests.
pub const TEST_COLS: u16 = 80;

/// Standard settle time after sending keystrokes.
pub const SETTLE_TIME: Duration = Duration::from_millis(500);

/// Initial settle time after spawning a pager (longer for startup).
pub const STARTUP_SETTLE: Duration = Duration::from_millis(800);

/// Check if GNU less is available on the system.
pub fn less_available() -> bool {
    Command::new("less")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Skip the current test if GNU less is not available.
///
/// Use `skip_if_no_less!()` macro instead for cleaner test code.
pub fn skip_if_no_less_impl() -> bool {
    if !less_available() {
        eprintln!("GNU less not found, skipping conformance test");
        return true;
    }
    false
}

/// Skip the current test if GNU less is not installed.
macro_rules! skip_if_no_less {
    () => {
        if super::helpers::skip_if_no_less_impl() {
            return;
        }
    };
}
pub(crate) use skip_if_no_less;

/// Generate a temporary file with N numbered lines.
///
/// Each line has the format "Line NNN" with zero-padded numbers.
pub fn generate_numbered_file(lines: usize) -> tempfile::NamedTempFile {
    let width = format!("{lines}").len();
    let mut content = String::new();
    for i in 1..=lines {
        content.push_str(&format!("Line {i:0>width$}\n"));
    }
    generate_file(&content)
}

/// Generate a temporary file with the given content.
pub fn generate_file(content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("failed to create temp file");
    f.write_all(content.as_bytes())
        .expect("failed to write temp file");
    f.flush().expect("failed to flush temp file");
    f
}

/// Generate a file with long lines (each `width` characters wide).
pub fn generate_long_lines_file(lines: usize, width: usize) -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=lines {
        let prefix = format!("Line {i:03}: ");
        let padding_len = width.saturating_sub(prefix.len());
        content.push_str(&prefix);
        for _ in 0..padding_len {
            content.push('x');
        }
        content.push('\n');
    }
    generate_file(&content)
}

/// Spawn both pgr and less, send the same keystrokes, and compare
/// the content area (excluding prompt line). Panics on mismatch.
pub fn assert_content_conformance(args: &[&str], input_file: &str, keystrokes: &str) {
    let mut pgr =
        PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn pgr");
    let mut less =
        PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn less");

    // Let initial render complete.
    pgr.settle(STARTUP_SETTLE).expect("pgr startup settle");
    less.settle(STARTUP_SETTLE).expect("less startup settle");

    // Send keystrokes if any.
    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes).expect("pgr send keys");
        less.send_keys(keystrokes).expect("less send keys");

        pgr.settle(SETTLE_TIME).expect("pgr settle");
        less.settle(SETTLE_TIME).expect("less settle");
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    if let Err(diff) = compare_content(&pgr_screen, &less_screen) {
        pgr.quit().ok();
        less.quit().ok();
        panic!("Content conformance failed:\n{diff}");
    }

    pgr.quit().ok();
    less.quit().ok();
}

/// Spawn both pgr and less, send the same keystrokes, and compare
/// the full screen including the prompt. Panics on mismatch.
pub fn assert_screen_conformance(args: &[&str], input_file: &str, keystrokes: &str) {
    let mut pgr =
        PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn pgr");
    let mut less =
        PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn less");

    pgr.settle(STARTUP_SETTLE).expect("pgr startup settle");
    less.settle(STARTUP_SETTLE).expect("less startup settle");

    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes).expect("pgr send keys");
        less.send_keys(keystrokes).expect("less send keys");

        pgr.settle(SETTLE_TIME).expect("pgr settle");
        less.settle(SETTLE_TIME).expect("less settle");
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    if let Err(diff) = compare_screens(&pgr_screen, &less_screen) {
        pgr.quit().ok();
        less.quit().ok();
        panic!("Screen conformance failed:\n{diff}");
    }

    pgr.quit().ok();
    less.quit().ok();
}

/// Spawn both pgr and less, send keystrokes as raw bytes, and compare
/// the content area. Panics on mismatch.
///
/// Useful for sending escape sequences (arrow keys, etc).
pub fn assert_content_conformance_bytes(args: &[&str], input_file: &str, keystrokes: &[u8]) {
    let mut pgr =
        PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn pgr");
    let mut less =
        PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn less");

    pgr.settle(STARTUP_SETTLE).expect("pgr startup settle");
    less.settle(STARTUP_SETTLE).expect("less startup settle");

    if !keystrokes.is_empty() {
        pgr.send_bytes(keystrokes).expect("pgr send bytes");
        less.send_bytes(keystrokes).expect("less send bytes");

        pgr.settle(SETTLE_TIME).expect("pgr settle");
        less.settle(SETTLE_TIME).expect("less settle");
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    if let Err(diff) = compare_content(&pgr_screen, &less_screen) {
        pgr.quit().ok();
        less.quit().ok();
        panic!("Content conformance failed:\n{diff}");
    }

    pgr.quit().ok();
    less.quit().ok();
}

/// Multi-step conformance: spawn both pagers, run a sequence of keystroke
/// groups with settle between each, then compare content after the final step.
pub fn assert_content_conformance_steps(args: &[&str], input_file: &str, steps: &[&str]) {
    let mut pgr =
        PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn pgr");
    let mut less =
        PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn less");

    pgr.settle(STARTUP_SETTLE).expect("pgr startup settle");
    less.settle(STARTUP_SETTLE).expect("less startup settle");

    for step in steps {
        pgr.send_keys(step).expect("pgr send keys");
        less.send_keys(step).expect("less send keys");

        pgr.settle(SETTLE_TIME).expect("pgr settle");
        less.settle(SETTLE_TIME).expect("less settle");
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    if let Err(diff) = compare_content(&pgr_screen, &less_screen) {
        pgr.quit().ok();
        less.quit().ok();
        panic!("Content conformance failed:\n{diff}");
    }

    pgr.quit().ok();
    less.quit().ok();
}

/// Multi-step conformance with raw byte steps.
pub fn assert_content_conformance_byte_steps(args: &[&str], input_file: &str, steps: &[&[u8]]) {
    let mut pgr =
        PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn pgr");
    let mut less =
        PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS).expect("spawn less");

    pgr.settle(STARTUP_SETTLE).expect("pgr startup settle");
    less.settle(STARTUP_SETTLE).expect("less startup settle");

    for step in steps {
        pgr.send_bytes(step).expect("pgr send bytes");
        less.send_bytes(step).expect("less send bytes");

        pgr.settle(SETTLE_TIME).expect("pgr settle");
        less.settle(SETTLE_TIME).expect("less settle");
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    if let Err(diff) = compare_content(&pgr_screen, &less_screen) {
        pgr.quit().ok();
        less.quit().ok();
        panic!("Content conformance failed:\n{diff}");
    }

    pgr.quit().ok();
    less.quit().ok();
}
