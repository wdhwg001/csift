---
name: csift
description: Search and audit Claude Code session + subagent jsonl transcripts. Use when you need to find what was said/done in a past (or the current) Claude Code session — regex-search the transcript corpus under ~/.claude/projects, list sessions to identify "which session is this", inspect a session's subagent lifecycle (built-in Task agents + OMC/workflow agents), see which files/dirs a session modified and when, reconstruct a file's content from the transcript's Read/Write/Edit stream (incl. a deleted plan via recover --file @plan), locate the plan file bound to a session, identify the calling session, or supply verbatim message history (e.g. standing directives) as a supplement after a context compaction. ripgrep-for-transcripts: pure regex, no embeddings/semantic search.
user-invocable: true
---

# csift — ripgrep for Claude Code session transcripts

`csift` is a fast Rust CLI that **lists** and **regex-searches** Claude Code session `.jsonl`
transcripts under `~/.claude/projects/<encoded-cwd>/`. Its primary consumer is an LLM (a Claude
Code agent searching or recovering its own or a peer session), so output is clean,
token-efficient, regex-driven text by default, with `--format json` for machine use.

It is **pure regex / ripgrep** — explicitly **no BM25, no embeddings, no semantic search**.
Lexical scoring across scripts (CJK / multi-byte) is intractable; regex is the whole point.

**Custom Claude home.** csift reads `~/.claude` by default but honors a relocated config dir on
**every** subcommand: the global `--claude-home <DIR>` flag and Claude Code's own
`$CLAUDE_CONFIG_DIR` env var both repoint it (flag wins, then env, then `$HOME/.claude`). `<DIR>`
is the `.claude` equivalent — transcripts are read from `<DIR>/projects/<encoded>/*.jsonl`.

Binary: `csift` (build with `cargo build --release` in this repo → `target/release/csift`;
or `cargo run -- <subcommand>` during development).

---

## When to use this skill

- **"What did session X say / decide / do about Y?"** → `csift search "Y" --session <uuid>` (or a
  positional project `PATH`, like every sibling subcommand). Returns the *complete round-trip
  exchange* (the matched record plus its paired request/response), never a bare fragment.
- **"Which session is this transcript / which session was working on Z?"** → `csift list <path>`
  emits a fast identity tuple per session (first user msg, last user msg, last agent msg, cwd,
  branch, CC version) without parsing whole files.
- **"What subagents did this session spawn, when, what did they return, what did they change?"** →
  `csift agents --session <uuid>` builds the toolUseId-linked topology — each subagent's kind / TRUE
  trigger time / completion / duration / status, plus (on demand) its returned message
  (`--returned-message` or `--agent <hex>`), its files-changed (`--with-files`), and the parent→child
  tree with workflow runs as parents (`--tree`).
- **"Which files/dirs did this session modify, and when?"** → `csift files --session <uuid>` rolls up
  Edit/Write/Notebook (authoritative) + Bash (heuristic) mutations per dir/file, with create-vs-edit
  discrimination and first/last timestamps. Answers "how many distinct gap docs touched / `/tmp` docs
  created" directly.
- **"Reconstruct / restore a file (or plan) from the transcript."** → `csift recover --session <uuid>
  --file <abs>` rebuilds a file's CONTENT line-by-line from its Read/Write/Edit stream — as segmented
  diff-patches (`--patches`), a point-in-time partial snapshot (`--at`), or a coverage/scoping summary
  (`--coverage`); `--file` is required for all three. To restore a plan (DELETED ok), pass the magic
  value `--file @plan`, which resolves the session-bound plan file and rebuilds its full Write+Edit
  history under any mode. It SEGMENTS at integrity boundaries (a `modified since read` error, an
  `originalFile` disagreement, an external edit, a heuristic Bash mutation), is necessarily PARTIAL
  (unknown lines are explicit gaps, never fabricated; an errored Edit/Write never mutated the file so it
  is skipped, not applied), and every output line carries the JSONL LINE NUMBER. The motivating use:
  restore a deleted plan / a file lost in a bad-recovery.
- **"Where is this session's plan file?"** → `csift plan <uuid>` (or no target ⇒ the calling session)
  LOCATES the plan file BOUND to a session via its `plan_mode` attachment record — authoritative, not a
  path guess. To DUMP that plan's content use `csift recover --session <uuid> --file @plan`.
- **"Restore the verbatim back-and-forth a compaction summary clipped."** → `csift turns --session
  <uuid> --budget 40000` reconstructs the verbatim user/assistant TURNS, in original order, that a
  Claude Code compaction summary lossily clipped (its "All user messages" section truncates real prose
  turns to `...`-clipped bullets; the assistant side collapses to a single quote — and that single
  quote is the turn's LAST message, frequently a throwaway wrap-up rather than the substance). Per
  genuine-user turn the default keeps the LONGEST agent message (the substantive Rich Response, which
  often sits in a MIDDLE message) plus a substantive first plus the rich middles — see `--agent-msgs`.
  Recency-first selection within a char/token budget, ~50% reserved as a HARD FLOOR for complete round-
  trips, role-asymmetric middle-truncation for over-cap turns, and a backward walk that reaches across
  multiple compaction boundaries by default. Every line carries the JSONL LINE NUMBER. SUPPLEMENTS the
  summary (which owns task state) — it does not re-derive intent / the plan / the file ledger.
- **"Who am I (the calling session)?"** → `csift whoami` resolves the current session id from
  `$CLAUDE_CODE_SESSION_ID`.
- **Post-compaction recovery** → after a context compaction the conversation is replaced by a single
  recency-biased summary pass, so durable early instructions can thin out or shift in phrasing. csift
  supplies the lossless side: `csift turns` re-emits the verbatim TURN back-and-forth the summary
  clipped (automating its own "read the full transcript at `<path>`" pointer), and a summary-vs-jsonl
  diff can surface standing directives as a SUPPLEMENT to the summary — see "Integration recipes → (A)".

Reach for csift instead of hand-grepping `~/.claude/projects/**/*.jsonl`: it understands the
record model (genuine-user vs tool_result-carrier, thinking/tool/agent categories), spans subagent
transcripts, and reconstructs whole turns rather than emitting line fragments.

---

## Command surface

Nine subcommands: `list`, `search`, `agents`, `whoami`, `files`, `recover`, `plan`, `turns`, `image`.
`search` doubles as the message-fetcher (`--line`/`--uuid` address specific records, rendered full).
`image` lists + extracts a session's inline images by the `#N` handle it uses (or the exact
`L<line>i<n>` locator); `--out <dir>` decodes them back to files.
`list`/`search`/`files`/`recover`/`image` span each session's subagent transcripts **by default**
(`--no-subagents` opts out). On these four `--include-subagents` is a **default-ON no-op** kept only
for symmetry/explicitness — it never changes the result, and `--no-subagents` is **DOMINANT**: when
present it always wins, regardless of flag order (passing `--include-subagents` last does not
re-enable the fan-out you suppressed). `turns` is the exception — a single-thread recovery tool whose
per-session budget MULTIPLIES, so it defaults to the **top-level thread only** and opts INTO spanning
via `--include-subagents` (there it is load-bearing, last-flag-wins). (`agents` LISTS subagents as its
targets, so it has no span flag.) `plan` also spans subagents by default (surfacing their own bound
plans) with `--no-subagents` to opt out, but carries only `--no-subagents` / `--format` (no
`--include-subagents`, no SCOPE banner). Every subcommand takes `--format text|json` (default `text`).

**Span disclosure (uniform across every spanning subcommand).** Whenever a bare-uuid invocation
fans out across ≥1 subagent, the surface announces the span UP FRONT, identically:
- TEXT: a leading `scope  N sessions in scope (X top-level + Y subagent)` banner (on
  `list`/`files`/`search`/`recover`; `turns` uses the same wording plus its budget clause).
- JSON: a leading `{kind:"session_header", sessions_in_scope, top_level_sessions, subagent_sessions}`
  record (on `list`/`files`/`search`/`recover`/`turns`).
Both are SUPPRESSED under `--no-subagents` / a single-transcript scope. A `--subagents-only` flag
mistyped onto a non-`files` subcommand gives a pointed "that's a `files`-only flag" error.

A **target** is EITHER a real filesystem cwd (csift path-encodes it for you) OR an already-encoded
`-Users-...` projects-dir token OR a direct `~/.claude/projects/<encoded>` path. With no target,
the whole corpus (every project) is scanned.

### `list` — session identity index

```
csift list [PATH...] [--no-subagents] [--format text|json]
```

Per session jsonl it emits: session id, FIRST genuine-user message (+ time), LAST genuine-user
message (+ time), LAST agent message (+ time), decoded cwd / git branch / CC version. A forward
HEAD read finds the first user; a backward TAIL read finds the last — neither parses the whole
file, so it stays fast on 200 MB+ transcripts. `PATH` is repeatable; default = all projects.

Examples:

```bash
csift list                                                  # every session, all projects
csift list .                                                # sessions for the current cwd's project
csift list /Users/testuser/Projects/widget_app_prototype  # a real path (gets encoded)
csift list -Users-testuser-Projects-widget-app-prototype  # a pre-encoded dir token
csift list --no-subagents .                                 # top-level sessions only, no subagents
csift list --format json .                                  # machine-readable index
```

Text output shape (`◂` = user side, `▸` = agent side; timestamps are system-local + raw UTC):

```
SESSION  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  cwd      /Users/testuser/Projects/widget_app_prototype   (branch main, CC 2.1.133)
  first ◂  2026-05-09 00:27:08 AEST (2026-05-08T14:27:08.769Z)
           Audit `/Users/testuser/Projects/widget_app_prototype`… (+2296 chars)
  last ◂   2026-05-09 00:27:08 AEST (2026-05-08T14:27:08.769Z)
           …
  last ▸   2026-05-09 00:31:26 AEST (2026-05-08T14:31:26.214Z)
           …
```

JSON: a leading `{kind:"session_header", sessions_in_scope, top_level_sessions, subagent_sessions}`
scope record when the scope spans subagents, then one object per session (`session_id`, `is_subagent`,
`parent_session_id`, `path`, `cwd`, `version`, `git_branch`, `first_user`, `last_user`, `last_agent`,
`skipped_lines`). A subagent row carries `is_subagent:true` + the re-feedable `parent_session_id`; the
default spans subagents, so the text output leads with a `SCOPE` banner and brands each subagent row
`SUBAGENT <hex> · parent SESSION <uuid>` (add `--no-subagents` for just the top-level row).

