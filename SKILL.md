---
name: csift
description: Search and audit Claude Code session + subagent jsonl transcripts. Use when you need to find what was said/done in a past (or the current) Claude Code session — regex-search the transcript corpus under ~/.claude/projects, list sessions to identify "which session is this", inspect a session's subagent lifecycle (built-in Task agents + OMC/workflow agents), see which files/dirs a session modified and when, reconstruct a file's content (or restore a deleted plan) from the transcript's Read/Write/Edit stream, identify the calling session, or recover standing directives a context-compaction dropped. ripgrep-for-transcripts: pure regex, no embeddings/semantic search.
user-invocable: true
---

# csift — ripgrep for Claude Code session transcripts

`csift` is a fast Rust CLI that **lists** and **regex-searches** Claude Code session `.jsonl`
transcripts under `~/.claude/projects/<encoded-cwd>/`. Its primary consumer is an LLM (a Claude
Code agent searching or recovering its own or a peer session), so output is clean,
token-efficient, regex-driven text by default, with `--format json` for machine use.

It is **pure regex / ripgrep** — explicitly **no BM25, no embeddings, no semantic search**.
Lexical scoring across scripts (CJK / multi-byte) is intractable; regex is the whole point.

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
  diff-patches (`--patches`), a point-in-time partial snapshot (`--at`), a coverage/scoping summary
  (`--coverage`), or a restored plan (`--plan`). It SEGMENTS at integrity boundaries (a `modified
  since read` error, an `originalFile` disagreement, an external edit, a heuristic Bash mutation), is
  necessarily PARTIAL (unknown lines are explicit gaps, never fabricated), and every output line
  carries the JSONL LINE NUMBER. The motivating use: restore a deleted plan / a file lost in a
  bad-recovery.
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
- **Post-compaction recovery** → after a context compaction, diff the compaction summary against the
  lossless jsonl to surface STANDING DIRECTIVES the summary dropped or inverted (the motivating
  use-case — see "Integration recipes → (A)"). For the verbatim TURN back-and-forth (vs standing
  directives), `csift turns` automates the summary's own "read the full transcript at `<path>`" pointer.

Reach for csift instead of hand-grepping `~/.claude/projects/**/*.jsonl`: it understands the
record model (genuine-user vs tool_result-carrier, thinking/tool/agent categories), spans subagent
transcripts, and reconstructs whole turns rather than emitting line fragments.

---

## Command surface

Seven subcommands: `list`, `search`, `agents`, `whoami`, `files`, `recover`, `turns`.
`list`/`search`/`files`/`recover`/`turns` span each session's subagent transcripts **by default**
(`--no-subagents` opts out; `agents` LISTS subagents as its targets, so it has no span flag).
Every subcommand takes `--format text|json` (default `text`).

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

JSON: one object per session (`session_id`, `path`, `cwd`, `version`, `git_branch`,
`first_user`, `last_user`, `last_agent`, `skipped_lines`).

### `search` — regex over transcripts, complete round-trip per hit

```
csift search [PATTERN] [PATH...] [--session ID] [--no-subagents]
             [-t|--category thinking|user|tool|tool-response|agent]...
             [-i|--ignore-case] [--multiline]
             [--turn-range START..END] [--since WHEN] [--until WHEN]
             [--max-count N] [--resolve-persisted] [--format text|json]
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
csift search "persisted-output" --resolve-persisted --format json
```

Text output shape:

```
═══ SESSION 0a1b2c3d · TURN 0 ═══
◂ user  2026-06-03 22:51:53 AEST (2026-06-03T12:51:53.206Z)
   Audit harness-correctness in the worktree … (+2958 chars)

matched 2 exchanges (category=user)  ·  18 dropped by --max-count
```

JSON: one object per exchange (`session_id`, `turn_index`, `hits[]` with
`{category, excerpt, ts_utc, ts_local, tool_name}`, `record_uuids[]`), then a trailing summary
object `{matched, dropped_by_cap, skipped_lines}`.

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
switch axis. A subagent's id is the **bare hex** everywhere (`agents`/`files`/`recover`/`list`), so a
file mutation is joinable back to its node.

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
`--format json` emits `{"session_id":"…","path":"…"}`.

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
  a `>` or quote inside them can no longer fabricate a redirect row. Bash carries no path field in
  its result, so all of the above are best-effort and **always labelled `(heuristic)`**.

