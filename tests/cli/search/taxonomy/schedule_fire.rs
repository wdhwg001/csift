//! `harness.schedule.fire`, the prompt a scheduled task fires, and the
//! content-start anchoring that keeps a record QUOTING a loop tick from being read as one.
//!
//! The fixture carries the three shapes measured live: a cron task firing a plain armed
//! prompt (twice), an autonomous loop whose FIRST fire delivers the check preamble and whose
//! later fire delivers the driver tick, and a skill's instruction record that embeds the
//! check preamble at a non-zero offset to explain it.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7a6b5c4d-3e2f-4a1b-9c8d-7e6f5a4b3c2d";

/// L1 the human's prompt, L2 its reply. Then the repeating fire shape, three lines each: a
/// `queue-operation` rider carrying the same text, the `system`/`scheduled_task_fire` record
/// naming the instant, and the isMeta `promptSource:"system"` prompt parented to it.
/// L3-L5 and L6-L8 are a cron task's two fires; L9-L11 the autonomous loop's FIRST fire (the
/// check preamble) and L12-L14 a later one (the driver tick). L15 is the `/loop` skill's own
/// instruction record, which EMBEDS the check preamble mid-body and is not a fire at all.
fn fire_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000001","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","cwd":"/Users/dev/example-project","message":{"role":"user","content":"arm a one-minute loop that answers with one word"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-4000-8000-000000000002","parentUuid":"00000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"the cron entry is armed"}]}}"#, "\n",
            r#"{"type":"queue-operation","operation":"enqueue","sessionId":"7a6b5c4d-3e2f-4a1b-9c8d-7e6f5a4b3c2d","timestamp":"2026-06-07T05:28:00.000Z","content":"zzfired reply with the single word PEONY"}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000010","parentUuid":"00000000-0000-4000-8000-000000000002","timestamp":"2026-06-07T05:28:00.100Z","userType":"external","isMeta":false,"cron":"*/1 * * * *","taskId":"task-1","content":"Running scheduled task (Jun 7 5:28am)"}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000011","parentUuid":"00000000-0000-4000-8000-000000000010","timestamp":"2026-06-07T05:28:00.200Z","isMeta":true,"promptSource":"system","userType":"external","scheduledTaskId":"task-1","message":{"role":"user","content":"zzfired reply with the single word PEONY"}}"#, "\n",
            r#"{"type":"queue-operation","operation":"enqueue","sessionId":"7a6b5c4d-3e2f-4a1b-9c8d-7e6f5a4b3c2d","timestamp":"2026-06-07T05:29:00.000Z","content":"zzfired reply with the single word PEONY"}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000020","parentUuid":"00000000-0000-4000-8000-000000000011","timestamp":"2026-06-07T05:29:00.100Z","userType":"external","isMeta":false,"cron":"*/1 * * * *","taskId":"task-1","content":"Running scheduled task (Jun 7 5:29am)"}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000021","parentUuid":"00000000-0000-4000-8000-000000000020","timestamp":"2026-06-07T05:29:00.200Z","isMeta":true,"promptSource":"system","userType":"external","scheduledTaskId":"task-1","message":{"role":"user","content":"zzfired reply with the single word PEONY"}}"#, "\n",
            r##"{"type":"queue-operation","operation":"enqueue","sessionId":"7a6b5c4d-3e2f-4a1b-9c8d-7e6f5a4b3c2d","timestamp":"2026-06-07T05:31:00.000Z","content":"# Autonomous loop check"}"##, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000030","parentUuid":"00000000-0000-4000-8000-000000000021","timestamp":"2026-06-07T05:31:00.100Z","userType":"external","isMeta":false,"content":"Claude resuming /loop wakeup (Jun 7 5:31am)"}"#, "\n",
            r##"{"type":"user","uuid":"00000000-0000-4000-8000-000000000031","parentUuid":"00000000-0000-4000-8000-000000000030","timestamp":"2026-06-07T05:31:00.200Z","isMeta":true,"promptSource":"system","userType":"external","message":{"role":"user","content":"# Autonomous loop check\n\nYou're being invoked on a timer while the user is away or occupied. zzcheckbody"}}"##, "\n",
            r##"{"type":"queue-operation","operation":"enqueue","sessionId":"7a6b5c4d-3e2f-4a1b-9c8d-7e6f5a4b3c2d","timestamp":"2026-06-07T05:32:00.000Z","content":"# Autonomous loop tick"}"##, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-000000000040","parentUuid":"00000000-0000-4000-8000-000000000031","timestamp":"2026-06-07T05:32:00.100Z","userType":"external","isMeta":false,"content":"Claude resuming /loop wakeup (Jun 7 5:32am)"}"#, "\n",
            r##"{"type":"user","uuid":"00000000-0000-4000-8000-000000000041","parentUuid":"00000000-0000-4000-8000-000000000040","timestamp":"2026-06-07T05:32:00.200Z","isMeta":true,"promptSource":"system","userType":"external","message":{"role":"user","content":"# Autonomous loop tick\n\nRun the autonomous check using the loop instructions established earlier. zztickbody"}}"##, "\n",
            r##"{"type":"user","uuid":"00000000-0000-4000-8000-000000000050","parentUuid":"00000000-0000-4000-8000-000000000041","timestamp":"2026-06-07T05:33:00.000Z","isMeta":true,"message":{"role":"user","content":"# /loop - schedule the autonomous default\n\nzzskillbody The user invoked /loop with no prompt, so each fire delivers this text:\n\n# Autonomous loop check\n\nYou're being invoked on a timer while the user is away or occupied."}}"##, "\n",
        ),
    );
    h
}

