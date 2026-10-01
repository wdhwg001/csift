# Live state: is a session running, stopped, or waiting (status), waiting on it (wait), background tasks, pending questions

Read this before saying a session is running, finished or stuck, before blocking until a session stops or asks something, before counting its background tasks, before relying on a pending AskUserQuestion showing up, and before reading the verdict of a session that may have ended long ago.

## Contents

- status and its seven verdicts
- Background tasks and the lens
- What the evidence is made of
- wait
- Pending questions: the elicitation sidecar
- JSON

These two commands answer "now": run twice, they can differ, while `search`, `show` and `stats` give the same answer over the same bytes (docs: `csift status --help`).

## status and its seven verdicts

`csift status @<uuid>` joins three sources into one verdict with the evidence that produced it: Claude Code's session registry (`<claude-home>/sessions/<pid>.json`), the transcript's tail (an unanswered call at the tail is a tool in flight), and whether the owning process is alive. It takes exactly one session; a project target exits 1 (`csift: error: status/wait watch exactly ONE session per call; '.' resolves to 23 sessions.`, measured: `csift status .`). Subagent lanes join by default; `--no-subagents` watches the main lane only.

| verdict | means |
|---|---|
| `running` | a tool call in flight at the tail, or the registry says `busy` |
| `waiting-children` | the main lane is idle and subagent or workflow lanes are live (a lane its workflow run settled is not live: What the evidence is made of, below) |
| `waiting-hitl` | blocked on a person: a pending question in the elicitation sidecar, the registry's `waiting` (any blocking dialog: a question, a permission prompt, a plan approval), or an unanswered AskUserQuestion or ExitPlanMode at the tail |
| `idle-background-open` | the turn ended and background tasks the lens counts have not returned; neither running nor stopped |
| `idle-eot` | the turn ended and nothing is pending anywhere |
| `stale-dead` | the registry row's process is gone (a process-start-time check guards against a reused pid); the tail says how it ended. Without a registry row a dead session gets no `stale-dead`: it reads from its tail alone, as the verdict of the transcript's last state |
| `unknown` | the evidence is missing or contradicts itself |

