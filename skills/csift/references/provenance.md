# Provenance: which message a record belongs to, and what a message led to

Read this before answering "what did this message lead to", "what is this record for", before using `search --for`, and before reading a `for`, `for_root` or `[for: ...]` / `[root ...]` tag.

## Contents

- for_root: the turn a unit belongs to
- for: the relations a record states
- The relation inventory
- search --for: the reverse closure

Only `search` hits and `show` rows carry provenance. The other surfaces (`list`, `stats`, `files`, `recover`, `verbatim`, `agents`, `image`) index their rows in their own terms; to ask what one of their rows corresponds to, fetch its record with `csift show TARGET --line N`.

## for_root: the turn a unit belongs to

Every hit carries `for_root`, always an object `{line, uuid, hops, note}` naming the record that opened the unit's turn and the parent hops up to it. Its four answers:

| JSON | text tag | means |
|---|---|---|
| `{"line":12,"uuid":"<uuid>","hops":3,"note":null}` | `[root L12 +3]` | the parent links walk from this unit to the opener in 3 hops |
| `{"line":12,"uuid":"<uuid>","hops":null,"note":"not reached by the parent chain"}` | `[root L12]` | the turn grouping places the unit there but the parent links do not reach the opener |
| `{"line":null,"uuid":null,"hops":null,"note":"turn opener"}` | none | the unit is the opener |
| `{"line":null,"uuid":null,"hops":null,"note":"no opener above this unit"}` | none | nothing opened a turn above it: the context before the first opener (a SessionStart hook's output, a caveat, a compaction boundary), or the head of an abandoned branch |

The opener is the turn's own opener, not the first record of turn 0 (docs: `csift search --help`).

## for: the relations a record states

`for` is an array on every hit whose record names a relation, the JSON-only `parent` edge counting as one, with an entry for each relation the record states; it is absent where the record names none: a subagent's spawn prompt (a uuid, a null parent and no other edge), a bookkeeping line with no target, and `user.queued`, while a bookkeeping line that names a target carries its edge (`leaf`, `versions`, `forked_from`, `continued_in`) (measured: on one session's spanning flagless scan 1,787 `agent.communication.inbox` hits carried no `for`, the spawn prompts of its 1,787 lanes; under `-t` 4,091 `last-prompt` hits carried `leaf` and 1,338 `file-history` hits `versions`; on another session 32,872 of 32,872 hits of a flagless scan carried it). Each entry is `{kind, basis, target, reason}`.

