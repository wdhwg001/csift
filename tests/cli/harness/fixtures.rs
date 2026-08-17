use super::home::*;

/// A session whose ONLY event for the target is an edit with no preceding read → the edit
/// is un-anchorable, leaving an empty buffer and no seen_total (events non-empty, but the
/// reconstruction is empty). Used to drive the at-mode "nothing known" skip in both
/// renderers.
pub(crate) fn recover_empty_reconstruction_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#, "\n",
            // An Edit result for /p/e.rs with NO prior read → un-anchorable, no known lines.
            r#"{"type":"user","uuid":"c0","timestamp":"2026-06-07T05:00:01.000Z","toolUseResult":{"filePath":"/p/e.rs","oldString":"x","newString":"y","structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-x","+y"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ed0","content":"ok"}]}}"#, "\n",
        ),
    );
    h
}

/// Build the `turns` integration fixture: a realistic multi-compaction transcript
/// authored with LOCALE-NEUTRAL tokens only (accented-Latin + emoji - the
/// same house charset style as the recover fixtures).
///
/// Shape (all on the SINGLE top-level session jsonl so the spanning walk is exercised
/// without subagent noise; tests pass `--no-subagents`):
///   • 3 genuine round-trip turns, then a compaction SUMMARY #1 (its §6 quotes turn 0's
///     user verbatim + §9 quotes the last assistant - drives the dedup test),
///   • 3 more round-trips, then SUMMARY #2,
///   • 3 more round-trips, then SUMMARY #3,
///   • a final live block: one HUGE round-trip (user > 600 chars, assistant > 900 chars
///     → drives the role-asymmetric ellipsis test) and an assistant-heavy tail (a turn
///     whose assistant side is enormous + a pure tool-call turn) → drives the 50% floor.
/// Plus one malformed line for the skipped-line accounting.
/// A tiny jsonl builder for the turns fixture: free methods on a struct so there is no
/// closure-capture conflict (the earlier closure form double-borrowed the buffer).
pub(crate) struct TurnsBuilder {
    pub(crate) out: String,
    pub(crate) ts: u64,
}

impl TurnsBuilder {
    pub(crate) fn new() -> Self {
        TurnsBuilder {
            out: String::new(),
            ts: 0,
        }
    }

    pub(crate) fn next_ts(&mut self) -> String {
        self.ts += 1;
        let t = self.ts;
        format!(
            "2026-06-07T{:02}:{:02}:{:02}.000Z",
            t / 3600,
            (t / 60) % 60,
            t % 60
        )
    }

    pub(crate) fn line(&mut self, s: &str) {
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// A round-trip turn: user opener (+ optional N tool calls) + assistant EOT text.
    pub(crate) fn round_trip(&mut self, user: &str, asst: &str, tools: usize) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"{user}"}}}}"#
        ));
        if tools > 0 {
            let blocks: Vec<String> = (0..tools)
                .map(|i| format!(r#"{{"type":"tool_use","id":"t{i}","name":"Bash","input":{{}}}}"#))
                .collect();
            let ts = self.next_ts();
            self.line(&format!(
                r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{}]}}}}"#,
                blocks.join(",")
            ));
        }
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":"{asst}"}}]}}}}"#
        ));
    }

    /// Emit one assistant text record (one agent message in a turn's run).
    pub(crate) fn agent_text(&mut self, text: &str) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        ));
    }

    /// Emit one assistant tool_use record (drives the per-message attribution span).
    pub(crate) fn tool_use(&mut self) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"tx","name":"Bash","input":{{}}}}]}}}}"#
        ));
    }

    /// A turn with a LONG agent-message run: a user opener, then the ordered list of
    /// agent messages (each preceded by one tool_use so the placeholder Y is non-zero),
    /// to drive the richness selection. The LAST entry is the EOT.
    pub(crate) fn long_agent_run(&mut self, user: &str, agent_msgs: &[&str]) {
        let ts = self.next_ts();
        self.line(&format!(
            r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"{user}"}}}}"#
        ));
        for m in agent_msgs {
            self.tool_use();
            self.agent_text(m);
        }
    }
}

