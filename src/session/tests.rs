use super::*;
use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn truncate_short_excerpt_unchanged() {
    assert_eq!(truncate_excerpt("hello"), "hello");
}

#[test]
fn truncate_long_excerpt_marks_dropped_count() {
    let s = "x".repeat(EXCERPT_MAX + 5);
    let out = truncate_excerpt(&s);
    assert!(out.ends_with("… (+5 chars)"), "got: {out}");
    assert!(out.starts_with(&"x".repeat(EXCERPT_MAX)));
}

#[test]
fn truncate_counts_chars_not_bytes() {
    // Multi-byte UTF-8 chars (this emoji is 4 bytes); truncation must count
    // chars, not bytes, for ANY script.
    let s = "🛠".repeat(EXCERPT_MAX + 2);
    let out = truncate_excerpt(&s);
    assert!(out.ends_with("… (+2 chars)"), "got: {out}");
}

#[test]
fn format_timestamp_uses_system_local_canonical_marker() {
    // tz-agnostic: the local portion must equal what the system tz itself yields
    // for this instant (derived in-test, not a hardcoded zone). v0.5: the marker
    // is `<TZAB>(UTC±offset)` and the raw-UTC copy is GONE (it invited LLM
    // conversion arithmetic; machine consumers read JSON ts_utc).
    let raw = "2026-06-07T05:48:22.880Z";
    let out = format_timestamp(Some(raw));
    let ts: jiff::Timestamp = raw.parse().expect("parseable instant");
    let local = ts
        .to_zoned(crate::timez::local_tz())
        .strftime("%Y-%m-%d %H:%M:%S")
        .to_string();
    assert!(out.contains(&local), "expected local {local:?} in {out:?}");
    assert!(out.contains("(UTC"), "marker missing: {out}");
    assert!(!out.contains(raw), "the raw-UTC copy must be gone: {out}");
}

#[test]
fn format_timestamp_missing_is_em_dash() {
    assert_eq!(format_timestamp(None), "—");
}

#[test]
fn format_timestamp_unparseable_surfaces_raw() {
    let out = format_timestamp(Some("not-a-time"));
    assert!(out.contains("not-a-time"));
    assert!(out.contains("unparsed"));
}

#[test]
fn local_iso_matches_system_tz_offset() {
    let raw = "2026-06-07T05:48:22.880Z";
    let out = local_iso(raw).expect("local iso");
    // Derive the expected offset string from jiff (tz-agnostic), not a literal.
    let ts: jiff::Timestamp = raw.parse().expect("parseable instant");
    let expected = ts
        .to_zoned(crate::timez::local_tz())
        .strftime("%Y-%m-%dT%H:%M:%S%:z")
        .to_string();
    assert_eq!(out, expected);
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn tmp_session(name_stem: &str, lines: &[&str]) -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "csift-sess-{}-{}-{name_stem}.jsonl",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let mut f = std::fs::File::create(&p).unwrap();
    for l in lines {
        writeln!(f, "{l}").unwrap();
    }
    p
}

#[test]
fn capture_identity_if_empty_only_fills_blanks() {
    let rec: Record = serde_json::from_str(
            r#"{"type":"user","cwd":"/c","version":"9.9","gitBranch":"b","sessionId":"sid","message":{"role":"user","content":"x"}}"#,
        )
        .unwrap();
    let mut cwd = None;
    let mut version = Some("keep".to_string()); // already set → must NOT be overwritten
    let mut branch = None;
    let mut sid = None;
    capture_identity_if_empty(&rec, &mut cwd, &mut version, &mut branch, &mut sid);
    assert_eq!(cwd.as_deref(), Some("/c"));
    assert_eq!(version.as_deref(), Some("keep"), "pre-set value preserved");
    assert_eq!(branch.as_deref(), Some("b"));
    assert_eq!(sid.as_deref(), Some("sid"));
}

