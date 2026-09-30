//! `user.unsent`: a draft stays outside turn numbering but is searchable under its own
//! leaf; the collapse count is disclosed; addresses still fetch. Every one of those
//! answers is derived from the SURVIVAL AXIS, so the header carries the axis's own
//! wording, "draft (superseded ...)".

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7a6b5c4d-3e2f-4109-a876-543210fedcba";

/// L1 = a draft opener (parent p0) superseded by L2 (same parent, later); L3 = the reply.
/// L4 = a metadata line no address can render.
fn draft_fixture(h: &Home) {
    h.write(
        &format!("{ENC}/{SESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"d1","parentUuid":"p0","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"the abandoned phrasing of the ask"}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"p0","timestamp":"2026-06-07T05:00:20.000Z","message":{"role":"user","content":"the sent phrasing of the ask"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:25.000Z","message":{"role":"assistant","content":[{"type":"text","text":"answered the sent one"}]}}"#, "\n",
            r#"{"type":"file-history-snapshot","messageId":"m1"}"#, "\n",
        ),
    );
}

#[test]
fn search_keeps_drafts_out_of_user_message_and_discloses_the_count() {
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["search", "", at(SESS).as_str(), "-t", "user.message"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("the sent phrasing") && !out.stdout.contains("abandoned phrasing"),
        "user.message stays pure - a draft never rides it:\n{}",
        out.stdout
    );
    assert!(
        out.stdout
            .contains("1 superseded draft(s) outside turn numbering"),
        "the draft set is DISCLOSED, never silent:\n{}",
        out.stdout
    );
    let outj = h.run(&[
        "search",
        "",
        at(SESS).as_str(),
        "-t",
        "user.message",
        "--format",
        "json",
    ]);
    let summary: serde_json::Value =
        serde_json::from_str(outj.stdout.lines().last().unwrap()).unwrap();
    assert_eq!(summary["superseded_drafts"], 1, "{}", outj.stdout);

    // A draft-free session prints NO draft note and NO zero-malformed note.
    let clean_sess = "8b7c6d5e-4f30-4219-b987-654321fedcba";
    h.write(
        &format!("{ENC}/{clean_sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"c1","timestamp":"2026-06-07T06:00:00.000Z","message":{"role":"user","content":"clean lane"}}"#,
            "\n",
        ),
    );
    let clean = h.run(&["search", "", &at(clean_sess), "-t", "user.message"]);
    assert!(
        !clean.stdout.contains("superseded draft") && !clean.stdout.contains("malformed"),
        "clean run prints no zero notes:\n{}",
        clean.stdout
    );
}

#[test]
fn show_fetches_an_addressed_draft_with_an_honest_header() {
    // The refetch law: the draft IS a real record; only turn numbering excludes it.
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["show", at(SESS).as_str(), "--line", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        !out.stdout.contains("malformed"),
        "a clean fetch prints no zero note:\n{}",
        out.stdout
    );
    assert!(
        out.stdout.contains("abandoned phrasing")
            && out.stdout.contains("draft (superseded")
            && out.stdout.contains("outside turn numbering"),
        "the draft renders with an honest header, never a fabricated t<N>:\n{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("\nt0 ") && !out.stdout.contains("\nt1 "),
        "no fabricated turn header on a draft-only fetch:\n{}",
        out.stdout
    );

    let outj = h.run(&["show", at(SESS).as_str(), "--line", "1", "--format", "json"]);
    let row: serde_json::Value = outj
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .find(|o: &serde_json::Value| o["kind"] == "record")
        .expect("record row");
    assert_eq!(row["superseded_draft"], true, "{}", outj.stdout);
    assert!(
        row["turn_index"].is_null(),
        "no fabricated index: {}",
        outj.stdout
    );

    // A normal record fetched in the same call keeps its numbering and flag=false.
    let both = h.run(&[
        "show",
        at(SESS).as_str(),
        "--line",
        "1..2",
        "--format",
        "json",
    ]);
    let rows: Vec<serde_json::Value> = both
        .stdout
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .filter(|o: &serde_json::Value| o["kind"] == "record")
        .collect();
    assert_eq!(rows.len(), 2, "{}", both.stdout);
    let bsummary: serde_json::Value =
        serde_json::from_str(both.stdout.lines().last().unwrap()).unwrap();
    assert_eq!(
        bsummary["records"], 2,
        "summary counts the units: {}",
        both.stdout
    );
    assert!(
        rows.iter()
            .any(|r| r["superseded_draft"] == false && r["turn_index"] == 0),
        "the sent opener keeps t0: {}",
        both.stdout
    );
}

#[test]
fn show_prints_the_malformed_note_when_a_torn_line_exists() {
    let h = Home::new();
    let sess = "9c8d7e6f-5a4b-4321-8765-fedcba098765";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"solid line"}}"#,
            "\n",
            "not json here",
            "\n",
        ),
    );
    let out = h.run(&["show", &at(sess), "--line", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("1 malformed line(s) skipped"),
        "the torn line is booked on a fetch too:\n{}",
        out.stdout
    );
}

