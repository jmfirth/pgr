/// Conformance tests for display features: prompt rendering, ANSI handling,
/// line numbers, squeeze blank lines, chop long lines, color system, and tildes.
///
/// Each test spawns both pgr and GNU less with identical arguments and input,
/// then compares terminal screen output. Tests are `#[ignore]` because they
/// require GNU less installed and are slow (PTY-based).
///
/// References: SPECIFICATION.md display-related sections.
use super::compare;
use super::harness::PagerSession;
use super::helpers::{
    assert_content_conformance, assert_prompt_conformance, fixture_path, generate_file,
    generate_numbered_file, skip_if_no_less, SETTLE_INITIAL, SETTLE_KEY, TEST_COLS, TEST_ROWS,
};

// ── Prompt rendering ────────────────────────────────────────────────────────

/// Test 1: Default short prompt displays filename when more content is below.
///
/// With the default prompt, less shows the filename on the initial display.
/// We compare the prompt line directly.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_default_short_prompt() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&[], path, "");
}

/// Test 2: Short prompt at EOF shows `(END)`.
///
/// After scrolling to the end of file with `G`, the prompt should display `(END)`.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_short_prompt_at_eof() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&[], path, "G");
}

/// Test 3: Medium prompt (`-m`) shows filename and percent.
///
/// With `-m`, the prompt includes the filename and a percentage indicator.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_medium_prompt() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-m"], path, "");
}

/// Test 4: Long prompt (`-M`) shows filename, lines, bytes, percent.
///
/// With `-M`, the prompt includes detailed file information.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_long_prompt() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-M"], path, "");
}

/// Test 5: Custom prompt (`-P`) renders the custom format string.
///
/// With `-Ps"page %d"`, the `s` prefix selects the short prompt style and
/// the rest is the custom template.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_custom_prompt() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_prompt_conformance(&["-Ps\"page %d\""], path, "");
}

