//! The JSON `body` field: the rendered section text with its newlines intact, beside the
//! match-centered flat `excerpt`. Cross-command, because ONE `Hit` field feeds both
//! `search --format json` (`body`, non-null only under `--no-truncate`) and `show
//! --format json` (`body` beside `text`, non-null only under the `--line`/`--uuid`
//! address that lifts the excerpt cap).

use crate::harness::*;

/// `search::EXCERPT_MAX` - the default excerpt budget in CHARS. Not importable from an
/// e2e test, so it is restated here; a change to the constant must break these tests.
const EXCERPT_MAX: usize = 400;

/// The documented `model::normalize_line` rule: every run of whitespace (newlines and
/// tabs included) becomes ONE space, and the ends are trimmed. This is what makes an
/// `excerpt` one line, and re-deriving it here is how the excerpt bytes get pinned to the
/// documented semantics instead of to a captured run.
fn collapse(s: &str) -> String {
    let mut out = String::new();
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !prev_ws && !out.is_empty() {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    while out.ends_with(' ') {
        out.pop();
    }
    out
}

/// A multi-line assistant body: a lead paragraph, a BLANK line (the consecutive-newline
/// run a markdown paragraph break produces), then padded rows so the whole text runs past
/// `EXCERPT_MAX` and the default excerpt is provably a fragment. The needle leads the text
/// so the match-centered window starts at char 0 - which makes the expected default
/// excerpt derivable without guessing where the window opens.
fn multiline_body() -> String {
    let mut lines = vec!["BODYSEAM leads the body".to_string(), String::new()];
    for i in 0..8 {
        lines.push(format!("| row {i:02} | {} |", "padding".repeat(8)));
    }
    lines.push(String::new());
    lines.push("and a closing paragraph".to_string());
    lines.join("\n")
}

/// Write a one-turn transcript whose single assistant record carries `body` as its ONLY
/// text block, and return the `@<uuid>` target.
fn home_with_body(h: &Home, body: &str) -> String {
    let enc = "-Users-dev-Projects-bodyfield";
    let sess = "11111111-2222-4333-8444-555555555555";
    let esc = serde_json::to_string(body).expect("json string");
    let jsonl = format!(
        concat!(
            r#"{{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-07-01T00:00:00.000Z","sessionId":"{sess}","cwd":"/Users/dev/Projects/bodyfield","message":{{"role":"user","content":"render the BODYSEAM table"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-07-01T00:00:01.000Z","sessionId":"{sess}","message":{{"role":"assistant","id":"m1","model":"test-model","content":[{{"type":"text","text":{esc}}}]}}}}"#,
            "\n",
        ),
        sess = sess,
        esc = esc,
    );
    h.write(&format!("{enc}/{sess}.jsonl"), &jsonl);
    at(sess)
}

/// The one `agent.message` hit of a `search --format json` run.
fn agent_hit(stdout: &str) -> serde_json::Value {
    let hits: Vec<serde_json::Value> = json_rows(stdout, "exchange")
        .into_iter()
        .flat_map(|o| o["hits"].as_array().cloned().unwrap_or_default())
        .filter(|h| h["label"] == "agent.message")
        .collect();
    assert_eq!(hits.len(), 1, "exactly one agent.message hit: {stdout}");
    hits.into_iter().next().unwrap()
}

#[test]
fn search_body_keeps_newlines_and_excerpt_keeps_its_flat_bytes() {
    let h = Home::new();
    let body = multiline_body();
    let target = home_with_body(&h, &body);
    let newlines = body.matches('\n').count();
    assert_eq!(newlines, 11, "the fixture body's own newline count");
    let total_chars = body.chars().count();
    assert!(
        total_chars > EXCERPT_MAX,
        "the fixture must overrun the default cap: {total_chars} chars"
    );

    // ── default cap: `body` is null, `excerpt` is the flat match-centered fragment ──
    let def = h.run(&["search", "BODYSEAM", &target, "--format", "json"]);
    assert!(def.success, "stderr: {}", def.stderr);
    let hit = agent_hit(&def.stdout);
    assert_eq!(
        hit["body"],
        serde_json::Value::Null,
        "under the default cap the excerpt is a fragment, so `body` is null - not a \
         second full copy of every matched record"
    );
    // The needle leads the text, so `win_start` saturates to 0: the window is the first
    // EXCERPT_MAX chars, collapsed, plus the explicit remainder marker. No leading `…`.
    let window: String = body.chars().take(EXCERPT_MAX).collect();
    let expected_default = format!(
        "{}… (+{} chars)",
        collapse(&window),
        total_chars - EXCERPT_MAX
    );
    assert_eq!(
        hit["excerpt"].as_str().expect("excerpt"),
        expected_default,
        "the default excerpt bytes are unchanged: the whitespace-collapsed window plus \
         the explicit `… (+N chars)` marker"
    );
    assert_eq!(
        hit["excerpt"].as_str().unwrap().matches('\n').count(),
        0,
        "an excerpt is one line by contract"
    );

    // ── --no-truncate: `body` is the rendered section text VERBATIM, newlines intact ──
    let full = h.run(&[
        "search",
        "BODYSEAM",
        &target,
        "--format",
        "json",
        "--no-truncate",
    ]);
    assert!(full.success, "stderr: {}", full.stderr);
    let hit = agent_hit(&full.stdout);
    assert_eq!(
        hit["body"].as_str().expect("body under --no-truncate"),
        body,
        "`body` is the hit's own section text, byte for byte"
    );
    assert_eq!(
        hit["body"].as_str().unwrap().matches('\n').count(),
        newlines,
        "every newline the render carried survives into `body`"
    );
    // `excerpt` keeps its own contract in this mode too: the WHOLE text, still flat.
    assert_eq!(
        hit["excerpt"].as_str().expect("excerpt"),
        collapse(&body),
        "under --no-truncate the excerpt is the whole text, whitespace still collapsed"
    );
    assert_eq!(
        hit["excerpt"].as_str().unwrap().matches('\n').count(),
        0,
        "--no-truncate lifts the CAP, it does not un-flatten the excerpt"
    );
}

#[test]
fn search_body_is_per_section_on_a_multi_block_record() {
    // Two text blocks on one assistant record emit two `agent.message` hits, and `body`
    // is that SECTION's text - never the record's concatenation.
    let h = Home::new();
    let enc = "-Users-dev-Projects-bodysections";
    let sess = "22222222-3333-4444-8555-666666666666";
    let first = "SECTIONONE first\nsecond line";
    let second = "SECTIONONE other\nthird line\nfourth line";
    let jsonl = format!(
        concat!(
            r#"{{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-07-01T00:00:00.000Z","sessionId":"{sess}","cwd":"/Users/dev/Projects/bodysections","message":{{"role":"user","content":"go"}}}}"#,
            "\n",
            r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-07-01T00:00:01.000Z","sessionId":"{sess}","message":{{"role":"assistant","id":"m1","model":"test-model","content":[{{"type":"text","text":{a}}},{{"type":"text","text":{b}}}]}}}}"#,
            "\n",
        ),
        sess = sess,
        a = serde_json::to_string(first).unwrap(),
        b = serde_json::to_string(second).unwrap(),
    );
    h.write(&format!("{enc}/{sess}.jsonl"), &jsonl);
    let target = at(sess);

    let out = h.run(&[
        "search",
        "SECTIONONE",
        &target,
        "--format",
        "json",
        "--no-truncate",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let bodies: Vec<String> = json_rows(&out.stdout, "exchange")
        .into_iter()
        .flat_map(|o| o["hits"].as_array().cloned().unwrap_or_default())
        .filter(|h| h["label"] == "agent.message")
        .map(|h| h["body"].as_str().expect("body").to_string())
        .collect();
    assert_eq!(bodies, vec![first.to_string(), second.to_string()]);
}

#[test]
fn search_body_renders_the_flattened_form_for_a_user_opener() {
    // `body` is the RENDERED section text, not the raw JSON: a user opener goes through
    // the shared `reconstructed_user_text` -> `flatten_content_text` path, which collapses
    // in the MODEL layer, so its body carries no newline either. `--raw` is the only path
    // to the bytes, and this pins that the field never claims more than it delivers.
    let h = Home::new();
    let enc = "-Users-dev-Projects-bodyuser";
    let sess = "33333333-4444-4555-8666-777777777777";
    let typed = "USERSEAM first line\n\nsecond paragraph";
    let jsonl = format!(
        concat!(
            r#"{{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-07-01T00:00:00.000Z","sessionId":"{sess}","cwd":"/Users/dev/Projects/bodyuser","message":{{"role":"user","content":{t}}}}}"#,
            "\n",
        ),
        sess = sess,
        t = serde_json::to_string(typed).unwrap(),
    );
    h.write(&format!("{enc}/{sess}.jsonl"), &jsonl);
    let target = at(sess);

    let out = h.run(&[
        "search",
        "USERSEAM",
        &target,
        "--format",
        "json",
        "--no-truncate",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let hit = json_rows(&out.stdout, "exchange")
        .into_iter()
        .flat_map(|o| o["hits"].as_array().cloned().unwrap_or_default())
        .find(|h| h["label"] == "user.message")
        .expect("a user.message hit");
    assert_eq!(
        hit["body"].as_str().expect("body"),
        collapse(typed),
        "the opener renderer flattens in the model layer, so `body` is that flat render"
    );
}

#[test]
fn show_body_follows_the_address_that_lifts_the_cap() {
    let h = Home::new();
    let body = multiline_body();
    let target = home_with_body(&h, &body);
    let newlines = body.matches('\n').count();

    // `--line` lifts the excerpt cap, so `body` is the record text verbatim while `text`
    // keeps its flat form.
    let by_line = h.run(&["show", &target, "--line", "2", "--format", "json"]);
    assert!(by_line.success, "stderr: {}", by_line.stderr);
    let rows = json_rows(&by_line.stdout, "record");
    assert_eq!(rows.len(), 1, "one addressed record: {}", by_line.stdout);
    assert_eq!(rows[0]["body"].as_str().expect("body"), body);
    assert_eq!(
        rows[0]["body"].as_str().unwrap().matches('\n').count(),
        newlines
    );
    assert_eq!(rows[0]["text"].as_str().expect("text"), collapse(&body));

    // `--uuid` is the same address in the other spelling - same answer.
    let by_uuid = h.run(&["show", &target, "--uuid", "a1", "--format", "json"]);
    assert!(by_uuid.success, "stderr: {}", by_uuid.stderr);
    let rows = json_rows(&by_uuid.stdout, "record");
    assert_eq!(rows[0]["body"].as_str().expect("body"), body);

    // `--turn` does NOT lift the cap (it selects the turn's records through the ordinary
    // excerpt budget), so its `text` is a capped fragment and `body` is null rather than a
    // truncated half-body pretending to be the record.
    let by_turn = h.run(&["show", &target, "--turn", "0", "--format", "json"]);
    assert!(by_turn.success, "stderr: {}", by_turn.stderr);
    let agent = json_rows(&by_turn.stdout, "record")
        .into_iter()
        .find(|r| r["label"] == "agent.message")
        .expect("the agent record of turn 0");
    assert!(
        agent["text"].as_str().expect("text").contains("chars)"),
        "--turn keeps the default cap, so its text carries the explicit marker: {}",
        by_turn.stdout
    );
    assert_eq!(agent["body"], serde_json::Value::Null);
}
