//! Record-text resolution for the v0.10.0 promoted leaves + the duration formatter.

use super::*;

fn rec(line: &str) -> Record {
    serde_json::from_str(line).unwrap()
}

#[test]
fn promoted_text_is_verbatim_for_queued_and_recap() {
    let q = rec(
        r#"{"type":"queue-operation","operation":"enqueue","timestamp":"t","sessionId":"s","content":"check the widget build"}"#,
    );
    // Both the promoted path and the shared raw-text entry return the verbatim content
    // (the promoted arm is pinned directly: the D7 system fallback would also read
    // `content`, so only a direct call proves the arm is live).
    assert_eq!(
        promoted_record_text(&q).as_deref(),
        Some("check the widget build")
    );
    assert_eq!(
        record_raw_text(&q, None, None, &ClassifyCtx::top_level()).as_deref(),
        Some("check the widget build")
    );
    let a = rec(
        r#"{"type":"system","subtype":"away_summary","content":"Two checks finished.","uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(
        promoted_record_text(&a).as_deref(),
        Some("Two checks finished.")
    );
    assert_eq!(
        record_raw_text(&a, None, None, &ClassifyCtx::top_level()).as_deref(),
        Some("Two checks finished.")
    );
    // A non-promoted record is untouched by the promoted path.
    let u = rec(r#"{"type":"user","message":{"role":"user","content":"hi"}}"#);
    assert_eq!(promoted_record_text(&u), None);
    assert_eq!(
        record_raw_text(&u, None, None, &ClassifyCtx::top_level()).as_deref(),
        Some("hi")
    );
}

#[test]
fn turn_duration_excerpt_lists_present_fields_only() {
    let full = rec(
        r#"{"type":"system","subtype":"turn_duration","durationMs":64911,"messageCount":908,"pendingBackgroundAgentCount":2,"uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(
        promoted_record_text(&full).as_deref(),
        Some("[turn duration: 1m 5s · durationMs=64911 messageCount=908 pendingBackgroundAgentCount=2]")
    );
    let bare = rec(r#"{"type":"system","subtype":"turn_duration","durationMs":1400,"uuid":"u"}"#);
    assert_eq!(
        promoted_record_text(&bare).as_deref(),
        Some("[turn duration: 1s · durationMs=1400]")
    );
    // No numeric field at all: nothing fabricated.
    let empty = rec(r#"{"type":"system","subtype":"turn_duration","uuid":"u"}"#);
    assert_eq!(promoted_record_text(&empty), None);
    // An absurd span (a turn straddling a resume gap) reads as days, not seconds.
    let long =
        rec(r#"{"type":"system","subtype":"turn_duration","durationMs":926676611,"uuid":"u"}"#);
    assert!(
        promoted_record_text(&long)
            .unwrap()
            .starts_with("[turn duration: 10d 17h ·"),
        "{:?}",
        promoted_record_text(&long)
    );
}

#[test]
fn stop_hooks_excerpt_carries_the_ledger_and_commands() {
    let sh = rec(
        r#"{"type":"system","subtype":"stop_hook_summary","hookCount":2,"hookInfos":[{"command":"/x/a.sh","durationMs":9},{"command":"/x/b.sh"}],"hookErrors":[{"command":"/x/b.sh"}],"preventedContinuation":true,"uuid":"u","timestamp":"t"}"#,
    );
    assert_eq!(
        promoted_record_text(&sh).as_deref(),
        Some("[stop hooks: count=2 errors=1 prevented=true]\n/x/a.sh (9ms)\n/x/b.sh")
    );
    // Missing arrays degrade to zero, never a crash.
    let bare = rec(r#"{"type":"system","subtype":"stop_hook_summary","uuid":"u"}"#);
    assert_eq!(
        promoted_record_text(&bare).as_deref(),
        Some("[stop hooks: count=0 errors=0 prevented=false]")
    );
}

#[test]
fn snapshot_and_delta_excerpts_name_paths_and_versions() {
    let snap = rec(
        r#"{"type":"file-history-snapshot","messageId":"m","snapshot":{"messageId":"m","trackedFileBackups":{"/z/b.md":{"version":1},"/a/f.txt":{"version":3,"backupFileName":"f.txt@v3"}},"timestamp":"2026-06-07T05:00:00.000Z"},"isSnapshotUpdate":true}"#,
    );
    assert_eq!(
        promoted_record_text(&snap).as_deref(),
        Some("[file-history snapshot at 2026-06-07T05:00:00.000Z: /a/f.txt@v3, /z/b.md@v1]")
    );
    let delta = rec(
        r#"{"type":"file-history-delta","messageId":"m","trackingPath":"/a/f.txt","backup":{"backupFileName":"f.txt@v4","version":4},"timestamp":"2026-06-07T05:01:00.000Z"}"#,
    );
    assert_eq!(
        promoted_record_text(&delta).as_deref(),
        Some("[file-history delta at 2026-06-07T05:01:00.000Z: /a/f.txt@v4 backup=f.txt@v4]")
    );
    // A snapshot line with no snapshot object has nothing to render.
    let none = rec(r#"{"type":"file-history-snapshot","messageId":"m"}"#);
    assert_eq!(promoted_record_text(&none), None);
    // A delta with no tracking path likewise.
    let none = rec(r#"{"type":"file-history-delta","messageId":"m","timestamp":"t"}"#);
    assert_eq!(promoted_record_text(&none), None);
}

/// The six `type:"system"` EVENT leaves render `[<subtype> <level>] <content>`, and the
/// renderer that decides it is the SYSTEM FAMILY's, not the promoted one.
///
/// Before the render seam these four shapes were pinned against `promoted_record_text`;
/// the arm moved into `record_text/system.rs` and the assertions moved with it, byte for
/// byte. The last three cases are what the move makes statable: the promoted renderer now
/// answers NOTHING for these leaves, so the render has exactly one owner rather than two,
/// and the family refuses a system subtype whose leaf belongs to another category.
#[test]
fn a_system_event_renders_its_subtype_level_and_content() {
    let ctx = ClassifyCtx::top_level();
    let render = |r: &Record| system::render(r, None, Class::SystemNotice, &ctx);
    // String content with a level: the Remote Control disconnect shape.
    let info = rec(
        r#"{"type":"system","subtype":"informational","level":"warning","uuid":"i","timestamp":"t","content":"Remote Control disconnected - run /remote-control"}"#,
    );
    assert_eq!(
        render(&info).as_deref(),
        Some("[informational warning] Remote Control disconnected - run /remote-control")
    );
    // No level: the head is the bare subtype; a non-string content renders as its JSON.
    let killed = rec(
        r#"{"type":"system","subtype":"agents_killed","uuid":"k","timestamp":"t","content":{"count":2}}"#,
    );
    assert_eq!(
        render(&killed).as_deref(),
        Some(r#"[agents_killed] {"count":2}"#)
    );
    // No content at all: the head alone (still a searchable, addressable record).
    let bare = rec(r#"{"type":"system","subtype":"api_error","uuid":"e","timestamp":"t"}"#);
    assert_eq!(render(&bare).as_deref(), Some("[api_error]"));
    // An empty level string is treated as absent.
    let empty = rec(
        r#"{"type":"system","subtype":"local_command","level":"","uuid":"l","timestamp":"t","content":"ran"}"#,
    );
    assert_eq!(render(&empty).as_deref(), Some("[local_command] ran"));
    assert_eq!(promoted_record_text(&info), None);
    assert_eq!(promoted_record_text(&killed), None);
    let boundary = rec(
        r#"{"type":"system","subtype":"compact_boundary","uuid":"b","timestamp":"t","content":"Conversation compacted"}"#,
    );
    assert_eq!(
        render(&boundary),
        None,
        "a compaction boundary's leaf is in another category - the family refuses it"
    );
}

#[test]
fn fmt_ms_picks_the_top_two_units() {
    assert_eq!(fmt_ms(0), "0s");
    assert_eq!(fmt_ms(499), "0s");
    assert_eq!(fmt_ms(999), "1s");
    assert_eq!(fmt_ms(12_000), "12s");
    assert_eq!(fmt_ms(64_911), "1m 5s");
    assert_eq!(fmt_ms(3_600_000), "1h 0m");
    assert_eq!(fmt_ms(7_380_000), "2h 3m");
    assert_eq!(fmt_ms(926_676_611), "10d 17h");
}

/// C-33: the boundary excerpt gains the harness's own SURVIVOR fields, and a legacy
/// boundary carrying only the four scalars still renders byte-for-byte what it always did.
#[test]
fn boundary_excerpt_keeps_the_legacy_four_scalar_form_byte_for_byte() {
    let legacy = rec(
        r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"t","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preTokens":1000,"postTokens":200,"durationMs":50}}"#,
    );
    assert_eq!(
        system_record_text(&legacy).as_deref(),
        Some(
            "Conversation compacted [compaction boundary: trigger=auto preTokens=1000 \
             postTokens=200 durationMs=50]"
        )
    );
}

#[test]
fn boundary_excerpt_renders_every_survivor_field_when_present() {
    let full = rec(
        r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"t","logicalParentUuid":"lp1","content":"Conversation compacted","compactMetadata":{"trigger":"manual","preTokens":41041,"postTokens":8121,"durationMs":44736,"messagesSummarized":66,"cumulativeDroppedTokens":32920,"preservedSegment":{"headUuid":"aaaa1111-2222-4333-8444-555566667777","anchorUuid":"bbbb2222-3333-4444-8555-666677778888","tailUuid":"cccc3333-4444-4555-8666-777788889999"},"preservedMessages":{"anchorUuid":"bbbb2222-3333-4444-8555-666677778888","uuids":["a","b"],"allUuids":["a","b","c"]}}}"#,
    );
    assert_eq!(
        system_record_text(&full).as_deref(),
        Some(
            "Conversation compacted [compaction boundary: trigger=manual preTokens=41041 \
             postTokens=8121 durationMs=44736 messagesSummarized=66 \
             cumulativeDroppedTokens=32920 preserved=2 uuids, 3 allUuids, anchor bbbb2222 \
             segment=aaaa1111..cccc3333] [logicalParent=lp1]"
        )
    );
}

#[test]
fn boundary_excerpt_prints_only_the_survivor_parts_it_has() {
    // preservedMessages without allUuids, preservedSegment with one end only.
    let partial = rec(
        r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"t","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preservedMessages":{"uuids":["a"]},"preservedSegment":{"headUuid":"aaaaaaaabbbb"}}}"#,
    );
    assert_eq!(
        system_record_text(&partial).as_deref(),
        Some(
            "Conversation compacted [compaction boundary: trigger=auto preserved=1 uuids \
             segment=aaaaaaaa..]"
        )
    );
    // Empty survivor objects contribute nothing (no `preserved=` / `segment=` noise).
    let empty = rec(
        r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"t","content":"Conversation compacted","compactMetadata":{"trigger":"auto","preservedMessages":{},"preservedSegment":{}}}"#,
    );
    assert_eq!(
        system_record_text(&empty).as_deref(),
        Some("Conversation compacted [compaction boundary: trigger=auto]")
    );
    // No compactMetadata at all: the content line stands alone, as before.
    let bare = rec(
        r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"t","content":"Conversation compacted"}"#,
    );
    assert_eq!(
        system_record_text(&bare).as_deref(),
        Some("Conversation compacted")
    );
}
/// The comm class a `tool_use` BLOCK carries, at all three answers it has. The spawn arm
/// is a GUARD, and a guard that always says yes is invisible to a test that only ever
/// asks about spawn tools: a mutation run replaced both `is_spawn_tool_name` and the
/// guard calling it with `true` and nothing failed, so the half of the rule that says
/// every OTHER tool is not a communication had never been asserted. A false positive
/// there relabels an ordinary `Bash` call `agent.communication.sent`.
#[test]
fn only_a_send_or_a_spawn_is_a_communication_tool_use() {
    for spawn in ["Task", "Agent", "Workflow"] {
        assert_eq!(
            tool_use_comm_class(Some(spawn), None),
            Some(Class::CommSent),
            "{spawn}"
        );
    }
    assert_eq!(
        tool_use_comm_class(Some("SendMessage"), None),
        Some(Class::CommSent)
    );
    let signal = serde_json::json!({"message": {"type": "shutdown_request"}});
    assert_eq!(
        tool_use_comm_class(Some("SendMessage"), Some(&signal)),
        Some(Class::CommSignal)
    );
    // The half the mutants walked through, including two names that only LOOK like the
    // spawn set and one that differs from it by case alone.
    for other in ["Bash", "Read", "TaskStop", "AgentOutput", "", "task"] {
        assert_eq!(tool_use_comm_class(Some(other), None), None, "{other:?}");
    }
    assert_eq!(tool_use_comm_class(None, None), None);
}

/// Both overruns that decide an excerpt are STRICT, and the boundary is the whole
/// difference: a body of exactly the budget is complete, so nothing is clipped, the
/// window form is not entered, and the reader is not told that something was elided.
/// Two mutants relaxed the two comparisons and neither was caught - one would have
/// flagged a whole message as truncated, the other would have re-rendered it as a
/// centered fragment and flagged it anyway.
#[test]
fn an_excerpt_exactly_at_the_budget_is_whole_with_or_without_a_span() {
    let max = 40usize;
    let exact = "x".repeat(max);

    let (text, truncated) = match_excerpt(&exact, None, max);
    assert_eq!(text, exact);
    assert!(!truncated, "a body of exactly the budget is whole");

    // WITH a span, the window form must still not be reached at the boundary.
    let (text, truncated) = match_excerpt(&exact, Some((0, 1)), max);
    assert_eq!(text, exact);
    assert!(!truncated, "the window form is for a body that overruns");

    let over = "x".repeat(max + 1);
    let (_, truncated_over) = match_excerpt(&over, None, max);
    assert!(truncated_over, "one char past the budget IS clipped");
}

/// The windowed form keeps a QUARTER of the budget as leading context and counts what
/// follows exactly. Both numbers were unasserted: a mutant that turned the quarter into a
/// remainder put the window at the match itself, dropping the context that makes a hit
/// readable, and a mutant that relaxed the trailing test appended `... (+0 chars)` to a
/// window that had already reached the end - a promise of more text that is not there.
#[test]
fn a_windowed_excerpt_keeps_its_quarter_of_context_and_counts_only_real_remainder() {
    let max = 40usize;
    let text = format!("{}NEEDLE{}", "A".repeat(60), "B".repeat(60));
    let (out, truncated) = match_excerpt(&text, Some((60, 66)), max);
    assert!(truncated);
    // 10 chars of context (max / 4) precede the match, and the elision mark precedes them.
    assert!(
        out.starts_with(&format!("\u{2026}{}NEEDLE", "A".repeat(10))),
        "the window opens a quarter of the budget before the match: {out}"
    );
    // 126 chars total, a 40-char window starting at char 50, so 36 follow.
    assert!(
        out.ends_with("\u{2026} (+36 chars)"),
        "the remainder is counted exactly: {out}"
    );

    // A window that reaches the END promises nothing after it.
    let tail = format!("{}NEEDLE{}", "A".repeat(60), "B".repeat(24));
    let (out, _) = match_excerpt(&tail, Some((60, 66)), max);
    assert!(
        !out.contains("(+0 chars)"),
        "a window at the end must not promise more: {out}"
    );
}

// ── The matchable-text law: a key=value HEAD over the line's verbatim BODY ──

/// A bookkeeping line with a head renders BOTH: the summary a reader scans, then the
/// bytes a pattern searches. The instrument is a key the head elides - a file-history
/// delta's head names the path and the version, so `messageId` exists only in the body,
/// and a render that stopped at the head would make that id unfindable by `search` on
/// any surface.
#[test]
fn a_bookkeeping_line_renders_its_head_over_its_own_bytes() {
    let line = r#"{"type":"file-history-delta","messageId":"msg-88ff","trackingPath":"/a/f.txt","backup":{"backupFileName":"f.txt@v4","version":4},"timestamp":"2026-06-07T05:01:00.000Z"}"#;
    let text = record_raw_text(&rec(line), Some(line), None, &ClassifyCtx::top_level())
        .expect("a bookkeeping line renders");
    let (head, body) = text
        .split_once('\n')
        .expect("head and body, newline between");
    assert_eq!(
        head,
        "[file-history delta at 2026-06-07T05:01:00.000Z: /a/f.txt@v4 backup=f.txt@v4]"
    );
    assert_eq!(body, line, "the body is the line, byte for byte");
    assert!(
        !head.contains("msg-88ff") && body.contains("msg-88ff"),
        "the id the head elides is matchable in the body"
    );
}

/// A line type the family builds a head for from its own BYTES takes the same shape: the
/// key=value summary, then the line. `cost-state` is the instrument because none of its
/// twelve keys is modeled on `Record`, so a head exists here only because the render
/// reads the source line.
#[test]
fn a_raw_read_bookkeeping_head_renders_over_its_own_bytes() {
    let line = r#"{"type":"cost-state","sessionId":"0a1b2c3d-0000-0000-0000-000000000000","totalCostUSD":1.5}"#;
    assert_eq!(
        record_raw_text(&rec(line), Some(line), None, &ClassifyCtx::top_level()).as_deref(),
        Some(format!("[cost-state totalCostUSD=1.5]\n{line}").as_str())
    );
}

/// And with no source line in hand the head still renders. A caller that holds no bytes
/// (the whole-file gate before this lane handed the line through, a fixture) gets the
/// summary rather than nothing, which is the conservative half of the law.
#[test]
fn a_bookkeeping_line_without_its_bytes_renders_the_head_alone() {
    let line = r#"{"type":"file-history-delta","messageId":"m","trackingPath":"/a/f.txt","backup":{"version":4},"timestamp":"2026-06-07T05:01:00.000Z"}"#;
    let text = record_raw_text(&rec(line), None, None, &ClassifyCtx::top_level())
        .expect("the head survives on its own");
    assert!(!text.contains('\n'));
    assert!(text.starts_with("[file-history delta at "));
}

/// The join itself: each half alone is the whole text, and neither half is `None` turned
/// into an empty string.
#[test]
fn head_and_body_keeps_whichever_half_exists() {
    assert_eq!(
        head_and_body(Some("H".into()), Some("B".into())).as_deref(),
        Some("H\nB")
    );
    assert_eq!(head_and_body(Some("H".into()), None).as_deref(), Some("H"));
    assert_eq!(head_and_body(None, Some("B".into())).as_deref(), Some("B"));
    assert_eq!(head_and_body(None, None), None);
    // `verbatim_body` is the line and nothing else - no trim, no re-encode.
    assert_eq!(
        verbatim_body(Some("  {\"a\":1} ")).as_deref(),
        Some("  {\"a\":1} ")
    );
    assert_eq!(verbatim_body(None), None);
}
