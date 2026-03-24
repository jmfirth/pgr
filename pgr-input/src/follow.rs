//! File watching for follow mode using platform-native mechanisms.
//!
//! On macOS, uses kqueue with `EVFILT_VNODE` / `NOTE_WRITE` to detect file
//! modifications. On other platforms, falls back to polling via `select`.

use std::os::unix::io::RawFd;
use std::time::Duration;

/// Events that can occur while waiting in follow mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowEvent {
    /// New data was written to the watched file.
    NewData,
    /// A keypress is ready to be read from the key input fd.
    KeyReady,
    /// The timeout expired with no activity.
    Timeout,
}

/// Convert a non-negative `RawFd` (`i32`) to `uintptr_t` for kqueue ident.
///
/// File descriptors from the kernel are always non-negative, so the cast
/// cannot lose the sign bit.
#[allow(clippy::cast_sign_loss)] // fd values from the kernel are always >= 0
fn fd_to_ident(fd: RawFd) -> libc::uintptr_t {
    fd as libc::uintptr_t
}

/// Watches a file descriptor for new data using kqueue (macOS).
///
/// Used by follow mode to detect when a file grows so the pager can
/// re-read and display the new content.
pub struct FileWatcher {
    /// The kqueue file descriptor.
    kq: RawFd,
    /// The file descriptor being watched.
    file_fd: RawFd,
}

impl FileWatcher {
    /// Creates a new watcher for the given file descriptor.
    ///
    /// Registers `file_fd` for `EVFILT_VNODE` / `NOTE_WRITE` notifications
    /// via kqueue.
    ///
    /// # Errors
    ///
    /// Returns an error if kqueue creation or event registration fails.
    pub fn watch(file_fd: RawFd) -> crate::Result<Self> {
        // SAFETY: `kqueue()` creates a new kernel event queue fd. No memory
        // safety invariants beyond checking the return value.
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(std::io::Error::last_os_error().into());
        }

        // Register the file fd for vnode write notifications.
        let changelist = [libc::kevent {
            ident: fd_to_ident(file_fd),
            filter: libc::EVFILT_VNODE,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
            fflags: libc::NOTE_WRITE | libc::NOTE_EXTEND,
            data: 0,
            udata: std::ptr::null_mut(),
        }];

        // SAFETY: `kevent` modifies the kqueue registration list. The changelist
        // array is valid for the duration of the call. We pass null for the
        // event output list since we only want to register, not wait.
        let ret = unsafe {
            libc::kevent(
                kq,
                changelist.as_ptr(),
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: closing the fd we just created.
            unsafe { libc::close(kq) };
            return Err(err.into());
        }

        Ok(Self { kq, file_fd })
    }