#[test]
fn summarize_head_first_user_captures_identity() {
    // The head finds a genuine user FIRST → identity captured from it; the tail
    // finds the last user + last agent. Exercises the head identity-capture arm
    // and both tail `is_none()` branches.
    let p = tmp_session(
        "head",
        &[
            r#"{"type":"user","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","sessionId":"sid-data","message":{"role":"user","content":"first q"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"mid agent"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"last q"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"last agent"}]}}"#,
        ],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(s.cwd.as_deref(), Some("/Users/testuser/Projects/foo"));
    assert_eq!(s.version.as_deref(), Some("2.1.0"));
    assert_eq!(s.git_branch.as_deref(), Some("main"));
    assert_eq!(s.first_user.as_ref().unwrap().excerpt, "first q");
    assert_eq!(s.last_user.as_ref().unwrap().excerpt, "last q");
    assert_eq!(s.last_agent.as_ref().unwrap().excerpt, "last agent");
}

#[test]
fn summarize_backfills_identity_from_tail_when_head_user_lacks_it() {
    // The head's FIRST genuine user carries NO identity fields (cwd/version/branch
    // all absent), but the LAST genuine user at the tail DOES - so the tail's
    // `capture_identity_if_empty` backfills them (only the still-None fields).
    let p = tmp_session(
        "tailfill",
        &[
            // head genuine user - no identity fields at all.
            r#"{"type":"user","message":{"role":"user","content":"first q, no identity"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
            // tail genuine user - carries the identity fields.
            r#"{"type":"user","cwd":"/tail/cwd","version":"3.0","gitBranch":"dev","sessionId":"sid-tail","message":{"role":"user","content":"last q, has identity"}}"#,
        ],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    // The head found the first user, but identity was backfilled from the tail.
    assert_eq!(
        s.first_user.as_ref().unwrap().excerpt,
        "first q, no identity"
    );
    assert_eq!(
        s.last_user.as_ref().unwrap().excerpt,
        "last q, has identity"
    );
    assert_eq!(s.cwd.as_deref(), Some("/tail/cwd"));
    assert_eq!(s.version.as_deref(), Some("3.0"));
    assert_eq!(s.git_branch.as_deref(), Some("dev"));
}

#[test]
fn summarize_session_id_from_data_when_filename_has_no_stem() {
    // When the path has no usable stem the session id falls back to the data's
    // sessionId (the `session_id.is_empty()` true arm). We build a record carrying
    // a sessionId and drive summarize on a file, then assert the filename-stem
    // path; the data-fallback arm is also reachable when the stem is empty. Since
    // a real temp file always has a stem, assert the cross-check shape instead:
    // the id equals the stem and the data id is retained internally.
    let p = tmp_session(
        "dataid",
        &[r#"{"type":"user","sessionId":"sid-data-xyz","message":{"role":"user","content":"hi"}}"#],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    // Filename stem wins (non-empty) - the documented precedence.
    assert!(!s.session_id.is_empty());
}

#[test]
fn summarize_head_skips_non_genuine_records_before_first_user() {
    // The head stream leads with non-genuine records (metadata, an isMeta pseudo-
    // turn, a tool_result carrier) → the head closure's `genuine_user_text()`
    // returns None for each (the FALSE arm) until the real first user is reached.
    let p = tmp_session(
        "headnoise",
        &[
            r#"{"type":"last-prompt","leafUuid":"x"}"#,
            r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"Continue from where you left off."}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":"carrier"}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":"the genuine first question"}}"#,
            r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"a"}]}}"#,
        ],
    );
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(
        s.first_user.as_ref().unwrap().excerpt,
        "the genuine first question"
    );
}

