use crate::harness::ScreenCapture;

/// Difference report between two screen captures.
#[derive(Debug)]
pub struct ScreenDiff {
    /// Rows that differ.
    pub differing_rows: Vec<RowDiff>,
    /// Summary message.
    pub message: String,
}

/// A single differing row between pgr and less output.
#[derive(Debug)]
pub struct RowDiff {
    pub row: usize,
    pub pgr_text: String,
    pub less_text: String,
}

impl std::fmt::Display for ScreenDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}", self.message)?;
        for diff in &self.differing_rows {
            writeln!(f, "  row {}:", diff.row)?;
            writeln!(f, "    pgr:  {:?}", diff.pgr_text)?;
            writeln!(f, "    less: {:?}", diff.less_text)?;
        }
        Ok(())
    }
}

impl std::error::Error for ScreenDiff {}

/// Compare only the content area (all rows except the prompt/status line).
///
/// Trailing whitespace on each line is stripped before comparison.
pub fn compare_content(
    pgr_screen: &ScreenCapture,
    less_screen: &ScreenCapture,
) -> Result<(), ScreenDiff> {
    let pgr_content = pgr_screen.content_lines();
    let less_content = less_screen.content_lines();

    let max_rows = pgr_content.len().max(less_content.len());
    let mut diffs = Vec::new();

    for i in 0..max_rows {
        let pgr_line = pgr_content.get(i).map_or("", String::as_str).trim_end();
        let less_line = less_content.get(i).map_or("", String::as_str).trim_end();

        if pgr_line != less_line {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_line.to_string(),
                less_text: less_line.to_string(),
            });
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(ScreenDiff {
            message: format!(
                "content mismatch: {} of {} rows differ",
                diffs.len(),
                max_rows
            ),
            differing_rows: diffs,
        })
    }
}

/// Compare full screens including the prompt line.
///
/// Trailing whitespace on each line is stripped before comparison.
#[allow(dead_code)] // Used by downstream conformance tests (Tasks 126-128)
pub fn compare_screens(
    pgr_screen: &ScreenCapture,
    less_screen: &ScreenCapture,
) -> Result<(), ScreenDiff> {
    let max_rows = pgr_screen.rows.len().max(less_screen.rows.len());
    let mut diffs = Vec::new();

    for i in 0..max_rows {
        let pgr_line = pgr_screen.rows.get(i).map_or("", String::as_str).trim_end();
        let less_line = less_screen
            .rows
            .get(i)
            .map_or("", String::as_str)
            .trim_end();

        if pgr_line != less_line {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_line.to_string(),
                less_text: less_line.to_string(),
            });
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(ScreenDiff {
            message: format!(
                "screen mismatch: {} of {} rows differ",
                diffs.len(),
                max_rows
            ),
            differing_rows: diffs,
        })
    }
}

/// Compare with normalization: strip trailing whitespace and tilde markers.
///
/// GNU less uses `~` to mark lines below EOF. Both pgr and less may differ
/// in exact prompt formatting. This function strips those differences.
pub fn compare_normalized(
    pgr_screen: &ScreenCapture,
    less_screen: &ScreenCapture,
) -> Result<(), ScreenDiff> {
    let pgr_content = pgr_screen.content_lines();
    let less_content = less_screen.content_lines();

    let max_rows = pgr_content.len().max(less_content.len());
    let mut diffs = Vec::new();

    for i in 0..max_rows {
        let pgr_line = normalize_line(pgr_content.get(i).map_or("", String::as_str));
        let less_line = normalize_line(less_content.get(i).map_or("", String::as_str));

        if pgr_line != less_line {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_line,
                less_text: less_line,
            });
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(ScreenDiff {
            message: format!(
                "normalized content mismatch: {} of {} rows differ",
                diffs.len(),
                max_rows
            ),
            differing_rows: diffs,
        })
    }
}

/// Normalize a line for comparison: strip trailing whitespace and
/// replace standalone `~` rows (EOF markers) with empty strings.
fn normalize_line(line: &str) -> String {
    let trimmed = line.trim_end();
    if trimmed == "~" {
        String::new()
    } else {
        trimmed.to_string()
    }
}
