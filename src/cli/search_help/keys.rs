//! The `search --help` section for the KEYS family: the EXPOSED KEYS - facts a record
//! carries about the call behind it (which agent, skill, plugin or MCP tool it ran under,
//! which request and block it came from, why it failed, what a person typed refusing it),
//! which belong to no single leaf and are therefore a section rather than a taxonomy row.
//!
//! Information parity with SKILL is the contract (AGENTS section 1): every marker word,
//! JSON key and render form the family ships is named here too - the five label-zone tags
//! with their variable halves, the eleven JSON keys, the `for` edge's three forms, and the
//! one shape whose contents reach no path but `--raw`.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and needs
//! no trailing one. The SHAPE, which every family writes alike: `"\n\n"`, the head line
//! at column ZERO (the way every other head on the page sits), `"\n  "`, then the body
//! at indent two - and it ends on the body's last word, because the next region
//! separates itself. The regions come AFTER the JSON schema block and its closing
//! notes: a head indented under that block would read as a subsection of the JSON
//! schema rather than as a section of the page.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str =
    "\n\nEXPOSED KEYS (facts a record states about the call behind it)\n  \
     A record carries keys about the CALL it came from, and they belong to no leaf - so \
     they ride every hit as JSON, and the ones below that name a tag also render it in the \
     label zone. \
     `attribution` {agent, skill, plugin, mcp_server, mcp_tool} names what the request ran \
     under, and the label zone takes the SPECIFIC half only - `[via skill <name>]`, else \
     `[via plugin <name>]`, else `[via mcp <server or tool>]` - because the agent name is \
     on every record of a subagent lane. `request_id` and `api_block_index` say which API \
     request and which block of its response this record is; they are join keys and ride \
     JSON alone. `tool_ends_turn` renders `[ends turn]`. `user_feedback` renders \
     `[feedback: <first 60 characters, on one line>]` and carries the whole text on the wire. The \
     API-failure set rides one `api_failure` object {error, status, quota{limit_type, \
     resets_at_utc, resets_at_local}, truncated_after_output, aborted_mid_stream} \
     and renders `[api failure <kind> status <n> quota <window> resets <local instant> \
     truncated after output aborted mid-stream]`, each half only when the record names it - \
     a quota naming only its reset instant renders `quota resets <local instant>` - and a \
     record naming none of them carries no object at all, so the tag can name no floor \
     word. `resets_at_utc` and `resets_at_local` are the quota's reset instant, which the \
     harness writes as epoch seconds, as the `_utc` / `_local` pair every other instant \
     carries. \
     `stop_reason` and `stop_details` {type, category, explanation, \
     fallback_has_prefill_claim} ride a record whose `message.stop_details` names at least \
     one member - the API's own account of why it stopped the response, which reads \
     `refusal` on every record observed - and render `[api <type>: <category>]`, or \
     `[api <type>]` with no category. The tag is display-only - a pattern scan matches a \
     unit's text, never its label zone - so the selector and the key are the way to these \
     records. \
     `pdf_pages` {first, last, count, path} rides the `agent.tool.result` of a Read that \
     carried a `pages` parameter and renders `[pdf pages A-B of <basename>]`: which pages \
     of which document the result holds, read off `toolUseResult.firstPage` plus the \
     result's own content elements. The tag names the basename because it sits beside a \
     leaf path on one line, and the key carries the whole path; `csift image` prints the \
     same derivation per page, so the two surfaces agree by construction. The gate is both \
     halves of the record's own result (`type == \"parts\"` WITH a `firstPage`) AND the \
     record carrying exactly one `tool_result` block, since a `parts` result describes one \
     tool call. It is ABSENT on anything else - a screenshot, a build before the key \
     existed, the REPL tool's re-emitted pages, a two-block record - rather than a range \
     that starts at 1. \
     `interrupted_message_id`, `hook_run_id`, `scheduled_task_id` and `scheduled_fire_id` \
     are the explicit joins, JSON only. The unit's ONE `for` edge comes from the first two: \
     `[for: interrupted <id> (not in this file)]`, `[for: hook_run <first 8> (not in this \
     file)]`, and `[for: hook_run unknown (<reason>)]` where a Stop-hook summary names no \
     run - neither id is a record uuid, so the middle form is what an ordinary transcript \
     renders. ABSENCE is read at KEY level: a missing key means this record does not say, \
     never no, and a fact object that IS present names every member with null for one the \
     record lacks, and a key of an UNEXPECTED JSON TYPE reads exactly as an absent one: \
     `\"apiBlockIndex\":\"3\"` gives null rather than 3, the record still emits with its text \
     searchable, and nothing is booked malformed - csift reports what a record says and \
     never coerces one type into another. ONE key is never rendered anywhere: a \
     `type:\"system\"` `api_error` \
     record's `error` object holds the failed request's whole HTTP response header set, so \
     csift reads that key's string form only and `--raw` is the one way to those bytes.";
