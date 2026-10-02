# Labels: every leaf, its gate, what reaches the model, and every tag a unit prints

Read this before choosing a `-t`/`-T` leaf you have not used, before reading a bracketed tag in a hit, and before claiming a census covers every kind of record. For one leaf or tag, search this file for it and read that row.

## Contents

- How a selector narrows
- Gates: what a scan reads at all
- The 73 leaves
- Tags any leaf can carry
- How a unit's text is built
- The 1:1 and emission rules
- Retired spellings

The on-disk shape of each leaf, and how to grab its raw bytes, is references/shapes.md.

## How a selector narrows

A selector matches a label when it is a dot-segment prefix of it (`agent.tool` matches `agent.tool.use` and `agent.tool.result`). The effective set is (the `-t` selectors, or every leaf) minus the `-T` selectors, so `-t user -T user.message` is set subtraction (the visible remainder is `user.answer` and `user.rejection`), and only a `-T` that cancels every `-t` is an error. A record carrying several leaves emits once, under its richest view that your filter leaves alive (a SendMessage call emits under `agent.communication.sent`, not `agent.tool.use`), and its `labels` array is not ordered by that choice (measured: `labels` read `["agent.tool.use","agent.communication.sent"]` on such a hit); a record with several SECTIONS (a `<task-notification>` pulse carrying a result, a batched notification) emits one unit per section and still counts once in a census (docs: `csift search --help`).

A bare role or category narrows on three independent axes; a glob (`-t 'user.*'`) or an exact leaf ignores all three.

| axis | decided per | a record is dropped from the bare face when |
|---|---|---|
| leaf visibility | leaf | its leaf is one the model does not receive (the "model receives" column below) |
| delivery | record | Claude Code's request assembler drops it though its leaf is visible: an `isVirtual` record, the `<synthetic>` API-error placeholder, an attachment whose payload type the converter drops; a `system`/`local_command` record and a `hook_success` carrying a top-level `rendered` are sent though their leaf defaults say otherwise. Every hit carries JSON `delivered`, false whether this axis or the leaf's own default withheld the record (the `[not delivered]` tag, below) |
| survival | record | the conversation chain no longer reaches it (`abandoned`); a `pre-cut` record is never dropped (references/survival.md) |

One leaf is exempt from the delivery axis: `agent.message.structured` rides a converter-dropped `structured_output` attachment, yet `-t agent` and `-t agent.message` keep it, because it is the answer the model wrote and a turn produces either free text or a structured answer, never both; its `delivered` stays false and it still prints `[not delivered]` (docs: `csift search --help`). A bare selector whose whole face is invisible (`-t harness.bookkeeping`, `-t harness.turn`) is accepted, matches nothing, and prints `csift: -t harness.turn selects no LLM-visible leaf; use harness.turn.* or a leaf` on stderr, with JSON `empty_visible_face` (measured: `csift search '' TARGET -t harness.turn`). A misspelt selector exits 2 with `error: invalid value 'foo' for '--label <SELECTOR>': unknown label selector 'foo'.` followed by every valid selector (measured: `csift search x TARGET -t foo`).

## Gates: what a scan reads at all

The gate words are `csift search --help`'s own. A leaf's gate decides whether a scan without `-t` reads its lines at all; everything a gate keeps out is invisible to a census and to an absence claim.

| gate | the scan reads the line when |
|---|---|
| `scan` | always: a role-bearing line |
| `scan/needle` | always, through the leaf's own value needle: a csift-channel delivery, the person's mid-turn steer, the model's structured answer. Only a `-T` excluding the leaf turns it off |
| `-t` | a `-t` selector reaches the leaf: the leaf or a glob over it always; its bare category or role only for a leaf the model receives (`harness.system.command`), never for the others, so `-t harness.bookkeeping` reads 0 records and prints `-t harness.bookkeeping selects no LLM-visible leaf; use harness.bookkeeping.* or a leaf` (measured on one session: `-t 'harness.bookkeeping.*'` read 17,236) |
| `-t/att` | a `-t` selector reaches it, or `--attachments` is given, which admits every attachment line |
| `address` | never on a scan; `csift show --line N` renders it |

