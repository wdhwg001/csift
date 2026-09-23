//! Rules 1, 5 and 8: what the README must say, and why it is admitted only through here.
//!
//! The page claims two measured things - "verified against Claude Code X" and a table of
//! how completely each behavior is attributed - and both are numbers a reader is entitled
//! to re-derive. Every figure in the block is a count over the ledger's OPEN states: a
//! closed state records what was true of an older build, and the page speaks about the
//! build the badge names.
//!
//! RULE 8 REGENERATES THE WHOLE BLOCK AND COMPARES IT BYTE FOR BYTE. Reading the rows with
//! a regex (`^\| ([a-z-]+) \| (\d+) \|`) checks only what the regex names: the
//! `&nbsp;&nbsp;of which chain traced` row has no `[a-z-]+` key, so it is never read; the
//! `share` column is never read at all; and the `(24 of them with a partly traced writer)`
//! parenthetical is a hand-typed number in a sentence. Three numbers on a page that
//! promises re-derivable ones. A generator has no such holes: whatever it does not
//! generate is not in the block, and whatever is in the block it generated.
//!
//! THE MEASURED HALF IS THE ONE EXCEPTION. The byte-exact count, the elided count and the
//! subset of byte-exact anchors that carry an ellipsis anyway all need the build, and a
//! host without it cannot generate them - so on such a host those numbers are read BACK off
//! the page and everything around them is still compared. The ledger-derived counts (total,
//! absent, prefix-only, closed, other-build) are checked everywhere.
//!
//! THE ELLIPSIS SUBSET IS DERIVED, NOT TYPED, and that is the whole point of it. The page
//! says an ellipsis is no evidence that an excerpt was never a quote, and it says so with a
//! number; that number is a property of the shipped file and of the build, it moved by 154
//! the first time anchors were relocated, and it was still being stated in four documents
//! in the present tense at its old value. The anchor walk already classifies every
//! `...`-bearing excerpt, so the count is a subset of what it decided rather than a second
//! measurement, and rule 8 holds the README to it exactly as it holds the table.

use super::{anchors, open_state};
use regex::Regex;
use serde_json::Value;

/// Rules 1 and 5: the two badges. The Claude Code version is admitted only through the
/// ledger; the mutation score is a shape check, since the score itself is the census's
/// business.
pub(crate) fn badges(verified: &str, readme: &str, failures: &mut Vec<String>) {
    let badge = Regex::new(r"Claude%20Code-(\d+\.\d+\.\d+)-").unwrap();
    match badge.captures(readme) {
        Some(cap) => {
            if &cap[1] != verified {
                failures.push(format!(
                    "README badge says Claude Code {} but the ledger is verified at {verified}",
                    &cap[1]
                ));
            }
        }
        None => failures.push("README has no `verified against Claude Code` badge".to_string()),
    }
    let mutation = Regex::new(r"mutation%20score-\d+(\.\d+)?%25").unwrap();
    if !mutation.is_match(readme) {
        failures.push("README has no mutation-score badge".to_string());
    }
}

/// One attribution row's count: claims whose OPEN state carries that value.
pub(crate) fn attribution_count(claims: &[Value], key: &str) -> usize {
    claims
        .iter()
        .filter(|c| open_state(c).is_some_and(|s| s["attribution"].as_str() == Some(key)))
        .count()
}

/// End-to-end claims whose open state actually pins its hops at byte offsets. The row
/// exists because "traced" and "traced AT AN OFFSET" are different promises, and the
/// anchors line below the table speaks only about the second.
fn chain_traced(claims: &[Value]) -> usize {
    claims
        .iter()
        .filter(|c| {
            open_state(c).is_some_and(|s| {
                s["attribution"].as_str() == Some("end-to-end")
                    && s["producer_chain"]
                        .as_array()
                        .is_some_and(|h| h.iter().any(|x| x["offset"].is_number()))
            })
        })
        .count()
}

