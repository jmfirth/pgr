use std::fmt::Write as _;
use std::io::Write;
use std::process::Command;
use std::time::Duration;

use crate::compare::compare_content;
use crate::harness::PagerSession;

/// Standard terminal dimensions for conformance tests.
/// Matches the default PTY dimensions from ptyprocess.
pub const TEST_ROWS: usize = 24;
pub const TEST_COLS: usize = 80;

/// Default settle time for pager to process input and render.
pub const SETTLE_TIME: Duration = Duration::from_millis(500);

/// Check if GNU less is available on the system.
pub fn less_available() -> bool {
    Command::new("less")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Skip the current test if GNU less is not installed.
///
/// Use via the `skip_if_no_less!()` macro for ergonomic early return.
#[macro_export]
macro_rules! skip_if_no_less {
    () => {
        if !$crate::helpers::less_available() {
            eprintln!("GNU less not found, skipping conformance test");
            return;
        }
    };
}

/// Generate a test file with N numbered lines (1-indexed).
///
/// Each line reads: `Line NNN` where NNN is zero-padded to 3 digits.
pub fn generate_numbered_file(lines: usize) -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=lines {
        let _ = writeln!(content, "Line {i:03}");
    }
    generate_file(&content)
}

/// Generate a test file with specific content.
pub fn generate_file(content: &str) -> tempfile::NamedTempFile {
    let mut f = tempfile::NamedTempFile::new().expect("failed to create temp file");
    f.write_all(content.as_bytes())
        .expect("failed to write temp file");
    f.flush().expect("failed to flush temp file");
    f
}

/// Generate a file with long lines for wrap/chop testing.
///
/// Each line is `width * 2` characters long so it wraps on an 80-col terminal.
pub fn generate_long_lines_file(lines: usize, width: usize) -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=lines {
        let prefix = format!("Line {i:03} ");
        let padding_len = (width * 2).saturating_sub(prefix.len());
        content.push_str(&prefix);
        for _ in 0..padding_len {
            content.push('x');
        }
        content.push('\n');
    }
    generate_file(&content)
}

/// Run a conformance comparison: spawn both pagers with the same files and
/// args, send the same keystrokes, and compare content areas.
///
/// Panics with a readable diff on mismatch.
pub fn assert_content_conformance(args: &[&str], files: &[&str], keystrokes: &str) {
    let mut pgr =
        PagerSession::spawn_pgr(args, files, TEST_ROWS, TEST_COLS).expect("failed to spawn pgr");
    let mut less =
        PagerSession::spawn_less(args, files, TEST_ROWS, TEST_COLS).expect("failed to spawn less");

    pgr.settle(SETTLE_TIME);
    less.settle(SETTLE_TIME);

    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes).expect("pgr send_keys failed");
        less.send_keys(keystrokes).expect("less send_keys failed");

        pgr.settle(SETTLE_TIME);
        less.settle(SETTLE_TIME);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    if let Err(diff) = compare_content(&pgr_screen, &less_screen) {
        panic!("Conformance failure:\n{diff}");
    }

    pgr.quit();
    less.quit();
}

/// Run a conformance comparison with step-by-step keystroke sequences.
///
/// Each entry in `steps` is a keystroke string to send. After each step,
/// the pager is given time to settle. The screen is captured after the
/// final step.
pub fn assert_content_conformance_steps(args: &[&str], files: &[&str], steps: &[&str]) {
    let mut pgr =
        PagerSession::spawn_pgr(args, files, TEST_ROWS, TEST_COLS).expect("failed to spawn pgr");
    let mut less =
        PagerSession::spawn_less(args, files, TEST_ROWS, TEST_COLS).expect("failed to spawn less");

    pgr.settle(SETTLE_TIME);
    less.settle(SETTLE_TIME);

    for step in steps {
        pgr.send_keys(step).expect("pgr send_keys failed");
        less.send_keys(step).expect("less send_keys failed");

        pgr.settle(SETTLE_TIME);
        less.settle(SETTLE_TIME);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    if let Err(diff) = compare_content(&pgr_screen, &less_screen) {
        panic!("Conformance failure:\n{diff}");
    }

    pgr.quit();
    less.quit();
}
