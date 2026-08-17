//! Hit collection: per-turn hits, siblings, collect_record_hits (the emission engine).

use super::*;

// Internal pipeline function: the arg list grew as `tool_names` (tool-response naming) and
// `address` (--line/--uuid selector) were threaded through the per-turn scan. Bundling into a
// struct would only relocate the same fields without simplifying the data flow.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_turn_hits(
    turn: &Turn<'_>,
    filter: LabelFilter<'_>,
    matcher: &Matcher,
    time_window: &TimeWindow,
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    address: Option<&AddressSet>,
    env: &ClassifyEnv<'_>,
) -> (Vec<Hit>, Vec<usize>) {
    let mut hits = Vec::new();
    let mut hit_idxs = Vec::new();
    for (i, kept) in turn.records.iter().enumerate() {
        // Addressing (`--line`/`--uuid`): only the ADDRESSED records are eligible to hit — the
        // selector that turns `search` into the message-getter. (Applied before the keyword
        // prefilter so an addressed record is fetched regardless of the pattern literal.)
        if let Some(addr) = address {
            if !addr.addresses(kept) {
                continue;
            }
        }
        // §7d keyword prefilter: if the raw line provably lacks the required
        // literal, this record can't be a hit — skip the regex work. (It still
        // stays a member of this turn for the complete round-trip; we just don't
        // emit a hit for it.)
        if !kept.can_hit {
            continue;
        }
        let rec = &kept.rec;
        // Time window applies per-record (records with no timestamp never match a
        // bounded window, per SPEC §6.2).
        if !time_window.contains(rec.timestamp.as_deref()) {
            continue;
        }
        let before = hits.len();
        collect_record_hits(
            rec,
            filter,
            matcher,
            resolve_persisted,
            excerpt_max,
            plan_index,
            tool_names,
            &env.ctx_for(kept),
            &mut hits,
        );
        // Backfill the source record's address onto every hit this record produced.
        backfill_address(&mut hits[before..], kept);
        if hits.len() > before {
            hit_idxs.push(i);
        }
    }
    (hits, hit_idxs)
}

/// Stamp the source record's line number + uuid onto each hit just appended for it — the
/// `csift show --line/--uuid` address. Done by the turn collector (not `make_hit`) because the line number
/// lives on the `Kept`, not the `Record`. Also attaches the record's image ids to its FIRST
/// hit (so an image-bearing message exposes the extractable `#N`/`L<line>i<n>` id once, not
/// repeated per matched block).
pub(crate) fn backfill_address(hits: &mut [Hit], kept: &Kept) {
    for h in hits.iter_mut() {
        h.line = kept.line_no;
        h.uuid = kept.rec.uuid.clone();
        h.from_sidecar = kept.from_sidecar;
    }
    if let Some(first) = hits.first_mut() {
        first.image_ids = crate::image::image_ids_for_record(&kept.rec, kept.line_no);
    }
}

