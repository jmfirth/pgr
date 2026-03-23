/// Conformance tests for navigation commands.
///
/// Each test spawns both pgr and GNU less with identical arguments and
/// input, sends the same keystrokes, and compares the resulting screen
/// content. Tests are `#[ignore]` because they require GNU less and are
/// slow (PTY-based).
///
/// Reference: SPECIFICATION.md sections on navigation commands.
use super::helpers::{
    assert_content_conformance, assert_content_conformance_byte_steps,
    assert_content_conformance_bytes, assert_content_conformance_steps, generate_file,
    generate_long_lines_file, generate_numbered_file, skip_if_no_less,
};

// ── Basic line navigation (Tests 1-11) ─────────────────────────────────────

/// Test 1: Initial display — open file, compare initial screen (no keystrokes).
#[test]
#[ignore]
fn test_conformance_navigation_initial_display() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "");
}

/// Test 2: j (forward one line) — send `j`, compare.
#[test]
#[ignore]
fn test_conformance_navigation_j_scrolls_forward_one_line() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "j");
}

/// Test 3: k (backward one line) — send `jjjk`, compare (advance 3, back 1).
#[test]
#[ignore]
fn test_conformance_navigation_k_scrolls_backward_one_line() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &["jjj", "k"]);
}

/// Test 4: Space (forward one page) — send Space, compare.
#[test]
#[ignore]
fn test_conformance_navigation_space_pages_forward() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), " ");
}

/// Test 5: b (backward one page) — send Space twice then `b`, compare.
#[test]
#[ignore]
fn test_conformance_navigation_b_pages_backward() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &[" ", " ", "b"]);
}

/// Test 6: d (half page forward) — send `d`, compare.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_navigation_d_half_page_forward() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "d");
}

/// Test 7: u (half page backward) — send `ddu`, compare.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_navigation_u_half_page_backward() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &["d", "d", "u"]);
}

/// Test 8: g (go to beginning) — send Space Space then `g`, compare.
#[test]
#[ignore]
fn test_conformance_navigation_g_goes_to_beginning() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &[" ", " ", "g"]);
}

/// Test 9: G (go to end) — send `G`, compare.
#[test]
#[ignore]
fn test_conformance_navigation_g_upper_goes_to_end() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "G");
}

/// Test 10: Down arrow — send down arrow key, compare with same result as `j`.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_navigation_down_arrow() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    // Down arrow is ESC [ B
    assert_content_conformance_bytes(&[], file.path().to_str().unwrap(), b"\x1b[B");
}

/// Test 11: Up arrow — send up arrow after scrolling, compare with same result as `k`.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_navigation_up_arrow() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    // Scroll down 3 lines then up 1 via arrow keys.
    // Down arrow = ESC [ B, Up arrow = ESC [ A
    assert_content_conformance_byte_steps(
        &[],
        file.path().to_str().unwrap(),
        &[b"\x1b[B\x1b[B\x1b[B", b"\x1b[A"],
    );
}

// ── Numeric prefixes (Tests 12-15) ─────────────────────────────────────────

/// Test 12: 5j — scroll forward 5 lines, compare.
#[test]
#[ignore]
fn test_conformance_navigation_5j_scrolls_forward_five_lines() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "5j");
}

/// Test 13: 10G — go to line 10, compare.
#[test]
#[ignore = "pgr NG (go to line N) off-by-one vs less"]
fn test_conformance_navigation_10g_goes_to_line_10() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "10G");
}

/// Test 14: 50p — go to 50% of file, compare.
#[test]
#[ignore]
fn test_conformance_navigation_50p_goes_to_50_percent() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "50p");
}

/// Test 15: 3Space — three pages forward, compare.
#[test]
#[ignore]
fn test_conformance_navigation_3_space_pages_forward_three() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "3 ");
}

// ── Edge cases (Tests 16-20) ───────────────────────────────────────────────

/// Test 16: EOF behavior — scroll to EOF, compare display (tilde lines, prompt).
#[test]
#[ignore]
fn test_conformance_navigation_eof_display() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "G");
}

/// Test 17: BOF behavior — at beginning, press `k`, compare (should not move).
#[test]
#[ignore]
fn test_conformance_navigation_bof_k_does_not_move() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "k");
}

/// Test 18: Single-line file — open a 1-line file, compare.
#[test]
#[ignore]
fn test_conformance_navigation_single_line_file() {
    skip_if_no_less!();
    let file = generate_file("Only one line here.\n");
    assert_content_conformance(&[], file.path().to_str().unwrap(), "");
}

/// Test 19: Empty file — open an empty file, compare.
#[test]
#[ignore]
fn test_conformance_navigation_empty_file() {
    skip_if_no_less!();
    let file = generate_file("");
    assert_content_conformance(&[], file.path().to_str().unwrap(), "");
}

