//! Kill the capture when the Glimpse that started it dies
//! ([ADR 0019](../../../docs/adr/0019-a-recording-outlives-a-killed-glimpse.md)).
//!
//! This is what runs as `glimpse --reap <parent-pid> <child-pid>`: a second copy
//! of the binary, spawned alongside ffmpeg, whose whole job is to outlive the
//! application by exactly long enough to notice it is gone.
//!
//! **Why a separate process at all.** Linux sets `PR_SET_PDEATHSIG` and the
//! kernel does this for free. macOS has no equivalent, so something has to be
//! watching, and anything inside Glimpse dies with the Glimpse it is watching —
//! which is the case that matters, because every other way out is already
//! handled in the app. `SIGKILL` and Force Quit are the ones no process can
//! handle for itself.
//!
//! **Measured, not assumed.** Killing Glimpse with `-9` mid-recording left
//! ffmpeg writing at ~5 MB/s — about 18 GB/hour — holding the screen capture
//! device until the disk filled or Glimpse was started again. The same run ruled
//! out the cheap fix: ffmpeg's stdin is already a pipe the parent holds, so the
//! kill closed it, and ffmpeg carried on regardless. EOF on stdin is not a
//! lifetime signal.
//!
//! **Why `kevent` rather than polling.** `EVFILT_PROC`/`NOTE_EXIT` blocks with no
//! timer and no wakeups, so the watchdog is asleep until the thing it waits for
//! happens. A poll loop would work and would be simpler; the reason not to is
//! that a process which wakes up 3600 times an hour to do nothing is a process
//! that can itself misbehave.

#[cfg(target_os = "macos")]
mod imp {
    use std::os::unix::io::{AsRawFd, FromRawFd, OwnedFd};

    /// Watch `parent`, and kill `child` when it exits. Returns when either is
    /// gone.
    ///
    /// Both are watched. If the capture exits first — the ordinary case, every
    /// time a recording stops normally — there is nothing left to guard and this
    /// returns rather than lingering as a process with no purpose.
    pub fn watch(parent: i32, child: i32) {
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            // Without a queue there is nothing to wait on, and guessing would be
            // worse than saying so: a reaper that exits quietly looks exactly
            // like one that is working.
            eprintln!("glimpse --reap: kqueue failed: {}", last_error());
            return;
        }
        // Owned so the descriptor closes on every path out of this function.
        let kq = unsafe { OwnedFd::from_raw_fd(kq) };

        let changes = [proc_exit_filter(parent), proc_exit_filter(child)];
        let mut event: libc::kevent = unsafe { std::mem::zeroed() };

        // Registering is also the liveness check, and it has to be: between
        // Glimpse spawning this process and this line running, the parent may
        // already be gone. `kevent` answers ESRCH for a pid that no longer
        // exists, and treating that as "nothing to do" would leave the capture
        // running forever in precisely the race this exists to close.
        let registered = unsafe {
            libc::kevent(
                kq.as_raw_fd(),
                changes.as_ptr(),
                changes.len() as libc::c_int,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if registered < 0 {
            let err = std::io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::ESRCH) {
                // A pid that cannot be watched is a pid that is gone, and the
                // capture must not be left running on the strength of a second
                // opinion.
                //
                // The first version asked `kill(parent, 0)` here to decide which
                // of the two had died. It answers **true for a zombie**, so a
                // parent killed microseconds earlier still read as alive, and
                // the capture was left running in precisely the case this
                // exists for — the test caught it. `process_is_alive` in
                // `glimpse-core` carries the same scar.
                //
                // If it was the *capture* that had already exited, this kill is
                // a no-op, which is the right outcome for that case too.
                kill_capture(child);
                return;
            }
            eprintln!("glimpse --reap: kevent registration failed: {err}");
            return;
        }

        loop {
            let n = unsafe {
                libc::kevent(
                    kq.as_raw_fd(),
                    std::ptr::null(),
                    0,
                    &mut event,
                    1,
                    std::ptr::null(),
                )
            };
            if n < 0 {
                let err = std::io::Error::last_os_error();
                // A signal interrupting the wait is not the event being waited
                // for. Returning here would silently give up guarding.
                if err.kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                eprintln!("glimpse --reap: kevent wait failed: {err}");
                return;
            }
            if n == 0 {
                continue;
            }
            let who = event.ident as i32;
            if who == parent {
                kill_capture(child);
            }
            // Either way there is nothing left to guard: the parent is gone and
            // the capture with it, or the capture finished on its own.
            return;
        }
    }

