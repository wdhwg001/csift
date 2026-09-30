//! Which Bash rows a `--file` target admits - a content anchor and a heuristic touch row
//! alike: the operand as written, or the file it resolves to through the record's shell
//! cwd, and nothing for any other file.

use super::*;

/// One turn whose single Bash call is `cmd`, run at `/work/proj`, answered by `result`.
fn one_bash_turn(cmd: &str, result: &str) -> Vec<(usize, Record)> {
    let use_line = format!(
        r#"{{"type":"assistant","timestamp":"2026-06-07T05:00:01.000Z","cwd":"/work/proj","message":{{"role":"assistant","content":[{{"type":"tool_use","id":"b1","name":"Bash","input":{{"command":{c}}}}}]}}}}"#,
        c = serde_json::to_string(cmd).unwrap()
    );
    numbered(&[
        r#"{"type":"user","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"go"}}"#,
        use_line.leak(),
        result.to_string().leak(),
    ])
}

const WRITE_RESULT: &str = r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":""}]}}"#;
const READ_RESULT: &str = r#"{"type":"user","timestamp":"2026-06-07T05:00:02.000Z","toolUseResult":{"stdout":"alpha\n","stderr":"","interrupted":false},"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"b1","content":"alpha\n"}]}}"#;

fn anchors_for(records: &[(usize, Record)], target: &str) -> TurnBashAnchors {
    let view = view_of(records);
    collect_turn_bash_anchors(
        records,
        view.turns()[0].as_slice(),
        &Target::new(Some(target)),
        &Default::default(),
    )
}

#[test]
fn an_anchor_on_another_file_is_never_admitted() {
    // The write and the read both resolve to /work/proj/notes.md; a target naming a
    // different file admits neither and suppresses no touch row.
    for (cmd, result) in [
        ("cat > notes.md <<'EOF'\nalpha\nEOF", WRITE_RESULT),
        ("cat notes.md", READ_RESULT),
    ] {
        let records = one_bash_turn(cmd, result);
        let out = anchors_for(&records, "/work/proj/other.md");
        assert!(
            out.events.is_empty() && out.suppress.is_empty(),
            "{cmd:?}: {:?}",
            out.events
        );
        // The same turn with the target naming the file does admit it, so the empty
        // answer above is the target's doing and not a gate's.
        assert_eq!(anchors_for(&records, "/work/proj/notes.md").events.len(), 1);
    }
}

#[test]
fn an_operand_spelled_as_the_target_is_admitted_though_its_resolved_form_is_not() {
    // `./notes.md` resolves to /work/proj/notes.md, which the target `./notes.md` does not
    // name (the dot is no path component of it); the operand as written is the target
    // itself, so the anchor is admitted on the verbatim match.
    for (cmd, result) in [
        ("cat > ./notes.md <<'EOF'\nalpha\nEOF", WRITE_RESULT),
        ("cat ./notes.md", READ_RESULT),
    ] {
        let records = one_bash_turn(cmd, result);
        let out = anchors_for(&records, "./notes.md");
        assert_eq!(out.events.len(), 1, "{cmd:?}: {:?}", out.events);
        assert!(
            matches!(&out.events[0].kind, EventKind::FullSnapshot { content, .. } if content == "alpha\n"),
            "{:?}",
            out.events
        );
    }
}

#[test]
fn a_touch_row_spelled_as_the_target_is_admitted_though_its_resolved_form_is_not() {
    // `sed -i` is no content anchor, so the heuristic touch row is the only trace. Its
    // operand `./notes.md` resolves to /work/proj/notes.md, which the target `./notes.md`
    // does not name; the operand as written does, so the row is kept.
    let records = one_bash_turn("sed -i 's/a/b/' ./notes.md", WRITE_RESULT);
    let touches = |target: &str| {
        extract_events(&records, target)
            .into_iter()
            .filter(|e| matches!(e.kind, EventKind::BashTouch { .. }))
            .count()
    };
    assert_eq!(touches("./notes.md"), 1);
    assert_eq!(touches("/work/proj/notes.md"), 1);
    assert_eq!(touches("/work/proj/other.md"), 0);
}
