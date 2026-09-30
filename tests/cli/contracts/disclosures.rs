//! A closing disclosure prints exactly when there is something to disclose - BOTH ways.
//!
//! Three surfaces end a stream with a note on stderr about what the stream does NOT carry:
//! what `search --raw` had no bytes for, what `show --raw`'s cap dropped, and what
//! `verbatim`'s budget left unselected. Silence above zero is the silent truncation the
//! output contract forbids - the reader has a short stream and no reason to doubt it. A
//! note at zero is the same lie the other way round, a loss claimed where nothing was lost.
//! So every predicate is pinned on both sides: a one-sided assertion also passes for a
//! condition that can never fire.

use crate::harness::*;

const ENC: &str = "-Users-dev-disclose";

/// A transcript with two matching exchanges and one torn line, beside an elicitation
/// sidecar holding an unresolved pending whose rendered text matches the same pattern - so
/// all three of `--raw`'s accounting notes have something to say at once.
fn raw_notes_home() -> (Home, String) {
    let h = Home::new();
    let sess = "44444444-2222-4333-8444-555555555555";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ZZNOTE the first ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ZZNOTE first answer"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"ZZNOTE the second ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T06:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ZZNOTE second answer"}]}}"#, "\n",
            r#"{"type":"user","role":"user" broken json after the marker}"#, "\n",
        ),
    );
    h.write(
        &format!("{ENC}/{sess}/elicitations.jsonl"),
        concat!(
            r#"{"type":"assistant","uuid":"e-k01","timestamp":"2026-06-07T07:00:00.000Z","sessionId":"44444444-2222-4333-8444-555555555555","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"k01","name":"AskUserQuestion","input":{"questions":[{"question":"ZZNOTE pending question"}]}}]},"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"k01"}"#, "\n",
        ),
    );
    (h, at(sess))
}

/// The same shape with nothing to disclose: two clean exchanges, no torn line, no sidecar.
fn clean_home() -> (Home, String) {
    let h = Home::new();
    let sess = "44444444-2222-4333-8444-5555555555ff";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"ZZNOTE the only ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"ZZNOTE the only answer"}]}}"#, "\n",
        ),
    );
    (h, at(sess))
}

const SIDECAR_NOTE: &str = "sidecar-merged record(s) have no physical";
const CAP_NOTE: &str = "dropped by --max-count";
const MALFORMED_NOTE: &str = "malformed line(s) skipped";

