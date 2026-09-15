//! Text and JSON projections for the four reach surfaces.
//!
//! The section rows are printed WITHOUT an envelope: `whoami` owns its stream and prints the
//! header before them and the summary after, so its existing shape keeps its existing meaning
//! and the new rows just join it. `--to` and `--peers` are terminal modes, so each prints a
//! whole envelope of its own.

use super::*;

/// Build and print the `self` / `parent` / `topology` sections for one resolved lane. Returns
/// `(live child lanes, other live lanes)` for the caller's own summary line.
pub(crate) fn emit_lane_sections(r: &LaneRef, format: OutputFormat) -> Result<(usize, usize)> {
    let s = build_sections(r)?;
    match format {
        OutputFormat::Text => render_sections_text(&s),
        OutputFormat::Json => render_sections_json(&s)?,
    }
    Ok((s.subtree.len(), s.others))
}

fn render_sections_text(s: &Sections) {
    println!("self     {}", s.me.lane);
    if let Some(routing) = &s.me.routing_id {
        // Both forms, always: the routing id is what the official send takes and it can
        // collide; the transcript id names the file and never does.
        println!("  routing  {routing}");
    }
    println!("  kind     {}", self_kind_line(s));
    match (s.exact, s.me.depth) {
        (true, Some(d)) => println!("  depth    {d}"),
        _ => println!("  depth    unknown from the environment alone"),
    }
    println!("  state    {}", s.me.state.as_str());
    if s.via == "target" {
        println!("  path     {}", s.me.path.display());
    }
    match &s.parent {
        Some(p) => {
            println!("parent   {}", p.lane);
            println!("  kind     {}", p.kind.as_str());
            println!(
                "  alive    {}",
                match p.alive {
                    Some(true) => "yes",
                    Some(false) => "no",
                    None => "unknown",
                }
            );
            println!("  reply    {}", p.reply_channel);
        }
        None => println!("parent   none (a top-level session has no lane above it)"),
    }
    println!("topology {}", topology_head(s));
    for lane in &s.subtree {
        println!("  {}  {}  {}", lane.lane, lane.kind.as_str(), lane.state);
    }
    println!("  {} other live lane(s) (--peers to list)", s.others);
}

/// The `self` kind line. Under the environment form the kind is an ASSUMPTION, because the
/// variable names the top-level session even when a subagent reads it.
fn self_kind_line(s: &Sections) -> String {
    if s.exact {
        return s.me.kind.as_str().to_string();
    }
    format!(
        "{} (assumed: resolved from the environment, which names the top-level session in \
         every lane)",
        s.me.kind.as_str()
    )
}

fn topology_head(s: &Sections) -> String {
    let scope = if s.me.kind == ReceiverKind::TopLevel {
        "this session"
    } else {
        "this lane's subtree"
    };
    match s.subtree.len() {
        0 => format!("no live child lanes in {scope}"),
        n => format!("{n} live child lane(s) in {scope}"),
    }
}

fn render_sections_json(s: &Sections) -> Result<()> {
    let row = json!({
        "kind": "self",
        "lane": s.me.lane,
        "routing_id": s.me.routing_id,
        "lane_kind": s.me.kind.as_str(),
        "session": s.me.session,
        // Under the environment form the lane itself is an assumption, so its id domain is
        // unknowable too: null, exactly as the identity row reports it.
        "is_subagent": if s.exact { json!(s.me.kind != ReceiverKind::TopLevel) } else { serde_json::Value::Null },
        "lane_exact": s.exact,
        "resolved_via": s.via,
        "depth": if s.exact { s.me.depth.map(serde_json::Value::from).unwrap_or(serde_json::Value::Null) } else { serde_json::Value::Null },
        "state": s.me.state.as_str(),
        "version": s.me.version,
        "path": s.me.path.to_string_lossy(),
        "ts_utc": s.me.last_activity_utc,
        "ts_local": s.me.last_activity_utc.as_deref().and_then(crate::timez::local_iso),
    });
    println!("{}", serde_json::to_string(&row)?);
    if let Some(p) = &s.parent {
        let row = json!({
            "kind": "parent",
            "lane": p.lane,
            "lane_kind": p.kind.as_str(),
            "alive": p.alive,
            "reply_channel": p.reply_channel,
        });
        println!("{}", serde_json::to_string(&row)?);
    }
    for lane in &s.subtree {
        println!("{}", serde_json::to_string(&lane_json("lane", lane))?);
    }
    Ok(())
}

fn lane_json(kind: &str, lane: &LaneRow) -> serde_json::Value {
    json!({
        "kind": kind,
        "lane": lane.lane,
        "lane_kind": lane.kind.as_str(),
        "state": lane.state,
        "last_activity_utc": lane.last_activity_utc,
        "last_activity_local": lane.last_activity_utc.as_deref().and_then(crate::timez::local_iso),
    })
}

// -- the reach prediction --

