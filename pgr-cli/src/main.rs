#![warn(clippy::pedantic)]

mod env;
mod options;

use std::io::Cursor;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};

use pgr_core::{Buffer, LineIndex, MarkStore};
use pgr_input::{stdin_is_pipe, LoadedFile, PipeBuffer, PreprocessResult, Preprocessor};
use pgr_keys::{
    find_tag, parse_lesskey_file, resolve_pattern, FileEntry, FileList, KeyReader, LesskeyConfig,
    Pager, RawTerminal, RuntimeOptions, TagState,
};

use crate::env::EnvConfig;
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

/// Create a [`FileEntry`] from in-memory byte data (preprocessor pipe output).
///
/// Immediately drains the data into the `PipeBuffer` so it's available for
/// random-access reads without needing further `refresh()` calls.
fn file_entry_from_bytes(data: Vec<u8>, display_name: String) -> anyhow::Result<FileEntry> {
    let mut pipe = PipeBuffer::new(Cursor::new(data));
    // Drain the cursor into the PipeBuffer so all data is immediately available.
    while !pipe.is_eof() {
        pipe.refresh()?;
    }
    let buf_len = pipe.len() as u64;
    let buffer: Box<dyn Buffer> = Box::new(pipe);
    let index = LineIndex::new(buf_len);
    Ok(FileEntry {
        path: None,
        display_name,
        buffer,
        index,
        marks: MarkStore::new(),
        saved_top_line: 0,
        saved_horizontal_offset: 0,
    })
}

