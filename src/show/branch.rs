//! `show --branch-points`: conversation FORK facts for one transcript.
//!
//! A Claude Code rewind, retry, or parallel lane leaves one plain DAG fact: some record
//! has MORE THAN ONE conversation child (a later `parentUuid` re-attach). csift reports
//! those facts and RANKS them by the widest inter-child time gap: a rewind usually shows
//! a wide gap, a parallel lane a near-zero one.
//!
//! Which child is LIVE is a fact too, since v0.12.0, and it comes from Claude Code rather
//! than from the shape: the loader's own conversation chain ([`crate::model::Chain`])
//! decides it. A classifier keyed on the DAG shape alone was prototyped and REFUTED
//! against real corpora, and the family that refutes it is not parallel tool fan-out -
//! whose carriers never reach the child predicate - but the repeated-uuid run, an append
//! artifact wearing a fork's shape. So each fork names its live child and gives every
//! other child the chain's verdict (`rewound` when it was answered, `draft` when nothing
//! ever answered it, plain `abandoned` mid-branch, `pre-cut` above a compaction cut).
//! Beyond what the chain resolves, csift still guesses nothing.
//!
//! A "conversation child" is a `user`/`assistant` record, EXCLUDING user records that
//! carry a `tool_result` block (parallel tool results share a parent by construction),
//! `isMeta` records, and compaction summaries. The PARENT, though, can be any record the
//! loader admits, and usually IS one the role prefilter drops - a prompt submitted after a
//! SessionStart hook is parented to that hook's `attachment` record - so the uuid is
//! located over the structural spine rows too ([`crate::parse::spine_record`]), and the
//! fork prints the parent's line AND type.

use super::*;
use crate::model::{Block, Chain, Kind, Record, Survival};
use serde_json::json;

/// One parsed jsonl line: a full record, or the structural [`crate::parse::spine_record`]
/// of a line the role prefilter drops (kept ONLY so the chain walk and the parent lookup
/// can see the whole DAG - it carries no payload and is never a child).
///
/// The two kinds are scanned into SEPARATE streams and re-merged by line here, so the
/// narrow spine row is never carried at a record's width.
#[derive(Debug, Clone, Copy)]
enum Row<'a> {
    Full(usize, &'a Record),
    Spine(&'a crate::parse::SpineRow),
}

impl<'a> Row<'a> {
    fn line(self) -> usize {
        match self {
            Row::Full(l, _) => l,
            Row::Spine(s) => s.line(),
        }
    }

    /// This row as the chain reads it.
    fn node(self) -> crate::model::ChainNode<'a> {
        match self {
            Row::Full(_, r) => crate::model::ChainNode::Full(r),
            Row::Spine(s) => crate::model::ChainNode::Spine(s),
        }
    }

    /// The full record, or `None` for a chain-only spine row (never a fork CHILD).
    fn full(self) -> Option<&'a Record> {
        match self {
            Row::Full(_, r) => Some(r),
            Row::Spine(_) => None,
        }
    }
}

/// One child edge of a branch point.
#[derive(Debug)]
struct Child {
    line: usize,
    uuid: Option<String>,
    ts_utc: Option<String>,
    record_type: String,
    /// This child's place in the surviving conversation (`live` | `pre-cut` |
    /// `abandoned`).
    survival: &'static str,
    /// The finer read: `live` · `rewound` (answered, then rewound past) · `draft`
    /// (nothing ever answered it) · `abandoned` (off-chain, not an opener) · `pre-cut`.
    verdict: &'static str,
}

