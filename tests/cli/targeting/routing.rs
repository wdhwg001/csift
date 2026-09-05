//! The teammate ROUTING form `@<Name>@<Team>` as a target: resolution, collisions, misses.

use crate::harness::*;

const ENC_R: &str = "-Users-dev-Projects-relay";
const SESS_R: &str = "00000000-0000-4000-8000-000000000001";
const RELAY_ID: &str = "aRelay-0123456789abcdef";
const TWIN_ID: &str = "aRelay-fedcba9876543210";

/// A session owning the teammate `Relay` of team `harbor`. With `twin`, a SECOND teammate
/// carrying the same name and team - the collision the routing form cannot resolve, and the
/// transcript form always can.
fn relay_home(twin: bool) -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC_R}/{SESS_R}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the relay"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_relay","name":"Agent","input":{"description":"relay work","subagent_type":"general-purpose","name":"Relay"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{SESS_R}/subagents/agent-{RELAY_ID}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aRelay-0123456789abcdef","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"ROUTINGPROBE relay the throttle beacon"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"beacon relayed"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{SESS_R}/subagents/agent-{RELAY_ID}.meta.json"),
        r#"{"agentType":"Relay","description":"relay work","name":"Relay","taskKind":"in_process_teammate","teamName":"harbor"}"#,
    );
    if twin {
        h.write(
            &format!("{ENC_R}/{SESS_R}/subagents/agent-{TWIN_ID}.jsonl"),
            concat!(
                r#"{"type":"user","isSidechain":true,"agentId":"aRelay-fedcba9876543210","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":"ROUTINGTWIN the second relay"}}"#, "\n",
                r#"{"type":"assistant","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"second beacon relayed"}]}}"#, "\n",
            ),
        );
        h.write(
            &format!("{ENC_R}/{SESS_R}/subagents/agent-{TWIN_ID}.meta.json"),
            r#"{"agentType":"Relay","description":"relay work","name":"Relay","taskKind":"in_process_teammate","teamName":"harbor"}"#,
        );
    }
    h
}

#[test]
fn routing_form_targets_the_teammate_transcript() {
    // `@Name@Team` is the id the official SendMessage takes; csift accepts it as a target and
    // resolves it to the transcript form, so a caller holding only the routing id can read the
    // lane it addresses.
    let h = relay_home(false);
    let s = h.run(&["search", "ROUTINGPROBE", "@Relay@harbor"]);
    assert!(s.success, "search @Relay@harbor: {}", s.stderr);
    assert!(
        s.stdout.contains("ROUTINGPROBE"),
        "search did not reach the teammate transcript: {}",
        s.stdout
    );
    let sh = h.run(&["show", "@Relay@harbor", "--line", "1"]);
    assert!(sh.success, "show @Relay@harbor: {}", sh.stderr);
    assert!(
        sh.stdout.contains("ROUTINGPROBE"),
        "show fetched the wrong transcript: {}",
        sh.stdout
    );
    // `list` names the resolved TRANSCRIPT id, so the two forms are joinable from one call.
    let l = h.run(&["list", "@Relay@harbor"]);
    assert!(l.success, "list @Relay@harbor: {}", l.stderr);
    assert!(
        l.stdout.contains(RELAY_ID),
        "list did not name the transcript id: {}",
        l.stdout
    );
}

#[test]
fn routing_form_matches_name_and_team_exactly() {
    // Claude Code interpolates the requested name and the raw team name into the routing id
    // and folds no case, so a case-differing token names a DIFFERENT teammate: it must miss
    // loudly rather than resolve to this one.
    let h = relay_home(false);
    for token in ["@relay@harbor", "@Relay@Harbor", "@Relay@region"] {
        let out = h.run(&["show", token, "--line", "1"]);
        assert!(!out.success, "{token} must not resolve");
        assert!(
            out.stderr.contains("no teammate") && out.stderr.contains("ROUTING form"),
            "{token} error must name the grammar; stderr: {}",
            out.stderr
        );
        assert!(
            out.stderr.contains("--shape teammate"),
            "{token} error must name the discovery command; stderr: {}",
            out.stderr
        );
    }
}

