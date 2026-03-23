use std::fmt;

use super::harness::ScreenCapture;

/// Difference report between two screen captures.
#[derive(Debug)]
pub struct ScreenDiff {
    /// Rows that differ.
    pub differing_rows: Vec<RowDiff>,
    /// Summary message.
    pub message: String,
}

/// A single row difference between pgr and less output.
#[derive(Debug)]
pub struct RowDiff {
    /// Row index.
    pub row: usize,
    /// The text pgr produced.
    pub pgr_text: String,
    /// The text less produced.
    pub less_text: String,
}

impl fmt::Display for ScreenDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}", self.message)?;
        writeln!(f, "{} row(s) differ:", self.differing_rows.len())?;
        for diff in &self.differing_rows {
            writeln!(f, "  row {}:", diff.row)?;
            writeln!(f, "    pgr:  {:?}", diff.pgr_text)?;
            writeln!(f, "    less: {:?}", diff.less_text)?;
        }
        Ok(())
    }
}

/// Compare two screen captures fully (all rows including prompt).
///
/// Returns `Ok(())` if they match, or panics with a readable diff describing
/// the differences.
pub fn compare_screens(pgr: &ScreenCapture, less: &ScreenCapture) {
    let diff = diff_rows(&pgr.rows, &less.rows);
    if !diff.is_empty() {
        let screen_diff = ScreenDiff {
            message: "full screen comparison failed".to_string(),
            differing_rows: diff,
        };
        panic!("{screen_diff}");
    }
}

/// Compare only the content area (all rows except the prompt/status line).
///
/// This is the most common comparison mode — the prompt format may differ
/// between pgr and less, but the content area must be identical.
pub fn compare_content(pgr: &ScreenCapture, less: &ScreenCapture) {
    let pgr_content = pgr.content_lines();
    let less_content = less.content_lines();

    let diff = diff_rows(pgr_content, less_content);
    if !diff.is_empty() {
        let screen_diff = ScreenDiff {
            message: "content area comparison failed (prompt line excluded)".to_string(),
            differing_rows: diff,
        };
        panic!("{screen_diff}");
    }
}

/// Compare with normalization: strip trailing whitespace on each row.
///
/// This is useful for cases where minor trailing whitespace differences
/// are expected but the visible content should match.
pub fn compare_normalized(pgr: &ScreenCapture, less: &ScreenCapture) {
    let pgr_trimmed: Vec<String> = pgr
        .content_lines()
        .iter()
        .map(|s| s.trim_end().to_string())
        .collect();
    let less_trimmed: Vec<String> = less
        .content_lines()
        .iter()
        .map(|s| s.trim_end().to_string())
        .collect();

    let diff = diff_rows(&pgr_trimmed, &less_trimmed);
    if !diff.is_empty() {
        let screen_diff = ScreenDiff {
            message: "normalized content comparison failed (trailing ws stripped)".to_string(),
            differing_rows: diff,
        };
        panic!("{screen_diff}");
    }
}

/// Compare prompt lines specifically.
pub fn compare_prompts(pgr: &ScreenCapture, less: &ScreenCapture) {
    let pgr_prompt = pgr.prompt_line().trim_end();
    let less_prompt = less.prompt_line().trim_end();

    if pgr_prompt != less_prompt {
        panic!(
            "prompt line mismatch:\n  pgr:  {:?}\n  less: {:?}",
            pgr_prompt, less_prompt
        );
    }
}

/// Compute row-level diffs between two slices of strings.
fn diff_rows(pgr_rows: &[String], less_rows: &[String]) -> Vec<RowDiff> {
    let max_rows = pgr_rows.len().max(less_rows.len());
    let mut diffs = Vec::new();

    for i in 0..max_rows {
        let pgr_text = pgr_rows.get(i).map_or("", String::as_str);
        let less_text = less_rows.get(i).map_or("", String::as_str);

        // Normalize by trimming trailing whitespace for comparison.
        if pgr_text.trim_end() != less_text.trim_end() {
            diffs.push(RowDiff {
                row: i,
                pgr_text: pgr_text.to_string(),
                less_text: less_text.to_string(),
            });
        }
    }

    diffs
}
