//! Conformance tests for file management and miscellaneous commands.
//!
//! Compares pgr against GNU less for: file navigation (:n, :p, :d, :x),
//! examine (:e), marks (m, '), option toggling (-, _), info (=), and
//! shell commands (!).
//!
//! SPECIFICATION.md sections: 3.5 (file management), 3.6 (marks),
//! 3.7 (misc commands), 3.8 (option toggling).

use std::time::Duration;

use crate::compare::compare_normalized;
use crate::harness::PagerSession;
use crate::helpers::{
    assert_content_conformance, assert_content_conformance_steps, generate_file,
    generate_long_lines_file, generate_numbered_file, SETTLE_TIME, TEST_COLS, TEST_ROWS,
};
use crate::skip_if_no_less;

// ────────────────────────────────────────────────────────────────────────────
// File management: :n, :p, :d, :x
// ────────────────────────────────────────────────────────────────────────────

/// Test 1: `:n` switches to the next file in the file list.
///
/// Opens two files, sends `:n\n`, verifies the second file is displayed.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_file_next_switches_to_second_file() {
    skip_if_no_less!();

    let file_a = generate_file("File A line 1\nFile A line 2\nFile A line 3\n");
    let file_b = generate_file("File B line 1\nFile B line 2\nFile B line 3\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance(&[], &[path_a, path_b], ":n\n");
}

/// Test 2: `:p` switches back to the previous file.
///
/// Opens two files, navigates to second with `:n`, then back with `:p`.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_file_prev_returns_to_first_file() {
    skip_if_no_less!();

    let file_a = generate_file("File A line 1\nFile A line 2\nFile A line 3\n");
    let file_b = generate_file("File B line 1\nFile B line 2\nFile B line 3\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_steps(&[], &[path_a, path_b], &[":n\n", ":p\n"]);
}

/// Test 3: `:n` at the last file produces an error message.
///
/// Opens two files, navigates to the last file, then tries `:n` again.
/// Both pagers should display an error or stay on the last file.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_file_next_at_last_file() {
    skip_if_no_less!();

    let file_a = generate_file("File A line 1\nFile A line 2\n");
    let file_b = generate_file("File B line 1\nFile B line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_steps(&[], &[path_a, path_b], &[":n\n", ":n\n"]);
}

/// Test 4: `:p` at the first file produces an error message.
///
/// Opens two files and tries `:p` while on the first file.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_file_prev_at_first_file() {
    skip_if_no_less!();

    let file_a = generate_file("File A line 1\nFile A line 2\n");
    let file_b = generate_file("File B line 1\nFile B line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance(&[], &[path_a, path_b], ":p\n");
}

/// Test 5: `:d` removes the current file and shows the next.
///
/// Opens three files, removes the first with `:d`, verifies second file shown.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_file_delete_removes_current() {
    skip_if_no_less!();

    let file_a = generate_file("File A line 1\nFile A line 2\n");
    let file_b = generate_file("File B line 1\nFile B line 2\n");
    let file_c = generate_file("File C line 1\nFile C line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();
    let path_c = file_c.path().to_str().unwrap();

    assert_content_conformance(&[], &[path_a, path_b, path_c], ":d\n");
}

/// Test 6: `:x` returns to the first file from a later file.
///
/// Opens three files, navigates to the third, then `:x` goes back to first.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_file_first_returns_to_first() {
    skip_if_no_less!();

    let file_a = generate_file("File A line 1\nFile A line 2\n");
    let file_b = generate_file("File B line 1\nFile B line 2\n");
    let file_c = generate_file("File C line 1\nFile C line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();
    let path_c = file_c.path().to_str().unwrap();

    assert_content_conformance_steps(&[], &[path_a, path_b, path_c], &[":n\n", ":n\n", ":x\n"]);
}

// ────────────────────────────────────────────────────────────────────────────
// Examine command: :e
// ────────────────────────────────────────────────────────────────────────────

/// Test 7: `:e filename` opens a new file.
///
/// Opens one file, then uses `:e` to open another file.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_examine_opens_new_file() {
    skip_if_no_less!();

    let file_a = generate_file("File A line 1\nFile A line 2\n");
    let file_b = generate_file("Examine target line 1\nExamine target line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b_owned = file_b.path().to_str().unwrap().to_string();

    let keys = format!(":e {path_b_owned}\n");

    assert_content_conformance(&[], &[path_a], &keys);
}

/// Test 8: `:e` with no argument refreshes the current file.
///
/// Opens a file, scrolls down, then `:e\n` should re-display the file.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_examine_refresh_no_arg() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    // Scroll down a few lines, then :e to refresh (re-read same file)
    assert_content_conformance_steps(&[], &[path], &["10j", ":e\n"]);
}

// ────────────────────────────────────────────────────────────────────────────
// Marks: m, ', special marks
// ────────────────────────────────────────────────────────────────────────────

/// Test 9: Set mark `a` at a position, scroll away, return to mark with `'a`.
///
/// Opens a 100-line file, scrolls down 10 lines, sets mark 'a',
/// scrolls down 20 more lines, then returns to mark 'a'.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_mark_set_and_return() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    assert_content_conformance_steps(&[], &[path], &["10j", "ma", "20j", "'a"]);
}

