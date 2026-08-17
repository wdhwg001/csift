//! The @-token grammar end to end: uuids, prefixes, agent ids, @main, @trap, encoded dirs.

use crate::harness::*;

#[test]
fn dashed_teammate_name_id_round_trips_as_target() {
    // A teammate NAME may carry dashes (real data: teammate "P1-engine" → agent id
    // `aP1-engine-9cf2f06d6235ca64`). The id `csift agents` prints must round-trip as an
    // `@<agent-id>` target - it used to fall through to the project-dir branch and fail.
    let enc = "-Users-testuser-Projects-dashmate";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let id = "aP1-engine-9cf2f06d6235ca64";
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Agent","input":{"name":"P1-engine","subagent_type":"executor","description":"do it"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{id}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aP1-engine-9cf2f06d6235ca64","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"teammate: probe the dashy widget"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"probed"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{id}.meta.json"),
        r#"{"agentType":"P1-engine","name":"P1-engine","taskKind":"in_process_teammate"}"#,
    );
    let l = h.run(&["list", &format!("@{id}")]);
    assert!(l.success, "list @dashed-teammate-id: {}", l.stderr);
    assert!(
        l.stdout.contains("SUBAGENT") && l.stdout.contains(id),
        "the subagent banner names the teammate id: {}",
        l.stdout
    );
    let s = h.run(&["show", &format!("@{id}"), "--line", "1"]);
    assert!(s.success, "show @dashed-teammate-id: {}", s.stderr);
    assert!(
        s.stdout.contains("teammate: probe the dashy widget"),
        "show fetches from the teammate transcript: {}",
        s.stdout
    );
}

#[test]
fn at_agent_hex_subtree_includes_descendants_unless_no_subagents() {
    // The rule: locating an AGENT → itself (--no-subagents), else itself + ALL topological
    // descendants. Build a nested pair (PARENT spawns CHILD) flat on disk, linked via the
    // child's meta toolUseId pointing at the Agent tool_use recorded in PARENT's transcript.
    let enc = "-Users-testuser-Projects-agtree";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let parent = "aaaa1111bbbb2222c"; // 17 hex
    let child = "cccc3333dddd4444e"; // 17 hex
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_parent","name":"Agent","input":{"description":"parent"}}]}}"#, "\n",
        ),
    );
    // PARENT spawns CHILD (the Agent tool_use is recorded HERE).
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{parent}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaaa1111bbbb2222c","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"PARENTWORK"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call_child","name":"Agent","input":{"description":"child"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{parent}.meta.json"),
        r#"{"agentType":"general-purpose","toolUseId":"call_parent"}"#,
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{child}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"cccc3333dddd4444e","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":"CHILDWORK"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{child}.meta.json"),
        r#"{"agentType":"Explore","toolUseId":"call_child"}"#,
    );

    // Default `@<parent>` → parent + descendant child both searchable.
    let full = h.run(&["search", "WORK", &format!("@{parent}")]);
    assert!(full.success, "stderr: {}", full.stderr);
    assert!(
        full.stdout.contains("PARENTWORK"),
        "parent in scope: {}",
        full.stdout
    );
    assert!(
        full.stdout.contains("CHILDWORK"),
        "descendant child in scope: {}",
        full.stdout
    );

    // `--no-subagents` → the parent agent ALONE (child excluded).
    let alone = h.run(&["search", "WORK", &format!("@{parent}"), "--no-subagents"]);
    assert!(alone.success, "stderr: {}", alone.stderr);
    assert!(
        alone.stdout.contains("PARENTWORK"),
        "parent still in scope: {}",
        alone.stdout
    );
    assert!(
        !alone.stdout.contains("CHILDWORK"),
        "child EXCLUDED under --no-subagents: {}",
        alone.stdout
    );
}

