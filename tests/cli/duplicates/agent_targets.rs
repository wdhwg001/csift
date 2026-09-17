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
    // Equal sizes, and the cwd tie-break cannot fire on a sidecar path (it compares the
    // file's parent directory name, which here is `subagents`), so the winner is the
    // smallest path - the answer on every run and every machine, not whatever `read_dir`
    // happened to yield.
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
    // What was duplicated is an AGENT, and the refusal says so: a reader told these are
    // copies of one session id goes looking for a uuid that is not in play here.
    assert!(
        show.stderr.contains(&format!("subagent {AGENT}")),
        "the bail names what it refused, in the resolver's own words:\n{}",
        show.stderr
    );
    assert!(!show.stderr.contains("session id"), "{}", show.stderr);
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
    // the session's winner, the agent path decides on its own bytes and, at equal sizes,
    // takes the smallest path (the cwd tie-break reads a parent directory name and a
    // sidecar's is `subagents`, so it never fires here) - so both copies survived
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
    // An agent group is NAMED as one: its id is the shared basename `agent-<hex>`, whose
    // first eight characters are the prefix every agent group shares.
    assert!(
        out.stderr.contains(&format!("subagent {AGENT}")),
        "the note names the subagent by its canonical id:\n{}",
        out.stderr
    );
    assert!(!out.stderr.contains("session agent-"), "{}", out.stderr);

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
fn an_ordinary_listing_names_no_transcript_and_one_run_stays_one_row() {
    // The `transcript` line exists to separate two blocks that share an id. With one copy
    // of everything there is nothing to separate, and a path per agent would be one line of
    // noise per agent - so the ordinary listing prints none.
    let h = Home::new();
    h.write(&format!("{ENC}/{SESS}.jsonl"), &body_two());
    h.write(
        &format!("{ENC}/{SESS}/subagents/agent-{AGENT}.jsonl"),
        &agent_record(1, "child ask"),
    );
    let plain = h.run(&["agents", &at(SESS)]);
    assert!(plain.success, "{}", plain.stderr);
    assert!(
        !plain.stdout.contains("transcript "),
        "no id occurs twice, so nothing needs separating:\n{}",
        plain.stdout
    );

    // A copied project directory carries the workflow MANIFESTS too, so the same run is
    // discovered once per session copy. One run is one row.
    let run = "{\"runId\":\"wf_dup1\",\"workflowName\":\"demo\",\"status\":\"completed\",\
               \"agentCount\":1}";
    let h2 = Home::new();
    h2.write(&format!("{ENC}/{SESS}.jsonl"), &record(1, "one branch"));
    h2.write(
        &format!("{ENC_SHORT}/{SESS}.jsonl"),
        &record(1, "another branch"),
    );
    for enc in [ENC, ENC_SHORT] {
        h2.write(&format!("{enc}/{SESS}/workflows/wf_dup1.json"), run);
        h2.write(
            &format!("{enc}/{SESS}/subagents/workflows/wf_dup1/agent-{AGENT}.jsonl"),
            &agent_record(1, "child ask"),
        );
    }
    let out = h2.run(&["agents", &at(SESS), "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    let runs = json_rows(&out.stdout, "run");
    assert_eq!(
        runs.len(),
        1,
        "one run, not one per session copy: {}",
        out.stdout
    );
    let session = json_rows(&out.stdout, "session");
    assert_eq!(session[0]["runs"], 1, "{}", out.stdout);
}

#[test]
fn two_sessions_that_share_a_run_id_keep_a_row_each() {
    // The copy fold keys on the PAIR (session, run). A run id is unique inside the session
    // that minted it and nothing makes it unique across sessions, so an id-keyed fold would
    // drop the second session's run as though it were the first one's copy - the same row
    // this file's sibling test wants folded, wanted kept.
    let other = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c88";
    let run = "{\"runId\":\"wf_same\",\"workflowName\":\"demo\",\"status\":\"completed\",\
               \"agentCount\":1}";
    let h = Home::new();
    for (sess, agent) in [(SESS, AGENT), (other, "beefcafe01234567")] {
        h.write(&format!("{ENC}/{sess}.jsonl"), &record(1, "a session"));
        h.write(&format!("{ENC}/{sess}/workflows/wf_same.json"), run);
        h.write(
            &format!("{ENC}/{sess}/subagents/workflows/wf_same/agent-{agent}.jsonl"),
            &agent_record(1, "child ask"),
        );
    }
    let out = h.run(&["agents", "--format", "json"]);
    assert!(out.success, "{}", out.stderr);
    let runs = json_rows(&out.stdout, "run");
    assert_eq!(runs.len(), 2, "one run row per session: {}", out.stdout);
    let owners: Vec<&str> = runs
        .iter()
        .map(|r| r["session_id"].as_str().expect("a session id"))
        .collect();
    assert!(owners.contains(&SESS), "{owners:?}");
    assert!(owners.contains(&other), "{owners:?}");
}

#[test]
#[cfg(unix)]
fn an_unreadable_copy_reaches_the_header_of_the_one_surface_that_survives_it() {
    use std::os::unix::fs::PermissionsExt;
    let h = Home::new();
    // A duplicated session whose second copy csift cannot read, and which has NO sidecar
    // directory beside it - `agents` opens a session file only through its sidecar, so this
    // copy is never read and the run completes.
    h.write(&format!("{ENC}/{SESS}.jsonl"), &body_two());
    h.write(&format!("{ENC_SHORT}/{SESS}.jsonl"), &body_two());
    // An unrelated session with one agent, so the listing has a lane to print.
    let other = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c99";
    h.write(&format!("{ENC}/{other}.jsonl"), &body_two());
    h.write(
        &format!("{ENC}/{other}/subagents/agent-{AGENT}.jsonl"),
        &agent_record(1, "child ask"),
    );

    let locked = h.projects().join(ENC_SHORT).join(format!("{SESS}.jsonl"));
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root - and any filesystem that does not enforce modes - can still read it, and then
    // there is no unreadable copy to test.
    if std::fs::File::open(&locked).is_ok() {
        return;
    }
    let agents = h.run(&["agents", "--format", "json"]);
    let list = h.run(&["list", "--no-subagents"]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();

    assert!(
        agents.success,
        "`agents` reads a session file only through its sidecar:\n{}",
        agents.stderr
    );
    let header = json_rows(&agents.stdout, "header");
    assert_eq!(
        header[0]["unreadable_transcripts"], 1,
        "the one place a reader of the STREAM learns it: {}",
        header[0]
    );
    assert_eq!(header[0]["duplicate_transcripts"], 0, "{}", header[0]);
    assert_eq!(header[0]["divergent_transcripts"], 0, "{}", header[0]);
    assert!(
        agents.stderr.contains("could not read"),
        "and the note is the one a reader of stderr gets:\n{}",
        agents.stderr
    );
    assert_eq!(
        json_rows(&agents.stdout, "agent").len(),
        1,
        "{}",
        agents.stdout
    );

    // The surfaces that read EVERY file in scope die on that copy instead - fail-loud, after
    // the same note and before any header of their own.
    assert!(!list.success, "{}", list.stdout);
    assert!(list.stderr.contains("Permission denied"), "{}", list.stderr);
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
