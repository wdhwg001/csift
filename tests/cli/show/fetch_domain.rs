//! The refetch law's edge: an explicit address renders the record it names even when csift
//! models NO leaf for it. `classify` deliberately emits nothing for an `isMeta` user record
//! matching no harness marker (the M2b rule - better silent than mislabeled `user.message`),
//! and until the fetch domain caught that case the address bailed with "no such record(s)"
//! on a line that is plainly a message.

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "00000000-0000-4000-8000-0000000000fd";

/// A transcript whose line 2 is an `isMeta` user record carrying prose that matches no
/// harness marker at all - the shape a novel hook wrapper or a generic timer tick takes.
fn home_with_an_unlabeled_record() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"m0","isMeta":true,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"zzunmodeled harness pseudo-turn nobody models yet"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charting"}]}}"#, "\n",
        ),
    );
    h
}

#[test]
fn an_addressed_record_with_no_modeled_leaf_renders_instead_of_bailing() {
    let h = home_with_an_unlabeled_record();
    let out = h.run(&["show", at(SESS).as_str(), "--line", "2"]);
    assert!(
        out.success,
        "an address must render, not bail: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("(no label)"),
        "the empty label is named, never borrowed from a neighbouring leaf:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("zzunmodeled harness pseudo-turn"),
        "the record's own text renders:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("L2"),
        "under its own address:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("fetched 1 record unit(s)"),
        "counted as one unit:\n{}",
        out.stdout
    );
}

#[test]
fn the_unlabeled_unit_carries_a_null_label_and_an_empty_label_set_in_json() {
    let h = home_with_an_unlabeled_record();
    let out = h.run(&[
        "show",
        at(SESS).as_str(),
        "--uuid",
        "m0",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let rows = json_rows(&out.stdout, "record");
    assert_eq!(rows.len(), 1, "one record row in:\n{}", out.stdout);
    assert!(
        rows[0]["label"].is_null(),
        "no leaf to name: {}",
        rows[0]["label"]
    );
    assert_eq!(
        rows[0]["labels"].as_array().map(Vec::len),
        Some(0),
        "and an EMPTY label set, never a fabricated one: {}",
        rows[0]["labels"]
    );
    assert_eq!(rows[0]["uuid"], "m0");
    assert_eq!(rows[0]["line"], 2);
}

#[test]
fn a_scan_still_sees_nothing_so_no_census_or_selector_result_moves() {
    // The unlabeled unit is reachable ONLY through an address: a bare scan, a `-t` query and
    // a label census all stay exactly where they were, so the fetch-domain fix cannot leak
    // an unmodeled record into a count.
    let h = home_with_an_unlabeled_record();
    let out = h.run(&["search", "zzunmodeled", at(SESS).as_str()]);
    assert!(
        out.success,
        "a definitive absence is exit 0: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("no matching exchanges"),
        "a scan never emits the unlabeled record:\n{}",
        out.stdout
    );
    let census = h.run(&["search", "", at(SESS).as_str(), "--count-by", "label"]);
    assert!(census.success, "stderr: {}", census.stderr);
    assert!(
        !census.stdout.contains("no label"),
        "and it never enters the label census:\n{}",
        census.stdout
    );
}

#[test]
fn a_tagless_peer_record_is_unlabeled_and_still_addressable() {
    // T3. Four corpus records carry `origin.kind:"peer"` with NO peer tag at all - the relay
    // preamble followed by bare prose. csift classifies a peer by TAG, matching Claude Code's own
    // reader, so such a record falls into the same empty-label hole as any other isMeta
    // pseudo-turn: no leaf, no census row, no `-t` result. That is deliberate; what is NOT
    // acceptable is the address failing on it. Fails pre-fix, where `show --line` bailed.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000fc";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"user","uuid":"t0","isMeta":true,"promptSource":"system","userType":"external","origin":{"kind":"peer","from":"probe-external","verifiedPeerPid":4242},"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"Another Claude session sent a message:\nzztagless reply with exactly PONG\n\nThis came from another Claude session."}}"#, "\n",
        ),
    );
    // No tag means no peer section, so no comm leaf and nothing for a scan to emit.
    let scan = h.run(&["search", "zztagless", &at(sess)]);
    assert!(
        scan.success,
        "a definitive absence is exit 0: {}",
        scan.stderr
    );
    assert!(
        scan.stdout.contains("no matching exchanges"),
        "classification is by TAG, so a tagless peer record carries no label:\n{}",
        scan.stdout
    );
    let inbox = h.run(&[
        "search",
        "zztagless",
        &at(sess),
        "-t",
        "agent.communication.inbox",
    ]);
    assert!(
        inbox.stdout.contains("no matching exchanges"),
        "and it is certainly not an inbound comm:\n{}",
        inbox.stdout
    );
    // But the address still renders it - that is the whole point of the fetch domain.
    let shown = h.run(&["show", &at(sess), "--line", "2"]);
    assert!(
        shown.success,
        "the address must render, not bail: {}",
        shown.stderr
    );
    assert!(
        shown.stdout.contains("(no label)"),
        "as an honest unlabeled unit:\n{}",
        shown.stdout
    );
    assert!(
        shown.stdout.contains("zztagless reply with exactly PONG"),
        "carrying the record's own text:\n{}",
        shown.stdout
    );
}

#[test]
fn an_unlabeled_unit_enters_no_census_axis() {
    // T6. The unlabeled unit exists ONLY under an address. A census never addresses, so every
    // axis must read exactly the two LABELLED records of this fixture - the human opener and the
    // agent reply - and never the isMeta pseudo-turn between them. `matched_records` is the total
    // the axis saw (`excluded_records` is the subset outside its domain), so the same number is
    // the invariant on all five. Fails on any variant that lets the address fallback reach a scan.
    let h = home_with_an_unlabeled_record();
    for axis in ["label", "turn", "session", "pairing", "attachment"] {
        let out = h.run(&[
            "search",
            "",
            at(SESS).as_str(),
            "--count-by",
            axis,
            "--format",
            "json",
        ]);
        assert!(out.success, "{axis}: {}", out.stderr);
        let summary = json_summary(&out.stdout);
        assert_eq!(
            summary["matched_records"], 2,
            "{axis} must census the two LABELLED records only:\n{}",
            out.stdout
        );
        assert!(
            !out.stdout.contains("no label"),
            "{axis} must not mint a key for an unlabeled record:\n{}",
            out.stdout
        );
    }
}

#[test]
fn a_line_that_is_not_a_record_at_all_stays_a_hard_miss() {
    // The fetch domain did not widen to everything: a session-state cache line carries no
    // message, no attachment and no promoted leaf, so the address is still a hard miss that
    // points at `--raw` rather than rendering a fabricated unit.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000fe";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"last-prompt","lastPrompt":"chart the reef"}"#, "\n",
        ),
    );
    let out = h.run(&["show", at(sess).as_str(), "--line", "2"]);
    assert!(!out.success, "stdout: {}", out.stdout);
    assert!(
        out.stderr.contains("no such record(s)"),
        "stderr: {}",
        out.stderr
    );
    let raw = h.run(&["show", at(sess).as_str(), "--line", "2", "--raw"]);
    assert!(
        raw.success,
        "but --raw still shows the bytes: {}",
        raw.stderr
    );
    assert!(raw.stdout.contains("last-prompt"), "{}", raw.stdout);
}