#[test]
fn routing_form_ignores_a_non_teammate_agent_of_the_same_name() {
    // The gate is the meta's `taskKind:"in_process_teammate"`, not the name: a built-in Task
    // subagent that happens to carry the same `name` has no routing id at all.
    let h = Home::new();
    h.write(
        &format!("{ENC_R}/{SESS_R}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{SESS_R}/subagents/agent-{RELAY_ID}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aRelay-0123456789abcdef","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"ROUTINGPROBE plain task subagent"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{SESS_R}/subagents/agent-{RELAY_ID}.meta.json"),
        r#"{"agentType":"general-purpose","name":"Relay","teamName":"harbor","toolUseId":"toolu_relay"}"#,
    );
    let out = h.run(&["show", "@Relay@harbor", "--line", "1"]);
    assert!(
        !out.success,
        "a non-teammate must not answer a routing form"
    );
    assert!(out.stderr.contains("no teammate"), "stderr: {}", out.stderr);
    // The transcript form still reaches it - only the routing form is teammate-only.
    let byid = h.run(&["show", &at(RELAY_ID), "--line", "1"]);
    assert!(byid.success, "show @<agent-id>: {}", byid.stderr);
    assert!(byid.stdout.contains("ROUTINGPROBE"), "{}", byid.stdout);
}

#[test]
fn routing_form_collision_bails_listing_every_transcript_id() {
    // Two live teammates can share one routing id (the spawn de-duplicates only its own
    // name-to-id registry key), so csift never picks: it names both transcript ids, which is
    // exactly what the caller needs to address one of them.
    let h = relay_home(true);
    let out = h.run(&["show", "@Relay@harbor", "--line", "1"]);
    assert!(!out.success, "an ambiguous routing form must bail");
    assert!(
        out.stderr.contains("AMBIGUOUS")
            && out.stderr.contains(RELAY_ID)
            && out.stderr.contains(TWIN_ID),
        "the ambiguity must list every matching transcript id; stderr: {}",
        out.stderr
    );
    // Each transcript id still resolves on its own - the collision is the routing form's.
    let one = h.run(&["show", &at(TWIN_ID), "--line", "1"]);
    assert!(one.success, "show @<twin id>: {}", one.stderr);
    assert!(one.stdout.contains("ROUTINGTWIN"), "{}", one.stdout);
}

