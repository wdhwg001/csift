//! One session id under two project directories: the relation test and the winner rule.

use super::*;

/// A synthetic projects tree: `<root>/<encoded-dir>/<uuid>.jsonl`, all names generated.
fn transcript(tree: &Tree, dir: &str, id: &str, body: &str) -> PathBuf {
    tree.write(&format!("{dir}/{id}.jsonl"), body)
}

const ID: &str = "11111111-2222-4333-8444-555555555555";

/// One record line carrying a `cwd`, so the tie-break has something to read.
fn line(cwd: &str, n: usize) -> String {
    format!("{{\"type\":\"user\",\"cwd\":\"{cwd}\",\"uuid\":\"u{n}\"}}\n")
}

#[test]
fn a_session_id_under_one_directory_is_not_a_duplicate() {
    let tree = Tree::new();
    let a = transcript(&tree, "-Users-dev-proj", ID, &line("/Users/dev/proj", 1));
    let (kept, groups) = dedup_copies(vec![a.clone()]);
    assert_eq!(kept, vec![a]);
    assert!(groups.is_empty());
}

#[test]
fn byte_identical_copies_merge_onto_the_directory_the_recorded_cwd_encodes() {
    let tree = Tree::new();
    let body = line("/Users/dev/proj", 1) + &line("/Users/dev/proj", 2);
    // The SHORT-name directory is the one a rename leaves behind: it matches no record's
    // cwd, so the tie-break sends the win to the directory the first cwd encodes.
    let short = transcript(&tree, "-Users-dev-p", ID, &body);
    let long = transcript(&tree, "-Users-dev-proj", ID, &body);

    let (kept, groups) = dedup_copies(vec![short.clone(), long.clone()]);
    assert_eq!(kept, vec![long.clone()]);
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(g.session_id, ID);
    assert_eq!(g.relation, DuplicateRelation::Identical);
    assert_eq!(g.relation.slug(), "identical");
    assert_eq!(g.kept, long);
    assert_eq!(g.dropped, vec![short]);
    // Two records in each copy, so the merge removed two lines from the scope.
    assert_eq!(g.dropped_lines(), 2);
}

#[test]
fn a_strict_byte_prefix_merges_onto_the_longer_copy() {
    let tree = Tree::new();
    let shorter = line("/Users/dev/proj", 1);
    let longer = shorter.clone() + &line("/Users/dev/proj", 2) + &line("/Users/dev/proj", 3);
    // The SHORT copy sits in the directory the cwd encodes, so if size did not decide
    // first the tie-break would hand the win to the copy missing two records.
    let cut = transcript(&tree, "-Users-dev-proj", ID, &shorter);
    let whole = transcript(&tree, "-Users-dev-p", ID, &longer);

    let (kept, groups) = dedup_copies(vec![cut.clone(), whole.clone()]);
    assert_eq!(kept, vec![whole.clone()]);
    let g = &groups[0];
    assert_eq!(g.relation, DuplicateRelation::Prefix);
    assert_eq!(g.relation.slug(), "prefix");
    assert_eq!(g.kept, whole);
    assert_eq!(g.dropped, vec![cut]);
    assert_eq!(g.dropped_lines(), 1);
    assert_eq!(g.members[0].lines, 3);
}

#[test]
fn copies_that_disagree_are_never_merged() {
    let tree = Tree::new();
    let a = transcript(&tree, "-Users-dev-one", ID, &line("/Users/dev/one", 1));
    let b = transcript(&tree, "-Users-dev-two", ID, &line("/Users/dev/two", 9));

    let (mut kept, groups) = dedup_copies(vec![a.clone(), b.clone()]);
    kept.sort();
    let mut both = vec![a, b];
    both.sort();
    assert_eq!(kept, both, "a divergent group keeps every copy in scope");
    let g = &groups[0];
    assert_eq!(g.relation, DuplicateRelation::Divergent);
    assert_eq!(g.relation.slug(), "divergent");
    assert!(g.dropped.is_empty());
    assert_eq!(g.dropped_lines(), 0);
    assert_eq!(g.members.len(), 2);
}

#[test]
fn one_diverging_copy_makes_the_whole_group_unmergeable() {
    let tree = Tree::new();
    let body = line("/Users/dev/proj", 1);
    let a = transcript(&tree, "-Users-dev-a", ID, &body);
    let b = transcript(&tree, "-Users-dev-b", ID, &body);
    let c = transcript(&tree, "-Users-dev-c", ID, &line("/Users/dev/proj", 7));

    let (kept, groups) = dedup_copies(vec![a, b, c]);
    assert_eq!(kept.len(), 3);
    assert_eq!(groups[0].relation, DuplicateRelation::Divergent);
    assert!(groups[0].dropped.is_empty());
}

