# verbatim: restore the turns a compaction summary clipped

Read this before reconstructing what was said before a compaction, before building a SessionStart hook that re-injects clipped turns, and before reading `verbatim` output or its JSON.

## Contents

- When to use it
- Budget and selection
- Reading the output
- Slicing for hooks
- JSON
- Saving to a file

## When to use it

A compaction summary keeps the task state and loses the wording of the turns; `csift verbatim @<uuid>` re-emits the person's and the assistant's own words as rendered text (Reading the output says what the rendering changes), each unit with its `L<line>`, chosen newest first and printed oldest first, crossing as many compactions as the budget reaches. It is not for reading recent turns: `csift show TARGET --turn -3..` does that, and on a session that never compacted `verbatim` says so on stderr: ``csift: note: @<uuid> has no compaction — nothing was clipped; for plain reading use `csift show @<uuid> --turn <N|A..B|-k..>` (full records, no budget)`` (measured: `csift verbatim @<uuid>` on a session with no compaction). A target is required, and only the top-level thread is read unless `--subagents` is given, because the budget applies per session and a spanning run multiplies it (docs: `csift verbatim --help`).

A slash command with arguments opens no turn (references/ranges.md, Turns), so its words are never replayed: a session the person drove through such commands reconstructs as assistant units only (measured: `csift verbatim @<uuid> --format json` gave 7 assistant and 0 user units on one).

No abandoned turn is replayed: live and pre-cut turns are. A turn the conversation was rewound past, or a recalled draft, is counted in `abandoned  N turn(s) off the surviving conversation, not replayed: L... (read one: csift show @<uuid> --line N)` and not shown, because the compaction that clipped the conversation never saw it either. Turns above a compaction the walk could not cross are replayed and marked `pre-cut` in JSON (references/survival.md; measured: `csift verbatim @<uuid> --format json` on one session replayed 14 `live` and 61 `pre-cut` units and counted 43 abandoned turns). A pending question merged from the elicitation sidecar is appended as its own newest unit, an ASSISTANT unit (`△ (elicitation sidecar)  ASSISTANT`, JSON `role:"assistant"`, `source:"elicitation-sidecar"`, `line:null`, `is_automation:false`, body `AskUserQuestion: <question>`) that the header counts under `selected_assistant`, because the pending line stands for the assistant's call and carries nothing the person said (docs: `csift verbatim --help`; measured: `csift --claude-home <scratch home holding one session and one pending question> verbatim @<id>` printed `selected 1 user + 2 assistant units across 2 turns`, the third unit being the sidecar one).

## Budget and selection

| flag | does | default (docs: `csift verbatim --help`) |
|---|---|---|
| `--budget N` | characters per session (about 4 per token) | 40000 |
| `--round-trip-fraction F` | the share of the budget reserved for complete person-to-assistant round trips; F strictly between 0 and 1 | 0.5 |
| `--agent-msgs MODE` | per turn: `longest` keeps the longest agent message, the first when substantial, and each rich middle; `eot-only` the last; `rich` the last, the first, and each rich middle on long runs; `all` every one | `longest` |
| `--profile heavy`, `--profile light` | threshold bundles (heavy: run threshold 4, rich-min 200, declaration-max 140; light: 8, 360, 240); does not change `--agent-msgs` | none |
| `--max-compactions N` | stop after crossing N compactions; `0` means unlimited here, while the global `--cross-compactions 0` means cross none (references/survival.md) | 0 |
| `--turn`, `--since`, `--until` | window the candidates | none |

The header says `spanned K of N compaction boundaries in scope`: K counts the boundaries the selected turns crossed, N is the session's total, which `stats` also prints as `compactions`. K = 0 means no clipped turn was replayed: selection runs newest first, so on a session that kept working after its compaction the budget fills with later turns. Then window the candidates to end at the newest summary, taking its instant from `csift search '' @<uuid> --no-subagents -t harness.compaction.summary --format json | jq -r 'select(.kind=="exchange") | .hits[] | select(.label=="harness.compaction.summary") | .ts_utc' | tail -1` and passing it as `csift verbatim @<uuid> --until <that instant>` (measured on one such session: `spanned 0 of 1` by default, `spanned 1 of 1` with the `--until`). A budget smaller than a session's first round trip skips that session whole: stdout prints `SESSION <uuid>  skipped — its first round-trip needs ≥ 436 chars; raise --budget (now 100)`, the exit is 0, and JSON's header reads `sessions_rendered` 0 (measured: `csift verbatim @<uuid> --budget 100` on one session). Automation pulses open turns and are labelled `[<kind> <task-id> <status>] <summary>` with kind `background-command`, `workflow`, `agent`, `monitor` or `task`; they are excluded from the round-trip reservation, and the header splits `selected N user (M automation triggers)`. A wakeup tick opens no turn, so the work it triggered is grouped under the turn before it.

## Reading the output

