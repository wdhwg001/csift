//! verbatim turn selection: richness modes, dedup demotion, automation floors.

use crate::harness::*;

#[test]
fn turns_agent_msgs_rich_restores_middles_and_collapses_declarations() {
    // `--agent-msgs rich` over the long-run turn: the rich first / sudden-rich middle /
    // fused body survive verbatim; the pure-declaration middles collapse into a
    // placeholder carrying a fetchable L{a}–L{b} range. The default (eot-only) shows ONLY
    // the EOT - proving the flag changes behavior.
    let h = turns_home();
    let rich = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
    ]);
    assert!(rich.success, "stderr: {}", rich.stderr);
    // Rich members survive verbatim.
    assert!(
        rich.stdout.contains("AGENTRICHFIRST"),
        "rich first kept: {}",
        rich.stdout
    );
    assert!(
        rich.stdout.contains("AGENTRICHMID"),
        "sudden rich middle kept"
    );
    assert!(
        rich.stdout.contains("FUSEDTAIL"),
        "fused finding+decl body kept whole"
    );
    assert!(rich.stdout.contains("AGENTEOT"), "the EOT is always kept");
    // The pure declarations are collapsed - their unique token must NOT appear verbatim.
    assert!(
        !rich.stdout.contains("LETMEDECL"),
        "pure declarations must be collapsed, not emitted: {}",
        rich.stdout
    );
    // A placeholder line with a fetchable range is present.
    assert!(
        rich.stdout.contains("agent message") && (rich.stdout.contains("tool call")),
        "a collapsed-agents placeholder is present: {}",
        rich.stdout
    );
    // The `eot-only` ESCAPE keeps only the EOT - the intermediate rich members are absent.
    let eot = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "eot-only",
    ]);
    assert!(eot.stdout.contains("AGENTEOT"), "eot-only keeps the EOT");
    assert!(
        !eot.stdout.contains("AGENTRICHFIRST") && !eot.stdout.contains("AGENTRICHMID"),
        "the eot-only escape must NOT restore intermediate agent messages: {}",
        eot.stdout
    );
}

#[test]
fn turns_default_longest_restores_substance_and_drops_declarations() {
    // The NEW DEFAULT (`longest`, no flag) over the long-run fixture turn. The agent run's
    // char lengths are: AGENTRICHFIRST=43, decls 26–34, AGENTRICHMID=45, FUSEDTAIL=72
    // (the LONGEST), AGENTEOT=35. So the default keeps:
    //   • FUSEDTAIL - the LONGEST (72 chars) → the substantive Rich Response.
    //   • AGENTRICHMID - a RICH middle (file:line + ratio) → a mid-run major finding.
    // and COLLAPSES everything else into placeholders, INCLUDING:
    //   • AGENTRICHFIRST - a SHORT first (43 < 280 rich-min) and not the longest → dropped
    //     (proves the first is kept only when SUBSTANTIVE, not merely rich/present).
    //   • AGENTEOT - a SHORT, non-rich LAST (the ~35-char throwaway wrap-up) → dropped
    //     (THE headline: the last is no longer unconditionally kept; the substance is).
    //   • the pure LETMEDECL declarations.
    // This is exactly the substance the OLD `agents.last()` default silently dropped, plus
    // the deliberate dropping of the throwaway last.
    let h = turns_home();
    let dflt = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(dflt.success, "stderr: {}", dflt.stderr);
    // The LONGEST + the rich middle are restored.
    assert!(
        dflt.stdout.contains("FUSEDTAIL"),
        "default restores the LONGEST agent message: {}",
        dflt.stdout
    );
    assert!(
        dflt.stdout.contains("AGENTRICHMID"),
        "default restores the rich middle finding: {}",
        dflt.stdout
    );
    // The throwaway last (AGENTEOT) and the short first (AGENTRICHFIRST) are NOT kept by
    // the default - they fall below the substantive/rich bar and are not the longest.
    assert!(
        !dflt.stdout.contains("AGENTEOT"),
        "default drops the non-rich throwaway LAST (the headline case): {}",
        dflt.stdout
    );
    assert!(
        !dflt.stdout.contains("AGENTRICHFIRST"),
        "default drops a SHORT (non-substantive) first: {}",
        dflt.stdout
    );
    // The pure declarations still collapse - the default is NOT `all`.
    assert!(
        !dflt.stdout.contains("LETMEDECL"),
        "default collapses pure declarations into a placeholder: {}",
        dflt.stdout
    );
    assert!(
        dflt.stdout.contains("agent message") && dflt.stdout.contains("tool call"),
        "a collapsed-agents placeholder is present under the default: {}",
        dflt.stdout
    );
}