#[test]
fn the_fired_prompt_carries_the_new_leaf_and_its_instant() {
    let h = fire_home();
    let out = h.run(&[
        "search",
        "zzfired",
        &at(SESS),
        "-t",
        "harness.schedule.fire",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert_eq!(
        hit_lines(&out.stdout, "harness.schedule.fire"),
        2,
        "both cron fires carry the leaf:\n{}",
        out.stdout
    );
    // The instant comes from the `scheduled_task_fire` sibling the prompt is parented to -
    // display-only, in the label zone, while the body stays the verbatim armed text.
    assert!(
        out.stdout.contains("[scheduled fire Jun 7 5:28am]")
            && out.stdout.contains("[scheduled fire Jun 7 5:29am]"),
        "each fire names its own instant:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("zzfired reply with the single word PEONY"),
        "the prompt body renders verbatim:\n{}",
        out.stdout
    );
}

/// Count the HIT lines a text render carries under one leaf (the label leads each hit line
/// after the role glyph; the banner and footer mention the leaf too, so a bare substring
/// count over the whole render is not the number of records).
fn hit_lines(stdout: &str, leaf: &str) -> usize {
    stdout
        .lines()
        .filter(|l| l.trim_start().starts_with(&format!("\u{2699} {leaf}")))
        .count()
}

#[test]
fn the_leaf_is_reachable_by_prefix_role_and_full_path() {
    let h = fire_home();
    for sel in ["harness", "harness.schedule", "harness.schedule.fire"] {
        let out = h.run(&["search", "zzfired", &at(SESS), "-t", sel]);
        assert!(out.success, "{sel}: stderr {}", out.stderr);
        assert_eq!(
            hit_lines(&out.stdout, "harness.schedule.fire"),
            2,
            "selector {sel}:\n{}",
            out.stdout
        );
    }
    // It is a harness record, never a user one: the human never typed it.
    let user = h.run(&["search", "zzfired", &at(SESS), "-t", "user", "-c"]);
    assert_eq!(user.stdout.trim(), "0", "{}", user.stdout);
}

#[test]
fn the_label_census_counts_the_new_leaf_and_leaves_the_tick_leaves_alone() {
    let h = fire_home();
    let out = h.run(&["search", "", &at(SESS), "--count-by", "label"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let row = |leaf: &str| {
        out.stdout.lines().find_map(|l| {
            let mut f = l.split_whitespace();
            let n = f.next()?.parse::<usize>().ok()?;
            (f.next() == Some(leaf)).then_some(n)
        })
    };
    assert_eq!(
        row("harness.schedule.fire"),
        Some(2),
        "only the two unmarked fires:\n{}",
        out.stdout
    );
    // The first autonomous fire keeps the wakeup leaf, the later one the driver leaf - arm
    // order, not a second predicate - and the skill's instruction record joins neither.
    assert_eq!(row("harness.schedule.wakeup"), Some(1), "{}", out.stdout);
    assert_eq!(row("harness.schedule.tick"), Some(1), "{}", out.stdout);
    // The `scheduled_task_fire` lines the instant was read from stay GATED under their
    // own leaf: a default scan gains no hit from the keep that parsed them. The leaf has
    // to be the LIVE one - asserting the absence of a retired spelling would pass on any
    // build, including one where the records leaked under their real name.
    assert_eq!(
        row("harness.schedule.event"),
        None,
        "the fire records stay gated:\n{}",
        out.stdout
    );
    // And naming the leaf reaches them, so the absence above is a GATE and not a gap.
    let named = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.schedule.event",
        "--count-by",
        "label",
        "--format",
        "json",
    ]);
    assert!(named.success, "stderr: {}", named.stderr);
    assert!(
        named.stdout.contains("harness.schedule.event"),
        "the fire records are reachable by their own selector:\n{}",
        named.stdout
    );
}

#[test]
fn json_carries_the_label_and_the_scheduled_at_field() {
    let h = fire_home();
    let out = h.run(&[
        "search",
        "zzfired",
        &at(SESS),
        "-t",
        "harness.schedule.fire",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits: Vec<serde_json::Value> = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("exchange"))
        .flat_map(|v| {
            v.get("hits")
                .and_then(|h| h.as_array())
                .cloned()
                .unwrap_or_default()
        })
        .collect();
    assert_eq!(hits.len(), 2, "two fired prompts:\n{}", out.stdout);
    for hit in &hits {
        assert_eq!(hit["label"], "harness.schedule.fire", "{hit}");
    }
    let instants: Vec<&str> = hits
        .iter()
        .filter_map(|h| h["scheduled_at"].as_str())
        .collect();
    assert_eq!(
        instants,
        vec!["Jun 7 5:28am", "Jun 7 5:29am"],
        "{}",
        out.stdout
    );
}

#[test]
fn a_record_quoting_the_preamble_is_not_the_tick() {
    let h = fire_home();
    // The skill's instruction record embeds BOTH wakeup markers deep in its body. Anchoring
    // at content start leaves it where an unmarked isMeta record has always been: no label,
    // so no leaf claims it and the real tick's count stays honest.
    let wake = h.run(&["search", "zzskillbody", &at(SESS), "-c"]);
    assert!(wake.success, "stderr: {}", wake.stderr);
    assert_eq!(
        wake.stdout.trim(),
        "0",
        "an unlabeled isMeta record is not surfaced by a scan:\n{}",
        wake.stdout
    );
    // It is also not a fire: it carries no `promptSource`, so the fire arm refuses it too.
    let fire = h.run(&[
        "search",
        "zzskillbody",
        &at(SESS),
        "-t",
        "harness.schedule.fire",
        "-c",
    ]);
    assert_eq!(fire.stdout.trim(), "0", "{}", fire.stdout);
    // The genuine check tick, whose marker IS at content start, still carries the leaf.
    let real = h.run(&[
        "search",
        "zzcheckbody",
        &at(SESS),
        "-t",
        "harness.schedule.wakeup",
    ]);
    assert_eq!(
        hit_lines(&real.stdout, "harness.schedule.wakeup"),
        1,
        "{}",
        real.stdout
    );
}

#[test]
fn the_demote_leaves_the_fire_line_addressable_and_keeps_it_on_the_chain() {
    let h = fire_home();
    // An ADDRESS turns the gated `harness.schedule.event` leaf on, so the demote that a
    // scan applies must not run: `show` renders the fire record whole, under the fire
    // record's own head - the instant it names, then its content.
    let show = h.run(&["show", &at(SESS), "--line", "4"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout.contains("[scheduled Jun 7 5:28am]")
            && show
                .stdout
                .contains("Running scheduled task (Jun 7 5:28am)"),
        "an addressed fire record renders under the system leaf:\n{}",
        show.stdout
    );
    // Under a SCAN the record is demoted to a spine row rather than dropped, because
    // the chain walks parentUuid THROUGH it - a fired prompt's parent IS one. Losing
    // the row would break the walk there and leave every record above it pre-cut.
    let json = h.run(&[
        "search",
        "zzfired",
        &at(SESS),
        "-t",
        "harness.schedule.fire",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let survival: Vec<String> = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| {
            v["hits"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|h| h["survival"].as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        survival,
        vec!["live".to_string(), "live".to_string()],
        "both fired prompts stay on the chain:\n{}",
        json.stdout
    );
}

#[test]
fn a_fire_record_with_no_readable_instant_still_gates_its_prompt() {
    // The instant is a DISPLAY fact read off the fire record's parenthesised tail; the
    // LEAF is the parent link. A fire record whose tail is unreadable therefore still
    // claims the prompt under it, and the instant reports null rather than a guess.
    let h = Home::new();
    let sess = "6b5a4c3d-2e1f-4a0b-8c7d-6e5f4a3b2c1d";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-0000000000a1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"arm the monitor"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-4000-8000-0000000000a2","parentUuid":"00000000-0000-4000-8000-0000000000a1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"armed"}]}}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"00000000-0000-4000-8000-0000000000f1","parentUuid":"00000000-0000-4000-8000-0000000000a2","timestamp":"2026-06-07T05:09:59.000Z","content":"Running scheduled task"}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-0000000000a3","parentUuid":"00000000-0000-4000-8000-0000000000f1","timestamp":"2026-06-07T05:10:00.000Z","isMeta":true,"promptSource":"system","userType":"external","message":{"role":"user","content":"zzsiblingless monitoring tick, read-only"}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzsiblingless",
        &at(sess),
        "-t",
        "harness.schedule.fire",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("\"label\":\"harness.schedule.fire\""),
        "the leaf holds on the link alone:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("\"scheduled_at\":null"),
        "an unreadable tail means a null instant, never a guessed one:\n{}",
        out.stdout
    );
    let text = h.run(&[
        "search",
        "zzsiblingless",
        &at(sess),
        "-t",
        "harness.schedule.fire",
    ]);
    assert!(
        text.stdout.contains("harness.schedule.fire") && !text.stdout.contains("[scheduled fire"),
        "the bare leaf renders without a fabricated instant:\n{}",
        text.stdout
    );
}