pub(crate) fn turns_fixture_jsonl() -> String {
    let mut b = TurnsBuilder::new();

    // ── Block A: 3 round-trips, then SUMMARY #1 ──
    b.round_trip(
        "the very first ask about the café budget walk logic",
        "first reply café🛠",
        2,
    );
    b.round_trip(
        "second ask explain the panic path now please",
        "second reply",
        2,
    );
    b.round_trip("third ask about the carry boundary", "third reply", 0);
    // SUMMARY #1: §6 quotes turn 0's user verbatim (dedup target), §9 quotes an assistant.
    b.line(
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued from a previous conversation.\n6. All user messages:\n   - \"the very first ask about the café budget walk logic\"\n   - \"second ask explain ...\"\n9. Optional Next Step:\n   The assistant said \"third reply\" before compaction."}}"#,
    );

    // ── Block B: 3 round-trips, then SUMMARY #2 ──
    b.round_trip(
        "fourth ask after the first compaction boundary",
        "fourth reply",
        1,
    );
    b.round_trip("fifth ask keep going", "fifth reply", 3);
    b.round_trip("sixth ask about détours", "sixth reply", 0);
    b.line(
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued.\n6. All user messages:\n   - \"fourth ask after the first compaction boundary\"\n9. Optional Next Step:\n   The assistant said \"sixth reply\"."}}"#,
    );

    // ── Block C: 3 round-trips, then SUMMARY #3 ──
    b.round_trip("seventh ask post second boundary", "seventh reply", 2);
    b.round_trip("eighth ask almost there", "eighth reply", 1);
    b.round_trip("ninth ask wrap it up", "ninth reply", 0);
    b.line(
        r#"{"type":"user","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"message":{"role":"user","content":"This session is being continued.\n6. All user messages:\n   - \"seventh ask post second boundary\"\n9. Optional Next Step:\n   The assistant said \"ninth reply\"."}}"#,
    );

    // ── Block D (live region, after the newest summary) ──
    // A LONG agent-message run in ONE turn (8 agent messages > the default >6 threshold):
    // a rich first, pure-declaration middles, a sudden rich middle, a FUSED finding+decl
    // body, and the EOT - drives the richness selection + placeholder integration tests.
    b.long_agent_run(
        "kick off the long debugging chain",
        &[
            "found the AGENTRICHFIRST root cause already", // first - rich (lexeme) → kept
            "let me try the LETMEDECL one next",           // middle decl → collapse
            "now i will check LETMEDECL another",          // middle decl → collapse
            // A declaration with a digit adjacent to multi-byte chars (the exact
            // shape that once panicked the ±16-byte number-of-substance window): a
            // signal-less intent-verb opener → collapses, and must NOT panic.
            "let me LETMEDECL look at the 🤖 07:40 log", // middle decl → collapse
            "AGENTRICHMID 12 passed 3 failed in src/x.rs:9", // sudden rich middle → kept
            "let me write LETMEDECL it up",              // middle decl → collapse
            "now let me LETMEDECL finalize",             // middle decl → collapse
            "root cause confirmed in src/y.rs:42 — now let me FUSEDTAIL write the fix", // fused → kept
            "the AGENTEOT final committed answer", // last - always kept
        ],
    );
    // A HUGE round-trip: user > 600 chars, assistant > 900 chars → role-asymmetric ellipsis.
    let huge_user = format!("HEADuser {} TAILuser", "u".repeat(800));
    let huge_asst = format!("HEADasst {} TAILasst", "a".repeat(1100));
    b.round_trip(&huge_user, &huge_asst, 5);
    // An assistant-heavy tail: a complete turn whose assistant side is enormous.
    let big_asst = format!("big {} end", "b".repeat(3000));
    b.round_trip("short live ask", &big_asst, 2);
    // A pure tool-call turn (no assistant EOT text) → partial turn.
    let ts = b.next_ts();
    b.line(&format!(
        r#"{{"type":"user","timestamp":"{ts}","message":{{"role":"user","content":"do the final thing"}}}}"#
    ));
    let ts = b.next_ts();
    b.line(&format!(
        r#"{{"type":"assistant","timestamp":"{ts}","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"z","name":"Bash","input":{{}}}}]}}}}"#
    ));
    // A malformed line that survives the prefilter (carries "role":"user") → counted.
    b.line(r#"{"type":"user","role":"user" broken json after marker}"#);

    b.out
}