#[test]
fn a_routing_form_shared_across_two_sessions_is_ambiguous_too() {
    // The routing form is scoped to whatever the command is scoped to, so a name+team that is
    // unique inside one session can still collide across the corpus. Nothing about the form
    // makes it a session-local id, so the ambiguity is the same ambiguity - and the answer is
    // still both transcript ids, never a pick.
    let h = relay_home(false);
    let second = "00000000-0000-4000-8000-000000000002";
    let other_id = "aRelay-00112233445566aa";
    h.write(
        &format!("{ENC_R}/{second}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"start the second relay"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{second}/subagents/agent-{other_id}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aRelay-00112233445566aa","timestamp":"2026-06-07T06:00:02.000Z","message":{"role":"user","content":"ROUTINGOTHER the other session's relay"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{second}/subagents/agent-{other_id}.meta.json"),
        r#"{"agentType":"Relay","description":"relay work","name":"Relay","taskKind":"in_process_teammate","teamName":"harbor"}"#,
    );

    let out = h.run(&["show", "@Relay@harbor", "--line", "1"]);
    assert!(!out.success, "two sessions, one routing id: {}", out.stdout);
    assert!(
        out.stderr.contains("AMBIGUOUS")
            && out.stderr.contains(RELAY_ID)
            && out.stderr.contains(other_id),
        "both sessions' transcript ids are listed; stderr: {}",
        out.stderr
    );
    // Naming another session alongside it does NOT disambiguate: `@` targets are a UNION, so
    // each positional resolves on its own and the routing form still has two answers. The way
    // out is the one the error gives - address a transcript id, which never collides.
    let still = h.run(&["search", "ROUTINGPROBE", &at(SESS_R), "@Relay@harbor"]);
    assert!(!still.success, "a sibling target narrows nothing");
    assert!(still.stderr.contains("AMBIGUOUS"), "{}", still.stderr);
    let byid = h.run(&["search", "ROUTINGOTHER", &at(other_id)]);
    assert!(byid.success, "stderr: {}", byid.stderr);
    assert!(byid.stdout.contains("ROUTINGOTHER"), "{}", byid.stdout);
}

#[test]
fn a_teammate_meta_with_no_transcript_is_not_a_teammate_in_scope() {
    // The index is built from the transcripts that exist, each joined to its meta: a meta left
    // behind by a lane whose transcript was pruned names nothing csift could read, so it must
    // miss loudly rather than resolve to a file that is not there.
    let h = Home::new();
    h.write(
        &format!("{ENC_R}/{SESS_R}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC_R}/{SESS_R}/subagents/agent-{RELAY_ID}.meta.json"),
        r#"{"agentType":"Relay","description":"relay work","name":"Relay","taskKind":"in_process_teammate","teamName":"harbor"}"#,
    );
    let out = h.run(&["show", "@Relay@harbor", "--line", "1"]);
    assert!(!out.success, "a meta alone is not a lane: {}", out.stdout);
    assert!(
        out.stderr.contains("no teammate `Relay@harbor`") && out.stderr.contains("ROUTING form"),
        "the miss is the zero-match error, not a read failure; stderr: {}",
        out.stderr
    );
    // The transcript form misses just as loudly, and for the same reason.
    let byid = h.run(&["show", &at(RELAY_ID), "--line", "1"]);
    assert!(!byid.success, "{}", byid.stdout);
}

#[test]
fn the_sessions_from_id_list_takes_transcript_ids_only() {
    // `--sessions-from` is a machine-fed list (it consumes `search -l`), and every id csift
    // emits into one is a transcript id. The routing form CAN collide, so admitting it here
    // would let a list resolve to a different lane than the one that produced it: the list
    // refuses it by name and says which shapes it takes.
    let h = relay_home(false);
    let list = h.root.join("ids.txt");
    std::fs::write(&list, "@Relay@harbor\n").unwrap();
    let out = h.run(&["list", "--sessions-from", list.to_str().unwrap()]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("--sessions-from")
            && out.stderr.contains("not a session id")
            && out.stderr.contains("agent id"),
        "the refusal names the accepted shapes; stderr: {}",
        out.stderr
    );
    // The transcript form the routing id resolves to is what the list takes.
    std::fs::write(&list, format!("{RELAY_ID}\n")).unwrap();
    let ok = h.run(&["list", "--sessions-from", list.to_str().unwrap()]);
    assert!(ok.success, "stderr: {}", ok.stderr);
    assert!(ok.stdout.contains(RELAY_ID), "{}", ok.stdout);
    // ...and the routing form stays a POSITIONAL target, which is where it resolves.
    let positional = h.run(&["list", "@Relay@harbor"]);
    assert!(positional.success, "stderr: {}", positional.stderr);
    assert!(
        positional.stdout.contains(RELAY_ID),
        "{}",
        positional.stdout
    );
}

#[test]
fn malformed_routing_shapes_stay_with_the_at_grammar_error() {
    // A second `@`, an empty half or a character outside the name charset is NOT a routing
    // form: it falls to the @-grammar error, which now names the routing shape too.
    let h = relay_home(false);
    for token in ["@Relay@har@bor", "@Relay@", "@Relay@har.bor"] {
        let out = h.run(&["show", token, "--line", "1"]);
        assert!(!out.success, "{token} must not resolve");
        assert!(
            out.stderr.contains("not a recognized @-target")
                && out.stderr.contains("@<name>@<team>"),
            "{token} must hit the @-grammar error naming the routing form; stderr: {}",
            out.stderr
        );
    }
    // Without the `@` sigil the token is not a path either - say the exact fix.
    let bare = h.run(&["show", "Relay@harbor", "--line", "1"]);
    assert!(!bare.success, "a bare routing form must not be a path");
    assert!(
        bare.stderr.contains("looks like a session/agent id")
            && bare.stderr.contains("@<name>@<team>"),
        "stderr: {}",
        bare.stderr
    );
}
