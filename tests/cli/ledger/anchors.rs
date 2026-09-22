//! Rule 12: the ledger's byte anchors, re-read against the verified Claude Code build.
//!
//! Every `producer_chain` hop and every `enumeration` entry may carry a byte `offset` into
//! the build. An offset is a locator and the excerpt is the evidence, so the check is one
//! comparison: the bytes at the offset ARE the excerpt, or the anchor no longer points at
//! its producer. Three anchors are counted and skipped instead of compared - an ELIDED
//! excerpt (it carries an ellipsis, so it never had a byte-exact form), and one the ledger
//! itself marks `anchor_status: "absent"` or `"prefix-only"`, the two outcomes recorded
//! when an excerpt cannot be located at all. Those marks are the standing worklist: they
//! are what a later pass re-locates first.
//!
//! A MARK IS A CLAIM, NOT A PASS. Skipping a marked hop without testing what it asserts
//! would make the mark a way to quiet the rule by typing a word, so every mark is
//! verified against the build too: `absent` means the excerpt is there at NO offset,
//! `prefix-only` means that plus a head that still matches where the offset points.
//!
//! AN EMPTY EXCERPT IS NEVER EVIDENCE. `bytes[off..off]` equals an empty excerpt at every
//! offset in every build, so an offset beside one passed the comparison while anchoring
//! nothing - and an offset of 0, which is a placeholder for "no byte location", made that
//! a silent free pass. A hop that has no byte location carries no `offset` at all; one
//! that carries an offset owes an excerpt.
//!
//! OPEN STATES ONLY. A closed state's range ended at an older build, where its anchors
//! were exact; re-reading them against a later build would demand that a superseded fact
//! still sit at its old bytes. They are COUNTED, so the README's line discloses how much
//! of the ledger the rule did not re-read, and never compared.
//!
//! A HOP CAN NAME ITS OWN BUILD. Some facts are only readable in a build the badge does
//! not name - a Windows artifact, a version whose writer the current one replaced - and
//! those hops were being compared against the verified build, failing, and then marked
//! `absent`: 33 false marks asserting an excerpt was nowhere, about a build it was never
//! read in. An optional `build` on a hop names the artifact it WAS read in; the rule
//! compares against that one when the host holds it, and counts it `other-build, not
//! cached` when it does not. The mark is then reserved for what it means.
//!
//! A DOT RUN IS NOT AN ELLIPSIS. Four or more dots in an excerpt is the signature of a
//! LOSSY RENDER: the capture printed every non-printable byte as `.`, so the excerpt is a
//! picture of the bytes rather than the bytes, and it can never be byte-exact. Worse, it
//! contains `...`, so the elided arm counted it skipped and 76 anchors checked nothing at
//! all. It is refused the way an empty excerpt is, and the remedy is the same: re-cut the
//! excerpt to the clean run around the literal.
//!
//! THE COMPARISON IS ON BYTES, not on a UTF-8 round trip. A few excerpts were captured
//! from regions that are not valid UTF-8 and stored one code point per byte; encoding such
//! a string back to UTF-8 widens every byte above 0x7f, so the comparison could never
//! match while the anchor was perfectly sound. Both readings are tried.
//!
//! The verified build is read from `$CSIFT_LEDGER_BINARY`, else from the native install
//! layout `<home>/.local/share/claude/versions/<verified_claude_code>`; a hop's own build
//! is read from `$CSIFT_LEDGER_BUILDS/<build>/claude`, else the cache the release matrix
//! keeps, else that same native layout. A host without a build - a container, a Windows
//! guest, a downstream `cargo test` - prints one line naming the path it looked for and
//! PASSES: an anchor is a property of a build, not of this repository, and a machine that
//! does not hold the build cannot decide one.

use serde_json::Value;
use std::collections::HashMap;

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
    /// An offset beside an empty excerpt, which anchors nothing.
    Empty,
    /// An excerpt carrying a run of four or more dots: a lossy render of bytes that are
    /// not printable, which no build can ever equal.
    DotRun,
    /// The offset sits at or past the END of the compared build. That is an error about the
    /// LOCATOR rather than about the excerpt, so it is refused before the eliding and the
    /// marks - both of which would otherwise let an offset no file can hold pass as skipped.
    PastEnd { len: usize },
    /// The anchor rotted. `at` is the excerpt byte where the build stops agreeing.
    Drifted { at: usize },
}

