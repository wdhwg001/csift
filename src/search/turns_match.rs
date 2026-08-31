//! Turn delimitation + match: reconstruct_and_match, spawn lookup, pairing.

use super::*;

/// Walk retained records in file order, delimit turns by genuine-user records, and
/// for each turn decide whether it matches the filters + regex; emit a complete
/// Exchange per matching turn.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reconstruct_and_match(
    path: &Path,
    records: &[Kept],
    args: &SearchArgs,
    matcher: &Matcher,
    turn_range: Option<crate::text::RangeSpec>,
    time_window: &TimeWindow,
    address: Option<&AddressSet>,
    want_siblings: bool,
    spawn_map: &HashMap<PathBuf, Option<Arc<DiscoveredSpawns>>>,
    inner_parallel: bool,
) -> (Vec<Exchange>, usize, usize) {
    // Canonical bare-hex id (subagent `agent-` prefix stripped) - the SAME derivation
    // every other surface uses, so a `search` subagent hit's `session_id` is joinable to
    // `files`/`turns`/`recover`/`agents` (id-form unification; a top-level uuid is
    // unaffected). See [`crate::subagent::session_id_from_path`].
    let session_id = crate::subagent::session_id_from_path(path);
    // A subagent transcript's owner is the parent uuid (the dir before `subagents/`) - the
    // scope-token for re-targeting the whole session. For a top-level file there is no
    // parent, so the parent IS the session id.
    let is_subagent = crate::subagent::is_subagent_path(path);
    let parent_session_id =
        crate::subagent::parent_session_id_from_path(path).unwrap_or_else(|| session_id.clone());

    // Group records into turns via the shared §6.4 delimiter (model::group_turn_indices
    // is the single source of truth, used identically by `files`). The outer index is
    // the 0-based turn index; map each index group back to its `Kept` borrows.
    // The skip set is computed EXPLICITLY (not inside the deduped grouper) so the
    // collapse can be DISCLOSED and an addressed draft can still be fetched (C-18).
    let skip = crate::model::superseded_draft_indices(records, |k| &k.rec);
    let index_turns = crate::model::group_turn_indices_core(records, |k| k.rec.opens_turn(), &skip);
    // ExitPlanMode plan pointers for this session (§4.2.4) - a rejection-with-message
    // hit surfaces a `[plan: <path>]` pointer. Cheap; empty in a no-plan session.
    let plan_index = PlanIndex::from_records(records.iter().map(|k| &k.rec));

    let filter = args.label_filter();
    // `tool_use_id → tool name` across the whole file, so a `tool-response` (a bare
    // `tool_result` carrying only the id) can name the tool it answers (e.g. `tool-response Edit`).
    let tool_names = build_tool_name_index(records);
    // The `▹` pairing id sets (GOLD §7): every `tool_use` id + every `tool_result` `tool_use_id`
    // in this transcript, joined GLOBALLY (not by contiguity) so a use↔result pair resolves across
    // records / parallel calls. A use with no result-id ⇒ pending; a result with no use-id ⇒ orphan.
    let (use_ids, result_ids) = tool_pair_ids(records);
    // Cross-record classify context (GOLD §6): owner identity, subagent-ness, parent id, the first
    // turn-opener line (the subagent spawn-prompt seed), and a spawn lookup. The lookup is HOISTED
    // (GOLD §3): `run_search` built one `DiscoveredSpawns` per DISTINCT discovery-root up front, so
    // here we just BORROW this file's root's entry from the shared map - never re-run the (formerly
    // O(N²) per-file) `discover_subagents` dir+meta scan.
    let spawn_lookup = spawn_map
        .get(&discovery_root_for(path))
        .and_then(|o| o.as_deref());
    let first_opener_line = records
        .iter()
        .find(|k| k.rec.opens_turn())
        .map(|k| k.line_no);
    let env = ClassifyEnv {
        owner_id: &session_id,
        is_subagent,
        parent_id: &parent_session_id,
        first_opener_line,
        spawn: spawn_lookup.map(|s| s as &(dyn SpawnLookup + Sync)),
    };
    // `--no-truncate` lifts the excerpt cap so a found message renders end-to-end (no `… (+N)`).
    // Addressing (`--line`/`--uuid`) means "fetch THIS record" → always full, no excerpt cap.
    let excerpt_max = if args.no_truncate || address.is_some() {
        usize::MAX
    } else {
        EXCERPT_MAX
    };
    // Resolve the `--turn` spec against THIS transcript's turn count (0-based), so
    // open/from-end forms (`N..`, `-3..` = the last 3) materialize per-file.
    let turn_bounds = turn_range.map(|spec| spec.resolve(index_turns.len(), false));

    // Build one turn's Exchange (or None when range-filtered / hit-free) - ONE closure
    // shared verbatim by the serial and parallel walks below, so the two paths cannot
    // drift apart.
    let build_exchange = |turn_index: usize, idxs: &[usize], draft: bool| -> Option<Exchange> {
        // Turn-range filter (inclusive, 0-based on genuine-user order). A draft sits
        // OUTSIDE turn numbering, so the range never applies to it (it is only reachable
        // by an explicit address anyway).
        if !draft {
            if let Some((lo, hi)) = turn_bounds {
                if turn_index < lo || turn_index > hi {
                    return None;
                }
            }
        }

        let turn = Turn {
            index: turn_index,
            records: idxs.iter().map(|&i| &records[i]).collect(),
        };

        // Collect the hits in this turn that satisfy category + time + regex, plus the
        // turn-record indices that produced them (so siblings can exclude matched records).
        let (mut hits, hit_idxs) = collect_turn_hits(
            &turn,
            filter,
            matcher,
            time_window,
            args.resolve_persisted,
            excerpt_max,
            &plan_index,
            &tool_names,
            address,
            &env,
        );
        if hits.is_empty() {
            return None;
        }

        // `--siblings`: render the turn's NON-matched records (the rest of the
        // back-and-forth) so a matched user question surfaces with the agent's reply -
        // fixed policy (see [`sibling_cap`]), the capped-away remainder counted.
        let (mut siblings, siblings_hidden) = if want_siblings {
            collect_turn_siblings(
                &turn,
                &hit_idxs,
                args.resolve_persisted,
                excerpt_max,
                &plan_index,
                &tool_names,
                &env,
            )
        } else {
            (Vec::new(), 0)
        };

        // Resolve the `▹` tool-pairing state of every tool hit/sibling against the file-level id
        // sets (GOLD §7) now that the hits are collected.
        for h in hits.iter_mut().chain(siblings.iter_mut()) {
            set_pairing(h, &use_ids, &result_ids);
        }

        let record_uuids = turn
            .records
            .iter()
            .filter_map(|k| k.rec.uuid.clone())
            .collect();

        // Chronological key for the combined timeline: the turn-opening (genuine-user)
        // record's timestamp, falling back to the earliest hit's timestamp when the
        // opener carries none. ISO-8601 UTC sorts lexicographically == chronologically.
        let started_utc = turn
            .records
            .first()
            .and_then(|k| k.rec.timestamp.clone())
            .or_else(|| hits.iter().find_map(|h| h.timestamp_utc.clone()));

        let turn_line_nos: Vec<usize> = turn
            .records
            .iter()
            .map(|k| k.line_no)
            .filter(|&n| n > 0)
            .collect();
        let turn_lines = match (turn_line_nos.iter().min(), turn_line_nos.iter().max()) {
            (Some(&a), Some(&b)) => (a, b),
            _ => (0, 0),
        };
        Some(Exchange {
            session_id: session_id.clone(),
            is_subagent,
            parent_session_id: parent_session_id.clone(),
            turn_index: turn.index,
            started_utc,
            hits,
            siblings,
            siblings_hidden,
            turn_lines,
            record_uuids,
            superseded_draft: draft,
        })
    };

    // Per-turn match+render is INDEPENDENT work, and it used to run serially per file -
    // on a scoped query against a single giant transcript the whole phase sat on one
    // worker while the pool idled (the dominant `cvwait` in a real-corpus profile).
    // The fan-out is DOUBLE-GATED: `inner_parallel` (the caller's scope is too small to
    // fill the pool from the outside - a broad scan keeps the serial walk: nested
    // fan-out under a saturated pool measurably ADDS steal churn) and a size threshold
    // (a small file's join overhead isn't worth it). An ordered collect keeps the
    // output byte-identical to the serial walk.
    const PAR_TURNS_MIN_RECORDS: usize = 1024;
    let mut out: Vec<Exchange> = if inner_parallel && records.len() >= PAR_TURNS_MIN_RECORDS {
        index_turns
            .par_iter()
            .enumerate()
            .filter_map(|(i, idxs)| build_exchange(i, idxs, false))
            .collect()
    } else {
        index_turns
            .iter()
            .enumerate()
            .filter_map(|(i, idxs)| build_exchange(i, idxs, false))
            .collect()
    };

    // C-18: an EXPLICIT address reaches a superseded draft too (the refetch law - it IS
    // a real record; only turn numbering excludes it). Each addressed draft renders as
    // its own annotated unit; scans never emit one.
    if address.is_some() {
        let mut draft_idxs: Vec<usize> = skip.iter().copied().collect();
        draft_idxs.sort_unstable();
        for i in draft_idxs {
            out.extend(build_exchange(0, &[i], true));
        }
    }

    // The turn COUNT rides along as the `--turn` resolution domain (show's miss reporting).
    (out, index_turns.len(), skip.len())
}

