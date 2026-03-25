/// Conformance tests for Phase 2 CLI option flags.
///
/// Each test spawns both pgr and GNU less with identical arguments and
/// input, sends the same keystrokes, and compares the resulting screen
/// content. Tests are `#[ignore]` because they require GNU less and are
/// slow (PTY-based).
///
/// Reference: SPECIFICATION.md sections on command-line options.
use super::compare;
use super::harness::PagerSession;
use super::helpers::{
    assert_content_conformance, assert_content_conformance_steps, generate_file,
    generate_numbered_file, skip_if_no_less, SETTLE_INITIAL, SETTLE_KEY, TEST_COLS, TEST_ROWS,
};

// ── Status column -J (Tests 1-4) ───────────────────────────────────────────

/// Test 1: `-J` displays a status column on the left.
///
/// With `-J`, less shows a status column. After a search, the column
/// marks matching lines.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_j_status_column_initial() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&["-J"], file.path().to_str().unwrap(), "");
}

/// Test 2: `-J` with a search marks matching lines in the status column.
///
/// After `/Line 05`, the status column should indicate lines containing
/// the match.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_j_status_column_with_search() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-J"], path, "/Line 05\n");
}

/// Test 3: `-J` combined with `-N` line numbers.
///
/// Both status column and line numbers should render without overlap.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_j_status_column_with_line_numbers() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&["-J", "-N"], file.path().to_str().unwrap(), "");
}

/// Test 4: `-J` after scrolling — status column still displays correctly.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_j_status_column_after_scroll() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&["-J"], path, &["/Line 0\n", " "]);
}

// ── Header lines --header (Tests 5-8) ──────────────────────────────────────

/// Test 5: `--header=1` freezes the first line as a header.
///
/// The first line should remain visible even after scrolling.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_header_one_line() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["--header=1"], path, " ");
}

/// Test 6: `--header=3` freezes the first three lines as headers.
///
/// The first three lines should remain visible after scrolling a full page.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_header_three_lines() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["--header=3"], path, " ");
}

/// Test 7: `--header=1` with line numbers `-N`.
///
/// Header line should have its proper line number even after scrolling.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_header_with_line_numbers() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["--header=1", "-N"], path, " ");
}

/// Test 8: `--header=1` at end of file still shows header.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_header_at_eof() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["--header=1"], path, "G");
}

// ── Word wrap --wordwrap (Tests 9-11) ──────────────────────────────────────

/// Test 9: `--wordwrap` wraps long lines at word boundaries.
///
/// Long lines should break at spaces instead of mid-word.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_wordwrap_basic() {
    skip_if_no_less!();
    let content = "This is a very long line that should be wrapped at word boundaries instead of breaking in the middle of a word when the terminal is only eighty columns wide which is a standard width.\n";
    let file = generate_file(&content.repeat(10));
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["--wordwrap"], path, "");
}

/// Test 10: `--wordwrap` with a line that has no spaces.
///
/// A line with no spaces should still display (falls back to character wrap).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_wordwrap_no_spaces() {
    skip_if_no_less!();
    let long_word = "x".repeat(200);
    let content = format!("{long_word}\nNormal line after long word.\n");
    let file = generate_file(&content);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["--wordwrap"], path, "");
}

/// Test 11: `--wordwrap` combined with `-N` line numbers.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_wordwrap_with_line_numbers() {
    skip_if_no_less!();
    let content = "This is a moderately long line that will wrap with word wrapping enabled on a standard eighty column terminal display.\n";
    let file = generate_file(&content.repeat(10));
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["--wordwrap", "-N"], path, "");
}

// ── Window size -z (Tests 12-14) ───────────────────────────────────────────

/// Test 12: `-z10` sets forward window size to 10 lines.
///
/// Space should scroll exactly 10 lines forward.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_z_positive_window_size() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-z10"], path, " ");
}

