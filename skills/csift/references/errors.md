# Errors: what csift printed, why, and the next move

Read this when csift prints an error or a note SKILL.md does not decode, and before a script branches on a csift exit code. For one message, search this file for its first words and read that row. Every message below was copied from a run of csift 0.13.0 (measured: the command in the second column, with an invented or placeholder target); `<...>` marks the part that varies. Parser errors start `error:` and csift's own `csift: error:`; Exit codes, below, gives each its code.

## Contents

- Exit codes
- Arguments and targets
- Selectors, patterns and windows
- Addresses
- Commands with a single target
- Closed pipe
- Notes that are not errors

## Exit codes

A command exits 0 on success and non-zero on failure, and a script tests only that split (measured: the commands under the table). Beneath the split:

| exit | when |
|---|---|
| 0 | success; a FILTER that matches nothing (a zero-match `search`, a `--turn` window past the end on `search`, `stats` or `files`); a `recover` mode other than the default restore on a path the scope never touched, which prints `no recoverable history for <path> in range` or its `--salvage`/`--at` twin; `send` on every verdict, `REFUSED` included (docs: `csift send --help`) |
| 1 | a csift refusal (`csift: error:`); an ADDRESS that names nothing: `show --line`, `--uuid`, `--turn`, a pinned `@id`, `image --id`, `search --for`, and `recover --file` in its default restore mode; a closed pipe on the commands that buffer their output (Closed pipe, below) |
| 2 | a malformed argument or a conflicting flag pair the parser catches (`error:`), including a flag the command does not take and a pattern starting with `-`, except the span flags on `show` and `agents`, which csift refuses itself with exit 1 (Arguments and targets, below); and the `deliver` hook on Stop and SubagentStop when it hands a queued message out on stderr, which is how it keeps the turn from ending (docs: `csift deliver --help`) |
| 101 | a closed pipe on the commands that print row by row (Closed pipe, below) |
| 124 | `wait` reached its `--timeout` (docs: `csift wait --help`) |

Measured: `csift --format json list @<uuid>` exits 2, `csift search x TARGET -t user -T user` exits 1, `csift recover @<uuid> --file /no/such/zz.txt` exits 1 with no mode and 0 with `--coverage`, `--patches`, `--salvage` and `--at @latest`, and `csift search x @<uuid> --turn 90000..99999` exits 0 where `csift show @<uuid> --turn 90..99` exits 1.

## Arguments and targets

