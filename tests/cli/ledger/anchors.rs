//! Rule 12: the ledger's byte anchors, re-read against the verified Claude Code build.
//!
//! Every `producer_chain` hop and every `enumeration` entry may carry a byte `offset` into
//! the build. An offset is a locator and the excerpt is the evidence, so the check is one
//! comparison: the bytes at the offset ARE the excerpt, or the anchor no longer points at
//! its producer. Three anchors are counted instead of being compared WHOLE - an ELIDED
//! excerpt, which is held to its own head instead (below), and one the ledger itself marks
//! `anchor_status: "absent"` or `"prefix-only"`, the two outcomes recorded when an excerpt
//! cannot be located at all. Those marks are the standing worklist: they are what a later
//! pass re-locates first.
//!
//! AN ELLIPSIS EXCUSES THE TAIL, NEVER THE HEAD. Carrying `...` says nothing about whether
//! an excerpt is a quote: minified JavaScript's spread operator wears the same three dots,
//! and 1,319 `...`-bearing excerpts are byte-exact at their own offsets, which is why the
//! comparison runs FIRST and an ellipsis is never a category that pre-empts it. What is
//! left after it misses really has elided something - and the run BEFORE the first ellipsis
//! is a quote of bytes like any other, so it is held to the offset exactly as `prefix-only`
//! holds a head. Without that the whole bucket was an unverified escape hatch: replacing an
//! already-elided excerpt with wholly fabricated text moved no figure and the gate exited 0.
//!
//! A MARK IS A CLAIM, NOT A PASS. Skipping a marked hop without testing what it asserts
//! would make the mark a way to quiet the rule by typing a word, so every mark is
//! verified against the build too: `absent` means the excerpt is there at NO offset,
//! `prefix-only` means that plus a head that still matches where the offset points.
//!
//! AND "OCCURS NOWHERE" IS SATISFIED BY ENGLISH. An excerpt reading `a string-table copy of
//! the property name in the non-code region` is not a quote of bytes at all - it is a
//! DESCRIPTION of them - so it occurs at no offset in any build, and the mark it carries
//! passes while deciding nothing. That is the dot-run defect in another spelling, so it is
//! refused the same way: an excerpt whose every whitespace-separated token is a plain word,
//! five or more of them, is prose and can never anchor. The refusal reaches an EXCERPT and
//! not only a mark: it used to be asked by `verify_mark` alone, so census prose wearing an
//! ellipsis was counted skipped and anchored nothing. It is asked AFTER the comparison,
//! because a build string really can be an English sentence - an error message, a prompt -
//! and an excerpt that IS the bytes is byte-exact whatever it reads like.
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
//! compares against that one when the host holds it.
//!
//! A BUILD NOBODY HOLDS IS NOT A PASS. Counting such a hop and moving on made the `build`
//! field a way to quiet the rule by typing a version: set an excerpt to garbage, name a
//! build this host cannot read, and the gate goes green. So the count-and-pass posture is
//! reserved for the host that holds NO build at all - a container, a Windows guest, a
//! downstream `cargo test` - where no anchor of any kind can be decided. On a host that
//! holds the VERIFIED build, a hop naming an artifact the host cannot read is UNREAD rather
//! than unreadable, and the rule says so by name: cache that artifact, or re-anchor the hop
//! in a build this host holds. The printed line keeps its `not cached` count, which reads 0
//! wherever the rule passes.
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

mod builds;
mod marks;

