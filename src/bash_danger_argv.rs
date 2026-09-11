//! How a command string becomes the argv the structured checker reads.
//!
//! The harness gets this from tree-sitter: a `command` node carries its argv with
//! quotes removed, its leading variable assignments and its redirections as
//! separate children, and an expansion or a command substitution already replaced
//! by the stand-in the decomposer left. csift has no parser, so this file is the
//! hand walk that produces the same shape, plus the two argv reductions `_9` is
//! called through - the prefix peel `Kx` @162800225 and the positional-operand
//! extractor `Fg` @162774310.

use regex::Regex;
use std::sync::LazyLock;

/// `st` @159474266 - a command substitution's stand-in operand.
pub(crate) const CMDSUB_SENTINEL: &str = "__CMDSUB_OUTPUT__";
/// `Le` @159474266 - a tracked variable expansion's stand-in operand.
pub(crate) const VAR_SENTINEL: &str = "__TRACKED_VAR__";
/// csift's own marker for an expansion the decomposer would have RESOLVED from
/// the environment - the value is not in the transcript, so the verdict is
/// `NeedsFs` rather than a guessed sentinel.
pub(crate) const ENV_MARKER: &str = "__CSIFT_ENV_VALUE__";

/// `To` @159474575 - the variable names the decomposer resolves instead of
/// tracking. An operand carrying one of these is environment-dependent.
const RESOLVED_ENV: &[&str] = &[
    "HOME",
    "PWD",
    "OLDPWD",
    "USER",
    "LOGNAME",
    "SHELL",
    "PATH",
    "HOSTNAME",
    "UID",
    "EUID",
    "PPID",
    "RANDOM",
    "SECONDS",
    "LINENO",
    "TMPDIR",
    "BASH_VERSION",
    "BASHPID",
    "SHLVL",
    "HISTFILE",
    "IFS",
];

/// `Kx` @162800225 - the prefix commands whose argv is peeled before the verb is
/// read. Unchanged at 2.1.268 (`QT` @166824062), so this set is not generational.
const PREFIX_COMMANDS: &[&str] = &[
    "time", "nohup", "timeout", "nice", "stdbuf", "env", "command",
];

/// A leading `NAME=value` / `NAME+=value` word, which tree-sitter hangs off the
/// command node as a `variable_assignment` rather than as argv.
static ASSIGNMENT_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*\+?=").expect("assignment_word"));

/// `wPe` @162791990 - basename-normalise the verb, but only when the basename is
/// `rm` or `rmdir`; anything else keeps its full token.
pub(crate) fn normalise_verb(head: &str) -> &str {
    let base = head.rsplit(['/', '\\']).next().unwrap_or(head);
    if base == "rm" || base == "rmdir" {
        base
    } else {
        head
    }
}

/// `Kx` @162800225, reduced to the peel every prefix command shares: drop the
/// prefix word and, for the flag-taking ones, the tokens it consumes.
fn strip_prefix_commands(argv: &[String]) -> Vec<String> {
    let mut v: Vec<String> = argv.to_vec();
    loop {
        let Some(head) = v.first() else { return v };
        let base = head.rsplit(['/', '\\']).next().unwrap_or(head).to_string();
        if !PREFIX_COMMANDS.contains(&base.as_str()) {
            return v;
        }
        let mut i = 1usize;
        // A prefix command's own options and, for `env`, its NAME=value settings.
        while i < v.len() {
            let t = &v[i];
            if t == "--" {
                i += 1;
                break;
            }
            if t.starts_with('-') {
                i += 1;
                continue;
            }
            if base == "env" && t.contains('=') {
                i += 1;
                continue;
            }
            if base == "timeout" && is_duration(t) {
                i += 1;
                continue;
            }
            break;
        }
        if i >= v.len() {
            return Vec::new();
        }
        v = v[i..].to_vec();
    }
}

