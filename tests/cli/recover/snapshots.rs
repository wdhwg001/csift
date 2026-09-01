//! The file-history snapshot instrument end to end: divergence rebase, content-less
//! disclosure, and the mtime gate through the CLI.

use crate::harness::*;

const SSESS: &str = "abcd1234-5678-4abc-8def-001122334455";

/// A session whose file gets a tool Write, then a SILENT external write captured
/// only by the snapshot version bump. `with_store` controls whether the store blobs
/// exist (mtime-matched to their backupTime).
fn snapshot_home(with_store: bool) -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"write the config"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"w1","name":"Write","input":{"file_path":"/w/app.toml","content":"name = \"harbor\"\nkeep = \"yes\"\n"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"type":"create","filePath":"/w/app.toml","content":"name = \"harbor\"\nkeep = \"yes\"\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"w1","content":"File created successfully at: /w/app.toml"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:01:00.000Z","trackedFileBackups":{"/w/app.toml":{"backupFileName":"feed0001@v1","version":1,"backupTime":"2026-06-07T05:01:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:05:00.000Z","message":{"role":"user","content":"carry on"}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m2","snapshot":{"messageId":"m2","timestamp":"2026-06-07T05:06:00.000Z","trackedFileBackups":{"/w/app.toml":{"backupFileName":"feed0001@v2","version":2,"backupTime":"2026-06-07T05:06:00.000Z"}}},"isSnapshotUpdate":false}"#, "\n",
        ),
    );
    if with_store {
        let set = |rel: &str, content: &str, ts: &str| {
            let p = h.write_claude(rel, content);
            let instant: jiff::Timestamp = ts.parse().unwrap();
            let t = std::time::UNIX_EPOCH
                + std::time::Duration::from_secs(u64::try_from(instant.as_second()).unwrap());
            std::fs::File::options()
                .write(true)
                .open(&p)
                .unwrap()
                .set_modified(t)
                .unwrap();
        };
        set(
            &format!("file-history/{SSESS}/feed0001@v1"),
            "name = \"harbor\"\nkeep = \"yes\"\n",
            "2026-06-07T05:01:00Z",
        );
        // The silent write DELETED the `keep` key - the v2 blob is the disk truth.
        set(
            &format!("file-history/{SSESS}/feed0001@v2"),
            "name = \"harbor\"\n",
            "2026-06-07T05:06:00Z",
        );
    }
    h
}

#[test]
fn divergent_snapshot_rebases_the_replay() {
    let h = snapshot_home(true);
    // 0.9.3 replayed the never-existed state (the deleted key still present) and
    // called it complete. The verified v2 blob now rebases the replay.
    let out = h.run(&[
        "recover",
        at(SSESS).as_str(),
        "--file",
        "/w/app.toml",
        "--at",
        "@latest",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("keep"),
        "the silently-deleted key must not resurrect:\n{}",
        out.stdout
    );
    assert!(out.stdout.contains("name = \"harbor\""), "{}", out.stdout);
    let cov = h.run(&[
        "recover",
        at(SSESS).as_str(),
        "--file",
        "/w/app.toml",
        "--coverage",
    ]);
    assert!(
        cov.stdout.contains("REBASED on the snapshot content"),
        "the divergence is disclosed:\n{}",
        cov.stdout
    );
}

#[test]
fn content_less_jump_discloses_without_rebasing() {
    let h = snapshot_home(false);
    let cov = h.run(&[
        "recover",
        at(SSESS).as_str(),
        "--file",
        "/w/app.toml",
        "--coverage",
    ]);
    assert!(cov.success, "stderr: {}", cov.stderr);
    assert!(
        cov.stdout.contains("version jumped") && cov.stdout.contains("unavailable"),
        "the silent jump is disclosed even with the store pruned:\n{}",
        cov.stdout
    );
    // The replay itself is unchanged (nothing to rebase on) - the stale content
    // still renders, with the boundary counted as HARD.
    assert!(
        cov.stdout.contains("1 hard"),
        "external_write counts hard:\n{}",
        cov.stdout
    );
}