    /// Waits for either file changes or key input, returning whichever fires first.
    ///
    /// Watches both the file fd (for `NOTE_WRITE`/`NOTE_EXTEND`) and the key
    /// input fd (for readability). Returns `FollowEvent::NewData` if the file
    /// changed, `FollowEvent::KeyReady` if a keypress is available, or
    /// `FollowEvent::Timeout` if the timeout expired.
    ///
    /// # Errors
    ///
    /// Returns an error if the kqueue wait fails.
    pub fn wait_with_key_check(
        &self,
        key_fd: RawFd,
        timeout: Duration,
    ) -> crate::Result<FollowEvent> {
        let file_ident = fd_to_ident(self.file_fd);
        let key_ident = fd_to_ident(key_fd);

        // Register both the file vnode event and the key fd for read readiness.
        let changelist = [
            libc::kevent {
                ident: file_ident,
                filter: libc::EVFILT_VNODE,
                flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
                fflags: libc::NOTE_WRITE | libc::NOTE_EXTEND,
                data: 0,
                udata: std::ptr::null_mut(),
            },
            libc::kevent {
                ident: key_ident,
                filter: libc::EVFILT_READ,
                flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
                fflags: 0,
                data: 0,
                udata: std::ptr::null_mut(),
            },
        ];

        #[allow(clippy::cast_possible_wrap)] // Duration seconds will never overflow i64
        let ts = libc::timespec {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_nsec: i64::from(timeout.subsec_nanos()),
        };

        let mut events = [libc::kevent {
            ident: 0,
            filter: 0,
            flags: 0,
            fflags: 0,
            data: 0,
            udata: std::ptr::null_mut(),
        }; 2];

        // The changelist and events arrays have compile-time-known lengths of 2,
        // which always fit in i32.

        // SAFETY: `kevent` blocks until an event fires or the timeout expires.
        // The changelist and events arrays are valid for the call duration.
        // The timespec pointer is valid and on the stack.
        let nev = unsafe {
            libc::kevent(
                self.kq,
                changelist.as_ptr(),
                2,
                events.as_mut_ptr(),
                2,
                &raw const ts,
            )
        };

        if nev < 0 {
            let err = std::io::Error::last_os_error();
            // EINTR is not an error — treat as timeout so the caller re-loops.
            if err.kind() == std::io::ErrorKind::Interrupted {
                return Ok(FollowEvent::Timeout);
            }
            return Err(err.into());
        }

        if nev == 0 {
            return Ok(FollowEvent::Timeout);
        }

        // We know nev is between 1 and 2 (the max events we asked for).
        #[allow(clippy::cast_sign_loss)] // nev is checked > 0 above
        let event_count = nev as usize;

        // Check which event(s) fired. Prioritize key input so the user can
        // always interrupt promptly.
        for ev in &events[..event_count] {
            if ev.filter == libc::EVFILT_READ && ev.ident == key_ident {
                return Ok(FollowEvent::KeyReady);
            }
        }
        for ev in &events[..event_count] {
            if ev.filter == libc::EVFILT_VNODE && ev.ident == file_ident {
                return Ok(FollowEvent::NewData);
            }
        }

        // Fallback — some event we didn't expect; treat as timeout.
        Ok(FollowEvent::Timeout)
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        // SAFETY: closing the kqueue fd we own. The kernel cleans up all
        // registered events automatically.
        unsafe {
            libc::close(self.kq);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::io::AsRawFd;

    #[test]
    fn test_file_watcher_watch_creates_watcher() {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let fd = tmp.as_file().as_raw_fd();
        let watcher = FileWatcher::watch(fd);
        assert!(watcher.is_ok());
    }

    #[test]
    fn test_file_watcher_timeout_returns_timeout() {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let fd = tmp.as_file().as_raw_fd();
        let watcher = FileWatcher::watch(fd).expect("watch failed");

        // Use a pipe for the key fd — nothing will be written so we get timeout.
        let (read_end, _write_end) = pipe_fds();
        let result = watcher
            .wait_with_key_check(read_end, Duration::from_millis(50))
            .expect("wait failed");
        assert_eq!(result, FollowEvent::Timeout);

        // Clean up pipe fds.
        // SAFETY: closing fds we created.
        unsafe {
            libc::close(read_end);
            libc::close(_write_end);
        }
    }

    #[test]
    fn test_file_watcher_detects_key_ready() {
        let tmp = tempfile::NamedTempFile::new().expect("create temp file");
        let fd = tmp.as_file().as_raw_fd();
        let watcher = FileWatcher::watch(fd).expect("watch failed");

        // Write a byte to the pipe so the key fd is immediately readable.
        let (read_end, write_end) = pipe_fds();
        // SAFETY: writing one byte to a valid pipe fd.
        unsafe {
            libc::write(write_end, b"x".as_ptr().cast(), 1);
        }

        let result = watcher
            .wait_with_key_check(read_end, Duration::from_millis(500))
            .expect("wait failed");
        assert_eq!(result, FollowEvent::KeyReady);

        // SAFETY: closing fds we created.
        unsafe {
            libc::close(read_end);
            libc::close(write_end);
        }
    }

    #[test]
    fn test_file_watcher_detects_new_data() {
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        tmp.write_all(b"initial\n").expect("initial write");
        tmp.flush().expect("flush");

        let fd = tmp.as_file().as_raw_fd();
        let watcher = FileWatcher::watch(fd).expect("watch failed");

        // Create a pipe for key fd — don't write to it.
        let (read_end, write_end) = pipe_fds();

        // Append data in a background thread after a short delay.
        let path = tmp.path().to_path_buf();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open for append");
            f.write_all(b"new data\n").expect("append write");
            f.flush().expect("flush append");
        });

        let result = watcher
            .wait_with_key_check(read_end, Duration::from_secs(2))
            .expect("wait failed");
        assert_eq!(result, FollowEvent::NewData);

        handle.join().expect("thread join");

        // SAFETY: closing fds we created.
        unsafe {
            libc::close(read_end);
            libc::close(write_end);
        }
    }

    #[test]
    fn test_follow_event_debug_display() {
        // Ensure all variants have Debug output.
        let events = [
            FollowEvent::NewData,
            FollowEvent::KeyReady,
            FollowEvent::Timeout,
        ];
        for ev in &events {
            let _ = format!("{ev:?}");
        }
    }

    /// Create a pipe and return (read_fd, write_fd).
    fn pipe_fds() -> (RawFd, RawFd) {
        let mut fds = [0i32; 2];
        // SAFETY: `pipe` creates two valid fds. The array is on the stack
        // and large enough.
        let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(ret, 0, "pipe() failed");
        (fds[0], fds[1])
    }
}
