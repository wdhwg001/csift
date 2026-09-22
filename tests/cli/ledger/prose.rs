//! Rule 14: the prose law, as a gate.
//!
//! The ledger is a clean fact record keyed by Claude Code version only, written for a
//! machine reader. A count is stated with its counting rule and its scope, a version fact
//! as a Claude Code version, and an open leg names only the instrument that closes it.
//! The commit and the release carry the when and the which-csift; the ledger carries the
//! what. Three families are barred, and each is barred because it can only be the process
//! that produced the sentence rather than the fact the sentence states:
//!
//!   * a CALENDAR DATE - a corpus census is reproducible from its counting rule and its
//!     scope, never from the day it ran;
//!   * a CSIFT VERSION - which csift build read a fact is the commit's business;
//!   * an AUDIT-PROCESS WORD - `verifier`, `tracer`, `today`, `kicked` and their kin
//!     describe who looked, which is never part of what Claude Code does;
//!   * FIRST PERSON - a sentence with a person as its subject is about the reading rather
//!     than about the build.
//!
//! WHAT IS NOT BARRED, and why the list is shorter than the law reads. `session`, `lane`
//! and `rejected` are csift's own domain vocabulary - a top-level session, a frozen lane,
//! a rejected tool use - so banning the token would destroy meaning rather than process
//! narrative; those words are barred in their audit SENSE, which no regex decides.
//!
//! The first-person family is MECHANICAL but narrow, and the narrowness is measured. These
//! strings quote minified JavaScript where `we`, `We` and `wE` are ordinary identifier
//! names (`let we=fe.sourceToolAssistantUUID`, `function wE(e,n,r,o,d)`, `origin:we`), so a
//! case-folded `\bwe\b` over every string value of the shipped ledger reads 746 hits and
//! exactly 2 of them have whitespace on both sides - both inside a `code[].snippet`, which
//! quotes csift's own source and which this rule does not scan. Counting rule: one hit per
//! non-overlapping match, over every string value of `INTROSPECTION.json`, whitespace read
//! as the Unicode property. So the arms are case-SENSITIVE and anchored on whitespace both
//! sides: a pronoun is a standalone English word, while an identifier sits against `=`,
//! `(`, `.`, `:` or `;`.
//!
//! A BACKTICKED span is exempt, for the reason an `excerpt` is: it quotes the build or a
//! command verbatim, and Claude Code really does intern a beta named
//! `server-side-fallback-2026-06-01`. THREE conditions bound that exemption. The backticks
//! must BALANCE - an odd count means one span is unclosed, and pairing left to right from
//! there exempts prose instead of a literal, which is how a date and a first-person clause
//! once hid behind an exemption they had no claim to. The span must be SHORT: a run of
//! more than `BACKTICK_MAX` characters between two backticks is not a literal anyone
//! quotes, it is prose that happens to sit between them, and one such run of 2.2 KB shipped
//! a bare `today`. And it must not READ as prose: 400 characters is a lot of English, so a
//! sentence carrying a date, a process word and a first-person clause fits inside the bound
//! comfortably and was exempt for sitting between two backticks. A literal is not made of
//! words, so the test is whether plain words carry the span (`span_is_prose`).
//! `probe.literal` is exempt for the same reason a quotation is. Everything a pass WROTE is
//! scanned.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

/// The BOUNDARIES are the rule, and both of them are decisions.
///
/// The trailing `\b` fails against the `T` of an ISO-8601 instant, so `2026-09-01T06:33:25Z`
/// is not a hit - and that is right rather than lucky: a record's own timestamp, a build
/// date, the instant a snapshot was taken are FACTS the ledger states, while a bare
/// `2026-09-04` beside a count is the day a pass ran.
///
/// A PARTIAL date (`2026-09`, a year and a month) is deliberately NOT barred, and the reason
/// is measured rather than assumed - with BOTH halves of the instrument named, because the
/// count moves with each. Over the field set `prose_fields` walks, with THIS rule's own
/// backtick masking applied, 74 such spans occur: 51 are the head of a full ISO-8601 instant,
/// and the other 23 are readings of a census keyed by MONTH - `0 of 5,415 (2026-05)` beside
/// its count, and the per-month tables of FH-002 and FH-017, which hold all 23 between them -
/// which is a fact with a counting rule behind it. NONE is the day a pass ran. Without the
/// masking the same walk reads 83: nine more ISO heads, and the identical 23. So an arm here
/// would fire on 74 facts to catch nothing: a date that IS the day a pass ran is a claim to
/// rewrite by hand, which is what the one such span got - and why this reading finds none.
fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap())
}

