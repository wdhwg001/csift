//! Rule 12: the byte anchors, over a synthetic build.
//!
//! See the parent module for why each rule owes a specimen.

use super::*;

#[test]
fn ledger_gate_rule_12_anchor_classifier_decides_every_verdict() {
    let build: &[u8] = b"alpha bravo charlie ...rest delta echo";
    // Exact: the bytes at the offset are the excerpt.
    assert_eq!(
        anchors::classify(build, 6, "bravo", None),
        anchors::Anchor::Exact
    );
    // Byte-exactness outranks eliding: a `...` that really is in the build is exact.
    assert_eq!(
        anchors::classify(build, 20, "...rest", None),
        anchors::Anchor::Exact
    );
    // Elided: an ellipsis in the excerpt, in either spelling.
    assert_eq!(
        anchors::classify(build, 6, "bra...vo", None),
        anchors::Anchor::Elided
    );
    assert_eq!(
        anchors::classify(build, 6, "bra\u{2026}vo", None),
        anchors::Anchor::Elided
    );
    // Drifted: the head agrees for four bytes, then the build says something else.
    assert_eq!(
        anchors::classify(build, 6, "bravado", None),
        anchors::Anchor::Drifted { at: 4 }
    );
    // Drifted with the excerpt running off the end of the build.
    assert_eq!(
        anchors::classify(build, 34, "echoes", None),
        anchors::Anchor::Drifted { at: 4 }
    );
    // The two marks are counted and skipped.
    assert_eq!(
        anchors::classify(build, 6, "zulu", Some("absent")),
        anchors::Anchor::Absent
    );
    assert_eq!(
        anchors::classify(build, 6, "bravado", Some("prefix-only")),
        anchors::Anchor::PrefixOnly
    );
    // A mark never hides a hop that is byte-exact again.
    assert_eq!(
        anchors::classify(build, 6, "bravo", Some("absent")),
        anchors::Anchor::Exact
    );
    // An EMPTY excerpt matches at every offset, so it is refused before the byte test -
    // including at offset 0, the placeholder for "no byte location".
    assert_eq!(
        anchors::classify(build, 6, "", None),
        anchors::Anchor::Empty
    );
    assert_eq!(
        anchors::classify(build, 0, "", Some("absent")),
        anchors::Anchor::Empty
    );
}

/// A mark is a CLAIM about the build, so the rule tests what it asserts. Without this, an
/// anchor that rotted could be quieted by typing `absent` beside it.

#[test]
fn ledger_gate_rule_12_a_mark_is_a_claim_not_a_pass() {
    let build: &[u8] = b"alpha bravo charlie ...rest delta echo";
    // The honest marks: the excerpt is nowhere in the build.
    assert_eq!(anchors::verify_mark(build, 6, "zulu", "absent"), None);
    assert_eq!(
        anchors::verify_mark(build, 6, "bravado", "prefix-only"),
        None
    );
    // Marking a hop whose excerpt IS in the build is refused, whatever the offset says.
    assert_eq!(
        anchors::verify_mark(build, 999, "bravo", "absent"),
        Some(
            "marked `absent` but the excerpt occurs in the build at offset 6 - \
             re-anchor the offset instead of marking the hop"
                .to_string()
        )
    );
    assert!(anchors::verify_mark(build, 999, "bravo", "prefix-only").is_some());
    // `prefix-only` also asserts a head match where the offset points.
    assert_eq!(
        anchors::verify_mark(build, 12, "bravado", "prefix-only"),
        Some(
            "marked `prefix-only` but no leading byte of the excerpt matches at offset 12 - \
             that mark asserts a head match, so the hop is `absent`"
                .to_string()
        )
    );
    // `absent` makes no claim about the offset, so a head mismatch there is fine.
    assert_eq!(anchors::verify_mark(build, 12, "zulu", "absent"), None);
    // An empty excerpt asserts nothing and can never be marked.
    assert!(anchors::verify_mark(build, 6, "", "absent").is_some());
}

