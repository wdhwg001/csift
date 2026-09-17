//! `--for <ADDR>`: everything that is FOR one record, transitively.
//!
//! The forward surfaces are pinned over the shared reachability fixture, which carries one
//! line per leaf and nothing that CHAINS. The reverse query needs the opposite shape: a
//! short transcript where one message draws a reply, the reply makes a call, the call
//! returns, a background task reports on it two turns later - and where an unrelated turn
//! sits beside all of it, so "everything" can be shown to mean "everything reachable" and
//! not "everything".

use crate::harness::*;

const ENC: &str = "-Users-dev-reverse-project";
const SESS: &str = "5c6d7e8f-9a0b-4c1d-8e2f-3a4b5c6d7e8f";

/// The chain under test, and one turn beside it.
///
/// L2 is the human message every line below it descends from, directly or through the
/// call. L4's `tool_result` answers the call L3 made, which is the `result_of` relation;
/// L5's notification names that same call in its `<tool-use-id>` tag two turns later, which
/// is `reports` and is the hop a parent walk alone would never make. L6 and L7 are a
/// SEPARATE turn: they must not come back.
const TRANSCRIPT: &str = concat!(
    r#"{"type":"user","uuid":"r1","timestamp":"2026-06-08T01:00:00.000Z","message":{"role":"user","content":"zzrev set the whole thing up"}}"#,
    "\n",
    r#"{"type":"user","uuid":"r2","parentUuid":"r1","timestamp":"2026-06-08T01:00:01.000Z","message":{"role":"user","content":"zzrev build the project in the background"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"r3","parentUuid":"r2","timestamp":"2026-06-08T01:00:02.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01rev","name":"Bash","input":{"command":"zzrev cargo build","run_in_background":true}}]}}"#,
    "\n",
    r#"{"type":"user","uuid":"r4","parentUuid":"r3","timestamp":"2026-06-08T01:00:03.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_01rev","content":"zzrev Command running in background with ID: b0000001."}]}}"#,
    "\n",
    r#"{"type":"user","uuid":"r5","parentUuid":"r4","timestamp":"2026-06-08T01:02:00.000Z","message":{"role":"user","content":"<task-notification>\n<task-id>b0000001</task-id>\n<tool-use-id>toolu_01rev</tool-use-id>\n<status>completed</status>\n<summary>zzrev background command completed</summary>\n</task-notification>"}}"#,
    "\n",
    r#"{"type":"user","uuid":"r6","parentUuid":"r5","timestamp":"2026-06-08T01:03:00.000Z","message":{"role":"user","content":"zzrev a different question entirely"}}"#,
    "\n",
    r#"{"type":"assistant","uuid":"r7","parentUuid":"r6","timestamp":"2026-06-08T01:03:01.000Z","message":{"role":"assistant","content":[{"type":"text","text":"zzrev answering the different question"}]}}"#,
    "\n",
);

fn home() -> Home {
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), TRANSCRIPT);
    h
}

