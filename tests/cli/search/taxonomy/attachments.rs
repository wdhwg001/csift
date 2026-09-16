//! The ATTACHMENT family end to end: which body an attachment unit shows, what its label
//! zone says, which record it is for, and what the harness's own converter did with it.
//!
//! One fixture line per leaf the family owns, built from the on-disk shapes the inventory
//! names and never copied from a corpus. TWO of the nine `harness.context` leaves are
//! deliberately absent: `image` and `skill` are isMeta USER records, not attachment lines,
//! so their specimens belong to the injections family.
//!
//! This root holds the shared fixture and the one assertion helper; the cases sit in the
//! children, split by what they are about - `body` (which copy the unit shows), `labels`
//! (what each leaf calls itself, its census key, its delivery verdict), `edges` (the
//! provenance facts) and `surfaces` (an address against a scan, the two help pages, a
//! payload type csift models no leaf for).

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7c1d2e3f-4a5b-4c6d-8e9f-a0b1c2d3e4f5";

/// The transcript every case here reads. One line per attachment-borne leaf, each with a
/// distinct parent so the handles do not collide, plus a hook run with a persisted
/// conversion and one without.
fn attachment_home() -> Home {
    let h = Home::new();
    let lines = [
        r#"{"type":"user","uuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzatt chart the reef"}}"#,
        // harness.hook.context, with the harness's own conversion persisted.
        r#"{"type":"attachment","uuid":"bb000000-0000-4000-8000-000000000002","parentUuid":"aa000000-0000-4000-8000-000000000001","timestamp":"2026-06-07T05:00:01.000Z","attachment":{"type":"hook_additional_context","hookName":"reef-brief","hookEvent":"UserPromptSubmit","toolUseID":"hook-bb000000","content":["zzatt the reef brief"]},"rendered":[{"content":"<system-reminder>\nzzatt the reef brief\n</system-reminder>"}]}"#,
        // harness.hook.outcome, a run the converter turned into nothing.
        r#"{"type":"attachment","uuid":"cc000000-0000-4000-8000-000000000003","parentUuid":"bb000000-0000-4000-8000-000000000002","timestamp":"2026-06-07T05:00:02.000Z","attachment":{"type":"hook_success","hookName":"reef-gate","hookEvent":"PostToolUse","toolUseID":"toolu_01zzreef","command":"zzatt-check --reef","content":"","stdout":"zzatt stdout reef ok","stderr":"","exitCode":0,"durationMs":42}}"#,
        // harness.hook.outcome, the conditional arm: a run the converter DID convert.
        r#"{"type":"attachment","uuid":"dd000000-0000-4000-8000-000000000004","parentUuid":"cc000000-0000-4000-8000-000000000003","timestamp":"2026-06-07T05:00:03.000Z","attachment":{"type":"hook_success","hookName":"reef-start","hookEvent":"SessionStart","toolUseID":"hook-dd000000","command":"zzatt-start --reef","content":"zzatt start notice","stdout":"zzatt stdout start raw","stderr":"","exitCode":0,"durationMs":7},"rendered":[{"content":"<system-reminder>\nzzatt start notice\n</system-reminder>"}]}"#,
        // harness.context.listing
        r#"{"type":"attachment","uuid":"ee000000-0000-4000-8000-000000000005","parentUuid":"dd000000-0000-4000-8000-000000000004","timestamp":"2026-06-07T05:00:04.000Z","attachment":{"type":"skill_listing","content":"zzatt skills reef-mapper"}}"#,
        // harness.context.file with a NON-TEXT conversion: no persisted copy, still delivered.
        r#"{"type":"attachment","uuid":"ff000000-0000-4000-8000-000000000006","parentUuid":"ee000000-0000-4000-8000-000000000005","timestamp":"2026-06-07T05:00:05.000Z","attachment":{"type":"file","filename":"zzatt-reef.png","content":{"type":"image"}}}"#,
        // agent.message.structured, one of the converter's own drop arms.
        r#"{"type":"attachment","uuid":"a1000000-0000-4000-8000-000000000007","parentUuid":"ff000000-0000-4000-8000-000000000006","timestamp":"2026-06-07T05:00:06.000Z","attachment":{"type":"structured_output","toolUseID":"toolu_01zzout","output":{"zzatt":"reef depth 12m"}}}"#,
        // the person's own mid-turn steer, carrying the ordinary conversion and NOT the
        // human-turn one - the shape 20 of the 164 measured human-origin lines have, and
        // none of them carries the second key.
        r#"{"type":"attachment","uuid":"b2000000-0000-4000-8000-000000000008","parentUuid":"a1000000-0000-4000-8000-000000000007","timestamp":"2026-06-07T05:00:07.000Z","attachment":{"type":"queued_command","commandMode":"prompt","origin":{"kind":"human"},"prompt":"zzatt also map the channel"},"rendered":[{"content":"zzatt also map the channel, ordinary turn"}]}"#,
        // a queued RIDER: same payload type, no human origin, so it keeps the task leaf.
        r#"{"type":"attachment","uuid":"c3000000-0000-4000-8000-000000000009","parentUuid":"b2000000-0000-4000-8000-000000000008","timestamp":"2026-06-07T05:00:08.000Z","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"zzatt rider pulse"}}"#,
        // harness.context.reminder
        r#"{"type":"attachment","uuid":"d4000000-0000-4000-8000-000000000010","parentUuid":"c3000000-0000-4000-8000-000000000009","timestamp":"2026-06-07T05:00:09.000Z","attachment":{"type":"total_tokens_reminder","content":"zzatt budget 120k"}}"#,
        // harness.context.prompt
        r#"{"type":"attachment","uuid":"e5000000-0000-4000-8000-000000000011","parentUuid":"d4000000-0000-4000-8000-000000000010","timestamp":"2026-06-07T05:00:10.000Z","attachment":{"type":"instructions","content":"zzatt house rules"}}"#,
        // harness.context.mode
        r#"{"type":"attachment","uuid":"f6000000-0000-4000-8000-000000000012","parentUuid":"e5000000-0000-4000-8000-000000000011","timestamp":"2026-06-07T05:00:11.000Z","attachment":{"type":"plan_mode","content":"zzatt plan mode on"}}"#,
        // harness.context.ide
        r#"{"type":"attachment","uuid":"a7000000-0000-4000-8000-000000000013","parentUuid":"f6000000-0000-4000-8000-000000000012","timestamp":"2026-06-07T05:00:12.000Z","attachment":{"type":"opened_file_in_ide","filename":"zzatt-reef.rs"}}"#,
        // a second RIDER, carrying BOTH conversions - the shape every measured line with
        // the human-turn key has (all of them riders, none of them the person's own
        // steer). Appended LAST so no line number above it moves.
        r#"{"type":"attachment","uuid":"b8000000-0000-4000-8000-000000000014","parentUuid":"a7000000-0000-4000-8000-000000000013","timestamp":"2026-06-07T05:00:13.000Z","attachment":{"type":"queued_command","commandMode":"task-notification","prompt":"zzatt also map the channel"},"rendered":[{"content":"zzatt also map the channel, ordinary turn"}],"renderedInHumanTurn":[{"content":"zzatt also map the channel, human turn"}]}"#,
    ];
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{}\n", lines.join("\n")),
    );
    h
}

fn at_sess() -> String {
    format!("@{SESS}")
}

mod body;
mod edges;
mod labels;
mod surfaces;

/// The first stdout line containing `needle`, for an assertion that wants ONE unit.
fn line_with(stdout: &str, needle: &str) -> String {
    stdout
        .lines()
        .find(|l| l.contains(needle))
        .unwrap_or_else(|| panic!("no line carrying {needle:?} in:\n{stdout}"))
        .to_string()
}
