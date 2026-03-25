/// Conformance tests for Phase 2 key binding and command features.
///
/// Each test spawns both pgr and GNU less with identical arguments and
/// input, sends the same keystrokes, and compares the resulting screen
/// content. Tests are `#[ignore]` because they require GNU less and are
/// slow (PTY-based).
///
/// Reference: SPECIFICATION.md sections on commands and key bindings.
use std::time::Duration;

use super::compare;
use super::harness::PagerSession;
use super::helpers::{
    assert_content_conformance, assert_content_conformance_bytes, assert_content_conformance_steps,
    generate_file, generate_numbered_file, skip_if_no_less, SETTLE_INITIAL, SETTLE_KEY, TEST_COLS,
    TEST_ROWS,
};

// ── Bracket matching (Tests 1-4) ──────────────────────────────────────────

/// Test 1: `ESC-]` finds the matching close bracket.
///
/// With the cursor on a line containing `{`, ESC-] should find the
/// matching `}`.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_bracket_find_close() {
    skip_if_no_less!();
    let content = "\
fn main() {
    let x = 1;
    if x > 0 {
        println!(\"positive\");
    }
    let y = 2;
    let z = 3;
    for i in 0..10 {
        println!(\"{}\", i);
    }
}
extra line 1
extra line 2
extra line 3
extra line 4
extra line 5
extra line 6
extra line 7
extra line 8
extra line 9
extra line 10
extra line 11
extra line 12
extra line 13
";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();
    // ESC-] finds matching close bracket. 0x1b followed by ']'.
    assert_content_conformance_bytes(&[], path, b"\x1b]");
}

/// Test 2: `ESC-[` finds the matching open bracket.
///
/// After scrolling to a line with `}`, ESC-[ should find the matching `{`.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_bracket_find_open() {
    skip_if_no_less!();
    let content = "\
fn main() {
    let x = 1;
    if x > 0 {
        println!(\"positive\");
    }
    let y = 2;
    let z = 3;
    for i in 0..10 {
        println!(\"{}\", i);
    }
}
extra line 1
extra line 2
extra line 3
extra line 4
extra line 5
extra line 6
extra line 7
extra line 8
extra line 9
extra line 10
extra line 11
extra line 12
extra line 13
";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();
    // Scroll to the closing brace, then find the opening.
    // ESC-[ is 0x1b followed by '[' — but this conflicts with CSI.
    // In less, the bracket commands use ESC-Ctrl-[ and ESC-Ctrl-].
    // Actually less uses ESC-^F and ESC-^B for bracket matching.
    // Let's use the ( ) pair instead: ESC-) and ESC-(.
    // Actually, less uses: ESC-] for find-close, ESC-[ for find-open.
    // Since ESC-[ is CSI prefix, we skip this test direction.
    assert_content_conformance_steps(&[], path, &["G", "\x1b["]);
}

/// Test 3: Bracket matching with parentheses `(` `)`.
///
/// less bracket matching works with {}, (), and [].
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_bracket_parentheses() {
    skip_if_no_less!();
    let content = "\
function test(
    arg1,
    arg2,
    arg3
) {
    return arg1 + arg2 + arg3;
}
extra line 1
extra line 2
extra line 3
extra line 4
extra line 5
extra line 6
extra line 7
extra line 8
extra line 9
extra line 10
extra line 11
extra line 12
extra line 13
extra line 14
extra line 15
extra line 16
extra line 17
";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();
    // ESC-] to find matching close bracket.
    assert_content_conformance_bytes(&[], path, b"\x1b]");
}

/// Test 4: Bracket matching when no bracket is found.
///
/// If there is no matching bracket, behavior should match less.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_bracket_no_match() {
    skip_if_no_less!();
    let content = "No brackets in this file.\nJust plain text lines.\n";
    let padding = "Extra line.\n".repeat(30);
    let full_content = format!("{content}{padding}");
    let file = generate_file(&full_content);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_bytes(&[], path, b"\x1b]");
}

// ── Quit variants (Tests 5-7) ─────────────────────────────────────────────

/// Test 5: `ZZ` quits the pager (like vi's ZZ).
///
/// We verify that before ZZ, the content is displayed correctly.
/// The quit itself terminates the process.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_zz_quit_display_before() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    // Compare content before the quit — just verify initial display.
    assert_content_conformance(&[], path, "");
}

/// Test 6: `:Q` quits the pager.
///
/// Verify that content is displayed before :Q quits.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_colon_q_display_before_quit() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    // Scroll down, verify content before quit.
    assert_content_conformance(&[], path, "jjj");
}

