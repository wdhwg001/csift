//! Shell-cwd tracking and operand resolution: joins a relative Bash operand to the
//! directory the shell actually ran in, without ever guessing.
//!
//! ## How Claude Code manages the Bash tool's cwd (verified against CC 2.1.237)
//!
//! Claude Code does NOT keep a long-lived shell. Every Bash tool call spawns a fresh
//! shell whose starting directory is a TRACKED cwd, and that tracked value is stamped
//! on every jsonl record as the top-level `cwd` field. Concretely:
//!
//! - The runner appends `&& pwd -P >| <tmp>/claude-<id>-cwd` to the command and reads
//!   the file back afterward. Because the read-back is `&&`-chained, a command that
//!   exits non-zero, and any backgrounded command, never advances the tracked cwd.
//! - A `cd` into a SUBDIRECTORY of the project persists silently into later tool
//!   calls; the tracked cwd (and so the record `cwd` field) follows it. A reset back
//!   to the original directory happens only when the shell ends OUTSIDE the original
//!   cwd and the `/add-dir` set, and it prints "Shell cwd was reset to <path>" into
//!   the tool result. With `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR` set truthy, the
//!   reset happens on every call and silently.
//! - ANCHOR LAW: read the `cwd` off the TOOL_USE record, not the result record. The
//!   post-command cwd update is asynchronous and in about 0.24% of calls the result
//!   record still carries the pre-command value.
//!
//! The consequence for this parser: no cross-command state is needed. Each Bash record
//! carries its own spawn cwd; only `cd` movements INSIDE one command must be tracked,
//! and an operand resolves against the checkpoint in effect at its position in the
//! command (a `cd` takes effect for the segments after it, never its own).
//!
//! ## Resolution classes (nothing is guessed)
//!
//! - `Absolute`: the operand was typed absolute; used as-is.
//! - `CwdJoined`: a relative operand before any `cd`; joined to the record's own `cwd`
//!   field. That value is data Claude Code wrote, so the join involves no inference.
//! - `CdTracked`: a relative operand after one or more literal in-command `cd`s;
//!   the join uses the lexically tracked directory. This is inference: a `cd` that
//!   failed at runtime is invisible here. Measured against Claude Code's own
//!   modified-file hints, this class resolves correctly in about 99.7% of cases.
//! - `Unresolved`: the operand (or a `cd` on the way to it) carries `~`, `$VAR`, a
//!   substitution, or the cwd is otherwise unknowable. The operand is kept VERBATIM
//!   and callers must disclose it as unresolved, never treat it as a full path.

use super::*;

/// The shell's working directory in effect at one point of a command, relative to the
/// spawn state. `Rel` chains unresolved relative `cd`s (joined to the record cwd at
/// resolve time); `Abs` is reached through a literal absolute `cd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CwdAt {
    /// No `cd` ran before this point: the shell is at the record's own `cwd`.
    Spawn,
    /// Only relative literal `cd`s ran: this path is relative to the spawn cwd.
    Rel(String),
    /// A literal absolute `cd` ran (possibly followed by relative ones).
    Abs(String),
    /// A `cd` whose target this lexical layer cannot know (`cd "$V"`, `cd ~`, `cd`
    /// with no operand, `popd`, a substitution). Every later relative operand is
    /// unresolvable until a literal absolute `cd` restores certainty.
    Unknown,
}

/// How a reported mutation path was obtained. See the module doc for the exact
/// confidence semantics of each class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    Absolute,
    CwdJoined,
    CdTracked,
    Unresolved,
}

impl Resolution {
    /// The wire spelling used in JSON output.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Resolution::Absolute => "absolute",
            Resolution::CwdJoined => "cwd-joined",
            Resolution::CdTracked => "cd-tracked",
            Resolution::Unresolved => "unresolved",
        }
    }
}