A zero-match search lists on stderr the 23 `-t` leaves and the 9 `-t/att` leaves it did not read (measured: `csift search zzqq TARGET`). A census without `-t` shows only the leaves that scan reads: on one session, 26 keys flagless against 48 under `-t 'user.*' -t 'agent.*' -t 'harness.*'` (measured: `csift search '' TARGET --no-subagents --count-by label` with and without those globs). `--count-by attachment` implies `--attachments`. An explicit `show` address renders any line whatever its gate.

## The 73 leaves

Columns. "gate": as above. "model receives": the leaf's default before the per-record delivery axis (measured: `csift search '' -t user -t agent -t harness --count-by label` against the same with `-t 'user.*' -t 'agent.*' -t 'harness.*'` over one projects root, a leaf the first never counts reading `no`; `synthetic` marks one read instead off `delivered` in `csift --claude-home FIXTURE search '' @<uuid> -t 'agent.*' -t 'harness.*' --format json` on a synthetic transcript). "person sees": whether Claude Code shows the line on the person's screen, `verbose` meaning its verbose view only and `indirect` meaning the person sees an effect of the line but not the line itself (UNTESTED: csift's shape inventory); `?` means csift has not established it. Bracketed text in the "what" column is a HEAD that opens the unit's text after `L<n>`, which a pattern matches, unless marked (tag): a tag prints before `L<n>` in the label zone and never matches (measured: `csift search '' -t LEAF` text hits over one projects root, each line split at its ` L<n> `, the count of hits carrying the tag given where the row gives one; `csift search 'kill\]' -t harness.system.kill -c` printed 15; UNTESTED on a tag no hit carried).

| leaf | what it is, and what its unit prints | gate | model receives | person sees |
|---|---|---|---|---|
| `user.message` | the person's own message; a slash command with arguments renders `/name args` and opens no turn (references/ranges.md, Turns). A message the harness relayed into a running lane (an isMeta record whose `origin.kind` is `human`) takes this leaf with `[relayed]` (tag; JSON `relayed`) and opens no turn. The person's steer typed mid-turn (a `queued_command` attachment whose `origin.kind` is `human`) takes it with `[mid-turn]` (tag: 174 hits) | scan (the steer: scan/needle) | yes | yes; a relayed one no |
| `user.answer` | an answered AskUserQuestion as one unit: `[AskUserQuestion · 1 question]` (`· 2 questions` and so on), each question's header, question, options with their notes, the answer and any `annotations` notes; an answer written by another answerer shows a `response:` line (UNTESTED: no specimen), and a carrier with no `toolUseResult` shows the bare `[AskUserQuestion]`. Only the `AskUserQuestion` call's own result is an answer (references/shapes.md, `user.answer`) | scan | yes | ? |
| `user.rejection` | a tool call the person refused with typed text (the text after `To tell you how to proceed, the user said:`); ends `[plan: <path>]` where the refused call resolves to a plan file | scan | yes | ? |
| `user.unsent` | an esc-recalled draft: an opener a later opener replaced before anything answered it. Outside turn numbering; the turn header above the hit carries a `↳` line, `↳ differs from the sent message in N chars (P% of the final): may carry an addition or a correction`, `↳ differs from the sent message in more than N chars (more than P% of the final)` where the comparison stopped at its bound, or `↳ identical to the sent message`, and nothing when no later message replaced it (measured over one projects root, `csift search '' -t user.unsent -t user.rewound`: 976 `differs` lines, 17 `more than`, 27 `identical`) | scan | no | ? |
| `user.rewound` | an opener that was answered, then rewound past; its `↳` line under the turn header opens `rewound: the conversation continued from L<n> instead - ` before the distance | scan | no | ? |
| `user.queued` | the person's text while it sat in the input queue, verbatim; a tag names the queue event `[enqueue]`, `[popAll]`, `[remove · <reason>]`, a bare `[remove]` when the line gives no reason, or `[queued]` when the line names no event (tag: over one projects root 1,947, 121, 69 and 84 hits of the first four; JSON `queue_operation`, `queue_reason`) | -t | no | yes |
| `agent.message.plain` | the assistant's text. A turn served by a fallback model prints `[served by <model>, fell back from <model>]` (tag; JSON `served_by`, `fell_back_from`), on a scan too, and a record whose only block is that fallback block has no other text, so the same words are its text too and a pattern matches them there (measured over one projects root: `csift search 'fell back from' -t agent.message.plain` matched 45 such records, their text opening `served by <model>, fell back from <model>`); a block whose whole text is `[Thinking removed]` prints `[thinking removed by the client]` (tag, UNTESTED: no hit on the measured root) | scan | yes | ? |
| `agent.message.structured` | a schema-bound subagent's final structured answer: `[structured]` (tag) over the payload JSON | scan/needle | yes by its leaf, and `-t agent` keeps it, yet every record reads `delivered:false`, because its attachment is one the converter drops (measured over one projects root: 1,813 records under `-t agent` and under the exact leaf, all `delivered:false`) | no |
| `agent.thinking.summary` | the API's summary of the model's reasoning (never the raw reasoning); a block salvaged from a stream the person cancelled prints `[cancelled stream salvage]` (tag: 49 hits) | scan | yes | verbose |
| `agent.thinking.narration` | the API's one-line progress update between tool calls, `[narration summary]` (tag); `-t agent.thinking -T agent.thinking.narration` keeps the reasoning summaries alone | scan | yes | ? |
| `agent.thinking.omitted` | a thinking block returned with no text: empty body, `[no summary text]` (tag), so no pattern reaches it; use `''` with this exact leaf, a census, or an address | scan | no | ? |
| `agent.thinking.redacted` | the API's redacted ciphertext block: empty body, `[redacted thinking]` (tag, UNTESTED); its `data` prints only under `--raw` | scan | no (synthetic) | verbose |
| `agent.tool.use` | a tool call, joined to its result by `▹`, or followed by `(no result — pending)` when no result answered it (JSON `pairing` `pending`); also a pending AskUserQuestion, ExitPlanMode or MCP elicitation merged from the elicitation sidecar (references/live.md) | scan | yes | yes |
| `agent.tool.result` | a tool's result; `[error]` (tag) when it errored (JSON `is_error`), `[denied: <kind>]` (tag) when the call did not run, the kind being `permission-rule` (a permission rule, or a PreToolUse hook whose refusal reads `PreToolUse:<Tool> hook error`: 15 of 15 such results over one projects root), `user-rejected` (the person refused it), `interrupted` or `cancelled` (JSON `denial_kind`; measured over one projects root: 615 tagged hits, 532, 78, 4 and 1 by kind) | scan | yes | yes |
| `agent.communication.inbox` | a message received from another lane, rendered `<from> ⇨ <to>`: a peer or teammate message, a subagent's spawn prompt, a subagent's return, a coordinator relay (`coordinator ⇨ self`), or `? ⇨ self` when the record names no sender. A peer relay prints `[peer lane <id>]` (tag: 69 hits; JSON `peer_sender_task_id`). | scan | yes | no for a coordinator relay, else yes |
| `agent.communication.sent` | a message sent with `SendMessage`, or a spawn | scan | yes | ? |
| `agent.communication.signal` | a control message (`idle_notification`, `shutdown_*`) | scan | yes | ? |
| `agent.communication.channel` | a csift-channel delivery, verbatim from its header line, which opens `[csift-channel v1 id=<16 hex>` (references/channel.md) | scan/needle | yes (synthetic) | ? |
| `harness.notification.workflow` | a `<task-notification>` for a workflow run | scan | yes | ? |
| `harness.notification.monitor` | a Monitor or scheduled-task pulse; its status slot prints the pulse's event | scan | yes | ? |
| `harness.notification.subagent` | a subagent's completion pulse; the head prints the harness's word `agent`, as in `[agent <task-id> completed]`. The plain-text notice that background agents were stopped prints `[subagent stopped] <notice>` as the head of its text (14 hits, none in the label zone) | scan | yes | ? |
| `harness.notification.background-command` | a background shell command's pulse | scan | yes | ? |
| `harness.notification.task` | any other `<task-notification>` | scan | yes | ? |
| `harness.compaction.summary` | the compaction summary; a `/rewind` summarize prints `[summarize up_to]` (tag: 2 hits) or `[summarize from]` (tag, UNTESTED: no hit) | scan | yes | ? |
| `harness.compaction.boundary` | the compaction's metrics record, its text opening `Conversation compacted` and then `[compaction boundary: trigger=<t> preTokens=<n> postTokens=<n> durationMs=<n> cumulativeDroppedTokens=<n> preserved=<n> uuids, <m> allUuids, anchor <8 hex> segment=<8 hex>..<8 hex>]` (present fields only; a summarize adds `messagesSummarized=<n>`) and `[logicalParent=<uuid>]` | scan | no | yes |
| `harness.command.invocation` | a slash command's wrapper record; a wrapper with arguments carries `user.message` first | scan | yes | ? |
| `harness.command.stdout` | a local command's output record | scan | yes | ? |
| `harness.command.caveat` | the caveat the slash-command executor writes after a local command | scan | yes | no |
| `harness.interrupt.user` | `[Request interrupted by user]` | scan | yes | ? |
| `harness.interrupt.tool` | `[Request interrupted by user for tool use]` | scan | yes | ? |
| `harness.schedule.wakeup` | a fired timer's wakeup prompt, recognised by its marker; one whose parent is a fire record also carries `scheduled_at` and a `fires` edge (measured: the 1 wakeup hit on one projects root); the `ScheduleWakeup` call that armed it is an ordinary `agent.tool.use` | scan | yes | no |
| `harness.schedule.event` | the scheduler's own fire record: `[scheduled <when>] task=<id> cron=<expr> prompt=<first 200 chars>` when the record carries a `taskId` (JSON `task_id`, `cron`, `prompt`, `scheduled_at`), which every cron task and some `/loop` wakeups do; a fire record with none prints `[scheduled <when>] Claude resuming /loop wakeup (<when>)`, those three JSON keys null (measured over one projects root: 8 of 467 event hits carried a task, one of them a `/loop` wakeup) | -t | no | yes |
| `harness.schedule.fire` | the prompt a scheduled task submits, recognised by its parent link to the fire record: `[scheduled fire <when>]` (tag; JSON `scheduled_at`) | scan | yes | no |
| `harness.schedule.tick` | an autonomous-loop driver tick; one whose parent is a fire record also carries `scheduled_at` and a `fires` edge (measured: the 2 tick hits on one projects root) | scan | yes | no |
| `harness.resume.prompt` | the repair prompt `Continue from where you left off.` the loader appends when a resumed transcript ends on a dangling user record; neither the person nor the model wrote it or the placeholder below, yet the model receives both. A person's own message opening with that sentence has no authorship flag and stays `user.message` | scan | yes | no |
| `harness.resume.placeholder` | the stand-in reply spliced after that record: `[paired]` or `[unpaired]` (tag; JSON `resume_paired`) | scan | yes | ? |
| `harness.hook.context` | context a hook injected: `[hook <event> <name>]` (tag) | -t/att | yes | no |
| `harness.hook.outcome` | a hook run's own record: `[hook <event> <name> exit <code> <ms>ms]`, or `[hook <payload type>]` for the six non-success types (tag) | -t/att | only a `hook_success` carrying a rendered body | verbose |
| `harness.hook.feedback` | a blocking hook's feedback to the model, for the hooks `Stop`, `TeammateIdle`, `TaskCreated`, `TaskCompleted` | scan | yes | no |
| `harness.context.reminder` | a reminder the harness re-injects: reminder attachments (`[context <payload type>]`, tag), and injected `<system-reminder>` records no narrower template matches | -t/att for the attachments, scan for the injections | yes, except `batching_reminder_sent` | ? |
| `harness.context.listing` | injected inventories: skills, agents, deferred tools, MCP server instructions | -t/att | yes, except `deferred_tools_record` | ? |
| `harness.context.prompt` | a persisted system-prompt section: instructions, date, environment, model, session context | -t/att | yes, except `prompt_snapshot` | ? |
| `harness.context.file` | file content the harness attached: a read, an edited file's snapshot, a compact file reference, nested memory, a plan file | -t/att | yes | ? |
| `harness.context.mode` | a mode transition: plan mode entry, exit and re-entry, auto mode, command permissions | -t/att | yes, except `command_permissions` | ? |
| `harness.context.ide` | IDE state: the open file, the selection, diagnostics | -t/att | yes | ? |
| `harness.context.task` | task, goal and memory state, and a queued command that is not the person's own text | -t/att | yes, except `goal_status`, `thinking_stripped`, `thinking_drop` | ? |
| `harness.context.image` | an image the harness referenced into the conversation: an `[Image: ...]` reference record, or an isMeta record holding only image blocks, which prints `[context image-only: N image block(s), M document block(s)]` (placement UNTESTED; JSON `image_only`) and, having no text, is reached only by an address or `csift image` | scan | yes | no |
| `harness.context.skill` | a skill's instruction body injected into a transcript, top-level or subagent | scan | yes | no |
| `harness.enforce.structured-output` | the nudge telling the model to call its StructuredOutput tool | scan | yes | no |
| `harness.enforce.brief` | the nudge telling the model it ended a turn without calling SendUserMessage | scan | yes (synthetic) | no |
| `harness.enforce.handback` | the nudge telling a subagent to hand its result back | scan | yes | no |
| `harness.enforce.retry` | the harness continuing a turn the model did not finish: malformed-call redo nudges, a classifier stop, the output-token-limit resume, a mid-stream cut-off, and the notice that an Edit failed because the file changed | scan | yes | no |
| `harness.turn.duration` | end-of-turn telemetry: `[turn duration: <h m s> · durationMs=<n> messageCount=<n> ...]`, adding `pendingBackgroundAgentCount` and `pendingWorkflowCount` when present | -t | no | ? |
| `harness.turn.stop-hooks` | the Stop hooks that ran at turn end: `[stop hooks: count=<n> errors=<n> prevented=<bool>]`, then one `<command> (<n>ms)` line per hook | -t | no | ? |
| `harness.turn.recap` | the recap generated when the person returns after time away | -t | no | ? |
| `harness.system.refusal` | the API refused a request: `[refusal <subtype>] originalModel=<v> fallbackModel=<v> apiRefusalCategory=<v>`, `fallbackModel=none` when no second model was tried; the record's `retractedMessageUuids` are not exposed (`--raw` shows them) | -t | no | verbose on a fallback, else no |
| `harness.system.api-error` | one failed API attempt: `[api-error status=<s> type=<kind> retry <n>/<max> in <ms>ms]` then the failure's wording, `status=` and `type=` each printed only when the record names it, `?` for an attempt the line omits (measured over one projects root: 105 heads with `status=` alone, 22 with neither, 5 with both) | -t | no | no |
| `harness.system.notice` | a banner: `[notice <level>] <content>` | -t | no | verbose |
| `harness.system.kill` | background agents were stopped: the bare `[kill]` | -t | no | yes |
| `harness.system.command` | a slash command's output, re-sent to the model as a user message: `[command] <content>` | -t | yes | yes |
| `harness.bookkeeping.last-prompt` | a `last-prompt` line: `[last-prompt lastPrompt=<text> leafUuid=<8 hex> explicit=<bool>]`, edge `for: leaf` | -t | no | indirect |
| `harness.bookkeeping.mode` | a `mode` or `permission-mode` line: `[mode mode=<v>]`, `[permission-mode permissionMode=<v>]` | -t | no | indirect |
| `harness.bookkeeping.name` | an `ai-title` or `agent-name` line: `[ai-title aiTitle=<v>]`, `[agent-name agentName=<v>]` | -t | no | indirect |
| `harness.bookkeeping.atis` | an `atis-latch` line: `[atis-latch atis=<v>]` | -t | no | no |
| `harness.bookkeeping.bridge` | a `bridge-session` line: `[bridge-session bridgeSessionId=<v> lastSequenceNum=<v> declaredDialogKinds=<v> sessionGroupingId=<v> noHistoryBackfill=<v> ownerAccountUuid=<v> ownerOrganizationUuid=<v>]` | -t | no | indirect |
| `harness.bookkeeping.suppression` | a `history-suppression` line: `[history-suppression cause=<v> vetoedAgainstAccountUuid=<v> ts=<v>]`; its `cause` is one of `migration`, `fork_inherit`, `meta_seed`, `chokepoint_veto`, `restored_owner_mismatch`, `env_owner_mismatch` | -t | no | indirect |
| `harness.bookkeeping.pr-link` | a `pr-link` line: `[pr-link prNumber=<v> prUrl=<v> prRepository=<v>]` | -t | no | indirect |
| `harness.bookkeeping.artifact` | a `frame-link`, `artifact-comment-monitor` or `artifact-autoreact-ledger` line: `[frame-link path=<v> frameUrl=<v> title=<v> artifactCount=<v>]`, `[<ledger type> v=<v> accountUuid=<v> artifacts=<sorted id list>]` | -t | no | indirect |
| `harness.bookkeeping.file-history` | a `file-history-snapshot` (the per-prompt table of tracked files and versions) or `file-history-delta` (one path's version bump): `[file-history snapshot at <local instant>: <path>@vN, ...]`, `[file-history delta at <local instant>: <path>@vN backup=<name>]`, edge `for: versions` | -t | no | indirect |
| `harness.bookkeeping.cost` | a `cost-state` checkpoint: `[cost-state totalCostUSD=<n> totalDuration=<ms> startTime=<epoch ms>]` and `[checkpoint closes <local instant>]` (tag; JSON `checkpoint_closes_at_utc`, `checkpoint_closes_at_local`, `checkpoint_join`) | -t | no | indirect |
| `harness.bookkeeping.fork` | a `fork-context-ref` line: `[fork-context-ref agentId=<v> parentSessionId=<v> parentLastUuid=<v> contextLength=<v>]`, edge `for: forked_from` | -t | no | no |
| `harness.bookkeeping.continued` | a `continued-in` line naming the child a background handoff created, edge `[for: continued_in @<session>]` | -t | no | indirect |
| `harness.bookkeeping.queue` | a `queue-operation` line that is not the person's own text (a `dequeue`, a harness notice): `[queue-operation operation=<v> reason=<v>]`; carries `queue_operation`, `queue_reason` | -t | no | no |

The `harness.bookkeeping` family renders `[<line type> key=value ...]` from the line's own keys in the writer's order, an account, organisation or record id cut to 8 characters, other values cut at 80 characters, an object as its sorted key list; every unit prints `[not delivered]` and carries no `for` key unless the table names an edge (docs: `csift search --help`; measured for the cost line: `csift search '' TARGET -t harness.bookkeeping.cost --format json`).

## Tags any leaf can carry

| tag | meaning | JSON |
|---|---|---|
| `[not delivered]` | the model never received this record, because its leaf is one the model does not receive or the delivery axis dropped it; printed on `search` and `show` on exactly the hits whose `delivered` reads false, so a text run and a JSON run count the same records (measured: `csift search '' @<uuid> --no-subagents -t 'user.*' -t 'agent.*' -t 'harness.*'` in text and with `--format json` on two sessions, 17,604 and 48,512 tags against as many `delivered:false` hits; `csift search '' -t LEAF --max-count 3` over one projects root printed it on every hit of the three `harness.turn` leaves, `harness.compaction.boundary`, `harness.schedule.event`, `harness.system.kill`, `user.queued` and `user.unsent`, and `csift show @<uuid> --line N` on a record of six of them) | `delivered` |
| `[abandoned]`, `[rewound]` | off the surviving conversation (references/survival.md) | `survival`, `abandoned_root_line` |
| `[replay copy of L<n>]` | an earlier copy of a record re-appended at line n | `replay_copy_of` |
| `[root L<n> +<hops>]`, `[root L<n>]` | the record that opened this unit's turn, and the parent hops to it (references/provenance.md) | `for_root` |
| `[for: <kind> L<n>]` and its variants | a relation the record states (references/provenance.md) | `for` |
| `[via skill <name>]`, else `[via plugin <name>]`, else `[via mcp <name>]` | what the request ran under | `attribution` |
| `[ends turn]` | the assistant's turn stopped at this tool | `tool_ends_turn` |
| `[feedback: <first 60 chars>]` | the words a person typed when refusing a call, on one line whatever newlines they held (the 60 and the one line: docs, `csift search --help`; measured: `csift show @<uuid> --line N` on a refusal whose feedback held 9 newlines printed its tag on one line) | `user_feedback`, the whole text with its newlines |
| `[api failure <kind> status <n> quota <window> resets <local instant> truncated after output aborted mid-stream]`, each part only when the record names it (a quota naming only its reset instant renders `quota resets <local instant>`; measured: `csift show @<uuid> --line 52996` printed `[api failure rate_limit status 429 quota five_hour resets 2026-09-03 23:00:00 AEST(UTC+10)]`) | why an API call failed, on the stand-in unit it left | `api_failure` |
| `[api <type>: <category>]` | the API's own stop details | `stop_details`, `stop_reason` |
| `[pdf pages A-B of <file>]` | which PDF pages a Read result holds | `pdf_pages` |

A tag is display only: csift writes it from fields and from other records, and a pattern never matches it; the one place its words are also matched is a fallback-only record, whose text they are (`agent.message.plain`, above). The JSON keys and their members are in references/search.md, JSON and Exposed keys.

## How a unit's text is built

An attachment unit shows one of two copies of itself and says which: `[rendered]` is the harness's converted text, the one the model received and a resume replays; `[payload]` is the payload's own field (a hook run's stdout, a hook's injected strings) or its verbatim JSON (JSON `body_source`). A `queued_command` that also carries a conversion for a human turn adds `[human-turn render differs]` and still shows the ordinary copy, because the record does not say which turn it was replayed into. Every attachment unit also names its producer, `[produced_by <command>]`, or an honest `[produced_by unknown (<reason>)]` (JSON `produced_by`, an object `{command, hook_name, payload_type}`; measured: `csift search '' TARGET -t harness.hook.context --format json`). A pattern matches the copy shown: a converted record matches its converted text, not the payload text it replaced.

A bookkeeping unit's text is its key=value head followed by the line's own verbatim JSON, so a key or a full uuid the head shortens still matches a pattern. A notification unit replaces the `<task-notification>` XML with `[<kind> <task-id>[, <task-id>...] <status>] <summary>`, naming every task the pulse closes (JSON `task_ids`); a pulse that reconciles orphaned tasks at a session start adds `(orphan reconciliation: <kind>)` (JSON `orphan_kind`).

## The 1:1 and emission rules

An address never misses a parseable line: `csift show TARGET --line N` renders a line csift models no leaf for as `? unmodeled: <type[/subtype|/payload type]>`, followed by the record's own text when it has any (`? unmodeled: user/isMeta ... L10  <its text>`) and otherwise by a key=value excerpt of its top-level keys (JSON `label` null, `labels` empty, `unmodeled` the shape; measured on a synthetic transcript: `csift --claude-home FIXTURE show @<uuid> --line N`), and a scan never emits such a unit, so no count moves. In that excerpt every member named `headers` prints `<elided>`, because an `api_error` record carries the failed request's HTTP response headers (cookies, organisation and request ids), and every member named `data` prints `<elided>`, because a base64 blob identifies nothing; `--raw` is the one path to those bytes. A line that is not a JSON object is an address miss pointing at `--raw` (docs: `csift show --help`).

A record whose leaf the model receives is emitted once by a scan without flags, over the lines that scan admits; the one exception is an image-only record, which has no text to match and is reached by an address or `csift image` (measured: 1,002 `harness.context.image` scan hits over one root, none of them image-only).

## Retired spellings

A retired selector, flag or subcommand (`harness.meta` and its leaves, the flat `thinking`, `tool` and `tool-response`, `harness.schedule.continuation`, `search --additional-context`, `csift turns`) is refused with its successors named, exit 2 for a selector and exit 1 for the flag and the subcommand; follow the error (measured: `csift search x TARGET -t harness.meta`, `csift search --additional-context x`, `csift turns @<uuid>`). `agent.message` and `agent.thinking` are categories, not retired spellings.
