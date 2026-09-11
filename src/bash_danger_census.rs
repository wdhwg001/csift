//! `Bno` (2.1.258 @162863005) - the AST substitution census that runs on the
//! too-complex arm AFTER the lexical classifier returned nothing. Introduced at
//! 2.1.208 (the literal `too many to analyze for catastrophic removals` is absent
//! at 2.1.207), still present at 2.1.268 as `$Fo` with the same threshold and the
//! same reason strings, so it belongs to generations 2 and 3 alike.
//!
//! Three things happen here, in this order:
//! 1. every command / process substitution and `${ |cmd}` expansion is collected;
//!    MORE THAN 64 of them (strictly greater) with an `rm`/`rmdir` word anywhere in
//!    the text is itself an ask - the bail returns the SAME immune ask object.
//!    Without the rm word the scan simply stops and nothing is asked.
//! 2. the lexical classifier re-runs on EACH substitution's own text.
//! 3. a SECOND, BOUNDED fixpoint (@162864328) replaces backticks and then `$(...)`
//!    with the literal token `__CMDSUB__`, capped at 16 iterations, and the
//!    tokenised remainder is re-decomposed and handed to the structured checker.

use crate::bash_danger_argv::substitution_bodies;
use crate::bash_danger_lexical::{self, RM_WORD};
use crate::bash_danger_removal::{self, OperandVerdict};
use regex::Regex;
use std::sync::LazyLock;

/// The `>64` bail threshold @162863359 - strictly greater, so 65 collected nodes
/// bail and exactly 64 do not.
const SUBSTITUTION_BAIL: usize = 64;
/// The iteration cap of the second fixpoint @162864328.
const CMDSUB_FIXPOINT_ITERATIONS: usize = 16;
/// The token that fixpoint substitutes in.
const CMDSUB_TOKEN: &str = "__CMDSUB__";

/// The bail's reason tail @162863581, verbatim (its leading character is the
/// harness's own em dash).
pub(crate) const TAIL_TOO_MANY: &str = "\u{2014} too many command substitutions to analyze";
/// The per-substitution lexical hit's tail @162864253.
pub(crate) const TAIL_IN_SUBSTITUTION: &str =
    "on possibly-empty variable path inside command substitution";

/// `u7e` @162266075 - a `${ cmd}` / `${|cmd}` expansion counts as a substitution.
static BRACE_COMMAND: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{[ \t\n|]").expect("u7e"));

/// What the census decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CensusVerdict {
    /// An immune ask from the census itself (the bail).
    Bail { count: usize },
    /// An immune ask from the lexical classifier inside a substitution.
    Lexical { verb: &'static str, target: String },
    /// An immune ask from the structured checker on the tokenised remainder.
    Structured { tail: &'static str, target: String },
    /// The structured checker reached its filesystem-dependent arms.
    NeedsFs,
    /// Nothing asked.
    Clear,
}

/// Run the census over a command's text. `gen3` selects which lexical classifier
/// the per-substitution re-run uses, since the census outlived the rewrite.
pub(crate) fn census(command: &str, gen3: bool) -> CensusVerdict {
    let subs = collect_substitutions(command);
    if subs.len() > SUBSTITUTION_BAIL {
        if RM_WORD.is_match(command) {
            return CensusVerdict::Bail { count: subs.len() };
        }
        return CensusVerdict::Clear;
    }
    let mut needs_fs = false;
    let mut texts: Vec<String> = Vec::with_capacity(subs.len() + 1);
    texts.push(command.to_string());
    texts.extend(subs);
    for text in texts {
        for stmt in statements_of(&text) {
            let inner = unwrap_group(stmt.trim());
            let hit = if gen3 {
                crate::bash_danger_out::out(&inner).map(|h| (h.command, h.target))
            } else {
                bash_danger_lexical::hnt(&inner).map(|h| (h.command, h.target))
            };
            if let Some((verb, target)) = hit {
                return CensusVerdict::Lexical { verb, target };
            }
            let tokenised = cmdsub_fixpoint(&inner);
            match bash_danger_removal::structured(&tokenised) {
                OperandVerdict::Ask { tail, target } => {
                    return CensusVerdict::Structured { tail, target }
                }
                OperandVerdict::NeedsFs => needs_fs = true,
                OperandVerdict::Clear => {}
            }
        }
    }
    if needs_fs {
        CensusVerdict::NeedsFs
    } else {
        CensusVerdict::Clear
    }
}

/// Every substitution node's own text, nested ones included: the harness walks the
/// whole AST and pushes each `command_substitution` / `process_substitution` node
/// plus each `${ |cmd}` expansion, so a nested pair counts twice.
pub(crate) fn collect_substitutions(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut queue = vec![text.to_string()];
    let mut guard = 0usize;
    while let Some(t) = queue.pop() {
        guard += 1;
        if guard > 4096 {
            break;
        }
        for body in substitution_bodies(&t) {
            out.push(body.trim().to_string());
            queue.push(body);
        }
        for m in BRACE_COMMAND.find_iter(&t) {
            if let Some(body) = brace_command_body(&t, m.start()) {
                out.push(body);
            }
        }
    }
    out
}

/// The inside of a `${ cmd}` / `${|cmd}` expansion, leading `|` and trailing `;`
/// trimmed the way `u7e` trims them.
pub(crate) fn brace_command_body(text: &str, at: usize) -> Option<String> {
    let rest = &text[at..];
    let close = rest.find('}')?;
    let inner = &rest[2..close];
    Some(
        inner
            .trim()
            .trim_start_matches('|')
            .trim_end_matches(';')
            .trim()
            .to_string(),
    )
}

/// `ep(text)` with csift's documented whole-string fallback, then the same clause
/// split the classifier uses, so a `;`-separated body still yields its parts.
pub(crate) fn statements_of(text: &str) -> Vec<String> {
    match crate::bash_danger_shape::split_statements(text) {
        Some(parts) => parts.iter().map(|s| (*s).to_string()).collect(),
        None => vec![text.to_string()],
    }
}

/// `if(I.startsWith("{")&&/;?\s*\}$/.test(I)||I.startsWith("(")&&I.endsWith(")"))`
/// - peel one group wrapper before the classifier runs on the body.
pub(crate) fn unwrap_group(s: &str) -> String {
    let brace = s.starts_with('{') && s.trim_end().ends_with('}');
    let paren = s.starts_with('(') && s.ends_with(')');
    if brace || paren {
        let inner = &s[1..];
        let trimmed = inner.trim_end();
        let cut = trimmed
            .strip_suffix(['}', ')'])
            .unwrap_or(trimmed)
            .trim_end()
            .trim_end_matches(';');
        return cut.trim().to_string();
    }
    s.to_string()
}

/// The SECOND fixpoint @162864328: backticks first, unconditionally, then
/// `$(...)` at most sixteen times.
pub(crate) fn cmdsub_fixpoint(s: &str) -> String {
    let mut b = bash_danger_lexical::BACKTICKS
        .replace_all(s, CMDSUB_TOKEN)
        .into_owned();
    let mut prev = String::new();
    let mut z = 0usize;
    while prev != b && z < CMDSUB_FIXPOINT_ITERATIONS {
        prev.clone_from(&b);
        b = bash_danger_lexical::DOLLAR_PAREN
            .replace_all(&b, CMDSUB_TOKEN)
            .into_owned();
        z += 1;
    }
    b
}

#[cfg(test)]
#[path = "bash_danger_census_tests.rs"]
mod tests;
