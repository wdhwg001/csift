//! The cap's continuation in every address form: a `--line` comma list, a `--uuid` list, the
//! two mixed, and a `--turn` window, rendered and raw. Each capped fetch is compared against
//! the uncapped fetch of the same address: the continuation must fetch exactly the units the
//! cap dropped, plus - for a window cut inside a turn - the kept units of that turn, which the
//! pointer counts.

use std::collections::BTreeMap;

use crate::harness::*;

const ENC: &str = "-Users-dev-example-project";
const SESS: &str = "7a6b5c4d-3e2f-4109-a876-5432100cab01";

/// Four turns, an abandoned draft inside the first two (L3, L11), a two-unit record (L5), an
/// attachment line inside t1 (L6: an address renders it, a turn window leaves it out) and two
/// bookkeeping lines after the last turn.
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
        rec("a1", "u1", 2, "assistant", &text("reply one")),
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
        rec("a3", "r2", 8, "assistant", &text("reply two")),
        rec("u3", "a3", 9, "user", "\"third prompt\""),
        rec("a4", "u3", 10, "assistant", &text("reply three")),
        rec("d2", "a4", 11, "user", "\"another draft\""),
        rec("u4", "a4", 12, "user", "\"fourth prompt\""),
        rec("a5", "u4", 13, "assistant", &text("reply four")),
        r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-06-07T05:00:30.000Z","trackedFileBackups":{}},"isSnapshotUpdate":false}"#.to_string(),
        format!(r#"{{"type":"last-prompt","lastPrompt":"fourth prompt","sessionId":"{SESS}","leafUuid":"a5"}}"#),
    ];
    h.write(&format!("{ENC}/{SESS}.jsonl"), &(lines.join("\n") + "\n"));
}

type Units = BTreeMap<(u64, String, String), usize>;

/// The record units of one rendered fetch, as a multiset keyed by line, label and text.
fn units(h: &Home, address: &[&str], cap: &str) -> (Units, serde_json::Value, String) {
    let mut args = vec!["show"];
    args.extend_from_slice(address);
    args.extend(["--max-count", cap, "--format", "json"]);
    let out = h.run(&args);
    assert!(out.success, "{args:?}: {}", out.stderr);
    let mut got = Units::new();
    for r in json_rows(&out.stdout, "record") {
        let key = (
            r["line"].as_u64().unwrap_or(0),
            r["label"].as_str().unwrap_or("").to_string(),
            r["text"].as_str().unwrap_or("").to_string(),
        );
        *got.entry(key).or_default() += 1;
    }
    let mut text_args = vec!["show"];
    text_args.extend_from_slice(address);
    text_args.extend(["--max-count", cap]);
    let text = h.run(&text_args).stdout;
    (got, json_rows(&out.stdout, "summary")[0].clone(), text)
}

fn minus(a: &Units, b: &Units) -> Units {
    let mut out = Units::new();
    for (k, &n) in a {
        let left = n.saturating_sub(b.get(k).copied().unwrap_or(0));
        if left > 0 {
            out.insert(k.clone(), left);
        }
    }
    out
}

fn total(u: &Units) -> usize {
    u.values().sum()
}

/// Run the capped fetch, the uncapped one and the printed continuation, and check that the
/// continuation fetches the dropped units plus exactly the overlap it declares, all of which
/// were shown. Returns the continuation's address arguments.
fn continues_exactly(h: &Home, address: &[&str], cap: &str) -> String {
    let (whole, _, _) = units(h, address, "0");
    let (kept, summary, text) = units(h, address, cap);
    let dropped = minus(&whole, &kept);
    assert_eq!(
        summary["dropped_by_cap"],
        total(&dropped),
        "{address:?} cap {cap}"
    );
    let cmd = summary["refetch_remainder"]
        .as_str()
        .unwrap_or_else(|| panic!("{address:?} cap {cap}: a continuation, {summary}"))
        .to_string();
    assert!(text.contains(&format!("continue: {cmd}")), "{text}");
    let rest: Vec<&str> = cmd.split_whitespace().skip(2).collect();
    let (again, _, _) = units(h, &rest, "0");
    assert!(
        minus(&dropped, &again).is_empty(),
        "{address:?} cap {cap}: `{cmd}` misses dropped units"
    );
    let repeated = minus(&again, &dropped);
    assert!(
        minus(&repeated, &kept).is_empty(),
        "{address:?} cap {cap}: `{cmd}` fetches units that were never addressed: {repeated:?}"
    );
    assert_eq!(
        summary["refetch_overlap"],
        total(&repeated),
        "{address:?} cap {cap}: the overlap it declares, {summary}"
    );
    let overlap = total(&repeated);
    assert_eq!(
        text.contains(&format!("(repeats the {overlap} unit(s)")),
        overlap > 0,
        "{text}"
    );
    rest.join(" ")
}

#[test]
fn a_capped_fetch_continues_with_exactly_the_records_it_dropped_in_every_address_form() {
    let h = Home::new();
    fixture(&h);
    let t = at(SESS);
    let t = t.as_str();
    for (address, cap, want) in [
        (vec![t, "--line", "1..4,9..15"], "2", "--line 3..4,9..15"),
        (vec![t, "--line", "1..4,9..15"], "5", "--line 10..15"),
        (vec![t, "--line", "1..15"], "5", "--line 5..15"),
        (vec![t, "--line", "2,4,6,8"], "2", "--line 6,8"),
        (
            vec![t, "--uuid", "u4,a1,d1,u2,a3,u3,u1"],
            "2",
            "--uuid u4,d1,u2,a3,u3",
        ),
        (
            vec![t, "--uuid", "u1,a1,d1,u2,a3,u3,u4"],
            "5",
            "--uuid u3,u4",
        ),
        (
            vec![t, "--line", "1..2", "--uuid", "a3,u4"],
            "2",
            "--uuid a3,u4",
        ),
        (
            vec![t, "--line", "1..2,5", "--uuid", "u4"],
            "1",
            "--line 2,5 --uuid u4",
        ),
        (vec![t, "--turn", "1..3"], "2", "--turn 1..3"),
        (vec![t, "--turn", "1..3"], "3", "--turn 1..3"),
        (vec![t, "--turn", "1..3"], "5", "--turn 2..3"),
        (vec![t, "--turn", ".."], "2", "--turn 1..3"),
        (vec![t, "--turn", ".."], "8", "--turn 2..3"),
    ] {
        let got = continues_exactly(&h, &address, cap);
        assert_eq!(got, format!("@{SESS} {want}"), "{address:?} cap {cap}");
    }
}

/// The raw fetch writes its stderr continuation by the same rule, one line a unit.
#[test]
fn a_capped_raw_fetch_continues_in_the_address_form_it_was_given() {
    let h = Home::new();
    fixture(&h);
    let t = at(SESS);
    for (address, cap, want) in [
        (
            vec!["--line", "1..4,9..15"],
            "2",
            "--line 3..4,9..15 --raw,",
        ),
        (vec!["--uuid", "u1,u2,a3"], "1", "--uuid u2,a3 --raw,"),
        (
            vec!["--turn", "1.."],
            "2",
            "--turn 1..3 --raw (repeats the 2 line(s) of t1 shown above",
        ),
    ] {
        let mut args = vec!["show", t.as_str()];
        args.extend_from_slice(&address);
        args.extend(["--raw", "--max-count", cap]);
        let out = h.run(&args);
        assert!(out.success, "{}", out.stderr);
        assert!(
            out.stderr
                .contains(&format!("continue: csift show @{SESS} {want}")),
            "{address:?}: {}",
            out.stderr
        );
    }
}
