use crate::harness::*;

#[test]
fn turns_json_header_carries_true_scope_and_rendered_and_by_kind() {
    // The JSON session_header distinguishes TRUE scope (sessions_in_scope) from rendered
    // (sessions_rendered), and carries the per-class automation_by_kind breakdown.
    let h = populated_home();
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let first = out.stdout.lines().next().unwrap_or("");
    let v: serde_json::Value = serde_json::from_str(first).expect("header json");
    assert_eq!(v["kind"], "header");
    assert!(
        v.get("sessions_in_scope").is_some(),
        "missing sessions_in_scope: {first}"
    );
    assert!(
        v.get("sessions_rendered").is_some(),
        "missing sessions_rendered: {first}"
    );
    assert_eq!(
        v["top_level_sessions"], 1,
        "targeted top-level counted: {first}"
    );
    let by = v
        .get("automation_by_kind")
        .expect("automation_by_kind present");
    for k in ["background-command", "agent", "workflow", "monitor", "task"] {
        assert!(by.get(k).is_some(), "by_kind missing class {k}: {first}");
    }
}

#[test]
fn search_finds_auq_option_descriptions_and_answer_notes_under_user() {
    let h = holes_home();
    // (1) A phrase that lives ONLY in an option's `description` must be searchable in the
    //     reconstructed USER turn (not merely in the raw assistant tool-call JSON).
    let desc = h.run(&[
        "search",
        "the conservative path that reuses existing state",
        "-t",
        "user",
        at(SESS).as_str(),
    ]);
    assert!(desc.success, "stderr: {}", desc.stderr);
    assert!(
        desc.stdout
            .contains("the conservative path that reuses existing state"),
        "option description not searchable under user:\n{}",
        desc.stdout
    );
    // (2) A phrase that lives ONLY in the answer's `annotations.notes` must be searchable
    //     under `user` — it IS the user's typed message. (Regression: previously dropped,
    //     so this returned "no matching exchanges".)
    let notes = h.run(&[
        "search",
        "more involved than a quick tweak",
        "-t",
        "user",
        at(SESS).as_str(),
    ]);
    assert!(notes.success, "stderr: {}", notes.stderr);
    assert!(
        notes.stdout.contains("more involved than a quick tweak"),
        "answer notes not searchable under user:\n{}",
        notes.stdout
    );
}

/// The shared `scope  N sessions in scope (X top-level + Y subagent)` banner is now emitted
/// by EVERY subagent-spanning text surface (list/files/search/recover/turns), not just
/// list/turns. populated_home spans 2 subagents under 1 top-level session.
#[test]
fn scope_banner_uniform_across_spanning_subcommands() {
    let h = populated_home();
    let f = h.run(&["files", at(SESS).as_str(), "--by", "file"]);
    assert!(
        f.stdout.contains("sessions in scope"),
        "files banner:\n{}",
        f.stdout
    );
    let s = h.run(&["search", "carry", at(SESS).as_str()]);
    assert!(
        s.stdout.contains("sessions in scope"),
        "search banner:\n{}",
        s.stdout
    );
    let r = h.run(&[
        "recover",
        at(SESS).as_str(),
        "--coverage",
        "--file",
        "/tmp/x",
    ]);
    assert!(
        r.stdout.contains("sessions in scope"),
        "recover banner:\n{}",
        r.stdout
    );
    let l = h.run(&["list", at(SESS).as_str()]);
    assert!(
        l.stdout.contains("sessions in scope"),
        "list banner:\n{}",
        l.stdout
    );
    // The banner is SUPPRESSED under --no-subagents (single top-level transcript).
    let f2 = h.run(&["files", at(SESS).as_str(), "--by", "file", "--no-subagents"]);
    assert!(
        !f2.stdout.contains("sessions in scope"),
        "files --no-subagents banner leaked:\n{}",
        f2.stdout
    );
}

