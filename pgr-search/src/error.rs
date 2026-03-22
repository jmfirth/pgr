//! Error types for the pgr-search crate.

use thiserror::Error;

/// Errors that can occur during search operations.
#[derive(Debug, Error)]
pub enum SearchError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A search pattern was invalid.
    #[error("invalid pattern: {0}")]
    InvalidPattern(String),
}

/// A specialized `Result` type for search operations.
pub type Result<T> = std::result::Result<T, SearchError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_error_io_display_shows_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = SearchError::Io(io_err);
        assert_eq!(err.to_string(), "I/O error: file not found");
    }

    #[test]
    fn test_search_error_invalid_pattern_display_shows_pattern() {
        let err = SearchError::InvalidPattern("unclosed group (".to_string());
        assert_eq!(err.to_string(), "invalid pattern: unclosed group (");
    }

    #[test]
    fn test_search_error_from_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "disk error");
        let err: SearchError = SearchError::from(io_err);
        assert!(matches!(err, SearchError::Io(_)));
        assert_eq!(err.to_string(), "I/O error: disk error");
    }
}
