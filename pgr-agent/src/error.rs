//! Error types for the pgr-agent crate.

use thiserror::Error;

/// Errors that can occur in the agent protocol server.
#[derive(Debug, Error)]
pub enum AgentError {
    /// An I/O error occurred.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// A protocol violation or malformed message.
    #[error("protocol error: {0}")]
    ProtocolError(String),
}

/// A specialized `Result` type for agent operations.
pub type Result<T> = std::result::Result<T, AgentError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_error_io_display_shows_message() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "socket not found");
        let err = AgentError::Io(io_err);
        assert_eq!(err.to_string(), "I/O error: socket not found");
    }

    #[test]
    fn test_agent_error_protocol_error_display_shows_message() {
        let err = AgentError::ProtocolError("invalid JSON".to_string());
        assert_eq!(err.to_string(), "protocol error: invalid JSON");
    }

    #[test]
    fn test_agent_error_from_io_error_converts() {
        let io_err = std::io::Error::new(std::io::ErrorKind::ConnectionReset, "connection reset");
        let err: AgentError = AgentError::from(io_err);
        assert!(matches!(err, AgentError::Io(_)));
        assert_eq!(err.to_string(), "I/O error: connection reset");
    }
}
