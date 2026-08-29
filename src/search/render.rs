//! Text + JSON rendering: headers, tokens, footers, envelope emission.

use super::*;

/// Glyph for the ROLE a hit sits on (GOLD §6): `◂` user, `▸` agent, `⚙` harness machinery.
/// (`⚙`/gear is the chosen distinct harness marker - visually separate from the two
/// conversational sides without colliding with the `⇨`/`▹` comm/pairing markers.)
pub(crate) fn role_glyph(class: Class) -> char {
    match class.role() {
        crate::model::Role::User => '◂',
        crate::model::Role::Agent => '▸',
        crate::model::Role::Harness => '⚙',
    }
}

/// The rendered label for a hit: the dotted [`Class::path`], DECORATED with the GOLD §4/§7
/// markers - a `▹` for a paired/pending/orphan tool hit, an `<from> ⇨ <to>` for a comm hit.
pub(crate) fn render_label(h: &Hit) -> String {
    // Tool pairing (▹) takes the dedicated two-sided form (GOLD §7).
    // C-13: a RESULT-side hit whose tool_result carried `is_error` says so in-band -
    // pairing answers "did a result come back"; the decoration answers "was it good".
    let err = if h.class == Class::AgentToolResult && h.is_error == Some(true) {
        " [error]"
    } else {
        ""
    };
    match (h.class, h.pair) {
        (Class::AgentToolUse | Class::AgentToolResult, Some(Pairing::Paired)) => {
            return format!("agent.tool.use ▹ agent.tool.result{err}");
        }
        (Class::AgentToolUse, Some(Pairing::PendingNoResult)) => {
            return "agent.tool.use (no result — pending)".to_string();
        }
        (Class::AgentToolResult, Some(Pairing::OrphanResult)) => {
            return format!("agent.tool.result (use not in scope){err}");
        }
        _ => {}
    }
    if !err.is_empty() {
        return format!("{}{err}", h.class.path());
    }
    // Comm direction (⇨): append `from ⇨ to` to the label path (GOLD §4).
    if let Some((from, to)) = &h.direction {
        return format!("{}  {from} ⇨ {to}", h.class.path());
    }
    h.class.path().to_string()
}

/// Singular/plural word pick for a count (the banner + footer share one rule).
pub(crate) fn noun<'a>(n: usize, one: &'a str, many: &'a str) -> &'a str {
    if n == 1 {
        one
    } else {
        many
    }
}

/// The end of the chronological stream a signed `--max-count` KEEPS - the head banner's
/// window word ("earliest" for `N`, "latest" for `-N`).
pub(crate) fn window_end(args: &SearchArgs) -> &'static str {
    if args.max_count.is_some_and(|n| n < 0) {
        "latest"
    } else {
        "earliest"
    }
}

/// The side the cap DROPPED - the tail footer's drop word ("later" when the earliest are
/// kept, "earlier" when the latest are).
pub(crate) fn dropped_side(args: &SearchArgs) -> &'static str {
    if args.max_count.is_some_and(|n| n < 0) {
        "earlier"
    } else {
        "later"
    }
}

/// The first `n` chars of an id (codepoint-safe; ids are ASCII in practice). A shorter id
/// renders whole.
pub(crate) fn id_prefix(id: &str, n: usize) -> &str {
    id.char_indices().nth(n).map_or(id, |(i, _)| &id[..i])
}

/// Header tokens for every DISTINCT owning transcript id in the output. A hex-led id (a
/// session uuid / a bare-hex agent id) takes its first 8 chars; when two DISTINCT in-scope ids
/// share those 8, the COLLIDING GROUP lengthens to its first 12 raw chars (for a uuid that
/// spans the first dash - still a valid `@` target); a still-colliding pair falls back to the
/// full id. A non-hex id shape (a teammate id embeds its NAME, not hex) is its own token -
/// rendered in full. Deterministic and derived from the ids alone, so tokens are STABLE
/// across invocations by construction.
pub(crate) fn header_tokens(exchanges: &[Exchange]) -> HashMap<&str, String> {
    let ids: std::collections::BTreeSet<&str> =
        exchanges.iter().map(|e| e.session_id.as_str()).collect();
    let mut tok: HashMap<&str, String> = HashMap::new();
    let mut by8: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for id in ids {
        if crate::path::is_uuid(id) || crate::path::is_bare_subagent_hex(id) {
            by8.entry(id_prefix(id, 8)).or_default().push(id);
        } else {
            tok.insert(id, id.to_string());
        }
    }
    for (p8, group) in by8 {
        if let [only] = group.as_slice() {
            tok.insert(only, p8.to_string());
            continue;
        }
        let mut by12: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for id in group {
            by12.entry(id_prefix(id, 12)).or_default().push(id);
        }
        for (p12, g) in by12 {
            if let [only] = g.as_slice() {
                tok.insert(only, p12.to_string());
            } else {
                for id in g {
                    tok.insert(id, id.to_string());
                }
            }
        }
    }
    tok
}

