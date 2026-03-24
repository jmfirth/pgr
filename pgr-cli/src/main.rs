#![warn(clippy::pedantic)]

mod env;
mod options;

use std::os::unix::io::AsRawFd;

use pgr_core::{Buffer, LineIndex, MarkStore};
use pgr_input::{stdin_is_pipe, LoadedFile, PipeBuffer};
use pgr_keys::{FileEntry, FileList, KeyReader, Pager, RawTerminal, RuntimeOptions};

use crate::options::Options;

/// Query terminal dimensions without entering raw mode.
///
/// Uses `TIOCGWINSZ` on `/dev/tty`. Falls back to `(24, 80)`.
fn terminal_dimensions() -> (usize, usize) {
    if let Ok(tty) = std::fs::File::open("/dev/tty") {
        let fd = tty.as_raw_fd();
        let mut ws = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: `TIOCGWINSZ` reads terminal size into a valid stack `winsize`.
        let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &raw mut ws) };
        if ret == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
            return (ws.ws_row as usize, ws.ws_col as usize);
        }
    }
    (24, 80)
}

/// Create a [`FileEntry`] from a named file on disk.
fn file_entry_from_path(path: &std::path::Path) -> anyhow::Result<FileEntry> {
    let loaded = LoadedFile::open(path)?;
    let display_name = path.display().to_string();
    let (buffer, index) = loaded.into_parts();
    Ok(FileEntry {
        path: Some(path.to_path_buf()),
        display_name,
        buffer,
        index,
        marks: MarkStore::new(),
        saved_top_line: 0,
        saved_horizontal_offset: 0,
    })
}

/// Create a [`FileEntry`] backed by stdin via a [`PipeBuffer`].
///
/// Used when `-` is specified as a filename in the file list.
fn file_entry_from_stdin() -> FileEntry {
    let pipe = PipeBuffer::new(std::io::stdin());
    let buffer: Box<dyn Buffer> = Box::new(pipe);
    let buf_len = buffer.len() as u64;
    let index = LineIndex::new(buf_len);
    FileEntry {
        path: None,
        display_name: String::from("(standard input)"),
        buffer,
        index,
        marks: MarkStore::new(),
        saved_top_line: 0,
        saved_horizontal_offset: 0,
    }
}

/// Determines whether input comes from stdin (pipe or `-` argument).
fn is_stdin_mode(options: &Options) -> bool {
    options.files.is_empty() || (options.files.len() == 1 && options.files[0].to_str() == Some("-"))
}

/// Check `-F` (quit-if-one-screen) for a file-backed buffer.
///
/// If content fits on one screen, prints it to stdout and returns `true`.
fn check_quit_if_one_screen_file(file_list: &mut FileList) -> anyhow::Result<bool> {
    let (rows, _) = terminal_dimensions();
    let content_rows = rows.saturating_sub(1);
    let entry = file_list.current_mut();
    let total = entry.index.total_lines(&*entry.buffer)?;
    if total <= content_rows {
        for i in 0..total {
            if let Some(line) = entry.index.get_line(i, &*entry.buffer)? {
                println!("{line}");
            }
        }
        return Ok(true);
    }
    Ok(false)
}

/// Check `-F` (quit-if-one-screen) for a pipe-backed buffer.
///
/// Reads enough data from the pipe to determine fit. If content fits,
/// prints it and returns `Ok(true)`.
fn check_quit_if_one_screen_pipe(
    buffer: &mut Box<dyn Buffer>,
    index: &mut LineIndex,
) -> anyhow::Result<bool> {
    let (rows, _) = terminal_dimensions();
    let content_rows = rows.saturating_sub(1);

    loop {
        let total = index.total_lines(&**buffer)?;
        if total > content_rows {
            return Ok(false);
        }
        let old_len = buffer.len();
        let new_len = buffer.refresh()?;
        if new_len == old_len {
            // EOF. Rebuild index with final length.
            *index = LineIndex::new(new_len as u64);
            break;
        }
        *index = LineIndex::new(new_len as u64);
    }

    let total = index.total_lines(&**buffer)?;
    if total <= content_rows {
        for i in 0..total {
            if let Some(line) = index.get_line(i, &**buffer)? {
                println!("{line}");
            }
        }
        return Ok(true);
    }
    Ok(false)
}

