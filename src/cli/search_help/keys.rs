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
     A record carries keys about the CALL it came from, and they belong to no leaf. They\n  \
     ride every hit as JSON, and the ones below that name a tag also render it in the label\n  \
     zone.\n    \
     `attribution` {agent, skill, plugin, mcp_server, mcp_tool}\n        \
     names what the request ran under. The label zone takes the SPECIFIC half only,\n        \
     `[via skill <name>]`, else `[via plugin <name>]`, else `[via mcp <server or tool>]`,\n        \
     because the agent name is on every record of a subagent lane.\n    \
     `request_id`, `api_block_index`\n        \
     say which API request and which block of its response this record is. They are\n        \
     join keys and ride JSON alone.\n    \
     `tool_ends_turn`\n        \
     renders `[ends turn]`.\n    \
     `user_feedback`\n        \
     renders `[feedback: <first 60 characters, on one line>]` and carries the whole text\n        \
     on the wire.\n    \
     `api_failure` {error, status, quota{limit_type, resets_at_utc, resets_at_local},\n                   \
     truncated_after_output, aborted_mid_stream}\n        \
     carries the API-failure set as one object and renders one tag, shown here on two\n        \
     lines.\n          \
     [api failure <kind> status <n> quota <window> resets <local instant>\n           \
     truncated after output aborted mid-stream]\n        \
     Each half appears only when the record names it, so a quota naming only its reset\n        \
     instant renders `quota resets <local instant>`. A record naming none of them\n        \
     carries no object at all, which leaves the tag no floor word. `resets_at_utc` and\n        \
     `resets_at_local` are the quota's reset instant, which the harness writes as epoch\n        \
     seconds, as the `_utc` / `_local` pair every other instant carries.\n    \
     `stop_reason`, `stop_details` {type, category, explanation, fallback_has_prefill_claim}\n        \
     ride a record whose `message.stop_details` names at least one member. They are the\n        \
     API's own account of why it stopped the response, which reads `refusal` on every\n        \
     record observed. They render `[api <type>: <category>]`, or `[api <type>]` with no\n        \
     category. The tag is display-only, because a pattern scan matches a unit's text and\n        \
     never its label zone, so the selector and the key are the way to these records.\n    \
     `pdf_pages` {first, last, count, path}\n        \
     rides the `agent.tool.result` of a Read that carried a `pages` parameter and\n        \
     renders `[pdf pages A-B of <basename>]`. It says which pages of which document the\n        \
     result holds, read off `toolUseResult.firstPage` plus the result's own content\n        \
     elements. The tag names the basename because it sits beside a leaf path on one\n        \
     line, and the key carries the whole path. `csift image` prints the same derivation\n        \
     per page, and the two surfaces agree by construction. The gate has two halves. The\n        \
     record's own result reads `type == \"parts\"` WITH a `firstPage`, AND the record\n        \
     carries exactly one `tool_result` block, since a `parts` result describes one tool\n        \
     call. The key is ABSENT on anything else (a screenshot, a build before the key\n        \
     existed, the REPL tool's re-emitted pages, a two-block record), never a range that\n        \
     starts at 1.\n    \
     `interrupted_message_id`, `hook_run_id`, `scheduled_task_id`, `scheduled_fire_id`\n        \
     are the explicit joins, JSON only. The unit's ONE `for` edge comes from the first\n        \
     two.\n          \
     [for: interrupted <id> (not in this file)]\n          \
     [for: hook_run <first 8> (not in this file)]\n          \
     [for: hook_run unknown (<reason>)]   where a Stop-hook summary names no run\n        \
     Neither id is a record uuid, so the `(not in this file)` form is what an ordinary\n        \
     transcript renders.\n\n  \
     ABSENCE is read at KEY level. A missing key means this record does not say, never\n  \
     no. A fact object that IS present names every member, with null for one the record\n  \
     lacks. A key of an UNEXPECTED JSON TYPE reads exactly as an absent one.\n  \
     `\"apiBlockIndex\":\"3\"` gives null, not 3, the record still emits with its text\n  \
     searchable, and nothing is booked malformed. csift reports what a record says and\n  \
     never coerces one type into another.\n\n  \
     ONE key is never rendered anywhere. A `type:\"system\"` `api_error` record's `error`\n  \
     object holds the failed request's whole HTTP response header set, so csift reads\n  \
     that key's string form only, and `--raw` is the one way to those bytes.";
