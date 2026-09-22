//! Rule 20: an open state's receipt names an ARTEFACT OUTSIDE csift's own tree.
//!
//! The receipt is what the badge rests on, so what makes a check one is not how carefully
//! it was written but WHAT IT TOUCHED. The artefact is the test and never the command:
//! `grep` over `src` is the same text pass as reading the file, while `grep -ab` over a
//! shipped build is a measurement. A check naming only csift's own tree - a source path, a
//! self-written fixture, this repository's documents, the claim's own sentence - decides
//! nothing about Claude Code, and a ledger whose receipt is one of those is a badge with
//! nothing behind it.
//!
//! The classifier lives HERE, in the gate, rather than only in the pass that built the
//! file: a rule that is only ever applied by the thing it grades is not a rule. Three
//! tables and one lead test, and each miss the shape teaches is a word the artefact was
//! spelled with rather than a new kind of artefact - `shipped 2.1.258 build` names what
//! `shipped build` names, and a pattern demanding the two words be adjacent reads it as
//! prose. That is why the build and live arms carry one token of slack.
//!
//! TWO ARTEFACTS WERE MISSING FROM THE TABLE, and both are the commonest instruments in the
//! file. A csift INVOCATION reads transcripts off disk, so `csift files <project> --by
//! timeline` is a measurement - but the table only ever reached one through the `@` target
//! shapes, so a run pointed at a project directory or a named transcript read as prose. And
//! a count over the WHOLE BUILD is the build arm under another spelling: `the whole-build
//! count of ...` says what `over the shipped binary` says. Both arms moved a receipt, which
//! is the only way the miss shows: the newest measurement at the verified build was passed
//! over for an older one.

use serde_json::Value;

/// ONE token of slack between the words of an artefact's name. It may not carry a full
/// stop, so it can never reach across a sentence into an artefact the check never touched.
const GAP: &str = r"(?:[\w.+/-]+ )?";

/// A build named by its PATH rather than by an English word for it.
///
/// The table read `binary`, `executable`, `the build` and the product-plus-version forms,
/// and a check that simply gave the path it opened named none of them: `grep -c "literal"
/// <cache>/2.1.258/claude` reads the shipped build and was classified a text pass. Three
/// shapes, all of them generic: a path whose last component is the executable's own name, a
/// path through a `versions/<triple>` directory (the native install layout), and one through
/// a `builds/<name>` directory (a cache of them). No machine's own path is spelled here.
const BUILD_PATH: &str = concat!(
    r"[\w.~$/{}<>-]*/claude\b",
    r"|\bversions/[12]\.\d{1,3}\.\d{1,3}",
    r"|\bbuilds/[\w.+-]+",
);

/// A BYTE OFFSET written without the `@` sigil. 23 base checks across the messaging family
/// say `at 158550335`, which is the same locator the table reads as `@158550335`; reading
/// only the sigil form passed over the newest measurement at the verified build on one of
/// them. The digit run is eight or nine, which is what an offset into this build is, and it
/// is anchored on the word that makes it a location rather than a count.
const BARE_OFFSET: &str = r"\b(?:at|offset|offsets)\s+\d{8,9}\b";

/// A csift INVOCATION, pointed at something. csift reads transcripts off disk, so the run is
/// a measurement - but the TARGET is what makes it one: `csift search` inside a sentence
/// about what the command does is a noun, while `csift files <project> --by timeline` walked
/// a project directory. So the arm is a subcommand plus a target shape within the same
/// clause - an `@` token, an angle-bracket placeholder, or a named transcript - and the
/// window may not carry a full stop, exactly as `GAP` may not.
const CSIFT_RUN: &str = concat!(
    r"\bcsift (?:list|search|show|stats|agents|whoami|files|recover|plan|verbatim|image",
    r"|status|wait|send|msg|ack|deliver)\b",
    r"[^.\n]{0,60}(?:@[\w:.-]+|<[a-z][a-z-]*>|[\w/-]+\.jsonl)",
);

fn outside() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        let pattern = [
            // the shipped build and its bytes
            r"\bbinar(?:y|ies)\b|\bexecutable\b|whole-executable|\bwhole-build\b",
            &format!(
                r"(?:shipped|cached|historical|native|verified|current|win32|windows) {GAP}builds?\b"
            ),
            r"builds? (?:at|of|from|carrying|lacking|named)\b|\bthe build\b|\bhistorical builds\b",
            // the build named by product and version, whatever noun the sentence chose
            r"\bClaude Code [12]\.\d{1,3}\.\d{1,3} (?:file|build|binary|executable|artifact)\b",
            r"@\d{6,}|\bbyte offsets?\b|\bstrings -|\bgrep -ab\b|\bbyte-search|byte-exact",
            BUILD_PATH,
            BARE_OFFSET,
            // the corpus
            r"\bcorpus\b|corpus-wide|whole-corpus|projects roots?\b|projects director(?:y|ies)\b",
            r"\bjsonl\b|\.jsonl|\btranscripts?\b|\bcensus(?:ed|es)?\b|\bon disk\b|~/\.claude",
            // the harness's own state, spelled as a path or as English
            r"<config-home>|<claude-home>|\bsessions/|\btasks/|\bregistry\b|\bmeta\.json\b",
            r"\bsessions director(?:y|ies)\b|\btasks director(?:y|ies)\b",
            // a session target handed to csift, and a csift run pointed at any target
            r"@trap:|@main\b|@[0-9a-f]{8}",
            CSIFT_RUN,
            // a live trial, including one named by its platform rather than by the word
            &format!(
                r"\blive {GAP}(?:trial|probe|session|arm|run|poll|pair|A/B|check|half|root|corpus|lane)\b"
            ),
            r"\bsampler\b|\bre-poll\b|\bwindows (?:host|guest|trial|11|10)\b|\bQEMU\b",
            r"\bWindows\b[^.\n]{0,24}\bhost\b",
        ]
        .join("|");
        regex::RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
            .expect("the artefact table compiles")
    })
}

