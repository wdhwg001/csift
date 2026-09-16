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
     Each `type:\"system\"` event kind renders its OWN fields, not a shared \
     `[<subtype> <level>]` head. `harness.system.refusal` reads `[refusal <subtype>] \
     originalModel=<v> fallbackModel=<v|none> apiRefusalCategory=<v>` in the harness's own \
     key words, with `fallbackModel=none` printed rather than elided because the absence \
     of a second model is what separates the two subtypes (with no source line the head \
     stops at the subtype rather than reporting a key it never read); \
     `harness.system.api-error` reads `[api-error status=<s> retry <attempt>/<max> in \
     <ms>ms]` followed by the error's own type and message (an attempt the line omits \
     prints as `?`, since a bare maximum would read as an attempt number); \
     `harness.system.notice` reads `[notice <level>] <content>`; `harness.system.kill` \
     reads the bare `[kill]` (the record carries no payload on disk); \
     `harness.system.command` reads `[command] <content>` and is the one LLM-visible \
     member - the request assembler re-mints it as a user message; \
     `harness.schedule.event` reads `[scheduled <when>] task=<id> cron=<expr> \
     prompt=<cut>`, the instant taken from the content's parenthesised tail. \
     A fallback-served assistant turn (its sole block is the API's `fallback` block) \
     renders under `agent.message.plain` with the label-zone marker \
     `[served by <to.model>, fell back from <from.model>]`. \
     Per-hit JSON keys, each present only when the line carried it: `retried`, \
     `original_model`, `fallback_model`, `category` (refusal); `status`, `retry_attempt`, \
     `max_retries`, `retry_in_ms`, `error` (type and message only) (api-error); `task_id`, \
     `cron`, `prompt` (cut at 200 chars) (schedule.event); `served_by`, `fell_back_from` \
     (the fallback turn). The fire record's instant is the engine's `scheduled_at` field, \
     which is written for the fired PROMPT and is null on the event hit - the head carries \
     it there. \
     PROVENANCE: a leaf that NAMES another record carries a `for` fact, rendered \
     `[for: <kind> L<n>]` when this transcript holds the named record, `[for: <kind> \
     <id8> (not in this file)]` when it does not, and `[for: <kind> unknown (<reason>)]` \
     when the record names none - the kinds are `refused` (a refusal's \
     `refusedUserMessageUuid`, basis key), `wraps` (a delivered command's `parentUuid`, \
     basis position - a run of local commands chains stdout to stdout, so the link names \
     the record above rather than the wrapper by construction), `armed_by` (a fire \
     record's `taskId`, always unknown: the arming call is joined by matching that id \
     inside a tool result, which a uuid map cannot do - the id itself is in the head and \
     in `task_id`) and `banner` (the fallback turn, always unknown: the banner names the \
     turn and not the reverse). JSON `for` is an ARRAY, one object per edge, each with \
     `kind`, `basis`, `target` (`line`, `uuid`, `session`, `note`) and `reason`. \
     ONE law is not this family's to relax: an `api_error` line carries the whole HTTP \
     response header set inside `error.headers`, and NO rendered or JSON path prints any \
     of it - `--raw` is the documented way to those bytes. That is also why this family \
     renders its named fields rather than the verbatim line the bookkeeping leaves use.";
