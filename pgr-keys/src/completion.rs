//! Tab completion for line editor prompts.
//!
//! Provides filename completion (for `:e` prompts) and option name
//! completion (for `--` prompts).

use std::path::Path;

/// The kind of completion to perform when Tab is pressed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionMode {
    /// Complete filesystem paths (used in `:e` examine prompt).
    Filename,
    /// Complete option names from a static list (used in `--` prompt).
    OptionName,
    /// No completion available.
    None,
}

/// Tracks cycling state across consecutive Tab presses.
#[derive(Debug, Clone)]
pub struct CompletionState {
    /// The original partial text before the first Tab in this sequence.
    original: String,
    /// The list of completions found for `original`.
    candidates: Vec<String>,
    /// Index into `candidates` for the next Tab press.
    index: usize,
}

impl CompletionState {
    /// Create a new completion state with the given original text and candidates.
    fn new(original: String, candidates: Vec<String>) -> Self {
        Self {
            original,
            candidates,
            index: 0,
        }
    }

    /// Advance to the next candidate and return it, or `None` if exhausted.
    ///
    /// Wraps around to the first candidate after reaching the end.
    fn next(&mut self) -> Option<&str> {
        if self.candidates.is_empty() {
            return None;
        }
        let candidate = &self.candidates[self.index];
        self.index = (self.index + 1) % self.candidates.len();
        Some(candidate)
    }

    /// Return all candidates.
    #[must_use]
    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }

    /// Return the original partial text.
    #[must_use]
    pub fn original(&self) -> &str {
        &self.original
    }
}

/// Result of a tab-completion attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionResult {
    /// No matches found; do nothing.
    NoMatch,
    /// Exactly one match; replace the input with this value.
    Single(String),
    /// Multiple matches; replace input with the longest common prefix.
    /// The `Vec<String>` contains all candidates for display.
    Multiple(String, Vec<String>),
}

/// Complete a partial filename against the filesystem.
///
/// Expands `~` to the home directory. Lists directory entries that match
/// the partial filename prefix. Directories in the result get a trailing `/`.
#[must_use]
pub fn complete_filename(partial: &str) -> Vec<String> {
    let expanded = expand_tilde(partial);

    let (dir, prefix) = split_path_prefix(&expanded);

    let dir_path = if dir.is_empty() { "." } else { &dir };

    let Ok(entries) = std::fs::read_dir(dir_path) else {
        return Vec::new();
    };

    let mut matches: Vec<String> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with(&prefix) {
                let full = if dir.is_empty() {
                    name.clone()
                } else if dir.ends_with('/') {
                    format!("{dir}{name}")
                } else {
                    format!("{dir}/{name}")
                };
                // Re-apply tilde if the original had one.
                let result = if partial.starts_with('~') {
                    unexpand_tilde(&full)
                } else {
                    full
                };
                // Append '/' for directories.
                let result = if entry.file_type().ok()?.is_dir() {
                    format!("{result}/")
                } else {
                    result
                };
                Some(result)
            } else {
                None
            }
        })
        .collect();

    matches.sort();
    matches
}

/// Known option long names for `--` prompt completion.
const OPTION_NAMES: &[&str] = &[
    "case-insensitive",
    "CASE-INSENSITIVE",
    "line-numbers",
    "chop-long-lines",
    "squeeze-blank-lines",
    "raw-control-chars",
    "RAW-CONTROL-CHARS",
    "quiet",
    "quit-at-eof",
    "QUIT-AT-EOF",
    "quit-if-one-screen",
    "no-init",
    "hilite-search",
    "HILITE-SEARCH",
    "hilite-unread",
    "HILITE-UNREAD",
    "status-column",
    "tabs",
    "jump-target",
    "shift",
    "window",
    "max-back-scroll",
    "max-forw-scroll",
    "wordwrap",
    "incsearch",
    "tilde",
];

/// Complete a partial option name against the known list.
#[must_use]
pub fn complete_option(partial: &str) -> Vec<String> {
    let mut matches: Vec<String> = OPTION_NAMES
        .iter()
        .filter(|name| name.starts_with(partial))
        .map(|&s| s.to_owned())
        .collect();
    matches.sort();
    matches
}

/// Compute the longest common prefix of a set of strings.
#[must_use]
pub fn longest_common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let first = &strings[0];
    let mut len = first.len();
    for s in &strings[1..] {
        len = len.min(s.len());
        for (i, (a, b)) in first.bytes().zip(s.bytes()).enumerate() {
            if a != b {
                len = len.min(i);
                break;
            }
        }
    }
    first[..len].to_owned()
}

/// Perform a completion attempt for the given mode and partial input.
///
/// Returns a `CompletionResult` indicating what happened.
#[must_use]
pub fn complete(mode: &CompletionMode, partial: &str) -> CompletionResult {
    let candidates = match mode {
        CompletionMode::Filename => complete_filename(partial),
        CompletionMode::OptionName => complete_option(partial),
        CompletionMode::None => return CompletionResult::NoMatch,
    };

    match candidates.len() {
        0 => CompletionResult::NoMatch,
        1 => CompletionResult::Single(candidates.into_iter().next().unwrap_or_default()),
        _ => {
            let prefix = longest_common_prefix(&candidates);
            CompletionResult::Multiple(prefix, candidates)
        }
    }
}

