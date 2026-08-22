//! Mutating-CLASS markers: commands known to rewrite files without naming them.
//!
//! A formatter (`cargo fmt`), a package manager (`npm install`), an archive extraction
//! (`tar -xf`), and a patch application all mutate real files whose names never appear
//! in the command text: the file set comes from the tool's own discovery, the lockfile
//! layout, or the archive/diff contents. No lexical parser can name those files, and
//! fabricating names would break the precision contract. So these commands are
//! reported as CLASS MARKERS in the `git:<sub>` style: a pseudo-path (`fmt:cargo`,
//! `pkg:npm`, `extract:tar`) that flags WHAT KIND of mutation ran, deliberately not
//! WHICH files. Markers are never resolved, joined, or matched against a `--file`;
//! `recover` counts them per window as opaque mutating activity.
//!
//! When a formatter DOES name file operands (`npx prettier --write a.ts b.ts`,
//! `rustfmt src/x.rs`), those operands are real targets and are emitted as ordinary
//! path rows (verb `fmt`) instead of a marker. Dry-run forms (`cargo fmt --check`,
//! `black --check`, `prettier` without `--write`, `eslint` without `--fix`) verify
//! and write nothing, so they emit nothing.

use super::*;

/// Dispatch a segment's verb to a class emitter. An unrecognized verb emits nothing.
pub(crate) fn emit_class(verb: &str, operands: &[&str], out: &mut Vec<BashMutation>) {
    match verb {
        // Runner prefixes: the real tool is the next token.
        "npx" | "bunx" => {
            if let Some((&tool, rest)) = operands.split_first() {
                emit_class(tool, rest, out);
            }
        }
        "cargo" => match operands.first().copied() {
            Some("fmt") if !has_flag(operands, &["--check"]) => marker(out, "fmt", "cargo"),
            Some("add" | "update" | "remove") => marker(out, "pkg", "cargo"),
            _ => {}
        },
        "rustfmt" => {
            if !has_flag(operands, &["--check"]) {
                fmt_operands_or_marker(operands, "rustfmt", out);
            }
        }
        "prettier" => {
            if has_flag(operands, &["--write", "-w"]) {
                fmt_operands_or_marker(operands, "prettier", out);
            }
        }
        "black" => {
            if !has_flag(operands, &["--check", "--diff"]) {
                fmt_operands_or_marker(operands, "black", out);
            }
        }
        "ruff" => emit_ruff(operands, out),
        "eslint" => {
            if has_flag(operands, &["--fix"]) {
                fmt_operands_or_marker(operands, "eslint", out);
            }
        }
        "gofmt" => {
            if has_flag(operands, &["-w"]) {
                fmt_operands_or_marker(operands, "gofmt", out);
            }
        }
        "clang-format" => {
            if has_flag(operands, &["-i"]) {
                fmt_operands_or_marker(operands, "clang-format", out);
            }
        }
        "dprint" => {
            if operands.first() == Some(&"fmt") {
                marker(out, "fmt", "dprint");
            }
        }
        "npm" | "pnpm" | "yarn" | "bun" => emit_node_runner(verb, operands, out),
        "pip" | "pip3" => {
            if matches!(operands.first().copied(), Some("install" | "uninstall")) {
                marker(out, "pkg", "pip");
            }
        }
        "poetry" => {
            if matches!(
                operands.first().copied(),
                Some("install" | "add" | "update" | "lock" | "remove")
            ) {
                marker(out, "pkg", "poetry");
            }
        }
        "uv" => {
            if matches!(
                operands.first().copied(),
                Some("pip" | "sync" | "add" | "lock")
            ) {
                marker(out, "pkg", "uv");
            }
        }
        "unzip" => {
            // `-l`/`-t` list/test without extracting.
            if !has_flag(operands, &["-l", "-t"]) {
                marker(out, "extract", "unzip");
            }
        }
        "7z" | "7za" | "7zz" => {
            if matches!(operands.first().copied(), Some("x" | "e")) {
                marker(out, "extract", "7z");
            }
        }
        "patch" => {
            if !has_flag(operands, &["--dry-run"]) {
                marker(out, "extract", "patch");
            }
        }
        _ => {}
    }
}

