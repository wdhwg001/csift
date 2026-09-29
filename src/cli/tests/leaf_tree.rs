//! The `search --help` leaf tree and the two sections beside it that name what a scan
//! reaches: the tree's GATE words, the `-t` flag's list of the leaves a bare face skips,
//! and the EXPOSED KEYS section's list of JSON keys.
//!
//! Each is read against the code that decides it - the inventory-derived leaf sets, the
//! visibility table, the keys family's own projector - so the page cannot keep a word
//! the binary stopped meaning.

#[allow(unused_imports)]
use super::*;
use crate::model::{rows, Class, Gate};

/// The tree, read as one run of words: from the `user` role line to the paragraph after
/// the tree, whitespace collapsed because the region wraps mid-fragment.
fn tree_region() -> String {
    let body = SEARCH_AFTER_HELP.as_str();
    let start = body
        .find("  user        .message")
        .expect("the leaf list opens with the user role");
    let end = body[start..]
        .find("A RENDERED DRAFT")
        .expect("the leaf list is followed by the draft paragraph")
        + start;
    body[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every leaf path the tree names, each with the GATE word that follows its fragment
/// (`None` when no `[..]` follows it).
///
/// The parse is the shorthand's own rule: each ROLE opens a block, and inside a block
/// every `.`-led fragment after a space is a path under that role, with a brace group
/// standing for one fragment per member. `agent` and `harness` occur in the prose too, so
/// a block opens at the whole `<role> .` - the role word and the dot of its first
/// fragment.
fn tree_leaves() -> Vec<(String, Option<String>)> {
    let region = tree_region();
    let agent_at = region.find("agent .").expect("the agent block opens");
    let harness_at = region.find("harness .").expect("the harness block opens");
    let blocks = [
        ("user", &region[..agent_at]),
        ("agent", &region[agent_at..harness_at]),
        ("harness", &region[harness_at..]),
    ];
    let mut found = Vec::new();
    for (role, block) in blocks {
        let bytes = block.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'.' || (i > 0 && bytes[i - 1] != b' ') {
                i += 1;
                continue;
            }
            let j = fragment_end(bytes, i);
            let frag = block[i..j].trim_end_matches('.');
            let word = block[j..]
                .strip_prefix(" [")
                .and_then(|rest| rest.split_once(']'))
                .map(|(w, _)| w.to_string());
            i = j;
            if let Some((head, members)) = frag.split_once(".{") {
                for m in members.trim_end_matches('}').split(',') {
                    found.push((format!("{role}{head}.{}", m.trim()), word.clone()));
                }
            } else if frag.contains('.') && frag.len() > 1 {
                found.push((format!("{role}{frag}"), word));
            }
        }
    }
    found
}

/// Where a fragment opened at `i` ends: the first character that is neither a path
/// character nor inside a brace group.
fn fragment_end(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 1;
    let mut brace = false;
    while j < bytes.len() {
        match bytes[j] {
            b'{' => brace = true,
            b'}' => return j + 1,
            _ if brace => {}
            b'a'..=b'z' | b'.' | b'-' => {}
            _ => break,
        }
        j += 1;
    }
    j
}

/// The `search --help` leaf list names exactly the `Class::ALL` set: a leaf the enum
/// carries and the list omits is a label a reader cannot learn from the page, and a leaf
/// named there and absent from the enum is a selector that hard-errors when they try it.
#[test]
fn the_search_help_leaf_list_is_the_class_set() {
    let mut found: Vec<String> = tree_leaves().into_iter().map(|(p, _)| p).collect();
    let mut want: Vec<String> = Class::ALL.iter().map(|c| c.path().to_string()).collect();
    // The list names a leaf once, so a duplicate is a drift of its own; sorting both sides
    // makes the assertion a SET comparison with the duplicate still visible.
    found.sort();
    want.sort();
    assert_eq!(
        found, want,
        "`search --help`'s leaf list has drifted from Class::ALL"
    );
}

/// The GATE word a leaf's lines are admitted under, derived from the sets the candidate
/// gate itself keys on rather than listed here.
///
/// `[-t/att]` is every leaf the COARSE attachment keep carries, which `--attachments`
/// admits; `[-t]` the other gated leaves, which only a selector reaching them admits;
/// `[scan/needle]` a leaf a flagless scan reaches only through its own needle - the
/// shapes whose inventory rows all carry `Gate::DefaultNeedle`, plus the csift-channel
/// delivery, whose needle is its envelope header and so rides no payload row of its own;
/// `[scan]` everything a flagless scan parses on its ordinary lines.
fn expected_word(c: Class) -> &'static str {
    let needle_only = {
        let gates: Vec<Gate> = rows()
            .filter(|r| r.leaf == Some(c))
            .map(|r| r.gate)
            .collect();
        !gates.is_empty() && gates.iter().all(|g| *g == Gate::DefaultNeedle)
    };
    if attachment_leaves().contains(&c) {
        "-t/att"
    } else if gated_leaves().contains(&c) || system_line_leaves().contains(&c) {
        "-t"
    } else if needle_only || c == Class::CommChannel {
        "scan/needle"
    } else {
        "scan"
    }
}