/// Per-command `cd` state machine. Fed one segment at a time IN ORDER by the parse
/// loop; `checkpoint()` is read BEFORE the segment's own effect is applied, so a
/// mutation in segment N resolves against the state established by segments 0..N-1.
#[derive(Debug)]
pub(crate) struct CwdTracker {
    cur: CwdAt,
    /// The state before the most recent `cd`, for `cd -`.
    prev: Option<CwdAt>,
    /// Bare-subshell `(` depth; a `cd` inside a subshell never affects the parent
    /// shell, so state changes are ignored while depth > 0.
    depth: usize,
}

impl CwdTracker {
    pub(crate) fn new() -> Self {
        CwdTracker {
            cur: CwdAt::Spawn,
            prev: None,
            depth: 0,
        }
    }

    /// The cwd in effect for operands of the CURRENT segment.
    pub(crate) fn checkpoint(&self) -> CwdAt {
        self.cur.clone()
    }

    /// Apply one segment's effect: update subshell depth, and track a top-level
    /// `cd`/`pushd`/`popd`. Call AFTER stamping the segment's mutations.
    pub(crate) fn observe_segment(&mut self, segment: &str, mask: &str) {
        let depth_at_start = self.depth;
        self.advance_depth(segment, mask);

        if depth_at_start > 0 {
            return; // inside a subshell: the parent shell's cwd is unaffected.
        }
        let toks = masked_tokens(segment, mask);
        let all: Vec<&str> = toks
            .iter()
            .filter(|t| !is_fully_masked(t.masked))
            .map(|t| t.orig)
            .collect();
        let cmd = strip_prefixes(&all);
        let Some((&verb, operands)) = cmd.split_first() else {
            return;
        };
        match verb {
            // `popd` restores a directory this layer does not model.
            "popd" => self.set(CwdAt::Unknown),
            "cd" | "pushd" | "chdir" => self.apply_cd(operands),
            _ => {}
        }
    }

    fn apply_cd(&mut self, operands: &[&str]) {
        // Skip cd's own flags (-P, -L, --); a lone `-` is the previous-dir target.
        let target = operands
            .iter()
            .map(|t| strip_quotes(t))
            .find(|t| *t == "-" || !t.starts_with('-'));
        let Some(t) = target else {
            return self.set(CwdAt::Unknown); // bare `cd` goes to $HOME: a guess.
        };
        if t == "-" {
            // Swap with the previous directory when it is known. A restored Spawn is
            // represented as `Rel("")`: the position equals the spawn cwd, but it was
            // reached through tracked cds, so it must keep the inference class.
            let back = match self.prev.clone() {
                Some(CwdAt::Spawn) => CwdAt::Rel(String::new()),
                Some(other) => other,
                None => CwdAt::Unknown,
            };
            return self.set(back);
        }
        if t.is_empty()
            || t.contains('$')
            || t.contains('`')
            || t.starts_with('~')
            || has_syntax_noise(t)
        {
            return self.set(CwdAt::Unknown);
        }
        let next = if is_absolute_shell_path(t) {
            CwdAt::Abs(t.to_string())
        } else {
            match &self.cur {
                CwdAt::Spawn => CwdAt::Rel(t.to_string()),
                CwdAt::Rel(r) => CwdAt::Rel(format!("{r}/{t}")),
                CwdAt::Abs(a) => CwdAt::Abs(join_shell_path(a, &[t])),
                CwdAt::Unknown => CwdAt::Unknown,
            }
        };
        self.set(next);
    }

    fn set(&mut self, next: CwdAt) {
        self.prev = Some(std::mem::replace(&mut self.cur, next));
    }

    /// Count bare-subshell parens visible in the mask. `$( `, `>(` and `<(` heads are
    /// substitutions, not subshells; their opening paren is skipped by looking one
    /// byte back. A close never takes depth below zero.
    fn advance_depth(&mut self, segment: &str, mask: &str) {
        let seg = segment.as_bytes();
        let m = mask.as_bytes();
        for i in 0..m.len() {
            if m[i] != seg[i] {
                continue; // masked byte (quote/procsub interior): not shell syntax.
            }
            match seg[i] {
                b'(' => {
                    let head = if i > 0 { seg[i - 1] } else { b' ' };
                    if !matches!(head, b'$' | b'>' | b'<' | b'(') {
                        self.depth += 1;
                    }
                }
                b')' => self.depth = self.depth.saturating_sub(1),
                _ => {}
            }
        }
    }
}