pub(crate) fn render_text(outcome: &SearchOutcome, args: &SearchArgs) {
    // SCOPE banner FIRST (before the empty check) so a bare `csift search '' <uuid>` fan-out
    // announces it spanned N subagents up front - same disclosure as list/files/turns.
    crate::text::emit_scope_banner(outcome.scope_top, outcome.scope_sub);
    if outcome.exchanges.is_empty() {
        println!("no matching exchanges");
        if outcome.skipped_lines > 0 {
            println!("({})", crate::text::malformed_note(outcome.skipped_lines));
        }
        return;
    }

    // ── MATCH banner (head): the TRUE totals + the emission direction, echoed BEFORE the
    //    first exchange so a `| head`-truncated read still knows what it is looking at; the
    //    tail footer repeats the totals (the both-ends placement law - anything load-bearing
    //    must survive a consumer that amputates one end). Free: the timeline is fully
    //    materialized before the first output byte. An active cap discloses the emitted
    //    window inline; undated exchanges (sorted last) are called out only when present. ──
    print!(
        "matches  {} {} · {} {} · oldest first",
        outcome.total_matched,
        noun(outcome.total_matched, "exchange", "exchanges"),
        outcome.total_sessions,
        noun(outcome.total_sessions, "session", "sessions"),
    );
    if outcome.dropped_by_cap > 0 {
        print!(
            " · showing {} {}",
            window_end(args),
            outcome.exchanges.len()
        );
    }
    if outcome.exchanges.iter().any(|ex| ex.started_utc.is_none()) {
        print!(" · undated last");
    }
    println!();

    // ── Self-resolving exchange headers: each header opens with a STABLE token - the leading
    //    chars of the owning transcript id - derived from the id alone, never from enumeration
    //    order, so a token pasted from ANY previous invocation still names the same transcript.
    //    The token is a valid `@` target as-is (prefix resolution is fail-loud on ambiguity),
    //    so every row is copy-paste addressable with zero joins, and the output has no
    //    O(matched-sessions) legend block ahead of the first hit. ──
    let tok = header_tokens(&outcome.exchanges);

    let mut image_hint_done = false;
    for ex in &outcome.exchanges {
        println!();
        // `<tok>·t6` - the id-prefix token + 0-based turn index (the SAME numbering
        // `show --turn` addresses) + the single compact local instant (offset already pins it;
        // no second UTC copy). Per-hit timestamps are omitted in text (this turn time covers
        // them); the JSON envelope still carries each hit's `ts_utc`. A subagent exchange
        // carries its parent on EVERY header (a tail-truncated read must still resolve); the
        // parent token is the plain first-8 of the owning top-level uuid - no collision
        // machinery (the resolver's fail-loud ambiguity check is the backstop).
        let t = &tok[ex.session_id.as_str()];
        if ex.is_subagent {
            println!(
                "{t}·t{} (parent {})  {}",
                ex.turn_index,
                id_prefix(&ex.parent_session_id, 8),
                format_local_compact(ex.started_utc.as_deref())
            );
        } else {
            println!(
                "{t}·t{}  {}",
                ex.turn_index,
                format_local_compact(ex.started_utc.as_deref())
            );
        }
        for hit in &ex.hits {
            print_record_line(role_glyph(hit.class), hit);
            // Close the text-mode ID-law hole for SUBAGENT hits: a line number is per-FILE, so
            // it must be fetched with the SUBAGENT's own id (`session_id`), NEVER the parent
            // uuid (that would silently fetch the WRONG record). Top-level hits are safe (the
            // header token IS an `@`-targetable prefix of the fetch id), so only the hazard
            // rows carry the explicit ready-to-run command (with the FULL id - zero ambiguity
            // risk) - the same address JSON's per-hit `refetch` gives.
            if ex.is_subagent && !hit.from_sidecar && hit.line > 0 {
                println!("      ↳ csift show @{} --line {}", ex.session_id, hit.line);
            }
            if !image_hint_done {
                if let Some(l) = image_hint_line(&ex.session_id, &hit.image_ids) {
                    println!("{l}");
                    image_hint_done = true;
                }
            }
        }
        // `--siblings`: the turn's non-matched records, under a dim `·` context marker so
        // they read as surrounding back-and-forth, not as matches.
        for sib in &ex.siblings {
            print_record_line('·', sib);
            if !image_hint_done {
                if let Some(l) = image_hint_line(&ex.session_id, &sib.image_ids) {
                    println!("{l}");
                    image_hint_done = true;
                }
            }
        }
        // The FIXED policy's capped-away remainder - explicit, with the exact fetch
        // command (self-healing escape hatch; `@<session_id>` round-trips for both a
        // top-level uuid and a subagent id).
        if ex.siblings_hidden > 0 && ex.turn_lines.0 > 0 {
            println!(
                "  · (+{} more · csift show @{} --line {}..{})",
                ex.siblings_hidden, ex.session_id, ex.turn_lines.0, ex.turn_lines.1
            );
        }
    }

    // ── Compact lowercase footer: the SAME true totals the head banner carries (the both-ends
    //    law - `-c`/`-l` isolate a single number for piping), drop accounting, unresolved. ──
    let cat = if args.labels.is_empty() {
        "all".to_string()
    } else {
        args.labels.join(",")
    };
    println!();
    let n = outcome.total_matched;
    let ex_word = noun(n, "exchange", "exchanges");
    let n_sessions = outcome.total_sessions;
    let sess_word = noun(n_sessions, "session", "sessions");
    print!("matched {n} {ex_word} · {n_sessions} {sess_word} · label={cat}");
    if !args.labels_not.is_empty() {
        print!(" · label-not={}", args.labels_not.join(","));
    }
    if outcome.dropped_by_cap > 0 {
        print!(
            " · {} {} dropped by --max-count",
            outcome.dropped_by_cap,
            dropped_side(args)
        );
    }
    println!();
    if merged_any_sidecar(&outcome.exchanges) {
        println!("with elicitation sidecar");
    }
    // C-18: the esc-edit draft collapse is DISCLOSED, never silent - a superseded opener
    // is a real record a scan deliberately hides (turn hygiene), so the count and the
    // escape hatch are stated.
    if outcome.superseded_drafts > 0 {
        println!(
            "({} superseded draft(s) collapsed — esc-edit resends outside turn numbering; \
             address one directly with csift show --line/--uuid to read it)",
            outcome.superseded_drafts
        );
    }
    if outcome.skipped_lines > 0 {
        println!("({})", crate::text::malformed_note(outcome.skipped_lines));
    }
    // ── Reader-caution (LAST, only when the default cap actually CLIPPED ≥1 excerpt) ──
    // The excerpts above are match-centered FRAGMENTS, not summaries - a consumer that trusts the
    // first sentences of a clipped fragment can badly misread the record's full intent. Tell it
    // exactly how to get the whole text. Auto-suppressed under --no-truncate / --line / --uuid (those
    // lift the cap, so nothing is truncated → `any_truncated_excerpt` is false).
    if any_image_ids(&outcome.exchanges) {
        println!(
            "note: image annotations above are extractable, not decorative: \
             csift image <target> --id <ID> --out DIR (bare number for a #N handle), then \
             Read the decoded file."
        );
    }
    if any_truncated_excerpt(&outcome.exchanges) {
        emit_truncation_caution();
    }
}