fn is_duration(t: &str) -> bool {
    let body = t.trim_end_matches(['s', 'm', 'h', 'd']);
    !body.is_empty() && body.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// `Fg` @162774310 - the positional operands: skip leading `-`-led flags until a
/// `--` or the first non-flag, then take everything.
pub(crate) fn positional_operands(argv_rest: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut after_dashdash = false;
    let mut started = false;
    for t in argv_rest {
        if after_dashdash || started {
            out.push(t.clone());
        } else if t == "--" {
            after_dashdash = true;
        } else if t == "-" || !t.starts_with('-') {
            out.push(t.clone());
            started = true;
        }
    }
    out
}

/// `S9` @162874750 - whether ANY statement of the command is a `cd`.
pub(crate) fn any_statement_is_cd(command: &str) -> bool {
    simple_commands(command)
        .iter()
        .any(|s| command_argv(s).first().map(String::as_str) == Some("cd"))
}

/// One statement's argv as the `command` NODE carries it: leading variable
/// assignments and every redirection are separate children of that node, never
/// argv members, so they are dropped before the prefix peel runs.
pub(crate) fn command_argv(stmt: &str) -> Vec<String> {
    let words = argv_words(stmt);
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    let mut leading = true;
    let mut i = 0usize;
    while i < words.len() {
        let w = &words[i];
        if leading && ASSIGNMENT_WORD.is_match(w) {
            i += 1;
            continue;
        }
        if crate::bash_danger_lexical::REDIR_START.is_match(w) {
            if crate::bash_danger_lexical::REDIR_OP.is_match(w) {
                i += 1;
            }
            i += 1;
            continue;
        }
        leading = false;
        out.push(w.clone());
        i += 1;
    }
    strip_prefix_commands(&out)
}

/// Decompose a command into the simple commands `EPe` fans out over: every
/// statement, every pipeline segment, and the inside of every substitution.
pub(crate) fn simple_commands(command: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut queue = vec![command.to_string()];
    let mut guard = 0usize;
    while let Some(text) = queue.pop() {
        guard += 1;
        if guard > 256 {
            break;
        }
        for inner in substitution_bodies(&text) {
            queue.push(inner);
        }
        let stripped = strip_substitutions(&text);
        let Some(parts) = crate::bash_danger_shape::split_statements(&stripped) else {
            out.push(stripped);
            continue;
        };
        for p in parts {
            let t = p.trim();
            if !t.is_empty() {
                out.push(t.to_string());
            }
        }
    }
    out
}

/// The text inside every `$(...)`, `` `...` ``, `<(...)` and `>(...)`.
pub(crate) fn substitution_bodies(text: &str) -> Vec<String> {
    let b: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            '\\' => i += 2,
            '`' => {
                if let Some(end) = b[i + 1..].iter().position(|c| *c == '`') {
                    out.push(b[i + 1..i + 1 + end].iter().collect());
                    i += end + 2;
                } else {
                    i += 1;
                }
            }
            '$' | '<' | '>' if i + 1 < b.len() && b[i + 1] == '(' => {
                let mut depth = 0i32;
                let mut j = i + 1;
                while j < b.len() {
                    if b[j] == '(' {
                        depth += 1;
                    } else if b[j] == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j < b.len() {
                    out.push(b[i + 2..j].iter().collect());
                    i = j + 1;
                } else {
                    i += 2;
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Replace every substitution with its sentinel so the outer statement split and
/// the operand tests see what `_9` sees.
fn strip_substitutions(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            '\\' if i + 1 < b.len() => {
                out.push(b[i]);
                out.push(b[i + 1]);
                i += 2;
            }
            '`' => {
                if let Some(end) = b[i + 1..].iter().position(|c| *c == '`') {
                    out.push_str(CMDSUB_SENTINEL);
                    i += end + 2;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            '$' | '<' | '>' if i + 1 < b.len() && b[i + 1] == '(' => {
                let mut depth = 0i32;
                let mut j = i + 1;
                while j < b.len() {
                    if b[j] == '(' {
                        depth += 1;
                    } else if b[j] == ')' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    j += 1;
                }
                if j < b.len() {
                    out.push_str(CMDSUB_SENTINEL);
                    i = j + 1;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Split one simple command into argv words: quotes removed, expansions replaced
/// by the decomposer's sentinels, backslash escapes consumed.
pub(crate) fn argv_words(stmt: &str) -> Vec<String> {
    let chars: Vec<char> = stmt.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has = false;
    let mut quote: Option<char> = None;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some('\'') => {
                if c == '\'' {
                    quote = None;
                } else {
                    cur.push(c);
                }
                i += 1;
            }
            Some(q) => {
                if c == '\\' && i + 1 < chars.len() {
                    cur.push(chars[i + 1]);
                    i += 2;
                    continue;
                }
                if c == q {
                    quote = None;
                    i += 1;
                    continue;
                }
                if c == '$' {
                    let (tok, used) = expansion_at(&chars, i);
                    cur.push_str(&tok);
                    i += used;
                    continue;
                }
                cur.push(c);
                i += 1;
            }
            None => {
                if c.is_whitespace() {
                    if has {
                        out.push(std::mem::take(&mut cur));
                        has = false;
                    }
                    i += 1;
                    continue;
                }
                has = true;
                match c {
                    '\\' if i + 1 < chars.len() => {
                        cur.push(chars[i + 1]);
                        i += 2;
                    }
                    '\'' | '"' => {
                        quote = Some(c);
                        i += 1;
                    }
                    '$' => {
                        let (tok, used) = expansion_at(&chars, i);
                        cur.push_str(&tok);
                        i += used;
                    }
                    _ => {
                        cur.push(c);
                        i += 1;
                    }
                }
            }
        }
    }
    if has {
        out.push(cur);
    }
    out
}

/// One `$`-led expansion, as the sentinel the decomposer would leave behind.
fn expansion_at(chars: &[char], i: usize) -> (String, usize) {
    let rest = &chars[i + 1..];
    if rest.first() == Some(&'{') {
        let mut j = 1usize;
        while j < rest.len() && rest[j] != '}' {
            j += 1;
        }
        let name: String = rest[1..j.min(rest.len())]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
            .collect();
        let used = if j < rest.len() {
            j + 2
        } else {
            rest.len() + 1
        };
        return (sentinel_for(&name), used);
    }
    let name: String = rest
        .iter()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '_')
        .collect();
    if name.is_empty() {
        return ("$".to_string(), 1);
    }
    let used = name.chars().count() + 1;
    (sentinel_for(&name), used)
}

fn sentinel_for(name: &str) -> String {
    if RESOLVED_ENV.contains(&name) {
        ENV_MARKER.to_string()
    } else {
        VAR_SENTINEL.to_string()
    }
}
