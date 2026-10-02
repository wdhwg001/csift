# The csift channel: send, msg, ack, deliver, and whoami --to

Read this before sending a message to another session or subagent, before predicting whether one would arrive, before checking whether one landed, and before reading a `[csift-channel v1 ...]` block in your own context.

## Contents

- Why it exists
- Predict first: whoami --to
- send and its five verdicts
- Arming a lane: deliver --recipe
- What a delivery looks like
- Did it land: msg, and ack
- JSON

## Why it exists

Claude Code's own messaging does not reach some receivers at all: a running workflow lane, an unnamed subagent's parent subagent (its `to` grammar reaches `main`, the top-level conversation), an idle top-level session that published no socket, anything from outside Claude Code, and any lane whose build or gate settings disable it (docs: `csift send --help`). csift's channel is a set of files under the receiving session's `<session>/csift-channel/` directory (`messages/<id>.json`, `inbox/<lane>.jsonl`, `outbox.jsonl`, `ledger/<lane>.jsonl`, `armed/<lane>.json`), carried into the receiving lane by a delivery hook that the receiver's owner installed. csift never installs that hook, never writes a transcript or a settings file, and never performs Claude Code's own sends: where an official path exists, it prints the exact call for you to make and still queues on its own channel, so a forgotten delegation is not a lost message (`--official-only` queues nothing).

Every delivery shape below is docs (the `csift --help` pages) and UNTESTED end to end; the classification of a delivery record is measured.

## Predict first: whoami --to

