//! Command dispatch loop — the main pager event loop.
//!
//! Reads keys, translates them to commands via the keymap, and executes
//! those commands by mutating the screen state and repainting.

use std::io::{Read, Write};

use pgr_core::{Buffer, LineIndex, Mark, MarkStore};
use pgr_display::{
    eval_prompt, paint_prompt, paint_screen, OverstrikeMode, PromptContext, PromptStyle,
    RawControlMode, RenderConfig, Screen, TabStops, DEFAULT_LONG_PROMPT, DEFAULT_MEDIUM_PROMPT,
    DEFAULT_SHORT_PROMPT,
};

use crate::error::Result;
use crate::file_list::FileList;
use crate::key::Key;
use crate::key_reader::KeyReader;
use crate::keymap::Keymap;
use crate::runtime_options::RuntimeOptions;
use crate::Command;

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
        }
    }

    /// Run the main loop. Blocks until the user quits or input is exhausted.
    ///
    /// # Errors
    ///
    /// Returns an error if key reading, buffer access, or terminal output fails.
    pub fn run(&mut self) -> Result<()> {
        self.repaint()?;

        loop {
            match self.reader.read_key() {
                Ok(key) => {
                    if !self.process_key(&key)? {
                        break;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                Err(e) => return Err(e.into()),
            }
        }

        Ok(())
    }

    /// Process a single key event. Returns `Ok(true)` if the pager should
    /// continue, `Ok(false)` if it should quit.
    fn process_key(&mut self, key: &Key) -> Result<bool> {
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
                // Not ^X: ignore the prefix, process key normally.
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
                    self.handle_query_option(c);
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
                let end = total.saturating_sub(self.screen.content_rows());
                self.screen.goto_line(end, total);
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
        let total = self.index.total_lines(&*self.buffer)?;

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
                let amount = self
                    .sticky_half_page
                    .unwrap_or(self.screen.content_rows() / 2);
                self.screen.scroll_forward(amount, total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::HalfPageBackward => {
                self.save_last_position();
                if let Some(c) = count {
                    self.sticky_half_page = Some(c);
                }
                let amount = self
                    .sticky_half_page
                    .unwrap_or(self.screen.content_rows() / 2);
                self.screen.scroll_backward(amount);
                self.repaint()?;
            }
            Command::GotoBeginning(n) => {
                self.save_last_position();
                self.screen.goto_line(count.or(n).unwrap_or(0), total);
                self.repaint()?;
            }
            Command::GotoEnd(n) => {
                self.save_last_position();
                let default = total.saturating_sub(self.screen.content_rows());
                self.screen.goto_line(count.or(n).unwrap_or(default), total);
                self.repaint()?;
                self.check_eof_quit(total);
            }
            Command::Repaint => {
                self.repaint()?;
            }
            Command::Quit => {
                self.should_quit = true;
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
        }

        Ok(())
    }

    /// Enter basic follow mode: scroll to end and exit immediately.
    ///
    /// A full follow mode with `inotify`/`kqueue` polling and non-blocking key
    /// reading is deferred to Phase 2. This stub scrolls to the end of the
    /// buffer and returns, which satisfies the basic "F scrolls to bottom"
    /// contract.
    fn follow_mode(&mut self) -> Result<()> {
        self.buffer.refresh()?;
        let new_len = self.buffer.len() as u64;
        self.index = LineIndex::new(new_len);
        self.index.index_all(&*self.buffer)?;
        let total = self.index.lines_indexed();
        let default = total.saturating_sub(self.screen.content_rows());
        self.screen.goto_line(default, total);
        self.repaint()?;
        Ok(())
    }

    /// Switch to the next file in the file list.
    fn switch_file_next(&mut self) -> Result<()> {
        if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            if file_list.next().is_ok() {
                self.apply_current_file();
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Switch to the previous file in the file list.
    fn switch_file_prev(&mut self) -> Result<()> {
        if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            if file_list.prev().is_ok() {
                self.apply_current_file();
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Switch to the N-th file (0-based) in the file list.
    fn switch_file_goto(&mut self, index: usize) -> Result<()> {
        if let Some(ref mut file_list) = self.file_list {
            file_list.save_viewport(self.screen.top_line(), self.screen.horizontal_offset());
            if file_list.goto(index).is_ok() {
                self.apply_current_file();
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Remove the current file from the file list.
    fn remove_current_file(&mut self) -> Result<()> {
        if let Some(ref mut file_list) = self.file_list {
            if file_list.remove_current().is_ok() {
                self.apply_current_file();
                self.repaint()?;
            }
        }
        Ok(())
    }

    /// Load the current file's display name and viewport into the pager state.
    fn apply_current_file(&mut self) {
        if let Some(ref file_list) = self.file_list {
            let entry = file_list.current();
            let (top_line, h_offset) = file_list.saved_viewport();
            self.filename = Some(entry.display_name.clone());
            self.screen.goto_line(top_line, usize::MAX);
            self.screen.set_horizontal_offset(h_offset);
        }
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
        let total = self.index.lines_indexed();
        let (start, end) = self.screen.visible_range();

        let mut lines: Vec<Option<String>> = Vec::with_capacity(self.screen.content_rows());
        for line_num in start..end {
            if line_num < total {
                let content = self.index.get_line(line_num, &*self.buffer)?;
                lines.push(content);
            } else {
                lines.push(None);
            }
        }

        paint_screen(&mut self.writer, &self.screen, &lines, &self.render_config)?;

        // Write the prompt on the last row.
        self.paint_status_prompt(total)?;

        Ok(())
    }

    /// Render and paint the status prompt on the last row.
    ///
    /// If a transient status message is set, it is displayed instead of
    /// the normal prompt and then cleared. Otherwise the prompt template
    /// is evaluated from the current pager state.
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

        paint_prompt(&mut self.writer, &text, rows, cols)?;

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

        PromptContext {
            filename: self.filename.as_deref(),
            top_line: top_line_0.saturating_add(1),
            bottom_line: bottom_display,
            total_lines: Some(total_lines),
            total_bytes: self.buffer.len() as u64,
            byte_offset: 0,
            file_index,
            file_count,
            at_eof,
            is_pipe: false,
            column: self.screen.horizontal_offset().saturating_add(1),
            page_number: None,
            input_line: None,
            pipe_size: None,
            search_active: false,
            search_pattern: None,
            line_numbers_enabled: self.runtime_options.line_numbers,
            marks_set: self.marks.has_any(),
            filter_active: false,
            filter_pattern: None,
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
    pub fn set_runtime_options(&mut self, opts: RuntimeOptions) {
        self.runtime_options = opts;
    }

    /// Access the runtime options (for testing).
    #[must_use]
    pub fn runtime_options(&self) -> &RuntimeOptions {
        &self.runtime_options
    }

    /// Handle a toggle option command (`-<flag>`).
    fn handle_toggle_option(&mut self, flag: char) -> Result<()> {
        if self.runtime_options.toggle(flag).is_ok() {
            // Sync render-affecting options to the screen/render_config.
            self.sync_runtime_to_render();
            self.repaint()?;
        }
        Ok(())
    }

    /// Handle a query option command (`_<flag>`).
    fn handle_query_option(&mut self, flag: char) {
        // Query is display-only; we just invoke it to get the message.
        // Full prompt display of the result is Phase 2.
        let _result = self.runtime_options.query(flag);
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
        // 10G: goto_line(10, 100) = min(10, 99) = 10
        assert_eq!(pager.screen().top_line(), 10);
    }

    #[test]
    fn test_dispatch_multiple_digits_123j_scrolls_forward_123_clamped() {
        let content = make_test_content(50);
        let pager = run_pager(b"123jq", &content);
        // 123 lines forward, but total is 50, so clamped to 49.
        assert_eq!(pager.screen().top_line(), 49);
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
        let content = make_test_content(10);
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
        // End: total(100) - content_rows(23) = 77
        assert_eq!(pager.screen().top_line(), 77);
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
        // 100 * 100 / 100 = 100, clamped to 99 (total_lines - 1)
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
        // Navigate to end with G, then J scrolls 1 line beyond.
        // G -> 77 (total 100 - content_rows 23), then J -> 78... but that's clamped.
        // Actually J is unclamped, so from 77 it goes to 78.
        // Let's scroll to the very last line first, then J.
        let pager = run_pager(b"99jJq", &content);
        // 99j -> scroll_forward clamped at 99. J -> unclamped 100.
        assert_eq!(pager.screen().top_line(), 100);
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
        // `-` then `i` toggles case_insensitive, then quit.
        let pager = run_pager(b"-iq", &content);
        assert!(pager.runtime_options().case_insensitive);
    }

    #[test]
    fn test_dispatch_dash_n_upper_toggles_line_numbers_and_repaints() {
        let content = make_test_content(50);
        // Toggling -N should flip line_numbers and trigger a repaint.
        let pager = run_pager(b"-Nq", &content);
        assert!(pager.runtime_options().line_numbers);
    }

    #[test]
    fn test_dispatch_dash_s_upper_toggles_chop_long_lines() {
        let content = make_test_content(50);
        let pager = run_pager(b"-Sq", &content);
        assert!(pager.runtime_options().chop_long_lines);
        // Screen chop mode should also be updated.
        assert!(pager.screen().chop_mode());
    }

    #[test]
    fn test_dispatch_dash_s_lower_toggles_squeeze_blank_lines() {
        let content = make_test_content(50);
        let pager = run_pager(b"-sq", &content);
        assert!(pager.runtime_options().squeeze_blank_lines);
    }

    #[test]
    fn test_dispatch_underscore_queries_option() {
        // Pressing _ then i should not change any state.
        let content = make_test_content(50);
        let pager = run_pager(b"_iq", &content);
        assert!(!pager.runtime_options().case_insensitive);
    }

    #[test]
    fn test_dispatch_dash_toggle_twice_reverts() {
        let content = make_test_content(50);
        // Toggle i on, then off.
        let pager = run_pager(b"-i-iq", &content);
        assert!(!pager.runtime_options().case_insensitive);
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
}