#[test]
fn search_raw_names_every_record_it_had_no_bytes_for_and_stays_silent_otherwise() {
    let (h, t) = raw_notes_home();
    // stdout is the pure jsonl stream; all three notes are stderr's job.
    let out = h.run(&["search", "ZZNOTE", t.as_str(), "--raw"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.lines().filter(|l| !l.trim().is_empty()).count() >= 2,
        "the matching transcript lines are the stream: {}",
        out.stdout
    );
    assert!(
        out.stderr.contains(&format!("1 {SIDECAR_NOTE}")),
        "a sidecar-merged hit has no transcript line to print, so it is named: {}",
        out.stderr
    );
    assert!(
        out.stderr.contains(&format!("1 {MALFORMED_NOTE}")),
        "the torn line is counted, never hidden: {}",
        out.stderr
    );
    assert!(
        !out.stderr.contains(CAP_NOTE),
        "nothing was capped here: {}",
        out.stderr
    );

    // The cap note, on the same fixture, with a cap that drops one exchange.
    let capped = h.run(&["search", "ZZNOTE", t.as_str(), "--raw", "--max-count", "1"]);
    assert!(capped.success, "stderr: {}", capped.stderr);
    assert!(
        capped
            .stderr
            .contains(&format!("1 later exchange(s) {CAP_NOTE}")),
        "a cap that dropped an exchange says so, with the side it dropped: {}",
        capped.stderr
    );

    // Nothing to disclose: no note of any kind, on a stream that is complete.
    let (h2, t2) = clean_home();
    let clean = h2.run(&["search", "ZZNOTE", t2.as_str(), "--raw"]);
    assert!(clean.success, "stderr: {}", clean.stderr);
    for note in [SIDECAR_NOTE, MALFORMED_NOTE, CAP_NOTE] {
        assert!(
            !clean.stderr.contains(note),
            "nothing was lost, so {note:?} must not be claimed: {}",
            clean.stderr
        );
    }
}

#[test]
fn show_raw_names_the_lines_its_cap_dropped_and_says_nothing_uncapped() {
    let (h, t) = raw_notes_home();
    let capped = h.run(&[
        "show",
        t.as_str(),
        "--line",
        "1..4",
        "--raw",
        "--max-count",
        "1",
    ]);
    assert!(capped.success, "stderr: {}", capped.stderr);
    assert_eq!(
        capped
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        1,
        "the cap keeps the first line only: {}",
        capped.stdout
    );
    assert!(
        capped
            .stderr
            .contains("+3 more line(s) beyond the 1-unit cap"),
        "the drop is counted: {}",
        capped.stderr
    );
    assert!(
        capped.stderr.contains("--line 2..4 --raw"),
        "and carries the exact command that continues it: {}",
        capped.stderr
    );

    // `--max-count 0` is uncapped, so there is no drop to name.
    let whole = h.run(&[
        "show",
        t.as_str(),
        "--line",
        "1..4",
        "--raw",
        "--max-count",
        "0",
    ]);
    assert!(whole.success, "stderr: {}", whole.stderr);
    assert_eq!(
        whole
            .stdout
            .lines()
            .filter(|l| !l.trim().is_empty())
            .count(),
        4,
        "every addressed line: {}",
        whole.stdout
    );
    assert!(
        !whole.stderr.contains("beyond the"),
        "nothing was dropped, so no cap is claimed: {}",
        whole.stderr
    );
}

#[test]
fn verbatim_names_automation_in_scope_that_its_budget_did_not_select() {
    // The header's `selected N user (M automation triggers)` counts what the BUDGET took.
    // This second line counts what the SESSION holds, and prints only when that is more -
    // without it a monitor-heavy session reads as "no automation" whenever the recency
    // window reached none of its pulses. Three pulses, each costing about a third of the
    // tight budget, so the split is wide on both sides rather than balanced on one value.
    let h = Home::new();
    let sess = "55555555-2222-4333-8444-555555555555";
    let mut jsonl = String::new();
    for i in 0..3 {
        let summary = format!(
            "Background job {i} completed: {}",
            "checked one more path segment, ".repeat(30)
        );
        jsonl.push_str(&format!(
            r#"{{"type":"user","uuid":"n{i}","timestamp":"2026-06-07T0{h}:10:00.000Z","message":{{"role":"user","content":"<task-notification>\n<task-id>job{i}</task-id>\n<status>completed</status>\n<summary>{summary}</summary>\n</task-notification>"}}}}"#,
            h = 5 + i
        ));
        jsonl.push('\n');
        jsonl.push_str(&format!(
            r#"{{"type":"assistant","uuid":"m{i}","parentUuid":"n{i}","timestamp":"2026-06-07T0{h}:10:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"ack {filler}"}}]}}}}"#,
            h = 5 + i,
            filler = "filler ".repeat(200)
        ));
        jsonl.push('\n');
    }
    h.write(&format!("{ENC}/{sess}.jsonl"), &jsonl);
    let t = at(sess);

    let tight = h.run(&["verbatim", t.as_str(), "--budget", "1500", "--no-subagents"]);
    assert!(tight.success, "stderr: {}", tight.stderr);
    assert!(
        tight
            .stdout
            .contains("selected 1 user (1 automation trigger: 1 task)"),
        "the budget reaches one of the three pulses: {}",
        tight.stdout
    );
    assert!(
        tight
            .stdout
            .contains("in scope (not all selected): 3 automation triggers — 3 task"),
        "so the session's own composition is named beside it: {}",
        tight.stdout
    );

    let wide = h.run(&[
        "verbatim",
        t.as_str(),
        "--budget",
        "20000",
        "--no-subagents",
    ]);
    assert!(wide.success, "stderr: {}", wide.stderr);
    assert!(
        wide.stdout
            .contains("selected 3 user (3 automation triggers: 3 task)"),
        "a wide budget reaches all three: {}",
        wide.stdout
    );
    assert!(
        !wide.stdout.contains("in scope (not all selected)"),
        "and then there is no gap to name: {}",
        wide.stdout
    );
}

/// The three terminal modes - `-c`, `-l`, `--count-by` - print a bare datum on stdout, a
/// count, an id list or a census; a torn line was never matched, so the datum is a claim
/// about the parseable lines only and each mode names the skipped line on stderr, in the
/// `--raw` form. `-c --format json` carries it in the summary's `skipped_lines`, as the
/// census's JSON summary always has. Silent on a clean transcript, both ways. On a ZERO match
/// the diagnosis's caveat already names the line and what it means for the absence, so every
/// mode names it exactly once.
#[test]
fn every_count_mode_names_a_torn_line_and_stays_silent_on_a_clean_scope() {
    let (h, t) = raw_notes_home();
    let (h2, t2) = clean_home();
    for mode in [
        vec!["-c"],
        vec!["-l"],
        vec!["--count-by", "label"],
        vec!["--count-by", "turn"],
    ] {
        let mut args = vec!["search", "ZZNOTE", t.as_str()];
        args.extend(mode.iter().copied());
        let out = h.run(&args);
        assert!(out.success, "{mode:?} stderr: {}", out.stderr);
        assert!(
            out.stderr
                .contains(&format!("csift: note: 1 {MALFORMED_NOTE}")),
            "{mode:?} must name the torn line: {}",
            out.stderr
        );
        assert!(
            !out.stdout.contains(MALFORMED_NOTE),
            "{mode:?}'s stdout is the datum alone: {}",
            out.stdout
        );
        let mut clean = vec!["search", "ZZNOTE", t2.as_str()];
        clean.extend(mode.iter().copied());
        let out = h2.run(&clean);
        assert!(
            !out.stderr.contains(MALFORMED_NOTE),
            "{mode:?} claims a torn line on a clean transcript: {}",
            out.stderr
        );
    }
    for mode in [
        vec!["-c"],
        vec!["-l"],
        vec!["--count-by", "label"],
        vec!["--raw"],
        vec![],
    ] {
        let mut args = vec!["search", "zzabsentqq", t.as_str()];
        args.extend(mode.iter().copied());
        let out = h.run(&args);
        assert!(out.success, "{mode:?} stderr: {}", out.stderr);
        assert_eq!(
            out.stderr.matches(MALFORMED_NOTE).count(),
            1,
            "{mode:?} names the torn line once on a zero match: {}",
            out.stderr
        );
        assert!(
            out.stderr
                .contains("the absence is definitive for parseable lines only"),
            "{mode:?}: {}",
            out.stderr
        );
    }
    for (home, target, want) in [(&h, t.as_str(), 1), (&h2, t2.as_str(), 0)] {
        for mode in [vec!["-c"], vec!["--count-by", "label"]] {
            let mut args = vec!["search", "ZZNOTE", target, "--format", "json"];
            args.extend(mode.iter().copied());
            let out = home.run(&args);
            let summary = json_rows(&out.stdout, "summary");
            assert_eq!(
                summary[0]["skipped_lines"], want,
                "{mode:?} --format json: {}",
                out.stdout
            );
        }
    }
}