pub(crate) use builds::{build_name_ok, build_path, named_build_paths, BuildCache};
pub(crate) use marks::{check_marks, reads_as_prose, verify_mark};

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
    /// An offset beside an empty excerpt, which anchors nothing.
    Empty,
    /// An excerpt carrying a run of four or more dots: a lossy render of bytes that are
    /// not printable, which no build can ever equal.
    DotRun,
    /// An excerpt that reads as PROSE: a description of the bytes rather than a quote of
    /// them, so it can equal no build's bytes and its offset decides nothing.
    Prose,
    /// An elided excerpt whose run BEFORE the first ellipsis is empty, so nothing of it is
    /// a quote at this offset.
    ElidedNoHead,
    /// An elided excerpt whose head does not match at the offset. `at` is the head byte
    /// where the build stops agreeing.
    ElidedHead { at: usize },
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
    // Checked SECOND, and before the eliding and the marks: an offset at or past the end of
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
    // THE COMPARISON COMES FIRST, and the three refusals below are what is left once it
    // has missed. A dot run and a prose sentence are both excerpts that CANNOT equal any
    // build's bytes - but a quote really can carry four dots, and a build string really can
    // be an English sentence (an error message, a prompt), so refusing either before the
    // comparison would fail an anchor that was sound. An excerpt that IS the bytes is
    // byte-exact whatever it reads like.
    if dot_run(excerpt) {
        return Anchor::DotRun;
    }
    if reads_as_prose(excerpt) {
        return Anchor::Prose;
    }
    // AN ELLIPSIS IS AN EXCUSE FOR THE TAIL, NEVER FOR THE HEAD. Minified JavaScript is
    // dense with `...`, so an excerpt carrying one is usually byte-exact anyway and the
    // comparison above has already said so. What is left is an excerpt that elides
    // something - and the run BEFORE the first ellipsis is a quote of bytes like any
    // other, so it is held to the offset exactly as `prefix-only` holds a head. Without
    // that, an elided excerpt was counted skipped and compared against nothing: replacing
    // one with wholly fabricated text moved no figure and the gate exited 0.
    if excerpt.contains("...") || excerpt.contains('\u{2026}') {
        let head = elision_head(excerpt);
        if head.is_empty() {
            return Anchor::ElidedNoHead;
        }
        for want in readings(head) {
            if avail.len() >= want.len() && avail[..want.len()] == want[..] {
                return Anchor::Elided;
            }
        }
        return Anchor::ElidedHead {
            at: avail
                .iter()
                .zip(head.as_bytes())
                .take_while(|(a, b)| a == b)
                .count(),
        };
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

/// The run BEFORE the first ellipsis, in either spelling.
///
/// The whitespace a writer puts in front of ` ... ` belongs to the ELISION rather than to
/// the quote - nobody writes a space to mean a byte there - so it is trimmed, and only the
/// trailing kind, because a leading space in an excerpt is a byte of the build.
pub(crate) fn elision_head(excerpt: &str) -> &str {
    let cut = [excerpt.find("..."), excerpt.find('\u{2026}')]
        .into_iter()
        .flatten()
        .min();
    cut.map_or(excerpt, |i| &excerpt[..i]).trim_end()
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

/// Classify every open state's anchor, push a failure for each one that rotted, print
/// the tally.
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
            // The verified build IS on this host, so nothing about the anchors is
            // undecidable in general: this ONE artifact is missing, and counting the hop
            // would make `build` a way to pass by naming a version nobody holds.
            uncached += 1;
            let looked = named_build_paths(at_build)
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            failures.push(format!(
                "{}: {}[{}] names build `{at_build}`, which this host does not hold (looked \
                 in {looked}) - the verified build is here, so the hop is UNREAD rather than \
                 unreadable: cache that artifact under $CSIFT_LEDGER_BUILDS, or re-anchor \
                 the hop in a build this host holds",
                site.id, site.field, site.index
            ));
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
            Anchor::Prose => failures.push(format!(
                "{}: {}[{}] offset {} carries an excerpt that reads as prose (`{}`) - that is \
                 a DESCRIPTION of the bytes rather than a quote of them, so it equals no \
                 build's bytes and the offset decides nothing; re-cut the excerpt to the run \
                 it describes and move the description into the entry's own prose",
                site.id,
                site.field,
                site.index,
                site.offset,
                site.excerpt.chars().take(80).collect::<String>()
            )),
            Anchor::ElidedNoHead => failures.push(format!(
                "{}: {}[{}] offset {} carries an excerpt that OPENS with its own ellipsis, so \
                 nothing of it is a quote of the bytes at that offset - re-cut it to the run \
                 the offset carries, or drop the offset",
                site.id, site.field, site.index, site.offset
            )),
            Anchor::ElidedHead { at } => failures.push(format!(
                "{}: {}[{}] offset {} no longer carries the run BEFORE its first ellipsis at \
                 Claude Code {at_build} (first difference at head byte {at}) - an ellipsis \
                 excuses the tail of an excerpt and never its head, so re-anchor the offset \
                 or re-cut the excerpt to what the build holds",
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