/// Test 13: `-z-3` sets window size to screen height minus 3.
///
/// Space should scroll (screen_height - 3) lines forward.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_z_negative_window_size() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-z-3"], path, " ");
}

/// Test 14: `-z5` followed by multiple space presses.
///
/// Verify the window size persists across multiple page-forward operations.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_z_persists_across_pages() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&["-z5"], path, &[" ", " ", " "]);
}

// ── Jump target -j (Tests 15-17) ───────────────────────────────────────────

/// Test 15: `-j5` sets the jump target to line 5 on screen.
///
/// After a search, the matched line should appear at row 5 of the screen.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_j_jump_target_integer() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-j5"], path, "/Line 050\n");
}

/// Test 16: `-j.5` sets the jump target to the middle of the screen.
///
/// After a search, the matched line should appear near the middle.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_j_jump_target_decimal() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-j.5"], path, "/Line 050\n");
}

/// Test 17: `-j-1` sets the jump target to the last line of the screen.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_j_jump_target_negative() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-j-1"], path, "/Line 050\n");
}

// ── Incremental search --incsearch (Tests 18-19) ───────────────────────────

/// Test 18: `--incsearch` enables incremental search.
///
/// As the user types characters in the search prompt, the display updates
/// to show the first match incrementally.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_incsearch_basic() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&["--incsearch"], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["--incsearch"], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // Start a search and type part of the pattern.
    pgr.send_keys("/Line 05");
    less.send_keys("/Line 05");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    // Complete the search.
    pgr.send_keys("\n");
    less.send_keys("\n");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    pgr.quit();
    less.quit();
}

/// Test 19: `--incsearch` reverts display when search is cancelled with ESC.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_incsearch_cancel() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&["--incsearch"], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["--incsearch"], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // Start search, type partial pattern, then cancel with ESC.
    pgr.send_keys("/Line 05");
    less.send_keys("/Line 05");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    // Cancel with ESC.
    pgr.send_bytes(b"\x1b");
    less.send_bytes(b"\x1b");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Quit at EOF -e/-E (Tests 20-22) ────────────────────────────────────────

/// Test 20: `-e` quits after reaching EOF the second time.
///
/// With `-e`, the first time EOF is reached, the pager stays.
/// Pressing Space again should quit. We verify the display at EOF.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_e_quit_at_eof_second_time() {
    skip_if_no_less!();
    let file = generate_numbered_file(30);
    let path = file.path().to_str().unwrap();
    // Go to EOF once — should still be in the pager.
    assert_content_conformance(&["-e"], path, "G");
}

/// Test 21: `-E` quits the first time EOF is reached.
///
/// With `-E`, the pager should quit as soon as it reaches EOF.
/// We verify the display just before EOF.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_e_upper_quit_at_eof_first_time() {
    skip_if_no_less!();
    let file = generate_numbered_file(30);
    let path = file.path().to_str().unwrap();
    // Scroll down but not to EOF — verify content before the auto-quit triggers.
    assert_content_conformance(&["-E"], path, "j");
}

/// Test 22: `-e` with a short file that fits on screen.
///
/// If the entire file fits on one screen, `-e` behavior should still work
/// correctly (EOF is already visible).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_e_short_file() {
    skip_if_no_less!();
    let file = generate_numbered_file(10);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-e"], path, "");
}

// ── Buffer limit -b (Tests 23-24) ──────────────────────────────────────────

/// Test 23: `-b10` limits buffer space for pipe input.
///
/// With a limited buffer, behavior on a regular file should still be normal.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_b_buffer_limit_file() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-b10"], path, "");
}

/// Test 24: `-b10` after scrolling — buffer limits should not break display.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_b_buffer_limit_after_scroll() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&["-b10"], path, &[" ", " ", "g"]);
}

// ── Initial command +cmd (Test 25) ─────────────────────────────────────────

/// Test 25: `+G` opens the file at the end.
///
/// The initial command `+G` should position the view at EOF.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_options_initial_command_plus_g() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["+G"], path, "");
}