### `search` — regex over transcripts, complete round-trip per hit

```
csift search [PATTERN] [PATH...] [--session ID] [--no-subagents] [--subagent HEX]
             [-t|--category thinking|user|tool|tool-response|agent]...
             [-i|--ignore-case] [--multiline]
             [--turn-range START..END] [--since WHEN] [--until WHEN]
             [--line SPEC]... [--uuid U]...   # address specific records (the message-fetcher)
             [--max-count N] [-c|--count] [-l|--files-with-matches]
             [--siblings] [--sibling-category CAT]... [--full|--no-truncate]
             [--resolve-persisted] [--format text|json]
```

- **PATTERN** is ripgrep-like, **smart-case** by default (case-insensitive unless it contains an
  uppercase letter; `-i` forces case-insensitive and always wins). `--multiline` lets `.` cross
  newlines. PATTERN MAY be empty — then it is a pure filter, matching every category-eligible
  record (combine with `--category` / `--since` / `--turn-range`; a bare empty pattern with no
  other filter warns it will emit a lot).
- **Scope target** is a POSITIONAL `[PATH]...` — the SAME unified surface every session-operating
  subcommand uses (`list`/`agents`/`files`/`recover`/`turns`; `csift search PATTERN .`). The legacy
  `--path <PATH>` flag still works on `search` as a hidden deprecated alias (no other subcommand has
  a `--path` flag). A bare session-UUID (8-4-4-4-12 hex) in the positional slot is routed to
  `--session` on ALL of them — including `list` (now unified; `csift list <uuid>` scopes to that one
  top-level session, spanning its subagents by default — add `--no-subagents` for just the single
  row) — and is searched across all projects when no project path is given. A bare **subagent hex**
  is NOT accepted as a positional (it never names a top-level jsonl); inspect one subagent with
  `csift agents --agent <hex>`, or pass the PARENT session uuid. `whoami` is the exception: it takes
  NO target (it reads `$CLAUDE_CODE_SESSION_ID`, falling back to `CODEX_COMPANION_SESSION_ID`).
- **Flag ordering** — the argv pre-pass routes declared flags (LONG and the search short flags
  `-t`/`-i`) away from the `[PATH]...` positional, so a flag works in ANY position, including
  trailing: `search PATTERN <path> -t user` and `search PATTERN <path> --format json` both parse.
- On a hit, csift returns the **whole turn** (a turn opens on a genuine user message, an answered
  AskUserQuestion, or a plan-rejection-with-message): a matched `tool_use` comes WITH its
  `tool_result`; a matched user turn WITH the agent's response.
- **`--category`/`-t`** (repeatable; with none given, all five are eligible) — see the category
  model below.
- **Windowing**: `--turn-range START..END` (inclusive, 0-based on turn-boundary order) is mutually
  exclusive with `--since`/`--until`. `--since`/`--until` accept ISO8601 (`2026-06-01`,
  `2026-06-01T05:00:00Z`) or a relative form (`2h`, `3d`, `90m`, `45s`, `1w`) meaning "that long
  ago" in the **system-local timezone**. A record with no timestamp never falls inside a bounded
  window.
- **`--max-count N`** caps emitted exchanges but **reports the dropped count** — there is NO silent
  truncation anywhere.
- **Always-on totals.** Every normal footer carries BOTH cheap totals: the match count AND the
  distinct-session count (`matched N exchanges across S sessions …`; JSON footer
  `{matched, sessions, dropped_by_cap, skipped_lines}`). You rarely need `-c`/`-l` — they just
  isolate one of those two totals for a pipe, and are **mutually exclusive** with each other.
- **`-c`/`--count`** prints ONLY the integer match total (the ripgrep `-c` idiom for "how many times
  X?") — no per-exchange output. Honors every filter and reports the TRUE total even when
  `--max-count` would cap the listing; `--format json` prints `{"matched":N}`.
- **`-l`/`--files-with-matches`** prints ONLY the distinct sessions that matched, one id per line
  (the ripgrep `-l` idiom for "WHICH sessions mention X?") — a re-feedable top-level uuid, or a bare
  subagent hex annotated with its `parent <uuid>`. Unaffected by `--max-count`; mutually exclusive
  with `-c`. `--format json` emits one `{session_id,is_subagent,parent_session_id}` per line.
- **`--siblings`** also renders the SIBLING records of each matched turn — the rest of the
  back-and-forth, not just the matched line — so a matched **user** question surfaces WITH the agent's
  reply (answers "I said X, what did you ask back?" without dropping to the raw jsonl). Sibling rows
  render under a `·` marker. By default the siblings shown are every category EXCEPT the match `-t`
  set (or ALL when no `-t`); **`--sibling-category CAT`** (repeatable, implies `--siblings`) narrows
  them. A record that itself matched is never repeated as a sibling.
- **`--full`** (alias `--no-truncate`) emits each matched (and `--siblings`) record's FULL text
  instead of the ~400-char centered excerpt — so you can READ a found message end-to-end (e.g. the
  question at the TAIL of a long reply) without dropping to the raw jsonl. Newlines are still
  collapsed to single spaces (one line per record); the explicit `… (+N chars)` marker disappears
  because nothing is clipped.
- **`--resolve-persisted`** resolves `<persisted-output>` pointers to their `tool-results/<id>.txt`
  file (large tool outputs are externalised).

Examples:

```bash
csift search "carry"                                  # all projects, smart-case
csift search "carry" .                                # this project (positional PATH, like every sibling)
csift search -i "askuserquestion" -t tool             # tool_use blocks naming AUQ
csift search "" -t user --since 2h .                  # user turns, last 2h, this project
csift search "tail.read" --multiline --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
csift search "panic" -t agent -t thinking --turn-range 10..20 --max-count 50
csift search "refactor" -c                            # COUNT matches only ("how many times?")
csift search "refactor" -l                            # LIST sessions that match ("which sessions?")
csift search "let's chat" -t user --sibling-category agent  # the match WITH the agent's reply
csift search "let's chat" -t user --sibling-category agent --full  # …and READ the reply in full
csift search "persisted-output" --resolve-persisted --format json
```

**Result ordering — a combined, stable chronological timeline.** Exchanges are emitted in ASCENDING
turn-opening time across the WHOLE scope, with subagent hits INTERLEAVED among top-level hits by absolute
timestamp (both clocks are the same machine's UTC) — NOT grouped by file. The order is stable/deterministic
(timestamp-less exchanges sort last, with a reproducible tie-break), and the global `--max-count` cap is
applied AFTER the sort (it keeps the EARLIEST N and reports the dropped remainder — never silent). Each
result carries its chronological position: the text header appends the turn-opening timestamp, and the JSON
envelope carries `ts_utc`/`ts_local`.

Text output shape (token-lean: each session's full id is printed ONCE in a label table, then every
exchange references the cheap `s<N>·t<turn>` label — an LLM follows the reference for free):

```
s1 = 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
s2 = 7f3c9e21 (subagent · parent s1)

s1·t0  2026-06-03 22:51:53.206+10:00
  ◂ user  L412  Audit harness-correctness in the worktree … (+2958 chars)
  ▸ tool-response Edit  L613  the file … was updated successfully

matched 2 exchanges · 1 session · category=user · 18 dropped by --max-count
```

`L412` is the hit record's 1-based physical line in its session jsonl — a STABLE address (jsonl is
append-only). The timestamp is a single local instant with offset + milliseconds (no second UTC copy).
A `tool-response` row names the tool it answers (`tool-response Edit`). Read a truncated tail with
`--full`, or **fetch the exact message(s) by address**: `csift search "" 0a1b2c3d --no-subagents
--line 412` (or `--line 400-420` for the span, or `--uuid <id>`). Addressing renders records FULL and
reports any explicitly-requested address that resolved to nothing as an `unresolved: L<line>` line.

JSON: a leading `{kind:"session_header", sessions_in_scope, top_level_sessions, subagent_sessions}`
scope record when the scope spans subagents, then one object per exchange (`session_id`, `is_subagent`,
`parent_session_id`, `turn_index`, the envelope-level `ts_utc`/`ts_local` = the turn-opening timestamp
the timeline is sorted on, `hits[]` with `{category, excerpt, ts_utc, ts_local, tool_name, line, uuid}`
— `line`/`uuid` are the per-record address, `tool_name` is set on a `tool-response` to the tool it
answers, and a per-hit `ts_utc` may be LATER than the envelope's for a deep tool_use match — and
`record_uuids[]` = every record stitched into the round-trip), then a trailing summary object
`{matched, sessions, dropped_by_cap, skipped_lines, unresolved}` (`unresolved` lists explicit
`--line`/`--uuid` addresses that matched no record; empty in a normal search).
With `--siblings` the envelope also carries a `siblings[]` array (same per-hit shape) for the turn's
non-matched records, present only when there are any. (`-c`/`-l` short-circuit this shape: `-c` prints
`{"matched":N}`; `-l` prints one `{session_id,is_subagent,parent_session_id}` per line, no footer.)
See **Output formats** below for a concrete record + jq/python.

#### Regex dialect — linear-time (RE2-class)

The `search` PATTERN is the Rust **`regex` 1.12** crate (`regex::bytes`), which **guarantees
linear-time matching** in the input length — there is **no catastrophic backtracking, ever**. That
is a deliberate boundary: the constructs that force a non-linear engine are not supported, and a
pattern using one **fails to compile** with a clear error (by design, not a bug).

| Supported | NOT supported (need a non-linear engine) |
| --- | --- |
| literals; classes `[...]` / `[^...]` / `\d \w \s` + Unicode `\p{...}` | backreferences `\1` |
| alternation `\|`; groups `(...)` / non-capturing `(?:...)` | lookahead / lookbehind `(?=) (?!) (?<=) (?<!)` |
| quantifiers `* + ? {m,n}` (greedy + lazy `*?`) | atomic groups / possessive quantifiers `(?>...)` / `a*+` |
| anchors `^ $ \b \B`; dot `.` (`--multiline` lets it cross newlines) | |
| inline flags `(?i)(?m)(?s)(?x)`; Unicode-aware by default | |

Case is **smart-case** by default (insensitive unless the pattern has an uppercase letter); `-i`
forces insensitive. `--multiline` lives in the same dialect (it sets the `(?s)(?m)` flags).

### `agents` — a session's subagent TOPOLOGY

```
csift agents [PATH...] [--session ID] [--kind builtin-task|workflow]...
             [--since WHEN] [--until WHEN] [--by trigger|start|completion]
             [--tree] [--agent HEX] [--with-files] [--returned-message]
             [--format text|json]
```

Builds the toolUseId-LINKED topology of the subagents a session spawned: each subagent joined back
to the parent `Agent`/`Task`/`Workflow` `tool_use` that triggered it. Per node: id, kind, the TRUE
trigger time (the parent tool_use ts — NOT the lagging child-head ts), start + completion, duration,
status (`completed`/`running`/`unknown`), and — on demand — the 3-way-resolved returned message and
files-changed. `--tree` shows workflow RUN nodes (from the top-level `workflows/wf_*.json`
manifests) as parents of their agents.

The 6 queries this answers: **count/where/when** (flat list + `--since`/`--until`), **the topology
tree** (`--tree`), **grab one subagent's returned message** (`--agent <hex>`), **every node's
returned message** (`--returned-message`), **a node's files-changed** (`--with-files`), and the
**time-filter** on the true trigger axis (the default).

Returned message is resolved 3 ways: **sync built-in** → the parent tool_result text;
**async built-in** (the `Async agent launched …` sentinel) → the child transcript tail;
**workflow** → the `journal.jsonl` `result` payload (source reported as `sync-tool-result` /
`async-child-tail` / `workflow-journal`).

`--since`/`--until` default to the **trigger** axis (the true spawn instant); `--by start|completion`
switch axis.

**Id-domain across surfaces (uniform).** A subagent transcript's bare-hex id is NOT a re-feedable
`--session` target, so every surface that emits a per-transcript identity also carries the
re-feedable **owning uuid**:

- `agents` keys on `agent_id` + `parent_session_id` (no overloaded `session_id`).
- `search` / `files` / `list` / `recover` / `turns` JSON each emit `session_id` (the transcript's
  own id — a top-level uuid, or a bare subagent hex) **plus** `is_subagent` + `parent_session_id`
  (the always-re-feedable owning uuid; `== session_id` for a top-level record). Re-feed
  `parent_session_id`, never a subagent `session_id`.
- Text headers brand a subagent block **uniformly** as `SUBAGENT <hex> · parent SESSION <uuid>` on
  ALL of `search` / `files` grouped views / `list` / `turns` (turns appends a `(subagent transcript)`
  suffix). The bare subagent hex is never tokened `SESSION`. `list`/`files`/`search`/`recover`/`turns`
  lead with a `SCOPE` banner when the scope spans subagents.

So a file mutation (or any per-transcript record) is joinable back to its node by structured field
on **all** surfaces — no `path`-string parsing required.

Examples:

```bash
csift agents --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d      # one session's subagent topology
csift agents . --kind workflow                                   # only workflow agents
csift agents --session <uuid> --since 2h                         # subagents TRIGGERED in the last 2h
csift agents --session <uuid> --since 6h --by completion         # filtered on COMPLETION time (last 6h)
csift agents --session <uuid> --tree                             # parent→child tree (runs as parents)
csift agents --session <uuid> --agent <hex> --with-files         # grab one subagent: returned msg + files
csift agents --session <uuid> --returned-message --format json   # every node's returned message
```

Text output shape (`(wf_…)` = workflow id, `[…]` = agentType sub-label):

```
SESSION  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  ae24045bd6d4bdaff  workflow  (wf_35cd8400-f04)  [Explore]  completed
    triggered  2026-05-29 19:53:14 AEST (2026-05-29T09:53:14.002Z)
    started    2026-05-29 19:53:16 AEST (2026-05-29T09:53:16.201Z)
    completed  2026-05-29 19:54:34 AEST (2026-05-29T09:54:34.593Z)
    duration   1m20s

1 subagent(s)  ·  kind=all  ·  window-axis=trigger
```

`--tree` adds a `WORKFLOW  wf_<id>  [name]  status` header above each run's agents (with
`agents`/`duration`/`tokens`/`model` lines).

