//! The provenance vocabulary and index, pinned directly.

use super::*;

/// The vocabulary IS [`EdgeKind::ALL`], generated beside the enum by the `edge_kinds!`
/// declaration, so a variant cannot exist outside it. The tests below walk that.
///
/// It was a hand-kept array here, and twice that was not enough. First it held all but two
/// of the set - `parent` and `result_of` absent - so the uniqueness and JSON-alone
/// tests were statements about a sample. Then an exhaustive variant-to-position `match`
/// was added to close it, and it did not: it forces an author to TOUCH this file, not to
/// put the variant into the list, and a twentieth variant given an arm, a colliding word
/// and no row in the array passed clippy and the whole suite.
/// The vocabulary is TWENTY-SEVEN relations, and that is the claim every document makes.
///
/// Asserted against the number rather than derived from the list, which would agree with
/// itself whatever the list held - and since the list is generated from the enum, this is
/// the enum's own variant count.
#[test]
fn the_vocabulary_is_twenty_seven_relations() {
    assert_eq!(
        EdgeKind::ALL.len(),
        27,
        "the closed vocabulary is twenty-seven relations"
    );
}

/// Exactly SIX relations ride JSON alone, and all six are the engine's own.
///
/// A family edge exists because a record carries a key naming a target, which is what the
/// label zone is for. The engine's six are read off the file's shape instead, and each
/// already reaches a reader through the unit it sits on - `parent` through the record
/// above, `result_of` through the pairing glyph, and the four SURVIVAL relations through
/// `[replay copy of L<n>]`, `[abandoned]` / `[rewound]` and the draft-distance line. They
/// are the documented exception, and an exception worth stating is worth pinning: a seventh
/// would quietly remove a relation from every text surface at once, and a sixth removed
/// would put a duplicate tag on every abandoned unit.
#[test]
fn only_the_engine_relations_ride_json_alone() {
    let hidden: Vec<&str> = EdgeKind::ALL
        .iter()
        .filter(|k| !k.rendered_in_label())
        .map(|k| k.as_str())
        .collect();
    assert_eq!(
        hidden,
        [
            "branches",
            "continues",
            "parent",
            "replays",
            "result_of",
            "supersedes"
        ]
    );
}

/// Every relation word is DISTINCT.
///
/// Two variants sharing a word would make the relation ambiguous wherever it is printed -
/// a reader following `attached_to` would be following two different relations - and the
/// enum is the only place that can be checked, because `as_str` is a match nobody reads
/// end to end.
#[test]
fn every_edge_kind_has_its_own_word() {
    let mut words: Vec<&str> = EdgeKind::ALL.iter().map(|k| k.as_str()).collect();
    let before = words.len();
    words.sort_unstable();
    words.dedup();
    assert_eq!(words.len(), before, "two relations share a word: {words:?}");
}

/// No word is empty and none carries a character a reader would have to quote to pass it
/// back - the words are printed in a label zone and read back as census keys.
///
/// A SPACE is refused HERE, at the vocabulary, and not only where a fixture happens to
/// render one: `--count-by for-kind` prints `<count>  <key>` per line, so a key carrying a
/// space parses as two fields for every reader that splits that surface on whitespace. The
/// census e2e can only fail for a relation some fixture emits, and a relation nothing in
/// the suite produces would ship its broken key unnoticed - which is how `hook run` lived
/// through a release cycle.
#[test]
fn every_word_is_printable_as_it_stands() {
    for k in EdgeKind::ALL {
        let w = k.as_str();
        assert!(!w.is_empty(), "{k:?} has no word");
        assert!(
            w.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "{k:?} renders {w:?}, which is not a bare relation word"
        );
    }
}

