//! v0.10.0 promoted non-record leaves: gated behind an explicit selector, each
//! reachable by its own path, fabricated excerpts sound under the whole-file gate,
//! `show` renders them flag-free, and the zero-match diagnosis names the gate.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7a6b5c40-3d2e-4f1a-8b9c-0d1e2f3a4b5c";

/// L1 user · L2 queued human text · L3 queued automation rider · L4 dequeue (no content) ·
/// L5 assistant · L6 stop-hook ledger · L7 turn duration · L8 away recap · L9 snapshot ·
/// L10 delta · L11 the queued text removed with a reason.
fn promoted_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"kick off the harbor survey"}}"#, "\n",
            r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:00:10.000Z","sessionId":"s","content":"also chart the reef shoals"}"#, "\n",
            r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:00:11.000Z","sessionId":"s","content":"<task-notification>\n<task-id>bq1</task-id>\n<status>completed</status>\n<summary>Background command finished: buoy check</summary>\n</task-notification>"}"#, "\n",
            r#"{"type":"queue-operation","operation":"dequeue","timestamp":"2026-06-07T05:00:12.000Z","sessionId":"s"}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","content":[{"type":"text","text":"surveying the harbor now"}]}}"#, "\n",
            r#"{"type":"system","subtype":"stop_hook_summary","uuid":"h1","parentUuid":"a1","timestamp":"2026-06-07T05:01:00.000Z","hookCount":2,"hookInfos":[{"command":"/opt/hooks/lint.sh","durationMs":9},{"command":"/opt/hooks/notify.sh","durationMs":4}],"hookErrors":[],"preventedContinuation":false}"#, "\n",
            r#"{"type":"system","subtype":"turn_duration","uuid":"t1","parentUuid":"h1","timestamp":"2026-06-07T05:01:05.000Z","durationMs":64911,"messageCount":12,"pendingBackgroundAgentCount":2}"#, "\n",
            r#"{"type":"system","subtype":"away_summary","uuid":"w1","parentUuid":"t1","timestamp":"2026-06-07T05:07:00.000Z","content":"Survey finished; the reef pass is charted."}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"u1","snapshot":{"messageId":"u1","trackedFileBackups":{"/proj/notes.md":{"backupFileName":"notes.md@v3","version":3,"backupTime":"2026-06-07T05:00:00.000Z"}},"timestamp":"2026-06-07T05:00:00.500Z"},"isSnapshotUpdate":false}"#, "\n",
            r#"{"type":"file-history-delta","messageId":"a1","snapshotMessageId":"u1","trackingPath":"/proj/notes.md","backup":{"backupFileName":"notes.md@v4","version":4,"backupTime":"2026-06-07T05:00:30.000Z"},"timestamp":"2026-06-07T05:00:30.000Z"}"#, "\n",
            r#"{"type":"queue-operation","operation":"remove","timestamp":"2026-06-07T05:08:00.000Z","sessionId":"s","content":"also chart the reef shoals","reason":"absorbed_mid_turn"}"#, "\n",
        ),
    );
    h
}

const PROMOTED_TEXTS: [&str; 5] = [
    "reef shoals",
    "[turn duration:",
    "[stop hooks:",
    "[file-history",
    "Survey finished",
];