pub(crate) fn turns_home() -> Home {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &turns_fixture_jsonl());
    h
}

/// Parse the JSON-lines stdout into a vector of serde_json::Value objects.
pub(crate) fn json_lines(stdout: &str) -> Vec<serde_json::Value> {
    stdout
        .lines()
        .filter(|l| l.trim_start().starts_with('{'))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .collect()
}

/// Strip the OPERATIONAL trailer lines the text renderer prints to stdout but that are
/// NOT part of the reconstruction DOCUMENT (per SPEC §6.8 the document
/// is: doc-header-block + unit headers + bodies + ellipsis markers + boundary banners).
/// The `(skipped N malformed …)` diagnostic and the `(wrote full reconstruction …)`
/// notice are stdout-only chrome, never written to `--out`; everything else stays.
pub(crate) fn turns_document_text(stdout: &str) -> String {
    let kept: Vec<&str> = stdout
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("(skipped ") && !t.starts_with("(wrote ")
        })
        .collect();
    // Re-join with the same '\n' the renderer used; trailing newline normalized away by
    // the line split, so this is the exact emitted-document char basis.
    kept.join("\n")
}

/// A fixture whose NEWEST summary quotes a LIVE-region turn verbatim → exercises the
/// live-region dedup demote-and-flag path (the main fixture's live turns are unique).
pub(crate) fn turns_dedup_home() -> Home {
    let h = Home::new();
    let mut s = String::new();
    // turn 0 (pre-boundary): user + assistant.
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"pre-boundary ask kept verbatim here"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"pre reply"}]}}"#);
    s.push('\n');
    // SUMMARY whose §6 quotes a LIVE turn that comes AFTER it (the "live duplicate ask").
    s.push_str(r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"6. All user messages:\n   - \"the live duplicate ask that the summary already holds verbatim\"\n9. Optional Next Step:\n   The assistant said \"pre reply\"."}}"#);
    s.push('\n');
    // turn 1 (LIVE region) whose user text matches the summary's §6 bullet → deduped.
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"the live duplicate ask that the summary already holds verbatim"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"live reply"}]}}"#);
    s.push('\n');
    // turn 2 (LIVE region) unique.
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T07:00:00.000Z","message":{"role":"user","content":"a unique live follow-up question"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T07:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"follow-up reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{SESS}.jsonl"), &s);
    h
}