/// FIRST WRITE WINS on a duplicate id, and an id the index never saw answers `None`.
///
/// The duplicate case is not hypothetical: a compaction re-anchor appends an earlier
/// record's block a second time, so the two copies carry the same `tool_use` id. The FIRST
/// is the call as it was MADE, which is where a result should be sent; a last-write index
/// would silently re-point every result in the file at the replay copy.
#[test]
fn a_tool_use_id_resolves_to_the_first_line_that_carried_it() {
    let mut ix = ProvIndex::default();
    ix.insert_tool_use("toolu_01aa", 4);
    ix.insert_tool_use("toolu_01aa", 9);
    ix.insert_tool_use("toolu_01bb", 7);
    assert_eq!(ix.tool_use_at("toolu_01aa"), Some(4));
    assert_eq!(ix.tool_use_at("toolu_01bb"), Some(7));
    assert_eq!(ix.tool_use_at("toolu_01zz"), None);
}

/// The FOUR root answers, each with its own shape, and none of them an absent field.
///
/// The fourth is the one a real transcript meets first: a session opens with a synthetic
/// pre-user lead the turn grouper folds into turn 0, and those units have no opener above
/// them at all. It was constructed nowhere in this file while the test's own name said
/// three, so the shape that the first lines of every live transcript produce was pinned by
/// the e2e alone.
#[test]
fn the_root_has_four_answers_and_says_which_it_is() {
    let opener = RootLink::opener();
    assert_eq!(opener.line, None);
    assert_eq!(opener.hops, None);
    assert_eq!(opener.note, Some(ROOT_IS_OPENER));
    assert_eq!(
        opener.label_zone(),
        None,
        "a tag on the opener would restate the line the reader is on"
    );

    let chained = RootLink::under(12, Some("u-1".to_string()), Some(3));
    assert_eq!(chained.line, Some(12));
    assert_eq!(chained.hops, Some(3));
    assert_eq!(chained.note, None, "a reached root has nothing to explain");
    assert_eq!(chained.label_zone().as_deref(), Some("[root L12 +3]"));

    let placed = RootLink::under(12, Some("u-1".to_string()), None);
    assert_eq!(placed.line, Some(12));
    assert_eq!(placed.hops, None);
    assert_eq!(
        placed.note,
        Some(ROOT_UNCHAINED),
        "the grouping placed it; the chain did not carry it"
    );
    assert_eq!(chained.label_zone().as_deref(), Some("[root L12 +3]"));
    assert_eq!(placed.label_zone().as_deref(), Some("[root L12]"));

    let lead = RootLink::no_opener();
    assert_eq!(lead.line, None);
    assert_eq!(lead.uuid, None);
    assert_eq!(lead.hops, None);
    assert_eq!(
        lead.note,
        Some(ROOT_NO_OPENER),
        "there is nothing above this unit to name, which is a fact and not a gap"
    );
    assert_eq!(
        lead.label_zone(),
        None,
        "no line, so no tag - the note carries the whole answer"
    );

    // The four notes are four DISTINCT values: a consumer switches on this field, so two
    // answers sharing one would be one answer wearing two shapes.
    let mut notes = vec![opener.note, chained.note, placed.note, lead.note];
    let before = notes.len();
    notes.sort_unstable();
    notes.dedup();
    assert_eq!(notes.len(), before, "two answers share a note: {notes:?}");
}

/// A root ALWAYS has a line, and this is why: the only record csift holds with no physical
/// line is a merged elicitation-sidecar record, and neither shape the sidecar carries opens
/// a turn - an unresolved AskUserQuestion or ExitPlanMode rides an `assistant` record, and
/// an MCP elicitation rides a `system` one. So no turn's opener can be one, and a guard
/// collapsing a zero line would stand for a caller that does not exist.
///
/// Pinned on the SHAPES rather than on the guard, because the guard is not the fact: if
/// one of these ever did open a turn, a `None` line would hide it behind a fifth wire shape
/// no manual names, and this test would name it instead.
#[test]
fn no_sidecar_shape_opens_a_turn_so_a_root_always_has_a_line() {
    for line in [
        r#"{"type":"assistant","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"AskUserQuestion","csiftKey":"toolu_01x","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_01x","name":"AskUserQuestion","input":{"questions":[]}}]}}"#,
        r#"{"type":"system","csift":"elicitation-marker-v1","csiftPhase":"pending","csiftKind":"mcp","csiftKey":"some-server","content":"MCP elicitation pending"}"#,
    ] {
        let r: crate::model::Record = serde_json::from_str(line).expect("fixture parses");
        assert!(r.is_elicitation_marker(), "the fixture is a marker: {line}");
        assert!(
            !r.opens_turn(None),
            "a sidecar record opens no turn: {line}"
        );
    }
    let r = RootLink::under(12, Some("u-1".to_string()), Some(1));
    assert_eq!(r.line, Some(12), "the address a reader can fetch");
    assert_eq!(r.label_zone().as_deref(), Some("[root L12 +1]"));
}