| message | from | exit | cause | next move |
|---|---|---|---|---|
| `error: unexpected argument '--format' found` + `tip: 'list --format' exists` | `csift --format json list @<uuid>` | 2 | a subcommand flag before the subcommand | put it after the subcommand; only `--claude-home` and `--cross-compactions` go before |
| `error: invalid value '--bogus' for '[PATH]...': unexpected argument '--bogus' — not a project target; did you mistype a flag? (a UNC-encoded dir is targeted as '@--bogus')` | `csift list @<uuid> --bogus` | 2 | an unknown flag | check `csift list --help` |
| `error: invalid value '' for '[PATH]...': an EMPTY target names nothing` | `csift search P ""` | 2 | usually a command substitution that printed nothing | fix the command that produced the id |
| ``csift: error: '<id>' looks like a session/agent id, not a project path — did you mean '@<id>'? (ids are targeted with an `@` prefix: `@<uuid>` \| `@<uuid-prefix>` \| `@<agent-id>` \| `@<name>@<team>`)`` | `csift list <uuid>` | 1 | an id without its sigil | prefix `@` |
| ``csift: error: `@abc` is too short for an id prefix — a prefix needs 4-11 leading (dashless) hex chars`` | `csift list @abc` | 1 | a 1 to 3 character prefix | give 4 or more hex characters |
| ``csift: error: `@<prefix>` is AMBIGUOUS: 2 ids start with it (<session uuid>, <agent id>). Use more of the id.`` | `csift list @<4 hex>` | 1 | a prefix resolves against session ids and agent ids alike, and two start with it | give more characters of the one you mean |
| ``csift: error: no subagent `<hex>` found under the resolved target(s). A dash-less run of 12 or more hex names an AGENT`` | `csift list @<12 dashless hex>` | 1 | 12 or more dashless hex is read as an agent id | use 11 or fewer, or the dashed form |
| ``csift: error: no session or agent id starts with `<prefix>` under the resolved target(s) — check the prefix, or widen the scope.`` | `csift list @abcd1234` | 1 | a well-formed prefix that starts no id in scope | `csift list --no-subagents` to see what exists |
| ``csift: error: `@<token>` is not a recognized @-target — expected`` and then every target form | `csift list @zzzz` | 1 | an `@` token of no target shape | one of the forms in SKILL.md, Targets |
| `csift: error: no session file found for session id [<uuid>] under the resolved target(s)` | `csift list @<uuid>` for an id not on disk | 1 | a pinned id that names nothing (or a transcript retention deleted) | `csift list --no-subagents` to see what exists |
| `csift: error: no such record(s): uuid <uuid> — ...` | `csift show @<uuid> --uuid <record uuid>` | 1 | a record uuid this transcript does not carry | it may sit in one of the session's subagent transcripts: `csift search '' @<uuid> -t 'user.*' -t 'agent.*' -t 'harness.*' --format json \| jq -r --arg u '<record uuid>' 'select(.kind=="exchange") \| .hits[] \| select(.uuid==$u) \| .refetch_uuid' \| sort -u` prints the `csift show @<agent-id> --uuid <record uuid>` that fetches it, for any record with a leaf (measured: a subagent's `harness.hook.outcome` record, absent from its session's own transcript, was found this way); `--for` answers what is for a record, never the record (measured: 0 matches) |
| `error: the argument '--subagents' cannot be used with '--no-subagents'` | `csift list @<uuid> --subagents --no-subagents` | 2 | contradictory span flags | keep one |
| ``csift: error: `agents` has no subagent-span flag`` / ``csift: error: `show` has no subagent-span flag`` | `csift agents @<uuid> --no-subagents` | 1 | `agents` lists subagents and `show` reads one file | drop the flag; target a subagent as `@<agent-id>` |
| ``csift: error: `csift turns` was RENAMED to `csift verbatim` in v0.5`` | `csift turns @<uuid>` | 1 | a retired subcommand | `csift verbatim`, or `csift show TARGET --turn -3..` to read |
| `csift: error: --additional-context was retired in 0.13.0; select the leaf: -t harness.hook.context` | `csift search --additional-context P` | 1 | a retired flag | `-t harness.hook.context` |
| `csift: error: no Claude Code project dir for <cwd> (looked for <projects>/<encoded>)` | `csift stats .` outside a project | 1 | `.` names a directory Claude Code never ran in | target `@<uuid>` or the project's path |
| ``csift: error: no subagent matched id `<id>` in scope.``, then how to list the ids | `csift agents @<uuid> --agent a9999999999999999` | 1 | an `--agent` id no lane in scope carries | `csift agents @<uuid>` lists the ids |
| `error: the following required arguments were not provided:`, then a line naming `<TARGET>` | `csift show`, `csift status` or `csift wait` with no target | 2 | these take exactly one target | name one (SKILL.md, Targets) |
| `csift: error: verbatim reconstructs ONE conversation's recent turns — name a target: ...` | `csift verbatim` | 1 | no target | `@<uuid>`, `@main` or `@<agent-id>` |

## Selectors, patterns and windows