/// The one classifier. Pure over (build, offset, excerpt, mark), so a synthetic byte
/// string exercises every verdict.
pub(crate) fn classify(build: &[u8], offset: usize, excerpt: &str, status: Option<&str>) -> Anchor {
    // Checked FIRST, before the comparison: an empty excerpt matches at every offset, so
    // letting it reach the byte test would count it exact while it anchors nothing.
    if excerpt.is_empty() {
        return Anchor::Empty;
    }
    // Checked SECOND, before the byte comparison and before the ellipsis arm: a run of four
    // or more dots is a lossy render rather than a quote, and the ellipsis arm would
    // otherwise count it skipped for carrying the `...` its own damage produced.
    if dot_run(excerpt) {
        return Anchor::DotRun;
    }
    // Checked THIRD, and before the eliding and the marks: an offset at or past the end of
    // the build addresses no byte of it, which is a defect in the LOCATOR. Three enumeration
    // offsets addressed a predecessor build and were compared against the verified one,
    // where the ellipsis arm counted them skipped rather than naming the mismatch.
    if offset >= build.len() {
        return Anchor::PastEnd { len: build.len() };
    }
    let avail = build.get(offset..).unwrap_or(&[]);
    for want in readings(excerpt) {
        if avail.len() >= want.len() && avail[..want.len()] == want[..] {
            return Anchor::Exact;
        }
    }
    let want = excerpt.as_bytes();
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

/// Four or more consecutive dots. Three is an ellipsis a writer typed; four is what a
/// printable-ASCII filter leaves where a run of non-printable bytes was.
pub(crate) fn dot_run(excerpt: &str) -> bool {
    excerpt.as_bytes().windows(4).any(|w| w == b"....")
}

/// The byte readings of one excerpt, strongest first: its UTF-8 bytes, and - when every
/// character fits in a byte - the LATIN-1 reading a capture from a non-UTF-8 region
/// produces. See the module doc: the second is why fourteen sound anchors could not match.
fn readings(excerpt: &str) -> Vec<Vec<u8>> {
    let utf8 = excerpt.as_bytes().to_vec();
    let mut out = vec![utf8];
    if excerpt.chars().any(|c| c as u32 > 0x7f) && excerpt.chars().all(|c| (c as u32) < 0x100) {
        out.push(excerpt.chars().map(|c| c as u8).collect());
    }
    out
}

/// The rule's counts. `absent` / `prefix_only` / `total` / `closed` come from the ledger
/// alone, so rule 8 can check the README's line on a host with no build; `measured` needs
/// one.
#[derive(Debug)]
pub(crate) struct Anchors {
    pub(crate) total: usize,
    pub(crate) absent: usize,
    pub(crate) prefix_only: usize,
    /// Anchors belonging to a CLOSED state: counted, never compared.
    pub(crate) closed: usize,
    /// Anchors whose hop names its OWN `build` rather than the verified one, and carries
    /// no mark. They are disclosed as a COUNT rather than as a verdict split, because
    /// whether this host holds thirteen other artifacts is a property of the host and not
    /// of the ledger - and the README states numbers a reader re-derives.
    pub(crate) other_build: usize,
    /// `(byte-exact, elided)` over the anchors read against the VERIFIED build, or `None`
    /// when that build is not on this host. The five counts partition the total exactly:
    /// `exact + elided + absent + prefix_only + other_build == total`.
    pub(crate) measured: Option<(usize, usize)>,
}

pub(crate) struct Site<'a> {
    pub(crate) id: &'a str,
    pub(crate) field: &'static str,
    pub(crate) index: usize,
    pub(crate) offset: usize,
    pub(crate) excerpt: &'a str,
    pub(crate) status: Option<&'a str>,
    /// The artifact this hop was read in, when it is not the verified build.
    pub(crate) build: Option<&'a str>,
}

/// Every offset-carrying hop of one state.
fn state_sites<'a>(id: &'a str, state: &'a Value, out: &mut Vec<Site<'a>>) {
    for field in ["producer_chain", "enumeration"] {
        let hops = state[field].as_array().map(Vec::as_slice).unwrap_or(&[]);
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
                build: h["build"].as_str(),
            });
        }
    }
}

/// `(the open states' anchors, the count of closed states' anchors)`.
fn sites(claims: &[Value]) -> (Vec<Site<'_>>, usize) {
    let mut out = Vec::new();
    let mut closed = 0usize;
    for c in claims {
        let id = c["id"].as_str().unwrap_or("");
        for state in super::states(c) {
            if state["to"].is_null() {
                state_sites(id, state, &mut out);
            } else {
                let mut skipped = Vec::new();
                state_sites(id, state, &mut skipped);
                closed += skipped.len();
            }
        }
    }
    (out, closed)
}