/// The turn's NON-matched records as sibling hits, restricted + CAPPED per the parsed
/// `--siblings <SPEC>`. Reuses [`collect_record_hits`] with a PURE-FILTER matcher (matches
/// every record, so each label-eligible unit of a sibling surfaces with a head excerpt). A
/// record that matched (its index is in `hit_idxs`) is never repeated. The per-record time
/// window is intentionally NOT re-applied: the turn already qualified, and the siblings are
/// context for that qualifying turn. Caps: a `<selector>:N` spec keeps the first N siblings under
/// that selector; a bare `N` keeps the first N across the labels with no typed cap ("the rest"),
/// and when ONLY a bare `N` was given it is a single TOTAL cap across all labels.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_turn_siblings(
    turn: &Turn<'_>,
    hit_idxs: &[usize],
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    env: &ClassifyEnv<'_>,
) -> (Vec<Hit>, usize) {
    let pure = Matcher::pure();
    let all = LabelFilter::all(); // every label is eligible — siblings ignore -t/-T
    let mut sibs = Vec::new();
    for (i, kept) in turn.records.iter().enumerate() {
        if hit_idxs.contains(&i) {
            continue;
        }
        let before = sibs.len();
        collect_record_hits(
            &kept.rec,
            all,
            &pure,
            resolve_persisted,
            excerpt_max,
            plan_index,
            tool_names,
            &env.ctx_for(kept),
            &mut sibs,
        );
        backfill_address(&mut sibs[before..], kept);
    }
    // FIXED policy (see [`sibling_cap`]): message classes always render; chattier
    // machinery keeps the FIRST N per leaf. The remainder is COUNTED (never silent) —
    // the caller renders an explicit `(+N more · csift show …)` pointer.
    let mut kept_per_leaf: HashMap<&'static str, usize> = HashMap::new();
    let mut hidden = 0usize;
    sibs.retain(|hit| match sibling_cap(hit.class) {
        None => true,
        Some(cap) => {
            let n = kept_per_leaf.entry(hit.class.path()).or_insert(0);
            if *n < cap {
                *n += 1;
                true
            } else {
                hidden += 1;
                false
            }
        }
    });
    (sibs, hidden)
}