/// Rule 12's open/closed split: a closed state's anchors are counted and never compared,
/// so a superseded fact is not asked to still sit at its old bytes.

#[test]
fn ledger_gate_rule_12_counts_closed_state_anchors_without_reading_them() {
    let mut c = good_claim();
    let mut closed = c["states"][0].clone();
    closed["from"] = json!("2.1.100");
    closed["to"] = json!("2.1.200");
    closed["producer_chain"] = json!([{"role": "writer", "offset": 99, "excerpt": "gone"}]);
    c["states"][0]["from"] = json!("2.1.201");
    // The OPEN state keeps one located hop, so the split is read off two states that each
    // carry one rather than off an open state with none. It is MARKED, because this specimen
    // is about the open/closed split and not about what the bytes say - and a mark is the one
    // verdict that decides itself without asking where the offset points.
    c["states"][0]["producer_chain"] = json!([{
        "role": "writer", "offset": 1,
        "excerpt": "zzq-no-build-carries-these-bytes-zzq",
        "anchor_status": "absent"
    }]);
    c["states"] = json!([closed, c["states"][0].clone()]);
    let mut f = Vec::new();
    let a = anchors::tally(std::slice::from_ref(&c), "2.1.258", &mut f);
    assert_eq!(a.closed, 1, "the closed state's anchor is counted");
    assert_eq!(a.total, 1, "and is not among the anchors that are read");
    assert!(f.is_empty(), "{f:?}");
}

/// A DOT RUN is a lossy render, not an ellipsis. Four or more dots means the capture
/// printed non-printable bytes as `.`, so the excerpt is a picture of the bytes rather than
/// the bytes - and because it contains `...`, the elided arm counted it skipped and 76
/// anchors checked nothing at all.
///
/// The refusal comes AFTER the byte comparison, which is the whole of the rest of this
/// specimen: an excerpt that IS the bytes at its offset is byte-exact whatever it looks
/// like, and testing the shape first made a quote a build really carries unanchorable.
#[test]
fn ledger_gate_rule_12_refuses_a_dot_run_only_where_the_bytes_do_not_match() {
    let build = b"alpha......bravo";
    assert_eq!(
        anchors::classify(build, 5, "......", None),
        anchors::Anchor::Exact,
        "the build really carries those bytes at that offset, so the anchor holds"
    );
    assert_eq!(
        anchors::classify(build, 4, "......", None),
        anchors::Anchor::DotRun,
        "and where it does not, the lossy render is refused by name"
    );
    assert_eq!(
        anchors::classify(build, 0, "alpha...bravo", None),
        anchors::Anchor::Elided,
        "three dots is still an ellipsis a writer typed, and its head is at the offset"
    );
    assert!(anchors::dot_run("sessionKind....trackingPath"));
    assert!(!anchors::dot_run("a...b"));
}

/// AN ELLIPSIS EXCUSES THE TAIL, NEVER THE HEAD. The elided bucket used to be compared
/// against nothing, so replacing an already-elided excerpt with fabricated text moved no
/// figure and the gate exited 0. The run before the first ellipsis is a quote of bytes like
/// any other, so it is held to the offset exactly as `prefix-only` holds a head.
#[test]
fn ledger_gate_rule_12_holds_an_elided_excerpt_to_its_own_head() {
    let build = b"alpha bravo charlie delta";
    assert_eq!(
        anchors::classify(build, 0, "alpha ... delta", None),
        anchors::Anchor::Elided,
        "the head is at the offset, so only the tail was elided"
    );
    assert_eq!(
        anchors::classify(build, 0, "alpxa ... delta", None),
        anchors::Anchor::ElidedHead { at: 3 },
        "a fabricated head is named with the byte where the build stops agreeing"
    );
    assert_eq!(
        anchors::classify(build, 0, "... delta", None),
        anchors::Anchor::ElidedNoHead,
        "an excerpt that opens with its own ellipsis quotes nothing at its offset"
    );
    assert_eq!(
        anchors::elision_head("alpha ... delta"),
        "alpha",
        "the space in front of the elision belongs to the elision, not to the quote"
    );
}

