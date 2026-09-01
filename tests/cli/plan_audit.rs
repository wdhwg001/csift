//! plan --audit: plan-file edits joined against corpus plan bindings.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const BINDER: &str = "11111111-2222-4333-8444-555566667777";
const AUDITEE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeffff0000";
const COBINDER: &str = "ffffffff-1111-4222-8333-444455556666";
const SHARED_PLAN: &str = "/Users/dev/.claude/plans/amber-drifting-kite.md";
const OWN_PLAN: &str = "/Users/dev/.claude/plans/copper-still-pond.md";

/// BINDER binds the shared plan; AUDITEE binds its own plan, edits its own plan once,
/// edits the shared plan twice, edits an ordinary file (never audited), and carries one
/// shape-malformed line; COBINDER ALSO binds AUDITEE's plan and edits it once (its own).
fn audit_scenario(h: &Home) {
    h.write(
        &format!("{ENC}/{BINDER}.jsonl"),
        &format!(concat!(
            r#"{{"type":"user","uuid":"b1","timestamp":"2026-06-07T04:00:00.000Z","message":{{"role":"user","content":"plan the kite"}}}}"#, "\n",
            r#"{{"type":"attachment","isSidechain":false,"attachment":{{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"{sp}","planExists":false}},"uuid":"batt","timestamp":"2026-06-07T04:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}}"#, "\n",
        ), sp = SHARED_PLAN),
    );
    h.write(
        &format!("{ENC}/{AUDITEE}.jsonl"),
        &format!(concat!(
            r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"work"}}}}"#, "\n",
            r#"{{"type":"attachment","isSidechain":false,"attachment":{{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"{op}","planExists":false}},"uuid":"aatt","timestamp":"2026-06-07T05:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t1","name":"Edit","input":{{"file_path":"{op}","old_string":"a","new_string":"b"}}}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a2","parentUuid":"u1","timestamp":"2026-06-07T05:00:03.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t2","name":"Edit","input":{{"file_path":"{sp}","old_string":"c","new_string":"d"}}}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a3","parentUuid":"u1","timestamp":"2026-06-07T05:00:04.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t3","name":"Write","input":{{"file_path":"{sp}","content":"whole"}}}}]}}}}"#, "\n",
            r#"{{"type":"assistant","uuid":"a4","parentUuid":"u1","timestamp":"2026-06-07T05:00:05.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"t4","name":"Edit","input":{{"file_path":"/Users/dev/example-project/src/main.rs","old_string":"e","new_string":"f"}}}}]}}}}"#, "\n",
            "not json here", "\n",
        ), op = OWN_PLAN, sp = SHARED_PLAN),
    );
    h.write(
        &format!("{ENC}/{COBINDER}.jsonl"),
        &format!(concat!(
            r#"{{"type":"user","uuid":"c1","timestamp":"2026-06-07T03:00:00.000Z","message":{{"role":"user","content":"resume the pond plan"}}}}"#, "\n",
            r#"{{"type":"attachment","isSidechain":false,"attachment":{{"type":"plan_mode","reminderType":"full","isSubAgent":false,"planFilePath":"{op}","planExists":true}},"uuid":"catt","timestamp":"2026-06-07T03:00:01.000Z","userType":"external","entrypoint":"cli","cwd":"/p"}}"#, "\n",
            r#"{{"type":"assistant","uuid":"ca1","parentUuid":"c1","timestamp":"2026-06-07T03:00:02.000Z","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"ct1","name":"Edit","input":{{"file_path":"{op}","old_string":"x","new_string":"y"}}}}]}}}}"#, "\n",
        ), op = OWN_PLAN),
    );
}

#[test]
fn plan_audit_warns_on_edits_to_an_unbound_plan() {
    let h = Home::new();
    audit_scenario(&h);
    let out = h.run(&["plan", &at(AUDITEE), "--audit"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout
            .contains(&format!("binds    {AUDITEE} -> {OWN_PLAN}")),
        "own binding status:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains(&format!("edits    {OWN_PLAN}  1 mutation(s)"))
            && out.stdout.contains("[bound by this session]"),
        "own-plan edits are fine:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains(&format!("edits    {SHARED_PLAN}  2 mutation(s)"))
            && out.stdout.contains("[NOT bound by the mutating session]"),
        "unbound-plan edits flagged:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains(&format!(
            "warning: 2 mutation(s) to {SHARED_PLAN} by session {AUDITEE}, which does NOT \
             bind it (bound by {BINDER}, L2). Only the BOUND plan is re-injected in full \
             after a compaction."
        )),
        "the exact warning wording:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("src/main.rs"),
        "an ordinary file is never audited:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("ok: every audited"),
        "no all-clear line while a warning stands:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("(1 malformed line(s) skipped)"),
        "the scan's malformed line is booked:\n{}",
        out.stdout
    );

    let outj = h.run(&["plan", &at(AUDITEE), "--audit", "--format", "json"]);
    let rows: Vec<serde_json::Value> = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows[0]["kind"], "header");
    assert_eq!(rows[0]["mode"], "audit");
    let shared = rows
        .iter()
        .find(|r| r["kind"] == "plan-edit" && r["path"] == SHARED_PLAN)
        .expect("shared-plan row");
    assert_eq!(shared["mutations"], 2);
    assert_eq!(shared["bound_by_owner"], false);
    assert_eq!(shared["binder_session_id"], BINDER);
    assert_eq!(shared["binder_line"], 2);
    let own = rows
        .iter()
        .find(|r| r["kind"] == "plan-edit" && r["path"] == OWN_PLAN)
        .expect("own-plan row");
    assert_eq!(own["bound_by_owner"], true);
    assert_eq!(
        own["binder_session_id"], COBINDER,
        "the displayed binder prefers a NON-owner co-binder: {}",
        outj.stdout
    );
    let summary = rows.last().unwrap();
    assert_eq!(summary["warnings"], 1);
    assert_eq!(summary["plan_files_touched"], 2);
}

