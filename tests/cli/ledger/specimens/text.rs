//! Rules 10, 14 and 17: what a field may not SAY.
//!
//! See the parent module for why each rule owes a specimen.

use super::*;

#[test]
fn ledger_gate_rule_10_names_an_unconsumed_text_leg() {
    let mut c = good_claim();
    c["states"][0]["open_legs"] = json!(["TEXT: the second sentence overstates the count"]);
    let mut f = Vec::new();
    legs::text_instructions("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("an unconsumed text correction (`TEXT ...`)"));

    let mut c = good_claim();
    c["states"][0]["open_legs"] = json!(["Text rewrite rejected: the count is still wrong"]);
    let mut f = Vec::new();
    legs::text_instructions("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("records a rejected rewrite"));
}

#[test]
fn ledger_gate_rule_14_names_every_barred_prose_family() {
    // A calendar date, a month name, a csift version, and four audit-process words.
    let hits = prose::hits(
        "Measured 2026-09-04 in September with csift 0.12.3 by an independent verifier \
         whose tracer batch was kicked today",
    );
    let rules: Vec<&str> = hits.iter().map(|(r, _)| *r).collect();
    assert!(rules.contains(&"calendar date"), "{hits:?}");
    assert!(rules.contains(&"month name"), "{hits:?}");
    assert!(rules.contains(&"csift version"), "{hits:?}");
    assert_eq!(
        hits.iter()
            .filter(|(r, _)| *r == "audit-process word")
            .count(),
        4,
        "{hits:?}"
    );

    // csift's OWN spellings of `audit` are spared by SHAPE; that negative control sits
    // beside the four positives it contrasts with, in the widening test below.
    // A Claude Code version is never a csift version.
    assert!(prose::hits("present from Claude Code 2.1.208").is_empty());

    // The date arm's two BOUNDARIES. An ISO-8601 instant is a fact the ledger states - a
    // record's timestamp, a build date - and the trailing word boundary is what keeps it
    // out; a partial date is the label a month-bucketed census is keyed by. Only a bare
    // date beside a count is the day a pass ran.
    assert!(prose::hits("the newest record is stamped 2026-09-01T06:33:25Z").is_empty());
    assert!(
        prose::hits("present on 0 of 5,415 (2026-05) and 9,259 of 80,890 (2026-07)").is_empty()
    );
    let bare = prose::hits("measured 2026-09-04 over the corpus");
    assert_eq!(bare.len(), 1, "{bare:?}");
    assert_eq!(bare[0].0, "calendar date");
    // Domain vocabulary a token ban would have destroyed.
    assert!(
        prose::hits("a frozen lane of the top-level session, whose tool use was rejected")
            .is_empty()
    );

    // The whole-state walk names the field and the rule.
    let mut c = good_claim();
    c["states"][0]["depends"] = json!("csift 0.12.3 reads that key.");
    let mut f = Vec::new();
    prose::check_state("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("states[0].depends: `0.12.3` is a csift version"));

    // An excerpt reproduces the build's own bytes and is exempt at every level.
    let mut c = good_claim();
    c["states"][0]["producer_chain"] = json!([{"role": "writer", "offset": 1, "excerpt": "kicked:!1,lastSeen:\"today\"", "note": "the state object"}]);
    let mut f = Vec::new();
    prose::check_state("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(f.is_empty(), "{f:?}");

    // The PLURALS and the agent noun: a list that stops at the singular has a hole in it.
    for token in [
        "adjudications",
        "auditor",
        "auditors",
        "verifiers",
        "tracers",
        "audits",
    ] {
        let hits = prose::hits(&format!("the {token} recorded it"));
        assert_eq!(hits.len(), 1, "{token}: {hits:?}");
        assert_eq!(hits[0].0, "audit-process word");
    }

    // A BACKTICKED span is exempt only where the backticks BALANCE: an unclosed one makes
    // left-to-right pairing exempt prose, which is how a date once hid behind it.
    assert!(prose::hits("the beta `server-side-fallback-2026-06-01` is interned").is_empty());
    let unbalanced = prose::hits("a cut quote `def || after: measured 2026-09-04 and got 42");
    assert_eq!(unbalanced.len(), 1, "{unbalanced:?}");
    assert_eq!(unbalanced[0].0, "calendar date");
}

/// Rule 14's three widenings, each one a defect the older shape shipped.
#[test]
fn ledger_gate_rule_14_reaches_what_the_older_shape_let_through() {
    // (a) The old guard spared any match ADJACENT to `.`, `/` or `-`, which spared four
    // process words. csift's own spellings are spared by SHAPE instead.
    for text in [
        "re-worded from the kicked-back version",
        "so a re-verifier can locate each by string",
        "Bash-side mutations are outside the audit.",
        "the tag universe rests on signatures decoded today.",
    ] {
        let hits = prose::hits(text);
        assert!(
            hits.iter().any(|(r, _)| *r == "audit-process word"),
            "{text}: {hits:?}"
        );
    }
    assert!(prose::hits("`csift plan --audit` reads src/commands/plan/audit.rs:36-43").is_empty());

    // (b) The two phrases a graded re-read leaves behind.
    for token in ["planted fake", "adversarial", "planted"] {
        let hits = prose::hits(&format!("the {token} control"));
        assert!(
            hits.iter().any(|(r, _)| *r == "audit-process word"),
            "{token}: {hits:?}"
        );
    }

    // (c) FIRST PERSON, and the identifier it must not mistake for one. `we` is a real
    // minified identifier name in the shipped bundle, so the arms are case-sensitive and
    // whitespace-anchored: the pronoun stands alone, the identifier sits against syntax.
    for text in [
        "I re-read the three gates",
        "I could not resolve the remaining sidecars",
        "my poll detected the written file",
        "so we counted the carriers",
        "the equality is telling us the stored form",
        "our reading of the preserved set",
        "Our second pass over the same corpus",
        // The VERB IS NOT ENUMERABLE: six spellings the closed list let through, and the
        // two the review planted live, in a receipt and in a residue line.
        "I then read the product back off disk",
        "in a 240 s poll I caught one session go idle",
        "every row whose JSON I actually parsed",
        "and I declare it as such",
        "point by point I now cite a trigger",
        "I ran the census over the whole projects root.",
        "the band rests on the existing check, not on a measurement I made.",
        // The BOUNDARY admits a closing quote, which an apostrophe used to defeat.
        "ESRCH-proven, not merely 'not visible to us'.",
    ] {
        let hits = prose::hits(text);
        assert!(
            hits.iter().any(|(r, _)| *r == "first-person phrase"),
            "{text}: {hits:?}"
        );
    }
    // The other spelling of that identifier is not a pronoun at all, and the rule's
    // case-sensitivity is what tells them apart.
    assert!(!prose::hits("function wE(e,n,r,o,d){try{let f=$fr(e,n,r);")
        .iter()
        .any(|(r, _)| *r == "first-person phrase"));
    // And where the identifier IS spelled `we`, the remedy is the backticks it should
    // always have carried: a build's identifier is a literal, and a literal is exempt.
    assert!(
        !prose::hits("the update branch computes `we` = fe ? [] : CTe(diff)")
            .iter()
            .any(|(r, _)| *r == "first-person phrase")
    );
    assert!(
        prose::hits("the update branch computes we = fe ? [] : CTe(diff)")
            .iter()
            .any(|(r, _)| *r == "first-person phrase"),
        "a bare `we` is read as the pronoun, which is what forces the literal to be quoted"
    );

    // (d) The backtick exemption is BOUNDED. A 2.2 KB run between two backticks is prose
    // that happens to sit there, and one shipped a bare `today` behind the exemption.
    let long = format!("`{} today {}`", "x ".repeat(150), "y ".repeat(150));
    let hits = prose::hits(&long);
    assert!(
        hits.iter().any(|(r, _)| *r == "audit-process word"),
        "a long run is scanned as prose: {hits:?}"
    );
    assert!(prose::hits("`a short literal saying today`").is_empty());
}

/// Rule 14's two later widenings: the word the alphabet was missing, and the sentence
/// boundary the version arm could not see past.
#[test]
fn ledger_gate_rule_14_bars_the_pass_and_reads_a_version_at_a_full_stop() {
    // `round` names the pass that produced a sentence, and it led 85 carried readings.
    for text in [
        "round three of the attribution push: the producer leg traced to closure",
        "round two of the attribution push: the specimen leg re-run",
        "Unlisted by the previous round.",
        "33 transcripts, 15 rounds, 495 file observations",
        "the round-two hop at 172652965 supplied what the mac build could not",
    ] {
        let hits = prose::hits(text);
        assert!(
            hits.iter().any(|(r, _)| *r == "audit-process word"),
            "{text}: {hits:?}"
        );
    }
    // csift's OWN spelling survives by shape: the compound in either spelling, and the call.
    for text in [
        "the value round-trips through input.command with an identical md5",
        "no unknown key can round-trip",
        "a gate round trip on a phrase a hook injects",
        "Round trip: the same search returned 'matched 1 exchange'",
        "out.append((n, round(sum(naive) / max(sum(ded), 1), 2)))",
    ] {
        assert!(
            !prose::hits(text)
                .iter()
                .any(|(r, _)| *r == "audit-process word"),
            "{text}: {:?}",
            prose::hits(text)
        );
    }
    // A csift version at the END of a sentence: the trailing class refused the full stop and
    // `$` could not see past it, so `0.10.0.` shipped.
    for text in [
        "while the neighbours are 0.10.0.",
        "the reading was taken at 0.9.4. The next one moved it.",
        "at v0.12.3, and again after",
    ] {
        let hits = prose::hits(text);
        assert!(
            hits.iter().any(|(r, _)| *r == "csift version"),
            "{text}: {hits:?}"
        );
    }
    // A Claude Code version is `1.x.y` or `2.x.y`, so the two never collide - at a full stop
    // either.
    assert!(prose::hits("verified at Claude Code 2.1.258.").is_empty());
}

/// Rule 10's widening: an instruction is an instruction wherever it is written down.
/// Fourteen of them sat in `residue` and in a receipt's `note`, the two places the rule did
/// not look.
#[test]
fn ledger_gate_rule_10_reaches_residue_and_the_receipt_note() {
    let mut c = good_claim();
    c["states"][0]["residue"] = json!(["An earlier reading. TEXT: strike the second sentence."]);
    let mut f = Vec::new();
    legs::text_instructions("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("residue[0]: an unconsumed text correction"));

    let mut c = good_claim();
    c["states"][0]["verified"]["note"] = json!("TEXT (REC-006): do not cite that build.");
    let mut f = Vec::new();
    legs::text_instructions("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("verified.note: an unconsumed text correction"));

    // `TEXT` as an ordinary word is not an instruction; the colon and the optional
    // parenthesised id are what make it one.
    let mut c = good_claim();
    c["states"][0]["residue"] = json!(["The TEXT block of the record carries the body."]);
    let mut f = Vec::new();
    legs::text_instructions("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(f.is_empty(), "{f:?}");
}

/// Rule 17: a probe naming a retired selector cannot run at all - the shipped binary
/// hard-errors on it - while a residue line saying what a leaf USED to be called is
/// history, which is what a superseded reading is for.
#[test]
fn ledger_gate_rule_17_names_a_retired_selector_in_an_argument() {
    let mut c = good_claim();
    c["states"][0]["probe"] = json!({
        "kind": "csift",
        "command": "csift search '' -t harness.meta.hook --count-by label",
        "scope": "one projects root",
        "rule": "one per record"
    });
    let mut f = Vec::new();
    legs::retired_spellings("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(only(&f).contains("names the retired selector `harness.meta.hook`"));

    // Every ARGUMENT position the rule reads, one specimen each: an arm with none is an
    // arm nobody has seen fire.
    for argv in [
        "csift search '' -T harness.meta.hook",
        "csift search '' --label harness.meta.hook",
        "csift search '' --label-not harness.meta.hook",
        "csift search '' --count-by harness.meta.hook",
        "jq 'select(.label == \"harness.meta.hook\")'",
    ] {
        let mut c = good_claim();
        c["states"][0]["probe"] = json!({
            "kind": "csift",
            "command": argv,
            "scope": "one projects root",
            "rule": "one per record"
        });
        let mut f = Vec::new();
        legs::retired_spellings("TEST-001 states[0]", &c["states"][0], &mut f);
        assert!(
            f.iter().any(|m| m.contains("names the retired selector")),
            "{argv}: {f:?}"
        );
    }

    // `tool` is the one spelling that is also something else, and the ARGUMENT decides: the
    // pre-v0.5 flat path hard-errors, the census axis of the same name always ran.
    for (argv, refused) in [
        ("csift search '' -t tool", true),
        ("csift search '' --count-by tool", false),
    ] {
        let mut c = good_claim();
        c["states"][0]["probe"] = json!({
            "kind": "csift", "command": argv,
            "scope": "one projects root", "rule": "one per record"
        });
        let mut f = Vec::new();
        legs::retired_spellings("TEST-001 states[0]", &c["states"][0], &mut f);
        assert_eq!(
            f.iter()
                .any(|m| m.contains("names the retired selector `tool`")),
            refused,
            "{argv}: {f:?}"
        );
    }

    // History, not an instruction.
    let mut c = good_claim();
    c["states"][0]["residue"] =
        json!(["The leaf was called harness.meta.loop before the family split."]);
    let mut f = Vec::new();
    legs::retired_spellings("TEST-001 states[0]", &c["states"][0], &mut f);
    assert!(f.is_empty(), "{f:?}");
}
