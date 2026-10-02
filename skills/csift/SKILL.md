---
name: csift
description: "Reads, searches and counts Claude Code session transcripts (the .jsonl files under ~/.claude/projects, subagents too) with the csift command-line tool, and ships hooks that re-inject compacted turns or record pending questions. Use when asked what a past or running session said, did, edited, spent or is doing now; when about to grep, jq or script over a transcript jsonl (a type==\"user\" filter overcounts the person's messages), count how many messages the person typed, tool calls or tokens, or claim something was never done or decided; to rebuild a lost file or plan from what a session wrote, extract pasted images, restore turns a compaction summary lost, find a subagent's own id when CLAUDE_CODE_SESSION_ID names the parent, wait on or message another session or subagent; or when csift prints \"unknown label selector\", \"show needs an address\", \"looks like a session/agent id, not a project path\" or \"0 matches — a DEFINITIVE absence\". Not for semantic or embedding search: csift matches regular expressions only."
---

# csift

csift reads Claude Code session transcripts (`~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`, their `subagents/` transcripts and sidecar files) and answers with labelled records, counts and addresses you can re-run. The failure it prevents: a hand-written `grep`, `jq` or script over that format returns a plausible answer that is silently wrong, for the reasons law 1 lists.

Surface: csift 0.13.0 (must equal the output of `csift --version`, which prints `csift 0.13.0`). `csift <command> --help` is the authority on flag names and syntax, and when this skill and a help page disagree on those, the help page wins; on a behaviour this skill measured, the measured line wins, because a help page describes intent and a probe describes the binary. When a command you were confident about errors, compare `csift --version` with this line before anything else. Provenance labels: "measured" (with the command that ran on this surface), "docs" (a help page of this surface, a section of csift's SPEC.md at https://github.com/wdhwg001/csift/blob/v0.13.0/SPEC.md, or a Claude Code tool schema as a session of the named Claude Code version loaded it), UNTESTED (not run).

Invocation: `csift <subcommand>` from any directory, with `csift` on PATH. In commands `TARGET` is any form in the Targets table, `P` a regex pattern and `PATH` an absolute file path. A lane is one thread with a transcript of its own: a top-level session, or one of its subagents; on a command that spans subagents (Targets names them), `@<uuid> --no-subagents` keeps the session's own lane. If `command -v csift` prints nothing, csift is not installed: give the user `cargo install --git https://github.com/wdhwg001/csift --tag v0.13.0 --locked` to run (Rust 1.89 or later, docs: the README and `rust-version` in Cargo.toml of that repository at the v0.13.0 tag; the install UNTESTED), not crates.io, whose release can lag the tag (`cargo search csift` prints its version), and report what could not be read instead of parsing the jsonl.

## Route by question

| you want to... | do |
|---|---|
| find where text appears, across every session | `csift search 'P'`, then `csift search 'P' TARGET` to narrow; text meant literally is escaped as Search and read shows |
| see what kinds of record a scope holds before filtering | `csift search '' TARGET --count-by label` (only the leaves a scan without `-t` reads; add `-t 'user.*' -t 'agent.*' -t 'harness.*'` for every leaf) |
| read the person's own messages | `csift search '' @<uuid> --no-subagents -t user.message`; a subagent transcript holds `user.message` records of its own (references/lanes.md) |
| count the person's own messages | `csift search '' @<uuid> --no-subagents -t user.message --count-by label` counts records: typed, relayed, a mid-turn steer, a slash command with arguments; a message the conversation was later rewound past is `user.rewound`, which this leaves out, so add `-t user.rewound` to count it too (measured on one session: 676 `user.message` and 6 `user.rewound`); `-c` counts another unit (the unit table below) |
| read AskUserQuestion answers and typed tool rejections | `csift search '' TARGET -t user.answer -t user.rejection`; before listing every call that did not run, typed reason or none, read references/search.md, Recipes |
| count matches, or list the sessions that matched | `csift search 'P' TARGET -c` (exchange rows, not records); `csift search 'P' -l` |
| scope the next command to the sessions that matched, or read a field csift does not render (stop_reason, keys newer than csift) | before piping csift into csift or into jq, read references/search.md, Recipes |
| the first or the latest occurrence | `csift search 'P' TARGET --max-count 1`; `csift search 'P' TARGET --max-count -1` (measured: `--max-count -1` exits 2 on `show`, `list` and `stats`) |
| read one record in full, or its raw bytes | `csift show TARGET --line N`; `csift show TARGET --line N --raw` |
| the last thing the assistant said, in a session or a subagent | `csift search '' TARGET --no-subagents -t agent.message --max-count -1`, then `csift show TARGET --line N` with the last `L<n>` it prints; without `--no-subagents` the newest message can be a subagent's, and why a tail fetch does not answer this is in references/search.md, show in depth |
| a session's newest records of every kind, or its latest turns | `csift show TARGET --line -30..`; `csift show TARGET --turn -3..`, then the `continue:` command it prints when it stops at its cap |
| unanswered tool calls, failed tool results | `csift search '' TARGET --count-by pairing`; `csift search '' TARGET --count-by result`; without `-t` an answer's and a typed rejection's results sit outside both axes, while an untyped refusal counts `paired` and `error` (references/search.md, Census axes); before listing the errored or refused ones, read references/search.md, Recipes |
| which models or Claude Code versions a session ran | `csift search '' TARGET --count-by model`; `csift search '' TARGET --count-by version` |
| esc-recalled drafts, text typed into the queue | `csift search '' TARGET -t user.unsent`; `csift search '' TARGET -t user.queued` |
| claim something was never done, said or decided | before saying so, read references/search.md, Absence claims |
| what a leaf means, which leaf to pick, what a bracketed tag in a hit means | before choosing an unfamiliar leaf or reading an unfamiliar tag, search references/labels.md for it and read that row, not the whole file |
| check a label against the raw line on disk; which jsonl field decided a leaf | before judging a label from `--raw` bytes, search references/shapes.md for the leaf |
| read `search` or `show` JSON; a `search` or `show` flag this file does not teach | before using either, read references/search.md; for one key or flag, search that file for its name |
| a range, a time window, what a turn number holds, a printed time | before writing a `--line`, `--turn`, `--file-lines`, `--since` or `--until` value this file does not show, comparing or reusing a turn number, or converting or comparing a printed time, read references/ranges.md |
| what a record corresponds to, what a message led to | before following provenance, read references/provenance.md |
| drafts, rewinds, forks, turns above a compaction | before reasoning about records off the conversation, read references/survival.md |
| which session is this; clones, clears, handoffs; tokens, tool calls, turn count | before listing sessions or totalling tokens, tool calls or turns, read references/sessions.md |
| files a session or its subagents changed | before listing them, read references/disk.md |
| rebuild a file or a deleted plan; a plan's approval | before running `csift recover` or `csift plan`, read references/disk.md |
| the turns a compaction summary clipped | before reconstructing them, read references/verbatim.md |
| a session's subagents, and whether one finished or is stuck; which subagent you are; what your parent asked of you; a teammate's ids, or stopping or steering one | before listing lanes, judging one, naming your own or stopping one, read references/lanes.md |
| images pasted or screenshotted into a session | before listing or extracting them, read references/image.md |
| is a session running, stopped or waiting on a person; wait on one | before judging liveness or waiting, read references/live.md; for a subagent, the row above, since `status` reads its parent's registry |
| message another session or subagent, or check that one arrived | before sending anything, read references/channel.md |
| hand the user a hook: `scripts/compact-slice.sh` re-injects clipped turns after a compaction, `scripts/taskstop-teammate.sh` redirects a failed teammate TaskStop, `scripts/elicitation-marker.sh` records pending questions | before handing a user any hook, read references/hooks.md |
| an error string this file does not decode; what an exit code means | search references/errors.md for the message's first words and read that row; before a script branches on an exit code, read its Exit codes section |

## Laws

1. Read transcripts through csift, and never by hand-parsing the jsonl, because a hand pass is wrong without an error: a `type:"user"` filter also counts tool results, peer messages and harness notifications as the person; AskUserQuestion answers and typed rejections ride inside tool_result carriers; subagent transcripts are separate files; a compaction summary replaced the words it summarised; large tool output lives in `tool-results/` files. Instead census with `--count-by label`, filter with `-t`, and read a field csift does not render from the record's raw line (references/search.md, Recipes).
2. An empty result is an answer, not a syntax error: a zero-match search exits 0 and its stderr names the filters and, under a label filter, where the pattern does occur (measured: `csift search zzqq TARGET -t user.message`). It is definitive only over the text the scan read: a scan without `-t` or `--attachments` never reads the gated leaves (Search and read), and that stderr lists them; without `--resolve-persisted` it never reads the tool output Claude Code moved into `tool-results/` files; and no scan reads a line csift models no leaf for (references/search.md, Absence claims). Do not rewrite the syntax or fall back to hand-parsing; instead read the stderr, then reach the gated leaves with `-t 'harness.*' -t user.queued`, `--attachments` or the exact leaf, add `--resolve-persisted`, or widen the target. The one rewrite owed first is a correct literal, metacharacters escaped and a tool call's input in its JSON form (Search and read).
3. Every cap reports what it dropped, so a result is either whole or says it is not: `list` stops at 50 rows on an unscoped run, `show` at 200 record units (one unit per record, or per section of a record that renders several, such as a notification carrying a result), `search` and `stats` are uncapped (docs: `csift search --help`, `csift stats --help`), and `--max-count 0` means uncapped (docs: `csift --help`). Never bound output with `| head`, which drops the closing notes and makes some commands abort (references/errors.md, Closed pipe); instead pass `--max-count N` to `list`, `search`, `show` or `stats`, the only four that take it, and bound every other command by target and filter (measured: `csift <cmd> @<uuid> --max-count 3` exits 2 on `agents`, `files`, `image`, `plan`, `recover`, `status`, `verbatim`, `whoami` and `msg`). `| tail -1` on `--format json` output is safe, because it reads the whole stream and keeps the summary line. Malformed lines are counted, never hidden, `search -c`, `-l` and `--count-by` included: each surface's text form is in references/errors.md, Notes that are not errors, and JSON carries `skipped_lines`.
4. Rows of `list`, `search`, `stats`, `files`, `verbatim`, `recover`, `image` and `plan` carry the id trio `session_id` / `is_subagent` / `parent_session_id`, and `agents` rows carry `agent_id` and `parent_session_id` instead (measured: each `--format json`). A subagent's `session_id` is its agent id (`a` + 16 hex, or a teammate's `a<Name>-<16 hex>`, as `csift agents` prints it): with the `@` sigil it addresses that subagent and any it spawned, never its session. A line number belongs to one file, so never pair it with an id taken from another record, because a parent uuid plus a subagent's line silently fetches another record; instead run the hit's own `refetch` command (text prints it after `↳` under a subagent's hit; a top-level hit needs none, since the `<id8>` heading its exchange is its own transcript's id cut to 8 characters and an `@` target itself), and scope a whole session with `parent_session_id`.
5. Before converting, comparing or subtracting a printed time, read references/ranges.md, Instants: text and JSON print an instant in different forms, and a few fields keep the harness's own wording.

