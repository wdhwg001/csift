//! The harness's own records: the automation pulse, the command and interrupt markers,
//! the schedule and hook injections, the compaction pair, and the two shapes that carry
//! no leaf at all.

use crate::harness::*;

#[test]
fn acceptance_harness_notification_monitor() {
    // sections D4 / G6 - a Monitor `<task-notification>` pulse (UNATTESTED in the corpus -> synthetic) ->
    // `harness.notification.monitor`, rendered as the `[monitor <id> <status>] <summary>` label.
    let h = acceptance_home();
    let out = acc(&h, "zzmonitor", "harness.notification.monitor");
    assert!(out.success, "D4: stderr {}", out.stderr);
    assert!(
        out.stdout.contains("harness.notification.monitor"),
        "D4/G6 monitor pulse → harness.notification.monitor:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("[monitor mon1"),
        "D4/G6 must render the automation_label, not raw XML:\n{}",
        out.stdout
    );
}

#[test]
fn acceptance_harness_command_and_interrupt() {
    // section D8 <command-name> wrapper, D9 <local-command-stdout>, D10 and D11 the two interrupt markers.
    let h = acceptance_home();
    for (oracle, token, selector) in [
        ("D8 invocation", "zzcmdargs", "harness.command.invocation"),
        ("D9 stdout", "zzstdout", "harness.command.stdout"),
        (
            "D10 interrupt.user",
            "interrupted by user",
            "harness.interrupt.user",
        ),
        (
            "D11 interrupt.tool",
            "interrupted by user for tool",
            "harness.interrupt.tool",
        ),
    ] {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_harness_schedule_and_meta() {
    // The fired wakeup tick, the resume repair prompt, the Stop-hook feedback and the
    // autonomous-loop driver tick. All ride on isMeta records that classify (not
    // user.message), each into the family its own producer belongs to.
    let h = acceptance_home();
    for (oracle, token, selector) in [
        ("D12 wakeup", "zzwakeup", "harness.schedule.wakeup"),
        (
            "D13 resume prompt",
            "Continue from where you left off",
            "harness.resume.prompt",
        ),
        ("stop-hook feedback", "zzhook", "harness.hook.feedback"),
        ("loop driver tick", "zzloop", "harness.schedule.tick"),
    ] {
        let out = acc(&h, token, selector);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains(selector),
            "{oracle} must classify {selector}:\n{}",
            out.stdout
        );
    }
}

#[test]
fn acceptance_compaction_summary_and_boundary_searchable() {
    // The isCompactSummary record is a `type:"user"` record, so it is searchable as
    // `harness.compaction.summary`. The `compact_boundary` is a `type:"system"` record and is
    // search-surfaced too - the section 7 prefilter keeps it (one memmem on `compact_boundary`) and
    // `record_raw_text` renders its top-level content + compactMetadata as the
    // match/excerpt, so compaction points can be enumerated and inspected.
    let h = acceptance_home();

    let summary = acc(&h, "zzsummary", "harness.compaction.summary");
    assert!(summary.success, "D6: stderr {}", summary.stderr);
    assert!(
        summary.stdout.contains("harness.compaction.summary"),
        "D6 compaction summary → searchable:\n{}",
        summary.stdout
    );

    let boundary = acc(&h, "zzboundary", "harness.compaction.boundary");
    assert!(
        boundary.success,
        "the boundary keep: stderr {}",
        boundary.stderr
    );
    assert!(
        boundary.stdout.contains("harness.compaction.boundary"),
        "the boundary keep surfaces a compact_boundary:\n{}",
        boundary.stdout
    );
    // The compactMetadata renders as the excerpt (trigger / pre/post tokens / duration).
    assert!(
        boundary.stdout.contains("trigger") && boundary.stdout.contains("auto"),
        "the boundary excerpt carries its compactMetadata:\n{}",
        boundary.stdout
    );
}

#[test]
fn acceptance_excluded_and_unmarked_meta_carry_no_label() {
    // section E an `attachment` carries no label; section J an isMeta record matching no harness marker is EXCLUDED
    // (never `user.message`). Neither surfaces under ANY selector.
    let h = acceptance_home();
    for (oracle, token) in [
        ("E attachment", "zzattach"),
        ("J isMeta-unmarked", "zzunmarked"),
    ] {
        // No `-t` -> every label eligible; still nothing, because classify returns empty.
        let out = h.run(&["search", token, &at(ACC_SESS), "--no-subagents"]);
        assert!(out.success, "{oracle}: stderr {}", out.stderr);
        assert!(
            out.stdout.contains("no matching exchanges"),
            "{oracle} must carry no label (no hit):\n{}",
            out.stdout
        );
    }
}

#[test]
fn harness_role_excludes_the_boundary_and_the_drilldown_keeps_it() {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"2026-06-07T05:01:00.000Z","content":"Compacted","compactMetadata":{"trigger":"auto","preTokens":900,"postTokens":90,"durationMs":4}}"#, "\n",
        ),
    );
    // The bare role: the metrics-only boundary is not the conversation.
    let role = h.run(&["search", "", &at(SESS), "-t", "harness"]);
    assert!(
        !role.stdout.contains("compaction boundary"),
        "-t harness excludes the boundary:\n{}",
        role.stdout
    );
    // The deliberate drill-down keeps its full set - compaction points stay findable.
    let drill = h.run(&["search", "", &at(SESS), "-t", "harness.compaction.*"]);
    assert!(
        drill.stdout.contains("trigger=auto"),
        "-t harness.compaction still reaches the boundary:\n{}",
        drill.stdout
    );
    // And the glob reaches everything too.
    let glob = h.run(&["search", "", &at(SESS), "-t", "harness.*"]);
    assert!(glob.stdout.contains("trigger=auto"), "{}", glob.stdout);
}
