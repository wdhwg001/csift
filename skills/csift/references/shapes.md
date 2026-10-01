# Shapes: where each leaf comes from on disk, and how to grab those bytes

Read this before reading `--raw` output by hand, before claiming a label is wrong for a line, and whenever you need to know which jsonl field a leaf was decided from. For one leaf, search this file for it and read that row.

## Contents

- Reading the table
- The 73 shapes

## Reading the table

Every label csift prints names a shape a transcript line really has, so any unit can be checked against its bytes. To grab one record of a leaf, find a line with `csift search '' @<uuid> --no-subagents -t LEAF --max-count 1`, which prints the earliest turn holding that leaf, then print that one line exactly as stored with `csift show @<uuid> --line N --raw`, N being the `L<n>` of one of the leaf's hits. `csift search ... --raw --max-count 1` is not a one-record grab: it prints every record of the leaf in that turn (measured: 76 raw lines for `-t agent.tool.use` on one session). An empty result is an ordinary empty (Laws, law 2 in SKILL.md). To read one field across every record of a leaf, pipe the raw lines into jq:

```sh
csift search '' @<uuid> --no-subagents -t user.answer --raw | jq -c '.toolUseResult | keys'
```

"Stamped" in the table means a `type:"user"` record carrying one of the harness's two authorship flags, `isMeta:true` or `turnCompanion:true`, which a person's own message never carries; a template is matched at the start of the content after leading whitespace, so a record that merely quotes a template later in its text keeps its own leaf. A peer or notification tag counts only at a section start: the content's start after leading whitespace, the point just after the relay preamble `Another Claude session sent a message:`, or the point just after a previous section's closing tag; a tag anywhere else is a quotation, and the record keeps its own leaf (measured on a synthetic transcript: `<teammate-message` at each of those three places took `agent.communication.inbox`, and the same tag quoted mid-sentence stayed `user.message`). A stamped record matching no template carries no leaf and renders under an address as `? unmodeled: user/isMeta` or `? unmodeled: user/turnCompanion` (measured on a synthetic transcript with `csift --claude-home FIXTURE show @<uuid> --line N`).

## The 73 shapes

