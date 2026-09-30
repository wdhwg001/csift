//! The `harness.bookkeeping` family: the key=value HEAD of a non-transcript line type,
//! and the provenance edge such a line names.
//!
//! These lines carry no `message{}` and no `uuid`, and almost none of their fields is
//! modeled on [`Record`] - so the head is read off the SOURCE LINE. That is the one place
//! in the tree where a render parses the line, and it is affordable because every
//! bookkeeping leaf is GATED: a scan that did not name the leaf never made the line a
//! parse candidate at all, so this parse is paid only by a query that asked for it.
//!
//! The head is FABRICATED, so it is not a byte substring of the line and the whole-file
//! gate needs a raw marker for it - one per line type, registered in
//! `matcher::synth::synth_marker_finders` behind the same predicate that admits the line.
//! The precedent is the promoted leaves (`turn_duration`, `stop_hook_summary`).
//!
//! The BODY is the line itself (the matchable-text law, [`super::verbatim_body`]): the
//! head is a summary and a summary is lossy, so every key it elides and every full uuid it
//! shortens stays matchable in the body.
//!
//! THREE rules decide what a head prints, and there is no fourth:
//! - a key naming an ACCOUNT, an ORGANISATION or a RECORD prints its first 8 characters
//!   ([`SHORT_KEYS`]) - enough to correlate two lines, and an identity a reader never
//!   needs whole;
//! - a string value that is an RFC 3339 INSTANT prints as csift's local text, the form
//!   every other text instant takes (the timestamp pair law: UTC belongs to JSON, and the
//!   UTC string stays matchable in the verbatim body) - `history-suppression`'s `ts` is
//!   the one such key a head names today, and a later line type's instant takes the rule
//!   without an edit;
//! - every other value is cut at [`VALUE_MAX`] characters with the shared `... (+N chars)`
//!   marker, so one long prompt or one big nested object cannot fill the excerpt budget.
//!
//! A value that LOOKS like a session uuid (`bridgeSessionId`, `parentSessionId`) prints
//! under the second rule rather than the first, because it is a JOIN KEY carried to
//! another file and half a uuid joins nothing - not because it is re-feedable to csift,
//! which a bridge id is not.

use super::*;

/// Per-value cut for a fabricated bookkeeping head.
pub(crate) const VALUE_MAX: usize = 80;

/// The keys whose value prints as its first 8 characters: an ACCOUNT or ORGANISATION
/// identity, and a RECORD uuid. Nothing in csift consumes an account uuid, and a record
/// uuid is scannable at eight while `show --uuid` takes the full value from the body or
/// from JSON. See the module doc for why a session-shaped value is NOT here.
const SHORT_KEYS: [&str; 6] = [
    "leafUuid",
    "parentLastUuid",
    "accountUuid",
    "ownerAccountUuid",
    "ownerOrganizationUuid",
    "vetoedAgainstAccountUuid",
];

/// The keys each non-transcript line type renders, in a fixed order.
///
/// The order is the WRITER's: each list is the key order of the object literal Claude Code
/// builds for that type, so a head reads the way the line was written. A key absent from
/// the line is skipped, never printed empty - several types have optional keys, and an
/// absent one is a fact about the writer's gate rather than a hole to fill.
///
/// The head opens with the line's own `type` value, so the line types that SHARE a leaf
/// (`mode`/`permission-mode`, `ai-title`/`agent-name`, the three artifact ledgers) stay
/// apart in the output even though their label does not tell them apart.
///
/// `file-history-snapshot` / `file-history-delta` are deliberately absent: that leaf's
/// head is built from fields `Record` DOES model (`snapshot`, `trackingPath`, `backup`),
/// which names the path and the version a raw key walk could only print as nested JSON, so
/// the family delegates it to [`super::promoted_record_text`] ([`render`]).
fn render_keys(line_type: &str) -> &'static [&'static str] {
    match line_type {
        "last-prompt" => &["lastPrompt", "leafUuid", "explicit", "rewound"],
        "mode" => &["mode"],
        "permission-mode" => &["permissionMode"],
        "ai-title" => &["aiTitle"],
        "agent-name" => &["agentName"],
        "atis-latch" => &["atis"],
        "bridge-session" => &[
            "bridgeSessionId",
            "lastSequenceNum",
            "declaredDialogKinds",
            "sessionGroupingId",
            "noHistoryBackfill",
            "ownerAccountUuid",
            "ownerOrganizationUuid",
        ],
        "history-suppression" => &["cause", "vetoedAgainstAccountUuid", "ts"],
        "pr-link" => &["prNumber", "prUrl", "prRepository"],
        "frame-link" => &["path", "frameUrl", "title", "artifactCount"],
        "artifact-comment-monitor" | "artifact-autoreact-ledger" => {
            &["v", "accountUuid", "artifacts"]
        }
        "cost-state" => &["totalCostUSD", "totalDuration", "startTime"],
        "fork-context-ref" => &[
            "agentId",
            "parentSessionId",
            "parentLastUuid",
            "contextLength",
        ],
        "continued-in" => &["continuedInSessionId"],
        // The operation and its reason. The queued CONTENT is deliberately not in the
        // head: on a `harness.bookkeeping.queue` line it is a harness rider (a
        // `<task-notification>` frame, a peer relay) whose delivered twin renders under
        // its own leaf, and the raw body carries it either way. The line has no
        // `position` key - the taxonomy map's cell for this leaf names one and the
        // emitter's key set has none (claim QT-047) - so there is nothing to read.
        "queue-operation" => &["operation", "reason"],
        _ => &[],
    }
}