/// A SECOND clean session (no malformed lines) under the same project - exercises the
/// multi-session render path (blank separator) + the no-skipped-lines branch.
pub(crate) fn turns_two_sessions_home() -> Home {
    let h = Home::new();
    // Session 1: the main multi-summary fixture (has a malformed line).
    h.write(&format!("{ENC}/{SESS}.jsonl"), &turns_fixture_jsonl());
    // Session 2: a small CLEAN session (no malformed line, no summary).
    let sess2 = "00000000-0000-4000-8000-000000000002";
    let mut s = String::new();
    s.push_str(r#"{"type":"user","timestamp":"2026-06-07T08:00:00.000Z","message":{"role":"user","content":"clean session ask"}}"#);
    s.push('\n');
    s.push_str(r#"{"type":"assistant","timestamp":"2026-06-07T08:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"clean session reply"}]}}"#);
    s.push('\n');
    h.write(&format!("{ENC}/{sess2}.jsonl"), &s);
    h
}

/// The captured pre-feature baseline: the EXACT single-EOT stdout the `turns` tool emitted
/// on the §fixture BEFORE the multi-agent-message richness feature (one agent EOT message
/// per turn, no `△ L…–L…` collapsed-agents placeholder, no intermediate rich members). The
/// DEFAULT now keeps the LONGEST agent message + the first-if-substantive + the rich
/// middles, so this baseline is reproduced by the `--agent-msgs eot-only` ESCAPE, not by
/// the implicit default. Captured under `TZ=UTC` so the system-local timestamp render is
/// deterministic across machines. Re-capture (ONLY on an INTENDED eot-only-output change):
///   cargo test --test cli_integration recapture_turns_pre_feature_baseline -- --ignored
/// then review the baseline diff like any code change.
pub(crate) const TURNS_PRE_FEATURE_BASELINE: &str =
    include_str!("../../turns_pre_feature_baseline.txt");

/// A session whose ONLY genuine human opener is "start the work", followed by an
/// AskUserQuestion exchange (Q+options+multi-byte answer) and an ExitPlanMode plan that the
/// user REJECTS with a typed message, plus an interrupt marker that must NOT split a
/// turn.
pub(crate) fn holes_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            // turn 0: genuine human opener.
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the work"}}"#, "\n",
            // assistant asks (member of turn 0). Options carry per-option descriptions.
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"q1","name":"AskUserQuestion","input":{"questions":[{"question":"which option for step two?","header":"STEP TWO","options":[{"label":"option A (recommended)","description":"the conservative path that reuses existing state"},{"label":"option B","description":"the full path that rebuilds from scratch"}]}]}}]}}"#, "\n",
            // turn 1: the AUQ ANSWER opens a turn (the behavior change). The carrier echoes
            // the options WITH descriptions and carries free-text `annotations.notes`.
            r#"{"type":"user","uuid":"ans","parentUuid":"a0","timestamp":"2026-06-07T05:10:00.000Z","toolUseResult":{"questions":[{"question":"which option for step two?","header":"STEP TWO","options":[{"label":"option A (recommended)","description":"the conservative path that reuses existing state"},{"label":"option B","description":"the full path that rebuilds from scratch"}]}],"answers":{"which option for step two?":"option A is fine, the scope is broader than stated"},"annotations":{"which option for step two?":{"notes":"go with option A but budget for the edge cases, it is more involved than a quick tweak"}}},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"q1","content":"Your questions have been answered: \"which option for step two?\"=\"option A is fine, the scope is broader than stated\"."}]}}"#, "\n",
            // assistant proposes a plan (member of turn 1).
            r#"{"type":"assistant","uuid":"a1","parentUuid":"ans","timestamp":"2026-06-07T05:11:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_PLAN1","name":"ExitPlanMode","input":{"plan":"the plan body here","planFilePath":"/Users/testuser/.claude/plans/elegant-scribbling-dream.md"}}]}}"#, "\n",
            // turn 2: the user REJECTS the plan with a typed message → boundary + pointer.
            r#"{"type":"user","uuid":"rej","parentUuid":"a1","timestamp":"2026-06-07T05:20:00.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_PLAN1","is_error":true,"content":"The user doesn't want to proceed with this tool use. The tool use was rejected (eg. if it was a file edit, the new_string was NOT written to the file). To tell you how to proceed, the user said:\nplease run the smoke tests once before calling it done"}]}}"#, "\n",
            // an interrupt marker - a turn MEMBER of turn 2, NOT a new boundary.
            r#"{"type":"user","uuid":"int","parentUuid":"rej","timestamp":"2026-06-07T05:20:30.000Z","message":{"role":"user","content":[{"type":"text","text":"[Request interrupted by user]"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"int","timestamp":"2026-06-07T05:21:00.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ok, adding the smoke-test check"}]}}"#, "\n",
        ),
    );
    h
}

/// Extract the line texts (in order) from a `recover --at … --format json` snapshot.
pub(crate) fn recon_lines_from_at_json(stdout: &str) -> Vec<String> {
    let snap = stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v.get("kind").and_then(|t| t.as_str()) == Some("snapshot"))
        .unwrap_or_else(|| panic!("no snapshot object in:\n{stdout}"));
    let mut recon: std::collections::BTreeMap<usize, String> = std::collections::BTreeMap::new();
    for l in snap.get("lines").and_then(|v| v.as_array()).unwrap() {
        recon.insert(
            l.get("n").and_then(|v| v.as_u64()).unwrap() as usize,
            l.get("text").and_then(|v| v.as_str()).unwrap().to_string(),
        );
    }
    recon.into_values().collect()
}