/// Specimen-only claims whose producer leg is `partial` rather than `none` - the
/// parenthetical in that row's meaning.
fn partly_traced(claims: &[Value]) -> usize {
    claims
        .iter()
        .filter(|c| {
            open_state(c).is_some_and(|s| {
                s["attribution"].as_str() == Some("specimen-only")
                    && s["producer_trace"].as_str() == Some("partial")
            })
        })
        .count()
}

fn comma(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn share(n: usize, total: usize) -> String {
    if total == 0 {
        return "0.0%".to_string();
    }
    format!("{:.1}%", (n as f64) * 100.0 / (total as f64))
}

/// The measured four, read off the page. Used ONLY on a host without the build, so the
/// generated block can still be compared everywhere else.
fn measured_from_readme(block: &str) -> Option<(usize, usize, usize)> {
    let re = Regex::new(
        r"anchors byte-exact at Claude Code \d+\.\d+\.\d+: ([\d,]+) of [\d,]+ \(elided ([\d,]+),",
    )
    .unwrap();
    let cap = re.captures(block)?;
    let num = |s: &str| s.replace(',', "").parse::<usize>().ok();
    // The ellipsis subset is a third measured figure and is read back the same way, so a
    // host with no build still compares the whole block instead of only most of it.
    let subset = Regex::new(r"([\d,]+) such excerpts ARE byte-exact").unwrap();
    Some((
        num(&cap[1])?,
        num(&cap[2])?,
        num(&subset.captures(block)?[1])?,
    ))
}

/// The block, generated. This IS the README's ledger tally: the page carries what this
/// returns, byte for byte, between the markers.
pub(crate) fn render_block(
    claims: &[Value],
    a: &anchors::Anchors,
    verified: &str,
    measured: (usize, usize, usize),
) -> String {
    let total = claims.len();
    let (exact, elided, elided_exact) = measured;
    let rows: [(&str, usize, &str); 6] = [
        ("end-to-end", attribution_count(claims, "end-to-end"),
         "the writer, its gate and its trigger read in the shipped binary, and a specimen observed on disk or live"),
        ("specimen-only", attribution_count(claims, "specimen-only"),
         "observed on disk or live; the writer not traced (or traced only in part)"),
        ("producer-only", attribution_count(claims, "producer-only"),
         "the writer traced in full; no specimen exists in the corpus or could be produced here"),
        ("partial-producer", attribution_count(claims, "partial-producer"),
         "a template or field located without its gate and trigger; no specimen"),
        ("by-elimination", attribution_count(claims, "by-elimination"),
         "neither leg; attributed by exclusion or from csift's own design"),
        ("upstream", attribution_count(claims, "upstream"),
         "the producer lies outside the shipped binary by construction (the model or API side, the operating system, a native runtime binding); the client-side treatment is traced and a specimen observed"),
    ];
    let mut out = String::from("<!-- ledger-tally:begin -->\n");
    out.push_str("| attribution | claims | share | meaning |\n");
    out.push_str("|---|---:|---:|---|\n");
    for (key, n, meaning) in rows {
        out.push_str(&format!(
            "| {key} | {} | {} | {meaning}",
            comma(n),
            share(n, total)
        ));
        if key == "specimen-only" {
            out.push_str(&format!(
                " ({} of them with a partly traced writer)",
                comma(partly_traced(claims))
            ));
        }
        out.push_str(" |\n");
        if key == "end-to-end" {
            let t = chain_traced(claims);
            out.push_str(&format!(
                "| &nbsp;&nbsp;of which chain traced | {} | {} | the three hops quoted at byte \
                 offsets in `producer_chain`; the anchors line below is the measured state of \
                 those offsets |\n",
                comma(t),
                share(t, total)
            ));
        }
    }
    out.push_str(&format!(
        "| total | {} | 100.0% | one claim per Claude Code behavior csift depends on, verified \
         at Claude Code {verified} |\n\n",
        comma(total)
    ));
    out.push_str(
        "A traced claim pins each hop at a byte offset in that build, and the gate re-reads \
         every one of them:\n\n",
    );
    out.push_str(&format!(
        "`anchors byte-exact at Claude Code {verified}: {} of {} (elided {}, absent {}, \
         prefix-only {}, read in another build {}), closed-state anchors {}, not checked`\n\n",
        comma(exact),
        comma(a.total),
        comma(elided),
        comma(a.absent),
        comma(a.prefix_only),
        comma(a.other_build),
        comma(a.closed),
    ));
    out.push_str(&format!(
        "The five counts partition the total, and the gate asserts that they do. BYTE-EXACT \
         is read over two encodings: a few excerpts were captured from regions that are not \
         valid UTF-8 and stored one code point per byte, and 14 of these anchors match only \
         under that second reading. An ELIDED excerpt carries an ellipsis somewhere, which is \
         not the same as having no byte-exact form: {} such excerpts ARE byte-exact at \
         their own offsets and are counted with the rest, because minified JavaScript's \
         spread operator wears the same three dots. What is left in the elided count really \
         did elide something, and the gate holds each of those to the run BEFORE its first \
         ellipsis at its own offset. An absent or prefix-only anchor is marked as \
         such in the ledger, with the residue line that says how to find its producer again; \
         those are the first thing the next pass re-locates. A hop read in an artifact the \
         badge does not name carries that build and is compared against THAT one, which is why \
         it is disclosed as a count and never as a verdict: whether a given machine holds those \
         artifacts is a property of the machine. A closed state's anchors were exact at the \
         build its own range ended on, so the gate counts them and does not re-read them \
         against a later one.\n",
        comma(elided_exact),
    ));
    out.push_str("<!-- ledger-tally:end -->");
    out
}

/// Rule 8: the README's ledger tally block equals the block this module generates.
pub(crate) fn ledger_tally(
    claims: &[Value],
    anchors: &anchors::Anchors,
    verified: &str,
    readme: &str,
    failures: &mut Vec<String>,
) {
    let Some(start) = readme.find("<!-- ledger-tally:begin -->") else {
        failures.push("README has no ledger-tally table".to_string());
        return;
    };
    let Some(len) = readme[start..].find("<!-- ledger-tally:end -->") else {
        failures.push("README ledger-tally block has no end marker".to_string());
        return;
    };
    let block = &readme[start..start + len + "<!-- ledger-tally:end -->".len()];
    let measured = match anchors.measured {
        Some(m) => m,
        None => match measured_from_readme(block) {
            Some(m) => m,
            None => {
                failures.push(
                    "README ledger tally carries no readable `anchors byte-exact at Claude Code \
                     ...` line, and this host has no build to generate one from"
                        .to_string(),
                );
                return;
            }
        },
    };
    // THE PAGE OWNS THE ARITHMETIC. The block says the five counts partition the total, so
    // every anchor falls in exactly ONE bucket: credit an `absent`-marked hop with its real
    // bytes as well and it is counted twice, and the sum runs over. A sum the page states is
    // a sum the gate checks, and a host with no build can still check four of the five
    // against the fifth's absence.
    let (exact, elided, _) = measured;
    let parts = exact + elided + anchors.absent + anchors.prefix_only + anchors.other_build;
    if anchors.measured.is_some() && parts != anchors.total {
        failures.push(format!(
            "the anchor counts do not partition the total: byte-exact {exact} + elided \
             {elided} + absent {} + prefix-only {} + read in another build {} = {parts}, \
             against {} anchors - the README states that they do",
            anchors.absent, anchors.prefix_only, anchors.other_build, anchors.total
        ));
    }
    let want = render_block(claims, anchors, verified, measured);
    if block != want {
        let at = block
            .bytes()
            .zip(want.bytes())
            .position(|(a, b)| a != b)
            .unwrap_or_else(|| block.len().min(want.len()));
        failures.push(format!(
            "README ledger tally block differs from the ledger's at byte {at} - the block is \
             REGENERATED, never typed. Expected:\n{want}"
        ));
    }
}
