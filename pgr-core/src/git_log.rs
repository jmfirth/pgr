//! Git log parsing — extracts commit boundaries for `]g` / `[g` navigation.
//!
//! Parses `git log` output and records the line number of each `commit <hash>`
//! header so the pager can jump between commits.

/// A single commit entry in a `git log` stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    /// Line number (0-indexed in buffer) of the `commit <hash>` header.
    pub start_line: usize,
    /// The 40-character (or abbreviated) commit hash.
    pub hash: String,
}

/// Parse `git log` output and return a list of commit positions.
///
/// Each entry in the returned `Vec` corresponds to one `commit <hash>` line
/// found in `lines`. Lines are 0-indexed.  Returns an empty `Vec` if no
/// commit headers are found.
#[must_use]
pub fn parse_git_log(lines: &[&str]) -> Vec<GitCommit> {
    let mut commits = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(hash) = parse_commit_hash(line) {
            commits.push(GitCommit {
                start_line: i,
                hash: hash.to_string(),
            });
        }
    }
    commits
}

/// Extract the commit hash from a `commit <hash>` line.
///
/// Returns `Some(&str)` pointing to the hash portion when the line starts
/// with `"commit "` followed by 7–40 hex characters.  Returns `None`
/// otherwise.
fn parse_commit_hash(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("commit ")?;
    // Consume at most 40 hex chars; require at least 7.
    let bytes = rest.as_bytes();
    let mut hex_len = 0;
    while hex_len < bytes.len() && hex_len < 40 && bytes[hex_len].is_ascii_hexdigit() {
        hex_len += 1;
    }
    if hex_len < 7 {
        return None;
    }
    // The hash must be followed by end-of-line, a space, or a parenthesis
    // (for `git log --decorate` refs like `commit abc1234 (HEAD -> main)`).
    if hex_len < bytes.len() {
        let next = bytes[hex_len];
        if next != b' ' && next != b'(' {
            return None;
        }
    }
    Some(&rest[..hex_len])
}

/// Find the next commit start line after `current_line`.
///
/// Returns `None` if there are no commits after the current line.
/// If `wrap` is `true`, wraps around to the first commit.
#[must_use]
pub fn next_commit_line(commits: &[GitCommit], current_line: usize, wrap: bool) -> Option<usize> {
    for commit in commits {
        if commit.start_line > current_line {
            return Some(commit.start_line);
        }
    }
    if wrap {
        commits.first().map(|c| c.start_line)
    } else {
        None
    }
}

