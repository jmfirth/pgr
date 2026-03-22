#![warn(clippy::pedantic)]
//! Raw terminal mode, key binding, lesskey parsing, and command dispatch.

pub mod command;
pub mod dispatch;
pub mod error;
pub mod key;
pub mod key_reader;
pub mod keymap;
pub mod terminal;

pub use command::Command;
pub use dispatch::Pager;
pub use error::{KeyError, Result};
pub use key::Key;
pub use key_reader::KeyReader;
pub use keymap::Keymap;
pub use terminal::RawTerminal;
