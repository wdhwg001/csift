//! git + flag-driven outputs: downloads, dd, zip, tar, collect_flag_outputs.

use super::*;

/// `git <sub> …` - record a single coarse `git:<sub>` pseudo-path ONLY when `<sub>` is
/// in the mutating allowlist. Git does not name a clean per-file list lexically, so we
/// never try to enumerate files; the entry is flagged heuristic like everything here.
pub(crate) fn emit_git(operands: &[&str], out: &mut Vec<BashMutation>) {
    if let Some(sub) = git_subcommand(operands) {
        if GIT_MUTATING.contains(&sub) {
            out.push(BashMutation {
                path: format!("git:{sub}"),
                verb: "git",
            });
        }
    }
}

/// The git SUBCOMMAND token, skipping leading global options. The value-taking global
/// flags `-c <key=val>` and `-C <path>` consume the FOLLOWING token, so it must not be
/// mistaken for the subcommand (the `git -c user.name=x commit` case).
pub(crate) fn git_subcommand<'a>(operands: &[&'a str]) -> Option<&'a str> {
    let mut i = 0usize;
    while i < operands.len() {
        let tok = operands[i];
        if tok == "-c" || tok == "-C" {
            i += 2; // skip the flag AND its value
        } else if tok.starts_with('-') {
            i += 1; // a bare option flag (e.g. `--no-pager`)
        } else {
            return Some(tok); // the first non-option token is the subcommand
        }
    }
    None
}

/// The output-flag NAMES (without the leading `--`) whose value is a written path. Matched
/// both as `--name=<path>` and `--name <path>`. The value is run through
/// [`flag_output_path`], which drops a FORMAT-SELECTOR value (`--output json`,
/// `--output=yaml`, `kubectl/gh/docker/aws/jq` idioms) - a bare format word is a render
/// mode, not a file. A path-shaped value (`report.json`, `/tmp/out`, `build/x`) passes.
pub(crate) const OUTPUT_FLAGS: &[&str] = &[
    "junit-xml",
    "junitxml",
    "report-path",
    "output",
    "out-file",
    "outfile",
    "out",
    "logfile",
    "log-file",
];

/// Well-known FORMAT-SELECTOR values an `--output`/`--out`/`--logfile` flag commonly takes
/// instead of a path (`kubectl -o`, `gh`, `docker`, `aws`, `jq` idioms). A bare one of
/// these - with no `/` and no `.extension` to mark it path-shaped - is a render mode, never
/// a created file, so it must not fabricate a phantom-file row.
pub(crate) const FORMAT_SELECTORS: &[&str] = &[
    "json",
    "yaml",
    "yml",
    "wide",
    "table",
    "text",
    "name",
    "jsonpath",
    "go-template",
    "gotemplate",
    "template",
    "csv",
    "tsv",
    "summary",
    "none",
    "raw",
    "pretty",
];

/// Resolve an output-flag VALUE to a written path, rejecting a bare FORMAT-SELECTOR
/// (`json`/`yaml`/`summary`/…) that carries no `/` and no `.extension` - such a value is a
/// render mode, not a file (the doc-comment claim on [`OUTPUT_FLAGS`] that a format name is
/// "never misread" was only true once this guard existed). A path-shaped value
/// (`report.json` has a `.`, `/tmp/out` has a `/`) is NOT a format selector and passes.
pub(crate) fn flag_output_path(value: &str) -> Option<String> {
    let stripped = strip_quotes(value);
    if !stripped.contains(['/', '.']) && FORMAT_SELECTORS.contains(&stripped) {
        return None;
    }
    concrete_path(value)
}

/// `curl`/`wget` write to a LOCAL path only via an explicit output flag: `-o <path>`,
/// `--output <path>`/`--output=<path>` (curl + wget), or `wget -O <path>`. A `curl -O`
/// (uppercase, no path arg - the name is derived from the URL) has NO deterministic
/// local path, so it is intentionally skipped. The destination is emitted under the
/// download verb (`curl`/`wget`), is_create true.
pub(crate) fn emit_download_output(
    operands: &[&str],
    verb: &'static str,
    out: &mut Vec<BashMutation>,
) {
    // The long `--output` / `--output=` forms are owned by the generic
    // [`collect_flag_outputs`] scan (which runs on EVERY segment, `output` is in its
    // allowlist), so this arm handles ONLY the SHORT flags the generic `--name` scan
    // cannot see: curl `-o <path>`, and wget `-O <path>` / `--output-document <path>`.
    // (curl's `-O` derives the name from the URL → no deterministic local path → skip.)
    let mut i = 0usize;
    while i < operands.len() {
        let tok = operands[i];
        let takes_next =
            tok == "-o" || (verb == "wget" && matches!(tok, "-O" | "--output-document"));
        if takes_next {
            if let Some(next) = operands.get(i + 1) {
                if let Some(path) = path_operand(next) {
                    out.push(BashMutation { path, verb });
                }
                i += 2;
                continue;
            }
        }
        i += 1;
    }
}

/// `dd … of=<path>` - the output file is named by the `of=` operand. `path_operand`
/// rejects a `KEY=VALUE` token, so `of=` is parsed specially here (its value after the
/// `=` is the path). `of=/dev/null`-class sinks are dropped.
pub(crate) fn emit_dd(operands: &[&str], out: &mut Vec<BashMutation>) {
    for op in operands {
        if let Some(rest) = strip_quotes(op).strip_prefix("of=") {
            if is_dev_sink(rest) {
                continue;
            }
            // `rest` is a bare path (no KEY=VALUE wrapper now) - concrete-path filter.
            if let Some(path) = concrete_path(rest) {
                out.push(BashMutation { path, verb: "dd" });
            }
        }
    }
}