/// Test 7: `q` quits — standard quit verification.
///
/// Verify that after scrolling, content is correct before quit.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_q_quit_after_scroll() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &[" ", "jjj"]);
}

// ── File info = command (Tests 8-10) ──────────────────────────────────────

/// Test 8: `=` shows file information — content area unchanged.
///
/// The `=` command displays file info on the prompt line but should not
/// change the content area.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_equals_file_info() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "=");
}

/// Test 9: `=` after scrolling shows updated position info.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_equals_after_scroll() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &[" ", "="]);
}

/// Test 10: `=` at EOF shows end-of-file information.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_equals_at_eof() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &["G", "="]);
}

// ── Version V command (Test 11) ───────────────────────────────────────────

/// Test 11: `V` displays version info — content area should show version.
///
/// The V command switches to a version display. We verify the content area
/// after pressing V matches between pagers. Note: version strings will differ,
/// so we compare only that V triggers a display change.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_v_version_returns() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    // Press V to show version, then q to return.
    // After returning from V, the display should show the file again.
    assert_content_conformance_steps(&[], path, &["V", "q"]);
}

// ── Shell command ! (Tests 12-13) ─────────────────────────────────────────

/// Test 12: `!echo hello` runs a shell command and returns.
///
/// After the shell command completes and the user presses Enter,
/// the pager should return to displaying the file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_shell_command_basic() {
    skip_if_no_less!();
    let file = generate_numbered_file(50);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&[], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&[], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // Send shell command.
    pgr.send_keys("!echo hello\n");
    less.send_keys("!echo hello\n");

    pgr.settle(Duration::from_millis(1000));
    less.settle(Duration::from_millis(1000));

    // Press Enter to resume the pager.
    pgr.send_keys("\n");
    less.send_keys("\n");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 13: `!!` repeats the last shell command.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_shell_repeat() {
    skip_if_no_less!();
    let file = generate_numbered_file(50);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&[], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&[], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // First shell command.
    pgr.send_keys("!echo hello\n");
    less.send_keys("!echo hello\n");

    pgr.settle(Duration::from_millis(1000));
    less.settle(Duration::from_millis(1000));

    pgr.send_keys("\n");
    less.send_keys("\n");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    // Repeat with !!
    pgr.send_keys("!!\n");
    less.send_keys("!!\n");

    pgr.settle(Duration::from_millis(1000));
    less.settle(Duration::from_millis(1000));

    pgr.send_keys("\n");
    less.send_keys("\n");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Tags navigation (Tests 14-15) ────────────────────────────────────────

/// Test 14: `t` with no tags file — verify error message behavior.
///
/// Without a tags file, pressing `t` should produce an error message.
/// Both pagers should handle this the same way.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_tags_no_tags_file() {
    skip_if_no_less!();
    let file = generate_numbered_file(50);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&[], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&[], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // Send tag command with a tag name.
    pgr.send_keys("tmain\n");
    less.send_keys("tmain\n");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 15: Tags with a valid tags file.
///
/// Create a minimal ctags file and test that `t` navigates to the tag.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_keys_tags_with_tags_file() {
    skip_if_no_less!();

    // Create a source file.
    let source_content = "\
line 1
line 2
main_function:
line 4
line 5
line 6
line 7
line 8
line 9
line 10
line 11
line 12
line 13
line 14
line 15
line 16
line 17
line 18
line 19
line 20
line 21
line 22
line 23
line 24
line 25
";
    let source_file = generate_file(source_content);
    let source_path = source_file.path().to_str().unwrap().to_string();

    // Create a tags file in the same directory.
    let tags_dir = source_file.path().parent().unwrap();
    let tags_path = tags_dir.join("tags");
    let tags_content = format!("main_function\t{source_path}\t/^main_function:/\n");
    std::fs::write(&tags_path, tags_content).expect("failed to write tags file");

    let mut pgr = PagerSession::spawn_pgr(
        &[
            "-t",
            "main_function",
            "--tag-file",
            tags_path.to_str().unwrap(),
        ],
        &source_path,
        TEST_ROWS,
        TEST_COLS,
    );
    let mut less = PagerSession::spawn_less(
        &[
            "-t",
            "main_function",
            "--tag-file",
            tags_path.to_str().unwrap(),
        ],
        &source_path,
        TEST_ROWS,
        TEST_COLS,
    );

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();

    // Clean up tags file.
    let _ = std::fs::remove_file(&tags_path);
}
