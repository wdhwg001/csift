//! The `search --help` section for the ATTACHMENTS family: the `harness.context` leaves,
//! `harness.hook.{context,outcome}`, `agent.message.structured` and the person's mid-turn
//! `queued_command` - every leaf a `type:"attachment"` payload decides, the two bodies such
//! a line can render (the text the model received, or the payload JSON itself), and the
//! provenance facts every one of them carries.
//!
//! Information parity with SKILL is the contract (AGENTS section 1): every marker word,
//! JSON key and render form the family ships is named here too. This file is the family's
//! ONLY string on the page, so extending it disturbs no other family's sentences.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and closes
//! without one: `"\n\n"`, the head line at column zero, `"\n  "`, the body. That is the
//! shape every head on this page has, and the region sits after the JSON schema block and
//! its closing notes rather than inside them.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str =
    "\n\nATTACHMENT LEAVES (harness.hook, harness.context, agent.message.structured)\n  \
     An attachment line keeps TWO copies of itself, and the unit says which one it shows.\n    \
     [rendered]   the harness converter's own output, the text the model received and what\n                 \
     a resume replays\n    \
     [payload]    the line's own payload, either a field of it (a hook run's stdout, a\n                 \
     hook's injected strings) or the verbatim payload JSON\n  \
     The word rides JSON as `body_source`. A `queued_command` can carry a SECOND conversion\n  \
     for a human turn. Which copy the model got depends on the turn it was replayed into,\n  \
     and the record does not say which. The body stays the ordinary one, and the unit adds\n  \
     `[human-turn render differs]` (JSON `human_turn_render_differs`) and chooses neither.\n\n  \
     Each unit also names ITSELF.\n    \
     [hook <event> <name>]                     injected context\n    \
     [hook <event> <name> exit <code> <ms>ms]  a hook run\n    \
     [hook <payload type>]                     the other six outcome types,\n                                               \
     hook_blocking_error, hook_non_blocking_error,\n                                               \
     hook_cancelled, hook_stopped_continuation,\n                                               \
     hook_system_message, async_hook_response_batch\n    \
     [context <payload type>]                  the context leaves\n    \
     [structured]                              a schema-bound subagent's own answer\n    \
     [mid-turn]                                the person's steer\n  \
     The steer is a `user.message` that reached the model in the MIDDLE of a turn and opened\n  \
     none. It takes the same body rule as every other view, so a steer whose record carries\n  \
     a conversion reads `[rendered]`.\n\n  \
     PROVENANCE. Every attachment unit carries three facts. Two are `for` edges in the\n  \
     shared form, `attached_to` from the record's own `parentUuid` and `for_tool_use` from\n  \
     the payload's inner `toolUseID`.\n    \
     [for: <kind> L<line>]                   the target is a line of this transcript\n    \
     [for: <kind> <id> (not in this file)]   no line here carries the id\n    \
     [for: <kind> unknown (<reason>)]        the line names nothing at all\n  \
     A tool-call id always takes the middle form, because it is a wire id and never a record\n  \
     uuid. It prints WHOLE, since only a uuid-shaped target is cut to its eight-character\n  \
     handle. Both edges fold into the ONE JSON `for` array, each entry\n  \
     `{kind,basis,target,reason}`, with the WHOLE uuid under `target.uuid` (what\n  \
     `show --uuid` takes) beside `target.line`.\n  \
     The third fact is `[produced_by <command>]`, from a hook run's own `command`, with JSON\n  \
     `produced_by:{command,hook_name,payload_type}`. A command is not a record, so it keeps a\n  \
     key of its own and renders whole, never cut to an id handle. Its two empties differ.\n    \
     `no command on this line; hookName=<name>`   a hook line that records no command\n    \
     `no producer key; type=<t>`                  a payload type that is no hook's output\n  \
     The three facts ride every attachment unit, INCLUDING an addressed record csift models\n  \
     no leaf for. The shape stands in for the leaf path there, and the three render after\n  \
     it, exactly as they do on a labelled unit.\n\n  \
     MATCHING follows the BODY. A record that carries a conversion is searchable by the\n  \
     converted text, and the payload text it replaced no longer matches. `--raw` does not\n  \
     recover the difference, because the raw backfill runs after the match phase. The way\n  \
     to the bytes is an address, `csift show @<id> --line N --raw`.\n  \
     The decoded body is searchable by an ordinary pattern. Before it parses a line, csift\n  \
     checks the raw bytes for a run the pattern requires (its needle), and a needle\n  \
     carries no whitespace, no double quote, no backslash, no control character and no DEL.\n  \
     Such a run sits verbatim inside the raw line's JSON string, so the quick byte check\n  \
     never skips a line the rendered body would match. The backslash is on that list\n  \
     because a line stores it doubled, so a needle splits there. `C:\\Users\\dev` derives\n  \
     `Users`, which is in the bytes, and the pattern matches.";
