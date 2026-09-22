//! The introspection-ledger gate: `INTROSPECTION.json` (every Claude Code behavior csift
//! depends on, one claim per entry) must be internally consistent and must back the
//! README's "verified against Claude Code" badge. Runs under `cargo test`, which the
//! pre-commit hook runs; the hook also names this test explicitly.
//!
//! SCHEMA 2. A claim carries its `code[]` sites at CLAIM level - csift's tree is one tree
//! at any time, so where a fact is relied on is a property of the claim - and a `states[]`
//! list carrying the Claude Code-version facts. A state is a version RANGE (`from`, and
//! `to` = null while it is open) plus the claim text for that range, the legs, the
//! provenance and ONE `verified` receipt. A drift closes the old range and opens a new
//! one; the check history that produced a state lives in git, not here.
//!
//! Rules:
//! 1. `verified_claude_code` is a version triple, and the README badge carries the SAME
//!    version (the badge is admitted only through the ledger). It is also asked BEFORE
//!    rule 2: bumping the badge ahead of the audit is the ordinary way to reach this gate,
//!    and rule 2 would then name every claim, so when NO open state carries a receipt at
//!    the verified build the gate says so ONCE, names the build the receipts DO stand at,
//!    and rule 2's version arm stands down while its other arms keep running.
//! 2. Every claim's OPEN state carries a `verified` receipt made AT `verified_claude_code`
//!    with a verdict from the closed set. A CLOSED state's receipt was taken at the build
//!    its range ended on and is not re-made.
//! 3. A receipt's evidence is claim-specific: for `holds` / `refined` / `drifted` the
//!    (instrument, observed) pair of an OPEN state is non-empty and unique across the
//!    ledger (a mechanical bulk replacement leaves identical pairs); `unverifiable-here`
//!    names its reason in `observed`.
//! 4. Every cited code site exists, its snippet is found verbatim (whitespace
//!    normalized) in that file, and it is found INSIDE the declared `lines` range, which
//!    may not be wider than the snippet by more than two lines. So a refactor that moves
//!    the code fails here instead of silently orphaning the claim, and a range cannot be
//!    padded to the whole file to satisfy containment.
//! 5. The README also carries the mutation-score badge (a version-independent shape
//!    check; the score itself is the census's business).
//! 6. Every claim cites at least one code site: a claim with no site names a behavior
//!    csift does not demonstrably depend on, and no pass could ever anchor it.
//! 7. Every STATE states how completely its behavior is ATTRIBUTED (`attribution`, the
//!    closed set end-to-end | producer-only | specimen-only | partial-producer |
//!    by-elimination | upstream) and, unless end-to-end, lists the `open_legs` still to
//!    close; an end-to-end state carries NO open leg (a non-gap note goes to `residue`).
//!    A by-elimination state can never carry a `holds` verdict: with neither leg traced,
//!    "nothing changed" is exactly the conclusion a pass is not entitled to.
//! 8. The README's ledger tally block equals the ledger's per-attribution counts over
//!    OPEN states and carries rule 12's anchor line, so the page states a number a reader
//!    re-derives.
//! 9. No OPEN state's verdict is `drifted`: a drift closes that state's range and opens
//!    the next one, so a ledger whose current word on a claim is "drifted" is not
//!    releasable.
//! 10. No FIELD carries an unconsumed text instruction: a `TEXT ...` line is a claim-text
//!     correction a traced hop demanded, and a line recording a rejected rewrite is the
//!     old, refuted text still shipping; both are consumed by rewriting the text, never
//!     released around. The scan covers `open_legs`, `residue` AND `verified.note` - the
//!     legs alone were the one place the instructions were not.
//! 11. Every state below end-to-end carries both leg fields (`producer_trace`,
//!     `specimen`), so rule 7's derivation always runs: an attribution with no legs
//!     recorded is an opinion, not a derivation.
//! 12. Every byte anchor of an OPEN state still points at its own excerpt in the build it
//!     was read in: each `producer_chain` hop and `enumeration` entry carrying an `offset`
//!     must be byte-exact there, unless it is elided or marked (see `ledger/anchors.rs`).
//!     The build is the verified one unless the hop names its OWN in `build`, which is how
//!     a Windows artifact and an older version stop being compared against a build they
//!     were never read in - and a hop naming an artifact THIS host cannot read is then
//!     UNREAD rather than unreadable, so it is named rather than counted. A MARK IS A
//!     CLAIM, NOT A PASS, so every mark is verified against the build too, and two of its
//!     arms need no build at all: an EMPTY excerpt matches at every offset and PROSE at
//!     none, so a mark on either is true by construction and decides nothing. PROSE is
//!     refused on an UNMARKED excerpt as well, after the comparison has missed: a
//!     description of the bytes equals no build's bytes, and asking it of marks alone left
//!     census prose anchoring nothing. Neither can an excerpt carrying a run of four or
//!     more DOTS anchor - that is a lossy render of non-printable bytes - though both of
//!     those are asked AFTER the byte comparison, because a quote really can carry four
//!     dots and a build string really can be an English sentence. An ELLIPSIS excuses the
//!     TAIL of an excerpt and never its head: the run before the first one is held to the
//!     offset exactly as `prefix-only` holds a head, without which the whole elided bucket
//!     was compared against nothing and fabricated text passed. A
//!     CLOSED state's anchors were exact at the build its range ended on: they are counted
//!     and never compared against a later build. A host with NO build prints one line and
//!     passes, which is the one place counting and moving on is honest.
//! 13. A claim carries one or two states, exactly one of them open AND LAST; every state
//!     names a `from` and a `from_basis` from the closed set, a closed state names its
//!     `to`, and the states are ordered by `from`. The OPEN state's range opens at or below
//!     the verified build, unless `from_after_verified` says why it does not - a behavior
//!     first observed on a NEWER build has a receipt at the badge's build and a range above
//!     it, and that shape is stated rather than assumed.
//! 14. The prose law, as a gate over every text field of every state: no calendar date or
//!     long month name, no csift version number, no audit-process word (plurals and the
//!     agent noun included - a word list that stops at the singular is a list with a hole
//!     in it), no first person. The ledger is a fact record keyed by Claude Code version
//!     only; the commit and the release carry the when and the which-csift. A quoted
//!     `excerpt` is exempt - it reproduces the build's own bytes, where `kicked:!1`,
//!     `"today"` and a model's November release date are the build's words - and so is a
//!     BACKTICKED span, but only where the backticks BALANCE and the span is SHORT: an
//!     unclosed one makes left-to-right pairing exempt prose instead of a literal, and a
//!     2.2 KB run between two backticks is prose rather than a literal anyone quotes.
//! 15. Every state names its behavior, its depends and its instrument, and its `probe`
//!     twin carries a kind from the closed set plus the field that kind is answered by -
//!     a probe with a kind and nothing else is a placeholder wearing a schema.
//! 16. A CLOSED state's receipt is shape-checked too: a build no later than its own `to`,
//!     a verdict from the closed set, a non-empty instrument and observed. It is never
//!     re-made against a later build, but a range nothing ever stood behind is not a range.
//! 17. No field hands a RETIRED selector spelling to csift as an argument. The shipped
//!     binary hard-errors on `harness.meta.*` and the pre-0.5 flat paths, so a probe naming
//!     one cannot run at all; a residue line saying what a leaf USED to be called is
//!     history, which is what a superseded reading is for.
//! 18. The file's own shape: `schema` is 2, the claim list is non-empty, and every claim
//!     carries a unique non-empty `id` and a non-empty `area`.
//! 19. An instrument that carries a PROGRAM carries one that parses: every heredoc in an
//!     `instrument` or a `probe.command` is handed back to its own language (python
//!     `compile`, `bash -n`, `jq -n`). A heredoc's indent is its block structure, so a prose
//!     pass that normalises whitespace can leave every word intact and every program dead,
//!     and no other rule here would see it. An interpreter this host does not hold is
//!     counted and skipped, exactly as rule 12 skips a build it does not hold.
//! 20. An OPEN state's receipt names an ARTEFACT OUTSIDE csift's own tree - a Claude Code
//!     build or a byte offset in one, a corpus or projects root, the harness's own
//!     directories, a session target, a live trial. The artefact is the test and never the
//!     command: a check naming only `src/`, a self-written fixture or this repository's own
//!     documents decided nothing about Claude Code, and a badge resting on one rests on
//!     nothing. The classifier lives in `ledger/artefact.rs`, in the gate rather than only
//!     in the pass that writes the file.
//!
//! 21. A `bytes` probe names a literal the BUILD carries. The probe's whole content is the
//!     literal a later pass re-locates, and four shipped counting zero in every build their
//!     state names - two cut past where the build stops agreeing, two carrying a ` ... `
//!     elision inside the literal, which is not a byte string at all. The build is a SET,
//!     because a state names up to three and a rewrite between two of them is often what the
//!     probe is there to date. A build this host does not hold is counted and skipped.
//! 22. A probe's KIND and its SCOPE name the SAME artefact. The kind follows what the
//!     command reads and the scope is derived from the instrument, and nothing compared the
//!     two: thirty-eight states carried `corpus` beside a scope naming the shipped binary,
//!     and one carried `bytes` beside a scope naming a projects root. Each half was
//!     defensible alone, which is exactly how a pair that contradicts itself ships. Only
//!     the two kinds whose artefact is fixed are tested - `csift`, `live` and `repo` name a
//!     run, a gesture and this tree, any of which may mention either noun without
//!     contradiction.
//!
//! The attribution set carries `upstream`: the producer lies OUTSIDE the shipped binary
//! by construction (the model or API side, the operating system, a native runtime
//! binding) while the client-side treatment is traced and a specimen is observed;
//! `producer_trace: upstream` + `specimen: observed` derives it.

