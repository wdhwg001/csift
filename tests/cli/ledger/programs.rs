//! Rule 19: an instrument that carries a PROGRAM still carries one that parses.
//!
//! Several instruments hand a stranger a heredoc - `python3 - <<'EOF' ... EOF` - and a
//! heredoc carries a program whose INDENT is its block structure. A prose pass that
//! normalises whitespace runs destroys that structure without touching a single word, and
//! the result reads exactly like the original to every other rule here: the sentence is
//! intact, the counting rule is intact, and the program is dead. Nothing else in this file
//! can see that, which is why the parse is a RULE rather than a habit.
//!
//! Each language is asked in its own words - python `compile`, `bash -n`, `jq -n` - and an
//! interpreter this host does not hold is COUNTED and skipped, exactly as rule 12 skips a
//! build the host does not hold: whether a program parses is a property of the program, and
//! a downstream `cargo test` on a machine without jq is not evidence about it.

use serde_json::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

/// What the rule looked at, for the tally line it prints.
#[derive(Default)]
pub(crate) struct Programs {
    pub(crate) checked: usize,
    pub(crate) skipped: BTreeMap<&'static str, usize>,
}

/// One embedded program: the language its opening line names, its body, and the line the
/// heredoc opened on (which is what a reader needs to find it in a 700-character field).
struct Embedded {
    lang: &'static str,
    body: String,
    line: usize,
}

/// The interpreter word a heredoc's own command line names. A heredoc with no interpreter
/// in front of it is data rather than a program, and is not this rule's business.
fn language_of(prefix: &str) -> Option<&'static str> {
    let mut found = None;
    for tok in prefix.split_whitespace() {
        let word = tok.rsplit('/').next().unwrap_or(tok);
        found = match word {
            "python" | "python3" => Some("python"),
            "bash" | "sh" => Some("bash"),
            "jq" => Some("jq"),
            _ => found,
        };
    }
    found
}

/// Every heredoc in a text, as its language and its body.
///
/// The terminator is a LINE equal to the word, which is what the shell itself requires, so
/// a `<<` inside prose ("a shift of `<<that`") opens nothing and is never collected.
fn embedded(text: &str) -> Vec<Embedded> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let Some(pos) = line.find("<<") else {
            i += 1;
            continue;
        };
        let rest = &line[pos + 2..];
        let rest = rest.strip_prefix('-').unwrap_or(rest);
        let rest = rest.trim_start();
        let quoted = rest.starts_with('\'') || rest.starts_with('"');
        let body_start = if quoted { &rest[1..] } else { rest };
        let word: String = body_start
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if word.is_empty() || word.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            i += 1;
            continue;
        }
        let Some(lang) = language_of(&line[..pos]) else {
            i += 1;
            continue;
        };
        let Some(end) = (i + 1..lines.len()).find(|&j| lines[j].trim_end() == word) else {
            i += 1;
            continue; // an opener with no terminator is prose that happens to carry `<<`
        };
        out.push(Embedded {
            lang,
            body: lines[i + 1..end].join("\n"),
            line: i + 1,
        });
        i = end + 1;
    }
    out
}

/// Ask the language. `Ok(())` = it parses; `Err(msg)` = it does not, with the interpreter's
/// own first line; `Ok` with the interpreter absent is decided by the caller.
fn parses(lang: &str, body: &str) -> Result<(), String> {
    let (program, args): (&str, Vec<&str>) = match lang {
        "python" => (
            "python3",
            vec![
                "-c",
                "import sys; compile(sys.stdin.read(), '<instrument>', 'exec')",
            ],
        ),
        "bash" => ("bash", vec!["-n"]),
        "jq" => ("jq", vec!["-n", "-f", "/dev/stdin"]),
        _ => return Ok(()),
    };
    let mut child = match Command::new(program)
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return Err(String::from("\u{0}absent")),
    };
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(body.as_bytes());
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return Err(format!("\u{0}{e}")),
    };
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    // A `jq -n` run EXECUTES the program, so a failure that is not a compile error is the
    // program asking for input this rule deliberately does not give it.
    if lang == "jq" && !first.contains("syntax error") && !first.contains("compile error") {
        return Ok(());
    }
    Err(first.to_string())
}

/// Rule 19 over one state's `instrument` and `probe.command`.
pub(crate) fn check_state(
    at: &str,
    state: &Value,
    tally: &mut Programs,
    failures: &mut Vec<String>,
) {
    for (field, text) in [
        ("instrument", state["instrument"].as_str()),
        ("probe.command", state["probe"]["command"].as_str()),
    ] {
        let Some(text) = text else { continue };
        for prog in embedded(text) {
            match parses(prog.lang, &prog.body) {
                Ok(()) => tally.checked += 1,
                Err(msg) if msg.starts_with('\u{0}') => {
                    *tally.skipped.entry(prog.lang).or_insert(0) += 1;
                }
                Err(msg) => failures.push(format!(
                    "{at}: the {} program in `{field}` (heredoc opening at line {}) does not \
                     parse: {msg} - an instrument that carries a program carries one a \
                     stranger can run, and a whitespace pass is the way that breaks silently",
                    prog.lang, prog.line
                )),
            }
        }
    }
}

/// The tally line, in rule 12's shape: what ran, and what this host could not ask.
pub(crate) fn line(tally: &Programs) -> String {
    let mut s = format!(
        "ledger rule 19: embedded programs parsed: {}",
        tally.checked
    );
    if !tally.skipped.is_empty() {
        let missing: Vec<String> = tally
            .skipped
            .iter()
            .map(|(lang, n)| format!("{n} {lang}"))
            .collect();
        s.push_str(&format!(
            " [not checked, no interpreter on this host: {}]",
            missing.join(", ")
        ));
    }
    s
}
