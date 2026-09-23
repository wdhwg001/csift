//! The refetch law's edge: an explicit address renders the record it names even when csift
//! models NO leaf for it. `classify` deliberately emits nothing for an `isMeta` user record
//! matching no harness marker (better silent than mislabeled `user.message`), and a fetch
//! domain narrower than the parseable lines bails with "no such record(s)" on a line that
//! is plainly a message.

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
        out.stdout.contains("? unmodeled: user/isMeta"),
        "the unit NAMES what the line is - a harness-authored template csift models \
         none of - instead of borrowing a neighbouring leaf:\n{}",
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
    // acceptable is the address failing on it, which is what this pins.
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
        shown.stdout.contains("? unmodeled: user/isMeta"),
        "as an honest unmodeled unit naming its shape:\n{}",
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
    // The fetch domain is exactly what its name says. A bookkeeping line IS a record
    // now - it carries a leaf, so an address renders it - while a line that is not a
    // JSON object at all still misses and points at `--raw`.
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000fe";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"last-prompt","lastPrompt":"chart the reef"}"#, "\n",
        ),
    );
    // The bookkeeping line renders under its own leaf.
    let out = h.run(&["show", at(sess).as_str(), "--line", "2"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.bookkeeping.last-prompt"),
        "stdout: {}",
        out.stdout
    );
    // A TORN line is not a record at all: still a hard miss pointing at `--raw`.
    let torn = "00000000-0000-4000-8000-0000000000fd";
    h.write(
        &format!("{ENC}/{torn}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"last-prompt","lastProm"#, "\n",
        ),
    );
    let miss = h.run(&["show", at(torn).as_str(), "--line", "2"]);
    assert!(!miss.success, "stdout: {}", miss.stdout);
    assert!(
        miss.stderr.contains("no such record(s)"),
        "stderr: {}",
        miss.stderr
    );
    let raw = h.run(&["show", at(torn).as_str(), "--line", "2", "--raw"]);
    assert!(
        raw.success,
        "but --raw still shows the bytes: {}",
        raw.stderr
    );
    assert!(raw.stdout.contains("lastProm"), "{}", raw.stdout);
}

/// The RANGE counterpart of the miss above. A torn line NAMED on its own bails, but inside
/// an addressed range it is simply a line the range covered and no unit came back from, so
/// the footer says HOW MANY - and it says it in both directions. Silence above zero is the
/// silent truncation the output contract forbids: a reader counting two units against a
/// three-line range would conclude a record went missing. A note at zero is the opposite
/// lie, a loss claimed where nothing was lost.
#[test]
fn a_non_record_line_inside_an_addressed_range_is_counted_and_disclosed() {
    const NOTE: &str = "line(s) in the addressed range are not records";
    let h = Home::new();
    let torn = "00000000-0000-4000-8000-0000000000fb";
    h.write(
        &format!("{ENC}/{torn}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            "this line is not a JSON object at all\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#, "\n",
        ),
    );
    let out = h.run(&["show", at(torn).as_str(), "--line", "1..3"]);
    assert!(
        out.success,
        "a range does not bail on a line it merely covered: {}",
        out.stderr
    );
    assert!(
        out.stdout.contains("fetched 2 record unit(s)"),
        "two of the three lines are records: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(&format!("1 {NOTE}")),
        "the third is disclosed with its count: {}",
        out.stdout
    );
    let j = h.run(&[
        "show",
        at(torn).as_str(),
        "--line",
        "1..3",
        "--format",
        "json",
    ]);
    assert_eq!(
        json_summary(&j.stdout)["non_record_lines"],
        serde_json::json!(1),
        "the machine echo of the same count: {}",
        j.stdout
    );

    // The other direction: a range whose every line IS a record has nothing to disclose.
    let whole = "00000000-0000-4000-8000-0000000000fc";
    h.write(
        &format!("{ENC}/{whole}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the reef"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"charted"}]}}"#, "\n",
        ),
    );
    let clean = h.run(&["show", at(whole).as_str(), "--line", "1..2"]);
    assert!(clean.success, "stderr: {}", clean.stderr);
    assert!(
        !clean.stdout.contains(NOTE),
        "nothing was lost, so nothing is claimed: {}",
        clean.stdout
    );
}