fn build_path(verified: &str) -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("CSIFT_LEDGER_BINARY") {
        if !p.trim().is_empty() {
            return Some(std::path::PathBuf::from(p));
        }
    }
    native_build(verified)
}

fn native_build(name: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    Some(
        std::path::PathBuf::from(home)
            .join(".local/share/claude/versions")
            .join(name),
    )
}

/// Where a hop's OWN build is looked for, in order: the cache root `$CSIFT_LEDGER_BUILDS`
/// names, then the native install layout. A name is a directory holding one `claude`
/// executable, so a Windows artifact and an older version are addressed alike. No other
/// root is consulted: a verification cache is the operator's, and naming one here would
/// make a shipped test depend on a path that exists on one machine.
fn named_build_paths(name: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Ok(root) = std::env::var("CSIFT_LEDGER_BUILDS") {
        if !root.trim().is_empty() {
            out.push(std::path::PathBuf::from(&root).join(name).join("claude"));
        }
    }
    out.extend(native_build(name));
    out
}

/// Reads each named build at most once. A build this host does not hold is remembered as
/// absent, so its anchors are counted rather than compared and nothing is read twice.
#[derive(Default)]
pub(crate) struct BuildCache {
    builds: HashMap<String, Option<Vec<u8>>>,
}

impl BuildCache {
    fn get(&mut self, name: &str) -> Option<&[u8]> {
        self.builds
            .entry(name.to_string())
            .or_insert_with(|| {
                named_build_paths(name)
                    .iter()
                    .find_map(|p| std::fs::read(p).ok())
            })
            .as_deref()
    }
}

