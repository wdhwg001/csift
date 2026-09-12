//! `plan --audit`'s four BINDING facts, each with a fixture where it fires and one where it
//! does not: the slug's change points, whether the bound file is on disk, plan text held with
//! no binding, and the first slug-carrying record against the plan file's birth instant.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESSION: &str = "11111111-2222-4333-8444-555555555555";

/// A user record carrying a top-level `slug` - the binding key, as the harness stamps it.
fn slugged(uuid: &str, ts: &str, slug: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","slug":"{slug}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

/// The same record BEFORE the slug is minted: no such key.
fn unslugged(uuid: &str, ts: &str, text: &str) -> String {
    format!(
        r#"{{"type":"user","uuid":"{uuid}","timestamp":"{ts}","message":{{"role":"user","content":"{text}"}}}}"#
    )
}

/// The Plan-Mode entry attachment: the authoritative binding.
fn plan_mode(uuid: &str, ts: &str, slug: &str, plan_file: &str) -> String {
    let escaped = plan_file.replace('\\', "\\\\");
    format!(
        r#"{{"type":"attachment","uuid":"{uuid}","timestamp":"{ts}","slug":"{slug}","attachment":{{"type":"plan_mode","isSubAgent":false,"planExists":false,"planFilePath":"{escaped}"}}}}"#
    )
}

/// The post-compaction re-injection of the BOUND plan's whole content.
fn plan_reference(uuid: &str, ts: &str, plan_file: &str) -> String {
    let escaped = plan_file.replace('\\', "\\\\");
    format!(
        r##"{{"type":"attachment","uuid":"{uuid}","timestamp":"{ts}","attachment":{{"type":"plan_file_reference","planFilePath":"{escaped}","content":"# the plan"}}}}"##
    )
}

fn rows(out: &Output, kind: &str) -> Vec<serde_json::Value> {
    out.stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["kind"] == kind)
        .collect()
}

fn one(out: &Output, kind: &str) -> serde_json::Value {
    let r = rows(out, kind);
    assert_eq!(r.len(), 1, "expected exactly one {kind} row: {r:?}");
    r.into_iter().next().unwrap()
}

// ── (a) the slug's change points ──────────────────────────────────────────────

#[test]
fn the_slug_mint_point_is_reported_as_a_change_from_none() {
    // The measured corpus shape: a run of unslugged records, then the mint, then one stable
    // value - exactly one change point, whose `from` is absent.
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n{}\n{}\n{}\n",
            unslugged("u1", "2020-06-07T05:00:00.000Z", "before the mint"),
            slugged(
                "u2",
                "2020-06-07T05:01:00.000Z",
                "quiet-harbor-relay",
                "after"
            ),
            plan_mode(
                "a1",
                "2020-06-07T05:02:00.000Z",
                "quiet-harbor-relay",
                plan.to_str().unwrap()
            ),
            slugged(
                "u3",
                "2020-06-07T05:03:00.000Z",
                "quiet-harbor-relay",
                "more"
            ),
        ),
    );

    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains("slug     L2  none -> quiet-harbor-relay  (the slug is minted here)"),
        "the mint is the change point and is labelled as one:\n{}",
        out.stdout
    );

    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    assert_eq!(
        b["slug_changes"],
        serde_json::json!([{"line": 2, "from": null, "to": "quiet-harbor-relay"}]),
        "{b}"
    );
    assert_eq!(b["first_slug_line"], 2, "{b}");
}

#[test]
fn a_slug_that_moves_is_reported_as_a_second_change_point() {
    // The shape the check exists for and that the corpus does not contain: a binding that
    // MOVED mid-file. One change point per transition, in file order.
    let h = Home::new();
    let plan = h.write_claude("plans/second-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n{}\n{}\n",
            slugged(
                "u1",
                "2020-06-07T05:00:00.000Z",
                "first-harbor-relay",
                "one"
            ),
            slugged(
                "u2",
                "2020-06-07T05:01:00.000Z",
                "second-harbor-relay",
                "two"
            ),
            plan_mode(
                "a1",
                "2020-06-07T05:02:00.000Z",
                "second-harbor-relay",
                plan.to_str().unwrap()
            ),
        ),
    );

    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    assert!(
        out.stdout.contains(
            "slug     L2  first-harbor-relay -> second-harbor-relay  (the binding MOVED here)"
        ),
        "a move reads differently from a mint:\n{}",
        out.stdout
    );
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    let changes = b["slug_changes"].as_array().expect("array");
    assert_eq!(changes.len(), 2, "{b}");
    assert!(changes[0]["from"].is_null(), "{b}");
    assert_eq!(changes[1]["from"], "first-harbor-relay", "{b}");
}

