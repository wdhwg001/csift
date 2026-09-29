//! What the candidate GATES admit, end to end: one fixture line per mechanically mapped
//! shape, censused by leaf, plus the unmodeled unit an address renders for a shape csift
//! models no leaf for.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "5e4d3c2b-1a09-4f8e-b7d6-c5b4a3928170";

/// A transcript carrying ONE line per mechanically mapped shape: the 18 bookkeeping line
/// types, the 11 system subtypes, one attachment payload per context leaf, and the four
/// thinking-block shapes. Built from the on-disk shapes the inventory names, never
/// copied from a corpus.
fn shapes_home() -> Home {
    let mut lines: Vec<String> = vec![
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzshape open the survey"}}"#.to_string(),
    ];
    // -- the 18 non-transcript line types --
    for ty in [
        "last-prompt",
        "mode",
        "permission-mode",
        "ai-title",
        "agent-name",
        "atis-latch",
        "bridge-session",
        "history-suppression",
        "pr-link",
        "frame-link",
        "artifact-comment-monitor",
        "artifact-autoreact-ledger",
        "cost-state",
        "fork-context-ref",
        "continued-in",
        "file-history-snapshot",
        "file-history-delta",
    ] {
        lines.push(format!(
            r#"{{"type":"{ty}","sessionId":"{SESS}","note":"zzshape"}}"#
        ));
    }
    // A queue line with the HUMAN's own content, and one without.
    lines.push(format!(
        r#"{{"type":"queue-operation","operation":"enqueue","sessionId":"{SESS}","content":"zzshape queued by hand"}}"#
    ));
    lines.push(format!(
        r#"{{"type":"queue-operation","operation":"dequeue","sessionId":"{SESS}"}}"#
    ));
    // -- the 11 system subtypes --
    for (i, sub) in [
        "compact_boundary",
        "turn_duration",
        "stop_hook_summary",
        "away_summary",
        "scheduled_task_fire",
        "model_refusal_fallback",
        "model_refusal_no_fallback",
        "api_error",
        "informational",
        "agents_killed",
        "local_command",
    ]
    .iter()
    .enumerate()
    {
        lines.push(format!(
            r#"{{"type":"system","subtype":"{sub}","uuid":"s{i}","timestamp":"2026-06-07T05:00:0{}.000Z","content":"zzshape {sub}"}}"#,
            i % 10
        ));
    }
    // -- one attachment payload per context leaf, plus the hook and structured ones --
    for (i, ty) in [
        "hook_additional_context",
        "hook_success",
        "structured_output",
        "task_reminder",
        "skill_listing",
        "instructions",
        "edited_text_file",
        "plan_mode",
        "diagnostics",
        "task_status",
    ]
    .iter()
    .enumerate()
    {
        lines.push(format!(
            r#"{{"type":"attachment","uuid":"a{i}","parentUuid":"u1","timestamp":"2026-06-07T05:01:0{}.000Z","attachment":{{"type":"{ty}","content":["zzshape {ty}"]}}}}"#,
            i % 10
        ));
    }
    // -- the four thinking-block shapes, one assistant record each --
    for (i, block) in [
        r#"{"type":"thinking","thinking":"zzshape weigh it","signature":"sig"}"#,
        r#"{"type":"thinking","thinking":"","signature":"sig"}"#,
        r#"{"type":"redacted_thinking","data":"T3BhcXVl"}"#,
    ]
    .iter()
    .enumerate()
    {
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"t{i}","parentUuid":"u1","timestamp":"2026-06-07T05:02:0{}.000Z","message":{{"role":"assistant","content":[{block}]}}}}"#,
            i % 10
        ));
    }
    // A shape csift models no leaf for at all.
    lines.push(format!(
        r#"{{"type":"future-thing","sessionId":"{SESS}","zzshapeKey":"zzshape value","count":7}}"#
    ));
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    h
}