/// The RELATION INVENTORY in both manuals lists every edge of the vocabulary, exactly once,
/// and lists nothing as a `for[]` edge that is not one.
///
/// The inventory is the reader's whole map of what csift can answer about a record: the
/// closed vocabulary, the relations that ride a FIELD on some other surface instead, and the
/// ones nothing on disk names. Only the first block is derivable, and that is precisely the
/// block that rots silently - a relation added here renders on every hit at once while the
/// table a reader was handed does not list it.
///
/// BOTH DIRECTIONS, which is two different failures. A MISSING row is a word csift prints
/// and neither manual explains. An EXTRA edge row is a word the manual promises and no
/// edge can ever carry, which sends a reader looking for a census key that does not exist.
///
/// The two manuals lay the inventory out differently, so each is read in its own shape.
/// SPEC 4.12 keeps ONE table where the `for[]` marker in the last cell separates the edges
/// from the non-edge rows. The skill (`skills/csift/references/provenance.md`) keeps TWO:
/// the kind table, every row of which is an edge, and the non-edge table beside it. The
/// edge set is compared against the vocabulary in both; the skill's non-edge table must
/// exist and name no vocabulary word as its relation, since a word listed there and in the
/// kind table is one relation answered two ways. What carries `clone_of` or `produced_by`
/// is a sentence a person wrote about another surface, and nothing in this crate can
/// derive it, so the non-edge rows are otherwise outside the comparison.
#[test]
fn both_manuals_list_every_relation_of_the_vocabulary_exactly_once() {
    let mut want: Vec<&str> = EdgeKind::ALL.iter().map(|k| k.as_str()).collect();
    want.sort_unstable();
    let root = env!("CARGO_MANIFEST_DIR");
    let spec = std::fs::read_to_string(format!("{root}/SPEC.md"))
        .unwrap_or_else(|e| panic!("SPEC.md at the repo root: {e}"));
    let skill = std::fs::read_to_string(format!("{root}/skills/csift/references/provenance.md"))
        .unwrap_or_else(|e| panic!("skills/csift/references/provenance.md: {e}"));
    for (doc, mut got) in [
        ("SPEC.md", inventory_edges(&spec)),
        (
            "skills/csift/references/provenance.md",
            first_cell_words(&skill, SKILL_KIND_HEAD),
        ),
    ] {
        let before = got.len();
        got.sort_unstable();
        got.dedup();
        assert_eq!(
            got.len(),
            before,
            "{doc}: the inventory lists a relation twice: {got:?}"
        );
        assert_eq!(
            got, want,
            "{doc}: the relation inventory and the vocabulary disagree"
        );
    }
    let non_edges = first_cell_words(&skill, SKILL_NON_EDGE_HEAD);
    let both: Vec<&&str> = non_edges.iter().filter(|w| want.contains(w)).collect();
    assert!(
        both.is_empty(),
        "skills/csift/references/provenance.md lists a vocabulary word as a non-edge \
         relation: {both:?}"
    );
}

/// The skill's kind table: one row per `for` word, every row an edge.
const SKILL_KIND_HEAD: &str = "| kind | from, to | basis | carried by |";

/// The skill's non-edge table: relations that are real and are not `for` edges.
const SKILL_NON_EDGE_HEAD: &str = "| relation | answered by |";