**Subagent scope** (mutually exclusive): default spans subagents; `--no-subagents` reports only the
top-level session's own mutations; `--subagents-only` is the **complement** — only the files the
session's subagents created/modified, with the top-level session excluded (one command for the
"what did the fan-out touch?" set-difference that previously needed two runs).

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
`gaps`-style doc). For the create count, use `--timeline --format json` and filter `op` over `/tmp`
rows with `is_create == true` — the per-mutation `is_create` flag lives **only** in the `--timeline`
JSON; `--by-file` rows carry per-op COUNTS (`write`/`edit`/`bash`/…), not the create flag. The
`--summary` view shows the same as bucket op-counts + distinct-file counts.

Text output shape (Bash counts suffixed `(heuristic)`):

```
SESSION 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  /p/spec/gaps: 3 edit
  /tmp: 2 write, 1 bash (heuristic)

5 distinct file(s)  ·  6 mutation(s)  ·  detail=summary  ·  all turns
(Bash mutations are heuristic — parsed from the command string.)
```

JSON: one object per emitted unit (bucket / dir / file with `{session_id, <key>, write, edit,
notebook_edit, multi_edit, bash, total, distinct_files, first_utc, first_local, last_utc,
last_local}`; or per mutation for `--timeline` with `{session_id, path, op, ts_utc, ts_local,
turn_index, is_create, heuristic}`), then a trailing summary object
`{distinct_files, total_mutations, skipped_lines, detail_level}`.

### `recover` — reconstruct a file's content (or restore a plan)

```
csift recover [PATH...] [--session ID] --file <ABS_PATH> [--no-subagents]
              [--patches | --at WHEN | --coverage(--dry-run) | --plan]
              [--turn-range START..END] [--since WHEN] [--until WHEN]
              [--line-range START..END] [--out PATH] [--format text|json]
```

Where `files` reports THAT a file changed, `recover` rebuilds its **content** by replaying the
file's Read / Write / Edit stream in transcript order. Four mutually-exclusive modes (default
`--patches`):

- **`--patches`** (DEFAULT) — segmented unified-diff history of `--file`, split at **integrity
  boundaries** where reconstruction across them is invalid: a `modified since read` harness error
  (authoritative), an `originalFile` that disagrees with the replayed buffer (authoritative — the
  signal other recovery tools discard), an external `edited_text_file` (authoritative), or a Bash
  mutation (heuristic, always flagged). No diff spans a boundary.
- **`--at WHEN`** — the **partial, line-numbered "in the LLM's eyes" snapshot** as of a cutoff
  (ISO8601, relative `2h`, `@turn:<N>`, or `@line:<N>`). Known lines carry their number; unknown
  regions are explicit `??? lines A..B unknown` markers — **gaps are NEVER fabricated**.
- **`--coverage`** (alias `--dry-run`) — scope a recovery without dumping content: recoverable line
  ranges, where the boundaries sit, per-op counts (reads / edits / writes / bash / external-edits),
  fragment count.
- **`--plan`** — restore a plan (an `ExitPlanMode` text or a plan-file `Write`). TWO paths: **with
  `--file <abs>`** it reconstructs THAT plan file's `Write` content; **without `--file`** it
  ENUMERATES every plan candidate in range (prints `plan candidates: N`) and `--out` then writes the
  single latest heuristic-matched candidate. `--file` is optional only here, with `Lnnn`/turn/
  timestamp provenance.

`--file` is **required** for `--patches`/`--at`/`--coverage`, optional for `--plan`. `--out` writes
the reconstructed artifact (snapshot / plan / concatenated patches) verbatim to a file while the
summary still prints to stdout. **Every output line carries the JSONL line number** (`Lnnnnn`) so you
can `Read` the raw jsonl directly. Reconstruction is necessarily PARTIAL: an un-anchorable edit (its
old text over an unknown gap, or whose context disagrees with the buffer) is a counted coverage hole,
never a fabricated line — so the contiguous-from-line-1 prefix matches the on-disk file exactly.