/// Test 10: `''` returns to previous position after a large jump.
///
/// Jump to end with G, then `''` returns to the previous position.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_mark_return_to_previous_position() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    assert_content_conformance_steps(&[], &[path], &["G", "''"]);
}

/// Test 11: `'^` goes to the beginning of the file.
///
/// Scroll down, then `'^` should go to line 1.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_mark_caret_goes_to_beginning() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    assert_content_conformance_steps(&[], &[path], &["20j", "'^"]);
}

/// Test 12: `'$` goes to the end of the file.
///
/// Start at beginning, then `'$` should go to EOF.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_mark_dollar_goes_to_end() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    assert_content_conformance(&[], &[path], "'$");
}

/// Test 13: Multiple marks can be set and returned to independently.
///
/// Set marks a, b, c at different scroll positions, then return to each.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_multiple_marks() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    // Set mark 'a' at line ~10, 'b' at line ~30, 'c' at line ~50.
    // Then return to 'a' to verify.
    assert_content_conformance_steps(&[], &[path], &["10j", "ma", "20j", "mb", "20j", "mc", "'a"]);
}

// ────────────────────────────────────────────────────────────────────────────
// Option toggling: -, _
// ────────────────────────────────────────────────────────────────────────────

/// Test 14: `-i` toggles case-insensitive search.
///
/// Sends `-i\n` to toggle case sensitivity. The prompt should briefly
/// show the state change.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_option_toggle_case() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    // Toggle case sensitivity, then verify content is still the same.
    // The actual toggle message flashes on the prompt line.
    assert_content_conformance(&[], &[path], "-i\n");
}

/// Test 15: `-N` toggles line numbers on/off.
///
/// Sends `-N\n` to enable line numbers, verifying the display changes.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_option_toggle_line_numbers() {
    skip_if_no_less!();

    let file = generate_numbered_file(50);
    let path = file.path().to_str().unwrap();

    assert_content_conformance(&[], &[path], "-N\n");
}

/// Test 16: `-S` toggles line chopping (no wrap).
///
/// Sends `-S\n` with a file that has long lines. Lines should go from
/// wrapped to chopped.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_option_toggle_chop() {
    skip_if_no_less!();

    let file = generate_long_lines_file(20, TEST_COLS);
    let path = file.path().to_str().unwrap();

    assert_content_conformance(&[], &[path], "-S\n");
}

/// Test 17: `_i` queries the current state of an option.
///
/// Sends `_i\n` to query the case-sensitivity option state.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_option_query_case() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    // Query the option state — both pagers should show a similar message.
    // Content area should remain unchanged.
    assert_content_conformance(&[], &[path], "_i\n");
}

// ────────────────────────────────────────────────────────────────────────────
// Info commands: =
// ────────────────────────────────────────────────────────────────────────────

/// Test 18: `=` shows file info line.
///
/// Presses `=` to display file information. The content area should
/// remain the same; the prompt/status line will differ.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_info_equals_file_info() {
    skip_if_no_less!();

    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    // After pressing =, the content should still match.
    assert_content_conformance(&[], &[path], "=");
}

/// Test 19: `=` with multiple files shows "file N of M".
///
/// Opens two files and presses `=` to see the multi-file info.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_info_equals_multiple_files() {
    skip_if_no_less!();

    let file_a = generate_numbered_file(50);
    let file_b = generate_numbered_file(50);
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    // Content area should match even when = is showing info.
    assert_content_conformance(&[], &[path_a, path_b], "=");
}

// ────────────────────────────────────────────────────────────────────────────
// Shell commands: !
// ────────────────────────────────────────────────────────────────────────────

/// Test 20: `!echo test` executes a shell command.
///
/// Shell command comparison is tricky in PTY — we verify that after the
/// shell command completes and the pager resumes, the content area matches
/// between pgr and less.
///
/// NOTE: This test sends `!echo test\n`, waits for the shell output, then
/// presses Enter to resume the pager. Both pagers should return to
/// displaying the original file content.
#[test]
#[ignore = "conformance: requires GNU less"]
fn test_conformance_shell_command_echo() {
    skip_if_no_less!();

    let file = generate_numbered_file(50);
    let path = file.path().to_str().unwrap();

    let mut pgr =
        PagerSession::spawn_pgr(&[], &[path], TEST_ROWS, TEST_COLS).expect("failed to spawn pgr");
    let mut less =
        PagerSession::spawn_less(&[], &[path], TEST_ROWS, TEST_COLS).expect("failed to spawn less");

    // Let pagers render initial content.
    pgr.settle(SETTLE_TIME);
    less.settle(SETTLE_TIME);

    // Send shell command.
    pgr.send_keys("!echo test\n").expect("pgr send");
    less.send_keys("!echo test\n").expect("less send");

    // Wait for shell to execute.
    pgr.settle(Duration::from_millis(1000));
    less.settle(Duration::from_millis(1000));

    // Press Enter/Return to resume the pager.
    pgr.send_keys("\n").expect("pgr send");
    less.send_keys("\n").expect("less send");

    pgr.settle(SETTLE_TIME);
    less.settle(SETTLE_TIME);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    // Use normalized comparison — the prompt may differ after shell resume.
    if let Err(diff) = compare_normalized(&pgr_screen, &less_screen) {
        panic!("Conformance failure (shell command):\n{diff}");
    }

    pgr.quit();
    less.quit();
}
