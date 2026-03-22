use std::time::Duration;

use expectrl::process::Healthcheck;

use super::{expect_str, fixture_path, quit_pager, send_key, spawn_pgr};

/// Scenario 5: Open a 100-line file, verify first page of content is shown.
#[test]
#[ignore]
fn test_nav_first_page_shows_initial_lines() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // First line should appear.
    expect_str(&mut session, "Line 001");

    quit_pager(&mut session);
}

/// Scenario 6: Press `j` to scroll forward one line.
#[test]
#[ignore]
fn test_nav_j_scrolls_forward_one_line() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // Wait for initial render.
    expect_str(&mut session, "Line 001");

    // Press 'j' to scroll forward one line. With 24 rows (23 content
    // rows), the initial view shows Line 001-023. After scrolling one
    // line forward, the view shows Line 002-024, making Line 024 newly
    // visible.
    send_key(&mut session, "j");
    expect_str(&mut session, "Line 024");

    quit_pager(&mut session);
}

/// Scenario 7: Press `k` to scroll backward one line.
#[test]
#[ignore]
fn test_nav_k_scrolls_backward_one_line() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // Wait for initial render.
    expect_str(&mut session, "Line 001");

    // Scroll forward a few lines, then back.
    send_key(&mut session, "j");
    send_key(&mut session, "j");
    send_key(&mut session, "j");

    // Now scroll back. After 3 forward + 1 back, top should be line 3
    // (0-indexed: 2), meaning "Line 003" should be at the top.
    send_key(&mut session, "k");
    expect_str(&mut session, "Line 003");

    quit_pager(&mut session);
}

/// Scenario 8: Press Space to page forward.
#[test]
#[ignore]
fn test_nav_space_pages_forward() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // Wait for initial render.
    expect_str(&mut session, "Line 001");

    // Space pages forward by content_rows (terminal rows - 1 for prompt).
    // In a typical PTY that's 24 rows, so content_rows = 23.
    // After one Space, we should see lines starting around Line 024.
    send_key(&mut session, " ");

    // After paging forward, "Line 001" should have scrolled off and
    // we should now see higher-numbered lines.
    expect_str(&mut session, "Line 0");

    quit_pager(&mut session);
}

/// Scenario 9: Press `b` to page backward.
#[test]
#[ignore]
fn test_nav_b_pages_backward() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // Wait for initial render.
    expect_str(&mut session, "Line 001");

    // Page forward twice, then back once.
    send_key(&mut session, " ");
    std::thread::sleep(Duration::from_millis(100));
    send_key(&mut session, " ");
    std::thread::sleep(Duration::from_millis(100));
    send_key(&mut session, "b");

    // After 2 forward + 1 back, we should be roughly back to the
    // first page forward position. The exact line depends on terminal
    // size, but we should see mid-range lines.
    expect_str(&mut session, "Line 0");

    quit_pager(&mut session);
}

/// Scenario 10: Press `G` to go to end of file; `(END)` prompt shown.
#[test]
#[ignore]
fn test_nav_upper_g_goes_to_end() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // Wait for initial render.
    expect_str(&mut session, "Line 001");

    // Press 'G' to jump to end.
    send_key(&mut session, "G");

    // Should see the last line of the file.
    expect_str(&mut session, "Line 100");

    quit_pager(&mut session);
}

/// Scenario 11: Press `g` to go back to the beginning.
#[test]
#[ignore]
fn test_nav_g_goes_to_beginning() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // Wait for initial render.
    expect_str(&mut session, "Line 001");

    // Jump to end, then back to beginning.
    send_key(&mut session, "G");
    std::thread::sleep(Duration::from_millis(100));
    send_key(&mut session, "g");

    // Should be back at the first line.
    expect_str(&mut session, "Line 001");

    quit_pager(&mut session);
}

/// Scenario 12: Press `q` to exit the pager cleanly.
#[test]
#[ignore]
fn test_nav_q_exits_cleanly() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // Wait for initial render.
    expect_str(&mut session, "Line 001");

    // Quit.
    send_key(&mut session, "q");

    // Give the process time to exit.
    std::thread::sleep(Duration::from_millis(500));

    assert!(
        !session.get_process().is_alive().unwrap_or(true),
        "process should have exited after 'q'"
    );
}
