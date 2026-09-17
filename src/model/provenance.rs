//! PROVENANCE: the closed vocabulary of `for` relations, and the index they resolve
//! through.
//!
//! `for` answers one question about an emitted unit - which message does this thing
//! correspond to - and the six taxonomy families each answer it for the records they own.
//! What they were missing is a VOCABULARY: each family wrote its relation word as a bare
//! string at the call site, so the set of words was whatever the six files happened to
//! contain, nothing could enumerate it, and a census over it would have been a census over
//! an open set. A reader cannot learn a vocabulary that has no list, and a machine cannot
//! count keys that nothing declares.
//!
//! [`EdgeKind`] is that list. It is CLOSED on purpose: a relation word is a term a reader
//! learns once and then expects to mean the same thing everywhere, so adding one is an
//! edit here (and a row in the SKILL table) rather than a string typed into a family file.
//! The wire form is [`EdgeKind::as_str`], which is what the label zone prints and what
//! `--count-by for-kind` keys on.

/// The RELATION a `for` edge names: what this record is FOR.
///
/// Every variant is a word one family already renders, plus the two the engine itself
/// answers for every unit ([`EdgeKind::Parent`] and [`EdgeKind::ResultOf`]). The set is
/// small on purpose - a vocabulary a reader has to look up is a vocabulary they will not
/// use - and it is closed so that the census axis has keys it can declare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    /// The record this one was written UNDER: an attachment's parent, an isMeta
    /// injection's, the record a `queued_command` rode in on.
    AttachedTo,
    /// The call that armed a schedule. Joined by a scheduler id rather than a record
    /// uuid, so it is always the honest empty with that as its reason.
    ArmedBy,
    /// The `model_refusal_fallback` record a fallback-served assistant turn's banner
    /// belongs to - a DESCENT, which a uuid map cannot answer.
    Banner,
    /// The assistant turn a Stop hook's feedback blocked.
    Blocked,
    /// The child TRANSCRIPT a background handover moved to. The one relation whose target
    /// is a whole session and no record in it.
    ContinuedIn,
    /// The `scheduled_task_fire` record a fired prompt was submitted under.
    Fires,
    /// The tool call an attachment answers for, by the payload's own `toolUseID`.
    ForToolUse,
    /// The record a `/fork` cut at, in the transcript it was cut from.
    ForkedFrom,
    /// The hook RUN a `stop_hook_summary` belongs to, by the run id it shares with that
    /// run's injected-context attachments.
    HookRun,
    /// The assistant message an interrupt cut short.
    Interrupted,
    /// The record a `last-prompt` line's `leafUuid` names - the chain leaf at that write.
    Leaf,
    /// The record this one hangs off in Claude Code's own conversation DAG - `parentUuid`,
    /// resolved to a line. The ENGINE's own edge: every record that names a parent carries
    /// it, unless a family already named that same target with a word of its own.
    Parent,
    /// The assistant turn a harness redo nudge sent back.
    Redoes,
    /// The user message a model refusal was for.
    Refused,
    /// The message an inbound relay answers. No relay carries such a key, so this is
    /// always the honest empty - which is the point: the relation exists and the record
    /// does not state it.
    RepliesTo,
    /// The call a notification pulse reports on, by the `<tool-use-id>` it names.
    Reports,
    /// The `tool_use` block a `tool_result` answers, by the API's own `tool_use_id`. The
    /// ENGINE's other edge, and the one relation in the set that is not a uuid relation at
    /// all - which is why the index below exists.
    ResultOf,
    /// The prompt record a file-history line's `messageId` names.
    Versions,
    /// The slash-command record a delivered `local_command` is the output of.
    Wraps,
}

impl EdgeKind {
    /// The wire word: the label zone's `[for: <word> ...]` and the `--count-by for-kind`
    /// key. One spelling, so the text surface and JSON can never name a relation
    /// differently.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::AttachedTo => "attached_to",
            EdgeKind::ArmedBy => "armed_by",
            EdgeKind::Banner => "banner",
            EdgeKind::Blocked => "blocked",
            EdgeKind::ContinuedIn => "continued_in",
            EdgeKind::Fires => "fires",
            EdgeKind::ForToolUse => "for_tool_use",
            EdgeKind::ForkedFrom => "forked_from",
            // The one word in the set that is not a single token. It is what the keys
            // family already renders, and the id it names is a HOOK RUN id rather than
            // the API tool_use id its field name suggests - so the two words are the
            // distinction, and collapsing them would lose it.
            EdgeKind::HookRun => "hook run",
            EdgeKind::Interrupted => "interrupted",
            EdgeKind::Leaf => "leaf",
            EdgeKind::Parent => "parent",
            EdgeKind::Redoes => "redoes",
            EdgeKind::Refused => "refused",
            EdgeKind::RepliesTo => "replies_to",
            EdgeKind::Reports => "reports",
            EdgeKind::ResultOf => "result_of",
            EdgeKind::Versions => "versions",
            EdgeKind::Wraps => "wraps",
        }
    }

    /// Does this relation render a tag in the LABEL ZONE, or ride JSON alone?
    ///
    /// The label zone carries what a record SAYS about itself, and a family edge is exactly
    /// that: a key its producer wrote to name a target, on the few records whose shape has
    /// one. The ENGINE's two edges are not. `parent` is on nearly every unit and names the
    /// record above it, which a reader of a rendered transcript can already see;
    /// `result_of` names the call a result answers, which the pairing glyph says in one
    /// character. A tag per hit for either would cost a reader more than it tells them,
    /// while the machine surface - where a consumer FOLLOWS edges rather than reads them -
    /// carries both.
    #[must_use]
    pub fn rendered_in_label(self) -> bool {
        !matches!(self, EdgeKind::Parent | EdgeKind::ResultOf)
    }
}