/// The leading `{kind:"header", …}` JSON scope record is emitted by every spanning
/// subcommand's JSON, reusing turns' three span field names.
#[test]
fn scope_json_header_uniform_across_spanning_subcommands() {
    let h = populated_home();
    // Bind the `@<uuid>` target once so the vecs below can borrow it (a temporary `at(SESS)`
    // inside the array literal would be dropped before `h.run` borrows it).
    let at_sess = at(SESS);
    for args in [
        vec!["list", at_sess.as_str(), "--format", "json"],
        vec![
            "files",
            at_sess.as_str(),
            "--by",
            "file",
            "--format",
            "json",
        ],
        vec!["search", "carry", at_sess.as_str(), "--format", "json"],
        vec![
            "recover",
            at_sess.as_str(),
            "--coverage",
            "--file",
            "/tmp/x",
            "--format",
            "json",
        ],
    ] {
        let out = h.run(&args);
        assert!(out.success, "{:?} stderr: {}", args, out.stderr);
        let first = out.stdout.lines().find(|l| !l.trim().is_empty()).unwrap();
        let v: serde_json::Value = serde_json::from_str(first).unwrap();
        assert_eq!(
            v.get("kind").and_then(|k| k.as_str()),
            Some("header"),
            "{:?} first JSON line is not a session_header:\n{}",
            args,
            out.stdout
        );
        assert!(
            v.get("sessions_in_scope").is_some(),
            "{:?} header span",
            args
        );
        assert!(
            v.get("top_level_sessions").is_some(),
            "{:?} header span",
            args
        );
        assert!(
            v.get("subagent_sessions").is_some(),
            "{:?} header span",
            args
        );
    }
}