/// Build a top-level transcript that (a) binds a plan via a `plan_mode` attachment, (b) also
/// Edits a DIFFERENT plan file (which must NOT be mistaken for the bound one), and (c) has a
/// Write+Edit history of the bound plan (so `@plan` recover has content to rebuild).
pub(crate) fn write_planning_session(h: &Home, sess: &str, bound_abs: &str, other_abs: &str) {
    let jsonl = concat!(
        r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"plan it"}}"#, "\n",
        // plan_mode attachment → the AUTHORITATIVE binding.
        r#"{"type":"attachment","isSidechain":false,"attachment":{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"__BOUND__","planExists":true},"uuid":"att0","timestamp":"2026-06-07T05:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}"#, "\n",
        // An Edit of SOMEONE ELSE's plan file - a red herring for the resolver.
        r#"{"type":"assistant","uuid":"ax","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"ex","name":"Edit","input":{"file_path":"__OTHER__","old_string":"x","new_string":"y"}}]}}"#, "\n",
        r#"{"type":"user","uuid":"cx","timestamp":"2026-06-07T05:00:02.500Z","toolUseResult":{"filePath":"__OTHER__","oldString":"x","newString":"y","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":1,"oldLines":1,"newStart":1,"newLines":1,"lines":["-x","+y"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"ex","content":"ok"}]}}"#, "\n",
        // The bound plan's own Write + Edit history.
        r#"{"type":"assistant","uuid":"aw","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"pw","name":"Write","input":{"file_path":"__BOUND__","content":"P1\nP2\nP3\n"}}]}}"#, "\n",
        r#"{"type":"user","uuid":"cw","timestamp":"2026-06-07T05:00:03.500Z","toolUseResult":{"type":"create","filePath":"__BOUND__","content":"P1\nP2\nP3\n"},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"pw","content":"ok"}]}}"#, "\n",
        r#"{"type":"assistant","uuid":"ap","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"pe","name":"Edit","input":{"file_path":"__BOUND__","old_string":"P2","new_string":"P2-revised"}}]}}"#, "\n",
        r#"{"type":"user","uuid":"cp","timestamp":"2026-06-07T05:00:04.500Z","toolUseResult":{"filePath":"__BOUND__","oldString":"P2","newString":"P2-revised","originalFile":null,"replaceAll":false,"structuredPatch":[{"oldStart":2,"oldLines":1,"newStart":2,"newLines":1,"lines":["-P2","+P2-revised"]}]},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"pe","content":"ok"}]}}"#, "\n",
    )
    .replace("__BOUND__", &jpath(bound_abs))
    .replace("__OTHER__", &jpath(other_abs));
    h.write(&format!("{ENC}/{sess}.jsonl"), &jsonl);
}

/// A synthetic 1×1 transparent PNG (base64) - a REAL valid PNG (correct chunk CRCs, so the
/// strict `image` decoder accepts it for `--as` transcoding, not just the magic-byte checks).
pub(crate) const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVR4nGNgAAIAAAUAAXpeqz8AAAAASUVORK5CYII=";

/// Three more DISTINCT valid 1×1 PNGs (red / green / blue) - distinct content fingerprints, so
/// the listing's content-dedup treats them as separate screenshots (not one re-injected image).
pub(crate) const PNG_RED: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGP4z8DwHwAFAAH/iZk9HQAAAABJRU5ErkJggg==";

pub(crate) const PNG_GREEN: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNg+M/wHwAEAQH/cetH5QAAAABJRU5ErkJggg==";

pub(crate) const PNG_BLUE: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYPj/HwADAgH/5ncLrgAAAABJRU5ErkJggg==";