/// The FIXED `--siblings` policy (the former per-selector cap DSL is gone - one
/// zero-argument flag, one predictable behavior): within a matched turn's non-matched
/// records, MESSAGE-class units always render (user.*, agent.message,
/// agent.communication.*); the chattier machinery is capped per LEAF -
/// agent.thinking ≤ 2, agent.thinking.narration ≤ 1 (a summary of the reasoning
/// beside it - one suffices), agent.tool.use ≤ 3, agent.tool.result ≤ 3, harness.* ≤ 2.
/// Anything capped away is counted and surfaced as an explicit
/// `(+N more · csift show …)` pointer - self-healing, never silent.
pub(crate) fn sibling_cap(class: Class) -> Option<usize> {
    if class == Class::AgentThinkingNarration {
        return Some(1);
    }
    let path = class.path();
    if path.starts_with("agent.thinking") {
        Some(2)
    } else if path.starts_with("agent.tool.") {
        Some(3)
    } else if path.starts_with("harness") {
        Some(2)
    } else {
        None // user.* / agent.message / agent.communication.* - always rendered
    }
}

/// One reconstructed turn (the opening genuine-user record + every record chained
/// under it, in file order).
pub(crate) struct Turn<'a> {
    pub(crate) index: usize,
    pub(crate) records: Vec<&'a Kept>,
}