#[test]
fn show_renders_a_metadata_line_under_its_own_leaf() {
    // The 1:1 law: a metadata line an address names is a bookkeeping RECORD, not a miss.
    let h = Home::new();
    draft_fixture(&h);
    let out = h.run(&["show", at(SESS).as_str(), "--line", "4"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("harness.bookkeeping."),
        "the line renders under its own leaf:\n{}",
        out.stdout
    );
}

// -- the SURVIVAL AXIS through an address --

const RSESS: &str = "2b1a0f9e-8d7c-4465-9231-0e9d8c7b6a5f";

/// L1 prompt, L2 reply, L3 the REWOUND prompt, L4 its reply, L5 the resend, L6 its reply.
fn rewind_fixture(h: &Home) {
    h.write(
        &format!("{ENC}/{RSESS}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"chart the lagoon"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a0","parentUuid":"u0","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"assistant","id":"m0","content":[{"type":"text","text":"charting"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u1","parentUuid":"a0","timestamp":"2026-06-07T05:01:00.000Z","message":{"role":"user","content":"dredge the northern channel"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:01:05.000Z","message":{"role":"assistant","id":"m1","content":[{"type":"text","text":"dredging"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a0","timestamp":"2026-06-07T05:02:00.000Z","message":{"role":"user","content":"survey the southern shoal"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:02:05.000Z","message":{"role":"assistant","id":"m2","content":[{"type":"text","text":"surveying"}]}}"#, "\n",
        ),
    );
}

#[test]
fn show_renders_an_abandoned_record_with_its_marker() {
    // The refetch law: an address renders the record it names, whatever the chain says -
    // and it says WHAT the record is instead of fabricating a turn number.
    let h = Home::new();
    rewind_fixture(&h);
    let out = h.run(&["show", &at(RSESS), "--line", "3"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("rewound turn")
            && out.stdout.contains("outside turn numbering")
            && out.stdout.contains("user.rewound"),
        "the rewound prompt renders with its own leaf:\n{}",
        out.stdout
    );
    let j = h.run(&["show", &at(RSESS), "--line", "3..4", "--format", "json"]);
    let recs: Vec<serde_json::Value> = j
        .stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .filter(|v: &serde_json::Value| v["kind"] == "record")
        .collect();
    assert_eq!(recs.len(), 2, "{}", j.stdout);
    for r in &recs {
        assert_eq!(r["survival"], "abandoned", "{}", j.stdout);
        assert_eq!(r["abandoned_root_line"], 3, "{}", j.stdout);
        assert!(r["turn_index"].is_null(), "{}", j.stdout);
        assert!(r["replay_copy_of"].is_null(), "{}", j.stdout);
    }
}

#[test]
fn show_turn_addresses_the_live_numbering() {
    // t1 is the RESEND, not the rewound prompt: `show --turn` and `search`'s `tN` speak
    // the same numbering, and a rewound turn consumes no number.
    let h = Home::new();
    rewind_fixture(&h);
    let out = h.run(&["show", &at(RSESS), "--turn", "1"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("southern shoal") && !out.stdout.contains("northern channel"),
        "turn 1 is the surviving second turn:\n{}",
        out.stdout
    );
    let miss = h.run(&["show", &at(RSESS), "--turn", "2"]);
    assert!(!miss.success, "only two turns survive: {}", miss.stdout);
}

/// Three turns with an abandoned draft (L3) between the first reply and its resend. The
/// render puts an abandoned unit LAST, after every live one, so render order is not line
/// order - which is what the cap's continuation must not be read off.
fn capped_window_fixture(h: &Home, sess: &str) {
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"one"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply one"}]}}"#, "\n",
            r#"{"type":"user","uuid":"d1","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":"a draft of two"}}"#, "\n",
            r#"{"type":"user","uuid":"u2","parentUuid":"a1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"user","content":"two"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"u2","timestamp":"2026-06-07T05:00:04.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply two"}]}}"#, "\n",
            r#"{"type":"user","uuid":"u3","parentUuid":"a2","timestamp":"2026-06-07T05:00:05.000Z","message":{"role":"user","content":"three"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a3","parentUuid":"u3","timestamp":"2026-06-07T05:00:06.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reply three"}]}}"#, "\n",
        ),
    );
}

fn record_lines(stdout: &str) -> Vec<u64> {
    json_rows(stdout, "record")
        .iter()
        .filter_map(|r| r["line"].as_u64())
        .collect()
}

/// A capped fetch keeps the records on the lowest lines and its continuation runs from the
/// first dropped line to the end of the addressed range, so the continuation fetches exactly
/// the records the cap dropped - with the abandoned draft among the dropped (cap 2) and among
/// the kept (cap 4). Read off render order, the continuation ran backwards or stopped at the
/// draft and lost the lines after it.
#[test]
fn a_capped_fetch_continues_with_exactly_the_records_it_dropped() {
    let h = Home::new();
    let sess = "7a6b5c4d-3e2f-4109-a876-543210fedcbb";
    capped_window_fixture(&h, sess);
    let whole = h.run(&[
        "show",
        at(sess).as_str(),
        "--line",
        "1..7",
        "--format",
        "json",
    ]);
    assert_eq!(
        record_lines(&whole.stdout),
        vec![1, 2, 4, 5, 6, 7, 3],
        "the draft renders last: {}",
        whole.stdout
    );
    for (cap, kept, dropped) in [
        ("2", vec![1, 2], vec![3, 4, 5, 6, 7]),
        ("4", vec![1, 2, 4, 3], vec![5, 6, 7]),
    ] {
        let out = h.run(&[
            "show",
            at(sess).as_str(),
            "--line",
            "1..7",
            "--max-count",
            cap,
            "--format",
            "json",
        ]);
        assert!(out.success, "{}", out.stderr);
        assert_eq!(record_lines(&out.stdout), kept, "cap {cap}: {}", out.stdout);
        let summary = &json_rows(&out.stdout, "summary")[0];
        assert_eq!(summary["dropped_by_cap"], dropped.len(), "{summary}");
        let cmd = summary["refetch_remainder"]
            .as_str()
            .expect("a continuation");
        let want = format!("--line {}..7", dropped[0]);
        assert!(cmd.ends_with(&want), "cap {cap}: {cmd}");
        let text = h.run(&[
            "show",
            at(sess).as_str(),
            "--line",
            "1..7",
            "--max-count",
            cap,
        ]);
        assert!(
            text.stdout.contains(&format!("continue: {cmd}")),
            "{}",
            text.stdout
        );
        let args: Vec<&str> = cmd.split_whitespace().skip(1).collect();
        let mut rest = args.clone();
        rest.extend(["--max-count", "0", "--format", "json"]);
        let again = h.run(&rest);
        let mut got = record_lines(&again.stdout);
        got.sort_unstable();
        assert_eq!(
            got, dropped,
            "cap {cap}: the continuation fetches the dropped records"
        );
    }
}

/// Two edges of the cap. A first record holding more units than the cap is kept WHOLE - a
/// record's units share its line, and a cut through it would leave the continuation
/// re-fetching its kept half - and the continuation starts at the next line. A `--turn`
/// window continues as a window: cut inside its one turn, it re-enters that turn and says
/// how many of its units were already shown.
#[test]
fn a_capped_fetch_keeps_a_wide_first_record_and_a_turn_window_continues_as_a_window() {
    let h = Home::new();
    let sess = "7a6b5c4d-3e2f-4109-a876-543210fedcbc";
    h.write(
        &format!("{ENC}/{sess}.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"u1","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{"role":"user","content":"one"}}"#, "\n",
            r#"{"type":"assistant","uuid":"a1","parentUuid":"u1","timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"reading it"},{"type":"tool_use","id":"toolu_r","name":"Read","input":{"file_path":"/x"}}]}}"#, "\n",
            r#"{"type":"user","uuid":"r1","parentUuid":"a1","timestamp":"2026-06-07T05:00:02.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_r","content":"x"}]}}"#, "\n",
            r#"{"type":"assistant","uuid":"a2","parentUuid":"r1","timestamp":"2026-06-07T05:00:03.000Z","message":{"role":"assistant","content":[{"type":"text","text":"read"}]}}"#, "\n",
        ),
    );
    let target = at(sess);
    let capped = |range: &[&str]| {
        let mut args = vec!["show", target.as_str()];
        args.extend_from_slice(range);
        args.extend(["--max-count", "1", "--format", "json"]);
        let out = h.run(&args);
        assert!(out.success, "{}", out.stderr);
        let summary = json_rows(&out.stdout, "summary")[0].clone();
        (record_lines(&out.stdout), summary)
    };
    let (kept, summary) = capped(&["--line", "2..4"]);
    assert_eq!(kept, vec![2, 2], "L2's two units, whole: {summary}");
    assert_eq!(summary["dropped_by_cap"], 2, "{summary}");
    assert!(
        summary["refetch_remainder"]
            .as_str()
            .is_some_and(|c| c.ends_with("--line 3..4")),
        "{summary}"
    );
    assert_eq!(summary["refetch_overlap"], 0, "{summary}");
    let (kept, summary) = capped(&["--turn", ".."]);
    assert_eq!(kept, vec![1], "{summary}");
    assert_eq!(summary["dropped_by_cap"], 4, "{summary}");
    assert!(
        summary["refetch_remainder"]
            .as_str()
            .is_some_and(|c| c.ends_with("--turn 0")),
        "{summary}"
    );
    assert_eq!(summary["refetch_overlap"], 1, "{summary}");
}