/// Emit hits for every label-eligible UNIT of `rec` that matches the regex (the P2 cutover —
/// GOLD §6). The record is classified ONCE via [`Record::classify`]; each emission UNIT (the
/// record-level user/comm/harness text, the user-facing tool_result dual, or a block) picks the
/// RICHEST selected [`Class`] among its candidate labels (GOLD §3 Q4 dedup) and emits ONE hit.
/// Comm units carry the `from ⇨ to` direction ([`Record::direction`]); tool units carry the
/// `tool_use_id` for the later `▹` pairing pass. A record carrying NO label (metadata / an
/// excluded isMeta pseudo-turn) yields nothing.
// Internal pipeline function; `tool_names` (tool-response naming) + `ctx` (cross-record classify
// context) are threaded through. Same rationale as `collect_turn_hits` for not bundling into a
// struct.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_record_hits(
    rec: &Record,
    filter: LabelFilter<'_>,
    matcher: &Matcher,
    resolve_persisted: bool,
    excerpt_max: usize,
    plan_index: &PlanIndex,
    tool_names: &HashMap<String, String>,
    ctx: &ClassifyCtx,
    hits: &mut Vec<Hit>,
) {
    let labels = rec.classify(ctx);
    if labels.is_empty() {
        return; // unmodeled / excluded record — carries no role.class.sub label
    }
    let ts = rec.timestamp.clone();
    let model = rec
        .message
        .as_ref()
        .and_then(|m| m.model.as_ref())
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let label_paths: Vec<&'static str> = labels.iter().map(|c| c.path()).collect();
    let sel = |c: Class| filter.selected(c.path());
    let has = |c: Class| labels.contains(&c);
    // Direction is per-record (the first comm direction); computed only when a comm label is
    // present (it parses peer sections / scans blocks), and attached to comm hits only. The
    // owner's own id renders as `self` (GOLD §3/§4: `self ⇨ to`, `from ⇨ self`).
    let direction = if labels.iter().copied().any(is_comm_class) {
        alias_self(rec.direction(ctx), ctx.owner_id)
    } else {
        None
    };

    // One emission: locate the match, build the match-centered excerpt, carry class/labels/
    // direction/tool_use_id. `pair` is filled later by the per-file pairing pass.
    let mut emit = |class: Class,
                    text: &str,
                    tool_name: Option<String>,
                    dir: Option<(String, String)>,
                    tuid: Option<String>| {
        if let Some(span) = matcher.locate(text) {
            let (excerpt, truncated) = match_excerpt(text, span, excerpt_max);
            hits.push(Hit {
                class,
                labels: label_paths.clone(),
                excerpt,
                timestamp_utc: ts.clone(),
                tool_name,
                model: model.clone(),
                direction: dir,
                tool_use_id: tuid,
                pair: None,
                line: 0,
                uuid: None,
                raw: None,
                image_ids: Vec::new(),
                from_sidecar: false,
                truncated,
            });
        }
    };

    // ── 1. Record-level TEXT unit(s). A BATCHED record (≥1 `<task-notification>` / inbound-peer
    //    section) renders ONE hit PER section (GOLD §3 G4/G5), each with its own label + direction
    //    — so a notification-with-`<result>` ALSO surfaces its `agent.communication.inbox`
    //    (child ⇨ self, G1), and several mixed-kind sections no longer collapse to one. Any other
    //    record-text class (user.message, harness markers, compaction, a subagent-opener inbox)
    //    renders ONE richest-label hit. The §1 fix (teammate → inbox) + the `<task-notification>`
    //    → harness.notification reparent flow straight from `classify`. ──
    let sections = rec.record_text_sections(ctx);
    if sections.is_empty() {
        if let Some((class, text)) = record_text_emission(rec, &labels, filter, plan_index) {
            let dir = if is_comm_class(class) {
                direction.clone()
            } else {
                None
            };
            emit(class, &text, None, dir, None);
        }
    } else {
        for crate::model::RecordTextSection {
            class,
            text,
            direction: dir,
        } in sections
        {
            if !filter.selected(class.path()) {
                continue;
            }
            let dir = if is_comm_class(class) {
                alias_self(dir, ctx.owner_id)
            } else {
                None
            };
            emit(class, &text, None, dir, None);
        }
    }

    // ── 2. Record-level user-facing tool_result DUAL (AUQ answer / typed rejection) ──
    // These are RECORD-level facts, so emit ONCE (not per tool_result block); GOLD §3 Q4: the
    // user-facing view is RICHEST, superseding the agent.tool.result copy (the block loop then
    // skips it). `reconstructed_user_text` yields the clean Q+options+answer / rejection (+[plan:])
    // unit. When neither user-facing label is SELECTED, `user_dual` is None and the block loop
    // surfaces the plain agent.tool.result instead (so `-t agent.tool.result` still finds it).
    let user_dual = if has(Class::UserAnswer) && sel(Class::UserAnswer) {
        Some(Class::UserAnswer)
    } else if has(Class::UserRejection) && sel(Class::UserRejection) {
        Some(Class::UserRejection)
    } else {
        None
    };
    if let Some(class) = user_dual {
        if let Some(text) = rec.reconstructed_user_text(Some(plan_index)) {
            emit(class, &text, None, None, None);
        }
    }

    // ── 3. §3.10 MCP elicitation marker with NO tool_use block → agent.tool.use (content string).
    // The AUQ/ExitPlanMode markers DO carry a tool_use block and surface via the block loop, so
    // this arm is GUARDED to a no-tool_use marker to avoid a double emit (keep the guard). ──
    if has(Class::AgentToolUse)
        && sel(Class::AgentToolUse)
        && rec.is_elicitation_marker()
        && rec
            .blocks()
            .is_none_or(|bs| !bs.iter().any(|b| matches!(b, Block::ToolUse { .. })))
    {
        if let Some(text) = rec.content.as_ref().and_then(serde_json::Value::as_str) {
            emit(
                Class::AgentToolUse,
                text,
                rec.csift_kind.clone(),
                None,
                None,
            );
        }
    }

    // ── 4. Block-bearing units: thinking / agent text / tool_use (+comm) / tool_result (+comm). ──
    collect_block_hits(
        rec,
        &labels,
        filter,
        user_dual,
        &direction,
        resolve_persisted,
        tool_names,
        &mut emit,
    );
}

/// The hit-emission sink shared by [`collect_record_hits`] and its block loop:
/// (class, text, tool_name, direction, tool_use_id).
type EmitHit<'a> =
    dyn FnMut(Class, &str, Option<String>, Option<(String, String)>, Option<String>) + 'a;

