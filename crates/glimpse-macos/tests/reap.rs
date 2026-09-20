//! The guard from [ADR 0019](../../docs/adr/0019-a-recording-outlives-a-killed-glimpse.md):
//! a capture must not outlive the Glimpse that started it.
//!
//! **These kill a real parent and check a real child dies.** Nothing weaker
//! demonstrates it. The whole point is the one path no ordinary exit takes —
//! `SIGKILL`, which no process can handle for itself — and issue #45 is what
//! happens when that path is reasoned about rather than exercised.
//!
//! `watch` is called on a thread here rather than through `glimpse --reap`,
//! because a test that needs the built binary can only run after a build and
//! would be testing argument plumbing at the same time as the mechanism. The
//! plumbing is covered by `reap::args_from`'s unit tests and end to end by
//! `scripts/force-quit.sh`.

#![cfg(target_os = "macos")]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// A process that will sit there until something kills it.
fn sleeper() -> Child {
    Command::new("sleep")
        .arg("120")
        .spawn()
        .expect("spawning sleep")
}

/// Has this process ended? Asked by **reaping it**, not by signalling it.
///
/// `kill(pid, 0)` is the obvious way and it is wrong here, for the third time in
/// this codebase: it succeeds for a zombie. These processes are children of the
/// test, so when the guard kills one it stays a zombie until the test waits on
/// it — and `kill(pid, 0)` then reports the capture as alive forever. The first
/// version of this file did exactly that and failed against a guard that was
/// working correctly.
///
/// `try_wait` both answers the question and reaps, so the answer cannot be stale.
/// `process_is_alive` in `glimpse-core` and the ESRCH path in `reap.rs` carry the
/// same scar.
fn ended(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(Some(_)))
}

/// Poll rather than sleep a guessed interval: the kernel's notification latency
/// is not something to hardcode, and a fixed sleep is how four journeys ended up
/// failing on a slow CI runner.
fn ends_within(child: &mut Child, limit: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if ended(child) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn killing_the_parent_kills_the_capture() {
    let mut parent = sleeper();
    let mut capture = sleeper();
    let (ppid, cpid) = (parent.id() as i32, capture.id() as i32);

    let guard = std::thread::spawn(move || glimpse_macos::reap::watch(ppid, cpid));

    // Let the guard reach `kevent` before the parent dies, so this test
    // exercises the **notification** path rather than the already-gone one.
    //
    // Without this it raced, and lost: the parent was dead before registration,
    // `kevent` answered ESRCH, and what was under test was the fallback. Which
    // is how the zombie bug in that fallback was found, so the race was
    // informative — but a test that silently checks a different path than it
    // names is not one to keep. The already-gone path has its own test below.
    //
    // Both orderings now end the same way, so this is a sleep for *aim*, not for
    // correctness: if it is too short the other path runs and the assertion
    // still holds.
    std::thread::sleep(Duration::from_millis(300));

    // Both alive, and the guard waiting. Asserting this first means a later
    // failure cannot be "the capture was never running".
    assert!(!ended(&mut capture), "the capture should be running");

    // SIGKILL, because SIGTERM is the case that was already handled.
    unsafe { libc::kill(ppid, libc::SIGKILL) };
    let _ = parent.wait();

    assert!(
        ends_within(&mut capture, Duration::from_secs(10)),
        "the capture outlived a SIGKILLed parent — this is the #45 shape, \
         and on macOS it writes about 18 GB/hour until the disk fills"
    );

    guard.join().expect("the guard thread should return");
}

/// The ordinary case, every time a recording stops normally: the capture exits
/// first and the guard has nothing left to protect.
///
/// It must not linger, and it must not touch the parent. A guard that killed its
/// parent on the way out would take Glimpse down at the end of every recording.
#[test]
fn the_guard_leaves_when_the_capture_finishes_first() {
    let mut parent = sleeper();
    let mut capture = sleeper();
    let (ppid, cpid) = (parent.id() as i32, capture.id() as i32);

    let guard = std::thread::spawn(move || glimpse_macos::reap::watch(ppid, cpid));

    let _ = capture.kill();
    let _ = capture.wait();

    guard
        .join()
        .expect("the guard should return once the capture is gone");
    assert!(
        !ended(&mut parent),
        "the guard must not touch the parent — doing so would take Glimpse \
         down at the end of every recording"
    );

    let _ = parent.kill();
    let _ = parent.wait();
}

/// The race the registration path exists for.
///
/// Between Glimpse spawning the guard and the guard reaching `kevent`, the
/// parent can already be gone — which is exactly the moment a force-quit
/// happens. `kevent` answers ESRCH for a pid that no longer exists, and reading
/// that as "nothing to do" would leave the capture running forever in the one
/// case this was built for.
#[test]
fn a_parent_already_gone_still_kills_the_capture() {
    let mut parent = sleeper();
    let ppid = parent.id() as i32;
    let _ = parent.kill();
    // Waited, so the pid is not a zombie: a zombie is still registrable, and
    // this test would then be exercising the ordinary path while claiming to
    // exercise the race.
    let _ = parent.wait();

    let mut capture = sleeper();
    let cpid = capture.id() as i32;

    // Returns rather than blocking: there is nothing left to wait for.
    glimpse_macos::reap::watch(ppid, cpid);

    assert!(
        ends_within(&mut capture, Duration::from_secs(5)),
        "a capture whose parent died before the guard started was left running"
    );
}
