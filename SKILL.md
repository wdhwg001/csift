---
name: csift
description: Search, recover, and audit Claude Code session + subagent transcripts (the `.jsonl` logs under ~/.claude/projects). Reach for csift whenever a task touches a PAST or the CURRENT Claude Code session — regex-search what was said/done across sessions; list sessions to identify "which session is this"; recover a file's content (or a DELETED plan) from the transcript's Read/Write/Edit stream; restore the verbatim user/assistant turns a context-compaction summary clipped (e.g. standing directives, an earlier decision); see which files/dirs a session changed and when; inspect a session's subagents (built-in Task + workflow/OMC agents) — lifecycle, status, topology; locate the plan file bound to a session; identify the calling session; or extract images pasted into a transcript. ripgrep-for-transcripts: pure regex, no embeddings/semantic search.
user-invocable: true
---

# csift — ripgrep for Claude Code session transcripts

Fast Rust CLI to **search / recover / audit** Claude Code (CC) session `.jsonl` logs under
`~/.claude/projects/<encoded-cwd>/`. The consumer is an LLM (a CC agent inspecting its own or
a peer session), so output is clean, token-efficient, regex-driven text; add `--format json`
for machines. **Pure regex — no embeddings / BM25 / semantic search.**

Custom home: `--claude-home <DIR>` or CC's own `$CLAUDE_CONFIG_DIR` repoint `~/.claude` on
EVERY subcommand (flag > env > `$HOME/.claude`); transcripts read from `<DIR>/projects/<enc>/`.

## Reach for csift when… (question → command)

| You need | Run |
|---|---|
| what a session said/decided about a topic | `csift search "TOPIC" @<uuid>` |
| identify "which session is this" / index sessions | `csift list` · `csift list .` (this cwd) |
| which files/dirs a session changed, when | `csift files @<uuid> --by-file` |
| restore a file's content from the transcript | `csift recover @<uuid> --file /abs/x.rs` |
| get back a DELETED plan | `csift recover @<uuid> --file @plan --out plan.md` |
| restore verbatim turns a compaction summary clipped | `csift turns @<uuid> --budget 40000` |
| a session's subagents — kind / status / topology | `csift agents @<uuid> [--tree]` |
| which session is bound to a plan file | `csift plan --reverse /abs/plan.md` |
| identify the CALLING (current) session | `csift whoami` |
| identify which SUBAGENT *I* am | `csift agents @trap:<MarkerYouInvent4827>` |
| pull a pasted/screenshot image back out | `csift image @<uuid> --out /tmp/imgs` |
| fetch ONE exact message/record in full | `csift search "" @<uuid> --line <N>` |

No target ⇒ scan every project. A bare uuid is NOT a target — prefix it `@<uuid>`.

## Targeting — the positional `[PATH]...`

Every subcommand except `whoami` takes the SAME positional target. One of:

- **real cwd** (path-encoded for you) or an already-encoded `-Users-…` dir or `.` (this cwd's project) ⇒ scopes to project(s).
- **`@<uuid>`** (8-4-4-4-12) ⇒ one top-level session, searched across all projects if no project path is given.
- **`@<uuid-prefix>`** — leading 4–11 hex, e.g. `@13d9645a` (a uuid's first segment) ⇒ the UNIQUE session it prefixes (ambiguity → error listing candidates).
- **`@main`** ⇒ the calling TOP-LEVEL session (reads `$CLAUDE_CODE_SESSION_ID`).
- **`@trap:<marker>`** ⇒ the calling SUBAGENT (see below — a subagent cannot read its own id from the env).
- **`@<agent-hex>`** (≥12 hex, from `csift agents`) ⇒ that subagent + its topological descendants — or the agent ALONE under `--no-subagents`.
- **`*.jsonl`** path ⇒ that one transcript (a subagent transcript scopes to that agent's subtree).

`search` puts PATTERN first, so `csift search <uuid>` is a LITERAL pattern; scope with `csift search PATTERN @<uuid>`.

### Subagent span (default ON for `list`/`search`/`files`/`recover`/`plan`/`image`)

Those six span each session's subagent transcripts by default; `--no-subagents` restricts to
the top-level thread (dominant — wins in any position). `turns` is the EXCEPTION: its
per-session budget multiplies across fan-out, so it defaults to the top-level thread only and
`--include-subagents` opts in. `files` also has `--subagents-only` (ONLY what subagents
touched). `agents` LISTS subagents as targets, so it has no span flag.

### `@trap:<marker>` — "which subagent am I?"

CC sets `$CLAUDE_CODE_SESSION_ID` to the TOP-LEVEL id in every Bash env — even inside a
subagent — so a subagent can't name itself via env. Instead: **INVENT a marker and put it
LITERALLY in this very csift command**; csift finds the transcript whose Bash `csift` command
carries that marker (CC flushes a subagent's tool_use to disk before it runs → resolves
first-try) and scopes to that agent's subtree.

Marker discipline (csift ENFORCES it): invent it **one-shot, by you, right now** — an
imaginative, random token of **≥3 CamelCase words + 4 digits**, e.g. `@trap:JollyShinyBrook4283`.
NEVER script-generate it (the generator is itself a `csift` Bash call carrying the marker →
ambiguity), never build it from a shell variable/concatenation (must appear verbatim), never
reuse one. Rejected: <3 words, single-letter or ALLCAPS "words", not exactly 4 trailing
digits, or trivial digits (`1111`/`1234`/`9876`/`1357`/`2468`). From the MAIN thread just use
`@main` (a main-thread marker flushes only at turn end → may need a re-run).

## Subcommands

### `list` — fast "which session is this?" index
```
csift list [PATH...] [--no-subagents] [--format json]
```
Head+tail read only (never parses the whole file): per session prints first/last genuine-user
preview + last agent message + identity (branch, CC version, decoded cwd) + skipped-malformed
count. Spans subagents by default.

### `search` — regex round-trips + the message fetcher
```
csift search [PATTERN] [PATH...] [-t CAT]... [-i] [--multiline] [--since 2h] [--until ISO]
             [--turn-range A-B] [--max-count N] [--no-subagents] [--full] [--line SPEC] [--uuid U] [--format json]
```
Smart-case regex (case-insensitive unless PATTERN has an uppercase; `-i` forces). Empty
PATTERN = pure filter (combine with `-t`/`--since`/`--turn-range`). On a hit, returns the
COMPLETE round-trip (a matched `tool_use` WITH its `tool_result`; a user turn WITH the agent
reply). Capped output STATES how many were dropped.
- `-t/--category` (repeatable): `thinking` · `user` · `tool` · `tool-response` · `agent`.
- **Fetch mode** — `--line N` / `N-M` / `N,M` (needs a single transcript in scope, e.g. `@<uuid> --no-subagents`) or `--uuid <recordUuid>`: emit those records in FULL (the in-permission alternative to `Read`-ing raw jsonl). `--full` = no excerpt truncation on every hit.
```
csift search "panic" @<uuid> -t agent --since 6h
csift search "" @<uuid> --no-subagents --line 46550        # the exact message a hit cited, full
```

### `agents` — subagent lifecycle + topology
```
csift agents [PATH|@<uuid>] [--agent HEX] [--kind builtin-task|workflow] [--since][--until]
             [--by trigger|start|completion] [--tree] [--with-files] [--returned-message] [--format json]
```
Lists a session's subagents: kind (by on-disk location, NOT agentType), trigger/start/completion,
status. `--by` time axis defaults to **trigger** (true spawn instant). `--tree` renders the
agent→agent topology (nesting is flat on disk, reconstructed via the spawning `tool_use` link →
`parent_agent_id`/`depth`). Printed `session_id` is the bare `<hex>` (joinable to files/recover/list).
`--agent <hex>` drills into one; `--returned-message` adds each node's 3-way-resolved return.

### `whoami` — identify the calling session (false-positive-safe)
```
csift whoami [--show-path] [--format json]
```
Reads `$CLAUDE_CODE_SESSION_ID` (CC sets it per Bash process; its value IS the calling
session's jsonl basename); falls back to `$CODEX_COMPANION_SESSION_ID`. If neither is set it
**errors with guidance** to pass an `@<uuid>` — it NEVER guesses by mtime (many sessions run
concurrently). JSON carries only `{session_id, path}`; to learn "am I a subagent?" feed the id
to `csift agents --agent <id> --format json` and read `parent_session_id`/`is_subagent`.

### `files` — what a session changed, and when
```
csift files [PATH|@<uuid>] [--by-file] [--timeline] [--subagents-only] [--no-subagents] [--since][--until] [--format json]
```
Edit/Write/Notebook + heuristic Bash mutations. Default = coarse op rollup; `--by-file` = per-file
counts + first/last touch; `--timeline` = full chronological (heavy). Also detects
**Edit-before-Read boundaries** (files changed outside the tool stream — the risky-to-reconstruct
signal). Every row carries its `Lnnnn` jsonl line.

### `recover` — reconstruct a file (or plan) from the transcript
```
csift recover @<uuid> --file <ABS_PATH|@plan> [--out PATH] [--salvage] [--patches]
              [--at WHEN] [--line-range A..B] [--coverage] [--files-from MANIFEST] [--format json]
```
Default = restore RAW final content, or **fail (never a holey file)** if the session saw only
part — then reach for `--salvage` (best-effort, line-numbered, gaps explicit). `--patches` =
segmented unified diffs between integrity boundaries (no diff spans a boundary; each carries
line/turn/ts). `--at <ISO|2h|@turn:N|@line:N|@latest>` = point-in-time partial snapshot as the
LLM saw it. **`--file @plan`** = the bash-safe sigil for the session's bound plan (rebuilds even
a DELETED one). A `modified since read` boundary invalidates the pre-boundary buffer (no stale
lines presented as current).
```
csift recover @<uuid> --file /abs/app.py --out /abs/app.py   # straight back onto disk (raw bytes)
csift recover @<uuid> --file @plan --out /tmp/plan.md        # rebuild the bound plan, deleted ok
```

### `plan` — locate the plan file bound to a session (+ reverse lookup)
```
csift plan [PATH|@<uuid>] [--reverse PLAN_FILE] [--no-subagents] [--format json]
```
Resolves the `plan_mode`-attached plan path (LOCATES, doesn't dump — use `recover --file @plan`
to dump). No target ⇒ the calling session. **`--reverse <plan.md>`** inverts: "which session(s)
are bound to this plan file?" (an empty result is honest, not an error).

### `turns` — restore the verbatim turns a compaction summary clipped
```
csift turns @<uuid> [--budget N] [--include-subagents] [--agent-msgs longest|eot-only|rich|all]
            [--profile heavy|light] [--round-trip-fraction F] [--format json]
```
A CC compaction summary keeps task STATE but loses TURN fidelity (clips ~22 user prose turns →
~17 bullets; ~239 assistant turns → 1 quote). `turns` SUPPLEMENTS it by restoring verbatim
user/assistant turns in order within a char/`--budget`. `--agent-msgs longest` (default) keeps
the substance-bearing message per turn + collapses throwaway run into a fetchable `△ L..–L..`
placeholder. Top-level thread only unless `--include-subagents`.

### `image` — get a pasted image back out
```
csift image @<uuid> [--out DIR|FILE.ext] [--id '#N'|L<line>i<n>] [--no-subagents] [--since][--turn-range][--uuid] [--format json]
```
Pasted/screenshot images live INLINE as base64 blocks. Lists them (id · type · ~size · time);
`--out <dir>` decodes all to files (source formats), `--out <file.jpg|gif|webp|png>` converts a
single one. Address by the `#N` handle the session uses (`--id '#32,#33'`; bare `32`==`#32`) or
the exact `L<line>i<n>` locator. An ambiguous `#N` (CC reuses them across prompts) ERRORS with
the occurrence list — disambiguate via the locator or `--since`/`--turn-range`/`--uuid`. `#N`/`L..i..`
are per-transcript, so `--id` needs a single transcript in scope (`@<uuid> --no-subagents`).

## Reading the output (text)

Each session is declared ONCE — `s1 = <uuid>` (a subagent: `s2 = <hex> (subagent · parent s1)`)
— then exchanges reference the label: `s1·t<turn>  <local-ts>` (one system-local timestamp, ms +
offset, e.g. `2026-06-20 22:14:07.811+10:00`). Each matched record is a line `▸ <category>
L<line>  <excerpt>` (a `tool-response` row names its tool; excerpts center ~400 chars —
`--full`/`--line` give the whole record). A footer totals it: `matched N exchanges · M sessions ·
category=…`. `--format json` = one object per record/row/session with stable fields
(`line`/`uuid`/`ts_utc`/`ts_local`/`tool_name`/…).

## Gotchas

- **A `type:"user"` record is usually NOT a human turn** — `tool_result` carriers ride on
  `role:"user"` too (often 4–5× the genuine ones). csift's genuine-user classification handles
  this; the `user` category = real input + AUQ answers only.
- **A pending AskUserQuestion is NOT in the jsonl** — only an ANSWERED one (its answer returns
  as a later record + opens its own turn).
- **`whoami` from an orchestrated/workflow subagent** may report the PARENT id (the env holds
  the parent there); from a built-in Task subagent it's the subagent's own id. Disambiguate via
  `agents --agent <id>`. To target your own subagent reliably, use `@trap:<marker>`.
- **No silent truncation**: a capped/`--max-count` result says how many dropped; a skipped
  malformed line is counted, never hidden.