/// The block loop of [`collect_record_hits`]: one emission per selected block-bearing unit —
/// thinking (incl. the opaque redacted placeholder), assistant text, tool_use (richest comm
/// view first), tool_result (inbox > plain result; the user-facing dual, when SELECTED, was
/// already emitted as the richest view and suppresses the duplicate).
#[allow(clippy::too_many_arguments)]
fn collect_block_hits(
    rec: &Record,
    labels: &[Class],
    filter: LabelFilter<'_>,
    user_dual: Option<Class>,
    direction: &Option<(String, String)>,
    resolve_persisted: bool,
    tool_names: &HashMap<String, String>,
    emit: &mut EmitHit<'_>,
) {
    let sel = |c: Class| filter.selected(c.path());
    let has = |c: Class| labels.contains(&c);
    if let Some(blocks) = rec.blocks() {
        for block in blocks {
            match block {
                Block::Thinking { thinking, .. }
                    if has(Class::AgentThinking) && sel(Class::AgentThinking) =>
                {
                    emit(Class::AgentThinking, thinking, None, None, None);
                }
                Block::RedactedThinking { .. }
                    if has(Class::AgentThinking) && sel(Class::AgentThinking) =>
                {
                    // Opaque/encrypted reasoning — no readable text; surface a placeholder so
                    // `-t agent.thinking` still finds the block (GOLD §2 / oracle B3).
                    emit(
                        Class::AgentThinking,
                        REDACTED_THINKING_PLACEHOLDER,
                        None,
                        None,
                        None,
                    );
                }
                Block::Text { text }
                    if rec.is_type("assistant")
                        && has(Class::AgentMessage)
                        && sel(Class::AgentMessage) =>
                {
                    // Only assistant `text` blocks are the agent message; a user `text` block is
                    // a record-text unit (handled above), never agent.message.
                    emit(Class::AgentMessage, text, None, None, None);
                }
                Block::ToolUse { id, name, input } => {
                    // Richest-selected for this tool_use: comm (sent/signal) > agent.tool.use.
                    let comm = tool_use_comm_class(name.as_deref(), input.as_ref());
                    let class = match comm {
                        Some(cc) if has(cc) && sel(cc) => Some(cc),
                        _ if has(Class::AgentToolUse) && sel(Class::AgentToolUse) => {
                            Some(Class::AgentToolUse)
                        }
                        _ => None,
                    };
                    if let Some(class) = class {
                        let rendered = render_tool_use(name.as_deref(), input.as_ref());
                        let dir = if is_comm_class(class) {
                            direction.clone()
                        } else {
                            None
                        };
                        emit(class, &rendered, name.clone(), dir, id.clone());
                    }
                }
                Block::ToolResult {
                    content: Some(c),
                    tool_use_id,
                    ..
                } => {
                    // The user-facing dual was SELECTED + emitted as the richest view (§3 Q4) → skip
                    // the agent.tool.result duplicate. (When the dual is present but NOT selected —
                    // e.g. `-t agent.tool.result` alone — `user_dual` is None, so the plain result
                    // still surfaces and the answer is never lost.)
                    if user_dual.is_some() {
                        continue;
                    }
                    // Richest-selected: agent.communication.inbox (subagent return) > tool.result.
                    let class = if has(Class::CommInbox) && sel(Class::CommInbox) {
                        Class::CommInbox
                    } else if has(Class::AgentToolResult) && sel(Class::AgentToolResult) {
                        Class::AgentToolResult
                    } else {
                        continue;
                    };
                    let mut text = tool_result_content_text(c);
                    // §4.6: when asked, replace the inline persisted-output pointer with the real
                    // file content (matching runs against the resolved text).
                    if resolve_persisted {
                        if let Some(path) = rec.persisted_output_path() {
                            text = resolve_persisted_text(&path, &text);
                        }
                    }
                    let name = tool_use_id
                        .as_deref()
                        .and_then(|id| tool_names.get(id).cloned());
                    let dir = if class == Class::CommInbox {
                        direction.clone()
                    } else {
                        None
                    };
                    emit(class, &text, name, dir, tool_use_id.clone());
                }
                _ => {}
            }
        }
    }
}