pub(crate) fn drift_note(build: &[u8], site: &Site, at: usize) -> String {
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
pub(crate) fn verify_mark(
    build: &[u8],
    offset: usize,
    excerpt: &str,
    status: &str,
) -> Option<String> {
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
    let marked: Vec<&Site<'_>> = sites
        .iter()
        .filter(|s| s.status.is_some() && s.build.is_none())
        .collect();
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

/// Classify every open state's anchor, push a failure for each one that rotted, print the
/// tally.
/// A build NAME is a Claude Code version, optionally an artifact of it.
///
/// `9.9.9` is a version triple and is not a Claude Code version: every published one carries
/// a major of 1 or 2, which the prose law already reads. Without the shape a hop could name
/// any string, no host would hold it, the rule would count it `not cached`, and a rotted
/// anchor would pass by typing a build that never existed.
fn build_name_ok(name: &str) -> bool {
    let (version, artifact) = match name.split_once('-') {
        Some((v, rest)) => (v, Some(rest)),
        None => (name, None),
    };
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || !matches!(parts[0], "1" | "2") {
        return false;
    }
    if !parts
        .iter()
        .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
    {
        return false;
    }
    match artifact {
        None => true,
        // `<platform>-<arch>`, exactly two lowercase words.
        Some(rest) => {
            let words: Vec<&str> = rest.split('-').collect();
            words.len() == 2
                && words.iter().all(|w| {
                    !w.is_empty()
                        && w.bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                })
        }
    }
}

/// The SHAPE of every hop of every open state, checked on any host - it reads no build, so a
/// container answers it exactly as this one does.
///
/// Two defects it names, both ways a hop could carry a mark or a build that decides nothing:
/// a build name no Claude Code version could be, and a MARK with no locator. A mark is a
/// claim about a byte location, and `offset` 0 is this file's placeholder for having none -
/// four census notes carried one and asserted their excerpt was nowhere in a build they had
/// never addressed.
pub(crate) fn hop_shapes(claims: &[Value], failures: &mut Vec<String>) {
    for c in claims {
        let id = c["id"].as_str().unwrap_or("");
        for state in super::states(c) {
            if !state["to"].is_null() {
                continue;
            }
            for field in ["producer_chain", "enumeration"] {
                let hops = state[field].as_array().map(Vec::as_slice).unwrap_or(&[]);
                for (index, h) in hops.iter().enumerate() {
                    if let Some(name) = h["build"].as_str() {
                        if !build_name_ok(name) {
                            failures.push(format!(
                                "{id}: {field}[{index}] names build `{name}`, which is not a \
                                 Claude Code version (a triple with a major of 1 or 2, \
                                 optionally `-<platform>-<arch>`)"
                            ));
                        }
                    }
                    let Some(status) = h["anchor_status"].as_str() else {
                        continue;
                    };
                    match h["offset"].as_u64() {
                        None => failures.push(format!(
                            "{id}: {field}[{index}] is marked `{status}` and carries no offset - \
                             a mark is a claim about a byte location, so it needs one or it \
                             decides nothing"
                        )),
                        Some(0) => failures.push(format!(
                            "{id}: {field}[{index}] is marked `{status}` at offset 0, which is \
                             this file's placeholder for having no byte location - drop the \
                             offset and the mark, or point them at real bytes"
                        )),
                        Some(_) => {}
                    }
                }
            }
        }
    }
}

pub(crate) fn tally(claims: &[Value], verified: &str, failures: &mut Vec<String>) -> Anchors {
    hop_shapes(claims, failures);
    let (sites, closed) = sites(claims);
    let total = sites.len();
    // The five README counts PARTITION the total, so each anchor is in exactly one: a mark
    // is counted as its mark whichever build it belongs to, an unmarked hop naming its own
    // build is counted there, and only what is left is compared against the verified build.
    let absent = sites.iter().filter(|s| s.status == Some("absent")).count();
    let prefix_only = sites
        .iter()
        .filter(|s| s.status == Some("prefix-only"))
        .count();
    let other_build = sites
        .iter()
        .filter(|s| s.build.is_some() && s.status.is_none())
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
             {total} anchor(s) left unread, {closed} closed-state anchor(s) not checked"
        );
        return Anchors {
            total,
            absent,
            prefix_only,
            closed,
            other_build,
            measured: None,
        };
    };
    let (mut exact, mut elided) = (0usize, 0usize);
    let (mut named_exact, mut uncached) = (0usize, 0usize);
    let mut cache = BuildCache::default();
    for site in &sites {
        // A hop naming its own build is compared against THAT artifact; one this host does
        // not hold is counted and skipped, exactly as the verified build's absence is.
        let (bytes, at_build) = match site.build {
            None => (Some(&build[..]), verified),
            Some(name) => (cache.get(name), name),
        };
        let Some(bytes) = bytes else {
            uncached += 1;
            continue;
        };
        let verdict = classify(bytes, site.offset, site.excerpt, site.status);
        if site.build.is_some() {
            if let Some(why) = site
                .status
                .and_then(|st| verify_mark(bytes, site.offset, site.excerpt, st))
            {
                failures.push(format!(
                    "{}: {}[{}] {why} (Claude Code {at_build})",
                    site.id, site.field, site.index
                ));
            }
        }
        match verdict {
            // A hop naming its own build is disclosed as a COUNT, not as a verdict: the
            // page must state the same number on a host that holds thirteen artifacts and
            // on one that holds none.
            Anchor::Exact | Anchor::Elided if site.build.is_some() => named_exact += 1,
            Anchor::Exact => exact += 1,
            Anchor::Elided => elided += 1,
            Anchor::Absent | Anchor::PrefixOnly => {}
            Anchor::Empty => failures.push(format!(
                "{}: {}[{}] carries offset {} beside an EMPTY excerpt, which anchors nothing \
                 - drop the offset, or quote the bytes it points at",
                site.id, site.field, site.index, site.offset
            )),
            Anchor::PastEnd { len } => failures.push(format!(
                "{}: {}[{}] offset {} is at or past the END of Claude Code {at_build} ({len} \
                 bytes) - the locator addresses no byte of the build it is compared against, \
                 so name the build it WAS read in (`build`) or drop the offset",
                site.id, site.field, site.index, site.offset
            )),
            Anchor::DotRun => failures.push(format!(
                "{}: {}[{}] offset {} carries a run of four or more dots, which is a lossy \
                 render of bytes that are not printable rather than a quote of them - re-cut \
                 the excerpt to the clean run around its literal",
                site.id, site.field, site.index, site.offset
            )),
            Anchor::Drifted { at } => failures.push(format!(
                "{}: {}[{}] offset {} no longer carries its excerpt at Claude Code \
                 {at_build} ({}) - re-anchor the offset, name the build the hop was read in, \
                 or mark the hop `anchor_status` when the excerpt is nowhere in it",
                site.id,
                site.field,
                site.index,
                site.offset,
                drift_note(bytes, site, at)
            )),
        }
    }
    check_marks(&build, &sites, verified, failures);
    println!(
        "ledger rule 12: anchors byte-exact at Claude Code {verified}: {exact} of {total} \
         (elided {elided}, absent {absent}, prefix-only {prefix_only}, read in another build \
         {other_build}), closed-state anchors {closed}, not checked \
         [another build: {named_exact} read on this host, {uncached} not cached]"
    );
    Anchors {
        total,
        absent,
        prefix_only,
        closed,
        other_build,
        measured: Some((exact, elided)),
    }
}