#[test]
fn gated_leaves_stay_out_of_bare_and_role_scans() {
    let h = promoted_home();
    // A bare scan sees the conversation only.
    let bare = h.run(&["search", "", &at(SESS)]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(bare.stdout.contains("harbor survey"), "{}", bare.stdout);
    for t in PROMOTED_TEXTS {
        assert!(
            !bare.stdout.contains(t),
            "bare scan leaked {t:?}:\n{}",
            bare.stdout
        );
    }
    // Bare roles keep the visible leaves only.
    let user = h.run(&["search", "", &at(SESS), "-t", "user"]);
    assert!(
        user.stdout.contains("harbor survey") && !user.stdout.contains("reef shoals"),
        "{}",
        user.stdout
    );
    let harness = h.run(&["search", "", &at(SESS), "-t", "harness"]);
    assert!(
        !harness.stdout.contains("[turn duration:") && !harness.stdout.contains("Survey finished"),
        "{}",
        harness.stdout
    );
    // A -T-only filter is not an explicit reach either.
    let excl = h.run(&["search", "", &at(SESS), "-T", "agent"]);
    assert!(!excl.stdout.contains("reef shoals"), "{}", excl.stdout);
    // The label census without -t never counts them.
    let census = h.run(&["search", "", &at(SESS), "--count-by", "label"]);
    assert!(
        !census.stdout.contains("user.queued") && !census.stdout.contains("harness.meta."),
        "{}",
        census.stdout
    );
}

#[test]
fn explicit_selectors_reach_each_promoted_leaf() {
    let h = promoted_home();
    let q = h.run(&["search", "", &at(SESS), "-t", "user.queued"]);
    assert!(q.success, "stderr: {}", q.stderr);
    assert!(
        q.stdout.contains("user.queued [enqueue]")
            && q.stdout
                .contains("user.queued [remove · absorbed_mid_turn]")
            && q.stdout.matches("reef shoals").count() == 2,
        "both content-bearing queue events, labeled by operation:\n{}",
        q.stdout
    );
    // The automation rider and the content-less dequeue are not the human's queue.
    assert!(
        !q.stdout.contains("buoy check") && !q.stdout.contains("dequeue"),
        "{}",
        q.stdout
    );
    // The glob and the harness.meta prefix reach the rest.
    let glob = h.run(&["search", "", &at(SESS), "-t", "user.*"]);
    assert!(
        glob.stdout.contains("reef shoals") && glob.stdout.contains("harbor survey"),
        "{}",
        glob.stdout
    );
    let meta = h.run(&["search", "", &at(SESS), "-t", "harness.meta"]);
    assert!(meta.success, "stderr: {}", meta.stderr);
    for (leaf, text) in [
        (
            "harness.meta.turn-duration",
            "[turn duration: 1m 5s · durationMs=64911 messageCount=12 pendingBackgroundAgentCount=2]",
        ),
        ("harness.meta.stop-hooks", "[stop hooks: count=2 errors=0 prevented=false]"),
        ("harness.meta.away-summary", "Survey finished; the reef pass is charted."),
        (
            "harness.meta.snapshot",
            "[file-history snapshot at 2026-06-07T05:00:00.500Z: /proj/notes.md@v3]",
        ),
        (
            "harness.meta.snapshot",
            "[file-history delta at 2026-06-07T05:00:30.000Z: /proj/notes.md@v4 backup=notes.md@v4]",
        ),
    ] {
        assert!(
            meta.stdout.contains(leaf) && meta.stdout.contains(text),
            "missing {leaf} / {text:?}:\n{}",
            meta.stdout
        );
    }
    // The stop-hook commands are the ledger's searchable text.
    assert!(
        meta.stdout.contains("/opt/hooks/lint.sh (9ms)"),
        "{}",
        meta.stdout
    );
    // A single exact leaf selects just that leaf.
    let one = h.run(&["search", "", &at(SESS), "-t", "harness.meta.away-summary"]);
    assert!(
        one.stdout.contains("Survey finished") && !one.stdout.contains("[turn duration:"),
        "{}",
        one.stdout
    );
}

#[test]
fn fabricated_excerpts_survive_the_whole_file_gate() {
    // Each of these patterns matches ONLY fabricated text (never a raw byte substring:
    // the raw line reads `"pendingBackgroundAgentCount":2`, `"hookCount":2`,
    // `"version":3`), so without the synth marker the whole-file gate would skip the
    // file and silently drop the match.
    let h = promoted_home();
    for (pattern, leaf, want) in [
        (
            "pendingBackgroundAgentCount=2",
            "harness.meta.turn-duration",
            "[turn duration:",
        ),
        (
            "count=2 errors=0",
            "harness.meta.stop-hooks",
            "[stop hooks:",
        ),
        (
            "notes\\.md@v3",
            "harness.meta.snapshot",
            "[file-history snapshot",
        ),
        (
            "notes\\.md@v4",
            "harness.meta.snapshot",
            "[file-history delta",
        ),
        ("1m 5s", "harness.meta.turn-duration", "durationMs=64911"),
    ] {
        let out = h.run(&["search", pattern, &at(SESS), "-t", leaf]);
        assert!(out.success, "stderr: {}", out.stderr);
        assert!(
            out.stdout.contains(want) && out.stdout.contains("matched 1 exchange"),
            "{pattern:?} under {leaf}:\n{}\n{}",
            out.stdout,
            out.stderr
        );
    }
    // Verbatim content matches through the ordinary literal prefilter.
    let out = h.run(&[
        "search",
        "reef pass is charted",
        &at(SESS),
        "-t",
        "harness.meta.away-summary",
    ]);
    assert!(out.stdout.contains("matched 1 exchange"), "{}", out.stdout);
}

#[test]
fn json_carries_queue_facts_and_census_counts_gated_leaves() {
    let h = promoted_home();
    let j = h.run(&[
        "search",
        "reef shoals",
        &at(SESS),
        "-t",
        "user.queued",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &serde_json::Value| v["kind"] == "exchange")
        .collect();
    let hits: Vec<&serde_json::Value> = rows
        .iter()
        .flat_map(|r| r["hits"].as_array().unwrap())
        .collect();
    assert_eq!(hits.len(), 2, "{}", j.stdout);
    let ops: Vec<(String, Option<String>)> = hits
        .iter()
        .map(|h| {
            (
                h["queue_operation"].as_str().unwrap().to_string(),
                h["queue_reason"].as_str().map(str::to_string),
            )
        })
        .collect();
    assert!(ops.contains(&("enqueue".to_string(), None)), "{ops:?}");
    assert!(
        ops.contains(&("remove".to_string(), Some("absorbed_mid_turn".to_string()))),
        "{ops:?}"
    );
    assert_eq!(hits[0]["label"], "user.queued", "{}", j.stdout);
    assert_eq!(
        hits[0]["labels"],
        serde_json::json!(["user.queued"]),
        "{}",
        j.stdout
    );
    // A non-queue hit carries null queue facts.
    let u = h.run(&["search", "harbor survey", &at(SESS), "--format", "json"]);
    let row: serde_json::Value = u
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "exchange")
        .unwrap();
    assert!(row["hits"][0]["queue_operation"].is_null(), "{}", u.stdout);
    // The label census counts the gated leaves once explicitly reached.
    let census = h.run(&[
        "search",
        "",
        &at(SESS),
        "--count-by",
        "label",
        "-t",
        "harness.*",
    ]);
    for want in [
        "harness.meta.turn-duration",
        "harness.meta.stop-hooks",
        "harness.meta.away-summary",
        "harness.meta.snapshot",
    ] {
        assert!(
            census.stdout.contains(want),
            "census lacks {want}:\n{}",
            census.stdout
        );
    }
    assert!(!census.stdout.contains("user.queued"), "{}", census.stdout);
}

#[test]
fn show_renders_promoted_lines_flag_free() {
    let h = promoted_home();
    let td = h.run(&["show", &at(SESS), "--line", "7"]);
    assert!(td.success, "stderr: {}", td.stderr);
    assert!(
        td.stdout.contains("harness.meta.turn-duration")
            && td.stdout.contains("pendingBackgroundAgentCount=2"),
        "{}",
        td.stdout
    );
    let q = h.run(&["show", &at(SESS), "--line", "2"]);
    assert!(
        q.stdout.contains("user.queued [enqueue]") && q.stdout.contains("reef shoals"),
        "{}",
        q.stdout
    );
    let snap = h.run(&["show", &at(SESS), "--line", "9..10"]);
    assert!(
        snap.stdout.contains("notes.md@v3") && snap.stdout.contains("notes.md@v4"),
        "{}",
        snap.stdout
    );
    // By uuid too (the system records carry one).
    let byu = h.run(&["show", &at(SESS), "--uuid", "w1"]);
    assert!(byu.stdout.contains("Survey finished"), "{}", byu.stdout);
    // A content-less dequeue and an automation rider are still not renderable records.
    let dq = h.run(&["show", &at(SESS), "--line", "4"]);
    assert!(!dq.success, "{}", dq.stdout);
    assert!(
        dq.stderr.contains("no such record(s): L4") && dq.stderr.contains("--raw"),
        "{}",
        dq.stderr
    );
    let raw = h.run(&["show", &at(SESS), "--line", "4", "--raw"]);
    assert!(
        raw.stdout.contains(r#""operation":"dequeue""#),
        "{}",
        raw.stdout
    );
}

#[test]
fn zero_match_diagnosis_names_the_gate() {
    let h = promoted_home();
    // Absent everywhere, no selector: the note says the gated lines were never scanned.
    let out = h.run(&["search", "quartz-lantern", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stderr.contains("scanned only under an explicit -t"),
        "{}",
        out.stderr
    );
    let j = h.run(&["search", "quartz-lantern", &at(SESS), "--format", "json"]);
    let summary: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "summary")
        .unwrap();
    assert_eq!(summary["gated_leaves_unreached"], true, "{}", j.stdout);
    // With an explicit gated selector the note is gone (those lines WERE scanned).
    let out = h.run(&["search", "quartz-lantern", &at(SESS), "-t", "harness.meta"]);
    assert!(
        !out.stderr.contains("scanned only under an explicit -t"),
        "{}",
        out.stderr
    );
}

/// v0.10.1: the `harness.meta.system` catch-all. L1 user · L2 an `informational`
/// warning (the Remote Control disconnect notice shape) · L3 an `agents_killed`
/// record with a non-string content · L4 a `compact_boundary` (its own leaf, never
/// the catch-all) · L5 assistant.
fn system_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"system","subtype":"informational","level":"warning","uuid":"i1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","content":"Remote Control disconnected - signed-in account changed on this machine - run /remote-control to start a session"}"#, "\n",
            r#"{"type":"system","subtype":"agents_killed","uuid":"k1","parentUuid":"i1","timestamp":"2026-06-07T05:00:02.000Z","content":{"count":2,"reason":"user"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"b1","parentUuid":null,"timestamp":"2026-06-07T05:00:03.000Z","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":1000,"postTokens":100}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"b1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn meta_system_catch_all_is_gated_rendered_and_gate_sound() {
    let h = system_home();
    // A bare scan never parses the line; the diagnosis names the gate.
    let bare = h.run(&["search", "Remote Control disconnected", &at(SESS)]);
    assert!(
        bare.success && bare.stdout.contains("no matching exchanges"),
        "{}",
        bare.stdout
    );
    assert!(
        bare.stderr.contains("harness.meta.system"),
        "{}",
        bare.stderr
    );
    // The explicit leaf reaches both catch-all records, rendered as
    // `[<subtype> <level>] <content>` / `[<subtype>] <json>`.
    let out = h.run(&["search", "", &at(SESS), "-t", "harness.meta.system"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.meta.system")
            && out
                .stdout
                .contains("[informational warning] Remote Control disconnected")
            && out
                .stdout
                .contains(r#"[agents_killed] {"count":2,"reason":"user"}"#),
        "{}",
        out.stdout
    );
    // The compaction boundary keeps its own leaf: it never appears under the catch-all.
    assert!(
        !out.stdout.contains("Conversation compacted"),
        "{}",
        out.stdout
    );
    // Both records sit in the same turn: one exchange, two hit lines.
    assert!(out.stdout.contains("matched 1 exchange"), "{}", out.stdout);
    // The `harness.meta` prefix and the harness glob reach it too; the bare role does not.
    let prefix = h.run(&["search", "Remote Control", &at(SESS), "-t", "harness.meta"]);
    assert!(
        prefix.stdout.contains("[informational warning]"),
        "{}",
        prefix.stdout
    );
    let role = h.run(&["search", "Remote Control", &at(SESS), "-t", "harness"]);
    assert!(
        role.stdout.contains("no matching exchanges"),
        "{}",
        role.stdout
    );
    // The whole-file gate stays sound: a pattern matching ONLY the fabricated head.
    let head = h.run(&[
        "search",
        r"\[informational warning\]",
        &at(SESS),
        "-t",
        "harness.meta.system",
    ]);
    assert!(
        head.stdout.contains("matched 1 exchange"),
        "{}",
        head.stdout
    );
    // JSON carries the leaf; `show --line` renders it flag-free (the refetch law).
    let js = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.meta.system",
        "--format",
        "json",
    ]);
    assert!(
        js.stdout.contains(r#""label":"harness.meta.system""#),
        "{}",
        js.stdout
    );
    let shown = h.run(&["show", &at(SESS), "--line", "2"]);
    assert!(
        shown.success
            && shown
                .stdout
                .contains("[informational warning] Remote Control disconnected"),
        "{}\n{}",
        shown.stdout,
        shown.stderr
    );
    // The census counts it under its leaf.
    let census = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.meta.system",
        "--count-by",
        "label",
    ]);
    assert!(
        census.stdout.contains("2  harness.meta.system"),
        "{}",
        census.stdout
    );
}
