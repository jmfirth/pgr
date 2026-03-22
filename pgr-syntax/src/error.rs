//! Error types for the pgr-syntax crate.

use thiserror::Error;

/// Errors that can occur during syntax highlighting.
#[derive(Debug, Error)]
pub enum SyntaxError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A generic syntax highlighting error.
    #[error("{0}")]
    Message(String),
}

/// A specialized `Result` type for syntax operations.
pub type Result<T> = std::result::Result<T, SyntaxError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syntax_error_io_display_shows_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "grammar not found");
        let err = SyntaxError::Io(io_err);
        assert_eq!(err.to_string(), "I/O error: grammar not found");
    }

    #[test]
    fn test_syntax_error_message_display_shows_text() {
        let err = SyntaxError::Message("unsupported language".to_string());
        assert_eq!(err.to_string(), "unsupported language");
    }

    #[test]
    fn test_syntax_error_from_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "parse failed");
        let err: SyntaxError = SyntaxError::from(io_err);
        assert!(matches!(err, SyntaxError::Io(_)));
        assert_eq!(err.to_string(), "I/O error: parse failed");
    }
}