/// A date that IS the fact rather than the stamp of a reading. `100-nanosecond ticks since
/// 1601-01-01` names the FILETIME epoch and `0001-01-01` is the .NET zero instant a Windows
/// probe prints; deleting either removes the very thing the sentence states. Two shapes,
/// both decidable on the text: a year before 2000 is no stamp of any reading in this file,
/// and a date governed by `epoch`, `ticks`, `FILETIME` or `asctime` within a short window is
/// the thing being described.
fn date_is_a_fact(s: &str, start: usize, end: usize) -> bool {
    if !s[start..end].starts_with("20") {
        return true;
    }
    let lo = start.saturating_sub(60);
    let hi = (end + 60).min(s.len());
    let window = format!("{}{}", &s[lo..start], &s[end..hi]).to_ascii_lowercase();
    ["epoch", "ticks", "filetime", "asctime"]
        .iter()
        .any(|w| window.contains(w))
}

/// Long month names only. `May` is a month and an everyday verb, and a three-letter
/// abbreviation collides with minified identifiers, so neither is worth a false positive.
fn month_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(January|February|March|April|June|July|August|September|October|November|December)\b",
        )
        .unwrap()
    })
}

/// csift is pre-1.0, so its versions are the only `0.x.y` in play; a Claude Code version
/// is `1.x.y` or `2.x.y` and never matches. The neighbour classes stand in for the
/// look-around the regex crate does not have.
fn csift_version_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // The TRAILING class has a third arm, and it is the one a sentence ends on: a version
    // followed by a full stop that is not part of a longer number. `[^\w.]` alone refused
    // the `.`, `$` could not see past it, and `0.10.0.` at the end of a sentence shipped.
    RE.get_or_init(|| {
        Regex::new(r"(?:^|[^\w.])(v?0\.\d{1,2}\.\d{1,3})(?:[^\w.]|\.(?:\s|$)|$)").unwrap()
    })
}

/// The PLURALS and the agent noun are in the alphabet, because a word list that stops at
/// the singular is a list with a hole in it: `adjudications` shipped 67 times in ONE bulk
/// receipt sentence and `auditor` four times, while `adjudication` and `audit` were both
/// already barred. So are the two phrases a graded re-read leaves behind - `planted fake`
/// and `adversarial` - which name how a sentence was checked and never what a build does.
///
/// `auditing` and `attempted` join them, and the split there is by SHAPE. `attempted` can
/// only be the pass describing what it tried; `attempt` and `attempts` are a countable noun
/// this domain really uses - `one discovery attempt per poll` is a counting rule - so the
/// bare noun stays and only that one inflected form is barred. `attempting` is NOT barred,
/// and the reason is measured: the build's own edit-validator message says `Read it again
/// before attempting to write it`, so barring it would flag Claude Code's words.
fn audit_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(re-audits|re-audited|re-audit|verifiers|verifier|verify lane|auditors|auditor|auditing|tracers|tracer|scribes|scribe|adjudications|adjudication|adjudicated|adjudicate|kicked|today|yesterday|planted fakes|planted fake|planted|adversarial|devil's advocate|blind review|audits|audited|audit|attempted|rounds|round)\b",
        )
        .unwrap()
    })
}

/// A backticked span is single-line and at most this long. See the module doc: a longer run
/// is prose between two backticks, not a literal, and it is scanned as prose.
const BACKTICK_MAX: usize = 400;

fn backticked_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r"`[^`\n]{{0,{BACKTICK_MAX}}}`")).unwrap())
}

