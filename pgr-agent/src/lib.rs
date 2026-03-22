#![warn(clippy::pedantic)]
//! Pgr Protocol server, NDJSON, event subscriptions, and batch mode.

pub mod error;
pub use error::{AgentError, Result};