| message | from | exit | cause | next move |
|---|---|---|---|---|
| `error: invalid value 'foo' for '--label <SELECTOR>': unknown label selector 'foo'. ... Valid: ...` | `csift search P -t foo` | 2 | a misspelt selector | pick from the list it prints, or read references/labels.md |
| the same prefix, then a parenthesis naming the successors, such as `('thinking' is a flat spelling` | `-t harness.meta`, `-t thinking` | 2 | a retired spelling | the successors it names |
| `csift: error: -T excludes every label the -t selection includes ... this filter can never match anything.` | `-t user -T user` | 1 | `-T` cancels all of `-t` | narrow the `-T` |
| `csift: error: --raw IS the machine output (verbatim jsonl lines) — drop --format json` | `csift search P --raw --format json` | 1 | two output forms | keep one |
| ``csift: error: -l prints a plain id stream; with --format json read the summary's `transcript_ids` instead`` | `csift search P -l --format json` | 1 | two output forms | drop one |
| `error: the argument '--raw' cannot be used with '--sessions-with-matches'` | `csift search P --raw -l` | 2 | two output forms | keep one |
| ``csift: error: search's FIRST positional is the regex PATTERN — targets come AFTER it ... To literally match '@foo', escape the @: '\@foo'.`` | `csift search '@foo' @<uuid>` | 1 | a pattern starting with `@` | put the pattern first, or escape `\@` |
| `error: unexpected argument '<...>' found` + `tip: to pass '<...>' as a value, use '-- <...>'` | `csift search '-c is the' @<uuid>` | 2 | a pattern starting with `-` read as flags | escape the dash: `'\-c is the'` (SKILL.md, Search and read) |
| `csift: error: invalid regex pattern: ... look-around, including look-ahead and look-behind, is not supported` (and `backreferences are not supported`) | `csift search '(?=x)y'` | 1 | a construct the linear-time engine lacks | rewrite without it, or narrow with csift and finish with jq |
| `csift: error: --until is before --since (since=<utc>, until=<utc>)` | `--since 1h --until 2d` | 1 | an empty window (both are ages: `2d` is earlier than `1h`) | swap them |
| `csift: error: --line end (3) is before start (9)` | `csift show @<uuid> --line 9..3` | 1 | a reversed range | `3..9` |
| `csift: error: --line range is START..END (e.g. 495..500); a leading -N counts from the end ... got "3-9"` | `--line 3-9` | 1 | a dash range | `3..9` |

## Addresses

| message | from | exit | cause | next move |
|---|---|---|---|---|
| `csift: error: show needs an address: ...` | `csift show @<uuid>` | 1 | no `--line`, `--uuid` or `--turn` | add one: `--line -30..` for the newest records, `--turn -3..` for the latest turns (references/search.md, show in depth) |
| `csift: error: no such record(s): L<n> (file has <N> lines) — ...` | `csift show @<uuid> --line 99999` | 1 | a line past the end of the file | take the line from output again; the file has N lines |
| `csift: error: no such turn(s): t<A>..t<B> — the transcript has <N> turn(s) (t0..t<N-1>)` | `csift show @<uuid> --turn 90..99` | 1 | a turn range wholly past the end | `--turn -k..` for the last k |
| `error: the argument '--line <SPEC>' cannot be used with '--turn <N|A..B|N..|-k>'` | `csift show @<uuid> --line 10 --turn 3` | 2 | two address kinds | `--line` and `--uuid` combine; `--turn` goes alone |
| `csift: error: show targets exactly ONE transcript — '@<id8>' resolves to 2. These are copies of ONE session ...` | `csift show @<id> --line 1` on a duplicated session | 1 | one session under two project directories (references/sessions.md) | target the `*.jsonl` path it names |
| `csift: error: --for L42 addresses a LINE, and a line number names a record in ONE transcript; this scope resolves to <N> transcripts.` | `csift search '' @<uuid> --for L42` | 1 | the target spans subagents | add `--no-subagents`, or address by uuid |
| ``csift: error: --id is per-transcript ... but the scope resolves to <N> transcripts — pin it with `@<uuid> --no-subagents` `` (and the same for `--turn is per-transcript`) | `csift image @<uuid> --id 1`; `csift image @<uuid> --turn -1` | 1 | several transcripts in scope carry images (references/image.md) | add `--no-subagents` |
| `csift: error: --id matched no image: #<N> — present here: #<n> #<n> ...` | `csift image @<uuid> --no-subagents --id 999` | 1 | a handle that does not exist | pick one it lists (references/image.md) |
| `csift: error: --id matched no image: #<N> — the scope carries no images at all. ...` | `csift image @<uuid> --id 1` on a session with none | 1 | nothing in scope carries an image | `csift image @<uuid>` lists what exists |
| `csift: error: no recoverable history for <path> in this scope: it was never Read/Written/Edited here.` | `csift recover @<uuid> --file /no/such` | 1 | wrong path, or wrong session; the default restore mode only, since every other mode prints `no recoverable history for <path> in range` on stdout and exits 0 (measured: the same command with `--coverage`) | `csift files --glob '**/NAME' --by file` to find who touched it |
| `csift: error: cannot fully recover the LATEST <path> from this scope: recovered <k>/<n> line(s) [<ranges>], MISSING [<ranges>]. ...` | `csift recover @<uuid> --file PATH` | 1 | the session saw only part of the file | `--salvage`, `--patches` or `--coverage`, as it says |
| `csift: error: the transcripts in this scope DO hold events for <path> but no line content survives the replay.` | same | 1 | every event was cut by a boundary | `--patches`, `--coverage` or `--at @line:<N>` |
| ``csift: error: --file `<suffix>` names <N> distinct files in scope - replayed as one file it would stitch their edits into one buffer, or keep one file's history and drop the others':`` then one candidate per line (ten at most, then `... +N more (a longer suffix narrows the list)`) and `pass one of these paths, or a longer suffix that names exactly one` | `csift recover @<uuid> --file lib.rs --coverage` on a session writing `a/lib.rs` and `b/lib.rs` | 1 | a path suffix that several distinct files end in, each record path resolved lexically first (references/disk.md, recover); every mode refuses it, `--coverage` included | pass one of the paths it lists; each replays that file alone |
| `csift: error: --slice is 1-based: the first chunk is --slice 1` | `csift verbatim @<uuid> --slices 2 --slice 0` | 1 | chunks count from 1 | `--slice 1` |
| `csift: error: --slice requires the text format (the chunked-injection use case is verbatim text); drop --format json` | `csift verbatim @<uuid> --slices 2 --slice 1 --format json` | 1 | slices are hook text | drop `--format json` |
| ``csift: error: --id #<N> is ambiguous: it names <k> different images in this transcript (Claude Code reuses `#N` across prompts). Pick one by its exact `--id L<line>i<n>`,`` then its other options and one line per image | `csift image @<uuid> --no-subagents --id 1` on a session that reused `#1` | 1 | a paste number shown in more than one prompt | pass the `L<line>i<n>` it lists (references/image.md) |
| ``csift: error: --id: drop the '#' — pass the bare number (`--id 1`)`` | `csift image @<uuid> --no-subagents --id '#1'` | 1 | the `#` of the listing typed into the flag | `--id 1` |
| `csift: error: no channel record of message <id> in lane <uuid> ...` | `csift msg 0123456789abcdef --lane @<uuid>` | 1 | an id that lane never saw | `csift msg --lane @<uuid>` lists what it holds |
| ``csift: error: not a message id: `zz` - a message id is 16 lowercase hex characters`` | `csift msg zz` | 1 | a malformed id | copy the 16-hex id `send` printed |