/// The leaf keys a `--count-by label` census reports for `argv`.
fn census_keys(h: &Home, extra: &[&str]) -> Vec<String> {
    let mut argv: Vec<&str> = vec!["search", "", ""];
    let target = at(SESS);
    argv[2] = &target;
    argv.extend_from_slice(extra);
    argv.extend_from_slice(&["--count-by", "label", "--format", "json"]);
    let out = h.run(&argv);
    assert!(out.success, "stderr: {}", out.stderr);
    out.stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "census")
        .filter_map(|v| v["key"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn a_default_census_reaches_none_of_the_gated_shapes() {
    let h = shapes_home();
    let keys = census_keys(&h, &[]);
    for prefix in [
        "harness.bookkeeping.",
        "harness.turn.",
        "harness.system.",
        "harness.context.",
        "harness.hook.",
        "user.queued",
        "harness.schedule.event",
    ] {
        assert!(
            !keys.iter().any(|k| k.starts_with(prefix)),
            "a flagless scan must not parse {prefix}: {keys:?}"
        );
    }
    // The conversation is all it sees.
    assert!(keys.contains(&"user.message".to_string()), "{keys:?}");
    assert!(
        keys.contains(&"agent.thinking.summary".to_string()),
        "{keys:?}"
    );
}

#[test]
fn every_bookkeeping_leaf_is_reachable_by_its_own_selector() {
    let h = shapes_home();
    let keys = census_keys(&h, &["-t", "harness.bookkeeping.*"]);
    for leaf in [
        "harness.bookkeeping.last-prompt",
        "harness.bookkeeping.mode",
        "harness.bookkeeping.name",
        "harness.bookkeeping.atis",
        "harness.bookkeeping.bridge",
        "harness.bookkeeping.suppression",
        "harness.bookkeeping.pr-link",
        "harness.bookkeeping.artifact",
        "harness.bookkeeping.file-history",
        "harness.bookkeeping.cost",
        "harness.bookkeeping.fork",
        "harness.bookkeeping.continued",
        "harness.bookkeeping.queue",
    ] {
        assert!(keys.contains(&leaf.to_string()), "missing {leaf}: {keys:?}");
    }
    assert_eq!(
        keys.iter()
            .filter(|k| k.starts_with("harness.bookkeeping."))
            .count(),
        13,
        "all 13 leaves and nothing else: {keys:?}"
    );
    // Naming ONE leaf admits only that line kind - the per-leaf gate, not a family switch.
    let one = census_keys(&h, &["-t", "harness.bookkeeping.cost"]);
    assert_eq!(one, vec!["harness.bookkeeping.cost".to_string()], "{one:?}");
}

#[test]
fn every_system_and_turn_leaf_is_reachable_by_its_own_selector() {
    let h = shapes_home();
    let keys = census_keys(&h, &["-t", "harness.system.*", "-t", "harness.turn.*"]);
    for leaf in [
        "harness.system.refusal",
        "harness.system.api-error",
        "harness.system.notice",
        "harness.system.kill",
        "harness.system.command",
        "harness.turn.duration",
        "harness.turn.stop-hooks",
        "harness.turn.recap",
    ] {
        assert!(keys.contains(&leaf.to_string()), "missing {leaf}: {keys:?}");
    }
    // The scheduler's fire RECORD is its own leaf, not a system one.
    let fire = census_keys(&h, &["-t", "harness.schedule.event"]);
    assert_eq!(fire, vec!["harness.schedule.event".to_string()], "{fire:?}");
}

#[test]
fn every_attachment_and_thinking_leaf_is_reachable_by_its_own_selector() {
    let h = shapes_home();
    let keys = census_keys(&h, &["-t", "harness.context.*", "-t", "harness.hook.*"]);
    for leaf in [
        "harness.context.reminder",
        "harness.context.listing",
        "harness.context.prompt",
        "harness.context.file",
        "harness.context.mode",
        "harness.context.ide",
        "harness.context.task",
        "harness.hook.context",
        "harness.hook.outcome",
    ] {
        assert!(keys.contains(&leaf.to_string()), "missing {leaf}: {keys:?}");
    }
    let structured = census_keys(&h, &["-t", "agent.message.structured"]);
    assert_eq!(
        structured,
        vec!["agent.message.structured".to_string()],
        "{structured:?}"
    );
    // The thinking family rides role-bearing lines, so a default scan already has it.
    let thinking = census_keys(&h, &["-t", "agent.thinking.*"]);
    for leaf in [
        "agent.thinking.summary",
        "agent.thinking.omitted",
        "agent.thinking.redacted",
    ] {
        assert!(
            thinking.contains(&leaf.to_string()),
            "missing {leaf}: {thinking:?}"
        );
    }
}

#[test]
fn an_address_renders_an_unmodeled_line_instead_of_missing_it() {
    let h = shapes_home();
    // The last line is a shape csift models no leaf for. A SCAN never sees it.
    let scan = h.run(&["search", "zzshape value", &at(SESS)]);
    assert!(scan.success, "stderr: {}", scan.stderr);
    assert!(
        scan.stdout.contains("no matching exchanges"),
        "a scan never emits an unmodeled unit:\n{}",
        scan.stdout
    );
    // An ADDRESS renders it, names the shape, and shows the line's own keys.
    // The unmodeled line is the LAST one the fixture writes.
    let n = std::fs::read_to_string(
        h.root
            .join(".claude/projects")
            .join(format!("{ENC}/{SESS}.jsonl")),
    )
    .expect("the fixture")
    .lines()
    .position(|l| l.contains("future-thing"))
    .expect("the unmodeled line")
        + 1;
    let show = h.run(&["show", &at(SESS), "--line", &n.to_string()]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout.contains("? unmodeled: future-thing"),
        "the unit names the shape:\n{}",
        show.stdout
    );
    assert!(
        show.stdout.contains("zzshapeKey=zzshape value") && show.stdout.contains("count=7"),
        "and renders the line's own top-level keys:\n{}",
        show.stdout
    );
    // JSON: a null label, an empty label set, and the shape under its own key.
    let js = h.run(&[
        "show",
        &at(SESS),
        "--line",
        &n.to_string(),
        "--format",
        "json",
    ]);
    let row = js
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["kind"] == "record")
        .expect("the record row");
    assert_eq!(
        row["label"],
        serde_json::Value::Null,
        "{js:?}",
        js = js.stdout
    );
    assert_eq!(row["labels"], serde_json::json!([]), "{}", js.stdout);
    assert_eq!(row["unmodeled"], "future-thing", "{}", js.stdout);
}

#[test]
fn an_unmodeled_system_subtype_names_the_subtype_not_the_type() {
    let h = Home::new();
    let sess = "4d3c2b1a-0f9e-4d8c-b7a6-95847362510f";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzsub open"}}"#, "\n",
            r#"{"type":"system","subtype":"some_future_subtype","uuid":"s1","timestamp":"2026-06-07T05:00:01.000Z","content":"zzsub body"}"#, "\n",
        ),
    );
    let show = h.run(&["show", &at(sess), "--line", "2"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout
            .contains("? unmodeled: system/some_future_subtype"),
        "the deeper axis is what csift did not model:\n{}",
        show.stdout
    );
}

