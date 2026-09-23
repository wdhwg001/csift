//! The TEXT surface: what a row says about itself (the role glyph, the decorated label,
//! the header token) and the stream those rows sit in.
//!
//! A PAIRED ROOT, split when the 600-line cap bound: `exchange` holds the writers BELOW an
//! exchange header (one record line and its image annotations), `notes` the two closing
//! disclosures. EVERY stdout byte this surface writes goes through ONE
//! [`crate::text::Out`], the scope and duplicate banners included, so one query has one
//! answer for a write that cannot land; a stray `println!` anywhere below the writer would
//! jump the buffer's queue - see the `text::out` module docs.
// The `text::out` obligation, mechanically: a stray print on this thread jumps the
// buffer's queue and one inside the parallel render closure hangs the process, so a
// print outside an `Out` is an error here unless it says why (see `text::out`).
#![deny(clippy::print_stdout, clippy::print_stderr)]

mod exchange;
mod notes;

pub(crate) use exchange::*;
pub(crate) use notes::*;

use std::io::Write as _;

use super::*;

/// The two EXTENSION POINTS as one label-zone string: a family's `Marker`s in push order,
/// then every `HitFact` that projects a tag.
///
/// Split out of [`render_label`] because BOTH of its arms need it. A record csift models
/// no leaf for is still a record the one stamping site ran for, so a family can and does
/// attach facts to it - the attachment family's three provenance facts are on every
/// attachment line, whether or not csift has a leaf for that payload type. Those reached
/// JSON and vanished from the text surface, which made an address the one place the two
/// formats disagreed about the same unit.
fn extension_zone(h: &Hit) -> String {
    h.markers
        .iter()
        .map(|m| format!(" {}", m.render()))
        .chain(
            h.facts
                .iter()
                .filter_map(|f| f.label_zone().map(|z| format!(" {z}"))),
        )
        // The ROOT closes the zone rather than opening it: the edges say what the record
        // states about its neighbours, and the root says what the whole unit belongs to,
        // which is the answer a reader wants LAST once they know what they are looking at.
        // A unit that opens its own turn renders nothing here - a tag saying "this is the
        // opener" on every human message would restate the line the reader is on.
        .chain(h.for_root.label_zone().map(|z| format!(" {z}")))
        .collect()
}

/// Glyph for the ROLE a hit sits on: `◂` user, `▸` agent, `⚙` harness machinery.
/// (`⚙`/gear is the chosen distinct harness marker - visually separate from the two
/// conversational sides without colliding with the `⇨`/`▹` comm/pairing markers.) An
/// UNLABELED unit (an addressed record with no modeled leaf - see [`Hit::class`]) has no
/// role to read, so it takes `?` rather than borrowing one of the three.
pub(crate) fn role_glyph(class: Option<Class>) -> char {
    match class.map(Class::role) {
        Some(crate::model::Role::User) => '◂',
        Some(crate::model::Role::Agent) => '▸',
        Some(crate::model::Role::Harness) => '⚙',
        None => '?',
    }
}

