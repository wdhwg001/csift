//! Unit tests for the `turns` engine over small, locale-neutral fixtures.
//!
//! Charset discipline (CLAUDE.md / design §0): fixture strings use accented-Latin +
//! emoji multi-byte tokens only (`café🛠`), the house fixture style. The
//! end-to-end feature exercise on a real-shaped, multi-summary transcript lives in
//! `tests/cli_integration.rs`.

use super::*;

/// Build a [`TurnUnit`] for a role from a plain string (no record), for cost/ellipsis
/// tests. `orig_newlines` lets a test assert the `L lines elided` note.
fn unit(role: Role, line_no: usize, text: &str, orig_newlines: usize) -> TurnUnit {
    TurnUnit {
        line_no,
        role,
        full_chars: text.chars().count(),
        text: text.to_string(),
        orig_newlines,
        ts_utc: Some("2026-06-07T05:00:00.000Z".to_string()),
        also_in_summary: false,
        from_sidecar: false,
        inbound: None,
    }
}

/// Build a single [`AgentMsg`] wrapping a unit, with the given per-message attribution.
fn agent_msg(line_no: usize, text: &str, tools: usize, failed: usize) -> AgentMsg {
    AgentMsg {
        unit: unit(Role::Assistant, line_no, text, 0),
        pos: AgentPos::Last, // reassigned by the helper that assembles the run
        preceding_tool_calls: tools,
        preceding_failed: failed,
    }
}

/// Assign First/Middle/Last positions over an agent run (mirrors `build`).
fn assign_positions(agents: &mut [AgentMsg]) {
    let last = agents.len().saturating_sub(1);
    for (i, a) in agents.iter_mut().enumerate() {
        a.pos = if i == last {
            AgentPos::Last
        } else if i == 0 {
            AgentPos::First
        } else {
            AgentPos::Middle
        };
    }
}

/// The crate-DEFAULT richness config — `Longest` mode (keep the longest agent message + the
/// first-if-substantive + the rich middles). Drives the default-selection tests.
fn longest_cfg() -> RichnessCfg {
    RichnessCfg::default()
}

fn mk_turn(
    turn_index: usize,
    user: Option<&str>,
    asst: Option<&str>,
    tools: usize,
    comp: usize,
) -> TurnSlice {
    let mut agents: Vec<AgentMsg> = asst
        .map(|t| vec![agent_msg(turn_index * 10 + 5, t, 0, 0)])
        .unwrap_or_default();
    assign_positions(&mut agents);
    TurnSlice {
        turn_index,
        user: user.map(|t| unit(Role::User, turn_index * 10 + 1, t, 0)),
        tool_calls: tools,
        image_ids: Vec::new(),
        agents,
        compactions_before: comp,
        is_automation: false,
        automation: None,
    }
}

/// Build a turn whose agent run is the ORDERED list of `(line_no, text)` agent messages,
/// each with optional per-message tool/failed attribution defaulting to 0. For the
/// richness selection tests.
fn mk_turn_agents(
    turn_index: usize,
    user: Option<&str>,
    agent_texts: &[&str],
    comp: usize,
) -> TurnSlice {
    let mut agents: Vec<AgentMsg> = agent_texts
        .iter()
        .enumerate()
        .map(|(i, t)| agent_msg(turn_index * 100 + i + 5, t, 1, 0))
        .collect();
    assign_positions(&mut agents);
    TurnSlice {
        turn_index,
        user: user.map(|t| unit(Role::User, turn_index * 100 + 1, t, 0)),
        tool_calls: agents.len(),
        image_ids: Vec::new(),
        agents,
        compactions_before: comp,
        is_automation: false,
        automation: None,
    }
}

fn rec(json: &str) -> Record {
    serde_json::from_str(json).expect("valid record")
}

/// A `rich`-mode config with the documented Rich-mode defaults (threshold 6, rich-min
/// 280, declaration-max 200, keep-first true) for the multi-agent-message tests.
fn rich_cfg() -> RichnessCfg {
    RichnessCfg {
        mode: AgentMsgMode::Rich,
        ..RichnessCfg::default()
    }
}

/// Build a session of `n` complete round-trips + a trailing assistant-heavy block, to
/// drive the 50%-floor regression: a naive recency walk would starve users.
fn scan_with_turns(turns: Vec<TurnSlice>, summaries: Vec<SummaryInfo>) -> ScanResult {
    ScanResult {
        session_id: "s".to_string(),
        is_subagent: false,
        parent_session_id: "s".to_string(),
        turns,
        summaries,
        skipped_lines: 0,
    }
}

fn summary(line_no: usize, fps: Vec<&str>, body_chars: usize) -> SummaryInfo {
    SummaryInfo {
        line_no,
        fingerprints: fps.into_iter().map(fingerprint).collect(),
        body_chars,
    }
}

/// An EXPLICIT `EotOnly` (single-EOT) richness config. Every cost / plan / render test that
/// predates the multi-agent-message model runs against this so its assertions stay byte-
/// identical to the single-EOT behavior, INDEPENDENT of what the crate default mode is
/// (the default is now `Longest` — see [`longest_cfg`] / `richness_cfg_default_is_longest`).
fn cfg() -> RichnessCfg {
    RichnessCfg {
        mode: AgentMsgMode::EotOnly,
        ..RichnessCfg::default()
    }
}

mod part01;
mod part02;
mod part03;
mod part04;
mod part05;
mod part06;
mod part07;