#[test]
fn cross_surface_session_id_is_identical_for_a_subagent() {
    // id-form unification: the SAME subagent transcript reports the SAME bare-hex
    // session_id from files, search, and turns (search/turns previously kept `agent-`).
    let h = populated_home();
    let files = h.run(&[
        "files",
        at(SESS).as_str(),
        "--by",
        "file",
        "--format",
        "json",
    ]);
    let search = h.run(&["search", "", ENC, at(SESS).as_str(), "--format", "json"]);
    // turns now defaults to top-level-only, so opt INTO spanning subagents to exercise the
    // cross-surface id-form check on the turns surface too.
    let turns = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "8000",
        "--format",
        "json",
    ]);
    // The bare-hex subagent id (no `agent-` prefix) must appear in each surface's JSON,
    // and the `agent-` prefixed form must NOT.
    for (name, out) in [("files", &files), ("search", &search), ("verbatim", &turns)] {
        assert!(out.success, "{name} stderr: {}", out.stderr);
        assert!(
            !out.stdout.contains("\"agent-aaa111\"") && !out.stdout.contains("agent-aaa111"),
            "{name} leaked an agent- prefixed session_id: {}",
            out.stdout
        );
    }
    // At least one surface must actually mention the bare id (proves the subagent was
    // scanned, not just that the prefix is absent).
    assert!(
        files.stdout.contains("aaa111") || turns.stdout.contains("aaa111"),
        "no surface emitted the bare subagent id; files={} turns={}",
        files.stdout,
        turns.stdout
    );
}

#[test]
fn at_agent_hex_scopes_to_the_subtree() {
    // `@<agent-hex>` now SCOPES to that subagent (+ its topological descendants, unless
    // --no-subagents), per the rule "locating an agent: itself, or itself + descendants".
    // A realistic >=12-char hex is needed (a <=11-char token is a uuid PREFIX, not an agent).
    let enc = "-Users-testuser-Projects-agentscope";
    let sess = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let hex = "aaa111bbb222ccc33"; // 17 hex, like real agent ids
    let h = Home::new();
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"call0","name":"Agent","input":{"description":"do it"}}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{enc}/{sess}/subagents/agent-{hex}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"aaa111bbb222ccc33","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"sub: the WIDGET work"}}"#, "\n",
            r#"{"type":"assistant","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"sub done"}]}}"#, "\n",
        ),
    );

    // `search` within `@<agent-hex>` finds content in THAT subagent transcript.
    let s = h.run(&["search", "WIDGET", &format!("@{hex}")]);
    assert!(s.success, "stderr: {}", s.stderr);
    assert!(
        s.stdout.contains(hex),
        "scoped to the subagent: {}",
        s.stdout
    );
    assert!(
        s.stdout.contains("subagent"),
        "branded as a subagent: {}",
        s.stdout
    );

    // A NON-EXISTENT agent hex → honest "no subagent found" (not a session error).
    let miss = h.run(&["search", "x", "@deadbeefdeadbeef0"]);
    assert!(!miss.success);
    assert!(
        miss.stderr.contains("no subagent") && miss.stderr.contains("agents"),
        "guides to agents listing: {}",
        miss.stderr
    );
}

#[test]
fn agent_twelve_hex_fallback_ambiguity_fails_loud() {
    // Mutation pin: the 12+-hex exact-miss prefix FALLBACK has its own ambiguity guard -
    // two agents sharing 12 leading hex chars must produce the AMBIGUOUS error naming
    // both ids, never the generic no-subagent miss (and never a silent pick).
    let h = Home::new();
    let enc = "-Users-dev-example-project";
    let sess = "55667788-9900-4000-8000-00000000000a";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"lead"}}"#, "\n",
        ),
    );
    let twin_a = "abcd1234abcd1234aaaa";
    let twin_b = "abcd1234abcd1234bbbb";
    for (agent, word) in [(twin_a, "TWINALPHA"), (twin_b, "TWINBETA")] {
        h.write(
            &format!("{enc}/{sess}/subagents/agent-{agent}.jsonl"),
            &format!(
                "{}\n",
                format_args!(
                    r#"{{"type":"user","isSidechain":true,"agentId":"{agent}","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"user","content":"seed {word}"}}}}"#
                ),
            ),
        );
    }
    // 16 shared leading hex chars -> the 16-char token is an exact-miss AND a 2-way prefix.
    let ambi = h.run(&["search", "TWIN", &format!("@{}", &twin_a[..16])]);
    assert!(!ambi.success, "ambiguous fallback must error");
    assert!(
        ambi.stderr.contains("AMBIGUOUS")
            && ambi.stderr.contains(twin_a)
            && ambi.stderr.contains(twin_b),
        "the fallback names both candidates: {}",
        ambi.stderr
    );
}