## Targets

Every command takes its target as a positional argument after the subcommand; `search`'s first positional is the pattern and its targets follow it, and `msg` and `ack` name their lane with `--lane @<lane>` instead.

| form | scopes to |
|---|---|
| `@<uuid>` | one top-level session (plus its subagents on the spanning commands) |
| `@<uuid-prefix>` | the session or agent whose id starts with it: 4 to 11 dashless hex, or a dashed prefix; 12 or more dashless hex is read as an agent id, and a prefix that starts two ids is an error naming both (measured: `csift list @<first N hex> --no-subagents`) |
| `@<agent-id>` | one subagent and its subtree, by its agent id (measured: 17-character ids in `csift agents @<uuid> --format json`) |
| `@<Name>@<Team>` | a teammate by its routing form; an error when two teammates share it |
| `@main` | the calling top-level session, from `$CLAUDE_CODE_SESSION_ID`; from a subagent that is its parent, which a stderr note says (measured: `csift stats @main --no-subagents --turn -1`) |
| `@trap:<marker>` | the calling lane itself, found by a marker in the grammar references/lanes.md gives |
| `.`, a real path, or an encoded dir (`-Users-dev-proj`) | every session of that project |
| a `*.jsonl` path | that transcript, and on a spanning command the subagents it spawned as well, exactly as its `@` id would; that one file under `--no-subagents`, and always on `show` (measured: `csift stats <session>.jsonl --format json` gave `sessions_in_scope` 3 on a synthetic session with two subagents, 1 with `--no-subagents`) |
| nothing | every project under the Claude home, right for `search` when the question is "anywhere"; `list` stops at 50 rows and `agents`, `files` and `stats` print a block per session, so target those; `plan` and `msg` answer the calling session, `verbatim` exits 1, `show`, `status` and `wait` exit 2 (measured: each with no target) |