/// A backticked span that is PROSE rather than a literal, decided on its shape rather than
/// on its length alone.
///
/// The length bound was the only test, and 400 characters is a lot of English: a sentence
/// carrying a date, a process word and a first-person clause fits inside it comfortably and
/// was exempt for sitting between two backticks. A literal is not made of words - a command
/// is flags and paths, an excerpt is minified syntax, an output sample is columns, numbers
/// and paths - so the test is whether PLAIN WORDS carry the span: five or more of them AND
/// more than half its whitespace tokens.
///
/// Both halves are measured over the shipped file. Five or more alone scans 987 spans and
/// reads ONE violation, and that one is a false positive: a quoted sample of csift's own
/// output line, half of it columns and paths, whose date is a record's instant rather than
/// the day a pass ran. Adding the majority scans 919 and reads none, which is what a
/// literal's shape is for.
fn span_is_prose(inner: &str) -> bool {
    let (mut plain, mut total) = (0usize, 0usize);
    for token in inner.split_whitespace() {
        total += 1;
        let core = token.trim_matches(|c| c == '.' || c == ',');
        if !core.is_empty()
            && core
                .chars()
                .all(|c| c.is_ascii_alphabetic() || "-'".contains(c))
        {
            plain += 1;
        }
    }
    plain >= 5 && plain * 2 > total
}

/// `round` is an audit-process word - "the previous round", "round three of the attribution
/// push" - and it is also csift's own: a value that survives a serialize ROUND-TRIPS, and a
/// probe's arithmetic ROUNDS a ratio. Those two are decided by SHAPE, on what follows the
/// token: `-trip` and ` trip` are the hyphenated and open spellings of the same compound,
/// which one sentence writes either way, and `(` is the call. Everything else is the pass
/// that produced the sentence, whatever noun it wears.
fn round_is_csifts(s: &str, end: usize) -> bool {
    let rest = &s[end..];
    if rest.starts_with('(') {
        return true;
    }
    let head = rest
        .chars()
        .take(5)
        .collect::<String>()
        .to_ascii_lowercase();
    head == "-trip" || head == " trip"
}

/// FIRST PERSON, case-sensitive and whitespace-anchored. See the module doc for why: `we`
/// is a real identifier name in the shipped bundle, so the test is whether the token stands
/// alone as an English word rather than whether the letters occur.
///
/// THE VERB IS NOT ENUMERABLE. The arm used to list the verbs it knew - `am`, `was`, `did`,
/// anything ending in `ed` - and English is not a closed set, so `I then`, `I caught`,
/// `I actually`, `I made`, `I now` and `I declare` all shipped. `I` plus ANY word closes
/// that, and the cost is measured: six spans where `I` is a minified binding name rather
/// than a pronoun, which take the backticks a quote owes and which the exemption then
/// masks - the same remedy this rule's own note already records for `we`.
///
/// THE BOUNDARY ADMITS A CLOSING QUOTE. `us` needed whitespace or a mark after it, so
/// `not visible to us'.` was invisible for the apostrophe. A pronoun ends at the end of a
/// quoted phrase as readily as at a full stop, and `my`, `we`, `our` and `us` are bare
/// tokens now rather than a pronoun plus a word they need not govern.
fn first_person_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?:^|\s)(I [A-Za-z][A-Za-z'-]*|I'(?:m|ve|ll|d)|[Mm]y|[Ww]e|[Oo]ur|us)(?:[\s,.;:)\]'"`]|$)"#,
        )
        .unwrap()
    })
}

/// csift's own surface spells the flag `--audit`, the module `src/commands/plan/audit.rs`
/// and the mode `plan --audit`, and what makes those spellings csift's is their SHAPE: a
/// flag, a path, a source file. The older guard spared any match ADJACENT to `.`, `/` or
/// `-`, which spared `kicked-back`, `re-verifier`, `outside the audit.` and `re-run today.`:
/// four process words shipping because a hyphen or a full stop sat beside them. So the
/// whole non-space run around the match is read, and only these three shapes are exempt.
fn csift_spelling(s: &str, start: usize, end: usize) -> bool {
    let a = s[..start]
        .rfind(char::is_whitespace)
        .map_or(0, |i| i + s[i..].chars().next().map_or(1, char::len_utf8));
    let b = s[end..]
        .find(char::is_whitespace)
        .map_or(s.len(), |i| end + i);
    let run = s[a..b].trim_matches(|c: char| "`'\",;:()[].".contains(c));
    run.starts_with("--") && run[2..].chars().all(|c| c.is_alphanumeric() || c == '-')
        || run.contains('/')
            && run
                .chars()
                .all(|c| c.is_alphanumeric() || "./:@-_".contains(c))
        || run.ends_with(".rs")
        || run.split(':').next().is_some_and(|h| h.ends_with(".rs"))
}