JSON: one object per node (`agent_id`, `kind`, `parent_session_id`, `parent_agent_id`,
`spawn_tool_use_id`, `spawn_tool`, `workflow_id`, `agent_type`, `description`, `trigger_utc`,
`trigger_local`, `started_utc`, `started_local`, `completed_utc`, `completed_local`, `duration`,
`status`, `depth`, `skipped_lines`; plus `returned_message`/`returned_message_source` with
`--returned-message`/`--agent`, `files_changed[]` with `--with-files`). `--tree` JSON emits one
object per session: `{session_id, workflow_runs:[{…, children:[node]}], agents:[node]}`.

`agent_type` is the semantic agent **ROLE** / subagent-type string (e.g. `Explore`,
`general-purpose`, `oh-my-claudecode:critic`, `workflow-subagent`) — DISTINCT from `kind`, which is the
on-disk transcript **SHAPE** (`builtin-task` | `workflow`). There is no role filter today (only
`--kind` filters, by shape).

### `whoami` — identify the calling session

```
csift whoami [--show-path] [--format text|json]
```

Resolves the calling Claude Code session from **`$CLAUDE_CODE_SESSION_ID`** (Claude Code exports it
into every Bash-tool environment; its value equals the calling session's own jsonl basename
exactly). That is the PRIMARY signal csift trusts — per-session, version-independent, zero
false-positives — with `CODEX_COMPANION_SESSION_ID` (the Codex companion plugin's alias) accepted only
as a fallback when the canonical var is absent. When NEITHER var is
absent/empty (an old CC build, or running outside CC) whoami **does NOT guess** — most-recent-mtime
is a false-positive trap with concurrent sessions — it exits non-zero with guidance to pass
`--session <uuid>`. `--show-path` (boolean; legacy alias `--path`) prints the resolved jsonl path;
`--format json` emits `{"session_id":"…","path":"…"}`. whoami JSON INTENTIONALLY carries only
`{session_id, path}` — it does NOT include `is_subagent`/`parent_session_id` (unlike
list/search/files/recover/turns JSON). Inside a subagent the resolved id is the SUBAGENT's own id;
to learn whether it is a subagent + find its parent, feed it to `csift agents --agent <id> --format
json` and read `parent_session_id`.

> **SUBAGENT CAVEAT:** inside a Task/Agent subagent, `$CLAUDE_CODE_SESSION_ID` is the SUBAGENT's own
> id, not the parent/root session — so `whoami` there identifies the subagent. To reach the root
> session from inside a subagent, run `agents`/`list` on the project path to find the parent uuid.
> Note `whoami --show-path` is a BOOLEAN toggle, unlike the scope-target `--path <PATH>` on the
> session-operating subcommands.

```bash
csift whoami                  # print the calling session's uuid (+ its jsonl path if found)
csift whoami --show-path      # always show the resolved jsonl path (or a not-found note)
csift whoami --format json
```

If `whoami` errors that `CLAUDE_CODE_SESSION_ID is not set`, install the remediation hook in
"Integration recipes → (B)".

### `files` — which files/dirs a session modified

```
csift files [PATH...] [--session ID] [--no-subagents | --subagents-only]
            [--summary | --by-dir | --by-file | --timeline]
            [--turn-range START..END] [--since WHEN] [--until WHEN] [--format text|json]
```

Reports the files + directories a session changed, and when. Mutations are extracted from the
transcript (spanning subagents **by default** — OMC fan-out edits happen in subagents) with an
**authoritative vs heuristic** split:

- **Authoritative** — `Edit` / `Write` / `MultiEdit` (`input.file_path`) + `NotebookEdit`
  (`input.notebook_path`). create-vs-edit is resolved from the paired `tool_result`
  (`toolUseResult.type == "create"` = a new file).
- **Heuristic** — `Bash` file mutations, parsed **lexically** from the command string. Covers the
  verb allowlist (`rm`/`mv`/`cp`/`install`/`ln`/`rsync`/`mkdir`/`touch`/`tee`/`sed -i`/`git`/`dd
  of=`/`zip`/`tar -c` (a create flag → the `-f` archive is the written path; extract/list write
  nothing)), plain **and fd-qualified** redirects (`>`/`>>`, plus `2>`/`1>`/`&>` and their appends
  and the noclobber-override `>|` — a `2>&1` fd-dup and `/dev/null` sinks are correctly ignored),
  `curl`/`wget` output flags (`-o`/`-O`/`--output`), and allowlisted flag outputs
  (`--junit-xml=`/`--junitxml=`/`--report-path`/…). GNU `-t DIR` (cp/mv/install) is handled — the
  destination is the `-t` value, the positionals are read sources. Only **concrete, resolvable**
  paths are emitted — an unexpandable `$VAR`/`~` pseudo-path, a `/dev/null`-class sink (even with a
  glued command-substitution `)`), a process-substitution `>(…)` (its body args too), and a
  quote-severed fragment are all dropped, never fabricated. The parser is
  **quote/backtick/procsub/arith-aware**: a `>`/`<` (or a word) inside a quoted echo/printf prose or
  a quoted regex (`echo "idle >8min"`, `grep 'cur > base'`), inside a backtick command substitution
  (`` `date > /tmp/f` `` — the inner redirect is masked AND the closing backtick never glues onto a
  path), or inside an arithmetic/test comparison (`(( a > b ))` / `[[ a > b ]]` — the `>` is a
  comparison, not a redirect) is masked before redirect detection, so none of them fabricate a file. **Known limitation:** a write inside an embedded-language body (a heredoc,
  or `python -c "open('/tmp/x','w')…"`) is NOT parsed — out of scope for a lexical parser, so such
  writes are missed. The precision contract holds (the miss is **never mis-reported**): heredoc BODY
  lines are lexically skipped before redirect/verb scanning, and quoted/procsub spans are masked, so
  a `>` or quote inside them cannot fabricate a redirect row. A trailing OUTPUT redirect
  (`2>&1`, `2>/dev/null`, a spaced `> /tmp/log`/`>> /tmp/log`) is **removed from the operand stream**
  before verb dispatch (symmetric to input-redirect `<` handling), so it does not displace a real
  `cp`/`mv`/`ln`/`install`/`rsync` destination, mislabels a source, or double-emits the redirect path.
  Bash carries no path field in its result, so all of the above are best-effort and **always labelled
  `(heuristic)`**.

**Subagent scope** (mutually exclusive): default spans subagents; `--no-subagents` reports only the
top-level session's own mutations; `--subagents-only` is the **complement** — only the files the
session's subagents created/modified, with the top-level session excluded (one command for the
"what did the fan-out touch?" set-difference).