#[test]
fn a_slug_nested_in_a_payload_is_not_a_change_point() {
    // The walk is depth-1, so a tool input that happens to carry the key mints nothing.
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n{}\n",
            plan_mode(
                "a1",
                "2020-06-07T05:00:00.000Z",
                "quiet-harbor-relay",
                plan.to_str().unwrap()
            ),
            r#"{"type":"assistant","uuid":"a2","timestamp":"2020-06-07T05:01:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"slug":"not-a-binding","command":"true"}}]}}"#,
        ),
    );
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    let changes = b["slug_changes"].as_array().expect("array");
    assert_eq!(changes.len(), 1, "only the attachment's own slug: {b}");
    assert_eq!(changes[0]["to"], "quiet-harbor-relay", "{b}");
}

// ── (b) whether the bound plan file is on disk ────────────────────────────────

#[test]
fn the_binding_row_says_whether_the_bound_file_exists() {
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n",
            plan_mode(
                "a1",
                "2020-06-07T05:00:00.000Z",
                "quiet-harbor-relay",
                plan.to_str().unwrap()
            )
        ),
    );
    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    assert!(out.stdout.contains("[exists]"), "{}", out.stdout);
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    assert_eq!(b["plan_exists"], true, "{b}");
}

#[test]
fn a_bound_file_that_was_never_written_reads_missing() {
    // The name is minted at Plan-Mode entry and the file lands only when content is first
    // written, so `[missing]` is an ordinary state and not a fault.
    let h = Home::new();
    let absent = h.root.join(".claude/plans/never-written-plan.md");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n",
            plan_mode(
                "a1",
                "2020-06-07T05:00:00.000Z",
                "never-written-plan",
                absent.to_str().unwrap()
            )
        ),
    );
    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    assert!(out.stdout.contains("[missing]"), "{}", out.stdout);
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    assert_eq!(b["plan_exists"], false, "{b}");
}

// ── (c) plan text held with no binding ────────────────────────────────────────

#[test]
fn a_plan_reference_with_no_slug_anywhere_is_flagged_as_unbound_text() {
    // The fork-stripped shape: the re-injected plan content is here, but no record carries a
    // slug, so nothing binds it and nothing will re-inject it again.
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n{}\n",
            unslugged("u1", "2020-06-07T05:00:00.000Z", "carry on"),
            plan_reference("a1", "2020-06-07T05:01:00.000Z", plan.to_str().unwrap()),
        ),
    );
    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(
            "holds a plan_file_reference attachment (L2) while NO record carries a slug - plan \
             text without a binding"
        ),
        "the unbound-text warning names the line:\n{}",
        out.stdout
    );

    let out = h.run(&[
        "plan",
        "--audit",
        &format!("@{SESSION}"),
        "--format",
        "json",
    ]);
    let r = one(&out, "plan-unbound-text");
    assert_eq!(
        r["plan_file_reference_lines"],
        serde_json::json!([2]),
        "{r}"
    );
    let s = one(&out, "summary");
    assert_eq!(s["unbound_plan_text"], 1, "{s}");
    assert_eq!(
        s["warnings"], 1,
        "the unbound text counts as a warning: {s}"
    );
}

#[test]
fn a_plan_reference_beside_a_slug_is_the_ordinary_bound_case() {
    // Same attachment, one slug present: nothing to warn about.
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n{}\n",
            slugged("u1", "2020-06-07T05:00:00.000Z", "quiet-harbor-relay", "hi"),
            plan_reference("a1", "2020-06-07T05:01:00.000Z", plan.to_str().unwrap()),
        ),
    );
    let out = h.run(&[
        "plan",
        "--audit",
        &format!("@{SESSION}"),
        "--format",
        "json",
    ]);
    assert!(
        rows(&out, "plan-unbound-text").is_empty(),
        "a slug is present, so nothing is unbound:\n{}",
        out.stdout
    );
    assert_eq!(one(&out, "summary")["unbound_plan_text"], 0);
}

#[test]
fn a_prose_mention_of_the_attachment_type_is_not_a_plan_reference() {
    // The check reads the attachment's own `type`, never the byte match that admitted the
    // line - the same rule the `plan_mode` binding follows.
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n",
            unslugged(
                "u1",
                "2020-06-07T05:00:00.000Z",
                "what does plan_file_reference do"
            )
        ),
    );
    let out = h.run(&[
        "plan",
        "--audit",
        &format!("@{SESSION}"),
        "--format",
        "json",
    ]);
    assert!(
        rows(&out, "plan-unbound-text").is_empty(),
        "prose is not an attachment:\n{}",
        out.stdout
    );
}

// ── (d) the slug against the plan file's birth instant ────────────────────────