/// PROSE CAN NEVER ANCHOR, and the refusal reaches an EXCERPT rather than only a mark: it
/// used to be asked by `verify_mark` alone, so census prose wearing an ellipsis was counted
/// skipped and anchored nothing. A numeral is a word in that test, because prose carrying
/// one number is still prose.
#[test]
fn ledger_gate_rule_12_refuses_an_unmarked_excerpt_that_reads_as_prose() {
    let build = b"a string-table copy of the property name";
    assert_eq!(
        anchors::classify(build, 0, "a string-table copy of the property name", None),
        anchors::Anchor::Exact,
        "a build string really can be an English sentence, and this one is the bytes"
    );
    assert_eq!(
        anchors::classify(build, 1, "a string-table copy of the property name", None),
        anchors::Anchor::Prose,
        "where it is not the bytes, a description of them is refused by name"
    );
    assert_eq!(
        anchors::classify(
            build,
            1,
            "the pending count is 16 of 22 ... elsewhere",
            None
        ),
        anchors::Anchor::Prose,
        "an ellipsis does not make a sentence a quote"
    );
    assert!(anchors::reads_as_prose(
        "the offsets 5 of 7 records carry it"
    ));
    assert!(
        !anchors::reads_as_prose("160067268 160771073 180729337 182144236 165529356"),
        "a list of bare numbers is a census result, which has its own remedy"
    );
}

/// A hop can name its own BUILD, and a build NOBODY HOLDS is not a pass. Comparing a
/// Windows or an older-version hop against the verified build produced 33 marks asserting
/// an excerpt was nowhere - about a build it was never read in - which is why the field
/// exists; but counting the hop when the named artifact is missing made the field a way to
/// go green by typing a version. On a host that holds the verified build the hop is UNREAD,
/// and the rule says so by name.
#[test]
fn ledger_gate_rule_12_names_a_hop_whose_own_build_this_host_does_not_hold() {
    let mut c = good_claim();
    c["states"][0]["producer_chain"] = json!([{
        "role": "writer",
        "offset": 0,
        "excerpt": "zzq-no-build-carries-these-bytes-zzq",
        "build": "2.1.208-aix-ppc64",
        "note": "read in an artifact no host keeps"
    }]);
    let mut f = Vec::new();
    let a = anchors::tally(std::slice::from_ref(&c), "2.1.258", &mut f);
    assert_eq!(a.other_build, 1, "the hop is counted as another build's");
    assert_eq!(a.absent, 0, "and not as a mark");
    if a.measured.is_some() {
        assert_eq!(f.len(), 1, "the unread hop is named: {f:?}");
        assert!(f[0].contains("2.1.208-aix-ppc64"), "{}", f[0]);
        assert!(f[0].contains("this host does not hold"), "{}", f[0]);
        assert!(f[0].contains("producer_chain[0]"), "{}", f[0]);
    }
}

/// A MARK IS A CLAIM, and "occurs at no offset" is satisfied by ENGLISH. A marked excerpt
/// that describes the bytes rather than quoting them occurs in no build by construction, so
/// the mark passes while deciding nothing - the dot-run defect in another spelling.
#[test]
fn ledger_gate_rule_12_refuses_a_mark_on_an_excerpt_that_reads_as_prose() {
    let build: &[u8] = b"alpha bravo charlie";
    let prose = "a string-table copy of the property name in the non-code region";
    assert!(anchors::reads_as_prose(prose));
    let why = anchors::verify_mark(build, 6, prose, "absent").expect("prose is refused");
    assert!(why.contains("reads as prose"), "{why}");
    assert!(why.contains("re-cut the excerpt"), "{why}");
    assert!(anchors::verify_mark(build, 6, prose, "prefix-only").is_some());
    // What is NOT prose, and why each shape has to survive: a dot-joined identifier list is
    // ONE token; a census of byte offsets carries no letters at all; a quoted literal carries
    // code punctuation; and four words are not a sentence.
    for kept in [
        "Gjt.zjt.Vne.Vjt.Rw..m5e.ign",
        "96221632 178366535 178366757 179048582",
        "pidDomain:await HK()",
        "the property name",
    ] {
        assert!(!anchors::reads_as_prose(kept), "{kept}");
    }
    // An unmarked English build string still has to equal the bytes at its offset: the
    // refusal is on the MARK, not on the language.
    assert_eq!(
        anchors::classify(build, 0, "alpha bravo charlie", None),
        anchors::Anchor::Exact
    );
}

