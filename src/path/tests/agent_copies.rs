//! An `@<agent-id>` target meets the two-directory shape from the other side: a copied
//! project directory carries the whole sidecar with it, so ONE agent hex is owned by two
//! copies of one session. The three relations, at the entry that target actually takes.

use super::*;

const UUID: &str = "11111111-2222-4333-8444-555555555555";
const HEX: &str = "abcdef0123456789";
const DIR_A: &str = "-Users-dev-aaa";
const DIR_B: &str = "-Users-dev-bbb";

fn rec(n: usize, text: &str) -> String {
    format!(
        "{{\"type\":\"user\",\"uuid\":\"a{n}\",\
         \"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}\n"
    )
}

/// Two copies of one session, each carrying its own copy of the agent transcript.
fn two_copies(tree: &Tree, under_a: &str, under_b: &str) -> Vec<ProjectDir> {
    for (enc, body) in [(DIR_A, under_a), (DIR_B, under_b)] {
        tree.write(&format!("{enc}/{UUID}.jsonl"), &rec(0, "the session"));
        tree.write(&format!("{enc}/{UUID}/subagents/agent-{HEX}.jsonl"), body);
    }
    [DIR_A, DIR_B]
        .into_iter()
        .map(|enc| ProjectDir {
            dir: tree.path(enc),
            target_cwd: None,
        })
        .collect()
}

/// What the agent path COLLECTS - every copy it found, unmerged.
fn found(dirs: &[ProjectDir]) -> Vec<PathBuf> {
    resolve_agent_subtree(dirs, HEX, SubagentScope::TopLevelOnly).expect("the agent resolves")
}

/// What the RESOLVER then does with it: the ONE shared merge over the union. The two halves
/// are kept apart here for the same reason they are kept apart in the resolver - collecting
/// the copies is this module's job, deciding them is `dedup_copies`'.
fn resolve(dirs: &[ProjectDir]) -> Resolved {
    let (files, duplicates) = dedup_copies(found(dirs));
    Resolved { files, duplicates }
}

#[test]
fn identical_agent_copies_merge_onto_one() {
    let tree = Tree::new();
    let body = rec(1, "child ask");
    let r = resolve(&two_copies(&tree, &body, &body));

    assert_eq!(r.files.len(), 1, "{:?}", r.files);
    assert_eq!(r.duplicates.len(), 1);
    let g = &r.duplicates[0];
    assert_eq!(g.relation, DuplicateRelation::Identical);
    // The group's id is the shared BASENAME, which for a subagent is the `agent-` stem -
    // the reason the row join is on the path rather than on the id.
    assert_eq!(g.session_id, format!("agent-{HEX}"));
    assert_eq!(r.files[0], g.kept);
    assert_eq!(g.dropped.len(), 1);
}

#[test]
fn an_agent_group_is_named_as_a_subagent_not_a_session() {
    let tree = Tree::new();
    let body = rec(1, "child ask");
    let r = resolve(&two_copies(&tree, &body, &body));
    let g = &r.duplicates[0];

    // The group's id is the shared BASENAME, `agent-<hex>`, so cutting it to eight
    // characters yields `agent-ab` - what every agent group on disk starts with, and
    // nothing a reader can feed back to csift.
    assert_eq!(g.session_id, format!("agent-{HEX}"));
    assert_eq!(g.subject(), format!("subagent {HEX}"));
    let note = duplicate_note(g);
    assert!(note.starts_with(&format!("subagent {HEX} ")), "{note}");
    assert!(!note.contains("session agent-"), "{note}");
}

#[test]
fn a_prefix_agent_copy_merges_onto_the_longer_one() {
    let tree = Tree::new();
    let shorter = rec(1, "child ask");
    let longer = format!("{shorter}{}", rec(2, "child again"));
    // The longer copy sits in the directory that sorts LAST, so only the size rule can
    // send the win there - and the shorter copy is missing a record.
    let r = resolve(&two_copies(&tree, &shorter, &longer));

    assert_eq!(r.files.len(), 1, "{:?}", r.files);
    let g = &r.duplicates[0];
    assert_eq!(g.relation, DuplicateRelation::Prefix);
    assert!(r.files[0].starts_with(tree.path(DIR_B)), "{:?}", r.files[0]);
    assert_eq!(g.dropped_lines(), 1);
}

#[test]
fn divergent_agent_copies_both_stay_in_scope() {
    let tree = Tree::new();
    let r = resolve(&two_copies(
        &tree,
        &rec(1, "one branch"),
        &rec(1, "another branch"),
    ));

    assert_eq!(r.files.len(), 2, "{:?}", r.files);
    let g = &r.duplicates[0];
    assert_eq!(g.relation, DuplicateRelation::Divergent);
    assert!(g.dropped.is_empty());
    assert_eq!(g.members.len(), 2);
}

#[test]
fn a_directory_named_twice_contributes_its_copy_once() {
    let tree = Tree::new();
    let body = rec(1, "child ask");
    let dirs = two_copies(&tree, &body, &body);
    // One invocation can name the same directory twice (an explicit `@<encoded>` token
    // beside a real path that resolves to it). The copy under it is still ONE file, and
    // probing it twice would hand the merge one file as two copies of a session - a
    // fabricated duplicate with a fabricated dropped copy behind it.
    let named_twice = vec![dirs[0].clone(), dirs[1].clone(), dirs[1].clone()];

    let collected = found(&named_twice);
    assert_eq!(
        collected.len(),
        2,
        "one path per FILE, however often its directory is named: {collected:?}"
    );
    let r = resolve(&named_twice);
    assert_eq!(r.files.len(), 1, "{:?}", r.files);
    assert_eq!(r.duplicates[0].members.len(), 2);
    assert_eq!(r.duplicates[0].dropped.len(), 1);
}

#[test]
fn an_agent_under_one_directory_reports_no_duplicate() {
    let tree = Tree::new();
    let body = rec(1, "child ask");
    tree.write(&format!("{DIR_A}/{UUID}.jsonl"), &rec(0, "the session"));
    tree.write(
        &format!("{DIR_A}/{UUID}/subagents/agent-{HEX}.jsonl"),
        &body,
    );
    let dirs = vec![ProjectDir {
        dir: tree.path(DIR_A),
        target_cwd: None,
    }];

    let r = resolve(&dirs);
    assert_eq!(r.files.len(), 1);
    assert!(r.duplicates.is_empty(), "the ordinary case reports nothing");
}