Precedence when several apply: dead, then a person, then running, then children, then background, then end of turn, then unknown (docs: SPEC.md section 6.13, https://github.com/wdhwg001/csift/blob/v0.13.0/SPEC.md). Measured on three live sessions: `idle-background-open`, `waiting-hitl`, `waiting-children` (`csift status @<uuid> --format json`).

The everyday questions, and the verdict that answers each while the evidence carries a `registry` row (`csift status @<uuid> --format json`, measured on each session named):

| you want to know | the answer |
|---|---|
| is it open and waiting at its prompt | `idle-eot` with `pid alive` (a live session: registry `status idle`, pid alive) |
| is a dialog up: a question, a permission, a plan approval | `waiting-hitl` (a live session: registry `status waiting`, pid alive) |
| is it working now | `running` (a live session: registry `status busy`, pid alive) |
| has it ended | `stale-dead` (a synthetic registry row whose pid no process holds: `pid dead`); without a registry row, an old tail whatever the word (below) |
| is it idle with something it started still going | `idle-background-open` (a synthetic registry row naming a live process of this host and one open background task; the same with `--background-since now` read `idle-eot`) |

`waiting-children` answers "idle, while its subagents work", and its children half has no age rule: an in-flight child keeps it however old its unanswered call is, as long as the main lane is idle (measured: a live session read `waiting-children`, registry `idle` and pid alive, from 2 `in-flight` children whose unanswered calls were 288,296 s and 2,274,446 s old, its settled lanes folded into one count). The JSON `children[]` rows are where you decide a child is dead: each names the lane, its state and, in `detail`, the unanswered call and its age (`unreturned Bash call (288296s ago)`); `csift agents @<uuid> --agent @<child id> --format json` gives the same child's `pending_since_utc` and `last_activity_utc`, to compare with the current time yourself.

A verdict speaks about now only while its evidence carries a `registry` row and `pid alive`. With no registry row a note says so (`no registry row for this session (not currently registered, or an older Claude Code) - verdict from tail + children evidence`), and the verdict describes the transcript's last state however old it is; the age sits in each evidence row, `(Ns ago)` in text and `age_secs` in JSON. No registry row and a tail hours or days old means the session ended: nothing in it runs or waits, whatever the verdict word, and nothing can reach its lanes (measured: `csift --claude-home FIXTURE status @<uuid>` on a synthetic session whose tail was an unanswered spawn call 239 days old read `running`, its teammate `in-flight`, with that note; a live session read evidence `registry` `status busy (pid N)` and `pid` `alive (start-time guard matched)`).

The report ends with the person's newest words and the newest assistant message as excerpts (`last ◂`, `last ▸`; JSON `last.user` and `last.agent`, each `{text, truncated, ts_utc, ts_local}`), clipped with a marker near 400 characters (measured: `csift status @<uuid> --format json` gave a 415-character agent excerpt with `truncated:true`; the whole turn is `csift show @<uuid> --turn -1`). The person's words are the newest record labelled `user.message`, `user.answer` or `user.rejection`, whether or not it opened a turn, so a slash command typed with arguments shows as `/name args` and an automation pulse never takes the slot (docs: `csift status --help`; measured: on a session driven by one such command, `last.user.text` opens with `/`). An excerpt is a partial view, useful for one thing, judging whether a background task still matters; it is never a review of the work, however complete it sounds, and a supervising agent that trusts it has skipped the reading a person would do.

## Background tasks and the lens

A background shell returns its tool result at once and Claude Code writes nothing about it while it runs, so `status` scans the whole main transcript for launches and completion notices, and lists every OPEN task: kind (`shell`, `agent`, `monitor`), id, launch instant and age, description or command, output file size and last write. Closed ones fold into counts: completed, failed, killed, stopped, timed out, blocked, and any status csift does not know, listed under its own name and never counted as completed. A Monitor stays open until its end notice or timeout; a persistent one never returns. Not returned is not proof of running: ctrl+c stops background agents only, not shells or monitors (UNTESTED); a task stopped from the interface, a Monitor timeout, or an agent's teardown leaves no transcript mark, and Claude Code reconciles orphans only at the next session start. Long sessions often carry many dangling tasks (measured: one live session listed 25 open, `csift status @<uuid> --format json | jq 'select(.kind=="verdict").background.open'`).

A shell reaches the background four ways, and only the model's `run_in_background` shows on the call itself: the person pressing ctrl+b on a running call, the call hitting its timeout, or Claude Code moving it aside to deliver a queued message leave only the tool result's sentence (`Command was manually backgrounded by user with ID: ...`, `Command did not complete within its <N>s timeout and was moved to the background (ID: ...)`, `Command was moved to the background (ID: ...) so that a message ...`). The row says which: `entered by ctrl+b`, `entered by timeout` (sometimes with `after <duration>`), `entered to deliver a message`; JSON `entered_by` is `model`, `user`, `timeout` or `deliver-message`, null for an agent or a Monitor (measured: `csift status @<uuid>`). A row whose launch record is missing keeps the receipt's instant and says `launched-at unknown; receipt at ...`. A fifth way, a plugin aborting a turn, writes the ordinary sentence and cannot be told apart.

The LENS decides which open tasks count toward the verdict and toward `wait --until stop`; every task is still listed, an excluded one marked `[ignored: matches --ignore-background <RE>]` or `[ignored: launched before --background-since <WHEN>]`:

- `--background-since WHEN` counts only tasks launched at or after WHEN (the time grammar, or `now`, this command's start);
- `--ignore-background RE` (repeatable) drops tasks whose command or description matches: the dev server, the watcher.

With no lens everything counts, so a session running `npm run dev` never reaches `idle-eot`.

## What the evidence is made of

The registry row is written on transitions, not as a heartbeat, so an old `statusUpdatedAt` means unchanged; its `status` is one of `busy`, `shell` (idle with a background shell open, never a running signal), `idle`, `waiting`. A headless `claude -p` session writes a row too (`entrypoint` `sdk-cli`, `kind` `sdk`) carrying a status, and once its process is gone it reads `stale-dead` like any other (measured: a synthetic row of that shape, `status` `busy` and a dead pid, read `stale-dead`; a row Claude Code later removed leaves the session to its tail alone). The registry covers top-level sessions only, so a subagent target shows its parent session's registry row and pid beside its own tail: its verdict can read `running` from the parent's `busy` while its own `tail` row shows an ended turn, so read that row for the lane itself (measured: `csift status @<agent-id> --format json` gave evidence `registry`, `pid`, `tail`, `background` and verdict `running` on a lane whose tail ended long before). A pending PERMISSION prompt leaves no transcript mark at all; only a current registry row shows it. Process liveness uses `ps` (`/proc` where `ps` lacks the fields) on Unix and PowerShell `Get-Process` or `tasklist` on Windows (docs: `csift status --help`); csift pins its `ps` call to `LC_ALL=C` and `TZ=UTC` as Claude Code pins its own start-time field (docs: SPEC.md section 6.13, at the URL above), and a row written in another pid domain is never probed, which the verdict says (docs: `csift status --help`).

Child lanes read `in-flight` (an unanswered call), `generating` (a tail record under 300 s old and no end of turn yet, docs: `csift status --help`), or `settled` (its tail ended its turn, or the main transcript already holds its completion notice); settled lanes fold into one count. A workflow lane takes its run's word first: its journal line settles it (`result` completed, `failed` failed), else the manifest its ended run wrote (`completed`, or the manifest's own status such as `killed`), unless the lane's newest record is more than a minute newer than that manifest's end, which most likely means a run resumed under its id, and then the lane is read from its own tail like any other (docs: `csift agents --help`, the rule `status` reads; `csift agents` prints the reason as `settled_by`, references/lanes.md). A tasks section reads Claude Code's task list under `<claude-home>/tasks/` and names the store it read (`tasks store: <dir> (via own id | cleared_from root | team file)`, an empty store marked `- empty`), because a `/clear` leaves the list under the id the process started with. When the transcript's last line is a `cost-state` checkpoint, an evidence row names it; that only says nothing was appended after it.

## wait

`csift wait @<uuid> --until COND --timeout SECS` blocks until a condition fires (exit 0, naming it) or the timeout passes (exit 124, with a report of what happened meanwhile) (docs: `csift wait --help`). `--timeout` is required, because a background task can be designed never to return.

| condition | fires when |
|---|---|
| `stop` | the verdict becomes `idle-eot` or `stale-dead`; `idle-background-open` never satisfies it, and neither does the `waiting-hitl` a stale elicitation marker holds, so the wait runs to its timeout (references/hooks.md, Stale markers) |
| `hitl` | the verdict becomes `waiting-hitl` |
| `auq` | the elicitation sidecar gains an unanswered question; without the marker hook installed this never fires (references/hooks.md) |
| `notification[:RE]` | a `<task-notification>` lands in any watched lane |
| `tool:NAME[:RE]` | a call of tool NAME whose serialized input matches |
| `write:PATH_RE[:LINE_RE]` | a Write or Edit whose path matches, and a content line if given |
| `verdict:V` | any of the seven verdicts |

Conditions repeat and the first to fire wins: `csift wait @<uuid> --until stop --until hitl --timeout 600` returns at a stop or at a question, whichever comes first (docs: `csift wait --help`, whose example it is). `wait` records every watched file's size when it starts and fires only on what is written after that, so a condition history already satisfies never fires (that is `search`'s job); a line `csift: watching N file(s) ...` on stderr marks the start, so trigger anything you are waiting for after it. Lanes born during the wait join it. Polling adapts from 200 ms up to 2 s; `--interval MS` pins it (docs: `csift wait --help`).