- `kind`: one word of a closed vocabulary of 27, listed below, which is also the key set of `--count-by for-kind`.
- `basis`: `key` when the record names its target in a field, `position` when csift read it off where the record sits (the parent link points at the record above, not at a target its writer chose), and null on an `unknown` edge, which has no target to have read (measured: every `target:null` edge of `csift search '' @<uuid> --no-subagents -t 'user.*' -t 'agent.*' -t 'harness.*' --format json` on one session carried `basis:null`: 1,105 `for_tool_use`, 1 `attached_to` and 1 `reports`, each with the reason `the record carries no such key` or `the pulse names no tool-use-id`).
- `target`: `{line, uuid, session, note}` or null; its text forms are `[for: <kind> L<n>]` (a line of this transcript), `[for: <kind> <id> (not in this file)]` (an id no line here carries: a wire id, a record another session holds, a record a compaction deleted), `[for: <kind> @<session>]` (a whole transcript), `[for: <kind> @<session> <id8> (not in this file)]` (a record in another session the line names, as a fork's `forked_from` does; measured: 33 hits of `csift search '' -t harness.bookkeeping.fork` over one projects root), `[for: <kind> unknown (<reason>)]` (the relation exists and the record does not name its target; stated, never omitted). A uuid-shaped id prints its first 8 characters; any other id prints whole, because a wire id's head is the part they share (docs: `csift search --help`).

Every edge is one hop: it names the neighbour this record points at and follows nothing further. `parent` is dropped whenever another edge on the same unit already names the same record, so one relation is never printed under two words; the record's parent is unchanged. Six words ride JSON only and never print a tag, because the unit already shows the same fact another way: `parent` (the record above), `result_of` (the `▹` glyph), and the survival words `supersedes`, `continues`, `replays`, `branches` (the draft distance line and the `[abandoned]`, `[rewound]`, `[replay copy of L<n>]` tags). `result_of` sits on every unit that answers a call: a tool result, the `agent.communication.inbox` that stands for a subagent's return, `user.answer` (pointing at the question's call) and `user.rejection` (pointing at the refused call).

## The relation inventory

The 27 `for` words (docs: `csift search --help`, `--count-by for-kind`):

| kind | from, to | basis | carried by |
|---|---|---|---|
| `armed_by` | a schedule event, the call that armed it | null | always `unknown`: the join is an id inside a tool result; `search '<task id>'` finds it |
| `attached_to` | a record, the record it was written under | key, position, or null when `unknown` | attachments (their parent key); harness injections (the parent link) |
| `banner` | a fallback-served turn, its refusal banner | null | always `unknown`: one banner record to many turns |
| `blocked` | a blocking hook's feedback, the turn it stopped | position | injections |
| `branches` | a record, the head of its abandoned branch | position | JSON only |
| `continued_in` | a `continued-in` line, the child session a handoff moved to | key | bookkeeping; the target is a session, `[for: continued_in @<session>]` |
| `continues` | a rewound opener, the resend the conversation continued from | position | JSON only |
| `fires` | a scheduled prompt, its `scheduled_task_fire` record | position | injections |
| `for_tool_use` | an attachment, the call its payload names | key, or null when `unknown` | attachments; `unknown` on an attachment whose payload names no call |
| `forked_from` | a `fork-context-ref` line, the record the fork was cut at | key | bookkeeping |
| `hook_run` | a Stop-hooks summary, its hook run | key | a hook-run id, not an API tool call id |
| `interrupted` | an interrupt, the assistant message it cut short | key | |
| `leaf` | a `last-prompt` line, the chain leaf at that write | key | bookkeeping |
| `pairs` | a compaction boundary and its summary, both ways | key, position | key off the boundary's preserved-segment anchor, position where it names none |
| `parent` | any record, its `parentUuid` | position | JSON only |
| `precedes` | a compaction boundary, its `logicalParentUuid` | key | `not in this file` when the record was deleted with the compacted region |
| `preserves` | a compaction boundary, the head and the tail of the segment it kept | key | two edges; the tail can be `not in this file` |
| `redoes` | a harness redo nudge, the assistant turn it sent back | position | injections |
| `refused` | a model refusal, the user message it refused | key | system |
| `repairs` | a resume placeholder, the dangling prompt it answers | position | a paired placeholder only |
| `replays` | a replay copy, the surviving line carrying its uuid | key | JSON only |
| `replies_to` | an inbound relay, the message it answers | null | always `unknown`: no relay carries such a key |
| `reports` | a `<task-notification>` pulse, the call it reports on | key, or null when `unknown` | `unknown` when the pulse names no tool-use id |
| `result_of` | a unit answering a call, the call | key | JSON only |
| `supersedes` | a recalled draft, the message that replaced it | position | JSON only |
| `versions` | a file-history line, the prompt it versions | key | bookkeeping |
| `wraps` | a delivered local command's output, the record above it | position | system; a run of commands chains output to output, so follow `wraps` more than one hop to reach the slash wrapper |

Relations that are real and are not `for` edges, and where each answer lives. A relation whose target is in another transcript is a field on the surface that made the join, because an edge's target carries no re-feedable id trio; a relation whose target is not a record is a fact with its own key.

| relation | answered by |
|---|---|
| the turn a unit belongs to | `for_root` on every hit |
| a subagent, the call that spawned it | `agents`: `spawn_tool_use_id`, `parent_agent_id` (references/lanes.md) |
| a subagent transcript, its session | the id trio's `parent_session_id` |
| a clone, the session it was copied from; a `/clear` session, its predecessor; a handoff, its child; a record hydrated from another session | `list`: `clone_of`, `cleared_from`, `continued_in`, `minted_elsewhere` (references/sessions.md); the first two are inferred, the last two stated |
| a channel message, the record it landed in | `csift msg` (references/channel.md) |
| a peer relay, the lane that sent it | `peer_sender_task_id` |
| a hook attachment, the command that produced it | `produced_by` |
| a cost checkpoint (a `cost-state` line, leaf `harness.bookkeeping.cost`, reached only by `-t`), the instant it closes, `startTime` plus `totalDuration` | `checkpoint_closes_at_utc`, `checkpoint_closes_at_local` (references/search.md, Hit keys, family keys) |
| an image block, its record and position | `image_ids` and `csift image` handles |
| a `<persisted-output>` pointer, the file holding the output | `search --resolve-persisted` |
| an external write, the calls around it | `recover` boundaries (references/disk.md) |
| a summary, the region it replaced | nothing names it; `preserves` names what the compaction kept and `csift verbatim` rebuilds the rest |
| a turn's closing records, the turn they closed | `hook_run` for one hop; the rest only by position |
| a mid-turn steer, what it redirected | nothing names it |
| a unit `verbatim` folded, its placeholder | the fold markers in references/verbatim.md; a csift rendering choice, not a record |

## search --for: the reverse closure

`csift search '' TARGET --for ADDR` returns everything that is for one record, transitively: a record joins when it is for the address or for anything already in the answer, so one query returns the reply a message drew, the results of the calls that reply made, and a notification that reports on one of those calls later. `ADDR` is `L<line>`, which needs a scope of exactly one transcript, or a record uuid, which works in any scope. The one-transcript scopes are `@<uuid> --no-subagents`, `@<agent-id> --no-subagents`, a `*.jsonl` path with `--no-subagents`, and `@<agent-id>` of a subagent that spawned none; a `*.jsonl` path or an `@<agent-id>` alone takes the subagents below it, and a wider scope exits 1 naming its transcript count (measured on a synthetic session whose subagent spawned one of its own: `csift search '' TARGET --for L1 -c` exited 0 on each of those four scopes, and 1 on the session's path alone, 3 transcripts, and on the parent subagent's id or path alone, 2). A line past the file is an address miss, exit 1.

Three relations admit a record, and no others: its `parentUuid` names a member (which covers `parent` and every word built on the parent link: `attached_to`, `blocked`, `redoes`, `fires`, `wraps`), it carries a tool result answering a member's call (`result_of`), or it names a member's call by id (`reports`, `for_tool_use`). So `--for` on a record that only `versions`, `refused`, `interrupted`, `leaf`, `forked_from`, `continued_in`, `replies_to`, `armed_by`, `banner`, `hook_run`, a survival word, `precedes` or `preserves` points at comes back empty: a definitive absence about the three relations, not about the record; read the forward `for` on the hits, or `--count-by for-kind`, for those. A compaction summary and a paired resume placeholder do come back from their target, because their `parentUuid` is that target.

The parent relation stops at a turn boundary, so the person's next message is not in the answer. Each transcript gets its own closure, never a link between two, and the summary gains `for_of {address, records, transcripts}` (measured: `csift search '' TARGET --no-subagents --for L<n> --format json` gave `records:96, transcripts:1`). The pattern still applies, so use `''` for the whole set; it composes with `-t`, `-T`, time windows and scope. An address on a gated line (a hook attachment, a bookkeeping line) starts the closure there, but the addressed record itself prints only under its own gate: add `--attachments` to see an attachment address in the answer, and note that an attachment naming a member's call joins only when attachment lines are admitted.