/// The rows of the table opening with `head`, as cell lists, from the head to the first line
/// that is not a table row - ONE table, never every backticked word in the file.
fn table_rows<'a>(md: &'a str, head: &str) -> Vec<Vec<&'a str>> {
    let start = md
        .find(head)
        .unwrap_or_else(|| panic!("no relation inventory table (looked for {head:?})"));
    let mut out = Vec::new();
    for line in md[start..].lines().skip(1) {
        if !line.starts_with('|') {
            if out.is_empty() {
                continue; // the `|---|` separator under the head
            }
            break;
        }
        out.push(line.trim_matches('|').split('|').map(str::trim).collect());
    }
    out
}

/// The backticked word in the first cell of every row of the table opening with `head`; a
/// row whose first cell is prose (`the turn a unit belongs to`) contributes nothing.
fn first_cell_words<'a>(md: &'a str, head: &str) -> Vec<&'a str> {
    table_rows(md, head)
        .into_iter()
        .filter_map(|cells| {
            let first = cells.first()?;
            first.strip_prefix('`').and_then(|c| c.strip_suffix('`'))
        })
        .collect()
}

/// The relation word of every SPEC inventory row whose LAST cell marks it as a `for[]` edge.
///
/// The table is found by its own first column header. A row's relation is the backticked
/// word in its first cell; `answers` / `rejects` sits in a first cell as two words and is
/// not a `for[]` row, so the last-cell marker is what keeps it out rather than the shape of
/// the first.
fn inventory_edges(md: &str) -> Vec<&str> {
    table_rows(md, "| relation | what it joins | basis | where it rides |")
        .into_iter()
        .filter_map(|cells| {
            let (first, last) = (cells.first()?, cells.last()?);
            if !last.starts_with("`for[]`") {
                return None;
            }
            first.strip_prefix('`').and_then(|c| c.strip_suffix('`'))
        })
        .collect()
}

/// EVERY `[for: <word>` tag the two record-reading manuals RENDER names a relation of the
/// vocabulary.
///
/// The help-parity sweep already asks whether every JSON key a surface emits is named in
/// the manual that documents it. It reads KEY NAMES, so it is blind to the spelling of a
/// rendered TAG - and that is exactly where the vocabulary drifted: `hook run` was respelled
/// `hook_run` in the enum, the label zone, the census key and the skill, while
/// `search --help` went on showing `[for: hook run ...]` to the one reader who cannot check
/// it against the binary. Nothing failed.
///
/// This reads the help TEXT the way a person does and holds each tag word to `EdgeKind`.
/// `<kind>` is skipped: it is the manuals' own placeholder for "any relation word", not a
/// word.
///
/// It renders the WHOLE page through clap, for the root and for every subcommand, rather
/// than reading the `after_help` blocks. Most of a rendered `--help` is the ARGUMENT docs,
/// which those blocks do not contain: a stale tag in a flag's own doc comment - where the
/// `hook run` spelling in fact survived - sits outside them entirely, and a guard that reads
/// only the epilogue passes while the reader sees the wrong word. Rendering also costs
/// nothing here and cannot drift from what ships, because it IS what ships.
#[test]
fn every_for_tag_the_help_renders_names_a_relation_of_the_vocabulary() {
    use clap::CommandFactory;
    let mut cmd = crate::cli::Cli::command();
    let words: Vec<&'static str> = EdgeKind::ALL.iter().map(|k| k.as_str()).collect();
    let mut pages: Vec<(String, String)> =
        vec![("csift".to_string(), cmd.render_long_help().to_string())];
    for sub in cmd.get_subcommands_mut() {
        pages.push((
            sub.get_name().to_string(),
            sub.render_long_help().to_string(),
        ));
    }
    let mut found: Vec<String> = Vec::new();
    for (name, text) in &pages {
        for (i, _) in text.match_indices("[for: ") {
            let rest = &text[i + "[for: ".len()..];
            // The tag's word runs to the first space or the closing bracket; a rendered page
            // is wrapped, so a word can be followed by a newline too.
            let end = rest
                .find(|c: char| c.is_whitespace() || c == ']')
                .unwrap_or(rest.len());
            let word = &rest[..end];
            if word == "<kind>" {
                continue;
            }
            assert!(
                words.contains(&word),
                "`csift {name} --help` renders [for: {word} ...], which is no relation of \
                 the vocabulary: {words:?}"
            );
            found.push(word.to_string());
        }
    }
    found.sort();
    found.dedup();
    // A bare count would pass on one tag repeated, so the floor is on DISTINCT words, and
    // `hook_run` is named outright: it is the word that drifted, the only two-part spelling
    // the vocabulary ever had, and the one a reader of the rendered page cannot check
    // against the enum.
    assert!(
        found.iter().any(|w| w == "hook_run"),
        "the rendered pages no longer show a `hook_run` tag, so the respelling this guard \
         exists for is unwatched: {found:?}"
    );
    assert!(
        found.len() >= 3,
        "the rendered pages carried {} distinct `for` words ({found:?}) - the reader this \
         guards is gone, or the extraction stopped matching",
        found.len()
    );
}

