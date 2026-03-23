#![warn(clippy::pedantic)]
//! Raw terminal mode, key binding, lesskey parsing, and command dispatch.

pub mod command;
pub mod dispatch;
pub mod error;
pub mod file_list;
pub mod filename;
pub mod help;
pub mod info;
pub mod key;
pub mod key_reader;
pub mod keymap;
pub mod line_editor;
pub mod runtime_options;
pub mod shell;
pub mod terminal;

pub use command::Command;
pub use dispatch::{Pager, PendingCommand};
pub use error::{KeyError, Result};
pub use file_list::{FileEntry, FileList, FileListError};
pub use filename::{expand_filename, FilenameError};
pub use help::{version_string, HELP_TEXT};
pub use info::format_file_info;
pub use key::Key;
pub use key_reader::KeyReader;
pub use keymap::Keymap;
pub use line_editor::{LineEditResult, LineEditor};
pub use runtime_options::{HiliteMode, OptionError, RuntimeOptions};
pub use terminal::RawTerminal;
