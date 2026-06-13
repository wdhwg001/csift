//! `get` subcommand — fetch ONE message by its address and print it IN FULL.
//!
//! The companion to `search`, which now stamps every hit with an `L<line>` address (and the
//! record's `uuid`). You skim with `search`, then `get` the single message whose full body /
//! tail you actually need — no drop to the raw jsonl.
//!
//! Two address forms (exactly one):
//! - `--line N` — the 1-based PHYSICAL line in ONE resolved transcript. A jsonl is
//!   append-only, so a line number is a stable address; the scope must resolve to a single
//!   file (`--session`, or `--session --subagent`, or a single-session PATH).
//! - `--uuid U` — the record's own globally-unique jsonl `uuid`; scope is optional (a
//!   `--session`/PATH scope just makes the scan fast instead of spanning every project).
//!
//! The record renders like a `search` exchange body: a header then every category-eligible
//! block at FULL length (via [`crate::search::full_record_hits`], the shared extraction).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use memchr::memmem;

use crate::cli::{GetArgs, OutputFormat};
use crate::model::Record;
use crate::parse::{mmap_bytes, parse_line};
use crate::path::{self, Caller, SubagentScope};
use crate::search::{category_glyph, category_label, full_record_hits};
use crate::subagent::{is_subagent_path, parent_session_id_from_path, session_id_from_path};
use crate::timez::{format_timestamp, local_iso};

/// A located single record + its transcript identity (the `csift get` result).
struct Located {
    session_id: String,
    is_subagent: bool,
    parent_session_id: String,
    line: usize,
    rec: Record,
}

pub fn run_get(args: &GetArgs) -> Result<()> {
    // ── exactly one address ──
    let located = match (args.line, args.uuid.as_deref()) {
        (Some(_), Some(_)) => {
            bail!("--line and --uuid are mutually exclusive — give exactly ONE address")
        }
        (None, None) => bail!(
            "give an address: `--line N` (with `--session <uuid>` or a single-session scope), \
             or `--uuid U`. Copy either from a `search` hit's `L<line>` header / JSON."
        ),
        (Some(line), None) => locate_by_line(args, line)?,
        (None, Some(uuid)) => locate_by_uuid(args, uuid)?,
    };

    match args.format {
        OutputFormat::Text => render_text(&located),
        OutputFormat::Json => render_json(&located)?,
    }
    Ok(())
}

/// Resolve `--line N` to a record: the scope must pin exactly ONE transcript.
fn locate_by_line(args: &GetArgs, line: usize) -> Result<Located> {
    if line == 0 {
        bail!("--line is 1-based; line 0 does not exist");
    }
    // `--subagent` needs the subagent transcripts in scope; otherwise the top-level only.
    let scope = if args.subagent.is_some() {
        SubagentScope::WithSubagents
    } else {
        SubagentScope::TopLevelOnly
    };
    let files =
        path::resolve_session_files(&args.paths, args.session.as_deref(), scope, Caller::Other)?;

    let target: Vec<PathBuf> = if let Some(hex) = args.subagent.as_deref() {
        files
            .into_iter()
            .filter(|p| is_subagent_path(p) && session_id_from_path(p) == hex)
            .collect()
    } else {
        files.into_iter().filter(|p| !is_subagent_path(p)).collect()
    };

    let path = match target.as_slice() {
        [one] => one.clone(),
        [] => {
            if args.subagent.is_some() {
                bail!(
                    "--line: no subagent transcript `{}` found in scope — pass its parent \
                     `--session <uuid>` and check the hex with `csift agents`",
                    args.subagent.as_deref().unwrap_or("")
                )
            }
            bail!(
                "--line: the scope resolves to no single transcript — add `--session <uuid>` \
                 (line N of WHICH session?)"
            )
        }
        many => bail!(
            "--line is ambiguous: the scope resolves to {} transcripts. Narrow it with \
             `--session <uuid>` (or `--session <uuid> --subagent <hex>`) so line {line} names \
             one file.",
            many.len()
        ),
    };

    let raw = read_physical_line(&path, line)?
        .with_context(|| format!("line {line} is past the end of {}", path.display()))?;
    let rec = match parse_line(raw.as_bytes()) {
        Ok(Some(rec)) => rec,
        Ok(None) | Err(_) => bail!(
            "line {line} of {} is not a transcript message (e.g. an attachment / summary / \
             malformed line) — pick a line a `search` hit reported",
            path.display()
        ),
    };
    Ok(located_from(&path, line, rec))
}

