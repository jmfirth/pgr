/// Screen comparison logic for conformance testing.
///
/// Captures terminal state as a grid of text rows and provides
/// comparison functions that produce readable diffs.

/// A captured screen state: rows of text extracted from a vt100 parser.
#[derive(Debug, Clone)]
pub struct ScreenCapture {
    /// Text content of each row (trailing whitespace preserved from vt100).
    pub rows: Vec<String>,
    /// Terminal row count.
    pub terminal_rows: usize,
    /// Terminal column count.
    pub terminal_cols: usize,
    /// Cursor row at capture time.
    pub cursor_row: usize,
    /// Cursor col at capture time.
    pub cursor_col: usize,
}

impl ScreenCapture {
    /// Extract the content lines (all rows except the last, which is the prompt).
    pub fn content_lines(&self) -> &[String] {
        if self.rows.len() > 1 {
            &self.rows[..self.rows.len() - 1]
        } else {
            &self.rows
        }
    }

    /// Extract the prompt/status line (last row).
    pub fn prompt_line(&self) -> &str {
        self.rows.last().map_or("", String::as_str)
    }

    /// Get a specific row as text.
    pub fn row_text(&self, row: usize) -> &str {
        self.rows.get(row).map_or("", String::as_str)
    }
}

/// Difference report between two screen captures.
#[derive(Debug)]
pub struct ScreenDiff {
    /// Rows that differ.
    pub differing_rows: Vec<RowDiff>,
    /// Summary message.
    pub message: String,
}

/// A single row difference.
#[derive(Debug)]
pub struct RowDiff {
    /// Row index.
    pub row: usize,
    /// Text from pgr.
    pub pgr_text: String,
    /// Text from less.
    pub less_text: String,
}

impl std::fmt::Display for ScreenDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Screen mismatch: {}", self.message)?;
        for diff in &self.differing_rows {
            writeln!(f, "  Row {}:", diff.row)?;
            writeln!(f, "    pgr:  {:?}", diff.pgr_text)?;
            writeln!(f, "    less: {:?}", diff.less_text)?;
        }
        Ok(())
    }
}

impl std::error::Error for ScreenDiff {}

/// Normalize a row by trimming trailing whitespace.
fn normalize_row(row: &str) -> &str {
    row.trim_end()
}

/// Compare the content area (all rows except the prompt line) of two
/// screen captures. Trailing whitespace on each row is ignored.
///
/// Returns `Ok(())` if they match, or `Err(ScreenDiff)` with details.
pub fn compare_content(
    pgr_screen: &ScreenCapture,
    less_screen: &ScreenCapture,
) -> Result<(), ScreenDiff> {
    let pgr_content = pgr_screen.content_lines();
    let less_content = less_screen.content_lines();

    let max_rows = pgr_content.len().max(less_content.len());
    let mut diffs = Vec::new();

    for i in 0..max_rows {
        let pgr_row = pgr_content.get(i).map_or("", String::as_str);
        let less_row = less_content.get(i).map_or("", String::as_str);

        if normalize_row(pgr_row) != normalize_row(less_row) {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_row.to_string(),
                less_text: less_row.to_string(),
            });
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        let count = diffs.len();
        Err(ScreenDiff {
            differing_rows: diffs,
            message: format!("{count} content row(s) differ between pgr and less"),
        })
    }
}

/// Compare entire screens including the prompt line.
/// Trailing whitespace on each row is ignored.
pub fn compare_screens(
    pgr_screen: &ScreenCapture,
    less_screen: &ScreenCapture,
) -> Result<(), ScreenDiff> {
    let max_rows = pgr_screen.rows.len().max(less_screen.rows.len());
    let mut diffs = Vec::new();

    for i in 0..max_rows {
        let pgr_row = pgr_screen.rows.get(i).map_or("", String::as_str);
        let less_row = less_screen.rows.get(i).map_or("", String::as_str);

        if normalize_row(pgr_row) != normalize_row(less_row) {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_row.to_string(),
                less_text: less_row.to_string(),
            });
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        let count = diffs.len();
        Err(ScreenDiff {
            differing_rows: diffs,
            message: format!("{count} row(s) differ between pgr and less"),
        })
    }
}
