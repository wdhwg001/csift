//! The cap's continuation in every address form - a `--line` range, a comma list, explicit
//! lines, a `--uuid` list, a mix of the two, and a `--turn` window - rendered and raw. The
//! pin is the CHAIN: following the printed continuations at one cap until none is printed
//! yields every unit of the uncapped fetch exactly once, and no continuation is the command
//! it continues.

use std::collections::BTreeMap;

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7a6b5c4d-3e2f-4109-a876-5432100cab01";

/// Four turns and the shapes a page boundary can land on: a four-unit record (L2), an
/// abandoned draft (L3), a turn of ten units (t1) holding an attachment line (L6: an address
/// renders it, a turn window leaves it out), a replay copy (L12-L13, survived by L14-L15, so
/// `--uuid u3` names two lines), two bookkeeping lines after the last turn, and a pending
/// AskUserQuestion merged from the elicitation sidecar (no line; it renders at the end of t3).
fn fixture(h: &Home) {
    let rec = |uuid: &str, parent: &str, second: u32, role: &str, content: &str| {
        let parent = if parent.is_empty() {
            "null".to_string()
        } else {
            format!("\"{parent}\"")
        };
        format!(
            r#"{{"type":"{role}","uuid":"{uuid}","parentUuid":{parent},"timestamp":"2026-06-07T05:00:{second:02}.000Z","message":{{"role":"{role}","content":{content}}}}}"#
        )
    };
    let text = |t: &str| format!(r#"[{{"type":"text","text":"{t}"}}]"#);
    let lines = [
        rec("u1", "", 1, "user", "\"first prompt\""),
        rec(
            "a1",
            "u1",
            2,
            "assistant",
            r#"[{"type":"text","text":"alpha"},{"type":"thinking","thinking":"pondering","signature":"sig"},{"type":"text","text":"beta"},{"type":"text","text":"gamma"}]"#,
        ),
        rec("d1", "a1", 3, "user", "\"a draft never sent\""),
        rec("u2", "a1", 4, "user", "\"second prompt\""),
        rec(
            "a2",
            "u2",
            5,
            "assistant",
            r#"[{"type":"text","text":"listing"},{"type":"tool_use","id":"toolu_b1","name":"Bash","input":{"command":"ls"}}]"#,
        ),
        r#"{"type":"attachment","uuid":"at1","parentUuid":"a2","timestamp":"2026-06-07T05:00:06.000Z","attachment":{"type":"hook_additional_context","content":["capctx"],"hookName":"PostToolUse:Bash","hookEvent":"PostToolUse"}}"#.to_string(),
        rec(
            "r2",
            "at1",
            7,
            "user",
            r#"[{"type":"tool_result","tool_use_id":"toolu_b1","content":"x"}]"#,
        ),
        rec("a3", "r2", 8, "assistant", &text("step three")),
        rec("a4", "a3", 9, "assistant", &text("step four")),
        rec("a5", "a4", 10, "assistant", &text("step five")),
        rec("a6", "a5", 11, "assistant", &text("step six")),
        rec("u3", "a6", 12, "user", "\"third prompt\""),
        rec("a7", "u3", 13, "assistant", &text("reply three")),
        rec("u3", "a6", 14, "user", "\"third prompt\""),
        rec("a7", "u3", 15, "assistant", &text("reply three again")),
        rec("u4", "a7", 16, "user", "\"fourth prompt\""),
        rec("a8", "u4", 17, "assistant", &text("reply four")),
        r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:00:30.000Z","trackedFileBackups":{}},"isSnapshotUpdate":false}"#.to_string(),
        format!(r#"{{"type":"last-prompt","lastPrompt":"fourth prompt","sessionId":"{SESS}","leafUuid":"a8"}}"#),
    ];
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
    h.write(
        &format!("{ENC}/{SESS}/elicitations.jsonl"),
        &(format!(
            r#"{{"type":"assistant","uuid":"u-toolu_AQ1","timestamp":"2026-06-07T05:00:40.000Z","sessionId":"{SESS}","isSidechain":false,"message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_AQ1","name":"AskUserQuestion","input":{{"questions":[{{"question":"Pick a target"}}]}}}}]}},"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_AQ1","csiftHookEvent":"PreToolUse","hookInput":{{}}}}"#
        ) + "\n"),
    );
}

/// A page's units as a multiset: (line, uuid, label) rendered, the line's bytes raw.
type Units = BTreeMap<String, usize>;

/// One page: its units and the continuation's address arguments, when one is printed. The
/// rendered page also checks that the text pointer prints the command the JSON names.
fn page(h: &Home, address: &[String], cap: &str, raw: bool) -> (Units, Option<Vec<String>>) {
    let t = at(SESS);
    let mut args: Vec<&str> = vec!["show", t.as_str()];
    args.extend(address.iter().map(String::as_str));
    args.extend(["--max-count", cap]);
    let mut units = Units::new();
    if raw {
        args.push("--raw");
        let out = h.run(&args);
        assert!(out.success, "{args:?}: {}", out.stderr);
        for l in out.stdout.lines() {
            *units.entry(l.to_string()).or_default() += 1;
        }
        let cont = out
            .stderr
            .split_once(&format!("continue: csift show @{SESS} "))
            .and_then(|(_, rest)| rest.split_once(" --raw"))
            .map(|(a, _)| a.split_whitespace().map(String::from).collect());
        return (units, cont);
    }
    let text = h.run(&args).stdout;
    args.extend(["--format", "json"]);
    let out = h.run(&args);
    assert!(out.success, "{args:?}: {}", out.stderr);
    for r in json_rows(&out.stdout, "record") {
        let key = format!("{} {} {}", r["line"], r["uuid"], r["label"]);
        *units.entry(key).or_default() += 1;
    }
    let summary = &json_rows(&out.stdout, "summary")[0];
    let cont = summary["refetch_remainder"].as_str().map(|cmd| {
        assert!(text.contains(&format!("continue: {cmd}")), "{text}");
        cmd.split_whitespace().skip(3).map(String::from).collect()
    });
    assert_eq!(cont.is_some(), summary["dropped_by_cap"] != 0, "{summary}");
    (units, cont)
}

/// Follow the continuations from `address` at `cap` until none is printed, and check the
/// pages together are the uncapped fetch, each unit once. Returns the commands run.
fn chain(h: &Home, address: &[&str], cap: &str, raw: bool) -> Vec<String> {
    let start: Vec<String> = address.iter().map(|s| (*s).to_string()).collect();
    let (whole, none) = page(h, &start, "0", raw);
    assert!(none.is_none());
    let mut got = Units::new();
    let mut commands = Vec::new();
    let mut next = Some(start);
    while let Some(args) = next.take() {
        assert!(
            commands.len() < 40,
            "{address:?} cap {cap}: no end, {commands:?}"
        );
        let (units, cont) = page(h, &args, cap, raw);
        for (k, n) in units {
            *got.entry(k).or_default() += n;
        }
        commands.push(args.join(" "));
        if let Some(c) = &cont {
            assert_ne!(
                c, &args,
                "{address:?} cap {cap}: the continuation is the command run"
            );
        }
        next = cont;
    }
    assert_eq!(got, whole, "{address:?} cap {cap} raw {raw}: {commands:?}");
    commands
}

const FORMS: &[&[&str]] = &[
    &["--line", ".."],
    &["--line", "1..6,12..19"],
    &["--line", "2,6,12,14"],
    &["--uuid", "u1,a1,d1,u2,at1,r2,a3,u3,a7,u4,a8"],
    &["--uuid", "u4,u3,a1,u-toolu_AQ1"],
    &["--line", "1..5", "--uuid", "u3,a7,u-toolu_AQ1"],
    &["--turn", ".."],
    &["--turn", "1..3"],
    &["--turn", "1"],
    &["--turn", "-2.."],
];

#[test]
fn every_address_form_pages_forward_to_the_whole_fetch_exactly_once() {
    let h = Home::new();
    fixture(&h);
    for form in FORMS {
        for cap in ["1", "2", "3", "5"] {
            chain(&h, form, cap, false);
            chain(&h, form, cap, true);
        }
    }
}

#[test]
fn a_turn_wider_than_the_cap_continues_by_the_lines_it_left_out() {
    let h = Home::new();
    fixture(&h);
    let chains: Vec<Vec<String>> = vec![
        // t1 is wider than the cap: each page names the lines it did not show, and the turn
        // window's attachment line (L6) is never among them.
        chain(&h, &["--turn", "1"], "3", false),
        // L2's four units are one record, kept whole past a cap of five.
        chain(&h, &["--turn", ".."], "5", false),
        chain(&h, &["--turn", "0..1"], "2", false),
        // Raw counts lines: L5's two units are one line there.
        chain(&h, &["--turn", "1"], "3", true),
    ];
    assert_eq!(chains, TURN_CHAINS_WANT, "{chains:#?}");
}

const TURN_CHAINS_WANT: [&[&str]; 4] = [
    &["--turn 1", "--line 7..13", "--line 10..13", "--line 13"],
    &[
        "--turn ..",
        "--line 4..5,7..17 --uuid u-toolu_AQ1",
        "--line 9..17 --uuid u-toolu_AQ1",
        "--line 14..17 --uuid u-toolu_AQ1",
    ],
    &[
        "--turn 0..1",
        "--line 2,4..5,7..13",
        "--line 4..5,7..13",
        "--line 5,7..13",
        "--line 7..13",
        "--line 9..13",
        "--line 11..13",
        "--line 13",
    ],
    &["--turn 1", "--line 8..13", "--line 11..13"],
];

#[test]
fn a_line_or_uuid_continuation_names_exactly_what_the_cap_dropped() {
    let h = Home::new();
    fixture(&h);
    let mut got = Vec::new();
    for (form, cap) in [
        (&["--line", "1..6,12..19"][..], "5"),
        (&["--line", "2,6,12,14"][..], "5"),
        // L2 holds four units: under a cap of one it is kept whole and the next page starts
        // at L3.
        (&["--line", "2..4"][..], "1"),
        // A uuid list continues by line, and the sidecar record, which has no line, by uuid.
        (&["--uuid", "u4,u3,a1,u-toolu_AQ1"][..], "1"),
        // u3's replay copy (L12) is kept and its survivor (L14) dropped: the line names the
        // survivor alone.
        (&["--uuid", "u4,u3,a1,u-toolu_AQ1"][..], "5"),
        (&["--line", "1..5", "--uuid", "u3,a7"][..], "2"),
    ] {
        let start: Vec<String> = form.iter().map(|s| (*s).to_string()).collect();
        let (_, cont) = page(&h, &start, cap, false);
        got.push(cont.map(|c| c.join(" ")).unwrap_or_default());
    }
    assert_eq!(got, RECORD_CONTINUATIONS_WANT, "{got:#?}");
}

const RECORD_CONTINUATIONS_WANT: [&str; 6] = [
    "--line 3..6,12..19",
    "--line 12,14",
    "--line 3..4",
    "--line 12,14,16 --uuid u-toolu_AQ1",
    "--line 14,16 --uuid u-toolu_AQ1",
    "--line 2..5,12..15",
];

/// A landmark-shaped turn: one prompt and 535 replies is one turn of 536 units, and at the
/// default cap of 200 it pages as 200, 200 and 136, each page naming the lines it left out.
#[test]
fn a_turn_of_536_units_pages_as_200_200_and_136() {
    let h = Home::new();
    let sess = "7a6b5c4d-3e2f-4109-a876-5432100cab02";
    let mut lines = vec![format!(
        r#"{{"type":"user","uuid":"w0","parentUuid":null,"timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"user","content":"go"}}}}"#
    )];
    for i in 1..536 {
        lines.push(format!(
            r#"{{"type":"assistant","uuid":"w{i}","parentUuid":"w{}","timestamp":"2026-06-07T05:00:00.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"step {i}"}}]}}}}"#,
            i - 1
        ));
    }
    h.write(&format!("{ENC}/{sess}.jsonl"), &(lines.join("\n") + "\n"));
    let t = at(sess);
    let mut address = vec!["--turn".to_string(), "0".to_string()];
    let mut pages = Vec::new();
    let mut commands = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    loop {
        let mut args = vec!["show", t.as_str()];
        args.extend(address.iter().map(String::as_str));
        args.extend(["--format", "json"]);
        let out = h.run(&args);
        assert!(out.success, "{args:?}: {}", out.stderr);
        let rows = json_rows(&out.stdout, "record");
        for r in &rows {
            assert!(
                seen.insert(r["line"].as_u64()),
                "{address:?} repeats {}",
                r["line"]
            );
        }
        pages.push(rows.len());
        commands.push(address.join(" "));
        let summary = &json_rows(&out.stdout, "summary")[0];
        let Some(cmd) = summary["refetch_remainder"].as_str() else {
            break;
        };
        let next: Vec<String> = cmd.split_whitespace().skip(3).map(String::from).collect();
        assert_ne!(next, address, "a continuation is never its own command");
        address = next;
        assert!(pages.len() < 5, "{pages:?}");
    }
    assert_eq!(pages, [200, 200, 136], "{commands:?}");
    assert_eq!(
        commands,
        ["--turn 0", "--line 201..536", "--line 401..536"],
        "{pages:?}"
    );
    assert_eq!(seen.len(), 536);
}

#[test]
fn show_takes_no_skip_and_the_summary_names_no_skip() {
    let h = Home::new();
    fixture(&h);
    let t = at(SESS);
    let t = t.as_str();
    let out = h.run(&["show", t, "--turn", "1", "--skip", "3"]);
    assert_eq!(out.code, Some(2), "{}", out.stderr);
    let json = h.run(&["show", t, "--turn", "1", "--format", "json"]);
    assert!(json.success, "{}", json.stderr);
    let summary = &json_rows(&json.stdout, "summary")[0];
    assert!(summary.get("dropped_by_skip").is_none(), "{summary}");
    // The footer names the dropped lines and says what `--max-count 0` returns.
    let text = h.run(&["show", t, "--turn", "1", "--max-count", "3"]);
    assert!(
        text.stdout.contains(&format!(
            "beyond the 3-unit cap · continue: csift show @{SESS} --line "
        )) && text.stdout.contains("(or --max-count 0 = the whole fetch)"),
        "{}",
        text.stdout
    );
}

/// A uuid names the same records raw and rendered: both lines a replay copy shares with its
/// survivor, lowest first, and a record merged from the elicitation sidecar is omitted by the
/// raw fetch with a note - it has no line to print - rather than refused as a miss.
#[test]
fn a_raw_uuid_fetch_names_the_records_the_rendered_one_does() {
    let h = Home::new();
    fixture(&h);
    let t = at(SESS);
    let t = t.as_str();
    let rendered = h.run(&["show", t, "--uuid", "u3", "--format", "json"]);
    let lines: Vec<u64> = json_rows(&rendered.stdout, "record")
        .iter()
        .filter_map(|r| r["line"].as_u64())
        .collect();
    assert_eq!(lines, vec![12, 14], "{}", rendered.stdout);
    let raw = h.run(&["show", t, "--uuid", "u3", "--raw"]);
    assert!(raw.success, "{}", raw.stderr);
    let stamps: Vec<&str> = raw
        .stdout
        .lines()
        .map(|l| {
            l.split("\"timestamp\":\"")
                .nth(1)
                .unwrap_or("")
                .get(..19)
                .unwrap_or("")
        })
        .collect();
    assert_eq!(
        stamps,
        ["2026-06-07T05:00:12", "2026-06-07T05:00:14"],
        "{}",
        raw.stdout
    );
    let side = h.run(&["show", t, "--uuid", "u-toolu_AQ1,u1", "--raw"]);
    assert!(side.success, "{}", side.stderr);
    assert_eq!(side.stdout.lines().count(), 1, "{}", side.stdout);
    assert!(
        side.stderr.contains(
            "csift: note: uuid u-toolu_AQ1 names a sidecar-merged record, which has no \
             physical transcript line"
        ),
        "{}",
        side.stderr
    );
    let alone = h.run(&["show", t, "--uuid", "u-toolu_AQ1", "--raw"]);
    assert!(alone.success, "{}", alone.stderr);
    assert!(alone.stdout.is_empty(), "{}", alone.stdout);
    let miss = h.run(&["show", t, "--uuid", "nosuch", "--raw"]);
    assert!(!miss.success);
    assert!(
        miss.stderr.contains("no such record(s): uuid nosuch"),
        "{}",
        miss.stderr
    );
}

/// Only a top-level session has an elicitation sidecar - the hook keys it by the top-level
/// session - so neither fetch of a subagent transcript reads a file that sits where one
/// would: a uuid only such a file carries is a miss raw and rendered alike.
#[test]
fn a_subagent_transcript_reads_no_sidecar_in_either_fetch() {
    let h = Home::new();
    fixture(&h);
    let lane = h.write(
        &format!("{ENC}/{SESS}/subagents/agent-a1b2c3d4e5f6a7b8.jsonl"),
        concat!(
            r#"{"type":"user","uuid":"s1","parentUuid":null,"isSidechain":true,"timestamp":"2026-06-07T05:00:01.000Z","message":{"role":"user","content":"lane prompt"}}"#,
            "\n"
        ),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-a1b2c3d4e5f6a7b8/elicitations.jsonl"),
        &(format!(
            r#"{{"type":"assistant","uuid":"u-toolu_SUB","timestamp":"2026-06-07T05:00:40.000Z","sessionId":"{SESS}","isSidechain":false,"message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"toolu_SUB","name":"AskUserQuestion","input":{{"questions":[{{"question":"Pick"}}]}}}}]}},"csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_SUB","csiftHookEvent":"PreToolUse","hookInput":{{}}}}"#
        ) + "\n"),
    );
    let lane = lane.to_string_lossy().into_owned();
    for raw in [false, true] {
        let mut args = vec!["show", lane.as_str(), "--uuid", "u-toolu_SUB"];
        if raw {
            args.push("--raw");
        }
        let out = h.run(&args);
        assert!(!out.success, "raw {raw}: {}", out.stdout);
        assert!(
            out.stderr.contains("no such record(s): uuid u-toolu_SUB"),
            "raw {raw}: {}",
            out.stderr
        );
    }
    let found = h.run(&["show", lane.as_str(), "--uuid", "s1", "--raw"]);
    assert!(found.success, "{}", found.stderr);
    assert_eq!(found.stdout.lines().count(), 1, "{}", found.stdout);
}
