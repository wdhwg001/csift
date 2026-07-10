//! `show` subcommand — fetch specific record(s) of ONE transcript.
//!
//! The reader companion to `search`: search FINDS (match-centered excerpts across a
//! scope), `show` FETCHES the records you name — by 1-based jsonl line number (the
//! `Lnnnn` every csift surface prints) or by record uuid — rendered FULL through the
//! same per-record pipeline search hits use (classify → labels, plan pointers, tool
//! pairing, elicitation-sidecar merge). `--raw` instead emits the VERBATIM raw jsonl
//! line(s): the escape hatch for fields csift does not render (usage tokens,
//! stop_reason, model, …) and for inspecting corruption — raw reads the transcript
//! file only (no sidecar merge, no record parsing).
//!
//! Addressing discipline (SPEC §6.12): an EXPLICITLY named line/uuid that resolves to
//! no record is a HARD error (address-miss = error; filter-empty = ok is the
//! tool-wide exit law); a range clamps to the file but errors when it yields nothing.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Result};

use crate::cli::{OutputFormat, ShowArgs};
use crate::parse::{mmap_bytes, scan_lines_bytes};
use crate::path::{self, SubagentScope};
use crate::search::{fetch_records, merged_any_sidecar, print_record_line, role_glyph, Exchange};
use crate::timez::{format_local_compact, local_iso};

/// Parsed `--line` tokens: the EXPLICIT singletons (each must resolve — a miss is a
/// hard error) and the inclusive ranges (each clamps, but must yield ≥1 record).
#[derive(Debug, Default)]
struct LineSpecs {
    explicit: BTreeSet<usize>,
    ranges: Vec<(usize, usize)>,
}

impl LineSpecs {
    /// Every addressed line, expanded (ranges included).
    fn all(&self) -> BTreeSet<usize> {
        let mut out = self.explicit.clone();
        for &(a, b) in &self.ranges {
            out.extend(a..=b);
        }
        out
    }
}

/// Parse `--line` tokens (already comma-split by clap) into `(is_explicit, RangeSpec)` pairs
/// via the shared [`crate::text::parse_range_spec`] grammar (`N` · `A..B` · `N..` · `..N` ·
/// `-k` from the end). A single token (no `..`) is an EXPLICIT address (miss = hard error); a
/// `..` token is a clamping range. Open/from-end forms resolve against the file's line count in
/// [`resolve_line_specs`]. No subagent prefix — the TARGET names the transcript.
fn parse_line_specs(tokens: &[String]) -> Result<Vec<(bool, crate::text::RangeSpec)>> {
    let mut out = Vec::new();
    for tok in tokens {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        let is_explicit = !t.contains("..");
        out.push((
            is_explicit,
            crate::text::parse_range_spec(t, "--line", true)?,
        ));
    }
    Ok(out)
}

/// Resolve parsed `--line` specs against the transcript's line count (1-based), materializing
/// open/from-end forms. A single-token spec becomes an explicit address; a range spec clamps.
fn resolve_line_specs(parsed: &[(bool, crate::text::RangeSpec)], total_lines: usize) -> LineSpecs {
    let mut specs = LineSpecs::default();
    for (is_explicit, spec) in parsed {
        let (lo, hi) = spec.resolve(total_lines, true);
        if *is_explicit {
            specs.explicit.insert(lo); // lo == hi for a single token
        } else {
            specs.ranges.push((lo, hi));
        }
    }
    specs
}