#[test]
fn an_unmodeled_attachment_payload_names_the_payload_type() {
    // The third axis the unmodeled unit splits on. An attachment line carries its kind in
    // a payload `type`, so a payload csift has no row for must name THAT value - naming
    // `attachment` alone would point a reader at a line type csift models perfectly well.
    let h = Home::new();
    let sess = "5e4d3c2b-1a0f-49e8-97d6-a5b4c3d2e1f0";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"zzatt open"}}"#, "\n",
            r#"{"type":"attachment","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T06:00:01.000Z","attachment":{"type":"some_future_payload","content":"zzatt body"}}"#, "\n",
        ),
    );
    let show = h.run(&["show", &at(sess), "--line", "2"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout
            .contains("? unmodeled: attachment/some_future_payload"),
        "the payload type is what csift did not model:\n{}",
        show.stdout
    );
    let js = h.run(&["show", &at(sess), "--line", "2", "--format", "json"]);
    assert!(js.success, "stderr: {}", js.stderr);
    let row: serde_json::Value = js
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["kind"] == "record")
        .expect("the record row");
    assert_eq!(row["label"], serde_json::Value::Null, "{}", js.stdout);
    assert_eq!(
        row["unmodeled"], "attachment/some_future_payload",
        "{}",
        js.stdout
    );
}

#[test]
fn a_view_leaf_reaches_its_records_without_opening_the_whole_attachment_gate() {
    // Two leaves an ATTACHMENT carries that an ordinary query names: the human's mid-turn
    // steer (`user.message` through a `queued_command` payload whose origin says human)
    // and the model's structured answer. Each has its own payload needle, so naming it
    // must reach its own records and NOT admit every attachment line in the file - which
    // is what a `-t user` keyed on the wide attachment gate does, at four times the cost
    // of a flagless scan.
    let h = shapes_home();
    let user = census_keys(&h, &["-t", "user.message"]);
    assert!(
        user.contains(&"user.message".to_string()),
        "the human's queued steer is reachable by its own leaf: {user:?}"
    );
    for absent in [
        "harness.context.",
        "harness.hook.",
        "agent.message.structured",
    ] {
        assert!(
            !user.iter().any(|k| k.starts_with(absent)),
            "-t user.message must not admit other attachment leaves ({absent}): {user:?}"
        );
    }
    let structured = census_keys(&h, &["-t", "agent.message.structured"]);
    assert_eq!(
        structured,
        vec!["agent.message.structured".to_string()],
        "and the structured answer reaches its own records alone"
    );
    // A FLAGLESS scan parses the two VIEW lines and no other attachment line. Both view
    // leaves are words someone OWNED - the person's own mid-turn steer and the model's own
    // structured answer - and a flagless scan is a superset of the `-t user` / `-t agent`
    // queries that reach them, so it cannot miss what they find. Every other attachment
    // leaf stays behind its gate, which is what keeps the coarse keep off.
    let flagless = census_keys(&h, &[]);
    assert!(
        flagless.iter().any(|k| k == "agent.message.structured"),
        "a flagless scan reaches the model's own structured answer: {flagless:?}"
    );
    for absent in ["harness.context.", "harness.hook."] {
        assert!(
            !flagless.iter().any(|k| k.starts_with(absent)),
            "and no OTHER attachment leaf ({absent}): {flagless:?}"
        );
    }
}

