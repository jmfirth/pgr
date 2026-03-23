use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use super::compare;
use super::harness::PagerSession;

/// Standard terminal dimensions for conformance tests.
pub const TEST_ROWS: usize = 24;
/// Standard terminal width for conformance tests.
pub const TEST_COLS: usize = 80;

/// Standard settle duration for initial render.
pub const SETTLE_INITIAL: Duration = Duration::from_millis(500);
/// Standard settle duration after keystrokes.
pub const SETTLE_KEY: Duration = Duration::from_millis(300);
/// Alias used by some test modules.
pub const SETTLE_TIME: Duration = SETTLE_INITIAL;

/// Return the workspace root directory.
pub fn workspace_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // go from pgr-cli/ to workspace root
    path
}

/// Return the path to a fixture file.
pub fn fixture_path(name: &str) -> PathBuf {
    let mut path = workspace_root();
    path.push("fixtures");
    path.push(name);
    path
}

/// Check if GNU less is available on the system.
pub fn less_available() -> bool {
    Command::new("less")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// Skip macro -- call at the start of every conformance test.
///
/// If GNU less is not found, prints a message and returns early.
macro_rules! skip_if_no_less {
    () => {
        if !super::helpers::less_available() {
            eprintln!("GNU less not found, skipping conformance test");
            return;
        }
    };
}
pub(crate) use skip_if_no_less;

/// Generate a test file with N numbered lines.
///
/// Each line is formatted as "Line NNNN" with zero-padded line numbers.
pub fn generate_numbered_file(lines: usize) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let width = format!("{lines}").len();
    for i in 1..=lines {
        writeln!(file, "Line {:0>width$}", i, width = width).expect("failed to write to temp file");
    }
    file.flush().expect("failed to flush temp file");
    file
}

/// Generate a test file with specific content.
pub fn generate_file(content: &str) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("failed to create temp file");
    file.write_all(content.as_bytes())
        .expect("failed to write to temp file");
    file.flush().expect("failed to flush temp file");
    file
}

/// Generate a file with groups of blank lines.
///
/// Each tuple in `groups` is `(content_lines, blank_lines_after)`.
pub fn generate_file_with_blanks(groups: &[(usize, usize)]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("failed to create temp file");
    let mut line_num = 1;
    for &(content_count, blank_count) in groups {
        for _ in 0..content_count {
            writeln!(file, "Content line {line_num}").expect("failed to write");
            line_num += 1;
        }
        for _ in 0..blank_count {
            writeln!(file).expect("failed to write blank");
        }
    }
    file.flush().expect("failed to flush temp file");
    file
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

// ── Search test data generators ───────────────────────────────────────────

/// Generate a file for basic search tests.
///
/// Creates a 200-line file with "error" appearing on lines 15, 45, 90, 150, 180.
pub fn generate_search_basic() -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=200 {
        if [15, 45, 90, 150, 180].contains(&i) {
            content.push_str(&format!("Line {i:03}: error occurred here\n"));
        } else {
            content.push_str(&format!("Line {i:03}: normal content\n"));
        }
    }
    generate_file(&content)
}

/// Generate a file for case-sensitivity search tests.
///
/// Contains "error", "Error", and "ERROR" on different lines.
pub fn generate_search_case() -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=100 {
        match i {
            10 => content.push_str(&format!("Line {i:03}: error lowercase\n")),
            20 => content.push_str(&format!("Line {i:03}: Error mixed case\n")),
            30 => content.push_str(&format!("Line {i:03}: ERROR uppercase\n")),
            50 => content.push_str(&format!("Line {i:03}: error again\n")),
            70 => content.push_str(&format!("Line {i:03}: Error again\n")),
            90 => content.push_str(&format!("Line {i:03}: ERROR again\n")),
            _ => content.push_str(&format!("Line {i:03}: normal content\n")),
        }
    }
    generate_file(&content)
}

/// Generate a file for search highlight tests.
///
/// Contains "target" on several lines interspersed with other content.
pub fn generate_search_highlight() -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=100 {
        if i % 10 == 0 {
            content.push_str(&format!("Line {i:03}: contains target word here\n"));
        } else {
            content.push_str(&format!("Line {i:03}: regular line content\n"));
        }
    }
    generate_file(&content)
}

