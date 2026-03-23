/// Conformance tests for search behavior.
///
/// Each test spawns both pgr and GNU less with the same input file,
/// sends identical search commands, and compares the resulting screen
/// output. Tests are `#[ignore]` because they require GNU less installed
/// and are slow (PTY-based).
use std::time::Duration;

use super::compare;
use super::helpers::{
    generate_filter_test, generate_search_basic, generate_search_case, generate_search_highlight,
    generate_search_regex, send_keys_to_both, skip_if_no_less, spawn_pair,
};

// =========================================================================
// Basic forward search (tests 1-3)
// =========================================================================

/// `/pattern` finds the first match and scrolls to the matching line.
///
/// SPEC: Search for "error" in a file where the first occurrence is on line 15.
/// Both pagers should scroll to show the matching line.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_forward_finds_match() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `/pattern` with no match shows "Pattern not found" and position unchanged.
///
/// SPEC: Search for a non-existent pattern. Both pagers should display an error
/// message and remain at the same position.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_forward_no_match() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/zzzznotfound\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `/pattern` scrolls to a match near the bottom of the file.
///
/// SPEC: Start at the top, search for "error" which first appears on line 15.
/// The matching line should be visible on screen after the search.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_forward_scrolls_to_match() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Search for a pattern that appears later (line 90).
    // First skip past the early matches.
    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    send_keys_to_both(&mut pgr, &mut less, "n");
    send_keys_to_both(&mut pgr, &mut less, "n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// =========================================================================
// Basic backward search (tests 4-6)
// =========================================================================

/// `?pattern` finds a match above the current position.
///
/// SPEC: Scroll to the middle of the file, then search backward for "error".
/// Both pagers should move the cursor to a match above the current position.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_backward_finds_match() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Jump to the end to establish a position deep in the file.
    send_keys_to_both(&mut pgr, &mut less, "G");

    // Search backward.
    send_keys_to_both(&mut pgr, &mut less, "?error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `?pattern` with no match displays an error.
///
/// SPEC: Search backward for a non-existent pattern.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_backward_no_match() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "G");
    send_keys_to_both(&mut pgr, &mut less, "?zzzznotfound\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `?pattern` from the beginning wraps to the end (default wrap behavior).
///
/// SPEC: At line 1, search backward. If wrap is enabled (default), the search
/// should wrap around to find matches from the end of the file.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_backward_wraps_from_beginning() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // At the beginning, search backward for "error".
    send_keys_to_both(&mut pgr, &mut less, "?error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// =========================================================================
// Repeat search (tests 7-10)
// =========================================================================

/// `n` repeats the forward search.
///
/// SPEC: After `/error`, pressing `n` should find the next occurrence.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_n_repeats_forward() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    send_keys_to_both(&mut pgr, &mut less, "n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `N` reverses the search direction.
///
/// SPEC: After `/error` and `n`, pressing `N` should search backward.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_n_upper_reverses_direction() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    send_keys_to_both(&mut pgr, &mut less, "n");
    send_keys_to_both(&mut pgr, &mut less, "n");
    // Now reverse.
    send_keys_to_both(&mut pgr, &mut less, "N");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Multiple `n` presses find the 3rd, 4th, 5th occurrences.
///
/// SPEC: After `/error`, pressing `n` multiple times should walk through
/// successive matches.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_multiple_n_presses() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    // Walk through all 5 error occurrences (lines 15, 45, 90, 150, 180).
    send_keys_to_both(&mut pgr, &mut less, "n");
    send_keys_to_both(&mut pgr, &mut less, "n");
    send_keys_to_both(&mut pgr, &mut less, "n");
    send_keys_to_both(&mut pgr, &mut less, "n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `n` with no previous search shows "No previous search pattern".
///
/// SPEC: Without a prior search, pressing `n` should produce an error message.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_n_no_previous_search() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Press n without any prior search.
    send_keys_to_both(&mut pgr, &mut less, "n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// =========================================================================
// Numeric prefix with search (tests 11-12)
// =========================================================================

/// `3/pattern` finds the 3rd occurrence of the pattern.
///
/// SPEC: A numeric prefix before the search command jumps to the Nth match.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_numeric_prefix_forward() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Note: in less, the numeric prefix goes before the search command.
    send_keys_to_both(&mut pgr, &mut less, "3/error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `2n` skips one match and goes to the 2nd next occurrence.
///
/// SPEC: After a search, `2n` should skip the immediate next match and land on
/// the one after that.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_numeric_prefix_repeat() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/error\n");
    send_keys_to_both(&mut pgr, &mut less, "2n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// =========================================================================
// Case sensitivity (tests 13-16)
// =========================================================================

/// Default case sensitivity: `/Error` does not match "error".
///
/// SPEC: By default, searches are case-sensitive. Searching for "Error"
/// should not match lines containing only "error".
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_case_sensitive_default() {
    skip_if_no_less!();
    let file = generate_search_case();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/Error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `-i` mode: `/error` (all lowercase) matches "Error" (smart case).
///
/// SPEC: With `-i` active, a search pattern that is all lowercase should
/// match case-insensitively.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_case_insensitive_dash_i() {
    skip_if_no_less!();
    let file = generate_search_case();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Toggle -i mode.
    send_keys_to_both(&mut pgr, &mut less, "-i\n");

    // Search for lowercase pattern -- should match all case variants.
    send_keys_to_both(&mut pgr, &mut less, "/error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `-I` mode: `/Error` matches "error" (always case-insensitive).
///
/// SPEC: With `-I` active, all searches are case-insensitive regardless of
/// the pattern's case.
#[test]
#[ignore = "pgr -I toggle not finding first match; off-by-one"]
fn test_conformance_search_case_insensitive_dash_upper_i() {
    skip_if_no_less!();
    let file = generate_search_case();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Toggle -I mode.
    send_keys_to_both(&mut pgr, &mut less, "-I\n");

    // Search with mixed case -- should still match all variants.
    send_keys_to_both(&mut pgr, &mut less, "/Error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `-i` with uppercase pattern is case-sensitive (smart case behavior).
///
/// SPEC: With `-i` active, a search pattern containing uppercase letters
/// remains case-sensitive. This is the "smart case" behavior of less.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_smart_case_uppercase_pattern() {
    skip_if_no_less!();
    let file = generate_search_case();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Toggle -i (smart case).
    send_keys_to_both(&mut pgr, &mut less, "-i\n");

    // Search with uppercase letter -- should be case-sensitive despite -i.
    send_keys_to_both(&mut pgr, &mut less, "/Error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// =========================================================================
// Search highlighting (tests 17-19)
// =========================================================================

/// Highlight visible matches: after `/pattern`, all matches on screen have
/// standout/reverse video attributes.
///
/// SPEC: After a search, all visible occurrences of the pattern should be
/// highlighted. We compare the content lines to verify the search was
/// executed and the display was updated.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_highlight_visible_matches() {
    skip_if_no_less!();
    let file = generate_search_highlight();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "/target\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// ESC-u toggles search highlighting on and off.
///
/// SPEC: After a search with highlights, pressing ESC-u should toggle
/// the highlight display. Both pagers should show the same content.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_esc_u_toggles_highlighting() {
    skip_if_no_less!();
    let file = generate_search_highlight();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Search to establish highlights.
    send_keys_to_both(&mut pgr, &mut less, "/target\n");

    // Toggle highlights off with ESC-u.
    send_keys_to_both(&mut pgr, &mut less, "\x1bu");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    // Toggle highlights back on.
    send_keys_to_both(&mut pgr, &mut less, "\x1bu");

    let pgr_screen2 = pgr.capture_screen();
    let less_screen2 = less.capture_screen();

    compare::compare_content(&pgr_screen2, &less_screen2);

    pgr.quit();
    less.quit();
}

/// Highlight persists when scrolling to new content.
///
/// SPEC: After a search, scrolling to display new content should highlight
/// any matches that appear on the new screen.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_highlight_persists_on_scroll() {
    skip_if_no_less!();
    let file = generate_search_highlight();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Search to establish highlights.
    send_keys_to_both(&mut pgr, &mut less, "/target\n");

    // Scroll down to see new content.
    send_keys_to_both(&mut pgr, &mut less, " ");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// =========================================================================
// Filter mode (tests 20-22)
// =========================================================================

/// `&pattern` filters to only show lines containing the pattern.
///
/// SPEC: Entering `&error` should display only lines matching "error",
/// hiding all non-matching lines.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_filter_shows_matching_lines() {
    skip_if_no_less!();
    let file = generate_filter_test();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    send_keys_to_both(&mut pgr, &mut less, "&ERROR\n");

    // Give extra time for filter mode to render.
    pgr.wait_and_read(Duration::from_millis(300));
    less.wait_and_read(Duration::from_millis(300));

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `& Enter` clears the active filter and restores all lines.
///
/// SPEC: After filtering with `&error`, entering `&` followed by Enter
/// (empty pattern) should clear the filter and show all lines again.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_filter_clear() {
    skip_if_no_less!();
    let file = generate_filter_test();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Apply filter.
    send_keys_to_both(&mut pgr, &mut less, "&ERROR\n");
    pgr.wait_and_read(Duration::from_millis(300));
    less.wait_and_read(Duration::from_millis(300));

    // Clear filter with empty pattern.
    send_keys_to_both(&mut pgr, &mut less, "&\n");
    pgr.wait_and_read(Duration::from_millis(300));
    less.wait_and_read(Duration::from_millis(300));

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// Filter with inversion (`^N`): show lines NOT matching the pattern.
///
/// SPEC: Entering `&` then Ctrl-N then a pattern should invert the filter,
/// showing only lines that do NOT match.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_filter_inverted() {
    skip_if_no_less!();
    let file = generate_filter_test();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Enter filter mode, press Ctrl-N for inversion, then the pattern.
    // In less, Ctrl-N (0x0e) toggles the NOT modifier within the search prompt.
    send_keys_to_both(&mut pgr, &mut less, "&\x0eERROR\n");
    pgr.wait_and_read(Duration::from_millis(300));
    less.wait_and_read(Duration::from_millis(300));

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

// =========================================================================
// Search modifiers (tests 23-25)
// =========================================================================

/// `^R` literal search: search for regex metacharacters literally.
///
/// SPEC: Pressing Ctrl-R before the pattern disables regex interpretation,
/// treating the pattern as a literal string.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_ctrl_r_literal() {
    skip_if_no_less!();
    let file = generate_search_regex();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Enter search mode, press Ctrl-R for literal mode, then search for "[brackets]".
    // Ctrl-R is 0x12.
    send_keys_to_both(&mut pgr, &mut less, "/\x12[brackets]\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `^W` wrap-around search modifier.
///
/// SPEC: When at the end of file, a forward search with Ctrl-W wraps around
/// to the beginning to find matches above the current position.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_ctrl_w_wrap() {
    skip_if_no_less!();
    let file = generate_search_basic();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Go to end of file.
    send_keys_to_both(&mut pgr, &mut less, "G");

    // Search forward with wrap (Ctrl-W is 0x17).
    send_keys_to_both(&mut pgr, &mut less, "/\x17error\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}

/// `^N` inverted search: find lines NOT matching the pattern.
///
/// SPEC: Pressing Ctrl-N before the pattern inverts the match, finding
/// the next line that does NOT contain the pattern.
#[test]
#[ignore = "conformance: requires GNU less, slow PTY test"]
fn test_conformance_search_ctrl_n_inverted() {
    skip_if_no_less!();
    let file = generate_search_highlight();
    let path = file.path().to_str().unwrap();

    let (mut pgr, mut less) = spawn_pair(&[], path);

    // Search with Ctrl-N (0x0e) for inverted match.
    send_keys_to_both(&mut pgr, &mut less, "/\x0etarget\n");

    let pgr_screen = pgr.capture_screen();
    let less_screen = less.capture_screen();

    compare::compare_content(&pgr_screen, &less_screen);

    pgr.quit();
    less.quit();
}