An id without `@` is an error (`did you mean '@<id>'?`). `list search stats files recover plan image status wait` include subagent transcripts by default and take `--no-subagents`; `verbatim` spans only with `--subagents`; `show` and `agents` reject both flags (docs: `csift --help`; measured: `csift show @<uuid> --line 1 --no-subagents` and `csift agents @<uuid> --no-subagents` exit 1). `--sessions-from FILE` (`-` for stdin) adds the ids `search -l` prints to the targets of `list search stats files recover plan image agents verbatim`, each spanning exactly as its `@<id>` would, and an empty list scopes to nothing (docs: `csift search --help`; measured: `csift verbatim --sessions-from -` fed one uuid read 0 subagent transcripts). `show` takes exactly one transcript, `status` and `wait` exactly one session, and `whoami` refuses a session uuid (measured: `csift whoami @<uuid>` exits 1). Only `--claude-home DIR` (else `$CLAUDE_CONFIG_DIR`, else `~/.claude`) and `--cross-compactions` go before the subcommand (measured: `csift --format json list @<uuid>` exits 2).

## Search and read

`search` is RE2-class regex (no backreferences, no lookaround, which fail to compile) with smart case: case-insensitive unless the pattern spells an uppercase letter, as a literal, inside a bracket class or as an escape that encodes one (`\x45` is `E`); a class escape such as `\S`, `\W`, `\D` or `\B` is no letter, and a Unicode class is folded too, so `\p{Lu}rror` also matches `error` until `(?-i)` turns folding off; `-i` forces insensitive (docs: `csift search --help`; measured on a synthetic transcript: `\x45rror` 1 record against `\x65rror` 2, `\p{Lu}rror` 2, `(?-i)\p{Lu}rror` 1). An empty pattern `''` matches every record the filters admit.