/// A SELF-WRITTEN FIXTURE is never a specimen, so an instrument whose only artefact is one
/// is a text pass even where it names a transcript.
fn fixture_only() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::RegexBuilder::new(
            &[
                r"synthetic[^.]{0,60}(?:fixture|transcript|tree|file)",
                r"hand-(?:built|written)[^.]{0,40}(?:fixture|transcript)",
                r"fixture (?:tree|file|transcript|home)",
                r"the harness(?:'s)? own fixture",
            ]
            .join("|"),
        )
        .case_insensitive(true)
        .build()
        .expect("the fixture table compiles")
    })
}

/// ... and these are the artefacts a fixture cannot be.
///
/// It is a SUPERSET of `outside`'s build and offset arms, and that is load-bearing rather
/// than tidy: the `fixture_only` veto only stands down when a real artefact is also named,
/// so an arm missing here OVERRIDES an arm present there. A check that read the shipped
/// binary twice was refused because a later clause mentioned a fixture home, which is the
/// veto doing the opposite of its job.
fn real_artefact() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        let pattern = [
            r"@\d{6,}|\bcorpus\b|projects roots?\b|\bbinar(?:y|ies)\b|\bcensus|\bexecutable\b",
            r"\bwhole-build\b|\bstrings -|\bgrep -ab\b|\bbyte-search|byte-exact",
            r"\bbyte offsets?\b|\bthe build\b|\bhistorical builds\b",
            r"builds? (?:at|of|from|carrying|lacking|named)\b",
            &format!(r"(?:shipped|cached|historical|native|verified|current|win32|windows) {GAP}builds?\b"),
            BUILD_PATH,
            BARE_OFFSET,
            r"~/\.claude",
            r"\bClaude Code [12]\.\d{1,3}\.\d{1,3} (?:file|build|binary|executable|artifact)\b",
            &format!(r"\blive {GAP}(?:trial|session|run)\b"),
        ]
        .join("|");
        regex::RegexBuilder::new(&pattern)
            .case_insensitive(true)
            .build()
            .expect("the real-artefact table compiles")
    })
}

/// A check that DECLARES itself a text pass is one whatever it goes on to mention: the
/// sentence it re-read names the corpus the earlier measurement used, and a word test reads
/// those names as if this check had run them.
fn text_lead() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::RegexBuilder::new(&format!(
            "{}{}",
            r"^\s*(?:(?:a |the )?(?:first|second|third|fourth|fifth|final)[ -]?)?",
            [
                r"(?:text (?:pass|correction)\b",
                r"re-?read of the claim(?:'s)?(?: own)? text",
                r"re-?read of (?:every|each|the) (?:csift )?(?:site|sentence|claim)",
                r"name check over",
                r"prose (?:pass|sweep)\b",
                r"wording (?:pass|correction)\b)",
            ]
            .join("|")
        ))
        .case_insensitive(true)
        .build()
        .expect("the text-lead table compiles")
    })
}

/// Does this instrument name an artefact outside csift's own tree?
pub(crate) fn names_an_artefact(instrument: &str) -> bool {
    if !outside().is_match(instrument) {
        return false;
    }
    if text_lead().is_match(instrument) {
        return false;
    }
    if fixture_only().is_match(instrument) && !real_artefact().is_match(instrument) {
        return false;
    }
    true
}

/// Rule 20 over one claim's OPEN state.
pub(crate) fn open_receipt(at: &str, state: &Value, failures: &mut Vec<String>) {
    if !state["to"].is_null() {
        return; // a closed state's receipt was taken at its own build; rule 16 shapes it
    }
    let instrument = state["verified"]["instrument"].as_str().unwrap_or("");
    if instrument.trim().is_empty() {
        return; // rule 3 already names an empty receipt
    }
    if !names_an_artefact(instrument) {
        failures.push(format!(
            "{at}: the open state's receipt names no artefact outside csift's own tree, so it \
             is a TEXT PASS and cannot back the badge - `{}`",
            instrument.chars().take(160).collect::<String>()
        ));
    }
}
