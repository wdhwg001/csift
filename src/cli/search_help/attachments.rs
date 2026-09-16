//! The `search --help` section for the ATTACHMENTS family: the `harness.context` leaves,
//! `harness.hook.{context,outcome}`, `agent.message.structured` and the person's mid-turn
//! `queued_command` - every leaf a `type:"attachment"` payload decides, and the two bodies
//! such a line can render (the text the model received, or the payload JSON itself).
//!
//! EMPTY at the skeleton amendment, and that is deliberate: the family's sentences
//! land here, in the SAME commit series as the surface they document, without any
//! lane touching a string another lane is editing. Information parity with SKILL is
//! the contract (AGENTS section 1): every marker word, JSON key and render form the
//! family ships is named here too.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and needs
//! no trailing one. The SHAPE, which every family writes alike: `"\n\n"`, the head line
//! at column ZERO (the way every other head on the page sits), `"\n  "`, then the body
//! at indent two - and it ends on the body's last word, because the next region
//! separates itself. The regions come AFTER the JSON schema block and its closing
//! notes: a head indented under that block would read as a subsection of the JSON
//! schema rather than as a section of the page. While this const is EMPTY it renders
//! nothing at all - no head, no blank line - which a test asserts.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str = "";