/// The `--count-by for-kind` help's KEY SPACE is the vocabulary, word for word and counted.
///
/// The help enumerates all twenty-seven relation words and says how many there are, which is
/// what makes it usable without the source - and it is a hand-typed list, so a twenty-eighth
/// variant leaves it naming twenty-seven of the twenty-eight keys the census can print, with
/// nothing failing. The drift test over the SPEC and skill inventories does not reach it:
/// those are tables in markdown and this is a sentence in a doc comment.
///
/// The count is pinned as its spelled-out WORD, because that is the form the sentence uses
/// and therefore the form that goes stale. The table covers the plausible range and fails
/// loudly outside it rather than silently skipping the check.
#[test]
fn the_for_kind_help_names_every_relation_and_counts_them() {
    use clap::CommandFactory;
    const NUMBER_WORDS: [(usize, &str); 21] = [
        (20, "twenty"),
        (21, "twenty-one"),
        (22, "twenty-two"),
        (23, "twenty-three"),
        (24, "twenty-four"),
        (25, "twenty-five"),
        (26, "twenty-six"),
        (27, "twenty-seven"),
        (28, "twenty-eight"),
        (29, "twenty-nine"),
        (30, "thirty"),
        (31, "thirty-one"),
        (32, "thirty-two"),
        (33, "thirty-three"),
        (34, "thirty-four"),
        (35, "thirty-five"),
        (36, "thirty-six"),
        (37, "thirty-seven"),
        (38, "thirty-eight"),
        (39, "thirty-nine"),
        (40, "forty"),
    ];
    const MARKER: &str = "THE KEY SPACE is the closed vocabulary";
    const END: &str = "Six of them";

    let mut cmd = crate::cli::Cli::command();
    let sub = cmd
        .get_subcommands_mut()
        .find(|s| s.get_name() == "search")
        .expect("no `search` subcommand");
    // The rendered page wraps, so the enumeration is read with its newlines flattened.
    let page = sub.render_long_help().to_string().replace('\n', " ");
    let start = page
        .find(MARKER)
        .unwrap_or_else(|| panic!("`search --help` no longer says {MARKER:?}"));
    let rest = &page[start..];
    let end = rest
        .find(END)
        .unwrap_or_else(|| panic!("`search --help` no longer says {END:?} after the key space"));
    let listing = &rest[..end];

    let n = EdgeKind::ALL.len();
    let word = NUMBER_WORDS
        .iter()
        .find(|(k, _)| *k == n)
        .map(|(_, w)| *w)
        .unwrap_or_else(|| panic!("extend NUMBER_WORDS: the vocabulary is {n} relations"));
    assert!(
        listing.contains(&format!("{word} words")),
        "the help counts the key space as something other than `{word} words`:\n{listing}"
    );

    let mut named: Vec<&str> = listing
        .split('`')
        .skip(1)
        .step_by(2)
        .filter(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
        .collect();
    named.sort_unstable();
    named.dedup();
    let mut words: Vec<&str> = EdgeKind::ALL.iter().map(|k| k.as_str()).collect();
    words.sort_unstable();
    assert_eq!(
        named, words,
        "the `--count-by for-kind` help's key space is not the vocabulary"
    );
}
