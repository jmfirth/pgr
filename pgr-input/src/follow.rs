//! File watching for follow mode using platform-native mechanisms.
//!
//! On macOS/BSD, uses kqueue with `EVFILT_VNODE` / `NOTE_WRITE` to detect file
//! modifications. On Linux and other platforms, falls back to polling via `select`.

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
    /// The watched file was renamed (e.g. log rotation).
    FileRenamed,
    /// The watched file was deleted.
    FileDeleted,
}

// ── kqueue implementation (macOS / BSD) ─────────────────────────────────────

#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
mod kqueue {
    use super::{Duration, FollowEvent, RawFd};

    /// Convert a non-negative `RawFd` (`i32`) to `uintptr_t` for kqueue ident.
    ///
    /// File descriptors from the kernel are always non-negative, so the cast
    /// cannot lose the sign bit.
    #[allow(clippy::cast_sign_loss)] // fd values from the kernel are always >= 0
    fn fd_to_ident(fd: RawFd) -> libc::uintptr_t {
        fd as libc::uintptr_t
    }

    /// Watches a file descriptor for new data using kqueue.
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

            // Register the file fd for vnode write, rename, and delete notifications.
            let changelist = [libc::kevent {
                ident: fd_to_ident(file_fd),
                filter: libc::EVFILT_VNODE,
                flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_CLEAR,
                fflags: libc::NOTE_WRITE
                    | libc::NOTE_EXTEND
                    | libc::NOTE_RENAME
                    | libc::NOTE_DELETE,
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
            &mut self,
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
                    fflags: libc::NOTE_WRITE
                        | libc::NOTE_EXTEND
                        | libc::NOTE_RENAME
                        | libc::NOTE_DELETE,
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
                    // Check rename/delete before write so the caller can reopen.
                    if ev.fflags & libc::NOTE_RENAME != 0 {
                        return Ok(FollowEvent::FileRenamed);
                    }
                    if ev.fflags & libc::NOTE_DELETE != 0 {
                        return Ok(FollowEvent::FileDeleted);
                    }
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
}

// ── poll/select fallback (Linux and other platforms) ────────────────────────

#[cfg(not(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
mod poll {
    use super::{Duration, FollowEvent, RawFd};

    /// Watches a file descriptor for new data using poll-based file size checks.
    ///
    /// Fallback for platforms without kqueue. Checks whether the file has grown
    /// by comparing `fstat` results, and uses `select` to check key readiness.
    pub struct FileWatcher {
        /// The file descriptor being watched.
        file_fd: RawFd,
        /// Last known file size.
        last_size: u64,
    }

    impl FileWatcher {
        /// Creates a new watcher for the given file descriptor.
        ///
        /// # Errors
        ///
        /// Returns an error if `fstat` fails on the file descriptor.
        pub fn watch(file_fd: RawFd) -> crate::Result<Self> {
            let last_size = file_size(file_fd)?;
            Ok(Self { file_fd, last_size })
        }

        /// Waits for either file changes or key input.
        ///
        /// Polls the file size at ~100ms intervals. Returns `NewData` if the
        /// file grew, `KeyReady` if input is available on `key_fd`, or
        /// `Timeout` if the timeout expired.
        ///
        /// # Errors
        ///
        /// Returns an error if `select` or `fstat` fails.
        pub fn wait_with_key_check(
            &mut self,
            key_fd: RawFd,
            timeout: Duration,
        ) -> crate::Result<FollowEvent> {
            let poll_interval = Duration::from_millis(100);
            let mut remaining = timeout;

            loop {
                // Check key readiness with a short select timeout.
                let wait = remaining.min(poll_interval);
                if select_readable(key_fd, wait)? {
                    return Ok(FollowEvent::KeyReady);
                }

                // Check if the file grew.
                let current_size = file_size(self.file_fd)?;
                if current_size > self.last_size {
                    self.last_size = current_size;
                    return Ok(FollowEvent::NewData);
                }
                if current_size == 0 && self.last_size > 0 {
                    // File was truncated or deleted.
                    self.last_size = 0;
                    return Ok(FollowEvent::FileDeleted);
                }

                remaining = remaining.saturating_sub(wait);
                if remaining.is_zero() {
                    return Ok(FollowEvent::Timeout);
                }
            }
        }
    }

    /// Get the file size via `fstat`.
    fn file_size(fd: RawFd) -> crate::Result<u64> {
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fstat fills a stack-allocated struct from a valid fd.
        let ret = unsafe { libc::fstat(fd, &mut stat) };
        if ret < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        #[allow(clippy::cast_sign_loss)] // file size is non-negative
        Ok(stat.st_size as u64)
    }

    /// Check if `fd` is readable within `timeout` using `select`.
    fn select_readable(fd: RawFd, timeout: Duration) -> crate::Result<bool> {
        let mut read_set = unsafe { std::mem::zeroed::<libc::fd_set>() };
        // SAFETY: FD_ZERO and FD_SET operate on a stack-allocated fd_set.
        unsafe {
            libc::FD_ZERO(&mut read_set);
            libc::FD_SET(fd, &mut read_set);
        }

        #[allow(clippy::cast_possible_wrap)]
        let mut tv = libc::timeval {
            tv_sec: timeout.as_secs() as libc::time_t,
            tv_usec: i64::from(timeout.subsec_micros()),
        };

        // SAFETY: select blocks until fd is readable or timeout expires.
        let ret = unsafe {
            libc::select(
                fd + 1,
                &mut read_set,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut tv,
            )
        };

        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(err.into());
        }

        Ok(ret > 0)
    }
}

// Re-export the platform-appropriate FileWatcher.
#[cfg(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
))]
pub use kqueue::FileWatcher;

#[cfg(not(any(
    target_os = "macos",
    target_os = "freebsd",
    target_os = "openbsd",
    target_os = "netbsd"
)))]
pub use poll::FileWatcher;

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
        let mut watcher = FileWatcher::watch(fd).expect("watch failed");

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
        let mut watcher = FileWatcher::watch(fd).expect("watch failed");

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
        let mut watcher = FileWatcher::watch(fd).expect("watch failed");

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
            FollowEvent::FileRenamed,
            FollowEvent::FileDeleted,
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
