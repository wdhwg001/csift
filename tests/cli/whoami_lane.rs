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
fn whoami_rejects_every_target_except_trap_and_main() {
    let h = Home::new();
    lane_fixture(&h);
    let out = h.run_with_env(&["whoami", &at(SESS)], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(!out.success, "a session target is not a whoami question");
    assert!(
        out.stderr.contains("whoami accepts no target except"),
        "the pointed rejection:\n{}",
        out.stderr
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

/// A session-id variable set to anything but a session uuid is refused, naming the value and
/// the `@<uuid>` remedy, on `whoami` and on every `@main` resolution alike. `whoami` printed
/// `session  not-a-uuid` and exited 0, answering for a session no transcript is named after.
#[test]
fn a_malformed_session_env_value_is_refused_by_name() {
    let h = populated_home();
    for var in ["CLAUDE_CODE_SESSION_ID", "CODEX_COMPANION_SESSION_ID"] {
        for argv in [&["whoami"][..], &["list", "@main"][..], &["plan"][..]] {
            let out = h.run_with_env(argv, &[(var, "not-a-uuid")]);
            assert!(!out.success, "{var} {argv:?}: {}", out.stdout);
            assert!(
                out.stderr
                    .contains(&format!("{var} is set to `not-a-uuid`"))
                    && out.stderr.contains("@<uuid>"),
                "{var} {argv:?}: {}",
                out.stderr
            );
        }
    }
    // The refused value is quoted whole up to 64 characters and cut, with `...`, past them.
    let at_cap = "x".repeat(64);
    let over = "x".repeat(65);
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", at_cap.as_str())]);
    assert!(
        out.stderr.contains(&format!("is set to `{at_cap}`,")),
        "{}",
        out.stderr
    );
    let out = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", over.as_str())]);
    assert!(
        out.stderr.contains(&format!("is set to `{at_cap}...`,")),
        "{}",
        out.stderr
    );
    // A blank value is still "not set", and a uuid still resolves.
    let blank = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", "  ")]);
    assert!(!blank.stderr.contains("is set to"), "{}", blank.stderr);
    let good = h.run_with_env(&["whoami"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(good.success, "{}", good.stderr);
}

/// `@main` names its inference only once it has one: an unset or malformed variable resolved
/// nothing, so the refusal stands alone rather than under a note saying the session was
/// resolved.
#[test]
fn the_at_main_note_follows_a_resolution_and_never_a_refusal() {
    let h = Home::new();
    lane_fixture(&h);
    let note = "@main resolved the TOP-LEVEL session via env";
    let bad = h.run_with_env(
        &["list", "@main"],
        &[("CLAUDE_CODE_SESSION_ID", "not-a-uuid")],
    );
    assert!(!bad.success, "{}", bad.stdout);
    assert!(
        bad.stderr.contains("is set to `not-a-uuid`") && !bad.stderr.contains(note),
        "{}",
        bad.stderr
    );
    let unset = h.run(&["list", "@main"]);
    assert!(!unset.success, "{}", unset.stdout);
    assert!(
        unset.stderr.contains("is not set") && !unset.stderr.contains(note),
        "{}",
        unset.stderr
    );
    let good = h.run_with_env(&["list", "@main"], &[("CLAUDE_CODE_SESSION_ID", SESS)]);
    assert!(good.success, "{}", good.stderr);
    assert!(good.stderr.contains(note), "{}", good.stderr);
}