fn run_stdin_mode(options: &Options) -> anyhow::Result<()> {
    if !stdin_is_pipe() && options.files.is_empty() {
        eprintln!("pgr: missing filename (\"pgr --help\" for help)");
        std::process::exit(1);
    }

    let pipe = PipeBuffer::new(std::io::stdin());
    let mut buffer: Box<dyn Buffer> = Box::new(pipe);
    let mut index = LineIndex::new(buffer.len() as u64);

    if options.quit_if_one_screen && check_quit_if_one_screen_pipe(&mut buffer, &mut index)? {
        return Ok(());
    }

    // Open /dev/tty twice: one handle for raw-mode RAII, one for key reading.
    let tty_raw = std::fs::File::open("/dev/tty")?;
    let raw_terminal = RawTerminal::enter(tty_raw.as_raw_fd())?;
    let (rows, cols) = raw_terminal.dimensions()?;

    let tty_keys = std::fs::File::open("/dev/tty")?;
    let reader = KeyReader::new(tty_keys);
    let writer = std::io::stdout();

    let mut pager = Pager::new(
        reader,
        writer,
        buffer,
        index,
        Some(String::from("(standard input)")),
    );
    configure_pager(&mut pager, options, rows, cols);

    pager.run()?;
    drop(pager);
    drop(raw_terminal);
    drop(tty_raw);
    Ok(())
}

fn run_file_mode(options: &Options) -> anyhow::Result<()> {
    // Build a FileList from all named files (and `-` for stdin).
    let mut entries: Vec<FileEntry> = Vec::with_capacity(options.files.len());
    for path in &options.files {
        if path.to_str() == Some("-") {
            entries.push(file_entry_from_stdin());
        } else {
            entries.push(file_entry_from_path(path)?);
        }
    }
    let mut file_list = FileList::new(entries.remove(0));
    for entry in entries {
        file_list.push(entry);
    }

    if options.quit_if_one_screen && check_quit_if_one_screen_file(&mut file_list)? {
        return Ok(());
    }

    // Build the pager's own buffer for the first file.
    // For named files, re-open (FileList holds its own copy).
    // For stdin entries in the list, create a fresh PipeBuffer.
    let filename = file_list.current().display_name.clone();
    let (buffer, index): (Box<dyn Buffer>, LineIndex) =
        if let Some(path) = file_list.current().path.as_ref() {
            let loaded = LoadedFile::open(path)?;
            loaded.into_parts()
        } else {
            let pipe = PipeBuffer::new(std::io::stdin());
            let buf: Box<dyn Buffer> = Box::new(pipe);
            let len = buf.len() as u64;
            (buf, LineIndex::new(len))
        };

    // Open /dev/tty twice: one handle for raw-mode RAII, one for key reading.
    let tty_raw = std::fs::File::open("/dev/tty")?;
    let raw_terminal = RawTerminal::enter(tty_raw.as_raw_fd())?;
    let (rows, cols) = raw_terminal.dimensions()?;

    let tty_keys = std::fs::File::open("/dev/tty")?;
    let reader = KeyReader::new(tty_keys);
    let writer = std::io::stdout();

    let mut pager = Pager::new(reader, writer, buffer, index, Some(filename));
    configure_pager(&mut pager, options, rows, cols);

    if file_list.file_count() > 1 {
        pager.set_file_list(file_list);
    }

    pager.run()?;
    drop(pager);
    drop(raw_terminal);
    drop(tty_raw);
    Ok(())
}

/// Apply common option-derived settings to the pager.
fn configure_pager<R: std::io::Read, W: std::io::Write>(
    pager: &mut Pager<R, W>,
    options: &Options,
    rows: usize,
    cols: usize,
) {
    pager.set_raw_mode(options.raw_mode());
    pager.set_prompt_style(options.prompt_style());
    pager.set_tab_width(options.tab_width);
    pager.set_dimensions(rows, cols);

    // Wire all CLI display flags into runtime options (single call).
    let rt = RuntimeOptions {
        line_numbers: options.line_numbers,
        chop_long_lines: options.chop_long_lines,
        squeeze_blank_lines: options.squeeze_blank_lines,
        raw_control_mode: options.raw_mode(),
        tab_width: options.tab_width,
        tilde: options.tilde,
        ..RuntimeOptions::default()
    };
    pager.set_runtime_options(rt);

    if options.quit_at_eof {
        pager.set_quit_at_eof(true);
    }
    if options.quit_at_first_eof {
        pager.set_quit_at_first_eof(true);
    }

    // Wire initial commands (+cmd / ++cmd).
    if !options.initial_commands.is_empty() {
        pager.set_initial_commands(options.initial_commands.clone());
    }
    if !options.every_file_commands.is_empty() {
        pager.set_every_file_commands(options.every_file_commands.clone());
    }
}