| leaf | on disk |
|---|---|
| `user.message` | `type:"user"`, `message.content` a string or text blocks with no `tool_result`, no authorship flag, no template, no peer tag at a section start; or a stamped record with top-level `origin.kind:"human"` (relayed); or `type:"attachment"`, `attachment.type:"queued_command"` whose payload `origin.kind` is `human` (the mid-turn steer) |
| `user.answer` | a `tool_result` carrier answering an `AskUserQuestion` call, whose `toolUseResult` holds a non-empty `answers` map or a non-blank `response` string (the `response` form UNTESTED: every one of 127 answers over one projects root held `answers`, measured: `csift search '' -t user.answer --raw` into jq); notes sit in `annotations`, and there is no `chosenOption`, `answer` or `selected` key (measured: the jq recipe above gave `questions`, `answers`, `annotations`). With no `toolUseResult` on the carrier, a result text opening with one of the harness's answer markers (`Your questions have been answered:` among them), and only when the paired `tool_use` is `AskUserQuestion`; the same text on another tool's result, such as a `Bash` grep of it, is `agent.tool.result` and opens no turn (measured on a synthetic transcript holding both, `csift --claude-home FIXTURE search '' @<uuid> --count-by turn`: the AskUserQuestion result opened turn 1, the Bash result stayed in it) |
| `user.rejection` | a `tool_result` carrier whose text contains `To tell you how to proceed, the user said:` followed by typed text; a rejection with no typed text is an ordinary `agent.tool.result` |
| `user.unsent` | decided from the parent links, not from a field: an opener sharing its `parentUuid` with a later opener and with no assistant record below it |
| `user.rewound` | the same, with an assistant record below it |
| `user.queued` | `type:"queue-operation"` whose `content` is the person's text; `operation` names the event |
| `agent.message.plain` | a `{type:"text"}` block of a `type:"assistant"` record, or a `{type:"fallback"}` block naming the two models |
| `agent.message.structured` | `type:"attachment"`, `attachment.type:"structured_output"` |
| `agent.thinking.summary` | a `{type:"thinking", thinking, signature}` block whose tag inside the base64 `signature` reads thinking; the salvage form has an empty `signature` on a record whose model is `<synthetic>` |
| `agent.thinking.narration` | the same block shape with the signature tag narration |
| `agent.thinking.omitted` | a thinking block whose `thinking` is empty or whitespace and whose `signature` is not |
| `agent.thinking.redacted` | a `{type:"redacted_thinking", data}` block (measured on a synthetic transcript) |
| `agent.tool.use` | a `{type:"tool_use", id, name, input}` block of a `type:"assistant"` record; or a pending marker in `<session>/elicitations.jsonl`; a sidecar marker has no line, so a `--raw` grab omits it with a stderr note |
| `agent.tool.result` | a `{type:"tool_result", tool_use_id, content, is_error}` block on a `type:"user"` carrier, beside the carrier's top-level `toolUseResult` |
| `agent.communication.inbox` | a `type:"user"` record whose content opens, at a section start, with `<teammate-message`, `<agent-message` or `<cross-session-message`; a subagent transcript's first user record (its spawn prompt); the result of an `Agent`/`Task` call or a `<task-notification>` carrying a `<result>`; a stamped record whose `origin.kind` is `coordinator` |
| `agent.communication.sent` | a `tool_use` naming `SendMessage` or a spawn tool |
| `agent.communication.signal` | a `SendMessage` whose `input.message.type` is a control kind, or a peer control payload |
| `agent.communication.channel` | `type:"attachment"`, `attachment.type:"hook_additional_context"`, whose first `content` string opens `[csift-channel v1 ` (measured on a synthetic transcript) |
| `harness.notification.workflow` | a `<task-notification>` section, at a section start of a user record or inside a `[SYSTEM NOTIFICATION - NOT USER INPUT]` isMeta frame, whose summary names a workflow |
| `harness.notification.monitor` | the same, summary opening with Monitor, Scheduled or a cron word |
| `harness.notification.subagent` | the same for a spawned agent; or a `type:"user"` plain-text notice `N background agents were stopped by the user: ...` (measured on a synthetic transcript) |
| `harness.notification.background-command` | the same for a background shell command |
| `harness.notification.task` | any other `<task-notification>` section |
| `harness.compaction.summary` | `type:"user"`, `isCompactSummary:true`, string content, plus `isVisibleInTranscriptOnly:true` (a compaction) or `summarizeMetadata:{direction, messagesSummarized}` (a `/rewind` summarize) |
| `harness.compaction.boundary` | `type:"system"`, `subtype:"compact_boundary"`, `compactMetadata` (`trigger`, `preTokens`, `postTokens`, `durationMs`, `cumulativeDroppedTokens`, `preCompactDiscoveredTools`, `preservedSegment`, `preservedMessages`, and `messagesSummarized` on a summarize; measured: the keys of 8 boundaries on one session), a top-level `logicalParentUuid`, usually a null `parentUuid` |
| `harness.command.invocation` | a user record whose text opens `<command-name>` or `<command-message>` |
| `harness.command.stdout` | a user record whose text opens `<local-command-stdout>` |
| `harness.command.caveat` | stamped, opening `<local-command-caveat>` |
| `harness.interrupt.user` | a user record whose text is exactly `[Request interrupted by user]` |
| `harness.interrupt.tool` | exactly `[Request interrupted by user for tool use]` |
| `harness.schedule.wakeup` | stamped, opening `# Autonomous loop check`, `You're being invoked on a timer` or `<<autonomous-loop-dynamic>>` (measured on a synthetic transcript for the first and third) |
| `harness.schedule.event` | `type:"system"`, `subtype:"scheduled_task_fire"`, `content` ending in the fire instant in parentheses |
| `harness.schedule.fire` | an isMeta user record whose `parentUuid` names a `scheduled_task_fire` record; a wakeup or tick marker at its start wins over the parent link |
| `harness.schedule.tick` | stamped, opening `# Autonomous loop tick` or `Run the autonomous check` |
| `harness.resume.prompt` | stamped, opening `Continue from where you left off.` |
| `harness.resume.placeholder` | a `type:"assistant"` record whose model is `<synthetic>` and whose text is exactly `No response requested.` |
| `harness.hook.context` | `type:"attachment"`, `attachment.type:"hook_additional_context"` with `hookName`, `hookEvent` and a `content` string array |
| `harness.hook.outcome` | `type:"attachment"`, `attachment.type` one of `hook_success`, `hook_blocking_error`, `hook_non_blocking_error`, `hook_cancelled`, `hook_stopped_continuation`, `hook_system_message`, `async_hook_response_batch` |
| `harness.hook.feedback` | stamped, opening `Stop hook feedback:`, `TeammateIdle hook feedback:`, `TaskCreated hook feedback:` or `TaskCompleted hook feedback:` |
| `harness.context.reminder` | `type:"attachment"` with `attachment.type` `total_tokens_reminder`, `task_reminder`, `silent_turn_reminder`, `batching_reminder_sent`, `ultrathink_effort`; or a stamped record opening `<system-reminder>` that no narrower template matches (measured on a synthetic transcript) |
| `harness.context.listing` | attachment types `skill_listing`, `agent_listing_delta`, `deferred_tools_delta`, `deferred_tools_record`, `dynamic_skill`, `invoked_skills`, `mcp_instructions_delta` |
| `harness.context.prompt` | attachment types `instructions`, `date`, `session_context`, `environment`, `model`, `prompt_snapshot`, `date_change` |
| `harness.context.file` | attachment types `file`, `edited_text_file`, `compact_file_reference`, `nested_memory`, `plan_file_reference`, `read_truncation_notice`, `bash_output_audience_note` |
| `harness.context.mode` | attachment types `plan_mode`, `plan_mode_exit`, `plan_mode_reentry`, `auto_mode`, `command_permissions` |
| `harness.context.ide` | attachment types `opened_file_in_ide`, `selected_lines_in_ide`, `diagnostics` |
| `harness.context.task` | attachment types `task_status`, `queued_command` not from the person, `workflow_keyword_request`, `goal_status`, `remote_session_change`, `relevant_memories`, `memory_update`, `thinking_stripped`, `thinking_drop` |
| `harness.context.image` | a stamped record opening `[Image: ` or `[Image source: `; or an isMeta user record whose content is only `image` or `document` blocks. The same image-only record without `isMeta` carries no leaf (`? unmodeled: user/image`); an image beside text rides that record's own leaf; no scan emits an image-only record: `csift image TARGET` lists its images and `csift show TARGET --line N --raw` prints it |
| `harness.context.skill` | stamped, opening `Base directory for this skill:` (measured on a synthetic transcript) or `# Workflow authoring reference` |
| `harness.enforce.structured-output` | stamped, opening `[structured-output-enforce]` |
| `harness.enforce.brief` | stamped, opening `You ended the turn without calling SendUserMessage.` (measured on a synthetic transcript) |
| `harness.enforce.handback` | stamped, opening `[handback-send-enforce]` |
| `harness.enforce.retry` | stamped, whole text equal to one of nine harness messages: the four redo nudges `The previous response failed to produce a valid tool call. Please retry the tool call now.`, `Your tool call was malformed and could not be parsed. Please retry.`, `[Your previous response had no visible output. Please continue and produce a user-visible response.]` and `The PermissionDenied hook indicated you may retry this tool call.`; two classifier-stop forms opening `Your response above was stopped by a safety classifier` (the second adds that a tool call whose result reads `Interrupted` was already running); the resume opening `Output token limit hit. Resume directly`; two cut-off forms opening `Your response above was cut off mid-stream`, a main lane's continuing `. Resume directly from where it stops`, a subagent's ` and only your next message is delivered.`; or a text opening `The last Edit failed because the target file was modified`. Five of the nine contain an em dash, so a copy retyped with a hyphen matches none (docs: `csift search --help`) |
| `harness.turn.duration` | `type:"system"`, `subtype:"turn_duration"` |
| `harness.turn.stop-hooks` | `type:"system"`, `subtype:"stop_hook_summary"`; its `toolUseID` is a hook-run id, not an API tool call id |
| `harness.turn.recap` | `type:"system"`, `subtype:"away_summary"` |
| `harness.system.refusal` | `type:"system"`, `subtype` `model_refusal_fallback` or `model_refusal_no_fallback` |
| `harness.system.api-error` | `type:"system"`, `subtype:"api_error"`, with an `error` object that can hold the whole HTTP response header set; `--raw` prints those headers with the line |
| `harness.system.notice` | `type:"system"`, `subtype:"informational"` |
| `harness.system.kill` | `type:"system"`, `subtype:"agents_killed"`, no payload (measured on a synthetic transcript) |
| `harness.system.command` | `type:"system"`, `subtype:"local_command"` |
| `harness.bookkeeping.last-prompt` | `type:"last-prompt"` (`leafUuid`, `explicit`, `lastPrompt`) |
| `harness.bookkeeping.mode` | `type:"mode"` or `type:"permission-mode"` |
| `harness.bookkeeping.name` | `type:"ai-title"` or `type:"agent-name"` |
| `harness.bookkeeping.atis` | `type:"atis-latch"` |
| `harness.bookkeeping.bridge` | `type:"bridge-session"` |
| `harness.bookkeeping.suppression` | `type:"history-suppression"` |
| `harness.bookkeeping.pr-link` | `type:"pr-link"` |
| `harness.bookkeeping.artifact` | `type:"frame-link"`, `type:"artifact-comment-monitor"` or `type:"artifact-autoreact-ledger"` |
| `harness.bookkeeping.file-history` | `type:"file-history-snapshot"` or `type:"file-history-delta"` (`trackingPath`, `backup`) |
| `harness.bookkeeping.cost` | `type:"cost-state"` (`totalCostUSD`, `totalDuration`, `startTime`, ...) |
| `harness.bookkeeping.fork` | `type:"fork-context-ref"` (`parentSessionId`, `parentLastUuid`) |
| `harness.bookkeeping.continued` | `type:"continued-in"` (`continuedInSessionId`) |
| `harness.bookkeeping.queue` | `type:"queue-operation"` whose content is not the person's text, or which carries no content (a `dequeue`) |

None of the 18 bookkeeping line types (docs: `csift search --help`) carries a `uuid` or a `message`, so their only address is their line number. A queued text that was later sent also appears as the user record it became, and csift classifies each line by its own shape, so the same words can match under two leaves and count twice in a census.