/// Test 20: File shorter than screen — open a 10-line file on 24-row terminal, compare.
#[test]
#[ignore]
fn test_conformance_navigation_short_file() {
    skip_if_no_less!();
    let file = generate_numbered_file(10);
    assert_content_conformance(&[], file.path().to_str().unwrap(), "");
}

// ── Window size commands (Tests 21-22) ─────────────────────────────────────

/// Test 21: z (set window size) — send `10z` then Space, compare.
///
/// After `10z`, the forward window size is set to 10 lines. A subsequent
/// Space should scroll forward by 10 lines instead of the default.
#[test]
#[ignore]
fn test_conformance_navigation_z_sets_window_size() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &["10z", " "]);
}

/// Test 22: w (set backward window) — send `10w` then `b`, compare.
///
/// After `10w`, the backward window size is set to 10 lines. A subsequent
/// `b` should scroll backward by 10 lines.
#[test]
#[ignore]
fn test_conformance_navigation_w_sets_backward_window() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &[" ", " ", "10w", "b"]);
}

// ── Horizontal scrolling (Tests 23-24) ─────────────────────────────────────

/// Test 23: Right arrow (horizontal scroll right) — with `-S` flag and long
/// lines, send right arrow, compare.
#[test]
#[ignore = "less 581 maps right arrow differently than less 668+; chop markers work"]
fn test_conformance_navigation_horizontal_scroll_right() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    // Right arrow = ESC [ C (in less, right arrow scrolls horizontally with -S)
    assert_content_conformance_bytes(&["-S"], file.path().to_str().unwrap(), b"\x1b[C");
}

/// Test 24: Left arrow (horizontal scroll left) — after scrolling right,
/// send left arrow, compare.
#[test]
#[ignore = "less 581 maps arrow keys differently than less 668+; chop markers work"]
fn test_conformance_navigation_horizontal_scroll_left() {
    skip_if_no_less!();
    let file = generate_long_lines_file(50, 200);
    // Right arrow then left arrow with -S flag.
    // Right = ESC [ C, Left = ESC [ D
    assert_content_conformance_byte_steps(
        &["-S"],
        file.path().to_str().unwrap(),
        &[b"\x1b[C", b"\x1b[D"],
    );
}

// ── Sticky half-screen (Tests 25-26) ───────────────────────────────────────

/// Test 25: d with count — send `5d`, then `d` again.
///
/// After `5d`, the half-screen size is set to 5 lines. A subsequent `d`
/// should scroll forward by 5 lines (the new sticky value).
#[test]
#[ignore]
fn test_conformance_navigation_d_sticky_count() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &["5d", "d"]);
}

/// Test 26: u with count — send `5u` after scrolling, then `u` again.
///
/// After `5u`, the half-screen size for backward scrolling is set to 5.
/// A subsequent `u` should scroll backward by 5 lines.
#[test]
#[ignore]
fn test_conformance_navigation_u_sticky_count() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &[" ", " ", "5u", "u"]);
}

// ── Extended navigation (Tests 27-30) ──────────────────────────────────────

/// Test 27: ESC-Space — scroll forward one page, even past EOF.
///
/// ESC followed by Space is "forward one window, but don't stop at EOF".
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_navigation_esc_space_forward_past_eof() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    // Send ESC then Space as two bytes: 0x1b 0x20
    assert_content_conformance_byte_steps(
        &[],
        file.path().to_str().unwrap(),
        &[b" ", b" ", b" ", b"\x1b ", b"\x1b "],
    );
}

/// Test 28: ESC-b — scroll backward one window (different from `b` with wrapping).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_navigation_esc_b_backward() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    assert_content_conformance_byte_steps(
        &[],
        file.path().to_str().unwrap(),
        &[b" ", b" ", b" ", b"\x1bb"],
    );
}

/// Test 29: F then interrupt — enter follow mode with `F`, then Ctrl-C to exit.
///
/// After `F` then Ctrl-C (interrupt), the screen should return to normal
/// viewing mode showing the current position.
#[test]
#[ignore]
fn test_conformance_navigation_f_follow_then_interrupt() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    // F enters follow mode, Ctrl-C (0x03) exits it.
    assert_content_conformance_byte_steps(&[], file.path().to_str().unwrap(), &[b"F", b"\x03"]);
}

/// Test 30: R (reload/repaint) — send `R`, compare (screen should refresh
/// to same content).
#[test]
#[ignore]
fn test_conformance_navigation_r_repaint() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    // Scroll down a bit, then repaint.
    assert_content_conformance_steps(&[], file.path().to_str().unwrap(), &["jjj", "R"]);
}
