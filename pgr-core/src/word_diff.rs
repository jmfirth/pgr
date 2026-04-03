//! Word-level diff — tokenize lines and compute changed spans using LCS.
//!
//! This module provides pure-algorithm word diffing for use by the display
//! layer (Task 354). It identifies exactly which byte spans within a removed
//! and an added line differ, enabling fine-grained inline highlighting.

/// A single changed span within an old/new line pair.
///
/// Byte offsets are half-open (`[start, end)`). An empty span (`start == end`)
/// in one side indicates a pure insertion or deletion at that position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WordChange {
    /// Byte offset in the old line where the change starts.
    pub old_start: usize,
    /// Byte offset in the old line where the change ends (exclusive).
    pub old_end: usize,
    /// Byte offset in the new line where the change starts.
    pub new_start: usize,
    /// Byte offset in the new line where the change ends (exclusive).
    pub new_end: usize,
}

/// Maximum token count before falling back to line-level diff.
const MAX_TOKENS: usize = 200;

/// Compare an old (removed) line with a new (added) line at word granularity.
///
/// Returns the changed spans in both lines. If either line exceeds [`MAX_TOKENS`]
/// tokens the function returns an empty `Vec`, meaning no word-level detail
/// (the caller should treat the entire lines as changed).
#[must_use]
pub fn compute_word_diff(old_line: &str, new_line: &str) -> Vec<WordChange> {
    let old_tokens = tokenize(old_line);
    let new_tokens = tokenize(new_line);

    // Fall back to line-level diff for very long lines.
    if old_tokens.len() > MAX_TOKENS || new_tokens.len() > MAX_TOKENS {
        return Vec::new();
    }

    let ops = lcs_edit_script(old_line, new_line, &old_tokens, &new_tokens);
    build_changes(old_line, new_line, &old_tokens, &new_tokens, &ops)
}

/// Match removed lines with added lines in a hunk for word-level diffing.
///
/// Pairs consecutive removed and added lines 1:1. If the counts differ the
/// shorter side is exhausted first; remaining lines on the longer side are
/// unpaired and receive no word-level diff.
///
/// Both slices contain **indices** (e.g., buffer line numbers) that the caller
/// uses to retrieve the actual line text. This function does not inspect the
/// content; it only produces pairing metadata.
#[must_use]
pub fn pair_changed_lines(removed: &[usize], added: &[usize]) -> Vec<(usize, usize)> {
    removed
        .iter()
        .zip(added.iter())
        .map(|(&r, &a)| (r, a))
        .collect()
}

// ── Tokenizer ────────────────────────────────────────────────────────────────

/// A token: a byte range `[start, end)` within the source line.
#[derive(Debug, Clone, Copy)]
struct Token {
    start: usize,
    end: usize,
}

/// Tokenize a line into word tokens.
///
/// Each token is either:
/// - a maximal run of alphanumeric characters or `_`
/// - a single non-alphanumeric character (including whitespace, punctuation)
///
/// For non-ASCII code points the UTF-8 multi-byte sequence is emitted as a
/// single token (one `char` boundary to the next).
fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = line.char_indices().peekable();

    while let Some((start, ch)) = chars.next() {
        if ch.is_alphanumeric() || ch == '_' {
            // Consume the full word run.
            let mut end = start + ch.len_utf8();
            while let Some(&(next_start, next_ch)) = chars.peek() {
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    end = next_start + next_ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token { start, end });
        } else {
            // Single character token.
            tokens.push(Token {
                start,
                end: start + ch.len_utf8(),
            });
        }
    }

    tokens
}

// ── LCS via dynamic programming ──────────────────────────────────────────────

/// Edit operation in the diff script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Equal,
    Delete,
    Insert,
}

