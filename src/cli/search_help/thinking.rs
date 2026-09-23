//! The `search --help` section for the THINKING family: the four `agent.thinking.*` leaves
//! and the two shapes that are markers rather than leaves of their own - the block salvaged
//! from a cancelled stream, and the `[Thinking removed]` text the client can push in place
//! of one - beside `agent.message.plain`, the assistant's ordinary say.
//!
//! The family's sentences land here and in no other string on the page, so extending them
//! disturbs no other family's text. Information parity with SKILL is the contract (AGENTS
//! section 1): every marker word, JSON key and render form the family ships is named here
//! too.
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
pub(crate) const SECTION: &str = "\n\nTHINKING LEAVES (agent.thinking, agent.message.plain)\n  \
     Two of the four thinking leaves carry no text at all and render an EMPTY body: \
     agent.thinking.omitted, the signed block whose `thinking` the API returned empty, \
     marked [no summary text]; and agent.thinking.redacted, the opaque ciphertext block, \
     marked [redacted thinking] (its `data` payload is printed on no path but --raw). \
     Neither is reachable by a pattern scan - there is nothing rendered to match - so \
     reach them with an empty pattern plus an exact -t, with --count-by label, or with a \
     `show` address.\n  \
     Two more markers name text the CLIENT wrote into the assistant's role. A thinking \
     block with text and an empty signature, on a record carrying the <synthetic> model \
     sentinel, is the block salvaged from a stream the person cancelled: it keeps \
     agent.thinking.summary and is marked [cancelled stream salvage]. An assistant text \
     block whose WHOLE trimmed text is [Thinking removed] is the stand-in the client \
     pushes when stripping thinking emptied the message: it keeps agent.message.plain and \
     is marked [thinking removed by the client], so those three words never read as the \
     assistant speaking. Both markers are display-only and ride search and show; \
     `verbatim` replays the record's own text unmarked.\n  \
     Two per-hit JSON keys come with them. `signature_len` is the character length of a \
     thinking block's own signature, and it is NULL when the block carries no signature \
     field at all - absent and empty are different shapes, and the salvage discriminator \
     rests on exactly that difference; a redacted_thinking block has no signature field, \
     so it carries no `signature_len`. `thinking_text_absent` is true on an \
     agent.thinking.omitted unit alone, so a consumer can tell an API that sent no \
     summary text from an excerpt that merely rendered empty.";