/// The lines `--for <addr>` returns, ascending.
fn lines_for(h: &Home, addr: &str) -> Vec<u64> {
    let out = h.run(&[
        "search",
        "",
        &format!("@{SESS}"),
        "--for",
        addr,
        "--format",
        "json",
    ]);
    assert!(out.success, "--for {addr} failed: {}", out.stderr);
    let mut v: Vec<u64> = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .filter_map(|hit| hit["line"].as_u64())
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// The closure follows THREE relations and stops where the provenance stops.
///
/// From L2 it reaches the assistant that answered it (the parent link), the tool result
/// that answered the assistant's call (`result_of`, by the API's own id), and the
/// notification that reports on the same call two turns later (`reports`, by the
/// `<tool-use-id>` the pulse names). It does NOT reach the separate turn below, which is
/// what makes this a closure over provenance rather than a window over the file.
#[test]
fn the_closure_follows_the_parent_link_the_call_and_the_pulse() {
    let h = home();
    assert_eq!(
        lines_for(&h, "L2"),
        vec![2, 3, 4, 5],
        "the addressed record, its reply, the result of its call, and the pulse that \
         reports on that call"
    );
}

/// Naming the record by UUID gives the same set: the address form is a way in, not a
/// different question.
#[test]
fn a_uuid_address_and_a_line_address_name_the_same_record() {
    let h = home();
    assert_eq!(lines_for(&h, "r2"), lines_for(&h, "L2"));
}

/// A BARE number is the same address as `L<n>`: it is what a reader types after reading
/// `L2` off a hit line, and refusing it would be a grammar lesson rather than an answer.
#[test]
fn a_bare_number_is_a_line_address() {
    let h = home();
    assert_eq!(lines_for(&h, "2"), lines_for(&h, "L2"));
}

/// The closure STARTS at the address: asking about the tool call returns the call, its
/// result and the pulse, and nothing above it. The answer is what the record DREW, not the
/// turn it sat in.
#[test]
fn the_closure_starts_at_the_address_and_looks_only_downstream() {
    let h = home();
    assert_eq!(lines_for(&h, "L3"), vec![3, 4, 5]);
    assert_eq!(
        lines_for(&h, "L5"),
        vec![5],
        "nothing reports on the pulse, and the person's next message is not for it - the \
         parent link stops at a turn boundary"
    );
}

/// The summary says what the emitted set IS: the address in its canonical form, the record
/// count and how many transcripts contributed.
///
/// `records` counts RECORDS and not hits, the same grouping every census counts by: a
/// record whose several sections matched is one record, and a closure that reported
/// sections would count a text-plus-tool_use turn twice.
#[test]
fn the_summary_names_the_address_and_the_size_of_the_set() {
    let h = home();
    let out = h.run(&[
        "search",
        "",
        &format!("@{SESS}"),
        "--for",
        "2",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let sum = json_summary(&out.stdout);
    assert_eq!(
        sum["for_of"]["address"], "L2",
        "the canonical form, not the reader's spelling"
    );
    assert_eq!(sum["for_of"]["records"], 4);
    assert_eq!(sum["for_of"]["transcripts"], 1);
}

/// An ordinary search carries NO `for_of` at all. A null would read as a closure that found
/// nothing, which is a different answer from a query that asked for none.
#[test]
fn an_ordinary_search_reports_no_closure() {
    let h = home();
    let out = h.run(&["search", "zzrev", &format!("@{SESS}"), "--format", "json"]);
    assert!(out.success, "stderr: {}", out.stderr);
    assert!(
        json_summary(&out.stdout).get("for_of").is_none(),
        "for_of rides the summary only under the flag"
    );
}

/// An address that names no record is an EMPTY set and exit 0 - a definitive absence like
/// any other zero-match search, with the reverse query echoed among the active filters so
/// the reader can tell "the closure reached nothing" from "the pattern matched nothing".
#[test]
fn an_address_that_names_nothing_is_a_definitive_absence() {
    let h = home();
    let out = h.run(&[
        "search",
        "",
        &format!("@{SESS}"),
        "--for",
        "L999",
        "--format",
        "json",
    ]);
    assert!(
        out.success,
        "a definitive absence is exit 0: {}",
        out.stderr
    );
    let sum = json_summary(&out.stdout);
    assert_eq!(sum["matched"], 0);
    assert_eq!(sum["definitive_absence"], true);
    assert!(
        sum["active_filters"]
            .as_str()
            .unwrap_or_default()
            .contains("--for L999"),
        "the reverse query is echoed among the active filters: {sum}"
    );
}

/// A malformed address is a USAGE error, reported before any file is read.
#[test]
fn a_zero_line_address_is_rejected() {
    let h = home();
    let out = h.run(&["search", "", &format!("@{SESS}"), "--for", "L0"]);
    assert!(!out.success, "L0 is not a line: {}", out.stdout);
    assert!(
        out.stderr.contains("1-based"),
        "the error says why: {}",
        out.stderr
    );
}

/// `--for` composes with the label filter: the closure decides WHICH records, `-t` decides
/// which of them are shown.
#[test]
fn the_closure_composes_with_the_label_filter() {
    let h = home();
    let out = h.run(&[
        "search",
        "",
        &format!("@{SESS}"),
        "--for",
        "L2",
        "-t",
        "agent.tool.use",
        "--format",
        "json",
    ]);
    assert!(out.success, "stderr: {}", out.stderr);
    let lines: Vec<u64> = json_rows(&out.stdout, "exchange")
        .iter()
        .flat_map(|r| r["hits"].as_array().cloned().unwrap_or_default())
        .filter_map(|hit| hit["line"].as_u64())
        .collect();
    assert_eq!(lines, vec![3], "the one call inside the closure");
}