/// Count a transcript's physical jsonl lines (newlines + a torn final fragment), the 1-based
/// domain the `--line` from-end / open forms resolve against. An empty/missing file ⇒ 0.
fn count_lines(file: &std::path::Path) -> Result<usize> {
    let Some(mmap) = mmap_bytes(file)? else {
        return Ok(0);
    };
    let bytes: &[u8] = &mmap;
    Ok(memchr::memchr_iter(b'\n', bytes).count()
        + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n")))
}

/// Resolve the TARGET to exactly ONE transcript file. `@<uuid>` → that top-level
/// transcript (never spans); `@<agent-id>` → that subagent transcript alone; a
/// `*.jsonl` path → that file. Anything resolving to ≠1 file is a pointed error.
fn resolve_single_transcript(target: &std::path::Path) -> Result<PathBuf> {
    let files = path::resolve_session_files(
        std::slice::from_ref(&target.to_path_buf()),
        SubagentScope::TopLevelOnly,
        path::Caller::Other,
    )?;
    match files.as_slice() {
        [one] => Ok(one.clone()),
        many => bail!(
            "show targets exactly ONE transcript — '{}' resolves to {}. Name one: \
             `@<uuid>` (a top-level session) | `@<agent-id>` (a subagent, ids from \
             `csift agents`) | a `*.jsonl` path.",
            target.display(),
            many.len()
        ),
    }
}

/// Entry point for `csift show`.
pub fn run_show(args: &ShowArgs) -> Result<()> {
    if args.raw && args.format == OutputFormat::Json {
        bail!("--raw is mutually exclusive with --format json (raw IS the file's own JSON)");
    }
    let parsed = parse_line_specs(&args.line)?;
    let uuids: BTreeSet<String> = args
        .uuid
        .iter()
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
        .collect();
    // `--turn` addresses by turn index (0-based, the `s1·tN` search prints) rather than by
    // jsonl line — the SAME shared range grammar (`N`/`A..B`/`N..`/`-k`). Clap forbids mixing
    // it with `--line`/`--uuid`, so it is a self-contained addressing mode.
    let turn_spec = args
        .turn
        .as_deref()
        .map(|s| crate::text::parse_range_spec(s, "--turn", false))
        .transpose()?;

    let file = resolve_single_transcript(&args.target)?;
    if parsed.is_empty() && uuids.is_empty() && turn_spec.is_none() {
        bail!(
            "show needs an address: `--line <N|A..B|N..|-k>` (1-based jsonl lines, the `Lnnnn` \
             other csift commands print), `--turn <N|A..B|-k>` (0-based turn index, the `tN` \
             search prints), and/or `--uuid <U>`. The transcript is {} — csift never dumps a \
             whole transcript into your context.",
            file.display()
        );
    }
    // Resolve `--line` open/from-end forms (`N..`, `-20..` = the last 20) against this
    // transcript's line count, materializing the concrete explicit/range addresses.
    let specs = resolve_line_specs(&parsed, count_lines(&file)?);

    if args.raw {
        return run_raw(&file, &specs, &uuids, turn_spec);
    }
    run_rendered(&file, &specs, &uuids, turn_spec, args.format)
}

/// `--raw`: emit the verbatim bytes of each addressed jsonl line, ascending, exactly
/// as stored (a blank or torn line included — that is the point).
fn run_raw(
    file: &std::path::Path,
    specs: &LineSpecs,
    uuids: &BTreeSet<String>,
    turn_range: Option<crate::text::RangeSpec>,
) -> Result<()> {
    let Some(mmap) = mmap_bytes(file)? else {
        bail!("transcript {} is empty", file.display());
    };
    let bytes: &[u8] = &mmap;

    // uuid → line resolution: parse only the lines whose raw bytes contain a wanted
    // uuid (cheap memmem prefilter), then confirm against the parsed record's own uuid.
    let mut uuid_line: std::collections::BTreeMap<&String, Option<usize>> =
        uuids.iter().map(|u| (u, None)).collect();
    let mut wanted_lines = specs.all();
    // `--turn --raw`: resolve the turn range to its records' physical lines (via the shared
    // grouping, so the turn numbering matches `search`), then emit those lines verbatim.
    if let Some(spec) = turn_range {
        let (exchanges, _) = fetch_records(file, BTreeSet::new(), BTreeSet::new(), Some(spec))?;
        for ex in &exchanges {
            for h in &ex.hits {
                if h.line > 0 {
                    wanted_lines.insert(h.line);
                }
            }
        }
    }
    let mut total_lines = 0usize;
    let mut keep: std::collections::BTreeMap<usize, Vec<u8>> = std::collections::BTreeMap::new();

    let mut line_no = 0usize;
    scan_lines_bytes(bytes, |line| {
        line_no += 1;
        total_lines = line_no;
        let mut want = wanted_lines.contains(&line_no);
        if !want && !uuids.is_empty() {
            for (u, slot) in uuid_line.iter_mut() {
                if slot.is_none() && memchr::memmem::find(line, u.as_bytes()).is_some() {
                    // Confirm structurally: the record's OWN uuid must equal it (a body
                    // merely quoting the uuid must not satisfy the address).
                    if let Ok(Some(rec)) = crate::parse::parse_line(line) {
                        if rec.uuid.as_deref() == Some(u.as_str()) {
                            *slot = Some(line_no);
                            want = true;
                        }
                    }
                }
            }
        }
        if want {
            keep.insert(line_no, line.to_vec());
        }
    })?;

    // Address-miss = error: explicit lines beyond EOF, unresolved uuids, empty ranges.
    let mut misses: Vec<String> = specs
        .explicit
        .iter()
        .filter(|&&n| n > total_lines)
        .map(|n| format!("L{n} (file has {total_lines} lines)"))
        .collect();
    for &(a, b) in &specs.ranges {
        if a > total_lines {
            misses.push(format!("L{a}-{b} (file has {total_lines} lines)"));
        }
    }
    for (u, slot) in &uuid_line {
        if slot.is_none() {
            misses.push(format!("uuid {u}"));
        }
    }
    if !misses.is_empty() {
        bail!("no such record(s): {}", misses.join(", "));
    }

    let mut out = std::io::stdout().lock();
    for line in keep.values() {
        out.write_all(line)?;
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// Rendered mode: fetch through search's per-record pipeline (pure matcher — every
/// addressed record emits, FULL), then render text or the header/record/summary JSON.
fn run_rendered(
    file: &std::path::Path,
    specs: &LineSpecs,
    uuids: &BTreeSet<String>,
    turn_range: Option<crate::text::RangeSpec>,
    format: OutputFormat,
) -> Result<()> {
    let (exchanges, skipped) = fetch_records(file, specs.all(), uuids.clone(), turn_range)?;

    // Address-miss accounting (explicit lines + uuids must resolve; ranges must yield ≥1).
    let mut hit_lines: BTreeSet<usize> = BTreeSet::new();
    let mut hit_uuids: BTreeSet<&str> = BTreeSet::new();
    for ex in &exchanges {
        for h in &ex.hits {
            hit_lines.insert(h.line);
            if let Some(u) = h.uuid.as_deref() {
                hit_uuids.insert(u);
            }
        }
    }
    let mut misses: Vec<String> = specs
        .explicit
        .iter()
        .filter(|n| !hit_lines.contains(n))
        .map(|n| format!("L{n}"))
        .collect();
    for &(a, b) in &specs.ranges {
        if !hit_lines.iter().any(|n| (a..=b).contains(n)) {
            misses.push(format!("L{a}-{b}"));
        }
    }
    for u in uuids {
        if !hit_uuids.contains(u.as_str()) {
            misses.push(format!("uuid {u}"));
        }
    }
    if !misses.is_empty() {
        bail!(
            "no such record(s): {} — a rendered record is a `role:user`/`role:assistant` \
             message line (metadata/attachment lines are not records; inspect those with \
             `--raw`)",
            misses.join(", ")
        );
    }

    let session_id = crate::subagent::session_id_from_path(file);
    let is_subagent = crate::subagent::is_subagent_path(file);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(file).unwrap_or_else(|| session_id.clone());

    match format {
        OutputFormat::Text => {
            render_text(
                &exchanges,
                &session_id,
                is_subagent,
                &parent_session_id,
                skipped,
            );
        }
        OutputFormat::Json => render_json(
            &exchanges,
            file,
            &session_id,
            is_subagent,
            &parent_session_id,
            skipped,
        )?,
    }
    Ok(())
}

fn render_text(
    exchanges: &[Exchange],
    session_id: &str,
    is_subagent: bool,
    parent_session_id: &str,
    skipped: usize,
) {
    if is_subagent {
        println!("SUBAGENT {session_id} · parent SESSION {parent_session_id}");
    } else {
        println!("SESSION {session_id}");
    }
    let mut units = 0usize;
    for ex in exchanges {
        println!();
        println!(
            "t{}  {}",
            ex.turn_index,
            format_local_compact(ex.started_utc.as_deref())
        );
        for h in &ex.hits {
            print_record_line(role_glyph(h.class), h);
            units += 1;
        }
    }
    println!();
    println!("fetched {units} record unit(s)");
    if merged_any_sidecar(exchanges) {
        println!("with elicitation sidecar");
    }
    if skipped > 0 {
        println!("({})", crate::text::malformed_note(skipped));
    }
}

fn render_json(
    exchanges: &[Exchange],
    file: &std::path::Path,
    session_id: &str,
    is_subagent: bool,
    parent_session_id: &str,
    skipped: usize,
) -> Result<()> {
    use serde_json::json;
    let header = json!({
        "kind": "header",
        "command": "show",
        "session_id": session_id,
        "is_subagent": is_subagent,
        "parent_session_id": parent_session_id,
        "path": file.display().to_string(),
    });
    println!("{}", serde_json::to_string(&header)?);
    let mut units = 0usize;
    for ex in exchanges {
        for h in &ex.hits {
            units += 1;
            let (from, to) = match &h.direction {
                Some((f, t)) => (json!(f), json!(t)),
                None => (serde_json::Value::Null, serde_json::Value::Null),
            };
            let row = json!({
                "kind": "record",
                "session_id": session_id,
                "is_subagent": is_subagent,
                "parent_session_id": parent_session_id,
                "turn_index": ex.turn_index,
                // A merged elicitation-sidecar record has no physical line (null).
                "line": if h.from_sidecar { serde_json::Value::Null } else { json!(h.line) },
                "uuid": h.uuid,
                "label": h.class.path(),
                "labels": h.labels,
                "tool_name": h.tool_name,
                "from": from,
                "to": to,
                "pairing": crate::search::pairing_json(h.pair),
                "tool_use_id": h.tool_use_id,
                "source": if h.from_sidecar { json!("elicitation-sidecar") } else { serde_json::Value::Null },
                "ts_utc": h.timestamp_utc,
                "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
                "text": h.excerpt,
                "image_ids": h.image_ids,
            });
            println!("{}", serde_json::to_string(&row)?);
        }
    }
    let summary = json!({
        "kind": "summary",
        "records": units,
        "skipped_lines": skipped,
        "with_elicitation_sidecar": merged_any_sidecar(exchanges),
    });
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
