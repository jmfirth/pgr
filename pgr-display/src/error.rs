//! Error types for the pgr-display crate.

use thiserror::Error;

/// Errors that can occur during display and rendering operations.
#[derive(Debug, Error)]
pub enum DisplayError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A terminal operation failed.
    #[error("terminal error: {0}")]
    TerminalError(String),

    /// An invalid color specification was provided.
    #[error("invalid color specification: {0}")]
    InvalidColor(String),
}

/// A specialized `Result` type for display operations.
pub type Result<T> = std::result::Result<T, DisplayError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_error_io_display_shows_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = DisplayError::Io(io_err);
        assert_eq!(err.to_string(), "I/O error: file not found");
    }

    #[test]
    fn test_display_error_terminal_error_display_shows_message() {
        let err = DisplayError::TerminalError("cannot detect terminal size".to_string());
        assert_eq!(
            err.to_string(),
            "terminal error: cannot detect terminal size"
        );
    }

    #[test]
    fn test_display_error_from_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "write failed");
        let err: DisplayError = DisplayError::from(io_err);
        assert!(matches!(err, DisplayError::Io(_)));
        assert_eq!(err.to_string(), "I/O error: write failed");
    }
}