/// Find the previous commit start line before `current_line`.
///
/// Returns `None` if there are no commits before the current line.
/// If `wrap` is `true`, wraps around to the last commit.
#[must_use]
pub fn prev_commit_line(commits: &[GitCommit], current_line: usize, wrap: bool) -> Option<usize> {
    let mut best: Option<usize> = None;
    for commit in commits {
        if commit.start_line < current_line {
            best = Some(commit.start_line);
        }
    }
    if best.is_some() {
        return best;
    }
    if wrap {
        commits.last().map(|c| c.start_line)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_git_log ──

    #[test]
    fn test_parse_git_log_full_hash_returns_commit() {
        let lines = vec![
            "commit abcdef1234567890abcdef1234567890abcdef12",
            "Author: Alice <alice@example.com>",
            "Date:   Mon Jan 1 12:00:00 2024 +0000",
            "",
            "    Initial commit",
        ];
        let commits = parse_git_log(&lines);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].start_line, 0);
        assert_eq!(commits[0].hash, "abcdef1234567890abcdef1234567890abcdef12");
    }

    #[test]
    fn test_parse_git_log_abbreviated_hash_returns_commit() {
        let lines = vec![
            "commit abcdef1",
            "Author: Alice <alice@example.com>",
            "Date:   Mon Jan 1 12:00:00 2024 +0000",
        ];
        let commits = parse_git_log(&lines);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abcdef1");
    }

    #[test]
    fn test_parse_git_log_multiple_commits_returns_all() {
        let lines = vec![
            "commit aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "Author: Alice <alice@example.com>",
            "",
            "    First commit",
            "",
            "commit bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "Author: Bob <bob@example.com>",
            "",
            "    Second commit",
        ];
        let commits = parse_git_log(&lines);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].start_line, 0);
        assert_eq!(commits[1].start_line, 5);
    }

    #[test]
    fn test_parse_git_log_decorated_hash_returns_commit() {
        // `git log --decorate` appends ref names after the hash.
        let lines = vec![
            "commit abcdef1234567890abcdef1234567890abcdef12 (HEAD -> main, origin/main)",
            "Author: Alice <alice@example.com>",
        ];
        let commits = parse_git_log(&lines);
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].hash, "abcdef1234567890abcdef1234567890abcdef12");
    }

    #[test]
    fn test_parse_git_log_empty_input_returns_empty() {
        let commits = parse_git_log(&[]);
        assert!(commits.is_empty());
    }

    #[test]
    fn test_parse_git_log_no_commits_returns_empty() {
        let lines = vec!["just some text", "nothing to see here"];
        let commits = parse_git_log(&lines);
        assert!(commits.is_empty());
    }

    #[test]
    fn test_parse_git_log_short_hash_below_seven_is_ignored() {
        let lines = vec!["commit abc123"]; // only 6 hex chars
        let commits = parse_git_log(&lines);
        assert!(commits.is_empty());
    }

    #[test]
    fn test_parse_git_log_non_hex_hash_is_ignored() {
        let lines = vec!["commit not-a-hash-at-all"];
        let commits = parse_git_log(&lines);
        assert!(commits.is_empty());
    }

    // ── next_commit_line ──

    #[test]
    fn test_next_commit_line_finds_next() {
        let commits = vec![
            GitCommit {
                start_line: 0,
                hash: "aaa".to_string(),
            },
            GitCommit {
                start_line: 10,
                hash: "bbb".to_string(),
            },
            GitCommit {
                start_line: 20,
                hash: "ccc".to_string(),
            },
        ];
        assert_eq!(next_commit_line(&commits, 5, false), Some(10));
    }

    #[test]
    fn test_next_commit_line_no_target_no_wrap_returns_none() {
        let commits = vec![GitCommit {
            start_line: 0,
            hash: "aaa".to_string(),
        }];
        assert_eq!(next_commit_line(&commits, 10, false), None);
    }

    #[test]
    fn test_next_commit_line_no_target_wrap_returns_first() {
        let commits = vec![
            GitCommit {
                start_line: 0,
                hash: "aaa".to_string(),
            },
            GitCommit {
                start_line: 10,
                hash: "bbb".to_string(),
            },
        ];
        assert_eq!(next_commit_line(&commits, 10, true), Some(0));
    }

    #[test]
    fn test_next_commit_line_empty_returns_none() {
        assert_eq!(next_commit_line(&[], 0, true), None);
        assert_eq!(next_commit_line(&[], 0, false), None);
    }

    // ── prev_commit_line ──

    #[test]
    fn test_prev_commit_line_finds_previous() {
        let commits = vec![
            GitCommit {
                start_line: 0,
                hash: "aaa".to_string(),
            },
            GitCommit {
                start_line: 10,
                hash: "bbb".to_string(),
            },
            GitCommit {
                start_line: 20,
                hash: "ccc".to_string(),
            },
        ];
        assert_eq!(prev_commit_line(&commits, 15, false), Some(10));
    }

    #[test]
    fn test_prev_commit_line_at_first_no_wrap_returns_none() {
        let commits = vec![GitCommit {
            start_line: 5,
            hash: "aaa".to_string(),
        }];
        assert_eq!(prev_commit_line(&commits, 5, false), None);
        // Before the only commit
        assert_eq!(prev_commit_line(&commits, 3, false), None);
    }

    #[test]
    fn test_prev_commit_line_at_first_wrap_returns_last() {
        let commits = vec![
            GitCommit {
                start_line: 0,
                hash: "aaa".to_string(),
            },
            GitCommit {
                start_line: 10,
                hash: "bbb".to_string(),
            },
        ];
        assert_eq!(prev_commit_line(&commits, 0, true), Some(10));
    }

    #[test]
    fn test_prev_commit_line_empty_returns_none() {
        assert_eq!(prev_commit_line(&[], 5, true), None);
        assert_eq!(prev_commit_line(&[], 5, false), None);
    }

    // ── GitCommit struct ──

    #[test]
    fn test_git_commit_debug_format_is_nonempty() {
        let c = GitCommit {
            start_line: 0,
            hash: "abc".to_string(),
        };
        assert!(!format!("{c:?}").is_empty());
    }

    #[test]
    fn test_git_commit_clone_produces_equal_value() {
        let c = GitCommit {
            start_line: 42,
            hash: "def".to_string(),
        };
        assert_eq!(c.clone(), c);
    }

    #[test]
    fn test_parse_git_log_commit_not_at_line_start_is_ignored() {
        // Indented commit lines (in message body) must not be treated as headers.
        let lines = vec![
            "commit abcdef1234567890abcdef1234567890abcdef12",
            "Author: Alice <alice@example.com>",
            "",
            "    Mention commit abcdef1234567890abcdef1234567890abcdef12 in body",
        ];
        let commits = parse_git_log(&lines);
        // Only the first line is a commit header; the body mention is indented.
        assert_eq!(commits.len(), 1);
        assert_eq!(commits[0].start_line, 0);
    }
}
