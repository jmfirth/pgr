/// Conformance tests for Phase 2 prompt features.
///
/// Each test spawns both pgr and GNU less with identical arguments and
/// input, sends the same keystrokes, and compares the resulting screen
/// or prompt output. Tests are `#[ignore]` because they require GNU less
/// and are slow (PTY-based).
///
/// Reference: SPECIFICATION.md sections on prompt rendering and escapes.
use super::compare;
use super::harness::PagerSession;
use super::helpers::{
    assert_prompt_conformance, generate_numbered_file, skip_if_no_less, SETTLE_INITIAL, TEST_COLS,
    TEST_ROWS,
};

// ── Named prompts -Ps, -Pm, -PM (Tests 1-4) ─────────────────────────────

/// Test 1: `-Ps` custom short prompt.
///
/// Custom short prompt string should be rendered on the prompt line.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_ps_custom_short() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"custom prompt\""], path, "");
}

/// Test 2: `-Pm` custom medium prompt.
///
/// The medium prompt is used when `-m` is active.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_pm_custom_medium() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-m", "-Pm\"%f line %l\""], path, "");
}

/// Test 3: `-PM` custom long prompt.
///
/// The long prompt is used when `-M` is active.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_pm_upper_custom_long() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-M", "-PM\"%f lines %l-%L\""], path, "");
}

/// Test 4: Named prompt at EOF.
///
/// The short prompt at EOF should show `(END)` or the custom EOF prompt.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_named_at_eof() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&[], path, "G");
}

// ── Prompt escapes %f, %l, %L, %p, %i, %m (Tests 5-10) ─────────────────

/// Test 5: `%f` in custom prompt expands to filename.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_escape_f_filename() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"File: %f\""], path, "");
}

/// Test 6: `%l` in custom prompt expands to current line number.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_escape_l_line_number() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"Line: %l\""], path, " ");
}

/// Test 7: `%L` in custom prompt expands to total line count.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_escape_upper_l_total_lines() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"Total: %L\""], path, "");
}

/// Test 8: `%p` in custom prompt expands to byte percentage.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_escape_p_percent() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"Position: %p%%\""], path, " ");
}

/// Test 9: `%i` in custom prompt expands to file index in multi-file mode.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_escape_i_file_index() {
    skip_if_no_less!();
    let file_a = generate_numbered_file(30);
    let file_b = generate_numbered_file(30);
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr_files(
        &["-Ps\"File %i of %m\""],
        &[path_a, path_b],
        TEST_ROWS,
        TEST_COLS,
    );
    let mut less = PagerSession::spawn_less_files(
        &["-Ps\"File %i of %m\""],
        &[path_a, path_b],
        TEST_ROWS,
        TEST_COLS,
    );

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_prompts(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 10: `%m` in custom prompt expands to total file count.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_escape_m_file_count() {
    skip_if_no_less!();
    let file_a = generate_numbered_file(30);
    let file_b = generate_numbered_file(30);
    let file_c = generate_numbered_file(30);
    let path_a = file_a.path().to_str().unwrap();
    let path_b = file_b.path().to_str().unwrap();
    let path_c = file_c.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr_files(
        &["-Ps\"%m files\""],
        &[path_a, path_b, path_c],
        TEST_ROWS,
        TEST_COLS,
    );
    let mut less = PagerSession::spawn_less_files(
        &["-Ps\"%m files\""],
        &[path_a, path_b, path_c],
        TEST_ROWS,
        TEST_COLS,
    );

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_prompts(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Prompt with conditionals (Tests 11-12) ───────────────────────────────

/// Test 11: Conditional prompt `?e(END):more.` — shows "more" when not at EOF.
///
/// The conditional syntax `?eX:Y.` shows X if at EOF, Y otherwise.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_conditional_not_at_eof() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"?e(END):more.\""], path, "");
}

/// Test 12: Conditional prompt `?e(END):more.` — shows "(END)" at EOF.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_phase2_prompt_conditional_at_eof() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"?e(END):more.\""], path, "G");
}
