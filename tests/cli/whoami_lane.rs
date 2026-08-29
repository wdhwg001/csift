//! Lane honesty: env-only whoami reports unknowable fields as null; @main names its inference.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "5d4c3b2a-1e0f-4987-b654-3210fedcba98";

fn lane_fixture(h: &Home) {
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"hello lane"}}"#,
            "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"hi"}]}}"#,
            "\n",
        ),
    );
}

#[test]
fn whoami_env_form_reports_lane_fields_as_null_and_says_why() {
    // The env names the TOP-LEVEL session in every lane (a subagent is handed its
    // parent's id), so env-only resolution cannot know whether the CALLER is that
    // session: the three lane fields are null, never a fabricated false/0/echo, and
    // stderr names the resolution path.
    let h = Home::new();
    lane_fixture(&h);
    let out = h.run_with_env(
        &["whoami", "--format", "json"],
        &[("CLAUDE_CODE_SESSION_ID", SESS)],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let row: serde_json::Value = out
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "identity")
        .expect("identity row");
    assert_eq!(row["session_id"], SESS);
    assert!(row["is_subagent"].is_null(), "unknowable: {}", out.stdout);
    assert!(row["parent_session_id"].is_null(), "{}", out.stdout);
    assert!(row["depth"].is_null(), "{}", out.stdout);
    assert!(
        out.stderr.contains("TOP-LEVEL") && out.stderr.contains("@trap"),
        "resolution-path note on stderr: {}",
        out.stderr
    );

    // Text form: the lane line states the limit in-band.
    let text = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(
        text.stdout.contains("lane     unknown from env alone"),
        "{}",
        text.stdout
    );
}

#[test]
fn at_main_resolution_names_its_inference_on_stderr() {
    // csift cannot know the caller's lane, so EVERY @main resolution says what it
    // inferred - stderr only, stdout stays pure for pipes.
    let h = Home::new();
    lane_fixture(&h);
    let out = h.run_with_env(&["list", "@main"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(out.stdout.contains(SESS), "{}", out.stdout);
    assert!(
        out.stderr.contains("@main resolved the TOP-LEVEL session") && out.stderr.contains("@trap"),
        "unconditional lane note: {}",
        out.stderr
    );
    // stdout purity: the note never leaks into the data stream.
    assert!(
        !out.stdout.contains("csift: note:"),
        "stdout stays pure: {}",
        out.stdout
    );
}
