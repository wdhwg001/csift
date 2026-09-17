//! An `@<agent-id>` target meets the same shape: a copied project directory carries the
//! whole sidecar, so one `agent-<hex>.jsonl` is on disk under two directories too.

use super::*;

/// The agent id both copies of the session's sidecar carry: a copied project directory
/// brings the whole sidecar with it, so ONE agent hex is on disk under two directories.
const AGENT: &str = "abcdef0123456789";

fn agent_record(n: u32, text: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"uuid\":\"a{n}\",\"timestamp\":\"2026-06-07T0{n}:00:05.000Z\",\
         \"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}\n"
    )
}

/// One session under two project directories, each carrying its own copy of the agent
/// transcript the tests below target directly by `@<agent-id>`.
fn agent_home(under_enc: &str, under_short: &str) -> Home {
    let h = Home::new();
    let body = body_two();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &body);
    h.write(&format!("{ENC_SHORT}/{SESS}.jsonl"), &body);
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        under_enc,
    );
    h.write(
        &format!("{ENC_SHORT}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        under_short,
    );
    h
}

#[test]
fn an_agent_target_merges_its_own_copies_and_names_the_merge() {
    let child = agent_record(1, "child ask");
    let h = agent_home(&child, &child);
    let agent = at(AGENT);

    let out = h.run(&["list", &agent, "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(
        rows.len(),
        1,
        "one row per agent, not one per copy: {rows:?}"
    );
    assert_eq!(rows[0]["duplicate_relation"], "identical");
    assert_eq!(rows[0]["also_at_count"], 1);
    // Equal sizes and no cwd to read, so the winner is the smallest path - which is the
    // answer on every run and every machine, not whatever `read_dir` happened to yield.
    let kept = rows[0]["path"].as_str().expect("a path");
    assert!(kept.contains(ENC), "{kept}");
    assert!(!kept.contains(ENC_SHORT), "{kept}");
    assert!(
        out.stderr.contains("project directories"),
        "an agent target discloses its merge like any other:\n{}",
        out.stderr
    );

    let count = h.run(&["search", "child", &agent, "-c"]);
    assert_eq!(count.stdout.trim(), "1", "{}", count.stderr);
}

#[test]
fn an_agent_targets_prefix_copy_merges_onto_the_longer_one() {
    let shorter = agent_record(1, "child ask");
    let longer = format!("{shorter}{}", agent_record(2, "child again"));
    // The LONGER copy sits under the renamed directory, which sorts last: only the size
    // rule can send the win there, and picking the shorter one would drop a record.
    let h = agent_home(&shorter, &longer);
    let agent = at(AGENT);

    let out = h.run(&["list", &agent, "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0]["duplicate_relation"], "prefix");
    assert!(
        rows[0]["path"]
            .as_str()
            .expect("a path")
            .contains(ENC_SHORT),
        "the longer copy wins: {}",
        rows[0]["path"]
    );
    let header = json_rows(&out.stdout, "header");
    assert_eq!(header[0]["duplicate_transcripts"], 1);
    assert_eq!(header[0]["duplicate_records_dropped"], 1);

    let count = h.run(&["search", "child", &agent, "-c"]);
    assert_eq!(count.stdout.trim(), "2", "the kept copy is the longer one");
}

#[test]
fn an_agent_targets_divergent_copies_both_stay_and_show_names_them() {
    let h = agent_home(
        &agent_record(1, "one branch"),
        &agent_record(1, "another branch"),
    );
    let agent = at(AGENT);

    let out = h.run(&["list", &agent, "--format", "json"]);
    let rows = json_rows(&out.stdout, "session");
    assert_eq!(rows.len(), 2, "nothing merges when the bytes disagree");
    for r in &rows {
        assert_eq!(r["duplicate_relation"], "divergent");
        assert_eq!(r["also_at_count"], 1);
    }
    assert!(out.stderr.contains("DIFFER"), "{}", out.stderr);

    // The one refusal that stays: an address naming two files on disk must not pick one,
    // and `@<agent-id>` cannot separate the copies either - only a path can.
    let show = h.run(&["show", &agent, "--line", "1"]);
    assert!(!show.success, "{}", show.stdout);
    assert!(show.stderr.contains("name a path"), "{}", show.stderr);
    assert!(show.stderr.contains(ENC_SHORT), "{}", show.stderr);
    assert!(show.stderr.contains(ENC), "{}", show.stderr);
}

#[test]
fn the_agents_count_row_equals_the_rows_it_counts_and_the_tree_prints_them() {
    // Two copies of one session whose contents DIFFER merge into nothing, so both sidecars
    // are in scope and each carries its own copy of the agent transcript: two lanes that
    // share an agent id by construction. That is the shape where the count row and the rows
    // it counts used to disagree - the walk deduped by id, the count did not.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &record(1, "one branch"));
    h.write(
        &format!("{ENC_SHORT}/{SESS}.jsonl"),
        &record(1, "another branch"),
    );
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &agent_record(1, "child of one"),
    );
    h.write(
        &format!("{ENC_SHORT}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &agent_record(1, "child of the other"),
    );

    let out = h.run(&["agents", &at(SESS), "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    let session = json_rows(&out.stdout, "session");
    let agents = json_rows(&out.stdout, "agent");
    assert_eq!(
        session[0]["agents"], 2,
        "the count row states what follows it: {}",
        out.stdout
    );
    assert_eq!(
        agents.len(),
        2,
        "every lane in scope gets a row, id-sharing copies included: {}",
        out.stdout
    );
    let summary = json_rows(&out.stdout, "summary");
    assert_eq!(summary[0]["agents"], 2);
    // Two rows that a consumer cannot tell apart are two rows it cannot act on: every other
    // field of the pair is identical by construction, so the transcript path is what names
    // WHICH lane each row is.
    let paths: Vec<&str> = agents
        .iter()
        .map(|a| a["path"].as_str().expect("a path"))
        .collect();
    assert_ne!(paths[0], paths[1], "the rows name different transcripts");
    assert!(
        paths.iter().any(|p| p.contains(ENC)) && paths.iter().any(|p| p.contains(ENC_SHORT)),
        "one row per project directory: {paths:?}"
    );

    // The text tree prints exactly what the rows carry - same set, same count.
    let text = h.run(&["agents", &at(SESS)]);
    assert_eq!(
        text.stdout.matches(&format!("  {AGENT}  ")).count(),
        2,
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("2 subagent(s)"),
        "the footer counts the blocks above it:\n{}",
        text.stdout
    );
    // The text tree names them too, for exactly the ids that occur twice.
    assert_eq!(
        text.stdout.matches("transcript ").count(),
        2,
        "two blocks sharing an id each name their own transcript:\n{}",
        text.stdout
    );
    assert!(text.stdout.contains(ENC_SHORT), "{}", text.stdout);
}

#[test]
fn a_session_and_its_agent_in_one_invocation_resolve_one_copy_of_that_agent() {
    // TWO paths reach the same `agent-<hex>.jsonl`: the session target gathers it from the
    // sidecar of the session it kept, the agent target finds it under both directories.
    // They used to merge separately and pick DIFFERENT winners - the session path inherits
    // the session's winner, the agent path decides on its own bytes and, at equal sizes
    // with no `cwd` on an agent record, takes the smallest path - so both copies survived
    // the path-level dedup and every row claimed the other had been dropped.
    let child = agent_record(1, "child ask");
    let h = agent_home(&child, &child);
    let out = h.run(&["list", &at(SESS), &at(AGENT), "--format", "json"]);
    assert!(out.success, "{}", out.stderr);

    let rows = json_rows(&out.stdout, "session");
    let agents: Vec<_> = rows.iter().filter(|r| r["is_subagent"] == true).collect();
    assert_eq!(
        agents.len(),
        1,
        "one row per agent transcript, not one per path that found it: {}",
        out.stdout
    );
    let header = json_rows(&out.stdout, "header");
    // One session copy dropped and one agent copy dropped - 2 transcripts, and the lines
    // are the 2 of the session copy plus the 1 of the agent copy.
    assert_eq!(header[0]["duplicate_transcripts"], 2, "{}", header[0]);
    assert_eq!(header[0]["duplicate_records_dropped"], 3, "{}", header[0]);
    // ONE note per duplicated id, not one per path that decided it.
    assert_eq!(
        out.stderr.matches("project directories").count(),
        2,
        "one note per group:\n{}",
        out.stderr
    );

    // The merged truth, from a surface that counts records rather than rows.
    let count = h.run(&["search", "ask", &at(SESS), &at(AGENT), "-c"]);
    assert_eq!(count.stdout.trim(), "3", "{}", count.stderr);
}

#[test]
fn agents_merges_its_own_sidecar_copies_and_agrees_with_list_and_search() {
    // `agents` resolves TopLevelOnly and walks the sidecars itself, so the resolver's merge
    // never sees them - and this is the shape where that mattered: the sessions DIFFER so
    // both stay, and their two sidecars hold ONE byte-identical agent transcript. The
    // command whose whole job is counting agents used to be the one that counted this twice.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &record(1, "one branch"));
    h.write(
        &format!("{ENC_SHORT}/{SESS}.jsonl"),
        &record(1, "another branch"),
    );
    let child = agent_record(1, "child ask");
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &child,
    );
    h.write(
        &format!("{ENC_SHORT}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &child,
    );
    let sess = at(SESS);

    let out = h.run(&["agents", &sess, "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    let agents = json_rows(&out.stdout, "agent");
    assert_eq!(
        agents.len(),
        1,
        "one lane, not one per session copy: {}",
        out.stdout
    );
    let session = json_rows(&out.stdout, "session");
    assert_eq!(session[0]["agents"], 1, "{}", out.stdout);
    let header = json_rows(&out.stdout, "header");
    assert_eq!(
        header[0]["duplicate_transcripts"], 1,
        "the merge it performed is in its OWN header: {}",
        header[0]
    );
    assert_eq!(header[0]["duplicate_records_dropped"], 1, "{}", header[0]);
    assert!(
        out.stderr.contains("project directories"),
        "and named on stderr like any other merge:\n{}",
        out.stderr
    );

    // The three surfaces agree, which is the whole point of the item.
    let list = h.run(&["list", &sess, "--format", "json"]);
    let sub_rows = json_rows(&list.stdout, "session");
    let subs = sub_rows.iter().filter(|r| r["is_subagent"] == true).count();
    assert_eq!(subs, 1, "{}", list.stdout);
    let count = h.run(&["search", "child ask", &sess, "-c"]);
    assert_eq!(count.stdout.trim(), "1", "{}", count.stderr);

    let text = h.run(&["agents", &sess]);
    assert!(text.stdout.contains("1 subagent(s)"), "{}", text.stdout);
}

#[test]
fn the_sidecars_of_a_divergent_pair_still_merge_their_identical_agents() {
    // The sessions DIFFER, so neither is dropped and BOTH sidecars are walked - and those
    // hold one agent transcript twice, which every count would otherwise double.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &record(1, "one branch"));
    h.write(
        &format!("{ENC_SHORT}/{SESS}.jsonl"),
        &record(1, "another branch"),
    );
    let child = agent_record(1, "child ask");
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &child,
    );
    h.write(
        &format!("{ENC_SHORT}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &child,
    );

    let count = h.run(&["search", "child ask", &at(SESS), "-c"]);
    assert_eq!(count.stdout.trim(), "1", "{}", count.stderr);
    let json = h.run(&["search", "child", &at(SESS), "--format", "json"]);
    let header = json_rows(&json.stdout, "header");
    assert_eq!(
        header[0]["duplicate_transcripts"], 1,
        "the merged agent copy is disclosed like any other: {}",
        header[0]
    );
    assert_eq!(header[0]["duplicate_records_dropped"], 1);
}
