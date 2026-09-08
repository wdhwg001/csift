//! A turn the operator rewound past: what leaves the numbering, what a bare ROLE
//! selector stops seeing, and what the footer and the JSON summary say about it.

use super::*;

#[test]
fn a_rewound_turn_leaves_the_numbering_and_takes_its_subtree_with_it() {
    let h = rewind_home();
    // Two numbered turns remain: the first prompt and the resend. The rewound prompt has
    // no t<N> at all.
    let out = h.run(&["search", "", &at(SESS), "--count-by", "turn"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("t0") && out.stdout.contains("t1") && !out.stdout.contains("t2"),
        "two numbered turns:\n{}",
        out.stdout
    );
    let rewound = h.run(&["search", "", &at(SESS), "-t", "user.rewound"]);
    assert!(
        rewound.stdout.contains("dredge the northern channel")
            && rewound.stdout.contains("rewound turn")
            && rewound.stdout.contains("outside turn numbering"),
        "the rewound prompt surfaces under its own leaf:\n{}",
        rewound.stdout
    );
    assert!(
        !rewound.stdout.contains("southern shoal"),
        "the resend is not rewound:\n{}",
        rewound.stdout
    );
    // The C-27 line names where the conversation went instead.
    assert!(
        rewound
            .stdout
            .contains("rewound: the conversation continued from L8 instead"),
        "the diff line leads with the resend's address:\n{}",
        rewound.stdout
    );
}

#[test]
fn a_bare_role_skips_the_rewound_branch_and_the_glob_reaches_it() {
    let h = rewind_home();
    let role = h.run(&["search", "", &at(SESS), "-t", "user"]);
    assert!(
        !role.stdout.contains("dredge the northern channel"),
        "-t user is the surviving conversation:\n{}",
        role.stdout
    );
    assert!(
        role.stdout.contains("southern shoal") && role.stdout.contains("chart the lagoon"),
        "{}",
        role.stdout
    );
    // The rewound branch's ASSISTANT records leave the agent role too.
    let agent = h.run(&["search", "", &at(SESS), "-t", "agent"]);
    assert!(
        !agent.stdout.contains("dredging the northern channel"),
        "-t agent skips the rewound reply:\n{}",
        agent.stdout
    );
    assert!(
        agent.stdout.contains("surveying the southern shoal"),
        "{}",
        agent.stdout
    );
    // The explicit forms reach everything, marked.
    let glob = h.run(&["search", "", &at(SESS), "-t", "user.*"]);
    assert!(
        glob.stdout.contains("dredge the northern channel"),
        "-t 'user.*' includes it:\n{}",
        glob.stdout
    );
    let leaf = h.run(&["search", "", &at(SESS), "-t", "agent.tool.use"]);
    assert!(
        leaf.stdout.contains("[rewound]") && leaf.stdout.contains("depths.md"),
        "an explicit leaf reaches the branch, marked [rewound]:\n{}",
        leaf.stdout
    );
}

#[test]
fn the_footer_and_the_summary_disclose_the_axis() {
    let h = rewind_home();
    let out = h.run(&["search", "", &at(SESS)]);
    assert!(
        out.stdout
            .contains("1 rewound turn(s) outside turn numbering")
            && out.stdout.contains("-t user.rewound"),
        "the footer names the count and the leaf:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("record(s) off the surviving conversation"),
        "and the abandoned record count:\n{}",
        out.stdout
    );
    assert!(out.stdout.contains("chain leaf: tail"), "{}", out.stdout);
    let j = h.run(&["search", "", &at(SESS), "--format", "json"]);
    let s = summary(&j.stdout);
    assert_eq!(s["rewound_turns"], 1, "{}", j.stdout);
    assert_eq!(s["superseded_drafts"], 0, "{}", j.stdout);
    assert_eq!(s["abandoned_records"], 4, "{}", j.stdout);
    assert_eq!(s["replay_copies"], 0, "{}", j.stdout);
    assert!(s["boundary_cut_line"].is_null(), "{}", j.stdout);
    assert_eq!(s["leaf_source"], "tail", "{}", j.stdout);
}

#[test]
fn json_carries_the_survival_fields_per_hit() {
    let h = rewind_home();
    let j = h.run(&["search", "", &at(SESS), "-t", "user.*", "--format", "json"]);
    let hits: Vec<serde_json::Value> = rows(&j.stdout)
        .into_iter()
        .filter(|v| v["kind"] == "exchange")
        .flat_map(|v| v["hits"].as_array().cloned().unwrap_or_default())
        .collect();
    let rewound = hits
        .iter()
        .find(|h| h["line"] == 4)
        .expect("the rewound prompt's hit");
    assert_eq!(rewound["survival"], "abandoned", "{}", j.stdout);
    assert_eq!(rewound["label"], "user.rewound", "{}", j.stdout);
    assert_eq!(rewound["abandoned_root_line"], 4, "{}", j.stdout);
    assert!(rewound["replay_copy_of"].is_null(), "{}", j.stdout);
    let live = hits
        .iter()
        .find(|h| h["line"] == 8)
        .expect("the resend's hit");
    assert_eq!(live["survival"], "live", "{}", j.stdout);
    assert_eq!(live["label"], "user.message", "{}", j.stdout);
}

#[test]
fn a_rewind_with_no_resend_leaves_the_branch_live() {
    // Restore-conversation with nothing typed after it: the branch IS the file tail, so
    // the chain still reaches it and nothing leaves the conversation.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting the lagoon"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
        ),
    );
    let j = h.run(&["search", "", &at(SESS), "--format", "json"]);
    let s = summary(&j.stdout);
    assert_eq!(s["abandoned_records"], 0, "{}", j.stdout);
    assert_eq!(s["rewound_turns"], 0, "{}", j.stdout);
    let role = h.run(&["search", "", &at(SESS), "-t", "user"]);
    assert!(
        role.stdout.contains("dredge the northern channel"),
        "{}",
        role.stdout
    );
}

#[test]
fn a_turn_window_suppresses_abandoned_units() {
    let h = rewind_home();
    let windowed = h.run(&["search", "", &at(SESS), "--turn", "0..5"]);
    assert!(
        !windowed.stdout.contains("dredge the northern channel"),
        "a --turn window asks about NUMBERED turns:\n{}",
        windowed.stdout
    );
    assert!(
        windowed.stdout.contains("southern shoal"),
        "{}",
        windowed.stdout
    );
}
