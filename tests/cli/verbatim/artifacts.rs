//! verbatim artifacts: --out files, JSON units, determinism, the golden baseline.

use crate::harness::*;

#[test]
fn turns_json_units_carry_id_domain_discriminators() {
    // turns per-unit JSON gains is_subagent + parent_session_id (top-level run here).
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ask a real question"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"a substantive reply"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["verbatim", at(SESS).as_str(), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let unit = objs
        .iter()
        .find(|o| o["role"] == "user" || o["role"] == "assistant")
        .expect("a per-unit record present");
    assert_eq!(unit["is_subagent"], serde_json::json!(false));
    assert_eq!(unit["session_id"], serde_json::json!(SESS));
    assert_eq!(unit["parent_session_id"], serde_json::json!(SESS));
}

#[test]
fn turns_deterministic_byte_identical() {
    let h = turns_home();
    let a = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    let b = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "10000",
    ]);
    assert_eq!(
        a.stdout, b.stdout,
        "two identical invocations must be byte-identical"
    );
}

#[test]
fn turns_out_file_holds_full_reconstruction() {
    let h = turns_home();
    let out_path = h.root.join("turns-out.md");
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("wrote full reconstruction"),
        "{}",
        out.stdout
    );
    let body = std::fs::read_to_string(&out_path).expect("out file written");
    assert!(body.contains("▽ L"), "out file carries the rendered turns");
    assert!(
        body.contains("compaction boundary"),
        "out file carries banners"
    );
}

#[test]
fn turns_json_out_file_is_verbatim() {
    let h = turns_home();
    let out_path = h.root.join("turns.json");
    let out = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let body = std::fs::read_to_string(&out_path).expect("json out file");
    // The huge user unit's full verbatim text (un-truncated) is in the file.
    assert!(
        body.contains("HEADuser"),
        "out json carries the unit objects"
    );
    // Every non-blank line is valid JSON.
    for line in body.lines().filter(|l| !l.trim().is_empty()) {
        serde_json::from_str::<serde_json::Value>(line).expect("each out line is JSON");
    }
}

#[test]
fn turns_multi_session_text_has_blank_separator_and_both_sessions() {
    // A project-dir target → both sessions in the project are rendered, separated by a
    // blank line (the `if !first { println!() }` arm). Sessions are sorted by id.
    let h = turns_two_sessions_home();
    let out = h.run(&["verbatim", ENC, "--no-subagents", "--budget", "40000"]);
    assert!(out.success, "stderr: {}", out.stderr);
    let session_headers = out
        .stdout
        .lines()
        .filter(|l| l.starts_with("SESSION "))
        .count();
    assert!(
        session_headers >= 2,
        "both sessions rendered: {}",
        out.stdout
    );
    // The clean session's content is present.
    assert!(out.stdout.contains("clean session ask"), "{}", out.stdout);
}