/// The rendered label for a hit: the dotted [`Class::path`], DECORATED with the engine's own
/// markers - a `▹` for a paired/pending/orphan tool hit, an `<from> ⇨ <to>` for a comm hit.
pub(crate) fn render_label(h: &Hit) -> String {
    // The EXTENSION POINTS ride the label zone after the markers this function computes
    // itself: a family's `Marker` in push order, then any `HitFact` that projects one.
    // Computed FIRST because the unmodeled arm below needs it too - a leafless unit is
    // still a record, the stamping site still runs for it, and the facts a family
    // attached there reached JSON while the text surface dropped them on the floor.
    let ext = extension_zone(h);
    // An ADDRESSED record csift models no leaf for (the refetch law, `Hit::class`): say so
    // instead of borrowing a neighbouring leaf's name - and NAME the on-disk shape when
    // the record carries one, so a reader learns what the line is rather than only that
    // csift had nothing to say about it.
    let Some(class) = h.class else {
        // FIND the shape rather than reading the first fact: a family's facts ride the
        // same list, and the shape can sit anywhere in it. The catch-all arm is
        // deliberate - every OTHER fact projects itself through `HitFact::label_zone`,
        // so only the shape replaces a label, and a family adding a variant adds no arm
        // here.
        let shape = h.facts.iter().find_map(|f| match f {
            HitFact::Unmodeled(s) => Some(s.as_str()),
            #[allow(unreachable_patterns)]
            _ => None,
        });
        // ONE unmodeled form. A leafless unit always names its shape - the block it
        // carries, its subtype, its payload type, or the isMeta flag that makes the text
        // the harness's own - and the bare word is the floor for a line whose own `type`
        // key is missing, the only case with no value left to name.
        return match shape {
            Some(s) => format!("unmodeled: {s}{ext}"),
            None => format!("unmodeled{ext}"),
        };
    };
    // Tool pairing (`▹`) takes the dedicated two-sided form below.
    // A RESULT-side hit whose tool_result carried `is_error` says so in-band -
    // pairing answers "did a result come back"; the decoration answers "was it good".
    let err = if class == Class::AgentToolResult && h.is_error == Some(true) {
        " [error]"
    } else {
        ""
    };
    // A record Claude Code's request assembler DROPPED says so in the label zone -
    // display-only, like [narration summary]: it is not matchable text, so it needs no
    // synthesized-marker registration. Reachable only through a glob or an explicit
    // leaf; a bare role never selects such a record in the first place.
    // Rendered through [`Marker`] so the marker vocabulary has ONE renderer: a family's
    // own marker and the engine's own come out in the same shape.
    let nd = if h.delivery == Some(false) {
        format!(" {}", Marker::plain("not delivered").render())
    } else {
        String::new()
    };
    // The SURVIVAL AXIS rides the label zone the same way - display-only, so it
    // needs no synthesized-marker registration. `[rewound]` says the branch WAS answered
    // before the conversation left it; `[abandoned]` says nothing ever answered.
    let sv = match (h.survival, h.rewound_branch, h.replay_copy_of) {
        (_, _, Some(line)) => format!(" [replay copy of L{line}]"),
        (crate::model::Survival::Abandoned { .. }, true, _) => " [rewound]".to_string(),
        (crate::model::Survival::Abandoned { .. }, false, _) => " [abandoned]".to_string(),
        _ => String::new(),
    };
    // The harness's own `toolDenialKind`: a rejected call's tool_result is an ERROR like
    // any other, and this says WHY it never ran. Display-only, like the markers above.
    let dk = match (class, h.denial_kind.as_deref()) {
        (Class::AgentToolResult, Some(kind)) => format!(" [denied: {kind}]"),
        _ => String::new(),
    };
    let nd = format!("{nd}{sv}{dk}{ext}");
    let nd = nd.as_str();
    match (class, h.pair) {
        (Class::AgentToolUse | Class::AgentToolResult, Some(Pairing::Paired)) => {
            return format!("agent.tool.use ▹ agent.tool.result{err}{nd}");
        }
        (Class::AgentToolUse, Some(Pairing::PendingNoResult)) => {
            return format!("agent.tool.use (no result — pending){nd}");
        }
        (Class::AgentToolResult, Some(Pairing::OrphanResult)) => {
            return format!("agent.tool.result (use not in scope){err}{nd}");
        }
        _ => {}
    }
    if !err.is_empty() {
        return format!("{}{err}{nd}", class.path());
    }
    // A narration record is an API summary, never the model's reasoning - the marker
    // rides the LABEL zone (display-only, like [error]); matchable text stays verbatim.
    if class == Class::AgentThinkingNarration {
        return format!("{} [narration summary]{nd}", class.path());
    }
    // A queued line names its queue event (and a remove's reason) in the label
    // zone - display-only; the matchable text stays the verbatim queued content.
    if class == Class::UserQueued {
        let op = h.queue_operation.as_deref().unwrap_or("queued");
        return match h.queue_reason.as_deref() {
            Some(reason) => format!("{} [{op} · {reason}]{nd}", class.path()),
            None => format!("{} [{op}]{nd}", class.path()),
        };
    }
    // A resume PLACEHOLDER says whether it closes a repair pair - display-only, like
    // [narration summary]. The label cannot carry it (one writer produces both forms, so a
    // second leaf would split one producer in two); the marker and JSON `resume_paired` do.
    if class == Class::ResumePlaceholder {
        if let Some(paired) = h.resume_paired {
            let state = if paired { "paired" } else { "unpaired" };
            return format!("{} [{state}]{nd}", class.path());
        }
    }
    // A fired prompt names the instant it fired, read off its
    // `system`/`scheduled_task_fire` sibling. Label zone, display-only, like [summarize <dir>]
    // and for the same reason: the text comes from ANOTHER record, so putting it in the
    // matchable excerpt would need a synthesized-marker registration it does not earn. The
    // prompt body stays the verbatim armed text; a fire with no readable sibling prints the
    // bare leaf rather than a guessed instant.
    if class == Class::ScheduleFire {
        if let Some(when) = h.scheduled_at.as_deref() {
            return format!("{} [scheduled fire {when}]{nd}", class.path());
        }
    }
    // A compaction summary written by a `/rewind` summarize names its direction in the
    // label zone - display-only, like [narration summary] and [not delivered]: it is not
    // matchable text, so it needs no synthesized-marker registration, and the summary body
    // stays the verbatim recap. An ordinary compaction summary carries no `summarizeMetadata`,
    // so it renders the bare leaf.
    if class == Class::CompactionSummary {
        if let Some(dir) = h.compaction.as_ref().and_then(|c| c.direction.as_deref()) {
            return format!("{} [summarize {dir}]{nd}", class.path());
        }
    }
    // Comm direction (`⇨`): append `from ⇨ to` to the label path.
    if let Some((from, to)) = &h.direction {
        return format!("{}{nd}  {from} ⇨ {to}", class.path());
    }
    format!("{}{nd}", class.path())
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

pub(crate) fn render_text(outcome: &SearchOutcome, args: &SearchArgs) -> Result<()> {
    // SCOPE banner FIRST (before the empty check) so a bare `csift search '' <uuid>` fan-out
    // announces it spanned N subagents up front - same disclosure as list/files/turns. It
    // goes through the SAME writer as the rows below it: a banner printed outside the buffer
    // would be this surface's first stdout byte, and a write that cannot land would abort
    // there instead of being reported.
    let mut out = crate::text::Out::stdout();
    crate::text::write_scope_banner(&mut out, outcome.scope_top, outcome.scope_sub)?;
    crate::path::write_duplicate_banner(&mut out, &outcome.duplicates)?;
    if outcome.exchanges.is_empty() {
        writeln!(out, "no matching exchanges")?;
        if outcome.skipped_lines > 0 {
            writeln!(
                out,
                "({})",
                crate::text::malformed_note(outcome.skipped_lines)
            )?;
        }
        out.flush()?;
        return Ok(());
    }

    // -- MATCH banner (head): the TRUE totals + the emission direction, echoed BEFORE the
    //    first exchange so a `| head`-truncated read still knows what it is looking at; the
    //    tail footer repeats the totals (the both-ends placement law - anything load-bearing
    //    must survive a consumer that amputates one end). Free: the timeline is fully
    //    materialized before the first output byte. An active cap discloses the emitted
    //    window inline; undated exchanges (sorted last) are called out only when present. --
    write!(
        out,
        "matches  {} {} · {} {} · oldest first",
        outcome.total_matched,
        noun(outcome.total_matched, "exchange", "exchanges"),
        outcome.total_sessions,
        noun(outcome.total_sessions, "session", "sessions"),
    )?;
    if outcome.dropped_by_cap > 0 {
        write!(
            out,
            " · showing {} {}",
            window_end(args),
            outcome.exchanges.len()
        )?;
    }
    if outcome.exchanges.iter().any(|ex| ex.started_utc.is_none()) {
        write!(out, " · undated last")?;
    }
    writeln!(out)?;

    // -- Self-resolving exchange headers: each header opens with a STABLE token - the leading
    //    chars of the owning transcript id - derived from the id alone, never from enumeration
    //    order, so a token pasted from ANY previous invocation still names the same transcript.
    //    The token is a valid `@` target as-is (prefix resolution is fail-loud on ambiguity),
    //    so every row is copy-paste addressable with zero joins, and the output has no
    //    O(matched-sessions) legend block ahead of the first hit. --
    let tok = header_tokens(&outcome.exchanges);

    // The exchanges render in bounded parallel chunks and reach stdout in file order. An
    // exchange's block depends on nothing but the exchange, the token map and ONE
    // cross-row decision - which row owns the single image-extraction hint - so that
    // decision is taken here, once, before any worker starts (section 7k).
    let hint = first_image_row(&outcome.exchanges);
    crate::text::write_ordered(&mut out, &outcome.exchanges, |i, ex, buf| {
        write_exchange(buf, ex, &tok, hint.filter(|h| h.exchange == i))
    })?;

    // -- Compact lowercase footer: the SAME true totals the head banner carries (the both-ends
    //    law - `-c`/`-l` isolate a single number for piping), drop accounting, unresolved. --
    let cat = if args.labels.is_empty() {
        "all".to_string()
    } else {
        args.labels.join(",")
    };
    writeln!(out)?;
    let n = outcome.total_matched;
    let ex_word = noun(n, "exchange", "exchanges");
    let n_sessions = outcome.total_sessions;
    let sess_word = noun(n_sessions, "session", "sessions");
    write!(
        out,
        "matched {n} {ex_word} · {n_sessions} {sess_word} · label={cat}"
    )?;
    if !args.labels_not.is_empty() {
        write!(out, " · label-not={}", args.labels_not.join(","))?;
    }
    if outcome.dropped_by_cap > 0 {
        write!(
            out,
            " · {} {} dropped by --max-count",
            outcome.dropped_by_cap,
            dropped_side(args)
        )?;
    }
    writeln!(out)?;
    if merged_any_sidecar(&outcome.exchanges) {
        writeln!(out, "with elicitation sidecar")?;
    }
    write_chain_disclosure(&mut out, &outcome.chain)?;
    if outcome.skipped_lines > 0 {
        writeln!(
            out,
            "({})",
            crate::text::malformed_note(outcome.skipped_lines)
        )?;
    }
    // -- Reader-caution (LAST, only when the default cap actually CLIPPED >=1 excerpt) --
    // The excerpts above are match-centered FRAGMENTS, not summaries - a consumer that trusts the
    // first sentences of a clipped fragment can badly misread the record's full intent. Tell it
    // exactly how to get the whole text. Auto-suppressed under --no-truncate / --line / --uuid (those
    // lift the cap, so nothing is truncated -> `any_truncated_excerpt` is false).
    if any_image_ids(&outcome.exchanges) {
        writeln!(
            out,
            "note: image annotations above are extractable, not decorative: \
             csift image <target> --id <ID> --out DIR (bare number for a #N handle), then \
             Read the decoded file."
        )?;
    }
    if any_truncated_excerpt(&outcome.exchanges) {
        write_truncation_caution(&mut out)?;
    }
    out.flush()?;
    Ok(())
}

/// The header tag an ABANDONED unit carries in place of `t<N>`: what kind of thing it is
/// and why it has no turn number.
pub(crate) fn abandoned_tag(ex: &Exchange) -> String {
    match ex.abandoned_kind {
        Some(crate::model::Kind::Rewound { .. }) => {
            "rewound turn (the conversation was rewound past it — outside turn numbering)"
                .to_string()
        }
        Some(crate::model::Kind::Draft { .. }) => {
            "draft (superseded — outside turn numbering)".to_string()
        }
        None => "abandoned (off the surviving conversation — outside turn numbering)".to_string(),
    }
}
