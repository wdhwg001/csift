//! The THINKING family: the four `agent.thinking.*` leaves plus the two shapes that are
//! markers rather than leaves - the client-minted cancel SALVAGE block and the
//! `[Thinking removed]` text the strip path can push - and the per-block facts that
//! separate a block the API signed from one the client made.
//!
//! THE RULE THE WHOLE FAMILY FOLLOWS: the BODY is the text the block actually holds, and
//! everything csift says ABOUT a block that holds none rides the label zone instead. Two
//! of the four leaves carry no text at all - `agent.thinking.omitted` because the API
//! returned none, `agent.thinking.redacted` because the payload is ciphertext - so their
//! bodies are EMPTY. That is not a display preference: a word rendered into the body is
//! matchable text that is no substring of the raw line, which the SPEC 7d literal
//! prefilter and the 7f whole-file gate both reason about, and the section 4 law admits
//! such text only through a registered synthesized marker.
//!
//! The `data` ciphertext of a redacted block is rendered on no path but `--raw`.

use std::borrow::Cow;

use super::*;

/// What an `agent.thinking` RECORD says - the family's half of the record-level render
/// seam.
///
/// EMPTY, and structurally so: every thinking leaf is decided on a content BLOCK, so a
/// thinking unit never renders at the record level at all. The family's real render is
/// [`block_body`], which the block loop calls. This arm exists so the seam has one shape
/// for all five rendering families rather than five minus one.
pub(crate) fn render(
    _rec: &Record,
    _raw: Option<&str>,
    _class: Class,
    _ctx: &ClassifyCtx,
) -> Option<String> {
    None
}

/// What ONE thinking BLOCK says - the family's half of the per-block render seam.
///
/// The block loop calls this instead of deciding the body itself, so what a thinking
/// unit's text IS becomes this family's to change. `None` means a block shape this
/// family does not own, and the block loop emits nothing for it rather than a fabricated
/// body; today it cannot be reached, because `thinking_block_leaf` already refused
/// everything but these two before the loop gets here.
///
/// TWO arms answer the EMPTY string, and they are the two leaves that hold no readable
/// text. A `redacted_thinking` block's payload is base64 ciphertext, so rendering it
/// would put a wall of opaque bytes where a reader expects reasoning and make the
/// excerpt window meaningless - and the words that say what the block is belong in the
/// label zone, where they are display-only, rather than in matchable text that no raw
/// line carries. A `thinking` block whose text is only whitespace holds nothing readable
/// either, and it is exactly the block `agent.thinking.omitted` is assigned to (that
/// leaf's test is `trim().is_empty()`), so the empty body the leaf promises is empty
/// rather than a run of spaces.
///
/// The `Cow` is the seam's shape and every arm borrows: the block's own text, or the
/// empty string. It stays because the shared block loop's call site takes what this
/// returns by reference, and that file is not this family's to edit.
pub(crate) fn block_body(block: &Block) -> Option<Cow<'_, str>> {
    match block {
        Block::Thinking { thinking, .. } if thinking.trim().is_empty() => Some(Cow::Borrowed("")),
        Block::Thinking { thinking, .. } => Some(Cow::Borrowed(thinking.as_str())),
        Block::RedactedThinking { .. } => Some(Cow::Borrowed("")),
        _ => None,
    }
}

/// The thinking family's own facts.
///
/// Both variants are PER-BLOCK: the block they describe arrives on [`Section::block`], so
/// a record carrying two thinking blocks emits two units whose facts differ.
///
/// An EDGE to another record is never a variant here. Edges are SHARED on [`HitFact`]
/// itself, because several families carry them and a reader has to read every one the
/// same way; a family builds one through the shared helper instead of describing the
/// shape again.
#[derive(Debug, Clone)]
pub(crate) enum Fact {
    /// Character length of a `thinking` block's own `signature` field, and `None` when the
    /// block carries no such field at all - which is the honest empty, not a zero. The
    /// distinction is this family's own discriminator: a block the client minted from a
    /// cancelled stream carries the signature PRESENT and EMPTY, so collapsing an absent
    /// field onto 0 would put every unsigned block from an older transcript into that
    /// population.
    SignatureLen(Option<usize>),
    /// This thinking block holds no readable text: the API returned a signed block with no
    /// summary text (its `thinking.display` mode `omitted`). Rides the
    /// `agent.thinking.omitted` leaf alone, so a consumer can tell that absence from an
    /// excerpt that merely rendered empty for some other reason.
    TextAbsent,
}

impl Fact {
    /// The label-zone tag this fact adds, if any.
    ///
    /// NEITHER adds one, and that is a decision rather than an omission: a signature
    /// length is a number nobody reads in a label zone, and the text absence is already
    /// said there by the `[no summary text]` marker, so a second tag would read as a
    /// second fact.
    pub(crate) fn label_zone(&self) -> Option<String> {
        match *self {
            Fact::SignatureLen(_) | Fact::TextAbsent => None,
        }
    }

