//! Rule 12: the ledger's byte anchors, re-read against the verified Claude Code build.
//!
//! Every `producer_chain` hop and every `enumeration` entry carries a byte `offset` into
//! the build named by `verified_claude_code`. An offset is a locator and the excerpt is
//! the evidence, so the check is one comparison: the bytes at the offset ARE the excerpt,
//! or the anchor no longer points at its producer. Three anchors are counted and skipped
//! instead of compared - an ELIDED excerpt (it carries an ellipsis, so it never had a
//! byte-exact form), and one the ledger itself marks `anchor_status: "absent"` or
//! `"prefix-only"`, the two outcomes recorded when an excerpt cannot be located at all.
//! Those marks are the standing worklist: they are what a later pass re-locates first.
//!
//! A MARK IS A CLAIM, NOT A PASS. Skipping a marked hop without testing what it asserts
//! would make the mark a way to quiet the rule by typing a word, so every mark is
//! verified against the build too: `absent` means the excerpt is there at NO offset,
//! `prefix-only` means that plus a head that still matches where the offset points.
//!
//! The build is read from `$CSIFT_LEDGER_BINARY`, else from the native install layout
//! `<home>/.local/share/claude/versions/<verified_claude_code>`. A host without it - a
//! container, a Windows guest, a downstream `cargo test` - prints one line naming the
//! path it looked for and PASSES: an anchor is a property of a build, not of this
//! repository, and a machine that does not hold the build cannot decide one.

use serde_json::Value;

/// What one anchor is doing at its offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anchor {
    /// The bytes at the offset are the excerpt.
    Exact,
    /// The excerpt carries an ellipsis, so it was never a byte-exact quote.
    Elided,
    /// Marked `absent`: the excerpt is in the build at no offset at all.
    Absent,
    /// Marked `prefix-only`: the head matches at the offset and the tail does not.
    PrefixOnly,
    /// The anchor rotted. `at` is the excerpt byte where the build stops agreeing.
    Drifted { at: usize },
}

/// The one classifier. Pure over (build, offset, excerpt, mark), so a synthetic byte
/// string exercises every verdict.
pub(crate) fn classify(build: &[u8], offset: usize, excerpt: &str, status: Option<&str>) -> Anchor {
    let want = excerpt.as_bytes();
    let avail = build.get(offset..).unwrap_or(&[]);
    if avail.len() >= want.len() && &avail[..want.len()] == want {
        return Anchor::Exact;
    }
    // Checked AFTER the byte comparison, deliberately: minified JavaScript is dense with
    // `...`, so an excerpt carrying one is very often byte-exact anyway, and counting it
    // exact is the stronger reading. An ellipsis is an excuse, not a category that
    // pre-empts the check.
    if excerpt.contains("...") || excerpt.contains('\u{2026}') {
        return Anchor::Elided;
    }
    match status {
        Some("absent") => Anchor::Absent,
        Some("prefix-only") => Anchor::PrefixOnly,
        _ => Anchor::Drifted {
            at: avail.iter().zip(want).take_while(|(a, b)| a == b).count(),
        },
    }
}

/// The rule's counts. `absent` / `prefix_only` / `total` come from the ledger alone, so
/// rule 8 can check the README's line on a host with no build; `measured` needs one.
#[derive(Debug)]
pub(crate) struct Anchors {
    pub(crate) total: usize,
    pub(crate) absent: usize,
    pub(crate) prefix_only: usize,
    /// `(byte-exact, elided)`, or `None` when the verified build is not on this host.
    pub(crate) measured: Option<(usize, usize)>,
}

struct Site<'a> {
    id: &'a str,
    field: &'static str,
    index: usize,
    offset: usize,
    excerpt: &'a str,
    status: Option<&'a str>,
}

fn sites(claims: &[Value]) -> Vec<Site<'_>> {
    let mut out = Vec::new();
    for c in claims {
        let id = c["id"].as_str().unwrap_or("");
        for field in ["producer_chain", "enumeration"] {
            let hops = c[field].as_array().map(Vec::as_slice).unwrap_or(&[]);
            for (index, h) in hops.iter().enumerate() {
                let Some(offset) = h["offset"].as_u64() else {
                    continue;
                };
                out.push(Site {
                    id,
                    field,
                    index,
                    offset: offset as usize,
                    excerpt: h["excerpt"].as_str().unwrap_or(""),
                    status: h["anchor_status"].as_str(),
                });
            }
        }
    }
    out
}

fn build_path(verified: &str) -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("CSIFT_LEDGER_BINARY") {
        if !p.trim().is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join(".local/share/claude/versions")
            .join(verified),
    )
}