Exactly one **detail level** applies (mutually exclusive; default `--summary`):

- **`--summary`** (DEFAULT) — coarse **top-level-prefix** rollup: each path buckets on its first
  few directory segments, so a whole project tree collapses to ONE row. The smallest output, and
  **strictly coarser** than `--by-dir` (a real 4-level ladder: summary < by-dir < by-file <
  timeline). `git:<sub>` pseudo-paths roll up under their own `git:` bucket, out of the `./`
  relative sink.
- **`--by-dir`** — one row per distinct directory (the **full** parent path — finer than summary)
  with per-op counts + distinct-file count + first/last.
- **`--by-file`** — one row per distinct file (per-op counts + first/last touch).
- **`--timeline`** — full chronological list, one line per mutation. **Heavy — opt-in only.**

All levels honor `--turn-range` / `--since` / `--until` (same windowing semantics as `search`; a
mutation with no timestamp never falls inside a bounded window) and `--format json`.

Examples:

```bash
csift files <uuid>                          # default summary: coarse top-level-prefix op rollup
csift files <uuid> --by-file                # per-file op counts + first/last touch
csift files <uuid> --subagents-only --by-file   # ONLY what the session's subagents touched
csift files <uuid> --timeline --since 2h    # full chronological, last 2h (heavy)
csift files . --format json --by-dir        # machine-readable per-dir rollup
```

**Acid test — "how many distinct gap docs did this session touch, and how many `/tmp` docs did it
create?"** — `csift files <uuid> --by-file` lists one row per file (count rows ending in a
`gaps`-style doc). For the create count, use `--timeline --format json` and filter `/tmp` rows with
`is_create == true` (optionally AND `op` in `{write, multi_edit, notebook_edit}`). **There is no `op`
value `create`** — `op` is one of `{bash, edit, write, multi_edit, notebook_edit}`; create-vs-edit is
the SEPARATE `is_create` boolean. Both `op` and `is_create` live **only** in the `--timeline` JSON;
`--by-file` rows carry per-op COUNTS (`write`/`edit`/`bash`/…), not the create flag. The `--summary`
view shows the same as bucket op-counts + distinct-file counts.

Text output shape (Bash counts suffixed `(heuristic)`):

```
SESSION 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  /p/spec/gaps: 3 edit
  /tmp: 2 write, 1 bash (heuristic)

5 distinct file(s)  ·  6 mutation(s)  ·  detail=summary  ·  all turns
(Bash mutations are heuristic — parsed from the command string.)
```

JSON: a leading `{kind:"session_header", sessions_in_scope, top_level_sessions, subagent_sessions}`
scope record when the scope spans subagents (uniform with list/search/recover/turns), then one object
per emitted unit (bucket / dir / file with `{session_id, is_subagent,
parent_session_id, <key>, write, edit, notebook_edit, multi_edit, bash, total, distinct_files,
first_utc, first_local, last_utc, last_local}`; or per mutation for `--timeline` with `{session_id,
is_subagent, parent_session_id, path, op, ts_utc, ts_local, turn_index, is_create, heuristic}`), then
a trailing summary object
`{distinct_files, total_mutations, skipped_lines, detail_level}`. The `is_subagent`/`parent_session_id`
discriminators ride EVERY view (grouped + timeline); a subagent group's TEXT header is branded
`SUBAGENT <hex> · parent SESSION <uuid>` (re-feed the parent uuid). The on-wire `op` value is
UNDERSCORE-delimited (`notebook_edit`/`multi_edit`), the SAME spelling as the grouped per-op count
keys — so a script never special-cases the delimiter across the two modes (the human-readable TEXT
timeline still shows the hyphenated `notebook-edit`/`multi-edit`). **`heuristic`** is `true` ONLY for a
bash-derived mutation (a guessed path/op lexically parsed from a shell command, lower confidence);
`false` = a definitive Edit/Write/Notebook/MultiEdit tool call with an exact `file_path`. Filter
`heuristic==false` for confirmed mutations only. Bash mutation extraction strips shell `#` comments
(an unquoted `#` at a word boundary → end-of-line), so a trailing `# note` neither fabricates a path
nor displaces a real cp/mv destination; an in-path `#` (`/tmp/a#b`) is preserved.

### `recover` — reconstruct a file's content (incl. a plan via `--file @plan`)

```
csift recover [PATH...] [--session ID] --file <ABS_PATH|@plan> [--no-subagents]
              [--patches | --at WHEN | --coverage(--dry-run)]
              [--turn-range START..END] [--since WHEN] [--until WHEN]
              [--line-range START..END] [--out PATH] [--format text|json]
```

Where `files` reports THAT a file changed, `recover` rebuilds its **content** by replaying the
file's Read / Write / Edit stream in transcript order. Three mutually-exclusive modes (default
`--patches`); `--file` is **required** for all three:

- **`--patches`** (DEFAULT) — segmented unified-diff history of `--file`, split at **integrity
  boundaries** where reconstruction across them is invalid: a `modified since read` harness error
  (authoritative), an `originalFile` that disagrees with the replayed buffer (authoritative — the
  signal other recovery tools discard), an external `edited_text_file` (authoritative), or a Bash
  mutation (heuristic, always flagged). No diff spans a boundary.
- **`--at WHEN`** — the **partial, line-numbered "in the LLM's eyes" snapshot** as of a cutoff
  (ISO8601, relative `2h`, `@turn:<N>` = first line after genuine-user turn N, or `@line:<N>` =
  JSONL TRANSCRIPT line N — the `Lnnnnn`/`line_no` this tool prints, **NOT** a file line of `--file`;
  for a 1-based FILE-line span use `--line-range`). Known lines carry their number; unknown
  regions are explicit `??? lines A..B unknown` markers — **gaps are NEVER fabricated**.
- **`--coverage`** (alias `--dry-run`) — scope a recovery without dumping content: recoverable line
  ranges, where the boundaries sit, per-op counts (reads / edits / writes / bash / external-edits),
  fragment count.

