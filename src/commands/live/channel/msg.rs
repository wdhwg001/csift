//! `csift msg` and `csift ack`: read the channel back, and let a receiver close a message.
//!
//! `msg` is the reconciliation surface - it joins the per-lane ledger (csift's INTENT) to
//! the receiver lane's own transcript (the FACT) and reports one verdict. `ack` is the one
//! statement only the receiver can make: csift can see that a chunk was emitted and that a
//! record carrying the id exists, but only the lane that read it can say it acted on it.
//!
//! Both are addressed at a LANE. `--lane` names it; otherwise the calling Claude Code
//! session does, which is the TOP-LEVEL session in every lane - so the resolution path is
//! always printed on stderr rather than left to be assumed.

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::cli::{AckArgs, MsgArgs, OutputFormat};
use crate::text::{envelope_header, envelope_summary};
use crate::timez::{format_timestamp, local_iso};

use super::{
    append_ledger, is_message_expired, lane_from_target, now_utc, read_lane, read_message,
    reconcile, scan_facts, validate_message_id, Fact, LaneCtx, LaneView, LedgerLine, MsgReport,
    MsgVerdict,
};

/// What an external caller is told when `msg` has no lane to read.
const MSG_NEEDS_LANE: &str = "not a Claude Code lane (CLAUDE_CODE_SESSION_ID is unset), so there \
     is no calling lane to read: name one with `--lane @<lane>`";

/// What an external caller is told when it tries to ack. An ack asserts that a lane READ a
/// message, which a process outside Claude Code cannot know about any lane.
const ACK_NEEDS_LANE: &str = "an external caller cannot ack: an ack records that the RECEIVING \
     lane read the message, and a process outside Claude Code (CLAUDE_CODE_SESSION_ID unset) is \
     not that lane. Ack from inside the receiving session, or leave the message unacked - \
     `csift msg <id>` still reports whether it was delivered";

pub(crate) fn run_msg(args: &MsgArgs) -> Result<()> {
    let mut ctx = lane_for(args.lane.as_deref(), MSG_NEEDS_LANE)?;
    match args.id.as_deref() {
        Some(id) => {
            validate_message_id(id)?;
            ctx = redirect_to_receiver(ctx, id)?;
            let view = read_lane(&ctx)?;
            let (facts, scan_skipped) = scan_facts(&ctx.transcript, Some(id))?;
            let Some(report) = reconcile(&ctx, id, &view, facts.get(id).cloned())? else {
                bail!(
                    "no channel record of message {id} in lane {} (session {}): neither an inbox \
                     line nor a ledger line. `csift msg --lane @{}` lists what this lane does hold",
                    ctx.lane,
                    ctx.session,
                    ctx.lane
                );
            };
            emit(
                args.format,
                &ctx,
                &[report],
                view.skipped_lines + scan_skipped,
            )
        }
        None => {
            let view = read_lane(&ctx)?;
            let (facts, scan_skipped) = scan_facts(&ctx.transcript, None)?;
            let reports = lane_reports(&ctx, &view, &facts, args);
            emit(
                args.format,
                &ctx,
                &reports,
                view.skipped_lines + scan_skipped,
            )
        }
    }
}

pub(crate) fn run_ack(args: &AckArgs) -> Result<()> {
    validate_message_id(&args.id)?;
    // The env check runs even with `--lane`: the flag names WHICH lane inside the calling
    // session, it does not make an outside process into a receiver.
    if crate::path::resolve_env_session().is_err() {
        bail!("{ACK_NEEDS_LANE}");
    }
    let ctx = lane_for(args.lane.as_deref(), ACK_NEEDS_LANE)?;
    let view = read_lane(&ctx)?;
    let known = view.inboxes.contains_key(&args.id) || view.states.contains_key(&args.id);
    if !known {
        bail!(
            "lane {} (session {}) has no record of message {}: acking it would write a ledger \
             line that joins to nothing. `csift msg --lane @{}` lists this lane's messages",
            ctx.lane,
            ctx.session,
            args.id,
            ctx.lane
        );
    }
    let already = view.states.get(&args.id).is_some_and(|s| s.acked);
    let ts_utc = now_utc();
    append_ledger(
        &ctx.root,
        &ctx.lane,
        &LedgerLine::Ack {
            id: args.id.clone(),
            ts_utc: ts_utc.clone(),
        },
    )?;
    render_ack(args.format, &ctx, &args.id, &ts_utc, already)
}

