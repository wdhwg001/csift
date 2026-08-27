//! `show --branch-points`: conversation FORK facts for one transcript.
//!
//! A Claude Code rewind, retry, or parallel lane leaves one plain DAG fact: some record
//! has MORE THAN ONE conversation child (a later `parentUuid` re-attach). Which side is
//! "live" is NOT computable from the jsonl: a live/abandoned classifier was prototyped
//! and refuted against real corpora (parallel tool fan-out makes sibling leaves that
//! false-positive as abandoned branches on most sessions). So csift reports the facts
//! and RANKS them by the widest inter-child time gap: a rewind usually shows a wide
//! gap, a parallel lane a near-zero one. The reader applies judgment; csift never
//! classifies.
//!
//! A "conversation child" is a `user`/`assistant` record, EXCLUDING user records that
//! carry a `tool_result` block (parallel tool results share a parent by construction),
//! `isMeta` records, and compaction summaries.

use super::*;
use crate::model::{Block, Record};
use crate::parse::LineVerdict;
use serde_json::json;

/// One child edge of a branch point.
#[derive(Debug)]
struct Child {
    line: usize,
    uuid: Option<String>,
    ts_utc: Option<String>,
    record_type: String,
}

/// One record with 2+ conversation children.
#[derive(Debug)]
struct BranchPoint {
    uuid: String,
    /// The parent record's own jsonl line; `None` when its uuid was not located among
    /// the parsed role-candidate lines (a clipped parent, or a non-conversation line).
    line: Option<usize>,
    children: Vec<Child>,
    /// Widest gap between CONSECUTIVE children (file order), in whole seconds; `None`
    /// when any needed timestamp is absent or unparseable.
    widest_gap_secs: Option<i64>,
}

fn is_conversation_record(rec: &Record) -> bool {
    match rec.r#type.as_deref() {
        Some("assistant") => true,
        Some("user") => {
            !rec.is_meta.unwrap_or(false)
                && !rec.is_compact_summary.unwrap_or(false)
                && !rec
                    .blocks()
                    .is_some_and(|bs| bs.iter().any(|b| matches!(b, Block::ToolResult { .. })))
        }
        _ => false,
    }
}

fn parse_ts(raw: &str) -> Option<jiff::Timestamp> {
    raw.parse().ok()
}

/// `7190` → `1h59m50s` (compact, no zero-padding of the leading unit).
fn gap_label(secs: i64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h{m:02}m{s:02}s")
    } else if m > 0 {
        format!("{m}m{s:02}s")
    } else {
        format!("{s}s")
    }
}

pub(crate) fn run_branch_points(file: &std::path::Path, format: OutputFormat) -> Result<()> {
    let session_id = crate::subagent::session_id_from_path(file);
    let is_subagent = crate::subagent::is_subagent_path(file);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(file).unwrap_or_else(|| session_id.clone());

    let mut recs: Vec<(usize, Record)> = Vec::new();
    let mut skipped = 0usize;
    if let Some(mmap) = mmap_bytes(file)? {
        let bytes: &[u8] = &mmap;
        let (kept, s) = crate::parse::scan_lines_parallel(bytes, |line, line_no| {
            if !crate::parse::line_has_role_marker(line) {
                return crate::parse::non_candidate_verdict(line);
            }
            match crate::parse::parse_line(line) {
                Ok(Some(rec)) => LineVerdict::Keep((line_no, rec)),
                Ok(None) => LineVerdict::Ignore,
                Err(_) => LineVerdict::Skip,
            }
        });
        recs = kept;
        skipped = s;
    }

    // uuid → own line, for locating each branch parent (ANY record can be a parent).
    let line_of: std::collections::HashMap<&str, usize> = recs
        .iter()
        .filter_map(|(l, r)| r.uuid.as_deref().map(|u| (u, *l)))
        .collect();
    // parentUuid → conversation children, file order.
    let mut children_of: std::collections::HashMap<String, Vec<Child>> =
        std::collections::HashMap::new();
    let mut conversation_records = 0usize;
    for (line, rec) in &recs {
        if !is_conversation_record(rec) {
            continue;
        }
        conversation_records += 1;
        let Some(parent) = rec.parent_uuid.as_deref() else {
            continue;
        };
        children_of
            .entry(parent.to_string())
            .or_default()
            .push(Child {
                line: *line,
                uuid: rec.uuid.clone(),
                ts_utc: rec.timestamp.clone(),
                record_type: rec
                    .r#type
                    .clone()
                    .unwrap_or_else(|| "(untyped)".to_string()),
            });
    }

    let mut points: Vec<BranchPoint> = children_of
        .into_iter()
        .filter(|(_, ch)| ch.len() >= 2)
        .map(|(uuid, mut children)| {
            children.sort_by_key(|c| c.line);
            let mut widest: Option<i64> = None;
            for pair in children.windows(2) {
                let gap = match (pair[0].ts_utc.as_deref(), pair[1].ts_utc.as_deref()) {
                    (Some(a), Some(b)) => match (parse_ts(a), parse_ts(b)) {
                        (Some(ta), Some(tb)) => {
                            Some((tb.as_second() - ta.as_second()).unsigned_abs() as i64)
                        }
                        _ => None,
                    },
                    _ => None,
                };
                match gap {
                    // A single undated seam makes the whole ranking honest-unknown.
                    None => {
                        widest = None;
                        break;
                    }
                    Some(g) => widest = Some(widest.map_or(g, |w: i64| w.max(g))),
                }
            }
            BranchPoint {
                line: line_of.get(uuid.as_str()).copied(),
                uuid,
                children,
                widest_gap_secs: widest,
            }
        })
        .collect();
    // Ranked: widest gap first (unknown gaps last), then first-child line for stability.
    points.sort_by(|a, b| {
        let key = |p: &BranchPoint| {
            (
                p.widest_gap_secs.is_none(),
                std::cmp::Reverse(p.widest_gap_secs.unwrap_or(0)),
                p.children.first().map_or(0, |c| c.line),
            )
        };
        key(a).cmp(&key(b))
    });

    match format {
        OutputFormat::Text => {
            render_branch_text(&session_id, conversation_records, &points, skipped);
        }
        OutputFormat::Json => render_branch_json(
            &session_id,
            is_subagent,
            &parent_session_id,
            conversation_records,
            &points,
            skipped,
        )?,
    }
    Ok(())
}