/// `zip [opts] <dest.zip> <input…>` - the FIRST non-flag operand is the archive being
/// created/updated (the only path `zip` writes). Later operands are inputs (read), so
/// only the destination is emitted.
pub(crate) fn emit_zip(operands: &[&str], out: &mut Vec<BashMutation>) {
    for op in operands {
        if op.starts_with('-') {
            continue; // a zip option flag (`-r`, `-9`, …).
        }
        if let Some(path) = path_operand(op) {
            out.push(BashMutation { path, verb: "zip" });
        }
        return; // only the first non-flag operand (the archive dest).
    }
}

/// `tar` with a CREATE flag writes an archive; emit that archive path (the inputs that
/// follow are READ, never mutated, so only the destination is reported - symmetric with
/// `zip`). Three idioms are handled:
/// - bundled short flags WITHOUT a dash: `tar czf <archive> …` - a create+file token like
///   `czf`/`cJf`/`tzf`(no `c`→skip). The archive is the NEXT operand;
/// - bundled short flags WITH a dash: `tar -czf <archive> …` - same, the archive is next;
/// - long flags: `tar --create --file=<archive>` (inline) or `--create --file <archive>`
///   (spaced).
///
/// A `-f<archive>`/`f<archive>` form where the path is glued to the flag bundle is also
/// supported (the path is the bundle tail after `f`). Only emits when a `c`/`--create` flag
/// is present, so `tar -xzf …` (extract) and `tar -tzf …` (list) write nothing.
pub(crate) fn emit_tar(operands: &[&str], out: &mut Vec<BashMutation>) {
    // Pass 1: detect a create flag and find where the archive path is.
    let mut has_create = false;
    let mut file_long_inline: Option<&str> = None; // `--file=<archive>`
    let mut want_file_next = false; // a `-…f` / `czf` / `--file` expects the NEXT operand
    let mut glued_file: Option<&str> = None; // `-czfARCHIVE` / `czfARCHIVE` tail after `f`
    let mut archive: Option<&str> = None;

    for op in operands {
        if want_file_next {
            archive = Some(op);
            want_file_next = false;
            continue;
        }
        if let Some(rest) = op.strip_prefix("--file=") {
            file_long_inline = Some(rest);
            continue;
        }
        if *op == "--file" {
            want_file_next = true;
            continue;
        }
        if *op == "--create" {
            has_create = true;
            continue;
        }
        if op.starts_with("--") {
            continue; // another long flag (`--gzip`, `--verbose`, …)
        }
        // A bundled short-flag group: `-czf`, `czf`, `-cf`, `cJf`, possibly with a glued
        // archive tail after `f` (`-czfARCHIVE`). Strip an optional leading `-`.
        let bundle = op.strip_prefix('-').unwrap_or(op);
        // Only treat it as a flag bundle if it is all flag letters (or has an `f`-glued
        // path). A bare input path (`src/`) has a `/` or is not a known flag-letter run.
        if let Some(fpos) = bundle.find('f') {
            // Letters before `f` are flags; bytes after `f` are a glued archive path (if any).
            let flags_part = &bundle[..fpos];
            if flags_part.contains('c') {
                has_create = true;
            }
            let tail = &bundle[fpos + 1..];
            if tail.is_empty() {
                want_file_next = true;
            } else {
                glued_file = Some(tail);
            }
        } else if bundle.chars().all(|c| c.is_ascii_alphabetic()) && bundle.contains('c') {
            // A create bundle with no `f` (archive goes to stdout) - nothing to emit, but
            // record the create so a separate `--file` could still apply.
            has_create = true;
        }
    }

    if !has_create {
        return;
    }
    let dest = archive.or(glued_file).or(file_long_inline);
    if let Some(d) = dest {
        if let Some(path) = concrete_path(d) {
            out.push(BashMutation { path, verb: "tar" });
        }
    }
}

/// Scan every token for an allowlisted output FLAG and emit its path. Two shapes:
/// `--name=<path>` (inline) and `--name <path>` (the value is the next token). Only the
/// [`OUTPUT_FLAGS`] names qualify, keeping precision tight. Independent of the segment's
/// leading verb (a test runner names its report path the same way regardless of verb).
pub(crate) fn collect_flag_outputs(tokens: &[MaskedTok], out: &mut Vec<BashMutation>) {
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = tokens[i];
        // Flag NAME detection reads the MASK so a `--output` that is part of a quoted prose
        // arg is never treated as a real output flag; the path VALUE is sliced from the
        // original token (a quoted destination stays intact).
        if let Some(name_eq) = tok.masked.strip_prefix("--") {
            if let Some((name, _)) = name_eq.split_once('=') {
                // `--name=<path>` inline form. The `--name=` prefix is unmasked ASCII, so
                // its byte length is the same in the original - slice the value from there.
                if OUTPUT_FLAGS.contains(&name) {
                    let value = &tok.orig[name.len() + 3..]; // `--` + name + `=`
                    if let Some(path) = flag_output_path(value) {
                        out.push(BashMutation {
                            path,
                            verb: "flag-output",
                        });
                    }
                }
                i += 1;
                continue;
            }
            // `--name <path>` spaced form: the value is the NEXT token - but only when
            // that token is NOT itself a flag (so `--output --verbose` does not consume
            // `--verbose` as a fabricated path and skip a real flag).
            if OUTPUT_FLAGS.contains(&name_eq) {
                if let Some(next) = tokens.get(i + 1) {
                    if !next.masked.starts_with('-') {
                        if let Some(path) = flag_output_path(next.orig) {
                            out.push(BashMutation {
                                path,
                                verb: "flag-output",
                            });
                        }
                        i += 2;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
}
