---
name: csift
description: >-
  Read, search and analyze Claude Code session transcripts (the .jsonl under
  ~/.claude/projects). Use this INSTEAD of grep/ripgrep/cat/jq/python over those
  files - reach for it the moment you would hand-scan or hand-parse a session
  jsonl, or the moment you are about to record session facts (timestamps, ids,
  last-activity markers) yourself: query, do not shadow. Find where a phrase or
  regex appears across ALL sessions and when (timestamp, line, owning session
  per hit); read records by line, turn or uuid; see what a session is doing
  right now (recent turns, pending tool calls); see tools, tokens, models, files
  changed; recover a file or a deleted plan from the transcript; restore the
  verbatim turns a compaction summary clipped; locate the plan bound to a
  session; list and identify sessions. Also the query engine for your OWN
  automation: hooks and scripts read session facts through csift (read-only,
  sub-second), never shadow state. Full round-trips, ready-to-run refetch
  commands. Pure regex, not semantic search.
---

# csift — ripgrep for Claude Code session transcripts

Surface: **v0.7.7** (must == `csift --version`). If an invocation you were CONFIDENT about errors, your knowledge is stale — an older csift surface from prefill/summary/habit. Re-read THIS file (it always matches the installed binary); never fall back to hand-parsing the jsonl.

Rust CLI over CC session `.jsonl` under `~/.claude/projects/<encoded-cwd>/`. Built for an LLM consumer: token-lean text, uniform JSON, pure regex (RE2-class, linear-time; no backrefs/lookaround — they fail to compile by design). Smart-case: a pattern is case-insensitive unless it carries an uppercase; `-i` forces insensitive. `csift <cmd> --help` is the authoritative flag manual. Flag order is genuinely free — before/after the subcommand, before/after positionals, all equivalent.

## Route by QUESTION — one question ⇒ one command

| you want to know… | run |
|---|---|
| where does text X appear (regex, full round-trips) | `search PATTERN [target…]` |
| read exact record(s) — by line, turn, or uuid | `show TARGET (--line SPEC \| --turn SPEC \| --uuid U)` |
| read a session's recent turns ("what's it doing now") | `show TARGET --turn -3..` |
| what record-types live here, and how many | `search "" TARGET --count-by label` |
| which tools ran, how often (per-record census) | `search "" TARGET --count-by tool` — or `stats` (per-CALL counts) |
| any pending / unanswered tool calls | `search "" T --count-by pairing` (the count) — or `agents` (per-lane detail: which tool, since when, escalation-blocked vs awaiting) |
| which model(s) produced the replies | `search "" TARGET --count-by model` |
| hits per turn (a histogram) | `search PATTERN TARGET --count-by turn` |
| tokens burned · tool totals · turn count · time span | `stats [target…]` |
| what files changed, when; mutation timeline | `files [target…] --by file` / `--by timeline` |
| the FULL text of matched records (no clipping) | `search PATTERN … --no-truncate` |
| any field csift does not render (usage, stop_reason, …) | `search PATTERN … --raw \| jq` / `show T --line N --raw` |
| which sessions matched → scope the NEXT command | `search P -l \| csift <cmd> --sessions-from -` |
| which session is this / who am I | `list` / `whoami` |
| rebuild a file (even deleted) from history | `recover TARGET --file P` |
| restore turns a compaction summary CLIPPED | `verbatim TARGET…` (only when a compaction ate them) |
| subagent tree: lifecycle · status · frozen lanes | `agents [target]` |
| the session's bound plan file | `plan [target]` |
| pasted images: list / extract | `image [target]` |

Two commands read transcript content — pick by intent: `show` fetches from the live transcript (this includes the tail-peek `show T --turn -3..`); `verbatim` reconstructs what a compaction summary already discarded (budget-bounded, crosses boundaries). Everything you want to READ is `show`; `verbatim` is only for compaction-clipped history — and it tells you (stderr note) when you use it on a session with no compaction.

## Wrong assumptions that cost real sessions

| you might assume | actually |
|---|---|
| empty pattern `""` matches nothing | it matches EVERYTHING — the base filter for `-t`/time/turn/census |
| `-c` counts matching records/lines | it counts EXCHANGES (round-trips); per-record counts = `--count-by` |
| `-l` lists every matching transcript | it lists OWNING session uuids (re-feedable); per-transcript detail = JSON summary `transcript_ids` |
| `--sessions-from` scopes to exactly the listed ids | the ids then EXPAND to their subagents (span default) — add `--no-subagents` to pin |
| turn and line share a numbering | `turn` = 0-based logical (the `tN` search prints); `line` = 1-based physical jsonl (`Lnnnn`); read both from output, never compute |
| a line number works with any session id | line numbers are per-FILE: `show --line` must target the row's own `session_id` (a parent uuid + a subagent line silently fetches the wrong record); prefer running the row's `refetch` verbatim |
| `-t user -T user.message` is contradictory | it is set subtraction (→ `user.answer` + `user.rejection`); a selector typo is a parse error with suggestions, never a silent empty |
| an excerpt is a summary | it is a match-centered FRAGMENT (~400 chars); full text = `--no-truncate` (lifts the JSON `excerpt` too) or the hit's `refetch` |
| `--raw` and `--format json` combine | they exclude each other (`--raw` IS machine output: verbatim jsonl lines) |
| zero matches means your syntax failed | it is a DEFINITIVE absence (exit 0) and search says so on stderr — read the diagnosis; when a `-t` excluded the hits it NAMES the label they live under |
| a stopped teammate needs TaskStop / pkill | teammates are in-process: `SendMessage` by name with `{"type":"shutdown_request"}` — TaskStop rejects every teammate id form |
| `completed_utc` = "when it stopped" | non-null ONLY when `status:"completed"` — a frozen/running lane carries null; its tail instant is `last_activity_utc/_local` (every timestamped lane; == `pending_since_utc` when frozen) |
| the pairing census needs `-t agent.tool.use` | pairing rides the tool BLOCK through the communication views — a frozen `SendMessage` counts as `pending` with no `-t` at all |
| timestamps need timezone arithmetic | text timestamps are already LOCAL with the offset inline — `2026-07-11 15:33 AEST(UTC+10)`; UTC lives only in JSON `ts_utc` |
| a hook that needs a session fact needs its own state file | the transcript already records it - before persisting anything (last prompt time, ids, activity markers) ask: does the jsonl already have this? Query csift from the hook (read-only, sub-second, safe inside hooks); a shadow store duplicates ground truth and drifts |
| "previous prompt" from a UserPromptSubmit hook = the newest `-t user` hit | at that instant the CURRENT prompt's record may or may not be flushed yet (both observed live) - drop hits younger than now-2s and take the newest survivor; the @trap MAIN-thread flush race, different consumer |
| `@trap` failing = you mistyped the marker | from the MAIN thread a FIRST use ALWAYS misses (CC flushes the main record only after the command completes; a subagent resolves first try) — use `@main` there, or re-run the SAME command+marker; a FRESH marker restarts the race and misses again |
| "re-run the @trap command" = a second line in my script | the retry must be a NEW, SEPARATE shell-tool invocation (Bash / Windows PowerShell) — two attempts inside ONE shell script are still ONE in-flight command (nothing flushes until the whole script exits), so both miss; batching the retry to save a round trip is exactly the wrong move here |
| a fresh nonce string is reliably absent from the corpus | not from YOUR OWN live session: using it as a search pattern writes it into your transcript the moment that tool call flushes — the next unscoped/`@main` search finds your own earlier invocation (a self-echo, label `agent.tool.use`). Absence checks: scope away from your own session, or only trust the FIRST use |
| piping text output through `head -N` is safe | excerpts keep a record's LITERAL newlines (a multiline Bash command renders as-is) — `head` can cut mid-record and hide overflow pointers; the line-safe form is `--format json` (one object per line) |
| `stats` and `--count-by tool` should agree | three count units, three commands: `-c` = EXCHANGES, `--count-by` = RECORDS, `stats` tools = CALLS. A call = tool_use record + tool_result carrier, so `--count-by tool` reads ≈2× the `stats` tally (an answered AskUserQuestion re-homes its carrier to `user.answer`, so AUQ stays ≈1×) — a unit difference, not a bug |
| image `#N` handles run densely 1..N | `#N` is inherited from CC's paste-time `[Image #N]` numbering — handles can start past #1 and carry HOLES (that number's image never landed in this transcript); a `--id` miss errors naming the handles that DO exist |
| `.hits[]` flattening loses the ids | not anymore: the id trio rides EVERY hit row too (matching the exchange row), so `jq '.hits[] \| {session_id, label}'` carries real ids bare; a hit's `refetch` stays the preferred single-record fetch |
| `ScheduleWakeup` calls live under `harness.schedule.*` | a tool CALL classifies by role — arming a wakeup is `agent.tool.use` like any other tool; `harness.schedule.wakeup` is only the FIRED tick (the harness-injected, marker-carrying wakeup prompt), and a custom-prompt tick lands as an isMeta record (excluded, like all isMeta) |
| `csift turns` reads a session's turns | `turns` was RENAMED `verbatim` in v0.5 (compaction reconstruction only); the old name never runs — it errors naming the successor. Plain turn READING is `show <target> --turn -3..` |
| csift only reads CC's exact compact serialization | candidate detection is serialization-TOLERANT (since v0.6.9): a reserialized `"role": "user"` line (json.dumps defaults, a jq round-trip) is a full citizen — same previews, counts, matches. The framing law still stands: one record per LINE (pretty-printed multi-line JSON breaks jsonl framing and counts as malformed) |
| `list skipped_lines: 0` = the file is clean | `list` reads only the head/tail lines it needs (the fast-overview contract), so its count covers the LINES READ — a mid-file tear is outside its windows BY DESIGN, and the text note says so. The whole-file corruption census is `stats` (full scan; search/files/recover agree with it). Each `agents` row's `skipped_lines` is the same window census (lifecycle reads the transcript's edges) |

