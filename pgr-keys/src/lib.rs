#![warn(clippy::pedantic)]
//! Raw terminal mode, key binding, lesskey parsing, and command dispatch.

pub mod error;
pub mod key;
pub mod key_reader;
pub mod terminal;

pub use error::{KeyError, Result};
pub use key::Key;
pub use key_reader::KeyReader;
pub use terminal::RawTerminal;
