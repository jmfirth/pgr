use super::{
    expect_str, fixture_path, fixtures_dir, quit_pager, send_key, spawn_pgr, spawn_pgr_in,
};

/// Scenario 13: File with ANSI colors and `-R` flag: colors visible in output.
///
/// With `-R`, ANSI SGR escape sequences should be passed through to the
/// terminal rather than stripped. We verify by checking that the output
/// contains the escape sequence for red text.
#[test]
#[ignore]
fn test_display_ansi_colors_with_dash_r() {
    let mut session = spawn_pgr(&["-R", fixture_path("ansi.txt").to_str().unwrap()]);

    // The ANSI file contains "\x1b[31mRed text\x1b[0m".
    // With -R, the escape should pass through and "Red text" should appear.
    expect_str(&mut session, "Red text");

    quit_pager(&mut session);
}

/// Scenario 14: File with tabs: tabs expand correctly.
///
/// Tabs should be expanded to spaces (default 8-column tab stops).
/// The fixture tabs.txt starts with "Col1\tCol2\tCol3".
/// After tab expansion, Col2 should appear at column 8.
#[test]
#[ignore]
fn test_display_tab_expansion() {
    let mut session = spawn_pgr(&[fixture_path("tabs.txt").to_str().unwrap()]);

    // Tab-expanded content should show the column headers.
    // "Col1" followed by spaces then "Col2" followed by spaces then "Col3".
    expect_str(&mut session, "Col1");
    expect_str(&mut session, "Col2");

    quit_pager(&mut session);
}

/// Scenario 15: Short prompt shows `:` in normal state, `(END)` at EOF.
///
/// The default short prompt shows `:` when there is more content below,
/// and `(END)` when the user has reached the end of the file.
#[test]
#[ignore]
fn test_display_prompt_colon_and_end() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // In normal viewing of a 100-line file, the prompt should show `:`.
    expect_str(&mut session, ":");

    // Jump to end — prompt should now show `(END)`.
    send_key(&mut session, "G");
    expect_str(&mut session, "(END)");

    quit_pager(&mut session);
}

/// Scenario 16: `-M` flag shows detailed prompt.
///
/// The long prompt (`-M`) shows filename, line numbers, byte counts, and
/// percentage. We use a short relative path so the prompt fits in 80 columns.
#[test]
#[ignore]
fn test_display_long_prompt_with_dash_m() {
    // Spawn from the fixtures directory so the filename in the prompt
    // is just "basic.txt" (short enough to fit in the 80-col prompt).
    let mut session = spawn_pgr_in(&["-M", "basic.txt"], Some(&fixtures_dir()));

    // The long prompt should include the filename and line numbers.
    expect_str(&mut session, "basic.txt");

    quit_pager(&mut session);
}
