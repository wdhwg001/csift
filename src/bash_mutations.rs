//! Heuristic, regex-free Bash file-mutation parser.
//!
//! Bash tool_use records carry `input.command` but their `toolUseResult` is
//! `{stdout, stderr, interrupted, isImage, noOutputExpected}` - **no path field**. So
//! unlike the structured Write/Edit/MultiEdit/NotebookEdit tools (which name an exact
//! `file_path`), a Bash file mutation can only be inferred from the command STRING.
//!
//! This is a **best-effort LEXICAL** parse, NOT a shell parser: it splits the command
//! on `;`, `&&`, `||`, `|`, and newlines into segments, then inspects each segment's
//! leading command token against a conservative allowlist of mutating verbs. It does
//! not expand variables, globs, command substitutions, or aliases, and it does not
//! touch the filesystem. Every mutation it reports is therefore **labelled heuristic**
//! everywhere it surfaces (text output, JSON, help, SKILL) - see
//! [`crate::model::FileOp::is_heuristic`]. Relative paths are reported VERBATIM (the
//! session's cwd at command time is not reliably known, so absolutizing would
//! fabricate a path).
//!
//! ## What it catches (recall)
//!
//! Beyond the verb allowlist (`rm`/`mkdir`/`touch`/`tee`/`cp`/`mv`/`install`/`ln`/`rsync`/
//! `sed -i`/`git`) and plain `>`/`>>` redirects, it also reads:
//! - **fd-qualified redirects** - `2>`/`1>`/`&>` (+ `>>` forms), attached (`2>/tmp/x`)
//!   and spaced (`2> /tmp/x`), and the noclobber-override `>|`; a bare `2>&1` fd-dup and
//!   `/dev/null|stderr|stdout` sinks are NOT paths and are skipped.
//! - **`-t DIR` destination flag** - `cp`/`mv`/`install -t DIR src…` puts the written
//!   destination right after `-t` (sources LAST), so the `-t` value is the dest and every
//!   positional is a read source (without `-t`, the last positional is the dest).
//! - **`curl`/`wget` output flags** - `-o <path>` / `--output <path>` / `--output=…`
//!   (and `wget -O <path>`). A `curl -O` that derives the name from the URL has no
//!   deterministic local path and is skipped.
//! - **flag-specified outputs** - `--<name>=<path>` / `--<name> <path>` for a small
//!   allowlist (`junit-xml`, `junitxml`, `report-path`, `output`, `out-file`, …),
//!   `dd of=<path>`, and a `zip <dest>` archive.
//! - **`perl -i`** - the in-place twin of `sed -i` (`perl -pi -e 's/a/b/' file…`), verb
//!   `perl-i` ([`emit_perl`]).
//! - **interpreter write idioms** ([`interp`]) - a `python3 - <<'PY' … PY` / `node -e …`
//!   payload whose write target is provably literal (a direct literal argument, or a
//!   ONE-HOP constant bound exactly once) emits a real `interp-write` row; a write whose
//!   target the guards cannot extract emits an `interp:<lang>` class marker instead.
//! - **mutating-CLASS markers** ([`classes`]) - a formatter (`cargo fmt`, `prettier
//!   --write`), package manager (`npm install`), or archive extraction (`tar -xf`)
//!   mutates files it never names; those commands emit a `fmt:`/`pkg:`/`extract:`
//!   pseudo-path in the `git:<sub>` style, flagging the class without inventing paths.
//!
//! ## Precision contract (no fabricated rows)
//!
//! Only a path that names a real touched target is ever emitted. A token that cannot be
//! resolved to one is DROPPED, never surfaced as a noisy pseudo-row: an unresolved
//! `$VAR` / `${VAR}` / `$()`-bearing token (we cannot expand it, so a row would be a
//! fabricated path), a `/dev/null`-class sink, and a mis-parsed redirect tail all yield
//! nothing. Two informative non-concrete exceptions are kept verbatim because they
//! still name a real touched set and the heuristic label is explicit: a glob
//! (`*.tmp`), and a leading-`~` home path (`~/notes.md` - reported verbatim and
//! resolution-classed `unresolved`, never joined to the cwd and never expanded). The
//! coarse class-marker pseudo-paths (`git:<sub>`, `fmt:`/`interp:`/`pkg:`/`extract:`)
//! are intentional and are never treated as paths ([`is_class_marker`]).
//!
//! ## Out of scope (documented limitation)
//!
//! Heredoc BODY lines are lexically skipped ([`strip_heredoc_bodies_keeping`]) BEFORE
//! redirect/verb scanning, so a `>` or quote inside the body can never be mis-read as a
//! redirect (only the opener LINE, which may carry a real trailing `> file`, is
//! scanned). The stripped bodies are not discarded: when the segment's verb is an
//! interpreter, the body text is handed to [`interp`]'s write-idiom analyzer. That
//! analyzer is deliberately narrow - it extracts a target only when the target is
//! provably literal, and reports any other write as the opaque `interp:<lang>` class.
//! Arbitrary embedded-code dataflow (f-strings, argv, env, concatenation, reassigned
//! names) stays out of scope: such writes surface as the class marker, never as a
//! guessed path.
//!
//! A trailing `# …` SHELL COMMENT is likewise lexically masked ([`shell_mask`]) before any
//! token/redirect scan: an unquoted `#` at a word boundary opens a comment to end-of-line,
//! so neither the comment words, an in-comment `>` redirect target, nor an in-comment `;`/`|`
//! operator can fabricate a row OR displace a real cp/mv/ln destination (`cp src dst # note`
//! reports `dst`, not `note`). An IN-PATH `#` (`/tmp/a#b`) is preserved - `#` masks only when
//! it starts a token.

mod anchors;
mod classes;
mod commands;
mod cwd;
mod entry;
mod heredoc;
mod interp;
mod mask;
mod outputs;
mod redirect;

pub(crate) use anchors::*;
pub(crate) use classes::*;
pub(crate) use commands::*;
pub(crate) use cwd::*;
pub(crate) use entry::*;
pub(crate) use heredoc::*;
pub(crate) use interp::*;
pub(crate) use mask::*;
pub(crate) use outputs::*;
pub(crate) use redirect::*;

#[cfg(test)]
mod tests;