fn render_branch_text(
    session_id: &str,
    conversation_records: usize,
    points: &[BranchPoint],
    skipped: usize,
) {
    println!("BRANCH POINTS  {session_id}");
    println!(
        "  {conversation_records} conversation record(s) · {} branch point(s) (a record \
         with 2+ conversation children; tool-result carriers, isMeta records, and \
         compaction summaries never count)",
        points.len()
    );
    if points.is_empty() {
        println!("  no forks: every conversation record has at most one conversation child");
    }
    for (i, p) in points.iter().enumerate() {
        let loc = p.line.map_or_else(
            || "(parent line not located)".to_string(),
            |l| format!("L{l}"),
        );
        let gap = p
            .widest_gap_secs
            .map_or_else(|| "unknown (missing timestamps)".to_string(), gap_label);
        println!();
        println!(
            "  #{}  uuid {}  {loc}  children {} · widest gap {gap}",
            i + 1,
            p.uuid,
            p.children.len()
        );
        for c in &p.children {
            println!(
                "      L{}  {}  {}",
                c.line,
                crate::timez::format_timestamp(c.ts_utc.as_deref()),
                c.record_type
            );
        }
        if let Some(last) = p.children.last() {
            println!("      ↳ csift show @{session_id} --line {}", last.line);
        }
    }
    if points.iter().any(|p| p.widest_gap_secs.is_some()) {
        println!();
        println!(
            "  ranked by widest inter-child gap: a rewind or retry fork usually shows a \
             wide gap, a parallel lane a near-zero one. csift reports fork FACTS; it does \
             not guess which branch is live."
        );
    }
    if skipped > 0 {
        println!("  ({})", crate::text::malformed_note(skipped));
    }
}

fn render_branch_json(
    session_id: &str,
    is_subagent: bool,
    parent_session_id: &str,
    conversation_records: usize,
    points: &[BranchPoint],
    skipped: usize,
) -> Result<()> {
    let header = crate::text::envelope_header(
        "show",
        json!({
            "mode": "branch-points",
            "session_id": session_id,
            "is_subagent": is_subagent,
            "parent_session_id": parent_session_id,
        }),
    );
    println!("{}", serde_json::to_string(&header)?);
    for p in points {
        let children: Vec<serde_json::Value> = p
            .children
            .iter()
            .map(|c| {
                json!({
                    "line": c.line,
                    "uuid": c.uuid,
                    "record_type": c.record_type,
                    "ts_utc": c.ts_utc,
                    "ts_local": c.ts_utc.as_deref().and_then(crate::timez::local_iso),
                })
            })
            .collect();
        let obj = json!({
            "kind": "branch-point",
            "uuid": p.uuid,
            "line": p.line,
            "children": children,
            "widest_gap_seconds": p.widest_gap_secs,
        });
        println!("{}", serde_json::to_string(&obj)?);
    }
    let summary = crate::text::envelope_summary(json!({
        "branch_points": points.len(),
        "conversation_records": conversation_records,
        "skipped_lines": skipped,
    }));
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