/// Resolve the lane a channel command reads: `--lane` when given, else the calling Claude
/// Code session.
fn lane_for(flag: Option<&str>, external_hint: &str) -> Result<LaneCtx> {
    if let Some(target) = flag {
        return lane_from_target(target);
    }
    let Ok(session) = crate::path::resolve_env_session() else {
        bail!("{external_hint}");
    };
    // Unconditional, like every other env-resolved lane in csift: the environment names the
    // TOP-LEVEL session in EVERY lane, so a subagent that says nothing would silently read
    // its parent's channel.
    eprintln!(
        "csift: lane read from $CLAUDE_CODE_SESSION_ID, which names the TOP-LEVEL session \
         {session} in every lane - inside a subagent pass `--lane @<your agent id>`"
    );
    lane_from_target(&format!("@{session}"))
}

/// Follow a message to the lane it was actually addressed at.
///
/// `messages/` is keyed by SESSION and the inbox and ledger by LANE, so a message sent to a
/// child lane is readable from the session's own root but its state files sit under the
/// child's id. Redirecting keeps `csift msg <id>` answerable from the session the caller is
/// in, instead of reporting a message that is plainly there as unknown.
fn redirect_to_receiver(ctx: LaneCtx, id: &str) -> Result<LaneCtx> {
    let Some(message) = read_message(&ctx.root, id)? else {
        return Ok(ctx);
    };
    if message.to.lane == ctx.lane {
        return Ok(ctx);
    }
    eprintln!(
        "csift: message {id} is addressed at lane {} (session {}), reading that lane",
        message.to.lane, ctx.session
    );
    lane_from_target(&format!("@{}", message.to.lane))
}

/// Every message the lane holds, newest first, filtered by the mode flags.
fn lane_reports(
    ctx: &LaneCtx,
    view: &LaneView,
    facts: &std::collections::BTreeMap<String, Fact>,
    args: &MsgArgs,
) -> Vec<MsgReport> {
    let mut out: Vec<MsgReport> = Vec::new();
    for id in view.ids() {
        let Ok(Some(report)) = reconcile(ctx, &id, view, facts.get(&id).cloned()) else {
            continue;
        };
        if keeps(&report, args) {
            out.push(report);
        }
    }
    // Newest first: the message a caller wants is almost always the last one that arrived.
    out.sort_by(|a, b| b.sort_key().cmp(a.sort_key()).then(a.id.cmp(&b.id)));
    out
}

fn keeps(report: &MsgReport, args: &MsgArgs) -> bool {
    if args.held {
        return report.verdict == MsgVerdict::Held;
    }
    if args.sent {
        return !report.state.emitted_parts.is_empty();
    }
    if args.pending {
        return report.verdict == MsgVerdict::Queued;
    }
    true
}

fn emit(
    format: OutputFormat,
    ctx: &LaneCtx,
    reports: &[MsgReport],
    skipped_lines: usize,
) -> Result<()> {
    match format {
        OutputFormat::Json => render_json(ctx, reports, skipped_lines),
        OutputFormat::Text => {
            render_text(ctx, reports, skipped_lines);
            Ok(())
        }
    }
}

fn render_json(ctx: &LaneCtx, reports: &[MsgReport], skipped_lines: usize) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&envelope_header(
            "msg",
            json!({"lane": ctx.lane, "session": ctx.session}),
        ))?
    );
    for report in reports {
        println!("{}", serde_json::to_string(&msg_json(report))?);
    }
    println!(
        "{}",
        serde_json::to_string(&envelope_summary(json!({
            "messages": reports.len(),
            "skipped_lines": skipped_lines,
        })))?
    );
    Ok(())
}

/// The `msg` row projector (AGENTS section 4: hand-built `json!`, never a derive).
fn msg_json(report: &MsgReport) -> Value {
    let emits: Vec<Value> = report
        .emits
        .iter()
        .filter_map(|line| match line {
            LedgerLine::Emit {
                event,
                slot,
                part,
                parts,
                vehicle,
                ts_utc,
                ..
            } => Some(json!({
                "event": event,
                "slot": slot,
                "part": part,
                "parts": parts,
                "vehicle": vehicle.as_str(),
                "ts_utc": ts_utc,
                "ts_local": local_iso(ts_utc),
            })),
            _ => None,
        })
        .collect();
    let inbox = report.inbox.as_ref();
    let enqueued = inbox.map(|i| i.enqueued_utc.clone());
    let expires = inbox.and_then(|i| i.expires_utc.clone());
    json!({
        "kind": "msg",
        "id": report.id,
        "verdict": report.verdict.as_str(),
        "mode": report
            .message
            .as_ref()
            .map(|m| m.mode.as_str())
            .or_else(|| inbox.map(|i| i.mode.as_str())),
        "lane": report.lane,
        "session": report.session,
        "emits": emits,
        "held": !report.state.held_reasons.is_empty(),
        "held_reasons": report.state.held_reasons,
        "expired": is_message_expired(&report.state, inbox, &now_utc()),
        "acked": report.state.acked,
        "refused_reasons": report.state.refused_reasons,
        "from": report.message.as_ref().map(|m| m.from.envelope_token()),
        "relation": report.message.as_ref().map(|m| m.relation.as_str()),
        "enqueued_utc": enqueued,
        "enqueued_local": inbox.and_then(|i| local_iso(&i.enqueued_utc)),
        "expires_utc": expires,
        "expires_local": inbox.and_then(|i| i.expires_utc.as_deref()).and_then(local_iso),
        "fact": report.fact.as_ref().map(|f| json!({"line": f.line, "uuid": f.uuid})),
    })
}