/// Compute an LCS edit script from `old_tokens` to `new_tokens`.
///
/// Returns a `Vec<Op>` where `Equal` advances both indices, `Delete` advances
/// the old index only, and `Insert` advances the new index only.
fn lcs_edit_script(
    old_src: &str,
    new_src: &str,
    old_tokens: &[Token],
    new_tokens: &[Token],
) -> Vec<Op> {
    let m = old_tokens.len();
    let n = new_tokens.len();

    if m == 0 && n == 0 {
        return Vec::new();
    }

    // Build the LCS length table. dp[i][j] = LCS length for old[..i] vs new[..j].
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            let ot = old_tokens[i - 1];
            let nt = new_tokens[j - 1];
            if old_src[ot.start..ot.end] == new_src[nt.start..nt.end] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }

    // Backtrack to reconstruct the edit script (reversed, then flipped).
    let mut ops = Vec::new();
    let mut i = m;
    let mut j = n;
    while i > 0 || j > 0 {
        if i > 0 && j > 0 {
            let ot = old_tokens[i - 1];
            let nt = new_tokens[j - 1];
            if old_src[ot.start..ot.end] == new_src[nt.start..nt.end] {
                ops.push(Op::Equal);
                i -= 1;
                j -= 1;
                continue;
            }
        }
        if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            ops.push(Op::Insert);
            j -= 1;
        } else {
            ops.push(Op::Delete);
            i -= 1;
        }
    }

    ops.reverse();
    ops
}

// ── Change builder ────────────────────────────────────────────────────────────