/// The trailing reader-caution printed when ≥1 excerpt was truncated: what the excerpts ARE
/// (clipped fragments, not summaries), why that matters (a fragment can misrepresent the whole),
/// and the exact flags to read the full text. Kept as its own fn so the wording lives in one
/// place (text only - JSON callers read the `excerpts_truncated` summary flag instead).
pub(crate) fn emit_truncation_caution() {
    println!();
    println!(
        "note: matches above are TRUNCATED, match-centered FRAGMENTS — not summaries. A fragment \
         can read very differently from the record's full intent, so do NOT draw conclusions from \
         it alone."
    );
    println!("  whole records: re-run with --no-truncate");
    println!(
        "  one record in full: csift show <@session|@agent-id> --line <N> (the L<n> shown on \
         a row) or --uuid <U>"
    );
}

/// One hit/sibling line: `<marker> <label>[ <tool>]  L<line>  <excerpt>` (excerpt inline; its
/// newlines are already collapsed to single spaces). `marker` is the role glyph for a match or a
/// dim `·` for a `--siblings` context record; `<label>` is the dotted path with the GOLD §4/§7
/// `⇨`/`▹` decorations ([`render_label`]).
pub(crate) fn print_record_line(marker: char, h: &Hit) {
    let label = render_label(h);
    let name = h
        .tool_name
        .as_deref()
        .map(|n| format!(" {n}"))
        .unwrap_or_default();
    let images = image_suffix(&h.image_ids);
    // A merged elicitation-sidecar hit has no physical jsonl line - render the provenance
    // locator instead of a fabricated `Lnnnn` (§3.10).
    let locator = if h.from_sidecar {
        "(elicitation sidecar)".to_string()
    } else {
        format!("L{}", h.line)
    };
    println!(
        "  {marker} {label}{name}  {locator}  {}{}",
        h.excerpt, images
    );
}

