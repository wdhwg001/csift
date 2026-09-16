//! The `harness.bookkeeping` family end to end: one fixture line per non-transcript line
//! type, the census keys it produces, the head each one renders, the provenance relations,
//! and the matchability the head-plus-verbatim-body law buys.
//!
//! Every line below is built from the shape the inventory names, never copied from a
//! transcript, and every expected string is derived from the family's documented rules.

use crate::harness::*;

const ENC: &str = "-Users-dev-bookkeeping";
const SESS: &str = "11111111-2222-4333-8444-555555555555";
const LEAF: &str = "aaaaaaaa-0000-4000-8000-000000000001";
/// A record uuid NO line of this transcript carries - the fork edge's target, which is a
/// record in the parent's file.
const CUT: &str = "88888888-0000-4000-8000-000000000008";

/// One transcript: a human turn, then one line of each of the eighteen non-transcript
/// types, then the three `queue-operation` shapes (the person's own text, a rider carrying
/// a reason, a content-less dequeue).
fn family_home() -> Home {
    let h = Home::new();
    let lines = [
        format!(
            r#"{{"type":"user","uuid":"{LEAF}","parentUuid":null,"timestamp":"2026-01-01T00:00:00.000Z","sessionId":"{SESS}","message":{{"role":"user","content":"zzbook open the survey"}}}}"#
        ),
        format!(
            r#"{{"type":"last-prompt","sessionId":"{SESS}","leafUuid":"{LEAF}","explicit":true,"lastPrompt":"zzbook open the survey"}}"#
        ),
        format!(r#"{{"type":"mode","sessionId":"{SESS}","mode":"default"}}"#),
        format!(
            r#"{{"type":"permission-mode","sessionId":"{SESS}","permissionMode":"acceptEdits"}}"#
        ),
        format!(r#"{{"type":"ai-title","sessionId":"{SESS}","aiTitle":"Bookkeeping renders"}}"#),
        format!(r#"{{"type":"agent-name","sessionId":"{SESS}","agentName":"Sparrow"}}"#),
        format!(r#"{{"type":"atis-latch","sessionId":"{SESS}","atis":"latched-value"}}"#),
        format!(
            r#"{{"type":"bridge-session","sessionId":"{SESS}","bridgeSessionId":"bbbbbbbb-0000-4000-8000-000000000002","lastSequenceNum":7,"noHistoryBackfill":false,"ownerAccountUuid":"cccccccc-0000-4000-8000-000000000003"}}"#
        ),
        format!(
            r#"{{"type":"history-suppression","sessionId":"{SESS}","cause":"restored_owner_mismatch","ts":"2026-01-01T00:00:01.000Z","vetoedAgainstAccountUuid":"dddddddd-0000-4000-8000-000000000004"}}"#
        ),
        format!(
            r#"{{"type":"pr-link","sessionId":"{SESS}","timestamp":"2026-01-01T00:00:02.000Z","prNumber":42,"prRepository":"dev/proj"}}"#
        ),
        format!(
            r#"{{"type":"frame-link","sessionId":"{SESS}","timestamp":"2026-01-01T00:00:03.000Z","artifactCount":2,"title":"Frame"}}"#
        ),
        format!(
            r#"{{"type":"artifact-comment-monitor","sessionId":"{SESS}","v":1,"accountUuid":"eeeeeeee-0000-4000-8000-000000000005","artifacts":{{"slug-two":{{"seen":2}},"slug-one":{{"seen":1}}}}}}"#
        ),
        format!(
            r#"{{"type":"artifact-autoreact-ledger","sessionId":"{SESS}","v":1,"artifacts":{{"slug-three":{{"reacted":true}}}}}}"#
        ),
        format!(
            r#"{{"type":"file-history-snapshot","sessionId":"{SESS}","messageId":"{LEAF}","snapshot":{{"messageId":"{LEAF}","trackedFileBackups":{{"/Users/dev/proj/a.rs":{{"backupFileName":"a.rs@v3","version":3}}}},"timestamp":"2026-01-01T00:00:04.000Z"}},"isSnapshotUpdate":false}}"#
        ),
        format!(
            r#"{{"type":"file-history-delta","sessionId":"{SESS}","messageId":"{LEAF}","trackingPath":"/Users/dev/proj/a.rs","backup":{{"backupFileName":"a.rs@v4","version":4}},"timestamp":"2026-01-01T00:00:05.000Z"}}"#
        ),
        format!(
            r#"{{"type":"cost-state","sessionId":"{SESS}","totalCostUSD":1.5,"totalDuration":3600000,"startTime":"2026-01-01T00:00:00.000Z","modelUsage":{{"probe":{{"in":10}}}},"hasUnknownModelCost":false}}"#
        ),
        format!(
            r#"{{"type":"fork-context-ref","sessionId":"{SESS}","agentId":"a1b2c3d4e5f60718","parentSessionId":"99999999-8888-4777-8666-555555555555","parentLastUuid":"{CUT}","contextLength":1200}}"#
        ),
        format!(
            r#"{{"type":"continued-in","sessionId":"{SESS}","timestamp":"2026-01-01T00:00:06.000Z","continuedInSessionId":"77777777-6666-4555-8444-333333333333"}}"#
        ),
        format!(
            r#"{{"type":"queue-operation","sessionId":"{SESS}","operation":"enqueue","timestamp":"2026-01-01T00:00:07.000Z","content":"zzbook please continue"}}"#
        ),
        format!(
            r#"{{"type":"queue-operation","sessionId":"{SESS}","operation":"remove","reason":"absorbed_mid_turn","timestamp":"2026-01-01T00:00:08.000Z","content":"<task-notification><task-id>b12345678</task-id><status>completed</status>done</task-notification>"}}"#
        ),
        format!(
            r#"{{"type":"queue-operation","sessionId":"{SESS}","operation":"dequeue","timestamp":"2026-01-01T00:00:09.000Z"}}"#
        ),
    ];
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    h
}

/// `-t harness.bookkeeping.*` reaches all THIRTEEN leaves, and the per-leaf counts follow
/// the line types the fixture carries: three artifact ledgers, two file-history forms, two
/// mode writers, two name writers, two queue RIDERS (the person's own enqueue is
/// `user.queued`), and one each for the rest. Nineteen records over twenty non-transcript
/// lines, because one of them is the human's.
#[test]
fn every_bookkeeping_leaf_is_reachable_and_counted() {
    let h = family_home();
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.bookkeeping.*",
        "--count-by",
        "label",
    ]);
    assert!(out.success, "stderr {}", out.stderr);
    for (leaf, n) in [
        ("harness.bookkeeping.artifact", 3),
        ("harness.bookkeeping.file-history", 2),
        ("harness.bookkeeping.mode", 2),
        ("harness.bookkeeping.name", 2),
        ("harness.bookkeeping.queue", 2),
        ("harness.bookkeeping.atis", 1),
        ("harness.bookkeeping.bridge", 1),
        ("harness.bookkeeping.continued", 1),
        ("harness.bookkeeping.cost", 1),
        ("harness.bookkeeping.fork", 1),
        ("harness.bookkeeping.last-prompt", 1),
        ("harness.bookkeeping.pr-link", 1),
        ("harness.bookkeeping.suppression", 1),
    ] {
        assert!(
            out.stdout
                .lines()
                .any(|l| l.trim().starts_with(&format!("{n}  ")) && l.ends_with(leaf)),
            "{leaf} should count {n}:\n{}",
            out.stdout
        );
    }
    assert!(
        out.stderr
            .contains("19 matched record(s) across 13 label key(s)"),
        "stderr {}",
        out.stderr
    );
}

/// `show --line` renders the head of every line type, each opening with its OWN `type`, so
/// the pairs that share a leaf stay apart. SIXTEEN of the eighteen build that head from
/// their own bytes; the two file-history forms keep the head built from the fields csift
/// models. The human's own queued text stays `user.queued` and renders its content, not a
/// head.
#[test]
fn show_renders_one_head_per_line_type() {
    let h = family_home();
    let out = h.run(&["show", &at(SESS), "--line", "2..21"]);
    assert!(out.success, "stderr {}", out.stderr);
    for expect in [
        // Read off the source line, in the writer's own key order.
        "[last-prompt lastPrompt=zzbook open the survey leafUuid=aaaaaaaa explicit=true]",
        "[mode mode=default]",
        "[permission-mode permissionMode=acceptEdits]",
        "[ai-title aiTitle=Bookkeeping renders]",
        "[agent-name agentName=Sparrow]",
        "[atis-latch atis=latched-value]",
        "[bridge-session bridgeSessionId=bbbbbbbb-0000-4000-8000-000000000002 lastSequenceNum=7 noHistoryBackfill=false ownerAccountUuid=cccccccc]",
        "[history-suppression cause=restored_owner_mismatch vetoedAgainstAccountUuid=dddddddd ts=2026-01-01T00:00:01.000Z]",
        "[pr-link prNumber=42 prRepository=dev/proj]",
        "[frame-link title=Frame artifactCount=2]",
        "[artifact-comment-monitor v=1 accountUuid=eeeeeeee artifacts=slug-one, slug-two]",
        "[artifact-autoreact-ledger v=1 artifacts=slug-three]",
        "[cost-state totalCostUSD=1.5 totalDuration=3600000 startTime=2026-01-01T00:00:00.000Z]",
        "[fork-context-ref agentId=a1b2c3d4e5f60718 parentSessionId=99999999-8888-4777-8666-555555555555 parentLastUuid=88888888 contextLength=1200]",
        "[continued-in continuedInSessionId=77777777-6666-4555-8444-333333333333]",
        "[queue-operation operation=remove reason=absorbed_mid_turn]",
        "[queue-operation operation=dequeue]",
        // The two that keep the record-modeled head: it names the tracked PATH and its
        // version, which a raw key walk could only print as the nested snapshot object.
        "[file-history snapshot at 2026-01-01T00:00:04.000Z: /Users/dev/proj/a.rs@v3]",
        "[file-history delta at 2026-01-01T00:00:05.000Z: /Users/dev/proj/a.rs@v4 backup=a.rs@v4]",
    ] {
        assert!(
            out.stdout.contains(expect),
            "missing {expect}:\n{}",
            out.stdout
        );
    }
    // The person's own queued text is the human's voice under `user.queued`, and it
    // renders the content rather than a bookkeeping head.
    assert!(
        out.stdout.contains("◂ user.queued [enqueue]")
            && out.stdout.contains("zzbook please continue"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("[queue-operation operation=enqueue"),
        "the human's line never renders the family head:\n{}",
        out.stdout
    );
}

/// The head is a SUMMARY and the body is the line, so a key the head elides and a uuid it
/// shortens are both still matchable - and so is a key=value pair only the head spells.
/// This is the law's whole point: a render that stopped at the head would make csift
/// report a false absence.
#[test]
fn a_head_never_costs_the_body_its_matchability() {
    let h = family_home();
    let count = |pat: &str| {
        let out = h.run(&[
            "search",
            pat,
            &at(SESS),
            "-t",
            "harness.bookkeeping.*",
            "-c",
        ]);
        assert!(out.success, "{pat}: stderr {}", out.stderr);
        out.stdout.trim().to_string()
    };
    // In the BODY only (the head elides or shortens them).
    assert_eq!(
        count("cccccccc-0000-4000-8000-000000000003"),
        "1",
        "a shortened account uuid"
    );
    assert_eq!(count("modelUsage"), "1", "a key no head lists");
    assert_eq!(
        count("hasUnknownModelCost"),
        "1",
        "another key no head lists"
    );
    // In the HEAD only (the raw line spells these as `\"key\":value`).
    assert_eq!(
        count("totalCostUSD=1.5"),
        "1",
        "a fabricated key=value pair"
    );
    assert_eq!(
        count("artifacts=slug-one"),
        "1",
        "an object's sorted key list"
    );
    assert_eq!(count("cause=restored_owner_mismatch"), "1", "a cause pair");
}

/// The five provenance relations. Three name a RECORD and ride the shared `for` edge: a
/// uuid this transcript carries resolves to its LINE, and one it does not keeps the id
/// with the session it lives in. The other two name no record and are the family's own
/// facts - the child transcript, and the `/clear` join key `startTime + totalDuration`.
#[test]
fn the_five_provenance_relations_render() {
    let h = family_home();
    let text = h.run(&["show", &at(SESS), "--line", "2..21"]);
    assert!(text.success, "stderr {}", text.stderr);
    for expect in [
        "[for: leaf L1]",
        "[for: versions L1]",
        "[for: forked_from @99999999-8888-4777-8666-555555555555 88888888 (not in this file)]",
        "[for: continued_in @77777777-6666-4555-8444-333333333333]",
        "[checkpoint closes 2026-01-01T01:00:00Z]",
    ] {
        assert!(
            text.stdout.contains(expect),
            "missing {expect}:\n{}",
            text.stdout
        );
    }
}

/// The same relations on the machine surface. The edge is an ARRAY under one key, so
/// several on one unit accumulate; the family's own two are plain keys.
#[test]
fn the_provenance_relations_ride_the_json_row() {
    let h = family_home();
    let row = |line: &str| {
        let out = h.run(&["show", &at(SESS), "--line", line, "--format", "json"]);
        assert!(out.success, "stderr {}", out.stderr);
        let rows = json_rows(&out.stdout, "record");
        assert_eq!(rows.len(), 1, "{}", out.stdout);
        rows[0].clone()
    };
    // The fork edge: a target in ANOTHER file keeps its uuid AND names the session.
    let fork = row("17");
    let edge = &fork["for"][0];
    assert_eq!(edge["kind"], "forked_from");
    assert_eq!(edge["basis"], "key");
    assert_eq!(edge["target"]["uuid"], CUT);
    assert_eq!(
        edge["target"]["session"],
        "99999999-8888-4777-8666-555555555555"
    );
    assert_eq!(edge["target"]["line"], serde_json::Value::Null);
    assert_eq!(edge["target"]["note"], "not in this file");
    // The leaf edge: a uuid this transcript carries resolves to its line.
    let leaf = row("2");
    assert_eq!(leaf["for"][0]["kind"], "leaf");
    assert_eq!(leaf["for"][0]["target"]["line"], 1);
    assert_eq!(leaf["for"][0]["target"]["uuid"], LEAF);
    // The handover edge: a target that is a whole TRANSCRIPT and no record in it, so the
    // session stands alone under `target` with no uuid and no line beside it.
    let cont = row("18");
    let edge = &cont["for"][0];
    assert_eq!(edge["kind"], "continued_in");
    assert_eq!(edge["basis"], "key");
    assert_eq!(
        edge["target"]["session"],
        "77777777-6666-4555-8444-333333333333"
    );
    assert_eq!(edge["target"]["uuid"], serde_json::Value::Null);
    assert_eq!(edge["target"]["line"], serde_json::Value::Null);
    assert_eq!(edge["reason"], serde_json::Value::Null);
    // The family's own one.
    let cost = row("16");
    assert_eq!(cost["checkpoint_closes_at"], "2026-01-01T01:00:00Z");
    assert_eq!(
        cost["checkpoint_join"],
        "startTime+totalDuration; list joins it as cleared_from"
    );
}

/// Every unit of the family says `[not delivered]` in the label zone, and the JSON
/// `delivered` field says the same thing - the two surfaces agreed on nothing before.
/// The marker is the ENGINE's, so a record the request assembler itself drops spells it
/// the same way: a hook-outcome attachment carries a per-record delivery override, and
/// both render the identical word.
#[test]
fn the_family_is_marked_undelivered_in_the_engine_s_own_word() {
    let h = family_home();
    let text = h.run(&["show", &at(SESS), "--line", "3"]);
    assert!(
        text.stdout
            .contains("harness.bookkeeping.mode [not delivered]"),
        "{}",
        text.stdout
    );
    let json = h.run(&[
        "search",
        "default",
        &at(SESS),
        "-t",
        "harness.bookkeeping.mode",
        "--format",
        "json",
    ]);
    let rows = json_rows(&json.stdout, "exchange");
    let hit = &rows[0]["hits"][0];
    assert_eq!(hit["delivered"], false, "{}", json.stdout);

    // The engine's own spelling, on a record whose `delivery_override` is what makes it
    // undelivered rather than its leaf: the two code paths must print one word.
    let o = Home::new();
    let other = "22222222-3333-4444-8555-666666666666";
    o.write(
        &format!("{ENC}/{other}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"o1","timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"zzother go"}}"#,
            "\n",
            r#"{"type":"attachment","uuid":"o2","parentUuid":"o1","timestamp":"2026-01-01T00:00:01.000Z","attachment":{"type":"hook_success","hookName":"zzhook","command":"true"}}"#,
            "\n",
        ),
    );
    let hook = o.run(&["show", &at(other), "--line", "2"]);
    assert!(
        hook.stdout.contains("[not delivered]"),
        "the engine's own marker spells it the same way:\n{}",
        hook.stdout
    );
}

/// The honest empties. A line type that HAS a relation but not the key it reads renders
/// the relation with its reason rather than dropping the field, on both halves of the
/// split: the shared edge, and the family's own facts.
#[test]
fn a_missing_key_renders_the_relation_as_an_honest_empty() {
    let h = Home::new();
    let sess = "33333333-4444-4555-8666-777777777777";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"e1","timestamp":"2026-01-01T00:00:00.000Z","message":{"role":"user","content":"zzempty go"}}"#,
            "\n",
            r#"{"type":"last-prompt","sessionId":"33333333-4444-4555-8666-777777777777","lastPrompt":"zzempty go"}"#,
            "\n",
            r#"{"type":"continued-in","sessionId":"33333333-4444-4555-8666-777777777777","timestamp":"2026-01-01T00:00:06.000Z"}"#,
            "\n",
            r#"{"type":"cost-state","sessionId":"33333333-4444-4555-8666-777777777777","totalCostUSD":1.5}"#,
            "\n",
        ),
    );
    let out = h.run(&["show", &at(sess), "--line", "2..4"]);
    assert!(out.success, "stderr {}", out.stderr);
    for expect in [
        "[for: leaf unknown (the record carries no such key)]",
        "[for: continued_in unknown (the record carries no such key)]",
        "[checkpoint closes unknown (needs startTime and totalDuration)]",
    ] {
        assert!(
            out.stdout.contains(expect),
            "missing {expect}:\n{}",
            out.stdout
        );
    }
}