A pattern is matched against each record's rendered text: a message, a thinking block or a tool result as its decoded text; an AskUserQuestion answer as the whole question, options and answer; a tool call as its tool name plus its input re-serialized as JSON (`Bash {"command":"..."}`), so inside a call's input a double quote is `\"`, a backslash is doubled and a newline is the two characters `\n`, which `--multiline` never reaches; bracketed tags before `L<line>` are display only and never match, though a turn a fallback model served also prints its tag's words as its text, where they do (references/labels.md, `agent.message.plain`). To match text literally, put a backslash before each regex metacharacter it holds (`\ . + * ? ( ) [ ] { } ^ $ |`), and inside a tool call's input write the JSON form: `'commit -m \\"fix'` finds the typed `commit -m "fix`, `'C:\\\\Users'` finds `C:\Users`, while in message text `'C:\\Users'` does (measured on a synthetic transcript: each of the three examples matched its 1 record, where `'commit -m "fix'` and `'C:\\Users'` under `-t agent.tool.use` matched 0; `'\$HOME'` 1 against `'$HOME'` 0; `'[abandoned]'` 10 records against `'\[abandoned\]'` 1). A pattern starting with `-` is read as flags (exit 2), so escape the dash: `'\-c is'` for `-c is`; one starting with `@` is refused (references/search.md, Flags beyond the core).

Single-quote every pattern, so the shell hands it over unchanged: inside double quotes the shell turns `\$` into `$` before csift sees it, and a single quote inside the pattern is written `'\''` or `\x27`, so `'it'\''s the \$HOME'` finds `it's the $HOME` (measured on a synthetic transcript: 1 record, where `"it's the \$HOME"` found 0; `'\$HOME'` 1 against `"\$HOME"` 0).

`-t SEL` (repeatable) filters by label, `-T SEL` excludes. A selector is a leaf (`user.message`), a category (`agent.tool`), a role (`agent`) or any of those plus `.*`. A bare role or category narrows on three axes, keeping only the leaves the model receives, the records the harness delivered, and none the conversation was rewound or recalled past (references/labels.md, How a selector narrows); a glob or an exact leaf reaches every record. The gated leaves (`harness.bookkeeping`, `harness.turn` and `harness.system` leaves, `user.queued`, `harness.schedule.event`, the hook and context attachments) need a `-t` reaching them, which for a leaf the model never receives is the leaf or a glob, never the bare category (measured: `-t harness.bookkeeping` 0 records, `-t 'harness.bookkeeping.*'` all of them); the attachment leaves also come with `--attachments`, and references/labels.md gives each leaf's gate.

Three mutually exclusive terminal modes print counts instead of hits, each with its own unit:

| flag | prints | unit |
|---|---|---|
| `-c` | one integer | exchange rows: each matched numbered turn once (eight matching tool calls in one turn count 1), plus a row per matched recalled draft or rewound opener, the message that opened a turn the conversation was later rewound past (measured: `csift search '' @<uuid> --no-subagents -c` printed 160 where `csift stats` read 138 turns and 22 abandoned ones) |
| `-l` | owning session uuids, one per line | sessions (a subagent hit lists its parent) |
| `--count-by AXIS` | per line a count right-aligned with leading spaces, two spaces, the key (measured: `od -c`); an accounting line on stderr | records; axes `label tool turn session pairing model attachment version result for-kind` |