/// `tar` extraction (the create side is handled by `emit_tar`): a bundled `x` flag or
/// `--extract` writes the archive MEMBERS, whose names live inside the archive - an
/// `extract:tar` marker, never invented member paths. A list/test run (`-tzf`) emits
/// nothing.
pub(crate) fn tar_extract_marker(operands: &[&str], out: &mut Vec<BashMutation>) {
    let dash_extract = operands.iter().any(|t| {
        *t == "--extract" || (t.starts_with('-') && !t.starts_with("--") && t.contains('x'))
    });
    // The dashless old-style bundle (`tar xf arch`) is only ever the FIRST operand,
    // so a later input path that happens to contain `x` and `f` never matches.
    let bare_bundle = operands
        .first()
        .is_some_and(|t| !t.starts_with('-') && t.len() <= 5 && t.contains('x') && t.contains('f'));
    if dash_extract || bare_bundle {
        marker(out, "extract", "tar");
    }
}

/// `ruff format [files…]` / `ruff check --fix [files…]`: the subcommand word is not a
/// file operand, so it is peeled before the operand scan.
fn emit_ruff(operands: &[&str], out: &mut Vec<BashMutation>) {
    let (sub, rest) = match operands.split_first() {
        Some((&s, r)) if s == "format" || s == "check" => (Some(s), r),
        _ => (None, operands),
    };
    let formats = sub == Some("format") || has_flag(rest, &["--fix"]);
    if formats && !has_flag(rest, &["--check", "--diff"]) {
        fmt_operands_or_marker(rest, "ruff", out);
    }
}

/// The `npm`/`pnpm`/`yarn`/`bun` runner family: an install-class subcommand is a
/// `pkg:` marker; a `run` alias naming a formatter intent is an `fmt:npm-script`
/// marker (the alias hides the real tool entirely; the alias NAME is the only signal).
fn emit_node_runner(verb: &str, operands: &[&str], out: &mut Vec<BashMutation>) {
    match operands.first().copied() {
        Some("run") => {
            if operands
                .get(1)
                .is_some_and(|n| n.contains("format") || n.contains("fix"))
            {
                marker(out, "fmt", "npm-script");
            }
        }
        Some("install" | "i" | "ci" | "update" | "add" | "remove" | "uninstall") => {
            marker(out, "pkg", pkg_manager_name(verb));
        }
        Some("audit") if operands.get(1) == Some(&"fix") => {
            marker(out, "pkg", pkg_manager_name(verb));
        }
        _ => {}
    }
}

fn marker(out: &mut Vec<BashMutation>, class: &str, tool: &str) {
    out.push(BashMutation {
        path: format!("{class}:{tool}"),
        verb: class_verb(class),
        cwd_at: CwdAt::Spawn,
    });
}

/// The static verb string for a marker class.
fn class_verb(class: &str) -> &'static str {
    match class {
        "fmt" => "fmt",
        "pkg" => "pkg",
        _ => "extract",
    }
}

/// Formatter with explicit operands: emit each path operand as a real `fmt` row;
/// with no usable operand, fall back to the class marker.
fn fmt_operands_or_marker(operands: &[&str], tool: &str, out: &mut Vec<BashMutation>) {
    let before = out.len();
    for op in non_flag_operands(operands) {
        if let Some(path) = path_operand(op) {
            out.push(BashMutation {
                path,
                verb: "fmt",
                cwd_at: CwdAt::Spawn,
            });
        }
    }
    if out.len() == before {
        marker(out, "fmt", tool);
    }
}

fn has_flag(operands: &[&str], flags: &[&str]) -> bool {
    operands.iter().any(|t| flags.contains(t))
}

/// The pkg marker tool name for a runner verb (static strings only).
fn pkg_manager_name(verb: &str) -> &'static str {
    match verb {
        "pnpm" => "pnpm",
        "yarn" => "yarn",
        "bun" => "bun",
        _ => "npm",
    }
}