// An UNRECOGNIZED `@`-shape must fail loud naming the @-grammar - never strip the `@` and
// fall through to cwd-relative path resolution (the old behavior sent an ID typo down a
// misleading "no Claude Code project dir" filesystem trail).
#[test]
fn at_token_unrecognized_shapes_fail_loud_never_path_fallthrough() {
    let h = populated_home();
    // 1-3 dashless hex chars: below the 4-char uuid-prefix minimum → the dedicated message.
    for tok in ["@a", "@22", "@224"] {
        let out = h.run(&["list", tok]);
        assert!(!out.success, "{tok} must hard-error: {}", out.stdout);
        assert!(
            out.stderr.contains("too short") && out.stderr.contains("4-11"),
            "{tok} names the prefix minimum: {}",
            out.stderr
        );
        assert!(
            !out.stderr.contains("no Claude Code project dir"),
            "{tok} must not fall through to path resolution: {}",
            out.stderr
        );
    }
    // Non-hex / dashed / empty tokens → the general @-grammar error.
    for tok in ["@notanid", "@1234-ab", "@"] {
        let out = h.run(&["list", tok]);
        assert!(!out.success, "{tok} must hard-error: {}", out.stdout);
        assert!(
            out.stderr.contains("not a recognized @-target"),
            "{tok} names the grammar: {}",
            out.stderr
        );
    }
    // The `@-Users-…` encoded-dir spelling STILL resolves (encoded cwds lead with `-`).
    let enc = h.run(&["list", &format!("@{ENC}")]);
    assert!(enc.success, "stderr: {}", enc.stderr);
    assert!(
        enc.stdout.contains(SESS),
        "lists the fixture session: {}",
        enc.stdout
    );
}

#[test]
fn windows_drive_encoded_dir_targets_resolve() {
    // A Windows cwd (`C:\Users\dev\winproj`) encodes to a DRIVE-LETTER-led projects dir
    // (`C--Users-dev-winproj` - verbatim from CC's sanitizer), which leads with a letter,
    // not `-`. Both target forms must resolve it: the bare positional token and the
    // `@`-prefixed form.
    let h = Home::new();
    let enc = "C--Users-dev-winproj";
    let sess = "99aabbcc-ddee-4000-8000-00000000000e";
    h.write(
        &format!("{enc}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"win work"}}"#, "\n",
        ),
    );
    let bare = h.run(&["list", enc]);
    assert!(bare.success, "stderr: {}", bare.stderr);
    assert!(
        bare.stdout.contains(sess),
        "bare drive-encoded token resolves: {}",
        bare.stdout
    );
    let at_form = h.run(&["list", &format!("@{enc}")]);
    assert!(at_form.success, "stderr: {}", at_form.stderr);
    assert!(
        at_form.stdout.contains(sess),
        "@-prefixed drive-encoded token resolves: {}",
        at_form.stdout
    );
}