Before waiting: run `csift status @<uuid>` and read the open background tasks; choose the lens (`--background-since now` is the usual form, `--ignore-background` for known services); set `--timeout` and branch on exit 124; on a timeout read the report's `at exit`, `activity` (what landed while waiting), the open tasks and the last messages. A Bash tool call ends at its own timeout (docs: the Bash tool schema in a Claude Code 2.1.284 session, read 2026-09-30: default 120000 ms, maximum 600000 ms), after which Claude Code moves the command to the background, so run a long `wait` with `run_in_background`, or keep `--timeout` under the call's own limit. Waiting on `@main` from the main lane waits on the session that is blocked running the wait.

## Pending questions: the elicitation sidecar

A pending AskUserQuestion, ExitPlanMode or MCP elicitation may leave no record in the transcript until it is answered: a multi-question AskUserQuestion was observed on disk while its dialog was open, a single-question one only after it closed (docs: `csift status --help`). The marker hook in references/hooks.md records each one to `<session>/elicitations.jsonl`, and `search`, `show`, `verbatim` and `list` merge the unanswered ones as if they were native records: an `agent.tool.use` unit (an MCP one's text reads `MCP elicitation [<server>] (<mode>): <message>`, composed by the marker hook, so it matches a pattern), `(elicitation sidecar)` in place of `L<line>`, JSON `source:"elicitation-sidecar"` and a null `line`, addressable by `--uuid`, and the note `with elicitation sidecar` wherever one was merged. Once answered, Claude Code writes the real record and the marker pairs off, so nothing appears twice; a marker whose question the transcript already shows closed (a rejected plan fires no PostToolUse) is dropped too. A marker whose result never arrives stays pending, and the verdict then reads `waiting-hitl` for good, with a `sidecar` evidence row naming its kind; references/hooks.md, Stale markers, closes one. Tell it from a live question: a question still waiting is the last thing its session did, so the transcript's newest record is at most seconds newer than the marker's instant (the merged unit's `ts_utc`, `csift search '' @<uuid> -t agent.tool.use --format json`), and a session alive at it has a registry row, `waiting`, with its pid alive; a marker the transcript has moved past, or one in a session with no live registry row, is stale (measured: `csift --claude-home FIXTURE status @<uuid>` after an MCP marker whose result was never sent read `waiting-hitl`, evidence `tail` 18,183,813 s old and `sidecar` `pending elicitation: mcp-elicitation`, and the no-registry-row note; the merged unit's `ts_utc` was the marker's own instant, seven months after the tail). references/hooks.md says how the bundled marker hook pairs a result with its marker. Subagent transcripts have no sidecar, and `files`/`recover` do not read it. A marker line the current format cannot read counts as a malformed line (docs: `csift list --help`). Pointing a command at the sidecar file itself is an error, exit 1 (measured: `csift search x <session dir>/elicitations.jsonl` on a synthetic session).