/// ` [N image(s): …]` suffix when the hit's record carries images - the SAME ids `turns` shows,
/// feedable straight to `csift image <session> --id <ID>`. Empty string when there are none.
pub(crate) fn image_suffix(ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let noun = if ids.len() == 1 { "image" } else { "images" };
    format!("  [{} {}: {}]", ids.len(), noun, ids.join(", "))
}

/// The paste-ready extraction command for a row's images, printed ONCE per run under the
/// first image-bearing row (C-11: the inline `[N image(s): #7]` annotation taught nothing
/// about extraction, and fleets classified images as unreadable without ever testing).
/// INPUT forms only: `#7` is display, `--id` takes the bare number; `L<line>i<n>` is
/// identical in both. Addressed at the row's OWNING transcript id (the per-FILE ID law).
pub(crate) fn image_hint_line(session_id: &str, ids: &[String]) -> Option<String> {
    let first = ids.first()?;
    let input = first.strip_prefix('#').unwrap_or(first);
    Some(format!(
        "      ↳ read the image(s): csift image @{session_id} --id {input} --out DIR           (decodes to a file you can then Read - an image-bearing turn is evidence, not a blank)"
    ))
}

/// True when any hit or sibling row carries image ids - gates the capability footer note.
pub(crate) fn any_image_ids(exchanges: &[Exchange]) -> bool {
    exchanges.iter().any(|ex| {
        ex.hits
            .iter()
            .chain(ex.siblings.iter())
            .any(|h| !h.image_ids.is_empty())
    })
}

/// The JSON rendering of a tool hit's `▹` pairing state (shared with `show`).
pub(crate) fn pairing_json(p: Option<Pairing>) -> serde_json::Value {
    match p {
        Some(Pairing::Paired) => serde_json::json!("paired"),
        Some(Pairing::PendingNoResult) => serde_json::json!("pending"),
        Some(Pairing::OrphanResult) => serde_json::json!("orphan"),
        None => serde_json::Value::Null,
    }
}

/// Render one `Hit` (a match OR a `--siblings` context record) to its JSON object - the
/// shared per-hit shape used by both the `hits` and `siblings` envelope arrays.
/// The ready-to-run fetch command for a hit - `csift show` addressed at the transcript that
/// OWNS the hit's line number (`session_id`, never the parent uuid: line numbers are
/// per-file, so pointing the parent at a subagent's line would silently fetch the WRONG
/// record). A sidecar hit has no physical line → address by uuid; neither → null.
pub(crate) fn refetch_json(session_id: &str, h: &Hit) -> serde_json::Value {
    if !h.from_sidecar && h.line > 0 {
        serde_json::json!(format!("csift show @{session_id} --line {}", h.line))
    } else if let Some(u) = &h.uuid {
        serde_json::json!(format!("csift show @{session_id} --uuid {u}"))
    } else {
        serde_json::Value::Null
    }
}