Examples:

```bash
csift recover . --file /abs/PLAN.md --coverage          # scope first: covered ranges + boundaries
csift recover <uuid> --file /abs/app.py --patches       # segmented unified diffs over the session
csift recover <uuid> --file /abs/app.py --at @turn:42   # partial snapshot as the LLM saw it at turn 42
csift recover . --plan --out /tmp/restored-plan.md      # list plan candidates; write the latest to a file
csift recover . --plan --file /abs/PLAN.md --out /tmp/p.md  # reconstruct THAT plan file's Write content
```

JSON is NDJSON: one object per segment / boundary / snapshot / plan-candidate (every object carries
`line_no` + `ts_utc`/`ts_local`; `--at` lines carry `set_at_line` provenance), then a trailing summary.

**Use it to** restore a plan a compaction or bad-recovery dropped, extract a file's diff-history over a
turn/time range, or check (via `--coverage`) whether a file is even worth attempting to reconstruct
and where it will break — before dumping anything.

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
            [--out PATH] [--format text|json]
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
`task`, where `monitor` is the ScheduleWakeup / monitor / cron-tick family), NOT a hardcoded
`workflow` — instead of the raw `<task-id>`/`<output-file>`/`<status>` XML blob, and the `turns`
per-session header reports the human/automation split (e.g. `selected 20 user (3 automation
triggers) + 52 assistant units`). The trigger still opens a turn, but it is EXCLUDED from the
`--round-trip-fraction` HARD FLOOR (that lane is reserved for human exchanges) — it can still be picked
as Phase-2 fill. So a consumer sees at a glance which "user turns" were machine pulses, and the human
round-trip floor is never silently spent on a pulse→ack pair. In `--format json` the automation
attribution is STRUCTURAL on the user-segment object (`is_automation` + `trigger_kind` + `task_id` +
`status`), not only a text prefix; the stream opens with a `{kind:"session_header",…}` object carrying
the human/automation split + budget fan-out.

**Budget model.** `--budget` (default 40000, chars or `--budget-unit tokens` ≈4 chars/token) is applied
**PER session in scope** — and a bare-uuid target SPANS that session's subagents by default, so the
realized total is `budget × (sessions in scope)`. When more than one session renders, a top-of-output
`SCOPE` banner names the count + the top-level/subagent split + the realized multiplier; the
single-thread recovery use case (`turns <my-uuid>`) usually wants `--no-subagents`. Selection is
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
- **the LAST** — kept only when it is itself rich/substantive; a short throwaway wrap-up collapses
  (THE headline fix — the last is no longer kept unconditionally).
- **everything else** collapses into a placeholder.

A message is "rich" by a cheap single-pass test: a number-of-substance (`12 passed 3 failed`, `12/40`),
a commit-hash-like hex, a `file.rs:NNN` ref, a backtick `code` path, a finding/decision lexeme (`found`
/ `confirmed` / `root cause` / `DEFER` / `x` / `x` / `x` / …), or simply a body ≥
`--agent-rich-min-chars` (default 280). **`--agent-rich-min-chars` is the tuning knob for both the
default and `rich`:** in `longest` it gates the "keep the first if substantive" decision AND the rich
length arm; raise it to keep fewer first/middle messages, lower it to keep more.

In `rich` mode the spine is KEEP-ON-DOUBT instead: only a short (`< --agent-declaration-max-chars`,
default 200) signal-less intent-verb opener (`let me …` / `now I …` / `x…`) is COLLAPSED; anything
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