#[test]
fn search_surfaces_extractable_image_ids_on_a_hit() {
    // A `search` hit on a message that carries images must expose the SAME extractable ids as
    // `turns`/`image` — so a search result feeds straight into `csift image --id` with no
    // manual L+i assembly. r2 ("two more") carries a jpeg + a png at line 3 → L3i1, L3i2.
    let h = image_home();
    let out = h.run(&["search", "two more", at(SESS).as_str(), "--no-subagents"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("[2 images: L3i1, L3i2]"),
        "image-id suffix on the hit line:\n{}",
        out.stdout
    );
    // The JSON envelope carries the ids array on the hit too.
    let j = h.run(&[
        "search",
        "two more",
        at(SESS).as_str(),
        "--no-subagents",
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let hit = j
        .stdout
        .lines()
        .find(|l| l.contains("\"image_ids\"") && l.contains("L3i1"))
        .expect("a hit object carrying image_ids");
    assert!(hit.contains("L3i2"), "both ids present: {hit}");
}

#[test]
fn path_collision_does_not_leak_sibling_sessions_or_subagents() {
    // Two DIFFERENT cwds that encode to the SAME projects dir (§2.1 lossy collision):
    //   /Users/testuser/Projects/foo-bar   (a literal '-')
    //   /Users/testuser/Projects/foo_bar   (a '_'→'-')
    // both → -Users-testuser-Projects-foo-bar. CC stores both projects' sessions there;
    // csift must NOT leak the sibling's sessions (or its subagents) when you target one path.
    let h = Home::new();
    let enc = "-Users-testuser-Projects-foo-bar";
    let sess_a = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let sess_b = "0b1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";
    let rec = |sess: &str, cwd: &str, body: &str| {
        serde_json::json!({
            "type":"user","uuid":"u0","sessionId":sess,"cwd":cwd,
            "version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z",
            "message":{"role":"user","content":body}
        })
        .to_string()
            + "\n"
    };
    // session A: cwd .../foo-bar ; session B (the colliding sibling): cwd .../foo_bar
    h.write(
        &format!("{enc}/{sess_a}.jsonl"),
        &rec(sess_a, "/Users/testuser/Projects/foo-bar", "i am session A"),
    );
    h.write(
        &format!("{enc}/{sess_b}.jsonl"),
        &rec(
            sess_b,
            "/Users/testuser/Projects/foo_bar",
            "i am session B sibling",
        ),
    );
    // B also spawned a subagent (lives under B's sidecar in the SAME shared dir).
    h.write(
        &format!("{enc}/{sess_b}/subagents/agent-bbb999.jsonl"),
        &(serde_json::json!({
            "type":"user","isSidechain":true,"agentId":"bbb999","timestamp":"2026-06-07T05:00:01.000Z",
            "message":{"role":"user","content":"sibling B subagent work"}
        })
        .to_string()
            + "\n"),
    );

    // Target the REAL path of A → must see ONLY A, never B or B's subagent.
    let out = h.run(&["list", "/Users/testuser/Projects/foo-bar"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(sess_a),
        "session A must be found:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains(sess_b) && !out.stdout.contains("bbb999"),
        "COLLISION LEAK: sibling B / its subagent must NOT appear:\n{}",
        out.stdout
    );

    // Targeting the sibling's real path → only B (and B's subagent surfaces in search).
    // (Which sessions matched is read off the `--format json` records' `session_id`.)
    let out_b = h.run(&[
        "search",
        "",
        "/Users/testuser/Projects/foo_bar",
        "-t",
        "user",
        "--format",
        "json",
    ]);
    assert!(out_b.success, "stderr: {}", out_b.stderr);
    assert!(out_b.stdout.contains(sess_b) || out_b.stdout.contains("bbb999"));
    assert!(
        !out_b.stdout.contains(sess_a),
        "A must not leak into B's scope:\n{}",
        out_b.stdout
    );

    // The EXPLICIT encoded-dir token is the user's chosen scope → NOT cwd-filtered (both show).
    let both = h.run(&["list", enc]);
    assert!(both.success, "stderr: {}", both.stderr);
    assert!(
        both.stdout.contains(sess_a) && both.stdout.contains(sess_b),
        "an explicit encoded-dir token must show the whole dir:\n{}",
        both.stdout
    );
}

#[test]
fn search_finds_unresolved_askuserquestion_via_sidecar() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Which branch should I target?"
            )
        ),
    );

    // TEXT — the pending AUQ is found and marked `(elicitation sidecar)` (no fake Lnnnn).
    let out = h.run(&["search", "Which branch should I target", &at(SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("(elicitation sidecar)"),
        "a sidecar hit must render `(elicitation sidecar)`, not Lnnnn:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("with elicitation sidecar"),
        "the merged-records note must appear:\n{}",
        out.stdout
    );

    // JSON — the hit carries source:"elicitation-sidecar", null line; summary flags it.
    let j = h.run(&[
        "search",
        "Which branch should I target",
        &at(SESS),
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let lines: Vec<&str> = j.stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    let ex = json_rows(&j.stdout, "exchange").remove(0);
    let hit = &ex["hits"][0];
    assert_eq!(hit["source"], "elicitation-sidecar");
    assert!(
        hit["line"].is_null(),
        "no fabricated line for a sidecar hit: {hit}"
    );
    let summary: serde_json::Value = serde_json::from_str(lines.last().unwrap()).unwrap();
    assert_eq!(summary["with_elicitation_sidecar"], true);
}

#[test]
fn malformed_sidecar_line_is_skipped_and_counted_in_search() {
    let h = sidecar_session_home();
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &format!(
            "BROKEN {{ not json\n{}\n",
            auq_pending_line(
                "toolu_AQ1",
                "2026-06-27T01:02:03.000Z",
                "Which branch should I target?"
            )
        ),
    );
    let j = h.run(&[
        "search",
        "Which branch should I target",
        &at(SESS),
        "--format",
        "json",
    ]);
    assert!(j.success, "stderr: {}", j.stderr);
    let summary: serde_json::Value = serde_json::from_str(
        j.stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .next_back()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        summary["skipped_lines"], 1,
        "a malformed sidecar line is counted, never silent"
    );
    assert_eq!(summary["matched"], 1, "the valid pending still merges");
}

#[test]
fn acceptance_compaction_summary_and_boundary_searchable() {
    // §D6 the isCompactSummary record is a `type:"user"` record → searchable as
    // `harness.compaction.summary`. §D7 (user-reversed): the `compact_boundary` is a `type:"system"`
    // record NOW ALSO search-surfaced — the §7 prefilter keeps it (one memmem on `compact_boundary`)
    // and `record_raw_text` renders its top-level content + compactMetadata as the match/excerpt, so
    // compaction points can be enumerated + inspected.
    let h = acceptance_home();

    let summary = acc(&h, "zzsummary", "harness.compaction.summary");
    assert!(summary.success, "D6: stderr {}", summary.stderr);
    assert!(
        summary.stdout.contains("harness.compaction.summary"),
        "D6 compaction summary → searchable:\n{}",
        summary.stdout
    );

    let boundary = acc(&h, "zzboundary", "harness.compaction.boundary");
    assert!(boundary.success, "D7: stderr {}", boundary.stderr);
    assert!(
        boundary.stdout.contains("harness.compaction.boundary"),
        "D7 compact_boundary → now searchable:\n{}",
        boundary.stdout
    );
    // The compactMetadata renders as the excerpt (trigger / pre/post tokens / duration).
    assert!(
        boundary.stdout.contains("trigger") && boundary.stdout.contains("auto"),
        "D7 boundary excerpt carries its compactMetadata:\n{}",
        boundary.stdout
    );
}

#[test]
fn acceptance_excluded_and_unmarked_meta_carry_no_label() {
    // §E an `attachment` carries no label; §J an isMeta record matching no harness marker is EXCLUDED
    // (never `user.message`). Neither surfaces under ANY selector.
    let h = acceptance_home();
    for (oracle, token) in [
        ("E attachment", "zzattach"),
        ("J isMeta-unmarked", "zzunmarked"),
    ] {
        // No `-t` → every label eligible; still nothing, because classify returns empty.
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
fn acceptance_mcp_elicitation_searchable_under_tool_use() {
    // §G8 — an MCP elicitation (UNATTESTED in the corpus → synthetic sidecar). The pending marker is a
    // `type:"system"` record with no tool_use block; the guarded §3.10 arm classifies it
    // `agent.tool.use` and matches its content, so `-t agent.tool.use` finds it, rendered
    // `(elicitation sidecar)` (no fabricated L) with the `with elicitation sidecar` note.
    let h = Home::new();
    h.write(
        &format!("{ACC_ENC}/{ACC_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"/Users/x/acc","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"kick off the mcp flow"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"working"}]}}"#, "\n",
        ),
    );
    h.write(
        &format!("{ACC_ENC}/{ACC_SESS}/elicitations.jsonl"),
        concat!(
            r#"{"type":"system","subtype":"mcp_elicitation","uuid":"m-mcp1","timestamp":"2026-06-07T06:00:00.000Z","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","isSidechain":false,"content":"MCP elicitation [github] (url): zzmcp confirm the action","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"mcp1","csiftMcpServer":"github","hookInput":{}}"#, "\n",
        ),
    );
    let out = h.run(&["search", "zzmcp", "-t", "agent.tool.use", &at(ACC_SESS)]);
    assert!(out.success, "G8: stderr {}", out.stderr);
    assert!(
        out.stdout.contains("agent.tool.use"),
        "G8 MCP pending marker → agent.tool.use:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(elicitation sidecar)")
            && out.stdout.contains("with elicitation sidecar"),
        "G8 must render the sidecar provenance:\n{}",
        out.stdout
    );
}

#[test]
fn additional_context_is_invisible_by_default() {
    // The default scan never parses attachment lines: a pattern that lives only in the
    // hook-injected context is a DEFINITIVE absence (exit 0), not a hit.
    let h = Home::new();
    hook_context_scenario(&h);
    let out = h.run(&["search", "quartzlantern", &at(HOOKCTX_SESS)]);
    assert!(out.success, "zero-match exits 0: {}", out.stderr);
    assert!(
        out.stdout.contains("no matching exchanges"),
        "default scan must not see hook context:\n{}",
        out.stdout
    );
}