/// A build NAME is a Claude Code version, optionally an artifact of it. Without the shape a
/// hop could name any string, no host would hold it, the rule would count it `not cached`,
/// and a rotted anchor would pass by typing a build that never existed.
#[test]
fn ledger_gate_rule_12_refuses_a_build_name_no_claude_code_version_could_be() {
    for bad in [
        "9.9.9",
        "2.1",
        "2.1.258-win32",
        "latest",
        "2.1.258-Win32-ARM64",
    ] {
        let mut c = good_claim();
        c["states"][0]["producer_chain"] = json!([{
            "role": "writer", "offset": 1, "excerpt": "x", "build": bad, "note": "n"
        }]);
        let mut f = Vec::new();
        anchors::hop_shapes(std::slice::from_ref(&c), &mut f);
        assert_eq!(f.len(), 1, "`{bad}` must be refused: {f:?}");
        assert!(f[0].contains("not a Claude Code version"), "{}", f[0]);
    }
    for ok in [
        "2.1.258",
        "1.0.60",
        "2.1.258-win32-arm64",
        "2.1.258-linux-arm64",
    ] {
        let mut c = good_claim();
        c["states"][0]["producer_chain"] = json!([{
            "role": "writer", "offset": 1, "excerpt": "x", "build": ok, "note": "n"
        }]);
        let mut f = Vec::new();
        anchors::hop_shapes(std::slice::from_ref(&c), &mut f);
        assert!(f.is_empty(), "`{ok}` is a build name: {f:?}");
    }
}

/// A MARK IS A CLAIM ABOUT A BYTE LOCATION, so it needs one. Four census notes carried
/// `anchor_status: absent` at offset 0 - this file's placeholder for having no location -
/// and asserted their excerpt was nowhere in a build they had never addressed.
#[test]
fn ledger_gate_rule_12_refuses_a_mark_with_no_locator() {
    let mut c = good_claim();
    c["states"][0]["enumeration"] = json!([
        {"site": "a census note", "excerpt": "111 lines carry the needle",
         "verdict": "v", "anchor_status": "absent"},
        {"site": "another", "offset": 0, "excerpt": "0 occurrences in the whole build",
         "verdict": "v", "anchor_status": "absent"},
    ]);
    let mut f = Vec::new();
    anchors::hop_shapes(std::slice::from_ref(&c), &mut f);
    assert_eq!(f.len(), 2, "{f:?}");
    assert!(f[0].contains("carries no offset"), "{}", f[0]);
    assert!(
        f[1].contains("placeholder for having no byte location"),
        "{}",
        f[1]
    );
}

/// An offset at or past the END of the build it is compared against is an error about the
/// LOCATOR, and it is refused before the eliding and the marks - three enumeration offsets
/// addressed a predecessor build and the ellipsis arm counted them skipped.
#[test]
fn ledger_gate_rule_12_refuses_an_offset_past_the_end_of_the_build() {
    let build: &[u8] = b"alpha bravo";
    assert_eq!(
        anchors::classify(build, 99, "whatever ... it says", None),
        anchors::Anchor::PastEnd { len: 11 },
        "an ellipsis does not excuse a locator outside the file"
    );
    assert_eq!(
        anchors::classify(build, 11, "x", Some("absent")),
        anchors::Anchor::PastEnd { len: 11 },
        "and neither does a mark"
    );
    assert_eq!(
        anchors::classify(build, 10, "o", None),
        anchors::Anchor::Exact,
        "the last byte is still inside"
    );
}

