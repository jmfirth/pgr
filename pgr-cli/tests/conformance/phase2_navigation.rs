/// Conformance tests for Phase 2 navigation commands.
///
/// Each test spawns both pgr and GNU less with identical arguments and
/// input, sends the same keystrokes, and compares the resulting screen
/// content. Tests are `#[ignore]` because they require GNU less and are
/// slow (PTY-based).
///
/// Reference: SPECIFICATION.md sections on navigation commands.
use super::helpers::{
    assert_content_conformance, assert_content_conformance_byte_steps,
    assert_content_conformance_bytes, assert_content_conformance_steps, generate_long_lines_file,
    generate_numbered_file, skip_if_no_less,
};

// ── z/w window commands (Tests 1-6) ────────────────────────────────────────

/// Test 1: `z` with a count — `15z` then Space scrolls by the new window size.
///
/// After `15z`, the forward window size becomes 15 lines. A subsequent
/// Space should advance by 15 lines.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_z_count_sets_window() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &["15z", " "]);
}

/// Test 2: `z` without a count — scrolls forward by the current window size.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_z_no_count_scrolls_forward() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "z");
}

/// Test 3: `w` with a count — `15w` then `b` scrolls back by 15 lines.
///
/// After `15w`, the backward window size becomes 15 lines. A subsequent
/// `b` should go back by 15 lines.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_w_count_sets_backward_window() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &[" ", " ", " ", "15w", "b"]);
}

/// Test 4: `w` without a count — scrolls backward by the current window size.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_w_no_count_scrolls_backward() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &[" ", " ", "w"]);
}

/// Test 5: `z` then `z` — confirm window size persists across z presses.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_z_persists_window_size() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &["10z", "z", "z"]);
}

/// Test 6: `w` then `w` — confirm backward window size persists.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_w_persists_backward_window_size() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &["G", "10w", "w", "w"]);
}

// ── Horizontal scroll ESC-)/ESC-( (Tests 7-10) ────────────────────────────

/// Test 7: `ESC-)` scrolls right with `-S` and long lines.
///
/// ESC-) is the explicit keybinding for horizontal scroll right in less.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_right_scroll() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_bytes(&["-S"], path, b"\x1b)");
}

/// Test 8: `ESC-(` scrolls left after scrolling right.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_left_scroll() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_byte_steps(&["-S"], path, &[b"\x1b)", b"\x1b)", b"\x1b("]);
}

/// Test 9: Multiple `ESC-)` scrolls right by increasing amounts.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_right_multiple() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 300);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_byte_steps(&["-S"], path, &[b"\x1b)", b"\x1b)", b"\x1b)"]);
}

/// Test 10: `ESC-(` at column 0 does nothing.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_left_at_zero() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_bytes(&["-S"], path, b"\x1b(");
}

// ── ESC-}/ESC-{ scroll to end/home (Tests 11-12) ──────────────────────────

/// Test 11: `ESC-}` scrolls to the right end of the longest line.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_right_end() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_bytes(&["-S"], path, b"\x1b}");
}

/// Test 12: `ESC-{` scrolls back to column 0 after horizontal scrolling.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_left_home() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_byte_steps(&["-S"], path, &[b"\x1b)", b"\x1b)", b"\x1b{"]);
}

// ── p/% go to percent (Tests 13-15) ───────────────────────────────────────

/// Test 13: `50p` goes to 50% of the file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_50_percent() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "50p");
}

/// Test 14: `0p` goes to the beginning of the file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_0_percent() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &[" ", " ", "0p"]);
}

/// Test 15: `100p` goes to the end of the file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_100_percent() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "100p");
}

// ── F follow mode (Tests 16-17) ───────────────────────────────────────────

/// Test 16: `F` enters follow mode, Ctrl-C exits it.
///
/// After entering follow mode and exiting, the display should return to
/// normal viewing at the current position.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_f_follow_enter_and_exit() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_byte_steps(&[], path, &[b"F", b"\x03"]);
}

/// Test 17: `F` after scrolling to middle — follow mode starts from current position.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_f_follow_from_middle() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_byte_steps(&[], path, &[b" ", b"F", b"\x03"]);
}

// ── ESC-j/ESC-k file-line forward/backward (Tests 18-19) ──────────────────

/// Test 18: `ESC-j` scrolls forward one file line (ignoring wrapped lines).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_j_forward_one_file_line() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    let path = file.path().to_str().unwrap();
    // ESC-j: 0x1b followed by 'j'
    assert_content_conformance_bytes(&[], path, b"\x1bj");
}

/// Test 19: `ESC-k` scrolls backward one file line (ignoring wrapped lines).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_esc_k_backward_one_file_line() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    let path = file.path().to_str().unwrap();
    // Scroll down some, then ESC-k
    assert_content_conformance_byte_steps(&[], path, &[b"jjjjj", b"\x1bk"]);
}

// ── J/K force scroll (Tests 20-21) ────────────────────────────────────────

/// Test 20: `J` force-scrolls forward (like j but ignores wrapping count).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_j_upper_force_scroll_forward() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "J");
}

/// Test 21: `K` force-scrolls backward.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_k_upper_force_scroll_backward() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &["jjjjj", "K"]);
}

// ── Numeric prefix with g/G (Tests 22-25) ─────────────────────────────────

/// Test 22: `50g` goes to line 50.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_50g_goto_line_50() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "50g");
}

/// Test 23: `1G` goes to the first line.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_1g_goto_first_line() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance_steps(&[], path, &[" ", " ", "1G"]);
}

/// Test 24: Page forward with `f` (synonym for Space).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_f_page_forward() {
    skip_if_no_less!();
    let file = generate_numbered_file(200);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "f");
}

/// Test 25: Enter key scrolls forward one line.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_nav_enter_scrolls_forward_one_line() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&[], path, "\n");
}