/// A real 3-frame (red/green/blue, 0.5s each = 1.5s) 4×4 animated GIF89a, for the
/// "convert an animated GIF to a still format → first frame + warning" path.
pub(crate) const ANIM_GIF_3F: &str = "R0lGODlhBAAEAPAAAP8AAAAAACH/C05FVFNDQVBFMi4wAwEAAAAh+QQAMgAAACwAAAAABAAEAAACBISPCQUAIfkEADIAAAAsAAAAAAQABACAAIAAAAAAAgSEjwkFACH5BAAyAAAALAAAAAAEAAQAgAAA/wAAAAIEhI8JBQA7";

pub(crate) fn img_block(media: &str, data: &str) -> serde_json::Value {
    serde_json::json!({"type":"image","source":{"type":"base64","media_type":media,"data":data}})
}

pub(crate) fn image_home() -> Home {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":"/Users/testuser/Projects/foo",
        "version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"first screenshot"}, img_block("image/png", PNG_1X1)]}
    });
    let r1 = serde_json::json!({
        "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
        "message":{"role":"assistant","content":[{"type":"text","text":"got it"}]}
    });
    let r2 = serde_json::json!({
        "type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z",
        "message":{"role":"user","content":[{"type":"text","text":"two more"}, img_block("image/jpeg", PNG_RED), img_block("image/png", PNG_GREEN)]}
    });
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{r0}\n{r1}\n{r2}\n"),
    );
    h
}

/// Fixture: a session where `#1` names TWO different images (CC reuses `#N` per prompt). r0
/// (turn 0) carries #1=transparent + #2=red; a later r2 (turn 1) reuses #1 for a different
/// image (blue). Returns the home.
pub(crate) fn ambiguous_hash_home() -> Home {
    let h = Home::new();
    let r0 = serde_json::json!({
        "type":"user","uuid":"u0","sessionId":SESS,"cwd":"/Users/testuser/Projects/foo",
        "timestamp":"2026-06-07T05:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"look at [Image #1] and [Image #2]"},
            img_block("image/png", PNG_1X1), img_block("image/png", PNG_RED)]}
    });
    let r1 = serde_json::json!({
        "type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z",
        "message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}
    });
    let r2 = serde_json::json!({
        "type":"user","uuid":"u1deadbeef","timestamp":"2026-06-07T06:00:00.000Z",
        "message":{"role":"user","content":[
            {"type":"text","text":"now re-sharing [Image #1]"},
            img_block("image/png", PNG_BLUE)]}
    });
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        &format!("{r0}\n{r1}\n{r2}\n"),
    );
    h
}

/// A minimal top-level session jsonl (one genuine-user + one assistant turn) for the
/// elicitation-sidecar tests. Each test drops its own `<SESS>/elicitations.jsonl` content.
pub(crate) fn sidecar_session_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","cwd":"/Users/testuser/Projects/foo","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"text","text":"working on it"}]}}"#, "\n",
        ),
    );
    h
}

/// An unresolved AskUserQuestion pending sidecar record (native-shaped assistant tool_use +
/// csift marker fields).
pub(crate) fn auq_pending_line(key: &str, ts: &str, question: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"u-{key}","timestamp":"{ts}","sessionId":"{SESS}","cwd":"/Users/testuser/Projects/foo","isSidechain":false,"message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"{key}","name":"AskUserQuestion","input":{{"questions":[{{"question":"{question}"}}]}}}}]}},"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"{key}","csiftHookEvent":"PreToolUse","hookInput":{{}}}}"#
    )
}

pub(crate) fn resolved_line(key: &str, ts: &str) -> String {
    format!(
        r#"{{"type":"csift-elicitation-resolved","uuid":"r-{key}","timestamp":"{ts}","sessionId":"{SESS}","csift":"elicitation-marker-v1","csiftPhase":"resolved","csiftKind":"AskUserQuestion","csiftKey":"{key}"}}"#
    )
}

