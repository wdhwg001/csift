//! recover --list-backups: the file-history checkpoint store listing.

use crate::harness::*;
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime};

#[cfg(windows)]
const TARGET: &str = r"C:\work\relay\notes.md";
#[cfg(not(windows))]
const TARGET: &str = "/work/relay/notes.md";

const SESS_A: &str = "0f4e2a67-1b3c-4d5e-8f90-a1b2c3d4e5f6";
const SESS_B: &str = "9c8b7a65-4d3e-4f21-b098-fedcba987654";

fn store_key(path: &str) -> String {
    Sha256::digest(path.as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Write a checkpoint file into the fixture store with a pinned mtime.
fn write_checkpoint(h: &Home, session: &str, name: &str, content: &str, mtime_epoch: u64) {
    let dir = h.root.join(".claude").join("file-history").join(session);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, content).unwrap();
    let f = std::fs::File::options().write(true).open(&p).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(mtime_epoch))
        .unwrap();
}

#[test]
fn list_backups_orders_by_mtime_not_version_token() {
    let h = Home::new();
    let key = store_key(TARGET);
    // SESS_B's @v2 is OLDER than SESS_A's @v1: the mtime must order them, proving the
    // version token is not the order key. A different path's entry never surfaces.
    write_checkpoint(
        &h,
        SESS_A,
        &format!("{key}@v1"),
        "newer snapshot",
        1_700_000_000,
    );
    write_checkpoint(
        &h,
        SESS_B,
        &format!("{key}@v2"),
        "older snapshot",
        1_600_000_000,
    );
    write_checkpoint(
        &h,
        SESS_A,
        "aaaabbbbccccdddd@v1",
        "other file",
        1_650_000_000,
    );

    let out = h.run(&["recover", "--file", TARGET, "--list-backups"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let pos_b = out.stdout.find(SESS_B).expect("SESS_B row");
    let pos_a = out.stdout.find(SESS_A).expect("SESS_A row");
    assert!(
        pos_b < pos_a,
        "older mtime first despite the larger @vN:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("aaaabbbbccccdddd"),
        "a different path's entries never surface:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("2 checkpoint(s) across 2 session dir(s)")
            && out.stdout.contains("NOT an order key")
            && out.stdout.contains("tool-layer only"),
        "count + provenance disclosure:\n{}",
        out.stdout
    );

    let outj = h.run(&[
        "recover",
        "--file",
        TARGET,
        "--list-backups",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["kind"], "header");
    assert_eq!(rows[0]["mode"], "list-backups");
    assert_eq!(rows[0]["hash"], key.as_str());
    assert_eq!(rows[0]["store_present"], true);
    let backups: Vec<&serde_json::Value> = rows.iter().filter(|r| r["kind"] == "backup").collect();
    assert_eq!(backups.len(), 2, "{}", outj.stdout);
    assert_eq!(backups[0]["session_id"], SESS_B, "mtime order in JSON");
    assert_eq!(backups[0]["version"], "v2");
    assert_eq!(backups[1]["bytes"], 14, "'newer snapshot' is 14 bytes");
    assert!(
        backups[0]["backup_utc"]
            .as_str()
            .unwrap()
            .starts_with("2020-"),
        "epoch 1.6e9 renders as a 2020 instant: {}",
        outj.stdout
    );
    let summary = rows.last().unwrap();
    assert_eq!(summary["backups"], 2);
    assert_eq!(summary["sessions"], 2);
}

#[test]
fn list_backups_scopes_to_a_session_target() {
    let h = Home::new();
    let key = store_key(TARGET);
    write_checkpoint(&h, SESS_A, &format!("{key}@v1"), "from A", 1_700_000_000);
    write_checkpoint(&h, SESS_B, &format!("{key}@v1"), "from B", 1_700_000_100);
    // The @-target resolves through the normal fail-loud resolver, so SESS_A needs a
    // real transcript.
    h.write(
        &format!("-Users-dev-example-project/{SESS_A}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"hello"}}"#,
            "\n"
        ),
    );
    let out = h.run(&["recover", &at(SESS_A), "--file", TARGET, "--list-backups"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(SESS_A) && !out.stdout.contains(SESS_B),
        "a session target narrows the listing:\n{}",
        out.stdout
    );
}

#[test]
fn list_backups_empty_is_honest_and_exits_zero() {
    let h = Home::new();
    // Store entirely absent.
    let out = h.run(&["recover", "--file", TARGET, "--list-backups"]);
    assert!(
        out.success,
        "absence is a fact, not an error: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("store directory absent")
            && out
                .stdout
                .contains("NOT evidence the file was never edited"),
        "honest empty:\n{}",
        out.stdout
    );
    // Store present, no entry for this path.
    write_checkpoint(
        &h,
        SESS_A,
        "aaaabbbbccccdddd@v1",
        "other file",
        1_650_000_000,
    );
    let out2 = h.run(&["recover", "--file", TARGET, "--list-backups"]);
    assert!(out2.success);
    assert!(
        out2.stdout.contains("no checkpoints for this path"),
        "{}",
        out2.stdout
    );
}

#[test]
fn list_backups_rejects_unhashable_file_forms() {
    let h = Home::new();
    for bad in ["relative/notes.md", "@plan"] {
        let out = h.run(&["recover", "--file", bad, "--list-backups"]);
        assert!(!out.success, "unhashable --file must bail: {bad}");
        assert!(
            out.stderr.contains("ABSOLUTE"),
            "error names the requirement:\n{}",
            out.stderr
        );
    }
    let out = h.run(&["recover", "--list-backups"]);
    assert!(!out.success, "--file is required");
}