/// Every prose-law violation in one string, as (rule name, the matched text).
pub(crate) fn hits(s: &str) -> Vec<(&'static str, String)> {
    // Blank the quotations, keeping the offsets, and only where the quoting is balanced.
    let scanned = if s.matches('`').count() % 2 == 0 {
        backticked_re()
            .replace_all(s, |c: &regex::Captures| {
                let whole = &c[0];
                if span_is_prose(&whole[1..whole.len() - 1]) {
                    whole.to_string()
                } else {
                    " ".repeat(whole.len())
                }
            })
            .into_owned()
    } else {
        s.to_string()
    };
    let s = scanned.as_str();
    let mut out = Vec::new();
    for m in date_re().find_iter(s) {
        if !date_is_a_fact(s, m.start(), m.end()) {
            out.push(("calendar date", m.as_str().to_string()));
        }
    }
    for m in month_re().find_iter(s) {
        out.push(("month name", m.as_str().to_string()));
    }
    for c in csift_version_re().captures_iter(s) {
        if let Some(m) = c.get(1) {
            out.push(("csift version", m.as_str().to_string()));
        }
    }
    for c in audit_word_re().captures_iter(s) {
        let Some(m) = c.get(1) else { continue };
        let spared = csift_spelling(s, m.start(), m.end())
            || (m.as_str().eq_ignore_ascii_case("round") && round_is_csifts(s, m.end()));
        if !spared {
            out.push(("audit-process word", m.as_str().to_string()));
        }
    }
    for c in first_person_re().captures_iter(s) {
        if let Some(m) = c.get(1) {
            out.push(("first-person phrase", m.as_str().to_string()));
        }
    }
    out
}

/// Every field of a state a PASS wrote, as (path, text). THREE kinds are deliberately
/// absent, and all three are quotations rather than prose: an `excerpt`, a `probe.literal`,
/// and every member of `writer_literal_floor` - its `literal` is a build's own bytes and its
/// `lowest_present`, `highest_absent` and `ladder` are the versions that literal was counted
/// across, which is a measurement written in Claude Code versions and in nothing else. The
/// law's own sentence says every text field of every state; this is the list of what a pass
/// WROTE, which is what the law is about.
fn prose_fields(state: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut push = |path: String, v: &Value| {
        if let Some(s) = v.as_str() {
            out.push((path, s.to_string()));
        }
    };
    for key in ["behavior", "depends", "instrument"] {
        push(key.to_string(), &state[key]);
    }
    for key in ["open_legs", "residue"] {
        for (i, v) in state[key]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .enumerate()
        {
            push(format!("{key}[{i}]"), v);
        }
    }
    for (i, h) in state["producer_chain"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        push(format!("producer_chain[{i}].name"), &h["name"]);
        push(format!("producer_chain[{i}].note"), &h["note"]);
    }
    for (i, h) in state["enumeration"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
    {
        push(format!("enumeration[{i}].site"), &h["site"]);
        push(format!("enumeration[{i}].verdict"), &h["verdict"]);
    }
    for key in ["instrument", "observed", "rule", "note"] {
        push(format!("verified.{key}"), &state["verified"][key]);
    }
    for key in ["command", "scope", "rule"] {
        push(format!("probe.{key}"), &state["probe"][key]);
    }
    push("upstream_reason".to_string(), &state["upstream_reason"]);
    push(
        "from_after_verified".to_string(),
        &state["from_after_verified"],
    );
    out
}

pub(crate) fn check_state(at: &str, state: &Value, failures: &mut Vec<String>) {
    for (path, text) in prose_fields(state) {
        for (rule, token) in hits(&text) {
            // The article follows the LABEL: three of the five names begin with a vowel, so
            // a fixed `a` would print `a audit-process word` - and a gate that misspells its
            // own finding reads as one nobody maintains.
            let article = if rule.starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            failures.push(format!(
                "{at}.{path}: `{token}` is {article} {rule} - the ledger is a fact record \
                 keyed by Claude Code version only"
            ));
        }
    }
}
