//! The `search --help` section for the ATTACHMENTS family: the `harness.context` leaves,
//! `harness.hook.{context,outcome}`, `agent.message.structured` and the person's mid-turn
//! `queued_command` - every leaf a `type:"attachment"` payload decides, the two bodies such
//! a line can render (the text the model received, or the payload JSON itself), and the
//! provenance facts every one of them carries.
//!
//! Information parity with SKILL is the contract (AGENTS section 1): every marker word,
//! JSON key and render form the family ships is named here too, in the same commit series
//! as the surface it documents, without any lane touching a string another lane is editing.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and closes
//! without one: `"\n\n"`, the head line at column zero, `"\n  "`, the body. That is the
//! shape every head on this page has, and the region sits after the JSON schema block and
//! its closing notes rather than inside them.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str =
    "\n\nATTACHMENT LEAVES (harness.hook, harness.context, agent.message.structured)\n  \
     An attachment line keeps TWO copies of itself and the unit says which one it is \
     showing. `[rendered]` is the harness converter's own output - the text the model \
     received, and what a resume replays; `[payload]` is the line's own payload, either a \
     field of it (a hook run's stdout, a hook's injected strings) or the verbatim payload \
     JSON. The word rides JSON as `body_source`. A `queued_command` can carry a SECOND \
     conversion for a human turn: which copy the model got depends on the turn it was \
     replayed into, which the record does not say, so the body stays the ordinary one and \
     the unit adds `[human-turn render differs]` (JSON `human_turn_render_differs`) rather \
     than choosing. ONE view is rendered by another path and reads `[payload]` whatever \
     the record carries - the person's mid-turn steer, whose `user.message` unit resolves \
     as an opener before this family is asked.\n  \
     Each unit also names ITSELF: `[hook <event> <name>]` for injected context, `[hook \
     <event> <name> exit <code> <ms>ms]` for a hook run (`[hook <payload type>]` for the \
     other six outcome types), `[context <payload type>]` for the context leaves, \
     `[structured]` for a schema-bound subagent's own answer, and `[mid-turn]` for the \
     person's steer, which is a `user.message` that reached the model in the MIDDLE of a \
     turn rather than opening one - and which takes the same body rule as every other \
     view, so a steer whose record carries a conversion reads `[rendered]`.\n  \
     PROVENANCE, three facts on every attachment unit. Two are `for` edges in the shared \
     form: `attached_to` from the record's own `parentUuid` and `for_tool_use` from the \
     payload's inner `toolUseID`, rendering `[for: <kind> L<line>]` when the target is a \
     line of this transcript, `[for: <kind> <id> (not in this file)]` for an id no line \
     here carries (which a tool-call id always is - it is a wire id, never a record \
     uuid, so it prints WHOLE: only a uuid-shaped target is cut to its eight-character \
     handle), and `[for: <kind> unknown (<reason>)]` when the line names nothing at all. \
     Both fold into the ONE JSON `for` array, each entry `{kind,basis,target,reason}` with \
     the WHOLE uuid under `target.uuid` (what `show --uuid` takes) beside `target.line`. \
     The third is `[produced_by <command>]` from a hook run's own `command`, JSON \
     `produced_by:{command,hook_name,payload_type}` - a command is not a record, so it \
     keeps a key of its own and is rendered whole rather than cut to an id handle. Its \
     two empties differ: `no command on this line; hookName=<name>` for a hook line that \
     records no command, `no producer key; type=<t>` for a payload type that is no hook's \
     output. The three facts ride every attachment unit INCLUDING an addressed record \
     csift models no leaf for: the shape stands in for the leaf path there and the three \
     render after it, exactly as they do on a labelled unit.\n  \
     MATCHING follows the BODY. A record that carries a conversion is searchable by the \
     converted text, not by the payload text it replaced, and `--raw` does not recover the \
     difference (the raw backfill runs after the match phase); the way to the bytes is an \
     address - `csift show @<id> --line N --raw`. The decoded body is searchable by an \
     ordinary pattern: a needle the prefilter can use carries no whitespace, no double \
     quote, no backslash, no control character and no DEL, and such a run sits verbatim \
     inside the raw line's JSON string, so the line prefilter and the whole-file gate see \
     what the render does. The backslash is on that list because a line stores it \
     doubled, so a needle splits there - `C:\\Users\\dev` derives `Users`, which is in \
     the bytes, and the pattern matches.";