`--max-count N` keeps the earliest N exchange rows, `-N` the latest N, and names the drop.

Before reading or projecting the `--format json` output of `search` or `show`, read references/search.md, JSON: a projection shaped after the text output reads the wrong rows and misses hits.

`show TARGET` fetches from exactly one transcript by `--line`, `--uuid` (the two combine) or `--turn` (alone), rendered in full with the leaf; with no address it refuses, so a whole transcript never lands in context by accident, and `--branch-points` is its one mode that takes none (references/survival.md). A `--turn` fetch does not return every line that carries its turn's number, `show` takes no `-t`, and its cap keeps records from the start of what was addressed: before relying on a `--turn` fetch to hold a whole turn, or reading `show`'s record order, its cap or its continuation, read references/search.md, show in depth.

Text output groups hits under one heading per exchange (a matched turn, or a matched draft or rewound opener) and prints one line per hit, its leaf, `L<line>` and text among glyphs and tags; before reading any glyph, tag or `↳` line there, read references/search.md, Text output.

## Exit codes

An exit code does not always say what a script expects of it, and a reader that closes the pipe early changes it: before a script branches on a csift exit code, read references/errors.md, Exit codes.

## Commands that write, send or overwrite

csift writes to two kinds of place only: a file a recovery, reconstruction or image extraction is saved to (an `--out` path it replaces; a batch `recover` skips a file already there unless told to overwrite), and the channel files beside a session (`send` queues into the receiving lane's, `ack` and a `deliver` hook write the calling lane's own), where a sent, acknowledged or delivered message cannot be recalled. It never edits settings: a hook fragment, csift's or one of this skill's scripts, is merged by the user and then writes or injects on every event it serves. Each writing command, and each hook handed over, waits for the user's reply to a read-only preview saying go (the request that started the task is not that reply; a backup does not replace it). Before saving anything to disk, read references/disk.md, references/verbatim.md or references/image.md; before sending, acknowledging or delivering a message, or arming a lane, read references/channel.md; before handing the user a hook, read references/hooks.md.

## Wrong assumptions

| you might assume | actually |
|---|---|
| a `type:"user"` filter finds the person's messages | it also takes tool results, peer messages and harness notices; `-t user.message` with `--no-subagents` is the person (Laws, law 1) |
| an empty result means the syntax is wrong | a search that matches nothing succeeds, and its stderr says what was and was not read (Laws, law 2) |
| a pattern matches text as typed | a regex metacharacter needs a backslash, and a tool call's input is matched in its JSON form (Search and read) |
| a flagless `--count-by label` lists every leaf in the scope | it counts only the leaves a scan without `-t` reads (Search and read) |
| `-c`, `--count-by tool` and `stats` should agree | they count exchange rows, records and calls (Search and read, the unit table; references/sessions.md, stats) |
| `hits[0]` is the match | a JSON row is one exchange and can hold many hits (references/search.md, JSON) |
| summing `.message.usage` over `--raw` records gives a session's tokens | one response's usage repeats on each of its records; `csift stats` counts it once (references/sessions.md, stats) |
| a subagent's `session_id` works as a session scope | it is the agent id and scopes that subagent; `parent_session_id` scopes its session (Laws, law 4) |
| `whoami` inside a subagent names that subagent | the environment names the parent session; `@trap:<marker>` names you (references/lanes.md) |
| csift only reads | it writes recovered files, reconstructions, extracted images and channel messages (Commands that write, send or overwrite) |
| `-k` means the last k | `-k` is the single k-th from the end; the last k is `-k..` (references/ranges.md) |
| a turn begins only at a person's message, and every message the person typed begins one | answers, typed rejections, peer messages and notifications open turns too, while a relayed message or a slash command with arguments opens none (references/ranges.md, Turns) |
| a fresh random string cannot already be in your own session | your own search call records it before its result returns (references/search.md, Absence claims and the self-echo) |
| a rewound turn's edits were undone on disk | the edit reached the file; only the conversation moved past it (references/survival.md; references/disk.md) |
| a transcript lives forever | Claude Code deletes a transcript once its last write is older than the `cleanupPeriodDays` setting (references/sessions.md, Retention) |