#[test]
fn a_prompt_with_no_fire_record_above_it_is_not_claimed() {
    // The parent link is the ONLY mark the producer leaves on the prompt, so a record
    // that merely carries the isMeta + promptSource stamp is not a fired prompt: csift
    // declines to claim it rather than reading a stamp the submit path puts on every
    // isMeta submission.
    let h = Home::new();
    let sess = "7c6b5d4e-3f2a-4b1c-9d8e-7f6a5b4c3d2e";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-0000000000b1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"arm the monitor"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-4000-8000-0000000000b2","parentUuid":"00000000-0000-4000-8000-0000000000b1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"armed"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-0000000000b3","parentUuid":"00000000-0000-4000-8000-0000000000b2","timestamp":"2026-06-07T05:10:00.000Z","isMeta":true,"promptSource":"system","userType":"external","message":{"role":"user","content":"zzunlinked monitoring tick, read-only"}}"#, "\n",
        ),
    );
    let out = h.run(&[
        "search",
        "zzunlinked",
        &at(sess),
        "-t",
        "harness.schedule.fire",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "an unlinked isMeta prompt is not a fired one:\n{}",
        out.stdout
    );
}

/// Every hit object of a `--format json` run, flattened out of its exchange rows.
fn schedule_hits(stdout: &str) -> Vec<serde_json::Value> {
    json_rows(stdout, "exchange")
        .into_iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .collect()
}

