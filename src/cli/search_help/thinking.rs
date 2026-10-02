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
     Two of the four thinking leaves carry no text at all and render an EMPTY body.\n    \
     agent.thinking.omitted    the signed block whose `thinking` the API returned empty,\n                              \
     marked [no summary text]\n    \
     agent.thinking.redacted   the opaque ciphertext block, marked [redacted thinking].\n                              \
     Its `data` payload is printed on no path but --raw.\n  \
     A pattern scan reaches neither, because nothing is rendered to match. Reach them with\n  \
     an empty pattern plus an exact -t, with --count-by label, or with a `show` address.\n\n  \
     Two more markers name text the CLIENT wrote into the assistant's role.\n    \
     [cancelled stream salvage]         a thinking block with text and an empty signature,\n                                       \
     on a record carrying the <synthetic> model sentinel.\n                                       \
     It is the block salvaged from a stream the person\n                                       \
     cancelled, and it keeps agent.thinking.summary.\n    \
     [thinking removed by the client]   an assistant text block whose WHOLE trimmed text is\n                                       \
     [Thinking removed], the stand-in the client pushes\n                                       \
     when stripping thinking emptied the message. It\n                                       \
     keeps agent.message.plain, and the marker keeps those\n                                       \
     three words from reading as the assistant speaking.\n  \
     Both markers are display-only and ride search and show. `verbatim` replays the\n  \
     record's own text unmarked.\n\n  \
     Two per-hit JSON keys come with them.\n    \
     `signature_len`          the character length of a thinking block's own signature.\n                             \
     It is NULL when the block carries no signature field at all,\n                             \
     because absent and empty are different shapes, and an EMPTY\n                             \
     signature is what marks a [cancelled stream salvage] block. A\n                             \
     redacted_thinking block has no signature field, so it carries\n                             \
     no `signature_len`.\n    \
     `thinking_text_absent`   true on an agent.thinking.omitted unit alone, so a consumer\n                             \
     can tell an API that sent no summary text from an excerpt that\n                             \
     merely rendered empty.";
