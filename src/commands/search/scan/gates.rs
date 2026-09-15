//! The CANDIDATE GATES: which lines a query admits beyond the role keep, and what each
//! one costs.
//!
//! Every keep past the two role markers is `&&`-gated on a bool derived from the active
//! `-t` selector, so a query that cannot want a line kind never reaches that kind's
//! memmem. The needles obey the R13 law - a bare VALUE substring or a quoted KEY, never a
//! compact `"key":"value"` pair, which a reserialized line would silently drop.

use super::*;

/// The 15 non-transcript line types that need a candidate needle of their own, paired
/// with the leaf whose selector gates it. `file-history-*` and `queue-operation` keep
/// their older dedicated gates, so they are not repeated here.
///
/// R13 needle law: each is a QUOTED VALUE (`"cost-state"`), which survives a
/// reserialize and which prose cannot carry - an in-content quote is escaped to `\"` in
/// raw bytes. The exception is `mode`, whose quoted value is also a common KEY, so its
/// needle is deliberately broad and the PARSE decides: the gate is only ever on when a
/// selector reached `harness.bookkeeping.mode`.
pub(crate) const BOOKKEEPING_NEEDLES: [(&str, Class); 15] = [
    ("\"last-prompt\"", Class::BookkeepingLastPrompt),
    ("\"mode\"", Class::BookkeepingMode),
    ("\"permission-mode\"", Class::BookkeepingMode),
    ("\"ai-title\"", Class::BookkeepingName),
    ("\"agent-name\"", Class::BookkeepingName),
    ("\"atis-latch\"", Class::BookkeepingAtis),
    ("\"bridge-session\"", Class::BookkeepingBridge),
    ("\"history-suppression\"", Class::BookkeepingSuppression),
    ("\"pr-link\"", Class::BookkeepingPrLink),
    ("\"frame-link\"", Class::BookkeepingArtifact),
    ("\"artifact-comment-monitor\"", Class::BookkeepingArtifact),
    ("\"artifact-autoreact-ledger\"", Class::BookkeepingArtifact),
    ("\"cost-state\"", Class::BookkeepingCost),
    ("\"fork-context-ref\"", Class::BookkeepingFork),
    ("\"continued-in\"", Class::BookkeepingContinued),
];

/// One bit per [`BOOKKEEPING_NEEDLES`] entry, set when that needle's leaf is reached by
/// an explicit selector (or by an address). Zero on every default scan, which is what
/// short-circuits the whole group before any memmem runs.
pub(crate) fn bookkeeping_mask(args: &SearchArgs, addressed: bool) -> u32 {
    let mut mask = 0u32;
    for (i, (_, leaf)) in BOOKKEEPING_NEEDLES.iter().enumerate() {
        if args.scans_gated(*leaf) || addressed {
            mask |= 1 << i;
        }
    }
    mask
}