#[test]
fn target_jsonl_file_path_scopes_to_session() {
    // THE marquee fix: an LLM that has the session's transcript PATH (from ls/find) passes it
    // directly. Before, csift re-encoded the whole path into a bogus dir and errored.
    let h = populated_home();
    let jsonl = h.projects().join(format!("{ENC}/{SESS}.jsonl"));
    let out = h.run(&["search", "carry", jsonl.to_str().unwrap(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(&format!("{}\u{b7}t", &SESS[..8])),
        "scoped to the session: {}",
        out.stdout
    );
    // A non-existent jsonl target errors honestly (never fabricates a dir).
    let bad = h.run(&["search", "carry", "/no/such/session.jsonl"]);
    assert!(!bad.success);
    assert!(
        bad.stderr.contains("no session transcript at"),
        "honest missing-file error: {}",
        bad.stderr
    );
}

#[test]
fn target_at_uuid_routes_to_session() {
    // `@<uuid>` is the explicit session-id target (the form that will replace --session).
    let h = populated_home();
    let out = h.run(&["agents", &format!("@{SESS}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("builtin-task"),
        "resolved the session: {}",
        out.stdout
    );
}

#[test]
fn target_at_main_resolves_env_session() {
    // `@main` resolves the calling session from CLAUDE_CODE_SESSION_ID.
    let h = populated_home();
    let out = h.run_with_env(&["agents", "@main"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("builtin-task"),
        "@main → env session: {}",
        out.stdout
    );
    // @main with no env errors with guidance.
    let no_env = h.run(&["agents", "@main"]);
    assert!(!no_env.success);
    assert!(
        no_env.stderr.contains("CLAUDE_CODE_SESSION_ID is not set"),
        "guidance when env absent: {}",
        no_env.stderr
    );
}

#[test]
fn target_at_encoded_dir_resolves() {
    // `@<encoded-dir>` names a project dir by its encoded form directly.
    let h = populated_home();
    let out = h.run(&["agents", &format!("@{ENC}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("builtin-task"),
        "@encoded resolved: {}",
        out.stdout
    );
}

#[test]
fn target_at_uuid_prefix_resolves_unique_and_errors_on_ambiguity() {
    // `@<first-segment>` (the emergent shorthand): a short hex prefix resolves the UNIQUE
    // session whose uuid starts with it, and errors (never silently picks) when ambiguous.
    let h = Home::new();
    // Two sessions sharing the 8-hex first segment `0a1b2c3d`, and one distinct (`deadbeef`).
    let a = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let b = "0a1b2c3d-ffff-4a6b-8c7d-9e0f1a2b3c4d";
    let c = "deadbeef-1111-2222-3333-444455556666";
    for s in [a, b, c] {
        h.write(
            &format!("{ENC}/{s}.jsonl"),
            &format!(
                "{{\"type\":\"user\",\"sessionId\":\"{s}\",\"cwd\":\"/p\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}}}}\n"
            ),
        );
    }
    // A unique prefix → that one session (list shows its id).
    let uniq = h.run(&["list", "@deadbeef"]);
    assert!(uniq.success, "stderr: {}", uniq.stderr);
    assert!(
        uniq.stdout.contains(c),
        "unique prefix resolved: {}",
        uniq.stdout
    );
    assert!(
        !uniq.stdout.contains(a),
        "scoped to ONLY the matched session: {}",
        uniq.stdout
    );

    // An ambiguous prefix → error listing BOTH candidates, never a silent pick.
    let amb = h.run(&["list", "@0a1b2c3d"]);
    assert!(!amb.success, "ambiguous prefix must error: {}", amb.stdout);
    assert!(
        amb.stderr.contains("AMBIGUOUS"),
        "says ambiguous: {}",
        amb.stderr
    );
    assert!(
        amb.stderr.contains(a) && amb.stderr.contains(b),
        "lists both candidates: {}",
        amb.stderr
    );

    // A prefix nobody starts with → honest no-match.
    let none = h.run(&["list", "@99999999"]);
    assert!(!none.success);
    assert!(
        none.stderr.contains("no session or agent id starts with"),
        "{}",
        none.stderr
    );
}

#[test]
fn uuid_prefix_minimum_boundary_is_exactly_four() {
    // Mutation pin: the too-short-hex boundary is len < 4 (a 4-char prefix RESOLVES; a
    // 3-char one errors with the dedicated guidance).
    let h = Home::new();
    h.write(
        "-Users-testuser-Projects-pfx/abcd1234-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl",
        "{\"type\":\"user\",\"uuid\":\"u0\",\"timestamp\":\"2026-06-07T05:00:00.000Z\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
    );
    let ok = h.run(&["list", "@abcd"]);
    assert!(
        ok.success && ok.stdout.contains("abcd1234"),
        "4-hex prefix resolves: {}\n{}",
        ok.stderr,
        ok.stdout
    );
    let short = h.run(&["list", "@abc"]);
    assert!(
        !short.success && short.stderr.contains("too short"),
        "3-hex errors with guidance: {}",
        short.stderr
    );
}

#[test]
fn bare_id_shaped_tokens_get_the_at_hint() {
    // Mutation pin: the bare-id shape catch fires for BOTH the uuid-prefix shape and the
    // bare agent-hex shape (the || chain + the !exists guard).
    let h = Home::new();
    let p = h.run(&["list", "abcd1234"]);
    assert!(
        !p.success && p.stderr.contains("did you mean '@abcd1234'"),
        "prefix shape hint: {}",
        p.stderr
    );
    let a = h.run(&["list", "aabbccdd11223344"]);
    assert!(
        !a.success && a.stderr.contains("did you mean '@aabbccdd11223344'"),
        "agent-hex shape hint: {}",
        a.stderr
    );
}
