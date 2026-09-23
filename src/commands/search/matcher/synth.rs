//! The SYNTHESIZED-text marker registry and the section 7f stage-2 machinery.
//!
//! Two halves of one promise. [`synth_marker_finders`] is the registry: the raw marker
//! every synthesized render is detectable by, which is what lets the literal prefilter
//! keep its right to prove a negative. `Matcher::synth_texts_match` is the stage-2
//! re-render that spends that right - the ONE place csift re-renders a line to decide
//! whether a whole file can be skipped.
//!
//! A taxonomy family's own facts reach this file and no other shared one: a family that
//! fabricates a head registers its raw marker here, beside the marker it belongs with.

use super::*;

impl Matcher {
    /// True when the haystack carries ANY synthesized-text marker (either tier).
    pub(crate) fn synth_may_match(&self, haystack: &[u8]) -> bool {
        self.synth_verifiable
            .iter()
            .chain(self.synth_conservative.iter())
            .any(|f| f.find(haystack).is_some())
    }

    /// True when the haystack carries a CONSERVATIVE marker (must full-scan).
    pub(crate) fn synth_conservative_hits(&self, haystack: &[u8]) -> bool {
        self.synth_conservative
            .iter()
            .any(|f| f.find(haystack).is_some())
    }

    /// True when the haystack carries a VERIFIABLE marker (stage-2 re-render + check).
    pub(crate) fn synth_verifiable_hits(&self, haystack: &[u8]) -> bool {
        self.synth_verifiable
            .iter()
            .any(|f| f.find(haystack).is_some())
    }