/// The `&&`-gated candidate keeps beyond the role marker (each one a SIMD memmem that
/// runs only when its gate is on, so a default scan pays ZERO for all of them).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CandidateGates {
    /// D7: the `compact_boundary` metrics record (selected label).
    pub(crate) compact_boundary: bool,
    /// `-t harness.hook.context` (or an address): hook-injected context attachments.
    pub(crate) hook_context: bool,
    /// `--attachments` / the attachment axis (or an address): every attachment line.
    pub(crate) attachments: bool,
    /// v0.11.0 (a selector reaching `agent.communication.channel`, which a bare scan
    /// does): a csift-channel delivery attachment. DEFAULT-ON, unlike every other
    /// attachment keep - a delivery is a message, not machinery.
    pub(crate) channel: bool,
    /// v0.10.0 (explicit selector or address): `queue-operation` lines.
    pub(crate) queued: bool,
    /// v0.13.0: one bit per [`crate::cli::attachment_view_needles`] entry, set when that
    /// entry's leaf is reached by a selector (or by an address). Zero on a query that
    /// names neither, which short-circuits the whole group - and never turns the COARSE
    /// attachment keep on, which is the point: `-t user` and `-t agent` both reach one of
    /// these leaves and used to parse every attachment line in the transcript.
    pub(crate) attachment_views: u32,
    /// v0.10.0: `file-history-snapshot` + `file-history-delta` lines.
    pub(crate) snapshot: bool,
    /// v0.10.1, widened at v0.13.0: EVERY `type:"system"` subtype but the compaction
    /// boundary - the `harness.system.*`, `harness.turn.*` and `harness.schedule.event`
    /// leaves. One key-only `"subtype"` needle covers the whole axis, which is why the
    /// three v0.10.0 per-subtype value needles are gone: their flags were this one and
    /// every line they could admit carries the key.
    pub(crate) system: bool,
    /// v0.12.2 (a selector reaching `harness.schedule.fire`, which a bare scan does): the
    /// `system`/`scheduled_task_fire` line that names WHEN a fired prompt fired. DEFAULT-ON
    /// like the channel keep, and demoted to a chain row again once its instant is read.
    pub(crate) schedule_fire: bool,
    /// v0.13.0: one bit per [`BOOKKEEPING_NEEDLES`] entry (`bookkeeping_mask`). Zero on a
    /// default scan, which short-circuits the whole group.
    pub(crate) bookkeeping: u32,
}

/// One bit per [`crate::cli::attachment_view_needles`] entry, set when a selector reaches
/// that entry's leaf. Zero on every query that names neither, which is what keeps the
/// group off the flagless path.
pub(crate) fn attachment_view_mask(args: &SearchArgs, addressed: bool) -> u32 {
    let mut mask = 0u32;
    for (i, (_, leaf)) in crate::cli::attachment_view_needles().iter().enumerate() {
        // The CHANNEL keep's gate: on unless the label set EXCLUDES the leaf. A flagless
        // scan is a SUPERSET of `-t user` and of `-t agent`, so neither the person's own
        // message nor the model's own answer can be reachable only from the narrower
        // query - while `-t harness` still short-circuits before the memmem. Every OTHER
        // attachment leaf keeps `reaches_gated`, the gated-leaf rule, through the coarse
        // keep above.
        if addressed || args.label_filter().selected(leaf.path()) {
            mask |= 1 << i;
        }
    }
    mask
}

/// The BIT a record's own attachment VIEW answers to, if it is one - the demotion's half
/// of [`attachment_view_mask`]. `None` for every record that is not one of the two views,
/// which is every ordinary attachment line.
#[must_use]
pub(crate) fn record_view_bit(rec: &Record) -> Option<u32> {
    let leaf = rec.attachment_view_leaf()?;
    crate::cli::attachment_view_needles()
        .iter()
        .position(|(_, c)| *c == leaf)
        .map(|i| 1u32 << i)
}

/// The per-VIEW needles: one quoted payload value each, tested only when a selector
/// reaches that leaf. Built from the one table so the needle and the leaf it admits
/// cannot drift ([`crate::cli::attachment_view_needles`]).
static VIEW_FINDERS: std::sync::LazyLock<Vec<memmem::Finder<'static>>> =
    std::sync::LazyLock::new(|| {
        crate::cli::attachment_view_needles()
            .into_iter()
            .map(|(n, _)| memmem::Finder::new(n.as_bytes()))
            .collect()
    });

/// The per-view keeps: only the needles whose bit is set.
fn view_hit(line: &[u8], mask: u32) -> bool {
    VIEW_FINDERS
        .iter()
        .enumerate()
        .any(|(i, f)| mask & (1 << i) != 0 && f.find(line).is_some())
}

/// The bookkeeping keeps: run ONLY the needles whose bit is set, so a selector naming
/// one leaf never pays for the other fourteen. Called behind a `gates.bookkeeping != 0`
/// short-circuit, so a default scan reaches none of it.
fn bookkeeping_hit(line: &[u8], mask: u32) -> bool {
    static FINDERS: std::sync::LazyLock<Vec<memmem::Finder<'static>>> =
        std::sync::LazyLock::new(|| {
            BOOKKEEPING_NEEDLES
                .iter()
                .map(|(n, _)| memmem::Finder::new(n.as_bytes()))
                .collect()
        });
    (0..BOOKKEEPING_NEEDLES.len())
        .filter(|i| mask & (1 << i) != 0)
        .any(|i| FINDERS[i].find(line).is_some())
}

