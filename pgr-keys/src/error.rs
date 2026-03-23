//! Error types for the pgr-keys crate.

use thiserror::Error;

/// Errors that can occur during key binding and command dispatch.
#[derive(Debug, Error)]
pub enum KeyError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A core buffer/index operation failed.
    #[error("core error: {0}")]
    Core(#[from] pgr_core::CoreError),

    /// A key binding specification was invalid.
    #[error("invalid binding: {0}")]
    InvalidBinding(String),

    /// A search operation failed.
    #[error("search error: {0}")]
    Search(#[from] pgr_search::SearchError),
}

/// A specialized `Result` type for key operations.
pub type Result<T> = std::result::Result<T, KeyError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_error_io_display_shows_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err = KeyError::Io(io_err);
        assert_eq!(err.to_string(), "I/O error: file not found");
    }

    #[test]
    fn test_key_error_invalid_binding_display_shows_binding() {
        let err = KeyError::InvalidBinding("ctrl-??".to_string());
        assert_eq!(err.to_string(), "invalid binding: ctrl-??");
    }

    #[test]
    fn test_key_error_from_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::Other, "terminal read failed");
        let err: KeyError = KeyError::from(io_err);
        assert!(matches!(err, KeyError::Io(_)));
        assert_eq!(err.to_string(), "I/O error: terminal read failed");
    }

    #[test]
    fn test_key_error_core_display_shows_message() {
        let core_err = pgr_core::CoreError::Buffer("test failure".to_string());
        let err = KeyError::Core(core_err);
        assert_eq!(err.to_string(), "core error: buffer error: test failure");
    }

    #[test]
    fn test_key_error_from_core_error_converts() {
        let core_err = pgr_core::CoreError::Buffer("overflow".to_string());
        let err: KeyError = KeyError::from(core_err);
        assert!(matches!(err, KeyError::Core(_)));
    }
}
