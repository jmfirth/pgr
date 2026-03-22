//! Error types for the pgr-input crate.

use thiserror::Error;

/// Errors that can occur during input reading and processing.
#[derive(Debug, Error)]
pub enum InputError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A generic input error.
    #[error("{0}")]
    Message(String),

    /// An error propagated from pgr-core.
    #[error("core error: {0}")]
    Core(#[from] pgr_core::CoreError),
}

/// A specialized `Result` type for input operations.
pub type Result<T> = std::result::Result<T, InputError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_error_io_display_shows_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = InputError::Io(io_err);
        assert_eq!(err.to_string(), "I/O error: file not found");
    }

    #[test]
    fn test_input_error_message_display_shows_text() {
        let err = InputError::Message("unexpected EOF".to_string());
        assert_eq!(err.to_string(), "unexpected EOF");
    }

    #[test]
    fn test_input_error_core_display_shows_message() {
        let core_err = pgr_core::CoreError::Buffer("test failure".to_string());
        let err = InputError::Core(core_err);
        assert_eq!(err.to_string(), "core error: buffer error: test failure");
    }

    #[test]
    fn test_input_error_from_core_error_converts() {
        let core_err = pgr_core::CoreError::Buffer("conversion test".to_string());
        let err: InputError = InputError::from(core_err);
        assert!(matches!(err, InputError::Core(_)));
    }

    #[test]
    fn test_input_error_from_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken pipe");
        let err: InputError = InputError::from(io_err);
        assert!(matches!(err, InputError::Io(_)));
        assert_eq!(err.to_string(), "I/O error: broken pipe");
    }
}