/// A DEFAULT-ON needle is a byte substring, so a non-transcript line that merely QUOTES one
/// is admitted by it - a queue rider carrying a `<task-notification>` that mentions the
/// channel envelope, the person's own queued text mentioning it, a `last-prompt` line whose
/// prompt does. Each such line's leaf is still gated: a flagless scan emits none of them,
/// and the selector naming each leaf emits exactly its own line.
#[test]
fn a_default_needle_quoted_in_a_non_transcript_line_opens_no_gate() {
    let h = Home::new();
    let lines = [
        format!(
            r#"{{"type":"user","uuid":"q1","timestamp":"2026-06-07T05:00:00.000Z","sessionId":"{SESS}","message":{{"role":"user","content":"zzquote kick off"}}}}"#
        ),
        format!(
            r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:00:01.000Z","sessionId":"{SESS}","content":"<task-notification>\n<task-id>bzzq00001</task-id>\n<status>completed</status>\n<summary>zzquote a pulse that names [csift-channel v1 id=abc]</summary>\n</task-notification>"}}"#
        ),
        format!(
            r#"{{"type":"queue-operation","operation":"enqueue","timestamp":"2026-06-07T05:00:02.000Z","sessionId":"{SESS}","content":"zzquote my own words about csift-channel v1"}}"#
        ),
        format!(
            r#"{{"type":"last-prompt","sessionId":"{SESS}","lastPrompt":"zzquote read the csift-channel v1 header","leafUuid":"q1"}}"#
        ),
    ];
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    let flagless = census_keys(&h, &[]);
    assert!(
        !flagless
            .iter()
            .any(|k| k.starts_with("harness.bookkeeping.") || k == "user.queued"),
        "a quoted needle opened a gated leaf on a flagless scan: {flagless:?}"
    );
    assert!(
        flagless.contains(&"user.message".to_string()),
        "{flagless:?}"
    );
    // The fixture holds ONE line per leaf, so naming a leaf and finding it is the count.
    for leaf in [
        "harness.bookkeeping.queue",
        "user.queued",
        "harness.bookkeeping.last-prompt",
    ] {
        let keys = census_keys(&h, &["-t", leaf]);
        assert_eq!(
            keys,
            vec![leaf.to_string()],
            "`-t {leaf}` emits its own line"
        );
    }
}

/// A `last-prompt` line a quoted needle admitted still DECIDES the conversation's leaf once
/// its leaf is found unreached: it leaves the record set as the spine row the byte walk
/// would have lifted for it, so the branch it names stays the live one. Here the recorded
/// leaf is the first of two sibling prompts; the second, later in the file, is the draft.
#[test]
fn a_demoted_last_prompt_line_still_names_the_live_branch() {
    let h = Home::new();
    let lines = [
        format!(
            r#"{{"type":"user","uuid":"v1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","sessionId":"{SESS}","message":{{"role":"user","content":"zzleaf start"}}}}"#
        ),
        format!(
            r#"{{"type":"assistant","uuid":"v2","parentUuid":"v1","timestamp":"2026-06-07T05:00:01.000Z","sessionId":"{SESS}","message":{{"id":"msg_v2","role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"zzleaf reply"}}]}}}}"#
        ),
        format!(
            r#"{{"type":"user","uuid":"v3","parentUuid":"v2","timestamp":"2026-06-07T05:00:02.000Z","sessionId":"{SESS}","message":{{"role":"user","content":"zzleaf kept branch"}}}}"#
        ),
        format!(
            r#"{{"type":"user","uuid":"v4","parentUuid":"v2","timestamp":"2026-06-07T05:00:03.000Z","sessionId":"{SESS}","message":{{"role":"user","content":"zzleaf other branch"}}}}"#
        ),
        format!(
            r#"{{"type":"last-prompt","sessionId":"{SESS}","lastPrompt":"zzleaf about csift-channel v1","leafUuid":"v3"}}"#
        ),
    ];
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    let out = h.run(&["search", "zzleaf", &at(SESS), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let survival = |needle: &str| {
        hits.iter()
            .find(|h| h["excerpt"].as_str().is_some_and(|e| e.contains(needle)))
            .map(|h| h["survival"].as_str().unwrap_or("").to_string())
            .unwrap_or_else(|| panic!("a hit for {needle}: {}", out.stdout))
    };
    assert_eq!(survival("kept branch"), "live", "{}", out.stdout);
    assert_ne!(survival("other branch"), "live", "{}", out.stdout);
}
