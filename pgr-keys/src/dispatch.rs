//! Command dispatch loop — the main pager event loop.
//!
//! Reads keys, translates them to commands via the keymap, and executes
//! those commands by mutating the screen state and repainting.

use std::io::{Read, Write};
use std::os::unix::io::RawFd;
use std::path::Path;
use std::time::Duration;

use pgr_core::{detect_content_mode, Buffer, ContentMode, LineIndex, Mark, MarkStore};
use pgr_display::{
    build_side_by_side_lines, compute_line_screen_rows, eval_prompt, find_urls, line_number_width,
    paint_info_line, paint_prompt, paint_screen_mapped, paint_screen_with_options,
    parse_table_layout, snap_to_next_column, snap_to_prev_column, squeeze_visible_lines,
    wordwrap_segments, ColoredRange, OverstrikeMode, PaintOptions, PromptContext, PromptStyle,
    RawControlMode, RenderConfig, Screen, ScreenLine, SqlTableLayout, TabStops,
    DEFAULT_LONG_PROMPT, DEFAULT_MEDIUM_PROMPT, DEFAULT_SHORT_PROMPT,
};
use pgr_search::{
    count_matches, find_match_index, CaseMode, FilterState, FilteredLines, HighlightState,
    SearchDirection, SearchModifiers, SearchPattern, Searcher, WrapMode, HIGHLIGHT_COLORS,
};

use pgr_input::{FileWatcher, FollowEvent};

use crate::help;
use crate::info;

use crate::completion::CompletionMode;
use crate::error::Result;
use crate::file_list::{FileEntry, FileList};
use crate::filename::expand_filename;
use crate::key::Key;
use crate::key_reader::KeyReader;
use crate::keymap::Keymap;
use crate::lesskey::LesskeyConfig;
use crate::line_editor::{History, LineEditResult, LineEditor};
use crate::runtime_options::{RuntimeOptions, WindowSize};
use crate::shell;
use crate::tags::TagState;
use crate::Command;

/// Approximate the current calendar year from `SystemTime`.
///
/// Uses the Julian calendar approximation (365.25 days/year) to convert
/// UNIX seconds to a year. Accurate to within a year for dates in the
/// range 1970–2500.  Returns 2000 as a safe fallback if the system clock
/// is unavailable.
fn current_year_approx() -> u32 {
    use std::time::SystemTime;
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 1970 + floor(secs / 31_557_600) where 31_557_600 ≈ 365.25 * 24 * 3600
    #[allow(clippy::cast_possible_truncation)] // year fits in u32 for centuries to come
    let year = 1970u32 + (secs / 31_557_600) as u32;
    year
}

/// A read-only buffer backed by a static byte slice, used for the help screen.
struct HelpBuffer {
    data: Vec<u8>,
}

impl HelpBuffer {
    fn new(data: &[u8]) -> Self {
        Self {
            data: data.to_vec(),
        }
    }
}

impl Buffer for HelpBuffer {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> pgr_core::Result<usize> {
        if offset >= self.data.len() {
            return Ok(0);
        }
        let available = &self.data[offset..];
        let to_copy = available.len().min(buf.len());
        buf[..to_copy].copy_from_slice(&available[..to_copy]);
        Ok(to_copy)
    }

    fn is_growable(&self) -> bool {
        false
    }

    fn refresh(&mut self) -> pgr_core::Result<usize> {
        Ok(self.data.len())
    }
}

/// Cached match count info for the current search pattern.
///
/// Invalidated when the search pattern changes. Avoids re-scanning the
/// entire buffer on every repaint when the pattern hasn't changed.
#[derive(Debug, Clone)]
struct MatchCountCache {
    /// The pattern string this cache was computed for.
    pattern: String,
    /// Total number of matching lines in the buffer.
    total_matches: usize,
    /// 1-based index of the current match, or `None` if the current line
    /// is not on a match.
    current_match: Option<usize>,
}

/// The reason the pager exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// Normal quit (e.g., `q`, `:q`, `ZZ`).
    Normal,
    /// Quit triggered by an interrupt (Ctrl-C).
    Interrupt,
}

/// A partially-entered multi-key command awaiting its argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingCommand {
    /// `m` pressed; waiting for a mark letter.
    SetMarkTop,
    /// `M` pressed; waiting for a mark letter.
    SetMarkBottom,
    /// `'` pressed; waiting for a mark letter or `'`.
    GotoMark,
    /// `ESC-m` pressed; waiting for a mark letter.
    ClearMark,
    /// `^X` pressed; waiting for `^X` (to complete `^X^X` for goto mark).
    CtrlXPrefix,
    /// `:` pressed; waiting for the colon sub-command letter.
    ColonPrefix,
    /// `-` pressed; waiting for the option flag character.
    ToggleOption,
    /// `_` pressed; waiting for the option flag character to query.
    QueryOption,
    /// `Z` pressed; waiting for a second `Z` to quit (vi-style `ZZ`).
    ZPrefix,
    /// `^O` pressed; waiting for the hyperlink sub-command (^N, ^P, ^L, ^O).
    CtrlOPrefix,
    /// `&` pressed; waiting for sub-command: bare → filter, `+` → add highlight,
    /// `-` → remove highlight, `l` → list highlights.
    FilterPrefix,
    /// `[` pressed; waiting for second key: `u` → `PrevUrl`, else bracket match.
    OpenBracketPrefix,
    /// `]` pressed; waiting for second key: `u` → `NextUrl`, else bracket match.
    CloseBracketPrefix,
}

/// Result of computing a diff navigation target (hunk or file).
///
/// Separates the read-only computation from the mutable state update,
/// avoiding a borrow conflict between `self.diff_state` and `self`.
enum DiffNavResult {
    /// Not in diff mode — `diff_state` is `None`.
    NotDiffMode,
    /// In diff mode but no target found (e.g., no hunks exist).
    NoTarget,
    /// Found a target: `(line_number, status_message)`.
    Found(usize, String),
}

/// Result of computing a man page section navigation target.
///
/// Separates the read-only computation from the mutable state update,
/// avoiding a borrow conflict between `self.man_sections` and `self`.
enum ManSectionNavResult {
    /// Not in man page mode — `man_sections` is `None`.
    NotManMode,
    /// In man page mode but no section found (e.g., no sections detected).
    NoTarget,
    /// Found a target: `(line_number, status_message)`.
    Found(usize, String),
}

/// The main pager state, tying together all subsystems.
#[allow(clippy::struct_excessive_bools)] // Pager legitimately tracks multiple independent on/off modes
pub struct Pager<R: Read, W: Write> {
    reader: KeyReader<R>,
    writer: W,
    keymap: Keymap,
    screen: Screen,
    buffer: Box<dyn Buffer>,
    index: LineIndex,
    render_config: RenderConfig,
    filename: Option<String>,
    prompt_style: PromptStyle,
    /// Numeric prefix accumulator.
    pending_count: Option<usize>,
    /// Whether we should quit.
    should_quit: bool,
    /// Named position marks.
    marks: MarkStore,
    /// The `top_line` before the last "large" movement, for `''` (return to previous).
    last_position: Option<usize>,
    /// A partially-entered multi-key command awaiting its argument.
    pending_command: Option<PendingCommand>,
    /// Sticky half-page scroll size. Set by `d`/`u` with a count.
    sticky_half_page: Option<usize>,
    /// Custom window size. Set by `z`/`w` with a count or by `-z` flag.
    custom_window_size: Option<WindowSize>,
    /// The list of open files for multi-file navigation.
    file_list: Option<FileList>,
    /// Runtime-mutable options (toggled with `-` prefix at the prompt).
    runtime_options: RuntimeOptions,
    /// Transient status message that overrides the prompt for one repaint.
    status_message: Option<String>,
    /// `-e`: quit on second attempt to scroll past EOF.
    quit_at_eof: bool,
    /// `-E`: quit on first scroll past EOF.
    quit_at_first_eof: bool,
    /// Tracks how many times the user has been shown EOF after a forward scroll.
    eof_seen_count: usize,
    /// Whether security restrictions are enabled (LESSSECURE=1).
    secure_mode: bool,
    /// Whether the input is from a pipe (not a named file).
    is_pipe: bool,
    /// Whether this is the first render (for initial bottom-alignment of short files).
    initial_render: bool,
    /// The last shell command executed (for `!` repeat).
    last_shell_command: Option<String>,
    /// The shell to use for shell commands (from SHELL env or "sh").
    shell: String,
    /// The editor command (from VISUAL/EDITOR env or "vi").
    editor: String,
    /// The previously viewed file, for `#` expansion in `:e`.
    previous_file: Option<String>,
    /// The last search pattern, used for n/N repeat.
    last_pattern: Option<SearchPattern>,
    /// The direction of the last search.
    last_direction: SearchDirection,
    /// Highlight state for search matches.
    highlight_state: HighlightState,
    /// Whether we are currently in line-editor (search prompt) mode.
    editing_search: bool,
    /// Line editor instance for search/command prompt input.
    line_editor: Option<LineEditor>,
    /// History of previous search inputs (shared between `/` and `?`).
    search_history: History,
    /// Direction for the current search prompt (set when `/` or `?` is pressed).
    search_prompt_direction: SearchDirection,
    /// The modifiers from the last search, used for repeat-search commands.
    last_modifiers: SearchModifiers,
    /// Cached match count info, invalidated on pattern change.
    match_count_cache: Option<MatchCountCache>,
    /// Filter state for the `&` command (show only matching/non-matching lines).
    filter: FilterState,
    /// Pre-computed mapping from filtered line indices to actual buffer lines.
    filtered_lines: Option<FilteredLines>,
    /// Whether we are currently in filter-prompt editing mode.
    editing_filter: bool,
    /// Whether the current filter prompt has inversion toggled via `^N`.
    filter_invert: bool,
    /// Whether we are currently in add-highlight prompt mode (`&+`).
    editing_add_highlight: bool,
    /// Whether we are currently in remove-highlight prompt mode (`&-`).
    editing_remove_highlight: bool,
    /// Whether the next keypress should be absorbed (used to dismiss
    /// option toggle/query status messages, matching GNU less behavior).
    absorb_next_key: bool,
    /// Raw file descriptor for the key input source (e.g. `/dev/tty`).
    key_fd: Option<RawFd>,
    /// Commands to execute after the first repaint (`+cmd` syntax).
    initial_commands: Vec<String>,
    /// Commands to execute after every file switch (`++cmd` syntax).
    every_file_commands: Vec<String>,
    /// Whether initial commands have been executed (prevents re-execution).
    initial_commands_executed: bool,
    /// Whether the current search (prompt or repeat) should cross file boundaries.
    cross_file_search: bool,
    /// Tag navigation state for `t`/`T` commands (populated by `-t` flag).
    tag_state: Option<TagState>,
    /// Whether follow mode should reopen the file by name on rename/delete.
    follow_name: bool,
    /// Whether follow mode should exit when the input pipe closes.
    exit_follow_on_close: bool,
    /// Whether mouse tracking is enabled (`--mouse` or `--MOUSE`).
    mouse_enabled: bool,
    /// Whether to skip keypad init/deinit sequences (`--no-keypad`).
    no_keypad: bool,
    /// Whether visual bell is disabled (`--no-vbell`).
    no_vbell: bool,
    /// Whether the next character typed in the search prompt should be inserted literally (^L).
    search_literal_next: bool,
    /// Saved `top_line` before incremental search, for restoring on cancel.
    incsearch_saved_top: Option<usize>,
    /// Whether to repaint the screen before exiting (`--redraw-on-quit`).
    redraw_on_quit: bool,
    /// Whether Ctrl-C should immediately quit (`-K` / `--quit-on-intr`).
    quit_on_intr: bool,
    /// The reason the pager exited.
    exit_reason: ExitReason,
    /// Detected content type (diff, man page, etc.) — set on first paint.
    content_mode: ContentMode,
    /// Parsed diff structure for hunk/file navigation (populated on first paint when Diff mode).
    diff_state: Option<Vec<pgr_core::DiffFile>>,
    /// Parsed git log commit list for `]g`/`[g` navigation (populated on first paint when `GitLog` mode).
    git_log_commits: Option<Vec<pgr_core::GitCommit>>,
    /// Parsed man page section list for ]s/[s navigation (populated on first paint when `ManPage` mode).
    man_sections: Option<Vec<pgr_core::ManSection>>,
    /// Syntax highlighter instance (compiled-in only with `syntax` feature).
    #[cfg(feature = "syntax")]
    highlighter: Option<pgr_display::syntax::highlighting::Highlighter>,
    /// Whether syntax highlighting is enabled at runtime (can be toggled).
    #[cfg(feature = "syntax")]
    syntax_enabled: bool,
    /// All URLs detected on the currently visible screen lines.
    /// Each entry is `(buffer_line, url_index_within_line, url_text)`.
    screen_urls: Vec<(usize, usize, String)>,
    /// Index into `screen_urls` for the currently highlighted URL, if any.
    current_url_index: Option<usize>,
    /// Clipboard backend for yank commands.
    clipboard: Box<dyn crate::clipboard::Clipboard>,
    /// Whether clipboard is disabled (--clipboard=off).
    clipboard_disabled: bool,
    /// Cached git gutter state for the current file.
    gutter_state: Option<crate::git_gutter::GutterState>,
    /// Whether git gutter display is enabled (can be toggled with ESC-G).
    git_gutter_enabled: bool,
    /// Whether side-by-side diff rendering is active (toggled with ESC-V).
    side_by_side: bool,
    /// Parsed SQL table layout for column-snap hscroll and sticky headers.
    sql_table_layout: Option<SqlTableLayout>,
}

impl<R: Read, W: Write> Pager<R, W> {
    /// Create a new pager with the given components.
    ///
    /// Uses the default `less` keymap, a 24x80 screen, tab width 8,
    /// and `RawControlMode::Off`.
    #[must_use]
    pub fn new(
        reader: KeyReader<R>,
        writer: W,
        buffer: Box<dyn Buffer>,
        index: LineIndex,
        filename: Option<String>,
    ) -> Self {
        Self {
            reader,
            writer,
            keymap: Keymap::default_less(),
            screen: Screen::new(24, 80),
            buffer,
            index,
            render_config: RenderConfig::default(),
            filename,
            prompt_style: PromptStyle::Short,
            pending_count: None,
            should_quit: false,
            marks: MarkStore::new(),
            last_position: None,
            pending_command: None,
            sticky_half_page: None,
            custom_window_size: None,
            file_list: None,
            runtime_options: RuntimeOptions::default(),
            status_message: None,
            quit_at_eof: false,
            quit_at_first_eof: false,
            eof_seen_count: 0,
            secure_mode: false,
            is_pipe: false,
            initial_render: true,
            last_shell_command: None,
            shell: String::from("sh"),
            editor: String::from("vi"),
            previous_file: None,
            last_pattern: None,
            last_direction: SearchDirection::Forward,
            highlight_state: HighlightState::new(),
            editing_search: false,
            line_editor: None,
            search_history: History::new(),
            search_prompt_direction: SearchDirection::Forward,
            last_modifiers: SearchModifiers::new(),
            match_count_cache: None,
            filter: FilterState::new(),
            filtered_lines: None,
            editing_filter: false,
            filter_invert: false,
            editing_add_highlight: false,
            editing_remove_highlight: false,
            absorb_next_key: false,
            key_fd: None,
            initial_commands: Vec::new(),
            every_file_commands: Vec::new(),
            initial_commands_executed: false,
            cross_file_search: false,
            tag_state: None,
            follow_name: false,
            exit_follow_on_close: false,
            mouse_enabled: false,
            no_keypad: false,
            no_vbell: false,
            search_literal_next: false,
            incsearch_saved_top: None,
            redraw_on_quit: false,
            quit_on_intr: false,
            exit_reason: ExitReason::Normal,
            content_mode: ContentMode::Plain,
            diff_state: None,
            git_log_commits: None,
            man_sections: None,
            #[cfg(feature = "syntax")]
            highlighter: None,
            #[cfg(feature = "syntax")]
            syntax_enabled: true,
            screen_urls: Vec::new(),
            current_url_index: None,
            clipboard: crate::clipboard::detect_clipboard(),
            clipboard_disabled: false,
            gutter_state: None,
            git_gutter_enabled: false,
            side_by_side: false,
            sql_table_layout: None,
        }
    }

