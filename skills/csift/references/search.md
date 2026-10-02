# search and show in depth: every flag, every JSON key, the zero-match diagnosis

Read this before piping csift into csift or into jq, before listing the calls that did not run or the results that failed, before reading or projecting the `--format json` output of `search` or `show`, before using a `search` or `show` flag that SKILL.md's "Search and read" section does not teach, before reading a glyph, tag or `↳` line in `search`'s text output, before relying on a `show --turn` fetch to hold a whole turn, on `show`'s cap or on a tail fetch, before claiming something never happened anywhere, and before parsing a zero-match summary. For one key, flag or stderr line, search this file for its name and read that row.

## Contents

- Recipes
- Flags beyond the core
- Census axes
- Text output
- JSON: header, exchange rows, hits, summary
- Exposed keys: facts a record states about the call behind it
- A zero match, read line by line
- Absence claims and the self-echo
- show in depth

## Recipes

Run one search over only the sessions another one matched:

```sh
csift search 'P' -l | csift search 'Q' --sessions-from -
```

Your own earlier csift calls are on disk, so both stages also match your own session unless each takes `-T 'agent.*'`, right when the words would occur only in the person's or the harness's text (measured: two phrases that existed only in the calling session's own csift commands printed that session, and nothing with `-T 'agent.*'`; the pipeline on a synthetic home, `csift search 'Error' -l | csift search 'two' --sessions-from - -c`, printed 1, both stages exit 0).

Read a field csift does not render (`stop_reason`, a key newer than csift) from the record's raw line:

```sh
csift show @<uuid> --line N --raw | jq '.message.stop_reason'
```

Across many records, `csift search 'P' @<uuid> --no-subagents -t LEAF --raw` prints one jsonl line per record; before judging a leaf from those bytes, read references/shapes.md. Never total `.message.usage` that way: one response's usage repeats on each of its records (references/sessions.md, stats).

List every call that did not run and every result that failed, each with the command that fetches it:

```sh
csift search '' TARGET -t agent.tool.result --format json | jq -c 'select(.kind=="exchange") | .hits[] | select(.denial_kind != null or .is_error == true) | {session_id, is_subagent, line, tool_name, denial_kind, is_error, refetch}'
```

A line number belongs to one transcript, the one the row's `session_id` names (an agent id on a subagent row), so on a spanning target `csift show @<session uuid> --line <it>` silently fetches another record; run the row's `refetch` instead, which addresses that transcript (measured on one session: 13 calls that did not run, 11 of them in subagent transcripts). `denial_kind` says why a call did not run (JSON, below) and is null on a call that ran and failed; it is null too on a call a Claude Code validator blocked before it ran (`<tool_use_error>Blocked: ...`, `is_error` true), which no denial kind names, so `.denial_kind != null` alone misses those (measured on that session: 119 rows, 13 with a denial kind and 106 errored with none, two of those 106 a validator's `Blocked: ...` result). The `agent.tool.result` view carries the refused tool's name, which the same record's `user.rejection` hit leaves null (measured on one session: an ExitPlanMode and an AskUserQuestion the person refused, one of them its `user.rejection`, and a Bash call a permission rule refused).

## Flags beyond the core

| flag | does | watch for |
|---|---|---|
| `--no-truncate` | prints each matched record's whole text instead of the excerpt, and fills JSON `body` | text stays one line per record; `body` keeps the rendered text's newlines, but a typed user message, answer or rejection is flattened before it renders, so `--raw` is the path to its line breaks (docs: `csift search --help`) |
| `--raw` | prints each matched record's jsonl line verbatim, one per record however many leaves matched; notes go to stderr | a record merged from the elicitation sidecar has no line and is omitted with a note; a `--max-count` drop is a stderr note too, `csift: note: 45 later exchange(s) dropped by --max-count` (`earlier` under a negative count); refuses `--format json` (exit 1) and `-l`, `-c` and `--count-by` (exit 2) (measured: `csift search P TARGET --raw` with each, and with `--max-count 1` and `-1`); with `--resolve-persisted` the line printed is still the original |
| `--siblings` | also prints the other records of each matched turn under a `·` marker: messages always; at most 2 thinking, 1 narration, 3 tool uses, 3 tool results and 2 harness records per leaf among the NON-matching records | the capped rest is announced as `(+N more · csift show @<id> --line A..B)`, a command to run as printed; JSON adds a `siblings` array and `siblings_hidden`, `turn_lines` (docs: `csift search --help`) |
| `--resolve-persisted` | replaces each `<persisted-output>` pointer with the content of its `tool-results/<id>.txt` file before matching, so a pattern reaches output Claude Code moved out of the transcript | a file that cannot be read keeps the pointer and appends `[csift: could not resolve persisted output <path>: <error>]` |
| `--attachments` | also scans every attachment line, the same as naming every attachment leaf with `.*` | attachment lines are most of many transcripts' bytes; name the one leaf instead (`-t harness.hook.context`) when you know it |
| `--multiline` | lets `.` cross newlines (`(?s)(?m)`) | reaches newlines in message, thinking and tool result text only; a newline inside a tool call's input is the two characters `\n` (measured under `-t agent.tool.use` on one session: `'git add[^"]*\\ngit'` matched 3 turns, the same pattern with a real newline, `--multiline 'git add[^"]*\ngit'`, matched 0) |
| `--turn`, `--since`, `--until` | window the scan (references/ranges.md) | an abandoned record belongs to no turn, so a `--turn` window leaves it out, and a zero match under `--turn` counts what it left out (A zero match, below) |
| `--max-count ±N` | earliest N or latest N exchange rows; `0` uncapped; the negative form is `search`'s alone | the footer names the dropped side (`45 later dropped by --max-count`); `-l` lists only the sessions of the kept rows and says so on stderr, `csift: note: N exchange(s) dropped by --max-count — this session listing may be incomplete; raise --max-count` (measured: `csift search P -l --max-count 1`) |
| `--for ADDR` | everything that is for one record, transitively | one closure per transcript; an `L<line>` address needs exactly one transcript in scope, a record uuid works in any scope (references/provenance.md) |

Short forms `search` accepts: `-t`, `-T`, `-i`, `-c`, `-l`, `-h` (docs: `csift search --help`). A pattern starting with `@` is an error, because targets follow the pattern (escape it `\@` to search for the character); a uuid in the pattern position with no target is searched as text and prints `csift: note: searching for this uuid as TEXT across the scope; ...` (measured: `csift search '@foo' @<uuid>` exits 1, `csift search <uuid> -c` exits 0 with the note). How a pattern is matched and written is SKILL.md's "Search and read".

A line written with spaces after its separators (another tool's re-serialization) reads like Claude Code's own; a record pretty-printed across several lines breaks the one-record-per-line framing, matches nothing, and each of its lines counts as malformed (measured on a synthetic transcript: a pretty-printed record gave 0 matches and `skipped_lines` 13 under `csift stats @<uuid> --format json`).

## Census axes

`--count-by AXIS` counts matched RECORDS, one line per key: the count right-aligned with leading spaces, two spaces, the key (`   2618  Bash`, so a split on the first space yields an empty field; measured: `od -c`), with an accounting line on stderr such as `csift: 2120 matched record(s) across 16 tool key(s) · 616 record(s) have no tool (outside this axis)` (measured: `csift search '' TARGET --no-subagents --count-by tool`). A record whose several sections match counts once. With `--format json`: `census` rows `{axis, key, records}` and a summary `{axis, matched_records, distinct_keys, excluded_records, dropped_by_cap, skipped_lines}` plus the crossing keys (references/survival.md).

| axis | one key per | notes |
|---|---|---|
| `label` | leaf | a record counts under every leaf it carries that your `-t`/`-T` leaves alive, so a leaf's count is how many records `-t <leaf>` would show; without `-t` only the leaves a scan without `-t` reads (references/labels.md, Gates) |
| `tool` | tool name | tool use and tool result records both count, so a completed call counts twice where `csift stats` counts calls once (measured on one session: Edit 478 records against `csift stats @<uuid> --no-subagents` Edit×239). Without `-t`, a result that is a `user.answer` or a `user.rejection` (a refusal with typed text) is emitted under that leaf and sits outside this axis and the `pairing` and `result` axes; under `-t agent.tool.result` it counts on all three; a refusal with no typed text counts `paired` and `error` either way (measured on one session: AskUserQuestion 13 records against 11 calls, its 9 answers outside the axis; on another, `--count-by result` read 4,326 `ok` and 117 `error` without `-t` and 4,331 and 119 with `-t agent.tool.result`, the difference its 5 answers and 2 typed rejections, while its 11 refusals with no typed text sat in `error` in both). Windows' separate `PowerShell` tool is its own key (UNTESTED) |
| `turn` | turn, ascending | keys `t<N>`, or `<transcript>·t<N>` with several transcripts in scope; abandoned records gather under `t? (abandoned)` (measured: `csift search '' TARGET --no-subagents --count-by turn` gave 58 rows for 57 numbered turns) |
| `session` | transcript: a session uuid or an agent id | |
| `pairing` | `paired`, `pending`, `orphan` | covers every record riding a tool block, SendMessage and spawns included, so "any calls left unanswered" needs no `-t` |
| `model` | the raw `message.model` | Claude Code's fabricated stand-in records report `<synthetic>` |
| `attachment` | attachment payload type | implies `--attachments` |
| `version` | Claude Code version stamped on the record | shows where an upgrade landed |
| `result` | `ok`, `error` | `pairing` says whether a result came back, `result` whether it was good |
| `for-kind` | provenance relation word | a record counts under every relation its edges name (references/provenance.md) |

Records outside an axis's domain are excluded and counted, never silently.

## Text output

A run prints a head (scope, totals, direction), the matched exchanges oldest first across every transcript in scope (undated last), and a tail repeating the totals with integrity notes: superseded drafts, abandoned records, the compaction cut, the crossing lines, `(chain leaf: <gate>)`, malformed lines skipped, and, when any excerpt was clipped, that too. An exchange is one matched turn, or one matched recalled draft or rewound opener (the message that opened a turn the conversation was later rewound past). Each gets a heading, then one line per hit: the role glyph, the leaf, bracketed tags, the tool name, `L<line>`, the text (measured: `csift search '' @<uuid> -t 'user.*' -t 'agent.*' -t 'harness.*'` on a session with subagents and drafts; the pending marker on a lane's own unanswered call):

| mark | where | means |
|---|---|---|
| `<id8>·t<N>  <local time>` | an exchange's heading | the transcript's id cut to 8 characters, lengthened to 12 or to the whole id when two transcripts in the output collide (a teammate's prints whole), itself an `@` target; then the turn; a subagent's heading adds `(parent <id8>)` (docs: `csift search --help`) |
| `<id8>·draft (superseded — outside turn numbering)`, `<id8>·rewound turn (the conversation was rewound past it — outside turn numbering)` | a recalled draft's or rewound opener's heading, in place of `·t<N>` | the record belongs to no numbered turn (measured: `csift search '' TARGET -t user.unsent`, `-t user.rewound`) |
| `◂`, `▸`, `⚙` | first on a hit line | the record's role: user, agent, harness |
| `?` | first on a line only a `show` address prints | a shape csift models no leaf for (references/labels.md, The 1:1 and emission rules) |
| `▹` | between two leaves | a tool use joined to its result |
| `(no result — pending)` | in place of `▹` and the result | a call no result answered |
| `⇨` | after a message's leaf | its direction, `<from> ⇨ <to>` |
| `[<tag>]` before `L<line>` | the label zone | a tag: display only, never matched (references/labels.md, Tags any leaf can carry) |
| `[1 image: #265]`, `[3 images: #1, #2, #5]` | at the end of a hit line | the images the record carries, as `csift image --id` handles (references/image.md) |
| `↳ csift show @<agent-id> --line N` | under a subagent's hit | the hit's own refetch; an image-carrying hit's `↳` line reads `↳ read the image(s): csift image @<uuid> --id N --out DIR` and a note, a command that WRITES files: irreversible, run only under references/image.md's preview-and-reply rule (measured: `csift search '' TARGET -t user.message` on a session with pasted images) |
| `↳ differs from the sent message in N chars (P% of the final)`, `↳ identical to the sent message` | under a draft's or rewound opener's heading | its distance from the message that replaced it (references/labels.md, `user.unsent`) |

## JSON: header, exchange rows, hits, summary

`search --format json` prints one JSON object per line: a header, one `exchange` row per exchange, and a summary, always the last line (`tail -1`). A row's `hits` array holds every match of its exchange (measured: `csift search Bash TARGET -t agent.tool.use --format json` gave 46 rows carrying 240 hits), so select the rows, then iterate the hits, never `.hits[0]`: `jq -c 'select(.kind=="exchange") | .hits[] | {label, line, excerpt}'`. A projection applied to the whole stream turns the header and summary into rows of nulls.

Header: `command`, `sessions_in_scope`, `top_level_sessions`, `subagent_sessions`, and `duplicate_transcripts`, `duplicate_records_dropped`, `divergent_transcripts`, `unreadable_transcripts` (references/sessions.md, one session under two directories).

Exchange row: the id trio; `turn_index` (null on a draft's or rewound opener's row); `superseded_draft` (true on a draft's row and on a rewound opener's row alike); `ts_utc`/`ts_local` (the turn opener's instant); `record_uuids` (every record of the turn); `hits`; and on a draft's or rewound opener's row `superseding_line`, `superseding_uuid`, `diff_chars`, `diff_pct`, `diff_exact`, all five absent, not null, on a draft nothing replaced, which prints no distance line either (measured: `csift search '' -t user.unsent --format json` over one projects root, 5 of 1,001 draft rows). With `--siblings`, `siblings`, `siblings_hidden`, `turn_lines`.

Hit keys, with what each holds per record kind:

| key | holds |
|---|---|
| `label`, `labels` | the leaf the unit was emitted under, one of `labels`; every leaf of the record, in an order that does not name that choice (references/labels.md, How a selector narrows) |
| `line`, `uuid` | the record's jsonl line and uuid; `line` is null for a sidecar-merged record |
| `ts_utc`, `ts_local` | the record's own instant, set on every hit whose line carries a timestamp and null on the rest, which are most `harness.bookkeeping` lines (measured on one session under the three globs and `--attachments`: 2,136 of 2,136 hits set without bookkeeping lines in scope; on another, null on 29,583 of 33,859 bookkeeping hits and on no hit of any other category) |
| `excerpt` | one line of rendered text around the match, whitespace runs collapsed, clipped near 400 characters with an ellipsis and `(+N chars)` (docs: `csift search --help`); for a tool use it is the tool name and the re-serialized input, for a tool result its content, for a notification the parsed head |
| `body` | the same rendered text uncut, newlines kept; null unless `--no-truncate` |
| `tool_name` | the tool's name on a hit whose record is a tool call or result, including `agent.communication.sent` and `.signal` (a SendMessage or spawn call) and the `agent.communication.inbox` unit standing for a subagent's return; null on every other kind, including `user.answer` and `user.rejection` (measured: 9 of 9 answers on one session) |
| `tool_use_id` | on a tool hit, the call's API id; on `user.answer` and `user.rejection`, the call the answer or refusal belongs to; null elsewhere |
| `pairing` | `paired`, `pending` or `orphan` on the same hits as `tool_name`; null elsewhere, including a subagent's return delivered as a notification (measured: `csift search '' @<uuid> --no-subagents --format json`). `orphan` is a result whose call is on no line of this file, its `result_of` edge then reading `(not in this file)` |
| `is_error` | true or false on a tool result; null elsewhere |
| `denial_kind` | why a call did not run, on that call's result, which the `agent.tool.result` view of the record carries whatever else it is labelled: `permission-rule` (a permission rule refused it, or a PreToolUse hook did: a result reading `PreToolUse:<Tool> hook error` carries this kind too, measured over one projects root: 15 of 15 such results), `user-rejected` (the person refused it, with or without typed text; with typed text the same record is also a `user.rejection`), `interrupted`, `cancelled`; null elsewhere (measured over one projects root: 532, 78, 4 and 1 results) |
| `from`, `to` | the direction of an `agent.communication.*` hit (`self` for the reading lane, `coordinator`, `?` unknown); null elsewhere |
| `delivered` | whether the model received this record (references/labels.md) |
| `survival`, `abandoned_root_line`, `replay_copy_of` | `live`, `pre-cut` or `abandoned`, the abandoned branch's head line, the line of the surviving copy (references/survival.md) |
| `for`, `for_root` | provenance edges: present wherever the record names a relation, the JSON-only `parent` edge included, and absent where it names none: a subagent's spawn prompt (a uuid, a null parent, no other edge), a bookkeeping line with no target, `user.queued`; a bookkeeping line that names a target carries its edge (measured: on one session's spanning flagless scan 1,787 `agent.communication.inbox` hits carried no `for`, the spawn prompts of its 1,787 lanes, while 4,091 `last-prompt` hits carried `leaf` and 1,338 `file-history` hits `versions` under their `-t`); the turn opener, an object on every hit (references/provenance.md) |
| `source` | `elicitation-sidecar` for a merged pending question, else null |
| `queue_operation`, `queue_reason` | the queue event and remove reason on `user.queued` and `harness.bookkeeping.queue`, both gated (reached only by a `-t` naming them); null elsewhere |
| `task_ids`, `orphan_kind` | on a notification: every task the pulse closes, and the reconciled kind; elsewhere `task_ids` is an empty array, never null, and `orphan_kind` null (measured: 2,110 empty arrays and no null over 2,136 hits on one session) |
| `resume_paired` | on `harness.resume.placeholder`: whether the prompt above is the repair prompt |
| `scheduled_at` | the fire instant exactly as the harness wrote it in the fire record's parenthesised tail (not ISO), on `harness.schedule.fire` and `.event` and on a wakeup or tick record whose parent is a fire record, which also carries a `fires` edge (measured over one projects root: 463 fire, 1 wakeup and 2 tick hits, all carrying both) |
| `mode`, `compact_metadata` | on the compaction pair: `compact`, `summarize-from-here` or `summarize-up-to-here`, which a boundary fetched alone still reports, the pairing being read over the whole file, and null only where the file leaves the boundary unpaired (docs: `csift show --help`; measured: `csift show @<uuid> --line <boundary line> --format json` read `compact`); the boundary's metadata object, uuid lists whole |
| `image_ids` | the image handles the record carries, an empty array when none |
| `refetch`, `refetch_uuid` | the `csift show` command for this record by line, and by uuid; prefer the uuid form across a live session, because Claude Code rewrites a live transcript in place on a rewind or a compaction and line numbers above the cut shift |
| family keys | set only on their leaf: `signature_len`, `thinking_text_absent` (thinking); `body_source`, `human_turn_render_differs`, `produced_by` (attachments); `relayed`, `peer_sender_task_id`, `image_only` (injections); `checkpoint_closes_at`, `checkpoint_join` (cost); `retried`, `original_model`, `fallback_model`, `category` (refusal); `status`, `retry_attempt`, `max_retries`, `retry_in_ms`, `error` (api error, the error's type and message only, never its headers); `task_id`, `cron`, `prompt` (schedule event; null when the fire record carries no `taskId`, which a `/loop` wakeup's can carry too; prompt cut at 200 characters, docs: `csift search --help`; measured over one projects root: 8 of 467 event hits carried them, one a `/loop` wakeup); `served_by`, `fell_back_from` (fallback turn) |

Summary: `matched` (exchange rows, as `-c` counts them), `sessions`, `transcript_ids` (the matching transcripts, first 100, with `transcript_ids_truncated`; not the same set as `-l`, which prints owning sessions), `dropped_by_cap`, `skipped_lines`, `with_elicitation_sidecar`, `excerpts_truncated`, `superseded_drafts`, `abandoned_records`, `rewound_turns`, `replay_copies`, `boundary_cut_line`, `leaf_source`, the six crossing keys at a non-zero `--cross-compactions`, `for_of` under `--for`, and on a zero match the keys of the next section but one. `-c --format json` prints the header and `{"kind":"summary","matched":N,"skipped_lines":M}`, with no crossing keys and no zero-match keys, even when N is 0 (measured: `csift search Bash TARGET -c --format json` and the same with a pattern that matched nothing).

## Exposed keys: facts a record states about the call behind it

These ride every `search` hit and `show` row whose record names them, whatever its leaf; a missing key means the record does not say, and a key of an unexpected JSON type reads as absent (`"apiBlockIndex":"3"` gives null), never coerced (docs: `csift search --help`). The tag each prints in the text output is in references/labels.md, Tags any leaf can carry.

| key | on | holds |
|---|---|---|
| `attribution{agent, skill, plugin, mcp_server, mcp_tool}` | assistant units | what the request ran under |
| `request_id` | assistant units, refusal units | the API request; records of one response share it |
| `api_block_index` | assistant units | which content block of that response |
| `tool_ends_turn` | `agent.tool.result` | the turn stopped at this tool |
| `user_feedback` | a refused call's result | the person's refusal text, whole, newlines kept |
| `api_failure{error, status, quota{limit_type, resets_at_utc, resets_at_local}, truncated_after_output, aborted_mid_stream}` | the stand-in unit a failed call left | why the call failed; `resets_at_utc` and `resets_at_local` are the quota's reset instant, which the harness writes in epoch seconds, as the usual pair (measured: `csift show @<uuid> --line 52996 --format json` gave `2026-09-03T13:00:00Z` and `2026-09-03T23:00:00+10:00` for a `resetsAt` of `1788440400`) |
| `stop_details{type, category, explanation, fallback_has_prefill_claim}`, `stop_reason` | any unit whose record's `message.stop_details` names a member | the API's own account of why it stopped |
| `pdf_pages{first, last, count, path}` | the result of a `Read` that passed `pages` | which pages of which document (references/image.md) |
| `interrupted_message_id` | an interrupt | the API message it cut short, also a `for` edge |
| `hook_run_id` | `harness.turn.stop-hooks` (gated: `-t`) | the hook run, not a tool call, also a `for` edge |
| `scheduled_task_id`, `scheduled_fire_id` | scheduled prompts | the task and the individual firing |

## A zero match, read line by line

stdout stays the normal empty output (`no matching exchanges`, or the header and summary lines in JSON) and the exit code is 0; stderr carries, in order (measured: `csift search zzqq TARGET`, `-c`, `-l` and `--count-by label` print it alike; the turn-window row with `csift search '' @<uuid> --no-subagents -t user.unsent --turn ..` on a session holding 2 drafts, in every mode; the malformed-line row on synthetic transcripts holding 13 and 1 malformed lines, under the plain run, `--raw`, `-c`, `-l`, `--count-by label` and their JSON forms):

| stderr line | means | next move |
|---|---|---|
| `csift: 0 matches — a DEFINITIVE absence (exit 0), NOT an error. Scope: N session(s). Active filters: ...` | what was asked | check the scope and filters are the ones you meant |
| `csift: ⚠ but the filter matches N record(s) OUTSIDE turn numbering (<leaf> ×n) - a recalled draft, a rewound turn or a record on an abandoned branch, which a --turn window leaves out; drop --turn, or window by time with --since/--until.` | under `--turn`, the pattern and the label filter match records that belong to no numbered turn | drop `--turn`, or window by `--since`/`--until` |
| `csift: ⚠ but "P" DOES occur — N record(s) under: <leaf> ×n · ...`, or with the empty pattern `csift: ⚠ but the filter DOES occur — N record(s) under: ...` | under a `-t`/`-T`, the pattern occurs under other leaves; the second wording is the empty pattern's, whose bare category or role selected nothing (measured: `csift search zzqq TARGET -t user.message` and `csift search '' TARGET -t harness.turn`) | select one of those leaves, or drop the filter |
| `csift: (even without the -t/-T filter, "P" has 0 matches here — genuinely absent in this scope, not a label mistake.)` | the filter is not the cause | widen the target or change the pattern |
| `csift: to see what a scope holds before guessing a filter, run ...` | no label filter was on | run the census it names |
| `csift: -t X selects no LLM-visible leaf; use X.* or a leaf` | a bare role or category whose leaves the model never receives | use the glob or a leaf |
| `csift: note: the gated leaves (...) are scanned only under a -t that reaches them` | those 23 leaves were not read | add a `-t` reaching them |
| `csift: note: the attachment leaves (...) are read only under --attachments or a -t that reaches them` | those 9 leaves were not read | add `--attachments` or the leaf |
| `csift: caveat: N malformed line(s) skipped — the absence is definitive for parseable lines only.` | N lines in scope did not parse; on a zero match every mode names them here, once, and prints no `csift: note: N malformed line(s) skipped` line (a plain run also prints `(N malformed line(s) skipped)` on stdout) | the absence holds for the other lines; `csift stats TARGET --no-subagents` counts the file's malformed lines |

JSON summary keys on a zero match: `definitive_absence` (true), `active_filters`, `excluded_by_label` (`{records, by_label}` when the probe found the pattern under other leaves; null when no label filter was on AND when the probe found nothing either, measured: `csift search zzqq TARGET -t user.message --format json`), `outside_turn_numbering` (the count of the turn-window line, 0 when a `--turn` window left nothing out, null without `--turn`; measured: the three cases on one session), `empty_visible_face` (the bare roles and categories that selected nothing), `gated_leaves_unreached` (true when no selector reached a gated leaf), `attachment_leaves_unread` (the attachment leaves not read, all nine without `--attachments` or a reaching `-t`).

## Absence claims and the self-echo

Before saying something was never done, decided or derived, search every session with the pattern written as a correct literal (SKILL.md, Search and read): `csift search 'P'` with no target, then `csift search 'P' -t 'user.*' -t 'agent.*' -t 'harness.*' --resolve-persisted`, which reads every gated leaf, the attachment leaves included, and the tool output Claude Code moved into `tool-results/` files; adding `--attachments` to it changes nothing (measured on one session: 6,558 records under the three globs with and without `--attachments`; on a synthetic transcript whose only copy of a string sat in a `tool-results/*.txt` file, 0 without `--resolve-persisted` under the globs and 1 with it). A configuration that steered a model often survives only as an attachment (`auto_mode` and similar), so census `csift search '' TARGET --count-by attachment` before concluding a setting was never seen. Some slash commands leave nothing on disk at all (UNTESTED, from csift's reading of Claude Code): `/btw`, `/tasks` (`/bashes`), `/rewind`, `/stop`, and any command panel closed with Escape; `/release-notes` writes a `harness.system.notice` record (gated: `-t`) the model never receives. No wrapper record is therefore no proof a command was not run.

No scan, under any flag, reads a line csift models no leaf for (an attachment payload type or a stamped `user` record no template matches, usage-limit notices the model received among them), and `--count-by label` cannot show such lines; only an address renders one (references/labels.md, The 1:1 and emission rules). Census them with a whole-transcript fetch, the cap lifted on purpose, then read each listed line with `csift show @<uuid> --line N`:

```sh
csift show @<uuid> --line 1.. --max-count 0 --format json | jq -c 'select(.kind=="record" and .label==null) | {line, unmodeled}'
```

Measured: on a synthetic transcript whose only copy of a word sat in an unmodeled `credential_org` attachment, the three globs with `--attachments` and `--resolve-persisted` printed `genuinely absent`, and this census listed `attachment/credential_org` at its line; on one live session it listed 14 `attachment/credential_org`, 4 `user/isMeta` and 3 `user/turnCompanion` lines in 6.3 s.

Running a search writes its own pattern into your transcript as a tool call the moment the call is recorded, so a later unscoped or `@main` search finds your own earlier invocation, labelled `agent.tool.use`. From a subagent the call is on disk when the command starts, so even the first search finds itself (measured from a subagent: a first `csift search '<fresh string>' @<parent uuid> -c` printed 1, a single `agent.tool.use` record). To check an absence, exclude your own records: `-T 'agent.*'` when the pattern would only occur in the person's or the harness's text, because your own lane's thinking and messages can quote it too (measured: the same search with `-T agent.tool.use` printed 0 on a first use, while a lane whose own thinking quoted the string still matched it under `agent.thinking.summary`), or target sessions other than your own.

## show in depth

A `show --turn` fetch returns the turn's `user` and `assistant` records (messages, thinking, tool calls and results, and the harness text a user record carries: notifications, injected skills and images, command wrappers, interrupts, a compaction summary) plus a compaction boundary. It leaves out every attachment line (hook records, reminders, the other context attachments), every bookkeeping line and `user.queued`, and every other `system` record (turn duration, stop hooks, recap, notices, API errors, scheduled events), though `search --turn N` gives each of those lines that turn's number (measured on two sessions: `csift show @<uuid> --turn .. --max-count 0 --format json` against `csift search '' @<uuid> --no-subagents -t 'user.*' -t 'agent.*' -t 'harness.*' --attachments --format json`, every left-out line being one of those kinds). `show` takes no `-t` (measured: `csift show @<uuid> --turn 0 -t harness.turn.duration` exits 1: `csift: error: show reads exactly ONE transcript per call; got 3 targets: '@<uuid>', '-t', 'harness.turn.duration'`), so reach those lines by `--line` or `--uuid`, or with `csift search '' @<uuid> --no-subagents --turn N -t LEAF`.

The tail of a transcript, every kind of record, is `csift show @<id> --line -30..`, its last 30 lines: on a subagent lane that holds its final message (measured: `csift show @<agent-id> --line -30.. --format json` on 150 completed lanes of one session found the lane's last `agent.message.*` record within 2 lines of the last line on all 150; on 150 lanes of another, within 5 lines on 146, the other 4 ending on a tool call and its result). It does not answer "what did the assistant say last" for a top-level session, whose tail is mostly bookkeeping, hook and wrapper lines (measured: 1 of the last 30 lines of one live session was an `agent.message` record; on a synthetic transcript the last 3 lines were all bookkeeping), and neither does `--turn -k..`, whose cap keeps the head of a long turn (below). That answer is `csift search '' TARGET --no-subagents -t agent.message --max-count -1`: the last hit it prints (one exchange can hold several), then `csift show TARGET --line <its line>`. Without `--no-subagents` a subagent that wrote later wins (measured on a synthetic session whose subagent wrote after the main thread's last message: the spanning form printed the subagent's L2, `--no-subagents` the main thread's L6; on a live session the spanning form printed a subagent's structured answer).

`show TARGET --line A..B` prints a range's live and pre-cut records first and its abandoned ones after them, so a recalled draft inside the range comes last (measured: `csift show @<uuid> --line A..B --format json` around a draft: lines 390 to 396 came back as 390, 391, 392, 394, 395, 396, then 393).

The cap is 200 record units unless `--max-count` says otherwise; a unit is one record, or one section of a record that renders several (measured: `csift show @<uuid> --line 1..400 --format json` kept 200 lines and reported 201 dropped units, line 382 being a notification that carries a subagent's result and renders as two). The cap keeps units in one order whatever order they print in and whichever end the address counts from: lowest line first, a record merged from the elicitation sidecar last, a `--turn` window by turn and then line, the units of one record in the order they render (docs: `csift show --help`, `--max-count`). So a `--turn -3..` fetch of long turns shows their start, and a `--turn -1..` fetch of a one-turn lane its first 200 units (measured: `csift show @<agent-id> --turn -1..` on a 424-line one-turn lane kept lines 1 to 351 and dropped 43 units). The drop prints a continuation that names the dropped units by their lines, sorted, with ranges that overlap or touch merged, and a dropped record merged from the elicitation sidecar, which has no line, by `--uuid`, whatever the address (measured on a synthetic transcript with an abandoned draft at L3, an attachment line at L6, a replay copy at L12 and L13 and a merged sidecar record):

| address | the continuation |
|---|---|
| `--line` | `--line 1..6,12..19 --max-count 5` continued `--line 3..6,12..19` |
| `--uuid` | `--uuid u4,u3,a1,u-toolu_AQ1 --max-count 5` continued `--line 14,16 --uuid u-toolu_AQ1`: the lines of the dropped records, the sidecar record by its uuid |
| `--line` with `--uuid` | `--line 1..5 --uuid u3,a7 --max-count 2` continued `--line 2..5,12..15` |
| `--turn` | `--turn 1 --max-count 3` continued `--line 7..13`, which leaves out the attachment line L6 the window never fetched |

A continuation always moves forward: followed one after another at the same cap until none prints, the printed commands return every unit of the uncapped fetch exactly once (measured with a chain follower on that transcript, every form above plus `--turn ..`, `--turn 1..3` and `--turn -2..` at caps 1, 2, 3 and 5, rendered and raw: 80 of 80 chains ended with the `--max-count 0` fetch, no unit missing or repeated; `--turn .. --max-count 5` continued `--line 4..5,7..17 --uuid u-toolu_AQ1`, then `--line 9..17 --uuid u-toolu_AQ1`, then `--line 14..17 --uuid u-toolu_AQ1`). A turn wider than the cap pages forward the same way (measured: a 536-unit turn paged `--turn 0`, `--line 201..536`, `--line 401..536` at the default cap, 200 + 200 + 136). A wide window's continuation is long, because the lines it names sit among lines the window never fetched (measured: `--turn ..` on a 397 MiB transcript continued with 17,534 line pieces, 156,874 bytes). The text pointer reads `+15 more record unit(s) beyond the 5-unit cap · continue: csift show @<uuid> --line 4..5,7..17 --uuid u-toolu_AQ1  (or --max-count 0 = the whole fetch)`, and JSON carries the same command as `refetch_remainder`. `show` takes no `--skip` (exit 2, measured: `csift show @<uuid> --turn 1 --skip 3`).

`--raw` prints the jsonl lines verbatim, refuses `--format json` (exit 1, measured: `csift show @<uuid> --line 30 --raw --format json`), and stops at 200 lines the same way, naming the dropped lines on stderr: `csift: note: +200 more line(s) beyond the 200-unit cap · continue: csift show @<uuid> --line 201..400 --raw  (or --max-count 0 = the whole fetch)` (measured: `csift show @<uuid> --line 1..400 --raw` on a 536-line transcript). A `--raw --uuid` fetch prints every line carrying the uuid, so a replay copy and its survivor both print, lowest line first; a uuid that names a record merged from the elicitation sidecar has no line, so it prints nothing and `csift: note: uuid <U> names a sidecar-merged record, which has no physical transcript line — omitted under --raw (the rendered fetch shows it)`, exit 0 (measured on a synthetic transcript holding a replay copy at L14 of the record at L15 and an open sidecar marker). A range partly past the end returns what exists and reports `lines_past_end` (measured: `csift show @<uuid> --line 6550..99999 --format json` on a 6,561-line file).

`show --format json`: a header `{command, session_id, is_subagent, parent_session_id, path}`; one `record` row per unit carrying the hit keys above except `excerpt`, `refetch` and `refetch_uuid`, plus `text`, `turn_index`, `superseded_draft` and the five draft distance keys (measured: the same three records under `csift show @<uuid> --line N --format json` and in a `search --format json` hit), where `text` is complete under `--line`/`--uuid` and clipped near 400 characters under `--turn`, and `body` keeps the newlines under an address and is null under `--turn`; a summary `{records, dropped_by_cap, refetch_remainder, lines_past_end, non_record_lines, skipped_lines, with_elicitation_sidecar}` plus the crossing keys, `records` counting units, and `refetch_remainder` null when nothing was dropped (measured: the synthetic `--turn .. --max-count 5 --format json`, whose summary read `records` 5, `dropped_by_cap` 15; docs: `csift show --help`).
