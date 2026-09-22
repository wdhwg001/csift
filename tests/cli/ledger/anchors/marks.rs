//! What a MARK asserts, and whether it is true.
//!
//! `absent` says the excerpt is in the build at NO offset; `prefix-only` says that plus a
//! head that still matches where the offset points. Counting a marked hop without testing
//! what it asserts would let any rotted anchor be quieted by typing a word, which is the one
//! move rule 12 exists to catch.
//!
//! TWO ARMS NEED NO BUILD AT ALL, and both are vacuity. An EMPTY excerpt matches at every
//! offset, so a mark on one is true wherever it is read. PROSE matches at none: an excerpt
//! reading `a string-table copy of the property name in the non-code region` describes the
//! bytes rather than quoting them, so `absent` is true of it by construction. Each is
//! refused before the build is consulted, and the remedy for both is the dot run's - re-cut
//! the excerpt to the run it describes.

use super::Site;

/// What a mark asserts about its own EXCERPT, decidable without any build. Both arms are
/// vacuity: an empty excerpt matches everywhere, and prose matches nowhere, so a mark on
/// either decides nothing about where the producer went.
pub(crate) fn mark_shape(excerpt: &str, status: &str) -> Option<String> {
    if excerpt.is_empty() {
        return Some(format!(
            "marked `{status}` with an empty excerpt, which asserts nothing"
        ));
    }
    if reads_as_prose(excerpt) {
        return Some(format!(
            "marked `{status}` with an excerpt that reads as prose (`{}`) - that is a \
             DESCRIPTION of the bytes rather than a quote of them, so it occurs in no build \
             and the mark asserts nothing; re-cut the excerpt to the run it describes",
            excerpt.chars().take(80).collect::<String>()
        ));
    }
    None
}

/// A marked excerpt that is a DESCRIPTION of bytes rather than a quote of them. Every
/// whitespace-separated token is a plain word - letters, with a hyphen or an apostrophe
/// inside and a full stop or comma allowed at the edge - and there are five or more of
/// them. Such a string occurs in no build by construction, so a mark on it passes while
/// asserting nothing; the remedy is the dot-run's, re-cut the excerpt to the run it
/// describes. A single token can never be prose here, whatever it spells, because a
/// dot-joined identifier list is one token and a quoted literal usually is too.
pub(crate) fn reads_as_prose(excerpt: &str) -> bool {
    let mut words = 0usize;
    for token in excerpt.split_whitespace() {
        let core = token.trim_matches(|c| c == '.' || c == ',');
        if core.is_empty()
            || !core
                .chars()
                .all(|c| c.is_ascii_alphabetic() || "-'".contains(c))
        {
            return false;
        }
        words += 1;
    }
    words >= 5
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
    if let Some(why) = mark_shape(excerpt, status) {
        return Some(why);
    }
    let want = excerpt.as_bytes();
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
pub(crate) fn check_marks(
    build: &[u8],
    sites: &[Site<'_>],
    verified: &str,
    failures: &mut Vec<String>,
) {
    let mut marked: Vec<&Site<'_>> = Vec::new();
    for s in sites.iter().filter(|s| s.build.is_none()) {
        let Some(status) = s.status else { continue };
        // The SHAPE half of a mark needs no build - an empty excerpt is vacuous at every
        // offset and a prose one at none - so it is decided here and the hop never reaches
        // the occurrence pass, which would otherwise name it twice.
        if let Some(why) = mark_shape(s.excerpt, status) {
            failures.push(format!(
                "{}: {}[{}] {why} (Claude Code {verified})",
                s.id, s.field, s.index
            ));
            continue;
        }
        marked.push(s);
    }
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