#[test]
fn turns_multi_session_json_runs_both() {
    // JSON over two sessions (a project-dir target) → both sessions' units emitted.
    let h = turns_two_sessions_home();
    let out = h.run(&[
        "verbatim",
        ENC,
        "--no-subagents",
        "--budget",
        "40000",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let objs = json_lines(&out.stdout);
    let sessions: std::collections::BTreeSet<&str> = objs
        .iter()
        .filter_map(|o| o.get("session_id").and_then(|v| v.as_str()))
        .collect();
    assert!(
        sessions.len() >= 2,
        "both sessions present in JSON: {sessions:?}"
    );
}

/// Run csift with stderr wired to the SAME pipe as stdout, which is what `2>&1` does,
/// and return the interleaved stream. A per-stream capture cannot see the order between
/// them: both come back complete whichever one the process wrote first.
fn merged(h: &Home, args: &[&str]) -> String {
    use std::io::Read as _;
    let (mut reader, writer) = std::io::pipe().expect("an anonymous pipe");
    let dup = writer
        .try_clone()
        .expect("a second handle on the write end");
    let mut child = h
        .cmd(args)
        .stdout(std::process::Stdio::from(writer))
        .stderr(std::process::Stdio::from(dup))
        .spawn()
        .expect("spawn csift");
    // Both parent-side handles are gone with the Stdio conversions, so the read ends at
    // the child's exit rather than blocking on a write end this process still holds.
    let mut s = String::new();
    reader
        .read_to_string(&mut s)
        .expect("read the merged stream");
    child.wait().expect("wait csift");
    s
}

/// `--out` REFUSES to clobber a file with an empty reconstruction, and says so on stderr -
/// from `commands::recover::write_out_guarded`, which no module carrying the print lint
/// contains. So the flush that keeps the note behind the rows it is about cannot be
/// derived from the lint, and this is what proves it is there: with a budget too small to
/// render anything, the interleaved stream carries the per-session skip note FIRST and the
/// refusal SECOND. Without the flush the buffer still holds the skip note when the refusal
/// is printed, and the two come out the other way round.
#[test]
fn the_out_refusal_note_follows_the_rows_that_explain_it() {
    let h = turns_home();
    let out_path = h.root.join("already-there.md");
    std::fs::write(&out_path, "a file the run must not clobber\n").expect("seed the out file");
    let stream = merged(
        &h,
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "1",
            "--out",
            out_path.to_str().unwrap(),
        ],
    );
    let skip = stream
        .find("skipped")
        .unwrap_or_else(|| panic!("the budget-too-small row is the stdout half: {stream}"));
    let note = stream
        .find("nothing reconstructed in range")
        .unwrap_or_else(|| panic!("the refusal note is the stderr half: {stream}"));
    assert!(
        skip < note,
        "the stderr note overtook the buffered rows it is about:\n{stream}"
    );
    assert_eq!(
        std::fs::read_to_string(&out_path).expect("out file"),
        "a file the run must not clobber\n",
        "an empty reconstruction leaves the file alone"
    );
}

/// The ordinary shape of the same block: the reconstruction lands, and its one-line
/// receipt is the LAST thing on a stream that carries no stderr at all.
#[test]
fn the_out_receipt_closes_the_stream_when_the_file_is_written() {
    let h = turns_home();
    let out_path = h.root.join("written.md");
    let target = at(SESS);
    let args = [
        "verbatim",
        target.as_str(),
        "--no-subagents",
        "--budget",
        "40000",
        "--out",
        out_path.to_str().unwrap(),
    ];
    let plain = h.run(&args);
    assert!(plain.success, "stderr: {}", plain.stderr);
    assert_eq!(plain.stderr, "", "the written shape says nothing on stderr");
    let stream = merged(&h, &args);
    let last = stream
        .lines()
        .filter(|l| !l.trim().is_empty())
        .next_back()
        .expect("a non-empty stream");
    assert!(
        last.starts_with("(wrote full reconstruction to "),
        "the receipt closes the stream, it does not overtake it: {last}"
    );
}

/// The JSON stream cannot reach that refusal at all, and saying so is the point: its blob
/// opens with the envelope header, which is written whatever the budget selected, so the
/// blob is never empty and the file is always written. What IS observable on the JSON side
/// is the failing write - an `--out` path whose directory does not exist - and its error
/// belongs after the rows, which the writer's own drop takes care of.
#[test]
fn the_json_out_blob_is_never_empty_and_its_write_error_follows_the_rows() {
    let h = turns_home();
    let out_path = h.root.join("envelope-only.json");
    let ok = h.run(&[
        "verbatim",
        at(SESS).as_str(),
        "--no-subagents",
        "--budget",
        "1",
        "--format",
        "json",
        "--out",
        out_path.to_str().unwrap(),
    ]);
    assert!(ok.success, "stderr: {}", ok.stderr);
    assert!(
        !ok.stderr.contains("nothing reconstructed in range"),
        "a JSON blob always carries its header, so the refusal arm is unreachable: {}",
        ok.stderr
    );
    let body = std::fs::read_to_string(&out_path).expect("the json out file is written");
    assert!(
        body.contains("\"kind\":\"header\""),
        "the envelope header is what keeps the blob non-empty: {body}"
    );

    let missing = h.root.join("no-such-dir").join("x.json");
    let stream = merged(
        &h,
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--format",
            "json",
            "--out",
            missing.to_str().unwrap(),
        ],
    );
    let summary = stream
        .find("\"kind\":\"summary\"")
        .unwrap_or_else(|| panic!("the envelope summary is the last stdout row: {stream}"));
    let err = stream
        .find("csift: error: cannot write --out file")
        .unwrap_or_else(|| panic!("an unwritable --out path is an ordinary error: {stream}"));
    assert!(
        summary < err,
        "the write error overtook the rows it follows:\n{stream}"
    );
}

/// The executable re-capture procedure for the baseline above - NOT a behavioral test
/// (ignored by default; the fixture is a temp Home, so no hand-run command can reproduce
/// it). Writes the current eot-only output to tests/turns_pre_feature_baseline.txt.
#[test]
#[ignore = "capture tool — rewrites tests/turns_pre_feature_baseline.txt; run only on an intended output change"]
fn recapture_turns_pre_feature_baseline() {
    let h = turns_home();
    let out = h.run_with_env(
        &[
            "verbatim",
            at(SESS).as_str(),
            "--no-subagents",
            "--budget",
            "40000",
            "--agent-msgs",
            "eot-only",
        ],
        &[("TZ", "UTC")],
    );
    assert!(out.success, "stderr: {}", out.stderr);
    let dest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/turns_pre_feature_baseline.txt");
    std::fs::write(&dest, &out.stdout).expect("write baseline");
    eprintln!("captured {} bytes to {}", out.stdout.len(), dest.display());
}
