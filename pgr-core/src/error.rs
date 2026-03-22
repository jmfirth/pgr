//! Error types for the pgr-core crate.

use thiserror::Error;

/// Errors that can occur in core buffer and indexing operations.
#[derive(Debug, Error)]
pub enum CoreError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A buffer operation failed.
    #[error("buffer error: {0}")]
    Buffer(String),

    /// A line index was out of bounds.
    #[error("index out of bounds: line {line}, total {total}")]
    LineOutOfBounds {
        /// The requested line number.
        line: usize,
        /// The total number of lines available.
        total: usize,
    },

    /// An invalid mark character was specified.
    #[error("invalid mark: {0}")]
    InvalidMark(char),
}

/// A specialized `Result` type for core operations.
pub type Result<T> = std::result::Result<T, CoreError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_error_io_display_shows_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = CoreError::Io(io_err);
        assert_eq!(err.to_string(), "I/O error: file not found");
    }

    #[test]
    fn test_core_error_buffer_display_shows_message() {
        let err = CoreError::Buffer("overflow".to_string());
        assert_eq!(err.to_string(), "buffer error: overflow");
    }

    #[test]
    fn test_core_error_line_out_of_bounds_display_shows_line_and_total() {
        let err = CoreError::LineOutOfBounds { line: 10, total: 5 };
        assert_eq!(err.to_string(), "index out of bounds: line 10, total 5");
    }

    #[test]
    fn test_core_error_invalid_mark_display_shows_char() {
        let err = CoreError::InvalidMark('z');
        assert_eq!(err.to_string(), "invalid mark: z");
    }

    #[test]
    fn test_core_error_from_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err: CoreError = CoreError::from(io_err);
        assert!(matches!(err, CoreError::Io(_)));
        assert_eq!(err.to_string(), "I/O error: access denied");
    }
}
