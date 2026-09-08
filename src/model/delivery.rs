//! The record-level delivery override: the three cases where Claude Code's own
//! request-assembler drop predicate disagrees with the leaf default
//! ([`Class::llm_visible`]).
//!
//! The assembler that turns a transcript into an API request walks the record list and
//! skips a record iff its drop predicate says so. Read at Claude Code 2.1.258, the
//! predicate KEEPS an `api_system` record outright (its first branch returns early) and
//! then drops a `progress` record, a `system` record that is NOT
//! `subtype:"local_command"`, a user/assistant record marked `isVirtual`, and the
//! assistant API-error placeholder (`isApiErrorMessage` with the `<synthetic>` model);
//! everything else is sent. Its `case"system"` arm then RE-MINTS the spared
//! `local_command` record as an ordinary user message built from the record's own
//! `content`, `uuid` and `timestamp`.
//!
//! Nothing on that path reads `isMeta`, `isVisibleInTranscriptOnly` or any slash-command
//! tag, so neither is a delivery instrument (they stay authorship and display flags).
//!
//! Only THREE of the predicate's verdicts need an override, and the reason differs per
//! arm. A `progress` record and an `api_system` record are UNREACHABLE rather than
//! invisible: csift models no leaf for either type, so neither can be selected at all
//! and the predicate's verdict on them - drop and keep respectively - has nothing to
//! disagree with (the corpus holds no line of either type). Every non-`local_command`
//! system subtype does carry a leaf, and that leaf's default already says invisible,
//! which is the verdict the predicate reaches too. That leaves the three cases below,
//! where the predicate and the leaf default genuinely part ways.

use super::*;

impl Record {
    /// Whether Claude Code's request assembler sent this record to the model, when its
    /// verdict DIFFERS from the leaf default. `None` = no override, so
    /// [`Class::llm_visible`] decides.
    ///
    /// - a `system`/`local_command` record => `Some(true)`: message-less, so the leaf
    ///   default calls it invisible, yet the assembler re-mints it as a user message.
    ///   Its content is a slash command's own echo and stdout.
    /// - a user/assistant record with `isVirtual:true` => `Some(false)`.
    /// - an assistant record with `isApiErrorMessage:true` whose `message.model` is the
    ///   [`SYNTHETIC_MODEL`] sentinel => `Some(false)`: the harness's own API-error notice,
    ///   rendered to the human and never returned to the model.
    ///
    /// The RESUME PLACEHOLDER (`harness.resume.placeholder`) shares that sentinel model and
    /// is deliberately NOT covered by this arm: the loader mints it WITHOUT
    /// `isApiErrorMessage`, so the drop predicate's own conjunction is false and the record
    /// reaches the model like any other. Its leaf default already says visible, so no
    /// override is needed - the pair is delivered, which is the whole point of splicing it.
    ///
    /// A BARE role selector (`-t harness`) asks for what the model received, so it
    /// keys on this per RECORD; an intermediate prefix, a glob and an explicit leaf
    /// keep their full sets. This is the second fact assigned outside a pure
    /// per-record classify (the `user.unsent` scan-layer precedent is the first),
    /// except that here the fact IS per record - it is the SELECTION that needs it.
    #[must_use]
    pub fn delivery_override(&self) -> Option<bool> {
        let ty = self.r#type.as_deref()?;
        if ty == "system" {
            return (self.subtype.as_deref() == Some("local_command")).then_some(true);
        }
        if ty != "user" && ty != "assistant" {
            return None;
        }
        if self.is_virtual == Some(true) {
            return Some(false);
        }
        let synthetic_api_error = ty == "assistant"
            && self.is_api_error_message == Some(true)
            && self.message.as_ref().and_then(Message::model_id) == Some(SYNTHETIC_MODEL);
        synthetic_api_error.then_some(false)
    }
}