/// The HEAD half of a `harness.bookkeeping` unit: `[<line type> key=value ...]`, read off
/// the SOURCE LINE.
///
/// `None` when there is no source line, when it is not a JSON object, or when the line
/// type has no render keys - the caller then falls back to the record-modeled head
/// (`file-history`) or to the body alone, so a line type a later build adds renders its
/// own bytes rather than an empty bracket.
pub(crate) fn head(raw: Option<&str>) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(raw?).ok()?;
    head_of(v.as_object()?)
}

/// [`head`] over an object already parsed, so a caller that needs the line's fields for
/// something else pays ONE parse rather than two.
fn head_of(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let line_type = obj.get("type")?.as_str()?;
    let keys = render_keys(line_type);
    if keys.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::new();
    for key in keys {
        if let Some(value) = obj.get(*key) {
            parts.push(format!("{key}={}", render_value(key, value)));
        }
    }
    Some(if parts.is_empty() {
        format!("[{line_type}]")
    } else {
        format!("[{line_type} {}]", parts.join(" "))
    })
}

/// One value of a bookkeeping head: a string verbatim, an OBJECT as its sorted key list
/// (the artifact ledgers key their map by artifact slug, so the KEYS are the ids a reader
/// wants and the per-slug state is not), anything else as compact JSON. The result is cut
/// at [`VALUE_MAX`] with the shared explicit marker, and a [`SHORT_KEYS`] key is cut to
/// its first 8 characters instead.
fn render_value(key: &str, value: &serde_json::Value) -> String {
    let rendered = match value {
        serde_json::Value::String(s) if s.parse::<jiff::Timestamp>().is_ok() => {
            crate::timez::format_local_compact(Some(s))
        }
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(map) => {
            let mut names: Vec<&str> = map.keys().map(String::as_str).collect();
            names.sort_unstable();
            names.join(", ")
        }
        other => other.to_string(),
    };
    if SHORT_KEYS.contains(&key) {
        return rendered.chars().take(8).collect();
    }
    crate::text::truncate_excerpt(&rendered, VALUE_MAX)
}

/// What a `harness.bookkeeping` LINE says - the family's half of the render seam.
///
/// Called by [`super::family_render`] for every unit whose leaf sits in this family's
/// category, BEFORE the engine's generic arms; `None` falls through to them.
///
/// HEAD then BODY ([`super::head_and_body`]): the key=value summary a reader scans, and
/// the line's own verbatim JSON, which is what keeps every key the summary elided
/// matchable. The verbatim half is a byte substring of the source line, so it costs the
/// section 7 prefilter laws nothing; the head is fabricated and pays its marker instead.
///
/// SIXTEEN of the eighteen line types build their head here. The other two -
/// `file-history-snapshot` and `file-history-delta`, one leaf - keep the head
/// [`super::promoted_record_text`] builds from the fields `Record` models, because that
/// one names the tracked PATH and its version where a raw key walk could only print the
/// snapshot object as JSON.
///
/// THE GATE IS THE RECORD'S OWN LINE TYPE, and it is not redundant with the dispatch.
/// The seam reads `render_leaf(rec).or(class)`, so a record with NO leaf of its own -
/// every isMeta injection, every `<task-notification>` - arrives here carrying only the
/// CALLER's class, and answering it unconditionally hands a role-bearing record's whole
/// raw JSON line back as its body. So the family asks the one question that identifies
/// its own record: is this line's own type a bookkeeping one.
pub(crate) fn render(
    rec: &Record,
    raw: Option<&str>,
    _class: Class,
    _ctx: &ClassifyCtx,
) -> Option<String> {
    if rec
        .promoted_class()
        .is_none_or(|c| c.category() != "harness.bookkeeping")
    {
        return None;
    }
    super::head_and_body(
        head(raw).or_else(|| super::promoted_record_text(rec)),
        super::verbatim_body(raw),
    )
}

