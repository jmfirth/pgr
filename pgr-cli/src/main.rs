#![warn(clippy::pedantic)]

mod env;
mod options;

use std::os::unix::io::AsRawFd;

use pgr_input::LoadedFile;
use pgr_keys::{KeyReader, Pager, RawTerminal};

use crate::options::Options;

fn main() -> anyhow::Result<()> {
    let options = Options::parse();

    if options.version {
        println!("pgr version {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if options.help {
        // Re-parse with --help to let clap print usage and exit.
        <Options as clap::Parser>::parse_from(["pgr", "--help"]);
        return Ok(());
    }

    if options.files.is_empty() {
        eprintln!("pgr: no files specified (stdin not yet supported)");
        std::process::exit(1);
    }

    // Open the first file (multi-file support is Phase 1).
    let loaded = LoadedFile::open(&options.files[0])?;
    let filename = options.files[0].display().to_string();
    let (buffer, index) = loaded.into_parts();

    // Set up terminal.
    let stdin_fd = std::io::stdin().as_raw_fd();
    let raw_terminal = RawTerminal::enter(stdin_fd)?;
    let (rows, cols) = raw_terminal.dimensions()?;

    // Initialize pager.
    let reader = KeyReader::new(std::io::stdin());
    let writer = std::io::stdout();

    let mut pager = Pager::new(reader, writer, buffer, index, Some(filename));
    pager.set_raw_mode(options.raw_mode());
    pager.set_prompt_style(options.prompt_style());
    pager.set_tab_width(options.tab_width);
    pager.set_dimensions(rows, cols);

    pager.run()?;

    // Terminal restored by raw_terminal drop.
    drop(raw_terminal);

    Ok(())
}