/// Generate a file for regex search tests.
///
/// Contains lines with regex metacharacters like "[brackets]".
pub fn generate_search_regex() -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=100 {
        match i {
            15 => content.push_str(&format!("Line {i:03}: has [brackets] here\n")),
            30 => content.push_str(&format!("Line {i:03}: has a.b pattern\n")),
            45 => content.push_str(&format!("Line {i:03}: has (parens) here\n")),
            60 => content.push_str(&format!("Line {i:03}: has $dollar and ^caret\n")),
            _ => content.push_str(&format!("Line {i:03}: plain text content\n")),
        }
    }
    generate_file(&content)
}

/// Generate a file for filter mode tests.
///
/// Contains lines with "ERROR" markers interspersed with other content.
pub fn generate_filter_test() -> tempfile::NamedTempFile {
    let mut content = String::new();
    for i in 1..=100 {
        if i % 7 == 0 {
            content.push_str(&format!("Line {i:03}: ERROR critical failure\n"));
        } else {
            content.push_str(&format!("Line {i:03}: normal operation\n"));
        }
    }
    generate_file(&content)
}

// ── Spawn helpers ─────────────────────────────────────────────────────────

/// Spawn a pgr/less pair with the given arguments and input file.
///
/// Returns `(pgr_session, less_session)` after both have settled.
pub fn spawn_pair(args: &[&str], input_file: &str) -> (PagerSession, PagerSession) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    (pgr, less)
}

/// Send the same keystrokes to both pager sessions with settle between.
pub fn send_keys_to_both(pgr: &mut PagerSession, less: &mut PagerSession, keys: &str) {
    pgr.send_keys(keys);
    less.send_keys(keys);

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);
}

// ── Single-file conformance assertions ────────────────────────────────────

/// Run a full conformance comparison: spawn both pagers with the same args
/// and file, send the same keystrokes, compare full screen output.
///
/// Panics on mismatch with a readable diff.
pub fn assert_conformance(args: &[&str], input_file: &str, keystrokes: &str) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes);
        less.send_keys(keystrokes);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_screens(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Same as `assert_conformance` but only compares the content area
/// (all rows except the prompt/status line).
pub fn assert_content_conformance(args: &[&str], input_file: &str, keystrokes: &str) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes);
        less.send_keys(keystrokes);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Compare content with normalized trailing whitespace.
pub fn assert_normalized_conformance(args: &[&str], input_file: &str, keystrokes: &str) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes);
        less.send_keys(keystrokes);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_normalized(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Compare prompt lines between pgr and less.
pub fn assert_prompt_conformance(args: &[&str], input_file: &str, keystrokes: &str) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes);
        less.send_keys(keystrokes);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_prompts(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Like `assert_content_conformance` but sends raw bytes instead of a string.
///
/// Useful for sending escape sequences (arrow keys, etc).
pub fn assert_content_conformance_bytes(args: &[&str], input_file: &str, keystrokes: &[u8]) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    if !keystrokes.is_empty() {
        pgr.send_bytes(keystrokes);
        less.send_bytes(keystrokes);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Multi-step conformance: spawn both pagers, run a sequence of keystroke
/// groups with settle between each, then compare content after the final step.
pub fn assert_content_conformance_steps(args: &[&str], input_file: &str, steps: &[&str]) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    for step in steps {
        pgr.send_keys(step);
        less.send_keys(step);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Multi-step conformance with raw byte steps.
pub fn assert_content_conformance_byte_steps(args: &[&str], input_file: &str, steps: &[&[u8]]) {
    let mut pgr = PagerSession::spawn_pgr(args, input_file, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(args, input_file, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    for step in steps {
        pgr.send_bytes(step);
        less.send_bytes(step);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Multi-file conformance assertions ─────────────────────────────────────

/// Content conformance with multiple input files.
pub fn assert_content_conformance_files(args: &[&str], files: &[&str], keystrokes: &str) {
    let mut pgr = PagerSession::spawn_pgr_files(args, files, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less_files(args, files, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    if !keystrokes.is_empty() {
        pgr.send_keys(keystrokes);
        less.send_keys(keystrokes);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Multi-file multi-step content conformance.
pub fn assert_content_conformance_files_steps(args: &[&str], files: &[&str], steps: &[&str]) {
    let mut pgr = PagerSession::spawn_pgr_files(args, files, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less_files(args, files, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    for step in steps {
        pgr.send_keys(step);
        less.send_keys(step);

        pgr.settle(SETTLE_KEY);
        less.settle(SETTLE_KEY);
    }

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}