/// The comparison is on BYTES. A few excerpts were captured from regions that are not valid
/// UTF-8 and stored one code point per byte; encoding such a string back to UTF-8 widens
/// every byte above 0x7f, so the comparison could never match while the anchor was sound.
#[test]
fn ledger_gate_rule_12_reads_a_latin_1_capture_as_bytes() {
    let build = &[b'a', 0xe9, b'b'];
    let excerpt = "a\u{e9}b"; // three CHARACTERS, four UTF-8 bytes
    assert_eq!(excerpt.len(), 4);
    assert_eq!(
        anchors::classify(build, 0, excerpt, None),
        anchors::Anchor::Exact
    );
}

/// Rule 12's MARK SWEEP takes two roads and both of them owe a specimen. The "occurs at no
/// offset" half of every mark runs as ONE Aho-Corasick pass over the build instead of one
/// search per mark; only when that pass matches something does the exact per-mark path run.
/// A short-circuit nobody has seen take both roads is one that quietly stops checking, so
/// this drives each: a mark the build refutes must still be named, and a set of marks the
/// build carries none of must take the cheap road and still catch a `prefix-only` whose
/// head does not match where its offset points.
#[test]
fn ledger_gate_rule_12_sweeps_every_mark_and_names_the_false_one() {
    // The pass MATCHES: one mark says `absent` about bytes every build of this line carries.
    let mut c = good_claim();
    c["states"][0]["producer_chain"] = json!([
        {"role": "writer", "offset": 1, "excerpt": "CLAUDE_CODE_USE_POWERSHELL_TOOL",
         "anchor_status": "absent", "note": "a mark the build refutes"},
        {"role": "gate", "offset": 2, "excerpt": "zzq-no-build-carries-these-bytes-zzq",
         "anchor_status": "absent", "note": "a mark that holds"}
    ]);
    let mut f = Vec::new();
    let a = anchors::tally(std::slice::from_ref(&c), "2.1.258", &mut f);
    if a.measured.is_some() {
        assert_eq!(f.len(), 1, "only the refuted mark is named: {f:?}");
        assert!(
            f[0].contains("marked `absent` but the excerpt occurs"),
            "{f:?}"
        );
        assert!(f[0].contains("producer_chain[0]"), "{f:?}");
    }

    // The pass finds NOTHING: the cheap road, which still owes the `prefix-only` head test.
    let mut c = good_claim();
    c["states"][0]["producer_chain"] = json!([
        {"role": "writer", "offset": 3, "excerpt": "zzq-no-build-carries-these-bytes-zzq",
         "anchor_status": "prefix-only", "note": "a head that cannot match at offset 3"},
        {"role": "gate", "offset": 4, "excerpt": "zzq-nor-these-either-zzq",
         "anchor_status": "absent", "note": "a mark that holds"}
    ]);
    let mut f = Vec::new();
    let a = anchors::tally(std::slice::from_ref(&c), "2.1.258", &mut f);
    if a.measured.is_some() {
        assert_eq!(f.len(), 1, "only the false head is named: {f:?}");
        assert!(
            f[0].contains("marked `prefix-only` but no leading byte"),
            "{f:?}"
        );
    }
}