    /// §7f stage-2: could this parsed marker-line record's SYNTHESIZED texts match?
    /// Renders through the same shared engines the hit collector uses (no drift):
    /// notification section labels + normalized `<result>` bodies
    /// (`record_text_sections` - direction/owner do not affect the TEXT, so the neutral
    /// ctx is exact), the answered-AUQ reconstruction (`auq_exchange`), and the
    /// compact-boundary content + metadata excerpt (`record_raw_text`). Any VERBATIM
    /// text these return is already covered by the literal scan, so a miss here plus a
    /// literal miss proves the record cannot hit.
    ///
    /// `raw` is the record's own source LINE, and it is not optional in spirit: the
    /// caller holds it (stage 2 collected the line to parse it), and a render that reads
    /// the line answers differently without it. `None` is for a caller that genuinely has
    /// no line, and it can only make the gate MORE conservative, never less.
    ///
    /// The re-render walks EVERY VIEW, not the line alone: the render seam dispatches on
    /// the line's own leaf when it has one and on the CALLER's class when it does not, so
    /// a gate that only ever rendered the line was structurally blind to every family
    /// reachable through a class. See the two blocks at the bottom of the body.
    pub(crate) fn synth_texts_match(&self, rec: &Record, raw: Option<&str>) -> bool {
        let ctx = crate::model::ClassifyCtx::top_level();
        if rec
            .record_text_sections(&ctx)
            .iter()
            .any(|sec| self.locate(&sec.text).is_some())
        {
            return true;
        }
        if let Some(t) = rec.auq_exchange() {
            if self.locate(&t).is_some() {
                return true;
            }
        }
        // The agents-stopped notice renders through `automation_label` (a fabricated
        // `[subagent stopped]` head) without a `<task-notification>` section.
        if let Some(t) = rec.automation_label() {
            if self.locate(&t).is_some() {
                return true;
            }
        }
        // The SOURCE LINE is handed through, and it has to be. Several renders read the
        // line rather than the record - a bookkeeping line's whole content lives in
        // fields `Record` does not model, and a family's fabricated `[<leaf> key=value]`
        // head is built from those fields - so re-rendering without it answers "cannot
        // match" for text the emission would have produced, and the whole-file gate then
        // prunes a file whose only hit lies in that head. That is the section 7 silent-drop
        // landmine, on the one path that is allowed to prove a negative.
        // FIRST the line on its own: `record_raw_text` with no class reads the line's own
        // leaf ([`render_leaf`]) and, failing that, the engine's generic arms. That one
        // call covers every class-INDEPENDENT answer there is, and for a line that HAS a
        // leaf of its own it covers the family render too (the seam takes the line's leaf
        // first, so the caller's class never changes the text).
        if let Some(t) = record_raw_text(rec, raw, None, &ctx) {
            if self.locate(&t).is_some() {
                return true;
            }
        }
        // THEN every VIEW, which is the only way to reach a family that renders a record
        // carrying no leaf of its own. An isMeta injection is a `type:"user"` line with no
        // attachment payload type and no promoted class, so `render_leaf` answers `None`
        // and the injections family is reachable ONLY through the caller's class - the one
        // the gate never had. A file whose only hit lay in such a head was pruned and
        // reported as a definitive absence while `show --line` rendered it, which is the
        // section 7 silent-drop landmine on the one path that may prove a negative.
        //
        // The union over `rendering_classes` EQUALS the union of `record_raw_text` over
        // every class: with a class in hand the only thing that changes is which family
        // is offered the record, and everything past the seam is class-independent and
        // already answered above. The loop is skipped outright for a line with its own
        // leaf, where the seam's answer does not vary with the view.
        if crate::commands::search::record_text::render_leaf(rec).is_none() {
            for class in crate::commands::search::record_text::rendering_classes() {
                if let Some(t) =
                    crate::commands::search::record_text::family_render(rec, raw, *class, &ctx)
                {
                    if self.locate(&t).is_some() {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Build the SYNTHESIZED-text marker finders for [`Matcher::synth`] (one SIMD
/// `memmem` scan per needle, per line - see the field doc for why not Aho-Corasick).
///
/// Rationale: the literal prefilter proves absence only for matchable text that is a
/// VERBATIM substring of the record's raw line bytes. A small, closed set of render
/// paths synthesizes text from other sources; each is detectable by a raw marker its
/// carrier record ALWAYS contains:
/// - `<task-notification>` - `automation_label` fabricates the kind slug
///   (`subagent`/`background-command`/…), a `completed` status fallback, and `[…]`
///   scaffolding; the inbox view normalizes the `<result>` body.
/// - `"answers"` + the three synthesized answer markers - the ANSWER carrier's
///   `auq_exchange` render fabricates the `[AskUserQuestion · N question(s)]` scaffold,
///   `Q1/A1` labels and option lists that appear verbatim nowhere in the raw line.
///   (The QUESTION-side `tool_use` needs no needle: its matchable text is
///   `render_tool_use` = the verbatim `name` + the re-serialized `input`, and the name
///   bytes sit in the raw line - a bare `AskUserQuestion` needle would disable the gate
///   for the ~29% of files whose injected context merely MENTIONS the tool.)
/// - `To tell you how to proceed` - the rejection reconstruction appends a
///   `[plan: <path>]` pointer whose path lives on a DIFFERENT record.
/// - `compact_boundary` (only when the `-t` selection can reach
///   `harness.compaction.boundary` - otherwise the boundary line is not even a scan
///   candidate, so its synthesized excerpt is unreachable) - `trigger=…`/`preTokens=…`
///   key=value text is fabricated from `compactMetadata`.
/// - `turn_duration` / `stop_hook_summary` / `file-history-` (only under an
///   explicit selector reaching the leaf) - the promoted renders fabricate
///   `[turn duration: …]` / `[stop hooks: …]` / `<path>@vN` text from the fields.
/// - `"subtype"` (only under a selector that scans `type:"system"` lines) - the system
///   family fabricates a head per EVENT KIND (`[refusal <subtype>] originalModel=…`,
///   `[api-error status=… retry N/M in Xms]`, `[notice <level>]`, `[kill]`, `[command]`,
///   `[scheduled <when>] task=… cron=… prompt=…`), and not one of those words is a byte
///   substring of the line.
/// - the sixteen BOOKKEEPING line-type values (only under a selector reaching that
///   leaf) - the family render fabricates a `[<line type> key=value ...]` head from the
///   line's own fields. COST: at most sixteen extra memmem scans per line the literal
///   prefilter rejects, and only for a query that named a `harness.bookkeeping` leaf; a
///   default scan runs none of them, because each needle's gate is the one that admits
///   the line in the first place.
/// - Under `--resolve-persisted`: `persistedOutputPath` / `Full output saved to:` -
///   the matched text is EXTERNAL file content, absent from the transcript bytes by
///   definition.
///
/// False positives only cost speed (the line/file falls back to the full parse +
/// regex pipeline); false negatives are what the set is built to make impossible.
pub(crate) fn synth_marker_finders(
    args: &SearchArgs,
) -> (Vec<memmem::Finder<'static>>, Vec<memmem::Finder<'static>>) {
    // VERIFIABLE (stage-2 re-renderable from the line alone; see `Matcher::synth_*`).
    let mut verifiable: Vec<&[u8]> = vec![
        b"<task-notification>",
        br#""answers""#,
        b"User has answered your questions",
        b"Your questions have been answered",
        b"The user answered:",
        b"The user responded:",
        // The agents-stopped kill notice renders a fabricated `[subagent stopped]` head.
        b"stopped by the user",
    ];
    if args
        .label_filter()
        .selected(Class::CompactionBoundary.path())
    {
        verifiable.push(b"compact_boundary");
    }
    // Promoted lines whose render FABRICATES text (key=value excerpts): the
    // marker is the line's own type/subtype value, active only when the explicit
    // selector admits the line (otherwise it is not a candidate at all). The queued
    // and away-summary leaves render VERBATIM content and need no marker.
    if args.reaches_gated(Class::TurnDuration) {
        verifiable.push(b"turn_duration");
    }
    if args.reaches_gated(Class::TurnStopHooks) {
        verifiable.push(b"stop_hook_summary");
    }
    if args.reaches_gated(Class::BookkeepingFileHistory) {
        verifiable.push(b"file-history-");
    }
    // The `type:"system"` event lines: the system family fabricates a head per EVENT
    // KIND and its `key=` words, none of them a byte substring of the line, so the
    // key-only `"subtype"` needle (the same needle that admits the line) marks it for
    // stage-2 re-rendering. The predicate is `scans_system_lines`, NOT `reaches_gated`:
    // a bare `harness` role selector admits these lines too (for the delivered
    // `local_command` records), and a marker set narrower than the candidate gate would
    // let the whole-file gate prune a file whose only hit is in that head.
    if args.scans_system_lines() {
        verifiable.push(br#""subtype""#);
    }
    // The BOOKKEEPING family's key=value heads (`[cost-state totalCostUSD=...]`), one
    // needle per line type, taken from the SAME table the candidate gate uses so the two
    // sets cannot drift: a marker set narrower than the gate lets the whole-file gate
    // prune a file whose only hit lies in a head, which is the section 7 silent drop on
    // the one path allowed to prove a negative. The needle IS the quoted type value,
    // which is what admits the line, so each one runs exactly when its own leaf is
    // scanned and a default scan runs none of them. `file-history-` above already covers
    // that leaf's two line types.
    for (needle, leaf) in crate::commands::search::BOOKKEEPING_NEEDLES {
        if args.scans_gated(leaf) {
            verifiable.push(needle.as_bytes());
        }
    }
    // The queue line's head is the same shape, and its leaf is not in that table: ONE
    // needle admits `queue-operation` for two leaves, and only `harness.bookkeeping.queue`
    // fabricates anything - the person's own queued text renders verbatim.
    if args.reaches_gated(Class::BookkeepingQueue) {
        verifiable.push(br#""queue-operation""#);
    }
    // CONSERVATIVE (needs cross-record / external data - force the full scan).
    let mut conservative: Vec<&[u8]> = vec![b"To tell you how to proceed"];
    if args.resolve_persisted {
        conservative.push(b"persistedOutputPath");
        conservative.push(b"Full output saved to:");
    }
    let mk = |ns: Vec<&[u8]>| {
        ns.into_iter()
            .map(|n| memmem::Finder::new(n).into_owned())
            .collect()
    };
    (mk(verifiable), mk(conservative))
}
