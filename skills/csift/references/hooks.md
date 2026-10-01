# Hooks: three scripts a user can install

Read this before handing a user any hook built on csift: re-injecting the turns a compaction clipped, redirecting a failed TaskStop aimed at a teammate, or recording pending questions so csift can see them.

## Contents

- Installing: the user's edit, never yours
- What each installed hook writes or injects
- Testing a script by hand
- Checking that a hook ran
- compact-slice.sh: re-inject clipped turns after a compaction
- taskstop-teammate.sh: redirect a failed TaskStop
- elicitation-marker.sh: record pending questions

## Installing: the user's edit, never yours

Each script lives in this skill's `scripts/` directory: the directory holding this file's `references/` folder, plus `scripts/NAME.sh`. `${CLAUDE_SKILL_DIR}` is a skill-content variable, not one a settings hook expands (UNTESTED), so write the absolute path into the fragment (the path you read this file from tells you where the skill is). Every script is run as `bash '/ABSOLUTE/PATH/scripts/NAME.sh'`, the path single-quoted inside the command string, and reads the hook payload on stdin. The quotes matter because the command string runs through a shell: an unquoted path holding a space splits into two words, bash exits 127 and the script never starts (measured: a fragment's command string run with `/bin/sh -c` from a skill directory named `space dir`: quoted, exit 0 and the slice's JSON; unquoted, exit 127 and `bash: <path up to the space>: No such file or directory` on stderr); a path holding a `'` writes it as `'\''`.

What a script needs, and what it does without it: all three need `jq` on the hook's PATH, and compact-slice.sh and taskstop-teammate.sh also need `csift` (elicitation-marker.sh runs none). A script whose tool is missing does nothing and says nothing: it exits 0, prints no byte and writes no file, exactly as it does for a payload it does not handle, so a broken install never blocks a session and never announces itself either (measured: each script run as `env PATH=<a directory without jq> bash scripts/NAME.sh`, then with a PATH holding jq but not csift, on a payload it handles, and on empty stdin, non-JSON stdin, a payload missing its field and a session id carrying a quote: exit 0 and 0 bytes on stdout each time). A hook runs with the PATH Claude Code gives it, which need not be your shell's, so after installing one, check that it ran (next section).

Give the user the fragment and the settings file it goes in (`~/.claude/settings.json` for every project, or `$CLAUDE_CONFIG_DIR/settings.json` when that variable is set, which the scripts and csift honour too; `.claude/settings.json` or `.claude/settings.local.json` in a project); each fragment below is a top-level `{"hooks": {...}}` object, merged at the file's top level: its `hooks` key merges with the file's own `hooks` key, and each event's array is appended to the one already there, never replacing it (the same rule `csift deliver --recipe` prints for its fragment); pasted inside the file's `hooks` object it would nest `hooks` twice. A lane whose process started before the edit may not run the hook until it restarts (UNTESTED).