/// The `harness.bookkeeping` family's own facts.
///
/// A family's own variants live HERE and nowhere else: one enum per family is what keeps
/// `types/facts.rs` out of every family's way, and what stops two families minting one
/// idea twice.
///
/// An EDGE to another record is never a variant here. Edges are SHARED on [`HitFact`]
/// itself, because several families carry them and a reader has to read every one the
/// same way; a family builds one through the shared helper instead of describing the
/// shape again.
/// TWO of the family's five relations point at something that is NOT a record, so the
/// shared `for` edge - whose target is a uuid and a line - has no honest slot for them:
/// `continued-in` names a whole TRANSCRIPT and `cost-state` names an INSTANT. They are
/// this family's own facts instead, and each renders its honest empty rather than
/// vanishing when the key it reads is absent.
#[derive(Debug, Clone)]
pub(crate) enum Fact {
    /// The DELEGATION PIN. Each of [`HitFact`]'s six family arms forwards `label_zone`
    /// and `json` to its family's own enum, and a forward nothing ever takes is a
    /// forward that can be deleted with every test still passing - which is exactly the
    /// state an uninhabited enum leaves the crate in. It is a value a unit test can watch
    /// cross both projections whatever else the family carries, so real variants sit
    /// BESIDE it rather than replacing it.
    #[cfg(test)]
    Probe(&'static str),
    /// `cost-state`: the instant this checkpoint stopped accumulating, `startTime` plus
    /// `totalDuration`. It is the KEY the `/clear` join is computed from, not the joined
    /// session: that join runs the other way, from a cleared session's own wrapper back
    /// to the sibling holding the checkpoint, and reading it from this side means
    /// opening every sibling transcript in the project directory. The stamping site is
    /// handed the record, its line and the classify context, and none of them names this
    /// transcript's path - a `cost-state` line carries no `cwd` either, so the directory
    /// cannot even be derived. `list` reports the resolved side as `cleared_from`.
    Checkpoint(Option<String>),
}

impl Fact {
    /// The label-zone tag this fact adds, if any.
    pub(crate) fn label_zone(&self) -> Option<String> {
        Some(match self {
            #[cfg(test)]
            Fact::Probe(head) => (*head).to_string(),
            // TEXT carries the instant as local text, the form every text instant takes;
            // JSON keeps the UTC one (`json` below).
            Fact::Checkpoint(Some(at)) => {
                format!(
                    "[checkpoint closes {}]",
                    crate::timez::format_local_compact(Some(at))
                )
            }
            Fact::Checkpoint(None) => {
                "[checkpoint closes unknown (needs startTime and totalDuration as numbers)]"
                    .to_string()
            }
        })
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match self {
            #[cfg(test)]
            Fact::Probe(head) => vec![("bookkeeping_probe", serde_json::json!(head))],
            Fact::Checkpoint(at) => vec![
                ("checkpoint_closes_at", serde_json::json!(at)),
                (
                    "checkpoint_join",
                    serde_json::json!("startTime+totalDuration; list joins it as cleared_from"),
                ),
            ],
        }
    }
}

/// The `harness.bookkeeping` family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing it does is refuse a unit that is not its own. The gate is
/// [`Record::promoted_class`] rather than the bare line type, because ONE line type
/// splits between two families: a `queue-operation` carrying the person's own text is
/// `user.queued`, which is the human's voice and none of this family's business.
pub(crate) fn facts(
    rec: &Record,
    raw: Option<&str>,
    _section: &Section<'_>,
    ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    if rec
        .promoted_class()
        .is_none_or(|c| c.category() != "harness.bookkeeping")
    {
        return (Vec::new(), Vec::new());
    }
    let line_type = rec.r#type.as_deref().unwrap_or_default();
    let facts = edge(line_type, raw, ctx).map_or_else(Vec::new, |f| vec![f]);
    // No marker: a bookkeeping line is never in the model's context, and the engine's own
    // `[not delivered]` follows `Hit::delivered`, which reads that off the leaf.
    (facts, Vec::new())
}

/// The PROVENANCE relation a bookkeeping line carries - what the line is FOR.
///
/// FIVE of the eighteen line types name something outside themselves:
/// - `last-prompt` names the conversation LEAF (`leafUuid`), the record Claude Code's own
///   resume walk starts from;
/// - a file-history line names the prompt-or-write record its versions belong to
///   (`messageId`);
/// - `fork-context-ref` names the record the forked agent's prefix was cut at
///   (`parentLastUuid`) and the SESSION that record lives in (`parentSessionId`);
/// - `continued-in` names the CHILD session a background handover moved to;
/// - `cost-state` names the INSTANT the `/clear` join keys on.
///
/// FOUR of the five are the shared [`HitFact::For`] edge, built through
/// [`super::for_edge`] - the one place the uuid-to-line lookup happens, so every family's
/// edge resolves and renders alike. Three name a RECORD; `continued-in` names a whole
/// TRANSCRIPT and no record in it, which is the builder's session-only answer
/// (`[for: continued_in @<session>]`, an address a reader feeds straight back to csift).
/// The fifth points at an INSTANT, which no target shape can express without putting a
/// timestamp in a field documented as the target record's uuid, so it stays a [`Fact`]
/// variant carrying the same information.
///
/// The LINE TYPE is matched BEFORE the parse, so the thirteen types with no relation pay
/// nothing here. The five that do pay one `serde_json::from_str` of a line the query
/// already admitted - the same parse [`head`] pays on the render side, which the two
/// cannot share: they run at different points with no value between them.
fn edge(line_type: &str, raw: Option<&str>, ctx: &ClassifyCtx) -> Option<HitFact> {
    match line_type {
        "last-prompt"
        | "file-history-snapshot"
        | "file-history-delta"
        | "fork-context-ref"
        | "continued-in"
        | "cost-state" => {}
        _ => return None,
    }
    let v: serde_json::Value = serde_json::from_str(raw?).ok()?;
    let obj = v.as_object()?;
    let at = |k: &str| obj.get(k).and_then(serde_json::Value::as_str);
    Some(match line_type {
        "last-prompt" => super::for_edge(EdgeKind::Leaf, Basis::Key, at("leafUuid"), None, ctx),
        "file-history-snapshot" | "file-history-delta" => {
            super::for_edge(EdgeKind::Versions, Basis::Key, at("messageId"), None, ctx)
        }
        "fork-context-ref" => super::for_edge(
            EdgeKind::ForkedFrom,
            Basis::Key,
            at("parentLastUuid"),
            at("parentSessionId"),
            ctx,
        ),
        // No uuid at all: a handover names the child TRANSCRIPT, and nothing in it.
        "continued-in" => super::for_edge(
            EdgeKind::ContinuedIn,
            Basis::Key,
            None,
            at("continuedInSessionId"),
            ctx,
        ),
        _ => HitFact::Bookkeeping(Fact::Checkpoint(checkpoint_join(obj))),
    })
}

/// The `/clear` join key a `cost-state` line carries: `startTime + totalDuration`, the
/// instant the session this checkpoint belongs to stopped accumulating, as the UTC ISO
/// instant `list` compares a clear-minted sibling's wrapper timestamp against. The sum is
/// [`Record::checkpoint_close_ms`], the join's own reading of the two NUMBERS.
///
/// `None` when either number is missing or not one - the honest empty, which
/// [`Fact::Checkpoint`] then renders rather than dropping.
///
/// The instant is built with `jiff` here rather than through `crate::timez`, which renders
/// a raw timestamp STRING into local text and has no epoch-milliseconds-to-UTC-instant
/// form; adding one is a change to a shared base module, and this is the only caller.
fn checkpoint_join(obj: &serde_json::Map<String, serde_json::Value>) -> Option<String> {
    let close = Record::checkpoint_close_ms(obj)?;
    Some(jiff::Timestamp::from_millisecond(close).ok()?.to_string())
}
