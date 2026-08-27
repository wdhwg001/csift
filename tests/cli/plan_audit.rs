//! plan --audit: plan-file edits joined against corpus plan bindings.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const BINDER: &str = "11111111-2222-4333-8444-555566667777";
const AUDITEE: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeffff0000";
const SHARED_PLAN: &str = "/Users/dev/.claude/plans/amber-drifting-kite.md";
const OWN_PLAN: &str = "/Users/dev/.claude/plans/copper-still-pond.md";

/// BINDER binds the shared plan; AUDITEE binds its own plan, edits its own plan once,
/// edits the shared plan twice, and edits an ordinary file (never audited).
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
        ), op = OWN_PLAN, sp = SHARED_PLAN),
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
}