pub(crate) fn hit_json(ex: &Exchange, h: &Hit) -> serde_json::Value {
    let session_id: &str = &ex.session_id;
    // Comm direction (GOLD §4): `from`/`to` only for an `agent.communication.*` hit, else null.
    let (from, to) = match &h.direction {
        Some((f, t)) => (serde_json::json!(f), serde_json::json!(t)),
        None => (serde_json::Value::Null, serde_json::Value::Null),
    };
    // Tool pairing (GOLD §7): the `▹` join state of an agent.tool.use/result hit, else null.
    let pairing = pairing_json(h.pair);
    serde_json::json!({
        // The id TRIO rides EVERY hit row too (R9): bare `.hits[]` flattening is the single
        // most natural jq idiom against the most-piped command, and with the trio only on
        // the exchange row it yielded silent nulls - two independent audits tripped on it.
        // jq cannot fail loud on a missing key, so the data matches the natural access
        // pattern instead. (The exchange row keeps its copy; `refetch` stays the preferred
        // single-hit path.)
        "session_id": ex.session_id,
        "is_subagent": ex.is_subagent,
        "parent_session_id": ex.parent_session_id,
        // The matched dotted leaf path (`label`) + the record's FULL label set (`labels`).
        "label": h.class.path(),
        "labels": h.labels,
        "excerpt": h.excerpt,
        "ts_utc": h.timestamp_utc,
        "ts_local": h.timestamp_utc.as_deref().and_then(local_iso),
        "tool_name": h.tool_name,
        // Comm direction (`agent.communication.*`); null on a non-comm hit.
        "from": from,
        "to": to,
        // Tool-pairing (§7): the use↔result join state + the joining `tool_use_id`; null on a
        // non-tool hit.
        "pairing": pairing,
        "is_error": h.is_error,
        "tool_use_id": h.tool_use_id,
        // The `csift show --line/--uuid` address: 1-based source line + the record uuid (when
        // present). A merged elicitation-sidecar hit has NO physical line, so `line` is null and
        // `source:"elicitation-sidecar"` marks the provenance (§3.10); a native hit omits `source`.
        "line": if h.from_sidecar { serde_json::Value::Null } else { serde_json::json!(h.line) },
        "uuid": h.uuid,
        "source": if h.from_sidecar { serde_json::json!("elicitation-sidecar") } else { serde_json::Value::Null },
        // Extractable image ids (`#N`/`L<line>i<n>`) the record carries; empty array when none.
        "image_ids": h.image_ids,
        // The ready-to-run `csift show` command for this record - already addressed at the
        // RIGHT transcript (this row's session_id; a parent uuid + a subagent line number
        // fetches the wrong record).
        "refetch": refetch_json(session_id, h),
    })
}

