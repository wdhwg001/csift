//! The `csift send` receipt: the two projections of one decided send.
//!
//! They live beside the command rather than inside it because they are the SAME facts twice.
//! A field that reaches the text and not the JSON (or the other way round) is a reader being
//! told two different things about one send, so both read from one gathered [`Receipt`] and
//! neither is allowed to compute anything of its own.

use anyhow::Result;
use serde_json::json;

use super::caller::{Receiver, SettingsDisclosure};
use super::policy::{self, Decision, SendContext};
use super::send::SlotCensus;
use super::{Message, Verdict, CHUNK_BUDGET};
use crate::text;

/// Everything one receipt prints, gathered once by the command.
pub(crate) struct Receipt<'a> {
    pub(crate) msg: &'a Message,
    pub(crate) receiver: &'a Receiver,
    pub(crate) ctx: &'a SendContext,
    pub(crate) decision: &'a Decision,
    pub(crate) slots: &'a SlotCensus,
    pub(crate) armed: &'a [u32],
    pub(crate) settings: &'a SettingsDisclosure,
}

pub(crate) fn render_text(r: &Receipt) {
    let (msg, receiver, ctx, decision) = (r.msg, r.receiver, r.ctx, r.decision);
    println!("csift channel · send");
    println!("  id          {}", msg.id);
    println!("  verdict     {}", decision.verdict.as_str());
    println!("  channel     {}", decision.channel);
    println!("  mode        {}", msg.mode.as_str());
    println!(
        "  queued      {}",
        if decision.queued { "yes" } else { "no" }
    );
    println!(
        "  receiver    {}{}",
        receiver.lane,
        receiver
            .routing_id
            .as_deref()
            .map(|id| format!("  (routing {id})"))
            .unwrap_or_default()
    );
    println!("    kind      {}", receiver.kind.as_str());
    println!("    state     {}", receiver.state.as_str());
    println!(
        "    version   {}",
        receiver.version.as_deref().unwrap_or("unknown")
    );
    println!("    session   {}", receiver.session);
    println!("  gates       teams: {}", ctx.teams.verdict);
    println!("              harbor: {}", ctx.harbor.verdict);
    println!("  slots       {}", slot_line(r.slots));
    println!("  armed       {}", armed_line(r.armed));
    // The cascade the gates, the slot census and every hook risk above were read from: a
    // verdict is checkable only when its sources are named, absent ones included.
    for (i, line) in r.settings.lines().iter().enumerate() {
        let label = if i == 0 { "settings" } else { "        " };
        println!("  {label}    {line}");
    }
    println!(
        "  message     {} char(s), {} chunk(s) of {CHUNK_BUDGET}",
        msg.body.chars().count(),
        ctx.chunks
    );
    if decision.verdict == Verdict::Full {
        println!("  full        {}", policy::full_note(ctx));
    }
    println!("  prediction  {}", decision.prediction);
    for risk in &decision.risks {
        println!("  risk        {risk}");
    }
    if let Some(o) = &decision.official {
        println!("  official    {}", o.call);
        println!(
            "              csift never performs an official send (it is a tool, not a file): \
             make this call yourself."
        );
    }
}

pub(crate) fn slot_line(slots: &SlotCensus) -> String {
    if slots.per_event.is_empty() {
        return "none configured on any delivery event".to_string();
    }
    slots
        .per_event
        .iter()
        .map(|(event, ks)| {
            format!(
                "{event} {}",
                ks.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
            )
        })
        .collect::<Vec<_>>()
        .join("  ")
}

pub(crate) fn armed_line(armed: &[u32]) -> String {
    if armed.is_empty() {
        return "none (no delivery hook has run in this lane)".to_string();
    }
    armed
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn render_json(r: &Receipt) -> Result<()> {
    let (msg, receiver, ctx, decision) = (r.msg, r.receiver, r.ctx, r.decision);
    let configured: serde_json::Map<String, serde_json::Value> = r
        .slots
        .per_event
        .iter()
        .map(|(e, ks)| (e.clone(), json!(ks)))
        .collect();
    println!(
        "{}",
        serde_json::to_string(&text::envelope_header("send", json!({})))?
    );
    let row = json!({
        "kind": "send",
        "id": msg.id,
        "verdict": decision.verdict.as_str(),
        "channel": decision.channel,
        "mode": msg.mode.as_str(),
        "receiver": {
            "lane": receiver.lane,
            "routing_id": receiver.routing_id,
            "session": receiver.session,
            "kind": receiver.kind.as_str(),
            "state": receiver.state.as_str(),
            "version": receiver.version,
            "configured_slots": configured,
            "armed_slots": r.armed,
        },
        "official": decision.official.as_ref().map(|o| json!({
            "delegated": true,
            "tool": o.tool,
            "to": o.to,
        })),
        "prediction": decision.prediction,
        "risks": decision.risks,
        "settings": r.settings.json(),
    });
    println!("{}", serde_json::to_string(&row)?);
    println!(
        "{}",
        serde_json::to_string(&text::envelope_summary(json!({
            "queued": decision.queued,
            "chunks": ctx.chunks,
            "message_chars": msg.body.chars().count(),
            "relation": msg.relation.as_str(),
            "cross_project": msg.cross_project,
            "ttl_secs": msg.ttl_secs,
            "official_floor_met": ctx.official_possible(),
        })))?
    );
    Ok(())
}