/// The instant and the `fires` edge are facts of the PARENT LINK, not of the fire leaf: a
/// fired prompt whose armed text opens with a loop marker takes the wakeup or tick leaf (the
/// marker arms sit ahead of the fire arm), and it is still the prompt its fire record
/// submitted. Both read null and edge-less before; a tick with no fire record above it
/// carries neither.
#[test]
fn a_wakeup_or_tick_under_a_fire_record_carries_the_instant_and_the_fires_edge() {
    let h = fire_home();
    let out = h.run(&[
        "search",
        "",
        &at(SESS),
        "-t",
        "harness.schedule.wakeup",
        "-t",
        "harness.schedule.tick",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hits = schedule_hits(&out.stdout);
    let seen: Vec<(u64, String, String, Vec<String>)> = hits
        .iter()
        .map(|h| {
            (
                h["line"].as_u64().unwrap_or(0),
                h["label"].as_str().unwrap_or("").to_string(),
                h["scheduled_at"].as_str().unwrap_or("-").to_string(),
                h["for"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| e["kind"].as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            (
                11,
                "harness.schedule.wakeup".into(),
                "Jun 7 5:31am".into(),
                vec!["fires".into()]
            ),
            (
                14,
                "harness.schedule.tick".into(),
                "Jun 7 5:32am".into(),
                vec!["fires".into()]
            ),
        ],
        "{}",
        out.stdout
    );
    let text = h.run(&[
        "search",
        "zztickbody",
        &at(SESS),
        "-t",
        "harness.schedule.tick",
    ]);
    assert!(
        text.stdout.contains("[scheduled fire Jun 7 5:32am]"),
        "the label zone names the instant on the tick too:\n{}",
        text.stdout
    );

    // A tick nothing fired: no fire record is its parent, so it carries neither.
    let bare = Home::new();
    let sess = "7a6b5c4d-3e2f-4a1b-9c8d-000000000060";
    bare.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-000000000061","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r##"{"type":"user","uuid":"00000000-0000-4000-8000-000000000062","parentUuid":"00000000-0000-4000-8000-000000000061","timestamp":"2026-06-07T05:01:00.000Z","isMeta":true,"message":{"role":"user","content":"# Autonomous loop tick\n\nzzbaretick"}}"##, "\n",
        ),
    );
    let out = bare.run(&[
        "search",
        "zzbaretick",
        &at(sess),
        "-t",
        "harness.schedule.tick",
        "--format",
        "json",
    ]);
    let hit = schedule_hits(&out.stdout)
        .into_iter()
        .next()
        .expect("the bare tick");
    assert_eq!(hit["scheduled_at"], serde_json::Value::Null, "{hit}");
    let kinds: Vec<&str> = hit["for"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["kind"].as_str()).collect())
        .unwrap_or_default();
    assert!(!kinds.contains(&"fires"), "{hit}");
}

/// Claude Code parents every append to the running tail, so a background agent's
/// `<task-notification>` arriving right after a fire record is that record's child too. It
/// did not fire: the instant and the `fires` edge belong to the scheduler's own submissions,
/// and the pulse's inbox view keeps its direction in text as in JSON.
#[test]
fn a_pulse_under_a_fire_record_carries_no_fire_facts_and_keeps_its_direction() {
    const PSESS: &str = "8b7c6d5e-4f3a-4b2c-8d1e-0f9a8b7c6d5e";
    let h = Home::new();
    h.write(
        &format!("{ENC}/{PSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"arm the cron"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"armed"}]}}"#, "\n",
            r#"{"type":"system","subtype":"scheduled_task_fire","uuid":"f1","parentUuid":"a1","timestamp":"2026-06-07T05:28:00.100Z","isMeta":false,"cron":"*/1 * * * *","taskId":"task-1","content":"Running scheduled task (Jun 7 5:28am)"}"#, "\n",
            r#"{"type":"user","uuid":"p1","parentUuid":"f1","timestamp":"2026-06-07T05:28:00.200Z","isMeta":true,"promptSource":"system","message":{"role":"user","content":"zzfired check the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"n1","parentUuid":"f1","timestamp":"2026-06-07T05:28:01.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>bgjob</task-id>\n<tool-use-id>toolu_bg</tool-use-id>\n<status>completed</status>\n<summary>Agent \"zzpulse survey\" completed</summary>\n<result>zzpulse the survey is done</result>\n</task-notification>"}}"#, "\n",
        ),
    );
    let text = h.run(&["search", "zzpulse", &at(PSESS)]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(!text.stdout.contains("[scheduled fire"), "{}", text.stdout);
    let inbox = text
        .stdout
        .lines()
        .find(|l| l.contains("agent.communication.inbox"))
        .unwrap_or_else(|| panic!("an inbox unit:\n{}", text.stdout));
    assert!(inbox.contains("\u{21e8} self"), "{inbox}");
    let json = h.run(&["search", "zzpulse", &at(PSESS), "--format", "json"]);
    let hits: Vec<serde_json::Value> = json
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    assert_eq!(hits.len(), 2, "{}", json.stdout);
    for hit in &hits {
        assert!(hit["scheduled_at"].is_null(), "{hit}");
        let edges = hit["for"].as_array().cloned().unwrap_or_default();
        assert!(!edges.iter().any(|e| e["kind"] == "fires"), "{hit}");
    }
    let inbox = hits
        .iter()
        .find(|h| h["label"] == "agent.communication.inbox")
        .expect("the inbox hit");
    assert_eq!(inbox["to"], "self", "{inbox}");
    // The prompt the fire record submitted still carries both.
    let fired = h.run(&["search", "zzfired", &at(PSESS), "--format", "json"]);
    let prompt = fired
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .find(|h| h["label"] == "harness.schedule.fire")
        .unwrap_or_else(|| panic!("the fired prompt:\n{}", fired.stdout));
    assert_eq!(prompt["scheduled_at"], "Jun 7 5:28am", "{prompt}");
    let edges = prompt["for"].as_array().cloned().unwrap_or_default();
    assert!(edges.iter().any(|e| e["kind"] == "fires"), "{prompt}");
}
