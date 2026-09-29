//! Help/doc drift guards: flags mentioned exist, examples parse, SKILL version stamp.

#[allow(unused_imports)]
use super::*;

// -- Drift guards: the help text and the repo docs must never resurrect a removed
//    surface - a help page teaching a flag that hard-errors sends its reader down a
//    dead path, and these pins make that class of drift a test failure. --

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

/// Every `csift <sub> ...` EXAMPLE line in any help page must use only flags that
/// SUB actually declares (+ the global --claude-home) - the exact drift a flag moving
/// to another subcommand leaves behind (record fetching moved to `show`, so a
/// `search --line` example is a dead one).
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
            // `$( ... )` command substitutions - check each segment independently.
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

use crate::cli::show_keys::{k, record_row_keys, Family, FAMILY_ORDER, RECORD_KEYS};

// -- The per-family HELP sections --

/// `search --help`'s body is its section files joined in the fixed order, with nothing of
/// the assembler's own between them.
///
/// The point of the split is that a family edits ONE file and cannot disturb the shared
/// body, so what has to hold is the FRAME: the intro leads, its closing notes end the
/// JSON schema block, and the six family REGIONS follow as the tail, in family order.
/// A family filling its section changes the tail and nothing else, which is why the tail
/// is compared as a join rather than pinned to a literal that would go stale the first
/// time a family wrote a sentence.
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
    // The seam the split has to get right. A line continuation strips the NEXT line's
    // leading whitespace inside ONE literal and cannot strip across two, so the eight
    // spaces a continuation would eat are dropped by hand at the split instead; getting
    // that wrong inserts them into the rendered help and nothing else complains. The ONE
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

/// The PROVENANCE edge is ONE key and one row, owned by no family.
///
/// Four families build edges and every one of them folds into the same array through
/// `attach_facts`, so four families listing the key produced four rows spelled four
/// different ways - `for:[...] (kind/basis/reason + target{...})`, `for:[{kind|basis|target|
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
    // Strip EXACTLY ONE brace at each end. `trim_matches` strips every occurrence, so a
    // row whose own rendered form ENDS in `}` - any object-valued key - lost its closing
    // brace together with the list's when it sat LAST, and its name then matched no table
    // row. That is a property of the parse and not of the table, so it is fixed here
    // rather than by ordering the table around it; `an_object_valued_key_parses_last`
    // below is the pin.
    let unwrap_list = |s: &str| -> String { s[1..s.len() - 1].to_string() };
    let body = unwrap_list(&rendered);
    // EVERY row exactly once: the family blocks partition the table, so the rendered
    // list is as long as the table and no name is repeated. A grouping that put a row in
    // more than one block, or dropped a block, changes this count and nothing else.
    let names: Vec<&str> = body.split(", ").collect();
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
    for name in body.split(", ") {
        let row = RECORD_KEYS
            .iter()
            .find(|r| r.rendered == name)
            .expect("every rendered name comes from a table row");
        let at = order(row.family);
        assert!(at >= last, "{name} printed outside its family block");
        last = at;
    }
}

/// An OBJECT-VALUED key renders and parses back wherever it sits, LAST included. The
/// rendered list is `{a, b, c}` and such a row's own form ends in `}`, so a parse that
/// trims every trailing brace swallows the row's along with the list's and the last name
/// matches no table row - which is what happened, as a panic inside the walk above, the
/// moment an object-valued key was appended at the end of a family block. The table's
/// real order is not what this pins: the list is rebuilt here with the object-valued row
/// moved last, so the parse is tested against the shape whatever the table does.
#[test]
fn an_object_valued_key_parses_last() {
    let object_valued: Vec<&str> = RECORD_KEYS
        .iter()
        .map(|r| r.rendered)
        .filter(|r| r.ends_with('}'))
        .collect();
    assert!(
        !object_valued.is_empty(),
        "the shape this pins has to exist in the table"
    );
    let mut names: Vec<&str> = RECORD_KEYS
        .iter()
        .map(|r| r.rendered)
        .filter(|r| !r.ends_with('}'))
        .collect();
    names.extend(object_valued.iter().copied());
    let rendered = format!("{{{}}}", names.join(", "));
    let parsed: Vec<&str> = rendered[1..rendered.len() - 1].split(", ").collect();
    assert_eq!(parsed, names, "one brace off each end, never every brace");
    assert_eq!(
        parsed.last(),
        object_valued.last(),
        "the last name survives whole"
    );
}

/// `whoami --help` names every SELF target the binary accepts, and states ONE reading of what
/// the environment hands a subagent, on both halves of the page.
///
/// The FLAG NOTE said `@trap:<marker>` or `@main` ONLY while a lane id and a teammate's
/// routing form both resolve (exit 0, the self and parent sections printed); and the long
/// description said a built-in subagent's env holds its OWN id while the after-help, and
/// every current build, hand it the top-level session's.
#[test]
fn the_whoami_page_names_every_self_target_and_one_env_reading() {
    use clap::CommandFactory;
    let help = Cli::command()
        .find_subcommand_mut("whoami")
        .expect("the whoami subcommand")
        .render_long_help()
        .to_string();
    let note_at = help.rfind("FLAG NOTE\n").expect("the after-help FLAG NOTE");
    let note = &help[note_at..];
    let note = &note[..note.find("\n\n").unwrap_or(note.len())];
    for form in ["@main", "@trap:<marker>", "@<agent-id>", "@<Name>@<Team>"] {
        assert!(
            note.contains(form),
            "`whoami --help`'s FLAG NOTE must name the `{form}` target: {note}"
        );
    }
    assert!(
        !note.contains("ONLY"),
        "the FLAG NOTE names every form: {note}"
    );
    let caveat_at = help
        .find("SUBAGENT CAVEAT:")
        .expect("the long description's caveat");
    let caveat = &help[caveat_at..];
    let caveat = &caveat[..caveat.find("\n\n").unwrap_or(caveat.len())];
    assert!(
        caveat.contains("TOP-LEVEL") && !caveat.contains("it is the SUBAGENT's OWN id"),
        "the long description reads the env id as the after-help does: {caveat}"
    );
}

/// `verbatim --help`'s compaction paragraph states the crossing rule the survival walk runs,
/// with the default read off the flag's own constant, and names the exchange header by the
/// token `search` prints.
///
/// It said csift's walk stops at a compaction boundary "too", which only budget 0 does, and
/// it named the header `s·t<n>`, a legend spelling the header no longer carries.
#[test]
fn the_verbatim_page_states_the_crossing_rule_and_the_header_token() {
    use clap::CommandFactory;
    let help = Cli::command()
        .find_subcommand_mut("verbatim")
        .expect("the verbatim subcommand")
        .render_long_help()
        .to_string();
    let at = help
        .find("ACROSS A COMPACTION CUT")
        .expect("the compaction paragraph");
    let para = &help[at..];
    let para = &para[..para.find("\n\n").unwrap_or(para.len())];
    let default = format!("default {}", crate::model::CROSS_COMPACTIONS_DEFAULT);
    assert!(
        para.contains(&default) && para.contains("--cross-compactions"),
        "the paragraph names the crossing flag and its default ({default}): {para}"
    );
    assert!(!para.contains("stops there too"), "{para}");
    assert!(help.contains("<tok>·t<N>"), "the exchange header token");
    assert!(!help.contains("s·t<n>"), "the retired legend spelling");
}
