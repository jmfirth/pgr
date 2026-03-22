//! RAII raw terminal mode management.

use std::os::fd::RawFd;

/// RAII guard for raw terminal mode. Restores original terminal state on drop.
///
/// When created via [`RawTerminal::enter`], the terminal is placed into raw mode
/// suitable for a pager: no canonical processing, no echo, no signal generation.
/// The original terminal settings are restored when this value is dropped.
pub struct RawTerminal {
    fd: RawFd,
    original: libc::termios,
}

impl RawTerminal {
    /// Enter raw mode on the given file descriptor (typically stdin).
    ///
    /// Disables canonical mode, echo, signal generation, extended input processing,
    /// software flow control, CR-to-NL mapping, and output post-processing.
    /// Sets VMIN=1 and VTIME=0 for byte-at-a-time blocking reads.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if `tcgetattr` or `tcsetattr` fails.
    pub fn enter(fd: RawFd) -> std::io::Result<Self> {
        let original = get_termios(fd)?;
        let mut raw = original;

        // Input flags: disable flow control, CR→NL mapping, break handling, parity, strip
        raw.c_iflag &= !(libc::IXON | libc::ICRNL | libc::BRKINT | libc::INPCK | libc::ISTRIP);

        // Output flags: disable post-processing
        raw.c_oflag &= !libc::OPOST;

        // Local flags: disable canonical mode, echo, signal generation, extended input
        raw.c_lflag &= !(libc::ICANON | libc::ECHO | libc::ISIG | libc::IEXTEN);

        // Control characters: read returns after 1 byte, no timeout
        raw.c_cc[libc::VMIN] = 1;
        raw.c_cc[libc::VTIME] = 0;

        set_termios(fd, &raw)?;

        Ok(Self { fd, original })
    }

    /// Get terminal dimensions (rows, columns) via ioctl.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the `TIOCGWINSZ` ioctl fails.
    pub fn dimensions(&self) -> std::io::Result<(usize, usize)> {
        let mut winsize = libc::winsize {
            ws_row: 0,
            ws_col: 0,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        // SAFETY: `TIOCGWINSZ` is a well-defined ioctl that writes a `winsize` struct.
        // We pass a valid mutable pointer to a stack-allocated `winsize`. The fd is
        // required to be a valid terminal file descriptor by the caller of `enter`.
        let ret = unsafe { libc::ioctl(self.fd, libc::TIOCGWINSZ, &raw mut winsize) };

        if ret == -1 {
            return Err(std::io::Error::last_os_error());
        }

        Ok((winsize.ws_row as usize, winsize.ws_col as usize))
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        // Best-effort restore; ignore errors during drop.
        let _ = set_termios(self.fd, &self.original);
    }
}

/// Read current terminal attributes via `tcgetattr`.
fn get_termios(fd: RawFd) -> std::io::Result<libc::termios> {
    // SAFETY: `tcgetattr` reads terminal attributes into a `termios` struct.
    // We zero-initialize the struct first. The fd must be a valid terminal fd.
    let mut termios = unsafe { std::mem::zeroed::<libc::termios>() };
    // SAFETY: `&raw mut` produces a raw pointer without an implicit borrow.
    let ret = unsafe { libc::tcgetattr(fd, &raw mut termios) };
    if ret == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(termios)
}

/// Set terminal attributes via `tcsetattr` with `TCSAFLUSH`.
fn set_termios(fd: RawFd, termios: &libc::termios) -> std::io::Result<()> {
    // SAFETY: `tcsetattr` with `TCSAFLUSH` sets terminal attributes after flushing
    // output. The termios pointer is valid and the fd must be a valid terminal fd.
    let ret = unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, termios) };
    if ret == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
