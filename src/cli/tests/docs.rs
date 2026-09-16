//! Help/doc drift guards: flags mentioned exist, examples parse, SKILL version stamp.

#[allow(unused_imports)]
use super::*;

// ── Drift guards: the help text and the repo docs must never resurrect a removed
//    surface (the v0.2→v0.3 cleanup found `--help` teaching flags that hard-error;
//    these pins make that class of drift a compile-time-adjacent failure). ──

/// Every `--long-flag` token any rendered help page mentions must be a DECLARED long
/// flag of some command (clap introspection - zero-drift), or sit on the tiny
/// allowlist of spellings the help deliberately names as REMOVED.
#[test]
fn help_mentions_only_declared_flags() {
    use clap::CommandFactory;
    // clap built-ins + documented-as-removed spellings the help may NAME.
    let allow: &[&str] = &["help", "version", "full"];
    let mut cmd = Cli::command();
    let mut declared: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut helps: Vec<(String, String)> = Vec::new();
    helps.push(("csift".into(), cmd.render_long_help().to_string()));
    for a in Cli::command().get_arguments() {
        if let Some(longs) = a.get_long_and_visible_aliases() {
            declared.extend(longs.into_iter().map(str::to_string));
        }
    }
    for sub in cmd.get_subcommands_mut() {
        let name = sub.get_name().to_string();
        for a in sub.get_arguments() {
            if let Some(longs) = a.get_long_and_visible_aliases() {
                declared.extend(longs.into_iter().map(str::to_string));
            }
        }
        helps.push((name, sub.render_long_help().to_string()));
    }
    let flag_re = regex::Regex::new(r"--([a-z][a-z0-9-]*)").unwrap();
    for (page, help) in &helps {
        for cap in flag_re.captures_iter(help) {
            let flag = cap[1].to_string();
            assert!(
                declared.contains(&flag) || allow.contains(&flag.as_str()),
                "`csift {page} --help` mentions `--{flag}`, which no command declares — \
                     a resurrected/renamed flag in help text"
            );
        }
    }
}

/// Every `csift <sub> …` EXAMPLE line in any help page must use only flags that
/// SUB actually declares (+ the global --claude-home) - the exact drift that left
/// dead `search --line` examples in the v0.1 help.
#[test]
fn help_examples_use_the_named_subcommands_own_flags() {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    let mut flags_by_sub: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        std::collections::BTreeMap::new();
    let mut helps: Vec<String> = vec![cmd.render_long_help().to_string()];
    let global: std::collections::BTreeSet<String> = Cli::command()
        .get_arguments()
        .filter_map(|a| a.get_long().map(str::to_string))
        .collect();
    for sub in cmd.get_subcommands_mut() {
        let mut set: std::collections::BTreeSet<String> = sub
            .get_arguments()
            .flat_map(|a| {
                a.get_long_and_visible_aliases()
                    .unwrap_or_default()
                    .into_iter()
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
        set.extend(global.iter().cloned());
        flags_by_sub.insert(sub.get_name().to_string(), set);
        helps.push(sub.render_long_help().to_string());
    }
    for help in &helps {
        for line in help.lines() {
            // A line may chain several invocations through pipes or nest them in
            // `$( … )` command substitutions - check each segment independently.
            for seg in line.split('|').flat_map(|s| s.split("$(")) {
                let Some(rest) = seg.trim_start().strip_prefix("csift ") else {
                    continue;
                };
                let mut parts = rest.split_whitespace();
                let Some(sub) = parts.next() else { continue };
                let Some(flags) = flags_by_sub.get(sub) else {
                    continue; // `csift --help` etc - not a subcommand example
                };
                for tok in parts {
                    if let Some(f) = tok.strip_prefix("--") {
                        let f: String = f
                            .chars()
                            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                            .collect();
                        if f.is_empty() {
                            continue;
                        }
                        assert!(
                            flags.contains(&f),
                            "help example `csift {sub} …` uses `--{f}`, which `{sub}` \
                                 does not declare: {line}"
                        );
                    }
                }
            }
        }
    }
}

/// The hand-written SUBCOMMANDS block in the root `long_about` is a SECOND list of the
/// commands, kept because it explains each one in a sentence clap's own list has no room for.
/// A second list drifts, so it is pinned against the enum: every visible subcommand appears
/// exactly once as a block entry, in the same order clap prints them, and no entry names a
/// command that does not exist. The count word in the module doc is checked against the same
/// enum, because a spelled-out number is the one part of a doc nothing else contradicts.
#[test]
fn the_subcommands_block_matches_the_command_enum() {
    use clap::CommandFactory;
    let cmd = Cli::command();
    let visible: Vec<String> = cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set())
        .map(|s| s.get_name().to_string())
        .collect();
    let long = cmd
        .get_long_about()
        .expect("the root carries a long_about")
        .to_string();
    let block: Vec<&str> = long
        .lines()
        .skip_while(|l| l.trim() != "SUBCOMMANDS")
        .skip(1)
        .take_while(|l| !l.trim().is_empty())
        .collect();
    assert!(!block.is_empty(), "the SUBCOMMANDS block is missing");

    // An ENTRY line opens a command's description: two spaces, the name, then more spaces.
    // A continuation line is indented past the description column and names nothing.
    let entries: Vec<String> = block
        .iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("  ")?;
            let name = rest.split_whitespace().next()?;
            (!rest.starts_with(' ') && visible.iter().any(|v| v == name)).then(|| name.to_string())
        })
        .collect();
    assert_eq!(
        entries, visible,
        "the SUBCOMMANDS block must list every command exactly once, in clap's own order"
    );
    // No entry may name something clap does not offer: an entry whose head is not a command
    // would have been dropped by the filter above, so count the entry-shaped lines instead.
    let entry_shaped = block
        .iter()
        .filter(|line| {
            line.strip_prefix("  ")
                .is_some_and(|rest| !rest.starts_with(' '))
        })
        .count();
    assert_eq!(
        entry_shaped,
        visible.len(),
        "an entry-shaped line names no command: {block:?}"
    );

    // The spelled-out count in the module doc, which is where the number lives.
    let doc = include_str!("../../cli.rs");
    let words = [
        "",
        "One",
        "Two",
        "Three",
        "Four",
        "Five",
        "Six",
        "Seven",
        "Eight",
        "Nine",
        "Ten",
        "Eleven",
        "Twelve",
        "Thirteen",
        "Fourteen",
        "Fifteen",
        "Sixteen",
        "Seventeen",
        "Eighteen",
        "Nineteen",
        "Twenty",
    ];
    let want = format!(
        "{} subcommands",
        words
            .get(visible.len())
            .copied()
            .expect("extend the number words")
    );
    assert!(
        doc.contains(&want),
        "src/cli.rs must open with `{want}` - the enum has {} visible commands",
        visible.len()
    );
}