fn drift_note(build: &[u8], site: &Site, at: usize) -> String {
    let want = site.excerpt.as_bytes();
    match build.get(site.offset.saturating_add(at)) {
        Some(found) => format!(
            "first difference at excerpt byte {at}: the excerpt has 0x{:02x}, the build has 0x{found:02x}",
            want[at]
        ),
        None => "the offset runs past the end of the build".to_string(),
    }
}

/// A MARK IS A CLAIM, NOT A PASS. `absent` asserts the excerpt is in the build at no
/// offset at all; `prefix-only` asserts the same PLUS a head that still matches where the
/// offset points. Counting a marked hop without testing what it asserts would let any
/// rotted anchor be quieted by typing a word, which is the one move this whole rule
/// exists to catch. `None` = the mark holds.
fn verify_mark(build: &[u8], offset: usize, excerpt: &str, status: &str) -> Option<String> {
    let want = excerpt.as_bytes();
    if want.is_empty() {
        return Some(format!(
            "marked `{status}` with an empty excerpt, which asserts nothing"
        ));
    }
    if let Some(at) = memchr::memmem::find(build, want) {
        return Some(format!(
            "marked `{status}` but the excerpt occurs in the build at offset {at} - \
             re-anchor the offset instead of marking the hop"
        ));
    }
    if status == "prefix-only" {
        let avail = build.get(offset..).unwrap_or(&[]);
        if avail.first() != want.first() {
            return Some(format!(
                "marked `prefix-only` but no leading byte of the excerpt matches at offset \
                 {offset} - that mark asserts a head match, so the hop is `absent`"
            ));
        }
    }
    None
}

