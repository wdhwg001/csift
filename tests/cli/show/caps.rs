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
];

#[test]
fn every_address_form_pages_forward_to_the_whole_fetch_exactly_once() {
    let h = Home::new();
    fixture(&h);
    for form in FORMS {
        for cap in ["1", "2", "3", "5"] {
            chain(&h, form, cap, false);
            // A record merged from the sidecar has no line for `--raw` to print.
            if !form.iter().any(|a| a.contains("u-toolu_AQ1")) {
                chain(&h, form, cap, true);
            }
        }
    }
}

#[test]
fn a_turn_wider_than_the_cap_continues_past_what_it_showed() {
    let h = Home::new();
    fixture(&h);
    // t1 holds ten rendered units: each page passes over everything shown before it.
    assert_eq!(
        chain(&h, &["--turn", "1"], "3", false),
        [
            "--turn 1",
            "--turn 1 --skip 3",
            "--turn 1 --skip 6",
            "--turn 1 --skip 9",
        ]
    );
    // Cut on a turn boundary the window continues with no skip; L2's four units are one
    // record, kept whole past a cap of two.
    assert_eq!(
        chain(&h, &["--turn", ".."], "5", false),
        [
            "--turn ..",
            "--turn 1..3",
            "--turn 1..3 --skip 5",
            "--turn 2..3"
        ]
    );
    assert_eq!(
        chain(&h, &["--turn", "0..1"], "2", false)[..2],
        ["--turn 0..1", "--turn 0..1 --skip 1"]
    );
    // Raw counts lines: t1 is nine lines there (the attachment is outside the window and
    // L5's two units are one line).
    assert_eq!(
        chain(&h, &["--turn", "1"], "3", true),
        ["--turn 1", "--turn 1 --skip 3", "--turn 1 --skip 6"]
    );
}

#[test]
fn a_line_or_uuid_continuation_fetches_exactly_what_the_cap_dropped() {
    let h = Home::new();
    fixture(&h);
    for (form, cap, want) in [
        (&["--line", "1..6,12..19"][..], "5", "--line 3..6,12..19"),
        (&["--line", "2,6,12,14"][..], "5", "--line 12,14"),
        // L2 holds four units: under a cap of one it is kept whole and the next page starts
        // at L3.
        (&["--line", "2..4"][..], "1", "--line 3..4"),
        (
            &["--uuid", "u4,u3,a1,u-toolu_AQ1"][..],
            "1",
            "--uuid u4,u3,u-toolu_AQ1",
        ),
        // u3's replay copy (L12) is kept and its survivor (L14) dropped: `--uuid u3` would
        // fetch the copy again, so the survivor is named by its line.
        (
            &["--uuid", "u4,u3,a1,u-toolu_AQ1"][..],
            "5",
            "--line 14 --uuid u4,u-toolu_AQ1",
        ),
        (
            &["--line", "1..5", "--uuid", "u3,a7"][..],
            "2",
            "--line 2..5 --uuid u3,a7",
        ),
    ] {
        let start: Vec<String> = form.iter().map(|s| (*s).to_string()).collect();
        let (_, cont) = page(&h, &start, cap, false);
        assert_eq!(
            cont.map(|c| c.join(" ")).as_deref(),
            Some(want),
            "{form:?} cap {cap}"
        );
    }
}

#[test]
fn skip_past_the_end_is_an_empty_answer_and_is_not_an_address() {
    let h = Home::new();
    fixture(&h);
    let t = at(SESS);
    let t = t.as_str();
    let out = h.run(&["show", t, "--turn", "1", "--skip", "99", "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    assert!(
        json_rows(&out.stdout, "record").is_empty(),
        "{}",
        out.stdout
    );
    let summary = &json_rows(&out.stdout, "summary")[0];
    assert_eq!(summary["dropped_by_skip"], 10, "{summary}");
    assert_eq!(summary["dropped_by_cap"], 0, "{summary}");
    assert!(
        out.stderr.contains(
            "csift: note: --skip 99 passes over all 10 record unit(s) the address selects"
        ),
        "{}",
        out.stderr
    );
    let raw = h.run(&["show", t, "--turn", "1", "--skip", "9", "--raw"]);
    assert!(raw.success, "{}", raw.stderr);
    assert!(raw.stdout.is_empty(), "{}", raw.stdout);
    assert!(
        raw.stderr.contains("--skip 9 passes over all 9 line(s)"),
        "{}",
        raw.stderr
    );
    // Short of the end: the page and the text footer say what was passed over.
    let text = h.run(&["show", t, "--turn", "1", "--skip", "7"]);
    assert!(
        text.stdout.contains("fetched 3 record unit(s)"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout
            .contains("--skip passed over the first 7 record unit(s) the address selects"),
        "{}",
        text.stdout
    );
    assert!(!text.stderr.contains("passes over all"), "{}", text.stderr);
    // Not an address, and not a branch-point option.
    let bare = h.run(&["show", t, "--skip", "3"]);
    assert!(!bare.success);
    assert!(
        bare.stderr.contains("show needs an address"),
        "{}",
        bare.stderr
    );
    let forks = h.run(&["show", t, "--branch-points", "--skip", "3"]);
    assert_eq!(forks.code, Some(2), "{}", forks.stderr);
}
