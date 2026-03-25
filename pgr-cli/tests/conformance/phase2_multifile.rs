/// Conformance tests for Phase 2 multi-file features.
///
/// Each test spawns both pgr and GNU less with identical arguments and
/// input files, sends the same keystrokes, and compares the resulting
/// screen content. Tests are `#[ignore]` because they require GNU less
/// and are slow (PTY-based).
///
/// Reference: SPECIFICATION.md sections 3.5 (file management).
use super::compare;
use super::harness::PagerSession;
use super::helpers::{
    assert_content_conformance_files, assert_content_conformance_files_steps, generate_file,
    generate_numbered_file, skip_if_no_less, SETTLE_INITIAL, SETTLE_KEY, TEST_COLS, TEST_ROWS,
};

// ── :n next file (Tests 1-3) ──────────────────────────────────────────────

/// Test 1: `:n` advances to the next file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_next_file() {
    skip_if_no_less!();
    let file_a = generate_file("Alpha file line 1\nAlpha file line 2\nAlpha file line 3\n");
    let file_b = generate_file("Bravo file line 1\nBravo file line 2\nBravo file line 3\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files(&[], &[path_a, path_b], ":n\n");
}

/// Test 2: `:n` twice with three files goes to the third file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_next_twice() {
    skip_if_no_less!();
    let file_a = generate_file("File A line 1\nFile A line 2\n");
    let file_b = generate_file("File B line 1\nFile B line 2\n");
    let file_c = generate_file("File C line 1\nFile C line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();
    let path_c = file_c.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b, path_c], &[":n\n", ":n\n"]);
}

/// Test 3: `:n` at the last file — should produce an error/stay.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_next_at_last() {
    skip_if_no_less!();
    let file_a = generate_file("File A content\n");
    let file_b = generate_file("File B content\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &[":n\n", ":n\n"]);
}

// ── :p previous file (Tests 4-6) ─────────────────────────────────────────

/// Test 4: `:p` goes back to the previous file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_prev_file() {
    skip_if_no_less!();
    let file_a = generate_file("Alpha content\nAlpha line 2\n");
    let file_b = generate_file("Bravo content\nBravo line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &[":n\n", ":p\n"]);
}

/// Test 5: `:p` at the first file — should produce an error/stay.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_prev_at_first() {
    skip_if_no_less!();
    let file_a = generate_file("File A content\n");
    let file_b = generate_file("File B content\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files(&[], &[path_a, path_b], ":p\n");
}

/// Test 6: `:n` then `:p` round-trips back to original content.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_round_trip() {
    skip_if_no_less!();
    let file_a = generate_numbered_file(30);
    let file_b = generate_numbered_file(30);
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &[":n\n", ":p\n"]);
}

// ── :x first file (Tests 7-8) ────────────────────────────────────────────

/// Test 7: `:x` returns to the first file from any position.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_first_file() {
    skip_if_no_less!();
    let file_a = generate_file("First file content\nFirst line 2\n");
    let file_b = generate_file("Second file content\nSecond line 2\n");
    let file_c = generate_file("Third file content\nThird line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();
    let path_c = file_c.path().to_str().unwrap();

    assert_content_conformance_files_steps(
        &[],
        &[path_a, path_b, path_c],
        &[":n\n", ":n\n", ":x\n"],
    );
}

/// Test 8: `:x` when already on the first file is a no-op.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_first_when_already_first() {
    skip_if_no_less!();
    let file_a = generate_file("First file content\n");
    let file_b = generate_file("Second file content\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files(&[], &[path_a, path_b], ":x\n");
}

// ── :d remove file (Tests 9-10) ──────────────────────────────────────────

/// Test 9: `:d` removes the current file and shows the next.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_delete_current() {
    skip_if_no_less!();
    let file_a = generate_file("File A to be removed\n");
    let file_b = generate_file("File B remains\nFile B line 2\n");
    let file_c = generate_file("File C remains\nFile C line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();
    let path_c = file_c.path().to_str().unwrap();

    assert_content_conformance_files(&[], &[path_a, path_b, path_c], ":d\n");
}

/// Test 10: `:d` on the last file — should show previous file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_delete_last_file() {
    skip_if_no_less!();
    let file_a = generate_file("File A content\nFile A line 2\n");
    let file_b = generate_file("File B to be removed\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &[":n\n", ":d\n"]);
}

// ── :e examine new file (Tests 11-12) ────────────────────────────────────

/// Test 11: `:e filename` opens a new file for examination.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_examine_new_file() {
    skip_if_no_less!();
    let file_a = generate_file("Original file line 1\nOriginal file line 2\n");
    let file_b = generate_file("Examined file line 1\nExamined file line 2\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b_owned = file_b.path().to_str().unwrap().to_string();

    let mut pgr = PagerSession::spawn_pgr(&[], path_a, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&[], path_a, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let keys = format!(":e {path_b_owned}\n");
    pgr.send_keys(&keys);
    less.send_keys(&keys);

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 12: `:e` after examine — can navigate back with `:p`.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_examine_then_prev() {
    skip_if_no_less!();
    let file_a = generate_file("Original file content\n");
    let file_b = generate_file("Examined file content\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b_owned = file_b.path().to_str().unwrap().to_string();

    let mut pgr = PagerSession::spawn_pgr(&[], path_a, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&[], path_a, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // Examine new file, then go back.
    let examine_keys = format!(":e {path_b_owned}\n");
    pgr.send_keys(&examine_keys);
    less.send_keys(&examine_keys);
    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    pgr.send_keys(":p\n");
    less.send_keys(":p\n");
    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Cross-file search (Tests 13-15) ──────────────────────────────────────

/// Test 13: Search finds match in current file across file boundary.
///
/// Open two files, search for a pattern in the first file. After exhausting
/// matches in the first, ESC-n finds it in the second.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_cross_file_search() {
    skip_if_no_less!();
    let file_a = generate_file("File A normal line\nFile A normal line\n");
    let file_b = generate_file("File B has target here\nFile B normal line\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &["/target\n", "\x1bn"]);
}

/// Test 14: Cross-file search backward with ESC-N.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_cross_file_search_backward() {
    skip_if_no_less!();
    let file_a = generate_file("File A has target here\nFile A line 2\n");
    let file_b = generate_file("File B normal line\nFile B normal line\n");
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    assert_content_conformance_files_steps(&[], &[path_a, path_b], &[":n\n", "/target\n", "\x1bN"]);
}

/// Test 15: Multi-file with `-M` prompt shows file position information.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_multifile_long_prompt_info() {
    skip_if_no_less!();
    let file_a = generate_numbered_file(30);
    let file_b = generate_numbered_file(30);
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr_files(&["-M"], &[path_a, path_b], TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less_files(&["-M"], &[path_a, path_b], TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}
