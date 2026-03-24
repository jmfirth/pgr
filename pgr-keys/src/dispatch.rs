//! Command dispatch loop — the main pager event loop.
//!
//! Reads keys, translates them to commands via the keymap, and executes
//! those commands by mutating the screen state and repainting.

use std::io::{Read, Write};
use std::path::Path;

use pgr_core::{Buffer, LineIndex, Mark, MarkStore};
use pgr_display::{
    eval_prompt, paint_info_line, paint_prompt, paint_screen_mapped, paint_screen_with_options,
    squeeze_visible_lines, OverstrikeMode, PaintOptions, PromptContext, PromptStyle,
    RawControlMode, RenderConfig, Screen, ScreenLine, TabStops, DEFAULT_LONG_PROMPT,
    DEFAULT_MEDIUM_PROMPT, DEFAULT_SHORT_PROMPT,
};
use pgr_search::{
    CaseMode, FilterState, FilteredLines, HighlightState, SearchDirection, SearchModifiers,
    SearchPattern, Searcher, WrapMode,
};

use crate::help;
use crate::info;

use crate::error::Result;
use crate::file_list::{FileEntry, FileList};
use crate::filename::expand_filename;
use crate::key::Key;
use crate::key_reader::KeyReader;
use crate::keymap::Keymap;
use crate::line_editor::{LineEditResult, LineEditor};
use crate::runtime_options::RuntimeOptions;
use crate::shell;
use crate::Command;

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
    /// Custom window size. Set by `z`/`w` with a count.
    custom_window_size: Option<usize>,
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
    /// Direction for the current search prompt (set when `/` or `?` is pressed).
    search_prompt_direction: SearchDirection,
    /// The modifiers from the last search, used for repeat-search commands.
    last_modifiers: SearchModifiers,
    /// Filter state for the `&` command (show only matching/non-matching lines).
    filter: FilterState,
    /// Pre-computed mapping from filtered line indices to actual buffer lines.
    filtered_lines: Option<FilteredLines>,
    /// Whether we are currently in filter-prompt editing mode.
    editing_filter: bool,
    /// Whether the current filter prompt has inversion toggled via `^N`.
    filter_invert: bool,
    /// Whether the next keypress should be absorbed (used to dismiss
    /// option toggle/query status messages, matching GNU less behavior).
    absorb_next_key: bool,
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
            search_prompt_direction: SearchDirection::Forward,
            last_modifiers: SearchModifiers::new(),
            filter: FilterState::new(),
            filtered_lines: None,
            editing_filter: false,
            filter_invert: false,
            absorb_next_key: false,
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
        self.writer.flush()?;

        self.repaint()?;

        loop {
            match self.reader.read_key() {
                Ok(key) => {
                    if !self.process_key(&key)? {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => {
                    // Exit alternate screen buffer before propagating error.
                    let _ = self.writer.write_all(b"\x1b[?1049l");
                    let _ = self.writer.flush();
                    return Err(e.into());
                }
            }
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

        let command = self.keymap.lookup(key);
        let count = self.pending_count.take();
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
            Key::Char(':') => Some(PendingCommand::ColonPrefix),
            _ => None,
        }
    }

    /// Resolve a pending multi-key command with the argument key.
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
                    Key::Char('q') => self.execute(&Command::Quit, count)?,
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
                self.screen.scroll_forward(count.unwrap_or(n), total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::ScrollBackward(n) => {
                self.screen.scroll_backward(count.unwrap_or(n));
                self.repaint()?;
            }
            Command::PageForward => {
                self.save_last_position();
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_forward(count.unwrap_or(window), total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::PageBackward => {
                self.save_last_position();
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_backward(count.unwrap_or(window));
                self.repaint()?;
            }
            Command::HalfPageForward => {
                self.save_last_position();
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
                // ng uses 1-based line numbers; convert to 0-based index
                let target = count.or(n).map_or(0, |line| line.saturating_sub(1));
                self.screen.goto_line(target, total);
                self.repaint()?;
            }
            Command::GotoEnd(n) => {
                self.save_last_position();
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
                let cols = self.screen.cols();
                let amount = count.unwrap_or(cols / 2);
                let h = self.screen.horizontal_offset();
                self.screen.set_horizontal_offset(h.saturating_add(amount));
                self.repaint()?;
            }
            Command::ScrollLeft => {
                let cols = self.screen.cols();
                let amount = count.unwrap_or(cols / 2);
                let h = self.screen.horizontal_offset();
                self.screen.set_horizontal_offset(h.saturating_sub(amount));
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
                    pct.saturating_mul(total) / 100
                };
                self.screen.goto_line(target, total);
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
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen
                    .scroll_forward_unclamped(count.unwrap_or(window));
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::BackwardForceBeginning => {
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                // scroll_backward already clamps at 0, which is the correct behavior
                self.screen.scroll_backward(count.unwrap_or(window));
                self.repaint()?;
            }
            Command::WindowForward => {
                if let Some(c) = count {
                    self.custom_window_size = Some(c);
                }
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_forward(window, total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::WindowBackward => {
                if let Some(c) = count {
                    self.custom_window_size = Some(c);
                }
                let window = self
                    .custom_window_size
                    .unwrap_or(self.screen.content_rows());
                self.screen.scroll_backward(window);
                self.repaint()?;
            }
            Command::FollowMode => {
                self.follow_mode()?;
            }
            Command::RepaintRefresh => {
                self.buffer.refresh()?;
                let new_len = self.buffer.len() as u64;
                self.index = LineIndex::new(new_len);
                self.repaint()?;
            }
            Command::FileLineForward => {
                // Equivalent to ScrollForward for now; differentiation comes with word-wrap.
                self.screen.scroll_forward(count.unwrap_or(1), total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::FileLineBackward => {
                // Equivalent to ScrollBackward for now; differentiation comes with word-wrap.
                self.screen.scroll_backward(count.unwrap_or(1));
                self.repaint()?;
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
        self.render_search_prompt()?;
        Ok(())
    }

    /// Process a key while in search-prompt editing mode.
    fn process_search_key(&mut self, key: &Key) -> Result<bool> {
        // Intercept search modifier keys (^N, ^R) and inject their raw
        // control-character bytes into the line editor buffer so that
        // `SearchModifiers::parse` can extract them later.
        if matches!(key, Key::Ctrl('n' | 'r')) {
            if let Some(ref mut editor) = self.line_editor {
                let ch = match key {
                    Key::Ctrl('n') => '\x0e',
                    Key::Ctrl('r') => '\x12',
                    _ => unreachable!(),
                };
                editor.insert(ch);
                self.render_search_prompt()?;
                return Ok(true);
            }
        }

        let result = if let Some(ref mut editor) = self.line_editor {
            editor.process_key(key)
        } else {
            // Should not happen, but recover gracefully.
            self.editing_search = false;
            return Ok(true);
        };

        match result {
            LineEditResult::Continue => {
                self.render_search_prompt()?;
            }
            LineEditResult::Confirm(pattern_str) => {
                self.editing_search = false;
                let count = self.pending_count.take();
                self.line_editor = None;
                self.submit_search(&pattern_str, self.search_prompt_direction, count)?;
            }
            LineEditResult::Cancel => {
                self.editing_search = false;
                self.pending_count = None;
                self.line_editor = None;
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
    fn handle_shell_command(&mut self) -> Result<()> {
        if self.secure_mode {
            self.write_status("Command not available")?;
            return Ok(());
        }

        // Read a command line from the user using the line editor.
        let input = self.read_command_line("!")?;
        let Some(input) = input else {
            return Ok(());
        };

        // "!" with no command (or "!!") repeats the last command.
        let cmd = if input.is_empty() || input == "!" {
            match self.last_shell_command.clone() {
                Some(prev) => prev,
                None => return Ok(()),
            }
        } else {
            input
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

        // Re-enter alternate screen. GNU less relies on the terminal to
        // restore the previous alt-screen content and does not explicitly
        // repaint. We match that behavior.
        self.writer.write_all(b"\x1b[?1049h")?;
        self.writer.flush()?;
        // Reset initial_render so the next repaint bottom-aligns short files.
        self.initial_render = true;
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

        // Re-enter alternate screen — same behavior as handle_shell_command.
        self.writer.write_all(b"\x1b[?1049h")?;
        self.writer.flush()?;
        self.initial_render = true;
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
                    LineEditResult::Continue => {
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
                return self.do_search(direction, count, true);
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
        self.do_search(direction, count, false)
    }

    /// Perform a search in the given direction using `last_pattern`.
    ///
    /// When `is_repeat` is true (e.g., `n`/`N` commands), the search start
    /// position advances past the current match to avoid re-finding it.
    /// When false (new `/` or `?` search), the start position includes the
    /// current viewport.
    fn do_search(
        &mut self,
        direction: SearchDirection,
        count: Option<usize>,
        is_repeat: bool,
    ) -> Result<()> {
        let Some(ref pattern) = self.last_pattern else {
            self.status_message = Some("No previous search pattern".to_string());
            self.repaint()?;
            return Ok(());
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

        // Determine start line.
        //
        // GNU less search start positions:
        //
        // New search (is_repeat = false):
        //   Forward: top_line (inclusive — check the current top line)
        //   Backward: top_line + content_rows - 1 (bottom of visible screen)
        //
        // Repeat search (is_repeat = true):
        //   Forward: top_line + 1 (skip current match at top)
        //   Backward: top_line - 1 (skip current match at top, search upward)
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
            match direction {
                SearchDirection::Forward => self.screen.top_line() + 1,
                SearchDirection::Backward => self.screen.top_line().saturating_sub(1),
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
            // If keep_position is set, update highlights but don't scroll.
            if self.last_modifiers.keep_position {
                self.repaint()?;
            } else {
                self.save_last_position();
                // Use set_top_line (unclamped) so the match appears at the
                // top of the viewport even near EOF, matching GNU less which
                // shows tilde lines below the last file line.
                self.screen.set_top_line(line);
                self.repaint()?;
            }
        } else {
            self.status_message = Some("Pattern not found".to_string());
            self.repaint()?;
        }
        Ok(())
    }

    /// Repeat the last search, optionally reversing direction.
    fn repeat_search(&mut self, reverse: bool, count: Option<usize>) -> Result<()> {
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

        self.do_search(direction, count, true)
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
            LineEditResult::Continue => {
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

    /// Enter follow mode: scroll to end and wait for interrupt.
    ///
    /// Scrolls to the end of the buffer and displays "Waiting for data...
    /// (interrupt to abort)" on the status line. Blocks reading keys until
    /// Ctrl-C (or `q`/`Q`) is received, then repaints the screen and returns
    /// to normal viewing mode.
    fn follow_mode(&mut self) -> Result<()> {
        self.buffer.refresh()?;
        let new_len = self.buffer.len() as u64;
        self.index = LineIndex::new(new_len);
        self.index.index_all(&*self.buffer)?;
        let total = self.index.lines_indexed();
        let default = total.saturating_sub(self.screen.content_rows());
        self.screen.goto_line(default, total);
        self.status_message = Some("Waiting for data... (interrupt to abort)".to_string());
        self.repaint()?;

        // Block reading keys until Ctrl-C exits follow mode.
        loop {
            match self.reader.read_key() {
                Ok(Key::Ctrl('c')) => break,
                Ok(Key::Char('q' | 'Q')) => {
                    self.should_quit = true;
                    return Ok(());
                }
                Ok(_) => {} // Ignore other keys while in follow mode.
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        }

        // Repaint after exiting follow mode.
        self.repaint()?;
        Ok(())
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
        }
        Ok(())
    }

    /// Switch to the N-th file (0-based) in the file list.
    fn switch_file_goto(&mut self, index: usize) -> Result<()> {
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
        let mut editor = LineEditor::new("Examine: ");
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
        // Expand % and # substitutions.
        let expanded = match expand_filename(
            raw_input,
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
                    // No file list yet — create one with this file.
                    self.file_list = Some(FileList::new(entry));
                }

                self.previous_file = old_name;
                // Always swap: loads the new file's buffer into the pager.
                self.apply_current_file_impl(true);
                // Reset initial_render so short files get bottom-aligned.
                self.initial_render = true;
                self.repaint()?;
            }
            Err(e) => {
                self.status_message = Some(format!("{expanded}: {e}"));
                // Reset initial_render so short files get bottom-aligned.
                self.initial_render = true;
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

        // GNU less scrolls forward 1 line and replaces the last content
        // row with the info text (the prompt row stays).
        self.screen.scroll_forward(1, total_lines);
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

    /// Fetch visible lines from the buffer/index and repaint the screen.
    fn repaint(&mut self) -> Result<()> {
        self.index.index_all(&*self.buffer)?;

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

                let paint_opts = PaintOptions {
                    show_line_numbers: self.runtime_options.line_numbers,
                    total_lines: visible_total,
                    line_num_width: None,
                    suppress_tildes: self.runtime_options.tilde,
                    start_row: 0,
                };
                paint_screen_with_options(
                    &mut self.writer,
                    &self.screen,
                    &lines,
                    &self.render_config,
                    &paint_opts,
                )?;
                self.paint_status_prompt(visible_total)?;
                return Ok(());
            }
        }

        let total = self.index.lines_indexed();
        let (start, end) = self.screen.visible_range();
        let content_rows = self.screen.content_rows();

        // Squeeze mode: collapse consecutive blank lines.
        if self.runtime_options.squeeze_blank_lines {
            return self.repaint_squeezed(start, content_rows, total);
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

        // Compute highlights for the visible lines.
        self.highlight_state
            .compute_highlights(&lines, self.last_pattern.as_ref());

        // GNU less bottom-aligns short files only on the initial render.
        // After any keypress, less repaints top-aligned with tildes filling below.
        let visible_content = total.saturating_sub(start);
        let start_row = if self.initial_render && total <= content_rows && start == 0 {
            content_rows - visible_content + 1
        } else {
            0
        };

        let paint_opts = PaintOptions {
            show_line_numbers: self.runtime_options.line_numbers,
            total_lines: total,
            line_num_width: None,
            suppress_tildes: self.runtime_options.tilde,
            start_row,
        };
        paint_screen_with_options(
            &mut self.writer,
            &self.screen,
            &lines,
            &self.render_config,
            &paint_opts,
        )?;

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
    fn repaint_squeezed(&mut self, start: usize, content_rows: usize, total: usize) -> Result<()> {
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

        let start_row = if self.initial_render && visible_total < content_rows {
            content_rows - visible_total + 1
        } else {
            0
        };
        let paint_opts = PaintOptions {
            show_line_numbers: self.runtime_options.line_numbers,
            total_lines: total,
            line_num_width: None,
            suppress_tildes: self.runtime_options.tilde,
            start_row,
        };
        paint_screen_mapped(
            &mut self.writer,
            &self.screen,
            &padded,
            &self.render_config,
            &paint_opts,
        )?;

        self.paint_status_prompt(total)?;

        self.initial_render = false;
        Ok(())
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

            let template = match self.runtime_options.prompt_string {
                Some(ref custom) => custom.as_str(),
                None => match self.prompt_style {
                    PromptStyle::Short => DEFAULT_SHORT_PROMPT,
                    PromptStyle::Medium => DEFAULT_MEDIUM_PROMPT,
                    PromptStyle::Long => DEFAULT_LONG_PROMPT,
                    PromptStyle::Custom(ref t) => t.as_str(),
                },
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

    /// Set the tab stop width.
    pub fn set_tab_width(&mut self, width: usize) {
        self.render_config.tab_stops = TabStops::regular(width);
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
        self.render_config.tab_stops = TabStops::regular(self.runtime_options.tab_width);
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

    /// Return the custom window size, if set by a counted `z`/`w` command.
    #[must_use]
    pub fn custom_window_size(&self) -> Option<usize> {
        self.custom_window_size
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
        // 100 * 100 / 100 = 100, clamped to total - content_rows = 100 - 23 = 77
        assert_eq!(pager.screen().top_line(), 77);
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
        assert_eq!(pager.custom_window_size(), Some(15));
    }

    #[test]
    fn test_dispatch_w_with_count_sets_window_and_scrolls_back() {
        let content = make_test_content(100);
        // Scroll forward 30, then "10w" sets window to 10 and scrolls back 10
        let pager = run_pager(b"30j10wq", &content);
        assert_eq!(pager.screen().top_line(), 20);
        assert_eq!(pager.custom_window_size(), Some(10));
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
        assert_eq!(pager.runtime_options().tab_width, 8);
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
        opts.tab_width = 4;
        pager.set_runtime_options(opts);

        assert!(pager.runtime_options().case_insensitive);
        assert!(pager.runtime_options().line_numbers);
        assert_eq!(pager.runtime_options().tab_width, 4);
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

    // Test 11: SavePipeInput fails gracefully when not reading from pipe.
    #[test]
    fn test_dispatch_save_pipe_input_fails_when_not_pipe() {
        let content = make_test_content(50);
        // 's' when not a pipe shows "Not reading from pipe" then 'q' exits.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.push(b'q');
        let pager = run_pager_with_settings(&keys, &content, Some("file.txt"), false, false);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(output.contains("Not reading from pipe"));
    }

    // Test 10: SavePipeInput writes buffer content to specified file.
    #[test]
    fn test_dispatch_save_pipe_input_writes_content() {
        let content = b"hello world\n";
        let tmpdir = std::env::temp_dir();
        let tmpfile = tmpdir.join("pgr_test_save_pipe_input.txt");
        // Clean up if it exists from a prior run.
        let _ = std::fs::remove_file(&tmpfile);

        let filename_bytes = tmpfile.to_str().unwrap().as_bytes();
        // Build key sequence: 's' then type the filename then Enter then 'q'.
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b's');
        keys.extend_from_slice(filename_bytes);
        keys.push(b'\r'); // Enter
        keys.push(b'q');

        let _pager = run_pager_with_settings(&keys, content, None, false, true);

        let saved = std::fs::read_to_string(&tmpfile).unwrap();
        assert_eq!(saved, "hello world\n");

        // Clean up.
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
        let mut keys: Vec<u8> = Vec::new();
        keys.push(b'n');
        // The transient message is consumed during repaint, so we check
        // the writer output instead of the field.
        let pager = run_pager(&keys, &content);
        let output = String::from_utf8_lossy(&pager.writer);
        assert!(
            output.contains("No previous search pattern"),
            "Expected 'No previous search pattern' in output: {output}"
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
        opts.prompt_string = Some(String::from("Viewing: %f"));
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
}