/// A [`SpawnLookup`] for one session, built from its discovered subagents (a cheap
/// `discover_subagents` dir+meta scan - NOT a transcript re-read). Maps the spawn `tool_use_id`
/// → the spawned child's agent id (the id-join) and the spawn NAME → child (the teammate
/// name-join, GOLD §4). Powers comm direction (`self ⇨ child`) + subagent-return detection in
/// [`Record::classify`]/[`Record::direction`]. Absent ⇒ those degrade to the raw name / `?`.
#[derive(Debug, Default)]
pub(crate) struct DiscoveredSpawns {
    pub(crate) by_tool_use_id: HashMap<String, String>,
    pub(crate) by_name: HashMap<String, String>,
}

impl SpawnLookup for DiscoveredSpawns {
    fn child_for_spawn_tool_use_id(&self, tool_use_id: &str) -> Option<String> {
        self.by_tool_use_id.get(tool_use_id).cloned()
    }
    fn child_for_spawn_name(&self, name: &str) -> Option<String> {
        self.by_name.get(name).cloned()
    }
}

/// The TOP-LEVEL parent session `.jsonl` for a subagent transcript path
/// `<ENCODED>/<uuid>/subagents/…/agent-<hex>.jsonl` → `<ENCODED>/<uuid>.jsonl`. `None` when `path`
/// is not under a `subagents/` dir. The parent's sidecar holds the FLAT set of ALL subagents under
/// it, so a lookup built from it resolves an in-subagent spawn / Task-return (GOLD §4).
pub(crate) fn parent_session_jsonl(path: &Path) -> Option<PathBuf> {
    for anc in path.ancestors() {
        if anc.file_name().and_then(|n| n.to_str()) == Some("subagents") {
            // The `<uuid>/` dir sits directly above `subagents/`; the parent session file is its
            // `.jsonl` sibling (a uuid carries no `.`, so `with_extension` only appends).
            return anc.parent().map(|d| d.with_extension("jsonl"));
        }
    }
    None
}