mod anchors;
mod artefact;
mod legs;
mod probes;
mod programs;
mod prose;
mod rules;
mod specimens;
mod states;
mod tally;

use serde_json::Value;
use std::collections::HashMap;

pub(crate) fn repo() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whitespace-insensitive comparison, so a re-indented snippet still anchors.
pub(crate) fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn version_triple(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// A version as a comparable tuple, for rule 13's ordering.
pub(crate) fn version_key(s: &str) -> Option<(u32, u32, u32)> {
    let p: Vec<&str> = s.split('.').collect();
    if p.len() != 3 {
        return None;
    }
    Some((p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?))
}

/// Every state of a claim, in file order.
pub(crate) fn states(claim: &serde_json::Value) -> &[serde_json::Value] {
    claim["states"].as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// The one state whose range is still open. `None` is rule 13's business, not the
/// caller's, so every other rule simply skips a claim with no open state.
pub(crate) fn open_state(claim: &serde_json::Value) -> Option<&serde_json::Value> {
    states(claim).iter().find(|s| s["to"].is_null())
}

/// Rule 18: the file's own shape, as a rule with a name rather than a bare assertion in
/// the driver. `schema` is 2, the claim list is non-empty, and every claim carries a unique
/// non-empty `id` and a non-empty `area`. These were three `assert!`s and an inline `if`,
/// which is exactly how a check ends up with no specimen behind it.
pub(crate) fn file_shape<'a>(doc: &'a Value, failures: &mut Vec<String>) -> Vec<&'a Value> {
    if doc["schema"] != 2 {
        failures.push(format!(
            "ledger `schema` is {}, not 2 - this gate reads schema 2",
            doc["schema"]
        ));
    }
    match doc["verified_claude_code"].as_str() {
        Some(v) if version_triple(v) => {}
        other => failures.push(format!(
            "`verified_claude_code` is {other:?}, not a Claude Code version"
        )),
    }
    let Some(claims) = doc["claims"].as_array() else {
        failures.push("`claims` is not an array".to_string());
        return Vec::new();
    };
    if claims.is_empty() {
        failures.push("the ledger is empty".to_string());
    }
    let mut ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for c in claims {
        let id = c["id"].as_str().unwrap_or("");
        if id.is_empty() {
            failures.push("a claim carries no `id`".to_string());
            continue;
        }
        if !ids.insert(id) {
            failures.push(format!("claim id `{id}` occurs more than once"));
            continue;
        }
        if c["area"].as_str().is_none_or(|s| s.trim().is_empty()) {
            failures.push(format!("{id}: `area` is empty"));
        }
        out.push(c);
    }
    out
}

