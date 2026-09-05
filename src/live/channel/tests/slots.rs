//! The slot chain: naming, the head slot's wipe, waiting for a slow predecessor, and
//! the timeout that never blocks a delivery.

use super::*;

use std::time::{Duration, Instant};

/// A unique parent-pid stand-in per test, so parallel tests never share a chain.
fn ppid() -> u32 {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    900_000 + seq
}

struct ChainGuard(std::path::PathBuf);

impl Drop for ChainGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn the_chain_directory_is_keyed_by_parent_process_event_and_lane() {
    let dir = seq_dir(4242, "Stop", AGENT);
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert_eq!(name, format!("csift-deliver-4242-Stop-{AGENT}"));
    assert_eq!(dir.parent(), Some(std::env::temp_dir().as_path()));
    // A different event is a different chain: a PreToolUse slot must not wait on a
    // Stop slot.
    assert_ne!(
        seq_dir(4242, "Stop", AGENT),
        seq_dir(4242, "PreToolUse", AGENT)
    );
    assert_ne!(seq_dir(4242, "Stop", AGENT), seq_dir(4243, "Stop", AGENT));
}

#[test]
fn a_hostile_event_name_cannot_walk_out_of_the_temp_directory() {
    let dir = seq_dir(4242, "../../etc", AGENT);
    let name = dir.file_name().unwrap().to_string_lossy().into_owned();
    assert!(!name.contains('/'), "{name}");
    assert!(!name.contains(".."), "{name}");
    assert_eq!(dir.parent(), Some(std::env::temp_dir().as_path()));
}

#[test]
fn slot_one_never_waits_and_wipes_a_stale_chain() {
    let pid = ppid();
    let guard = ChainGuard(seq_dir(pid, "Stop", AGENT));
    // A leftover marker from the previous firing of this event.
    let first = open_slot(pid, "Stop", AGENT, 1).unwrap();
    mark_done(&first).unwrap();
    assert!(guard.0.join("s1.done").exists());

    // The next chain opens at slot 1 again and clears the stale marker, so slot 2 of
    // the new firing cannot be released by the old one.
    let restarted = open_slot(pid, "Stop", AGENT, 1).unwrap();
    assert_eq!(wait_prev(&restarted), WaitOutcome::First);
    assert!(!guard.0.join("s1.done").exists());
}

#[test]
fn a_later_slot_waits_for_a_slow_predecessor_and_then_proceeds() {
    let pid = ppid();
    let guard = ChainGuard(seq_dir(pid, "Stop", AGENT));
    let first = open_slot(pid, "Stop", AGENT, 1).unwrap();
    let second = open_slot(pid, "Stop", AGENT, 2).unwrap();

    let slow = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        mark_done(&first).unwrap();
    });
    let started = Instant::now();
    let outcome = wait_prev_deadline(&second, 4000);
    let waited = started.elapsed();
    slow.join().unwrap();

    assert_eq!(outcome, WaitOutcome::Ready);
    assert!(
        waited >= Duration::from_millis(150),
        "slot 2 did not actually wait ({waited:?})"
    );
    assert!(guard.0.join("s1.done").exists());
}

#[test]
fn a_predecessor_that_never_arrives_times_out_instead_of_blocking() {
    let pid = ppid();
    let _guard = ChainGuard(seq_dir(pid, "Stop", AGENT));
    let third = open_slot(pid, "Stop", AGENT, 3).unwrap();
    let started = Instant::now();
    let outcome = wait_prev_deadline(&third, 150);
    assert_eq!(outcome, WaitOutcome::TimedOut);
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the timeout did not release the slot"
    );
    // The delivery goes ahead; the receiver is told the order may be disturbed.
    assert_eq!(
        disorder_warning(3),
        "[csift-channel warning: slot 3 emitted before slot 2; order may be disturbed]"
    );
}

#[test]
fn the_default_wait_is_five_seconds_polled_every_fifty_milliseconds() {
    assert_eq!(SLOT_WAIT_MS, 5000);
    assert_eq!(SLOT_POLL_MS, 50);
}

#[test]
fn the_last_slot_removes_the_chain_and_an_earlier_one_leaves_it() {
    let pid = ppid();
    let guard = ChainGuard(seq_dir(pid, "Stop", AGENT));
    let first = open_slot(pid, "Stop", AGENT, 1).unwrap();
    mark_done(&first).unwrap();
    cleanup_if_last(&first, 4);
    assert!(guard.0.exists(), "slot 1 of 4 must not remove the chain");

    let last = open_slot(pid, "Stop", AGENT, 4).unwrap();
    mark_done(&last).unwrap();
    cleanup_if_last(&last, 4);
    assert!(!guard.0.exists(), "the last slot removes the chain");
    // Cleaning an already-removed chain is not an error: the directory is scratch.
    cleanup_if_last(&last, 4);
}