/// Try to preprocess a file through LESSOPEN. Falls back to direct open.
///
/// If a preprocessor is active, runs it and returns the appropriate `FileEntry`.
/// Otherwise opens the file directly.
fn file_entry_with_preproc(
    path: &std::path::Path,
    preprocessor: Option<&Preprocessor>,
) -> anyhow::Result<FileEntry> {
    if let Some(preproc) = preprocessor {
        let filename = path.to_string_lossy();
        match preproc.preprocess(&filename)? {
            PreprocessResult::PipeData(data) => {
                let display_name = path.display().to_string();
                return file_entry_from_bytes(data, display_name);
            }
            PreprocessResult::ReplacementFile(repl_path) => {
                return file_entry_from_path(&repl_path);
            }
            PreprocessResult::Unchanged => {
                // Fall through to normal open.
            }
        }
    }
    file_entry_from_path(path)
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
/// Uses `LESS_SHELL_LINES` (via `env_config`) when set, otherwise terminal height.
fn check_quit_if_one_screen_file(
    file_list: &mut FileList,
    env_config: &EnvConfig,
) -> anyhow::Result<bool> {
    let (detected_rows, _) = terminal_dimensions();
    let rows = env_config.shell_screen_height(detected_rows);
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
/// Uses `LESS_SHELL_LINES` (via `env_config`) when set, otherwise terminal height.
fn check_quit_if_one_screen_pipe(
    buffer: &mut Box<dyn Buffer>,
    index: &mut LineIndex,
    env_config: &EnvConfig,
) -> anyhow::Result<bool> {
    let (detected_rows, _) = terminal_dimensions();
    let rows = env_config.shell_screen_height(detected_rows);
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

    let env_config = EnvConfig::from_env();

    let pipe = PipeBuffer::new(std::io::stdin());
    let mut buffer: Box<dyn Buffer> = Box::new(pipe);
    let mut index = LineIndex::new(buffer.len() as u64);

    if options.quit_if_one_screen
        && check_quit_if_one_screen_pipe(&mut buffer, &mut index, &env_config)?
    {
        return Ok(());
    }

    // Open /dev/tty twice: one handle for raw-mode RAII, one for key reading.
    let tty_raw = std::fs::File::open("/dev/tty")?;
    let raw_terminal = RawTerminal::enter(tty_raw.as_raw_fd())?;
    let (detected_rows, detected_cols) = raw_terminal.dimensions()?;
    let (rows, cols) = env_config.effective_dimensions(detected_rows, detected_cols);

    let tty_keys = std::fs::File::open("/dev/tty")?;
    let tty_keys_fd = tty_keys.as_raw_fd();
    let reader = KeyReader::new(tty_keys);
    let writer = std::io::stdout();

    let mut pager = Pager::new(
        reader,
        writer,
        buffer,
        index,
        Some(String::from("(standard input)")),
    );
    pager.set_key_fd(tty_keys_fd);
    configure_pager(&mut pager, options, rows, cols);
    apply_lesskey(&mut pager, options, &env_config);

    if env_config.secure_mode {
        pager.set_secure_mode(true);
    }

    if options.file_size {
        pager.index_all_immediate()?;
    }

    pager.run()?;
    drop(pager);
    drop(raw_terminal);
    drop(tty_raw);
    Ok(())
}

fn run_file_mode(options: &Options) -> anyhow::Result<()> {
    // Set up LESSOPEN preprocessor if configured and not disabled.
    let env_config = EnvConfig::from_env();
    let preprocessor = if options.no_lessopen || env_config.secure_mode {
        None
    } else {
        env_config.lessopen.as_deref().and_then(|lo| {
            let shell = env_config.shell_command();
            Preprocessor::new(lo, env_config.lessclose.as_deref(), shell)
        })
    };

    // Build a FileList from all named files (and `-` for stdin).
    let mut entries: Vec<FileEntry> = Vec::with_capacity(options.files.len());
    for path in &options.files {
        if path.to_str() == Some("-") {
            entries.push(file_entry_from_stdin());
        } else {
            entries.push(file_entry_with_preproc(path, preprocessor.as_ref())?);
        }
    }
    let mut file_list = FileList::new(entries.remove(0));
    for entry in entries {
        file_list.push(entry);
    }

    if options.quit_if_one_screen && check_quit_if_one_screen_file(&mut file_list, &env_config)? {
        return Ok(());
    }

    // Build the pager's own buffer for the first file.
    // For named files, re-open (FileList holds its own copy).
    // For preprocessed or stdin entries, create an appropriate buffer.
    let filename = file_list.current().display_name.clone();
    let first_path = options.files.first();
    let (buffer, index): (Box<dyn Buffer>, LineIndex) = if let Some(path) = first_path {
        if path.to_str() == Some("-") {
            let pipe = PipeBuffer::new(std::io::stdin());
            let buf: Box<dyn Buffer> = Box::new(pipe);
            let len = buf.len() as u64;
            (buf, LineIndex::new(len))
        } else if let Some(ref preproc) = preprocessor {
            let fname = path.to_string_lossy();
            match preproc.preprocess(&fname)? {
                PreprocessResult::PipeData(data) => {
                    let pipe = PipeBuffer::new(Cursor::new(data));
                    let buf: Box<dyn Buffer> = Box::new(pipe);
                    let len = buf.len() as u64;
                    (buf, LineIndex::new(len))
                }
                PreprocessResult::ReplacementFile(repl) => {
                    let loaded = LoadedFile::open(&repl)?;
                    loaded.into_parts()
                }
                PreprocessResult::Unchanged => {
                    let loaded = LoadedFile::open(path)?;
                    loaded.into_parts()
                }
            }
        } else {
            let loaded = LoadedFile::open(path)?;
            loaded.into_parts()
        }
    } else {
        let pipe = PipeBuffer::new(std::io::stdin());
        let buf: Box<dyn Buffer> = Box::new(pipe);
        let len = buf.len() as u64;
        (buf, LineIndex::new(len))
    };

    // Open /dev/tty twice: one handle for raw-mode RAII, one for key reading.
    let tty_raw = std::fs::File::open("/dev/tty")?;
    let raw_terminal = RawTerminal::enter(tty_raw.as_raw_fd())?;
    let (detected_rows, detected_cols) = raw_terminal.dimensions()?;
    let (rows, cols) = env_config.effective_dimensions(detected_rows, detected_cols);

    let tty_keys = std::fs::File::open("/dev/tty")?;
    let tty_keys_fd = tty_keys.as_raw_fd();
    let reader = KeyReader::new(tty_keys);
    let writer = std::io::stdout();

    let mut pager = Pager::new(reader, writer, buffer, index, Some(filename));
    pager.set_key_fd(tty_keys_fd);
    configure_pager(&mut pager, options, rows, cols);
    apply_lesskey(&mut pager, options, &env_config);

    if env_config.secure_mode {
        pager.set_secure_mode(true);
    }

    if file_list.file_count() > 1 {
        pager.set_file_list(file_list);
    }

    if options.file_size {
        pager.index_all_immediate()?;
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
    let (prompt_short, prompt_medium, prompt_long) = options.custom_prompt_overrides();
    let rt = RuntimeOptions {
        line_numbers: options.line_numbers,
        chop_long_lines: options.chop_long_lines,
        squeeze_blank_lines: options.squeeze_blank_lines,
        raw_control_mode: options.raw_mode(),
        tab_width: options.tab_width,
        tilde: options.tilde,
        status_column: options.status_column,
        prompt_string_short: prompt_short,
        prompt_string_medium: prompt_medium,
        prompt_string_long: prompt_long,
        ..RuntimeOptions::default()
    };
    pager.set_runtime_options(rt);

    if options.quit_at_eof {
        pager.set_quit_at_eof(true);
    }
    if options.quit_at_first_eof {
        pager.set_quit_at_first_eof(true);
    }

    // Wire follow mode enhancements.
    if options.follow_name {
        pager.set_follow_name(true);
    }
    if options.exit_follow_on_close {
        pager.set_exit_follow_on_close(true);
    }

    // Wire mouse support.
    if options.mouse || options.mouse_reversed {
        pager.set_mouse_enabled(true);
        let wheel_lines = options.wheel_lines.unwrap_or(3);
        if options.mouse_reversed {
            pager.set_wheel_reversed(wheel_lines);
        } else {
            pager.set_wheel_lines(wheel_lines);
        }
    }

    // Wire header lines (--header=N,C,G).
    let (header_lines, _header_cols, _header_gap) = options.header_params();
    if header_lines > 0 {
        pager.set_header_lines(header_lines);
    }

    // Wire initial commands (+cmd / ++cmd / --cmd).
    let mut initial_cmds = options.initial_commands.clone();
    if let Some(ref cmd) = options.cmd {
        initial_cmds.push(cmd.clone());
    }
    if !initial_cmds.is_empty() {
        pager.set_initial_commands(initial_cmds);
    }
    if !options.every_file_commands.is_empty() {
        pager.set_every_file_commands(options.every_file_commands.clone());
    }

    // Wire terminal behavior flags.
    if options.no_keypad {
        pager.set_no_keypad(true);
    }
    if options.no_vbell {
        pager.set_no_vbell(true);
    }
    if options.redraw_on_quit {
        pager.set_redraw_on_quit(true);
    }
}

/// Discover and load a lesskey source file.
///
/// Checks the following locations in priority order:
/// 1. `--lesskey-src=FILE` command-line flag
/// 2. `$LESSKEYIN` / `$LESSKEY` environment variable
/// 3. `~/.lesskey` (legacy path)
/// 4. `$XDG_CONFIG_HOME/lesskey` or `~/.config/lesskey`
///
/// Returns `None` if no lesskey file is found or if parsing fails.
/// Missing files are silently ignored; I/O errors are logged to stderr.
fn load_lesskey_config(options: &Options, env_config: &EnvConfig) -> Option<LesskeyConfig> {
    // 1. --lesskey-src flag takes highest priority
    if let Some(ref path) = options.lesskey_src {
        return load_lesskey_from_path(Path::new(path));
    }

    // 2. $LESSKEYIN / $LESSKEY environment variable
    if let Some(ref path) = env_config.lesskey {
        return load_lesskey_from_path(Path::new(path));
    }

    // 3. ~/.lesskey (legacy path)
    if let Some(ref home) = env_config.home {
        let legacy = PathBuf::from(home).join(".lesskey");
        if let Some(config) = load_lesskey_from_path(&legacy) {
            return Some(config);
        }
    }

    // 4. $XDG_CONFIG_HOME/lesskey or ~/.config/lesskey
    let xdg_config = env_config
        .xdg_config_home
        .as_deref()
        .map(PathBuf::from)
        .or_else(|| {
            env_config
                .home
                .as_ref()
                .map(|h| PathBuf::from(h).join(".config"))
        });
    if let Some(config_dir) = xdg_config {
        let xdg_path = config_dir.join("lesskey");
        if let Some(config) = load_lesskey_from_path(&xdg_path) {
            return Some(config);
        }
    }

    None
}

/// Try to load and parse a lesskey source file from the given path.
///
/// Returns `None` if the file does not exist. Logs to stderr on I/O errors.
fn load_lesskey_from_path(path: &std::path::Path) -> Option<LesskeyConfig> {
    match parse_lesskey_file(path) {
        Ok(Some(config)) if !config.command_bindings.is_empty() => Some(config),
        Ok(_) => None,
        Err(e) => {
            eprintln!(
                "pgr: warning: failed to read lesskey file {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// Apply lesskey configuration to the pager, if available.
///
/// Also emits a warning if the `-k` flag (binary lesskey format) was used.
fn apply_lesskey<R: std::io::Read, W: std::io::Write>(
    pager: &mut Pager<R, W>,
    options: &Options,
    env_config: &EnvConfig,
) {
    if options.lesskey_file.is_some() {
        eprintln!("pgr: warning: binary lesskey format (-k) is not supported; use --lesskey-src for source format");
    }

    if let Some(config) = load_lesskey_config(options, env_config) {
        pager.apply_lesskey_config(&config);
    }
}

fn run_tag_mode(options: &Options, tag: &str) -> anyhow::Result<()> {
    let tags_file_path = options.tag_file.as_deref().unwrap_or("tags");
    let tags_path = std::path::Path::new(tags_file_path);

    let entries = find_tag(tag, tags_path).map_err(|e| anyhow::anyhow!("{e}"))?;

    if entries.is_empty() {
        anyhow::bail!("tag not found: {tag}");
    }

    let first = &entries[0];
    let file_path = &first.file;
    let tag_state = TagState::new(entries.clone());

    // Resolve the pattern to a line number if possible.
    let target_line = std::fs::read_to_string(file_path)
        .ok()
        .and_then(|content| resolve_pattern(&first.pattern, &content));

    let loaded = LoadedFile::open(file_path)?;
    let display_name = file_path.display().to_string();
    let (buffer, index) = loaded.into_parts();

    let env_config = EnvConfig::from_env();

    let tty_raw = std::fs::File::open("/dev/tty")?;
    let raw_terminal = RawTerminal::enter(tty_raw.as_raw_fd())?;
    let (detected_rows, detected_cols) = raw_terminal.dimensions()?;
    let (rows, cols) = env_config.effective_dimensions(detected_rows, detected_cols);

    let tty_keys = std::fs::File::open("/dev/tty")?;
    let tty_keys_fd = tty_keys.as_raw_fd();
    let reader = KeyReader::new(tty_keys);
    let writer = std::io::stdout();

    let mut pager = Pager::new(reader, writer, buffer, index, Some(display_name));
    pager.set_key_fd(tty_keys_fd);
    configure_pager(&mut pager, options, rows, cols);
    pager.set_tag_state(tag_state);

    if env_config.secure_mode {
        pager.set_secure_mode(true);
    }

    // Position at the tag's location via an initial command.
    if let Some(line) = target_line {
        // +Ng jumps to 1-based line N.
        pager.set_initial_commands(vec![format!("{}g", line + 1)]);
    }

    pager.run()?;
    drop(pager);
    drop(raw_terminal);
    drop(tty_raw);
    Ok(())
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

    if let Some(ref tag) = options.tag {
        return run_tag_mode(&options, tag);
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
        let env_cfg = EnvConfig::default();
        let result = check_quit_if_one_screen_file(&mut file_list, &env_cfg).unwrap();
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
        let env_cfg = EnvConfig::default();
        let result = check_quit_if_one_screen_file(&mut file_list, &env_cfg).unwrap();
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
        let env_cfg = EnvConfig::default();
        let result = check_quit_if_one_screen_file(&mut file_list, &env_cfg).unwrap();
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

    // ── 14. LESS_SHELL_LINES overrides -F screen height ──────────────────

    #[test]
    fn test_quit_if_one_screen_respects_shell_lines() {
        // 5 lines of content. With shell_lines=3, it won't fit (returns false).
        let tmp = make_temp_file(b"line1\nline2\nline3\nline4\nline5\n");
        let entry = file_entry_from_path(tmp.path()).unwrap();
        let mut file_list = FileList::new(entry);
        let env_cfg = EnvConfig {
            shell_lines: Some(3),
            ..EnvConfig::default()
        };
        let result = check_quit_if_one_screen_file(&mut file_list, &env_cfg).unwrap();
        assert!(!result);
    }

    // ── 15. LESS_SHELL_LINES large enough -> fits ────────────────────────

    #[test]
    fn test_quit_if_one_screen_shell_lines_large_enough() {
        let tmp = make_temp_file(b"line1\nline2\n");
        let entry = file_entry_from_path(tmp.path()).unwrap();
        let mut file_list = FileList::new(entry);
        let env_cfg = EnvConfig {
            shell_lines: Some(100),
            ..EnvConfig::default()
        };
        let result = check_quit_if_one_screen_file(&mut file_list, &env_cfg).unwrap();
        assert!(result);
    }

    // ── Task 212: lesskey integration tests ──────────────────────────────

    #[test]
    fn test_load_lesskey_config_from_lesskey_src_flag() {
        let tmp = make_temp_file(b"x quit\n");
        let opts = Options::parse_from([
            "pgr",
            "--lesskey-src",
            tmp.path().to_str().unwrap(),
            "dummy.txt",
        ]);
        let env_cfg = EnvConfig::default();
        let config = load_lesskey_config(&opts, &env_cfg);
        assert!(config.is_some());
        let config = config.unwrap();
        assert!(!config.command_bindings.is_empty());
    }

    #[test]
    fn test_load_lesskey_config_from_env_lesskey() {
        let tmp = make_temp_file(b"x quit\n");
        let opts = Options::parse_from(["pgr", "dummy.txt"]);
        let env_cfg = EnvConfig {
            lesskey: Some(tmp.path().to_str().unwrap().to_string()),
            ..EnvConfig::default()
        };
        let config = load_lesskey_config(&opts, &env_cfg);
        assert!(config.is_some());
    }

    #[test]
    fn test_load_lesskey_config_default_paths_checked() {
        // With no lesskey-src, no env, and no home, should return None
        let opts = Options::parse_from(["pgr", "dummy.txt"]);
        let env_cfg = EnvConfig::default();
        let config = load_lesskey_config(&opts, &env_cfg);
        // No lesskey file exists in any default path in test environment
        assert!(config.is_none());
    }

    #[test]
    fn test_load_lesskey_config_missing_file_silently_ignored() {
        let opts = Options::parse_from([
            "pgr",
            "--lesskey-src",
            "/nonexistent/path/lesskey",
            "dummy.txt",
        ]);
        let env_cfg = EnvConfig::default();
        let config = load_lesskey_config(&opts, &env_cfg);
        assert!(config.is_none());
    }

    #[test]
    fn test_load_lesskey_config_lesskey_src_takes_priority_over_env() {
        let tmp_src = make_temp_file(b"x quit\n");
        let tmp_env = make_temp_file(b"y page-forward\n");
        let opts = Options::parse_from([
            "pgr",
            "--lesskey-src",
            tmp_src.path().to_str().unwrap(),
            "dummy.txt",
        ]);
        let env_cfg = EnvConfig {
            lesskey: Some(tmp_env.path().to_str().unwrap().to_string()),
            ..EnvConfig::default()
        };
        let config = load_lesskey_config(&opts, &env_cfg);
        assert!(config.is_some());
        let config = config.unwrap();
        // Should get the binding from --lesskey-src (x -> quit), not env
        assert_eq!(config.command_bindings.len(), 1);
        assert_eq!(config.command_bindings[0].key, pgr_keys::Key::Char('x'));
    }

    #[test]
    fn test_load_lesskey_from_path_nonexistent_returns_none() {
        let result = load_lesskey_from_path(&PathBuf::from("/no/such/file"));
        assert!(result.is_none());
    }

    #[test]
    fn test_load_lesskey_from_path_empty_file_returns_none() {
        let tmp = make_temp_file(b"");
        let result = load_lesskey_from_path(&tmp.path().to_path_buf());
        assert!(result.is_none());
    }

    #[test]
    fn test_load_lesskey_from_path_valid_file_returns_config() {
        let tmp = make_temp_file(b"x quit\n");
        let result = load_lesskey_from_path(&tmp.path().to_path_buf());
        assert!(result.is_some());
    }
}
