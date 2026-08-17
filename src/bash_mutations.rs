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
//!
//! ## Precision contract (no fabricated rows)
//!
//! Only a **concrete, resolvable** path is ever emitted. A token that does not name a
//! real path is DROPPED, never surfaced as a noisy pseudo-row: an unresolved `$VAR` /
//! `${VAR}` / `~`-or-`$()`-bearing token (we cannot expand it, so a row would be a
//! fabricated path), a `/dev/null`-class sink, and a mis-parsed redirect tail all yield
//! nothing. (Globs like `*.tmp` remain the one informative non-concrete exception, kept
//! verbatim, because they still name a real touched set and the heuristic label is
//! explicit.) The `git:<sub>` coarse pseudo-path is intentional and unaffected.
//!
//! ## Out of scope (documented limitation)
//!
//! Write calls inside an EMBEDDED-LANGUAGE body are NOT parsed - a heredoc
//! (`python3 - <<'PY' … PY`), an inline `python3 -c "open('/tmp/x','w')…"`, a `Path(…)
//! .write_text(…)`, etc. This is a deliberate limit of a lexical (non-shell, non-Python)
//! parser: the body is opaque command TEXT, and reliably parsing arbitrary embedded code
//! is out of scope. Such writes are missed (a recall gap), but the precision contract
//! above guarantees they never produce a WRONG row - heredoc BODY lines are lexically
//! skipped ([`strip_heredoc_bodies`]) BEFORE redirect/verb scanning, so a `>` or quote
//! inside the body can no longer be mis-read as a redirect (only the opener LINE, which
//! may carry a real trailing `> file`, is scanned).
//!
//! A trailing `# …` SHELL COMMENT is likewise lexically masked ([`shell_mask`]) before any
//! token/redirect scan: an unquoted `#` at a word boundary opens a comment to end-of-line,
//! so neither the comment words, an in-comment `>` redirect target, nor an in-comment `;`/`|`
//! operator can fabricate a row OR displace a real cp/mv/ln destination (`cp src dst # note`
//! reports `dst`, not `note`). An IN-PATH `#` (`/tmp/a#b`) is preserved - `#` masks only when
//! it starts a token.

mod commands;
mod entry;
mod heredoc;
mod mask;
mod outputs;
mod redirect;

pub(crate) use commands::*;
pub(crate) use entry::*;
pub(crate) use heredoc::*;
pub(crate) use mask::*;
pub(crate) use outputs::*;
pub(crate) use redirect::*;

#[cfg(test)]
mod tests;