Subagent transcripts (`--include-subagents`, default ON) get the SAME richness treatment via the shared
code path.

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
```

JSON (`--format json`) emits one VERBATIM (un-truncated) object per emitted unit, interleaved
compaction-boundary records, and a `collapsed_agents` placeholder record (with `agent_messages` /
`tool_calls` / `failed` / `first_line` / `last_line`) per collapsed span.

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
user messages that were previously missed).

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

## Integration recipes

### (A) Post-compaction standing-directive recovery — the motivating use-case

**Problem.** When Claude Code compacts context, the whole conversation is replaced by a single
summary written by the same main-loop model in one lossy pass. The loss is **recency-biased**, not
importance-biased: durable STANDING DIRECTIVES — persistent "keep going / don't stop until done"
style instructions the user hammered early — get **dropped or even inverted** (e.g. a standing
"never pause mid-task" silently becoming "waits are legitimate"). The lossless signal still exists:
the full pre-compaction transcript is on disk as the session jsonl.

**Intended integration.** A Claude Code **`SessionStart` hook with `source: "compact"`** (the
`matcher` for after auto/manual compaction) runs `csift` to compare the just-written compaction
**summary** against the lossless session **jsonl** (`transcript_path` is provided on the hook's
stdin), then emits a pointer-note back into context via `hookSpecificOutput.additionalContext` so
the freshly-compacted session re-sees the directives the summary lost. csift supplies the lossless
side: `csift search` over genuine-user turns (`-t user`) recovers the original directive text, and
the summary is the `isCompactSummary:true` record at/after the `compact_boundary`. Hook event +
field shapes per the official docs: https://code.claude.com/docs/en/hooks.md

Skeleton (`.claude/hooks/csift-compaction-rescue.sh`, registered for `matcher: "compact"`):

```bash
#!/usr/bin/env bash
# SessionStart(source=compact) — surface standing directives the summary dropped/inverted.
set -euo pipefail
input="$(cat)"                                          # hook JSON on stdin
session_id="$(printf '%s' "$input" | jq -r '.session_id')"
transcript="$(printf '%s' "$input" | jq -r '.transcript_path')"   # lossless jsonl

# Lossless side: genuine-user directive text from the full transcript.
# (A real implementation does a STRUCTURED semantic loss-diff against the
#  isCompactSummary record — see "hard problems" below — not a naive grep.)
directives="$(csift search "" -t user --session "$session_id" --format json 2>/dev/null \
  | jq -rs '...semantic loss-diff vs the compaction summary...')"

if [ -n "${directives:-}" ]; then
  jq -n --arg ctx "STANDING DIRECTIVES the compaction summary dropped/inverted:
$directives" \
    '{hookSpecificOutput:{hookEventName:"SessionStart", additionalContext:$ctx}}'
fi
```

Register in `settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "compact",
        "hooks": [
          { "type": "command",
            "command": "${CLAUDE_PROJECT_DIR}/.claude/hooks/csift-compaction-rescue.sh" }
        ]
      }
    ]
  }
}
```

**HONEST status — hard problems to solve before this is reliable (do NOT ship a naive regex diff):**

1. **The compaction summary is newest-first** and is itself a bounded-length artifact — it may not
   reach old directives at all. A directive issued near session start can be entirely absent from
   the summary, so "present in summary?" is the wrong test for "survived". You must compare against
   the *occurrence history* in the lossless jsonl, not against the summary's coverage window.
2. **~10% of records can lack a `headUuid`** (and some records have no `timestamp`), so you cannot
   assume clean uuid/parent linkage or strict chronological anchoring for every record. The diff
   must tolerate missing-linkage records rather than crash or silently drop them.
3. **A durable occurrence ledger is required.** "Did this directive get dropped/inverted?" is a
   judgement over how many times + how recently the directive appeared across the whole session
   (and prior sessions), versus how the summary now phrases it. That needs persisted state across
   compactions, not a single point-in-time grep.
4. **Hallucination + secret guards.** The note is injected straight back into model context, so the
   diff must not fabricate a directive that was never given, and must not echo secrets/credentials
   that appeared in the transcript. Treat surfaced text as untrusted and bound + sanitize it.

Net: a **structured semantic loss-diff** (directive occurrence ledger → compare lossless history vs
summary phrasing → flag dropped/inverted, with hallucination/secret guards) is required. A naive
regex diff of summary-vs-jsonl is insufficient and will mislead. csift provides the fast,
record-aware lossless extraction layer this diff sits on top of; the ledger + semantic comparison +
guards are the part still to be built before the hook is trustworthy.

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
# The pattern mixes English directives with a multi-byte CJK token to show that regex
# search handles arbitrary UTF-8 literals (serde_json emits non-ASCII verbatim):
csift search "don't stop|keep going|x" -i --session "$(csift whoami --format json | jq -r .session_id)"

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