/// The DISCOVERY-ROOT for a session file - the transcript whose sidecar holds the FLAT set of
/// subagents that `classify()`/`direction()` must resolve. For a SUBAGENT transcript that is its
/// PARENT top-level `.jsonl` (ALL of a session's subagents share ONE root); for a TOP-LEVEL file
/// it is the file itself. Because the spawn lookup is IDENTICAL for every file sharing a root,
/// `run_search` builds it ONCE per distinct root and shares it - the O(N²)→O(N) hoist (GOLD §3).
/// (The `parent_session_jsonl` fallback is unreachable: `is_subagent_path` true ⇒ a `subagents/`
/// ancestor exists ⇒ `parent_session_jsonl` returns `Some`; the `unwrap_or_else` only satisfies
/// the type and, even if hit, `discover_subagents` on a subagent path yields no spawns ⇒ `None`.)
pub(crate) fn discovery_root_for(path: &Path) -> PathBuf {
    if is_subagent_path(path) {
        parent_session_jsonl(path).unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    }
}

/// Build the [`DiscoveredSpawns`] lookup powering comm direction (`self ⇨ child`) + subagent-return
/// detection, from an already-resolved DISCOVERY-ROOT (see [`discovery_root_for`]). A failed /
/// empty discovery yields `None` (the engine degrades gracefully). Cheap: dir-listing + small
/// `meta.json` reads, bounded by the subagent count - never a transcript content scan. Called ONCE
/// per distinct root (not per file) - the GOLD §3 hoist.
pub(crate) fn build_spawn_lookup(discovery_root: &Path) -> Option<DiscoveredSpawns> {
    let subs = discover_subagents(discovery_root).ok()?;
    if subs.is_empty() {
        return None;
    }
    let mut out = DiscoveredSpawns::default();
    for s in subs {
        if let Some(tuid) = s.spawn_tool_use_id {
            out.by_tool_use_id.entry(tuid).or_insert(s.agent_id.clone());
        }
        if let Some(name) = s.name {
            out.by_name.entry(name).or_insert(s.agent_id.clone());
        }
    }
    if out.by_tool_use_id.is_empty() && out.by_name.is_empty() {
        return None;
    }
    Some(out)
}

/// The per-file cross-record context [`Record::classify`]/[`Record::direction`] need (GOLD §6):
/// the transcript-owner identity, whether it is a subagent transcript, the parent id (a subagent
/// opener's FROM), the FIRST turn-opener line (the spawn-prompt seed), and the spawn lookup.
/// [`Self::ctx_for`] mints the per-record [`ClassifyCtx`] (only `is_transcript_opener` varies).
pub(crate) struct ClassifyEnv<'a> {
    pub(crate) owner_id: &'a str,
    pub(crate) is_subagent: bool,
    pub(crate) parent_id: &'a str,
    /// The physical line of the first record that `opens_turn()` - the subagent spawn-prompt seed
    /// (flips it from `user.message` to `agent.communication.inbox`). `None` ⇒ no opener.
    pub(crate) first_opener_line: Option<usize>,
    pub(crate) spawn: Option<&'a (dyn SpawnLookup + Sync)>,
}