## Five laws (all commands)

1. **Exit law**: an ADDRESS that misses = hard error, exit≠0 (`show --line 99`, `show --turn 99`, `--uuid`, a pinned `@id`, `--agent`, `recover --file`, `image --id`). A FILTER that matches nothing = honest empty, exit 0 (`search`, time windows, open/from-end ranges) — and a zero-match `search` self-diagnoses on stderr: definitive absence + active filters + (under `-t`/`-T`) the labels the pattern DOES occur under. Never re-derive syntax because a result came back empty.
2. **One range grammar, two axes**: every range flag (`--line` / `--turn` / `--file-lines`) takes `N` · `A..B` · `N..` · `..N` · `-k` (k-th from the END; `-3..` = last 3) · `..` (all), inclusive. `A-B` hard-errors with the correct spelling; a statically reversed `9..3` errors at parse. Axes: `turn`/`tN` = 0-based logical turn; `line`/`Lnnnn` = 1-based physical jsonl line; `--file-lines` (recover) = the reconstructed FILE's lines. On `show`, an explicit `--turn N`/`A..B` is an address (law 1); open/from-end forms clamp. `--turn` (windowing) ∧ `--since/--until` intersect (AND) everywhere.
3. **Span law**: subagents included by default; `--no-subagents` restricts; both switches exist everywhere (contradictory pair = parse error). `verbatim` is the one opt-IN (`--subagents`; its budget multiplies per session) and the one command that REQUIRES a target. `agents` rejects both (it LISTS subagents).
4. **Caps law — no silent truncation**: every cap reports its drop and how to get more. Defaults: `list` 50 rows on an unscoped all-projects run; `show` 200 record units (the drop prints the exact continuation command); `search`/`stats` uncapped until `--max-count N`. `--max-count 0 = uncapped`, uniformly. Malformed lines are counted (`N malformed line(s) skipped`), never hidden — including obviously-corrupt lines the byte prefilters never parse (free-text garbage, crash-truncation: any non-blank line that isn't `{…}`-framed); the one undetectable residue is a `{…}`-framed line whose INTERIOR is invalid JSON on a non-candidate line (validating those would repeal the perf contract). SCOPE of the count: the full-scan commands (`search`/`stats`/`show`/`files`/`recover`/`verbatim`/`image`) census the whole file; the head/tail readers (`list` rows, `agents` rows) census the LINES THEY READ — booked exactly once (the two windows are disjoint), scope-qualified in the text note, and never a whole-file verdict (that is `stats`). A sidecar marker line the current schema cannot read (schema skew — e.g. a pre-release fossil) counts as malformed too; it never merges and never vanishes.
5. **Time law**: every TEXT timestamp is local — `YYYY-MM-DD HH:MM:SS[.mmm] <TZAB>(UTC±offset)`, e.g. `2026-07-11 15:33:37 AEST(UTC+10)`, `IST(UTC+05:30)`. The marker is a format, not a value: name + offset derive from the machine zone at that instant (DST-correct), so the only mental step is "shift by the given offset". No UTC copies in text. Machine time = JSON, always paired: `ts_utc`+`ts_local` for a record's own instant, `<name>_utc`+`<name>_local` for named instants (`first_*`, `trigger_*`, …). Raw bytes = `--raw`.

## Targeting (positional, every command; `whoami` optional)

`@<uuid>` one session · `@<uuid-prefix>` (4-11 hex, unique else error) · `@main` calling top-level (env) · `@trap:<marker>` calling SUBAGENT (§trap) · `@<agent-id>` a subagent + its subtree (ids from `agents`; bare hex ≥12 or teammate form `aVSRepro-68a2…` — a teammate name may itself carry dashes, `aP1-engine-9cf2…`) · `.`/real path/encoded dir (`-Users-…`; Windows `C--Users-…`; UNC via `@--server-…`) ⇒ project(s) · `*.jsonl` one transcript · 0 targets ⇒ ALL projects (`list` caps the unscoped flood; `verbatim` REQUIRES a target).
- A bare id without `@` errors with "did you mean '@…'?" — ids always take `@`.
- An unrecognized `@`-shape (a 1-3-char prefix, a dashed fragment, a non-id token) errors naming the grammar — it never falls through to path resolution.
- `--sessions-from <FILE|->` (every multi-target command): scope to an id list — whitespace-separated uuid/prefix/agent-id tokens, bare or `@`-prefixed (exactly what `search -l` emits); UNION with positionals, per-id fail-loud, an explicitly empty list = empty scope (exit 0 — a pipeline that found nothing propagates nothing).
- `search` is the one command whose FIRST positional is PATTERN; targets follow: `csift search P @<uuid>`. A pattern starting `@` errors (escape `\@`); a uuid-shaped pattern prints a stderr note.
- `show` targets exactly ONE transcript: `@<uuid>` = that top-level file (never spans), `@<agent-id>` = that subagent's file.

### @trap:<marker> — "which subagent am I?"
A running subagent cannot read its own id from env. Invent a fresh marker, put it literally IN the csift command; csift finds the transcript whose shell tool_use carries it (Bash — or Windows' separate `PowerShell` tool, same `command` field). Grammar (enforced): exactly 3 CamelCase words + exactly 4 non-trivial trailing digits, hand-invented, context-independent — shaped like `@trap:JollyShinyBrook4283`, which is a RESERVED example csift hard-rejects (invent your own; never script-generate or reuse). TIMING: a subagent's transcript records the command mid-run — a first try resolves; the MAIN thread's own record is flushed only AFTER the command completes, so a top-level FIRST use always misses. From MAIN use `@main` (env-based, no race); if you must trap there, re-run the SAME command with the SAME marker **as a new, separate Bash invocation** — a second attempt inside the same shell script does NOT count (the whole script is ONE still-in-flight command; nothing flushes until it exits). One-shot means one marker per identity question, not one per attempt (a fresh marker restarts the race and misses again). UNIQUENESS is conversation-wide: in a team/multi-subagent setting the marker must be unique across ALL concurrently-running agents, not just your own retries — a marker that lands in two transcripts (e.g. relayed to a peer in a message) errors AMBIGUOUS, fail-loud, never a silent wrong match. `whoami @trap:<marker>` returns the full upstream ancestry chain.

## Labels (`-t/--label` · `-T/--label-not`) — dotted `role.class.sub`, 3 roles, 25 leaves

Selector = dot-segment prefix: `-t agent` (role) · `-t agent.tool` (use+result) · a full leaf = just it. No `-t` ⇒ all. `-T` EXCLUDES with the same grammar (effective set = includes minus excludes; a combination excluding everything it includes errors). Multi-label records emit once under the richest surviving view (an AUQ answer → `user.answer`; a SendMessage/spawn/`<result>` pulse → `agent.communication.*`; a slash-command-with-args → `user.message` rendered `/name args`). The complete rule is MECHANICAL, not a lookup table: JSON `labels[]` is always ordered richest-first, and the rendered view is simply the FIRST label in `labels[]` that survives your `-t`/`-T` — for any unlisted combination, read it off `labels[]`. Don't guess a record's leaf — run `--count-by label` to see the distribution.

```
user     .message   genuine human prose (incl. slash-command args, rendered `/name args`)
         .answer    AskUserQuestion answer (Q+options+answer unit)
         .rejection plan/tool reject + typed instruction
agent    .message · .thinking (redacted → "[redacted thinking]") · .tool.use · .tool.result
         .communication.{inbox,sent,signal}   peer msgs — rendered `from ⇨ to` (self = owner)
harness  .notification.{workflow,monitor,subagent,background-command,task}  ← <task-notification>
         .compaction.{summary,boundary}   boundary renders its compactMetadata (trigger=…)
         .command.{invocation,stdout} · .interrupt.{user,tool}
         .schedule.{wakeup,continuation} · .meta.{hook,loop}
```
Glyphs: `◂` user · `▸` agent · `⚙` harness · `·` sibling · `▹` tool use↔result pair (unreturned → `(no result — pending)`; orphan → `(use not in scope)`) · `⇨` comm direction. Turn boundary = genuine user ∨ AUQ answer ∨ typed rejection ∨ inbound peer message; slash-command wrappers, interrupts, `<local-command-stdout>`, compaction summaries never open a turn.

---

## search — find round-trips

```
csift search PATTERN [target…] [-t SEL]… [-T SEL]… [-i] [--multiline] [--since W] [--until W]
  [--turn N|A..B|N..|-k] [--max-count ±N] [-c | -l | --count-by AXIS] [--raw] [--siblings]
  [--no-truncate] [--resolve-persisted] [--additional-context] [--sessions-from F] [--no-subagents] [--format json]
```
Empty `""` pattern = pure filter. A hit returns the complete round-trip (tool_use with its result; user turn with reply; an answered AUQ as one Q+A unit). Terminal modes (mutually exclusive): `-c` prints one integer (EXCHANGES, `--max-count` drops added back) · `-l` prints the distinct owning session uuids, one per line, uncapped — pipes into `--sessions-from -` · `--count-by AXIS` prints a census (below).
- `--count-by AXIS` — a per-key census of the matched RECORDS along ONE closed axis (not a query language; a record whose several sections match still counts once): `label` (per leaf; a record counts under every leaf it carries THAT SURVIVES your `-t`/`-T` — a dual-labeled record never leaks its filtered-out twin into the keys, so `-t user -T user.message --count-by label` shows user.* keys only; with no filter that is the full label set — run with `""` and no `-t` before guessing any `-t`) · `tool` (per tool name) · `turn` (ascending histogram) · `session` (per transcript) · `pairing` (paired | pending | orphan, joined by tool_use_id; rides the tool block through the communication views, so a frozen SendMessage is `pending` — "any pending tools?" needs no `-t`) · `model` (per assistant model — the raw `message.model` value; CC's `<synthetic>` placeholder, a fabricated stand-in assistant record such as an API-error notice, is reported verbatim). Records outside an axis's domain are excluded and the excluded count is reported.
- Excerpts are ~400-char match-centered fragments; when anything clipped, text prints a caution + JSON summary `excerpts_truncated:true`. Full text: `--no-truncate` (also un-clips JSON `excerpt`) or the hit's `refetch`.
- `--siblings` (zero-arg): also render the turn's other records — messages always, thinking≤2 · tool.use≤3 · tool.result≤3 · harness≤2 per leaf — the caps apply to NON-matching context records only; your actual hits always render in full, so a block can legitimately show more than the cap count of same-leaf lines. Overflow prints `(+N more · csift show @<id> --line A..B)` — run it verbatim.
- `--raw`: the matched records' VERBATIM jsonl lines on the whole filter surface — stdout pure jsonl for `jq` (notes → stderr; sidecar-merged hits have no physical line and are omitted with a note). The answer to any unrendered-field question.
- `--resolve-persisted`: inline `tool-results/<id>.txt` files before matching (regex reaches externalized output); under `--raw` it affects matching only.
- `--additional-context`: ALSO scan hook-injected additionalContext (the `attachment` records a SessionStart/UserPromptSubmit/... hook writes — where `<stamp>`-style injected context lives). Off by default (machinery; echoes prompts/files wholesale). Hits surface under `harness.meta.hook`; the printed `csift show @<id> --line N` refetch renders one WITHOUT the flag (explicit address always works).
- `--multiline` sets `(?s)(?m)`. Caveat: EVERY tool_use's matchable text is its name + the RE-SERIALIZED JSON input (not only AskUserQuestion's), so a real newline inside e.g. a Bash `input.command` is already the two-character sequence `\n` by match time — match the literal `\\n`; `--multiline` is correctly irrelevant there. It helps only where rendered text keeps real newlines (message text, thinking, tool_result bodies).
- OUTPUT GEOMETRY (text): exchanges emit oldest-first (stable chronological across every transcript in scope; undated exchanges last). Each exchange header opens with a STABLE id-prefix token `<tok>·t<N>` — `<tok>` = the first 8 chars of the owning transcript id, directly usable as an `@` target, identical across invocations (a within-output collision lengthens the colliding group to 12 chars, then the full id; a teammate id renders whole); a subagent exchange carries `(parent <first-8-of-owning-uuid>)` on EVERY header. The head carries scope + match totals + direction (`matches  N exchanges · M sessions · oldest first[· showing earliest|latest K][· undated last]`); the tail repeats the totals and adds integrity notes + refetch guidance; each over-long fragment marks its own truncation inline (`(+N chars)`). To limit output, prefer `--max-count N` (earliest N) or `--max-count -N` (latest N) over piping into `head`/`tail` — a capped run keeps every note; a pipe amputates one end of the ledger.
- `--max-count` is SIGNED: `N` keeps the EARLIEST N of the chronological stream, `-N` the LATEST N, `0` = uncapped; the kept exchanges still emit oldest-first among themselves. The footer names the dropped side (`N later|earlier dropped by --max-count`).
- Zero matches: stderr prints "0 matches — a DEFINITIVE absence (exit 0), NOT an error" + active filters + the malformed-line count when >0 (the absence is definitive for parseable lines only) + (under `-t`/`-T`) `⚠ but "X" DOES occur — N record(s) under: <labels>`. JSON summary: `definitive_absence`/`active_filters`/`excluded_by_label`.

```bash
csift search "panic" @<uuid> -t agent --since 6h
csift search "" @<uuid> --count-by pairing                     # any pending tools?
csift search "todo" . -l | csift stats --sessions-from -       # aggregate matching sessions
csift search "" @<uuid> -t agent -T agent.thinking             # agent role minus thinking
csift search "X" --max-count 1                                 # when did X FIRST happen? (earliest exchange, local ts on the header)
csift search "X" --max-count -1                                # most recent occurrence of X
# find a phrase across ALL sessions, then read one hit in full:
#   copy <tok> and L<n> from the hit → csift show @<tok> --line <n>   (or --turn <N> for the whole turn)
```

## show — fetch records by line / turn / uuid (the reader; also the raw escape hatch)

```
csift show TARGET ( (--line N|A..B|N..|-k,…)… | --turn N|A..B|N..|-k | (--uuid U,…)… )
  [--raw] [--max-count N] [--format json]
```
- TARGET = exactly one transcript — `show` is the ONE targeting command with NO subagent-span pair (`--no-subagents`/`--subagents` are rejected with the rule, not a typo guess); to read a subagent, target its own `@<agent-id>`. One addressing mode is REQUIRED (no selector = a teaching error; csift never dumps a whole transcript by accident). `--turn N` fetches EVERY record of that turn — its whole back-and-forth — in the same numbering `search` prints (a `<tok>·t270` header ⇒ `--turn 270`, and the header's `<tok>` is the `@` target); `--turn -3..` is the tail-peek. A "turn" is everything since the last human-authored boundary — on a heavily-agentic session one turn can be DOZENS of records (a whole autonomous investigation); the 200-unit cap + drop report keep even a huge turn context-safe.
- Address misses error with the domain: `no such turn(s): t99 — the transcript has 2 turn(s) (t0..t1)`; open/from-end forms clamp (a `--turn -9..` on a 2-turn session is fine).
- Renders FULL records through search's pipeline (labels, pairing, plan pointers, sidecar merge). A metadata/attachment line is not a record — a range covering some prints `N line(s) in the addressed range are not records (… — inspect with --raw)`; a single-line miss error points at `--raw`.
- Cap: 200 record units by default; the drop prints `+N more record unit(s) … · continue: csift show @<id> --line A..B` (JSON `dropped_by_cap` + `refetch_remainder`). `--max-count N` / `0` = uncapped. `--raw` caps by line with the same stderr continuation.
- `--raw`: verbatim jsonl bytes — for unrendered fields (usage, stop_reason, model, any new field) and torn lines; excludes `--format json`.

```bash
csift show @<uuid> --turn -3..              # the last 3 turns — tail-peek a session
csift show @<uuid> --line 46550             # the record search cited as L46550
csift show @<agent-id> --line 88,495..500   # from a subagent transcript (its OWN id)
csift show @<uuid> --line 46550 --raw       # exact bytes (all fields)
```

## list — which session is this?

```
csift list [target…] [--since W] [--until W] [--max-count N] [--sessions-from F]
  [--no-subagents] [--format json]
```
Head+tail read only (fast at any size). Unscoped all-projects run caps at the 50 most-recently-active rows (drop reported; a scoped query is uncapped; `--max-count N` overrides, `0` = uncapped) — the scope banner / JSON header `sessions_in_scope` stay the PRE-cap resolved range ("how big is my corpus" reads off line 1; only the ROWS are capped). `--since/--until` keep a session iff its [first, last] activity span intersects the window. Rows: cwd (+branch, CC version), first ◂ / last ◂ / last ▸ excerpts (200 chars) + timestamps; subagent rows read `SUBAGENT <hex> · parent SESSION <uuid>`; pending elicitations annotate. JSON adds the sidecar tri-state: `sidecar_present:true` + pendings = blocked; `true` + none = provably not blocked on an elicitation; `false` = hook not installed, cannot conclude. `skipped_lines` is a census of the lines list actually READ (head/tail windows, each line booked once) — never a whole-file verdict; the whole-file corruption census is `stats`.

## whoami — identify the caller (false-positive-safe)

```
csift whoami [@main|@trap:<marker>] [--format json]
```
Reads `$CLAUDE_CODE_SESSION_ID` (alias `$CODEX_COMPANION_SESSION_ID`). Neither set ⇒ errors with guidance — never guesses by mtime. Subagent caveat: current CC hands an Agent-tool subagent the PARENT session's id (the subagent's OWN id is withheld from env; workflow `agent()` likewise; older builds handed a Task subagent its own id) — so from a subagent, plain `whoami` usually resolves the ROOT; `@trap` resolves the true self env-independently and returns the full upstream chain (self depth 0 → root). @trap timing: subagent = first try; MAIN = first use always misses (use `@main`, or re-run the SAME command+marker) — see §trap.

## stats — aggregates (tokens · tools · turns · span)

```
csift stats [target…] [--since W] [--until W] [--turn N|A..B|N..|-k] [--max-count N]
  [--sessions-from F] [--no-subagents] [--format json]
```
Per session: lines, user/assistant record counts, turns, compactions, first→last span + duration, tokens per model (input/output/cache_read/cache_creation), tool CALLS by name (the tool-frequency ranking). `--turn` windows the aggregates on the turn axis — token burn of the last N turns is `stats @main --turn -N..` (everything windows EXCEPT `lines`, which stays the file's physical line count — a file fact, not a window fact). Scope total block when >1 session; JSON summary carries scope totals (`tail -1 | jq .tokens`).

## files — what changed, when

Two different "which sessions touched X?" questions, two commands: sessions that EDITED the file = `files --glob '**/X'` (structural — Edit/Write/MultiEdit authoritative, Bash marked `heuristic`); sessions that merely MENTION it = `search "X" -l` (text recall, catches discussion too). Default to `files` for mutations, `search -l` for provenance/chatter.

```
csift files [target…] [--by summary|dir|file|timeline] [--regex RE] [--glob PAT]
  [--turn N|A..B|N..|-k] [--since W] [--until W] [--sessions-from F] [--no-subagents] [--format json]
```
Authoritative for Edit/Write/MultiEdit/NotebookEdit (create-vs-edit from the paired result); Bash mutations are lexical-heuristic and always tagged `(heuristic)`; failed ops excluded. `--by`: summary = top-prefix rollup (default) · dir · file · timeline (one line per mutation). `--regex` (full absolute path, case-exact) ∧ `--glob` (`**` crosses `/`) filter before rollup. An Edit-before-Read boundaries section always follows — out-of-band changes (formatter/git/editor) that forced a re-Read; the "risky to recover" signal (then `recover --coverage`).

```bash
csift files @<uuid> --by file --format json | jq -r 'select(.kind=="file" and (.heuristic|not)).path'
csift files --glob '**/foo.rs' --by file      # which sessions touched foo.rs (all projects)
```

## recover — rebuild a file (or a deleted plan) from the transcript

```
csift recover TARGET --file <ABS|SUFFIX|@plan> [--salvage|--patches|--at WHEN|--coverage]
  [--out PATH] [--file-lines N|A..B|N..|-k] [--turn N|A..B|N..|-k] [--since W] [--until W]
  [--files-from MANIFEST --out-dir DIR [--force]] [--sessions-from F] [--no-subagents] [--format json]
```
Replays the file's Read/Write/Edit stream into a sparse buffer — absent lines are explicit gaps, never fabricated. `--file` matches exact or component-aligned trailing suffix (`app.py`≡`src/app.py`; `b.rs`≠`ab.rs`). Five exclusive modes: **restore** (default; hard-fails if partial, naming covered+missing ranges + the recipe) · **--salvage** (never-fails fragment; gaps marked `??? lines A..B unknown`) · **--patches** (unified diffs; 3 anti-fabrication anchor checks gate every hunk) · **--at WHEN** (point-in-time; WHEN = ISO | `2h` | `@turn:N` | `@line:N` — a TRANSCRIPT line | `@latest`) · **--coverage** (scoping dry-run; run before trusting a salvage). `--file @plan` resolves the session-bound plan (rebuilds even a deleted one). Batch: `--files-from` manifest + `--out-dir`, one corpus scan.

```bash
csift recover @<uuid> --file /abs/gone.py --salvage
csift recover @<uuid> --file @plan --out /tmp/plan.md
```

## plan — locate the bound plan file

```
csift plan [target] [--reverse PLAN.md] [--no-subagents] [--format json]
```
The authoritative binding is the `plan_mode` attachment — never path heuristics. No target ⇒ the calling session (env). `--reverse <file>` inverts: which session(s) bind this plan. Locates only — then pick by question: what's on DISK now → `cat` the path; what the SESSION last had (or the plan is deleted) → `recover --file @plan`. The two can differ.

## verbatim — restore turns a compaction summary clipped

```
csift verbatim TARGET… [--budget N] [--round-trip-fraction F] [--agent-msgs longest|eot-only|rich|all]
  [--profile heavy|light] [--max-compactions N] [--subagents] [--turn N|A..B|N..|-k] [--since W] [--until W]
  [--sessions-from F] [--slice N [--slices N] [--window N]] [--out PATH] [--format json]
```
Not the tail-peek tool — that is `show --turn -N..`. A compaction summary keeps task state but loses turn fidelity; `verbatim` re-emits the verbatim user/assistant turns (each `Lnnnnn`), selected backward from EOF, printed ascending, transparent across many boundaries. On a session with NO compaction it tells you so (stderr note pointing at `show --turn`) — nothing was clipped there.
- A target is REQUIRED (the `--budget` is per-session; a bare run would multiply it across every project).
- `--budget N` = CHARS per session (default 40000; ≈4 chars/token). The header's `spanned K of N compaction boundaries in scope` is budget-relative on K (what the backward-from-EOF selection crossed — a small budget can honestly read `0 of 4`); N is the session's true total (== `stats`' unwindowed `compactions`). `--round-trip-fraction F` (default .5) reserves a floor for human round-trips. `--agent-msgs longest` (default) · `eot-only` · `rich` · `all`; `--profile heavy|light` is the whole tuning surface.
- Collapsed runs render `△ L{a}-L{b} [X agent message(s), …]` → fetch via the row's `refetch`. Units over the role caps middle-truncate with explicit counts (JSON `text` is always full). A turn already quoted by the newest summary is flagged `(also in summary)` and demoted, never dropped.
- Slicing (hook injection, ≤10000-char cap): `--slices N --slice i --window W` = fixed-fleet chunks, whole turns; out-of-range ⇒ nothing, exit 0 (hook affordance). Text-only.

## agents — subagent lifecycle + topology

```
csift agents [target] [--agent ID] [--shape builtin-task|workflow|teammate]…
  [--since W] [--until W] [--order-by trigger|start|completion] [--with-files] [--returned-message]
  [--sessions-from F] [--format json]
```
Text = a parent→child tree (nesting is logical, from spawn links; disk is flat). JSON = FLAT kind-tagged rows: per session a light `session` row (counts), each workflow run a `run` row, every agent its own `agent` row in tree pre-order — rebuild nesting from `parent_agent_id`/`depth`; `jq 'select(.kind=="agent")'` reaches every node.
- `shape` = transcript shape: `builtin-task` · `workflow` · `teammate` (built-in location + meta `taskKind:"in_process_teammate"`; name-embedded id; csift recovers the real `agent_type` + spawn via name-join).
- `--order-by` sets sort AND the `--since/--until` axis: `trigger` (default; the parent tool_use ts = true spawn) · `start` · `completion`. `--agent ID` = one node (implies returned-message; miss = error).
- Frozen lane: the newest record an unreturned tool_use ⇒ `status:"running"` + `pending_classification`: `escalation-blocked` (a dangerous-rm Bash CC hoists for approval — the one positively confirmable state) | `awaiting-execution` (slow OR wedged OR abandoned — jsonl can't tell them apart; at corpus scale a lane pending for hours/days is overwhelmingly "parent session ended", not in-flight — weigh `pending_since_utc` against now yourself).
- Teammate control (csift is read-only; this is a pointer): steer/terminate via `SendMessage` BY NAME (`message:{type:"shutdown_request"}`), never TaskStop (rejects every teammate id form) or pkill (in-process). Text footer + node `control_hint`.
- `returned_message` = the NEWEST message the child EVER returned — on a non-completed lane it predates the pending call, and the text render brands it inline (`history — predates the still-open lane, NOT the outcome`): a "work is complete"-sounding tail on a frozen lane is history, not the ending. It answers "what did the ORCHESTRATOR record", not "what did the agent conclude" — a `sync-tool-result` source can be the harness's truncated `Done. agentId: …` wrapper; the child's own final words are always `show @<agent-id> --turn -1..`. A `run` row's `status` is the workflow journal's verbatim last word (open set; observed `completed`/`killed`).

## image — pasted images

```
csift image [target] [--id N|L<line>i<n>]… [--out DIR|FILE.ext]
  [--since W] [--until W] [--turn N|A..B|N..|-k] [--uuid PREFIX] [--sessions-from F] [--no-subagents] [--format json]
```
Default list (content-deduped). `--id` input = bare digits (the `[Image #N]` number; display shows `#N`, input drops the `#`) or the always-unique locator `L<line>i<n>`; an ambiguous `#N` hard-errors with the occurrence list. `#N` is inherited from CC's paste-time `[Image #N]` numbering, NOT a dense 1..N index — handles can start past #1 and carry holes (a source gap: that number's image never landed in this transcript); a `--id` miss errors naming the handles that DO exist. `--out`: dir ⇒ source-format files; `file.ext` ⇒ converted by extension (png lossless · jpeg/webp q90 · gif dithered). search/verbatim cite ids inline (`[1 image: #265]`).

---

## JSON — envelope v2 + schema reference (transcribed from live output)

Every `--format json` stream is exactly: `{"kind":"header","command":…}` first (span commands add `sessions_in_scope`/`top_level_sessions`/`subagent_sessions`) → kind-tagged rows → `{"kind":"summary",…}` last. Universal idiom: `jq 'select(.kind=="<row>")'`; summary = `tail -1 | jq`.

Row kinds: list→`session` · search→`exchange` | `census` · show→`record` · stats→`session` · files→`mutation|file|dir|bucket|boundary` · agents→`session|run|agent` · verbatim→`turn|compaction_boundary|collapsed_agents` · plan→`plan` · image→`image|extract` · whoami→`identity` · recover→`coverage|segment|snapshot|restore|boundary`.

Shared row fields — the id trio on every spanning row: `session_id` (the transcript's OWN id: top-level uuid or subagent agent-id, both round-trip as `@…`) + `is_subagent` + `parent_session_id` (the owning uuid; = session_id on top-level rows). The two-rule id law: line-addressed fetches use `session_id`; scope-level re-targeting uses `parent_session_id`. Hits and collapsed rows carry `refetch` — a ready-to-run `csift show` command at the right id; prefer running it verbatim over assembling your own.

Field enums: `pairing` = `paired` | `pending` | `orphan` | null · census `axis` = `label|tool|turn|session|pairing|model` · `shape` = `builtin-task|workflow|teammate` · `source` = `elicitation-sidecar` | null · files `op` = `write|edit|multi_edit|notebook_edit|bash`. An agents RUN row's `status` is NOT one of these closed sets — it is the workflow journal's own last status verbatim (open set; observed: `completed`, `killed`), distinct from an agent row's csift-computed `status`.

Key fields per row (fixture-verified):
- search `exchange`: trio + `turn_index, ts_utc/local, record_uuids, hits[]`; each hit: trio (v0.6.5 — bare `.hits[]` flattening keeps real ids) + `label, labels[], line (null=sidecar), uuid, ts_utc/local, tool_name, from, to, pairing, tool_use_id, source, excerpt, image_ids[], refetch`. Summary: `matched, sessions, transcript_ids[≤100], transcript_ids_truncated, dropped_by_cap, skipped_lines, with_elicitation_sidecar, excerpts_truncated` (+ on zero matches `definitive_absence, active_filters, excluded_by_label`).
- search `census`: `axis, key, records`. Summary: `axis, matched_records, distinct_keys, excluded_records, dropped_by_cap, skipped_lines`.
- show `record`: trio + `turn_index, line (null=sidecar), uuid, label, labels[], tool_name, from, to, pairing, tool_use_id, source, ts_utc/local, text (FULL), image_ids[]`. Summary: `records, dropped_by_cap, refetch_remainder, non_record_lines, skipped_lines, with_elicitation_sidecar`.
- list `session`: trio + `path, cwd, version, git_branch, first_user/last_user/last_agent ({excerpt, ts_utc, ts_local}|null), skipped_lines (WINDOW census — the head/tail lines list read, each booked once; whole-file census = stats), pending_elicitations[], sidecar_present, with_elicitation_sidecar`. Summary: `sessions, dropped_by_cap, skipped_lines`.
- plan `plan`: trio + `line (the bind record's jsonl line), plan_file (the bound path — NOT `path`), plan_exists`. Summary: `plans`. (plan's header is the light `{command}` form — no scope split.)
- recover rows by mode — `coverage`: trio + `file, fragments, events, boundaries, covered_ranges, recoverable_lines, seen_total_lines` · `segment` (--patches): trio + `segment_index, line, line_start/line_end, turn_start/turn_end, ts_utc/local, pre_state_known, anchor_source, unified_diff` · `snapshot` (--at): trio + `file, lines, gaps, line, seen_total_lines` · plus `boundary` rows (the Edit-before-Read classifier, key `cause`). Summary: `{file, mode, sessions, skipped_lines}`. Default-restore JSON on an unrecoverable file = header + the hard error (exit≠0), never a partial body.
- image `image`: trio + `id, handle (#N|null), seq, img_index, line, record_uuid, media_type, source_kind, b64_len, est_bytes, url, ts_utc/local`. Summary: `images, transcripts, skipped_lines`.
- stats `session`: trio + `lines, user_records, assistant_records, turns, compactions, first_utc/local, last_utc/local, tokens{model→{input,output,cache_read,cache_creation}}, tools{name→count}, skipped_lines`. Summary adds scope totals (`tokens`, `tools`, `turns`).
- agents `session`: `session_id, runs, agents` (counts). `run`: `session_id, run_id, task_id, workflow_name, status, agent_count, duration_ms, total_tokens, total_tool_calls, default_model, started_utc/local`. `agent`: `agent_id, shape, parent_session_id, parent_agent_id, depth, workflow_id, agent_type, name, team_name, description, spawn_tool(_use_id), trigger/started/completed/last_activity_utc+local, duration, status, pending_tool_use_id/tool_name/classification/since_*, skipped_lines (head/tail window census, like list's) — completed_* and duration are non-null ONLY when status=completed; last_activity_* is the tail instant on every timestamped lane (== pending_since_* when frozen), control_hint?` (+ `returned_message(_source)`, `files_changed[]` when requested — `returned_message` is the NEWEST message the child EVER returned; on a frozen/running lane it predates the pending call, so read it beside `pending_*`, never as the outcome). Summary: `sessions, runs, agents`.
- verbatim `turn`: trio + `turn_index, line (null=sidecar), source, role, ts_utc/local, tool_calls, full_chars, rendered_chars, truncated, elided_*, also_in_summary, compactions_before, text (FULL), is_automation (+trigger_kind, task_id, status, event)`; `compaction_boundary`: `line, summary_chars`; `collapsed_agents`: `first_line, last_line, refetch`. Header adds the full budget accounting: `budget_chars, max_total_chars, round_trip_fraction, chars_used, boundaries_spanned (budget-window-relative), boundaries_total (scope's true total), selected_user, selected_assistant, automation_by_kind (the SELECTED triggers per class), automation_in_scope_by_kind (every in-scope pulse REGARDLESS of budget — the same window-vs-scope pairing as boundaries_*), automation_triggers (flat total of automation_by_kind's values), budget_is_per_session, sessions_rendered, with_elicitation_sidecar`.
- files rows carry the trio + `path, op, turn_index, line, is_create, heuristic, ts_utc/local` (timeline) or per-op counts + `first/last_utc/local` (grouped); `boundary`: `path, line, turn_index, cause, ts_utc/local`. Summary: `sessions, distinct_files, total_mutations, edit_before_read_boundaries, skipped_lines, detail_level` (no cap ⇒ no `dropped_by_cap`).
- whoami `identity`: trio + `depth, path`.

## jq canon — csift narrows, jq refines

Never jq the transcript FILE (you lose span resolution, the sidecar merge, and the malformed-line count). Always let csift emit the records, then jq freely — this is the intended pipeline, not a fallback:

```bash
csift search "" @U -t agent.message --raw | jq -r '.message.model' | sort | uniq -c     # any raw field
csift search P @U --format json | jq 'select(.kind=="exchange") | .hits[] | {label, line, excerpt}'
csift search P @U --format json | tail -1 | jq                                          # the summary
# flatten hits BARE — every hit carries the id trio itself (v0.6.5), no merge step needed:
csift search P @U --format json | jq -c 'select(.kind=="exchange") | .hits[] | {session_id, label, line}'
csift agents @U --format json | jq 'select(.kind=="agent") | {agent_id, shape, status}'
csift stats @main --no-subagents --format json | tail -1 | jq .tokens                    # token burn
csift files @U --by file --format json | jq -r 'select(.kind=="file" and (.heuristic|not)).path'
csift search "" @U -t agent.tool.use --raw | jq 'select(.message.content[]?.input.command? // "" | test("rm "))'
# RECORD-level pipeline (fetch a filtered subset of hits FULL): select in jq, then run the
# csift-generated refetch commands — each already addresses the hit's OWN transcript, so this
# composes across subagents where a line-number pipe could not (lines are per-FILE):
csift search P @U --format json | jq -r 'select(.kind=="exchange") | .hits[] | select(.label=="agent.tool.result") | .refetch' | sh
```

## What csift will NOT do (and the designated alternative)

- Semantic/BM25 search → regex is the tool; broaden the pattern, or census first (`--count-by label`).
- Arbitrary aggregation/group-by DSL → the closed `--count-by` axes, `stats`, `files --by`; anything else = `--raw | jq`.
- Field-predicate queries / joins ("tool_use where input.x AND result errored") → `search` narrows the scope, `--raw | jq` applies the predicate.
- Diffs between turns/files → `show` both, diff outside; file states = `recover --at`.
- Writing/terminating anything → csift is read-only; it prints control HINTS (teammates → SendMessage).

## Elicitation sidecar — pending AskUserQuestion / ExitPlanMode / MCP

While pending, these three leave NO trace in native jsonl (whole-turn buffered / in-memory) — a session blocked on a human looks stalled. A CC hook (recipe 3) appends markers to `<uuid>/elicitations.jsonl` (a sidecar, never the transcript). csift merges UNRESOLVED pendings automatically wherever it reads a session: they classify `agent.tool.use`, verbatim appends each as its own newest turn, list annotates. Once answered, CC writes the real record and the pending pairs off — no duplicates. GHOST GUARD: a REJECTED AUQ/plan fires no PostToolUse, so the hook can miss its `resolved` marker — csift therefore also drops any AUQ/ExitPlanMode pending whose tool id already appears on a native record (structural check, a quoted id in prose doesn't count): the native transcript outranks the sidecar, so a stale marker can never report a long-closed elicitation as pending. MCP markers have no native form — sidecar pairing stays their only signal. A merged record has no physical line: renders `(elicitation sidecar)`, JSON `source:"elicitation-sidecar"` + null `line`; surfaces print `with elicitation sidecar`. The sidecar cannot be targeted directly (error). Tri-state on list rows: `sidecar_present:false` = hook not installed — "nothing pending" is then NOT concludable. A sentinel-bearing marker the CURRENT schema cannot read (schema skew — a pre-release fossil under old field names) is counted into `skipped_lines`, never merged, never invisible.

## Conventions

- WHEN grammar (`--since/--until/--at`): relative `45s 90m 2h 3d 1w` (that long ago, system-local) or ISO8601 — bare date ⇒ local midnight · BARE datetime (`2026-06-01T05:00:00`, no `Z`/offset) ⇒ that LOCAL wall-clock time · `Z`/`+10:00` ⇒ explicit zone. Bounds inclusive; records without timestamps never match a bounded window.
- `--claude-home DIR` (global, any position — even before the subcommand) repoints `~/.claude`; precedence flag > `$CLAUDE_CONFIG_DIR` > the OS home's `.claude` (`$HOME` on Unix, `%USERPROFILE%` on Windows — the same resolution Claude Code uses; a stray Git-Bash `HOME` is ignored there).
- Path filters (`files --regex/--glob`) are case-exact (paths); search PATTERN is smart-case (text).
- Retention: CC deletes a transcript once its file MTIME (the session's last write — NOT its creation date; reading refreshes nothing) is older than `cleanupPeriodDays` (default 30!), and removes the session's whole sidecar tree (subagents, tool-results) with it. Check `jq '.cleanupPeriodDays // 30' ~/.claude/settings.json`; recommend 180/365 — csift can only read what survives.
- Exit codes: the contract is 0 vs non-zero, nothing finer. De facto a USAGE error (clap parse: bad flag/selector/conflict) exits 2 while a csift-level error (address miss, pinned id matching nothing) exits 1 — clap's convention, informational only; don't build on the split.

## Recipes

```bash
# which sessions mention X — then run ANY command over exactly those (the composition loop):
csift search "X" -l | csift files --sessions-from - --by file
# read what search found: run the hit's refetch verbatim (text prints the L<n> + the right id)
csift show @U --turn 270              # turn 270's whole exchange
csift show @U --turn -3..             # the last 3 turns (tail-peek "what's it doing")
# what record-types exist before you filter (so an empty -t is never a mystery):
csift search "" @U --count-by label
# pending tools / tool frequency / model census — one command each:
csift search "" @U --count-by pairing
csift stats @U --no-subagents          # tool CALL counts + tokens + turns
csift search "" @U --count-by model
# any unrendered field of matched records — never hand-parse the file:
csift search "" @U -t agent.message --raw | jq -r '.message.model' | sort | uniq -c
# only reach for verbatim when a COMPACTION clipped the turns (it tells you if not):
csift verbatim @U --turn -20..
```

### Hook 1 — SessionStart(compact): re-inject verbatim turns after compaction (#1 recipe)
N hooks run `verbatim --slices N --slice i` to supplement the summary with the recent verbatim turns (lands as an `attachment` record — csift ignores it, no feedback loop; safe to re-fire). Load-bearing race fix: CC runs same-event hooks CONCURRENTLY and concatenates in completion order — a `$PPID`-namespaced done-flag barrier forces slice order. ONE script registered N times:

```bash
#!/usr/bin/env bash
set -euo pipefail
slice="${1:?1-based slice index}"; N=4; WINDOW=9000 # N MUST equal the number of registered hooks
SEQ="/tmp/csift-turns-slice-$PPID"
release(){ mkdir -p "$SEQ"; touch "$SEQ/s$slice.done"; [ "$slice" = "$N" ] && rm -rf "$SEQ"; return 0; }
trap release EXIT
if [ "$slice" -le 1 ]; then rm -rf "$SEQ"; mkdir -p "$SEQ"
else u=$(($(date +%s)+5)); until [ -f "$SEQ/s$((slice-1)).done" ] || [ "$(date +%s)" -ge "$u" ]; do sleep 0.05; done; fi
command -v jq >/dev/null || exit 0
in=$(cat); [ "$(jq -r '.source//empty' <<<"$in")" = compact ] || exit 0
C=$(command -v csift||true); [ -x "$C" ] || C=$HOME/.cargo/bin/csift; [ -x "$C" ] || exit 0
sid=$(jq -r '.session_id//empty' <<<"$in"); [ -n "$sid" ] || exit 0
chunk=$("$C" verbatim "@$sid" --slices $N --window $WINDOW --slice "$slice" 2>/dev/null||true)
[ -n "$chunk" ] || exit 0
jq -n --arg c "Verbatim turns the compaction summary clipped (part $slice - a supplement; the summary still owns task state):
$chunk" '{hookSpecificOutput:{hookEventName:"SessionStart",additionalContext:$c}}'
```
Register N times: `{"matcher":"compact","hooks":[{"type":"command","command":"/ABS/csift-turns-slice.sh i"}]}`, i=1..N. Absolute path (global installs have no `$CLAUDE_PROJECT_DIR`); `--window 9000` stays under the 10K additionalContext cap.

### Hook 2 — PostToolUseFailure(TaskStop): redirect a failed teammate-kill
TaskStop can't stop a teammate (wrong tool, every id form rejected — a real session burned 30 min). On TaskStop FAILURE, confirm via csift that the id is a teammate, then inject the correct call. Fail-open.

```bash
#!/usr/bin/env bash
set -uo pipefail
in=$(cat)
command -v jq >/dev/null 2>&1 || exit 0
CSIFT=$(command -v csift 2>/dev/null||true); [ -x "$CSIFT" ]||CSIFT="$HOME/.cargo/bin/csift"; [ -x "$CSIFT" ]||exit 0
[ "$(jq -r '.tool_name//empty' <<<"$in" 2>/dev/null)" = TaskStop ] || exit 0
id=$(jq -r '.tool_input.task_id//.tool_input.shell_id//empty' <<<"$in" 2>/dev/null); [ -n "$id" ]||exit 0
sid=$(jq -r '.session_id//empty' <<<"$in" 2>/dev/null); [ -n "$sid" ]||exit 0
run(){ if command -v timeout >/dev/null 2>&1; then timeout 20 "$@"; elif command -v gtimeout >/dev/null 2>&1; then gtimeout 20 "$@"; else "$@"; fi; }
tm=$(run "$CSIFT" agents "@$sid" --shape teammate --format json 2>/dev/null)||exit 0; [ -n "$tm" ]||exit 0
m=$(printf '%s' "$tm" | jq -rs --arg id "$id" '[ .[]?|..|objects|select(.shape?=="teammate") ] as $t
  | ($id|split("@")[0]) as $b | ( $t[]|select(.name==$id or .agent_id==$id or .name==$b)|.name )' 2>/dev/null | head -n1)
[ -n "$m" ]||exit 0
ctx="TaskStop cannot terminate \"$id\" — csift confirms it is the teammate \"$m\" (in-process Agent subagent, no task_id / no separate PID). Use SendMessage: {\"to\":\"$m\",\"message\":{\"type\":\"shutdown_request\",\"reason\":\"<why>\"}}. A plain message only QUEUES until its current run ends; shutdown_request is the interrupt."
jq -n --arg c "$ctx" '{hookSpecificOutput:{hookEventName:"PostToolUseFailure",additionalContext:$c}}'
```
Register: `{"matcher":"TaskStop","hooks":[{"type":"command","command":"/ABS/taskstop-teammate-redirect.sh"}]}` under `PostToolUseFailure`.

### Hook 3 — Elicitation markers: backfill the sidecar csift merges
Fires when AUQ/ExitPlanMode/MCP elicitation OPENS and CLOSES; appends pending/resolved markers to the sidecar. MUST print nothing (observe only). Verified live: the pending marker lands the instant the picker appears. A REJECTION does NOT fire `PostToolUse` (verified on real data: every observed unpaired pending was a rejected plan/AUQ), so subscribe `PostToolUseFailure` too — belt-and-suspenders for whichever closure events CC emits; csift's ghost guard drops any still-unpaired stale pending against the native transcript regardless.

```bash
#!/usr/bin/env bash
set -uo pipefail
in=$(cat 2>/dev/null) || exit 0
command -v jq >/dev/null 2>&1 || exit 0
ev=$(jq -r '.hook_event_name//empty' <<<"$in" 2>/dev/null); tool=$(jq -r '.tool_name//empty' <<<"$in" 2>/dev/null)
kind=""; phase=""
case "$ev" in
  PreToolUse)  case "$tool" in AskUserQuestion|ExitPlanMode) kind="$tool"; phase="pending";; esac ;;
  PostToolUse|PostToolUseFailure) case "$tool" in AskUserQuestion|ExitPlanMode) kind="$tool"; phase="resolved";; esac ;;
  Elicitation)       kind="mcp-elicitation"; phase="pending" ;;
  ElicitationResult) kind="mcp-elicitation"; phase="resolved" ;;
esac
[ -n "$kind" ] || exit 0
tp=$(jq -r '.transcript_path//empty' <<<"$in" 2>/dev/null); sid=$(jq -r '.session_id//empty' <<<"$in" 2>/dev/null)
key=$(jq -r '.tool_use_id // .elicitation_id // .mcp_server_name // "unknown"' <<<"$in" 2>/dev/null)
srv=$(jq -r '.mcp_server_name//empty' <<<"$in" 2>/dev/null)
sidecar=""
if [ -n "$tp" ] && [ "${tp%.jsonl}" != "$tp" ]; then sidecar="${tp%.jsonl}/elicitations.jsonl"
elif [ -n "$sid" ]; then f=$(ls "${CLAUDE_CONFIG_DIR:-$HOME/.claude}"/projects/*/"$sid".jsonl 2>/dev/null|head -1); [ -n "$f" ] && sidecar="${f%.jsonl}/elicitations.jsonl"; fi
[ -n "$sidecar" ] || exit 0
mkdir -p "$(dirname "$sidecar")" 2>/dev/null || exit 0
ts=$(date -u +%Y-%m-%dT%H:%M:%S.000Z); uuid=$( (command -v uuidgen>/dev/null 2>&1 && uuidgen) || echo "csift-$$-$ts")
if [ "$phase" = resolved ]; then
  rec=$(jq -cn --arg k "$kind" --arg key "$key" --arg sid "$sid" --arg ts "$ts" --arg u "$uuid" \
    '{type:"csift-elicitation-resolved",uuid:$u,timestamp:$ts,sessionId:$sid,csift:"elicitation-marker-v1",csiftPhase:"resolved",csiftKind:$k,csiftKey:$key}' 2>/dev/null) || exit 0
elif [ "$kind" = mcp-elicitation ]; then
  msg=$(jq -r '.message//empty' <<<"$in" 2>/dev/null); mode=$(jq -r '.mode//"elicitation"' <<<"$in" 2>/dev/null)
  rec=$(jq -cn --arg key "$key" --arg sid "$sid" --arg ts "$ts" --arg u "$uuid" --arg srv "$srv" \
    --arg content "MCP elicitation [$srv] ($mode): $msg" --argjson hi "$in" \
    '{type:"system",subtype:"mcp_elicitation",uuid:$u,timestamp:$ts,sessionId:$sid,isSidechain:false,content:$content,csift:"elicitation-marker-v1",csiftPhase:"pending",csiftKind:"mcp-elicitation",csiftKey:$key,csiftMcpServer:$srv,hookInput:$hi}' 2>/dev/null) || exit 0
else
  rec=$(jq -cn --arg k "$kind" --arg key "$key" --arg sid "$sid" --arg ts "$ts" --arg u "$uuid" --argjson hi "$in" \
    '{type:"assistant",uuid:$u,timestamp:$ts,sessionId:$sid,isSidechain:false,message:{role:"assistant",stop_reason:"tool_use",content:[{type:"tool_use",id:$key,name:$k,input:($hi.tool_input//{})}]},csift:"elicitation-marker-v1",csiftPhase:"pending",csiftKind:$k,csiftKey:$key,csiftHookEvent:"PreToolUse",hookInput:$hi}' 2>/dev/null) || exit 0
fi
printf '%s\n' "$rec" >>"$sidecar" 2>/dev/null || exit 0
```
Register 5 events (one script, absolute path): `PreToolUse`+`PostToolUse`+`PostToolUseFailure` matcher `"AskUserQuestion|ExitPlanMode"`; `Elicitation`+`ElicitationResult` (no matcher).
