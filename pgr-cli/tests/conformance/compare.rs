/// Screen comparison logic for conformance tests.
///
/// Compares screen captures from pgr and GNU less, producing readable
/// diffs when they diverge.
use super::harness::ScreenCapture;

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
    /// Row index (0-based).
    pub row: usize,
    /// Text from pgr.
    pub pgr_text: String,
    /// Text from less.
    pub less_text: String,
}

impl std::fmt::Display for ScreenDiff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Screen diff: {}", self.message)?;
        for diff in &self.differing_rows {
            writeln!(f, "  Row {}: ", diff.row)?;
            writeln!(f, "    pgr:  {:?}", diff.pgr_text)?;
            writeln!(f, "    less: {:?}", diff.less_text)?;
        }
        Ok(())
    }
}

/// Normalize a row by trimming trailing whitespace.
fn normalize_row(row: &str) -> String {
    row.trim_end().to_string()
}

/// Compare only the content area (ignore prompt line) with normalization.
///
/// Trailing whitespace is stripped before comparison.
pub fn compare_content(pgr: &ScreenCapture, less: &ScreenCapture) -> Result<(), ScreenDiff> {
    let pgr_content = pgr.content_lines();
    let less_content = less.content_lines();

    let rows_to_compare = pgr_content.len().min(less_content.len());
    let mut diffs = Vec::new();

    for i in 0..rows_to_compare {
        let pgr_norm = normalize_row(&pgr_content[i]);
        let less_norm = normalize_row(&less_content[i]);
        if pgr_norm != less_norm {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_norm,
                less_text: less_norm,
            });
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(ScreenDiff {
            message: format!("{} of {} content rows differ", diffs.len(), rows_to_compare),
            differing_rows: diffs,
        })
    }
}

/// Compare full screens (content + prompt) with normalization.
#[allow(dead_code)] // Used by other conformance suites (Tasks 126, 128, 129)
pub fn compare_screens(pgr: &ScreenCapture, less: &ScreenCapture) -> Result<(), ScreenDiff> {
    let rows_to_compare = pgr.rows.len().min(less.rows.len());
    let mut diffs = Vec::new();

    for i in 0..rows_to_compare {
        let pgr_norm = normalize_row(&pgr.rows[i]);
        let less_norm = normalize_row(&less.rows[i]);
        if pgr_norm != less_norm {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_norm,
                less_text: less_norm,
            });
        }
    }

    if diffs.is_empty() {
        Ok(())
    } else {
        Err(ScreenDiff {
            message: format!("{} of {} rows differ", diffs.len(), rows_to_compare),
            differing_rows: diffs,
        })
    }
}

/// Compare prompts specifically — checks that both have similar prompt content.
#[allow(dead_code)] // Used by other conformance suites (Tasks 126, 128, 129)
pub fn compare_prompts(pgr: &ScreenCapture, less: &ScreenCapture) -> Result<(), ScreenDiff> {
    let pgr_prompt = normalize_row(pgr.prompt_line());
    let less_prompt = normalize_row(less.prompt_line());

    if pgr_prompt == less_prompt {
        Ok(())
    } else {
        Err(ScreenDiff {
            message: "prompt lines differ".to_string(),
            differing_rows: vec![RowDiff {
                row: pgr.terminal_rows.saturating_sub(1),
                pgr_text: pgr_prompt,
                less_text: less_prompt,
            }],
        })
    }
}