/// One transcript's provenance INDEX: the join the chain's own uuid map cannot make.
///
/// The uuid map answers "this record NAMES a uuid, which line is that", which is the whole
/// resolution every family edge needs. ONE relation is not a uuid relation: a `tool_result`
/// names the `tool_use` BLOCK it answers by the API's `tool_use_id`, a wire id that appears
/// in no `uuid` field and that the uuid map therefore cannot see. So this holds the one map
/// that closes it.
///
/// It is filled by the SAME walk the tool-pairing sets already run over a file, so the file
/// pays nothing it was not already paying: what changed is that one walk now produces what
/// two used to.
#[derive(Debug, Default)]
pub struct ProvIndex {
    /// `tool_use` block id -> the 1-based jsonl LINE carrying it. The LINE and not the
    /// record index, because a line is what an edge renders and what `csift show --line`
    /// takes, and nothing downstream has the record list to resolve an index against.
    tool_use_at: std::collections::HashMap<String, usize>,
}

impl ProvIndex {
    /// Record one `tool_use` block's id at the line carrying it. FIRST WRITE WINS: the ids
    /// are unique by construction, so a second sighting is a replay copy of the same record
    /// and the call as it was MADE is the one a result should be sent to.
    pub fn insert_tool_use(&mut self, id: &str, line: usize) {
        if !self.tool_use_at.contains_key(id) {
            self.tool_use_at.insert(id.to_string(), line);
        }
    }

    /// The line carrying the `tool_use` block this id names, or `None` when this transcript
    /// does not hold it - a call a compaction dropped, or a subagent's result whose call
    /// lives in the parent's file.
    #[must_use]
    pub fn tool_use_at(&self, id: &str) -> Option<usize> {
        self.tool_use_at.get(id).copied()
    }
}

/// WHICH MESSAGE this unit corresponds to: the record that opened its turn, and how far up
/// the parent chain it sits.
///
/// This is the primitive's own question, and it is answered for every emitted unit rather
/// than for the few records that carry a key naming a target. An edge says what a record
/// STATES about one neighbour; the root says what the record BELONGS to, which is what a
/// reader holding one hit actually wants to know.
///
/// THREE answers, and none of them is a silently absent field. A unit that IS the opener
/// says so (`turn opener`) - it corresponds to nothing above it, which is a fact and not a
/// gap. A unit the parent chain walks to the opener from carries the address AND the hop
/// count, because the distance is what separates the reply to a question from a tool result
/// forty calls into the same turn. A unit whose chain does not reach the opener still
/// carries the opener's address - the turn grouping states it - with a null hop count and
/// the reason, because csift knows WHICH turn it is in without being able to say how it got
/// there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootLink {
    /// The opening record's 1-based jsonl line, when there is an opener above this unit.
    pub line: Option<usize>,
    /// The opening record's uuid, the address that survives a transcript rewrite.
    pub uuid: Option<String>,
    /// Parent hops from this unit's record up to the opener; `None` when the chain did not
    /// reach it.
    pub hops: Option<u32>,
    /// Why the line or the hop count is absent, when one of them is.
    pub note: Option<&'static str>,
}

/// The note a unit that opens its own turn carries: it corresponds to nothing above it.
pub const ROOT_IS_OPENER: &str = "turn opener";
/// The note a unit whose parent chain does not reach its turn opener carries.
pub const ROOT_UNCHAINED: &str = "not reached by the parent chain";

impl RootLink {
    /// The answer for a unit that opens its own turn.
    #[must_use]
    pub fn opener() -> RootLink {
        RootLink {
            note: Some(ROOT_IS_OPENER),
            ..RootLink::default()
        }
    }

    /// The answer for a unit under an opener: its address, and the hop count when the
    /// parent chain reached it.
    #[must_use]
    pub fn under(line: usize, uuid: Option<String>, hops: Option<u32>) -> RootLink {
        RootLink {
            line: (line > 0).then_some(line),
            uuid,
            hops,
            note: hops.is_none().then_some(ROOT_UNCHAINED),
        }
    }

    /// The compact label-zone form: `[root L12]`, or `[root L12 +3]` when the chain reached
    /// it. `None` for the opener itself, whose tag would restate the line the reader is
    /// already looking at.
    #[must_use]
    pub fn label_zone(&self) -> Option<String> {
        let line = self.line?;
        Some(match self.hops {
            Some(h) => format!("[root L{line} +{h}]"),
            None => format!("[root L{line}]"),
        })
    }
}

#[cfg(test)]
mod tests;