/// A MARKED HOP IS JUDGED BY ITS MARK, and the arm order is what says so.
///
/// The elided arm used to run first, so a hop marked `absent` or `prefix-only` whose
/// excerpt happened to carry an ellipsis with a head that still matched at its offset came
/// back `Elided`: counted in the elided bucket by `tally` AND under its own mark by the
/// ledger-only filter, which puts one hop in two of the five counts the README says
/// partition the total. No hop in the shipped file is shaped that way, which is exactly why
/// the order has to carry the rule rather than the data.
#[test]
fn ledger_gate_rule_12_judges_a_marked_hop_by_its_mark_before_eliding() {
    let build: &[u8] = b"alpha bravo charlie ...rest delta echo";
    // Unmarked, this is the shape the elided arm takes: the run before the first ellipsis
    // is `bra`, and the build carries it at the offset.
    assert_eq!(
        anchors::classify(build, 6, "bra...vo", None),
        anchors::Anchor::Elided
    );
    // Marked, the same hop lands in ONE bucket - its own.
    assert_eq!(
        anchors::classify(build, 6, "bra...vo", Some("absent")),
        anchors::Anchor::Absent
    );
    assert_eq!(
        anchors::classify(build, 6, "bra...vo", Some("prefix-only")),
        anchors::Anchor::PrefixOnly
    );
    // The byte comparison still outranks the mark, so a hop that is exact again is exact.
    assert_eq!(
        anchors::classify(build, 6, "bravo", Some("absent")),
        anchors::Anchor::Exact
    );
    // A LOSSY RENDER wearing a mark no longer reaches `classify`'s dot-run refusal, so the
    // refusal lives where the mark is judged - the same vacuity as prose and an empty
    // excerpt, and it must still be named.
    let why = anchors::mark_shape("al....ha", "absent").expect("a marked dot run is refused");
    assert!(why.contains("four or more dots"), "{why}");

    // And the five counts still partition the total when a mark sits on an excerpt that
    // elides something.
    let mut c = good_claim();
    c["states"][0]["producer_chain"] = json!([
        {"role": "writer", "offset": 5,
         "excerpt": "zzq-no-build-carries ... these-bytes-zzq",
         "anchor_status": "absent", "note": "a mark on an excerpt that also elides"}
    ]);
    let mut f = Vec::new();
    let a = anchors::tally(std::slice::from_ref(&c), "2.1.258", &mut f);
    if let Some((exact, elided)) = a.measured {
        assert!(f.is_empty(), "the mark holds, so nothing is named: {f:?}");
        assert_eq!(a.absent, 1, "the hop is counted under its mark");
        assert_eq!(elided, 0, "and not a second time as elided");
        assert_eq!(
            exact + elided + a.absent + a.prefix_only + a.other_build,
            a.total,
            "the five counts partition the total"
        );
    }
}

/// EVERY REFUSAL WHOSE REMEDY IS BYTES NAMES THE ARTIFACT THOSE BYTES ARE IN.
///
/// Four verdicts describe the EXCERPT rather than the build - an empty one, a lossy render,
/// a prose sentence, an excerpt opening with its own ellipsis - so they are decided without
/// reading anything, and their sentences read as though no artifact were involved. Each one
/// still sends the reader to re-cut the excerpt from real bytes, and a hop carrying its own
/// `build` is re-cut from THAT artifact rather than from the badge's, so the sentence owes
/// the name. Three of the seven already carried it.
#[test]
fn ledger_gate_rule_12_every_excerpt_refusal_names_the_build_to_re_cut_from() {
    for (excerpt, want) in [
        ("", "beside an EMPTY excerpt"),
        ("al....ha", "four or more dots"),
        (
            "a string-table copy of the property name in the non-code region",
            "reads as prose",
        ),
        ("...only a tail", "OPENS with its own ellipsis"),
    ] {
        let mut c = good_claim();
        c["states"][0]["producer_chain"] = json!([{
            "role": "writer", "offset": 5, "excerpt": excerpt,
            "note": "an excerpt that cannot be the bytes"
        }]);
        let mut f = Vec::new();
        let a = anchors::tally(std::slice::from_ref(&c), "2.1.258", &mut f);
        if a.measured.is_none() {
            continue;
        }
        assert_eq!(f.len(), 1, "one defect, one line: {f:?}");
        assert!(f[0].contains(want), "{}", f[0]);
        assert!(
            f[0].contains("Claude Code 2.1.258"),
            "the refusal names the artifact to re-cut from: {}",
            f[0]
        );
    }
}