/// Convert an edit script into [`WordChange`] spans.
///
/// Consecutive deletes and inserts are merged into a single `WordChange`
/// covering the full replaced region.
fn build_changes(
    old_src: &str,
    new_src: &str,
    old_tokens: &[Token],
    new_tokens: &[Token],
    ops: &[Op],
) -> Vec<WordChange> {
    let mut changes = Vec::new();
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;

    let mut op_iter = ops.iter().peekable();
    while let Some(&op) = op_iter.next() {
        match op {
            Op::Equal => {
                old_idx += 1;
                new_idx += 1;
            }
            Op::Delete | Op::Insert => {
                // Collect a run of deletes and inserts as one WordChange.
                let old_start_idx = old_idx;
                let new_start_idx = new_idx;

                if op == Op::Delete {
                    old_idx += 1;
                } else {
                    new_idx += 1;
                }

                while let Some(&&next_op) = op_iter.peek() {
                    if next_op == Op::Delete || next_op == Op::Insert {
                        op_iter.next();
                        if next_op == Op::Delete {
                            old_idx += 1;
                        } else {
                            new_idx += 1;
                        }
                    } else {
                        break;
                    }
                }

                // Byte offsets for the changed region in the old line.
                let old_byte_start = old_tokens
                    .get(old_start_idx)
                    .map_or(old_src.len(), |t| t.start);
                let old_byte_end = old_idx
                    .checked_sub(1)
                    .and_then(|last| old_tokens.get(last))
                    .map_or_else(
                        || {
                            old_tokens
                                .get(old_start_idx)
                                .map_or(old_src.len(), |t| t.start)
                        },
                        |t| t.end,
                    );

                // Byte offsets for the changed region in the new line.
                let new_byte_start = new_tokens
                    .get(new_start_idx)
                    .map_or(new_src.len(), |t| t.start);
                let new_byte_end = new_idx
                    .checked_sub(1)
                    .and_then(|last| new_tokens.get(last))
                    .map_or_else(
                        || {
                            new_tokens
                                .get(new_start_idx)
                                .map_or(new_src.len(), |t| t.start)
                        },
                        |t| t.end,
                    );

                changes.push(WordChange {
                    old_start: old_byte_start,
                    old_end: old_byte_end,
                    new_start: new_byte_start,
                    new_end: new_byte_end,
                });
            }
        }
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── tokenize ──

    #[test]
    fn test_tokenize_words_and_spaces() {
        let s = "hello world";
        let tokens = tokenize(s);
        // "hello", " ", "world"
        assert_eq!(tokens.len(), 3);
        assert_eq!(&s[tokens[0].start..tokens[0].end], "hello");
        assert_eq!(&s[tokens[1].start..tokens[1].end], " ");
        assert_eq!(&s[tokens[2].start..tokens[2].end], "world");
    }

    #[test]
    fn test_tokenize_punctuation() {
        let s = "foo()";
        let tokens = tokenize(s);
        // "foo", "(", ")"
        assert_eq!(tokens.len(), 3);
        assert_eq!(&s[tokens[0].start..tokens[0].end], "foo");
        assert_eq!(&s[tokens[1].start..tokens[1].end], "(");
        assert_eq!(&s[tokens[2].start..tokens[2].end], ")");
    }

    #[test]
    fn test_tokenize_empty() {
        assert!(tokenize("").is_empty());
    }

    // ── compute_word_diff ──

    #[test]
    fn test_compute_word_diff_simple_word_change() {
        // "hello world" vs "hello earth" → "world"/"earth" changed
        let changes = compute_word_diff("hello world", "hello earth");
        assert_eq!(changes.len(), 1);
        let c = &changes[0];
        assert_eq!(&"hello world"[c.old_start..c.old_end], "world");
        assert_eq!(&"hello earth"[c.new_start..c.new_end], "earth");
    }

    #[test]
    fn test_compute_word_diff_added_word() {
        // "a b" vs "a c b" → "c " inserted (somewhere in the middle)
        let changes = compute_word_diff("a b", "a c b");
        assert!(!changes.is_empty());
        let inserted: String = changes
            .iter()
            .map(|c| &"a c b"[c.new_start..c.new_end])
            .collect();
        assert!(
            inserted.contains('c'),
            "expected 'c' in inserted span, got: {inserted:?}"
        );
    }

    #[test]
    fn test_compute_word_diff_removed_word() {
        // "a b c" vs "a c" → "b " removed
        let changes = compute_word_diff("a b c", "a c");
        assert!(!changes.is_empty());
        let removed: String = changes
            .iter()
            .map(|c| &"a b c"[c.old_start..c.old_end])
            .collect();
        assert!(
            removed.contains('b'),
            "expected 'b' in removed span, got: {removed:?}"
        );
    }

    #[test]
    fn test_compute_word_diff_punctuation_change() {
        // "foo()" vs "foo()?" → "?" appended; old span is empty, new span is "?"
        let changes = compute_word_diff("foo()", "foo()?");
        assert_eq!(changes.len(), 1);
        let c = &changes[0];
        assert_eq!(&"foo()"[c.old_start..c.old_end], "");
        assert_eq!(&"foo()?"[c.new_start..c.new_end], "?");
    }

    #[test]
    fn test_compute_word_diff_identical_lines() {
        let changes = compute_word_diff("same", "same");
        assert!(changes.is_empty());
    }

    #[test]
    fn test_compute_word_diff_empty_old() {
        // "" vs "added" → whole new line is a change
        let changes = compute_word_diff("", "added");
        assert_eq!(changes.len(), 1);
        let c = &changes[0];
        assert_eq!(c.old_start, 0);
        assert_eq!(c.old_end, 0);
        assert_eq!(&"added"[c.new_start..c.new_end], "added");
    }

    #[test]
    fn test_compute_word_diff_empty_new() {
        // "removed" vs "" → whole old line is a change
        let changes = compute_word_diff("removed", "");
        assert_eq!(changes.len(), 1);
        let c = &changes[0];
        assert_eq!(&"removed"[c.old_start..c.old_end], "removed");
        assert_eq!(c.new_start, 0);
        assert_eq!(c.new_end, 0);
    }

    #[test]
    fn test_compute_word_diff_long_line_fallback() {
        // Build a line with >200 tokens (201 words separated by spaces).
        let long: String = (0..201).map(|i| format!("w{i} ")).collect();
        let long = long.trim_end().to_string();
        let changes = compute_word_diff(&long, "something else");
        assert!(changes.is_empty(), "expected empty fallback for long line");
    }

    // ── pair_changed_lines ──

    #[test]
    fn test_pair_changed_lines_equal_counts() {
        let removed = vec![10, 11, 12];
        let added = vec![20, 21, 22];
        let pairs = pair_changed_lines(&removed, &added);
        assert_eq!(pairs, vec![(10, 20), (11, 21), (12, 22)]);
    }

    #[test]
    fn test_pair_changed_lines_more_added() {
        // 2 removed + 4 added → 2 pairs, 2 unpaired additions
        let removed = vec![10, 11];
        let added = vec![20, 21, 22, 23];
        let pairs = pair_changed_lines(&removed, &added);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], (10, 20));
        assert_eq!(pairs[1], (11, 21));
    }

    #[test]
    fn test_pair_changed_lines_only_additions() {
        let removed: Vec<usize> = vec![];
        let added = vec![5, 6, 7];
        let pairs = pair_changed_lines(&removed, &added);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_pair_changed_lines_only_removals() {
        let removed = vec![5, 6, 7];
        let added: Vec<usize> = vec![];
        let pairs = pair_changed_lines(&removed, &added);
        assert!(pairs.is_empty());
    }

    #[test]
    fn test_pair_changed_lines_empty() {
        let pairs = pair_changed_lines(&[], &[]);
        assert!(pairs.is_empty());
    }
}
