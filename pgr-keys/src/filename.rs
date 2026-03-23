//! Filename expansion for `%` and `#` substitutions.
//!
//! In `less`, the `:e` command supports `%` to expand to the current filename
//! and `#` to expand to the previously viewed filename. Doubled characters
//! (`%%`, `##`) produce literal `%` and `#` respectively.

/// Errors from filename expansion.
#[derive(Debug, thiserror::Error)]
pub enum FilenameError {
    /// `%` was used but there is no current file.
    #[error("no current filename for % expansion")]
    NoCurrentFile,
    /// `#` was used but there is no previous file.
    #[error("no previous filename for # expansion")]
    NoPreviousFile,
}

/// Expand special characters in a filename string.
///
/// - `%` is replaced with the current file's path.
/// - `#` is replaced with the previously viewed file's path.
/// - `%%` is a literal `%`.
/// - `##` is a literal `#`.
///
/// # Errors
///
/// Returns [`FilenameError::NoCurrentFile`] if `%` is used but `current_file`
/// is `None`, or [`FilenameError::NoPreviousFile`] if `#` is used but
/// `previous_file` is `None`.
pub fn expand_filename(
    input: &str,
    current_file: Option<&str>,
    previous_file: Option<&str>,
) -> std::result::Result<String, FilenameError> {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '%' => {
                if chars.peek() == Some(&'%') {
                    chars.next();
                    result.push('%');
                } else {
                    let name = current_file.ok_or(FilenameError::NoCurrentFile)?;
                    result.push_str(name);
                }
            }
            '#' => {
                if chars.peek() == Some(&'#') {
                    chars.next();
                    result.push('#');
                } else {
                    let name = previous_file.ok_or(FilenameError::NoPreviousFile)?;
                    result.push_str(name);
                }
            }
            other => result.push(other),
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test 1: expand_filename with no special chars returns the input unchanged
    #[test]
    fn test_expand_filename_no_special_chars_returns_unchanged() {
        let result = expand_filename("somefile.txt", Some("cur"), Some("prev"));
        assert_eq!(result.unwrap(), "somefile.txt");
    }

    // Test 2: expand_filename with % replaces with current filename
    #[test]
    fn test_expand_filename_percent_replaces_with_current() {
        let result = expand_filename("%.bak", Some("myfile.txt"), None);
        assert_eq!(result.unwrap(), "myfile.txt.bak");
    }

    // Test 3: expand_filename with # replaces with previous filename
    #[test]
    fn test_expand_filename_hash_replaces_with_previous() {
        let result = expand_filename("#", None, Some("oldfile.txt"));
        assert_eq!(result.unwrap(), "oldfile.txt");
    }

    // Test 4: expand_filename with %% produces literal %
    #[test]
    fn test_expand_filename_double_percent_produces_literal_percent() {
        let result = expand_filename("100%%", Some("cur"), None);
        assert_eq!(result.unwrap(), "100%");
    }

    // Test 5: expand_filename with ## produces literal #
    #[test]
    fn test_expand_filename_double_hash_produces_literal_hash() {
        let result = expand_filename("test##value", None, Some("prev"));
        assert_eq!(result.unwrap(), "test#value");
    }

    // Test 6: expand_filename with % but no current file returns error
    #[test]
    fn test_expand_filename_percent_no_current_file_returns_error() {
        let result = expand_filename("%", None, Some("prev"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FilenameError::NoCurrentFile));
    }

    // Test 7: expand_filename with # but no previous file returns error
    #[test]
    fn test_expand_filename_hash_no_previous_file_returns_error() {
        let result = expand_filename("#", Some("cur"), None);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), FilenameError::NoPreviousFile));
    }

    // Additional: mixed expansion
    #[test]
    fn test_expand_filename_mixed_percent_and_hash() {
        let result = expand_filename("cp % #", Some("source.txt"), Some("dest.txt"));
        assert_eq!(result.unwrap(), "cp source.txt dest.txt");
    }

    // Additional: empty input
    #[test]
    fn test_expand_filename_empty_input_returns_empty() {
        let result = expand_filename("", Some("cur"), Some("prev"));
        assert_eq!(result.unwrap(), "");
    }

    // Additional: error display messages
    #[test]
    fn test_filename_error_no_current_file_display() {
        let err = FilenameError::NoCurrentFile;
        assert_eq!(err.to_string(), "no current filename for % expansion");
    }

    #[test]
    fn test_filename_error_no_previous_file_display() {
        let err = FilenameError::NoPreviousFile;
        assert_eq!(err.to_string(), "no previous filename for # expansion");
    }

    // Additional: %% at end of string
    #[test]
    fn test_expand_filename_double_percent_at_end() {
        let result = expand_filename("test%%", Some("cur"), None);
        assert_eq!(result.unwrap(), "test%");
    }

    // Additional: single % at end of string
    #[test]
    fn test_expand_filename_single_percent_at_end() {
        let result = expand_filename("test%", Some("current.txt"), None);
        assert_eq!(result.unwrap(), "testcurrent.txt");
    }
}
