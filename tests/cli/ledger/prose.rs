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
//!     describe who looked, which is never part of what Claude Code does.
//!
//! WHAT IS NOT BARRED, and why the list is shorter than the law reads. `session`, `lane`
//! and `rejected` are csift's own domain vocabulary - a top-level session, a frozen lane,
//! a rejected tool use - so banning the token would destroy meaning rather than process
//! narrative; those words are barred in their audit SENSE, which no regex decides, and
//! the sweep that converted the ledger rewrote them by phrase. First person is barred the
//! same way and for a second reason: these strings quote minified JavaScript, where `I`,
//! `we` and `my` are ordinary identifiers.
//!
//! A BACKTICKED span is exempt, for the reason an `excerpt` is: it quotes the build or a
//! command verbatim, and Claude Code really does intern a beta named
//! `server-side-fallback-2026-06-01`. The exemption applies ONLY when the backticks
//! BALANCE - an odd count means one span is unclosed, and pairing left to right from there
//! exempts prose instead of a literal, which is how a date and a first-person clause once
//! hid behind an exemption they had no claim to. `probe.literal` is exempt for the same
//! reason a quotation is. Everything a pass WROTE is scanned.

use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").unwrap())
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
    RE.get_or_init(|| Regex::new(r"(?:^|[^\w.])(v?0\.\d{1,2}\.\d{1,3})(?:[^\w.]|$)").unwrap())
}

/// The PLURALS and the agent noun are in the alphabet, because a word list that stops at
/// the singular is a list with a hole in it: `adjudications` shipped 67 times in ONE bulk
/// receipt sentence and `auditor` four times, while `adjudication` and `audit` were both
/// already barred.
fn audit_word_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(re-audits|re-audit|verifiers|verifier|auditors|auditor|tracers|tracer|scribes|scribe|adjudications|adjudication|adjudicated|kicked|today|yesterday|audits|audited|audit)\b",
        )
        .unwrap()
    })
}

fn backticked_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"`[^`\n]*`").unwrap())
}

/// csift's own surface spells the flag `--audit`, the module `plan/audit.rs` and the mode
/// `plan --audit`, so an `audit` touching `.`, `/` or `-` is the tool's word, not the
/// process's. The regex crate has no look-behind, so the neighbours are read here.
fn path_adjacent(s: &str, start: usize, end: usize) -> bool {
    let before = s[..start].chars().next_back();
    let after = s[end..].chars().next();
    let touching = |c: Option<char>| matches!(c, Some('.') | Some('/') | Some('-'));
    touching(before) || touching(after)
}

/// Every prose-law violation in one string, as (rule name, the matched text).
pub(crate) fn hits(s: &str) -> Vec<(&'static str, String)> {
    // Blank the quotations, keeping the offsets, and only where the quoting is balanced.
    let scanned = if s.matches('`').count() % 2 == 0 {
        backticked_re()
            .replace_all(s, |c: &regex::Captures| " ".repeat(c[0].len()))
            .into_owned()
    } else {
        s.to_string()
    };
    let s = scanned.as_str();
    let mut out = Vec::new();
    for m in date_re().find_iter(s) {
        out.push(("calendar date", m.as_str().to_string()));
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
        if !path_adjacent(s, m.start(), m.end()) {
            out.push(("audit-process word", m.as_str().to_string()));
        }
    }
    out
}

/// Every field of a state a PASS wrote, as (path, text). An `excerpt` and a build literal
/// are quotations and are deliberately absent.
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
            failures.push(format!(
                "{at}.{path}: `{token}` is a {rule} - the ledger is a fact record keyed by \
                 Claude Code version only"
            ));
        }
    }
}