fn render_text(ctx: &LaneCtx, reports: &[MsgReport], skipped_lines: usize) {
    println!("lane {} (session {})", ctx.lane, ctx.session);
    if reports.is_empty() {
        println!("no channel messages");
    } else if reports.len() == 1 {
        render_one(&reports[0]);
    } else {
        for report in reports {
            println!("{}", one_line(report));
        }
    }
    println!(
        "{} message(s){}",
        reports.len(),
        if skipped_lines > 0 {
            format!("  ·  {}", crate::text::malformed_note(skipped_lines))
        } else {
            String::new()
        }
    );
    println!(
        "the fact half on its own: csift search '<id>' @{} --additional-context",
        ctx.lane
    );
}

/// One row of the lane view.
fn one_line(report: &MsgReport) -> String {
    let mode = report
        .message
        .as_ref()
        .map(|m| m.mode.as_str())
        .or_else(|| report.inbox.as_ref().map(|i| i.mode.as_str()))
        .unwrap_or("?");
    let fact = report
        .fact
        .as_ref()
        .map_or_else(|| "-".to_string(), |f| format!("L{}", f.line));
    format!(
        "{}  {:<11}  {mode:<5}  enqueued {}  emits {}  fact {fact}",
        report.id,
        report.verdict.as_str(),
        format_timestamp(report.inbox.as_ref().map(|i| i.enqueued_utc.as_str())),
        report.emits.len(),
    )
}

/// The full report for one addressed message.
fn render_one(report: &MsgReport) {
    println!("{}  {}", report.id, report.verdict.as_str());
    if let Some(m) = &report.message {
        println!(
            "  from        {}  ·  mode {}  ·  relation {}",
            m.from.envelope_token(),
            m.mode.as_str(),
            m.relation.as_str()
        );
    }
    if let Some(i) = &report.inbox {
        println!(
            "  enqueued    {}",
            format_timestamp(Some(i.enqueued_utc.as_str()))
        );
        if let Some(exp) = &i.expires_utc {
            println!("  expires     {}", format_timestamp(Some(exp.as_str())));
        }
    }
    for line in &report.emits {
        if let LedgerLine::Emit {
            event,
            slot,
            part,
            parts,
            vehicle,
            ts_utc,
            ..
        } = line
        {
            println!(
                "  emit        {event} slot {slot} part {part}/{parts} {} {}",
                vehicle.as_str(),
                format_timestamp(Some(ts_utc.as_str()))
            );
        }
    }
    for reason in &report.state.held_reasons {
        println!("  held        {reason}");
    }
    for reason in &report.state.refused_reasons {
        println!("  refused     {reason}");
    }
    match &report.fact {
        Some(f) => println!(
            "  fact        L{} uuid {}",
            f.line,
            f.uuid.as_deref().unwrap_or("-")
        ),
        None => println!("  fact        none in this lane's transcript"),
    }
    if report.state.acked {
        println!("  acked       yes");
    }
}

fn render_ack(
    format: OutputFormat,
    ctx: &LaneCtx,
    id: &str,
    ts_utc: &str,
    already: bool,
) -> Result<()> {
    match format {
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string(&envelope_header(
                    "ack",
                    json!({"lane": ctx.lane, "session": ctx.session}),
                ))?
            );
            println!(
                "{}",
                serde_json::to_string(&json!({
                    "kind": "ack",
                    "id": id,
                    "lane": ctx.lane,
                    "session": ctx.session,
                    "ts_utc": ts_utc,
                    "ts_local": local_iso(ts_utc),
                    "already_acked": already,
                }))?
            );
            println!(
                "{}",
                serde_json::to_string(&envelope_summary(json!({"acked": 1})))?
            );
        }
        OutputFormat::Text => {
            println!(
                "acked {id} in lane {} (session {}) at {}",
                ctx.lane,
                ctx.session,
                format_timestamp(Some(ts_utc))
            );
            if already {
                println!("this lane had already acked it; the ledger keeps both lines");
            }
        }
    }
    Ok(())
}