#[test]
fn ledger_gate_ledger_is_consistent_and_backs_the_readme_badges() {
    let raw = std::fs::read_to_string(repo().join("INTROSPECTION.json"))
        .expect("INTROSPECTION.json must exist at the repo root");
    let doc: serde_json::Value = serde_json::from_str(&raw).expect("INTROSPECTION.json parses");
    let mut failures: Vec<String> = Vec::new();
    let kept = file_shape(&doc, &mut failures);
    let verified = doc["verified_claude_code"].as_str().unwrap_or("");

    let claims = doc["claims"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let mut evidence: HashMap<String, String> = HashMap::new();
    let mut file_cache: HashMap<String, Option<rules::FileText>> = HashMap::new();
    let mut embedded = programs::Programs::default();

    // Rule 1 stands in FRONT of rule 2: a badge bumped ahead of the audit is one fact, and
    // rule 2 would spell it out once per claim.
    let badge_moved = rules::badge_stands(&kept, verified);
    let report_version = badge_moved.is_none();
    if let Some(note) = badge_moved {
        failures.push(note);
    }

    for c in kept {
        let id = c["id"].as_str().unwrap_or("");
        states::state_shape(id, c, verified, &mut failures);
        rules::code_sites(id, c, &mut file_cache, &mut failures);
        rules::receipts(
            id,
            c,
            verified,
            report_version,
            &mut evidence,
            &mut failures,
        );
        for (i, state) in states(c).iter().enumerate() {
            let at = format!("{id} states[{i}]");
            states::state_text(&at, state, &mut failures);
            states::closed_receipt(&at, state, &mut failures);
            rules::attribution(&at, state, &mut failures);
            artefact::open_receipt(&at, state, &mut failures);
            legs::text_instructions(&at, state, &mut failures);
            legs::retired_spellings(&at, state, &mut failures);
            prose::check_state(&at, state, &mut failures);
            programs::check_state(&at, state, &mut embedded, &mut failures);
        }
    }
    println!("{}", programs::line(&embedded));
    println!("{}", probes::line(&probes::check(claims, &mut failures)));
    // Rule 22: a probe's kind and its scope name the same artefact.
    probes::kind_scope(claims, &mut failures);

    // Rule 12: every byte anchor of an open state still carries its own excerpt.
    let anchors = anchors::tally(claims, verified, &mut failures);

    // Rules 1, 5 and 8: the README.
    let readme = std::fs::read_to_string(repo().join("README.md")).expect("README.md");
    tally::badges(verified, &readme, &mut failures);
    tally::ledger_tally(claims, &anchors, verified, &readme, &mut failures);

    assert!(
        failures.is_empty(),
        "INTROSPECTION.json gate failed ({} problem(s)):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