/// SKILL.md's surface stamp must match the crate version - forces the LLM-facing
/// skill to be (at least) OPENED on every release, so the "re-read this file on an
/// unexpected error" recovery path always lands on current truth.
#[test]
fn skill_stamp_matches_crate_version() {
    let skill = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/SKILL.md"))
        .expect("SKILL.md at the repo root");
    let want = format!("v{}", env!("CARGO_PKG_VERSION"));
    assert!(
        skill.contains(&want),
        "SKILL.md must carry the surface stamp `{want}` (found none) — update the \
             stamp (and the surface docs) with every version bump"
    );
}

/// The four repo docs must never resurrect a token this cleanup removed. SPEC's
/// `>`-quoted change LEDGER lines are exempt (they record the renames themselves).
#[test]
fn docs_do_not_resurrect_removed_tokens() {
    let deny: &[&str] = &[
        "--include-subagents",
        "agents --kind",
        "--by-file",
        "--line A-B",
        "category=",
    ];
    for doc in ["SKILL.md", "AGENTS.md", "SPEC.md", "README.md"] {
        let path = format!("{}/{doc}", env!("CARGO_MANIFEST_DIR"));
        let body = std::fs::read_to_string(&path).expect("repo doc");
        for (i, line) in body.lines().enumerate() {
            if line.starts_with('>') {
                continue; // SPEC change-ledger lines legitimately name old spellings
            }
            for tok in deny {
                assert!(
                    !line.contains(tok),
                    "{doc}:{} resurrects the removed token `{tok}`: {line}",
                    i + 1
                );
            }
        }
    }
}

use crate::cli::show_keys::{k, record_row_keys, Family, FAMILY_ORDER, RECORD_KEYS};

// ── The per-family HELP sections ──