/// Every marked hop, checked against the build.
///
/// The expensive half of a mark is "occurs at NO offset", one whole-binary search each,
/// so all of them run as ONE Aho-Corasick pass instead of hundreds of separate scans. It
/// is sound as a short-circuit: if any marked excerpt occurred anywhere, the pass would
/// report at least one match, so ZERO matches proves every "absent" half. Only when
/// something DID match does the exact per-mark path run, and that run is failing anyway.
fn check_marks(build: &[u8], sites: &[Site<'_>], verified: &str, failures: &mut Vec<String>) {
    let marked: Vec<&Site<'_>> = sites.iter().filter(|s| s.status.is_some()).collect();
    if marked.is_empty() {
        return;
    }
    let needles: Vec<&[u8]> = marked
        .iter()
        .map(|s| s.excerpt.as_bytes())
        .filter(|b| !b.is_empty())
        .collect();
    let clean = needles.len() == marked.len()
        && aho_corasick::AhoCorasick::new(&needles)
            .expect("the marked excerpts build an automaton")
            .find(build)
            .is_none();
    if clean {
        // No marked excerpt is anywhere in the build; only the cheap head check is left.
        for s in &marked {
            if s.status == Some("prefix-only")
                && build.get(s.offset) != s.excerpt.as_bytes().first()
            {
                failures.push(format!(
                    "{}: {}[{}] marked `prefix-only` but no leading byte of the excerpt \
                     matches at offset {} - that mark asserts a head match, so the hop is \
                     `absent` (Claude Code {verified})",
                    s.id, s.field, s.index, s.offset
                ));
            }
        }
        return;
    }
    for s in &marked {
        let Some(status) = s.status else { continue };
        if let Some(why) = verify_mark(build, s.offset, s.excerpt, status) {
            failures.push(format!(
                "{}: {}[{}] {why} (Claude Code {verified})",
                s.id, s.field, s.index
            ));
        }
    }
}

/// Classify every anchor, push a failure for each one that rotted, print the tally.
pub(crate) fn tally(claims: &[Value], verified: &str, failures: &mut Vec<String>) -> Anchors {
    let sites = sites(claims);
    let total = sites.len();
    let absent = sites.iter().filter(|s| s.status == Some("absent")).count();
    let prefix_only = sites
        .iter()
        .filter(|s| s.status == Some("prefix-only"))
        .count();
    let path = build_path(verified);
    let build = path.as_ref().and_then(|p| std::fs::read(p).ok());
    let Some(build) = build else {
        let where_ = path.map_or_else(
            || "no home directory in the environment".to_string(),
            |p| p.display().to_string(),
        );
        println!(
            "ledger rule 12: Claude Code {verified} is not on this host ({where_}) - \
             {total} anchor(s) left unread"
        );
        return Anchors {
            total,
            absent,
            prefix_only,
            measured: None,
        };
    };
    let (mut exact, mut elided) = (0usize, 0usize);
    for site in &sites {
        match classify(&build, site.offset, site.excerpt, site.status) {
            Anchor::Exact => exact += 1,
            Anchor::Elided => elided += 1,
            Anchor::Absent | Anchor::PrefixOnly => {}
            Anchor::Drifted { at } => failures.push(format!(
                "{}: {}[{}] offset {} no longer carries its excerpt at Claude Code \
                 {verified} ({}) - re-anchor the offset, or mark the hop \
                 `anchor_status` when the excerpt is nowhere in the build",
                site.id,
                site.field,
                site.index,
                site.offset,
                drift_note(&build, site, at)
            )),
        }
    }
    check_marks(&build, &sites, verified, failures);
    println!(
        "ledger rule 12: anchors byte-exact at Claude Code {verified}: {exact} of {total} \
         (elided {elided}, absent {absent}, prefix-only {prefix_only})"
    );
    Anchors {
        total,
        absent,
        prefix_only,
        measured: Some((exact, elided)),
    }
}

/// The classifier over a synthetic build: one string carrying a literal `...`, so every
/// verdict and both ordering rules are exercised without the real binary.
#[test]
fn ledger_gate_anchor_classifier_decides_the_five_verdicts() {
    let build: &[u8] = b"alpha bravo charlie ...rest delta echo";
    // Exact: the bytes at the offset are the excerpt.
    assert_eq!(classify(build, 6, "bravo", None), Anchor::Exact);
    // Byte-exactness outranks eliding: a `...` that really is in the build is exact.
    assert_eq!(classify(build, 20, "...rest", None), Anchor::Exact);
    // Elided: an ellipsis in the excerpt, in either spelling.
    assert_eq!(classify(build, 6, "bra...vo", None), Anchor::Elided);
    assert_eq!(classify(build, 6, "bra\u{2026}vo", None), Anchor::Elided);
    // Drifted: the head agrees for four bytes, then the build says something else.
    assert_eq!(
        classify(build, 6, "bravado", None),
        Anchor::Drifted { at: 4 }
    );
    // Drifted with the excerpt running off the end of the build.
    assert_eq!(
        classify(build, 34, "echoes", None),
        Anchor::Drifted { at: 4 }
    );
    // The two marks are counted and skipped.
    assert_eq!(classify(build, 6, "zulu", Some("absent")), Anchor::Absent);
    assert_eq!(
        classify(build, 6, "bravado", Some("prefix-only")),
        Anchor::PrefixOnly
    );
    // A mark never hides a hop that is byte-exact again.
    assert_eq!(classify(build, 6, "bravo", Some("absent")), Anchor::Exact);
    // The drift note names the first differing byte on both sides.
    let site = Site {
        id: "TEST-001",
        field: "producer_chain",
        index: 0,
        offset: 6,
        excerpt: "bravado",
        status: None,
    };
    assert_eq!(
        drift_note(build, &site, 4),
        "first difference at excerpt byte 4: the excerpt has 0x61, the build has 0x6f"
    );
}

/// A mark is a CLAIM about the build, so the rule tests what it asserts. Without this, an
/// anchor that rotted could be quieted by typing `absent` beside it.
#[test]
fn ledger_gate_a_mark_is_a_claim_not_a_pass() {
    let build: &[u8] = b"alpha bravo charlie ...rest delta echo";
    // The honest marks: the excerpt is nowhere in the build.
    assert_eq!(verify_mark(build, 6, "zulu", "absent"), None);
    assert_eq!(verify_mark(build, 6, "bravado", "prefix-only"), None);
    // Marking a hop whose excerpt IS in the build is refused, whatever the offset says.
    assert_eq!(
        verify_mark(build, 999, "bravo", "absent"),
        Some(
            "marked `absent` but the excerpt occurs in the build at offset 6 - \
             re-anchor the offset instead of marking the hop"
                .to_string()
        )
    );
    assert!(verify_mark(build, 999, "bravo", "prefix-only").is_some());
    // `prefix-only` also asserts a head match where the offset points.
    assert_eq!(
        verify_mark(build, 12, "bravado", "prefix-only"),
        Some(
            "marked `prefix-only` but no leading byte of the excerpt matches at offset 12 - \
             that mark asserts a head match, so the hop is `absent`"
                .to_string()
        )
    );
    // `absent` makes no claim about the offset, so a head mismatch there is fine.
    assert_eq!(verify_mark(build, 12, "zulu", "absent"), None);
    // An empty excerpt asserts nothing and can never be marked.
    assert!(verify_mark(build, 6, "", "absent").is_some());
}