#[test]
fn plan_audit_clean_session_reports_ok_and_none() {
    // The BINDER session edits nothing: honest empty, exit 0.
    let h = Home::new();
    audit_scenario(&h);
    let out = h.run(&["plan", &at(BINDER), "--audit"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("edits    none")
            && out
                .stdout
                .contains("bash-side edits are outside this audit"),
        "honest empty + the audit's stated blind spot:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("ok: every audited"),
        "the all-clear line needs audited rows, not an empty set:\n{}",
        out.stdout
    );
}

#[test]
fn plan_audit_all_clear_and_multi_owner_attribution() {
    let h = Home::new();
    audit_scenario(&h);
    // COBINDER edits only a plan it binds: rows exist, zero warnings, the all-clear line.
    let ok = h.run(&["plan", &at(COBINDER), "--audit"]);
    assert!(ok.success, "stderr: {}", ok.stderr);
    assert!(
        ok.stdout.contains("ok: every audited") && !ok.stdout.contains("warning:"),
        "all-clear on own-plan-only edits:\n{}",
        ok.stdout
    );
    // A multi-session scope names the mutating owner on each edits row.
    let multi = h.run(&["plan", ENC, "--audit"]);
    assert!(multi.success, "stderr: {}", multi.stderr);
    assert!(
        multi
            .stdout
            .contains(&format!("2 mutation(s) by {AUDITEE}")),
        "edits rows carry the owner in a multi-owner scope:\n{}",
        multi.stdout
    );
}

// ── slug-only binding (Claude Code's first-slug law; the forked-session shape) ──

const SLUG_ENC: &str = "-Users-dev-example-project";

#[test]
fn plan_slug_only_binding_minted_at_compaction() {
    // No plan_mode anywhere; the FIRST slug carrier is the compact_boundary itself
    // (the fork/mint signature). Claude Code will inject/rebuild the slug's file, so
    // "no plan" would be a wrong answer.
    let h = Home::new();
    let sess = "4b3a2c1d-9e8f-4765-b432-10fedcba9877";
    h.write(
        &format!("{SLUG_ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"start"}}"#, "\n",
            r#"{"type":"system","subtype":"compact_boundary","uuid":"cb1","timestamp":"2026-06-07T06:00:00.000Z","slug":"minted-harbor-lantern","compactMetadata":{"trigger":"auto","preTokens":1000,"postTokens":100}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T06:00:01.000Z","slug":"minted-harbor-lantern","message":{"role":"assistant","content":[{"type":"text","text":"resumed"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["plan", &format!("@{sess}")]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("minted-harbor-lantern.md")
            && out.stdout.contains("[missing]")
            && out.stdout.contains("slug only")
            && out.stdout.contains("MINTED at a compaction boundary"),
        "{}",
        out.stdout
    );
    let j = h.run(&["plan", &format!("@{sess}"), "--format", "json"]);
    let row: serde_json::Value = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "plan")
        .expect("plan row");
    assert_eq!(row["binding_source"], "slug-only", "{}", j.stdout);
    assert_eq!(row["minted_at_compaction"], true, "{}", j.stdout);
    assert_eq!(row["slug"], "minted-harbor-lantern", "{}", j.stdout);
    assert_eq!(row["line"], 2, "{}", j.stdout);
}

#[test]
fn plan_slug_only_ordinary_carrier_and_invalid_slug() {
    let h = Home::new();
    // Ordinary first carrier: slug-only, NOT minted-at-compaction.
    let s1 = "5c4b3a2d-8f9e-4654-a321-0fedcba98766";
    h.write(
        &format!("{SLUG_ENC}/{s1}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","slug":"steady-reef-charter","message":{"role":"assistant","content":[{"type":"text","text":"working"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["plan", &format!("@{s1}"), "--format", "json"]);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "plan")
        .expect("row");
    assert_eq!(row["binding_source"], "slug-only", "{}", out.stdout);
    assert_eq!(row["minted_at_compaction"], false, "{}", out.stdout);

    // A slug value Claude Code itself would reject never binds (honest empty).
    let s2 = "6d5c4b3a-7e8f-4543-b210-fedcba987655";
    h.write(
        &format!("{SLUG_ENC}/{s2}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"b1","timestamp":"2026-06-07T05:00:01.000Z","slug":"Not_A_Valid_Slug","message":{"role":"assistant","content":[{"type":"text","text":"working"}]}}"#, "\n",
        ),
    );
    let bad = h.run(&["plan", &format!("@{s2}")]);
    assert!(bad.success);
    assert!(
        bad.stderr.contains("no plan file is bound"),
        "{} / {}",
        bad.stdout,
        bad.stderr
    );
}

#[test]
fn plan_mode_attachment_still_outranks_the_slug_law() {
    // Both present: the explicit attachment wins (it carries the verbatim path).
    let h = Home::new();
    let sess = "7e6d5c4b-6a9f-4432-a109-edcba9876544";
    h.write(
        &format!("{SLUG_ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","slug":"quiet-shoal-beacon","message":{"role":"assistant","content":[{"type":"text","text":"planning"}]}}"#, "\n",
            r#"{"type":"attachment","uuid":"p1","timestamp":"2026-06-07T05:00:02.000Z","slug":"quiet-shoal-beacon","attachment":{"type":"plan_mode","planFilePath":"/Users/dev/plans/quiet-shoal-beacon.md","isSubAgent":false}}"#, "\n",
        ),
    );
    let out = h.run(&["plan", &format!("@{sess}"), "--format", "json"]);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["kind"] == "plan")
        .expect("row");
    assert_eq!(row["binding_source"], "plan_mode", "{}", out.stdout);
    assert_eq!(
        row["plan_file"], "/Users/dev/plans/quiet-shoal-beacon.md",
        "{}",
        out.stdout
    );
}

#[test]
fn plan_slug_only_honors_plans_directory_and_reverse() {
    const SLUGSESS: &str = "dddd4444-5555-4666-8777-888899990000";
    let h = Home::new();
    // A RELATIVE plansDirectory joins to the config home.
    h.write_claude("settings.json", r#"{"plansDirectory":"my-plans"}"#);
    h.write_claude("my-plans/quiet-harbor-lantern.md", "plan body\n");
    h.write(
        &format!("{ENC}/{SLUGSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","slug":"quiet-harbor-lantern","message":{"role":"user","content":"forked work"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","timestamp":"2026-06-07T05:00:01.000Z","slug":"quiet-harbor-lantern","message":{"role":"assistant","content":[{"type":"text","text":"ok"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["plan", &at(SLUGSESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("my-plans/quiet-harbor-lantern.md") && out.stdout.contains("slug only"),
        "plansDirectory honored on the slug law:\n{}",
        out.stdout
    );
    // Reverse: which session binds this plan file - resolves through the slug law
    // too; a SECOND binder exercises the deterministic ordering.
    const SLUGSESS2: &str = "eeee4444-5555-4666-8777-888899990000";
    h.write(
        &format!("{ENC}/{SLUGSESS2}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","slug":"quiet-harbor-lantern","message":{"role":"user","content":"second fork"}}"#, "\n",
        ),
    );
    let plan_abs = h.root.join(".claude/my-plans/quiet-harbor-lantern.md");
    let rev = h.run(&["plan", "--reverse", plan_abs.to_str().unwrap()]);
    assert!(
        rev.stdout.contains(SLUGSESS) && rev.stdout.contains(SLUGSESS2),
        "reverse finds both slug-only binders:\n{}",
        rev.stdout
    );
    let a = rev.stdout.find(SLUGSESS).unwrap();
    let b = rev.stdout.find(SLUGSESS2).unwrap();
    assert!(a < b, "deterministic id order:\n{}", rev.stdout);
}

#[test]
fn plan_slug_over_length_never_binds() {
    const LSESS: &str = "ffff4444-5555-4666-8777-888899990000";
    let h = Home::new();
    let long = "a".repeat(121);
    h.write(
        &format!("{ENC}/{LSESS}.jsonl"),
        &format!(
            "{}\n",
            format_args!(
                r#"{{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","slug":"{long}","message":{{"role":"user","content":"work"}}}}"#
            )
        ),
    );
    let out = h.run(&["plan", &at(LSESS)]);
    assert!(
        !out.stdout.contains("slug only"),
        "a 121-char slug fails the validity rule and never binds:\n{}",
        out.stdout
    );
}