impl ClassifyEnv<'_> {
    pub(crate) fn ctx_for(&self, kept: &Kept) -> ClassifyCtx<'_> {
        ClassifyCtx {
            owner_id: Some(self.owner_id),
            owner_name: None,
            is_subagent: self.is_subagent,
            parent_id: Some(self.parent_id),
            // Only the subagent transcript's first opener (a real native line) is the seed.
            is_transcript_opener: self.is_subagent
                && kept.line_no != 0
                && Some(kept.line_no) == self.first_opener_line,
            spawn: self.spawn.map(|s| s as &dyn SpawnLookup),
        }
    }
}

/// Gather the label-eligible, time-windowed, regex-matching hits inside a turn, plus the
/// indices (into `turn.records`) of the records that produced at least one hit - so
/// `--siblings` can exclude an already-matched record from the sibling rendering.
/// Build the `tool_use_id → tool name` index for a file's records: every `tool_use` block's
/// `{id, name}`. A later `tool_result` (which carries only the `tool_use_id`) looks its tool up
/// here so a `tool-response` row can say WHICH tool it answers. First write wins (ids are unique).
pub(crate) fn build_tool_name_index(records: &[Kept]) -> HashMap<String, String> {
    let mut map: HashMap<String, String> = HashMap::new();
    for k in records {
        if let Some(blocks) = k.rec.blocks() {
            for b in blocks {
                if let Block::ToolUse {
                    id: Some(id),
                    name: Some(name),
                    ..
                } = b
                {
                    map.entry(id.clone()).or_insert_with(|| name.clone());
                }
            }
        }
    }
    map
}

/// The `▹` pairing id sets for a file (GOLD §7): every `tool_use` block's `id` and every
/// `tool_result` block's `tool_use_id`. Joined GLOBALLY (membership, not contiguity) so a use
/// pairs with its result across records / parallel calls.
pub(crate) fn tool_pair_ids(records: &[Kept]) -> (HashSet<String>, HashSet<String>) {
    let mut uses = HashSet::new();
    let mut results = HashSet::new();
    for k in records {
        if let Some(blocks) = k.rec.blocks() {
            for b in blocks {
                match b {
                    Block::ToolUse { id: Some(id), .. } => {
                        uses.insert(id.clone());
                    }
                    Block::ToolResult {
                        tool_use_id: Some(id),
                        ..
                    } => {
                        results.insert(id.clone());
                    }
                    _ => {}
                }
            }
        }
    }
    (uses, results)
}

/// Resolve a tool hit's [`Pairing`] against the file-level id sets (GOLD §7). Pairing is a
/// property of the underlying tool_use/tool_result BLOCK, so it rides EVERY view of that
/// block - the plain `agent.tool.*` views AND the communication views that supersede them
/// under the richest-view law (a SendMessage/spawn `agent.communication.sent`/`.signal`
/// rides a tool_use block; a subagent-return `agent.communication.inbox` rides a
/// tool_result block). Without this, a FROZEN SendMessage (the dominant stuck-lane shape
/// in a teams session) fell outside the `pairing` census whenever its comm view won the
/// dedup. A use-side hit is paired iff its result-id is present (else pending - frozen /
/// elicitation / unreturned); a result-side hit is paired iff its use-id is present (else
/// orphan - compacted / sliced away). A hit with no `tool_use_id` (a record-text unit:
/// an inbound teammate-message, an idle signal section) stays `None` - outside the axis.
pub(crate) fn set_pairing(h: &mut Hit, use_ids: &HashSet<String>, result_ids: &HashSet<String>) {
    let Some(id) = h.tool_use_id.as_deref() else {
        return;
    };
    h.pair = match h.class {
        Class::AgentToolUse | Class::CommSent | Class::CommSignal => {
            Some(if result_ids.contains(id) {
                Pairing::Paired
            } else {
                Pairing::PendingNoResult
            })
        }
        Class::AgentToolResult | Class::CommInbox => Some(if use_ids.contains(id) {
            Pairing::Paired
        } else {
            Pairing::OrphanResult
        }),
        _ => None,
    };
}