/// Every leaf in the tree wears the GATE word the scan actually admits it under.
///
/// Three words had drifted: the structured answer read `[-t/att]` while a flagless
/// census counted it, the channel delivery read `[scan]` beside a sentence calling it one
/// of the three `[scan/needle]` leaves, and `harness.hook.context` read `[-t]` while
/// `--attachments` admits it. The positive controls are one leaf per word, so a sweep
/// that derives an empty set for any of them fails here rather than passing on nothing.
#[test]
fn every_leaf_in_the_tree_wears_the_gate_word_the_scan_gives_it() {
    let leaves = tree_leaves();
    for (want_path, want_word) in [
        ("agent.message.structured", "scan/needle"),
        ("agent.communication.channel", "scan/needle"),
        ("harness.hook.context", "-t/att"),
        ("harness.bookkeeping.cost", "-t"),
        ("user.message", "scan"),
    ] {
        let c = Class::ALL
            .iter()
            .copied()
            .find(|c| c.path() == want_path)
            .expect("a control leaf");
        assert_eq!(expected_word(c), want_word, "positive control {want_path}");
    }
    for (path, word) in leaves {
        let c = Class::ALL
            .iter()
            .copied()
            .find(|c| c.path() == path)
            .expect("a tree leaf is a class (the leaf-list test pins the set)");
        assert_eq!(
            word.as_deref(),
            Some(expected_word(c)),
            "`search --help`'s leaf tree tags `{path}` with the wrong GATE word"
        );
    }
}

/// The `-t` flag's help names every leaf a bare role or category skips on the first
/// axis, by its path or by a category that holds only such leaves.
///
/// It once named three and called the rest "the six gated leaves", while 28 leaves are
/// outside a bare face. A leaf is covered when the flag's text carries its own path, or
/// its category path when EVERY leaf of that category is invisible (`harness.bookkeeping`),
/// or its category plus a brace group naming the leaf.
#[test]
fn the_label_flag_help_names_every_leaf_a_bare_face_skips() {
    use clap::CommandFactory;
    let cmd = Cli::command();
    let search = cmd
        .get_subcommands()
        .find(|s| s.get_name() == "search")
        .expect("the search subcommand");
    let label = search
        .get_arguments()
        .find(|a| a.get_long() == Some("label"))
        .expect("the -t/--label flag");
    let text: String = label
        .get_long_help()
        .or_else(|| label.get_help())
        .expect("the -t flag has help")
        .to_string()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let invisible: Vec<Class> = Class::ALL
        .iter()
        .copied()
        .filter(|c| !c.llm_visible())
        .collect();
    assert_eq!(invisible.len(), 28, "the visibility table's count");
    for c in invisible {
        let path = c.path();
        let (cat, leaf) = path.rsplit_once('.').expect("a leaf path");
        let whole_category_invisible = Class::ALL
            .iter()
            .filter(|o| o.path().rsplit_once('.').map(|(k, _)| k) == Some(cat))
            .all(|o| !o.llm_visible());
        let braced = text.find(&format!("`{cat}.{{")).is_some_and(|at| {
            text[at..]
                .split_once('}')
                .is_some_and(|(group, _)| group.contains(leaf))
        });
        let named = text.contains(&format!("`{path}`"))
            || (whole_category_invisible && text.contains(&format!("`{cat}`")))
            || braced
            || text.contains(&format!("`{cat}` kind but"))
            || text.contains(&format!("`{cat}` closers"));
        assert!(
            named,
            "`search --help`'s -t flag must name `{path}` among the leaves a bare face skips"
        );
    }
}

/// The EXPOSED KEYS section names every JSON key the keys family projects, read off the
/// family's own projector source so a key added there without a sentence here fails.
#[test]
fn the_exposed_keys_section_names_every_key_the_family_projects() {
    let src = include_str!("../../commands/search/record_text/keys.rs");
    let json_at = src
        .find("pub(crate) fn json(&self)")
        .expect("the keys family's projector");
    let body = &src[json_at..];
    let body = &body[..body.find("\n    }\n}").expect("the projector ends")];
    // A key is the first member of a `(name, value)` pair, whether the value follows on
    // the same line or opens an object on the next.
    let pair = regex::Regex::new(r#"\(\s*"([a-z_]+)","#).expect("a valid pattern");
    let keys: Vec<&str> = pair
        .captures_iter(body)
        .filter_map(|c| c.get(1).map(|m| m.as_str()))
        .collect();
    assert!(
        keys.contains(&"stop_details") && keys.contains(&"pdf_pages"),
        "positive control: the projector parse finds the object-valued keys, got {keys:?}"
    );
    let help = SEARCH_AFTER_HELP.as_str();
    let section = &help[help
        .find("EXPOSED KEYS")
        .expect("the section heads the keys family")..];
    for k in keys {
        assert!(
            section.contains(&format!("`{k}`")),
            "`search --help`'s EXPOSED KEYS section must name `{k}`"
        );
    }
}