A hook that needs a fact about its own session (the last prompt's time, an id, an activity marker) can ask csift from the hook instead of keeping a state file that drifts from the transcript. One timing trap: a UserPromptSubmit hook looking for the PREVIOUS prompt may or may not already see the current one, because the main conversation records a message 1 to 3.4 s after it is sent (docs: `csift --help`, the @trap paragraph); so drop user records newer than 4 s, a margin over the 3.4 s the help states, and take the newest remaining: `csift search '' "@$SESSION_ID" --no-subagents -t user.message --until 4s --max-count -1 --format json`, with the session id from the payload's `session_id` (measured: that command exited 0 with one exchange row whose hits are that turn's person records).

## What each installed hook writes or injects

Installing a fragment is a change to the user's settings that keeps acting on every event it serves, in every session the settings reach, until it is removed, so each one is handed over the way a writing command is run:

| fragment | irreversible: what the installed hook writes or injects, its preview, the rule |
|---|---|
| compact-slice.sh | irreversible: after every compaction, each registered copy injects one slice of `csift verbatim` into the session's context, at most 9,000 characters of reconstruction under a header line the script adds, so a slice runs past 9,000 UTF-16 units (the sizes are measured in the compact-slice.sh section below), and Claude Code records each as a `harness.hook.context` attachment in the transcript; preview with `csift verbatim @<uuid> --slices 4 --slice I --window 9000 --until T` for I from 1 to 4, T being the newest compaction summary's instant from `csift search '' @<uuid> --no-subagents -t harness.compaction.summary --format json | jq -r 'select(.kind=="exchange") | .hits[] | select(.label=="harness.compaction.summary") | .ts_utc' | tail -1`, which prints what each copy injected at that compaction, minus the header line, and writes nothing; without `--until` a session that kept working after its compaction previews its later turns instead (measured on one such session: slices covering lines 8,617 to 9,341 without it, and 3,424 to 4,960 around the summary at line 4,628 with it); the fragment goes into the user's settings only after their reply to that preview says go (the request that opened the task is not that reply) |
| taskstop-teammate.sh | irreversible: after every failed TaskStop aimed at a teammate, it injects one context text naming the teammate's status and, for a running one, the call that can still stop it (measured: 467 to 951 bytes of hook output over the TaskStop payloads the taskstop-teammate.sh section below lists, on synthetic homes); it writes no file; preview with `csift agents @<uuid> --shape teammate --format json`, the rows the hook reads; the fragment goes into the user's settings only after their reply to that preview says go (the request that opened the task is not that reply) |
| elicitation-marker.sh | irreversible: every AskUserQuestion, ExitPlanMode and MCP elicitation appends to `<transcript path without .jsonl>/elicitations.jsonl` one line when it opens, the question's whole `tool_input` plus about 700 bytes, and one of about 280 bytes when it closes (measured on a synthetic home: 832 bytes for a 120-byte question, 281 for its close; 697 and 277 for an MCP pair), a file csift then reads on every `search`, `show`, `verbatim`, `list` and `status` of that session; preview with `csift list @<uuid> --no-subagents --format json`, whose `path` is the transcript the sidecar sits beside and `sidecar_present` whether it exists, then `ls -l` of the sidecar path; the fragment goes into the user's settings only after their reply to that preview says go (the request that opened the task is not that reply) |

The scripts have no dry-run mode; the previews above are the read-only commands that show what they would act on.

## Testing a script by hand

Run a script by hand only against a scratch Claude home (`CLAUDE_CONFIG_DIR=<a scratch directory holding a copy of a transcript>`) or with a payload whose `session_id` names no transcript and whose `transcript_path` names no file, for which elicitation-marker.sh writes nothing (measured: `printf '{"hook_event_name":"PreToolUse","tool_name":"AskUserQuestion","session_id":"<a uuid no transcript carries>","tool_use_id":"toolu_q"}' | CLAUDE_CONFIG_DIR=<scratch home> bash scripts/elicitation-marker.sh` exited 0, printed nothing and added no sidecar line). A payload naming a real session plants a real pending marker beside its transcript, and csift then reads that session as `waiting-hitl` until the marker is closed (Stale markers, below).

## Checking that a hook ran

A hook that exited quietly and one that never ran look the same from inside a session, so after the event it serves, look for its output:

| script | after | the check | ran when |
|---|---|---|---|
| compact-slice.sh | a compaction | `csift search 'Verbatim turns the compaction summary clipped' @<uuid> --no-subagents -t harness.hook.context -c` | the count is above 0 |
| taskstop-teammate.sh | a failed TaskStop aimed at a teammate | `csift search 'TaskStop did not resolve' @<uuid> --no-subagents -t harness.hook.context -c` | the count is above 0 |
| elicitation-marker.sh | the first AskUserQuestion, ExitPlanMode or MCP elicitation | `csift list @<uuid> --no-subagents --format json`, its `session` row | `sidecar_present` is true |

The `-t harness.hook.context` is required, because the injected text is a gated leaf a scan without it never reads (measured: `csift --claude-home FIXTURE search 'TaskStop did not resolve' @<uuid> --no-subagents -t harness.hook.context -c` printed 1 on a synthetic transcript holding one such record, and 0 without the `-t`; the compaction check printed 7 on a live session whose own compaction hook opens its text with those words; the marker check is in the elicitation section below).

## compact-slice.sh: re-inject clipped turns after a compaction

After a compaction, N copies of this hook each inject one slice of `csift verbatim @<session> --slices N --slice i --window 9000` (references/verbatim.md), so the model gets back the original wording beside the summary. Each slice opens with the line `Verbatim turns the compaction summary clipped, part i of N.`, because same-event hooks can finish in any order; a slice past the end injects nothing (measured: `printf '{"source":"compact","session_id":"<uuid>"}' | bash scripts/compact-slice.sh 1 2` on a compacted synthetic session printed JSON whose text opened `... part 1 of 2.`, and slice 2 printed nothing). The injected text is recorded in the transcript as a `harness.hook.context` attachment. `--window 9000` bounds the reconstruction only: the script puts its 238-unit header line (the line above plus two sentences on reading the parts, and its newline) on top, so a slice runs past 9,000 and stays under Claude Code's 10,000-unit cap for a hook's additional context (docs: `csift verbatim --help`, `--window`; measured: `printf '{"source":"compact","session_id":"<uuid>"}' | bash scripts/compact-slice.sh I 4` for I from 1 to 4 on one compacted session, its `additionalContext` 9,217, 9,194, 9,193 and 6,873 UTF-16 units, the bare `csift verbatim` slice 1 8,980 with its trailing newline, which the script drops).

```json
{"hooks": {"SessionStart": [{"matcher": "compact", "hooks": [
  {"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/compact-slice.sh' 1 4"},
  {"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/compact-slice.sh' 2 4"},
  {"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/compact-slice.sh' 3 4"},
  {"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/compact-slice.sh' 4 4"}
]}]}}
```

The second argument must equal the number of copies registered. A third argument sets the window in characters (the script's default is 9000); lower it for text heavy in emoji, which count two units each.

## taskstop-teammate.sh: redirect a failed TaskStop

When a TaskStop call fails, the hook asks `csift agents @<session> --shape teammate --format json` whether the failed id is a teammate's agent id, name or `<Name>@<Team>`, and reads that teammate's `status`. An ended teammate (`completed`, `failed`, `killed`) gets `it has already ended; no message can stop it` and no advice, because a SendMessage addressed to its agent id would resume it. A running one gets the call that can still work, in the form the skill teaches for writers: `irreversible:`, the preview `csift agents @<session> --agent @<agent id>`, and the rule that the call waits for the user's reply to that preview. After a failed agent id that call is TaskStop by the name or `<Name>@<Team>` (Claude Code 2.1.198 and later) or SendMessage to the bare name with a `shutdown_request`; after a failed name or `<Name>@<Team>` it is the SendMessage alone, because the failed form is the one TaskStop takes and a second TaskStop fails the same way. SendMessage's `to` is never a form carrying `@`. When two teammates in the session share the name, the name forms (`TaskStop "Name"`, `TaskStop "Name@Team"`, SendMessage to the name) cannot pick one, and TaskStop takes no other form: a failed name or `Name@Team` gets both agent ids with their statuses and the instruction to ask the user which one, and a failed agent id of a running one gets, under the same `irreversible:` rule, the one call that picks it, SendMessage with `to` set to that agent id and a `shutdown_request`. An id that names no teammate injects nothing. Measured: `printf '{"tool_name":"TaskStop","session_id":"<uuid>","tool_input":{"task_id":"<id>"}}' | bash scripts/taskstop-teammate.sh` on synthetic homes, with `<id>` each of an ended teammate's three ids, each of a running one's (the TaskStop forms named after its agent id and absent after its name and its `Name@Team`), a name two teammates share, the agent id of a running teammate whose name another teammate shares (the agent-id SendMessage named and no TaskStop advised), and `b1234abcd`. Claude Code records a PostToolUseFailure hook's `additionalContext` as delivered to the model (measured: `csift search '' -t harness.hook.context --format json` over one projects root found 652 PostToolUseFailure records, all `delivered:true`).

```json
{"hooks": {"PostToolUseFailure": [{"matcher": "TaskStop", "hooks": [
  {"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/taskstop-teammate.sh'"}
]}]}}
```

## elicitation-marker.sh: record pending questions

A pending AskUserQuestion, ExitPlanMode or MCP elicitation can be absent from the transcript until it is answered (references/live.md). This hook appends a marker when one opens and another when it closes to `elicitations.jsonl` in the session's sidecar directory, the file csift merges; it prints nothing. That directory is the transcript's path without `.jsonl`: the transcript is the payload's `transcript_path` when it ends in `<session-uuid>.jsonl` and names an existing file (one that names no file writes nothing and creates nothing), else the one `<session-uuid>.jsonl` under `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects/*/`, and nothing is written when there are two. It writes only when the payload names one of the events below, a uuid-shaped session, and the key that pairs the two markers: the tool call id, or the MCP `elicitation_id`, or the MCP `mcp_server_name` when the elicitation carries no id. Subscribing PostToolUseFailure matters, because a rejected question fires no PostToolUse; csift also drops a marker whose question the transcript already shows closed.

An opening marker is shaped like the record Claude Code writes once the question closes (an assistant `tool_use` holding the call's `tool_input`, or a `system` record whose `content` reads `MCP elicitation [<server>] (<mode>): <message>`) and carries `csift:"elicitation-marker-v1"`, `csiftPhase`, `csiftKind`, `csiftKey` (the pairing key), `sessionId`, `csiftHookEvent` and `csiftTranscriptPath`, plus `csiftMcpServer` and `csiftElicitationId` on an MCP marker; a closing marker is `{"type":"csift-elicitation-resolved", uuid, timestamp, sessionId, "csift":"elicitation-marker-v1", "csiftPhase":"resolved", csiftKind, csiftKey}`. The hook payload itself is not copied (measured on a synthetic home with 1 MB payloads: an AskUserQuestion whose 1 MB sat in its question wrote a 1,048,802-byte line, against 2,096,822 when the payload was copied too, and an MCP elicitation whose 1 MB sat in another payload key wrote 707 bytes, against 1,048,607).

An MCP result closes the open marker whose key is its `elicitation_id`; when no open marker has that key, or the result carries no id, it closes every marker of its `mcp_server_name` still open in the sidecar, each under that marker's own key, so a result closes its question whichever side carried the id. What stays open is a marker whose result never arrives (Stale markers, below). Measured, each pairing on a synthetic compacted session, `csift status @<uuid> --format json` read after the pending marker and after the result: id then the same id, id then no id, no id then no id, and no id then an id each read `waiting-hitl` then `idle-eot`; two open markers of one server and a result naming one closed that one alone, and a result naming an id no marker has closed both. After the PreToolUse marker of an AskUserQuestion `csift list @<uuid> --format json` read `sidecar_present:true` with 1 pending, and after the PostToolUse marker 0 pending (measured).

The event names and payload keys the script reads (`hook_event_name`, `tool_use_id`, `session_id`, `transcript_path`, and `elicitation_id`, `mcp_server_name`, `message`, `mode` for an MCP elicitation) are UNTESTED against a live Elicitation event; the MCP ones occur as literals in the Claude Code 2.1.285 build (measured: a byte count of `"Elicitation"`, `"ElicitationResult"`, `elicitation_id`, `mcp_server_name`, `hook_event_name`, `transcript_path`, each above zero).

```json
{"hooks": {
  "PreToolUse": [{"matcher": "AskUserQuestion|ExitPlanMode", "hooks": [{"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/elicitation-marker.sh'"}]}],
  "PostToolUse": [{"matcher": "AskUserQuestion|ExitPlanMode", "hooks": [{"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/elicitation-marker.sh'"}]}],
  "PostToolUseFailure": [{"matcher": "AskUserQuestion|ExitPlanMode", "hooks": [{"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/elicitation-marker.sh'"}]}],
  "Elicitation": [{"hooks": [{"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/elicitation-marker.sh'"}]}],
  "ElicitationResult": [{"hooks": [{"type": "command", "command": "bash '/ABSOLUTE/PATH/scripts/elicitation-marker.sh'"}]}]
}}
```

With this hook installed, `csift wait --until auq` can fire and a `list` row's `sidecar_present:false` means no question was ever recorded for that session.

### Stale markers

A marker whose result never arrives keeps its session at `waiting-hitl` for good, because a person outranks the end of a turn (the close row below tells such a marker from a live question), so `csift wait @<uuid> --until stop` never fires and exits 124 at its `--timeout` (`stop` fires only at `idle-eot` or `stale-dead`, docs: `csift wait --help`; `status` read `waiting-hitl` on such a marker, measured; the exit 124 UNTESTED). The script closes an MCP marker by its server when a result carries no id; a marker no result will close is closed by hand with the closing line the script writes, keyed by the marker's own `csiftKind` and `csiftKey`:

| action | irreversible: the command, its preview, the rule |
|---|---|
| close a stale marker | irreversible: `printf '%s\n' '{"type":"csift-elicitation-resolved","uuid":"<a fresh uuid>","timestamp":"<now, ISO 8601 UTC>","sessionId":"<session uuid>","csift":"elicitation-marker-v1","csiftPhase":"resolved","csiftKind":"<its csiftKind>","csiftKey":"<its csiftKey>"}' >> '<transcript path without .jsonl>/elicitations.jsonl'` appends a close the file keeps, and the session's verdict moves off `waiting-hitl`; preview with `jq -c '{csiftPhase, csiftKind, csiftKey, timestamp}' '<transcript path without .jsonl>/elicitations.jsonl'`, where an open marker is a `pending` key with no `resolved` line, `csift list @<uuid> --no-subagents --format json` (its `pending_elicitations`), and `csift status @<uuid> --format json` with `csift search '' @<uuid> --no-subagents -t agent.tool.use --format json | jq -c 'select(.kind=="exchange") | .hits[] | select(.source=="elicitation-sidecar") | {ts_utc, tool_name}'`, the marker's own instant; a question still waiting is the last thing its session did, so the marker is stale, and is closed, only when the status evidence carries no `registry` row with `pid alive`, or the transcript holds records newer than the marker's instant by more than seconds (`tail`'s `age_secs` against the marker's `ts_utc`); run only after the user's reply to that preview says go (the request that opened the task is not that reply) |

Measured on a synthetic home: an MCP marker with no result read `waiting-hitl` with 1 pending; after the appended line `status` read `idle-eot` and `pending_elicitations` was empty.