#[test]
fn turns_dedup_demotes_summary_match_never_drops() {
    // Turn 0's user ("the very first ask...") is quoted verbatim by SUMMARY #1's §6.
    // BUT turn 0 sits BEFORE older boundaries (compactions_before > 0), so it is NOT
    // deduped (older summary content is gone from context). To exercise live-region
    // dedup we check the NEWEST summary's quotes against live turns; the fixture's live
    // turns are unique, so dedup count may be 0 here - assert the mechanism via the
    // header only when it fires, and always assert nothing is dropped.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    // Every selected unit has a boolean also_in_summary field (mechanism wired).
    for o in objs.iter().filter(|o| o.get("role").is_some()) {
        assert!(o["also_in_summary"].is_boolean());
    }
    // Turn 0's verbatim user text is still present (not dropped) even though SUMMARY #1
    // quotes it - pre-boundary turns are pure restoration.
    assert!(
        objs.iter().any(|o| o["role"] == "user"
            && o["text"]
                .as_str()
                .unwrap()
                .contains("the very first ask about the café")),
        "the pre-boundary verbatim user turn is restored, never dropped"
    );
}

#[test]
fn turns_automation_notification_does_not_consume_human_round_trip_floor() {
    // The round-trip HARD FLOOR is reserved for HUMAN exchanges. A session whose RECENT
    // turns are machine automation pulses (each with an agent ack) plus ONE older human
    // round-trip, at a small budget, must still recover the human turn - the pulses must NOT
    // crowd it out of the protected floor (the prior `is_round_trip` ignored is_automation).
    let h = Home::new();
    let sess = "22222222-3333-4444-5555-666666666666";
    let mut lines = vec![
        // The OLDER human round-trip (the one the floor must protect).
        r#"{"type":"user","uuid":"u0","cwd":"/Users/x/r","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"HUMAN-QUESTION-MARKER please explain the carry-propagation bug in detail"}}"#.to_string(),
        r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"The carry is the partial line held across a chunk boundary; here is the full explanation of the propagation path and the fix."}]}}"#.to_string(),
    ];
    // SEVEN newer automation pulses (each a round-trip pulse→ack) - recency-first, these
    // would be picked before the human turn and (under the bug) consume the floor.
    for i in 0..7 {
        lines.push(format!(
            r#"{{"type":"user","uuid":"n{i}","timestamp":"2026-06-07T06:0{i}:00.000Z","message":{{"role":"user","content":"<task-notification>\n<task-id>auto{i}</task-id>\n<status>completed</status>\n<summary>Background command \"job {i}\" completed (exit code 0)</summary>\n</task-notification>"}}}}"#
        ));
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"m{i}","parentUuid":"n{i}","timestamp":"2026-06-07T06:0{i}:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"Acknowledged pulse {i}."}}]}}}}"#
        ));
    }
    h.write(
        &format!("-Users-x-r/{sess}.jsonl"),
        &(lines.join("\n") + "\n"),
    );
    // A budget small enough that, if the floor were spent on pulses, the human turn would be
    // crowded out - but large enough to fit the human round-trip in its protected lane.
    let t = h.run(&[
        "verbatim",
        at(sess).as_str(),
        "--budget",
        "1200",
        "--no-subagents",
    ]);
    assert!(t.success, "stderr: {}", t.stderr);
    assert!(
        t.stdout.contains("HUMAN-QUESTION-MARKER"),
        "the human round-trip must survive the floor despite newer automation pulses; got: {}",
        t.stdout
    );
}

#[test]
fn turns_fidelity_beats_summary_verbatim_count() {
    // The summary holds ~1 verbatim assistant quote (§9) + a handful of clipped §6
    // bullets. `turns` restores MANY more verbatim units. Assert concrete counts:
    // >= 3 restored user units and >= 3 restored assistant units, far exceeding the
    // summary's single verbatim assistant quote.
    let h = turns_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&out.stdout);
    let users = objs.iter().filter(|o| o["role"] == "user").count();
    let asst = objs.iter().filter(|o| o["role"] == "assistant").count();
    assert!(
        users >= 3,
        "restored user units {users} must exceed the summary's clipped bullets"
    );
    assert!(
        asst >= 3,
        "restored assistant units {asst} must exceed the summary's 1 verbatim quote"
    );
}

#[test]
fn turns_live_region_dedup_demotes_and_flags() {
    let h = turns_dedup_home();
    // Text: the dedup header line + the (also in summary) flag must appear.
    let text = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
    ]);
    assert!(text.success, "stderr: {}", text.stderr);
    assert!(
        text.stdout.contains("also present in summary"),
        "dedup header line: {}",
        text.stdout
    );
    assert!(
        text.stdout.contains("(also in summary)"),
        "demoted-unit flag: {}",
        text.stdout
    );
    // JSON: exactly the live duplicate unit carries also_in_summary:true, and it is still
    // PRESENT (demoted, never dropped).
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    let objs = json_lines(&json.stdout);
    let flagged: Vec<_> = objs
        .iter()
        .filter(|o| o.get("also_in_summary").and_then(|v| v.as_bool()) == Some(true))
        .collect();
    assert!(
        !flagged.is_empty(),
        "at least one unit flagged also_in_summary"
    );
    assert!(
        flagged.iter().any(|o| o["text"]
            .as_str()
            .unwrap()
            .contains("the live duplicate ask")),
        "the live duplicate unit is flagged AND present (not dropped)"
    );
}