## Commands with a single target

| message | from | exit | cause | next move |
|---|---|---|---|---|
| `csift: error: status/wait watch exactly ONE session per call; '.' resolves to <N> sessions.` | `csift status .` | 1 | a project target | `@<uuid>`, `@main`, `@<agent-id>` or a `*.jsonl` path (an agent target reads its parent's registry, references/live.md) |
| ``csift: error: show reads exactly ONE transcript per call; got 3 targets: '@<uuid>', '-t', '<leaf>' ...`` | `csift show @<uuid> --turn 0 -t harness.turn.duration` | 1 | `show` takes no `-t`, so the selector reads as two more targets | `csift search '' @<uuid> --no-subagents --turn N -t LEAF` (references/search.md, show in depth) |
| ``csift: error: whoami accepts no target except `@trap:<marker>` (which SUBAGENT am I?), `@main` (the calling top-level session — the default), or a LANE id `@<agent-id>` / `@<Name>@<Team>` (answer for that lane). Got `@<uuid>`. A session uuid is not a whoami question: to inspect a DIFFERENT session, use `csift list @<uuid>` / `csift agents @<uuid>`.`` | `csift whoami @<uuid>` | 1 | a session uuid given to whoami | `csift list @<uuid>` or `csift agents @<uuid>`, as the message says |
| `csift: error: cannot identify the calling session: CLAUDE_CODE_SESSION_ID is not set ...` | `csift whoami` outside Claude Code | 1 | no session variable; the message's own advice to pass `@<uuid>` is refused by the row above | inside a Claude Code lane, `csift whoami @trap:<marker>` typed in your own shell call (references/lanes.md); outside one there is no lane to name, and stdout names the channel way in; `csift list @<uuid>` inspects a session by id |
| ``csift: error: CLAUDE_CODE_SESSION_ID is set to `not-a-uuid`, which is not a session uuid`` | `CLAUDE_CODE_SESSION_ID=not-a-uuid csift whoami` | 1 | the variable was overwritten | unset it, or use `csift whoami @trap:<marker>` from your own shell call |
| ``csift: error: @trap: marker `<m>` is malformed.`` / ``csift: error: @trap: marker `<m>` must be EXACTLY 3 CamelCase words (each: 1 uppercase + >=2 lowercase) before the 4 digits.`` / ``csift: error: @trap: marker `<m>` must END with exactly 4 digits.`` / ``csift: error: @trap: the 4 digits `<dddd>` are a trivial run — pick non-sequential random digits.`` / ``csift: error: @trap: marker `<m>` is the RESERVED documentation example`` | `csift whoami @trap:<m>` with `FooBar5821`, an all-caps word, three digits, `1357`, the help page's example | 1 | a marker outside the grammar | invent a new one in the grammar references/lanes.md gives |
| ``csift: error: @trap: marker `<m>` not found in a `csift` shell command (Bash / PowerShell)`` | `csift whoami @trap:<m>` run by a script, or a first try from the main conversation | 1 | the marker must be in a shell call your own lane made, already on disk | from a subagent, type the command in your own shell tool call; from the main conversation use `@main` (references/lanes.md) |

## Closed pipe

A reader that exits before csift finishes (`| head`, `| head -c N`) turns csift's next write into an error, and which one depends on how the command writes (measured: each command through `| head -c 1` with output larger than the pipe, exit read from zsh `pipestatus`):

| stderr | exit (measured) | commands |
|---|---|---|
| `csift: error: Broken pipe (os error 32)` | 1 | `search` hits (text, JSON, `--raw`), `show` (text, JSON, `--raw`), `files`, `agents`, `image`, `verbatim` |
| `thread 'main' panicked ... failed printing to stdout: Broken pipe (os error 32)` | 101 | `list`, `stats`, `status`, `recover`, `plan`, `show --branch-points`, `search -l`, `search --count-by` (text and JSON) |

The exit is the same with stderr on the same pipe (`csift search needle TARGET 2>&1 | head -c 1`): the error line is lost with the pipe and the reporting commands still exit 1, the panic commands still exit 101 (docs: `csift --help`, exit codes; measured: `csift search '' TARGET --raw`, `--format json`, `csift show TARGET --line 1..3000`, `--raw`, `csift files TARGET`, `csift agents TARGET`, `csift verbatim TARGET --subagents` each exit 1 with stderr on the pipe and with stderr in a file, and `csift list`, `csift stats TARGET`, `csift show TARGET --branch-points`, `csift search '' TARGET --count-by session` each exit 101 both ways; `$pipestatus[1]` in zsh). A stderr note that cannot land because stderr is closed is dropped on a reporting command and the exit stays 0 (measured: `csift search '' TARGET --raw --max-count 1 2>&- | wc -c` and `csift show TARGET --line 1..400 --raw --max-count 2 2>&- | wc -c` exit 0 and still write their stdout).

Output small enough to finish before the reader closes exits 0, which is why `search -c` passes (measured: `csift search '' -c | head -c 1` and the same with `--format json` exit 0 over one projects root). Bound output with `--max-count` on `list`, `search`, `show` and `stats`, and with a narrower target or filter elsewhere, never with `head`.

## Notes that are not errors

| stderr | exit | means |
|---|---|---|
| `csift: 0 matches — a DEFINITIVE absence (exit 0), NOT an error. ...` | 0 | an honest empty; read the rest of the diagnosis (references/search.md) |
| `csift: note: session <id8> is on disk under 2 project directories (byte-identical copies); keeping <path> ... dropping 1 copy ...` | 0 | copies merged (references/sessions.md) |
| `csift: note: session <id8> is on disk under 2 project directories (one a strict byte PREFIX of the other - a copy taken while the session was still being written); keeping <path> ... and dropping 1 copy ...` | 0 | a prefix copy merged onto the longer one |
| `csift: note: session <id8> is on disk under 2 project directories whose contents DIFFER (...) - csift merges neither, so both stay in scope and every count below includes both. ...` | 0 | divergent copies, both kept (measured on a synthetic Claude home holding one session under two directories, one byte changed) |
| ``csift: lane read from $CLAUDE_CODE_SESSION_ID, which names the TOP-LEVEL session <uuid> in every lane - inside a subagent pass `--lane @<your agent id>` `` | the command's own | `msg` without `--lane` (the note names `$CODEX_COMPANION_SESSION_ID` when only that alias is set; measured: `env -u CLAUDE_CODE_SESSION_ID CODEX_COMPANION_SESSION_ID=<uuid> csift msg`); from a subagent it read the parent's lane, so pass `--lane @<the lane that holds the message>` (references/channel.md; measured: `csift msg 0123456789abcdef` from a subagent) |
| ``csift: note: @<uuid> has no compaction — nothing was clipped; for plain reading use `csift show @<uuid> --turn <N\|A..B\|-k..>` (full records, no budget)`` | 0 | `verbatim` on a session with no compaction |
| `csift: note: searching for this uuid as TEXT across the scope; to scope the search TO that session, pass it as a target` | 0 | a uuid in the pattern position with no target |
| `csift: warning: empty pattern with no category/time/turn/session filter matches every exchange in scope — this may emit a lot.` | 0 | an empty pattern with no `-t`, time window, `--turn` or target (measured: `csift search '' -l --max-count 1`); every record in every project matches, so add a target or a filter, or a terminal mode such as `-c` |
| ``csift: note: resolved via env ($CLAUDE_CODE_SESSION_ID), which names the TOP-LEVEL session in every lane - from a subagent this is the PARENT's id, not yours. Lane fields are null under env-only resolution; a first-try `whoami @trap:<marker>` hit proves your true lane.`` | 0 | whoami's env form (references/lanes.md); the parenthesis names the variable that answered, `$CODEX_COMPANION_SESSION_ID` when that alias alone carried the id (measured: `env -u CLAUDE_CODE_SESSION_ID CODEX_COMPANION_SESSION_ID=<uuid> csift whoami`) |
| ``csift: note: @main resolved the TOP-LEVEL session via env ($CLAUDE_CODE_SESSION_ID). If you are a subagent this is NOT your transcript (a subagent's env carries the parent's id); a first-try `@trap:<marker>` hit names yours.`` | 0 | any command given `@main`; the parenthesis names the variable that answered, as above (measured: `csift stats @main --no-subagents --turn -1`, and `csift search zzqqnothing @main` with only `$CODEX_COMPANION_SESSION_ID` set); from a subagent the numbers are the parent session's |
| `csift: note: subagent <agent id> is on disk under one session's sidecar at the built-in and the workflow locations whose contents DIFFER (...) - csift merges neither, so both stay in scope and every count below includes both. ...` | 0 | one agent id at both sidecar locations of one session with different contents; target the `*.jsonl` path of the copy you mean, with `--no-subagents` (measured: `csift agents @<uuid> --shape teammate` on a live session carrying two such lanes) |
| ``csift: lane unknown, sending as the top-level session; pass --from @<your lane id> for an exact sender (a subagent's own id is withheld from its environment - `csift whoami @trap:<marker>` recovers it).`` | 0 | `whoami --to` or `send` without `--from` inside Claude Code |
| `(N malformed line(s) skipped)` in the closing lines | 0 | N lines did not parse; printed on stdout by `search` hits, `show`, `stats`, `files` and `verbatim` |
| `csift: note: N malformed line(s) skipped` | 0 | the same, on stderr, from the `search` modes whose stdout is data (`--raw`, `-c`, `-l`, a text `--count-by`, after its accounting line), when they matched something; on a zero match only the caveat below names the lines; `-c --format json` and `--count-by --format json` carry it as the summary's `skipped_lines` instead |
| `note     N malformed line(s) skipped (among the head/tail lines read — full census: csift stats)` | 0 | a `list` row; `list` reads each file's head and tail only |
| `csift: caveat: N malformed line(s) skipped — the absence is definitive for parseable lines only.` | 0 | a zero-match `search` over a scope holding unparseable lines, printed once in every mode, JSON included |

The four malformed-line forms were measured on a synthetic transcript holding one record pretty-printed across 13 lines (`csift --claude-home FIXTURE search '' @<uuid>`, the same with `--raw`, `list`, a zero-match `search`, `stats`, `show --turn ..`, `files`, `verbatim`), and the count modes on one holding a user line, an assistant line and the line `not json` (`csift --claude-home FIXTURE search hello @<uuid>` with `-c`, `-l`, `--count-by label`, `--raw`, and the first and third again with `--format json`: stdout `1`, the session id, `1  user.message`, the line, then summaries carrying `"skipped_lines":1`, each text run with the note on stderr; a pattern that matched nothing, run plain, with those six forms and with `--format json`, printed the caveat once and no note).
