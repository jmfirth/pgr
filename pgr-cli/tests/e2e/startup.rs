use std::time::Duration;

use expectrl::process::Healthcheck;
use expectrl::{Eof, Expect};

use super::{expect_str, fixture_path, quit_pager, spawn_pgr};

/// Scenario 1: `pgr file.txt` opens and shows first page of content.
#[test]
#[ignore]
fn test_startup_file_shows_first_page() {
    let mut session = spawn_pgr(&[fixture_path("basic.txt").to_str().unwrap()]);

    // The first line of basic.txt is "Line 001".
    expect_str(&mut session, "Line 001");

    quit_pager(&mut session);
}

/// Scenario 2: `pgr -V` prints version and exits.
#[test]
#[ignore]
fn test_startup_version_flag_prints_version_and_exits() {
    let mut session = spawn_pgr(&["-V"]);

    // Should print version string.
    expect_str(&mut session, "pgr version");

    // Process should exit on its own — wait for EOF on the PTY stream.
    let _ = session.expect(Eof);
}

/// Scenario 3: `pgr` with no args prints error and exits with non-zero.
#[test]
#[ignore]
fn test_startup_no_args_prints_error_and_exits() {
    let mut session = spawn_pgr(&[]);

    // Should print an error about no files.
    expect_str(&mut session, "no files specified");

    // Process should exit on its own — wait for EOF.
    let _ = session.expect(Eof);
}

/// Scenario 4: `pgr nonexistent.txt` prints error and exits.
#[test]
#[ignore]
fn test_startup_nonexistent_file_prints_error_and_exits() {
    let mut session = spawn_pgr(&["nonexistent_file_that_does_not_exist.txt"]);

    // The process should print an error and exit. Wait for EOF.
    let _ = session.expect(Eof);

    // After EOF, the process should not be alive.
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !session.get_process().is_alive().unwrap_or(true),
        "process should have exited for nonexistent file"
    );
}