pub(crate) fn mcp_pending_line(key: &str, ts: &str, server: &str, message: &str) -> String {
    format!(
        r#"{{"type":"system","subtype":"mcp_elicitation","uuid":"m-{key}","timestamp":"{ts}","sessionId":"{SESS}","isSidechain":false,"content":"MCP elicitation [{server}] (url): {message}","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp-elicitation","csiftKey":"{key}","csiftMcpServer":"{server}","hookInput":{{}}}}"#
    )
}

pub(crate) const ACC_ENC: &str = "-Users-x-acc";

pub(crate) const ACC_SESS: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";

pub(crate) const ACC_SUB: &str = "c0ffeec0ffeec0ff";

/// A `$HOME` whose single top-level transcript carries one record per §A–J disk-shape (each with a
/// unique `zz<name>` token so a search targets it precisely), plus one subagent transcript whose
/// opener seed exercises C9 (parent ⇨ self).
pub(crate) fn acceptance_home() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ACC_ENC}/{ACC_SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","sessionId":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","cwd":"/Users/x/acc","version":"2.1.0","gitBranch":"main","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzgenuine human prose here"}}"#, "\n",
            r#"{"type":"user","uuid":"u0b","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":[{"type":"text","text":"zzblocktext genuine prose in a text block"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0b","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzagentmsg the visible reply"},{"type":"thinking","thinking":"zzthink hidden reasoning"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"a0","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"spawn1","name":"Agent","input":{"subagent_type":"general-purpose","name":"audit-x","prompt":"zzspawn go audit the thing"}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"a1","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sm1","name":"SendMessage","input":{"to":"GraftBoard","type":"message","message":"zzsent ship the fix"}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"a2","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"sm2","name":"SendMessage","input":{"to":"GraftBoard","type":"shutdown_request","message":{"type":"shutdown_request","reason":"zzshutdownreq stop now"}}}]}}"#, "\n",
            r#"{"type":"user","uuid":"sig1","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"SOurDnd\" color=\"green\">\n{\"type\":\"idle_notification\",\"from\":\"SOurDnd\",\"idleReason\":\"zzidle available\"}\n</teammate-message>"}}"#, "\n",
            r#"{"type":"user","uuid":"sig2","timestamp":"2026-06-07T05:00:07.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"system\">\n{\"type\":\"teammate_terminated\",\"message\":\"zzterminated B38 has shut down.\"}\n</teammate-message>"}}"#, "\n",
            r#"{"type":"user","uuid":"sig3","timestamp":"2026-06-07T05:00:08.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"B38\">\n{\"type\":\"shutdown_approved\",\"from\":\"B38\",\"reason\":\"zzapproved ok done\"}\n</teammate-message>"}}"#, "\n",
            r#"{"type":"user","uuid":"tm2","timestamp":"2026-06-07T05:00:09.000Z","message":{"role":"user","content":"<teammate-message teammate_id=\"team-lead\" color=\"blue\">\nzzbareinbox please check the rate limit\n</teammate-message>"}}"#, "\n",
            r#"{"type":"user","uuid":"n1","timestamp":"2026-06-07T05:00:10.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>mon1</task-id>\n<status>completed</status>\n<event>tick</event>\n<summary>Monitor \"zzmonitor liveness\" fired</summary>\n</task-notification>"}}"#, "\n",
            r#"{"type":"user","uuid":"cmd1","timestamp":"2026-06-07T05:00:11.000Z","message":{"role":"user","content":"<command-name>/deploy</command-name>\n<command-message>deploy</command-message>\n<command-args>zzcmdargs to staging now</command-args>"}}"#, "\n",
            r#"{"type":"user","uuid":"out1","timestamp":"2026-06-07T05:00:12.000Z","message":{"role":"user","content":"<local-command-stdout>zzstdout Login successful</local-command-stdout>"}}"#, "\n",
            r#"{"type":"user","uuid":"int1","timestamp":"2026-06-07T05:00:13.000Z","message":{"role":"user","content":"[Request interrupted by user]"}}"#, "\n",
            r#"{"type":"user","uuid":"int2","timestamp":"2026-06-07T05:00:14.000Z","message":{"role":"user","content":"[Request interrupted by user for tool use]"}}"#, "\n",
            r##"{"type":"user","uuid":"wake1","isMeta":true,"timestamp":"2026-06-07T05:00:15.000Z","message":{"role":"user","content":"# Autonomous loop check\n\nYou're being invoked on a timer while the user is away. zzwakeup"}}"##, "\n",
            r#"{"type":"user","uuid":"cont1","isMeta":true,"timestamp":"2026-06-07T05:00:16.000Z","message":{"role":"user","content":[{"type":"text","text":"Continue from where you left off."}]}}"#, "\n",
            r#"{"type":"user","uuid":"hook1","isMeta":true,"timestamp":"2026-06-07T05:00:17.000Z","message":{"role":"user","content":"Stop hook feedback:\nThe last Edit failed. zzhook retry"}}"#, "\n",
            r##"{"type":"user","uuid":"loop1","isMeta":true,"timestamp":"2026-06-07T05:00:18.000Z","message":{"role":"user","content":"# Autonomous loop tick (dynamic pacing)\nzzloop driver"}}"##, "\n",
            r#"{"type":"user","uuid":"meta0","isMeta":true,"timestamp":"2026-06-07T05:00:19.000Z","message":{"role":"user","content":"zzunmarked novel hook wrapper text"}}"#, "\n",
            r#"{"type":"user","uuid":"sum1","isCompactSummary":true,"isVisibleInTranscriptOnly":true,"timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"user","content":"This session is being continued. zzsummary prior context preserved"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"bnd1","timestamp":"2026-06-07T05:00:21.000Z","content":"Conversation compacted zzboundary","compactMetadata":{"trigger":"auto","preTokens":1000,"postTokens":200,"durationMs":50}}"#, "\n",
            r#"{"type":"attachment","uuid":"att1","timestamp":"2026-06-07T05:00:22.000Z","attachment":{"type":"hook_success","note":"zzattach should not surface"}}"#, "\n",
        ),
    );
    // C9: a subagent transcript whose opener seed is the delivered spawn prompt (parent ⇨ self).
    h.write(
        &format!("{ACC_ENC}/{ACC_SESS}/subagents/agent-{ACC_SUB}.jsonl"),
        concat!(
            r#"{"type":"user","isSidechain":true,"agentId":"c0ffeec0ffeec0ff","uuid":"so0","timestamp":"2026-06-07T05:00:03.500Z","message":{"role":"user","content":"zzopener do the delegated work please"}}"#, "\n",
            r#"{"type":"assistant","uuid":"sa0","parentUuid":"so0","timestamp":"2026-06-07T05:00:03.700Z","message":{"role":"assistant","content":[{"type":"text","text":"zzsubreply on it"}]}}"#, "\n",
        ),
    );
    h
}