pub(crate) fn render_json(
    outcome: &SearchOutcome,
    diagnosis: Option<&EmptyDiagnosis>,
) -> Result<()> {
    use serde_json::json;
    // envelope v2: header (always) → kind-tagged exchange rows → summary (always).
    println!(
        "{}",
        serde_json::to_string(&crate::text::envelope_scope_header(
            "search",
            outcome.scope_top,
            outcome.scope_sub,
            json!({})
        ))?
    );
    for ex in &outcome.exchanges {
        let hits: Vec<_> = ex.hits.iter().map(|h| hit_json(ex, h)).collect();
        let mut obj = json!({
            "kind": "exchange",
            "session_id": ex.session_id,
            // Discriminate the id-domain so a consumer can tell a re-feedable parent UUID
            // from a non-re-feedable subagent transcript hex: `is_subagent` + the always-
            // re-feedable `parent_session_id` (= session_id for a top-level hit).
            "is_subagent": ex.is_subagent,
            "parent_session_id": ex.parent_session_id,
            "turn_index": ex.turn_index,
            // Envelope-level chronological position = the turn-opening timestamp, the key
            // the combined timeline is sorted on. `ts_local` is the same instant in the
            // host TZ. Per-hit `ts_utc` (in `hits`) can diverge for a deep tool_use match.
            "ts_utc": ex.started_utc,
            "ts_local": ex.started_utc.as_deref().and_then(local_iso),
            "hits": hits,
            "record_uuids": ex.record_uuids,
        });
        // `--siblings`: attach the non-matched records of the turn (same per-hit shape).
        // Present only when there are siblings - absent ⇒ none (keeps the common envelope lean).
        if !ex.siblings.is_empty() || ex.siblings_hidden > 0 {
            let sibs: Vec<_> = ex.siblings.iter().map(|h| hit_json(ex, h)).collect();
            obj["siblings"] = json!(sibs);
            obj["siblings_hidden"] = json!(ex.siblings_hidden);
            obj["turn_lines"] = json!([ex.turn_lines.0, ex.turn_lines.1]);
        }
        println!("{}", serde_json::to_string(&obj)?);
    }
    // envelope v2 summary. `session_ids` = the distinct matching transcript ids (sorted,
    // first-100 capped with an EXPLICIT truncation flag - never silent) so "WHICH sessions
    // matched" is one `tail -1 | jq .session_ids` away, no per-row jq pipeline.
    let mut session_ids: Vec<&str> = outcome
        .exchanges
        .iter()
        .map(|ex| ex.session_id.as_str())
        .collect();
    session_ids.sort_unstable();
    session_ids.dedup();
    let ids_total = session_ids.len();
    let ids_truncated = ids_total > 100;
    session_ids.truncate(100);
    let mut summary_fields = json!({
        "matched": outcome.exchanges.len(),
        "sessions": distinct_session_count(&outcome.exchanges),
        // `transcript_ids` = the distinct MATCHING-TRANSCRIPT ids (a subagent hit contributes
        // its bare agent-id, a top-level hit its uuid). DELIBERATELY named apart from `-l`,
        // which emits the OWNING-session ids (`parent_session_id`) - the two answer different
        // "which sessions?" questions, so the wire names them differently.
        "transcript_ids": session_ids,
        "transcript_ids_truncated": ids_truncated,
        "dropped_by_cap": outcome.dropped_by_cap,
        "skipped_lines": outcome.skipped_lines,
        // C-18: superseded-draft openers the turn reconstruction collapsed (esc-edit
        // resends) - real records outside turn numbering, fetchable by explicit address.
        "superseded_drafts": outcome.superseded_drafts,
        // True when ≥1 emitted record was merged from the elicitation sidecar (§3.10) - the
        // machine echo of the `with elicitation sidecar` text note.
        "with_elicitation_sidecar": merged_any_sidecar(&outcome.exchanges),
        // True when ≥1 emitted excerpt was CLIPPED to the default cap - the machine echo of the
        // trailing reader-caution. A consumer seeing this should re-fetch the record in full
        // (per-hit `excerpt` is a match-centered fragment, not the whole text) via
        // `--no-truncate`, or one record via `csift show --line/--uuid`. Always false there.
        "excerpts_truncated": any_truncated_excerpt(&outcome.exchanges),
    });
    // Zero-match self-diagnosis (§T0.1): make the empty result machine-legible as a definitive
    // absence (never a syntax error), echo the active filters, and - when a `-t`/`-T` filter hid
    // otherwise-matching records - carry the excluded labels so a consumer can self-correct.
    if let Some(d) = diagnosis {
        summary_fields["definitive_absence"] = json!(true);
        summary_fields["active_filters"] = json!(d.active_filters);
        summary_fields["excluded_by_label"] = match &d.excluded_by_label {
            Some((rows, recs)) => {
                let by: serde_json::Map<String, serde_json::Value> =
                    rows.iter().map(|(l, n)| (l.clone(), json!(n))).collect();
                json!({ "records": recs, "by_label": serde_json::Value::Object(by) })
            }
            None => serde_json::Value::Null,
        };
    }
    let summary = crate::text::envelope_summary(summary_fields);
    println!("{}", serde_json::to_string(&summary)?);
    Ok(())
}