/// `search --help`'s body is its section files joined in the fixed order, with nothing of
/// the assembler's own between them.
///
/// The point of the split is that a family lane edits ONE file and cannot disturb the
/// shared body, so what has to hold is the FRAME: the intro leads, its closing notes end
/// the JSON schema block, and the six family REGIONS follow as the tail, in family order.
/// A family filling its section changes the tail and nothing else, which is why the tail
/// is compared as a join rather than pinned to a literal that would go stale the first
/// time a lane wrote a sentence.
#[test]
fn the_search_help_body_is_its_sections_joined_in_order() {
    let body = SEARCH_AFTER_HELP.as_str();
    assert_eq!(body, crate::cli::search_help::sections().concat());
    let intro = crate::cli::search_help::SHARED[0];
    let outro = crate::cli::search_help::SHARED[1];
    let shared = format!("{intro}{outro}");
    assert!(
        body.starts_with(&shared),
        "the shared body no longer leads, or its closing notes no longer end it"
    );
    assert_eq!(
        &body[shared.len()..],
        crate::cli::search_help::FAMILY_SECTIONS.concat(),
        "the family regions are the tail, in family order, and nothing else"
    );
    // The seam the split had to get right. A line continuation strips the NEXT line's
    // leading whitespace inside ONE literal and cannot strip across two, so the eight
    // spaces the continuation used to eat are dropped at the split instead; getting that
    // wrong inserts them into the rendered help and nothing else complains. The ONE
    // separating space belongs to the OUTRO rather than to the intro's tail: a space left
    // at the intro's end would trail the JSON paragraph on every run.
    assert!(
        !intro.ends_with(' '),
        "the seam left a trailing space on the shared body"
    );
    assert!(outro.starts_with(' '), "the seam lost the separating space");
    assert!(!outro.starts_with("  "), "the seam gained a literal indent");

    // The REGION shape every family writes, and the reason it can be checked rather than
    // reviewed: a head at column 0 reads as a page section, a head at indent 2 reads as a
    // subsection of whatever block came before it - which is exactly what the first
    // filled section did under the JSON schema head.
    for (i, section) in crate::cli::search_help::FAMILY_SECTIONS.iter().enumerate() {
        if section.is_empty() {
            continue;
        }
        assert!(
            section.starts_with("\n\n"),
            "family {i}: a region opens with its own blank-line separator"
        );
        let head = section[2..].lines().next().unwrap_or_default();
        assert!(
            !head.is_empty() && !head.starts_with(' '),
            "family {i}: the head line sits at column 0, not indented: {head:?}"
        );
        assert!(
            section[2 + head.len()..].starts_with("\n  ")
                && !section[2 + head.len()..].starts_with("\n   "),
            "family {i}: the body is indented two, like every other section's"
        );
        assert!(
            !section.ends_with(' ') && !section.ends_with('\n'),
            "family {i}: a region closes with its last word - the NEXT one separates itself"
        );
    }
    // And an EMPTY family renders NOTHING: no head, no blank line. The count of column-0
    // lines in the tail is exactly the number of families that wrote something, so a
    // section shipping empty can never leave a bare heading on the page.
    let filled = crate::cli::search_help::FAMILY_SECTIONS
        .iter()
        .filter(|s| !s.is_empty())
        .count();
    let tail_heads = body[shared.len()..]
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with(' '))
        .count();
    assert_eq!(
        tail_heads, filled,
        "one column-0 head per FILLED family region, and none for an empty one"
    );
}