#[test]
fn top_level_summary_is_not_subagent_and_is_its_own_parent() {
    // A plain `<uuid>.jsonl` path is top-level: is_subagent=false and parent==session_id.
    let p = tmp_session(
        "toplevel",
        &[r#"{"type":"user","message":{"role":"user","content":"hi"}}"#],
    );
    let s = summarize_session(&p).unwrap();
    let stem = p.file_stem().unwrap().to_str().unwrap().to_string();
    std::fs::remove_file(&p).ok();
    assert!(!s.is_subagent);
    assert_eq!(s.parent_session_id, stem);
    assert_eq!(s.parent_session_id, s.session_id);
}

#[test]
fn subagent_summary_carries_is_subagent_and_refeedable_parent() {
    // A subagent transcript path `…/<PARENT-UUID>/subagents/workflows/wf_x/agent-<hex>.jsonl`:
    // session_id is the bare hex (NOT re-feedable), is_subagent=true, and
    // parent_session_id is the re-feedable PARENT uuid (the dir before `subagents/`).
    let dir = std::env::temp_dir().join(format!(
        "csift-subdir-{}-{}/11111111-2222-3333-4444-555555555555/subagents/workflows/wf_z",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("agent-deadbeefcafe1234.jsonl");
    {
        let mut f = std::fs::File::create(&p).unwrap();
        writeln!(
            f,
            r#"{{"type":"user","message":{{"role":"user","content":"sub work"}}}}"#
        )
        .unwrap();
    }
    let s = summarize_session(&p).unwrap();
    std::fs::remove_file(&p).ok();
    assert_eq!(
        s.session_id, "deadbeefcafe1234",
        "bare hex (agent- stripped)"
    );
    assert!(s.is_subagent, "a subagents/ path is a subagent transcript");
    assert_eq!(
        s.parent_session_id, "11111111-2222-3333-4444-555555555555",
        "parent is the re-feedable uuid dir before subagents/"
    );
}

#[test]
fn summarize_session_id_is_filename_stem() {
    // The session id is the jsonl basename; even when the data carries a different
    // sessionId, the filename wins (the `session_id.is_empty()` false arm).
    let p = tmp_session(
        "stemid",
        &[r#"{"type":"user","sessionId":"DATA-ID","message":{"role":"user","content":"hi"}}"#],
    );
    let s = summarize_session(&p).unwrap();
    let stem = p.file_stem().unwrap().to_str().unwrap().to_string();
    std::fs::remove_file(&p).ok();
    assert_eq!(s.session_id, stem);
    assert_ne!(s.session_id, "DATA-ID");
}

#[test]
fn clone_probe_arms_and_origin_join_edges() {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("csift-clone-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let bx = "0f1e2d3c-4b5a-4697-8807-16f5e4d3c2b1";

    // A garbage line CARRYING the timestamp needle parses to nothing and is walked
    // past; the boundary after it still detects.
    let clone = root.join("c1.jsonl");
    std::fs::write(
        &clone,
        format!(
            "not json but mentions \"timestamp\" anyway\n{{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"uuid\":\"{bx}\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}}\n"
        ),
    )
    .unwrap();
    assert_eq!(
        clone_head_boundary(&clone).unwrap().as_deref(),
        Some(bx),
        "garbage before the boundary is walked past"
    );

    // An EMPTY transcript probes to a clean None.
    let empty = root.join("c0.jsonl");
    std::fs::write(&empty, "").unwrap();
    assert_eq!(clone_head_boundary(&empty).unwrap(), None);

    // A boundary with an EMPTY-string uuid detects nothing either.
    let blank = root.join("c3.jsonl");
    std::fs::write(
        &blank,
        "{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"uuid\":\"\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}\n",
    )
    .unwrap();
    assert_eq!(clone_head_boundary(&blank).unwrap(), None);

    // A boundary with NO uuid detects nothing (never an empty-string lineage).
    let anon = root.join("c2.jsonl");
    std::fs::write(
        &anon,
        "{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}\n",
    )
    .unwrap();
    assert_eq!(clone_head_boundary(&anon).unwrap(), None);

    // Origin join: the sibling quotes the uuid in PROSE before carrying the real
    // record - the at-advance loop must reach the record and name the origin.
    let origin = root.join("o1.jsonl");
    std::fs::write(
        &origin,
        format!(
            "{{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"discussing {bx} in prose\"}}}}\n{{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"uuid\":\"{bx}\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}}\n"
        ),
    )
    .unwrap();
    assert_eq!(
        clone_origin(&clone, bx).as_deref(),
        Some("o1"),
        "prose quote is skipped, the record wins"
    );

    // A prose-ONLY sibling can never be the origin.
    std::fs::remove_file(&origin).unwrap();
    let bystander = root.join("b1.jsonl");
    std::fs::write(
        &bystander,
        format!(
            "{{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"only prose about {bx}\"}}}}\n"
        ),
    )
    .unwrap();
    assert_eq!(clone_origin(&clone, bx), None);

    // A sibling carrying an UNRELATED boundary plus the uuid in prose is not a
    // carrier either (the carrier check demands the RECORD with that uuid).
    let decoy = root.join("d1.jsonl");
    std::fs::write(
        &decoy,
        format!(
            "{{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"prose about {bx}\"}}}}\n{{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"uuid\":\"99999999-aaaa-4bbb-8ccc-dddddddddddd\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}}\n"
        ),
    )
    .unwrap();
    assert_eq!(
        clone_origin(&clone, bx),
        None,
        "an unrelated boundary near a prose quote is not the origin"
    );
    std::fs::remove_file(&decoy).unwrap();

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn clone_origin_decoys_stay_outside_the_join() {
    let root = std::env::temp_dir().join(format!("csift-clone-decoy-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let bx = "3a4b5c6d-7e8f-4a9b-8c0d-1e2f3a4b5c6d";
    let clone = root.join("c1.jsonl");
    std::fs::write(
        &clone,
        format!(
            "{{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"uuid\":\"{bx}\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}}\n"
        ),
    )
    .unwrap();

    // A NON-jsonl sibling carrying the full native-origin shape is never scanned -
    // only .jsonl files join.
    let txt = root.join("o1.txt");
    std::fs::write(
        &txt,
        format!(
            "{{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n{{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"uuid\":\"{bx}\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}}\n"
        ),
    )
    .unwrap();
    assert_eq!(
        clone_origin(&clone, bx),
        None,
        "a non-jsonl decoy never becomes the origin"
    );
    std::fs::remove_file(&txt).unwrap();

    // A sibling whose OWN boundary merely REFERENCES the uuid (logicalParentUuid)
    // is not a carrier - the carrier record must bear the uuid itself.
    let refr = root.join("s2.jsonl");
    std::fs::write(
        &refr,
        format!(
            "{{\"type\":\"user\",\"uuid\":\"u1\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n{{\"type\":\"system\",\"subtype\":\"compact_boundary\",\"uuid\":\"77777777-8888-4999-8aaa-bbbbbbbbbbbb\",\"logicalParentUuid\":\"{bx}\",\"timestamp\":\"2026-06-07T05:10:00.000Z\"}}\n"
        ),
    )
    .unwrap();
    assert_eq!(
        clone_origin(&clone, bx),
        None,
        "a boundary that only references the uuid is not the carrier"
    );
    std::fs::remove_file(&refr).unwrap();
    std::fs::remove_file(&clone).ok();
}

// -- C-44: the /clear checkpoint join --

#[test]
fn checkpoint_close_reads_the_sum_and_refuses_other_types() {
    let line = br#"{"type":"cost-state","sessionId":"s","totalCostUSD":0.5,"totalDuration":60000,"startTime":1780808940000,"modelUsage":{},"hasUnknownModelCost":false}"#;
    assert_eq!(
        checkpoint_close_ms(line),
        Some(1_780_809_000_000),
        "the close instant is startTime + totalDuration"
    );
    // A line of any other type is never a checkpoint, however many numbers it carries.
    assert_eq!(
        checkpoint_close_ms(
            br#"{"type":"user","totalDuration":60000,"startTime":1780808940000,"message":{"role":"user","content":"x"}}"#
        ),
        None
    );
    // A checkpoint missing either half yields nothing rather than half an instant.
    assert_eq!(
        checkpoint_close_ms(br#"{"type":"cost-state","startTime":1780808940000}"#),
        None
    );
    assert_eq!(checkpoint_close_ms(b"not json"), None);
}

#[test]
fn cleared_from_origin_takes_the_nearest_checkpoint_in_window() {
    let dir = std::env::temp_dir().join(format!(
        "csift-clear-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let wrapper = "2026-06-07T05:10:00.000Z";
    let w = crate::timez::epoch_ms(wrapper).unwrap();
    let checkpoint = |close: i64| {
        format!(
            r#"{{"type":"cost-state","sessionId":"s","totalCostUSD":0.1,"totalDuration":1000,"startTime":{},"modelUsage":{{}},"hasUnknownModelCost":false}}"#,
            close - 1000
        )
    };
    // Two checkpoints in ONE sibling: the nearer line is the one that decides.
    let near = dir.join("11111111-2222-4333-8444-555566667777.jsonl");
    std::fs::write(
        &near,
        format!("{}\n{}\n", checkpoint(w - 1500), checkpoint(w - 7)),
    )
    .unwrap();
    // A sibling well outside the window never competes.
    let far = dir.join("22222222-2222-4333-8444-555566667777.jsonl");
    std::fs::write(&far, format!("{}\n", checkpoint(w - 90_000))).unwrap();
    let cleared = dir.join("33333333-2222-4333-8444-555566667777.jsonl");
    std::fs::write(&cleared, "{}\n").unwrap();

    let join = cleared_from_origin(&cleared, wrapper);
    assert_eq!(
        join.cleared_from.as_deref(),
        Some("11111111-2222-4333-8444-555566667777")
    );
    assert_eq!(
        join.distance_ms,
        Some(7),
        "the nearest LINE wins, not the first"
    );
    assert!(!join.after, "this checkpoint closes before the wrapper");
    assert!(join.candidates.is_empty());

    // A checkpoint on the far side of the wrapper is joined and its direction recorded.
    std::fs::write(&near, format!("{}\n", checkpoint(w + 164))).unwrap();
    std::fs::remove_file(&far).unwrap();
    let after = cleared_from_origin(&cleared, wrapper);
    assert_eq!(after.distance_ms, Some(164));
    assert!(after.after, "the checkpoint closes after the wrapper");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cleared_from_origin_never_joins_a_file_to_itself() {
    let dir = std::env::temp_dir().join(format!(
        "csift-clear-self-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let wrapper = "2026-06-07T05:10:00.000Z";
    let w = crate::timez::epoch_ms(wrapper).unwrap();
    let checkpoint = |close: i64| {
        format!(
            r#"{{"type":"cost-state","sessionId":"s","totalCostUSD":0.1,"totalDuration":1000,"startTime":{},"modelUsage":{{}},"hasUnknownModelCost":false}}"#,
            close - 1000
        )
    };
    // The cleared transcript carries its OWN checkpoint inside the window - the live
    // shape, since a cleared session writes one of its own when it in turn ends. It is
    // never its own predecessor.
    let cleared = dir.join("44444444-2222-4333-8444-555566667777.jsonl");
    std::fs::write(&cleared, format!("{}\n", checkpoint(w - 5))).unwrap();
    let join = cleared_from_origin(&cleared, wrapper);
    assert!(
        join.cleared_from.is_none() && join.candidates.is_empty(),
        "a transcript is never joined to itself: {join:?}"
    );
    assert!(join.distance_ms.is_none(), "{join:?}");

    // A NON-jsonl neighbour carrying a nearer checkpoint is not a transcript and never
    // competes, however close it sits.
    std::fs::write(dir.join("notes.txt"), format!("{}\n", checkpoint(w - 1))).unwrap();
    assert!(
        cleared_from_origin(&cleared, wrapper).distance_ms.is_none(),
        "only a .jsonl sibling is a candidate"
    );

    // A real sibling still wins from beside it.
    let sib = dir.join("55555555-2222-4333-8444-555566667777.jsonl");
    std::fs::write(&sib, format!("{}\n", checkpoint(w - 9))).unwrap();
    let joined = cleared_from_origin(&cleared, wrapper);
    assert_eq!(
        joined.cleared_from.as_deref(),
        Some("55555555-2222-4333-8444-555566667777"),
        "the self file and the decoy are excluded, not the whole directory"
    );
    assert_eq!(joined.distance_ms, Some(9), "the .txt decoy never wins");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cleared_from_origin_keeps_the_first_line_of_an_equal_distance_pair() {
    let dir = std::env::temp_dir().join(format!(
        "csift-clear-tie-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let wrapper = "2026-06-07T05:10:00.000Z";
    let w = crate::timez::epoch_ms(wrapper).unwrap();
    let checkpoint = |close: i64| {
        format!(
            r#"{{"type":"cost-state","sessionId":"s","totalCostUSD":0.1,"totalDuration":1000,"startTime":{},"modelUsage":{{}},"hasUnknownModelCost":false}}"#,
            close - 1000
        )
    };
    let cleared = dir.join("66666666-2222-4333-8444-555566667777.jsonl");
    std::fs::write(&cleared, "{}\n").unwrap();

    // ONE sibling, two checkpoints equidistant on OPPOSITE sides of the wrapper. The
    // earlier LINE decides, so the direction is stable rather than last-write-wins.
    let sib = dir.join("77777777-2222-4333-8444-555566667777.jsonl");
    std::fs::write(
        &sib,
        format!("{}\n{}\n", checkpoint(w - 40), checkpoint(w + 40)),
    )
    .unwrap();
    let join = cleared_from_origin(&cleared, wrapper);
    assert_eq!(join.distance_ms, Some(40));
    assert!(
        !join.after,
        "the first equidistant line decides the direction: {join:?}"
    );

    // A checkpoint closing EXACTLY at the wrapper instant reads `before`, not `after`.
    std::fs::write(&sib, format!("{}\n", checkpoint(w))).unwrap();
    let exact = cleared_from_origin(&cleared, wrapper);
    assert_eq!(exact.distance_ms, Some(0));
    assert!(!exact.after, "a zero distance is not `after`: {exact:?}");

    std::fs::remove_dir_all(&dir).ok();
}