/// The 1:1 law over ROLE-BEARING records: a message line with no renderable text must not
/// hard-miss under an address, and it is the shape that can - the fallback has neither
/// text nor raw bytes to build a unit from. Each shape below names the axis its leaf would
/// have been decided on, so a reader learns what the line IS.
#[test]
fn a_textless_message_record_renders_its_shape_under_an_address() {
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000fe";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzshapes open"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"fallback","model":"claude-x"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","isMeta":true,"timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBOR"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"u2","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":[{"type":"document","source":{"type":"base64","media_type":"application/pdf","data":"JVBER"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"u4","parentUuid":"u3","isMeta":true,"timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"user","content":"zzresidue a template nobody models"}}"#, "\n",
        ),
    );

    // The fallback-only assistant record is still the assistant's TURN: it carries the
    // plain message leaf with an empty body, so it is a labeled unit and not an
    // unmodeled one. Naming the fallback in the render belongs to the system family.
    let fb = h.run(&["show", &at(sess), "--line", "2"]);
    assert!(fb.success, "stderr: {}", fb.stderr);
    assert!(
        fb.stdout.contains("agent.message.plain"),
        "a fallback-served turn keeps the assistant's leaf:\n{}",
        fb.stdout
    );
    assert!(
        !fb.stdout.contains("unmodeled"),
        "and is not an unmodeled unit:\n{}",
        fb.stdout
    );

    // An image-only record carries the leaf its text form carries.
    let img = h.run(&["show", &at(sess), "--line", "3"]);
    assert!(img.success, "stderr: {}", img.stderr);
    assert!(
        img.stdout.contains("harness.context.image"),
        "an image-only record is the attached image:\n{}",
        img.stdout
    );

    // A `document` block is a shape csift models nothing for: the unit names the BLOCK,
    // which is the axis the leaf would have come from.
    let doc = h.run(&["show", &at(sess), "--line", "4"]);
    assert!(doc.success, "stderr: {}", doc.stderr);
    assert!(
        doc.stdout.contains("? unmodeled: user/document"),
        "the block names the shape:\n{}",
        doc.stdout
    );

    // And the residue isMeta template names the authorship flag that makes it the
    // harness's own text rather than the operator's.
    let res = h.run(&["show", &at(sess), "--line", "5"]);
    assert!(res.success, "stderr: {}", res.stderr);
    assert!(
        res.stdout.contains("? unmodeled: user/isMeta"),
        "the residue names its authorship:\n{}",
        res.stdout
    );
    assert!(
        res.stdout.contains("zzresidue a template nobody models"),
        "carrying the record's own text:\n{}",
        res.stdout
    );

    // A SCAN still sees none of the two unmodeled units.
    let scan = h.run(&["search", "zzshapes|zzresidue", &at(sess)]);
    assert!(scan.success, "stderr: {}", scan.stderr);
    assert!(
        !scan.stdout.contains("unmodeled"),
        "a scan never emits an unmodeled unit:\n{}",
        scan.stdout
    );
}

/// The JSON side of the same law: a textless record with a LEAF keeps its label, and one
/// without names its shape under a null label.
#[test]
fn the_shape_and_the_leaf_both_reach_json() {
    let h = Home::new();
    let sess = "00000000-0000-4000-8000-0000000000fb";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"zzjson open"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":[{"type":"document","source":{}}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"assistant","content":[{"type":"fallback","model":"claude-x"}]}}"#, "\n",
        ),
    );
    let row = |line: &str| -> serde_json::Value {
        let out = h.run(&["show", &at(sess), "--line", line, "--format", "json"]);
        assert!(out.success, "stderr: {}", out.stderr);
        out.stdout
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["kind"] == "record")
            .expect("the record row")
    };
    let doc = row("2");
    assert_eq!(doc["label"], serde_json::Value::Null, "{doc}");
    assert_eq!(doc["labels"], serde_json::json!([]), "{doc}");
    assert_eq!(doc["unmodeled"], "user/document", "{doc}");
    let fb = row("3");
    assert_eq!(fb["label"], "agent.message.plain", "{fb}");
    assert_eq!(fb["unmodeled"], serde_json::Value::Null, "{fb}");
}

