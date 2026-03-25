/// Conformance tests for Phase 2 search features.
///
/// Each test spawns both pgr and GNU less with identical arguments and
/// input, sends the same keystrokes, and compares the resulting screen
/// content. Tests are `#[ignore]` because they require GNU less and are
/// slow (PTY-based).
///
/// Reference: SPECIFICATION.md sections on search commands and modifiers.
use super::compare;
use super::helpers::{
    assert_content_conformance_files_steps, generate_file, generate_numbered_file,
    generate_search_basic, generate_search_highlight, generate_search_regex, send_keys_to_both,
    skip_if_no_less, spawn_pair,
};

// ── ESC-u toggle highlight (Tests 1-3) ─────────────────────────────────────

/// Test 1: `ESC-u` toggles search highlighting off.
///
/// After a search, ESC-u should turn off highlighting. The content should
/// remain the same (highlights are a rendering attribute, not content).
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_u_toggle_off() {
    skip_if_no_less!();
    let file = generate_search_highlight();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/target\n");
    // Toggle highlights off.
    send_keys_to_both(&mut pgr, &mut less, "\x1bu");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 2: `ESC-u` twice toggles highlighting back on.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_u_toggle_on_again() {
    skip_if_no_less!();
    let file = generate_search_highlight();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/target\n");
    // Toggle off then on.
    send_keys_to_both(&mut pgr, &mut less, "\x1bu");
    send_keys_to_both(&mut pgr, &mut less, "\x1bu");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 3: `ESC-u` before any search has no visible effect.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_u_before_search() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Toggle highlight with no active search.
    send_keys_to_both(&mut pgr, &mut less, "\x1bu");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── ESC-U clear search pattern (Tests 4-5) ────────────────────────────────

/// Test 4: `ESC-U` clears the active search pattern.
///
/// After a search and ESC-U, pressing `n` should produce a
/// "No previous search pattern" error.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_upper_u_clears_pattern() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    // Clear pattern.
    send_keys_to_both(&mut pgr, &mut less, "\x1bU");
    // Try to repeat search — should fail.
    send_keys_to_both(&mut pgr, &mut less, "n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 5: `ESC-U` followed by a new search works normally.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_upper_u_then_new_search() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    send_keys_to_both(&mut pgr, &mut less, "\x1bU");
    send_keys_to_both(&mut pgr, &mut less, "/normal\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Cross-file search ESC-n/ESC-N (Tests 6-9) ────────────────────────────

/// Test 6: `ESC-n` continues search into the next file.
///
/// With two files open and a search active, ESC-n should find the match
/// in the next file if no more matches exist in the current file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_n_cross_file() {
    skip_if_no_less!();
    let file_a = generate_file("File A line 1\nFile A line 2\nFile A line 3\n");
    let file_b = generate_file("File B line 1\nFile B target line\nFile B line 3\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &["/target\n", "\x1bn"]);
}

/// Test 7: `ESC-N` searches backward across files.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_upper_n_cross_file_backward() {
    skip_if_no_less!();
    let file_a = generate_file("File A target line\nFile A line 2\nFile A line 3\n");
    let file_b = generate_file("File B line 1\nFile B line 2\nFile B line 3\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &[":n\n", "/target\n", "\x1bN"]);
}

/// Test 8: `ESC-/` opens cross-file forward search prompt.
///
/// ESC-/ should search across all files from the current position forward.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_slash_cross_file() {
    skip_if_no_less!();
    let file_a = generate_file("File A line 1\nFile A line 2\n");
    let file_b = generate_file("File B line 1\nFile B marker line\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &["\x1b/marker\n"]);
}

/// Test 9: `ESC-?` opens cross-file backward search prompt.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_esc_question_cross_file() {
    skip_if_no_less!();
    let file_a = generate_file("File A marker line\nFile A line 2\n");
    let file_b = generate_file("File B line 1\nFile B line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &[":n\n", "\x1b?marker\n"]);
}

// ── Search modifiers (Tests 10-15) ────────────────────────────────────────

/// Test 10: `^K` keep position — after search, stay at current position.
///
/// With ^K modifier, the search finds the pattern but does not scroll
/// to the match.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_ctrl_k_keep_position() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // ^K is 0x0b.
    send_keys_to_both(&mut pgr, &mut less, "/\x0berror\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 11: `^R` regex toggle — search for literal regex metacharacters.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_ctrl_r_regex_toggle() {
    skip_if_no_less!();
    let file = generate_search_regex();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // ^R is 0x12. Search for literal "[brackets]".
    send_keys_to_both(&mut pgr, &mut less, "/\x12[brackets]\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 12: `^N` inverted search — find lines NOT matching the pattern.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_ctrl_n_inverted_search() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // ^N is 0x0e. Search for lines NOT containing "error".
    send_keys_to_both(&mut pgr, &mut less, "/\x0eerror\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 13: `^W` wraps the search around EOF.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_ctrl_w_wrap_around() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Go to end.
    send_keys_to_both(&mut pgr, &mut less, "G");
    // ^W is 0x17. Search forward with wrap.
    send_keys_to_both(&mut pgr, &mut less, "/\x17error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 14: `^E` modifier — search to end of file without wrapping.
///
/// With ^E, the search should not wrap past EOF.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_ctrl_e_no_wrap() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Navigate close to end, search forward with ^E (0x05) to avoid wrapping.
    send_keys_to_both(&mut pgr, &mut less, "G");
    send_keys_to_both(&mut pgr, &mut less, "/\x05error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 15: `^F` modifier — search from first line (ignoring current position).
///
/// With ^F, the search should start from the first line of the file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_ctrl_f_from_first_line() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Scroll to middle.
    send_keys_to_both(&mut pgr, &mut less, " ");
    send_keys_to_both(&mut pgr, &mut less, " ");
    // ^F is 0x06. Search from first line.
    send_keys_to_both(&mut pgr, &mut less, "/\x06error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Search with -j jump target (Tests 16-18) ─────────────────────────────

/// Test 16: Search with `-j5` positions the match at line 5 on screen.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_with_jump_target() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&["-j5"], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 17: `n` with `-j5` — repeat search respects jump target.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_repeat_with_jump_target() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&["-j5"], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    send_keys_to_both(&mut pgr, &mut less, "n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 18: Backward search `?` also uses jump target.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_backward_with_jump_target() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&["-j5"], path);

    send_keys_to_both(&mut pgr, &mut less, "G");
    send_keys_to_both(&mut pgr, &mut less, "?error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Search with -J status column (Tests 19-20) ───────────────────────────

/// Test 19: `-J` marks search matches in status column.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_status_column_marks() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&["-J"], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 20: `-J` status marks persist after scrolling.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_search_status_column_after_scroll() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&["-J"], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    send_keys_to_both(&mut pgr, &mut less, " ");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}