fn main() -> anyhow::Result<()> {
    let options = Options::parse();

    if options.version {
        println!("pgr version {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if options.help {
        <Options as clap::Parser>::parse_from(["pgr", "--help"]);
        return Ok(());
    }

    if is_stdin_mode(&options) {
        run_stdin_mode(&options)
    } else {
        run_file_mode(&options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    /// Helper: create a temp file with the given content.
    fn make_temp_file(content: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("failed to create temp file");
        f.write_all(content).expect("failed to write");
        f.flush().expect("failed to flush");
        f
    }

    // ── 1. Stdin pipe detection ─────────────────────────────────────────

    #[test]
    fn test_stdin_is_pipe_returns_bool() {
        let _ = stdin_is_pipe();
    }

    // ── 2. Single file initialization creates one-entry FileList ────────

    #[test]
    fn test_single_file_creates_one_entry_file_list() {
        let tmp = make_temp_file(b"hello\nworld\n");

        let opts = Options::parse_from(["pgr", tmp.path().to_str().unwrap()]);
        let mut entries = Vec::new();
        for path in &opts.files {
            entries.push(file_entry_from_path(path).unwrap());
        }
        let file_list = FileList::new(entries.remove(0));
        assert_eq!(file_list.file_count(), 1);
        assert_eq!(
            file_list.current().path.as_ref().unwrap(),
            &PathBuf::from(tmp.path())
        );
    }

    // ── 3. Multiple files create a multi-entry FileList ─────────────────

    #[test]
    fn test_multiple_files_creates_multi_entry_file_list() {
        let tmp1 = make_temp_file(b"file1\n");
        let tmp2 = make_temp_file(b"file2\n");
        let tmp3 = make_temp_file(b"file3\n");

        let opts = Options::parse_from([
            "pgr",
            tmp1.path().to_str().unwrap(),
            tmp2.path().to_str().unwrap(),
            tmp3.path().to_str().unwrap(),
        ]);

        let mut entries = Vec::new();
        for path in &opts.files {
            entries.push(file_entry_from_path(path).unwrap());
        }
        let mut file_list = FileList::new(entries.remove(0));
        for entry in entries {
            file_list.push(entry);
        }
        assert_eq!(file_list.file_count(), 3);
        assert_eq!(file_list.current_index(), 0);
    }

    // ── 4. `-` as filename activates stdin mode ─────────────────────────

    #[test]
    fn test_dash_filename_is_stdin_mode() {
        let opts = Options::parse_from(["pgr", "-"]);
        assert!(is_stdin_mode(&opts));
    }

    // ── 5. -F with short file prints content (returns true) ─────────────

    #[test]
    fn test_quit_if_one_screen_file_short_returns_true() {
        let tmp = make_temp_file(b"short\n");
        let entry = file_entry_from_path(tmp.path()).unwrap();
        let mut file_list = FileList::new(entry);
        let result = check_quit_if_one_screen_file(&mut file_list).unwrap();
        assert!(result);
    }

    // ── 6. -F with long file enters pager (returns false) ───────────────

    #[test]
    fn test_quit_if_one_screen_file_long_returns_false() {
        let mut content = String::new();
        for i in 0..500 {
            content.push_str(&format!("line {i}\n"));
        }
        let tmp = make_temp_file(content.as_bytes());
        let entry = file_entry_from_path(tmp.path()).unwrap();
        let mut file_list = FileList::new(entry);
        let result = check_quit_if_one_screen_file(&mut file_list).unwrap();
        assert!(!result);
    }

    // ── 7. is_stdin_mode with no files ──────────────────────────────────

    #[test]
    fn test_is_stdin_mode_no_files() {
        let opts = Options::parse_from(["pgr"]);
        assert!(is_stdin_mode(&opts));
    }

    // ── 8. is_stdin_mode with named file is false ───────────────────────

    #[test]
    fn test_is_stdin_mode_named_file_is_false() {
        let opts = Options::parse_from(["pgr", "somefile.txt"]);
        assert!(!is_stdin_mode(&opts));
    }

    // ── 9. Empty file handled gracefully by -F ──────────────────────────

    #[test]
    fn test_quit_if_one_screen_empty_file_returns_true() {
        let tmp = make_temp_file(b"");
        let entry = file_entry_from_path(tmp.path()).unwrap();
        let mut file_list = FileList::new(entry);
        let result = check_quit_if_one_screen_file(&mut file_list).unwrap();
        assert!(result);
    }

    // ── 10. terminal_dimensions returns reasonable values ────────────────

    #[test]
    fn test_terminal_dimensions_returns_positive_values() {
        let (rows, cols) = terminal_dimensions();
        assert!(rows > 0);
        assert!(cols > 0);
    }

    // ── 11. Mixed files and stdin detection ──────────────────────────────

    #[test]
    fn test_is_stdin_mode_mixed_dash_and_files_is_false() {
        let opts = Options::parse_from(["pgr", "-", "extra.txt"]);
        assert!(!is_stdin_mode(&opts));
    }

    // ── 12. file_entry_from_path produces correct display name ──────────

    #[test]
    fn test_file_entry_from_path_display_name() {
        let tmp = make_temp_file(b"data\n");
        let entry = file_entry_from_path(tmp.path()).unwrap();
        assert_eq!(entry.display_name, tmp.path().display().to_string());
        assert!(entry.path.is_some());
    }

    // ── 13. file_entry_from_stdin produces correct metadata ─────────────

    #[test]
    fn test_file_entry_from_stdin_metadata() {
        let entry = file_entry_from_stdin();
        assert!(entry.path.is_none());
        assert_eq!(entry.display_name, "(standard input)");
    }
}