/// Expand a leading `~` to the user's home directory.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with('/') {
            if let Some(home) = home_dir() {
                return format!("{}{rest}", home.display());
            }
        }
    }
    path.to_owned()
}

/// Collapse a leading home directory path back to `~`.
fn unexpand_tilde(path: &str) -> String {
    if let Some(home) = home_dir() {
        let home_str = format!("{}", home.display());
        if let Some(rest) = path.strip_prefix(&home_str) {
            return format!("~{rest}");
        }
    }
    path.to_owned()
}

/// Get the home directory from the `HOME` environment variable.
fn home_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

/// Split a path into the directory portion and the filename prefix.
///
/// For example, `/foo/bar/baz` splits into (`"/foo/bar"`, `"baz"`).
/// A bare `"hello"` splits into (`""`, `"hello"`).
fn split_path_prefix(path: &str) -> (String, String) {
    let p = Path::new(path);
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) => {
            let parent_str = parent.to_string_lossy().into_owned();
            let name_str = name.to_string_lossy().into_owned();
            // If the path ends with '/', the user is completing inside a directory.
            if path.ends_with('/') {
                (path.to_owned(), String::new())
            } else {
                (parent_str, name_str)
            }
        }
        _ => (String::new(), path.to_owned()),
    }
}

/// Initiate or advance tab completion for the line editor.
///
/// On first Tab: compute candidates. If single match, return it.
/// If multiple, return common prefix.
/// On subsequent Tabs with an active `CompletionState`: cycle through candidates.
///
/// Returns `(replacement_text, Option<status_message>, updated_state)`.
#[must_use]
pub fn tab_complete(
    current_text: &str,
    mode: &CompletionMode,
    state: Option<CompletionState>,
) -> (Option<String>, Option<String>, Option<CompletionState>) {
    if *mode == CompletionMode::None {
        return (None, None, None);
    }

    // If we have an active completion state, cycle through candidates.
    if let Some(mut st) = state {
        if let Some(next) = st.next() {
            let replacement = next.to_owned();
            return (Some(replacement), None, Some(st));
        }
        return (None, None, None);
    }

    // First Tab: compute candidates.
    let result = complete(mode, current_text);
    match result {
        CompletionResult::NoMatch => (None, None, None),
        CompletionResult::Single(s) => (Some(s), None, None),
        CompletionResult::Multiple(prefix, candidates) => {
            let status = format!(
                "{} completions: {}",
                candidates.len(),
                candidates.join("  ")
            );
            let new_state = CompletionState::new(prefix.clone(), candidates);
            (Some(prefix), Some(status), Some(new_state))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── complete_option tests ──

    #[test]
    fn test_complete_option_empty_returns_all() {
        let result = complete_option("");
        assert_eq!(result.len(), OPTION_NAMES.len());
    }

    #[test]
    fn test_complete_option_unique_prefix_returns_single() {
        let result = complete_option("inc");
        assert_eq!(result, vec!["incsearch"]);
    }

    #[test]
    fn test_complete_option_no_match_returns_empty() {
        let result = complete_option("zzz");
        assert!(result.is_empty());
    }

    #[test]
    fn test_complete_option_multiple_matches() {
        let result = complete_option("quit");
        assert!(result.len() >= 2);
        for name in &result {
            assert!(name.starts_with("quit"));
        }
    }

    // ── longest_common_prefix tests ──

    #[test]
    fn test_longest_common_prefix_empty_input() {
        let result = longest_common_prefix(&[]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_longest_common_prefix_single_string() {
        let result = longest_common_prefix(&["hello".to_owned()]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_longest_common_prefix_shared_prefix() {
        let input = vec!["foobar".to_owned(), "foobaz".to_owned()];
        let result = longest_common_prefix(&input);
        assert_eq!(result, "fooba");
    }

    #[test]
    fn test_longest_common_prefix_no_common_prefix() {
        let input = vec!["abc".to_owned(), "xyz".to_owned()];
        let result = longest_common_prefix(&input);
        assert_eq!(result, "");
    }

    // ── complete (integrated) tests ──

    #[test]
    fn test_complete_none_mode_returns_no_match() {
        let result = complete(&CompletionMode::None, "anything");
        assert_eq!(result, CompletionResult::NoMatch);
    }

    #[test]
    fn test_complete_option_single_match() {
        let result = complete(&CompletionMode::OptionName, "wordw");
        assert_eq!(result, CompletionResult::Single("wordwrap".to_owned()));
    }

    #[test]
    fn test_complete_option_multiple_returns_prefix_and_candidates() {
        let result = complete(&CompletionMode::OptionName, "quit");
        match result {
            CompletionResult::Multiple(prefix, candidates) => {
                assert!(prefix.starts_with("quit"));
                assert!(candidates.len() >= 2);
            }
            _ => panic!("Expected Multiple result"),
        }
    }

    #[test]
    fn test_complete_option_no_match_returns_no_match() {
        let result = complete(&CompletionMode::OptionName, "zzzzz");
        assert_eq!(result, CompletionResult::NoMatch);
    }

    // ── complete_filename tests (using temp directory) ──

    #[test]
    fn test_complete_filename_nonexistent_dir_returns_empty() {
        let result = complete_filename("/nonexistent_dir_12345/abc");
        assert!(result.is_empty());
    }

    #[test]
    fn test_complete_filename_in_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();

        // Create some test files.
        std::fs::write(base.join("alpha.txt"), "").unwrap();
        std::fs::write(base.join("alpha2.txt"), "").unwrap();
        std::fs::write(base.join("beta.txt"), "").unwrap();
        std::fs::create_dir(base.join("gamma")).unwrap();

        let partial = format!("{}/al", base.display());
        let result = complete_filename(&partial);
        assert_eq!(result.len(), 2);
        assert!(result[0].contains("alpha"));
        assert!(result[1].contains("alpha2"));

        let partial_all = format!("{}/", base.display());
        let result_all = complete_filename(&partial_all);
        // Should include alpha.txt, alpha2.txt, beta.txt, gamma/
        assert_eq!(result_all.len(), 4);

        // Gamma should end with /
        let gamma_entry = result_all.iter().find(|s| s.contains("gamma")).unwrap();
        assert!(gamma_entry.ends_with('/'));
    }

    // ── tab_complete tests ──

    #[test]
    fn test_tab_complete_none_mode_returns_nothing() {
        let (replacement, status, state) = tab_complete("test", &CompletionMode::None, None);
        assert!(replacement.is_none());
        assert!(status.is_none());
        assert!(state.is_none());
    }

    #[test]
    fn test_tab_complete_single_match_returns_replacement() {
        let (replacement, status, state) = tab_complete("wordw", &CompletionMode::OptionName, None);
        assert_eq!(replacement, Some("wordwrap".to_owned()));
        assert!(status.is_none());
        // No cycling state needed for single match.
        assert!(state.is_none());
    }

    #[test]
    fn test_tab_complete_multiple_matches_returns_prefix_and_state() {
        let (replacement, status, state) = tab_complete("quit", &CompletionMode::OptionName, None);
        assert!(replacement.is_some());
        assert!(status.is_some());
        assert!(state.is_some());
        let status_str = status.unwrap();
        assert!(status_str.contains("completions"));
    }

    #[test]
    fn test_tab_complete_cycling_returns_next_candidate() {
        // Start a completion.
        let (_, _, state) = tab_complete("quit", &CompletionMode::OptionName, None);
        assert!(state.is_some());

        // Cycle through.
        let (replacement, _, state2) = tab_complete("quit", &CompletionMode::OptionName, state);
        assert!(replacement.is_some());
        assert!(state2.is_some());
    }

    #[test]
    fn test_tab_complete_no_match_returns_nothing() {
        let (replacement, status, state) = tab_complete("zzzzz", &CompletionMode::OptionName, None);
        assert!(replacement.is_none());
        assert!(status.is_none());
        assert!(state.is_none());
    }

    // ── split_path_prefix tests ──

    #[test]
    fn test_split_path_prefix_bare_name() {
        let (dir, prefix) = split_path_prefix("hello");
        assert_eq!(dir, "");
        assert_eq!(prefix, "hello");
    }

    #[test]
    fn test_split_path_prefix_with_directory() {
        let (dir, prefix) = split_path_prefix("/foo/bar/baz");
        assert_eq!(dir, "/foo/bar");
        assert_eq!(prefix, "baz");
    }

    #[test]
    fn test_split_path_prefix_trailing_slash() {
        let (dir, prefix) = split_path_prefix("/foo/bar/");
        assert_eq!(dir, "/foo/bar/");
        assert_eq!(prefix, "");
    }

    // ── CompletionState tests ──

    #[test]
    fn test_completion_state_cycling_wraps_around() {
        let mut state = CompletionState::new(
            "test".to_owned(),
            vec!["test1".to_owned(), "test2".to_owned()],
        );
        assert_eq!(state.next(), Some("test1"));
        assert_eq!(state.next(), Some("test2"));
        assert_eq!(state.next(), Some("test1")); // Wraps
    }

    #[test]
    fn test_completion_state_empty_candidates() {
        let mut state = CompletionState::new("test".to_owned(), vec![]);
        assert_eq!(state.next(), None);
    }

    #[test]
    fn test_completion_state_accessors() {
        let state = CompletionState::new(
            "test".to_owned(),
            vec!["test1".to_owned(), "test2".to_owned()],
        );
        assert_eq!(state.original(), "test");
        assert_eq!(state.candidates().len(), 2);
    }
}