| marker | means |
|---|---|
| `▽ L<n>  USER`, `△ L<n>  ASSISTANT` | a unit that opened a turn, and an assistant's unit; a `USER` unit is the person's words unless its body opens with an automation label such as `[background-command <task-id> <status>]` (JSON `is_automation`) (measured: `csift verbatim @<uuid> --format json` on one session: 27 of 46 user units automation pulses) |
| `▽ L<n>  agent.communication.inbox  <from> ⇨ self`, or `agent.communication.signal` | a message or a control signal another lane sent into this one, replayed in the person's place (JSON `is_inbound_comm`, `comm_label`, `comm_from`, `comm_to`; measured: `csift verbatim @<uuid> --format json` on a session with peer messages gave 6 inbox and 12 signal units) |
| `△ L<a>–L<b>  [N agent messages collapsed, N chars, N tool calls]`, with `, N failed` when a call errored | a folded run of agent messages (en dash between the lines); `N chars` is the length of what was folded; each folded message of 210 characters or more gets an indented preview line: `L<n>`, its first 60 characters, an ellipsis and `(+K chars)` (docs: `csift verbatim --help`); fetch the range with its `refetch` |
| `△ L<n>  [1 agent message collapsed, N chars, N tool calls]` | a fold of one message |
| `△ L<n>  [superseded by the same-prefix re-send at L<m>, N chars]` | an earlier message whose whole body a later message of the same turn repeats; the later one prints in full |
| `[+N chars, M lines elided]` between two ellipsis characters inside a body | a body cut in the middle; M counts the newlines inside the cut, so a cut between two newlines reports 0 and no line note prints |
| `[N tool calls]` | a kept turn's calls |
| `[N images: #1, #2]`, `[1 image: L<line>i<n>]` | images the turn carries, as `csift image --id` handles |
| `(also in summary)` | a turn the newest summary already quotes; kept, selected after the others |
| `══ compaction boundary · summary at L<n> · (turns above end before it, turns below end after it) ══` | where a compaction falls: turns are split by where they END, so the first unit below the marker can carry a line number smaller than the summary's (measured: `csift verbatim @<uuid>` on two sessions, one with its first unit below the marker at L2718 under `summary at L2794`); a `/rewind` summarize adds `· summarize <direction> ·` (docs: `csift verbatim --help`) |

Counts agree with their nouns (`[1 tool call]`, `1 line elided`) (measured: `csift verbatim @<uuid>`). A direct run caps a person's unit at 600 characters and an assistant's at 900, cutting the middle out; `--slices` drops those caps and keeps fewer, whole turns instead (docs: `csift verbatim --help`). Every unit is the rendered text, its line breaks flattened, so a double newline reads as one space: the person's exact words, line breaks included, are `csift show @<uuid> --line N --raw` on the unit's `L<n>` (measured on a synthetic session: a 720-character message holding one blank line printed 600 of its characters around a `… [+119 chars] …` cut, JSON `text` held 719 characters with no newline, and `--raw` kept both).

## Slicing for hooks

`--slice i` prints only the i-th chunk of the reconstruction, counting from 1 (`--slice 0` exits 1: `csift: error: --slice is 1-based: the first chunk is --slice 1`, measured), at most `--window` characters (default 10000; Claude Code caps a hook's `additionalContext` at 10,000 UTF-16 units, docs: `csift verbatim --help`, so pass about 9000 to leave room for a wrapper, less for text with emoji), with no header or footer. `--slices N` fixes the chunk count at N and fills them with whole turns, newest first, so N registered hooks always cover the output; the budget becomes N times the window. A slice past the end prints nothing and exits 0, so surplus hooks inject nothing. Slices are deterministic for a given session and settings, text only, and cannot be combined with `--out` (docs: `csift verbatim --help`). The ready hook is in references/hooks.md.

## JSON

`csift verbatim @<uuid> --format json`: a header whose `command` reads `verbatim`, with the budget accounting (`budget_chars`, `max_total_chars`, `round_trip_fraction`, `chars_used`, `boundaries_spanned`, `boundaries_total`, `selected_user`, `selected_assistant`, `automation_triggers`, `automation_by_kind` for the selected pulses, `automation_in_scope_by_kind` for every pulse in scope, `budget_is_per_session`, `sessions_in_scope`, `sessions_rendered`, `with_elicitation_sidecar`, `top_level_sessions`, `subagent_sessions` and the four duplicate keys); then `turn` rows (trio, `turn_index`, `line`, `source`, `survival`, `role`, instants, `tool_calls`, `full_chars`, `rendered_chars`, `truncated`, `elided_chars`, `elided_lines`, `also_in_summary`, `compactions_before`, `superseded_by_line`, `text` (the unit's whole rendered text, uncut by the 600- and 900-character caps, its line breaks flattened as above), `is_automation`, plus `trigger_kind`, `task_id`, `status`, `event` on a pulse and `is_inbound_comm`, `comm_label`, `comm_from`, `comm_to` on a message from another lane); `compaction_boundary` rows `{line, summary_chars, mode}` (`compact`, `summarize-from-here`, `summarize-up-to-here`); `collapsed_agents` rows (`agent_messages`, `tool_calls`, `failed`, `first_line`, `last_line`, `collapsed_chars`, `collapsed_previews[{line, excerpt}]`, `refetch`); a summary `{skipped_lines, abandoned_turns}` plus the crossing keys; the jsonl line is `line` on every row, as on every other command (measured: `csift verbatim @<uuid> --format json | jq -c 'select(.kind=="header") | keys'`, and the same for `turn` rows with `.is_inbound_comm`, on two live sessions, one of them carrying messages from another lane; docs: `csift verbatim --help`).

## Saving to a file

| action | irreversible: the command, its preview, the rule |
|---|---|
| save the reconstruction | irreversible: `csift verbatim @<uuid> --out PATH` replaces PATH with the reconstruction stdout shows, minus its opening `SESSION` block (the session line and the indented lines under it, whose number varies with the session: on a compacted one they add lines such as `in scope`, `dedup`, `abandoned` and the crossing notes, measured: `csift verbatim @<uuid>` printed seven, the closing rule included, on each of two compacted live sessions); stdout still prints the whole reconstruction and then `(wrote full reconstruction to <PATH>)` (measured on a synthetic session: stdout 14 lines without `--out`, the file 9, identical to those 14 from the sixth on); preview with `ls -l PATH` and `csift verbatim @<uuid>` with the same flags and no `--out`; run only after the user's reply to that preview says go |
