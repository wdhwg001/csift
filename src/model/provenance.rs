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
            EdgeKind::Redoes => "redoes",
            EdgeKind::Refused => "refused",
            EdgeKind::RepliesTo => "replies_to",
            EdgeKind::Reports => "reports",
            EdgeKind::Versions => "versions",
            EdgeKind::Wraps => "wraps",
        }
    }
}

#[cfg(test)]
mod tests;