/// One record with 2+ conversation children.
#[derive(Debug)]
struct BranchPoint {
    uuid: String,
    /// The parent record's own jsonl line and top-level `type`; `None` only when the uuid
    /// names no line in this file at all (a clipped or forked-away parent).
    parent: Option<(usize, String)>,
    children: Vec<Child>,
    /// The line(s) of the children the conversation chain still reaches. Exactly one on
    /// an ordinary fork; empty when the whole fork is off the surviving conversation.
    live_child_lines: Vec<usize>,
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

/// One child's place in the surviving conversation, as the chain reads it: the coarse
/// `survival` value plus the finer verdict a fork reader actually wants.
fn survival_verdict(chain: &Chain, i: usize) -> (&'static str, &'static str) {
    let survival = chain.survival(i);
    let verdict = match survival {
        Survival::Live => "live",
        Survival::PreCut => "pre-cut",
        Survival::Abandoned { .. } => match chain.kind(i) {
            Some(Kind::Rewound { .. }) => "rewound",
            Some(Kind::Draft { .. }) => "draft",
            None => "abandoned",
        },
    };
    (survival.as_str(), verdict)
}

/// Widest gap between CONSECUTIVE children in whole seconds; a single undated seam makes
/// the whole ranking honest-unknown rather than silently narrower.
fn widest_gap(children: &[Child]) -> Option<i64> {
    let mut widest: Option<i64> = None;
    for pair in children.windows(2) {
        let (Some(a), Some(b)) = (pair[0].ts_utc.as_deref(), pair[1].ts_utc.as_deref()) else {
            return None;
        };
        let (Some(ta), Some(tb)) = (parse_ts(a), parse_ts(b)) else {
            return None;
        };
        let g = (tb.as_second() - ta.as_second()).unsigned_abs() as i64;
        widest = Some(widest.map_or(g, |w: i64| w.max(g)));
    }
    widest
}

pub(crate) fn run_branch_points(file: &std::path::Path, format: OutputFormat) -> Result<()> {
    let session_id = crate::subagent::session_id_from_path(file);
    let is_subagent = crate::subagent::is_subagent_path(file);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(file).unwrap_or_else(|| session_id.clone());

    let mut full: Vec<(usize, Record)> = Vec::new();
    let mut spine: Vec<crate::parse::SpineRow> = Vec::new();
    let mut skipped = 0usize;
    if let Some(mmap) = mmap_bytes(file)? {
        let bytes: &[u8] = &mmap;
        let (f, sp, s) = crate::parse::scan_lines_parallel_split(bytes, |line, line_no| {
            if !crate::parse::line_has_role_marker(line) {
                // The parent of a fork is often NOT a conversation record, and the chain
                // walk threads through the same lines - so lift the structural fields of
                // every line the role prefilter drops (never the payload), onto its own
                // stream.
                if let Some(row) = crate::parse::spine_record(line_no, line) {
                    return crate::parse::SplitVerdict::Second(row);
                }
                return crate::parse::non_candidate_split(line);
            }
            match crate::parse::parse_line(line) {
                Ok(Some(rec)) => crate::parse::SplitVerdict::First((line_no, rec)),
                Ok(None) => crate::parse::SplitVerdict::Ignore,
                Err(_) => crate::parse::SplitVerdict::Skip,
            }
        });
        full = f;
        spine = sp;
        skipped = s;
    }
    // Back into ONE file-order list: the chain's index space, and the order the fork
    // children are collected in.
    let mut rows: Vec<Row<'_>> = Vec::with_capacity(full.len() + spine.len());
    {
        let (mut a, mut b) = (0usize, 0usize);
        while a < full.len() || b < spine.len() {
            let take_full = match (full.get(a), spine.get(b)) {
                (Some((la, _)), Some(sb)) => *la <= sb.line(),
                (Some(_), None) => true,
                _ => false,
            };
            if take_full {
                rows.push(Row::Full(full[a].0, &full[a].1));
                a += 1;
            } else {
                rows.push(Row::Spine(&spine[b]));
                b += 1;
            }
        }
    }

    // Claude Code's own conversation chain over the SAME rows: which child of a fork the
    // conversation continued from, and what became of the others.
    let chain = Chain::build_by(&rows, |r| r.node(), None);

    // uuid → (own line, own type), over EVERY parsed line - a fork parent can be an
    // attachment, a turn_duration system record, or any other line the loader admits.
    let line_of: std::collections::HashMap<&str, (usize, &str)> = rows
        .iter()
        .filter_map(|r| {
            let n = r.node();
            n.uuid()
                .map(|u| (u, (r.line(), n.kind().unwrap_or("(untyped)"))))
        })
        .collect();
    // parentUuid → conversation children, file order.
    let mut children_of: std::collections::HashMap<String, Vec<Child>> =
        std::collections::HashMap::new();
    let mut conversation_records = 0usize;
    for (i, row) in rows.iter().enumerate() {
        let Some(rec) = row.full().filter(|r| is_conversation_record(r)) else {
            continue;
        };
        conversation_records += 1;
        let Some(parent) = rec.parent_uuid.as_deref() else {
            continue;
        };
        let (survival, verdict) = survival_verdict(&chain, i);
        children_of
            .entry(parent.to_string())
            .or_default()
            .push(Child {
                line: row.line(),
                uuid: rec.uuid.clone(),
                ts_utc: rec.timestamp.clone(),
                record_type: rec
                    .r#type
                    .clone()
                    .unwrap_or_else(|| "(untyped)".to_string()),
                survival,
                verdict,
            });
    }

    let mut points: Vec<BranchPoint> = children_of
        .into_iter()
        .filter(|(_, ch)| ch.len() >= 2)
        .map(|(uuid, mut children)| {
            children.sort_by_key(|c| c.line);
            let widest = widest_gap(&children);
            let live_child_lines = children
                .iter()
                .filter(|c| c.survival == "live")
                .map(|c| c.line)
                .collect();
            BranchPoint {
                parent: line_of
                    .get(uuid.as_str())
                    .map(|(l, t)| (*l, (*t).to_string())),
                uuid,
                children,
                live_child_lines,
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

/// The fork header's live-child clause. Exactly one live child is the ordinary case; zero
/// means the whole fork sits off the surviving conversation, and more than one is possible
/// where the chain's membership rules keep same-`message.id` siblings - stated, not folded.
fn live_child_label(lines: &[usize]) -> String {
    match lines {
        [] => "live child: none (this fork is off the surviving conversation)".to_string(),
        [one] => format!("live child: L{one}"),
        many => format!(
            "live children: {}",
            many.iter()
                .map(|l| format!("L{l}"))
                .collect::<Vec<_>>()
                .join(" ")
        ),
    }
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
        let loc = p.parent.as_ref().map_or_else(
            || "parent uuid not in this file".to_string(),
            |(l, t)| format!("L{l}  {t}"),
        );
        let gap = p
            .widest_gap_secs
            .map_or_else(|| "unknown (missing timestamps)".to_string(), gap_label);
        println!();
        println!(
            "  #{}  uuid {}  {loc}  children {} · widest gap {gap} · {}",
            i + 1,
            p.uuid,
            p.children.len(),
            live_child_label(&p.live_child_lines)
        );
        for c in &p.children {
            println!(
                "      L{}  {}  {}  {}",
                c.line,
                crate::timez::format_timestamp(c.ts_utc.as_deref()),
                c.record_type,
                c.verdict
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
             wide gap, a parallel lane a near-zero one. csift reports fork FACTS - the \
             live child is one of them, decided by Claude Code's own conversation chain, \
             and beyond what that chain resolves csift does not guess."
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
                    "survival": c.survival,
                    "verdict": c.verdict,
                    "ts_utc": c.ts_utc,
                    "ts_local": c.ts_utc.as_deref().and_then(crate::timez::local_iso),
                })
            })
            .collect();
        let parent_line = p.parent.as_ref().map(|(l, _)| *l);
        let obj = json!({
            "kind": "branch-point",
            "uuid": p.uuid,
            // `line` is the envelope's own line key; `parent_line`/`parent_type` name the
            // same record explicitly, because on this row the record IS the fork parent.
            "line": parent_line,
            "parent_line": parent_line,
            "parent_type": p.parent.as_ref().map(|(_, t)| t.clone()),
            "live_child_line": match p.live_child_lines.as_slice() {
                [one] => Some(*one),
                _ => None,
            },
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
