/// Helper functions and test data generators for conformance tests.
use std::io::Write;
use std::process::Command;
use std::time::Duration;

use tempfile::NamedTempFile;

use super::harness::PagerSession;

/// Standard terminal dimensions for conformance tests.
pub const TEST_ROWS: u16 = 24;
/// Standard terminal column width for conformance tests.
pub const TEST_COLS: u16 = 80;

/// Standard settle time for pager to render initial screen.
pub const SETTLE_TIME: Duration = Duration::from_millis(500);

/// Shorter settle time after keystrokes.
pub const KEY_SETTLE_TIME: Duration = Duration::from_millis(300);

/// Check if GNU less is available on the system.
pub fn less_available() -> bool {
    Command::new("less")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Skip the test if GNU less is not installed.
///
/// Use at the start of each conformance test function.
macro_rules! skip_if_no_less {
    () => {
        if !crate::helpers::less_available() {
            eprintln!("GNU less not found, skipping conformance test");
            return;
        }
    };
}
pub(crate) use skip_if_no_less;

/// Generate a file with N numbered lines ("Line NNN").
#[allow(dead_code)] // Used by other conformance suites (Tasks 126, 128, 129)
pub fn generate_numbered_file(lines: usize) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    for i in 1..=lines {
        writeln!(f, "Line {i:03}").expect("failed to write");
    }
    f.flush().expect("failed to flush");
    f
}

/// Generate a file with specific content.
#[allow(dead_code)] // Used by other conformance suites (Tasks 126, 128, 129)
pub fn generate_file(content: &str) -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    f.write_all(content.as_bytes())
        .expect("failed to write content");
    f.flush().expect("failed to flush");
    f
}

/// Generate the `search_basic` fixture: 200 lines with "error" on specific lines.
///
/// Lines 15, 45, 90, 150, and 180 contain the word "error".
/// All other lines contain "normal output line NNN".
pub fn generate_search_basic() -> NamedTempFile {
    let error_lines = [15, 45, 90, 150, 180];
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    for i in 1..=200 {
        if error_lines.contains(&i) {
            writeln!(f, "Line {i:03}: error detected in module").expect("failed to write");
        } else {
            writeln!(f, "Line {i:03}: normal output line").expect("failed to write");
        }
    }
    f.flush().expect("failed to flush");
    f
}

/// Generate the `search_case` fixture: lines with various case patterns of "error".
pub fn generate_search_case() -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    let cases = [
        "Line 01: Error in module alpha",
        "Line 02: normal line",
        "Line 03: ERROR in module beta",
        "Line 04: normal line",
        "Line 05: error in module gamma",
        "Line 06: normal line",
        "Line 07: eRRoR in module delta",
        "Line 08: normal line",
        "Line 09: Error at startup",
        "Line 10: normal line",
        "Line 11: error at shutdown",
        "Line 12: normal line",
        "Line 13: ERROR at restart",
        "Line 14: normal line",
        "Line 15: warning no error here actually error is present",
        "Line 16: normal line",
        "Line 17: Error Error Error multiple",
        "Line 18: normal line",
        "Line 19: no issues found",
        "Line 20: error final line",
    ];
    for line in &cases {
        writeln!(f, "{line}").expect("failed to write");
    }
    // Pad to 50 lines so the file is long enough for searching.
    for i in 21..=50 {
        writeln!(f, "Line {i:02}: padding line with no matches").expect("failed to write");
    }
    f.flush().expect("failed to flush");
    f
}

/// Generate the `search_regex` fixture: lines containing regex metacharacters.
pub fn generate_search_regex() -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    let lines = [
        "Line 01: normal text",
        "Line 02: array[0] access",
        "Line 03: function(arg) call",
        "Line 04: price is $10.99",
        "Line 05: wildcard * match",
        "Line 06: range [a-z] test",
        "Line 07: escape \\n newline",
        "Line 08: pipe a|b choice",
        "Line 09: caret ^start of line",
        "Line 10: end of line$ here",
        "Line 11: group (a|b) test",
        "Line 12: repeat a+ plus",
        "Line 13: optional a? maybe",
        "Line 14: [brackets] literal",
        "Line 15: dot. any char",
        "Line 16: normal text again",
        "Line 17: curly {brace} test",
        "Line 18: backslash \\ test",
        "Line 19: normal text more",
        "Line 20: [brackets] again here",
    ];
    for line in &lines {
        writeln!(f, "{line}").expect("failed to write");
    }
    // Pad to 50 lines.
    for i in 21..=50 {
        writeln!(f, "Line {i:02}: normal padding line").expect("failed to write");
    }
    f.flush().expect("failed to flush");
    f
}

/// Generate the `search_highlight` fixture: every line contains the search term.
pub fn generate_search_highlight() -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    for i in 1..=40 {
        writeln!(f, "Line {i:02}: the word target appears here target end")
            .expect("failed to write");
    }
    f.flush().expect("failed to flush");
    f
}

/// Generate the `filter_test` fixture: log-like file with levels.
pub fn generate_filter_test() -> NamedTempFile {
    let mut f = NamedTempFile::new().expect("failed to create temp file");
    let levels = ["INFO", "WARNING", "ERROR", "INFO", "DEBUG"];
    for i in 1..=100 {
        let level = levels[i % levels.len()];
        writeln!(f, "{level} [{i:03}]: message from component").expect("failed to write");
    }
    f.flush().expect("failed to flush");
    f
}

/// Spawn both pgr and less with the same arguments and file, let them settle,
/// and return both sessions ready for interaction.
pub fn spawn_pair(args: &[&str], input_file: &str) -> (PagerSession, PagerSession) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS)
        .expect("failed to spawn pgr");
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS)
        .expect("failed to spawn less");

    pgr.wait_and_read(SETTLE_TIME);
    less.wait_and_read(SETTLE_TIME);

    (pgr, less)
}

/// Send keys to both sessions and let them settle.
pub fn send_keys_to_both(pgr: &mut PagerSession, less: &mut PagerSession, keys: &str) {
    pgr.send_keys(keys).expect("pgr send_keys failed");
    less.send_keys(keys).expect("less send_keys failed");

    pgr.wait_and_read(KEY_SETTLE_TIME);
    less.wait_and_read(KEY_SETTLE_TIME);
}