/// Resolve `--uuid U` to a record by scanning the scoped transcripts (uuid is globally
/// unique → first match is the only match). A literal-substring prefilter skips non-matching
/// lines before the JSON parse.
fn locate_by_uuid(args: &GetArgs, uuid: &str) -> Result<Located> {
    let files = path::resolve_session_files(
        &args.paths,
        args.session.as_deref(),
        SubagentScope::WithSubagents,
        Caller::Other,
    )?;
    let needle = memmem::Finder::new(uuid.as_bytes());
    for path in &files {
        let Some(mmap) = mmap_bytes(path)? else {
            continue;
        };
        for (i, raw) in mmap.split(|&b| b == b'\n').enumerate() {
            if needle.find(raw).is_none() {
                continue; // the uuid can't be on this line — skip the parse
            }
            if let Ok(Some(rec)) = parse_line(raw) {
                if rec.uuid.as_deref() == Some(uuid) {
                    return Ok(located_from(path, i + 1, rec));
                }
            }
        }
    }
    bail!(
        "no record with uuid {uuid} found in scope — widen the scope (drop `--session`/PATH) \
         or re-check the id"
    )
}

/// Read the 1-based physical line `line_no` of a jsonl, or `None` if past EOF.
fn read_physical_line(path: &Path, line_no: usize) -> Result<Option<String>> {
    let Some(mmap) = mmap_bytes(path)? else {
        return Ok(None);
    };
    for (i, raw) in mmap.split(|&b| b == b'\n').enumerate() {
        if i + 1 == line_no {
            return Ok(Some(String::from_utf8_lossy(raw).into_owned()));
        }
    }
    Ok(None)
}

fn located_from(path: &Path, line: usize, rec: Record) -> Located {
    let session_id = session_id_from_path(path);
    let is_subagent = is_subagent_path(path);
    let parent_session_id = parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());
    Located {
        session_id,
        is_subagent,
        parent_session_id,
        line,
        rec,
    }
}

fn render_text(loc: &Located) {
    let ts = format_timestamp(loc.rec.timestamp.as_deref());
    let uuid = loc.rec.uuid.as_deref().unwrap_or("-");
    if loc.is_subagent {
        println!(
            "═══ SUBAGENT {} · parent SESSION {} · L{} · {uuid} · {ts} ═══",
            loc.session_id, loc.parent_session_id, loc.line
        );
    } else {
        println!(
            "═══ SESSION {} · L{} · {uuid} · {ts} ═══",
            loc.session_id, loc.line
        );
    }
    let hits = full_record_hits(&loc.rec);
    if hits.is_empty() {
        let kind = loc.rec.r#type.as_deref().unwrap_or("unknown");
        println!("(no renderable content — record type: {kind})");
        return;
    }
    for h in &hits {
        let glyph = category_glyph(h.category);
        let label = category_label(h.category);
        let name = h
            .tool_name
            .as_deref()
            .map(|n| format!(" {n}"))
            .unwrap_or_default();
        println!("{glyph} {label}{name}");
        println!("   {}", h.excerpt);
    }
}

fn render_json(loc: &Located) -> Result<()> {
    use serde_json::json;
    let blocks: Vec<_> = full_record_hits(&loc.rec)
        .iter()
        .map(|h| {
            json!({
                "category": category_label(h.category),
                "text": h.excerpt,
                "tool_name": h.tool_name,
            })
        })
        .collect();
    let obj = json!({
        "session_id": loc.session_id,
        "is_subagent": loc.is_subagent,
        "parent_session_id": loc.parent_session_id,
        "line": loc.line,
        "uuid": loc.rec.uuid,
        "type": loc.rec.r#type,
        "ts_utc": loc.rec.timestamp,
        "ts_local": loc.rec.timestamp.as_deref().and_then(local_iso),
        "blocks": blocks,
    });
    println!("{}", serde_json::to_string(&obj)?);
    Ok(())
}