/// Test 6: Prompt with multiple files shows file count.
///
/// With `-M` and two files, the prompt should indicate "file N of M".
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_prompt_multiple_files() {
    skip_if_no_less!();
    let file1 = generate_numbered_file(50);
    let file2 = generate_numbered_file(50);
    let path1 = file1.path().to_str().unwrap().to_string();
    let path2 = file2.path().to_str().unwrap().to_string();

    let mut pgr = PagerSession::spawn_pgr(&["-M", &path2], &path1, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["-M", &path2], &path1, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    // The prompt line should mention the file count.
    // We compare the prompt content (both should show file info).
    compare::compare_prompts(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── ANSI pass-through ───────────────────────────────────────────────────────

/// Test 7: `-r` raw mode passes all ANSI codes through.
///
/// With `-r`, all escape sequences (including cursor movement) should pass
/// through to the terminal. Both pagers should produce identical output.
#[test]
#[ignore = "less 581 treats ANSI fixture as binary; pgr -r handling differs"]
fn test_conformance_display_ansi_raw_mode() {
    skip_if_no_less!();
    let path = fixture_path("ansi_colors.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&["-r"], path_str, "");
}

/// Test 8: `-R` SGR-only mode passes through SGR sequences, strips others.
///
/// With `-R`, only SGR (Select Graphic Rendition) escape sequences pass
/// through. Other escape codes (cursor movement, etc.) are stripped.
#[test]
#[ignore = "pgr -R SGR passthrough not fully implemented"]
fn test_conformance_display_ansi_sgr_only_mode() {
    skip_if_no_less!();
    let path = fixture_path("ansi_mixed.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&["-R"], path_str, "");
}

/// Test 9: Default mode (no -r/-R) displays ANSI codes as caret notation.
///
/// Without any raw/ANSI flags, escape characters should be displayed as
/// visible caret notation (e.g., `^[` for ESC).
#[test]
#[ignore = "less 581 treats ANSI fixture as binary; pgr caret notation differs"]
fn test_conformance_display_ansi_default_caret_notation() {
    skip_if_no_less!();
    let path = fixture_path("ansi_colors.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&[], path_str, "");
}

/// Test 10: Color preservation with `-R`.
///
/// A file with colored text should display the colors correctly when using
/// `-R`. Both pagers should produce identical colored output.
#[test]
#[ignore = "less 581 treats ANSI fixture as binary; pgr -R handling differs"]
fn test_conformance_display_color_preservation_with_r() {
    skip_if_no_less!();
    let path = fixture_path("ansi_colors.txt");
    let path_str = path.to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&["-R"], path_str, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["-R"], path_str, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    // Compare content area — both should render colors identically.
    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Line numbers (`-N`) ─────────────────────────────────────────────────────

/// Test 11: Line numbers displayed with `-N`.
///
/// With `-N`, line numbers should appear in the left margin.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_line_numbers_shown() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-N"], path, "");
}

/// Test 12: Line number width with a 1000-line file.
///
/// For a file with 1000+ lines, the line number column should be wide enough
/// to accommodate 4-digit numbers.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_line_number_width() {
    skip_if_no_less!();
    let path = fixture_path("numbered_1000.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&["-N"], path_str, "");
}

/// Test 13: Line numbers after scrolling are correct.
///
/// After scrolling down, line numbers should reflect the actual file line
/// numbers, not restart at 1.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_display_line_numbers_after_scroll() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();
    // Scroll down one page then compare.
    assert_content_conformance(&["-N"], path, " ");
}

/// Test 14: Line numbers with long lines and `-S`.
///
/// With `-N -S`, line numbers should not interfere with chopped content.
#[test]
#[ignore = "pgr -N -S: chop markers work but line numbers not shown via CLI flag"]
fn test_conformance_display_line_numbers_with_chop() {
    skip_if_no_less!();
    let path = fixture_path("long_lines.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&["-N", "-S"], path_str, "");
}

// ── Squeeze blank lines (`-s`) ──────────────────────────────────────────────

/// Test 15: Multiple consecutive blank lines are squeezed to one.
///
/// With `-s`, sequences of blank lines are collapsed to a single blank line.
#[test]
#[ignore = "pgr -s squeeze blank lines off-by-one vs less"]
fn test_conformance_display_squeeze_multiple_blanks() {
    skip_if_no_less!();
    let path = fixture_path("blank_groups.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&["-s"], path_str, "");
}

/// Test 16: Single blank lines are preserved with `-s`.
///
/// A single blank line between content should not be removed by squeeze.
#[test]
#[ignore]
fn test_conformance_display_squeeze_preserves_single_blanks() {
    skip_if_no_less!();
    // Create a file with only single blank lines between content.
    let content = "Line one\n\nLine two\n\nLine three\n\nLine four\n";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-s"], path, "");
}

/// Test 17: Squeeze at file start handles leading blank lines.
///
/// Leading blank lines at the start of the file should be squeezed.
#[test]
#[ignore = "pgr -s squeeze blank lines off-by-one vs less"]
fn test_conformance_display_squeeze_at_file_start() {
    skip_if_no_less!();
    let content = "\n\n\n\nFirst content line\nSecond content line\n";
    let file = generate_file(content);
    let path = file.path().to_str().unwrap();
    assert_content_conformance(&["-s"], path, "");
}

// ── Chop long lines (`-S`) ──────────────────────────────────────────────────

/// Test 18: Long lines are chopped at terminal width with `-S`.
///
/// With `-S`, lines longer than the terminal width are truncated rather than
/// wrapped.
#[test]
#[ignore = "conformance: PTY-based, requires GNU less"]
fn test_conformance_display_chop_long_lines() {
    skip_if_no_less!();
    let path = fixture_path("long_lines.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&["-S"], path_str, "");
}

/// Test 19: Chop with horizontal scroll (right arrow).
///
/// With `-S` and a right arrow keypress, the view should shift horizontally,
/// revealing content beyond the terminal width.
#[test]
#[ignore = "less 581 maps right arrow differently than less 668+; chop markers work"]
fn test_conformance_display_chop_with_horizontal_scroll() {
    skip_if_no_less!();
    let path = fixture_path("long_lines.txt");
    let path_str = path.to_str().unwrap();
    // ESC [ C is the right arrow key sequence.
    let mut pgr = PagerSession::spawn_pgr(&["-S"], path_str, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["-S"], path_str, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // Send right arrow to scroll horizontally.
    pgr.send_bytes(b"\x1b[C");
    less.send_bytes(b"\x1b[C");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 20: Default wrapping without `-S`.
///
/// Without `-S`, long lines should wrap to the next row.
#[test]
#[ignore = "pgr line wrapping renders differently from less"]
fn test_conformance_display_default_wrapping() {
    skip_if_no_less!();
    let path = fixture_path("long_lines.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&[], path_str, "");
}

// ── Color system (`-D`) ─────────────────────────────────────────────────────

/// Test 21: `-Ds` sets search highlight color.
///
/// After searching with a custom search color, the highlighted matches
/// should use the specified color. Compare after performing a search.
#[test]
#[ignore = "pgr search scrolls to match line differently than less"]
fn test_conformance_display_search_color() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&["-Ds1"], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["-Ds1"], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    // Perform a search for "Line 0".
    pgr.send_keys("/Line 0\n");
    less.send_keys("/Line 0\n");

    pgr.settle(SETTLE_KEY);
    less.settle(SETTLE_KEY);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 22: `-DP` sets prompt color.
///
/// The prompt line should use the specified color.
#[test]
#[ignore = "less 581 requires --use-color before -D; pgr -D differs"]
fn test_conformance_display_prompt_color() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&["-DP2"], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["-DP2"], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    // Content should be identical regardless of prompt color.
    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Test 23: `-DN` sets line number color with `-N`.
///
/// Line numbers should be rendered in the specified color.
#[test]
#[ignore = "pgr -N line numbers not implemented; less 581 -D requires --use-color"]
fn test_conformance_display_line_number_color() {
    skip_if_no_less!();
    let file = generate_numbered_file(100);
    let path = file.path().to_str().unwrap();

    let mut pgr = PagerSession::spawn_pgr(&["-N", "-DN2"], path, TEST_ROWS, TEST_COLS);
    let mut less = PagerSession::spawn_less(&["-N", "-DN2"], path, TEST_ROWS, TEST_COLS);

    pgr.settle(SETTLE_INITIAL);
    less.settle(SETTLE_INITIAL);

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// ── Tilde lines ─────────────────────────────────────────────────────────────

/// Test 24: Default tilde display after EOF.
///
/// When the file is shorter than the terminal, empty rows after the last
/// line of content should show `~` (tilde), matching less behavior.
#[test]
#[ignore]
fn test_conformance_display_tilde_after_eof() {
    skip_if_no_less!();
    let path = fixture_path("short_5.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&[], path_str, "");
}

/// Test 25: `--tilde` suppresses tildes after EOF.
///
/// With `--tilde` (or `-~`), empty rows after EOF should be blank instead
/// of showing `~`.
#[test]
#[ignore]
fn test_conformance_display_tilde_suppressed() {
    skip_if_no_less!();
    let path = fixture_path("short_5.txt");
    let path_str = path.to_str().unwrap();
    assert_content_conformance(&["--tilde"], path_str, "");
}