    /// The JSON key(s) and value(s) this fact adds to its hit object.
    pub(crate) fn json(&self) -> Vec<(&'static str, serde_json::Value)> {
        match *self {
            Fact::SignatureLen(n) => vec![("signature_len", serde_json::json!(n))],
            Fact::TextAbsent => vec![("thinking_text_absent", serde_json::json!(true))],
        }
    }
}

/// The thinking family's per-unit facts and markers.
///
/// Called for EVERY emitted unit through [`crate::commands::search::family_facts`], so
/// the first thing a filled body does is refuse a unit that is not its own. This is the
/// family that needs `section.block`: a signature belongs to the BLOCK, and a record
/// carrying two thinking blocks emits two units whose facts differ.
pub(crate) fn facts(
    rec: &Record,
    _raw: Option<&str>,
    section: &Section<'_>,
    _ctx: &ClassifyCtx,
) -> (Vec<HitFact>, Vec<Marker>) {
    // The gate that refuses a unit this family does not own. Every marker below is a
    // statement about a content BLOCK, so a record-level unit - the record text, an
    // answered AUQ, an elicitation marker - carries `None` here and gets nothing.
    let Some(block) = section.block else {
        return (Vec::new(), Vec::new());
    };
    (
        block_facts(block, section.class),
        block_markers(rec, block, section.class),
    )
}

/// The per-block FACTS a thinking unit carries into JSON.
///
/// `signature_len` rides every `thinking` block, because the signature is the field that
/// separates a block the API signed - and will accept back - from one the client minted.
/// It reports the CHARACTER length of the block's own `signature`, and `null` when the
/// block carries no such field: that is the honest empty, and it is load-bearing rather
/// than decorative here, because the family's salvage discriminator rests on exactly the
/// difference between a signature that is absent and one that is present and empty. A
/// `redacted_thinking` block has no signature field in its shape, so it reports nothing at
/// all rather than a fabricated zero.
///
/// `thinking_text_absent` rides the omitted leaf alone, so a JSON consumer can tell "the
/// API sent no summary text" from "this hit's excerpt happens to be empty".
#[must_use]
fn block_facts(block: &Block, class: Option<Class>) -> Vec<HitFact> {
    let Block::Thinking { signature, .. } = block else {
        return Vec::new();
    };
    let mut facts = vec![HitFact::Thinking(Fact::SignatureLen(
        signature.as_deref().map(|s| s.chars().count()),
    ))];
    if class == Some(Class::AgentThinkingOmitted) {
        facts.push(HitFact::Thinking(Fact::TextAbsent));
    }
    facts
}

/// The label-zone markers a content BLOCK adds to its unit, in push order.
///
/// Four shapes, and the split is WHO WROTE the block. Two are the API's own answers with
/// no text in them, so the marker is what a reader has instead of a body:
///
/// - `agent.thinking.omitted` -> `[no summary text]`: a signed block whose `thinking`
///   field the API returned empty, which is what its `thinking.display` mode `omitted`
///   means.
/// - `agent.thinking.redacted` -> `[redacted thinking]`: the block is opaque ciphertext.
///
/// The other two are the CLIENT's own text wearing the assistant's role, and there the
/// marker is what stops the unit reading as the model speaking:
///
/// - a cancel SALVAGE block -> `[cancelled stream salvage]`: text the client kept from a
///   stream the person cancelled ([`crate::model::thinking_block_is_salvage`]). It keeps
///   `agent.thinking.summary`, because the block IS a summary slot in the message; the
///   marker is what says who wrote it.
/// - an assistant TEXT block whose whole trimmed text is
///   [`crate::model::THINKING_REMOVED_TEXT`] -> `[thinking removed by the client]`: the
///   stand-in the strip path pushes when removing every thinking block would have left
///   the message empty. The test is the WHOLE text, never a substring, so an assistant
///   turn that QUOTES the phrase inside a longer message keeps its plain leaf unmarked.
///
/// `agent.thinking.narration` takes no marker here: the label renders its own
/// `[narration summary]`, and two words for one block would read as two facts.
#[must_use]
fn block_markers(rec: &Record, block: &Block, class: Option<Class>) -> Vec<Marker> {
    if let Block::Text { text } = block {
        // Only the assistant's own text blocks can be the stand-in: the leaf is what says
        // the unit is the assistant speaking, and a user text block is somebody else's
        // words.
        if class == Some(Class::AgentMessagePlain)
            && text.trim() == crate::model::THINKING_REMOVED_TEXT
        {
            return vec![Marker::plain("thinking removed by the client")];
        }
        return Vec::new();
    }
    if crate::model::thinking_block_is_salvage(rec, block) {
        return vec![Marker::plain("cancelled stream salvage")];
    }
    match class {
        Some(Class::AgentThinkingOmitted) => vec![Marker::plain("no summary text")],
        Some(Class::AgentThinkingRedacted) => vec![Marker::plain("redacted thinking")],
        _ => Vec::new(),
    }
}