/// `search --help`'s leaf list IS `Class::ALL` - every leaf named once, and nothing named
/// that is not one.
///
/// The list is the LLM's own index of the selector space, and it is hand-written prose in
/// a brace-collapsed shorthand (`.communication.{inbox,sent,signal}`), so nothing derived
/// it and nothing checked it. A leaf missing from it is a selector a reader will not find;
/// a leaf named there and absent from the enum is a selector that hard-errors when they
/// try it.
///
/// The parse is the shorthand's own rule: the region runs from the `user` role line to
/// the paragraph after it, each ROLE opens a block, and inside a block every `.`-led
/// fragment is a path under that role, with a brace group standing for one fragment per
/// member.
#[test]
fn the_search_help_leaf_list_is_the_class_set() {
    use crate::model::Class;

    let body = SEARCH_AFTER_HELP.as_str();
    let start = body
        .find("  user        .message")
        .expect("the leaf list opens with the user role");
    let end = body[start..]
        .find("A RENDERED DRAFT")
        .expect("the leaf list is followed by the draft paragraph")
        + start;
    // The region wraps mid-fragment, so it is read as one run of words.
    let region: String = body[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    // THREE blocks, opened by the role word standing alone before its first fragment.
    // `agent` and `harness` occur in the prose too, so the opener is the whole `<role> .`
    // - a role word followed by a space and the dot of its first fragment.
    let agent_at = region.find("agent .").expect("the agent block opens");
    let harness_at = region.find("harness .").expect("the harness block opens");
    let blocks = [
        ("user", &region[..agent_at]),
        ("agent", &region[agent_at..harness_at]),
        ("harness", &region[harness_at..]),
    ];

    let mut found: Vec<String> = Vec::new();
    for (role, block) in blocks {
        // A FRAGMENT opens at a dot that follows a space - an inner dot continues one -
        // and runs to the first character that is neither a path character nor part of a
        // brace group.
        let bytes = block.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'.' || (i > 0 && bytes[i - 1] != b' ') {
                i += 1;
                continue;
            }
            let mut j = i + 1;
            let mut brace = false;
            while j < bytes.len() {
                match bytes[j] {
                    b'{' => brace = true,
                    b'}' => {
                        j += 1;
                        break;
                    }
                    _ if brace => {}
                    b'a'..=b'z' | b'.' | b'-' => {}
                    _ => break,
                }
                j += 1;
            }
            let frag = block[i..j].trim_end_matches('.');
            i = j;
            if let Some((head, members)) = frag.split_once(".{") {
                for m in members.trim_end_matches('}').split(',') {
                    found.push(format!("{role}{head}.{}", m.trim()));
                }
            } else if frag.contains('.') && frag.len() > 1 {
                found.push(format!("{role}{frag}"));
            }
        }
    }

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

/// The PROVENANCE edge is ONE key and one row, owned by no family.
///
/// Four families build edges and every one of them folds into the same array through
/// `attach_facts`, so four families listing the key produced four rows spelled four
/// different ways - `for:[…] (kind/basis/reason + target{…})`, `for:[{kind|basis|target|
/// reason}] (what an injection was written for)`, and two more - which a consumer reading
/// the help reads as four keys. The row is Shared, and this is what keeps it there: a
/// family that starts carrying edges adds no row.
#[test]
fn the_provenance_edge_is_one_shared_key_not_one_per_family() {
    let rows: Vec<&crate::cli::show_keys::RecordKey> = RECORD_KEYS
        .iter()
        .filter(|r| r.rendered.starts_with("for:"))
        .collect();
    assert_eq!(
        rows.len(),
        1,
        "the edge is one array whatever family filled it: {:?}",
        rows.iter().map(|r| r.rendered).collect::<Vec<_>>()
    );
    assert_eq!(rows[0].family, Family::Shared, "no family owns the edge");
    assert_eq!(
        rows[0].rendered, "for:[{kind,basis,target:{line,uuid,session,note},reason}]",
        "the one spelling names every field the target carries"
    );
    // And the `search` hit schema names it too. A key on the wire that the help's
    // enumeration does not list is a key a consumer cannot find, and the two surfaces
    // carry the same array.
    assert!(
        SEARCH_AFTER_HELP.contains("for:[{kind,basis,target:{line,uuid,session,note},reason}]"),
        "the search JSON schema does not list the provenance edge"
    );
}

/// `show --help`'s record-row key list is BUILT from the key table, not typed into the
/// sentence - so a family adds a key to that row by adding a table row.
#[test]
fn the_show_record_keys_render_from_their_table() {
    // The row constructor keeps what it is given, in the column order the table reads.
    let row = k(Family::Keys, "probe_key");
    assert_eq!((row.family, row.rendered), (Family::Keys, "probe_key"));

    let rendered = record_row_keys();
    assert!(rendered.starts_with('{') && rendered.ends_with('}'));
    // EVERY row exactly once: the family blocks partition the table, so the rendered
    // list is as long as the table and no name is repeated. A grouping that put a row in
    // more than one block, or dropped a block, changes this count and nothing else.
    let names: Vec<&str> = rendered.trim_matches(['{', '}']).split(", ").collect();
    assert_eq!(names.len(), RECORD_KEYS.len());
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), names.len(), "a key is printed twice");
    for key in RECORD_KEYS {
        assert!(
            rendered.contains(key.rendered),
            "{} is in the table and not in the rendered list",
            key.rendered
        );
    }
    // The help carries exactly what the table renders, which is the whole contract: a
    // consumer reads that list to know which keys exist.
    assert!(
        SHOW_AFTER_HELP.contains(&rendered),
        "the help and the table disagree about the record row"
    );
    // Every row prints inside its own family's BLOCK: walking the rendered list, the
    // family order index never goes backwards. This is the guard that keeps one lane's
    // rows from interleaving with another's, now that a family block is filled.
    let order = |f: Family| {
        FAMILY_ORDER
            .iter()
            .position(|g| *g == f)
            .expect("every family is in the render order")
    };
    let mut last = 0usize;
    for name in rendered.trim_matches(['{', '}']).split(", ") {
        let row = RECORD_KEYS
            .iter()
            .find(|r| r.rendered == name)
            .expect("every rendered name comes from a table row");
        let at = order(row.family);
        assert!(at >= last, "{name} printed outside its family block");
        last = at;
    }
}