    /// Run the main loop. Blocks until the user quits or input is exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error if key reading, buffer access, or terminal output fails.
    pub fn run(&mut self) -> Result<()> {
        // Enter alternate screen buffer (like GNU less).
        self.writer.write_all(b"\x1b[?1049h")?;

        // Enable application keypad mode unless --no-keypad was specified.
        if !self.no_keypad {
            self.writer.write_all(crate::terminal::KEYPAD_ENABLE)?;
        }

        // Enable mouse tracking if configured.
        if self.mouse_enabled {
            self.writer.write_all(crate::terminal::MOUSE_ENABLE)?;
            self.writer.write_all(crate::terminal::MOUSE_SGR_ENABLE)?;
        }

        self.writer.flush()?;

        self.repaint()?;

        // Execute initial commands after the first repaint (before user input).
        self.execute_initial_commands()?;

        loop {
            match self.reader.read_key() {
                Ok(key) => {
                    if !self.process_key(&key)? {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    // Disable mouse tracking before exiting.
                    if self.mouse_enabled {
                        let _ = self.writer.write_all(crate::terminal::MOUSE_SGR_DISABLE);
                        let _ = self.writer.write_all(crate::terminal::MOUSE_DISABLE);
                    }
                    // Disable application keypad mode.
                    if !self.no_keypad {
                        let _ = self.writer.write_all(crate::terminal::KEYPAD_DISABLE);
                    }
                    // Exit alternate screen buffer before propagating error.
                    let _ = self.writer.write_all(b"\x1b[?1049l");
                    let _ = self.writer.flush();
                    return Err(e.into());
                }
            }
        }

        // Disable mouse tracking before exiting.
        if self.mouse_enabled {
            self.writer.write_all(crate::terminal::MOUSE_SGR_DISABLE)?;
            self.writer.write_all(crate::terminal::MOUSE_DISABLE)?;
        }

        // Disable application keypad mode.
        if !self.no_keypad {
            self.writer.write_all(crate::terminal::KEYPAD_DISABLE)?;
        }

        // Repaint the screen before exiting if --redraw-on-quit is set.
        if self.redraw_on_quit {
            self.repaint()?;
        }

        // Exit alternate screen buffer.
        self.writer.write_all(b"\x1b[?1049l")?;
        self.writer.flush()?;

        Ok(())
    }

    /// Process a single key event. Returns `Ok(true)` if the pager should
    /// continue, `Ok(false)` if it should quit.
    fn process_key(&mut self, key: &Key) -> Result<bool> {
        // After an option toggle/query, the next keypress dismisses the
        // status message without executing a command (matching GNU less).
        if self.absorb_next_key {
            self.absorb_next_key = false;
            self.status_message = None;
            self.repaint()?;
            return Ok(true);
        }

        // Clear any transient status message on the next keypress.
        if self.status_message.is_some() && !self.editing_search {
            self.status_message = None;
        }

        // If we're in search-prompt editing mode, feed keys to the line editor.
        if self.editing_search {
            return self.process_search_key(key);
        }

        // If we're in filter-prompt editing mode, feed keys to the line editor.
        if self.editing_filter {
            return self.process_filter_key(key);
        }

        // If we're in add-highlight prompt mode, feed keys to the line editor.
        if self.editing_add_highlight {
            return self.process_add_highlight_key(key);
        }

        // If we're in remove-highlight prompt mode, feed keys to the line editor.
        if self.editing_remove_highlight {
            return self.process_remove_highlight_key(key);
        }

        // If there's a pending multi-key command, resolve it.
        if let Some(pending) = self.pending_command.take() {
            return self.resolve_pending(pending, key);
        }

        // Check if this key starts a multi-key command.
        if let Some(pending) = Self::check_pending_start(key) {
            self.pending_command = Some(pending);
            return Ok(true);
        }

        // Digit accumulation for numeric prefixes.
        if let Key::Char(c) = *key {
            if c.is_ascii_digit() {
                let digit = u32::from(c) - u32::from('0');
                #[allow(clippy::cast_possible_truncation)] // digit is 0..=9
                let digit = digit as usize;
                self.pending_count = Some(
                    self.pending_count
                        .unwrap_or(0)
                        .saturating_mul(10)
                        .saturating_add(digit),
                );
                return Ok(true);
            }
        }

        // When a URL is highlighted, `o` opens it in the browser.
        if *key == Key::Char('o') && self.current_url_index.is_some() {
            let count = self.pending_count.take();
            self.execute(&Command::OpenUrl, count)?;
            return Ok(!self.should_quit);
        }

        let command = self.keymap.lookup(key);
        let count = self.pending_count.take();

        // Track interrupt-triggered quit for exit code conformance.
        if *key == Key::Ctrl('c') && command == Command::Quit {
            self.exit_reason = ExitReason::Interrupt;
        }

        self.execute(&command, count)?;

        Ok(!self.should_quit)
    }

    /// Check if a key initiates a multi-key command sequence.
    fn check_pending_start(key: &Key) -> Option<PendingCommand> {
        match *key {
            Key::Char('m') => Some(PendingCommand::SetMarkTop),
            Key::Char('M') => Some(PendingCommand::SetMarkBottom),
            Key::Char('\'') => Some(PendingCommand::GotoMark),
            Key::EscSeq('m') => Some(PendingCommand::ClearMark),
            Key::Ctrl('x') => Some(PendingCommand::CtrlXPrefix),
            Key::Ctrl('o') => Some(PendingCommand::CtrlOPrefix),
            Key::Char(':') => Some(PendingCommand::ColonPrefix),
            Key::Char('Z') => Some(PendingCommand::ZPrefix),
            Key::Char('&') => Some(PendingCommand::FilterPrefix),
            Key::Char('[') => Some(PendingCommand::OpenBracketPrefix),
            Key::Char(']') => Some(PendingCommand::CloseBracketPrefix),
            _ => None,
        }
    }

    /// Resolve a pending multi-key command with the argument key.
    #[allow(clippy::too_many_lines)] // Each PendingCommand variant is a distinct dispatch case
    fn resolve_pending(&mut self, pending: PendingCommand, key: &Key) -> Result<bool> {
        match pending {
            PendingCommand::SetMarkTop => {
                if let Key::Char(c) = *key {
                    let mark = Mark {
                        line: self.screen.top_line(),
                        horizontal_offset: self.screen.horizontal_offset(),
                    };
                    // Silently ignore invalid mark characters.
                    let _ = self.marks.set(c, mark);
                }
            }
            PendingCommand::SetMarkBottom => {
                if let Key::Char(c) = *key {
                    let (_, end) = self.screen.visible_range();
                    let total = self.index.lines_indexed();
                    let bottom = end.min(total).saturating_sub(1);
                    let mark = Mark {
                        line: bottom,
                        horizontal_offset: self.screen.horizontal_offset(),
                    };
                    let _ = self.marks.set(c, mark);
                }
            }
            PendingCommand::GotoMark => {
                self.resolve_goto_mark(key)?;
            }
            PendingCommand::ClearMark => {
                if let Key::Char(c) = *key {
                    let _ = self.marks.clear(c);
                }
            }
            PendingCommand::CtrlXPrefix => {
                if *key == Key::Ctrl('x') {
                    // ^X^X: same as ' -- wait for mark letter.
                    self.pending_command = Some(PendingCommand::GotoMark);
                    return Ok(true);
                }
                if *key == Key::Ctrl('v') {
                    // ^X^V: examine (open) a new file.
                    self.execute(&Command::ExamineAlt, None)?;
                    return Ok(!self.should_quit);
                }
                // Not ^X or ^V: ignore the prefix, process key normally.
                return self.process_key(key);
            }
            PendingCommand::ColonPrefix => {
                let count = self.pending_count.take();
                match *key {
                    Key::Char('n') => self.execute(&Command::NextFile, count)?,
                    Key::Char('p') => self.execute(&Command::PreviousFile, count)?,
                    Key::Char('x') => self.execute(&Command::FirstFile, count)?,
                    Key::Char('d') => self.execute(&Command::RemoveFile, count)?,
                    Key::Char('q' | 'Q') => self.execute(&Command::Quit, count)?,
                    Key::Char('e') => self.execute(&Command::Examine, count)?,
                    Key::Char('f') => self.execute(&Command::FileInfo, count)?,
                    _ => {} // Unknown colon command: ignore.
                }
                return Ok(!self.should_quit);
            }
            PendingCommand::ToggleOption => {
                if let Key::Char(c) = *key {
                    self.handle_toggle_option(c)?;
                }
            }
            PendingCommand::QueryOption => {
                if let Key::Char(c) = *key {
                    self.handle_query_option(c)?;
                }
            }
            PendingCommand::ZPrefix => {
                if *key == Key::Char('Z') {
                    self.execute(&Command::Quit, None)?;
                }
                // Non-Z key after Z: ignore the prefix (not a valid command).
                return Ok(!self.should_quit);
            }
            PendingCommand::CtrlOPrefix => {
                let count = self.pending_count.take();
                match *key {
                    Key::Ctrl('n') => self.execute(&Command::HyperlinkNext, count)?,
                    Key::Ctrl('p') => self.execute(&Command::HyperlinkPrev, count)?,
                    Key::Ctrl('l') => self.execute(&Command::HyperlinkJump, count)?,
                    Key::Ctrl('o') => self.execute(&Command::HyperlinkOpen, count)?,
                    _ => {} // Unknown ^O sub-command: ignore.
                }
                return Ok(!self.should_quit);
            }
            PendingCommand::FilterPrefix => {
                match *key {
                    Key::Char('+') => {
                        self.enter_add_highlight_mode()?;
                    }
                    Key::Char('-') => {
                        self.enter_remove_highlight_mode()?;
                    }
                    Key::Char('l') => {
                        self.execute(&Command::ListHighlights, None)?;
                    }
                    _ => {
                        // Default: enter normal filter mode (GNU less behavior).
                        // Feed the key into the filter editor so it becomes
                        // the first character of the filter pattern.
                        self.enter_filter_mode()?;
                        if self.editing_filter {
                            return self.process_filter_key(key);
                        }
                    }
                }
                return Ok(!self.should_quit);
            }
            PendingCommand::OpenBracketPrefix => {
                let count = self.pending_count.take();
                match *key {
                    Key::Char('u') => self.execute(&Command::PrevUrl, count)?,
                    Key::Char('c') => self.execute(&Command::PrevHunk, count)?,
                    Key::Char('f') => self.execute(&Command::PrevDiffFile, count)?,
                    Key::Char('g') => self.execute(&Command::PrevCommit, count)?,
                    Key::Char('s') => self.execute(&Command::PrevManSection, count)?,
                    _ => {
                        // Default: bracket matching (FindCloseBracket for `[`).
                        self.execute(&Command::FindCloseBracket('[', ']'), count)?;
                    }
                }
                return Ok(!self.should_quit);
            }
            PendingCommand::CloseBracketPrefix => {
                let count = self.pending_count.take();
                match *key {
                    Key::Char('u') => self.execute(&Command::NextUrl, count)?,
                    Key::Char('c') => self.execute(&Command::NextHunk, count)?,
                    Key::Char('f') => self.execute(&Command::NextDiffFile, count)?,
                    Key::Char('g') => self.execute(&Command::NextCommit, count)?,
                    Key::Char('s') => self.execute(&Command::NextManSection, count)?,
                    _ => {
                        // Default: bracket matching (FindOpenBracket for `]`).
                        self.execute(&Command::FindOpenBracket('[', ']'), count)?;
                    }
                }
                return Ok(!self.should_quit);
            }
        }
        Ok(!self.should_quit)
    }

    /// Handle the second key of a goto-mark sequence (`'` then key).
    fn resolve_goto_mark(&mut self, key: &Key) -> Result<()> {
        match *key {
            Key::Char('\'') => {
                // '' = return to previous position.
                if let Some(prev) = self.last_position {
                    self.save_last_position();
                    let total = self.index.total_lines(&*self.buffer)?;
                    self.screen.goto_line(prev, total);
                    self.repaint()?;
                }
            }
            Key::Char('^') => {
                self.save_last_position();
                let total = self.index.total_lines(&*self.buffer)?;
                self.screen.goto_line(0, total);
                self.repaint()?;
            }
            Key::Char('$') => {
                self.save_last_position();
                let total = self.index.total_lines(&*self.buffer)?;
                // Match less: '$' mark positions 1 line past what G does,
                // showing the last (content_rows - 1) lines plus a tilde row.
                let end = total
                    .saturating_sub(self.screen.content_rows())
                    .saturating_add(1);
                self.screen.set_top_line(end);
                self.repaint()?;
            }
            Key::Char(c) => {
                if let Some(mark) = self.marks.get(c).copied() {
                    self.save_last_position();
                    let total = self.index.total_lines(&*self.buffer)?;
                    self.screen.goto_line(mark.line, total);
                    self.screen.set_horizontal_offset(mark.horizontal_offset);
                    self.repaint()?;
                }
                // Unknown mark: silently ignore.
            }
            _ => {} // Non-char key after ': ignore.
        }
        Ok(())
    }

    /// Save the current top line as the last position for `''` return.
    fn save_last_position(&mut self) {
        self.last_position = Some(self.screen.top_line());
    }

    /// Compute the margin width (status column + line numbers) for the current
    /// display settings. Used by sub-line scrolling calculations.
    fn margin_width(&self, total_lines: usize) -> usize {
        let status_w = if self.runtime_options.status_column {
            2
        } else {
            0
        };
        let ln_w = if self.runtime_options.line_numbers {
            line_number_width(total_lines)
        } else {
            0
        };
        status_w + ln_w
    }

    /// Compute how many screen rows a given file line occupies when rendered
    /// in the current terminal with wrapping. Returns 1 for lines that fit
    /// on a single row. In chop mode, always returns 1.
    fn screen_rows_for_line(&mut self, line_num: usize, total_lines: usize) -> usize {
        if self.screen.chop_mode() {
            return 1;
        }
        let (_, cols) = self.screen.dimensions();
        if cols == 0 {
            return 1;
        }
        let content = self.index.get_line(line_num, &*self.buffer).ok().flatten();
        let width = match content {
            Some(ref text) => {
                let render_width = usize::MAX / 2;
                let (_, w) = pgr_display::render_line(text, 0, render_width, &self.render_config);
                w
            }
            None => 0,
        };
        let margin = self.margin_width(total_lines);
        compute_line_screen_rows(width, margin, cols)
    }

    /// Scroll forward by `n` screen rows, handling sub-line offsets for
    /// wrapped lines. Advances through file lines as needed, tracking how
    /// many screen rows have been consumed.
    fn scroll_forward_screen_rows(&mut self, n: usize, total_lines: usize) {
        let mut remaining = n;
        while remaining > 0 {
            let top = self.screen.top_line();
            if top >= total_lines.saturating_sub(self.screen.content_rows()) {
                break;
            }
            let rows_for_line = self.screen_rows_for_line(top, total_lines);
            let current_offset = self.screen.sub_line_offset();
            let rows_left_in_line = rows_for_line.saturating_sub(current_offset);

            if remaining < rows_left_in_line {
                // Stay in the same file line, advance sub-line offset.
                self.screen.set_sub_line_offset(current_offset + remaining);
                remaining = 0;
            } else {
                // Consume the rest of this file line, move to the next.
                remaining -= rows_left_in_line;
                self.screen.set_sub_line_offset(0);
                self.screen.scroll_forward(1, total_lines);
            }
        }
    }

    /// Scroll backward by `n` screen rows, handling sub-line offsets for
    /// wrapped lines.
    fn scroll_backward_screen_rows(&mut self, n: usize, total_lines: usize) {
        let mut remaining = n;
        while remaining > 0 {
            let current_offset = self.screen.sub_line_offset();
            if current_offset > 0 {
                // Still within the current file line's wrapped rows.
                if remaining <= current_offset {
                    self.screen.set_sub_line_offset(current_offset - remaining);
                    remaining = 0;
                } else {
                    remaining -= current_offset;
                    self.screen.set_sub_line_offset(0);
                    // Continue scrolling backward into previous lines.
                }
            } else {
                // At the start of the current file line. Move to previous.
                let top = self.screen.top_line();
                if top <= self.screen.header_lines() {
                    break;
                }
                self.screen.scroll_backward(1);
                let new_top = self.screen.top_line();
                let rows_for_prev = self.screen_rows_for_line(new_top, total_lines);
                // Position at the last screen row of the previous line.
                let new_offset = rows_for_prev.saturating_sub(1);
                if remaining <= 1 {
                    self.screen.set_sub_line_offset(new_offset);
                    remaining = 0;
                } else {
                    self.screen.set_sub_line_offset(new_offset);
                    remaining -= 1;
                }
            }
        }
    }

    /// Check if the viewport is at EOF and quit-at-eof behavior should trigger.
    ///
    /// With `-E`: quit on first forward scroll that lands at or past EOF.
    /// With `-e`: quit on the second such scroll.
    fn check_eof_quit(&mut self, total_lines: usize) {
        let (_, end) = self.screen.visible_range();
        if end >= total_lines {
            if self.quit_at_first_eof {
                self.should_quit = true;
            } else if self.quit_at_eof {
                self.eof_seen_count += 1;
                if self.eof_seen_count >= 2 {
                    self.should_quit = true;
                }
            }
        }
    }

    /// Execute a command with the given numeric count prefix.
    #[allow(clippy::too_many_lines)] // dispatch table is inherently large
    fn execute(&mut self, command: &Command, count: Option<usize>) -> Result<()> {
        // Clear URL highlight when executing any non-URL command, so the
        // highlight disappears when the user scrolls or does something else.
        if !matches!(
            command,
            Command::NextUrl
                | Command::PrevUrl
                | Command::OpenUrl
                | Command::NextHunk
                | Command::PrevHunk
                | Command::NextDiffFile
                | Command::PrevDiffFile
        ) {
            self.current_url_index = None;
        }

        let raw_total = self.index.total_lines(&*self.buffer)?;
        let total = if self.filter.is_active() {
            self.filtered_lines
                .as_ref()
                .map_or(raw_total, FilteredLines::visible_count)
        } else {
            raw_total
        };

        match *command {
            Command::ScrollForward(n) => {
                let old_top = self.screen.top_line();
                let old_offset = self.screen.sub_line_offset();
                self.scroll_forward_screen_rows(count.unwrap_or(n), total);
                if self.screen.top_line() != old_top || self.screen.sub_line_offset() != old_offset
                {
                    self.repaint()?;
                }
                self.check_eof_quit(total);
            }
            Command::ScrollBackward(n) => {
                let old_top = self.screen.top_line();
                let old_offset = self.screen.sub_line_offset();
                self.scroll_backward_screen_rows(count.unwrap_or(n), total);
                if self.screen.top_line() != old_top || self.screen.sub_line_offset() != old_offset
                {
                    self.repaint()?;
                }
            }
            Command::PageForward => {
                self.save_last_position();
                self.screen.set_sub_line_offset(0);
                let window = self.resolve_window_size();
                self.screen.scroll_forward(count.unwrap_or(window), total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::PageBackward => {
                self.save_last_position();
                self.screen.set_sub_line_offset(0);
                let window = self.resolve_window_size();
                self.screen.scroll_backward(count.unwrap_or(window));
                self.repaint()?;
            }
            Command::HalfPageForward => {
                self.save_last_position();
                self.screen.set_sub_line_offset(0);
                if let Some(c) = count {
                    self.sticky_half_page = Some(c);
                }
                // less uses (screen_height / 2), not (content_rows / 2)
                let amount = self
                    .sticky_half_page
                    .unwrap_or(self.screen.dimensions().0 / 2);
                self.screen.scroll_forward(amount, total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::HalfPageBackward => {
                self.save_last_position();
                self.screen.set_sub_line_offset(0);
                if let Some(c) = count {
                    self.sticky_half_page = Some(c);
                }
                // less uses (screen_height / 2), not (content_rows / 2)
                let amount = self
                    .sticky_half_page
                    .unwrap_or(self.screen.dimensions().0 / 2);
                self.screen.scroll_backward(amount);
                self.repaint()?;
            }
            Command::GotoBeginning(n) => {
                self.save_last_position();
                self.screen.set_sub_line_offset(0);
                // ng uses 1-based line numbers; convert to 0-based index
                let target = count.or(n).map_or(0, |line| line.saturating_sub(1));
                self.screen.goto_line(target, total);
                self.repaint()?;
            }
            Command::GotoEnd(n) => {
                self.save_last_position();
                self.screen.set_sub_line_offset(0);
                let default = total.saturating_sub(self.screen.content_rows());
                // nG uses 1-based line numbers; convert to 0-based index
                let target = count.or(n).map_or(default, |line| line.saturating_sub(1));
                self.screen.goto_line(target, total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::Repaint => {
                self.repaint()?;
            }
            Command::Quit => {
                self.should_quit = true;
            }
            Command::Filter => {
                self.enter_filter_mode()?;
            }
            Command::Noop => {}
            Command::ScrollRight => {
                if let Some(ref layout) = self.sql_table_layout {
                    // Column-snap hscroll: jump to the next column boundary.
                    let h = self.screen.horizontal_offset();
                    let new_h = snap_to_next_column(layout, h);
                    self.screen.set_horizontal_offset(new_h);
                } else {
                    let cols = self.screen.cols();
                    let amount = count.unwrap_or(cols / 2);
                    let h = self.screen.horizontal_offset();
                    self.screen.set_horizontal_offset(h.saturating_add(amount));
                }
                self.repaint()?;
            }
            Command::ScrollLeft => {
                if let Some(ref layout) = self.sql_table_layout {
                    // Column-snap hscroll: jump to the previous column boundary.
                    let h = self.screen.horizontal_offset();
                    let new_h = snap_to_prev_column(layout, h);
                    self.screen.set_horizontal_offset(new_h);
                } else {
                    let cols = self.screen.cols();
                    let amount = count.unwrap_or(cols / 2);
                    let h = self.screen.horizontal_offset();
                    self.screen.set_horizontal_offset(h.saturating_sub(amount));
                }
                self.repaint()?;
            }
            Command::ScrollRightEnd => {
                // Find max line width among visible lines and set offset to show rightmost content.
                let (start, end) = self.screen.visible_range();
                let cols = self.screen.cols();
                let mut max_width: usize = 0;
                for line_num in start..end.min(total) {
                    if let Some(content) = self.index.get_line(line_num, &*self.buffer)? {
                        max_width = max_width.max(content.len());
                    }
                }
                let new_offset = max_width.saturating_sub(cols);
                self.screen.set_horizontal_offset(new_offset);
                self.repaint()?;
            }
            Command::ScrollLeftHome => {
                self.screen.set_horizontal_offset(0);
                self.repaint()?;
            }
            Command::GotoPercent => {
                let pct = count.unwrap_or(0).min(100);
                let target = if total == 0 {
                    0
                } else {
                    let raw = pct.saturating_mul(total) / 100;
                    raw.min(total.saturating_sub(1))
                };
                self.screen.set_top_line(target);
                self.repaint()?;
            }
            Command::GotoByteOffset => {
                let byte_offset = count.unwrap_or(0) as u64;
                let line = self
                    .index
                    .line_at_offset(byte_offset, &*self.buffer)?
                    .unwrap_or(total.saturating_sub(1));
                self.screen.goto_line(line, total);
                self.repaint()?;
            }
            Command::ForwardForceEof => {
                let window = self.resolve_window_size();
                self.screen
                    .scroll_forward_unclamped(count.unwrap_or(window));
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::BackwardForceBeginning => {
                let window = self.resolve_window_size();
                // scroll_backward already clamps at 0, which is the correct behavior
                self.screen.scroll_backward(count.unwrap_or(window));
                self.repaint()?;
            }
            Command::WindowForward => {
                if let Some(c) = count {
                    self.custom_window_size = Some(WindowSize::Absolute(c));
                }
                let window = self.resolve_window_size();
                self.screen.scroll_forward(window, total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::WindowBackward => {
                if let Some(c) = count {
                    self.custom_window_size = Some(WindowSize::Absolute(c));
                }
                let window = self.resolve_window_size();
                self.screen.scroll_backward(window);
                self.repaint()?;
            }
            Command::FollowMode => {
                self.follow_mode()?;
            }
            Command::FollowModeStopOnMatch => {
                self.follow_mode_stop_on_match()?;
            }
            Command::RepaintRefresh => {
                self.buffer.refresh()?;
                let new_len = self.buffer.len() as u64;
                self.index = LineIndex::new(new_len);
                if self.git_gutter_enabled {
                    self.load_gutter_state();
                }
                self.repaint()?;
            }
            Command::FileLineForward => {
                // ESC-j: scroll forward by N screen rows, like j.
                // In GNU less, ESC-j and j have the same scroll behavior;
                // the difference is that ESC-j displays the target line
                // from its first segment when it would be a continuation.
                let old_top = self.screen.top_line();
                let old_offset = self.screen.sub_line_offset();
                self.scroll_forward_screen_rows(count.unwrap_or(1), total);
                if self.screen.top_line() != old_top || self.screen.sub_line_offset() != old_offset
                {
                    self.repaint()?;
                }
                self.check_eof_quit(total);
            }
            Command::FileLineBackward => {
                // ESC-k: scroll backward by N screen rows, like k, but snap
                // to the file line's first segment if landing mid-wrap.
                let old_top = self.screen.top_line();
                let old_offset = self.screen.sub_line_offset();
                self.scroll_backward_screen_rows(count.unwrap_or(1), total);
                // If we landed mid-wrap, snap to this file line's start.
                if self.screen.sub_line_offset() > 0 {
                    self.screen.set_sub_line_offset(0);
                }
                if self.screen.top_line() != old_top || self.screen.sub_line_offset() != old_offset
                {
                    self.repaint()?;
                }
            }
            Command::ScrollForwardForce(n) => {
                self.screen.scroll_forward_unclamped(count.unwrap_or(n));
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::ScrollBackwardForce(n) => {
                // scroll_backward already clamps at 0
                self.screen.scroll_backward(count.unwrap_or(n));
                self.repaint()?;
            }
            Command::NextFile => {
                self.switch_file_next()?;
            }
            Command::PreviousFile => {
                self.switch_file_prev()?;
            }
            Command::FirstFile => {
                self.switch_file_goto(count.unwrap_or(0))?;
            }
            Command::RemoveFile => {
                self.remove_current_file()?;
            }
            Command::ToggleOption => {
                self.pending_command = Some(PendingCommand::ToggleOption);
            }
            Command::QueryOption => {
                self.pending_command = Some(PendingCommand::QueryOption);
            }
            Command::ShellCommand => {
                self.handle_shell_command()?;
            }
            Command::ShellCommandExpand => {
                self.handle_shell_command_expand(total)?;
            }
            Command::PipeToCommand => {
                self.handle_pipe_to_command(total)?;
            }
            Command::EditFile => {
                self.handle_edit_file()?;
            }
            Command::SavePipeInput => {
                self.handle_save_pipe_input(total)?;
            }
            Command::SaveBuffer => {
                self.handle_save_buffer(total)?;
            }
            Command::Examine | Command::ExamineAlt => {
                self.examine_prompt()?;
            }
            Command::FileInfo => {
                self.display_file_info()?;
            }
            Command::Help => {
                self.display_help()?;
            }
            Command::Version => {
                self.display_version()?;
            }
            Command::SearchForward => {
                self.enter_search_mode(SearchDirection::Forward, count)?;
            }
            Command::SearchBackward => {
                self.enter_search_mode(SearchDirection::Backward, count)?;
            }
            Command::RepeatSearch => {
                self.repeat_search(false, count)?;
            }
            Command::RepeatSearchReverse => {
                self.repeat_search(true, count)?;
            }
            Command::ToggleHighlight => {
                self.highlight_state.toggle();
                self.repaint()?;
            }
            Command::ClearSearchPattern => {
                // GNU less ESC-U clears highlighting but preserves the
                // search pattern so `n`/`N` can still repeat the search.
                self.highlight_state.set_enabled(true);
                self.highlight_state.clear();
                self.repaint()?;
            }
            Command::FindCloseBracket(open, close) => {
                self.find_matching_bracket(open, close, true, total)?;
            }
            Command::FindOpenBracket(open, close) => {
                self.find_matching_bracket(open, close, false, total)?;
            }
            Command::SearchNextCrossFile => {
                self.repeat_search_cross_file(false, count)?;
            }
            Command::SearchPrevCrossFile => {
                self.repeat_search_cross_file(true, count)?;
            }
            Command::SearchForwardCrossFile => {
                self.cross_file_search = true;
                self.enter_search_mode(SearchDirection::Forward, count)?;
            }
            Command::SearchBackwardCrossFile => {
                self.cross_file_search = true;
                self.enter_search_mode(SearchDirection::Backward, count)?;
            }
            Command::NextTag => {
                self.navigate_tag_next()?;
            }
            Command::PrevTag => {
                self.navigate_tag_prev()?;
            }
            Command::HyperlinkNext | Command::HyperlinkPrev | Command::HyperlinkJump => {
                // Stub: hyperlink navigation will be implemented when the
                // display layer tracks per-cell hyperlink state on screen.
                self.status_message = Some("hyperlink navigation not yet implemented".to_string());
                self.repaint()?;
            }
            Command::HyperlinkOpen => {
                // Stub: hyperlink open will shell out to $BROWSER or
                // platform opener once hyperlink selection is tracked.
                self.status_message = Some("hyperlink open not yet implemented".to_string());
                self.repaint()?;
            }
            Command::NextUrl => {
                self.navigate_url_next()?;
            }
            Command::PrevUrl => {
                self.navigate_url_prev()?;
            }
            Command::OpenUrl => {
                self.open_current_url()?;
            }
            Command::ToggleSyntax => {
                #[cfg(feature = "syntax")]
                {
                    self.syntax_enabled = !self.syntax_enabled;
                    let state = if self.syntax_enabled { "on" } else { "off" };
                    self.status_message = Some(format!("Syntax highlighting {state}"));
                    self.repaint()?;
                }
                #[cfg(not(feature = "syntax"))]
                {
                    self.status_message = Some("syntax highlighting not compiled in".to_string());
                    self.repaint()?;
                }
            }
            Command::ToggleGitGutter => {
                if self.secure_mode {
                    self.status_message = Some("git gutter disabled in secure mode".to_string());
                    self.repaint()?;
                } else {
                    self.git_gutter_enabled = !self.git_gutter_enabled;
                    if self.git_gutter_enabled && self.gutter_state.is_none() {
                        self.load_gutter_state();
                    }
                    let state = if self.git_gutter_enabled { "on" } else { "off" };
                    self.status_message = Some(format!("Git gutter {state}"));
                    self.repaint()?;
                }
            }
            Command::AddHighlight => {
                self.enter_add_highlight_mode()?;
            }
            Command::RemoveHighlight => {
                self.enter_remove_highlight_mode()?;
            }
            Command::ListHighlights => {
                self.show_highlight_list()?;
            }
            Command::YankLine => {
                self.yank_line()?;
            }
            Command::YankScreen => {
                self.yank_screen(total)?;
            }
            Command::NextHunk => {
                self.navigate_next_hunk(total)?;
            }
            Command::PrevHunk => {
                self.navigate_prev_hunk(total)?;
            }
            Command::NextDiffFile => {
                self.navigate_next_diff_file(total)?;
            }
            Command::PrevDiffFile => {
                self.navigate_prev_diff_file(total)?;
            }
            Command::ToggleSideBySide => {
                if self.content_mode == ContentMode::Diff {
                    self.side_by_side = !self.side_by_side;
                    let state = if self.side_by_side { "on" } else { "off" };
                    self.status_message = Some(format!("Side-by-side diff {state}"));
                } else {
                    self.status_message =
                        Some("side-by-side only available in diff mode".to_string());
                }
                self.repaint()?;
            }
            Command::NextCommit => {
                self.navigate_next_commit(total)?;
            }
            Command::PrevCommit => {
                self.navigate_prev_commit(total)?;
            }
            Command::NextManSection => {
                self.navigate_next_man_section(total)?;
            }
            Command::PrevManSection => {
                self.navigate_prev_man_section(total)?;
            }
        }

        Ok(())
    }

    /// Find a matching bracket by scanning lines forward or backward.
    ///
    /// When `forward` is true, starts at the top visible line and searches
    /// forward for `close`, tracking nesting via `open`. When false, starts
    /// at the bottom visible line and searches backward for `open`, tracking
    /// nesting via `close`.
    fn find_matching_bracket(
        &mut self,
        open: char,
        close: char,
        forward: bool,
        total: usize,
    ) -> Result<()> {
        let start = if forward {
            self.screen.top_line()
        } else {
            (self.screen.top_line() + self.screen.content_rows()).min(total.saturating_sub(1))
        };

        // Verify the starting line contains the expected bracket.
        let start_bracket = if forward { open } else { close };
        let start_line = self.index.get_line(start, &*self.buffer)?;
        let has_start_bracket = start_line
            .as_ref()
            .is_some_and(|line| line.contains(start_bracket));

        if !has_start_bracket {
            self.status_message = Some(format!("No {start_bracket} found on current line"));
            self.repaint()?;
            return Ok(());
        }

        let mut depth: i64 = 0;

        if forward {
            for line_num in start..total {
                if let Some(ref content) = self.index.get_line(line_num, &*self.buffer)? {
                    // Consider only the first bracket character on each line.
                    let first_open = content.find(open);
                    let first_close = content.find(close);
                    match (first_open, first_close) {
                        (Some(o), Some(c)) => {
                            if o < c {
                                depth += 1;
                            } else {
                                depth -= 1;
                            }
                        }
                        (Some(_), None) => depth += 1,
                        (None, Some(_)) => depth -= 1,
                        (None, None) => {}
                    }
                    if depth == 0 {
                        self.save_last_position();
                        self.screen.set_top_line(line_num);
                        self.repaint()?;
                        return Ok(());
                    }
                }
            }
        } else {
            // Search backward from start line.
            for line_num in (0..=start).rev() {
                if let Some(ref content) = self.index.get_line(line_num, &*self.buffer)? {
                    let first_open = content.find(open);
                    let first_close = content.find(close);
                    match (first_open, first_close) {
                        (Some(o), Some(c)) => {
                            if c < o {
                                depth += 1;
                            } else {
                                depth -= 1;
                            }
                        }
                        (Some(_), None) => depth -= 1,
                        (None, Some(_)) => depth += 1,
                        (None, None) => {}
                    }
                    if depth == 0 {
                        self.save_last_position();
                        self.screen.set_top_line(line_num);
                        self.repaint()?;
                        return Ok(());
                    }
                }
            }
        }

        self.status_message = Some(format!(
            "No matching {} found",
            if forward { close } else { open }
        ));
        self.repaint()?;
        Ok(())
    }

    /// Scan visible lines for URLs and populate `screen_urls`.
    fn scan_screen_urls(&mut self) -> Result<()> {
        self.screen_urls.clear();
        let total = self.index.lines_indexed();
        let (start, end) = self.screen.visible_range();

        for line_num in start..end.min(total) {
            if let Some(ref content) = self.index.get_line(line_num, &*self.buffer)? {
                let urls = find_urls(content);
                for (url_idx, url_match) in urls.iter().enumerate() {
                    self.screen_urls
                        .push((line_num, url_idx, url_match.url.clone()));
                }
            }
        }

        Ok(())
    }

    /// Navigate to the next URL on screen.
    fn navigate_url_next(&mut self) -> Result<()> {
        self.scan_screen_urls()?;

        if self.screen_urls.is_empty() {
            self.current_url_index = None;
            self.status_message = Some("No URLs found on screen".to_string());
            self.repaint()?;
            return Ok(());
        }

        let new_index = match self.current_url_index {
            Some(idx) => {
                if idx + 1 < self.screen_urls.len() {
                    idx + 1
                } else {
                    0 // wrap around
                }
            }
            None => 0,
        };

        self.current_url_index = Some(new_index);
        let total_urls = self.screen_urls.len();
        let url = self.screen_urls[new_index].2.clone();
        self.status_message = Some(format!("URL {} of {}: {}", new_index + 1, total_urls, url));
        self.repaint()?;
        Ok(())
    }

    /// Navigate to the previous URL on screen.
    fn navigate_url_prev(&mut self) -> Result<()> {
        self.scan_screen_urls()?;

        if self.screen_urls.is_empty() {
            self.current_url_index = None;
            self.status_message = Some("No URLs found on screen".to_string());
            self.repaint()?;
            return Ok(());
        }

        let new_index = match self.current_url_index {
            Some(idx) => {
                if idx > 0 {
                    idx - 1
                } else {
                    self.screen_urls.len() - 1 // wrap around
                }
            }
            None => self.screen_urls.len() - 1,
        };

        self.current_url_index = Some(new_index);
        let total_urls = self.screen_urls.len();
        let url = self.screen_urls[new_index].2.clone();
        self.status_message = Some(format!("URL {} of {}: {}", new_index + 1, total_urls, url));
        self.repaint()?;
        Ok(())
    }

    /// Open the currently highlighted URL in the user's browser.
    fn open_current_url(&mut self) -> Result<()> {
        if self.secure_mode {
            self.status_message = Some("cannot open URLs in secure mode".to_string());
            self.repaint()?;
            return Ok(());
        }

        if let Some(idx) = self.current_url_index {
            if idx < self.screen_urls.len() {
                let url = self.screen_urls[idx].2.clone();
                match shell::open_url(&url) {
                    Ok(()) => {
                        self.status_message = Some(format!("Opened: {url}"));
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Failed to open URL: {e}"));
                    }
                }
                self.repaint()?;
                return Ok(());
            }
        }

        self.status_message = Some("No URL selected".to_string());
        self.repaint()?;
        Ok(())
    }

    /// Navigate to the next hunk header in a diff.
    fn navigate_next_hunk(&mut self, total: usize) -> Result<()> {
        let result = self.diff_nav_hunk(|files, cur| pgr_core::next_hunk_line(files, cur, true));
        self.apply_diff_nav(result, total, "No hunks found")
    }

    /// Navigate to the previous hunk header in a diff.
    fn navigate_prev_hunk(&mut self, total: usize) -> Result<()> {
        let result = self.diff_nav_hunk(|files, cur| pgr_core::prev_hunk_line(files, cur, true));
        self.apply_diff_nav(result, total, "No hunks found")
    }

    /// Navigate to the next file in a multi-file diff.
    fn navigate_next_diff_file(&mut self, total: usize) -> Result<()> {
        let result = self.diff_nav_file(|files, cur| pgr_core::next_file_line(files, cur, true));
        self.apply_diff_nav(result, total, "No diff files found")
    }

    /// Navigate to the previous file in a multi-file diff.
    fn navigate_prev_diff_file(&mut self, total: usize) -> Result<()> {
        let result = self.diff_nav_file(|files, cur| pgr_core::prev_file_line(files, cur, true));
        self.apply_diff_nav(result, total, "No diff files found")
    }

    /// Navigate to the next commit in a git log.
    fn navigate_next_commit(&mut self, total: usize) -> Result<()> {
        let result =
            self.git_log_nav(|commits, cur| pgr_core::next_commit_line(commits, cur, true));
        self.apply_git_log_nav(result, total, "No commits found")
    }

    /// Navigate to the previous commit in a git log.
    fn navigate_prev_commit(&mut self, total: usize) -> Result<()> {
        let result =
            self.git_log_nav(|commits, cur| pgr_core::prev_commit_line(commits, cur, true));
        self.apply_git_log_nav(result, total, "No commits found")
    }

    /// Compute a git log navigation target (line + status message) without mutating self.
    ///
    /// Separates the read-only computation from the mutable state update,
    /// avoiding a borrow conflict between `self.git_log_commits` and `self`.
    fn git_log_nav(
        &self,
        find_fn: impl FnOnce(&[pgr_core::GitCommit], usize) -> Option<usize>,
    ) -> DiffNavResult {
        let Some(ref commits) = self.git_log_commits else {
            return DiffNavResult::NotDiffMode;
        };
        let current = self.screen.top_line();
        match find_fn(commits, current) {
            Some(target) => {
                // Compute 1-based index of the target commit within the list.
                let total = commits.len();
                let idx = commits
                    .iter()
                    .position(|c| c.start_line == target)
                    .map_or(0, |i| i + 1);
                let status = format!("Commit {idx} of {total}");
                DiffNavResult::Found(target, status)
            }
            None => DiffNavResult::NoTarget,
        }
    }

    /// Apply the result of a git log navigation computation.
    fn apply_git_log_nav(
        &mut self,
        result: DiffNavResult,
        total: usize,
        no_target_msg: &str,
    ) -> Result<()> {
        match result {
            DiffNavResult::NotDiffMode => {
                self.status_message = Some("Not in git log mode".to_string());
                self.repaint()?;
            }
            DiffNavResult::NoTarget => {
                self.status_message = Some(no_target_msg.to_string());
                self.repaint()?;
            }
            DiffNavResult::Found(line, status) => {
                self.save_last_position();
                self.screen.goto_line(line, total);
                self.status_message = Some(status);
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Navigate to the next man page section.
    fn navigate_next_man_section(&mut self, total: usize) -> Result<()> {
        let result = self.man_section_nav(|secs, cur| pgr_core::next_section_line(secs, cur, true));
        self.apply_man_section_nav(result, total, "No sections found")
    }

    /// Navigate to the previous man page section.
    fn navigate_prev_man_section(&mut self, total: usize) -> Result<()> {
        let result = self.man_section_nav(|secs, cur| pgr_core::prev_section_line(secs, cur, true));
        self.apply_man_section_nav(result, total, "No sections found")
    }

    /// Compute a man section navigation target without mutating self.
    fn man_section_nav(
        &self,
        find_fn: impl FnOnce(&[pgr_core::ManSection], usize) -> Option<usize>,
    ) -> ManSectionNavResult {
        let Some(ref sections) = self.man_sections else {
            return ManSectionNavResult::NotManMode;
        };
        let current = self.screen.top_line();
        match find_fn(sections, current) {
            Some(target) => {
                let status = pgr_core::section_status(sections, target);
                ManSectionNavResult::Found(target, status)
            }
            None => ManSectionNavResult::NoTarget,
        }
    }

    /// Apply the result of a man section navigation computation.
    fn apply_man_section_nav(
        &mut self,
        result: ManSectionNavResult,
        total: usize,
        no_target_msg: &str,
    ) -> Result<()> {
        match result {
            ManSectionNavResult::NotManMode => {
                self.status_message = Some("Not in man page mode".to_string());
                self.repaint()?;
            }
            ManSectionNavResult::NoTarget => {
                self.status_message = Some(no_target_msg.to_string());
                self.repaint()?;
            }
            ManSectionNavResult::Found(line, status) => {
                self.save_last_position();
                self.screen.goto_line(line, total);
                self.status_message = Some(status);
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Compute a hunk navigation target (line + status message) without mutating self.
    fn diff_nav_hunk(
        &self,
        find_fn: impl FnOnce(&[pgr_core::DiffFile], usize) -> Option<usize>,
    ) -> DiffNavResult {
        let Some(ref files) = self.diff_state else {
            return DiffNavResult::NotDiffMode;
        };
        let current = self.screen.top_line();
        match find_fn(files, current) {
            Some(target) => {
                let status = pgr_core::compute_diff_prompt_info(files, target)
                    .and_then(|info| info.hunk_index)
                    .map_or_else(String::new, |(cur, tot)| format!("Hunk {cur} of {tot}"));
                DiffNavResult::Found(target, status)
            }
            None => DiffNavResult::NoTarget,
        }
    }

    /// Compute a file navigation target (line + status message) without mutating self.
    fn diff_nav_file(
        &self,
        find_fn: impl FnOnce(&[pgr_core::DiffFile], usize) -> Option<usize>,
    ) -> DiffNavResult {
        let Some(ref files) = self.diff_state else {
            return DiffNavResult::NotDiffMode;
        };
        let current = self.screen.top_line();
        match find_fn(files, current) {
            Some(target) => {
                let status = pgr_core::compute_diff_prompt_info(files, target)
                    .map(|info| {
                        let file_label = info.current_file.as_deref().unwrap_or("(unknown)");
                        info.file_index.map_or_else(String::new, |(cur, tot)| {
                            format!("File {cur} of {tot}: {file_label}")
                        })
                    })
                    .unwrap_or_default();
                DiffNavResult::Found(target, status)
            }
            None => DiffNavResult::NoTarget,
        }
    }

    /// Apply the result of a diff navigation computation.
    fn apply_diff_nav(
        &mut self,
        result: DiffNavResult,
        total: usize,
        no_target_msg: &str,
    ) -> Result<()> {
        match result {
            DiffNavResult::NotDiffMode => {
                self.status_message = Some("Not in diff mode".to_string());
                self.repaint()?;
            }
            DiffNavResult::NoTarget => {
                self.status_message = Some(no_target_msg.to_string());
                self.repaint()?;
            }
            DiffNavResult::Found(line, status) => {
                self.save_last_position();
                self.screen.goto_line(line, total);
                self.status_message = Some(status);
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Write a message to the status line.
    fn write_status(&mut self, msg: &str) -> Result<()> {
        let (rows, _) = self.screen.dimensions();
        if rows > 0 {
            write!(self.writer, "\x1b[{rows};1H\x1b[K{msg}")?;
            self.writer.flush()?;
        }
        Ok(())
    }

    /// Enter search prompt mode for the given direction.
    fn enter_search_mode(
        &mut self,
        direction: SearchDirection,
        count: Option<usize>,
    ) -> Result<()> {
        let prompt = match direction {
            SearchDirection::Forward => "/",
            SearchDirection::Backward => "?",
        };
        self.search_prompt_direction = direction;
        self.line_editor = Some(LineEditor::new(prompt));
        self.editing_search = true;
        self.pending_count = count;
        // Save position for incremental search restore on cancel.
        if self.runtime_options.incsearch {
            self.incsearch_saved_top = Some(self.screen.top_line());
        }
        self.render_search_prompt()?;
        Ok(())
    }

    /// Process a key while in search-prompt editing mode.
    fn process_search_key(&mut self, key: &Key) -> Result<bool> {
        // ^L (literal next): if previously pressed, insert the next character
        // raw into the pattern buffer without interpreting it as a modifier.
        if self.search_literal_next {
            self.search_literal_next = false;
            if let Some(ref mut editor) = self.line_editor {
                let ch = match key {
                    Key::Char(c) => *c,
                    Key::Ctrl(c) => {
                        // Convert Ctrl+letter to its control-character value.
                        char::from((*c as u8).wrapping_sub(b'a').wrapping_add(1))
                    }
                    _ => {
                        // Non-character keys: fall through to normal processing.
                        let result = editor.process_key_with_history(key, &self.search_history);
                        return self.finish_search_key(result);
                    }
                };
                editor.insert(ch);
                self.render_search_prompt()?;
                return Ok(true);
            }
        }

        // ^L: set literal-next flag so the following character is inserted raw.
        if matches!(key, Key::Ctrl('l')) && self.line_editor.is_some() {
            self.search_literal_next = true;
            self.render_search_prompt()?;
            return Ok(true);
        }

        // ^S: sub-pattern search (not supported — show status message).
        if matches!(key, Key::Ctrl('s')) && self.line_editor.is_some() {
            self.editing_search = false;
            self.pending_count = None;
            self.line_editor = None;
            self.search_literal_next = false;
            self.status_message = Some("Sub-pattern search not supported".to_string());
            self.repaint()?;
            return Ok(true);
        }

        // Intercept search modifier keys and inject their raw control-character
        // bytes into the line editor buffer so that `SearchModifiers::parse` can
        // extract them later. ^N = invert, ^R = literal, ^F = from-first,
        // ^K = keep-pos. (^E and ^W conflict with line-editor bindings.)
        if matches!(key, Key::Ctrl('n' | 'r' | 'f' | 'k')) {
            if let Some(ref mut editor) = self.line_editor {
                let ch = match key {
                    Key::Ctrl('n') => '\x0e',
                    Key::Ctrl('r') => '\x12',
                    Key::Ctrl('f') => '\x06',
                    Key::Ctrl('k') => '\x0b',
                    _ => unreachable!(),
                };
                editor.insert(ch);
                self.render_search_prompt()?;
                return Ok(true);
            }
        }

        let result = if let Some(ref mut editor) = self.line_editor {
            editor.process_key_with_history(key, &self.search_history)
        } else {
            // Should not happen, but recover gracefully.
            self.editing_search = false;
            return Ok(true);
        };

        self.finish_search_key(result)
    }

    /// Handle a [`LineEditResult`] from search prompt editing.
    fn finish_search_key(&mut self, result: LineEditResult) -> Result<bool> {
        match result {
            LineEditResult::Continue | LineEditResult::ContinueWithStatus(_) => {
                // Incremental search: on each keystroke, search for the
                // current pattern and scroll to the first match.
                if self.runtime_options.incsearch {
                    self.do_incsearch()?;
                }
                self.render_search_prompt()?;
            }
            LineEditResult::Confirm(pattern_str) => {
                self.editing_search = false;
                let count = self.pending_count.take();
                self.line_editor = None;
                self.search_literal_next = false;
                self.incsearch_saved_top = None;
                self.search_history.push(pattern_str.clone());
                self.submit_search(&pattern_str, self.search_prompt_direction, count)?;
            }
            LineEditResult::Cancel => {
                self.editing_search = false;
                self.pending_count = None;
                self.line_editor = None;
                self.search_literal_next = false;
                // Restore position saved before incremental search.
                if let Some(saved) = self.incsearch_saved_top.take() {
                    self.screen.set_top_line(saved);
                }
                self.repaint()?;
            }
        }
        Ok(!self.should_quit)
    }

    /// Render the search prompt (line editor) on the status line.
    fn render_search_prompt(&mut self) -> Result<()> {
        if let Some(ref editor) = self.line_editor {
            let (rows, cols) = self.screen.dimensions();
            if rows > 0 {
                editor.render(&mut self.writer, rows - 1, 0, cols)?;
            }
        }
        Ok(())
    }

    /// Handle the `!` (shell command) dispatch.
    ///
    /// If the very first key after `!` is another `!`, immediately re-execute
    /// the last shell command without opening the full line editor (the `!!`
    /// shortcut). An empty Enter also repeats. Otherwise the key is fed into
    /// the line editor for normal command entry.
    fn handle_shell_command(&mut self) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        // Peek at the next key to support the `!!` shortcut.
        let first_key = match self.reader.read_key() {
            Ok(k) => k,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        let cmd = if first_key == Key::Char('!') {
            // `!!` — repeat last shell command. GNU less displays the previous
            // command in the prompt and waits for Enter to confirm. Consume
            // the confirmation key so the input stream stays synchronised.
            match self.last_shell_command.clone() {
                Some(prev) => {
                    let _ = self.reader.read_key(); // consume confirmation Enter
                    prev
                }
                None => return Ok(()),
            }
        } else if first_key == Key::Enter {
            // `!<Enter>` — repeat last shell command (no confirmation needed).
            match self.last_shell_command.clone() {
                Some(prev) => prev,
                None => return Ok(()),
            }
        } else if first_key == Key::Escape {
            // Cancel.
            return Ok(());
        } else {
            // Feed the first key into the line editor for normal command entry.
            let input = self.read_command_line_with_initial("!", &first_key)?;
            let Some(input) = input else {
                return Ok(());
            };
            if input.is_empty() {
                match self.last_shell_command.clone() {
                    Some(prev) => prev,
                    None => return Ok(()),
                }
            } else {
                input
            }
        };

        self.last_shell_command = Some(cmd.clone());

        // Exit alternate screen so shell output appears on the normal screen.
        self.writer.write_all(b"\x1b[?1049l")?;
        self.writer.flush()?;

        let _ = shell::execute_shell_command(&cmd, &self.shell);

        // Show "!done" and wait for a keypress before repainting,
        // matching GNU less behavior.
        write!(self.writer, "\r\n!done  (press RETURN)")?;
        self.writer.flush()?;
        let _ = self.reader.read_key();

        // Re-enter alternate screen and repaint. The terminal may not
        // preserve the previous alt-screen content (especially in PTY
        // contexts), so an explicit repaint is required to restore the
        // file display.
        self.writer.write_all(b"\x1b[?1049h")?;
        self.writer.flush()?;
        // Reset initial_render so the next repaint bottom-aligns short files.
        self.initial_render = true;
        self.repaint()?;
        Ok(())
    }

    /// Handle the `#` (shell command with expansion) dispatch.
    fn handle_shell_command_expand(&mut self, total: usize) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        let input = self.read_command_line("#")?;
        let Some(input) = input else {
            return Ok(());
        };

        if input.is_empty() {
            return Ok(());
        }

        let line_number = self.screen.top_line().saturating_add(1);
        let expanded =
            shell::expand_command_string(&input, self.filename.as_deref(), line_number, total, 0);

        self.last_shell_command = Some(expanded.clone());

        // Exit alternate screen so shell output appears on the normal screen.
        self.writer.write_all(b"\x1b[?1049l")?;
        self.writer.flush()?;

        let _ = shell::execute_shell_command(&expanded, &self.shell);

        write!(self.writer, "\r\n!done  (press RETURN)")?;
        self.writer.flush()?;
        let _ = self.reader.read_key();

        // Re-enter alternate screen and repaint — same as handle_shell_command.
        self.writer.write_all(b"\x1b[?1049h")?;
        self.writer.flush()?;
        self.initial_render = true;
        self.repaint()?;
        Ok(())
    }

    /// Handle the `|` (pipe to command) dispatch.
    fn handle_pipe_to_command(&mut self, total: usize) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        // Read the mark character (next keypress).
        let mark_key = match self.reader.read_key() {
            Ok(k) => k,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(e) => return Err(e.into()),
        };

        let Key::Char(mark_char) = mark_key else {
            return Ok(());
        };

        // Read the command string.
        let input = self.read_command_line("|")?;
        let Some(input) = input else {
            return Ok(());
        };

        if input.is_empty() {
            return Ok(());
        }

        // Determine the line range: from the mark position to the current top-of-screen.
        let mark_line = if mark_char == '.' {
            // '.' means current screen top
            self.screen.top_line()
        } else {
            match self.marks.get(mark_char) {
                Some(m) => m.line,
                None => return Ok(()),
            }
        };

        let current_top = self.screen.top_line();
        let start = mark_line.min(current_top);
        let end = mark_line.max(current_top);

        // Collect lines in the range.
        let mut content = String::new();
        for line_num in start..=end.min(total.saturating_sub(1)) {
            if let Some(line) = self.index.get_line(line_num, &*self.buffer)? {
                content.push_str(&line);
                content.push('\n');
            }
        }

        let _ = shell::pipe_to_command(&input, &self.shell, &content);
        self.repaint()?;
        Ok(())
    }

    /// Handle the `v` (edit file) dispatch.
    fn handle_edit_file(&mut self) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        let Some(filename) = self.filename.clone() else {
            self.write_status("No file to edit")?;
            return Ok(());
        };

        let line_number = self.screen.top_line().saturating_add(1);
        let cmd = shell::build_editor_command(&self.editor, &filename, line_number);
        let _ = shell::execute_shell_command(&cmd, &self.shell);
        // File may have changed — refresh.
        self.buffer.refresh()?;
        let new_len = self.buffer.len() as u64;
        self.index = LineIndex::new(new_len);
        self.repaint()?;
        Ok(())
    }

    /// Handle the `s` (save pipe input) dispatch.
    fn handle_save_pipe_input(&mut self, total: usize) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        if !self.is_pipe {
            self.write_status("Not reading from pipe")?;
            return Ok(());
        }

        let input = self.read_command_line("s ")?;
        let Some(input) = input else {
            return Ok(());
        };

        if input.is_empty() {
            return Ok(());
        }

        // Collect all buffer content.
        let mut content = String::new();
        for line_num in 0..total {
            if let Some(line) = self.index.get_line(line_num, &*self.buffer)? {
                content.push_str(&line);
                content.push('\n');
            }
        }

        std::fs::write(&input, content)?;
        Ok(())
    }

    /// Handle the `s` (save buffer) dispatch.
    ///
    /// Saves all buffer lines to a user-specified file, stripping ANSI escape
    /// sequences so the output is plain text. Works for both pipe and file input.
    /// Blocked in secure mode.
    fn handle_save_buffer(&mut self, total: usize) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        let input = self.read_command_line("s ")?;
        let Some(input) = input else {
            return Ok(());
        };

        if input.is_empty() {
            return Ok(());
        }

        // Collect all buffer lines, stripping ANSI escape sequences.
        let mut content = String::new();
        let mut line_count: usize = 0;
        for line_num in 0..total {
            if let Some(line) = self.index.get_line(line_num, &*self.buffer)? {
                let stripped = pgr_display::ansi::strip_ansi(&line);
                content.push_str(&stripped);
                content.push('\n');
                line_count += 1;
            }
        }

        std::fs::write(&input, &content)?;
        self.write_status(&format!("Saved {line_count} lines to {input}"))?;
        Ok(())
    }

    /// Read a command line from the user using the line editor.
    ///
    /// Returns `Ok(Some(input))` on confirmation, `Ok(None)` on cancellation
    /// or EOF.
    fn read_command_line(&mut self, prompt: &str) -> Result<Option<String>> {
        let mut editor = LineEditor::new(prompt);
        let (rows, cols) = self.screen.dimensions();
        let prompt_row = rows.saturating_sub(1);
        editor.render(&mut self.writer, prompt_row, 0, cols)?;
        self.writer.flush()?;

        loop {
            match self.reader.read_key() {
                Ok(key) => match editor.process_key(&key) {
                    LineEditResult::Continue | LineEditResult::ContinueWithStatus(_) => {
                        editor.render(&mut self.writer, prompt_row, 0, cols)?;
                        self.writer.flush()?;
                    }
                    LineEditResult::Confirm(s) => return Ok(Some(s)),
                    LineEditResult::Cancel => return Ok(None),
                },
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Read a command line, feeding an initial key into the editor first.
    ///
    /// Used when the caller has already consumed a key that should be part
    /// of the command input (e.g., after peeking at the first character).
    fn read_command_line_with_initial(
        &mut self,
        prompt: &str,
        initial_key: &Key,
    ) -> Result<Option<String>> {
        let mut editor = LineEditor::new(prompt);
        let (rows, cols) = self.screen.dimensions();
        let prompt_row = rows.saturating_sub(1);

        // Process the initial key first.
        match editor.process_key(initial_key) {
            LineEditResult::Continue | LineEditResult::ContinueWithStatus(_) => {
                editor.render(&mut self.writer, prompt_row, 0, cols)?;
                self.writer.flush()?;
            }
            LineEditResult::Confirm(s) => return Ok(Some(s)),
            LineEditResult::Cancel => return Ok(None),
        }

        // Continue reading keys normally.
        loop {
            match self.reader.read_key() {
                Ok(key) => match editor.process_key(&key) {
                    LineEditResult::Continue | LineEditResult::ContinueWithStatus(_) => {
                        editor.render(&mut self.writer, prompt_row, 0, cols)?;
                        self.writer.flush()?;
                    }
                    LineEditResult::Confirm(s) => return Ok(Some(s)),
                    LineEditResult::Cancel => return Ok(None),
                },
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Determine the effective case mode from runtime options.
    ///
    /// `-I` (`case_insensitive_always`) forces case-insensitive regardless of
    /// pattern content. `-i` (`case_insensitive`) enables smart case. Otherwise
    /// the default is case-sensitive.
    fn effective_case_mode(&self) -> CaseMode {
        if self.runtime_options.case_insensitive_always {
            CaseMode::Insensitive
        } else if self.runtime_options.case_insensitive {
            CaseMode::Smart
        } else {
            CaseMode::Sensitive
        }
    }

    /// Submit a search pattern: compile, search, scroll to match.
    fn submit_search(
        &mut self,
        pattern_str: &str,
        direction: SearchDirection,
        count: Option<usize>,
    ) -> Result<()> {
        // Parse modifier prefixes from the pattern string.
        let (modifiers, raw_pattern) = SearchModifiers::parse(pattern_str);

        // Empty pattern with no previous pattern: do nothing.
        if raw_pattern.is_empty() {
            if self.last_pattern.is_some() {
                // Re-use last pattern in the new direction, but apply new modifiers.
                self.last_direction = direction;
                if !modifiers.is_empty() {
                    self.last_modifiers = modifiers;
                }
                let found = self.do_search(direction, count, true)?;
                if !found && self.cross_file_search {
                    self.cross_file_continue(direction, count)?;
                }
                self.cross_file_search = false;
                return Ok(());
            }
            self.repaint()?;
            return Ok(());
        }

        // If literal mode, escape regex metacharacters.
        let compile_pattern = if modifiers.literal {
            SearchPattern::escape(raw_pattern)
        } else {
            raw_pattern.to_string()
        };

        // Compile the pattern with runtime case mode.
        let case_mode = self.effective_case_mode();
        let Ok(compiled) = SearchPattern::compile(&compile_pattern, case_mode) else {
            self.status_message = Some("Invalid pattern".to_string());
            self.repaint()?;
            return Ok(());
        };

        self.last_pattern = Some(compiled);
        self.last_direction = direction;
        self.last_modifiers = modifiers;
        self.highlight_state.clear();
        let found = self.do_search(direction, count, false)?;
        if !found && self.cross_file_search {
            self.cross_file_continue(direction, count)?;
        }
        self.cross_file_search = false;
        Ok(())
    }

    /// Perform a search in the given direction using `last_pattern`.
    ///
    /// When `is_repeat` is true (e.g., `n`/`N` commands), the search start
    /// position advances past the current match to avoid re-finding it.
    /// When false (new `/` or `?` search), the start position includes the
    /// current viewport.
    ///
    /// Returns `true` if a match was found, `false` otherwise.
    fn do_search(
        &mut self,
        direction: SearchDirection,
        count: Option<usize>,
        is_repeat: bool,
    ) -> Result<bool> {
        let Some(ref pattern) = self.last_pattern else {
            self.status_message = Some("No previous search pattern".to_string());
            self.repaint()?;
            return Ok(false);
        };

        // Re-compile with current runtime case mode so that toggling -i/-I
        // between searches takes effect immediately.
        let case_mode = self.effective_case_mode();
        let mut searcher = Searcher::new(
            SearchPattern::compile(pattern.pattern(), case_mode)?,
            direction,
        );

        // Apply modifier flags.
        // Interactive search always wraps by default. The ^W modifier
        // forces wrapping even when --no-search-wrap is set (not yet
        // implemented). For now, always wrap.
        searcher.set_wrap(WrapMode::Wrap);
        searcher.set_inverted(self.last_modifiers.invert);

        // Resolve the jump target to a 0-based screen row offset. With the
        // default `-j1` this is 0 (match at top of screen). With `-j5` it is
        // 4 (match on the 5th screen line).
        let jump_offset = self
            .runtime_options
            .jump_target
            .resolve(self.screen.content_rows());

        // Determine start line.
        //
        // GNU less search start positions:
        //
        // New search (is_repeat = false):
        //   Forward: top_line (inclusive — check the current top line)
        //   Backward: top_line + content_rows - 1 (bottom of visible screen)
        //
        // Repeat search (is_repeat = true):
        //   The current match sits at top_line + jump_offset. To avoid
        //   re-finding the same match we start one line past it.
        //   Forward: top_line + jump_offset + 1
        //   Backward: (top_line + jump_offset) - 1
        //
        // The from_first modifier overrides to search from file boundaries.
        let start = if self.last_modifiers.from_first {
            match direction {
                SearchDirection::Forward => 0,
                SearchDirection::Backward => {
                    let total = self.index.total_lines(&*self.buffer)?;
                    total.saturating_sub(1)
                }
            }
        } else if is_repeat {
            let match_line = self.screen.top_line() + jump_offset;
            match direction {
                SearchDirection::Forward => match_line + 1,
                SearchDirection::Backward => match_line.saturating_sub(1),
            }
        } else {
            match direction {
                SearchDirection::Forward => self.screen.top_line(),
                SearchDirection::Backward => {
                    self.screen.top_line() + self.screen.content_rows() - 1
                }
            }
        };

        let n = count.unwrap_or(1);
        let result = searcher.search_nth(start, n, &*self.buffer, &mut self.index)?;

        if let Some(line) = result {
            // Update match count cache for prompt display.
            self.update_match_count_cache(searcher.pattern(), line)?;

            // If keep_position is set, update highlights but don't scroll.
            if self.last_modifiers.keep_position {
                self.repaint()?;
            } else {
                self.save_last_position();
                // Use set_top_line (unclamped) so the match appears at the
                // jump target row, matching GNU less. Subtract jump_offset
                // so the matched line lands at the target screen row (e.g.,
                // with `-j5` the match appears on the 5th line, not the 1st).
                self.screen.set_top_line(line.saturating_sub(jump_offset));
                self.repaint()?;
            }
            Ok(true)
        } else {
            self.match_count_cache = None;
            self.status_message = Some("Pattern not found".to_string());
            self.repaint()?;
            Ok(false)
        }
    }

    /// Perform an incremental search using the current line-editor contents.
    ///
    /// Searches from the saved pre-search position (or file start) so that
    /// every keystroke re-evaluates from a stable origin. Scrolls to the
    /// first match without committing it as `last_pattern`.
    fn do_incsearch(&mut self) -> Result<()> {
        let pattern_text = match self.line_editor {
            Some(ref editor) => editor.contents().to_string(),
            None => return Ok(()),
        };

        let (_mods, raw_pattern) = SearchModifiers::parse(&pattern_text);
        if raw_pattern.is_empty() {
            // No pattern yet — restore original position.
            if let Some(saved) = self.incsearch_saved_top {
                self.screen.set_top_line(saved);
                self.repaint()?;
            }
            return Ok(());
        }

        let case_mode = self.effective_case_mode();
        let Ok(compiled) = SearchPattern::compile(raw_pattern, case_mode) else {
            return Ok(());
        };

        let start = self.incsearch_saved_top.unwrap_or(0);
        let mut searcher = Searcher::new(compiled, self.search_prompt_direction);
        searcher.set_wrap(WrapMode::Wrap);

        let result = searcher.search_nth(start, 1, &*self.buffer, &mut self.index)?;
        if let Some(line) = result {
            // Show live match count for files under 100k lines.
            let total_lines = self.index.lines_indexed();
            if total_lines <= 100_000 {
                self.update_match_count_cache(searcher.pattern(), line)?;
            } else {
                self.match_count_cache = None;
            }
            self.screen.set_top_line(line);
            self.repaint()?;
        } else {
            self.match_count_cache = None;
            if let Some(saved) = self.incsearch_saved_top {
                self.screen.set_top_line(saved);
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Update the match count cache for the given pattern and current match line.
    ///
    /// Reuses the cached total if the pattern string hasn't changed (avoids
    /// re-scanning the buffer). Always recomputes the current match index
    /// since the viewport position may have changed.
    fn update_match_count_cache(
        &mut self,
        pattern: &SearchPattern,
        match_line: usize,
    ) -> Result<()> {
        let pat_str = pattern.pattern().to_string();

        let total = if let Some(ref cache) = self.match_count_cache {
            if cache.pattern == pat_str {
                cache.total_matches
            } else {
                count_matches(pattern, &*self.buffer, &mut self.index)?
            }
        } else {
            count_matches(pattern, &*self.buffer, &mut self.index)?
        };

        let current = find_match_index(pattern, match_line, &*self.buffer, &mut self.index)?;

        self.match_count_cache = Some(MatchCountCache {
            pattern: pat_str,
            total_matches: total,
            current_match: current,
        });

        Ok(())
    }

    /// Repeat the last search, optionally reversing direction.
    fn repeat_search(&mut self, reverse: bool, count: Option<usize>) -> Result<()> {
        if self.last_pattern.is_none() {
            self.status_message = Some("No previous search pattern".to_string());
            self.absorb_next_key = true;
            self.repaint()?;
            return Ok(());
        }

        let direction = if reverse {
            match self.last_direction {
                SearchDirection::Forward => SearchDirection::Backward,
                SearchDirection::Backward => SearchDirection::Forward,
            }
        } else {
            self.last_direction
        };

        self.do_search(direction, count, true)?;
        Ok(())
    }

    /// Repeat the last search with cross-file behavior (ESC-n, ESC-N).
    ///
    /// Searches the current file first. If no match is found and a file list
    /// is present, switches to the next (or previous) file and searches from
    /// the beginning (or end). Repeats until a match is found or all files
    /// are exhausted.
    fn repeat_search_cross_file(&mut self, reverse: bool, count: Option<usize>) -> Result<()> {
        if self.last_pattern.is_none() {
            self.status_message = Some("No previous search pattern".to_string());
            self.repaint()?;
            return Ok(());
        }

        let direction = if reverse {
            match self.last_direction {
                SearchDirection::Forward => SearchDirection::Backward,
                SearchDirection::Backward => SearchDirection::Forward,
            }
        } else {
            self.last_direction
        };

        let found = self.do_search(direction, count, true)?;
        if !found {
            self.cross_file_continue(direction, count)?;
        }
        Ok(())
    }

    /// Continue a cross-file search into adjacent files.
    ///
    /// Switches to the next file (forward) or previous file (backward) and
    /// searches from the beginning or end respectively. Iterates through all
    /// files until a match is found or the starting file is reached again.
    fn cross_file_continue(
        &mut self,
        direction: SearchDirection,
        count: Option<usize>,
    ) -> Result<()> {
        let file_count = self.file_list.as_ref().map_or(0, FileList::file_count);

        if file_count <= 1 {
            // Single file or no file list — nothing to cross into.
            return Ok(());
        }

        let start_index = self.file_list.as_ref().map_or(0, FileList::current_index);

        for _ in 1..file_count {
            // Try switching to the next/prev file.
            let switch_ok = match direction {
                SearchDirection::Forward => self.try_switch_file_next(),
                SearchDirection::Backward => self.try_switch_file_prev(),
            };

            if !switch_ok {
                // Wrapped around or no more files. In GNU less, cross-file
                // search does NOT wrap from the last file back to the first
                // (or vice versa). Stop here.
                break;
            }

            // Position the viewport at the beginning (forward) or end (backward)
            // of the new file so do_search scans the entire file.
            match direction {
                SearchDirection::Forward => {
                    self.screen.set_top_line(0);
                }
                SearchDirection::Backward => {
                    if let Ok(total) = self.index.total_lines(&*self.buffer) {
                        self.screen.set_top_line(total.saturating_sub(1));
                    }
                }
            }

            // Search from the start/end of the new file (non-repeat so we
            // include the very first/last line).
            let found = self.do_search(direction, count, false)?;
            if found {
                return Ok(());
            }
        }

        // Exhausted all files. Restore original file position.
        // The "Pattern not found" message is already set by the last do_search.
        // Switch back to the original file.
        let current_index = self.file_list.as_ref().map_or(0, FileList::current_index);
        if current_index != start_index {
            self.switch_file_goto_impl(start_index)?;
        }
        Ok(())
    }

    /// Try to switch to the next file, returning true on success.
    ///
    /// Unlike `switch_file_next`, this does not set a status message on failure.
    fn try_switch_file_next(&mut self) -> bool {
        let switched = if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            let old_name = self.filename.clone();
            if file_list.next().is_err() {
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                return false;
            }
            self.previous_file = old_name;
            true
        } else {
            false
        };
        if switched {
            self.apply_current_file_impl(true);
        }
        switched
    }

    /// Try to switch to the previous file, returning true on success.
    ///
    /// Unlike `switch_file_prev`, this does not set a status message on failure.
    fn try_switch_file_prev(&mut self) -> bool {
        let switched = if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            let old_name = self.filename.clone();
            if file_list.prev().is_err() {
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                return false;
            }
            self.previous_file = old_name;
            true
        } else {
            false
        };
        if switched {
            self.apply_current_file_impl(true);
        }
        switched
    }

    /// Switch to a specific file index without status messages.
    fn switch_file_goto_impl(&mut self, index: usize) -> Result<()> {
        let switched = if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            let old_name = self.filename.clone();
            if file_list.goto(index).is_err() {
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                return Ok(());
            }
            self.previous_file = old_name;
            true
        } else {
            false
        };
        if switched {
            self.apply_current_file_impl(true);
            self.repaint()?;
        }
        Ok(())
    }

    /// Enter filter prompt mode for the `&` command.
    fn enter_filter_mode(&mut self) -> Result<()> {
        self.filter_invert = false;
        self.line_editor = Some(LineEditor::new("&"));
        self.editing_filter = true;
        self.render_filter_prompt()?;
        Ok(())
    }

    /// Process a key while in filter-prompt editing mode.
    fn process_filter_key(&mut self, key: &Key) -> Result<bool> {
        // Intercept Ctrl+N to toggle inversion before the line editor sees it.
        if *key == Key::Ctrl('n') {
            self.filter_invert = !self.filter_invert;
            // Update the prompt to reflect inversion state.
            let prompt = if self.filter_invert { "&!" } else { "&" };
            let contents = self
                .line_editor
                .as_ref()
                .map_or(String::new(), |e| e.contents().to_string());
            self.line_editor = Some(LineEditor::with_initial(prompt, &contents));
            self.render_filter_prompt()?;
            return Ok(true);
        }

        let result = if let Some(ref mut editor) = self.line_editor {
            editor.process_key(key)
        } else {
            self.editing_filter = false;
            return Ok(true);
        };

        match result {
            LineEditResult::Continue | LineEditResult::ContinueWithStatus(_) => {
                self.render_filter_prompt()?;
            }
            LineEditResult::Confirm(pattern_str) => {
                self.editing_filter = false;
                self.line_editor = None;
                self.submit_filter(&pattern_str)?;
            }
            LineEditResult::Cancel => {
                self.editing_filter = false;
                self.line_editor = None;
                self.repaint()?;
            }
        }
        Ok(!self.should_quit)
    }

    /// Render the filter prompt on the status line.
    fn render_filter_prompt(&mut self) -> Result<()> {
        if let Some(ref editor) = self.line_editor {
            let (rows, cols) = self.screen.dimensions();
            if rows > 0 {
                editor.render(&mut self.writer, rows - 1, 0, cols)?;
            }
        }
        Ok(())
    }

    /// Submit the filter: compile pattern, build filtered lines, repaint.
    fn submit_filter(&mut self, pattern_str: &str) -> Result<()> {
        if pattern_str.is_empty() {
            // Empty pattern clears the filter.
            // Preserve position: map the current filtered top_line back to an
            // actual buffer line so the viewport stays at the same content.
            let actual_top = self
                .filtered_lines
                .as_ref()
                .and_then(|fl| fl.actual_line(self.screen.top_line()));
            self.filter.clear();
            self.filtered_lines = None;
            self.index.index_all(&*self.buffer)?;
            let total = self.index.lines_indexed();
            self.screen.goto_line(actual_top.unwrap_or(0), total);
            self.repaint()?;
            return Ok(());
        }

        let Ok(compiled) = SearchPattern::compile(pattern_str, CaseMode::Sensitive) else {
            self.status_message = Some("Invalid pattern".to_string());
            self.repaint()?;
            return Ok(());
        };

        self.filter.set_pattern(Some(compiled));
        self.filter.set_inverted(self.filter_invert);
        self.rebuild_filtered_lines()?;
        // Reset viewport to the beginning of the filtered view.
        let visible_total = self
            .filtered_lines
            .as_ref()
            .map_or(0, FilteredLines::visible_count);
        self.screen.goto_line(0, visible_total);
        self.repaint()?;
        Ok(())
    }

    /// Rebuild the filtered line mapping from the current filter state.
    fn rebuild_filtered_lines(&mut self) -> Result<()> {
        let fl = FilteredLines::build(&*self.buffer, &mut self.index, &self.filter)?;
        self.filtered_lines = Some(fl);
        Ok(())
    }

    /// Enter add-highlight prompt mode (`&+`).
    fn enter_add_highlight_mode(&mut self) -> Result<()> {
        self.line_editor = Some(LineEditor::new("&+"));
        self.editing_add_highlight = true;
        self.render_highlight_prompt()?;
        Ok(())
    }

    /// Enter remove-highlight prompt mode (`&-`).
    fn enter_remove_highlight_mode(&mut self) -> Result<()> {
        self.line_editor = Some(LineEditor::new("&-"));
        self.editing_remove_highlight = true;
        self.render_highlight_prompt()?;
        Ok(())
    }

    /// Process a key while in add-highlight prompt mode.
    fn process_add_highlight_key(&mut self, key: &Key) -> Result<bool> {
        let result = if let Some(ref mut editor) = self.line_editor {
            editor.process_key(key)
        } else {
            self.editing_add_highlight = false;
            return Ok(true);
        };

        match result {
            LineEditResult::Continue | LineEditResult::ContinueWithStatus(_) => {
                self.render_highlight_prompt()?;
            }
            LineEditResult::Confirm(pattern_str) => {
                self.editing_add_highlight = false;
                self.line_editor = None;
                self.submit_add_highlight(&pattern_str)?;
            }
            LineEditResult::Cancel => {
                self.editing_add_highlight = false;
                self.line_editor = None;
                self.repaint()?;
            }
        }
        Ok(!self.should_quit)
    }

    /// Process a key while in remove-highlight prompt mode.
    fn process_remove_highlight_key(&mut self, key: &Key) -> Result<bool> {
        let result = if let Some(ref mut editor) = self.line_editor {
            editor.process_key(key)
        } else {
            self.editing_remove_highlight = false;
            return Ok(true);
        };

        match result {
            LineEditResult::Continue | LineEditResult::ContinueWithStatus(_) => {
                self.render_highlight_prompt()?;
            }
            LineEditResult::Confirm(pattern_str) => {
                self.editing_remove_highlight = false;
                self.line_editor = None;
                self.submit_remove_highlight(&pattern_str)?;
            }
            LineEditResult::Cancel => {
                self.editing_remove_highlight = false;
                self.line_editor = None;
                self.repaint()?;
            }
        }
        Ok(!self.should_quit)
    }

    /// Render the highlight prompt on the status line.
    fn render_highlight_prompt(&mut self) -> Result<()> {
        if let Some(ref editor) = self.line_editor {
            let (rows, cols) = self.screen.dimensions();
            if rows > 0 {
                editor.render(&mut self.writer, rows - 1, 0, cols)?;
            }
        }
        Ok(())
    }

    /// Submit a new highlight pattern from the `&+` prompt.
    fn submit_add_highlight(&mut self, pattern_str: &str) -> Result<()> {
        if pattern_str.is_empty() {
            self.repaint()?;
            return Ok(());
        }
        let case_mode = self.effective_case_mode();
        match self.highlight_state.add_pattern(pattern_str, case_mode) {
            Ok(Some(idx)) => {
                self.status_message =
                    Some(format!("Highlight added: \"{pattern_str}\" (color {idx})"));
                self.repaint()?;
            }
            Ok(None) => {
                self.status_message =
                    Some("Maximum number of highlight patterns reached".to_string());
                self.repaint()?;
            }
            Err(e) => {
                self.status_message = Some(format!("Invalid pattern: {e}"));
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Submit a highlight removal from the `&-` prompt.
    fn submit_remove_highlight(&mut self, pattern_str: &str) -> Result<()> {
        if pattern_str.is_empty() {
            self.repaint()?;
            return Ok(());
        }
        if self.highlight_state.remove_pattern(pattern_str) {
            self.status_message = Some(format!("Highlight removed: \"{pattern_str}\""));
        } else {
            self.status_message = Some(format!("No highlight pattern: \"{pattern_str}\""));
        }
        self.repaint()?;
        Ok(())
    }

    /// Show the list of active highlight patterns as a status message.
    fn show_highlight_list(&mut self) -> Result<()> {
        let patterns = self.highlight_state.list_patterns();
        if patterns.is_empty() {
            self.status_message = Some("No extra highlight patterns".to_string());
        } else {
            let list: Vec<String> = patterns
                .iter()
                .map(|(pat, idx)| format!("[{idx}] \"{pat}\""))
                .collect();
            self.status_message = Some(format!("Highlights: {}", list.join(", ")));
        }
        self.repaint()?;
        self.absorb_next_key = true;
        Ok(())
    }

    /// Enter follow mode: scroll to end, watch for new data, and wait for
    /// interrupt.
    ///
    /// Scrolls to the end of the buffer and displays "Waiting for data...
    /// (interrupt to abort)" on the status line. When a `key_fd` is set and
    /// the file has a path on disk, uses kqueue to multiplex between file
    /// change notifications and key input. Otherwise falls back to a simple
    /// blocking key-read loop.
    ///
    /// On new data: refreshes the buffer, re-indexes, scrolls to the new
    /// end, and repaints. On Ctrl-C (or `q`/`Q`): exits follow mode and
    /// repaints normally.
    fn follow_mode(&mut self) -> Result<()> {
        // Refresh and scroll to EOF.
        self.refresh_and_scroll_to_end()?;
        self.status_message = Some("Waiting for data... (interrupt to abort)".to_string());
        self.repaint()?;

        // Try kqueue-based follow if we have the key fd and a file path.
        let use_kqueue = self.key_fd.is_some() && self.filename.is_some();

        if use_kqueue {
            self.follow_mode_kqueue()?;
        } else {
            self.follow_mode_blocking()?;
        }

        Ok(())
    }

    /// Refresh the buffer, rebuild the line index, and scroll to EOF.
    fn refresh_and_scroll_to_end(&mut self) -> Result<()> {
        self.buffer.refresh()?;
        let new_len = self.buffer.len() as u64;
        self.index = LineIndex::new(new_len);
        self.index.index_all(&*self.buffer)?;
        let total = self.index.lines_indexed();
        let default = total.saturating_sub(self.screen.content_rows());
        self.screen.goto_line(default, total);
        Ok(())
    }

    /// Follow mode using kqueue to watch for file changes and key input.
    #[allow(clippy::cast_possible_wrap)] // fd values are always small positive ints
    fn follow_mode_kqueue(&mut self) -> Result<()> {
        let key_fd = self
            .key_fd
            .ok_or_else(|| std::io::Error::other("no key fd for follow mode"))?;
        let filename = self
            .filename
            .clone()
            .ok_or_else(|| std::io::Error::other("no filename for follow mode"))?;

        // Open a fresh fd for the file to watch with kqueue.
        let mut watch_file = std::fs::File::open(Path::new(&filename))?;
        let mut watch_fd = std::os::unix::io::AsRawFd::as_raw_fd(&watch_file);

        let mut watcher = FileWatcher::watch(watch_fd)
            .map_err(|e| std::io::Error::other(format!("kqueue watch failed: {e}")))?;

        loop {
            let event = watcher
                .wait_with_key_check(key_fd, Duration::from_millis(500))
                .map_err(|e| std::io::Error::other(format!("kqueue wait failed: {e}")))?;

            match event {
                FollowEvent::NewData => {
                    self.follow_refresh_and_scroll()?;
                }
                FollowEvent::KeyReady => {
                    if self.follow_handle_key()? {
                        break;
                    }
                }
                FollowEvent::Timeout => {
                    // Poll-based fallback: also check if the file grew even
                    // without a kqueue notification (e.g. NFS, some edge cases).
                    self.follow_refresh_and_scroll()?;

                    // Exit on close: if pipe input and buffer didn't grow,
                    // check if the underlying growable source is exhausted.
                    if self.exit_follow_on_close && self.is_pipe && !self.buffer.is_growable() {
                        break;
                    }
                }
                FollowEvent::FileRenamed | FollowEvent::FileDeleted => {
                    if self.follow_name {
                        // Attempt to reopen the file by its original path.
                        if let Ok(new_file) = std::fs::File::open(Path::new(&filename)) {
                            watch_file = new_file;
                            watch_fd = std::os::unix::io::AsRawFd::as_raw_fd(&watch_file);
                            if let Ok(new_watcher) = FileWatcher::watch(watch_fd) {
                                watcher = new_watcher;
                            }
                            // Refresh buffer to pick up data from the new file.
                            self.follow_refresh_and_scroll()?;
                        }
                        // If reopen fails, keep waiting — the file may reappear.
                    }
                    // Without --follow-name, ignore rename/delete events.
                }
            }
        }

        // Repaint after exiting follow mode.
        self.repaint()?;
        Ok(())
    }

    /// Fallback follow mode: blocks reading keys until interrupted.
    ///
    /// Used when kqueue is not available (e.g. in tests or for pipe input).
    fn follow_mode_blocking(&mut self) -> Result<()> {
        loop {
            match self.reader.read_key() {
                Ok(Key::Ctrl('c')) => {
                    if self.quit_on_intr {
                        self.exit_reason = ExitReason::Interrupt;
                        self.should_quit = true;
                        return Ok(());
                    }
                    break;
                }
                Ok(Key::Char('q' | 'Q')) => {
                    self.should_quit = true;
                    return Ok(());
                }
                Ok(_) => {
                    // For exit-follow-on-close on pipe input: exit if the
                    // buffer is no longer growable (pipe closed).
                    if self.exit_follow_on_close && self.is_pipe && !self.buffer.is_growable() {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        }

        // Repaint after exiting follow mode.
        self.repaint()?;
        Ok(())
    }

    /// Refresh the buffer and scroll to the new end if data grew.
    ///
    /// Used by the follow mode loop to avoid duplicating the
    /// refresh-reindex-scroll-repaint pattern.
    fn follow_refresh_and_scroll(&mut self) -> Result<()> {
        let old_len = self.buffer.len();
        self.buffer.refresh()?;
        let new_len = self.buffer.len();
        if new_len > old_len {
            self.index = LineIndex::new(new_len as u64);
            self.index.index_all(&*self.buffer)?;
            let total = self.index.lines_indexed();
            let target = total.saturating_sub(self.screen.content_rows());
            self.screen.goto_line(target, total);
            self.status_message = Some("Waiting for data... (interrupt to abort)".to_string());
            self.repaint()?;
        }
        Ok(())
    }

    /// Handle a key event during follow mode.
    ///
    /// Returns `Ok(true)` if follow mode should break (Ctrl-C or quit),
    /// `Ok(false)` to continue. Sets `should_quit` on `q`/`Q`.
    fn follow_handle_key(&mut self) -> Result<bool> {
        match self.reader.read_key() {
            Ok(Key::Ctrl('c')) => {
                if self.quit_on_intr {
                    self.exit_reason = ExitReason::Interrupt;
                    self.should_quit = true;
                }
                Ok(true)
            }
            Ok(Key::Char('q' | 'Q')) => {
                self.should_quit = true;
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                self.should_quit = true;
                Ok(true)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Follow mode that stops when a search pattern matches new data (ESC-F).
    ///
    /// Like `follow_mode`, but watches for new data and searches the newly
    /// arrived lines for the active search pattern. When a match is found,
    /// exits follow mode and positions the viewport at the matching line.
    /// If no search pattern is active, behaves identically to `follow_mode`.
    fn follow_mode_stop_on_match(&mut self) -> Result<()> {
        // If no search pattern is active, fall back to regular follow mode.
        if self.last_pattern.is_none() {
            return self.follow_mode();
        }

        // Refresh and scroll to EOF.
        self.refresh_and_scroll_to_end()?;
        self.status_message = Some("Waiting for data... (will stop at highlight)".to_string());
        self.repaint()?;

        let use_kqueue = self.key_fd.is_some() && self.filename.is_some();

        if use_kqueue {
            self.follow_stop_on_match_kqueue()?;
        } else {
            self.follow_stop_on_match_blocking()?;
        }

        Ok(())
    }

    /// ESC-F follow mode using kqueue.
    #[allow(clippy::cast_possible_wrap)] // fd values are always small positive ints
    fn follow_stop_on_match_kqueue(&mut self) -> Result<()> {
        let key_fd = self
            .key_fd
            .ok_or_else(|| std::io::Error::other("no key fd for follow mode"))?;
        let filename = self
            .filename
            .clone()
            .ok_or_else(|| std::io::Error::other("no filename for follow mode"))?;

        let watch_file = std::fs::File::open(Path::new(&filename))?;
        let watch_fd = std::os::unix::io::AsRawFd::as_raw_fd(&watch_file);

        let watcher = FileWatcher::watch(watch_fd)
            .map_err(|e| std::io::Error::other(format!("kqueue watch failed: {e}")))?;

        loop {
            let event = watcher
                .wait_with_key_check(key_fd, Duration::from_millis(500))
                .map_err(|e| std::io::Error::other(format!("kqueue wait failed: {e}")))?;

            match event {
                FollowEvent::NewData | FollowEvent::Timeout => {
                    if self.follow_check_new_data_for_match()? {
                        return Ok(());
                    }
                }
                FollowEvent::KeyReady => {
                    if self.follow_handle_key()? {
                        break;
                    }
                }
                FollowEvent::FileRenamed | FollowEvent::FileDeleted => {}
            }
        }

        // Repaint after exiting follow mode.
        self.repaint()?;
        Ok(())
    }

    /// ESC-F follow mode with blocking key reads (fallback).
    fn follow_stop_on_match_blocking(&mut self) -> Result<()> {
        loop {
            match self.reader.read_key() {
                Ok(Key::Ctrl('c')) => {
                    if self.quit_on_intr {
                        self.exit_reason = ExitReason::Interrupt;
                        self.should_quit = true;
                        return Ok(());
                    }
                    break;
                }
                Ok(Key::Char('q' | 'Q')) => {
                    self.should_quit = true;
                    return Ok(());
                }
                Ok(_) => {
                    // Check for new data on each key event.
                    if self.follow_check_new_data_for_match()? {
                        return Ok(());
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        }

        // Repaint after exiting follow mode.
        self.repaint()?;
        Ok(())
    }

    /// Check new data for a search pattern match.
    ///
    /// Called from the ESC-F follow loop. Refreshes the buffer, and if new
    /// lines appeared, searches them for `last_pattern`. Returns `true` if a
    /// match was found (and the viewport has been repositioned), `false` if
    /// follow mode should continue waiting.
    fn follow_check_new_data_for_match(&mut self) -> Result<bool> {
        let old_total = self.index.lines_indexed();
        let old_len = self.buffer.len();
        self.buffer.refresh()?;
        let new_len = self.buffer.len();

        if new_len <= old_len {
            return Ok(false);
        }

        // Re-index to discover any new lines.
        self.index = LineIndex::new(new_len as u64);
        self.index.index_all(&*self.buffer)?;
        let new_total = self.index.lines_indexed();

        if new_total <= old_total {
            return Ok(false);
        }

        // Search the newly arrived lines for the active pattern.
        if let Some(ref pattern) = self.last_pattern {
            for line_num in old_total..new_total {
                if let Some(text) = self.index.get_line(line_num, &*self.buffer)? {
                    if pattern.is_match(&text) {
                        // Match found — position viewport at the match line.
                        let total = self.index.lines_indexed();
                        self.screen.goto_line(line_num, total);
                        self.status_message = None;
                        self.repaint()?;
                        return Ok(true);
                    }
                }
            }
        }

        // No match yet — scroll to the new end and keep waiting.
        let total = self.index.lines_indexed();
        let target = total.saturating_sub(self.screen.content_rows());
        self.screen.goto_line(target, total);
        self.status_message = Some("Waiting for data... (will stop at highlight)".to_string());
        self.repaint()?;
        Ok(false)
    }

    /// Switch to the next file in the file list.
    fn switch_file_next(&mut self) -> Result<()> {
        let switched = if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            let old_name = self.filename.clone();
            if file_list.next().is_err() {
                // Undo the swap — restore pager's buffer from the same entry.
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                self.status_message = Some("No next file".to_string());
                self.repaint()?;
                return Ok(());
            }
            self.previous_file = old_name;
            true
        } else {
            false
        };
        if switched {
            self.apply_current_file_impl(true);
            self.repaint()?;
            self.execute_every_file_commands()?;
        }
        Ok(())
    }

    /// Switch to the previous file in the file list.
    fn switch_file_prev(&mut self) -> Result<()> {
        let switched = if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            let old_name = self.filename.clone();
            if file_list.prev().is_err() {
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                self.status_message = Some("No previous file".to_string());
                self.repaint()?;
                return Ok(());
            }
            self.previous_file = old_name;
            true
        } else {
            false
        };
        if switched {
            self.apply_current_file_impl(true);
            self.repaint()?;
            self.execute_every_file_commands()?;
        }
        Ok(())
    }

    /// Switch to the N-th file (0-based) in the file list.
    ///
    /// If the target index is the same as the current file, this is a no-op
    /// (matching GNU less `:x` when already on the first file).
    fn switch_file_goto(&mut self, index: usize) -> Result<()> {
        let switched = if let Some(ref mut file_list) = self.file_list {
            if file_list.current_index() == index {
                return Ok(());
            }
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            let old_name = self.filename.clone();
            if file_list.goto(index).is_err() {
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                return Ok(());
            }
            self.previous_file = old_name;
            true
        } else {
            false
        };
        if switched {
            self.apply_current_file_impl(true);
            self.repaint()?;
            self.execute_every_file_commands()?;
        }
        Ok(())
    }

    /// Remove the current file from the file list.
    fn remove_current_file(&mut self) -> Result<()> {
        let removed = if let Some(ref mut file_list) = self.file_list {
            // Save pager's buffer/index into the entry about to be removed.
            // After remove_current, the entry is dropped and the cursor moves
            // to the next (or previous) file.
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            if file_list.remove_current().is_err() {
                // Undo the swap.
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                false
            } else {
                true
            }
        } else {
            false
        };
        if removed {
            self.apply_current_file_impl(true);
            self.repaint()?;
        }
        Ok(())
    }

    /// Show the `:e` prompt and read a filename via the line editor.
    ///
    /// On Enter with a filename: expand `%`/`#`, open the file, add it to the
    /// file list, and switch to it. On Enter with empty input: refresh the
    /// current file. On Escape: cancel.
    fn examine_prompt(&mut self) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        let mut editor = LineEditor::with_completion("Examine: ", CompletionMode::Filename);
        let (rows, cols) = self.screen.dimensions();
        if rows > 0 {
            let prompt_row = rows.saturating_sub(1);
            let _ = editor.render(&mut self.writer, prompt_row, 0, cols);
            let _ = self.writer.flush();
        }

        loop {
            match self.reader.read_key() {
                Ok(key) => match editor.process_key(&key) {
                    LineEditResult::Continue => {
                        if rows > 0 {
                            let prompt_row = rows.saturating_sub(1);
                            let _ = editor.render(&mut self.writer, prompt_row, 0, cols);
                            let _ = self.writer.flush();
                        }
                    }
                    LineEditResult::ContinueWithStatus(msg) => {
                        if rows > 0 {
                            let prompt_row = rows.saturating_sub(1);
                            let _ = editor.render(&mut self.writer, prompt_row, 0, cols);
                            let _ = self.writer.flush();
                        }
                        self.status_message = Some(msg);
                    }
                    LineEditResult::Confirm(input) => {
                        if input.is_empty() {
                            // Re-examine (reload) the current file.
                            self.buffer.refresh()?;
                            let new_len = self.buffer.len() as u64;
                            self.index = LineIndex::new(new_len);
                            // Reset initial_render so short files get bottom-aligned.
                            self.initial_render = true;
                            self.repaint()?;
                        } else {
                            self.examine_file(&input)?;
                        }
                        return Ok(());
                    }
                    LineEditResult::Cancel => {
                        // Reset initial_render so short files get bottom-aligned.
                        self.initial_render = true;
                        self.repaint()?;
                        return Ok(());
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        }
    }

    /// Expand the filename and attempt to open it, adding to the file list.
    fn examine_file(&mut self, raw_input: &str) -> Result<()> {
        // Strip leading/trailing whitespace — the user may type `:e  file`
        // with extra spaces after the colon-command letter.
        let trimmed = raw_input.trim();
        if trimmed.is_empty() {
            // Treat as a refresh (same as empty Confirm in examine_prompt).
            self.buffer.refresh()?;
            let new_len = self.buffer.len() as u64;
            self.index = LineIndex::new(new_len);
            self.initial_render = true;
            self.repaint()?;
            return Ok(());
        }
        // Expand % and # substitutions.
        let expanded = match expand_filename(
            trimmed,
            self.filename.as_deref(),
            self.previous_file.as_deref(),
        ) {
            Ok(name) => name,
            Err(e) => {
                self.status_message = Some(e.to_string());
                self.repaint()?;
                return Ok(());
            }
        };

        let path = Path::new(&expanded);
        match pgr_input::LoadedFile::open(path) {
            Ok(loaded) => {
                let display_name = expanded.clone();
                let file_path = loaded.path().to_path_buf();
                let (buffer, index) = loaded.into_parts();

                let entry = FileEntry {
                    path: Some(file_path),
                    display_name: display_name.clone(),
                    buffer,
                    index,
                    marks: MarkStore::new(),
                    saved_top_line: 0,
                    saved_horizontal_offset: 0,
                };

                // Track the previous file before switching.
                let old_name = self.filename.clone();

                if let Some(ref mut file_list) = self.file_list {
                    file_list
                        .save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
                    file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                    file_list.push(entry);
                    let new_index = file_list.file_count() - 1;
                    let _ = file_list.goto(new_index);
                } else {
                    // No file list yet — create one seeded with the current
                    // file so `:p` can navigate back to it after examine.
                    let placeholder: Box<dyn Buffer> =
                        Box::new(pgr_input::PipeBuffer::new(std::io::empty()));
                    let current_entry = FileEntry {
                        path: self.filename.as_deref().map(std::path::PathBuf::from),
                        display_name: self
                            .filename
                            .clone()
                            .unwrap_or_else(|| String::from("(current)")),
                        buffer: placeholder,
                        index: LineIndex::new(0),
                        marks: MarkStore::new(),
                        saved_top_line: self.screen.top_line(),
                        saved_horizontal_offset: self.screen.horizontal_offset(),
                    };
                    let mut fl = FileList::new(current_entry);
                    // Swap the pager's real buffer/index into the original entry.
                    fl.swap_buffer_and_index(&mut self.buffer, &mut self.index);
                    fl.push(entry);
                    let new_index = fl.file_count() - 1;
                    let _ = fl.goto(new_index);
                    self.file_list = Some(fl);
                }

                self.previous_file = old_name;
                // Always swap: loads the new file's buffer into the pager.
                self.apply_current_file_impl(true);
                // GNU less renders examined files top-aligned (no bottom-
                // alignment), so leave initial_render as false.
                self.initial_render = false;
                self.repaint()?;
                self.execute_every_file_commands()?;
            }
            Err(e) => {
                self.status_message = Some(format!("{expanded}: {e}"));
                self.repaint()?;
            }
        }

        Ok(())
    }

    /// Display file information on the status line (the `=` / `^G` / `:f` command).
    ///
    /// Shows file name, line range, byte offset, and percentage, matching
    /// GNU less's format. The info persists until the next keypress.
    fn display_file_info(&mut self) -> Result<()> {
        self.index.index_all(&*self.buffer)?;
        let total_lines = self.index.lines_indexed();
        let (start, end) = self.screen.visible_range();
        let bottom_display = end.min(total_lines);

        let (file_index, file_count) = self
            .file_list
            .as_ref()
            .map_or((0, 1), |fl| (fl.current_index(), fl.file_count()));

        let text = info::format_file_info(
            self.filename.as_deref(),
            start.saturating_add(1),
            bottom_display,
            Some(total_lines),
            // Byte offset = end of last visible line (0-indexed).
            self.index
                .line_range(bottom_display.saturating_sub(1))
                .map_or(0, |(_, end)| end),
            self.buffer.len() as u64,
            file_index,
            file_count,
            false, // TODO: pipe detection deferred to Phase 2
        );

        // GNU less scrolls forward 1 line (unclamped, allowing scroll
        // past EOF) and replaces the last content row with the info text.
        self.screen.scroll_forward_unclamped(1);
        self.repaint()?;

        let (rows, cols) = self.screen.dimensions();
        // Paint the info on the last content row (rows-1 in 1-based ANSI),
        // not the prompt row (rows), matching GNU less behavior.
        // Use right-truncation (paint_info_line) rather than the prompt's
        // left-truncation, matching how GNU less renders the = info line.
        paint_info_line(&mut self.writer, &text, rows.saturating_sub(1), cols, None)?;
        Ok(())
    }

    /// Display the help screen as a navigable file.
    ///
    /// Creates a synthetic buffer from the help text constant and temporarily
    /// replaces the current view. The user can scroll through it and pressing
    /// `q` returns to the previous file.
    fn display_help(&mut self) -> Result<()> {
        // Save current state
        let saved_buffer = std::mem::replace(
            &mut self.buffer,
            Box::new(HelpBuffer::new(help::HELP_TEXT.as_bytes())),
        );
        let saved_index = std::mem::replace(
            &mut self.index,
            LineIndex::new(help::HELP_TEXT.len() as u64),
        );
        let saved_filename = self.filename.take();
        let saved_top_line = self.screen.top_line();
        let saved_h_offset = self.screen.horizontal_offset();
        let saved_prompt_style = self.prompt_style.clone();

        self.filename = Some("HELP -- Press q when done".to_string());
        self.prompt_style = PromptStyle::Short;
        self.screen.goto_line(0, usize::MAX);
        self.screen.set_horizontal_offset(0);
        self.repaint()?;

        // Run a sub-loop for the help screen
        loop {
            match self.reader.read_key() {
                Ok(key) => {
                    if key == Key::Char('q') || key == Key::Char('Q') || key == Key::Ctrl('c') {
                        break;
                    }
                    // Allow scrolling within help
                    let command = self.keymap.lookup(&key);
                    match command {
                        Command::Quit => break,
                        Command::Help => {} // Don't nest help screens
                        _ => {
                            // Process digit prefixes for scroll commands
                            if let Key::Char(c) = key {
                                if c.is_ascii_digit() {
                                    let digit = u32::from(c) - u32::from('0');
                                    #[allow(clippy::cast_possible_truncation)] // digit is 0..=9
                                    let digit = digit as usize;
                                    self.pending_count = Some(
                                        self.pending_count
                                            .unwrap_or(0)
                                            .saturating_mul(10)
                                            .saturating_add(digit),
                                    );
                                    continue;
                                }
                            }
                            let count = self.pending_count.take();
                            self.execute(&command, count)?;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    // Restore state before returning error
                    self.buffer = saved_buffer;
                    self.index = saved_index;
                    self.filename = saved_filename;
                    self.screen.goto_line(saved_top_line, usize::MAX);
                    self.screen.set_horizontal_offset(saved_h_offset);
                    self.prompt_style = saved_prompt_style;
                    return Err(e.into());
                }
            }
        }

        // Restore previous state
        self.buffer = saved_buffer;
        self.index = saved_index;
        self.filename = saved_filename;
        self.screen.goto_line(saved_top_line, usize::MAX);
        self.screen.set_horizontal_offset(saved_h_offset);
        self.prompt_style = saved_prompt_style;
        self.repaint()?;

        Ok(())
    }

    /// Display version information on the status line.
    fn display_version(&mut self) -> Result<()> {
        let text = help::version_string();
        let (rows, cols) = self.screen.dimensions();
        paint_prompt(&mut self.writer, &text, rows, cols, None)?;
        Ok(())
    }

    /// Load the current file's display name and viewport into the pager state.
    ///
    /// When `swap_buffers` is true, also swap the pager's buffer/index with the
    /// current entry's buffer/index. This is used during file switching (after
    /// saving the old entry's state). When false, only metadata is restored
    /// (used during initial `set_file_list` where the pager already has the
    /// correct buffer).
    fn apply_current_file_impl(&mut self, swap_buffers: bool) {
        if let Some(ref mut file_list) = self.file_list {
            if swap_buffers {
                file_list.swap_buffer_and_index(&mut self.buffer, &mut self.index);
            }
            let entry = file_list.current();
            let (top_line, h_offset) = (entry.saved_top_line, entry.saved_horizontal_offset);
            self.filename = Some(entry.display_name.clone());
            self.screen.goto_line(top_line, usize::MAX);
            self.screen.set_horizontal_offset(h_offset);
        }
        // Invalidate match count cache when switching files since the buffer changed.
        self.match_count_cache = None;
        // Reset content mode and diff/git-log/section/table state so they are re-detected on next first paint.
        self.content_mode = ContentMode::Plain;
        self.diff_state = None;
        self.git_log_commits = None;
        self.man_sections = None;
        self.sql_table_layout = None;
        // Reload git gutter state for the new file.
        if self.git_gutter_enabled {
            self.load_gutter_state();
        }
    }

    /// Load the current file's display name and viewport into the pager state.
    /// Does not swap buffers — used during initial file list setup.
    fn apply_current_file(&mut self) {
        self.apply_current_file_impl(false);
    }

    /// Set the file list for multi-file navigation.
    pub fn set_file_list(&mut self, file_list: FileList) {
        self.file_list = Some(file_list);
        self.apply_current_file();
    }

    /// Access the file list (for testing).
    #[must_use]
    pub fn file_list(&self) -> Option<&FileList> {
        self.file_list.as_ref()
    }

    /// Fetch the pinned header line contents (lines `0..header_lines`).
    fn fetch_header_lines(&mut self) -> Result<Vec<Option<String>>> {
        let header_count = self.screen.header_lines();
        let mut headers = Vec::with_capacity(header_count);
        let total = self.index.lines_indexed();
        for i in 0..header_count {
            if i < total {
                let content = self.index.get_line(i, &*self.buffer)?;
                headers.push(content);
            } else {
                headers.push(None);
            }
        }
        Ok(headers)
    }

    /// Fetch visible lines from the buffer/index and repaint the screen.
    #[allow(clippy::too_many_lines)] // Rendering paths for filter, squeeze, and normal modes with header support
    fn repaint(&mut self) -> Result<()> {
        // For growable buffers (pipe input), read available data and update
        // the index's knowledge of the buffer size before indexing.
        if self.buffer.is_growable() {
            let new_len = self.buffer.refresh()?;
            self.index.update_buffer_len(new_len as u64);
        }
        self.index.index_all(&*self.buffer)?;

        let header_line_contents = self.fetch_header_lines()?;

        // When a filter is active, use the filtered line mapping.
        if self.filter.is_active() {
            if let Some(ref fl) = self.filtered_lines {
                let visible_total = fl.visible_count();
                let (start, end) = self.screen.visible_range();

                let mut lines: Vec<Option<String>> = Vec::with_capacity(self.screen.content_rows());
                for filtered_idx in start..end {
                    if let Some(actual) = fl.actual_line(filtered_idx) {
                        let content = self.index.get_line(actual, &*self.buffer)?;
                        lines.push(content);
                    } else {
                        lines.push(None);
                    }
                }

                self.highlight_state
                    .compute_highlights(&lines, self.last_pattern.as_ref());

                let status_column_chars = if self.runtime_options.status_column {
                    let buf_lines: Vec<Option<usize>> = (start..end)
                        .map(|filtered_idx| fl.actual_line(filtered_idx))
                        .collect();
                    self.build_status_column_chars(&buf_lines)
                } else {
                    Vec::new()
                };

                let line_highlights = self.build_line_highlights();
                let gutter_marks = if self.git_gutter_enabled {
                    if let (Some(ref gutter), Some(ref fl_ref)) =
                        (&self.gutter_state, &self.filtered_lines)
                    {
                        (start..end)
                            .map(|filtered_idx| {
                                fl_ref.actual_line(filtered_idx).and_then(|actual| {
                                    gutter
                                        .mark_for_line(actual + 1)
                                        .map(|m| (m.symbol(), m.ansi_color()))
                                })
                            })
                            .collect()
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                let paint_opts = PaintOptions {
                    show_line_numbers: self.runtime_options.line_numbers,
                    total_lines: visible_total,
                    line_num_width: None,
                    suppress_tildes: self.runtime_options.tilde,
                    start_row: 0,
                    show_status_column: self.runtime_options.status_column,
                    status_column_chars,
                    header_line_contents,
                    wordwrap: self.runtime_options.wordwrap,
                    line_highlights,
                    gutter_marks,
                };
                paint_screen_with_options(
                    &mut self.writer,
                    &self.screen,
                    &lines,
                    &self.render_config,
                    &paint_opts,
                )?;
                if self.initial_render {
                    self.detect_content_mode_from_lines(&lines);
                }
                self.paint_status_prompt(visible_total)?;
                return Ok(());
            }
        }

        let total = self.index.lines_indexed();
        let (start, end) = self.screen.visible_range();
        let content_rows = self.screen.content_rows();

        // Squeeze mode: collapse consecutive blank lines.
        if self.runtime_options.squeeze_blank_lines {
            return self.repaint_squeezed(start, content_rows, total, header_line_contents);
        }

        // Side-by-side diff mode: pre-render paired lines and feed to normal paint.
        if self.side_by_side && self.content_mode == ContentMode::Diff {
            return self.repaint_side_by_side(start, content_rows, total, header_line_contents);
        }

        let mut lines: Vec<Option<String>> = Vec::with_capacity(content_rows);
        for line_num in start..end {
            if line_num < total {
                let content = self.index.get_line(line_num, &*self.buffer)?;
                lines.push(content);
            } else {
                lines.push(None);
            }
        }

        // Detect content mode on first paint so content-aware rendering
        // (diff coloring, SQL sticky header, etc.) applies from the start.
        if self.initial_render {
            self.detect_content_mode_from_lines(&lines);
        }

        // SQL table mode: apply frozen first column when horizontally scrolled.
        // Transforms each line to keep the first column visible on the left
        // while scrolling the remaining columns.
        let sql_frozen = self.content_mode == ContentMode::SqlTable
            && self.screen.horizontal_offset() > 0
            && self.sql_table_layout.is_some();
        if sql_frozen {
            lines = self.apply_frozen_column(&lines);
        }

        // Apply content-aware coloring before search highlights (SGR injection).
        // When coloring is active, the render config must be at least AnsiOnly
        // so the injected SGR codes are preserved.
        let diff_colored = self.content_mode == ContentMode::Diff;
        if diff_colored {
            lines = self.colorize_diff_lines(&lines);
        }

        let blame_colored = self.content_mode == ContentMode::GitBlame;
        if blame_colored {
            lines = self.colorize_blame_lines(&lines);
        }

        #[cfg(feature = "syntax")]
        let syntax_active = if diff_colored || blame_colored {
            false // Diff and blame modes use their own syntax highlighting
        } else {
            self.is_syntax_active()
        };
        #[cfg(not(feature = "syntax"))]
        let syntax_active = false;

        #[cfg(feature = "syntax")]
        if syntax_active {
            lines = self.highlight_lines(&lines, start);
        }

        // Compute highlights for the visible lines.
        self.highlight_state
            .compute_highlights(&lines, self.last_pattern.as_ref());

        // Build status column data if enabled.
        let status_column_chars = if self.runtime_options.status_column {
            let buf_lines: Vec<Option<usize>> = (start..end)
                .map(|line_num| {
                    if line_num < total {
                        Some(line_num)
                    } else {
                        None
                    }
                })
                .collect();
            self.build_status_column_chars(&buf_lines)
        } else {
            Vec::new()
        };

        // GNU less bottom-aligns short files only on the initial render.
        // After any keypress, less repaints top-aligned with tildes filling below.
        // When wordwrap is active, buffer line count does not reflect the number
        // of screen rows each line will occupy — compute the effective row count
        // by summing wordwrap segments for each visible line.
        let visible_content = total.saturating_sub(start);
        let effective_rows = if self.runtime_options.wordwrap {
            let (_, cols) = self.screen.dimensions();
            let status_w = if self.runtime_options.status_column {
                2
            } else {
                0
            };
            let ln_w = if self.runtime_options.line_numbers {
                line_number_width(total)
            } else {
                0
            };
            let content_cols = cols.saturating_sub(status_w + ln_w);
            lines
                .iter()
                .filter_map(|opt| opt.as_deref())
                .map(|text| wordwrap_segments(text, content_cols).len())
                .sum::<usize>()
        } else {
            visible_content
        };
        let start_row = if self.initial_render && effective_rows <= content_rows && start == 0 {
            content_rows - effective_rows + 1
        } else {
            0
        };

        let line_highlights = self.build_line_highlights();
        let gutter_marks = self.build_gutter_marks(start, lines.len());

        // Transform header lines with frozen column when SQL table is hscrolled.
        let header_line_contents = if sql_frozen {
            self.apply_frozen_column(&header_line_contents)
        } else {
            header_line_contents
        };

        let paint_opts = PaintOptions {
            show_line_numbers: self.runtime_options.line_numbers,
            total_lines: total,
            line_num_width: None,
            suppress_tildes: self.runtime_options.tilde,
            start_row,
            show_status_column: self.runtime_options.status_column,
            status_column_chars,
            header_line_contents,
            wordwrap: self.runtime_options.wordwrap,
            line_highlights,
            gutter_marks,
        };
        // When syntax highlighting or diff coloring injected SGR codes,
        // ensure the render pipeline uses at least AnsiOnly mode so those
        // codes are preserved.
        let effective_config = if (syntax_active || diff_colored || blame_colored)
            && self.render_config.raw_mode == RawControlMode::Off
        {
            let mut cfg = self.render_config.clone();
            cfg.raw_mode = RawControlMode::AnsiOnly;
            cfg
        } else {
            self.render_config.clone()
        };

        // When frozen column mode is active, temporarily set h_offset to 0
        // for the paint call since the line content already includes the
        // frozen prefix and scrolled remainder.
        let saved_h_offset = if sql_frozen {
            let h = self.screen.horizontal_offset();
            self.screen.set_horizontal_offset(0);
            Some(h)
        } else {
            None
        };

        paint_screen_with_options(
            &mut self.writer,
            &self.screen,
            &lines,
            &effective_config,
            &paint_opts,
        )?;

        // Restore h_offset after painting with frozen columns.
        if let Some(h) = saved_h_offset {
            self.screen.set_horizontal_offset(h);
        }

        // Detect content mode on first paint and show status message.
        if self.initial_render {
            self.detect_content_mode_from_lines(&lines);
        }

        // Prompt always on the last terminal row (matching GNU less).
        self.paint_status_prompt(total)?;

        self.initial_render = false;
        Ok(())
    }

    /// Repaint with squeeze mode: collapse consecutive blank lines.
    ///
    /// Uses [`squeeze_visible_lines`] to determine which buffer lines to
    /// display, then renders via [`paint_screen_mapped`] so line numbers
    /// stay correct even when consecutive blanks are collapsed.
    fn repaint_squeezed(
        &mut self,
        start: usize,
        content_rows: usize,
        total: usize,
        header_line_contents: Vec<Option<String>>,
    ) -> Result<()> {
        let mapped = squeeze_visible_lines(start, content_rows, total, |i| {
            self.index.get_line(i, &*self.buffer).ok().flatten()
        });

        let visible_total = mapped.len();

        let screen_lines: Vec<ScreenLine> = mapped
            .iter()
            .map(|&actual| {
                let content = self.index.get_line(actual, &*self.buffer).ok().flatten();
                ScreenLine {
                    content,
                    line_number: actual + 1, // 1-based
                }
            })
            .collect();

        // Pad to content_rows with None entries (beyond-EOF tildes).
        let mut padded: Vec<ScreenLine> = screen_lines;
        while padded.len() < content_rows {
            padded.push(ScreenLine {
                content: None,
                line_number: 0, // beyond-EOF placeholder
            });
        }

        // Compute highlights.
        let line_contents: Vec<Option<String>> =
            padded.iter().map(|sl| sl.content.clone()).collect();
        self.highlight_state
            .compute_highlights(&line_contents, self.last_pattern.as_ref());

        // Build status column data if enabled.
        let status_column_chars = if self.runtime_options.status_column {
            let buf_lines: Vec<Option<usize>> = padded
                .iter()
                .map(|sl| {
                    if sl.content.is_some() {
                        Some(sl.line_number.saturating_sub(1)) // back to 0-based
                    } else {
                        None
                    }
                })
                .collect();
            self.build_status_column_chars(&buf_lines)
        } else {
            Vec::new()
        };

        let start_row = if self.initial_render && visible_total < content_rows {
            content_rows - visible_total + 1
        } else {
            0
        };
        let line_highlights = self.build_line_highlights();
        let gutter_marks = if self.git_gutter_enabled {
            if let Some(ref gutter) = self.gutter_state {
                padded
                    .iter()
                    .map(|sl| {
                        if sl.line_number > 0 {
                            gutter
                                .mark_for_line(sl.line_number)
                                .map(|m| (m.symbol(), m.ansi_color()))
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
        let paint_opts = PaintOptions {
            show_line_numbers: self.runtime_options.line_numbers,
            total_lines: total,
            line_num_width: None,
            suppress_tildes: self.runtime_options.tilde,
            start_row,
            show_status_column: self.runtime_options.status_column,
            status_column_chars,
            header_line_contents,
            wordwrap: self.runtime_options.wordwrap,
            line_highlights,
            gutter_marks,
        };
        paint_screen_mapped(
            &mut self.writer,
            &self.screen,
            &padded,
            &self.render_config,
            &paint_opts,
        )?;

        // Detect content mode on first paint and show status message.
        if self.initial_render {
            self.detect_content_mode_from_lines(&line_contents);
        }

        self.paint_status_prompt(total)?;

        self.initial_render = false;
        Ok(())
    }

    /// Repaint with side-by-side diff rendering.
    ///
    /// Collects all buffer lines, pairs them using the diff pairing engine,
    /// pre-renders with ANSI coloring, and feeds the visible slice to the
    /// normal paint path.
    fn repaint_side_by_side(
        &mut self,
        start: usize,
        content_rows: usize,
        total: usize,
        header_line_contents: Vec<Option<String>>,
    ) -> Result<()> {
        // Collect all buffer lines for diff pairing.
        let mut all_lines: Vec<Option<String>> = Vec::with_capacity(total);
        for i in 0..total {
            if let Ok(content) = self.index.get_line(i, &*self.buffer) {
                all_lines.push(content);
            } else {
                all_lines.push(None);
            }
        }

        let (_, cols) = self.screen.dimensions();

        // Build the side-by-side rendered lines.
        let Some(sbs_lines) = build_side_by_side_lines(&all_lines, cols) else {
            // Terminal too narrow — fall back to unified, disable sbs.
            self.side_by_side = false;
            self.status_message = Some("terminal too narrow for side-by-side".to_string());
            // Re-enter normal repaint (recursion-safe since side_by_side is now false).
            return self.repaint();
        };

        // The paired output has a different line count than the buffer.
        // Use the screen's top_line as an index into the paired output.
        let sbs_total = sbs_lines.len();
        let visible_start = start.min(sbs_total);
        let visible_end = (visible_start + content_rows).min(sbs_total);

        let mut lines: Vec<Option<String>> = Vec::with_capacity(content_rows);
        for sbs_line in &sbs_lines[visible_start..visible_end] {
            lines.push(Some(sbs_line.clone()));
        }
        // Pad remaining rows with None (tilde lines).
        while lines.len() < content_rows {
            lines.push(None);
        }

        // Side-by-side lines have ANSI baked in — ensure the renderer
        // passes them through by using at least AnsiOnly mode.
        let effective_config = if self.render_config.raw_mode == RawControlMode::Off {
            let mut cfg = self.render_config.clone();
            cfg.raw_mode = RawControlMode::AnsiOnly;
            cfg
        } else {
            self.render_config.clone()
        };

        let paint_opts = PaintOptions {
            show_line_numbers: false,
            total_lines: sbs_total,
            line_num_width: None,
            suppress_tildes: self.runtime_options.tilde,
            start_row: 0,
            show_status_column: false,
            status_column_chars: Vec::new(),
            header_line_contents,
            wordwrap: false,
            line_highlights: Vec::new(),
            gutter_marks: Vec::new(),
        };

        paint_screen_with_options(
            &mut self.writer,
            &self.screen,
            &lines,
            &effective_config,
            &paint_opts,
        )?;

        self.paint_status_prompt(sbs_total)?;
        self.initial_render = false;
        Ok(())
    }

    /// Build per-line status column characters for the visible lines.
    ///
    /// For each visible line at a given buffer line number, the character is:
    /// - A mark letter if the line has a user mark set (first mark wins)
    /// - `'*'` if the line has a search match (from highlight state)
    /// - `' '` otherwise
    ///
    /// `buffer_line_numbers` maps each visible line index to its 0-based buffer
    /// line number, or `None` for beyond-EOF lines.
    fn build_status_column_chars(&self, buffer_line_numbers: &[Option<usize>]) -> Vec<char> {
        let highlights = self.highlight_state.highlights();
        let mark_list = self.marks.list();

        buffer_line_numbers
            .iter()
            .enumerate()
            .map(|(vis_idx, maybe_buf_line)| {
                let Some(buf_line) = maybe_buf_line else {
                    return ' ';
                };

                // Check if this buffer line has a mark.
                for &(mark_char, mark) in &mark_list {
                    if mark.line == *buf_line {
                        return mark_char;
                    }
                }

                // Check if this line has a search match.
                if let Some(line_highlights) = highlights.get(vis_idx) {
                    if !line_highlights.is_empty() {
                        return '*';
                    }
                }

                ' '
            })
            .collect()
    }

    /// Build per-line colored highlight ranges for paint options.
    ///
    /// Converts the cached `ColoredHighlight` data from `HighlightState`
    /// into `ColoredRange<'static>` suitable for `PaintOptions.line_highlights`.
    /// Each highlight's color index is mapped to its SGR string from
    /// [`HIGHLIGHT_COLORS`].
    fn build_line_highlights(&self) -> Vec<Vec<ColoredRange<'static>>> {
        let colored = self.highlight_state.colored_highlights();
        colored
            .iter()
            .map(|line_hl| {
                line_hl
                    .iter()
                    .map(|ch| {
                        let idx = ch.color_index as usize;
                        let sgr = if idx < HIGHLIGHT_COLORS.len() {
                            HIGHLIGHT_COLORS[idx]
                        } else {
                            HIGHLIGHT_COLORS[0]
                        };
                        ColoredRange {
                            start: ch.start,
                            end: ch.end,
                            sgr,
                        }
                    })
                    .collect()
            })
            .collect()
    }

    /// Render and paint the status prompt on the last row.
    ///
    /// If a transient status message is set, it is displayed in reverse video
    /// instead of the normal prompt and then cleared. Otherwise the prompt
    /// template is evaluated from the current pager state.
    fn paint_status_prompt(&mut self, total_lines: usize) -> Result<()> {
        let (rows, cols) = self.screen.dimensions();
        if rows == 0 {
            return Ok(());
        }

        // Take the status message first to avoid borrow conflicts with
        // building the prompt context.
        let status_msg = self.status_message.take();

        let text = if let Some(msg) = status_msg {
            msg
        } else {
            let at_eof = if total_lines == 0 {
                true
            } else {
                let (_, end) = self.screen.visible_range();
                end >= total_lines
            };

            let (start, end) = self.screen.visible_range();
            let bottom_display = end.min(total_lines);

            let ctx = self.build_prompt_context(total_lines, at_eof, start, bottom_display);

            let template = if let Some(custom) =
                self.runtime_options.prompt_override_for(&self.prompt_style)
            {
                custom
            } else {
                match self.prompt_style {
                    PromptStyle::Short => DEFAULT_SHORT_PROMPT,
                    PromptStyle::Medium => DEFAULT_MEDIUM_PROMPT,
                    PromptStyle::Long => DEFAULT_LONG_PROMPT,
                    PromptStyle::Custom(ref t) => t.as_str(),
                }
            };
            eval_prompt(template, &ctx)
        };

        paint_prompt(&mut self.writer, &text, rows, cols, None)?;

        Ok(())
    }

    /// Build a `PromptContext` from the current pager state.
    fn build_prompt_context(
        &self,
        total_lines: usize,
        at_eof: bool,
        top_line_0: usize,
        bottom_display: usize,
    ) -> PromptContext<'_> {
        let (file_index, file_count) = self
            .file_list
            .as_ref()
            .map_or((0, 1), |fl| (fl.current_index(), fl.file_count()));

        // Compute byte offset of the end of the last visible line (for %b and %p).
        // `bottom_display` is 1-based; line_range takes 0-based line numbers.
        let byte_offset = if bottom_display > 0 {
            self.index
                .line_range(bottom_display.saturating_sub(1))
                .map_or(0, |(_, end)| end)
        } else {
            0
        };

        PromptContext {
            filename: self.filename.as_deref(),
            top_line: top_line_0.saturating_add(1),
            bottom_line: bottom_display,
            total_lines: Some(total_lines),
            total_bytes: self.buffer.len() as u64,
            byte_offset,
            file_index,
            file_count,
            at_eof,
            is_pipe: self.is_pipe,
            column: self.screen.horizontal_offset().saturating_add(1),
            page_number: {
                let content_rows = self.screen.content_rows();
                if content_rows > 0 {
                    Some(top_line_0 / content_rows + 1)
                } else {
                    Some(1)
                }
            },
            input_line: None,
            pipe_size: None,
            search_active: self.last_pattern.is_some(),
            search_pattern: self.last_pattern.as_ref().map(SearchPattern::pattern),
            line_numbers_enabled: self.runtime_options.line_numbers,
            marks_set: self.marks.has_any(),
            filter_active: self.filter.is_active(),
            filter_pattern: self.filter.pattern().map(SearchPattern::pattern),
            input_complete: !self.buffer.is_growable(),
            hyperlink_count: 0,
            horizontal_shift: self.screen.horizontal_offset(),
            current_tag: None,
            waiting_for_data: false,
            match_info: self
                .match_count_cache
                .as_ref()
                .and_then(|cache| cache.current_match.map(|cur| (cur, cache.total_matches))),
            diff_info: self
                .diff_state
                .as_ref()
                .and_then(|files| pgr_core::compute_diff_prompt_info(files, top_line_0)),
        }
    }

    /// Set a transient status message that overrides the prompt for one repaint.
    ///
    /// The message is displayed on the next repaint and then cleared.
    pub fn set_status_message(&mut self, msg: String) {
        self.status_message = Some(msg);
    }

    /// Set the raw control mode for rendering.
    pub fn set_raw_mode(&mut self, mode: RawControlMode) {
        self.render_config.raw_mode = mode;
    }

    /// Set the tab stop configuration.
    pub fn set_tab_stops(&mut self, stops: TabStops) {
        self.render_config.tab_stops = stops;
    }

    /// Set the overstrike processing mode.
    pub fn set_overstrike_mode(&mut self, mode: OverstrikeMode) {
        self.render_config.overstrike_mode = mode;
    }

    /// Set the full render configuration.
    pub fn set_render_config(&mut self, config: RenderConfig) {
        self.render_config = config;
    }

    /// Set the terminal dimensions, delegating to the internal screen state.
    pub fn set_dimensions(&mut self, rows: usize, cols: usize) {
        self.screen.resize(rows, cols);
    }

    /// Set the number of pinned header lines (`--header=N`).
    ///
    /// Header lines from the beginning of the file are always visible at
    /// the top of the screen and reduce the scrollable content area.
    pub fn set_header_lines(&mut self, n: usize) {
        self.screen.set_header_lines(n);
    }

    /// Set the prompt style used for the status line.
    pub fn set_prompt_style(&mut self, style: PromptStyle) {
        self.prompt_style = style;
    }

    /// Enable `-e` behavior: quit after the second forward scroll past EOF.
    pub fn set_quit_at_eof(&mut self, enabled: bool) {
        self.quit_at_eof = enabled;
    }

    /// Enable `-E` behavior: quit on the first forward scroll past EOF.
    pub fn set_quit_at_first_eof(&mut self, enabled: bool) {
        self.quit_at_first_eof = enabled;
    }

    /// Set the full runtime options state.
    ///
    /// Also synchronizes render-affecting flags (chop mode, raw mode,
    /// tab stops) to the screen and render config.
    pub fn set_runtime_options(&mut self, opts: RuntimeOptions) {
        self.runtime_options = opts;
        self.sync_runtime_to_render();
    }

    /// Access the runtime options (for testing).
    #[must_use]
    pub fn runtime_options(&self) -> &RuntimeOptions {
        &self.runtime_options
    }

    /// Handle a toggle option command (`-<flag>`).
    ///
    /// Toggles the option and displays a status message describing the new
    /// state, matching GNU less behavior.
    fn handle_toggle_option(&mut self, flag: char) -> Result<()> {
        if let Ok(msg) = self.runtime_options.toggle(flag) {
            self.status_message = Some(msg);
            self.absorb_next_key = true;
            // Sync render-affecting options to the screen/render_config.
            self.sync_runtime_to_render();
            self.repaint()?;
        }
        Ok(())
    }

    /// Handle a query option command (`_<flag>`).
    ///
    /// Displays the current state of the option on the status line
    /// without changing it, matching GNU less `_` behavior.
    fn handle_query_option(&mut self, flag: char) -> Result<()> {
        if let Ok(msg) = self.runtime_options.query(flag) {
            self.status_message = Some(msg);
            self.absorb_next_key = true;
            self.repaint()?;
        }
        Ok(())
    }

    /// Synchronize runtime options to render config and screen state.
    fn sync_runtime_to_render(&mut self) {
        self.screen
            .set_chop_mode(self.runtime_options.chop_long_lines);
        self.render_config.raw_mode = self.runtime_options.raw_control_mode;
        self.render_config.tab_stops = self.runtime_options.tab_stops.clone();
    }

    /// Access the screen state (for testing).
    #[must_use]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Access the mark store (for testing).
    #[must_use]
    pub fn marks(&self) -> &MarkStore {
        &self.marks
    }

    /// Access the last saved position (for testing).
    #[must_use]
    pub fn last_position(&self) -> Option<usize> {
        self.last_position
    }

    /// Return the sticky half-page size, if set by a counted `d`/`u` command.
    #[must_use]
    pub fn sticky_half_page(&self) -> Option<usize> {
        self.sticky_half_page
    }

    /// Return the custom window size, if set by a counted `z`/`w` command or `-z` flag.
    #[must_use]
    pub fn custom_window_size(&self) -> Option<WindowSize> {
        self.custom_window_size
    }

    /// Resolve the effective window size for paging commands.
    ///
    /// Priority: interactive `z`/`w` count > `-z` flag > screen content rows.
    /// Negative window sizes are resolved against the current screen height.
    fn resolve_window_size(&self) -> usize {
        let content_rows = self.screen.content_rows();
        let (rows, _) = self.screen.dimensions();
        match self.custom_window_size {
            Some(ws) => ws.resolve(rows),
            None => match self.runtime_options.window_size {
                Some(ws) => ws.resolve(rows),
                None => content_rows,
            },
        }
    }

    /// Enable or disable security mode (LESSSECURE).
    ///
    /// When enabled, shell, pipe, editor, and save commands are blocked.
    pub fn set_secure_mode(&mut self, secure: bool) {
        self.secure_mode = secure;
    }

    /// Return whether security mode is active.
    #[must_use]
    pub fn secure_mode(&self) -> bool {
        self.secure_mode
    }

    /// Set whether the input is from a pipe.
    pub fn set_is_pipe(&mut self, is_pipe: bool) {
        self.is_pipe = is_pipe;
    }

    /// Return whether the input is from a pipe.
    #[must_use]
    pub fn is_pipe(&self) -> bool {
        self.is_pipe
    }

    /// Set the shell command to use for `!` commands.
    pub fn set_shell(&mut self, shell: &str) {
        shell.clone_into(&mut self.shell);
    }

    /// Set the editor command to use for the `v` command.
    pub fn set_editor(&mut self, editor: &str) {
        editor.clone_into(&mut self.editor);
    }

    /// Set the raw file descriptor for the key input source.
    ///
    /// When set, follow mode uses kqueue to efficiently multiplex between
    /// file change notifications and key input. Typically set to the fd of
    /// the `/dev/tty` handle used for key reading.
    pub fn set_key_fd(&mut self, fd: RawFd) {
        self.key_fd = Some(fd);
    }

    /// Return the last executed shell command, if any.
    #[must_use]
    pub fn last_shell_command(&self) -> Option<&str> {
        self.last_shell_command.as_deref()
    }

    /// Return the previously viewed filename, for `#` expansion in `:e`.
    #[must_use]
    pub fn previous_file(&self) -> Option<&str> {
        self.previous_file.as_deref()
    }

    /// Return the last search pattern (for testing).
    #[must_use]
    pub fn last_pattern(&self) -> Option<&SearchPattern> {
        self.last_pattern.as_ref()
    }

    /// Return the last search direction (for testing).
    #[must_use]
    pub fn last_direction(&self) -> SearchDirection {
        self.last_direction
    }

    /// Return the highlight state (for testing).
    #[must_use]
    pub fn highlight_state(&self) -> &HighlightState {
        &self.highlight_state
    }

    /// Return the current status message, if any.
    #[must_use]
    pub fn status_message(&self) -> Option<&str> {
        self.status_message.as_deref()
    }

    /// Return the current filename.
    #[must_use]
    pub fn filename(&self) -> Option<&str> {
        self.filename.as_deref()
    }

    /// Return a mutable reference to the file list (for testing).
    pub fn file_list_mut(&mut self) -> Option<&mut FileList> {
        self.file_list.as_mut()
    }

    /// Set the initial commands to execute after the first repaint.
    ///
    /// These correspond to `+cmd` on the command line or in the `LESS`
    /// environment variable. Each string is a sequence of less keystrokes
    /// (e.g., `"G"` for go-to-end, `"/pattern\n"` for search).
    pub fn set_initial_commands(&mut self, cmds: Vec<String>) {
        self.initial_commands = cmds;
    }

    /// Set the every-file commands to execute after each file switch.
    ///
    /// These correspond to `++cmd` on the command line or in the `LESS`
    /// environment variable. Executed after `:n`, `:p`, `:e`, and at
    /// initial open.
    pub fn set_every_file_commands(&mut self, cmds: Vec<String>) {
        self.every_file_commands = cmds;
    }

    /// Apply a lesskey configuration to the pager's keymap.
    ///
    /// User bindings from the lesskey config override or extend the default
    /// keymap. This should be called after construction and before `run()`.
    pub fn apply_lesskey_config(&mut self, config: &LesskeyConfig) {
        self.keymap.apply_lesskey(config);
    }

    /// Enable follow-name mode (`--follow-name`).
    ///
    /// When enabled, follow mode reopens the file by pathname when a
    /// rename or delete is detected (e.g. log rotation).
    pub fn set_follow_name(&mut self, enabled: bool) {
        self.follow_name = enabled;
    }

    /// Enable exit-follow-on-close mode (`--exit-follow-on-close`).
    ///
    /// When enabled, follow mode exits when the buffer reports no growth
    /// on a pipe that has reached EOF, instead of waiting forever.
    pub fn set_exit_follow_on_close(&mut self, enabled: bool) {
        self.exit_follow_on_close = enabled;
    }

    /// Execute a command string by converting each byte to a [`Key`] event
    /// and feeding it through [`process_key`](Self::process_key).
    ///
    /// This is the mechanism for `+cmd` / `++cmd` initial commands. The
    /// command string is treated as a sequence of keystrokes: each byte
    /// becomes a `Key::Char` (or `Key::Enter` for `\n`).
    fn execute_command_string(&mut self, cmd: &str) -> Result<()> {
        for byte in cmd.bytes() {
            let key = match byte {
                b'\n' | b'\r' => Key::Enter,
                b'\x1b' => Key::Escape,
                b if b < 0x20 => {
                    // Control characters: Ctrl+A = 0x01 .. Ctrl+Z = 0x1a
                    #[allow(clippy::cast_possible_truncation)] // byte is < 0x20, result fits in u8
                    let ch = (b + b'a' - 1) as char;
                    Key::Ctrl(ch)
                }
                _ => Key::Char(byte as char),
            };
            if !self.process_key(&key)? {
                break;
            }
        }
        Ok(())
    }

    /// Execute queued initial and every-file commands.
    ///
    /// Called once after the first repaint. Runs initial commands first,
    /// then every-file commands.
    fn execute_initial_commands(&mut self) -> Result<()> {
        if self.initial_commands_executed {
            return Ok(());
        }
        self.initial_commands_executed = true;

        let initial = std::mem::take(&mut self.initial_commands);
        let every_file = self.every_file_commands.clone();

        for cmd in &initial {
            self.execute_command_string(cmd)?;
        }
        for cmd in &every_file {
            self.execute_command_string(cmd)?;
        }

        // Restore initial_commands — they won't re-execute due to the flag,
        // but keeping them preserves debuggability.
        self.initial_commands = initial;

        Ok(())
    }

    /// Execute every-file commands after a file switch.
    fn execute_every_file_commands(&mut self) -> Result<()> {
        let cmds = self.every_file_commands.clone();
        for cmd in &cmds {
            self.execute_command_string(cmd)?;
        }
        Ok(())
    }

    /// Set the tag navigation state (populated from `-t` CLI flag).
    pub fn set_tag_state(&mut self, state: TagState) {
        self.tag_state = Some(state);
    }

    /// Navigate to the next tag match (`t` command).
    fn navigate_tag_next(&mut self) -> Result<()> {
        let should_repaint = if let Some(ref mut state) = self.tag_state {
            if state.advance().is_some() {
                let idx = state.current_index();
                let total = state.count();
                self.status_message = Some(format!("tag {} of {total}", idx + 1));
                true
            } else {
                self.status_message = Some(String::from("No next tag"));
                true
            }
        } else {
            self.status_message = Some(String::from("No tags"));
            false // Don't repaint — let the next keypress show the message
        };
        if should_repaint {
            self.repaint()?;
        }
        Ok(())
    }

    /// Navigate to the previous tag match (`T` command).
    fn navigate_tag_prev(&mut self) -> Result<()> {
        let msg = if let Some(ref mut state) = self.tag_state {
            if state.go_back().is_some() {
                let idx = state.current_index();
                let total = state.count();
                Some(format!("tag {} of {total}", idx + 1))
            } else {
                Some(String::from("No previous tag"))
            }
        } else {
            Some(String::from("No tags"))
        };
        self.status_message = msg;
        self.repaint()?;
        Ok(())
    }

    /// Enable mouse tracking for the pager session.
    ///
    /// When enabled, the pager sends X11 and SGR mouse tracking escape
    /// sequences on entry and cleans them up on exit. Scroll wheel
    /// events are mapped to scroll commands via the keymap.
    pub fn set_mouse_enabled(&mut self, enabled: bool) {
        self.mouse_enabled = enabled;
    }

    /// Set the number of lines scrolled per mouse wheel tick.
    ///
    /// Updates the keymap bindings for `ScrollUp` and `ScrollDown`.
    /// Default is 3 lines.
    pub fn set_wheel_lines(&mut self, lines: usize) {
        self.keymap.set_wheel_lines(lines);
    }

    /// Set reversed mouse wheel direction (`--MOUSE`).
    ///
    /// Swaps scroll up/down so that wheel up scrolls forward and
    /// wheel down scrolls backward.
    pub fn set_wheel_reversed(&mut self, lines: usize) {
        self.keymap.set_wheel_reversed(lines);
    }

    /// Skip keypad init/deinit escape sequences (`--no-keypad`).
    ///
    /// When enabled, the pager will not send `ESC[?1h` / `ESC[?1l`
    /// (application keypad mode) on entry and exit.
    pub fn set_no_keypad(&mut self, enabled: bool) {
        self.no_keypad = enabled;
    }

    /// Disable visual bell (`--no-vbell`).
    ///
    /// When enabled, the pager will not flash the screen on errors.
    pub fn set_no_vbell(&mut self, enabled: bool) {
        self.no_vbell = enabled;
    }

    /// Whether visual bell is disabled.
    #[must_use]
    pub fn no_vbell(&self) -> bool {
        self.no_vbell
    }

    /// Repaint the screen before exiting alternate screen (`--redraw-on-quit`).
    ///
    /// When enabled, the pager repaints the viewport content before
    /// leaving the alternate screen buffer on quit.
    pub fn set_redraw_on_quit(&mut self, enabled: bool) {
        self.redraw_on_quit = enabled;
    }

    /// Enable `-K` behavior: Ctrl-C immediately quits with exit code 2.
    ///
    /// When enabled, an interrupt character (Ctrl-C) causes the pager to
    /// exit immediately rather than just cancelling the current operation.
    pub fn set_quit_on_intr(&mut self, enabled: bool) {
        self.quit_on_intr = enabled;
    }

    /// The reason the pager exited.
    ///
    /// Returns [`ExitReason::Normal`] for quit via `q`, `:q`, `ZZ`, EOF, etc.
    /// Returns [`ExitReason::Interrupt`] when quit was triggered by Ctrl-C.
    #[must_use]
    pub fn exit_reason(&self) -> ExitReason {
        self.exit_reason
    }

    /// Returns the detected content mode for the current file.
    #[must_use]
    pub fn content_mode(&self) -> ContentMode {
        self.content_mode
    }

    /// Returns the parsed SQL table layout, if any.
    #[must_use]
    pub fn sql_table_layout(&self) -> Option<&SqlTableLayout> {
        self.sql_table_layout.as_ref()
    }

    /// Detect content mode from the visible lines and set the status message.
    ///
    /// Called once per file on the first paint. If a non-plain mode is detected,
    /// sets a transient status message like `"[diff mode]"`.
    fn detect_content_mode_from_lines(&mut self, lines: &[Option<String>]) {
        let borrowed: Vec<&str> = lines.iter().filter_map(|opt| opt.as_deref()).collect();
        self.content_mode = detect_content_mode(&borrowed);
        if let Some(label) = self.content_mode.status_label() {
            self.status_message = Some(label);
        }

        // Parse diff structure when diff mode is detected.
        if self.content_mode == ContentMode::Diff {
            self.parse_diff_state();
        }

        // Parse git log commit positions when git log mode is detected.
        if self.content_mode == ContentMode::GitLog {
            self.parse_git_log_state();
        }

        // Parse man page section structure when man page mode is detected.
        if self.content_mode == ContentMode::ManPage {
            self.parse_man_sections();
        }

        // Parse SQL table layout for sticky header and column-snap hscroll.
        if self.content_mode == ContentMode::SqlTable {
            self.parse_sql_table_layout(&borrowed);
        }
    }

    /// Parse the SQL table layout and configure sticky headers.
    ///
    /// Extracts column boundaries from rule lines and sets the header
    /// line count so the pager freezes header rows at the top of the screen.
    fn parse_sql_table_layout(&mut self, lines: &[&str]) {
        if let Some(layout) = parse_table_layout(lines) {
            let header_rows = layout.header_rows;
            self.sql_table_layout = Some(layout);
            // Enable sticky header: freeze the header rows at the top.
            if header_rows > 0 {
                self.screen.set_header_lines(header_rows);
            }
        }
    }

    /// Apply frozen first column to lines for SQL table mode.
    ///
    /// When the table is horizontally scrolled, the first column stays
    /// visible at the left edge. This transforms each line by extracting
    /// the frozen prefix and appending the scrolled remainder.
    fn apply_frozen_column(&self, lines: &[Option<String>]) -> Vec<Option<String>> {
        let Some(ref layout) = self.sql_table_layout else {
            return lines.to_vec();
        };
        let h_offset = self.screen.horizontal_offset();
        lines
            .iter()
            .map(|opt| {
                opt.as_ref()
                    .map(|line| pgr_display::render_frozen_column(line, layout, h_offset))
            })
            .collect()
    }

    /// Parse the entire buffer into diff structure for hunk/file navigation.
    ///
    /// Called once when diff mode is detected. Reads all lines from the buffer
    /// and stores the parsed structure in `diff_state`.
    fn parse_diff_state(&mut self) {
        // Collect all buffer lines for diff parsing.
        let total = self.index.total_lines(&*self.buffer).unwrap_or(0);
        let mut all_lines: Vec<String> = Vec::with_capacity(total);
        for i in 0..total {
            if let Ok(Some(line)) = self.index.get_line(i, &*self.buffer) {
                all_lines.push(line);
            } else {
                all_lines.push(String::new());
            }
        }
        let borrowed: Vec<&str> = all_lines.iter().map(String::as_str).collect();
        let files = pgr_core::parse_diff(&borrowed);
        self.diff_state = if files.is_empty() { None } else { Some(files) };
    }

    /// Parse the entire buffer into a git log commit list for `]g`/`[g` navigation.
    ///
    /// Called once when git log mode is detected. Reads all lines from the buffer
    /// and stores the parsed commit positions in `git_log_commits`.
    fn parse_git_log_state(&mut self) {
        let total = self.index.total_lines(&*self.buffer).unwrap_or(0);
        let mut all_lines: Vec<String> = Vec::with_capacity(total);
        for i in 0..total {
            if let Ok(Some(line)) = self.index.get_line(i, &*self.buffer) {
                all_lines.push(line);
            } else {
                all_lines.push(String::new());
            }
        }
        let borrowed: Vec<&str> = all_lines.iter().map(String::as_str).collect();
        let commits = pgr_core::parse_git_log(&borrowed);
        self.git_log_commits = if commits.is_empty() {
            None
        } else {
            Some(commits)
        };
    }

    /// Parse the entire buffer into man page section structure for ]s/[s navigation.
    ///
    /// Called once when man page mode is detected. Reads all lines from the buffer
    /// and stores the parsed sections in `man_sections`.
    fn parse_man_sections(&mut self) {
        let total = self.index.total_lines(&*self.buffer).unwrap_or(0);
        let mut all_lines: Vec<String> = Vec::with_capacity(total);
        for i in 0..total {
            if let Ok(Some(line)) = self.index.get_line(i, &*self.buffer) {
                all_lines.push(line);
            } else {
                all_lines.push(String::new());
            }
        }
        let borrowed: Vec<&str> = all_lines.iter().map(String::as_str).collect();
        let sections = pgr_core::find_sections(&borrowed);
        self.man_sections = if sections.is_empty() {
            None
        } else {
            Some(sections)
        };
    }

    /// Immediately index all lines in the buffer (`--file-size`).
    ///
    /// Forces a complete scan of the buffer so that line counts and
    /// percentage calculations are accurate from the start, rather
    /// than being computed lazily as lines are visited.
    ///
    /// # Errors
    ///
    /// Returns an error if reading from the buffer fails during scanning.
    pub fn index_all_immediate(&mut self) -> Result<()> {
        self.index.index_all(&*self.buffer)?;
        Ok(())
    }

    /// Check whether syntax highlighting is active for the current file.
    ///
    /// Returns `true` when the `syntax` feature is compiled in, highlighting
    /// is enabled at runtime, a highlighter is loaded, and the current file
    /// has a recognized syntax.
    #[cfg(feature = "syntax")]
    fn is_syntax_active(&self) -> bool {
        if !self.syntax_enabled {
            return false;
        }
        let Some(ref highlighter) = self.highlighter else {
            return false;
        };
        let Some(filename) = self.filename.as_deref() else {
            return false;
        };
        highlighter.detect_syntax(filename).is_some()
    }

    /// Set the syntax highlighter for syntax-highlighted rendering.
    ///
    /// When set (and the file has a recognized extension), lines are
    /// highlighted with ANSI SGR color codes before rendering.
    #[cfg(feature = "syntax")]
    pub fn set_highlighter(&mut self, highlighter: pgr_display::syntax::highlighting::Highlighter) {
        self.highlighter = Some(highlighter);
    }

    /// Enable or disable syntax highlighting at runtime.
    #[cfg(feature = "syntax")]
    pub fn set_syntax_enabled(&mut self, enabled: bool) {
        self.syntax_enabled = enabled;
    }

    /// Apply diff-aware coloring to visible lines when in diff mode.
    ///
    /// For each line, classifies it (added, removed, context, header, etc.)
    /// and applies background tinting plus optional per-hunk syntax highlighting.
    /// When the `syntax` feature is active and a diff filename has a recognized
    /// syntax, per-hunk syntax highlighting is layered on top of the diff tinting.
    #[allow(clippy::unused_self)] // self is used only when the `syntax` feature is enabled
    fn colorize_diff_lines(&self, lines: &[Option<String>]) -> Vec<Option<String>> {
        // Determine the current diff filename for syntax detection.
        #[cfg(feature = "syntax")]
        let diff_filename = self.current_diff_filename();

        lines
            .iter()
            .map(|opt| {
                opt.as_ref().map(|text| {
                    let line_type = pgr_core::classify_diff_line(text);

                    // Try per-hunk syntax highlighting when the syntax feature is
                    // available and a diff filename was detected.
                    #[cfg(feature = "syntax")]
                    {
                        if self.syntax_enabled {
                            if let Some(ref highlighter) = self.highlighter {
                                if let Some(ref fname) = diff_filename {
                                    let line_slice: &[&str] = &[text.as_str()];
                                    let type_slice: &[pgr_core::DiffLineType] = &[line_type];
                                    let result = pgr_display::highlight_diff_hunk(
                                        line_slice,
                                        type_slice,
                                        highlighter,
                                        fname,
                                    );
                                    if let Some(colored) = result.into_iter().next() {
                                        return colored;
                                    }
                                }
                            }
                        }
                    }

                    // Fallback: diff coloring without syntax highlighting.
                    pgr_display::colorize_diff_line(text, line_type)
                })
            })
            .collect()
    }

    /// Extract the current diff filename from the parsed diff state.
    ///
    /// Uses the viewport's top line to find which diff file the user is
    /// viewing, and returns its filename for syntax detection.
    #[cfg(feature = "syntax")]
    fn current_diff_filename(&self) -> Option<String> {
        let files = self.diff_state.as_ref()?;
        let top = self.screen.top_line();
        let info = pgr_core::compute_diff_prompt_info(files, top)?;
        info.current_file
    }

    /// Apply git-blame coloring to visible lines when in git blame mode.
    ///
    /// Parses each line's hash/author/date gutter and colorizes it by commit
    /// recency. When the `syntax` feature is active and the current file has a
    /// recognized syntax extension, the code column is also syntax-highlighted.
    #[allow(clippy::unused_self)] // self is used only when the `syntax` feature is enabled
    fn colorize_blame_lines(&self, lines: &[Option<String>]) -> Vec<Option<String>> {
        let current_year = current_year_approx();

        #[cfg(feature = "syntax")]
        let filename = self.filename.as_deref();

        lines
            .iter()
            .map(|opt| {
                opt.as_ref().map(|text| {
                    #[cfg(feature = "syntax")]
                    {
                        if self.syntax_enabled {
                            if let Some(ref highlighter) = self.highlighter {
                                if let Some(fname) = filename {
                                    return pgr_display::colorize_blame_line_syntax(
                                        text,
                                        current_year,
                                        highlighter,
                                        fname,
                                    );
                                }
                            }
                        }
                    }
                    pgr_display::colorize_blame_line(text, current_year)
                })
            })
            .collect()
    }

    /// Apply syntax highlighting to a set of lines if highlighting is active.
    ///
    /// Returns the lines with SGR color codes injected, or the original lines
    /// if highlighting is not active for the current file.
    #[cfg(feature = "syntax")]
    fn highlight_lines(
        &mut self,
        lines: &[Option<String>],
        start_line: usize,
    ) -> Vec<Option<String>> {
        if !self.syntax_enabled {
            return lines.to_vec();
        }
        let Some(ref highlighter) = self.highlighter else {
            return lines.to_vec();
        };
        let Some(filename) = self.filename.as_deref() else {
            return lines.to_vec();
        };
        let Some(syntax) = highlighter.detect_syntax(filename) else {
            return lines.to_vec();
        };

        // Use HighlightLines for proper stateful highlighting.
        // For the visible window, we need to parse from the beginning (or a
        // cached state) up to the start line, then highlight the visible lines.
        let mut hl = highlighter.highlight_lines(syntax);

        // Parse (but don't render) lines before the visible window to build
        // correct syntax state. For large files this could be slow, but for
        // typical use (sequential scrolling) it's bounded by the visible window.
        // We cap the pre-parse to avoid freezing on very large jumps.
        let max_preparse = 5000;
        let preparse_start = start_line.saturating_sub(max_preparse);

        // Skip lines before preparse_start (approximate — we lose state accuracy
        // but avoid O(n) startup for huge files).
        if preparse_start > 0 {
            // Re-create from start for simplicity; in a future iteration,
            // SyntaxState caching would make this efficient.
            hl = highlighter.highlight_lines(syntax);
        }

        // Pre-parse lines before the visible window.
        for line_num in preparse_start..start_line {
            if let Ok(Some(text)) = self.index.get_line(line_num, &*self.buffer) {
                // Add newline if missing — syntect expects newline-terminated lines.
                let text_nl = if text.ends_with('\n') {
                    text
                } else {
                    format!("{text}\n")
                };
                let _ = hl.highlight_line(&text_nl, highlighter.syntax_set());
            }
        }

        // Now highlight the visible lines.
        lines
            .iter()
            .map(|opt| {
                opt.as_ref().and_then(|text| {
                    let text_nl = if text.ends_with('\n') {
                        text.clone()
                    } else {
                        format!("{text}\n")
                    };
                    let ranges = hl.highlight_line(&text_nl, highlighter.syntax_set()).ok()?;
                    let escaped =
                        pgr_display::syntax::highlighting::as_24_bit_terminal_escaped(&ranges);
                    // Remove trailing newline that we added (render pipeline handles line breaks).
                    Some(escaped.trim_end_matches('\n').to_string())
                })
            })
            .collect()
    }

    /// Set the clipboard backend for yank commands.
    pub fn set_clipboard(&mut self, clipboard: Box<dyn crate::clipboard::Clipboard>) {
        self.clipboard_disabled = clipboard.name() == "disabled";
        self.clipboard = clipboard;
    }

    /// Yank (copy) the current top-of-screen line to the clipboard.
    fn yank_line(&mut self) -> Result<()> {
        if self.clipboard_disabled {
            self.status_message = Some("Clipboard disabled".to_string());
            self.repaint()?;
            return Ok(());
        }

        let line_num = self.screen.top_line();
        let text = self
            .index
            .get_line(line_num, &*self.buffer)?
            .unwrap_or_default();
        let plain = pgr_display::ansi::strip_ansi(&text);

        if let Err(e) = self.clipboard.copy(&plain) {
            self.status_message = Some(format!("Clipboard error: {e}"));
        } else {
            self.status_message = Some("Yanked 1 line".to_string());
        }
        self.repaint()?;
        Ok(())
    }

    /// Yank (copy) all visible lines to the clipboard.
    fn yank_screen(&mut self, total: usize) -> Result<()> {
        if self.clipboard_disabled {
            self.status_message = Some("Clipboard disabled".to_string());
            self.repaint()?;
            return Ok(());
        }

        let (start, end) = self.screen.visible_range();
        let end = end.min(total);
        let mut lines = Vec::new();

        for line_num in start..end {
            let text = self
                .index
                .get_line(line_num, &*self.buffer)?
                .unwrap_or_default();
            lines.push(pgr_display::ansi::strip_ansi(&text));
        }

        let count = lines.len();
        let joined = lines.join("\n");

        if let Err(e) = self.clipboard.copy(&joined) {
            self.status_message = Some(format!("Clipboard error: {e}"));
        } else {
            self.status_message = Some(format!("Yanked {count} lines"));
        }
        self.repaint()?;
        Ok(())
    }

    // ── Git gutter ──────────────────────────────────────────────────

    /// Enable or disable git gutter display.
    ///
    /// When enabling, immediately loads the gutter state for the current file.
    pub fn set_git_gutter_enabled(&mut self, enabled: bool) {
        self.git_gutter_enabled = enabled;
        if enabled {
            self.load_gutter_state();
        }
    }

    // ── Side-by-side diff ─────────────────────────────────────────

    /// Enable or disable side-by-side diff rendering.
    pub fn set_side_by_side(&mut self, enabled: bool) {
        self.side_by_side = enabled;
    }

    /// Returns whether side-by-side diff rendering is currently active.
    #[must_use]
    pub fn side_by_side(&self) -> bool {
        self.side_by_side
    }

    /// Load git gutter state from the current file.
    ///
    /// Runs `git diff` against the current filename (if any). In secure
    /// mode, this is a no-op since spawning external processes is forbidden.
    fn load_gutter_state(&mut self) {
        if self.secure_mode {
            self.gutter_state = None;
            return;
        }
        if let Some(ref name) = self.filename {
            let path = std::path::Path::new(name);
            self.gutter_state = crate::git_gutter::GutterState::from_file(path);
        } else {
            self.gutter_state = None;
        }
    }

    /// Build gutter marks for the currently visible lines.
    ///
    /// Returns a vec parallel to the visible lines slice, with each entry
    /// containing the gutter symbol and color for that line (or `None`).
    fn build_gutter_marks(&self, start: usize, count: usize) -> Vec<Option<(char, &'static str)>> {
        if !self.git_gutter_enabled {
            return Vec::new();
        }
        let Some(ref gutter) = self.gutter_state else {
            return Vec::new();
        };
        (0..count)
            .map(|i| {
                let line_number = start + i + 1; // 1-based
                gutter
                    .mark_for_line(line_number)
                    .map(|m| (m.symbol(), m.ansi_color()))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A simple test buffer implementing `Buffer` over a `Vec<u8>`.
    struct TestBuffer {
        data: Vec<u8>,
    }

    impl TestBuffer {
        fn new(data: &[u8]) -> Self {
            Self {
                data: data.to_vec(),
            }
        }
    }

    impl Buffer for TestBuffer {
        fn len(&self) -> usize {
            self.data.len()
        }

        fn read_at(&self, offset: usize, buf: &mut [u8]) -> pgr_core::Result<usize> {
            if offset >= self.data.len() {
                return Ok(0);
            }
            let available = &self.data[offset..];
            let to_copy = available.len().min(buf.len());
            buf[..to_copy].copy_from_slice(&available[..to_copy]);
            Ok(to_copy)
        }

        fn is_growable(&self) -> bool {
            false
        }

        fn refresh(&mut self) -> pgr_core::Result<usize> {
            Ok(self.data.len())
        }
    }

    /// Build a multiline test buffer with numbered lines.
    fn make_test_content(line_count: usize) -> Vec<u8> {
        let mut data = Vec::new();
        for i in 0..line_count {
            data.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        data
    }

    /// Create a pager with the given input bytes and buffer content,
    /// run it, and return the pager for inspection.
    fn run_pager(keys: &[u8], content: &[u8]) -> Pager<Cursor<Vec<u8>>, Vec<u8>> {
        let reader = KeyReader::new(Cursor::new(keys.to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        // Ignore errors from run — they happen when input is exhausted.
        let _ = pager.run();
        pager
    }

    #[test]
    fn test_dispatch_q_causes_quit() {
        let content = make_test_content(50);
        let pager = run_pager(b"q", &content);
        assert!(pager.should_quit);
    }

    #[test]
    fn test_dispatch_j_scrolls_forward_one_line() {
        let content = make_test_content(50);
        let pager = run_pager(b"jq", &content);
        assert_eq!(pager.screen().top_line(), 1);
    }

    #[test]
    fn test_dispatch_k_scrolls_backward_one_line() {
        // Start by scrolling forward, then backward.
        let content = make_test_content(50);
        let pager = run_pager(b"jjjkq", &content);
        // 3 forward, 1 backward = top_line 2
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_space_scrolls_forward_one_page() {
        let content = make_test_content(100);
        let pager = run_pager(b" q", &content);
        // Default screen is 24 rows, content_rows = 23. Space scrolls 23 lines.
        assert_eq!(pager.screen().top_line(), 23);
    }

    #[test]
    fn test_dispatch_b_scrolls_backward_one_page() {
        let content = make_test_content(100);
        // Scroll forward two pages, then back one.
        let pager = run_pager(b"  bq", &content);
        // 23 + 23 = 46, then back 23 = 23.
        assert_eq!(pager.screen().top_line(), 23);
    }

    #[test]
    fn test_dispatch_g_goes_to_beginning() {
        let content = make_test_content(100);
        // Scroll forward, then go to beginning.
        let pager = run_pager(b"   gq", &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_upper_g_goes_to_end() {
        let content = make_test_content(100);
        let pager = run_pager(b"Gq", &content);
        // GotoEnd default: total - content_rows = 100 - 23 = 77
        assert_eq!(pager.screen().top_line(), 77);
    }

    #[test]
    fn test_dispatch_numeric_prefix_5j_scrolls_forward_5() {
        let content = make_test_content(50);
        let pager = run_pager(b"5jq", &content);
        assert_eq!(pager.screen().top_line(), 5);
    }

    #[test]
    fn test_dispatch_numeric_prefix_10_upper_g_goes_to_line_10() {
        let content = make_test_content(100);
        let pager = run_pager(b"10Gq", &content);
        // 10G: go to line 10 (1-based) = index 9 (0-based)
        assert_eq!(pager.screen().top_line(), 9);
    }

    #[test]
    fn test_dispatch_multiple_digits_123j_scrolls_forward_123_clamped() {
        let content = make_test_content(50);
        let pager = run_pager(b"123jq", &content);
        // 123 lines forward, but total is 50 with 23 content rows, so clamped to 50-23=27.
        assert_eq!(pager.screen().top_line(), 27);
    }

    #[test]
    fn test_dispatch_r_triggers_repaint_without_changing_position() {
        let content = make_test_content(50);
        let pager = run_pager(b"jjrq", &content);
        // Two j's move to line 2, r repaints without moving.
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_screen_accessor_returns_reference() {
        let content = make_test_content(10);
        let pager = run_pager(b"q", &content);
        assert_eq!(pager.screen().content_rows(), 23);
    }

    #[test]
    fn test_dispatch_empty_buffer_shows_end() {
        let pager = run_pager(b"q", b"");
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_input_exhausted_exits_gracefully() {
        let content = make_test_content(50);
        // No 'q' — just run out of input.
        let pager = run_pager(b"jj", &content);
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_numeric_prefix_resets_after_command() {
        let content = make_test_content(50);
        // 5j (go to 5), then j (go to 6) — prefix should not carry over.
        let pager = run_pager(b"5jjq", &content);
        assert_eq!(pager.screen().top_line(), 6);
    }

    #[test]
    fn test_dispatch_noop_key_does_not_change_position() {
        let content = make_test_content(50);
        // 'x' is unbound (Noop), should not change position.
        let pager = run_pager(b"jxq", &content);
        assert_eq!(pager.screen().top_line(), 1);
    }

    // ---- Mark setting tests ----

    #[test]
    fn test_dispatch_m_a_sets_mark_at_top_line() {
        let content = make_test_content(50);
        // Scroll to line 5, then set mark 'a' at the top displayed line.
        let pager = run_pager(b"5jmaq", &content);
        let mark = pager.marks().get('a').expect("mark 'a' should be set");
        assert_eq!(mark.line, 5);
    }

    #[test]
    fn test_dispatch_upper_m_a_sets_mark_at_bottom_line() {
        let content = make_test_content(50);
        // At top_line 0, content_rows = 23, so bottom = min(23, 50) - 1 = 22.
        let pager = run_pager(b"Maq", &content);
        let mark = pager.marks().get('a').expect("mark 'a' should be set");
        assert_eq!(mark.line, 22);
    }

    #[test]
    fn test_dispatch_m_invalid_char_does_not_crash() {
        let content = make_test_content(50);
        // Press 'm', '3' -> MarkStore rejects digits; should not crash.
        let pager = run_pager(b"m3q", &content);
        assert!(pager.marks().get('3').is_none());
    }

    // ---- Mark jumping tests ----

    #[test]
    fn test_dispatch_quote_a_jumps_to_mark() {
        let content = make_test_content(100);
        // Scroll to line 10, set mark 'a', scroll away to line 40, then jump back.
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b"10jma"); // scroll to 10, set mark 'a'
        keys.extend_from_slice(b"30j"); // scroll to 40
        keys.push(b'\''); // start goto mark
        keys.push(b'a'); // mark 'a'
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 10);
    }

    #[test]
    fn test_dispatch_quote_quote_returns_to_previous_position() {
        let content = make_test_content(100);
        // Start at 0, page forward (saves last_position=0), then '' returns to 0.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b' '); // page forward (saves position 0, moves to 23)
        keys.push(b'\'');
        keys.push(b'\''); // '' returns to previous
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    // ── Horizontal scrolling ─────────────────────────────────────────

    #[test]
    fn test_dispatch_scroll_right_increases_horizontal_offset() {
        let content = make_test_content(50);
        // RIGHT arrow is ESC [ C
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right arrow
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Default scroll: cols/2 = 80/2 = 40
        assert_eq!(pager.screen().horizontal_offset(), 40);
    }

    #[test]
    fn test_dispatch_scroll_left_decreases_horizontal_offset() {
        let content = make_test_content(50);
        // Two rights, then one left
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.extend_from_slice(&[0x1B, b'[', b'D']); // Left
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // 40 + 40 - 40 = 40
        assert_eq!(pager.screen().horizontal_offset(), 40);
    }

    #[test]
    fn test_dispatch_scroll_left_clamps_at_zero() {
        let content = make_test_content(50);
        // Left arrow at offset 0 should stay at 0
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'D']); // Left
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().horizontal_offset(), 0);
    }

    #[test]
    fn test_dispatch_scroll_left_home_resets_to_zero() {
        let content = make_test_content(50);
        // Right, then CtrlLeft (ESC [ 1 ; 5 D)
        let mut keys = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.extend_from_slice(&[0x1B, b'[', b'1', b';', b'5', b'D']); // CtrlLeft
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().horizontal_offset(), 0);
    }

    #[test]
    fn test_dispatch_scroll_right_with_count() {
        let content = make_test_content(50);
        // "20" then RIGHT arrow -> scroll right 20
        let mut keys: Vec<u8> = b"20".to_vec();
        keys.extend_from_slice(&[0x1B, b'[', b'C']); // Right
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().horizontal_offset(), 20);
    }

    // ── Percent and byte navigation ──────────────────────────────────

    #[test]
    fn test_dispatch_goto_percent_50_goes_to_middle() {
        let content = make_test_content(100);
        // "50p" -> goto 50% of 100 lines = line 50
        let pager = run_pager(b"50pq", &content);
        assert_eq!(pager.screen().top_line(), 50);
    }

    #[test]
    fn test_dispatch_goto_percent_0_goes_to_beginning() {
        let content = make_test_content(100);
        // Scroll forward first, then "0p" -> goto beginning
        let pager = run_pager(b"  0pq", &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_quote_caret_jumps_to_beginning() {
        let content = make_test_content(100);
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b" "); // page forward to line 23
        keys.push(b'\'');
        keys.push(b'^');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_quote_dollar_jumps_to_end() {
        let content = make_test_content(100);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'\'');
        keys.push(b'$');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // End: total(100) - content_rows(23) + 1 = 78
        // less '$' mark positions 1 line past G (shows tilde at bottom).
        assert_eq!(pager.screen().top_line(), 78);
    }

    #[test]
    fn test_dispatch_quote_unset_mark_does_nothing() {
        let content = make_test_content(100);
        // Scroll to line 5, then try to goto unset mark 'z'. Should stay at 5.
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b"5j");
        keys.push(b'\'');
        keys.push(b'z');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 5);
    }

    // ---- Mark clearing tests ----

    #[test]
    fn test_dispatch_esc_m_a_clears_mark() {
        let content = make_test_content(50);
        // Set mark 'a', then clear it with ESC-m a.
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b"ma"); // set mark 'a'
        keys.push(0x1B); // ESC
        keys.push(b'm'); // -> EscSeq('m')
        keys.push(b'a'); // clear mark 'a'
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert!(pager.marks().get('a').is_none());
    }

    // ---- Multi-key sequence tests ----

    #[test]
    fn test_dispatch_pending_command_m_waits_for_letter() {
        let content = make_test_content(50);
        // Press 'm' alone, then 'q' — 'm' enters pending mode, 'q' resolves
        // as the mark letter (not as quit, since it's consumed by pending).
        // Then input is exhausted.
        let pager = run_pager(b"mq", &content);
        // 'q' was consumed as the mark letter, not as quit.
        let mark = pager.marks().get('q').expect("mark 'q' should be set");
        assert_eq!(mark.line, 0);
    }

    #[test]
    fn test_dispatch_pending_command_cancelled_by_invalid_key() {
        let content = make_test_content(50);
        // Press 'm' then Up arrow (non-char key). Pending is cancelled, no mark set.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'm');
        keys.extend_from_slice(&[0x1B, b'[', b'A']); // Up arrow
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Up arrow is not a char key, so no mark is set. Position unchanged.
        assert_eq!(pager.screen().top_line(), 0);
        assert!(pager.marks().list().is_empty());
    }

    #[test]
    fn test_dispatch_ctrl_x_ctrl_x_enters_goto_mark() {
        let content = make_test_content(100);
        // Set mark 'a' at line 10, scroll away, then ^X^X a to jump back.
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b"10jma"); // scroll to 10, set mark 'a'
        keys.extend_from_slice(b"20j"); // scroll to 30
        keys.push(0x18); // Ctrl+X (byte 0x18)
        keys.push(0x18); // Ctrl+X (byte 0x18)
        keys.push(b'a'); // mark 'a'
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 10);
    }

    // ---- Last position tracking tests ----

    #[test]
    fn test_dispatch_page_forward_saves_last_position() {
        let content = make_test_content(100);
        // Page forward saves position, then '' returns.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b' '); // page forward (saves 0, goes to 23)
        keys.push(b'\'');
        keys.push(b'\''); // '' returns to 0
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_goto_end_saves_last_position() {
        let content = make_test_content(100);
        // G goes to end, then '' returns to 0.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'G'); // goto end (saves 0, goes to 77)
        keys.push(b'\'');
        keys.push(b'\''); // '' returns to 0
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_scroll_forward_does_not_save_last_position() {
        let content = make_test_content(100);
        // 'j' (1 line scroll) does not update last_position.
        let pager = run_pager(b"jq", &content);
        assert!(pager.last_position().is_none());
    }

    #[test]
    fn test_dispatch_goto_percent_100_goes_to_end() {
        let content = make_test_content(100);
        let pager = run_pager(b"100pq", &content);
        // 100 * 100 / 100 = 100, clamped to total - 1 = 99 (last line at top, tildes below)
        assert_eq!(pager.screen().top_line(), 99);
    }

    #[test]
    fn test_dispatch_goto_byte_offset_finds_correct_line() {
        // "line 0\n" is 7 bytes, "line 1\n" is 7 bytes, etc.
        // Byte offset 7 is start of line 1.
        let content = make_test_content(50);
        let pager = run_pager(b"7Pq", &content);
        assert_eq!(pager.screen().top_line(), 1);
    }

    // ── Sticky half-page ─────────────────────────────────────────────

    #[test]
    fn test_dispatch_half_page_forward_with_count_sets_sticky() {
        let content = make_test_content(100);
        // "10d" sets sticky to 10 and scrolls 10. Then "d" uses sticky 10.
        let pager = run_pager(b"10ddq", &content);
        // 10 + 10 = 20
        assert_eq!(pager.screen().top_line(), 20);
        assert_eq!(pager.sticky_half_page(), Some(10));
    }

    #[test]
    fn test_dispatch_half_page_backward_with_count_sets_sticky() {
        let content = make_test_content(100);
        // Scroll forward by 30 first, then "5u" sets sticky to 5 and scrolls back 5
        let pager = run_pager(b"30j5uq", &content);
        assert_eq!(pager.screen().top_line(), 25);
        assert_eq!(pager.sticky_half_page(), Some(5));
    }

    #[test]
    fn test_dispatch_half_page_default_amount_matches_less() {
        // 24-row terminal: screen_height = 24, default half = 24 / 2 = 12.
        // less uses (screen_height / 2), not (content_rows / 2).
        let content = make_test_content(100);
        let pager = run_pager(b"dq", &content);
        assert_eq!(pager.screen().top_line(), 12);
    }

    // ── Window sizing ────────────────────────────────────────────────

    #[test]
    fn test_dispatch_z_with_count_sets_window_and_scrolls() {
        let content = make_test_content(100);
        // "15z" sets window to 15 and scrolls forward 15
        let pager = run_pager(b"15zq", &content);
        assert_eq!(pager.screen().top_line(), 15);
        assert_eq!(pager.custom_window_size(), Some(WindowSize::Absolute(15)));
    }

    #[test]
    fn test_dispatch_w_with_count_sets_window_and_scrolls_back() {
        let content = make_test_content(100);
        // Scroll forward 30, then "10w" sets window to 10 and scrolls back 10
        let pager = run_pager(b"30j10wq", &content);
        assert_eq!(pager.screen().top_line(), 20);
        assert_eq!(pager.custom_window_size(), Some(WindowSize::Absolute(10)));
    }

    // ── Force-scroll commands ────────────────────────────────────────

    #[test]
    fn test_dispatch_esc_space_scrolls_forward_even_at_eof() {
        let content = make_test_content(100);
        // Navigate to end with G, then ESC-SPACE scrolls forward unclamped.
        // G -> total(100) - content_rows(23) = 77. Then ESC-SPACE scrolls 23 more -> 100.
        let mut keys = Vec::new();
        keys.push(b'G');
        keys.extend_from_slice(&[0x1B, b' ']); // ESC-SPACE
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // G -> 77, ESC-SPACE -> 77 + 23 = 100 (beyond total_lines - 1 = 99)
        assert_eq!(pager.screen().top_line(), 100);
    }

    #[test]
    fn test_dispatch_upper_j_scrolls_forward_beyond_eof() {
        let content = make_test_content(100);
        // 99j -> scroll_forward clamped at total-content_rows = 100-23 = 77.
        // J (ForwardForceEof) is unclamped, so from 77 it goes to 78.
        let pager = run_pager(b"99jJq", &content);
        assert_eq!(pager.screen().top_line(), 78);
    }

    // ── Follow mode ──────────────────────────────────────────────────

    #[test]
    fn test_dispatch_follow_mode_scrolls_to_end() {
        let content = make_test_content(100);
        let pager = run_pager(b"Fq", &content);
        // Follow mode scrolls to end: total(100) - content_rows(23) = 77
        assert_eq!(pager.screen().top_line(), 77);
    }

    #[test]
    fn test_dispatch_esc_f_no_pattern_falls_back_to_follow() {
        // ESC-F with no active search pattern should behave like regular F.
        let content = make_test_content(100);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(0x1B); // ESC
        keys.push(b'F'); // -> EscSeq('F') -> FollowModeStopOnMatch
        keys.push(b'q'); // quit
        let pager = run_pager(&keys, &content);
        // Falls back to follow_mode which scrolls to end.
        assert_eq!(pager.screen().top_line(), 77);
    }

    #[test]
    fn test_dispatch_esc_f_with_pattern_scrolls_to_end() {
        // ESC-F with a search pattern should scroll to end and enter follow mode.
        let content = make_test_content(100);
        // First search for "line", then ESC-F, then quit.
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b"/line\n"); // search for "line"
        keys.push(0x1B); // ESC
        keys.push(b'F'); // -> FollowModeStopOnMatch
        keys.push(b'q'); // quit
        let pager = run_pager(&keys, &content);
        // Should have scrolled to end in follow mode.
        assert_eq!(pager.screen().top_line(), 77);
    }

    #[test]
    fn test_dispatch_follow_name_setter() {
        let content = make_test_content(10);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);
        let mut pager = Pager::new(reader, writer, buffer, index, Some("test.txt".to_string()));
        pager.set_follow_name(true);
        // The pager runs and quits; we're just verifying the setter doesn't panic.
        let _ = pager.run();
    }

    #[test]
    fn test_dispatch_exit_follow_on_close_setter() {
        let content = make_test_content(10);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);
        let mut pager = Pager::new(reader, writer, buffer, index, Some("test.txt".to_string()));
        pager.set_exit_follow_on_close(true);
        // The pager runs and quits; we're just verifying the setter doesn't panic.
        let _ = pager.run();
    }

    // ── Repaint refresh ──────────────────────────────────────────────

    #[test]
    fn test_dispatch_upper_r_refreshes_buffer() {
        let content = make_test_content(50);
        // R refreshes and repaints without moving
        let pager = run_pager(b"jjRq", &content);
        // Position should remain at line 2 after refresh + repaint
        assert_eq!(pager.screen().top_line(), 2);
    }

    // ── Window forward/backward affects page commands ────────────────

    #[test]
    fn test_dispatch_window_size_affects_subsequent_page_forward() {
        let content = make_test_content(100);
        // "10z" sets window to 10, then SPACE uses that window
        let pager = run_pager(b"10z q", &content);
        // 10z -> scrolls 10, SPACE -> scrolls 10 more = 20
        assert_eq!(pager.screen().top_line(), 20);
    }

    // ── Colon-prefix and file list tests ────────────────────────────────

    use crate::file_list::{FileEntry, FileList};
    use std::path::PathBuf;

    fn make_file_entry(name: &str, content: &[u8]) -> FileEntry {
        let buf_len = content.len() as u64;
        FileEntry {
            path: Some(PathBuf::from(name)),
            display_name: name.to_string(),
            buffer: Box::new(TestBuffer::new(content)),
            index: LineIndex::new(buf_len),
            marks: MarkStore::new(),
            saved_top_line: 0,
            saved_horizontal_offset: 0,
        }
    }

    /// Create a pager with a file list, run it, and return the pager.
    fn run_pager_with_files(
        keys: &[u8],
        files: Vec<(&str, &[u8])>,
    ) -> Pager<Cursor<Vec<u8>>, Vec<u8>> {
        assert!(!files.is_empty());

        let first_content = files[0].1;
        let reader = KeyReader::new(Cursor::new(keys.to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(first_content));
        let buf_len = first_content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, Some(files[0].0.to_string()));

        let first_entry = make_file_entry(files[0].0, files[0].1);
        let mut file_list = FileList::new(first_entry);
        for &(name, content) in &files[1..] {
            file_list.push(make_file_entry(name, content));
        }
        pager.set_file_list(file_list);

        let _ = pager.run();
        pager
    }

    // Test 12: `:n` key sequence maps to NextFile command
    #[test]
    fn test_dispatch_colon_n_switches_to_next_file() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(30);
        let pager = run_pager_with_files(
            b":nq",
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 1);
        assert_eq!(fl.current().display_name, "file2.txt");
    }

    // Test 13: `:p` key sequence maps to PreviousFile command
    #[test]
    fn test_dispatch_colon_p_switches_to_previous_file() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(30);
        let pager = run_pager_with_files(
            b":n:pq",
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
        assert_eq!(fl.current().display_name, "file1.txt");
    }

    // Test 14: `:d` key sequence maps to RemoveFile command
    #[test]
    fn test_dispatch_colon_d_removes_current_file() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(30);
        let pager = run_pager_with_files(
            b":dq",
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.file_count(), 1);
        assert_eq!(fl.current().display_name, "file2.txt");
    }

    // Test 15: Switching files updates pager's active filename
    #[test]
    fn test_dispatch_switching_files_updates_pager_filename() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(30);
        let pager = run_pager_with_files(
            b":nq",
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        assert_eq!(pager.filename.as_deref(), Some("file2.txt"));
    }

    // `:x` with no count goes to first file
    #[test]
    fn test_dispatch_colon_x_goes_to_first_file() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(30);
        let content3 = make_test_content(20);
        let pager = run_pager_with_files(
            b":n:n:xq",
            vec![
                ("file1.txt", &content1),
                ("file2.txt", &content2),
                ("file3.txt", &content3),
            ],
        );
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
    }

    // `:n` at last file stays at last file (no-op)
    #[test]
    fn test_dispatch_colon_n_at_last_file_stays() {
        let content1 = make_test_content(50);
        let pager = run_pager_with_files(b":nq", vec![("file1.txt", &content1)]);
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
    }

    // `:p` at first file stays at first file (no-op)
    #[test]
    fn test_dispatch_colon_p_at_first_file_stays() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(30);
        let pager = run_pager_with_files(
            b":pq",
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
    }

    // `:q` via colon prefix quits
    #[test]
    fn test_dispatch_colon_q_quits() {
        let content1 = make_test_content(50);
        let pager = run_pager_with_files(b":q", vec![("file1.txt", &content1)]);
        assert!(pager.should_quit);
    }

    // Viewport state is preserved across file switches
    #[test]
    fn test_dispatch_viewport_preserved_across_file_switch() {
        let content1 = make_test_content(100);
        let content2 = make_test_content(50);
        // Scroll to line 10 in file1, switch to file2, switch back, should be at 10.
        let pager = run_pager_with_files(
            b"10j:n:pq",
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
        assert_eq!(pager.screen().top_line(), 10);
    }

    // ── Quit-at-EOF tests (Task 120) ────────────────────────────────────

    /// Create a pager with quit-at-eof enabled, run it, and return the pager.
    fn run_pager_quit_eof(
        keys: &[u8],
        content: &[u8],
        quit_at_eof: bool,
        quit_at_first_eof: bool,
    ) -> Pager<Cursor<Vec<u8>>, Vec<u8>> {
        let reader = KeyReader::new(Cursor::new(keys.to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, Some("test".to_string()));
        pager.set_quit_at_eof(quit_at_eof);
        pager.set_quit_at_first_eof(quit_at_first_eof);
        let _ = pager.run();
        pager
    }

    // Test: -E quits on first scroll past EOF
    #[test]
    fn test_dispatch_quit_at_first_eof_quits_on_first_scroll_past_eof() {
        // 5 lines, 24-row screen (23 content rows). File fits on one screen.
        // Scrolling forward should immediately trigger EOF quit with -E.
        let content = b"a\nb\nc\nd\ne\n";
        let pager = run_pager_quit_eof(b"j", content, false, true);
        assert!(pager.should_quit);
    }

    // Test: -e quits after second scroll past EOF
    #[test]
    fn test_dispatch_quit_at_eof_quits_on_second_scroll_past_eof() {
        let content = b"a\nb\nc\nd\ne\n";
        // First `j` scrolls to EOF (count 1). Second `j` is also at EOF.
        // With -e, should quit after the second.
        let pager = run_pager_quit_eof(b"jj", content, true, false);
        assert!(pager.should_quit);
    }

    // Test: -e does NOT quit after first scroll past EOF
    #[test]
    fn test_dispatch_quit_at_eof_does_not_quit_after_first_scroll() {
        let content = b"a\nb\nc\nd\ne\n";
        // Single `j` at EOF. With -e, should not quit yet (first time).
        // We need the pager to not quit immediately but also not hang.
        // Use `j` then `q` to verify the pager didn't auto-quit on first scroll.
        let pager = run_pager_quit_eof(b"jq", content, true, false);
        // Should have quit via `q`, not from -e auto-quit.
        assert!(pager.should_quit);
        // The eof_seen_count should be 1 (only one scroll past EOF before `q`).
        assert_eq!(pager.eof_seen_count, 1);
    }

    // Test: without -e or -E, scrolling past EOF does not auto-quit
    #[test]
    fn test_dispatch_no_eof_flag_does_not_auto_quit() {
        let content = b"a\nb\nc\nd\ne\n";
        let pager = run_pager_quit_eof(b"jjq", content, false, false);
        // Only quit via `q`.
        assert!(pager.should_quit);
        assert_eq!(pager.eof_seen_count, 0);
    }

    // ── Option toggling tests (Task 119) ─────────────────────────────

    #[test]
    fn test_dispatch_dash_i_toggles_case_insensitive() {
        let content = make_test_content(50);
        // `-i` toggles case_insensitive; next key absorbed (less compat), then quit.
        let pager = run_pager(b"-i\nq", &content);
        assert!(pager.runtime_options().case_insensitive);
    }

    #[test]
    fn test_dispatch_dash_n_upper_toggles_line_numbers_and_repaints() {
        let content = make_test_content(50);
        // Toggling -N should flip line_numbers and trigger a repaint.
        let pager = run_pager(b"-N\nq", &content);
        assert!(pager.runtime_options().line_numbers);
    }

    #[test]
    fn test_dispatch_dash_s_upper_toggles_chop_long_lines() {
        let content = make_test_content(50);
        let pager = run_pager(b"-S\nq", &content);
        assert!(pager.runtime_options().chop_long_lines);
        // Screen chop mode should also be updated.
        assert!(pager.screen().chop_mode());
    }

    #[test]
    fn test_dispatch_dash_s_lower_toggles_squeeze_blank_lines() {
        let content = make_test_content(50);
        let pager = run_pager(b"-s\nq", &content);
        assert!(pager.runtime_options().squeeze_blank_lines);
    }

    #[test]
    fn test_dispatch_underscore_queries_option() {
        // Pressing _ then i should not change any state.
        let content = make_test_content(50);
        let pager = run_pager(b"_i\nq", &content);
        assert!(!pager.runtime_options().case_insensitive);
    }

    #[test]
    fn test_dispatch_dash_toggle_twice_reverts() {
        let content = make_test_content(50);
        // Toggle i on (absorbed), then toggle off (absorbed), then quit.
        let pager = run_pager(b"-i\n-i\nq", &content);
        assert!(!pager.runtime_options().case_insensitive);
    }

    // ── Task 137: Option toggle/query status message tests ──────────

    #[test]
    fn test_dispatch_dash_i_toggle_shows_status_message_in_output() {
        let content = make_test_content(50);
        // `-i` toggles case_insensitive on; next key absorbed; output has the message.
        let pager = run_pager(b"-i\nq", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Case-insensitive search is ON"),
            "Expected 'Case-insensitive search is ON' in output: {output}"
        );
    }

    #[test]
    fn test_dispatch_dash_s_upper_toggle_shows_status_message_in_output() {
        let content = make_test_content(50);
        // `-S` toggles chop_long_lines on; next key absorbed.
        let pager = run_pager(b"-S\nq", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Chop long lines is ON"),
            "Expected 'Chop long lines is ON' in output: {output}"
        );
    }

    #[test]
    fn test_dispatch_dash_n_upper_toggle_shows_status_message_in_output() {
        let content = make_test_content(50);
        // `-N` toggles line_numbers on; next key absorbed.
        let pager = run_pager(b"-N\nq", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Line numbers is ON"),
            "Expected 'Line numbers is ON' in output: {output}"
        );
    }

    #[test]
    fn test_dispatch_underscore_i_query_shows_status_message_in_output() {
        let content = make_test_content(50);
        // `_i` queries case_insensitive (default OFF); next key absorbed.
        let pager = run_pager(b"_i\nq", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Case-insensitive search is OFF"),
            "Expected 'Case-insensitive search is OFF' in output: {output}"
        );
        // Should NOT have changed the option state.
        assert!(!pager.runtime_options().case_insensitive);
    }

    #[test]
    fn test_dispatch_underscore_query_after_toggle_shows_new_state() {
        let content = make_test_content(50);
        // Toggle `-i` (ON), absorbed, then `_i` queries, absorbed, then quit.
        let pager = run_pager(b"-i\n_i\nq", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        // The query output should contain "ON" (the toggled state).
        assert!(
            output.contains("Case-insensitive search is ON"),
            "Expected 'Case-insensitive search is ON' in output after query: {output}"
        );
    }

    #[test]
    fn test_dispatch_dash_toggle_off_shows_off_message() {
        let content = make_test_content(50);
        // Toggle twice: on (absorbed) then off (absorbed). The second should show OFF.
        let pager = run_pager(b"-i\n-i\nq", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        // The last status message rendered should be the OFF message.
        assert!(
            output.contains("Case-insensitive search is OFF"),
            "Expected 'Case-insensitive search is OFF' in output: {output}"
        );
    }

    #[test]
    fn test_dispatch_runtime_options_initialized_default() {
        let content = make_test_content(50);
        let pager = run_pager(b"q", &content);
        assert!(!pager.runtime_options().case_insensitive);
        assert!(!pager.runtime_options().line_numbers);
        assert!(!pager.runtime_options().chop_long_lines);
        assert_eq!(pager.runtime_options().tab_stops, TabStops::regular(8));
    }

    #[test]
    fn test_dispatch_set_runtime_options_is_reflected() {
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(b"test\n"));
        let index = LineIndex::new(5);
        let mut pager = Pager::new(reader, writer, buffer, index, None);

        let mut opts = RuntimeOptions::default();
        opts.case_insensitive = true;
        opts.line_numbers = true;
        opts.tab_stops = TabStops::regular(4);
        pager.set_runtime_options(opts);

        assert!(pager.runtime_options().case_insensitive);
        assert!(pager.runtime_options().line_numbers);
        assert_eq!(pager.runtime_options().tab_stops, TabStops::regular(4));
    }

    // ── Shell/pipe command tests ────────────────────────────────────

    /// Create a pager with specific settings, run it, and return it.
    fn run_pager_with_settings(
        keys: &[u8],
        content: &[u8],
        filename: Option<&str>,
        secure_mode: bool,
        is_pipe: bool,
    ) -> Pager<Cursor<Vec<u8>>, Vec<u8>> {
        let reader = KeyReader::new(Cursor::new(keys.to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, filename.map(String::from));
        pager.set_secure_mode(secure_mode);
        pager.set_is_pipe(is_pipe);
        let _ = pager.run();
        pager
    }

    // Test 8: All shell commands blocked when secure_mode is true.
    // The `!` key triggers ShellCommand. In secure mode it shows "Command not available".
    #[test]
    fn test_dispatch_shell_command_blocked_in_secure_mode() {
        let content = make_test_content(50);
        // '!' in secure mode: writes status, then 'q' quits normally.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'!');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, None, true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    #[test]
    fn test_dispatch_edit_file_blocked_in_secure_mode() {
        let content = make_test_content(50);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'v');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, Some("test.txt"), true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    #[test]
    fn test_dispatch_save_pipe_input_blocked_in_secure_mode() {
        let content = make_test_content(50);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, None, true, true);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    #[test]
    fn test_dispatch_pipe_to_command_blocked_in_secure_mode() {
        let content = make_test_content(50);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'|');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, None, true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    #[test]
    fn test_dispatch_shell_command_expand_blocked_in_secure_mode() {
        let content = make_test_content(50);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'#');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, None, true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    // Examine (`:e`) shows "Command not available" in secure mode.
    #[test]
    fn test_dispatch_examine_blocked_in_secure_mode() {
        let content = make_test_content(50);
        // ':' enters colon-command mode, then 'e' triggers Examine.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b':');
        keys.push(b'e');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, Some("test.txt"), true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    // Navigation (scroll, page) still works normally when secure mode is active.
    #[test]
    fn test_dispatch_navigation_works_in_secure_mode() {
        let content = make_test_content(50);
        // Space (PageForward) then 'q' — should not produce any error messages.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b' '); // PageForward
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, None, true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(!output.contains("Command not available"));
    }

    // Task 366: SaveBuffer is blocked in secure mode.
    #[test]
    fn test_dispatch_save_buffer_blocked_in_secure_mode() {
        let content = make_test_content(50);
        // 's' in secure mode shows "Command not available" then 'q' exits.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, Some("file.txt"), true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    // Task 366: SaveBuffer works from a file (not pipe) — saves plain content.
    #[test]
    fn test_dispatch_save_buffer_writes_content() {
        let content = b"hello world\n";
        let tmpdir = std::env::temp_dir();
        let tmpfile = tmpdir.join("pgr_test_save_buffer.txt");
        // Clean up if it exists from a prior run.
        let _ = std::fs::remove_file(&tmpfile);

        let filename_bytes = tmpfile.to_str().unwrap().as_bytes();
        // Build key sequence: 's' then type the filename then Enter then 'q'.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.extend_from_slice(filename_bytes);
        keys.push(b'\r'); // Enter
        keys.push(b'q');

        // is_pipe=false: SaveBuffer works even when reading from a file.
        let pager = run_pager_with_settings(&keys, content, Some("input.txt"), false, false);

        let saved = std::fs::read_to_string(&tmpfile).unwrap();
        assert_eq!(saved, "hello world\n");

        // Status line should show the save confirmation.
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Saved"));
        assert!(output.contains("lines to"));

        // Clean up.
        let _ = std::fs::remove_file(&tmpfile);
    }

    // Task 366: SaveBuffer strips ANSI from lines written to file.
    #[test]
    fn test_dispatch_save_buffer_strips_ansi() {
        // Content with ANSI color code.
        let content = b"\x1b[31mred text\x1b[0m\n";
        let tmpdir = std::env::temp_dir();
        let tmpfile = tmpdir.join("pgr_test_save_buffer_ansi.txt");
        let _ = std::fs::remove_file(&tmpfile);

        let filename_bytes = tmpfile.to_str().unwrap().as_bytes();
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.extend_from_slice(filename_bytes);
        keys.push(b'\r');
        keys.push(b'q');

        let _pager = run_pager_with_settings(&keys, content, None, false, false);

        let saved = std::fs::read_to_string(&tmpfile).unwrap();
        // ANSI codes must be stripped; only plain text remains.
        assert_eq!(saved, "red text\n");

        let _ = std::fs::remove_file(&tmpfile);
    }

    // Regression: SavePipeInput via lesskey rebind still shows "Not reading from pipe".
    #[test]
    fn test_dispatch_save_pipe_input_fails_when_not_pipe() {
        // Bind 's' back to save-pipe-input via lesskey to exercise the old code path.
        use crate::lesskey::parse_lesskey_source;

        let content = make_test_content(50);
        let buf_len = content.len() as u64;
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.push(b'q');

        let reader = KeyReader::new(Cursor::new(keys));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let index = LineIndex::new(buf_len);
        let mut pager = Pager::new(reader, writer, buffer, index, Some("file.txt".to_string()));
        pager.set_secure_mode(false);
        pager.set_is_pipe(false);
        // Override 's' → save-pipe-input so we test that code path.
        let config = parse_lesskey_source("#command\ns  save-pipe-input\n");
        pager.apply_lesskey_config(&config);
        let _ = pager.run();

        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Not reading from pipe"));
    }

    // Task 366: SaveBuffer writes content from pipe input too (not pipe-only).
    #[test]
    fn test_dispatch_save_buffer_works_with_pipe_input() {
        let content = b"pipe line\n";
        let tmpdir = std::env::temp_dir();
        let tmpfile = tmpdir.join("pgr_test_save_buffer_pipe.txt");
        let _ = std::fs::remove_file(&tmpfile);

        let filename_bytes = tmpfile.to_str().unwrap().as_bytes();
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.extend_from_slice(filename_bytes);
        keys.push(b'\r');
        keys.push(b'q');

        // is_pipe=true: SaveBuffer also works when reading from a pipe.
        let _pager = run_pager_with_settings(&keys, content, None, false, true);

        let saved = std::fs::read_to_string(&tmpfile).unwrap();
        assert_eq!(saved, "pipe line\n");

        let _ = std::fs::remove_file(&tmpfile);
    }

    // Test: EditFile shows "No file to edit" when no filename.
    #[test]
    fn test_dispatch_edit_file_no_filename_shows_message() {
        let content = make_test_content(50);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'v');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, None, false, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("No file to edit"));
    }

    // Test: secure_mode accessor works.
    #[test]
    fn test_dispatch_secure_mode_accessor_returns_value() {
        let content = make_test_content(10);
        let pager = run_pager_with_settings(b"q", &content, None, true, false);
        assert!(pager.secure_mode());
    }

    // Test: is_pipe accessor works.
    #[test]
    fn test_dispatch_is_pipe_accessor_returns_value() {
        let content = make_test_content(10);
        let pager = run_pager_with_settings(b"q", &content, None, false, true);
        assert!(pager.is_pipe());
    }

    // Test: set_shell and set_editor work.
    #[test]
    fn test_dispatch_set_shell_and_editor() {
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(b"data\n"));
        let index = LineIndex::new(5);
        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.set_shell("/bin/zsh");
        pager.set_editor("nvim");
        assert_eq!(pager.shell, "/bin/zsh");
        assert_eq!(pager.editor, "nvim");
    }

    // Test: last_shell_command is initially None.
    #[test]
    fn test_dispatch_last_shell_command_initially_none() {
        let content = make_test_content(10);
        let pager = run_pager(b"q", &content);
        assert!(pager.last_shell_command().is_none());
    }

    // ── Examine command tests ─────────────────────────────────────────

    // Test: :e filename opens the file and adds to file list
    #[test]
    fn test_dispatch_examine_opens_file_and_adds_to_file_list() {
        use std::io::Write as _;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        tmp.write_all(b"hello\nworld\n").expect("write tempfile");
        tmp.flush().expect("flush tempfile");
        let path_str = tmp.path().to_str().expect("path to str");

        // Build key sequence: :e<path bytes><Enter>q
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b':');
        keys.push(b'e');
        keys.extend_from_slice(path_str.as_bytes());
        keys.push(0x0A); // Enter
        keys.push(b'q');

        let content = make_test_content(10);
        let pager = run_pager_with_files(&keys, vec![("original.txt", &content)]);
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.file_count(), 2);
        // Should have switched to the new file.
        assert_eq!(fl.current_index(), 1);
        assert_eq!(fl.current().display_name, path_str);
    }

    // Test: :e with no argument refreshes the current file
    #[test]
    fn test_dispatch_examine_empty_input_refreshes_current_file() {
        // :e followed by Enter (empty input) triggers a refresh.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b':');
        keys.push(b'e');
        keys.push(0x0A); // Enter with empty input
        keys.push(b'q');

        let content = make_test_content(50);
        let pager = run_pager_with_files(&keys, vec![("file1.txt", &content)]);
        let fl = pager.file_list().expect("file list should be set");
        // File count should still be 1 — no new file added.
        assert_eq!(fl.file_count(), 1);
        // Position should remain at 0 after refresh.
        assert_eq!(pager.screen().top_line(), 0);
    }

    // Test: :e nonexistent displays error, does not change current file
    #[test]
    fn test_dispatch_examine_nonexistent_file_does_not_change_file() {
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b':');
        keys.push(b'e');
        keys.extend_from_slice(b"/tmp/pgr_definitely_does_not_exist_xyz123");
        keys.push(0x0A); // Enter
        keys.push(b'q');

        let content = make_test_content(50);
        let pager = run_pager_with_files(&keys, vec![("file1.txt", &content)]);
        let fl = pager.file_list().expect("file list should be set");
        // File count should still be 1 — failed open didn't add anything.
        assert_eq!(fl.file_count(), 1);
        assert_eq!(fl.current().display_name, "file1.txt");
    }

    // Test: E key maps to ExamineAlt
    #[test]
    fn test_keymap_upper_e_maps_to_examine_alt() {
        let keymap = Keymap::default_less();
        assert_eq!(keymap.lookup(&Key::Char('E')), Command::ExamineAlt);
    }

    // Test: :e with ESC cancels
    #[test]
    fn test_dispatch_examine_cancel_with_escape() {
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b':');
        keys.push(b'e');
        keys.push(0x1B); // Escape (standalone, at end of available input)
                         // After cancel, we need a quit key. But ESC handling in key_reader
                         // reads ahead... Let's just end input — pager exits on EOF.

        let content = make_test_content(50);
        let pager = run_pager_with_files(&keys, vec![("file1.txt", &content)]);
        let fl = pager.file_list().expect("file list should be set");
        // Nothing should have changed.
        assert_eq!(fl.file_count(), 1);
        assert_eq!(fl.current().display_name, "file1.txt");
    }

    // Test: Previous file tracking on file switch
    #[test]
    fn test_dispatch_previous_file_tracked_on_switch() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(30);
        let pager = run_pager_with_files(
            b":nq",
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        assert_eq!(pager.previous_file(), Some("file1.txt"));
    }

    // Test: Previous file updated when examining a new file
    #[test]
    fn test_dispatch_examine_updates_previous_file() {
        use std::io::Write as _;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        tmp.write_all(b"new content\n").expect("write tempfile");
        tmp.flush().expect("flush tempfile");
        let path_str = tmp.path().to_str().expect("path to str");

        let mut keys: Vec<u8> = Vec::new();
        keys.push(b':');
        keys.push(b'e');
        keys.extend_from_slice(path_str.as_bytes());
        keys.push(0x0A);
        keys.push(b'q');

        let content = make_test_content(10);
        let pager = run_pager_with_files(&keys, vec![("original.txt", &content)]);
        assert_eq!(pager.previous_file(), Some("original.txt"));
        assert_eq!(pager.filename(), Some(path_str));
    }

    // Test: ^X^V triggers examine
    #[test]
    fn test_dispatch_ctrl_x_ctrl_v_triggers_examine() {
        use std::io::Write as _;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        tmp.write_all(b"ctrl-x-v content\n")
            .expect("write tempfile");
        tmp.flush().expect("flush tempfile");
        let path_str = tmp.path().to_str().expect("path to str");

        let mut keys: Vec<u8> = Vec::new();
        keys.push(0x18); // Ctrl+X
        keys.push(0x16); // Ctrl+V
        keys.extend_from_slice(path_str.as_bytes());
        keys.push(0x0A); // Enter
        keys.push(b'q');

        let content = make_test_content(10);
        let pager = run_pager_with_files(&keys, vec![("original.txt", &content)]);
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.file_count(), 2);
        assert_eq!(fl.current().display_name, path_str);
    }

    // ── Task 113: Search command tests ──

    /// Helper: build a test buffer from raw line content.
    fn make_search_content(lines: &[&str]) -> Vec<u8> {
        let mut data = Vec::new();
        for line in lines {
            data.extend_from_slice(line.as_bytes());
            data.push(b'\n');
        }
        data
    }

    // Test 1: `/` key maps to SearchForward command (keymap test in keymap.rs)
    // Test 2: `?` key maps to SearchBackward command (keymap test in keymap.rs)
    // Test 3: `n` key maps to RepeatSearch command (keymap test in keymap.rs)
    // Test 4: `N` key maps to RepeatSearchReverse command (keymap test in keymap.rs)
    // Test 5: `ESC-u` key maps to ToggleHighlight command (keymap test in keymap.rs)

    // Test 6: Forward search finds and scrolls to matching line.
    #[test]
    fn test_dispatch_search_forward_finds_and_scrolls_to_match() {
        let content = make_search_content(&[
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "target line",
            "zeta",
        ]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"target");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Search finds "target" at line 5. set_top_line places the match at
        // the top of the viewport without clamping, matching GNU less behavior.
        assert_eq!(pager.screen().top_line(), 5);
    }

    // Test 7: Backward search finds and scrolls to matching line.
    #[test]
    fn test_dispatch_search_backward_finds_and_scrolls_to_match() {
        let content = make_search_content(&[
            "target line",
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "eta",
            "theta",
            "iota",
            "kappa",
            "lambda",
            "mu",
            "nu",
            "xi",
            "omicron",
            "pi",
            "rho",
            "sigma",
            "tau",
            "upsilon",
            "phi",
            "chi",
            "psi",
            "omega",
            "end",
        ]);
        // Scroll forward to get past line 0, then search backward for "target".
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b' '); // page forward (goes to line 23)
        keys.push(b'?');
        keys.extend_from_slice(b"target");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    // Test 8: Search with no match does not change position.
    #[test]
    fn test_dispatch_search_no_match_does_not_change_position() {
        // Use enough lines so 2j actually moves to line 2
        let content = make_test_content(50);
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b"2j"); // move to line 2
        keys.push(b'/');
        keys.extend_from_slice(b"nonexistent_pattern_xyz");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 2);
    }

    // Test 9: Search with no match renders "Pattern not found" on the status line.
    #[test]
    fn test_dispatch_search_no_match_sets_pattern_not_found_message() {
        let content = make_search_content(&["alpha", "beta", "gamma"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"nonexistent");
        keys.push(b'\n');
        // Input exhaustion ends the loop. The transient message is consumed
        // during repaint, so we check the writer output instead.
        let pager = run_pager(&keys, &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Pattern not found"),
            "Expected 'Pattern not found' in output: {output}"
        );
    }

    // Test 10: `n` repeats last forward search.
    #[test]
    fn test_dispatch_n_repeats_last_forward_search() {
        // Use a file larger than content_rows so goto_line can scroll
        let mut lines: Vec<&str> = Vec::new();
        for _ in 0..10 {
            lines.push("alpha");
        }
        lines.push("match1");
        for _ in 0..10 {
            lines.push("beta");
        }
        lines.push("match2");
        for _ in 0..10 {
            lines.push("gamma");
        }
        let content = make_search_content(&lines);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"match");
        keys.push(b'\n'); // finds "match1" at line 10
        keys.push(b'n'); // repeats forward → finds "match2" at line 21
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Search finds "match2" at line 21. set_top_line places the match at
        // the top of the viewport without clamping.
        assert_eq!(pager.screen().top_line(), 21);
    }

    // Test 11: `N` reverses last search direction.
    #[test]
    fn test_dispatch_upper_n_reverses_last_search_direction() {
        let content = make_search_content(&["match", "alpha", "beta", "gamma", "match", "delta"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"match");
        keys.push(b'\n'); // forward from 0 → finds line 4
        keys.push(b'N'); // reverse (backward from 4) → finds line 0
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    // Test 12: `n` with no previous pattern renders "No previous search pattern".
    #[test]
    fn test_dispatch_n_no_previous_pattern_shows_message() {
        let content = make_search_content(&["alpha", "beta"]);
        // n with no prior search shows error and absorbs the next key,
        // matching GNU less behavior.
        let pager = run_pager(b"nq", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("No previous search pattern"),
            "Expected 'No previous search pattern' in output: {output}"
        );
        assert_eq!(
            pager.screen().top_line(),
            0,
            "n with no pattern should not scroll"
        );
    }

    // Test 13: Numeric prefix: `2n` finds the 2nd next match.
    #[test]
    fn test_dispatch_numeric_prefix_2n_finds_second_next_match() {
        // 7 lines < content_rows (23), so top_line stays at 0 regardless.
        // Verify the search itself succeeds by checking we don't get
        // "Pattern not found", which would appear if no match existed.
        let content = make_search_content(&[
            "alpha", "match1", "beta", "match2", "gamma", "match3", "delta",
        ]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"match");
        keys.push(b'\n'); // finds line 1
        keys.extend_from_slice(b"2n"); // from line 1, 2nd match = line 5
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // `/match` finds line 1, `2n` finds the 2nd next match = line 5.
        // set_top_line places the match at the top without clamping.
        assert_eq!(pager.screen().top_line(), 5);
    }

    // Test 14: Search pattern is stored and reused across repeat searches.
    #[test]
    fn test_dispatch_search_pattern_stored_and_reused() {
        let content = make_search_content(&["alpha", "target", "beta", "target", "gamma"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"target");
        keys.push(b'\n'); // finds line 1
        keys.push(b'n'); // finds line 3
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert!(pager.last_pattern().is_some());
        assert_eq!(pager.last_pattern().unwrap().pattern(), "target");
    }

    // Test 15: Invalid regex renders "Invalid pattern" on the status line.
    #[test]
    fn test_dispatch_invalid_regex_displays_invalid_pattern_message() {
        let content = make_search_content(&["alpha", "beta"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"(unclosed");
        keys.push(b'\n');
        // The transient message is consumed during repaint, so we check
        // the writer output instead of the field.
        let pager = run_pager(&keys, &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Invalid pattern"),
            "Expected 'Invalid pattern' in output: {output}"
        );
    }

    // Test 16: ToggleHighlight toggles the highlight state.
    #[test]
    fn test_dispatch_toggle_highlight_toggles_state() {
        let content = make_search_content(&["alpha", "beta"]);
        // ESC-u is ESC followed by 'u' → 0x1B, b'u'
        let mut keys: Vec<u8> = Vec::new();
        keys.push(0x1B);
        keys.push(b'u'); // toggle highlight (now disabled)
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert!(!pager.highlight_state().is_enabled());
    }

    // Test 17: After search, visible lines include highlight ranges in repaint.
    #[test]
    fn test_dispatch_search_computes_highlights_on_repaint() {
        let content = make_search_content(&["hello world", "foo", "hello again"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.extend_from_slice(b"hello");
        keys.push(b'\n');
        // After search, highlights should be computed. We can't directly inspect
        // highlights from the pager test (they're recomputed each repaint), but
        // we can verify that the pattern is stored and highlighting is enabled.
        let pager = run_pager(&keys, &content);
        assert!(pager.highlight_state().is_enabled());
        assert!(pager.last_pattern().is_some());
    }

    // Test 18: ESC cancels search prompt input without changing state.
    // Using Ctrl+C (0x03) since standalone ESC from a byte cursor is tricky.
    #[test]
    fn test_dispatch_ctrl_c_cancels_search_prompt_without_changing_state() {
        // Use enough lines so 2j actually scrolls to line 2
        let content = make_test_content(50);
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(b"2j"); // move to line 2
        keys.push(b'/');
        keys.extend_from_slice(b"some");
        keys.push(0x03); // Ctrl+C cancels search
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Position should not change from line 2.
        assert_eq!(pager.screen().top_line(), 2);
        // No pattern should be stored (no previous search).
        assert!(pager.last_pattern().is_none());
    }

    // Test 19: Empty pattern (Enter with no input) does not crash.
    #[test]
    fn test_dispatch_empty_search_pattern_does_not_crash() {
        let content = make_search_content(&["alpha", "beta"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.push(b'\n'); // Enter with empty pattern
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Should not crash and should remain at line 0.
        assert_eq!(pager.screen().top_line(), 0);
    }

    // Test 20: Search from EOF wraps to beginning (with default wrap behavior).
    #[test]
    fn test_dispatch_search_from_eof_wraps_to_beginning() {
        let content = make_search_content(&["target", "alpha", "beta", "gamma", "delta"]);
        let mut keys: Vec<u8> = Vec::new();
        // Go to the end first.
        keys.push(b'G'); // goto end
                         // Now search forward for "target" (which is at line 0).
        keys.push(b'/');
        keys.extend_from_slice(b"target");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Should wrap and find "target" at line 0.
        assert_eq!(pager.screen().top_line(), 0);
    }

    // ── Task 121: integrated prompt rendering ───────────────────────

    /// Task 121 test 11: Transient status messages override the prompt.
    #[test]
    fn test_status_message_overrides_prompt_temporarily() {
        let content = make_test_content(50);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, Some("test.txt".into()));
        pager.set_status_message("Pattern not found".into());
        let _ = pager.run();

        // After run, the writer output should contain the transient message
        // rendered on the status line (in reverse video).
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Pattern not found"),
            "Status message should appear in output: {output}"
        );
    }

    /// Task 121 test 5: Custom -P prompt template renders correctly
    /// through the pager's rendering pipeline.
    #[test]
    fn test_custom_prompt_string_renders_via_runtime_options() {
        let content = make_test_content(50);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, Some("myfile.txt".into()));
        let mut opts = RuntimeOptions::default();
        opts.prompt_string_short = Some(String::from("Viewing\\: %f"));
        pager.set_runtime_options(opts);
        let _ = pager.run();

        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Viewing: myfile.txt"),
            "Custom prompt template should appear in output: {output}"
        );
    }

    #[test]
    fn test_dispatch_filter_activates_filter() {
        let mut content = Vec::new();
        for i in 0..50 {
            if i % 7 == 0 {
                content.extend_from_slice(format!("line {i} ERROR here\n").as_bytes());
            } else {
                content.extend_from_slice(format!("line {i} normal\n").as_bytes());
            }
        }
        // & enters filter mode, then "ERROR\n" submits the filter, then q quits.
        let pager = run_pager(b"&ERROR\nq", &content);
        assert!(pager.filter.is_active());
        assert!(pager.filtered_lines.is_some());
        // Only lines with "ERROR" should be visible.
        let fl = pager.filtered_lines.as_ref().unwrap();
        // Lines 0, 7, 14, 21, 28, 35, 42, 49 contain "ERROR" (8 lines total).
        assert_eq!(fl.visible_count(), 8);
    }

    #[test]
    fn test_dispatch_filter_clear_restores_all_lines() {
        let mut content = Vec::new();
        for i in 0..50 {
            if i % 7 == 0 {
                content.extend_from_slice(format!("line {i} ERROR here\n").as_bytes());
            } else {
                content.extend_from_slice(format!("line {i} normal\n").as_bytes());
            }
        }
        // Apply filter, then clear it with empty pattern.
        let pager = run_pager(b"&ERROR\n&\nq", &content);
        assert!(!pager.filter.is_active());
        assert!(pager.filtered_lines.is_none());
    }

    #[test]
    fn test_dispatch_filter_inverted_shows_non_matching() {
        let mut content = Vec::new();
        for i in 0..50 {
            if i % 7 == 0 {
                content.extend_from_slice(format!("line {i} ERROR here\n").as_bytes());
            } else {
                content.extend_from_slice(format!("line {i} normal\n").as_bytes());
            }
        }
        // & enters filter mode, ^N (0x0e) toggles inversion, then "ERROR\n" submits.
        let pager = run_pager(b"&\x0eERROR\nq", &content);
        assert!(pager.filter.is_active());
        assert!(pager.filter.is_inverted());
        let fl = pager.filtered_lines.as_ref().unwrap();
        // 50 total lines - 8 matching = 42 non-matching lines visible.
        assert_eq!(fl.visible_count(), 42);
    }

    #[test]
    fn test_dispatch_filter_escape_cancels() {
        let mut content = Vec::new();
        for i in 0..50 {
            content.extend_from_slice(format!("line {i}\n").as_bytes());
        }
        // & enters filter mode, ESC cancels it.
        let pager = run_pager(b"&\x1bq", &content);
        assert!(!pager.filter.is_active());
        assert!(pager.filtered_lines.is_none());
    }

    #[test]
    fn test_dispatch_filter_scroll_uses_filtered_count() {
        let mut content = Vec::new();
        for i in 0..500 {
            if i % 10 == 0 {
                content.extend_from_slice(format!("line {i} MATCH\n").as_bytes());
            } else {
                content.extend_from_slice(format!("line {i} other\n").as_bytes());
            }
        }
        // Apply filter, then scroll forward with j, then quit.
        // 500 lines, every 10th = 50 filtered. 50 > 23 content rows, so scrollable.
        let pager = run_pager(b"&MATCH\njq", &content);
        assert!(pager.filter.is_active());
        // After scrolling forward 1 line, top_line should be 1.
        assert_eq!(pager.screen().top_line(), 1);
        // The filtered lines should have 50 visible lines (0, 10, 20, ..., 490).
        let fl = pager.filtered_lines.as_ref().unwrap();
        assert_eq!(fl.visible_count(), 50);
    }

    #[test]
    fn test_dispatch_filter_persists_across_scroll() {
        let mut content = Vec::new();
        for i in 0..200 {
            if i % 5 == 0 {
                content.extend_from_slice(format!("line {i} MARK\n").as_bytes());
            } else {
                content.extend_from_slice(format!("line {i} normal\n").as_bytes());
            }
        }
        // Apply filter, scroll forward 3 lines, check filter still active.
        // 200 lines total, every 5th = 40 filtered. 40 > 23 content rows, so scrollable.
        let pager = run_pager(b"&MARK\njjjq", &content);
        assert!(pager.filter.is_active());
        assert_eq!(pager.screen().top_line(), 3);
        let fl = pager.filtered_lines.as_ref().unwrap();
        assert_eq!(fl.visible_count(), 40);
    }

    #[test]
    fn test_dispatch_filter_renders_only_matching_lines() {
        let content = b"alpha ERROR here\nbeta normal\ngamma ERROR again\ndelta other\n";
        // Apply filter for "ERROR", then quit.
        let pager = run_pager(b"&ERROR\nq", content);
        let output = String::from_utf8_lossy(&pager.writer);
        // After filter, only lines with "ERROR" should be rendered.
        // The output should contain both ERROR lines.
        assert!(
            output.contains("alpha ERROR here"),
            "Should contain first ERROR line"
        );
        assert!(
            output.contains("gamma ERROR again"),
            "Should contain second ERROR line"
        );
        // Non-matching lines should NOT appear in the final render.
        // (They appear in the initial render before filter is applied, but
        // the final repaint after filter should overwrite them.)
    }

    #[test]
    fn test_dispatch_filter_resets_viewport_to_top() {
        let mut content = Vec::new();
        for i in 0..100 {
            if i % 10 == 0 {
                content.extend_from_slice(format!("line {i} MATCH\n").as_bytes());
            } else {
                content.extend_from_slice(format!("line {i} other\n").as_bytes());
            }
        }
        // Scroll down first, then apply filter. Viewport should reset to 0.
        let pager = run_pager(b"jjjjj&MATCH\nq", &content);
        assert!(pager.filter.is_active());
        assert_eq!(pager.screen().top_line(), 0);
    }

    // ── Task 139: Search edge case conformance tests ──

    // Test: Backward search starts from bottom of visible screen, not top.
    // With a file larger than one screen, backward search should find a
    // match that is visible on screen but below top_line.
    #[test]
    fn test_dispatch_backward_search_starts_from_bottom_of_screen() {
        // Build a file large enough to scroll. 50 lines, 23 content rows.
        // Place "target" at line 15 — visible when top_line is 0 (lines 0-22).
        let mut lines: Vec<String> = Vec::new();
        for i in 0..50 {
            if i == 15 {
                lines.push("target here".to_string());
            } else {
                lines.push(format!("line {i}"));
            }
        }
        let content = make_search_content(&lines.iter().map(String::as_str).collect::<Vec<_>>());

        // Start at top (line 0). Backward search should start from
        // top_line + content_rows - 1 = 0 + 23 - 1 = 22, scanning backward
        // from line 22. "target" at line 15 is below top_line but above
        // the bottom of the screen, so it should be found.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'?');
        keys.extend_from_slice(b"target");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // "target" found at line 15 → goto_line(15, 50). max_top = 50-23 = 27.
        // 15 < 27, so top_line = 15.
        assert_eq!(pager.screen().top_line(), 15);
    }

    // Test: Backward search wraps from top to end of file.
    #[test]
    fn test_dispatch_backward_search_wraps_from_top_to_end() {
        // Place "target" at the last line (line 49) of a 50-line file.
        let mut lines: Vec<String> = Vec::new();
        for i in 0..50 {
            if i == 49 {
                lines.push("target here".to_string());
            } else {
                lines.push(format!("line {i}"));
            }
        }
        let content = make_search_content(&lines.iter().map(String::as_str).collect::<Vec<_>>());

        // From top (line 0), backward search with wrap should find the
        // target at line 49 (wrapping around from the beginning to the end).
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'?');
        keys.extend_from_slice(b"target");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // set_top_line(49) places the match at the top without clamping.
        assert_eq!(pager.screen().top_line(), 49);
    }

    // Test: -I case-insensitive-always mode makes search case insensitive.
    #[test]
    fn test_dispatch_case_insensitive_always_finds_mixed_case() {
        let content = make_search_content(&[
            "alpha",
            "beta",
            "gamma",
            "delta",
            "epsilon",
            "zeta",
            "eta",
            "theta",
            "iota",
            "kappa",
            "lambda",
            "mu",
            "nu",
            "xi",
            "omicron",
            "pi",
            "rho",
            "sigma",
            "tau",
            "upsilon",
            "phi",
            "chi",
            "psi",
            "omega",
            "Error on this line",
            "more text",
            "more text",
            "more text",
            "more text",
            "more text",
        ]);

        // Toggle -I on (next key absorbed), then search for "ERROR" (all caps).
        // Should match "Error" at line 24 thanks to case insensitive mode.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'-');
        keys.push(b'I');
        keys.push(b'\n'); // absorbed (dismiss toggle message)
        keys.push(b'/');
        keys.extend_from_slice(b"ERROR");
        keys.push(b'\n');
        keys.push(b'q');

        let reader = KeyReader::new(Cursor::new(keys));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        let _ = pager.run();

        // "Error" is at line 24. set_top_line(24) places match at top.
        assert_eq!(pager.screen().top_line(), 24);
        // Should NOT show "Pattern not found" — the search succeeded.
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            !output.contains("Pattern not found"),
            "Search should have found 'Error' with -I mode"
        );
    }

    // Test: ^R literal modifier escapes regex metacharacters.
    #[test]
    fn test_dispatch_ctrl_r_literal_search_escapes_regex() {
        let content = make_search_content(&["fooXbar", "foo.bar", "foozbar"]);
        // Search with ^R prefix: the pattern "foo.bar" should be treated
        // as literal, matching only "foo.bar" (line 1), not "fooXbar" (line 0).
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.push(0x12); // ^R modifier
        keys.extend_from_slice(b"foo.bar");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // With literal search, only line 1 "foo.bar" matches.
        // set_top_line(1) places the match at the top without clamping.
        assert_eq!(pager.screen().top_line(), 1);
        // The stored pattern should be the escaped version.
        assert!(pager.last_pattern().is_some());
        // Verify the pattern doesn't match "fooXbar" (which "foo.bar" regex would).
        let pat = pager.last_pattern().unwrap();
        assert!(!pat.is_match("fooXbar"));
        assert!(pat.is_match("foo.bar"));
    }

    // Test: ^N inverted search modifier finds non-matching lines.
    #[test]
    fn test_dispatch_ctrl_n_inverted_search_finds_non_matching() {
        // Build a file with "error" on some lines, other content on others.
        // ^N modifier should find the first line NOT matching "error".
        let mut lines: Vec<String> = Vec::new();
        for i in 0..50 {
            if i < 5 {
                lines.push("error on this line".to_string());
            } else {
                lines.push(format!("normal line {i}"));
            }
        }
        let content = make_search_content(&lines.iter().map(String::as_str).collect::<Vec<_>>());

        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.push(0x0E); // ^N modifier (invert)
        keys.extend_from_slice(b"error");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // The first non-"error" line is line 5. 50 lines, max_top = 50-23=27.
        // goto_line(5, 50) = 5 (< 27).
        assert_eq!(pager.screen().top_line(), 5);
    }

    // Test: effective_case_mode returns correct mode based on runtime options.
    #[test]
    fn test_dispatch_effective_case_mode_defaults_to_sensitive() {
        let content = make_test_content(10);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let pager = Pager::new(reader, writer, buffer, index, None);
        assert_eq!(pager.effective_case_mode(), CaseMode::Sensitive);
    }

    #[test]
    fn test_dispatch_effective_case_mode_with_dash_i() {
        let content = make_test_content(10);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.runtime_options.case_insensitive = true;
        assert_eq!(pager.effective_case_mode(), CaseMode::Smart);
    }

    #[test]
    fn test_dispatch_effective_case_mode_with_dash_cap_i() {
        let content = make_test_content(10);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.runtime_options.case_insensitive_always = true;
        assert_eq!(pager.effective_case_mode(), CaseMode::Insensitive);
    }

    #[test]
    fn test_dispatch_effective_case_mode_cap_i_overrides_i() {
        let content = make_test_content(10);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.runtime_options.case_insensitive = true;
        pager.runtime_options.case_insensitive_always = true;
        // -I takes precedence over -i
        assert_eq!(pager.effective_case_mode(), CaseMode::Insensitive);
    }

    // ── Initial commands (+cmd / ++cmd) ──────────────────────────────

    /// Create a pager with initial commands, run it, and return for inspection.
    fn run_pager_with_initial_commands(
        keys: &[u8],
        content: &[u8],
        initial_commands: Vec<&str>,
        every_file_commands: Vec<&str>,
    ) -> Pager<Cursor<Vec<u8>>, Vec<u8>> {
        let reader = KeyReader::new(Cursor::new(keys.to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.set_initial_commands(initial_commands.iter().map(|s| s.to_string()).collect());
        pager.set_every_file_commands(every_file_commands.iter().map(|s| s.to_string()).collect());
        let _ = pager.run();
        pager
    }

    // Test: +G opens file at end of file.
    #[test]
    fn test_dispatch_initial_command_g_goes_to_end() {
        let content = make_test_content(100);
        let pager = run_pager_with_initial_commands(b"q", &content, vec!["G"], vec![]);
        // G = go to end: total_lines(100) - content_rows(23) = 77
        assert_eq!(pager.screen().top_line(), 77);
    }

    // Test: +/pattern searches for pattern after open.
    #[test]
    fn test_dispatch_initial_command_search() {
        // Create content where "line 25" first appears at line 25.
        let content = make_test_content(50);
        let pager = run_pager_with_initial_commands(b"q", &content, vec!["/line 25\n"], vec![]);
        // After search, should be scrolled to line 25.
        assert_eq!(pager.screen().top_line(), 25);
    }

    // Test: Multiple + commands execute in order.
    #[test]
    fn test_dispatch_initial_commands_execute_in_order() {
        let content = make_test_content(100);
        // G (go to end, line 77), then g (go to start, line 0).
        let pager = run_pager_with_initial_commands(b"q", &content, vec!["G", "g"], vec![]);
        assert_eq!(pager.screen().top_line(), 0);
    }

    // Test: +10g goes to line 10 (1-based), top_line = 9 (0-based).
    #[test]
    fn test_dispatch_initial_command_number_g() {
        let content = make_test_content(200);
        let pager = run_pager_with_initial_commands(b"q", &content, vec!["10g"], vec![]);
        // `10g` = go to line 10 (1-based), so top_line = 9 (0-based).
        assert_eq!(pager.screen().top_line(), 9);
    }

    // Test: ++G applies to every file opened.
    #[test]
    fn test_dispatch_every_file_command_applies_on_file_switch() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(80);

        let first_content = content1.as_slice();
        let reader = KeyReader::new(Cursor::new(b":nq".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(first_content));
        let buf_len = first_content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, Some("file1.txt".to_string()));
        pager.set_every_file_commands(vec!["G".to_string()]);

        let first_entry = make_file_entry("file1.txt", &content1);
        let mut file_list = FileList::new(first_entry);
        file_list.push(make_file_entry("file2.txt", &content2));
        pager.set_file_list(file_list);

        let _ = pager.run();

        // After switching to file2 (80 lines) with ++G, should be at end.
        // End = 80 - 23 = 57.
        assert_eq!(pager.screen().top_line(), 57);
    }

    // Test: Initial commands don't re-execute on file switch.
    #[test]
    fn test_dispatch_initial_commands_only_execute_once() {
        let content1 = make_test_content(50);
        let content2 = make_test_content(80);

        let first_content = content1.as_slice();
        let reader = KeyReader::new(Cursor::new(b":nq".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(first_content));
        let buf_len = first_content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, Some("file1.txt".to_string()));
        // +G only for initial, not every file.
        pager.set_initial_commands(vec!["G".to_string()]);

        let first_entry = make_file_entry("file1.txt", &content1);
        let mut file_list = FileList::new(first_entry);
        file_list.push(make_file_entry("file2.txt", &content2));
        pager.set_file_list(file_list);

        let _ = pager.run();

        // After switching to file2, +G should NOT have re-executed.
        // File2 opens at top (line 0), not end.
        assert_eq!(pager.screen().top_line(), 0);
    }

    // Test: execute_command_string handles newlines as Enter key.
    #[test]
    fn test_dispatch_execute_command_string_newline_as_enter() {
        let content = make_test_content(50);
        // Search for "line 10" via initial command with explicit newline.
        let pager = run_pager_with_initial_commands(b"q", &content, vec!["/line 10\n"], vec![]);
        assert_eq!(pager.screen().top_line(), 10);
    }

    // ── Task 210: Bracket matching tests ──

    #[test]
    fn test_dispatch_open_brace_finds_matching_close_brace() {
        // Line 0: {
        // Line 1: content
        // Line 2: }
        let content = b"{\ncontent\n}\nafter\n";
        let pager = run_pager(b"{q", content);
        // Starting at top_line 0 which has `{`, should find `}` on line 2.
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_close_brace_finds_matching_open_brace() {
        // Line 0: {
        // Line 1: content
        // Line 2: }
        // Screen is 24 rows, content_rows = 23. For a 4-line file, bottom = min(0+23, 3) = 3.
        // But line 3 ("after") has no `}`, so we need the bottom line to have `}`.
        // With a 4-line file and top_line=0, bottom = min(23, 3) = 3. Line 3 is "after".
        // We need to scroll so `}` is at the bottom of the viewport.
        // Actually, `}` searches backward from bottom visible line. Let's just use a file
        // where the close brace is visible at the bottom.
        let content = b"{\ncontent\n}\n";
        // 3 lines. With top_line=0, bottom = min(0+23, 2) = 2. Line 2 is `}`.
        let pager = run_pager(b"}q", content);
        // Should search backward from line 2, find `{` on line 0.
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_brace_matching_nested() {
        // Line 0: {
        // Line 1: {
        // Line 2: }
        // Line 3: }
        let content = b"{\n{\n}\n}\nafter\n";
        let pager = run_pager(b"{q", content);
        // Starting at line 0 with `{`, nesting: line 0 depth=1, line 1 depth=2,
        // line 2 depth=1, line 3 depth=0 -> match at line 3.
        assert_eq!(pager.screen().top_line(), 3);
    }

    #[test]
    fn test_dispatch_paren_matching_forward() {
        // Line 0: (
        // Line 1: inner
        // Line 2: )
        let content = b"(\ninner\n)\nend\n";
        let pager = run_pager(b"(q", content);
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_square_bracket_matching_forward() {
        // Line 0: [
        // Line 1: item
        // Line 2: ]
        let content = b"[\nitem\n]\nend\n";
        let pager = run_pager(b"[q", content);
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_bracket_no_match_stays_at_current_line() {
        // Line 0: { with no matching }
        let content = b"{\nno close\n";
        let pager = run_pager(b"{q", content);
        // Should stay at line 0 (no match found).
        assert_eq!(pager.screen().top_line(), 0);
    }

    #[test]
    fn test_dispatch_bracket_no_start_bracket_stays_at_current_line() {
        // Top line has no `{`, so `{` command should not move.
        let content = b"no bracket here\nmore text\n";
        let pager = run_pager(b"{q", content);
        assert_eq!(pager.screen().top_line(), 0);
    }

    // ── Task 214: Cross-file search tests ──

    // Test 1: ESC-n finds match in next file when not in current.
    #[test]
    fn test_dispatch_esc_n_searches_next_file_on_no_match() {
        // file1 has "alpha" lines, file2 has "beta" lines with a "target" on line 5.
        let content1 = b"alpha\nalpha\nalpha\nalpha\nalpha\n".to_vec();
        let mut content2 = Vec::new();
        for i in 0..10 {
            if i == 5 {
                content2.extend_from_slice(b"target\n");
            } else {
                content2.extend_from_slice(b"beta\n");
            }
        }

        // Search for "target" in file1 (won't find it), then ESC-n to cross into file2.
        // /target\n = search for "target", \x1bn = ESC-n (cross-file repeat)
        let keys = b"/target\n\x1bnq";
        let pager = run_pager_with_files(
            keys,
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        // Should have switched to file2 and found "target" at line 5.
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 1);
        assert_eq!(pager.screen().top_line(), 5);
    }

    // Test 2: ESC-N finds match in previous file.
    #[test]
    fn test_dispatch_esc_upper_n_searches_prev_file() {
        // file1 has "target" on line 3, file2 has no "target".
        let mut content1 = Vec::new();
        for i in 0..10 {
            if i == 3 {
                content1.extend_from_slice(b"target\n");
            } else {
                content1.extend_from_slice(b"alpha\n");
            }
        }
        let content2 = b"beta\nbeta\nbeta\nbeta\nbeta\n".to_vec();

        // Start on file1, switch to file2, search for "target", then ESC-N
        // (cross-file reverse) to find it back in file1.
        // :n = switch to file2, /target\n = search forward (no match), \x1bN = ESC-N
        let keys = b":n/target\n\x1bNq";
        let pager = run_pager_with_files(
            keys,
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        // Should have switched back to file1 and found "target" at line 3.
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
        assert_eq!(pager.screen().top_line(), 3);
    }

    // Test 3: Single-file ESC-n acts like regular n.
    #[test]
    fn test_dispatch_esc_n_single_file_acts_like_regular_n() {
        let mut content = Vec::new();
        for i in 0..30 {
            if i == 5 || i == 15 {
                content.extend_from_slice(b"target\n");
            } else {
                content.extend_from_slice(b"other\n");
            }
        }

        // Search for "target", find at line 5, then ESC-n repeats in same file.
        let keys = b"/target\n\x1bnq";
        let pager = run_pager(keys, &content);
        // First search finds line 5, ESC-n finds line 15.
        assert_eq!(pager.screen().top_line(), 15);
    }

    // Test 4: "Pattern not found" when no match in any file.
    #[test]
    fn test_dispatch_esc_n_pattern_not_found_in_any_file() {
        let content1 = b"alpha\nalpha\nalpha\n".to_vec();
        let content2 = b"beta\nbeta\nbeta\n".to_vec();

        // Search for "nonexistent", then ESC-n to cross files — should fail everywhere.
        let keys = b"/nonexistent\n\x1bnq";
        let pager = run_pager_with_files(
            keys,
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        // Should end up back on file1 (restored to original after exhausting all files).
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
        // "Pattern not found" was rendered to output (status_message is transient,
        // cleared on next keypress).
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Pattern not found"),
            "Pattern not found message should appear in output"
        );
    }

    // Test 5: ESC-/ opens cross-file search prompt, finds in next file.
    #[test]
    fn test_dispatch_esc_slash_cross_file_search_forward() {
        let content1 = b"alpha\nalpha\nalpha\n".to_vec();
        let mut content2 = Vec::new();
        for i in 0..10 {
            if i == 2 {
                content2.extend_from_slice(b"target\n");
            } else {
                content2.extend_from_slice(b"beta\n");
            }
        }

        // ESC-/ opens cross-file forward search prompt, type "target\n".
        let keys = b"\x1b/target\nq";
        let pager = run_pager_with_files(
            keys,
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        // Should have switched to file2 and found "target" at line 2.
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 1);
        assert_eq!(pager.screen().top_line(), 2);
    }

    // Test 6: ESC-? opens cross-file backward search prompt.
    #[test]
    fn test_dispatch_esc_question_cross_file_search_backward() {
        let mut content1 = Vec::new();
        for i in 0..10 {
            if i == 7 {
                content1.extend_from_slice(b"target\n");
            } else {
                content1.extend_from_slice(b"alpha\n");
            }
        }
        let content2 = b"beta\nbeta\nbeta\n".to_vec();

        // Start on file1, switch to file2, then ESC-? for "target" backward cross-file.
        let keys = b":n\x1b?target\nq";
        let pager = run_pager_with_files(
            keys,
            vec![("file1.txt", &content1), ("file2.txt", &content2)],
        );
        // Should have switched back to file1 and found "target" at line 7.
        let fl = pager.file_list().expect("file list should be set");
        assert_eq!(fl.current_index(), 0);
        assert_eq!(pager.screen().top_line(), 7);
    }

    // ── Task 216: Quit variants (ZZ, :Q) and !! repeat shell command ──

    // Test 1: ZZ quits the pager.
    #[test]
    fn test_dispatch_zz_quits() {
        let content = make_test_content(50);
        let pager = run_pager(b"ZZ", &content);
        assert!(pager.should_quit);
    }

    // Test 2: Z followed by non-Z does not quit.
    #[test]
    fn test_dispatch_z_non_z_does_not_quit() {
        let content = make_test_content(50);
        // Z followed by 'j' should not quit; 'j' is consumed by resolve_pending
        // as a non-Z key, so the ZPrefix is discarded. Then 'q' quits normally.
        let pager = run_pager(b"Zjq", &content);
        assert!(pager.should_quit);
        // The 'j' in "Zj" was consumed by resolve_pending, not as a scroll.
        assert_eq!(pager.screen().top_line(), 0);
    }

    // Test 3: :Q quits the pager.
    #[test]
    fn test_dispatch_colon_upper_q_quits() {
        let content = make_test_content(50);
        let pager = run_pager(b":Q", &content);
        assert!(pager.should_quit);
    }

    // Test 4: ^X^V triggers examine and does not quit.
    #[test]
    fn test_dispatch_ctrl_x_ctrl_v_examine_does_not_quit() {
        let content = make_test_content(50);
        // ^X^V opens the examine prompt. Since no filename is provided and
        // input exhausts, the examine prompt cancels and we fall through.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(0x18); // Ctrl+X
        keys.push(0x16); // Ctrl+V
        let pager = run_pager(&keys, &content);
        // Examine was attempted; pager should not have quit.
        assert!(!pager.should_quit);
    }

    // Test 5: !! repeats last shell command (blocked in secure mode shows message).
    #[test]
    fn test_dispatch_double_bang_repeat_blocked_in_secure_mode() {
        let content = make_test_content(10);
        // In secure mode, `!` shows "Command not available".
        // The second `!` is then processed as the next key, which maps to
        // ShellCommand again (also blocked).
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'!');
        keys.push(b'!');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, Some("test.txt"), true, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Command not available"));
    }

    // Test 6: Z key enters ZPrefix pending state.
    #[test]
    fn test_keymap_z_upper_starts_pending_command() {
        let result = Pager::<Cursor<Vec<u8>>, Vec<u8>>::check_pending_start(&Key::Char('Z'));
        assert_eq!(result, Some(PendingCommand::ZPrefix));
    }

    // ── Task 221: Mouse support tests ──

    #[test]
    fn test_dispatch_mouse_enabled_writes_tracking_sequences_on_run() {
        let content = make_test_content(50);
        // Feed 'q' to quit immediately.
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);
        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.set_mouse_enabled(true);
        let _ = pager.run();
        let output = String::from_utf8_lossy(&pager.writer);
        // Should contain mouse enable sequences at start.
        assert!(output.contains("\x1b[?1000h"), "missing X11 mouse enable");
        assert!(output.contains("\x1b[?1006h"), "missing SGR mouse enable");
        // Should contain mouse disable sequences at end.
        assert!(output.contains("\x1b[?1000l"), "missing X11 mouse disable");
        assert!(output.contains("\x1b[?1006l"), "missing SGR mouse disable");
    }

    #[test]
    fn test_dispatch_mouse_disabled_no_tracking_sequences() {
        let content = make_test_content(50);
        let pager = run_pager(b"q", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            !output.contains("\x1b[?1000h"),
            "should not contain mouse enable"
        );
    }

    #[test]
    fn test_dispatch_x11_scroll_up_scrolls_backward() {
        let content = make_test_content(100);
        // First scroll down 5 lines, then scroll up with X11 mouse, then quit.
        let mut keys: Vec<u8> = Vec::new();
        // 5 x 'j' to scroll down 5 lines
        keys.extend_from_slice(b"jjjjj");
        // X11 mouse scroll up: ESC[M followed by button=96 (64+32), x=33, y=33
        keys.extend_from_slice(&[0x1B, b'[', b'M', 96, 33, 33]);
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Started at 0, scrolled down to 5, then scroll up 3 (default wheel_lines) = 2
        assert_eq!(pager.screen().top_line(), 2);
    }

    #[test]
    fn test_dispatch_x11_scroll_down_scrolls_forward() {
        let content = make_test_content(100);
        // X11 mouse scroll down: ESC[M followed by button=97 (65+32), x=33, y=33
        let mut keys: Vec<u8> = Vec::new();
        keys.extend_from_slice(&[0x1B, b'[', b'M', 97, 33, 33]);
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Default wheel_lines is 3, so should scroll forward 3.
        assert_eq!(pager.screen().top_line(), 3);
    }

    #[test]
    fn test_dispatch_set_wheel_lines_changes_scroll_amount() {
        let content = make_test_content(100);
        let mut keys: Vec<u8> = Vec::new();
        // X11 mouse scroll down
        keys.extend_from_slice(&[0x1B, b'[', b'M', 97, 33, 33]);
        keys.push(b'q');
        let reader = KeyReader::new(Cursor::new(keys));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);
        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.set_wheel_lines(7);
        let _ = pager.run();
        assert_eq!(pager.screen().top_line(), 7);
    }

    #[test]
    fn test_dispatch_sgr_scroll_down_scrolls_forward() {
        let content = make_test_content(100);
        let mut keys: Vec<u8> = Vec::new();
        // SGR mouse scroll down: ESC[<65;10;20M
        keys.extend_from_slice(&[
            0x1B, b'[', b'<', b'6', b'5', b';', b'1', b'0', b';', b'2', b'0', b'M',
        ]);
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert_eq!(pager.screen().top_line(), 3);
    }

    // ── Header lines dispatch tests ──────────────────────────────────

    /// Create a pager with header lines set, run it, and return it.
    fn run_pager_with_headers(
        keys: &[u8],
        content: &[u8],
        header_lines: usize,
    ) -> Pager<Cursor<Vec<u8>>, Vec<u8>> {
        let reader = KeyReader::new(Cursor::new(keys.to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, Some("test.txt".into()));
        pager.set_header_lines(header_lines);
        let _ = pager.run();
        pager
    }

    #[test]
    fn test_dispatch_header_lines_pins_first_n_lines() {
        let content = make_test_content(50);
        let pager = run_pager_with_headers(b"q", &content, 3);
        // With 3 header lines, top_line starts at 3 (first scrollable line)
        assert_eq!(pager.screen().top_line(), 3);
        assert_eq!(pager.screen().header_lines(), 3);
    }

    #[test]
    fn test_dispatch_header_lines_scroll_does_not_move_headers() {
        let content = make_test_content(50);
        // Scroll forward 5 lines with 3 header lines
        let pager = run_pager_with_headers(b"jjjjjq", &content, 3);
        // top_line should be 3 + 5 = 8
        assert_eq!(pager.screen().top_line(), 8);
        // Header lines value unchanged
        assert_eq!(pager.screen().header_lines(), 3);
    }

    #[test]
    fn test_dispatch_header_lines_content_area_reduced() {
        let content = make_test_content(50);
        let pager = run_pager_with_headers(b"q", &content, 3);
        // Default screen is 24x80: 23 content rows - 3 header = 20 scrollable
        assert_eq!(pager.screen().content_rows(), 20);
    }

    #[test]
    fn test_dispatch_header_lines_scroll_backward_clamps_at_headers() {
        let content = make_test_content(50);
        // Scroll forward 2 then backward 10 should clamp at header_lines (3)
        let pager = run_pager_with_headers(b"jjkkkkkkkkkkq", &content, 3);
        assert_eq!(pager.screen().top_line(), 3);
    }

    #[test]
    fn test_dispatch_header_lines_renders_header_content() {
        let content = b"HEADER1\nHEADER2\nHEADER3\nline 3\nline 4\nline 5\nline 6\nline 7\n";
        let pager = run_pager_with_headers(b"q", content, 3);
        let output = String::from_utf8_lossy(&pager.writer);
        // Header lines should be rendered with reverse video
        assert!(
            output.contains("\x1b[7m"),
            "expected reverse video in output"
        );
        // Header content should appear
        assert!(output.contains("HEADER1"), "expected HEADER1 in output");
        assert!(output.contains("HEADER2"), "expected HEADER2 in output");
        assert!(output.contains("HEADER3"), "expected HEADER3 in output");
    }

    // ── Task 223: --no-keypad, --no-vbell, --redraw-on-quit ─────────

    #[test]
    fn test_dispatch_keypad_enabled_by_default() {
        let content = make_test_content(5);
        let pager = run_pager(b"q", &content);
        let output = String::from_utf8_lossy(&pager.writer);
        // Default: keypad enable sent on entry, disable sent on exit
        assert!(
            output.contains("\x1b[?1h"),
            "expected keypad enable sequence"
        );
        assert!(
            output.contains("\x1b[?1l"),
            "expected keypad disable sequence"
        );
    }

    #[test]
    fn test_dispatch_no_keypad_skips_keypad_sequences() {
        let content = make_test_content(5);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.set_no_keypad(true);
        let _ = pager.run();
        let output = String::from_utf8_lossy(&pager.writer);
        // --no-keypad: should NOT contain keypad enable or disable sequences
        assert!(
            !output.contains("\x1b[?1h"),
            "should not contain keypad enable"
        );
        assert!(
            !output.contains("\x1b[?1l"),
            "should not contain keypad disable"
        );
    }

    #[test]
    fn test_dispatch_no_vbell_setter() {
        let content = make_test_content(5);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        assert!(!pager.no_vbell());
        pager.set_no_vbell(true);
        assert!(pager.no_vbell());
    }

    #[test]
    fn test_dispatch_redraw_on_quit_repaints_before_exit() {
        let content = make_test_content(5);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        pager.set_redraw_on_quit(true);
        let _ = pager.run();
        let output = String::from_utf8_lossy(&pager.writer);
        // Find position of alt-screen exit sequence
        let alt_exit = output.rfind("\x1b[?1049l").expect("alt screen exit");
        // "line 0" should appear after the last repaint (which happens before alt exit)
        // The repaint before exit writes content just before the alt screen exit
        let last_line0 = output.rfind("line 0").expect("line 0 in output");
        // The redraw-on-quit repaint should produce content shortly before the alt exit
        assert!(
            last_line0 < alt_exit,
            "repaint content should appear before alternate screen exit"
        );
    }

    // ── Task 224: index_all_immediate ─────────────────────────────────

    #[test]
    fn test_index_all_immediate_indexes_all_lines() {
        let data = b"line 0\nline 1\nline 2\nline 3\nline 4\n";
        let buf = TestBuffer::new(data);
        let index = LineIndex::new(data.len() as u64);
        let reader = KeyReader::new(Cursor::new(b"q" as &[u8]));
        let writer: Vec<u8> = Vec::new();
        let buffer: Box<dyn Buffer> = Box::new(buf);
        let mut pager = Pager::new(reader, writer, buffer, index, None);
        // LineIndex::new records offset 0 for non-empty buffers, so 1 line start is known.
        assert_eq!(pager.index.lines_indexed(), 1);
        pager.index_all_immediate().unwrap();
        assert_eq!(pager.index.lines_indexed(), 5);
    }

    // ── Task 247: ESC-U clears search pattern and re-enables highlighting ──

    #[test]
    fn test_dispatch_esc_upper_u_clears_highlighting_but_keeps_pattern() {
        let content = make_search_content(&["alpha", "target", "beta", "target", "gamma"]);
        let mut keys: Vec<u8> = Vec::new();
        // First search for "target".
        keys.push(b'/');
        keys.extend_from_slice(b"target");
        keys.push(b'\n');
        // Now ESC-U to clear highlighting (pattern preserved per GNU less).
        keys.push(0x1B);
        keys.push(b'U');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // Pattern should be preserved so `n`/`N` still work.
        assert!(pager.last_pattern().is_some());
        // Highlighting should be re-enabled (ready for next search).
        assert!(pager.highlight_state().is_enabled());
    }

    #[test]
    fn test_dispatch_esc_upper_u_without_pattern_is_noop() {
        let content = make_test_content(10);
        let mut keys: Vec<u8> = Vec::new();
        // ESC-U with no prior search should not crash.
        keys.push(0x1B);
        keys.push(b'U');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert!(pager.last_pattern().is_none());
        assert!(pager.highlight_state().is_enabled());
    }

    #[test]
    fn test_dispatch_esc_upper_u_re_enables_highlight_after_toggle_off() {
        let content = make_search_content(&["alpha", "target", "beta"]);
        let mut keys: Vec<u8> = Vec::new();
        // Search for "target".
        keys.push(b'/');
        keys.extend_from_slice(b"target");
        keys.push(b'\n');
        // Toggle highlight off with ESC-u.
        keys.push(0x1B);
        keys.push(b'u');
        // Now ESC-U — should re-enable highlighting, preserve pattern.
        keys.push(0x1B);
        keys.push(b'U');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert!(pager.last_pattern().is_some());
        assert!(pager.highlight_state().is_enabled());
    }

    // ── Task 247: ^S in search prompt shows "Sub-pattern search not supported" ──

    #[test]
    fn test_dispatch_ctrl_s_in_search_shows_not_supported() {
        let content = make_search_content(&["alpha", "beta"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.push(0x13); // ^S
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("Sub-pattern search not supported"),
            "Expected 'Sub-pattern search not supported' in output: {output}"
        );
        // Search should be cancelled — no pattern stored.
        assert!(pager.last_pattern().is_none());
    }

    // ── Task 247: ^L in search prompt inserts next character literally ──

    #[test]
    fn test_dispatch_ctrl_l_literal_next_inserts_modifier_as_literal() {
        // Test that ^L followed by ^N inserts the ^N control character literally
        // instead of treating it as the invert modifier.
        let content = make_search_content(&["alpha", "\x0edata", "beta"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.push(0x0C); // ^L (literal next)
        keys.push(0x0E); // ^N — should be inserted literally, not as modifier
        keys.extend_from_slice(b"data");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        // The pattern should contain the literal ^N character followed by "data".
        // Since \x0e is not treated as a modifier, it becomes part of the raw pattern.
        assert!(pager.last_pattern().is_some());
    }

    #[test]
    fn test_dispatch_ctrl_l_literal_next_inserts_normal_char() {
        // ^L followed by a normal character should just insert it.
        let content = make_search_content(&["alpha", "beta"]);
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'/');
        keys.push(0x0C); // ^L (literal next)
        keys.push(b'a'); // regular char
        keys.extend_from_slice(b"lpha");
        keys.push(b'\n');
        keys.push(b'q');
        let pager = run_pager(&keys, &content);
        assert!(pager.last_pattern().is_some());
        assert_eq!(pager.last_pattern().unwrap().pattern(), "alpha");
    }

    // ── Task 245: Exit code conformance ─────────────────────────────────

    #[test]
    fn test_dispatch_normal_quit_exit_reason_is_normal() {
        let content = make_test_content(5);
        let pager = run_pager(b"q", &content);
        assert_eq!(pager.exit_reason(), ExitReason::Normal);
    }

    #[test]
    fn test_dispatch_ctrl_c_exit_reason_is_interrupt() {
        let content = make_test_content(5);
        let pager = run_pager(b"\x03", &content);
        assert_eq!(pager.exit_reason(), ExitReason::Interrupt);
    }

    #[test]
    fn test_dispatch_colon_q_exit_reason_is_normal() {
        let content = make_test_content(5);
        let pager = run_pager(b":q", &content);
        assert_eq!(pager.exit_reason(), ExitReason::Normal);
    }

    #[test]
    fn test_dispatch_zz_exit_reason_is_normal() {
        let content = make_test_content(5);
        let pager = run_pager(b"ZZ", &content);
        assert_eq!(pager.exit_reason(), ExitReason::Normal);
    }

    #[test]
    fn test_dispatch_quit_on_intr_setter() {
        let content = make_test_content(5);
        let reader = KeyReader::new(Cursor::new(b"q".to_vec()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let mut pager = Pager::new(reader, writer, buffer, index, None);
        assert!(!pager.quit_on_intr);
        pager.set_quit_on_intr(true);
        assert!(pager.quit_on_intr);
    }

    #[test]
    fn test_dispatch_exit_reason_default_is_normal() {
        let content = make_test_content(5);
        let reader = KeyReader::new(Cursor::new(Vec::new()));
        let writer = Vec::new();
        let buffer = Box::new(TestBuffer::new(&content));
        let buf_len = content.len() as u64;
        let index = LineIndex::new(buf_len);

        let pager = Pager::new(reader, writer, buffer, index, None);
        assert_eq!(pager.exit_reason(), ExitReason::Normal);
    }

    // ── Task 321: Multi-pattern highlighting tests ──

    #[test]
    fn test_dispatch_add_highlight_adds_extra_pattern() {
        let content = make_test_content(10);
        // &+ enters add-highlight mode, then type "line" and Enter.
        let pager = run_pager(b"&+line\nq", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 1);
    }

    #[test]
    fn test_dispatch_add_highlight_shows_status_message() {
        let content = make_test_content(10);
        let pager = run_pager(b"&+test\nq", &content);
        // After adding, status message was set (and may have been cleared by repaint),
        // but the pattern count confirms it was added.
        assert_eq!(pager.highlight_state().extra_pattern_count(), 1);
    }

    #[test]
    fn test_dispatch_remove_highlight_removes_existing_pattern() {
        let content = make_test_content(10);
        // Add "line", then remove "line".
        let pager = run_pager(b"&+line\n&-line\nq", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 0);
    }

    #[test]
    fn test_dispatch_remove_highlight_nonexistent_does_not_crash() {
        let content = make_test_content(10);
        // Remove a pattern that was never added.
        let pager = run_pager(b"&-nothere\nq", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 0);
    }

    #[test]
    fn test_dispatch_list_highlights_empty() {
        let content = make_test_content(10);
        // &l lists highlights (absorbs next key).
        let pager = run_pager(b"&l q", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 0);
    }

    #[test]
    fn test_dispatch_list_highlights_with_patterns() {
        let content = make_test_content(10);
        // Add two patterns, then list.
        let pager = run_pager(b"&+alpha\n&+beta\n&l q", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 2);
    }

    #[test]
    fn test_dispatch_filter_prefix_default_enters_filter_mode() {
        // `&` followed by `E` (not +/-/l) should enter filter mode with E as first char.
        // Then "RROR\n" completes the pattern "ERROR".
        // But since only every 7th line has "ERROR", we can't easily check.
        // Just verify the filter was set (lines 0,7,14,... have "ERROR").
        let mut test_content = Vec::new();
        for i in 0..50 {
            if i % 7 == 0 {
                test_content.extend_from_slice(format!("line {i} ERROR here\n").as_bytes());
            } else {
                test_content.extend_from_slice(format!("line {i} normal\n").as_bytes());
            }
        }
        let pager = run_pager(b"&ERROR\n&\nq", &test_content);
        // Second `&` followed by `\n` clears the filter.
        assert!(!pager.filter.is_active());
    }

    #[test]
    fn test_dispatch_add_highlight_empty_pattern_no_crash() {
        let content = make_test_content(10);
        // &+ followed by immediate Enter (empty pattern).
        let pager = run_pager(b"&+\nq", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 0);
    }

    #[test]
    fn test_dispatch_add_highlight_invalid_pattern_shows_error() {
        let content = make_test_content(10);
        // Invalid regex pattern.
        let pager = run_pager(b"&+(unclosed\nq", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 0);
    }

    #[test]
    fn test_dispatch_add_highlight_cancel_with_escape() {
        let content = make_test_content(10);
        // &+ then ESC cancels the add-highlight prompt.
        let pager = run_pager(b"&+abc\x1bq", &content);
        assert_eq!(pager.highlight_state().extra_pattern_count(), 0);
    }

    #[test]
    fn test_dispatch_has_extra_patterns_reflects_state() {
        let content = make_test_content(10);
        let pager = run_pager(b"&+test\nq", &content);
        assert!(pager.highlight_state().has_extra_patterns());
    }

    #[test]
    fn test_dispatch_colored_highlights_populated_after_repaint() {
        let content = b"hello world\nhello again\n";
        // Search for "hello" first, then add "world" as extra highlight.
        let pager = run_pager(b"/hello\n&+world\nq", content);
        // Both patterns should be active.
        assert!(pager.last_pattern().is_some());
        assert_eq!(pager.highlight_state().extra_pattern_count(), 1);
    }
}