/// `search <pattern> -t <selector>` over the top-level acceptance transcript only.
pub(crate) fn acc(h: &Home, pattern: &str, selector: &str) -> Output {
    h.run(&[
        "search",
        pattern,
        "-t",
        selector,
        &at(ACC_SESS),
        "--no-subagents",
    ])
}

pub(crate) const HOOKCTX_SESS: &str = "5c1d9e02-4b7a-4f3c-9d21-6e8a0b4c7d15";

/// One session: a genuine user turn, a `hook_additional_context` attachment record
/// (content ARRAY - two blocks, joined with `\n`), and the agent's reply.
pub(crate) fn hook_context_scenario(h: &Home) {
    let enc = "-Users-dev-example-project";
    let body = concat!(
        r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start the beacon work"}}"#,
        "\n",
        r#"{"parentUuid":"u1","attachment":{"type":"hook_additional_context","content":["<project-memory-context>\nquartzlantern rules apply\n</project-memory-context>","second block harborlight note"],"hookName":"SessionStart","hookEvent":"SessionStart"},"type":"attachment","uuid":"att1","timestamp":"2026-06-07T05:00:01.000Z"}"#,
        "\n",
        r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"done with the beacon work"}]}}"#,
        "\n",
    );
    h.write(&format!("{enc}/{HOOKCTX_SESS}.jsonl"), body);
}