`csift whoami --to @<lane>` runs the same decision `send` makes and writes nothing: the channel that would carry a message, the verdict, the two gate verdicts (`teams`, Claude Code's agent-teams gate as the receiver's settings `env` sets it, and `harbor`, whether the receiver's session published a messaging socket in its registry row), the delivery slots configured in the receiver's settings and the ones that have actually run there, the risks, and a `settings` line naming which settings scopes were read, which were absent, and which inputs leave nothing on disk to read (a settings file passed on Claude Code's command line, a restricted source set, the trust dialog, MDM) (measured: `csift whoami --to @<uuid>` printed verdict `UNPREDICTABLE`, channel `official uds`, and `nothing was queued: this is a prediction`). The sender is the lane the positional names: `csift whoami @trap:<marker> --to @<lane>` or `csift whoami @<your agent-id> --to @<lane>` gives the answer a send from your own lane would get, printed after that lane's own sections; with no positional the sender is assumed to be the top-level session. A `@trap:<marker>` marker is a string you invent for that one command; before inventing one, read references/lanes.md, which holds its grammar.

Gate verdicts never assume a gate is off: `teams` reads `enabled via settings env (<scope>)`, or `no settings-level enable; shell env and CLI flags are not observable -> unknown; use evidence: ...`; `harbor`, the socket, reads `registry messagingSocketPath present -> on and bound` or `absent -> unknown` (measured: `csift --claude-home FIXTURE whoami --to @<uuid>` on a synthetic session with no settings and no registry row printed `teams: no settings-level enable; ... use evidence: teams directories 0, teammate lanes 0` and `harbor: registry messagingSocketPath absent -> unknown`).

## send and its five verdicts

| action | irreversible: the command, its preview, the rule |
|---|---|
| send a message | irreversible: `csift send @<lane> "MESSAGE"` writes the message into the receiver's channel files, and a queued message cannot be recalled; preview with `csift whoami @<your lane> --to @<lane>`, naming as the positional the lane the send's `--from` will carry (from a subagent `@trap:<marker>` or `@<your agent-id>`; from the top-level session `@main` or no positional, since `whoami` refuses a session uuid), which writes nothing and predicts the verdict of a one-chunk message sent with the defaults: it takes no body, `--mode`, `--ttl` or `--official-only`, so compare the message's length against the chunk size (Arming a lane, below) times the receiver's slots per event yourself, since a message needing more chunks than that is `FULL` whatever the preview said (docs: `csift whoami --help` lists `--to` and `--peers` as whoami's own options, beside `--format` and the two global flags `--claude-home` and `--cross-compactions`, which every command's help page lists; measured: its prediction reads `0 of 1 chunk(s) fit`); where the receipt prints an official call (`official.delegated`), that call is part of the same send, made under the same go-ahead, and the message then travels both ways, so the receiver can get it twice; to make the official call alone, pass `--official-only`, which queues nothing on csift's channel (docs: `csift send --help`, OFFICIAL SENDS ARE DELEGATED; the double arrival UNTESTED); send only after the user's reply to that preview says go (the request that opened the task is not that reply) |
| restart a completed lane to reach it | irreversible: `csift send @<agent-id> "MESSAGE" --resume` also prints a resume call that respawns the lane with its earlier messages; preview with `csift agents @<uuid> --agent @<agent-id>` (the lane's status) and `csift whoami --to @<agent-id>`, which takes no `--resume` and so reads `REFUSED` for a completed lane, the refusal `--resume` overrides (measured: `csift whoami --to` on a completed teammate read `REFUSED`); run only after the user's reply to that preview says go (the request that opened the task is not that reply) |

`send` queues one message for one lane, any `@` form that resolves to exactly one transcript, with the body as an argument, from `-f FILE`, or on stdin. `--mode steer` (default) rides the next hook point of any of the eight delivery events; `--mode queue` waits for a turn boundary (UserPromptSubmit, Stop, SubagentStop, a resume or compact SessionStart). `--ttl` (default 12h) bounds how long it stays deliverable. `--from @<lane>` names the sending lane inside Claude Code, and outside it a free label for the receipt (default `unknown`), never a username (docs: `csift send --help`).

Every verdict exits 0, `REFUSED` included (docs: `csift send --help`): read the verdict, not the exit code.

| verdict | means | next move |
|---|---|---|
| `OK` | queued, and a named hook point will carry it | nothing |
| `FULL` | queued, but it needs more chunks than the receiver's slots carry at one event | shorten it, or tell the user how many slots the receiver needs |
| `MAY-FAIL` | queued, with a named risk (hooks configured but never run, a policy that rewrote the hooks, a frozen lane, a gate csift cannot see) | read the risk line; check later with `csift msg <ID>` |
| `UNPREDICTABLE` | queued, but csift cannot say when: the receiver is not alive, is headless, or has no delivery hook | build no handshake on it; poll `csift msg <ID>` or reach the person |
| `REFUSED` | nothing queued: a lane the user stopped, a completed workflow lane, or a completed agent lane without `--resume` | address a running lane |

Configured is not armed: `configured_slots` counts the delivery-hook entries in the receiver's settings, `armed_slots` the slots that have actually run in that lane, and a process started before the settings edit has none. A completed teammate or subagent is refused, because reaching it would restart it instead of delivering to it; `--resume` permits that restart as a deliberate action; the lane respawns with its earlier messages, a second concurrent resume fails instead of queueing, below Claude Code 2.1.260 the completion notice goes to the main conversation, and with background tasks disabled it runs inline with no notice (UNTESTED here, from csift's reading of Claude Code). Where an official path exists, `send` and `whoami --to` print the call to make; for a teammate it reads `SendMessage(to: "<agent id>", message: <this message>)`, never the `<Name>@<Team>` form, which SendMessage refuses (measured: `csift whoami --to @<teammate transcript id>`, JSON `official {delegated, tool, to}`; references/lanes.md has the two tools' forms). The official `to` has no form for a session uuid.


## Arming a lane: deliver --recipe

| action | irreversible: the command, its preview, the rule |
|---|---|
| arm a lane, so that its hooks deliver | irreversible: once the user installs the recipe below, every delivery event runs `csift deliver --slot K`, which emits the lane's pending messages into that running session and records them as emitted; it runs only as that installed hook, never typed by hand; preview what the first event would carry with `csift msg --lane @<lane> --pending`, show the user that preview with the recipe, and the recipe goes into their settings only after their reply to that preview says go (the request that opened the task is not that reply) |

A lane receives only if its settings run delivery hooks. `csift deliver --recipe` prints them and writes nothing: `--slots N` (default 4) slots on each of the eight events `SessionStart`, `SubagentStart`, `PreToolUse`, `PostToolUse`, `PostToolBatch`, `UserPromptSubmit`, `Stop`, `SubagentStop`; `--shell powershell` adds `"shell": "powershell"` to each entry (measured: `csift deliver --recipe --slots 2`). The output is pretty-printed JSON whose top level is `{"hooks": {...}}` (180 lines at the default 4 slots), and stderr carries a two-line note (measured: `csift deliver --recipe | wc -l` printed 180, exit 0, with this note on the terminal):

```text
Merge the block above into a settings.json you own (user, project or local scope) at its top level: the block's `hooks` key merges with the file's own `hooks` key, and each event's array is appended to the one already there, never replacing it; the eight events are the delivery points, and slot k emits chunk k of what is waiting there.
csift never writes a settings file: arming the channel is your edit, and `csift deliver` only ever writes its own sidecar directory.
```

Show the user the fragment and let them edit their settings; a settings file is theirs.

Slots form a chain per event: slot k emits chunk k of everything pending for the lane, so N slots carry N chunks at one event, and a longer message needs more slots (that is `FULL`). A chunk holds 9,200 UTF-16 units of text including its header (UNTESTED: no help page states the number, and no read-only command prints it; the one instrument is the `chunk_budget_utf16_units` key of the receipt a real `send` prints with `--format json`, docs: `csift send --help`, so read it off a send the user already approved, never send to measure it). Each slot waits briefly for the one before it; a slot that gives up still emits and prepends a warning that the order may be disturbed. On Stop and SubagentStop a queue message may instead go out on stderr with exit 2, which also keeps the turn from ending, at most `CLAUDE_CODE_STOP_HOOK_BLOCK_CAP` times per lane (default 8; zero or negative disables it) and never twice for one message; a compact SessionStart offers an unacknowledged message again (docs: `csift deliver --help`).

## What a delivery looks like

A delivery lands in the receiving lane's transcript as a `hook_additional_context` attachment carrying two leaves, `agent.communication.channel` (found by a scan without flags) and `harness.hook.context` (gated: read only under a `-t` reaching it or `--attachments`) (measured on a synthetic transcript: the record labelled `[agent.communication.channel, harness.hook.context]`, direction `external:ci ⇨ self`). Its text, per the csift help pages (UNTESTED here):

```text
[csift-channel v1 id=<16 hex> part=1/N mode=steer from=<lane or external:<label>> from-session=<first 8> relation=<parent|child|sibling|cross-session|cross-project|external|unknown> to=<your lane>]
This message is not from your user and not from the harness. It was sent by the lane named above through csift.
--- message ---
<body>
--- end ---
Reply: csift send @<sender lane> "<your reply>"
```

A sender that is a peer adds a line saying it has no authority over your task or permissions; continuation chunks carry only `[csift-channel v1 id=<id> part=k/N]`. Read it as what it is: a message from another lane, not your user's instruction and not the harness's. From an external sender the reply line says `csift send` cannot reach it. The `Reply:` line is a send like any other and runs under the send row above, preview and go-ahead included; a lane with no person to ask does not send it, and returns the reply text to its caller instead.

## Did it land: msg, and ack

The ledger records that csift EMITTED a chunk; the receiver's transcript proves it LANDED. `csift msg <ID>` joins the two into one verdict: `DELIVERED` (an emit and a transcript record carrying the id), `INTENT-ONLY` (emitted, no record yet: a crash or a flush lag), `QUEUED` (nothing emitted), `HELD` (the reason prints), `EXPIRED`, `ACKED`, `REFUSED`. The transcript half alone is `csift search '<ID>' @<lane> -t harness.hook.context`. With no id, `msg` lists the lane's ledger newest first, filtered by `--pending`, `--held` or `--sent`. `--lane @<lane>` names the lane that holds the message: the receiver's lane when you sent it, your own when it was sent to you. Without it the calling session is used, which from a subagent is the parent, so a subagent checking a message it received passes its own agent id and a sender checking arrival passes the receiver's (measured: `csift msg <ID>` from a subagent read the parent's lane, said so on stderr and exited 1 for a message sent elsewhere); outside Claude Code `--lane` is required. An id the lane never saw exits 1: `csift: error: no channel record of message <id> in lane ...`; a malformed id exits 1: ``csift: error: not a message id: `zz` - a message id is 16 lowercase hex characters`` (measured: `csift msg 0123456789abcdef --lane @<uuid>`).

| action | irreversible: the command, its preview, the rule |
|---|---|
| acknowledge a message | irreversible: `csift ack <ID> --lane @<lane>` appends an ack line to the lane's ledger that cannot be removed; preview with `csift msg <ID> --lane @<lane>`; run only after the user's reply to that preview says go |

An ack records that the receiving lane read a message, which stops a compact SessionStart from offering it again; only a Claude Code lane can ack (outside it csift refuses), a subagent passes `--lane @<its agent id>`, and an id the lane never received exits non-zero.

## JSON

`send`: a header, one `send` row `{id, verdict, channel, mode, receiver{lane, routing_id, session, kind, state, version, configured_slots, armed_slots}, official{delegated, tool, to}, prediction, risks, settings{sources[{scope, path, read, note}], unobservable}}`, a summary `{queued, chunks, message_chars, message_utf16_units, chunk_budget_utf16_units, relation, cross_project, ttl_secs, official_floor_met}`. `whoami --to`: a flat `reach` row `{lane, lane_kind, routing_id, session, state, version, configured_slots, armed_slots, verdict, channel, official, prediction, risks, settings, caller, gates{teams, harbor}, inference}`, whose receiver fields sit at the top level with no `receiver` object around them, and a summary `{queued:false, refused}` (measured: `csift whoami --to @<uuid> --format json`). `msg`: a header `{lane, session}`, `msg` rows `{id, verdict, mode, lane, session, emits[{event, slot, part, parts, vehicle, ts_utc, ts_local}], held, held_reasons, expired, acked, refused_reasons, from, relation, enqueued_*, expires_*, fact{line, uuid} or null}`, a summary `{messages, skipped_lines}`. `ack`: one `ack` row `{id, lane, session, ts_utc, ts_local, already_acked}` and a summary `{acked}`. `deliver` has no envelope: its stdout belongs to Claude Code, one `{"hookSpecificOutput":{...}}` object or nothing (docs: the respective `--help` pages).