/// Whether this host records a file birth time. A musl target does not (`created()` errors
/// there), so the comparison can read nothing but `unknown` with that reason - the answer the
/// fact documents for such a platform, and the arm these tests assert there.
fn host_records_birth_time(plan_file: &std::path::Path) -> bool {
    std::fs::metadata(plan_file)
        .and_then(|m| m.created())
        .is_ok()
}

#[test]
fn a_slug_older_than_the_plan_file_reads_before() {
    // The ordinary order: the name is minted at Plan-Mode entry, the file lands later. The
    // fixture's 2020 timestamps are necessarily older than a file written by this test.
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n",
            plan_mode(
                "a1",
                "2020-06-07T05:00:00.000Z",
                "quiet-harbor-relay",
                plan.to_str().unwrap()
            )
        ),
    );
    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    if host_records_birth_time(&plan) {
        assert!(
            out.stdout.contains(
                "birth    the first slug-carrying record is before the plan file's birth"
            ),
            "{}",
            out.stdout
        );
        assert_eq!(b["slug_vs_plan_file"], "before", "{b}");
        assert!(b["slug_vs_plan_file_reason"].is_null(), "{b}");
    } else {
        assert!(
            out.stdout
                .contains("birth    unknown - this platform records no file birth time"),
            "{}",
            out.stdout
        );
        assert_eq!(b["slug_vs_plan_file"], "unknown", "{b}");
        assert_eq!(
            b["slug_vs_plan_file_reason"], "this platform records no file birth time",
            "{b}"
        );
    }
}

#[test]
fn a_slug_newer_than_the_plan_file_reads_after() {
    // The inherited-plan order: this transcript bound a file that already existed. A 2099
    // timestamp is necessarily newer than a file written by this test.
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n",
            plan_mode(
                "a1",
                "2099-06-07T05:00:00.000Z",
                "quiet-harbor-relay",
                plan.to_str().unwrap()
            )
        ),
    );
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    if host_records_birth_time(&plan) {
        assert_eq!(b["slug_vs_plan_file"], "after", "{b}");
    } else {
        assert_eq!(b["slug_vs_plan_file"], "unknown", "{b}");
        assert_eq!(
            b["slug_vs_plan_file_reason"], "this platform records no file birth time",
            "{b}"
        );
    }
}

#[test]
fn an_absent_plan_file_makes_the_comparison_unknown_with_its_reason() {
    // Unknown is a real answer and it names why, rather than picking a direction.
    let h = Home::new();
    let absent = h.root.join(".claude/plans/never-written-plan.md");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{}\n",
            plan_mode(
                "a1",
                "2020-06-07T05:00:00.000Z",
                "never-written-plan",
                absent.to_str().unwrap()
            )
        ),
    );
    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    assert!(
        out.stdout
            .contains("birth    unknown - the plan file is not on disk"),
        "{}",
        out.stdout
    );
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    assert_eq!(b["slug_vs_plan_file"], "unknown", "{b}");
    assert_eq!(
        b["slug_vs_plan_file_reason"], "the plan file is not on disk",
        "{b}"
    );
}

#[test]
fn a_binding_whose_records_carry_no_slug_says_so_instead_of_inventing_one() {
    // An older binding record predates the slug field. The binding still stands; the three
    // slug-derived facts report their own absence.
    let h = Home::new();
    let plan = h.write_claude("plans/quiet-harbor-relay.md", "# the plan\n");
    // A hand-built record: the path is JSON-escaped like the fixture helpers do, or a Windows
    // path's backslashes make the line unparseable and the binding vanishes with it.
    let escaped = plan.to_str().unwrap().replace('\\', "\\\\");
    h.write(
        &format!("{ENC}/{SESSION}.jsonl"),
        &format!(
            "{{\"type\":\"attachment\",\"uuid\":\"a1\",\"timestamp\":\"2020-06-07T05:00:00.000Z\",\"attachment\":{{\"type\":\"plan_mode\",\"isSubAgent\":false,\"planExists\":true,\"planFilePath\":\"{escaped}\"}}}}\n"
        ),
    );
    let out = h.run(&["plan", "--audit", &format!("@{SESSION}")]);
    assert!(
        out.stdout
            .contains("slug     none carried by this transcript's records"),
        "{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("birth    unknown - no slug-carrying record has a timestamp"),
        "{}",
        out.stdout
    );
    let b = one(
        &h.run(&[
            "plan",
            "--audit",
            &format!("@{SESSION}"),
            "--format",
            "json",
        ]),
        "binding",
    );
    assert_eq!(b["slug_changes"], serde_json::json!([]), "{b}");
    assert!(b["first_slug_line"].is_null(), "{b}");
    assert_eq!(b["slug_vs_plan_file"], "unknown", "{b}");
}
