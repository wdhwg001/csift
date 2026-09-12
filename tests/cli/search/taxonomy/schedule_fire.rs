//! v0.12.2: `harness.schedule.fire`, the prompt a scheduled task fires, and the
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
    assert_eq!(row("harness.meta.loop"), Some(1), "{}", out.stdout);
    // The `scheduled_task_fire` lines the instant was read from stay GATED under
    // `harness.meta.system`: a default scan gains no hit from the keep that parsed them.
    assert_eq!(
        row("harness.meta.system"),
        None,
        "the fire records stay gated:\n{}",
        out.stdout
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
    // An ADDRESS turns the gated `harness.meta.system` leaf on, so the demote that a
    // scan applies must not run: `show` renders the fire record whole.
    let show = h.run(&["show", &at(SESS), "--line", "4"]);
    assert!(show.success, "stderr: {}", show.stderr);
    assert!(
        show.stdout
            .contains("[scheduled_task_fire] Running scheduled task (Jun 7 5:28am)"),
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
fn a_fire_with_no_sibling_record_reports_no_instant() {
    // The builds that write no `scheduled_task_fire` record at all give the same answer a
    // windowed read does: the leaf holds, the instant is null, nothing is guessed.
    let h = Home::new();
    let sess = "6b5a4c3d-2e1f-4a0b-8c7d-6e5f4a3b2c1d";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-0000000000a1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"arm the monitor"}}"#, "\n",
            r#"{"type":"assistant","uuid":"00000000-0000-4000-8000-0000000000a2","parentUuid":"00000000-0000-4000-8000-0000000000a1","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"assistant","model":"claude-opus-4-8","content":[{"type":"text","text":"armed"}]}}"#, "\n",
            r#"{"type":"user","uuid":"00000000-0000-4000-8000-0000000000a3","parentUuid":"00000000-0000-4000-8000-0000000000a2","timestamp":"2026-06-07T05:10:00.000Z","isMeta":true,"promptSource":"system","userType":"external","message":{"role":"user","content":"zzsiblingless monitoring tick, read-only"}}"#, "\n",
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
        "the leaf holds without a sibling:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("\"scheduled_at\":null"),
        "no sibling means a null instant, never a guessed one:\n{}",
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
        "the bare leaf renders with no instant marker:\n{}",
        text.stdout
    );
}
