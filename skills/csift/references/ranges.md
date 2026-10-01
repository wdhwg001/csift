# Ranges, turns, time windows and instants: every --line, --turn, --file-lines, --since and --until value, and every printed time

Read this before writing a `--line`, `--turn`, `--file-lines`, `--since` or `--until` value SKILL.md does not show, before relying on what a turn number holds, before comparing or reusing a turn number across runs, transcripts or `--cross-compactions` values, and before converting, comparing or subtracting a time csift printed.

## The range grammar

Every range flag (`--line`, `--turn`, recover's `--file-lines`) takes one grammar, endpoints inclusive (docs: `csift --help`):

| form | means |
|---|---|
| `N` | that one |
| `A..B` | A through B |
| `N..` | N to the end |
| `..N` | the start through N |
| `-k` | the single k-th from the end: `-1` is the last one |
| `-k..` | the last k |
| `..` | all |

| you pass | csift does |
|---|---|
| `A-B` | refuses it, naming the `A..B` spelling: `csift: error: --turn range is START..END (e.g. 495..500); a leading -N counts from the end (-3.. = the last 3). got "3-9"` (the flag's own name leads) |
| a reversed `9..3` | refuses it: `csift: error: --line end (3) is before start (9)`, and `--turn end (3) is before start (9)` on `--turn` |
| a range partly past the end | clamps it to what exists |
| a range wholly past the end | on `show`, an address miss that exits 1; on `search`, `stats` and `files`, an empty answer that exits 0 (measured: `csift show @<uuid> --turn 90..99`, `csift search x @<uuid> --turn 90000..99999`) |
| several ranges at once | only `show --line` takes them, comma-joined (`--line 87,495..500`) |

Measured on a 57-turn session: `csift show @<uuid> --turn 9..3` and `--line 9..3` exit 1 with those texts, `--turn 3-9` exits 1 with the first, `--turn 50..99` returns records, `--turn 90..99` exits 1, and `search`, `stats` and `files` with `--turn 90000..99999` exit 0.

Lines (`Lnnnn`) are 1-based physical jsonl lines of one file. A `--turn` window over a scope of several transcripts resolves inside each of them, so over a session and its subagents `--turn -1` is the last turn of every transcript (measured: `csift search '' @<uuid> --turn -1 --count-by session` counted 16,361 records over 118 transcripts, and 42 with `--no-subagents`); pin one transcript with `--no-subagents` when you mean one.

## Turns

Turns (`tN`) are 0-based and number every turn except an abandoned one (a recalled draft, a turn rewound past). Pre-cut turns are numbered too: pre-cut is the verdict of records above the chain floor, the point, usually a compaction boundary, where csift's walk of the conversation stopped (references/survival.md). Turn 0 is the first opener's turn plus every line above it, an opener being the record that opens a turn (measured: `csift show @<uuid> --turn 0` on four sessions; `csift show @<uuid> --turn 0..4 --max-count 0 --format json` returned 308 records, all pre-cut, on one).

A turn opens on a person's typed message, an answered AskUserQuestion, a tool rejection carrying typed text, an inbound peer message or a `<task-notification>` pulse, and never on a relayed message (docs: `csift search --help`); a slash command with arguments carries `user.message` yet opens no turn either (measured: `csift stats @<uuid> --no-subagents` counted 1 turn on a session driven by one such command). So turns are not the person's messages.

Read line and turn numbers from output, never compute one from the other, and never reuse a turn number from an older run or under another `--cross-compactions` value, because a rewind and a change of budget both renumber turns (measured on one session: `csift --cross-compactions 0 stats @<uuid> --no-subagents` counted 139 turns and 138 at `3`, and `show @<uuid> --turn 101` fetched a different record at each). A uuid (`show --uuid`, a hit's `refetch_uuid`) is the address that lasts.

## Instants

Text output prints an instant in local time with its zone and offset (`2026-03-14 09:26:53 AEDT(UTC+11)`; the turn and record headers of `search` and `show` add milliseconds), except what a record states in its own words: a schedule's `[scheduled fire Jun 5 6:40am]` keeps the harness's text and a cost line's `startTime=` epoch milliseconds (measured: each command's text output on one session, `csift search '' -t harness.schedule.fire`, `-t harness.bookkeeping.cost`). JSON pairs `*_utc` with `*_local`, except `checkpoint_closes_at` (UTC alone) and `scheduled_at` (the harness's text); the `--until is before --since` error prints UTC. Do not convert text times by hand; instead read them as printed, and use the JSON pair for arithmetic.

## Time windows

`--since` / `--until` take a relative age (`45s 90m 2h 3d 1w 2mo 1y`, with `mo` 30 days and `y` 365), a bare date (local midnight), a bare datetime (local wall-clock), or ISO 8601 with `Z` or an offset (docs: `csift search --help`); `now` is accepted too (measured: `csift search '' @<uuid> --no-subagents --since now` exits 0). A `--turn` window and a time window apply together (AND). A time window is the way to reach records outside turn numbering, which every `--turn` window leaves out (references/search.md, A zero match).