/// True for a path the shell treats as absolute: unix-rooted, a Windows drive form
/// (`C:\` or `C:/`), or a UNC `\\server\...` prefix.
#[must_use]
pub fn is_absolute_shell_path(p: &str) -> bool {
    if p.starts_with('/') || p.starts_with("\\\\") {
        return true;
    }
    let b = p.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\')
}

/// Join path fragments onto an absolute base and normalize `.`/`..` lexically, as pure
/// string work in the BASE's separator family. Transcripts are analyzed on any host, so
/// host `Path` semantics must never leak in: a Windows-family base joins with `\` and
/// converts `/` in the fragments; a unix base joins with `/`. `..` never pops past the
/// root (the lexical-normalize convention used elsewhere in the crate).
#[must_use]
pub fn join_shell_path(base: &str, parts: &[&str]) -> String {
    let windows = {
        let b = base.as_bytes();
        base.starts_with("\\\\") || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
    };
    let sep = if windows { '\\' } else { '/' };
    // The non-popping root prefix: `/` for unix, `C:` for a drive, `\\server\share`
    // for UNC (server+share belong to the root, not the component list).
    let (root, rest): (String, &str) = if windows {
        if let Some(tail) = base.strip_prefix("\\\\") {
            let mut it = tail.splitn(3, ['\\', '/']);
            let server = it.next().unwrap_or("");
            let share = it.next().unwrap_or("");
            (format!("\\\\{server}\\{share}"), it.next().unwrap_or(""))
        } else {
            (base[..2].to_string(), &base[3..])
        }
    } else {
        ("".to_string(), base.trim_start_matches('/'))
    };
    let mut comps: Vec<&str> = Vec::new();
    for piece in std::iter::once(rest).chain(parts.iter().copied()) {
        for c in piece.split(['/', '\\']) {
            match c {
                "" | "." => {}
                ".." => {
                    comps.pop();
                }
                other => comps.push(other),
            }
        }
    }
    let joined = comps.join(&sep.to_string());
    if windows {
        format!("{root}{sep}{joined}")
    } else {
        format!("/{joined}")
    }
}

/// Class-marker pseudo-paths (`git:add` today; mutating-class markers later) are flags,
/// never file paths: they must not be resolved, joined, or matched against a `--file`.
#[must_use]
pub fn is_class_marker(path: &str) -> bool {
    const MARKER_HEADS: &[&str] = &["git:", "fmt:", "interp:", "pkg:", "extract:"];
    MARKER_HEADS.iter().any(|h| path.starts_with(h))
}

impl BashMutation {
    /// Resolve this mutation's operand against the recording shell's cwd (the
    /// TOOL_USE record's top-level `cwd` field; see the module doc's anchor law).
    /// Returns the path to report plus its [`Resolution`] class. An unresolvable
    /// operand comes back VERBATIM, never fabricated into a full path.
    #[must_use]
    pub fn resolve(&self, record_cwd: Option<&str>) -> (String, Resolution) {
        let p = self.path.as_str();
        if is_class_marker(p) {
            return (p.to_string(), Resolution::Unresolved);
        }
        if is_absolute_shell_path(p) {
            return (p.to_string(), Resolution::Absolute);
        }
        if p.starts_with('~') {
            return (p.to_string(), Resolution::Unresolved);
        }
        let cwd = record_cwd.filter(|c| is_absolute_shell_path(c));
        match (&self.cwd_at, cwd) {
            (CwdAt::Abs(a), _) => (join_shell_path(a, &[p]), Resolution::CdTracked),
            (CwdAt::Spawn, Some(c)) => (join_shell_path(c, &[p]), Resolution::CwdJoined),
            (CwdAt::Rel(r), Some(c)) => (join_shell_path(c, &[r, p]), Resolution::CdTracked),
            _ => (p.to_string(), Resolution::Unresolved),
        }
    }
}
