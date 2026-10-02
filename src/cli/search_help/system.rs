//! The `search --help` section for the SYSTEM family: the five `harness.system.*` event
//! kinds, `harness.schedule.event` and the three `harness.turn.*` closers - every leaf a
//! `type:"system"` subtype decides, plus the one family that stays OUTSIDE the
//! matchable-text law, because an `api_error` line carries the HTTP response headers.
//!
//! Information parity with SKILL is the contract (AGENTS section 1): every marker word,
//! JSON key and render form the family ships is named here too.
//!
//! The concatenation is byte-joined, so a section brings its OWN separator and needs
//! no trailing one. The SHAPE, which every family writes alike: `"\n\n"`, the head line
//! at column ZERO (the way every other head on the page sits), `"\n  "`, then the body
//! at indent two - and it ends on the body's last word, because the next region
//! separates itself. The regions come AFTER the JSON schema block and its closing
//! notes: a head indented under that block would read as a subsection of the JSON
//! schema rather than as a section of the page.

/// This family's `search --help` section. See the module doc.
pub(crate) const SECTION: &str = "\n\nSYSTEM LEAVES (harness.system, harness.schedule.event)\n  \
     Each `type:\"system\"` event kind renders its OWN fields, with no shared\n  \
     `[<subtype> <level>]` head.\n    \
     harness.system.refusal\n      \
     [refusal <subtype>] originalModel=<v> fallbackModel=<v|none> apiRefusalCategory=<v>\n      \
     The keys are the harness's own words. `fallbackModel=none` is printed, never elided,\n      \
     because the absence of a second model is what separates the two subtypes. With no\n      \
     source line the head stops at the subtype, since it reports no key it never read.\n    \
     harness.system.api-error\n      \
     [api-error status=<s> type=<kind> retry <attempt>/<max> in <ms>ms], then the\n      \
     failure's own wording\n      \
     An attempt the line omits prints as `?`, since a bare maximum would read as an\n      \
     attempt number. The kind and the wording are read where the shape puts them, and\n      \
     that differs by shape. A connection failure carries its wording in `error.message`\n      \
     and names no kind. An HTTP failure names the kind in `error.type` and keeps the\n      \
     response body it received, so its wording sits two hops down the `error` chain.\n    \
     harness.system.notice\n      \
     [notice <level>] <content>\n    \
     harness.system.kill\n      \
     [kill], bare, because the record carries no payload on disk\n    \
     harness.system.command\n      \
     [command] <content>. This is the one LLM-visible member. The request assembler\n      \
     re-mints it as a user message.\n    \
     harness.schedule.event\n      \
     [scheduled <when>] task=<id> cron=<expr> prompt=<cut>, with the instant taken\n      \
     from the content's parenthesised tail\n\n  \
     A fallback-served assistant turn (its sole block is the API's `fallback` block)\n  \
     renders under `agent.message.plain`, on a scan as under an address. It carries the\n  \
     label-zone marker `[served by <to.model>, fell back from <from.model>]`, and the\n  \
     marker's own words are its body. A model id or `fell back` therefore finds it.\n\n  \
     The per-hit JSON keys, each present only when the line carried it.\n    \
     refusal          `retried`, `original_model`, `fallback_model`, `category`\n    \
     api-error        `status`, `retry_attempt`, `max_retries`, `retry_in_ms`, `error`\n                     \
     (type and message only)\n    \
     schedule.event   `task_id`, `cron`, `prompt` (cut at 200 chars)\n    \
     fallback turn    `served_by`, `fell_back_from`\n  \
     The instant rides the JSON `scheduled_at` field on BOTH halves of the pair, and it\n  \
     means one thing on each, which is when the schedule fired. The fired PROMPT reads it\n  \
     off its event sibling, and the EVENT reads its own parenthesised tail. The head\n  \
     carries it too, because every key these leaves expose is also in the rendered text.\n\n  \
     PROVENANCE. A leaf that NAMES another record carries a `for` fact.\n    \
     [for: <kind> L<n>]                      this transcript holds the named record\n    \
     [for: <kind> <id> (not in this file)]   it does not. A uuid is cut to its\n                                            \
     eight-character handle, and any other id prints whole.\n    \
     [for: <kind> unknown (<reason>)]        the record names none\n  \
     The kinds are these.\n    \
     refused    a refusal's `refusedUserMessageUuid`, basis key\n    \
     wraps      a delivered command's `parentUuid`, basis position. A run of local commands\n               \
     chains stdout to stdout, so by construction the link names the record above,\n               \
     and that need not be the wrapper.\n    \
     armed_by   a fire record's `taskId`, always unknown. The arming call is joined by\n               \
     matching that id inside a tool result, which a lookup by record uuid\n               \
     cannot do. The id itself is in the head and in `task_id`.\n    \
     banner     the fallback turn, always unknown, because the banner names the turn and\n               \
     not the reverse\n  \
     JSON `for` is an ARRAY, one object per edge, each with `kind`, `basis`, `target`\n  \
     (`line`, `uuid`, `session`, `note`) and `reason`.\n\n  \
     An `api_error` line carries the whole HTTP response header set inside\n  \
     `error.headers`, and NO rendered or JSON path prints any of it. `--raw` is the\n  \
     documented way to those bytes. That is also why these leaves render their named\n  \
     fields, where the bookkeeping leaves render the verbatim line.";
