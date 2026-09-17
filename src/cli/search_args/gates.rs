//! Which LINES a `search` query admits: the leaf sets derived from the shape
//! inventory, and the [`SearchArgs`] predicates that read a selector against them.
//!
//! Split from `search_args.rs` at the 600-line structure gate, along the seam the
//! file already had: everything there is the FLAG SURFACE, the struct clap builds
//! `--help` out of, and everything here answers one question about a query - does
//! it reach this gated leaf, and so does the scan parse those lines at all. The
//! candidate gate and the section 7f whole-file gate both key on these predicates,
//! which is why they are one place and not two.

use super::*;

/// The NON-TRANSCRIPT leaves - scanned only under an explicit selector
/// ([`SearchArgs::reaches_gated`]) or a `show` address. DERIVED from the shape
/// inventory's `Gate::Selector` rows, so a shape that becomes gated (or stops being)
/// moves this set with it and the honest-empties note can never name a stale leaf.
/// `harness.hook.context` is here too: `--additional-context` was retired, and the leaf
/// it gated is now reached by naming it.
#[must_use]
pub fn gated_leaves() -> Vec<Class> {
    let mut out: Vec<Class> = Vec::new();
    for r in crate::model::rows() {
        if r.gate == crate::model::Gate::Selector {
            if let Some(c) = r.leaf {
                if !out.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    // `user.queued` is the one gated leaf with no shape row of its own: the line type
    // carries `harness.bookkeeping.queue` and the CONTENT decides which of the two it
    // is, so the inventory cannot list it and this set would otherwise lose it.
    out.push(Class::UserQueued);
    out
}

/// The leaves carried by a `type:"system"` line that is NOT the compaction boundary -
/// admitted by ONE key-only `"subtype"` candidate needle, so they share one admission
/// predicate ([`SearchArgs::scans_system_lines`]). Derived from the inventory's system
/// axis, minus the boundary, which a flagless scan reaches through its own keep.
#[must_use]
pub fn system_line_leaves() -> Vec<Class> {
    let mut out: Vec<Class> = Vec::new();
    for r in crate::model::rows() {
        if matches!(r.shape, crate::model::Shape::SystemSubtype(_)) {
            if let Some(c) = r.leaf {
                if c != Class::CompactionBoundary && !out.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// Every leaf whose only carrier is an attachment PAYLOAD TYPE - the label side of the
/// COARSE attachment keep, derived from the inventory's attachment axis.
///
/// Two leaves an attachment can also reach are deliberately NOT here: a csift-channel
/// delivery and the human's own mid-turn steer are second views of a payload whose type
/// carries another leaf, so a selector naming either of them would otherwise turn the
/// coarse keep on and parse every attachment line in the transcript - which is what
/// `-t user` and `-t agent` did. Each has its own VALUE needle instead
/// ([`attachment_view_needles`]), so naming it costs one memmem over the lines that carry
/// that payload.
#[must_use]
pub fn attachment_leaves() -> Vec<Class> {
    let narrow: Vec<Class> = attachment_view_needles()
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    let mut out: Vec<Class> = Vec::new();
    for r in crate::model::rows() {
        if matches!(r.shape, crate::model::Shape::AttachmentType(_)) {
            if let Some(c) = r.leaf {
                if !out.contains(&c) && !narrow.contains(&c) {
                    out.push(c);
                }
            }
        }
    }
    out
}

/// The attachment-carried leaves that need NO coarse keep: each is reached through a
/// SINGLE payload type, so one quoted VALUE needle admits exactly its lines. A selector
/// naming one costs a single memmem over the file instead of a parse of every attachment
/// line, and the R13 law holds because each needle is a quoted value token rather than a
/// key-and-value pair.
///
/// Both entries are leaves an ordinary query names: `user.message` is the human's own
/// message (an attachment carries it when the harness delivered a mid-turn steer) and
/// `agent.message.structured` is the model's own answer, so a bare `-t user` or `-t agent`
/// reaches one. That is exactly why they cannot ride the coarse keep.
///
/// BOTH take the v0.11.0 channel keep's GATE - a scan that does not EXCLUDE the leaf
/// admits the needle, so a flagless scan reaches it while `-t user` / `-t harness` still
/// short-circuit before the memmem. That is the selector law rather than a budget: a
/// flagless scan is a SUPERSET of `-t user` and of `-t agent`, so a leaf the narrower
/// query reaches and the wider one does not is the law broken. What the two have in
/// common is that both are words someone OWNED - the person's own mid-turn steer and the
/// model's own structured answer - which is the same reason the channel keep is default-on.
///
/// The needle only ADMITS the line; the parse decides the leaf. A `queued_command` whose
/// payload `origin.kind` is not `human` stays `harness.context.task` behind that leaf's
/// own gate, so a default scan gains the two views and no machinery. Every OTHER
/// attachment leaf keeps the coarse keep, so a flagless scan parses none of them.
///
/// `agent.communication.channel` is absent on purpose: it already has its own default-on
/// needle (the envelope header), a stronger discriminator than its payload type.
#[must_use]
pub fn attachment_view_needles() -> Vec<(&'static str, Class)> {
    vec![
        ("\"queued_command\"", Class::UserMessage),
        ("\"structured_output\"", Class::AgentMessageStructured),
    ]
}

impl SearchArgs {
    /// True when the scan should keep `type:"attachment"` lines: the explicit
    /// `--attachments` flag, or the `--count-by attachment` axis (unanswerable without
    /// the gate - the D7 implied-widening precedent).
    #[must_use]
    pub fn scan_attachments(&self) -> bool {
        self.attachments || matches!(self.count_by, Some(CountAxis::Attachment))
    }

    /// The v0.10.0 gated-leaf rule: a promoted non-record leaf is scanned ONLY when an
    /// EXPLICIT `-t` selector reaches it (the full path, a glob, or the CATEGORY
    /// prefix). A bare scan with no `-t` admits every label elsewhere but never parses
    /// these lines: most queued content is a duplicate automation pulse and every
    /// promoted line is a non-message line, so the default surface stays the
    /// conversation. `-T`-only filters do not reach them either.
    #[must_use]
    pub fn reaches_gated(&self, c: Class) -> bool {
        !self.labels.is_empty() && self.label_filter().selected(c.path())
    }

    /// True when a BARE `harness` role selector is active. Such a query asks for what
    /// the model received under that role, and one system leaf carries delivered
    /// records: `harness.system.command`, a `system`/`local_command` line which Claude
    /// Code's request assembler re-mints as a user message. The scan therefore admits
    /// the `"subtype"` candidate lines under this selector too - a key-only memmem on a
    /// rare line shape - and the per-record override then keeps only the
    /// `local_command` ones. Every OTHER gated leaf stays exactly as gated.
    #[must_use]
    pub fn bare_harness_role(&self) -> bool {
        self.labels.iter().any(|l| l == "harness")
    }

    /// True when the scan parses `type:"system"` candidate lines at all: an explicit
    /// selector reaching any [`SYSTEM_LINE_LEAVES`] leaf, or a bare `harness` role
    /// selector. THE one predicate for that admission - the candidate gate and the §7f
    /// whole-file gate's synthesized-marker set must agree, or a pattern that can only
    /// match the FABRICATED `[<subtype> <level>]` head would be gated away on a file
    /// that holds it (a silent drop, the §7 landmine).
    #[must_use]
    pub fn scans_system_lines(&self) -> bool {
        system_line_leaves()
            .into_iter()
            .any(|c| self.reaches_gated(c))
            || self.bare_harness_role()
    }

    /// True when the scan actually PARSES a gated leaf's lines: an explicit selector
    /// reaches it, or - for the system-line leaves - a bare `harness` role admits them
    /// ([`SearchArgs::scans_system_lines`]). THE predicate the candidate gate and the
    /// zero-match diagnosis both key on: `reaches_gated` answers "did a selector name
    /// it", which is not the same question.
    #[must_use]
    pub fn scans_gated(&self, c: Class) -> bool {
        if system_line_leaves().contains(&c) {
            return self.scans_system_lines();
        }
        self.reaches_gated(c)
    }

    /// True when NO gated leaf's lines were parsed (the zero-match diagnosis names them
    /// so an absence is never mistaken for a scan of them). Keys on
    /// [`SearchArgs::scans_gated`]: under a bare `harness` role the system lines WERE
    /// parsed, and telling a consumer otherwise is a false statement on the
    /// honest-empties keystone.
    #[must_use]
    pub fn gated_unreached(&self) -> bool {
        !gated_leaves().into_iter().any(|c| self.scans_gated(c))
    }

    /// Does any reader of THIS scan project the ENGINE's own `for` edges?
    ///
    /// SIX relations ride JSON alone - `parent`, `result_of` and the four SURVIVAL words
    /// `supersedes` / `continues` / `replays` / `branches`, which is exactly the set
    /// `EdgeKind::rendered_in_label` calls false - so a scan whose output is TEXT and which
    /// does not census them renders every byte it renders without any of them ever being
    /// built. This gate carries all six: the two one-hop edges are built under it in
    /// `backfill_provenance`, the four survival ones beside the survival FIELDS in
    /// `backfill_survival`, and both read this answer. Building them anyway costs a string
    /// allocation and a map lookup apiece on every emitted hit, which for the two one-hop
    /// edges on a wide-match row over a 397 MiB transcript measured 0.05 s of 4.47 s CPU
    /// for facts nothing on that path can show.
    ///
    /// It is NOT a format test: the `for-kind` census is a TEXT surface that reads the
    /// edges, and keying on the format alone would empty two of its keys.
    #[must_use]
    pub fn projects_engine_edges(&self) -> bool {
        matches!(self.format, OutputFormat::Json) || self.count_by == Some(CountAxis::ForKind)
    }

    /// True when the scan keeps EVERY `type:"attachment"` line: the `--attachments` flag
    /// (and the `--count-by attachment` axis, which cannot be answered without it), or an
    /// explicit `-t` reaching a leaf whose only carrier is an attachment payload type.
    /// The flag is the WIDE form and a selector naming such a leaf is the narrow one,
    /// which is what replaced the retired flag.
    ///
    /// The two leaves an attachment reaches as a second VIEW are not in that set and do
    /// not turn this on: they carry their own needles, so `-t user` and `-t agent` cost
    /// one memmem each over the lines that carry the payload instead of a parse of every
    /// attachment line in the file.
    #[must_use]
    pub fn scans_attachment_lines(&self) -> bool {
        let leaves = attachment_leaves();
        if self.scan_attachments() {
            return leaves
                .iter()
                .any(|c| self.label_filter().selected(c.path()));
        }
        leaves.into_iter().any(|c| self.reaches_gated(c))
    }
}