// ── the headers law ──
//
// The key=value excerpt above is the ONE rendered path that walks a line's own values
// without knowing what they are, so it is the one path an HTTP RESPONSE HEADER SET can
// reach. Measured over one projects root, 5 of 29 `api_error` records carry an
// `error.headers` object and the set holds 18 distinct names, among them a session
// cookie, an organization id and two request trace ids. csift output is pasted into
// reports, into chats and into other models' contexts; `--raw` prints the source line
// and says so, which is what an escape hatch is for.

/// Header VALUES here are invented for this fixture: `zz`-led strings that exist nowhere
/// else. The NAMES are the real ones, because the rule is keyed on the name.
const COOKIE: &str = "zzsession=zzdonotprint";
const ORG: &str = "zzorg-1111-2222-4333-8444";
const HSESS: &str = "00000000-0000-4000-8000-0000000000fe";

/// A `type:"system"` record whose subtype csift models NO leaf for, carrying the
/// `error.headers` shape an `api_error` line carries. The unmodeled subtype is what sends
/// it down the key=value excerpt, which is the path under test.
fn home_with_an_unmodeled_headers_record() -> Home {
    let h = Home::new();
    h.write(
        &format!("{ENC}/{HSESS}.jsonl"),
        &format!(
            concat!(
                r#"{{"type":"user","uuid":"u0","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"zzheaders open"}}}}"#, "\n",
                r#"{{"type":"system","subtype":"zz_future_failure","uuid":"s1","parentUuid":"u0","timestamp":"2026-06-07T05:00:01.000Z","error":{{"type":"overloaded_error","status":529,"headers":{{"set-cookie":"{cookie}","anthropic-organization-id":"{org}"}}}}}}"#, "\n",
            ),
            cookie = COOKIE,
            org = ORG,
        ),
    );
    h
}

/// No rendered surface prints a header VALUE, and the excerpt says the set is there
/// rather than dropping the key silently.
#[test]
fn an_unmodeled_line_never_prints_a_header_set() {
    let h = home_with_an_unmodeled_headers_record();
    for tail in [
        vec!["--line", "2"],
        vec!["--line", "2", "--format", "json"],
        vec!["--uuid", "s1"],
        vec!["--turn", "0"],
    ] {
        let target = at(HSESS);
        let mut argv = vec!["show", target.as_str()];
        argv.extend_from_slice(&tail);
        let out = h.run(&argv);
        assert!(out.success, "{argv:?} stderr: {}", out.stderr);
        let all = format!("{}{}", out.stdout, out.stderr);
        for secret in [COOKIE, ORG, "zzdonotprint"] {
            assert!(
                !all.contains(secret),
                "a header value reached {argv:?}:\n{all}"
            );
        }
    }
    let out = h.run(&["show", at(HSESS).as_str(), "--line", "2"]);
    assert!(
        out.stdout.contains("? unmodeled: system/zz_future_failure"),
        "the unit names the shape:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("<elided>"),
        "the headers member renders its elision, so a reader learns the set exists:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("overloaded_error"),
        "the error's own descriptive field still renders:\n{}",
        out.stdout
    );
}

/// `--raw` is the ONE documented way to the bytes, and it prints them: an escape hatch
/// that elides too is not an escape hatch, and a reader who asked for the source line has
/// said what they want.
#[test]
fn raw_is_the_one_path_that_prints_the_header_set() {
    let h = home_with_an_unmodeled_headers_record();
    let out = h.run(&["show", at(HSESS).as_str(), "--line", "2", "--raw"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(COOKIE) && out.stdout.contains(ORG),
        "--raw prints the source line whole:\n{}",
        out.stdout
    );
}