pub(crate) fn render_reach_text(r: &Reach) {
    println!("csift channel · reach");
    println!("  target      {}{}", r.lane, routing_suffix(r));
    println!("    kind      {}", r.kind.as_str());
    println!("    state     {}", r.state.as_str());
    println!(
        "    version   {}",
        r.version.as_deref().unwrap_or("unknown")
    );
    println!("    session   {}", r.session);
    println!(
        "  caller      {} {}",
        r.caller_kind.as_str(),
        r.caller_label
    );
    println!("  channel     {}", r.decision.channel);
    println!("  verdict     {}", r.decision.verdict.as_str());
    println!("  gates       teams: {}", r.teams.verdict);
    println!("              harbor: {}", r.harbor.verdict);
    println!("  slots       {}", slot_line(&r.slots));
    println!("  armed       {}", armed_line(&r.armed));
    // Same disclosure the send receipt prints: the cascade the gates and the slot census
    // above were read from, absent scopes and unobservable inputs named.
    for (i, line) in r.settings.lines().iter().enumerate() {
        let label = if i == 0 { "settings" } else { "        " };
        println!("  {label}    {line}");
    }
    println!("  prediction  {}", r.decision.prediction);
    for risk in &r.decision.risks {
        println!("  risk        {risk}");
    }
    if let Some(o) = &r.decision.official {
        println!("  official    {}", o.call);
    }
    if let Some(note) = &r.inference {
        println!("  inference   {note}");
    }
    println!(
        "  nothing was queued: this is a prediction. `csift send {}` writes the message.",
        send_target(r)
    );
}

fn routing_suffix(r: &Reach) -> String {
    r.routing_id
        .as_deref()
        .map(|id| format!("  (routing {id})"))
        .unwrap_or_default()
}

/// The target token a real send would take: the transcript form, which is unique.
fn send_target(r: &Reach) -> String {
    format!("@{}", r.lane)
}

pub(crate) fn slot_line(slots: &ReachSlots) -> String {
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

pub(crate) fn render_reach_json(r: &Reach) -> Result<()> {
    let configured: serde_json::Map<String, serde_json::Value> = r
        .slots
        .per_event
        .iter()
        .map(|(e, ks)| (e.clone(), json!(ks)))
        .collect();
    println!(
        "{}",
        serde_json::to_string(&text::envelope_header("whoami", json!({})))?
    );
    let row = json!({
        "kind": "reach",
        "lane": r.lane,
        "routing_id": r.routing_id,
        "session": r.session,
        "lane_kind": r.kind.as_str(),
        "state": r.state.as_str(),
        "version": r.version,
        "caller": {"kind": r.caller_kind.as_str(), "label": r.caller_label},
        "channel": r.decision.channel,
        "verdict": r.decision.verdict.as_str(),
        "gates": {"teams": r.teams.verdict, "harbor": r.harbor.verdict},
        "configured_slots": configured,
        "armed_slots": r.armed,
        "prediction": r.decision.prediction,
        "risks": r.decision.risks,
        "official": r.decision.official.as_ref().map(|o| json!({
            "delegated": true,
            "tool": o.tool,
            "to": o.to,
        })),
        "inference": r.inference,
        "settings": r.settings.json(),
    });
    println!("{}", serde_json::to_string(&row)?);
    println!(
        "{}",
        serde_json::to_string(&text::envelope_summary(json!({
            "queued": false,
            "refused": r.decision.verdict == Verdict::Refused,
        })))?
    );
    Ok(())
}

// -- the peer census --

pub(crate) fn render_peers_text(rows: &[PeerRow]) {
    println!("csift channel · peers");
    for p in rows {
        println!("  {}  {}  {}", p.lane, p.kind.as_str(), p.state);
    }
    println!("  {} live lane(s)", rows.len());
}

pub(crate) fn render_peers_json(rows: &[PeerRow]) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&text::envelope_header("whoami", json!({})))?
    );
    for p in rows {
        let row = json!({
            "kind": "peer",
            "lane": p.lane,
            "lane_kind": p.kind.as_str(),
            "state": p.state,
            "session": p.session,
            "last_activity_utc": p.last_activity_utc,
            "last_activity_local": p.last_activity_utc.as_deref().and_then(crate::timez::local_iso),
        });
        println!("{}", serde_json::to_string(&row)?);
    }
    println!(
        "{}",
        serde_json::to_string(&text::envelope_summary(json!({"peers": rows.len()})))?
    );
    Ok(())
}

// -- the caller that is not a lane --

/// The first line of the not-a-lane answer, kept as a const so the text and the JSON row and
/// the test that pins it cannot drift apart.
pub(crate) const NOT_A_LANE: &str = "not a Claude Code lane";

/// What an external caller can do: send, and what the receiver needs installed to hear it.
pub(crate) fn external_answer(format: OutputFormat) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("{NOT_A_LANE}");
            println!(
                "  no CLAUDE_CODE_SESSION_ID in this environment: this process holds no lane id \
                 and no hook points, so nothing can be delivered to it."
            );
            println!("  channel out   csift send @<lane> \"<message>\"");
            println!(
                "  receiver needs  `csift deliver --slot k` hook lines on its delivery events \
                 in settings.json; print the block with `csift deliver --recipe`"
            );
            println!(
                "  csift never installs a hook: those lines are pasted by the receiver's \
                 operator."
            );
            Ok(())
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string(&text::envelope_header("whoami", json!({})))?
            );
            let row = json!({
                "kind": "self",
                "lane": serde_json::Value::Null,
                "lane_kind": "external",
                "lane_exact": true,
                "resolved_via": "environment",
                "note": NOT_A_LANE,
                "channel_out": "csift send @<lane> \"<message>\"",
                "receiver_needs": "csift deliver --slot k hook lines on the delivery events in \
                                   the receiver's settings.json (csift deliver --recipe)",
            });
            println!("{}", serde_json::to_string(&row)?);
            println!(
                "{}",
                serde_json::to_string(&text::envelope_summary(json!({"identities": 0})))?
            );
            Ok(())
        }
    }
}