#[test]
fn turns_agent_msgs_rich_placeholder_range_is_fetchable_and_attributed() {
    // The JSON form carries a `collapsed_agents` record with X/Y/Z + first/last line so a
    // consumer can Read the raw range; Y is non-zero (each collapsed msg had a tool_use).
    let h = turns_home();
    let json = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "rich",
        "--format",
        "json",
    ]);
    assert!(json.success, "stderr: {}", json.stderr);
    let objs = json_lines(&json.stdout);
    let ph = objs
        .iter()
        .find(|o| o["kind"] == "collapsed_agents")
        .expect("a collapsed_agents placeholder record");
    assert!(ph["agent_messages"].as_u64().unwrap() >= 1);
    assert!(
        ph["tool_calls"].as_u64().unwrap() >= 1,
        "Y attributes the span's tool calls"
    );
    let first = ph["first_line"].as_u64().unwrap();
    let last = ph["last_line"].as_u64().unwrap();
    assert!(first <= last && first > 0, "a fetchable jsonl line range");
}

#[test]
fn turns_agent_msgs_all_keeps_every_message_no_placeholder() {
    // `--agent-msgs all` emits every agent message of the long run, no placeholder.
    let h = turns_home();
    let all = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--agent-msgs",
        "all",
    ]);
    assert!(all.success, "stderr: {}", all.stderr);
    // Even the pure declarations appear verbatim now.
    assert!(
        all.stdout.contains("LETMEDECL"),
        "all keeps declarations: {}",
        all.stdout
    );
    assert!(all.stdout.contains("AGENTRICHFIRST") && all.stdout.contains("AGENTEOT"));
    // No collapsed-agents placeholder line.
    assert!(
        !all.stdout.contains("agent messages]") && !all.stdout.contains("agent message]"),
        "all mode emits no placeholder: {}",
        all.stdout
    );
}

#[test]
fn turns_eot_only_escape_is_byte_identical_to_pre_feature_baseline() {
    // The `eot-only` ESCAPE reproduces the pre-feature single-EOT document byte-for-byte
    // (the "force last-only" guarantee), asserted TWO ways:
    //   (1) `--agent-msgs eot-only` is byte-identical to a CAPTURED pre-feature baseline -
    //       catches a drift in the last-only path even if the default moved with it;
    //   (2) the IMPLICIT default now DIFFERS - it restores intermediate substance (the
    //       longest + rich members) the old single-EOT default silently dropped.
    // `TZ=UTC` pins the system-local timestamp render so the captured baseline is portable.
    let h = turns_home();
    let tz_utc = [("TZ", "UTC")];
    let eot_only = h.run_with_env(
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--agent-msgs",
            "eot-only",
        ],
        &tz_utc,
    );
    assert!(eot_only.success, "stderr: {}", eot_only.stderr);
    assert_eq!(
        eot_only.stdout, TURNS_PRE_FEATURE_BASELINE,
        "`--agent-msgs eot-only` must be byte-identical to the captured pre-feature \
         (single-EOT) baseline; an INTENDED eot-only-output change requires re-capturing \
         tests/turns_pre_feature_baseline.txt under TZ=UTC --agent-msgs eot-only"
    );

    // The implicit default (Longest) is DIFFERENT - it restores the substance the
    // single-EOT default dropped (proving the default changed, not just a flag alias).
    let implicit = h.run_with_env(
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
        ],
        &tz_utc,
    );
    assert!(implicit.success, "stderr: {}", implicit.stderr);
    assert_ne!(
        implicit.stdout, eot_only.stdout,
        "the implicit default must NO LONGER equal eot-only — it keeps the longest + \
         rich members the single-EOT default silently dropped"
    );
}

#[test]
fn turns_profile_heavy_keeps_at_least_as_many_as_light() {
    // heavy (lower thresholds) selects >= as many KEPT agent messages as light, and both
    // are bounded by `all` and floored by `eot-only`.
    let h = turns_home();
    let at_sess = at(SESS);
    let kept_agents = |args: &[&str]| -> usize {
        let mut full = vec![
            "verbatim",
            at_sess.as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--format",
            "json",
        ];
        full.extend_from_slice(args);
        let out = h.run(&full);
        assert!(out.success, "stderr: {}", out.stderr);
        json_lines(&out.stdout)
            .iter()
            .filter(|o| o["role"] == "assistant")
            .count()
    };
    let eot = kept_agents(&["--agent-msgs", "eot-only"]);
    let light = kept_agents(&["--profile", "light"]);
    let heavy = kept_agents(&["--profile", "heavy"]);
    let all = kept_agents(&["--agent-msgs", "all"]);
    assert!(heavy >= light, "heavy {heavy} >= light {light}");
    assert!(light >= eot, "light {light} >= eot-only {eot}");
    assert!(all >= heavy, "all {all} >= heavy {heavy}");
}