    /// One-shot exit notification for `pid`.
    fn proc_exit_filter(pid: i32) -> libc::kevent {
        libc::kevent {
            ident: pid as libc::uintptr_t,
            filter: libc::EVFILT_PROC,
            flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
            fflags: libc::NOTE_EXIT,
            data: 0,
            udata: std::ptr::null_mut(),
        }
    }

    /// `SIGKILL`, not `SIGTERM`.
    ///
    /// There is no one left to finalise the container: the application that owns
    /// the recording is gone, so the intermediate is unfinished whatever happens
    /// next, and the workspace is removed by the start-up sweep rather than
    /// offered to anyone. Asking politely would only mean waiting to find out
    /// whether a process that is not reading its input intends to answer.
    fn kill_capture(child: i32) {
        // Never pid 0 or negative: POSIX reads those as "my process group" and
        // "a process group", so a stray value here would kill Glimpse's own
        // group rather than one capture. The same edge the workspace sweep had
        // to learn about.
        if child <= 0 {
            return;
        }
        unsafe { libc::kill(child, libc::SIGKILL) };
    }

    fn last_error() -> std::io::Error {
        std::io::Error::last_os_error()
    }
}

#[cfg(target_os = "macos")]
pub use imp::watch;

/// Parse `--reap <parent-pid> <child-pid>` out of the command line.
///
/// Returns `None` for anything else, so the binary carries on and puts up a
/// window. Deliberately not part of `--help`: no user types this, and a line in
/// the help text for an internal argument invites someone to.
pub fn args_from<I: IntoIterator<Item = String>>(args: I) -> Option<(i32, i32)> {
    let mut it = args.into_iter().skip(1);
    if it.next().as_deref() != Some("--reap") {
        return None;
    }
    let parent = it.next()?.parse().ok()?;
    let child = it.next()?.parse().ok()?;
    // Both must be real pids. A zero would mean "my process group" to `kill`,
    // and a negative one a whole group; neither is ever what was meant, and
    // refusing here is cheaper than being careful everywhere downstream.
    (parent > 0 && child > 0).then_some((parent, child))
}

#[cfg(test)]
mod tests {
    use super::args_from;

    fn argv(rest: &[&str]) -> Vec<String> {
        std::iter::once("glimpse".to_string())
            .chain(rest.iter().map(|s| s.to_string()))
            .collect()
    }

    #[test]
    fn parses_a_reap_invocation() {
        assert_eq!(args_from(argv(&["--reap", "42", "99"])), Some((42, 99)));
    }

    #[test]
    fn ignores_an_ordinary_launch() {
        assert_eq!(args_from(argv(&[])), None);
        assert_eq!(args_from(argv(&["--help"])), None);
    }

    /// pid 0 means "every process in my process group" to `kill`, and a negative
    /// pid means a process group. The workspace sweep had to learn this the hard
    /// way — a `glimpse-0-*` directory was immortal because `kill(0, 0)`
    /// succeeds — and here the same value would make the reaper kill Glimpse
    /// itself rather than one capture.
    #[test]
    fn refuses_pids_that_mean_a_process_group() {
        assert_eq!(args_from(argv(&["--reap", "0", "99"])), None);
        assert_eq!(args_from(argv(&["--reap", "42", "0"])), None);
        assert_eq!(args_from(argv(&["--reap", "-1", "99"])), None);
    }

    #[test]
    fn refuses_a_truncated_or_unparseable_invocation() {
        assert_eq!(args_from(argv(&["--reap"])), None);
        assert_eq!(args_from(argv(&["--reap", "42"])), None);
        assert_eq!(args_from(argv(&["--reap", "forty", "99"])), None);
    }
}