**`--file @plan` (a magic VALUE, not a mode).** `@plan` is bash-safe (no shell metacharacters, no
escaping in mixed scripts — consistent with `--at`'s `@line:`/`@turn:` sigils). It resolves the target
session's BOUND plan file (via the `plan_mode` attachment — see the `plan` subcommand) and reconstructs
THAT file exactly like any other: its FULL Write+Edit history, edit-aware (NOT just the latest Write). It
composes with every mode (`--patches`/`--at`/`--coverage`) and with `--out`/`--format`, so it is how you
DUMP a plan's content — including a DELETED plan rebuilt from the transcript alone. It prefers the
top-level session's own plan and **ERRORS clearly (never guesses)** when no plan is bound to the target,
or when the target spans sessions bound to DIFFERENT plans (the error asks for `--session`). A subagent
transcript surfaces as `SUBAGENT <hex> · parent SESSION <uuid>` in recover text (never a bare-hex
`SESSION`).

`--out` writes the reconstructed artifact (snapshot / plan / concatenated patches) verbatim to a file
while the summary still prints to stdout — but is **ignored in `--coverage` mode** (coverage is a scoping
summary, no artifact; a stderr note makes the no-op visible). `--out` is **data-safe on an empty
result**: when nothing is reconstructed (no recoverable history / over-budget), the destination is
left UNTOUCHED (never truncated to 0 bytes) and no false `(wrote …)` line is printed — a stderr
`note: nothing reconstructed … left untouched` fires instead (same guard across `--patches`/`--at`,
their JSON twins, and `turns`). `--line-range` (a 1-based FILE-line span of `--file`) applies in all
three modes. **Every output line carries the JSONL line number** (`Lnnnnn`) so you
can `Read` the raw jsonl directly. Reconstruction is necessarily PARTIAL: an un-anchorable edit (its
old text over an unknown gap, or whose context disagrees with the buffer) is a counted coverage hole,
never a fabricated line — so the contiguous-from-line-1 prefix matches the on-disk file exactly. An
Edit/Write whose result ERRORED (`is_error:true` — `String to replace not found in file`, or the
Edit-before-Read wall `File has not been read yet` for a Bash-created-then-directly-Edited file, or a
plan already in context that must be re-Read before it can be Edited) never mutated the file, so it is
SKIPPED (not applied, not counted as a recoverable edit; a not-read-yet case is an integrity annotation).

Examples:

```bash
csift recover . --file /abs/PLAN.md --coverage             # scope first: covered ranges + boundaries
csift recover <uuid> --file /abs/app.py --patches          # segmented unified diffs over the session
csift recover <uuid> --file /abs/app.py --at @turn:42      # partial snapshot as the LLM saw it at turn 42
csift recover <uuid> --file @plan --out /tmp/restored-plan.md  # rebuild the session's bound plan (DELETED ok)
```

JSON is NDJSON: a leading `{kind:"session_header", sessions_in_scope, top_level_sessions,
subagent_sessions}` scope record when the scope spans subagents, then one object per segment /
boundary / snapshot (every object carries `session_id` + the id-domain discriminators
`is_subagent` + `parent_session_id`, plus `line_no` + `ts_utc`/`ts_local`; `--at` lines carry
`set_at_line` provenance), then a trailing summary. Re-feed `parent_session_id`, never a subagent
`session_id`.

**Use it to** restore a plan (via `--file @plan`) a compaction or bad-recovery dropped, extract a file's
diff-history over a turn/time range, or check (via `--coverage`) whether a file is even worth attempting
to reconstruct and where it will break — before dumping anything.

---

### `plan` — locate the plan file BOUND to a session

```
csift plan [PATH-or-session] [--session ID] [--no-subagents] [--format text|json]
```

LOCATES (does not dump) the plan file a session is bound to. To DUMP that plan's content — a DELETED one
included — use `csift recover --session <uuid> --file @plan` (above), which resolves the SAME binding.

The binding is **AUTHORITATIVE, not a path heuristic**: when a session enters Plan Mode, Claude Code
writes a `plan_mode` attachment record
(`{"type":"attachment","attachment":{"type":"plan_mode","planFilePath":"…","isSubAgent":…,"planExists":…}}`),
and that `planFilePath` IS the session's plan. This matters because a session may freely Edit/Write OTHER
sessions' plan files (ordinary tool calls on a `~/.claude/plans/…` path) — those are NOT its own plan,
and a path guess would mis-attribute them. Plans live flat under `~/.claude/plans/` with a random
three-word name (`nested-prancing-popcorn.md`; a subagent's plan gets an `-agent-<hex>` suffix); the name
is NOT derivable from the session id — only the attachment binds them.

Target a project PATH / encoded-dir / bare session-UUID (positional) or `--session <uuid>`; with **no
target** it resolves the CALLING session from `$CLAUDE_CODE_SESSION_ID` (like `whoami`). It spans
subagents by default (their own plans surface, flagged subagent with the re-feedable parent uuid);
`--no-subagents` restricts to the top-level session. Per resolved session it emits `session_id`,
`is_subagent`, `parent_session_id`, `plan_file`, `plan_exists` (on disk), `line_no` (the jsonl line of
the binding attachment); text or `--format json` (NDJSON, one object per plan).

Examples:

```bash
csift plan                                  # the calling session's bound plan (resolves CLAUDE_CODE_SESSION_ID)
csift plan <uuid>                           # a specific session's bound plan
csift plan . --no-subagents --format json   # this project's top-level sessions, machine-readable
csift recover --session <uuid> --file @plan # DUMP the plan's content (plan only LOCATES it)
```

---

### `turns` — reconstruct the verbatim back-and-forth a compaction summary clipped

```
csift turns [PATH...] [--session ID] [--budget N] [--budget-unit chars|tokens]
            [--round-trip-fraction F] [--max-compactions N]
            [--agent-msgs longest|eot-only|rich|all] [--agent-run-threshold N]
            [--agent-rich-min-chars N] [--agent-declaration-max-chars N]
            [--keep-first | --no-keep-first] [--profile heavy|light]
            [--include-subagents | --no-subagents]
            [--turn-range START..END] [--since WHEN] [--until WHEN]
            [--out PATH] [--slice N] [--window N] [--format text|json]
```

A Claude Code compaction summary preserves task STATE but loses TURN fidelity: its "All user messages"
section clips real prose turns to `...`-truncated bullets, and the assistant side collapses to a single
§9 quote (the turn's LAST message). `turns` SUPPLEMENTS the summary by re-emitting the clipped user
phrasings + the substantive agent replies, in original order, each line carrying the jsonl `Lnnnnn` so
you can `Read` the raw record. Per turn the default surfaces the LONGEST agent message rather than the
last — see the richness model below for why.

**Automation triggers.** In automation-heavy sessions (OMC workflows, background commands), Claude Code
injects `<task-notification>` records that LOOK like user turns (they open a turn) but are machine
completion notices, not the operator's prose. `turns` (and `search -t user`) CLASSIFY these: the opener
renders as a parsed `[<kind> <task-id> <status>] <summary>` ATTRIBUTION label — where `<kind>` is the
TRUE trigger class read from the summary (`background-command` / `workflow` / `agent` / `monitor` /
`task`, where `monitor` matches a `<task-notification>` whose summary opens `Monitor`/`scheduled`/`cron`
OR a `Background command "…"` whose quoted command NAME carries a monitor-cadence token
(`monitor`/`re-arm`/`relaunch monitor`/`liveness`) — so a monitor loop implemented as `&`-detached
background commands is attributed to `monitor`, not disguised as generic `background-command`), NOT a hardcoded
`workflow` — instead of the raw `<task-id>`/`<output-file>`/`<status>` XML blob. For a **monitor**
pulse the real outcome lives in `<event>` and there is frequently NO `<status>`, so the label surfaces
the event (`[monitor b718g3gqq STAGE2_OUTPUT_READY]`, or a timeout notice) rather than fabricating
`completed`. **Limitation:** the `monitor` class covers only `<task-notification>` completion pulses;
the **`ScheduleWakeup` wakeup-tick prompts** that drive a monitor/cron *cadence* arrive as
`isMeta:true` user records (NOT `<task-notification>`s), so they are **not yet segmented or attributed**
— the assistant run a tick triggers currently groups under the preceding genuine-user turn. The
`turns` per-session header reports the human/automation split WITH a per-class
breakdown (e.g. `selected 20 user (3 automation triggers: 2 background-command, 1 agent) + 52
assistant units`). The trigger still opens a turn, but it is EXCLUDED from the
`--round-trip-fraction` HARD FLOOR (that lane is reserved for human exchanges) — it can still be picked
as Phase-2 fill. So a consumer sees at a glance which "user turns" were machine pulses, and the human
round-trip floor is never silently spent on a pulse→ack pair. In `--format json` the automation
attribution is STRUCTURAL on the user-segment object (`is_automation` + `trigger_kind` + `task_id` +
`status` + `event`), not only a text prefix; the stream opens with a `{kind:"session_header",…}` object
carrying the human/automation split (lumped `automation_triggers` + per-class `automation_by_kind` = the
SELECTED triggers, PLUS `automation_in_scope_by_kind` = the SAME breakdown over EVERY in-scope pulse
regardless of budget — so a monitor-heavy session is never read as `monitor:0` just because the recency
window selected none of its deep pulses; the text header prints an `in scope (not all selected): …` line
when more automation exists than was rendered) +
budget fan-out (`sessions_in_scope` = true scope, `sessions_rendered` = how many fit the budget).

**Budget model.** `--budget` (default 40000, chars or `--budget-unit tokens` ≈4 chars/token) is applied
**PER session in scope**. UNLIKE `files`/`search`, `turns` defaults to the **top-level thread only** —
it is a single-thread recovery tool whose per-session budget MULTIPLIES, so spanning hundreds of
unrelated fan-out subagents by default would bury the thread you asked to restore. A bare `turns <uuid>`
therefore reconstructs just that conversation at `budget` chars; add `--include-subagents` for the rare
cross-fan-out reconstruction, where the realized total is `budget × (sessions in scope)` and a
top-of-output `SCOPE` banner names the TRUE scope (all top-level + subagent sessions discovered), how
many rendered within budget, and the realized multiplier. A targeted top-level session that does not fit
the budget is reported with an explicit `SESSION <uuid>  skipped — its first round-trip needs ≥ N chars`
note, never silently dropped. Selection is
recency-first (most-recent turns win the budget); the emitted document is sorted ascending so it reads
forward. `--round-trip-fraction` (default 0.5) is a HARD FLOOR:
that fraction of the budget can only be spent on COMPLETE round-trips (user → `[N tool calls]` →
assistant), never on user-only / assistant-only fragments. Over-cap units are middle-truncated
(head+tail kept) with an explicit `… [+K chars, L lines elided] …` marker (the assistant head is larger
than the user head). The backward walk is transparent to compaction boundaries — a summary is a turn
member, never a delimiter — so a 40K budget reaches across multiple boundaries by default;
`--max-compactions N` caps the reach. A turn the NEWEST summary already quotes verbatim is flagged
`(also in summary)` and DEMOTED (selected after non-dup turns), never silently dropped.

**Richness model (`--agent-msgs`).** A single user turn can own a LONG run of agent messages — a
debugging/build chain the model narrates step by step — that the summary clips to one quote.

**Why the default keeps the LONGEST, not the LAST.** The last agent message of a turn is frequently a
~50-char throwaway wrap-up ("Done.", "Let me know if you want anything else.") while the SUBSTANTIVE
Rich Response — the actual finding, the committed answer, the design write-up — sits in a MIDDLE
message. The pre-feature default kept `agents.last()`, so it silently DROPPED the substance of exactly
those turns. The default now keeps the LONGEST agent message (by char count), the single best
one-message proxy for "where the substance is". Because more than one message often matters, the
default ALSO keeps a substantive first and the rich middles (below).

| Mode | Behavior |
| --- | --- |
| `longest` | **DEFAULT.** Keep the LONGEST agent message (the substantive Rich Response, often a middle) + the FIRST when substantive (`≥ --agent-rich-min-chars`) + each RICH middle; collapse everything else (including a short, non-rich throwaway last) into a placeholder. Applies to every multi-message turn. |
| `eot-only` | **Force last-only.** Keep ONLY each turn's last agent message — byte-identical to the pre-feature single-EOT output. Use when you specifically want the old behavior. |
| `rich` | Keep the last always + the first by position privilege + each non-droppable middle, collapsing pure declarations into a placeholder. Only fires on a run longer than `--agent-run-threshold` (default 6). |
| `all` | Keep every agent message — maximal fidelity, no collapse. |

**The keep-heuristics (`longest` mode).** Per turn the survivor set is:

- **the LONGEST agent message** — ALWAYS (the substantive Rich Response). On a tie the LAST maximum
  wins, so an all-equal run coincides with the old `agents.last()` pick.
- **the FIRST** — kept when SUBSTANTIVE (`≥ --agent-rich-min-chars`, default 280); the opening message
  often states the plan or an early finding. A short "let me look into this" opener falls below the
  gate and collapses.
- **each MIDDLE that is RICH** — a major finding can live mid-run.
- **the LAST** — kept only when it is itself rich/substantive; a short throwaway wrap-up collapses.
- **everything else** collapses into a placeholder.

A message is "rich" by a cheap single-pass test: a number-of-substance (`12 passed 3 failed`, `12/40`),
a commit-hash-like hex, a `file.rs:NNN` ref, a backtick `code` path, a finding/decision lexeme (`found`
/ `confirmed` / `root cause` / `DEFER` / …), or simply a body ≥
`--agent-rich-min-chars` (default 280). **`--agent-rich-min-chars` is the tuning knob for both the
default and `rich`:** in `longest` it gates the "keep the first if substantive" decision AND the rich
length arm; raise it to keep fewer first/middle messages, lower it to keep more.

In `rich` mode the spine is KEEP-ON-DOUBT instead: only a short (`< --agent-declaration-max-chars`,
default 200) signal-less intent-verb opener (`let me …` / `now I …`) is COLLAPSED; anything
uncertain is kept, and `--keep-first` (default) keeps the first by position privilege regardless of
richness (`--no-keep-first` decides it as a middle — `--keep-first` has no effect in `longest` mode,
where the first is gated on length). A contiguous collapsed run renders as one placeholder line
carrying the fetchable jsonl range and the per-message attribution:

```
△ L412–L437  [4 agent messages, 9 tool calls, 1 failed]
```

(`X agent messages` collapsed, `Y tool calls` owned by the span, `Z failed` erroring tool results — Z
omitted when 0). `--profile heavy` (threshold 4, rich-min 200, declaration-max 140) and `--profile
light` (threshold 8, rich-min 360, declaration-max 240) bundle the thresholds — applied before the
individual flags, so an explicit flag overrides the profile. The profile keeps the master `--agent-msgs`
mode as-is (so `--profile heavy` alone runs the default `longest` mode with heavier thresholds; add
`--agent-msgs rich` to also switch the keep-set). **Picking heavy vs light:** read the compaction summary
you are supplementing — pick `heavy` when its errors/decisions narrative is THIN (you need the debugging
back-and-forth restored), `light` when it is already rich (restore user phrasings + the substance, skip
the intermediate chatter).

Subagent transcripts (`--include-subagents`, **default OFF for `turns`** — opt in) get the SAME richness
treatment via the shared code path when spanned.

Examples:

```bash
csift turns .                                     # default 40K-char recon: longest agent msg + rich members per turn
csift turns <uuid> --budget 12000                 # a 200K-context-sized recovery
csift turns <uuid> --agent-msgs eot-only          # force the old single-EOT (last-message-only) output
csift turns <uuid> --agent-rich-min-chars 200     # default mode, lower bar → keep more first/middle messages
csift turns <uuid> --agent-msgs rich              # the keep-on-doubt keep-set (last always + non-droppable middles)
csift turns <uuid> --profile heavy                # lower thresholds (max fidelity)
csift turns <uuid> --agent-msgs all --budget 60000  # every agent message, no filtering
csift turns . --budget 40000 --out /tmp/turns.md  # full reconstruction to a file
csift turns . --slices 4 --window 9000 --slice 1   # FIXED-FLEET: the 1st of AT MOST 4 newest-first chunks for a SessionStart hook (count never drifts)
csift turns <uuid> --include-subagents            # ALSO span subagents (budget × N; rare cross-fan-out recon)
```

JSON (`--format json`) opens with a `{kind:"session_header",…}` object (`sessions_in_scope` vs
`sessions_rendered`, top-level/subagent split, lumped `automation_triggers` + per-class
`automation_by_kind` = SELECTED + `automation_in_scope_by_kind` = the SAME breakdown over EVERY in-scope
pulse regardless of budget, so a monitor-heavy session never reads `monitor:0`), then one VERBATIM
(un-truncated) object per emitted unit, interleaved
compaction-boundary records, and a `collapsed_agents` placeholder record (with `agent_messages` /
`tool_calls` / `failed` / `first_line` / `last_line`) per collapsed span. Each per-unit + collapsed
record carries `session_id` + the id-domain discriminators `is_subagent` + `parent_session_id`
(re-feed the parent uuid, never a subagent `session_id`). An automation user unit additionally
carries `trigger_kind` / `task_id` / `status` / `event`. `budget_chars`/`max_total_chars` are always
in CHARS — under `--budget-unit tokens` they read 4× `--budget` (a token budget is pre-multiplied
×4), so pass an explicit `--budget` when flipping to tokens or the default `40000` becomes 160000 chars.

**Chunked output for hook injection (`--slice` / `--window`).** A `SessionStart` hook can inject only
≤10,000 CHARACTERS per call: Claude Code caps each hook's `additionalContext` (and plain stdout) at that,
replacing any over-cap output with a file-path + short preview — so the body is effectively LOST to the
model. To fan a larger reconstruction across several hooks, `--slice N` paginates the verbatim DOCUMENT
(the SAME body `--out` writes — turn units + boundary banners, with NO scope/header/footer chrome) into
≤`--window`-character chunks and prints ONLY the Nth (1-based) chunk to stdout. `--window` defaults to
10000 and counts CHARACTERS (Unicode scalars — the unit the cap itself counts, so CJK-heavy prose is not
3× over-counted the way a byte budget would be); pass a little under (e.g. `--window 9000`) to leave
headroom for any wrapper text the hook adds. Two ways to size the fan-out:

- **`--slices N` (FIXED-FLEET — use this for a hook).** The slice COUNT is the hard constraint: csift
  fills the N newest-first chunks with WHOLE turns (a turn is ellipsized ONLY if it alone exceeds one
  window) and DISCARDS the oldest turns that don't fit, so it emits AT MOST N chunks NO MATTER how big the
  turns are. A registered hook fleet is a fixed set of N `SessionStart` hooks, so `--slices N` is what
  keeps the count from drifting to 5/6/7 (and silently dropping a slice) as a session's turns grow. The
  budget becomes N×`--window`; the per-role 600/900 body caps are dropped, so user directives — the
  recovery target — survive verbatim. `--slices N` requires `--slice i` to pick which chunk.
- **`--slice i` alone (LEGACY, budget-driven).** `--budget` decides the document; it paginates into a
  VARIABLE number of ≤`--window` chunks and `--slice i` prints the i-th. DETERMINISTIC (same session +
  budget ⇒ identical boundaries) and concatenating `1..K` reproduces the whole document byte-for-byte —
  but K is not knowable in advance, so this fits ad-hoc reads, NOT a fixed hook fleet.

Either way the HOOKS are not order-free: Claude Code runs same-event hooks concurrently and collects their
`additionalContext` in COMPLETION order, never settings-declaration order, so slices declared 1-2-3-4 can
land as 2-4-3-1 and scramble the reconstruction. Order MUST be enforced in the hook shell with a done-flag
barrier — slice N blocks until slice N-1 has emitted+exited, forcing process-exit order (= the harness's
collection order) into slice order. This is the exact mechanism a chunked-USER.md `SessionStart` loader
uses; recipe (A) below ports it in full. An out-of-range `N` prints nothing (exit 0), so a surplus hook in
a fixed fleet simply injects nothing — but it STILL must release the barrier so the chain doesn't stall.
`--slice` is text-only and is NOT combinable with `--out` (which writes the WHOLE document to a file).
Example: a fixed 4-hook fleet — hook `i` runs `csift turns . --slices 4 --window 9000 --slice i`. See
"Integration recipes → (A)" for the wiring and how this composes with compaction re-injection.

---

### `search` as a message-fetcher — `--line` / `--uuid` addressing

`search` doubles as the message-getter: instead of (or as well as) a pattern, address specific records
and they render FULL. This is the **in-permission alternative to `Read`-ing the raw jsonl** — an agent
without broad filesystem access can't `Read` a transcript outside its workspace without the user
approving each read, but it can `csift search`. Built for batch.

- **`--line SPEC`** — 1-based PHYSICAL line(s) in ONE resolved transcript. Repeatable AND comma-delimited,
  each token `N` or `A-B` (inclusive range): `--line 87,495-500,992`. Lines are per-file, so the scope
  must pin a single transcript: `--session <uuid>` [`--no-subagents`], `--session <uuid> --subagent <hex>`,
  or a single-session PATH. A range CLAMPS to the file; an EXPLICIT line that resolves to nothing is
  reported as `unresolved: L<line>` (a `--max-count`-style no-silent-truncation guarantee for addresses).
- **`--uuid U`** — record uuid(s) (globally unique). Repeatable + comma-delimited; scope optional (a
  `--session`/PATH scope just makes the scan fast).

Addressing composes with the normal filters (`-t`, `--since/--until`, `--turn-range`) and emits the same
exchange shape as any search (text label table + `s·t` rows; JSON exchange objects with full `hits[]`).

```bash
# skim, then fetch the exact message(s) — full, no drop to raw jsonl
csift search "deploy" -t user                        # → a hit row shows `◂ user  L46550 …`
csift search "" <id> --no-subagents --line 46550     # → that message, in full
csift search "" <id> --no-subagents --line 46540-46560  # → it plus its surrounding span
csift search "" --uuid 1f70fc7d-c4b3-4d0e-915c-edf09b32a7c0  # by record uuid (scope optional)
csift search "" <id> --subagent aaa111 --line 12     # a record inside a subagent transcript
```

### `image` — get a sent image back out of a transcript

A pasted/attached image (or a tool screenshot) lives INLINE on a record as a base64 image block, so
it is recoverable straight from the jsonl. Address an image two ways:

- **`#N`** — the SAME `[Image #N]` number the session uses ("re-share #32"). `turns` and `search` print
  it inline (`[N image(s): #32, #33, …]`), so you feed it straight back: `--id '#32,#33'`. **Not unique
  across the session** — CC reuses low numbers per prompt. If a `#N` names >1 DISTINCT image, `--id #N`
  **ERRORS** with the occurrence list (each one's `t<turn>` / `L<line>i<n>` / uuid / time / excerpt) instead
  of guessing — disambiguate with the locator, or narrow scope with `--since`/`--until` / `--turn-range` /
  `--uuid` (pre-applyable, e.g. `--since 1h`). Markers that can't be matched 1:1 leave `#N` unset (locator only).
- **`L<line>i<n>`** — the exact locator (carrying record's JSONL line + ordinal of the image within it),
  always unambiguous; use it to pin one specific occurrence.

`--out <dir>` decodes the (selected) images to real files (`<session-short>[-img<N>]-L<line>i<n>.<ext>`,
extension read per-image from its media type — **never assumed PNG**). `--as png|jpeg|gif|webp` forces the
output format: a different source is **converted, not rejected** (→jpeg lossy q90, →gif palette, →webp
lossless; an animated GIF → still keeps its first frame + warns). The bare LISTING is content-deduped (a
re-injected image shows once). Default is to LIST. Spans subagents by default; `--no-subagents` to restrict.

```bash
csift image <id>                                  # list (deduped): id · media-type · ~size · time
csift image <id> --out /tmp/imgs                  # extract ALL images to a dir (source formats)
csift image <id> --no-subagents --id '#32,#33,#34,#36' --out /tmp/imgs   # re-share by handle
csift image <id> --no-subagents --id '#1' --since 1h --out /tmp/imgs     # disambiguate a reused #1 by time
csift image <id> --no-subagents --id L6812i2 --as jpeg --out /tmp/imgs   # pin one occurrence + convert
csift image . --format json                       # one object per image + a trailing summary
```

A `--line`-style rule applies to `--id` / `--turn-range`: `#N`, the locator, and turn indices are all
per-transcript, so addressing needs a single transcript in scope — pin it with `--session <uuid> --no-subagents`.

---

## The session + subagent model

### Data location

```
~/.claude/projects/<ENCODED_CWD>/<session-uuid>.jsonl                                   # a session transcript
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/agent-<hex>.jsonl                 # (A) built-in Task/Agent subagent
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/workflows/wf_*/agent-<hex>.jsonl  # (B) workflow / OMC subagent
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/workflows/wf_*/journal.jsonl      # (C) workflow EVENT log (NOT a transcript)
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/**/*.meta.json                    # {agentType, description?, …} companion
~/.claude/projects/<ENCODED>/<session-uuid>/tool-results/<id>.txt                       # externalised tool output
```

**Path encoding** is deterministic forward, lossy reverse: Claude Code replaces **every** non-`[A-Za-z0-9]`
byte of the absolute cwd with a single `-` (no consecutive-dash collapse — `/`, `_`, `.`, space all
map to `-`; a `/.claude/` segment becomes a literal `--claude-`). So
`/Users/testuser/Projects/widget_app_prototype` →
`-Users-testuser-Projects-widget-app-prototype`. csift never reverses; it accepts either a real
path (encodes it) or an already-encoded token (uses as-is).

### Three kinds of transcript

- **omc / workflow** subagents — `subagents/workflows/wf_<id>/agent-<hex>.jsonl` (the dominant kind;
  OMC fan-out modes spawn these). Kind label: `workflow`.
- **builtin** subagents — `subagents/agent-<hex>.jsonl`, spawned by the built-in Task/Agent tool.
  Kind label: `builtin-task`.
- The top-level **session** itself — `<session-uuid>.jsonl`.

**Kind = on-disk PATH LOCATION, not `agentType`.** `agentType` (e.g. `Explore`, `general-purpose`,
`oh-my-claudecode:executor`, `workflow-subagent`) is the same spread across both kinds, so it is a
descriptive sub-label only. The canonical agent id is the bare `<hex>` (the record/journal
`agentId`), not the `agent-<hex>` filename stem. The workflow `journal.jsonl` is an event log
(`{agentId, type:"started"|"result"}`), never listed/searched — it is read **only** to corroborate
completion status.

### Record categories (`search -t/--category`)

A single `type:"user"` record is **NOT always a human turn** — `tool_result` blocks ride on
`role:"user"` records too (in one real session: ~393 genuine users vs ~1619 tool_result-carriers).
The genuine-user classification is load-bearing and excludes plain tool_result-carriers, `isMeta`
pseudo-turns, compaction summaries, interrupt markers (`[Request interrupted by user]`),
`<local-command-stdout>` output, and `<command-name>` slash-command wrappers. A turn boundary
ALSO opens on an answered AskUserQuestion and a tool-use rejection-with-message (both are genuine
user messages).

- **`thinking`** — assistant thinking blocks.
- **`user`** — genuine human input + the full AskUserQuestion Q+options+answer unit + a
  plan-rejection-with-message (with a `[plan: …]` pointer) + machine **automation triggers**
  (`<task-notification>` openers, rendered as the parsed `[<kind> <id> <status>] <summary>` label —
  recognize a machine opener by the `[<kind> …]` prefix). NOT plain tool_result-carriers, interrupts,
  or slash-command wrappers.
- **`tool`** — `tool_use` blocks (AskUserQuestion is a tool_use).
- **`tool-response`** — `tool_result` blocks.
- **`agent`** — assistant visible end-of-turn text (the agent message).

**Compaction shape** (load-bearing for recipe A): the summary is a `type:"user"` record with
`isCompactSummary:true` + `isVisibleInTranscriptOnly:true` carrying **string** content (NOT a
`type:"summary"` record); a separate `type:"system"` `subtype:"compact_boundary"` record carries
`compactMetadata:{trigger,preTokens,postTokens,durationMs}`. The compaction summary is excluded
from "genuine user".

---

## Output formats — `--format text` vs `--format json`

Every subcommand defaults to **text** (the headered, skimmable "Text output shape" blocks above) and
takes **`--format json`** for machine use. The JSON is **JSONL / NDJSON**, never one document — parse it
**line by line**, NOT with a whole-file `json.load` (that fails). A stream is:

```
{kind:"session_header", …}    # OPTIONAL leading scope record — only when the scope spans ≥1 subagent
{ …per-unit record… }         # one object per exchange / turn-unit / mutation / agent node / segment
{ …per-unit record… }
{ …trailing summary… }        # final accounting object (search/files/recover; turns folds it into the header)
```

Drop the optional header + trailing summary by **key presence** (a unit always carries its payload key,
e.g. `turn_index`/`role`/`op`). Concrete records with the REAL field names:

**`search`** — one envelope per matched exchange, in combined chronological order (subagents interleaved),
then a summary:

```json
{"session_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","is_subagent":false,"parent_session_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","turn_index":4,"ts_utc":"2026-06-03T12:51:53.206Z","ts_local":"2026-06-03T22:51:53+10:00","hits":[{"category":"user","excerpt":"Audit harness-correctness …","ts_utc":"2026-06-03T12:51:53.206Z","ts_local":"2026-06-03T22:51:53+10:00","tool_name":null}],"record_uuids":["u4","a4t","a4f"]}
{"matched":2,"dropped_by_cap":18,"skipped_lines":0}
```

**`turns`** — a `{kind:"session_header",…}` (carrying the budget/automation accounting — there is no
separate footer), then one verbatim `role`-bearing unit / `compaction_boundary` / `collapsed_agents`
record per line. A unit (full field set: `full_chars`, `rendered_chars`, `truncated`, `elided_chars`,
`elided_lines`, `tool_calls`, `compactions_before`, `also_in_summary` ride along too):

```json
{"session_id":"…","is_subagent":false,"parent_session_id":"…","turn_index":4,"role":"user","line_no":812,"ts_utc":"2026-06-03T12:51:53.206Z","ts_local":"2026-06-03T22:51:53+10:00","text":"the full verbatim message …","truncated":false}
```

**`files --timeline`** — one mutation per line, then a summary:

```json
{"session_id":"…","is_subagent":false,"parent_session_id":"…","path":"/p/spec/gaps.md","op":"edit","ts_utc":"…","ts_local":"…","turn_index":7,"is_create":false,"heuristic":false}
{"distinct_files":5,"total_mutations":6,"skipped_lines":0,"detail_level":"timeline"}
```

**jq** — JSONL feeds `jq` directly (no `-s`); filter the header/footer by the unit's payload key:

```bash
# search: each exchange's time + turn + first-hit excerpt, already chronological
csift search "panic" --session <uuid> --format json \
  | jq -r 'select(.turn_index!=null) | "\(.ts_local)  turn \(.turn_index)  \(.hits[0].excerpt)"'

# files: distinct /tmp files this session CREATED (authoritative only)
csift files <uuid> --timeline --format json \
  | jq -r 'select(.path? and (.path|startswith("/tmp")) and .is_create==true and .heuristic==false) | .path' | sort -u

# turns: the verbatim user-side text, in order
csift turns <uuid> --format json | jq -r 'select(.role=="user") | .text'
```

**python** — read line-by-line; skip blanks and tolerate the header/footer by key presence:

```python
import json, subprocess
out = subprocess.run(
    ["csift", "search", "panic", "--session", SID, "--format", "json"],
    capture_output=True, text=True, check=True,
).stdout
rows = [json.loads(l) for l in out.splitlines() if l.strip()]
exchanges = [r for r in rows if "turn_index" in r]   # drop scope header + summary footer
for ex in exchanges:                                 # already in chronological order
    print(ex["ts_utc"], ex["turn_index"], ex["hits"][0]["excerpt"])
```

Across subcommands the id-domain discriminators are uniform: a record's `session_id` is the transcript's
own id (a top-level uuid, or a bare subagent hex when `is_subagent` is true), and `parent_session_id` is
the always-re-feedable owning uuid — feed THAT to `csift turns/files/recover --session`, never a subagent
`session_id`.

---

## Integration recipes

### (A) Post-compaction standing-directive recovery — the motivating use-case

**Problem.** When Claude Code compacts context, the whole pre-boundary conversation is replaced by a
single summary written by the same main-loop model in one pass. That pass is lossy and tends to be
**recency-biased**: durable early instructions — persistent "keep going / don't stop until done" style
STANDING DIRECTIVES the user set up front — can thin out, or have their phrasing shift, by the time the
summary is written. The lossless signal still exists on disk as the full pre-compaction session jsonl, so
csift can re-surface that material verbatim as a SUPPLEMENT to the summary (it does not replace the
summary, which still owns task state).

**Why it's safe to re-run on EVERY compaction (no pile-up).** On compaction the post-boundary context is
rebuilt as `[boundary marker, summary, restored attachments, SessionStart('compact') hook results]`. The
previous injection lived in the pre-boundary messages, so it is fed to the summarizer and dropped; then
`SessionStart` re-fires with `source:"compact"` and re-injects a fresh copy. There is therefore always
exactly ONE current copy — the drop-and-re-inject cycle is self-balancing. Resume doesn't duplicate it
either: a `compact`-matched hook does NOT re-fire on resume (`source:"resume"` ≠ `"compact"`), so the
transcript-replayed copy is the only one (broaden the matcher to include `resume` and it would replay AND
re-fire — keep it `compact`-only; identical re-injections are de-duplicated by content regardless). The
injection is a SUPPLEMENT added alongside the summary, never folded in, so it must stand on its own and
stay within the 10K-char cap — which is what `--slice` (below) is for.

**The recipe — verbatim-turn supplement via `turns --slice`.** A `SessionStart` hook with
`matcher: "compact"` (so it fires after auto/manual compaction) re-injects the verbatim recent turns the
summary clipped: `csift turns` reconstructs them (the session id + `transcript_path` arrive on the hook's
stdin), and `--slice` fans a >10K reconstruction across a fixed fleet of hooks, one ≤10K chunk each. This
re-surfaces the back-and-forth verbatim as a SUPPLEMENT; it does not try to detect which directives the
summary dropped, and it never replaces the summary (which still owns task state). Hook field shapes per the
official docs: https://code.claude.com/docs/en/hooks.md

Slicing is deterministic, so each hook asks for its own slice with no coordination inside csift — but the
harness runs same-event hooks concurrently and collects their `additionalContext` in COMPLETION order, not
registration order, so slices declared 1-4 can land 2-4-3-1 and scramble (a turn split across a boundary
then glues back wrong). The script MUST carry a done-flag barrier: slice N waits for slice N-1's `.done`
before it emits + exits, forcing process-exit order into slice order. Same mechanism the chunked-USER.md
`SessionStart` loader uses.

`.claude/hooks/csift-turns-slice.sh` (one script, registered N times with a different slice arg):

```bash
#!/usr/bin/env bash
# SessionStart(source=compact) hook #i of N — inject the i-th ≤9000-char chunk of the verbatim turn
# reconstruction. Claude Code runs same-event hooks concurrently and collects their additionalContext in
# COMPLETION order (not registration order), so without sequencing the chunks arrive out of order.
# A filesystem done-flag barrier forces injection order = slice order: slice N waits for slice N-1's
# .done flag, then emits + exits + releases its own. Namespaced by the Claude Code PID (= this script's
# PPID) so concurrent sessions don't collide. Faithful port of the chunked-USER.md SessionStart loader.
set -euo pipefail
slice="${1:?pass the 1-based slice index as argv1}"
MAX_SLICES=4                       # the FIXED fleet size — MUST equal the number of registered hooks below
WAIT_TIMEOUT_SECS=5                # insurance: proceed anyway if a predecessor never fires

SEQ_DIR="/tmp/csift-turns-slice-seq-${PPID}"
DONE_FLAG="${SEQ_DIR}/slice-${slice}.done"

# Release the barrier for slice N+1 on EVERY exit path (success, empty slice, or error under set -e),
# and let the last-registered hook tear the namespace down. Installed before any fallible work so the
# chain can never stall on an early failure.
release() {
  mkdir -p "$SEQ_DIR" 2>/dev/null || true
  touch "$DONE_FLAG" 2>/dev/null || true
  [ "$slice" = "$MAX_SLICES" ] && rm -rf "$SEQ_DIR" 2>/dev/null || true
  return 0
}
trap release EXIT

# Slice 1 resets stale flags from a prior PID-collision run; later slices block on their predecessor.
if [ "$slice" -le 1 ]; then
  rm -rf "$SEQ_DIR" 2>/dev/null || true
  mkdir -p "$SEQ_DIR" 2>/dev/null || true
else
  prev="${SEQ_DIR}/slice-$((slice - 1)).done"
  deadline=$(( $(date +%s) + WAIT_TIMEOUT_SECS ))
  while [ ! -f "$prev" ] && [ "$(date +%s)" -lt "$deadline" ]; do sleep 0.05; done
fi

input="$(cat)"
session_id="$(printf '%s' "$input" | jq -r '.session_id')"
# `--slices N` pins the FLEET size: csift fills N newest-first slices with WHOLE turns (ellipsizing a turn
# only if it ALONE exceeds one window) and drops the oldest overflow, so the chunk count is ALWAYS ≤ N —
# it never drifts to 5/6/7 as turns grow, so the registered hook count never needs re-tuning per session.
chunk="$(csift turns --session "$session_id" --slices "$MAX_SLICES" --window 9000 --slice "$slice" 2>/dev/null || true)"
# An out-of-range slice prints nothing → inject nothing; the trap still releases the barrier.
[ -n "$chunk" ] || exit 0

jq -n --arg ctx "Verbatim turns the compaction summary clipped (part $slice — a supplement; the summary still owns task state):
$chunk" \
  '{hookSpecificOutput:{hookEventName:"SessionStart", additionalContext:$ctx}}'
```

Register the same script N times (here 4 = `MAX_SLICES`). With `--slices N` the slice COUNT is the hard
constraint, so N is simply how much recent history to recover (≈N×`--window` chars) — NOT a number to
re-tune per session. csift always emits ≤N chunks, dropping the oldest turns that don't fit, so the fleet
never has to grow as turns get bigger. A surplus slice (a short session) prints nothing and self-trims —
but still releases the barrier so the chain never stalls.

```json
{
  "hooks": {
    "SessionStart": [
      { "matcher": "compact", "hooks": [{ "type": "command",
        "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/csift-turns-slice.sh 1" }] },
      { "matcher": "compact", "hooks": [{ "type": "command",
        "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/csift-turns-slice.sh 2" }] },
      { "matcher": "compact", "hooks": [{ "type": "command",
        "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/csift-turns-slice.sh 3" }] },
      { "matcher": "compact", "hooks": [{ "type": "command",
        "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/csift-turns-slice.sh 4" }] }
    ]
  }
}
```

For a GLOBAL install (every project), drop the same script in `~/.claude/hooks/` and register the four
entries in `~/.claude/settings.json` with an ABSOLUTE `command` path — `${CLAUDE_PROJECT_DIR}` is unset
outside a project — and resolve `csift` from `~/.cargo/bin` / PATH rather than a repo-relative binary.

Safe to fire on every compaction (no pile-up, per above). `--window 9000` (under the 10K cap) leaves
headroom for the wrapper line each hook adds; the four slices carry the NEWEST ≈4×9000 chars of verbatim
turns (oldest overflow discarded — `--slices` keeps recency, not the whole document), and the barrier
guarantees they arrive oldest-kept → newest.

### (B) `whoami` remediation — export the session id when CC doesn't

Some Claude Code versions do **not** export `CLAUDE_CODE_SESSION_ID` into the Bash-tool env
(upstream issue: https://github.com/anthropics/claude-code/issues/24371). When that happens,
`csift whoami` cannot resolve the current session and (correctly) refuses to guess. Fix it with a
`SessionStart` hook that captures the session id from the hook's own stdin JSON and persists +
exports it, so csift can read it. Hook field shapes per https://code.claude.com/docs/en/hooks.md

`.claude/hooks/csift-export-session-id.sh`:

```bash
#!/usr/bin/env bash
# SessionStart — persist the session id so `csift whoami` works even on CC builds
# that don't export CLAUDE_CODE_SESSION_ID (anthropics/claude-code#24371).
set -euo pipefail
input="$(cat)"
session_id="$(printf '%s' "$input" | jq -r '.session_id')"
[ -n "$session_id" ] && [ "$session_id" != "null" ] || exit 0

# 1) Persist to a per-cwd file csift / your tooling can read deterministically.
state_dir="${XDG_STATE_HOME:-$HOME/.local/state}/csift"
mkdir -p "$state_dir"
printf '%s\n' "$session_id" > "$state_dir/current-session-id"

# 2) Surface it as context so the agent can pass --session explicitly if needed.
jq -n --arg sid "$session_id" \
  '{hookSpecificOutput:{hookEventName:"SessionStart",
    additionalContext:("csift: current session id is " + $sid +
      " (export unavailable on this CC build); pass `--session " + $sid + "` to csift.")}}'
```

Register (fire on every entry so it always refreshes):

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume|clear|compact",
        "hooks": [
          { "type": "command",
            "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/csift-export-session-id.sh" }
        ]
      }
    ]
  }
}
```

The hook cannot mutate the *parent* shell env directly (a hook runs in a subprocess), so the
durable file at `$XDG_STATE_HOME/csift/current-session-id` is the reliable handoff. When you need to
search the current session and `csift whoami` is unavailable, read that file and pass
`--session "$(cat "${XDG_STATE_HOME:-$HOME/.local/state}/csift/current-session-id")"`.

### (C) Quick agent-side patterns

```bash
# Find every time a directive was given in this session (current session id from whoami).
# The pattern mixes English directives with a multi-byte emoji token to show that regex
# search handles arbitrary UTF-8 literals (serde_json emits non-ASCII verbatim):
csift search "don't stop|keep going|🤖" -i --session "$(csift whoami --format json | jq -r .session_id)"

# ⚠ SUBAGENT CAVEAT for the recipe above: this whoami→--session chain only works from a
# TOP-LEVEL session. Inside a subagent, $CLAUDE_CODE_SESSION_ID (and thus whoami) yields the
# subagent's OWN bare hex, which --session REJECTS as a hard error on every corpus subcommand
# ("a bare SUBAGENT id never names a top-level session"). Map the hex to its PARENT first and
# scope to THAT (the parent uuid covers the whole conversation, the subagent transcript
# included):
PARENT="$(csift agents --agent "$(csift whoami --format json | jq -r .session_id)" --format json | jq -r .parent_session_id)"
csift search "don't stop|keep going" -i --session "$PARENT"

# Recover what a specific subagent did, then read its full exchange:
csift agents --session <uuid> --kind workflow            # find the agent + its time window
csift search "<pattern>" --session <uuid> --since 2h     # search that window (spans subagents)

# Identify an unknown transcript file's project/branch/last-activity:
csift list ~/.claude/projects/<encoded>
```

---

## Notes & guarantees

- **No silent truncation.** Capped result sets (`--max-count`) report the dropped count; skipped
  malformed lines are counted, never hidden; truncated excerpts carry an explicit `… (+N chars)`.
- **Tolerant parsing.** Real jsonl carries far more fields than documented and some records have no
  timestamp; csift deserializes only what it uses and never crashes on an unknown field/block type.
- **Performance is a contract.** `list`/`search` stay fast on 200 MB+ files via mmap + SIMD newline
  scan + a cheap byte/regex prefilter (full JSON parse only on candidate lines); tail reads seek
  backward from EOF; scanning fans out across files in parallel.
- **Timestamps** render as system-local (auto-detected timezone) alongside raw UTC.
- **Exit codes:** `0` success; `1` on error (e.g. no matching project dir, or `whoami` with no
  session signal), with the full error chain on stderr.