/// §7d stage-1 category prefilter on raw bytes: keep a line only if it could be a
/// transcript message (user/assistant role marker) - drops `attachment`,
/// `file-history-*`, `queue-operation`, and metadata noise pre-JSON unless the
/// matching [`CandidateGates`] flag admits them. Kept deliberately permissive
/// (substring, not structural) so no genuine turn is lost.
pub(crate) fn line_is_transcript_candidate(line: &[u8], gates: &CandidateGates) -> bool {
    let needs_compact_boundary = gates.compact_boundary;
    let needs_hook_context = gates.hook_context;
    let needs_attachments = gates.attachments;
    // Every user/assistant record carries a `"role":"user"`/`"role":"assistant"`
    // marker (genuine-user string content, tool carriers, assistant blocks all do).
    // R13: matched serialization-tolerantly - `"role": "user"` (reserialized JSON,
    // whitespace around the colon) is the same record and must not vanish silently.
    static COMPACT_BOUNDARY_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"compact_boundary"));
    // R13 needle law: a bare VALUE substring (the attachment payload's `type` value), never a
    // compact `"key":"value"` byte pair - a reserialized line keeps the value intact.
    static HOOK_CONTEXT_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"hook_additional_context"));
    // Quoted needle: `"attachment"` appears verbatim as the record's `"type"` VALUE (and as
    // its payload KEY); an in-content quote is escaped to `\"` in raw bytes, so prose that
    // merely mentions the word never false-keeps. Serialization-tolerant (the quoted value
    // survives a reserialize; R13).
    static ATTACHMENT_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"attachment\""));
    // v0.11.0 csift channel: the envelope needle is a bare VALUE substring of the injected
    // content - ASCII, no JSON-escaped character - so it survives a reserialize (R13) and a
    // decoded match implies a raw match. Shared with the classifier via one constant so the
    // byte scan and the leaf can never drift.
    static CHANNEL_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(Record::CSIFT_CHANNEL_NEEDLE.as_bytes()));
    // v0.10.0 promoted lines. Quoted-value needles per the R13 law: the `type` values
    // `"queue-operation"` and `"file-history-` (a prefix covering both `-snapshot` and
    // `-delta`). The system SUBTYPES need no value needle of their own - the key-only
    // `"subtype"` keep below admits every one of them under the same flag.
    static QUEUED_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"queue-operation\""));
    static SNAPSHOT_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"file-history-"));
    // v0.10.1 catch-all system subtypes: the key-only needle `"subtype"` (every system
    // record carries it; a quoted KEY survives a reserialize, and the classify arm
    // decides which subtype it is - the already-modeled ones simply reclassify). Also the
    // second half of the D7 boundary conjunction below, which narrows the same property to
    // the one subtype it models.
    static SUBTYPE_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| memmem::Finder::new(b"\"subtype\""));
    // v0.12.2: the fired-prompt sibling. Bare VALUE substring (R13-safe) shared with the
    // classifier through one constant, and the RARE half of the conjunction below, so it is
    // tested first and the key needle runs on the handful of lines that carry it.
    static SCHEDULE_FIRE_FINDER: std::sync::LazyLock<memmem::Finder<'static>> =
        std::sync::LazyLock::new(|| {
            memmem::Finder::new(crate::model::SCHEDULED_TASK_FIRE_SUBTYPE.as_bytes())
        });
    crate::parse::line_has_role_marker(line)
        // D7: ALSO keep the rare `compact_boundary` metrics record (a `type:"system"` record with no
        // role marker) so `search -t harness.compaction.boundary` can enumerate compaction points +
        // inspect their `compactMetadata` - but ONLY when an active `-t` selector can reach that label
        // (`needs_compact_boundary`, derived once via `label_selected`). For every other query the
        // `&&` short-circuits BEFORE the memmem, so a non-boundary search pays ZERO. When it IS run,
        // the `||` chain still reaches this memmem only on lines that already failed both role checks,
        // and boundary records are rare - so the §7 perf contract holds either way.
        // The keep is a CONJUNCTION because the rare literal alone is not the discriminator:
        // over every top-level transcript on one corpus (76 files, 2661636749 bytes, lines split
        // on `\n` only) 252 of 252 true boundaries - a JSON object with top-level
        // `type=="system"` and `subtype=="compact_boundary"` - carry the key-only bytes
        // `"subtype"`, while 226 of 226 lines that carry the literal in a payload and would
        // otherwise be admitted by D7 alone (attachments and `queue-operation` lines, which no
        // other flagless keep reaches) carry it in none. The key needle is R13-safe by form (a
        // quoted KEY survives a reserialize) and the boundary finder stays FIRST, so the second
        // memmem runs only on the handful of lines carrying the rare literal.
        || (needs_compact_boundary
            && COMPACT_BOUNDARY_FINDER.find(line).is_some()
            && SUBTYPE_FINDER.find(line).is_some())
        // Opt-in hook-injected additionalContext (`-t harness.hook.context`, or an explicit
        // `show --line`/`--uuid` address - the refetch a search hit prints must resolve with no
        // selector at all). Same `&&`-gating law as the boundary: a default scan pays ZERO.
        || (needs_hook_context && HOOK_CONTEXT_FINDER.find(line).is_some())
        // Opt-in FULL attachment keep (`search --attachments` / `--count-by attachment`, or an
        // explicit address). Same `&&`-gating law: a default scan pays ZERO.
        || (needs_attachments && ATTACHMENT_FINDER.find(line).is_some())
        // v0.11.0: a csift-channel delivery, kept under a DEFAULT scan (the label gate is on
        // whenever a selector can reach the leaf). Same `&&` shape as the keeps above, so it
        // still runs only on a line that failed both role checks, and a `-t` that cannot
        // reach the leaf short-circuits before the memmem.
        || (gates.channel && CHANNEL_FINDER.find(line).is_some())
        // v0.10.0 promoted lines, each behind its own explicit-selector gate.
        || (gates.queued && QUEUED_FINDER.find(line).is_some())
        // v0.13.0: the attachment VIEWS, each behind its own needle and its own bit, so a
        // bare `-t user` or `-t agent` never turns the coarse attachment keep on.
        || (gates.attachment_views != 0 && view_hit(line, gates.attachment_views))
        || (gates.snapshot && SNAPSHOT_FINDER.find(line).is_some())
        || (gates.system && SUBTYPE_FINDER.find(line).is_some())
        // v0.12.2: the `system`/`scheduled_task_fire` sibling of a fired prompt, kept under a
        // DEFAULT scan (the label gate is on whenever a selector can reach the leaf). A
        // CONJUNCTION for the C-40 reason - measured over the 80 top-level transcripts of one
        // corpus, all 465 records with top-level `type:"system"` and `subtype:"scheduled_task_fire"`
        // carry the key-only bytes `"subtype"`, and of the lines carrying the literal WITHOUT
        // being such a record not one does. That second population is a MOVING count, not a
        // fixed one: 216 lines at the instant of the measurement, 72 of them reached by no role
        // keep, and 211 of the 216 sat in the transcript of the session doing the measuring,
        // because a session writing ABOUT the literal accumulates it in prose. That is the
        // hazard itself, and it is why the discriminator is the KEY - a property of the
        // producer - and never the literal's rarity. The rare literal is tested FIRST, so the
        // key memmem runs on those few hundred lines rather than on every non-role line.
        || (gates.schedule_fire
            && SCHEDULE_FIRE_FINDER.find(line).is_some()
            && SUBTYPE_FINDER.find(line).is_some())
        // v0.13.0: the remaining non-transcript line types. ONE short-circuit for the
        // whole group, then only the needles whose leaf a selector actually reached.
        || (gates.bookkeeping != 0 && bookkeeping_hit(line, gates.bookkeeping))
}