## JSON

`status`: a header `{command, session_id, is_subagent, parent_session_id}`, one `verdict` row `{verdict, evidence[{surface, value, age_secs}], children[{session_id, state, detail}], settled_children, tasks[{id, subject, status, blocked_by}] (null without a tasks store), tasks_completed, tasks_stores[{dir, via}], last_checkpoint, pending, background{open, ignored, completed, failed, killed, stopped, timed_out, blocked, other, scanned_files, tasks[{kind, id, tool_use_id, lane, state, entered_by, timed_out_after_ms, launch_note, description, command, launched_utc, launched_local, age_secs, output_file, output_bytes, output_age_secs, ignored_by}], notes}, last{user, agent}, tail_state, notes}`, and a summary `{verdict}`; evidence surfaces seen: `registry`, `pid`, `tail`, `children`, `background`, and `sidecar` for a pending marker (measured: `csift status @<uuid> --format json`, the last on a synthetic session). Read the verdict with `jq -r 'select(.kind=="verdict").verdict'`. `wait --format json` prints ONE object on exit, no envelope: `{kind:"wait", fired (the condition or "timeout"), verdict, waited_secs, at_exit, activity{records, lanes, tools, thinking, agent_messages, user_prompts, notifications}, evidence, background, last, notes}` (docs: `csift wait --help`).