#[test]
#[cfg(unix)]
fn an_unreadable_copy_merges_nothing_and_the_note_names_it_with_the_error() {
    use std::os::unix::fs::PermissionsExt;
    let tree = Tree::new();
    let body = line("/Users/dev/proj", 1) + &line("/Users/dev/proj", 2);
    // Two byte-identical copies - the shape that WOULD merge - with one of them
    // unreadable. Read as `&[]` the locked copy is an EMPTY one, which is a prefix of
    // every other copy and, being larger by metadata, wins the order: the note would then
    // announce the readable copy as dropped and the command would die on the one it kept.
    let open = transcript(&tree, "-Users-dev-proj", ID, &body);
    let locked = transcript(&tree, "-Users-dev-p", ID, &body);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    // Root - and any filesystem that does not enforce modes - still opens it, and then
    // there is no unreadable copy to test: skip rather than assert the opposite.
    if std::fs::File::open(&locked).is_ok() {
        return;
    }

    let (mut kept, groups) = dedup_copies(vec![open.clone(), locked.clone()]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    kept.sort();
    let mut both = vec![open, locked.clone()];
    both.sort();
    assert_eq!(kept, both, "a copy csift could not read merges nothing");
    let g = &groups[0];
    assert_eq!(g.relation, DuplicateRelation::Unknown);
    assert_eq!(g.relation.slug(), "unknown");
    assert!(!g.relation.merges());
    assert!(g.dropped.is_empty());
    assert_eq!(g.dropped_lines(), 0);

    let note = duplicate_note(g);
    assert!(note.contains("could not read"), "{note}");
    assert!(note.contains(&locked.display().to_string()), "{note}");
    // The error the filesystem returned, not a csift paraphrase of it.
    assert!(note.contains("os error 13"), "{note}");
    assert!(!note.contains("dropping"), "{note}");
}

#[test]
fn an_equal_size_tie_no_cwd_can_break_falls_to_the_smallest_path() {
    let tree = Tree::new();
    // Neither directory name encodes the recorded cwd, so only the path ordering is left -
    // and it has to be the same answer on every run and every machine.
    let body = line("/Users/dev/elsewhere", 1);
    let a = transcript(&tree, "-Users-dev-aaa", ID, &body);
    let b = transcript(&tree, "-Users-dev-bbb", ID, &body);

    let (kept, _) = dedup_copies(vec![b.clone(), a.clone()]);
    assert_eq!(kept, vec![a.clone()]);
    let (kept_again, _) = dedup_copies(vec![a.clone(), b]);
    assert_eq!(kept_again, vec![a]);
}

#[test]
fn the_first_cwd_probe_reads_past_a_record_that_carries_none() {
    let tree = Tree::new();
    // Physical line 1 of a real transcript is routinely a session-state line with no cwd;
    // the tie-break reads on until a record carries one, which the line-1 collision-guard
    // read deliberately does not.
    let body =
        "{\"type\":\"mode\",\"mode\":\"default\"}\n".to_string() + &line("/Users/dev/proj", 1);
    let short = transcript(&tree, "-Users-dev-p", ID, &body);
    let long = transcript(&tree, "-Users-dev-proj", ID, &body);

    let (kept, _) = dedup_copies(vec![short, long.clone()]);
    assert_eq!(kept, vec![long]);
}

#[test]
fn the_head_window_steps_over_a_huge_leading_line_to_reach_the_first_cwd() {
    let tree = Tree::new();
    // The window exists to clear a big leading record (an image line runs to megabytes).
    // Two copies of EQUAL size, so only the cwd tie-break can decide: `-Users-dev-aaa`
    // sorts before `-Users-dev-proj`, so a probe that cannot reach the cwd hands the win
    // to the directory that does NOT encode it.
    let big = format!(
        "{{\"type\":\"attachment\",\"pad\":\"{}\"}}\n",
        "x".repeat(4096)
    );
    let body = big + &line("/Users/dev/proj", 1);
    let other = transcript(&tree, "-Users-dev-aaa", ID, &body);
    let home = transcript(&tree, "-Users-dev-proj", ID, &body);

    let (kept, _) = dedup_copies(vec![other, home.clone()]);
    assert_eq!(kept, vec![home]);
}

#[test]
fn a_torn_final_line_still_counts_as_a_record() {
    let tree = Tree::new();
    // No trailing newline on the shorter copy: a crash-truncated tail is still a line the
    // merge removed, so the disclosure has to count it.
    let shorter = format!("{}{{\"type\":\"user\"", line("/Users/dev/proj", 1));
    let longer = format!("{shorter}}}\n");
    let cut = transcript(&tree, "-Users-dev-a", ID, &shorter);
    let whole = transcript(&tree, "-Users-dev-b", ID, &longer);

    let (_, groups) = dedup_copies(vec![cut, whole]);
    assert_eq!(groups[0].relation, DuplicateRelation::Prefix);
    assert_eq!(groups[0].dropped_lines(), 2);
}

#[test]
fn the_header_fields_sum_every_group() {
    let tree = Tree::new();
    let body = line("/Users/dev/proj", 1);
    let id2 = "66666666-7777-4888-8999-aaaaaaaaaaaa";
    let a = transcript(&tree, "-Users-dev-a", ID, &body);
    let b = transcript(&tree, "-Users-dev-b", ID, &body);
    let c = transcript(&tree, "-Users-dev-a", id2, &body);
    let d = transcript(&tree, "-Users-dev-b", id2, &body);

    let (_, groups) = dedup_copies(vec![a, b, c, d]);
    assert_eq!(groups.len(), 2);
    let fields = duplicate_header_fields(&groups);
    assert_eq!(fields["duplicate_transcripts"], 2);
    assert_eq!(fields["duplicate_records_dropped"], 2);
}

#[test]
fn no_duplicate_reports_zero_rather_than_omitting_the_keys() {
    let fields = duplicate_header_fields(&[]);
    assert_eq!(fields["duplicate_transcripts"], 0);
    assert_eq!(fields["duplicate_records_dropped"], 0);
    assert_eq!(fields["divergent_transcripts"], 0);
    assert_eq!(fields["unreadable_transcripts"], 0);
    assert_eq!(
        fields.as_object().map(serde_json::Map::len),
        Some(4),
        "the header key set is a contract: {fields}"
    );
}

#[test]
fn the_note_names_the_kept_copy_the_dropped_one_and_how_they_relate() {
    let tree = Tree::new();
    let shorter = line("/Users/dev/proj", 1);
    let longer = shorter.clone() + &line("/Users/dev/proj", 2);
    let cut = transcript(&tree, "-Users-dev-a", ID, &shorter);
    let whole = transcript(&tree, "-Users-dev-b", ID, &longer);

    let (_, groups) = dedup_copies(vec![cut.clone(), whole.clone()]);
    let note = duplicate_note(&groups[0]);
    assert!(note.contains(&ID[..8]), "{note}");
    assert!(note.contains("PREFIX"), "{note}");
    assert!(note.contains(&whole.display().to_string()), "{note}");
    assert!(note.contains(&cut.display().to_string()), "{note}");
}

#[test]
fn a_divergent_note_gives_both_record_counts_and_refuses_to_merge() {
    let tree = Tree::new();
    let a = transcript(&tree, "-Users-dev-a", ID, &line("/Users/dev/one", 1));
    let b = transcript(
        &tree,
        "-Users-dev-b",
        ID,
        &(line("/Users/dev/two", 9) + &line("/Users/dev/two", 8)),
    );

    let (_, groups) = dedup_copies(vec![a, b]);
    let note = duplicate_note(&groups[0]);
    assert!(note.contains("DIFFER"), "{note}");
    assert!(note.contains("2 lines"), "{note}");
    assert!(note.contains("1 line at"), "{note}");
}

/// The note says WHERE a group's copies sit, in words a reader can check against the paths it
/// prints: two project directories, two sidecars of one project, two workflow runs of one
/// session, or a workflow run beside the built-in location. Each shape is its own sentence.
#[test]
fn the_note_names_where_the_copies_sit_by_their_shape() {
    let group = |paths: [&str; 2]| DuplicateGroup {
        session_id: "agent-a0f1e2d3c4b5a6978.jsonl".to_string(),
        relation: DuplicateRelation::Divergent,
        kept: PathBuf::from(paths[0]),
        dropped: Vec::new(),
        members: paths
            .iter()
            .map(|p| DuplicateMember {
                path: PathBuf::from(p),
                lines: 2,
                error: None,
            })
            .collect(),
    };
    for (paths, want) in [
        (
            ["/r/-Users-dev-a/s1.jsonl", "/r/-Users-dev-b/s1.jsonl"],
            "on disk under 2 project directories",
        ),
        (
            [
                "/r/-Users-dev-a/s1/subagents/agent-x.jsonl",
                "/r/-Users-dev-a/s2/subagents/agent-x.jsonl",
            ],
            "on disk at 2 paths under 1 project directory",
        ),
        (
            [
                "/r/-Users-dev-a/s1/subagents/workflows/wf_a/agent-x.jsonl",
                "/r/-Users-dev-a/s1/subagents/workflows/wf_b/agent-x.jsonl",
            ],
            "on disk at 2 paths under one session's sidecar",
        ),
        (
            [
                "/r/-Users-dev-a/s1/subagents/workflows/wf_a/agent-x.jsonl",
                "/r/-Users-dev-a/s1/subagents/agent-x.jsonl",
            ],
            "on disk under one session's sidecar at the built-in and the workflow locations",
        ),
    ] {
        let note = duplicate_note(&group(paths));
        assert!(note.contains(want), "{paths:?}: {note}");
    }
}
