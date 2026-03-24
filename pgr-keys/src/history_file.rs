//! Persistence for search and shell command history.
//!
//! The history file uses the same format as GNU less: section markers
//! (`.search`, `.shell_cmd`, `.mark`) on their own lines, followed by
//! one entry per line.

use std::io::{BufRead, BufWriter, Write};
use std::path::Path;

use crate::line_editor::History;

/// Default maximum number of history entries per section.
const DEFAULT_HISTSIZE: usize = 100;

/// Load search and shell command history from a file.
///
/// Returns `(search_history, shell_history)`. If the file does not exist,
/// returns two empty histories sized to `max_entries`.
///
/// # Errors
///
/// Returns `KeyError::Io` if the file exists but cannot be read.
pub fn load_history(path: &Path, max_entries: Option<usize>) -> crate::Result<(History, History)> {
    let max = max_entries.unwrap_or(DEFAULT_HISTSIZE);

    if !path.exists() {
        return Ok((History::with_max_size(max), History::with_max_size(max)));
    }

    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);

    let mut search_entries: Vec<String> = Vec::new();
    let mut shell_entries: Vec<String> = Vec::new();
    let mut current_section: Option<Section> = None;

    for line in reader.lines() {
        let line = line?;
        if let Some(section) = parse_section_marker(&line) {
            current_section = Some(section);
            continue;
        }
        match current_section {
            Some(Section::Search) => search_entries.push(line),
            Some(Section::ShellCmd) => shell_entries.push(line),
            // Skip .mark and unknown sections.
            _ => {}
        }
    }

    Ok((
        History::from_entries(&search_entries, max),
        History::from_entries(&shell_entries, max),
    ))
}

/// Save search and shell command history to a file.
///
/// Creates the parent directory if it does not exist. Writes section
/// markers followed by entries, matching the GNU less format.
///
/// # Errors
///
/// Returns `KeyError::Io` if the file or its parent directory cannot be
/// created or written.
pub fn save_history(path: &Path, search: &History, shell: &History) -> crate::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let file = std::fs::File::create(path)?;
    let mut writer = BufWriter::new(file);

    if !search.is_empty() {
        writeln!(writer, ".search")?;
        for entry in search.entries() {
            writeln!(writer, "{entry}")?;
        }
    }

    if !shell.is_empty() {
        writeln!(writer, ".shell_cmd")?;
        for entry in shell.entries() {
            writeln!(writer, "{entry}")?;
        }
    }

    writer.flush()?;
    Ok(())
}

/// Known section markers in the history file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Search,
    ShellCmd,
    Mark,
}

/// Parse a line as a section marker, returning `None` if it is not one.
fn parse_section_marker(line: &str) -> Option<Section> {
    match line {
        ".search" => Some(Section::Search),
        ".shell_cmd" => Some(Section::ShellCmd),
        ".mark" => Some(Section::Mark),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn test_load_history_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent");
        let (search, shell) = load_history(&path, None).unwrap();
        assert!(search.is_empty());
        assert!(shell.is_empty());
    }

    #[test]
    fn test_load_history_search_section_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, ".search\nfoo\nbar\n").unwrap();
        let (search, shell) = load_history(&path, None).unwrap();
        assert_eq!(search.len(), 2);
        assert_eq!(search.get(0), Some("foo"));
        assert_eq!(search.get(1), Some("bar"));
        assert!(shell.is_empty());
    }

    #[test]
    fn test_load_history_shell_section_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, ".shell_cmd\nls\npwd\n").unwrap();
        let (search, shell) = load_history(&path, None).unwrap();
        assert!(search.is_empty());
        assert_eq!(shell.len(), 2);
        assert_eq!(shell.get(0), Some("ls"));
        assert_eq!(shell.get(1), Some("pwd"));
    }

    #[test]
    fn test_load_history_both_sections_parsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, ".search\nalpha\n.shell_cmd\nbeta\n").unwrap();
        let (search, shell) = load_history(&path, None).unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search.get(0), Some("alpha"));
        assert_eq!(shell.len(), 1);
        assert_eq!(shell.get(0), Some("beta"));
    }

    #[test]
    fn test_load_history_respects_histsize_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, ".search\na\nb\nc\nd\ne\n").unwrap();
        let (search, _shell) = load_history(&path, Some(3)).unwrap();
        assert_eq!(search.len(), 3);
        assert_eq!(search.get(0), Some("c"));
        assert_eq!(search.get(1), Some("d"));
        assert_eq!(search.get(2), Some("e"));
    }

    #[test]
    fn test_save_history_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let mut search = History::new();
        search.push("pattern1".to_string());
        let shell = History::new();
        save_history(&path, &search, &shell).unwrap();
        assert!(path.exists());
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains(".search"));
        assert!(contents.contains("pattern1"));
        assert!(!contents.contains(".shell_cmd"));
    }

    #[test]
    fn test_save_history_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("history");
        let search = History::new();
        let shell = History::new();
        save_history(&path, &search, &shell).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_history_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");

        let mut search = History::new();
        search.push("first".to_string());
        search.push("second".to_string());
        let mut shell = History::new();
        shell.push("ls -la".to_string());

        save_history(&path, &search, &shell).unwrap();
        let (loaded_search, loaded_shell) = load_history(&path, None).unwrap();

        assert_eq!(loaded_search.len(), 2);
        assert_eq!(loaded_search.get(0), Some("first"));
        assert_eq!(loaded_search.get(1), Some("second"));
        assert_eq!(loaded_shell.len(), 1);
        assert_eq!(loaded_shell.get(0), Some("ls -la"));
    }

    #[test]
    fn test_parse_section_marker_known_sections() {
        assert_eq!(parse_section_marker(".search"), Some(Section::Search));
        assert_eq!(parse_section_marker(".shell_cmd"), Some(Section::ShellCmd));
        assert_eq!(parse_section_marker(".mark"), Some(Section::Mark));
    }

    #[test]
    fn test_parse_section_marker_unknown_returns_none() {
        assert_eq!(parse_section_marker("not a section"), None);
        assert_eq!(parse_section_marker(".other"), None);
        assert_eq!(parse_section_marker(""), None);
    }

    #[test]
    fn test_load_history_mark_section_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, ".mark\na 123 /some/file\n.search\nquery\n").unwrap();
        let (search, shell) = load_history(&path, None).unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search.get(0), Some("query"));
        assert!(shell.is_empty());
    }

    #[test]
    fn test_load_history_lines_before_section_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, "orphan line\n.search\nvalid\n").unwrap();
        let (search, shell) = load_history(&path, None).unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search.get(0), Some("valid"));
        assert!(shell.is_empty());
    }

    #[test]
    fn test_save_history_empty_histories_produces_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        let search = History::new();
        let shell = History::new();
        save_history(&path, &search, &shell).unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.is_empty());
    }
}
