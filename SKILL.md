---
name: csift
description: Search, recover, and audit Claude Code (CC) session + subagent transcripts - the `.jsonl` logs under ~/.claude/projects. Reach for csift for ANY task touching a PAST or the CURRENT CC session; regex-search what was said/done across sessions; list sessions to answer "which session is this"; recover a file's content (or a DELETED plan) from the transcript's Read/Write/Edit stream; restore the verbatim user/assistant turns a context-compaction summary clipped (standing directives, an earlier decision); see which files/dirs a session changed and when; inspect a session's subagents (built-in Task + workflow/OMC agents) - lifecycle, status, topology; locate the plan bound to a session; identify the calling session; fetch one exact message in full; extract pasted images. Nine subcommands - list search agents whoami files recover plan turns image. Pure regex; no embeddings/BM25/semantic.
user-invocable: true
---

# csift - ripgrep for Claude Code session transcripts

Rust CLI over CC session `.jsonl` under `~/.claude/projects/<encoded-cwd>/`. LLM-facing: clean token-efficient regex text; `--format json` for machines. **Pure regex - no embeddings/BM25/semantic.** Regex = Rust `regex` 1.12 (RE2-class, linear-time): classes, alternation, groups, quantifiers (+lazy `*?`), anchors, inline flags `(?i)(?m)(?s)(?x)`, `\p{…}`. NOT supported (fail-to-compile, by design): backrefs `\1`, lookaround `(?=)(?!)(?<=)(?<!)`, atomic/possessive `(?>)`/`a*+`. Default **smart-case** (insensitive unless the pattern has an uppercase). Nine subcommands: `list search agents whoami files recover plan turns image`. Run `csift <cmd> --help` for the live flag list (the `--help` is the authoritative manual).

## Global conventions (every subcommand)
- `--claude-home <DIR>` (global, ANY position, before OR after the subcommand): repoint `~/.claude`. Priority **flag > `$CLAUDE_CONFIG_DIR` > `$HOME/.claude`**. Transcripts read from `<DIR>/projects/<enc>/*.jsonl`.
- `--format text|json` (default `text`). JSON = one object per emitted unit (JSONL), deterministic order. Whole-file `json.load` FAILS - parse line-by-line.
- Timestamps render local-TZ (via `jiff`) ± offset, e.g. `2026-06-21 21:17:24.563+10:00`; JSON carries paired `ts_utc`+`ts_local` (see §JSON conventions).
- **Relative WHEN** (`--since`/`--until`/`recover --at`) = `Ns`/`Nm`/`Nh`/`Nd`/`Nw` = seconds/minutes/hours/days/**weeks** ago (`45s` `90m` `2h` `3d` `1w`). Or absolute ISO8601 (bare date => local midnight); records w/o `timestamp` never match. A datetime bound is **<=-INCLUSIVE** (highest line whose `ts ≤ bound`).
- **`--turn-range A..B`** (search/files/recover/turns/image) is inclusive, **0-based** - **turn 0 = the pre-first-user lead** (synthetic opening-context chain folded into the first genuine-user turn), so **`1..N` SKIPS it**; `0..N` re-includes it. Discover indices from the `s<k>·t<n>` header in `search` text or `turn_index` in JSON.
- **Exit codes**: `0` success, **non-zero (=1)** on ANY error -> stderr, prefix `csift: error: `, full `anyhow` chain (levels joined `: `). No numeric codes.
- **NOT errors (exit 0)**: `turns --slice` out-of-range (prints nothing); `search --line/--uuid` to no record (`unresolved: …`). **HARD non-zero**: `recover` partial/no-history; `image --id` no-match/ambiguous; exclusive-flag clashes; bad address syntax; `agents --agent <hex>` no-match.
- **No silent truncation**: every cap (`--max-count`) reports its drop count; malformed lines are counted, never hidden.
- **Flag order is FREE.** A pre-pass reorders declared flags (+ their values) ahead of the leading-dash project-target positionals (single `-` accepted; `--`-leading rejected as a typo'd flag). Put flags anywhere - `csift list <ENCODED> --format json` works.

### §JSON conventions (shared by every command's JSON block below)
- **Leading `session_header` record (THE jq trap)** = `{"kind":"session_header","sessions_in_scope":N,"top_level_sessions":T,"subagent_sessions":S}`, the FIRST line whenever scope holds ≥1 subagent (`S>0`) - but **NOT every command emits it**. It has NO `session_id`, so naive `jq -r '.session_id'` over an emitter hits a null first row; filter: `jq -r 'select(.kind!="session_header") | .session_id'`. **EMITS it (`S>0`):** `list`, `files`, `turns`, `recover` non-restore (`--salvage`/`--patches`/`--at`/`--coverage`), `search` DEFAULT mode. **HEADER-FREE always:** `whoami`, `agents`, `plan`, `image`, `recover` restore, `search -c`. Suppressed when `S==0`. `turns` emits a SUPERSET header (those 4 fields PLUS budget/automation fields - see §turns) - don't key consumers on field-presence ACROSS commands.
- **Per-command TERMINATORS are NON-uniform** - can't key EOF on one shape. `turns` is the ONLY kind-tagged trailer (`{"kind":"skipped_lines",…}`, ALWAYS closes even at 0). `search`/`files`/`image` close with an UNTAGGED summary; `recover` non-restore closes a NESTED `{"summary":{…}}`; `list`/`agents`/`plan`/`whoami`/`recover` restore emit NO trailer (EOF). Per-command fields inline below.
- **Row trio (EVERY spanning row carries all three; see AGENTS.md §3.7)**: `session_id` (display: a re-feedable top-level uuid, OR a bare SUBAGENT hex), `is_subagent` (bool discriminator), `parent_session_id` (the ALWAYS-re-feedable owning uuid; == `session_id` on a top-level row). **NEVER re-feed a subagent's bare-hex `session_id` as `@<uuid>`** - use `parent_session_id`. (`agents` omits `is_subagent` - it only lists subagents, so it is implied-true; there `agent_id` IS the `session_id` concept.)
- **`ts_utc`/`ts_local`** travel as a pair on every timestamped record (`ts_local` = system-local ISO+offset, `None` if absent/unparseable). The `list` excerpt sub-objects nest them as `{excerpt, ts_utc, ts_local}`.

## Targeting - the positional `[PATH]...` (every subcommand except `whoami`)
0 targets => scan ALL projects. A bare uuid is NOT a target - prefix `@`.
- **real cwd** (path-encoded for you) | already-encoded `-Users-…` dir | `.` (this project) => scopes to project(s).
- **`@<uuid>`** (8-4-4-4-12) => one top-level session, across all projects if no project path. **`@<uuid-prefix>`** - 4-11 leading hex (`@13d9645a`) => the UNIQUE session it prefixes (else error).
- **`@main`** => calling TOP-LEVEL session (reads `$CLAUDE_CODE_SESSION_ID`). **`@trap:<marker>`** => calling SUBAGENT (see below).
- **`@<agent-id>`** (from `csift agents` — a ≥12 bare hex, OR a name-embedded **teammate** id like `aVSRepro-68a2a1661c9390c1`) => that subagent + descendants (or the agent ALONE under `--no-subagents`). EVERY id `agents` prints round-trips here (and as the `--line <id>:<spec>` pin). **`*.jsonl`** => that one transcript + subtree.

`search` puts PATTERN first: `csift search <uuid>` searches the LITERAL string; scope via `csift search PATTERN @<uuid>`.

### Subagent span - default ON for list/search/files/recover/plan/image; `--no-subagents` (DOMINANT, any order) restricts to top-level. `turns` is the EXCEPTION: top-level only by default, `--include-subagents` opts in (then `--budget` is PER session, so it MULTIPLIES). `agents` LISTS subagents; it has no span flag (`--no-subagents` there is a pointed error). There is NO `--include-subagents` on the default-on commands and NO `--subagents-only` anywhere.

### `@trap:<marker>` - "which subagent am I?"
CC doesn't give a subagent a RELIABLE self-id via env (a workflow `agent()` subagent's `$CLAUDE_CODE_SESSION_ID` is the PARENT id, a built-in Task subagent's is its OWN - you can't tell which; see §whoami), so a subagent can't dependably name itself via env. Fix: INVENT a marker, put it LITERALLY in THIS csift command; csift scans the session's main + subagent transcripts for a **Bash `tool_use`** whose **`input.command`** holds BOTH the marker AND literal `csift`. Matching the tool_use INPUT (not the line / a `tool_result`) avoids a false hit on an ECHOED marker. CC flushes the tool_use BEFORE it runs => resolves first-try. Resolution: 1 subagent => that agent; only main => session; 0 => error (re-run); >1 => ambiguity error. (On `whoami`, `@trap` returns the upstream ancestry CHAIN - see §whoami.)
**Marker grammar (ENFORCED, rejects loudly)**: one-shot, invented NOW, imaginative + CONTEXT-INDEPENDENT; ASCII + **byte length ≥ 13**; **EXACTLY 3 CamelCase words** (each 1 upper + ≥2 lower - no single letters, no ALLCAPS like `HTML`/`USB`) + **exactly 4 trailing digits**, not a trivial run (constant digit-step -2..2 => rejects `0000`/`1234`/`1357`/`9876`/`2468`). NEVER script-generate (a generator call itself carries the marker => ambiguity), never from a shell var/concat (must appear verbatim), never reuse. From MAIN use `@main` (its marker flushes only at turn end => may need re-run). The marker SHAPE is `@trap:JollyShinyBrook4283` - but **do NOT copy that literal**: it is the documented example, so it sits in THIS SKILL right next to `csift`, and csift **hard-rejects it** (a pasted example self-collides - every agent that copies it clashes into ambiguity). Invent a FRESH one each call.

## Categories (`search -t`) - `thinking | user | tool | tool-response | agent`
- `thinking` = assistant thinking. `tool` = `tool_use` (AUQ counts). `tool-response` = `tool_result` (each names its tool). `agent` = visible assistant end-of-turn text.
- `user` = GENUINE human input + **AUQ answers** (full Q+options+answer unit) + **plan-rejection-with-message** + machine **automation-trigger openers** (`<task-notification>` -> `[<kind> <id> <status>] <summary>`); NOT tool_result carriers (which ride `role:"user"` too, ~4-5x as many). An UNanswered AUQ never reaches disk; only an ANSWERED one opens a turn.
- **Automation `kind`** (longest leading prefix of `<summary>`, case-insensitive): `background command`->`background-command`, `dynamic workflow`/`workflow`->`workflow`, `monitor`/`scheduled`/`cron`->`monitor`, `agent`->`agent`, else->`task`. **Monitor RECLASSIFICATION (load-bearing)**: a captured monitor loop is often a `&`-detached `Background command "<name>"` leading with `background command`, so a pure prefix check buries it under `background-command` and `monitor` matches zero. Rescue: scan the **QUOTED command NAME** (between the first `"` pair) - a monitor-cadence token there (standalone `monitor`/`liveness`, or substring `re-arm`/`relaunch monitor`; `tick`/`cadence` EXCLUDED as too broad) re-routes to `monitor`. Quoted-name-only so trailing prose isn't over-captured; no quotes => `background-command`.
- **isMeta wakeup-tick BYPASS**: classification only sees `<task-notification>` COMPLETION pulses. The `ScheduleWakeup` **wakeup-tick prompts** DRIVING a monitor/cron cadence are `isMeta:true` user records, NOT `<task-notification>`s - they bypass automation parsing (the isMeta gate in `is_genuine_user`) so driving ticks don't surface as `-t user`. **NOTE: this `kind` taxonomy is a DIFFERENT axis from `agents --kind` (the transcript-SHAPE filter `builtin-task|workflow|teammate`)** - they share only the literal token `workflow`.

---

## `list` - fast "which session is this?" index
```
csift list [PATH...] [--no-subagents] [--format json]
```
Head+tail read only (fast on 200MB+). Per session: first/last genuine-user (+ts), last agent (+ts), decoded `cwd`, `version`, `gitBranch`, skipped count. Excerpts cap `… (+N chars)` at 200. `--no-subagents` = top-level rows only (span: see §Subagent span). Text leads with a `scope` banner + brands subagent rows `SUBAGENT <hex> · parent SESSION <uuid>`.

```
SESSION  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  cwd      /Users/testuser/Projects/widget_app_prototype   (branch main, CC 2.1.159)
  first ◂  2026-06-10 01:43:40 AEST (2026-06-09T15:43:40.392Z)
           Port the legacy settings screen to the new component library, keep the layout… (+2903 chars)
  last ◂   2026-06-21 21:17:24 AEST (2026-06-21T11:17:24.563Z)
           ...the latest user turn excerpt... (+1745 chars)
  last ▸   2026-06-21 22:31:49 AEST (2026-06-21T12:31:49.744Z)
           Batch 3 is fixed and committed... (+2995 chars)
```
Glyphs `◂`=user `▸`=agent. JSON per session (NDJSON, NO trailer, leading `session_header` if `S>0`): the row trio (§JSON conventions) + `path, cwd, version, git_branch, first_user, last_user, last_agent, skipped_lines`. The three message fields are `{excerpt, ts_utc, ts_local}` sub-objects or `null`.

## `search` - regex round-trips + message fetcher
```
csift search [PATTERN] [PATH...] [-t CAT].. [-i] [--multiline] [--since 2h] [--until ISO]
  [--turn-range A..B] [--max-count N] [-c|--count-only] [--siblings SPEC].. [--full|--no-truncate]
  [--line SPEC] [--uuid U] [--resolve-persisted] [--no-subagents] [--format json]
```
Empty PATTERN (`""`) = pure filter (combine `-t`/`--since`/`--turn-range`). A hit returns the COMPLETE round-trip: a `tool_use` WITH its `tool_result` (paired by `tool_use_id`); a user turn WITH the agent reply; matched thinking/agent text in its turn (an answered AUQ -> Q+options+answer together). Turn boundary = `opens_turn` (genuine user OR AUQ-answer OR plan-rejection; see Categories) - shared with turns/files/recover so they never drift.
- `-t/--category` (repeatable) - default all five. `-i` forces insensitive (over smart-case). `--multiline` lets `.` cross newlines.
- `--turn-range A..B` (see §Targeting) - **mutually exclusive** with `--since`/`--until`.
- `--max-count N` caps emitted exchanges (GLOBAL, AFTER the cross-scope `started_utc` sort); reports drops. **Default = UNLIMITED.**
- `-c/--count-only` (`-c`) - ONLY the integer total; honors every filter; **adds back the `--max-count` remainder** (true total). Text=`N`, JSON=`{"matched":N}`. To list WHICH sessions matched: `--format json | jq -r 'select(.kind!="session_header").session_id' | sort -u` (there is NO `-l`/`--files-with-matches`).
- `--siblings <SPEC>` (repeatable / comma-joined) - also render each matched turn's NON-matched records under a `·` marker (a matched user Q surfaces WITH the agent reply). Each SPEC token is a CAP: bare `N` caps the TOTAL of the non-typed categories; `<cat>:N` (cat in thinking|user|tool|tool-response|agent, ≥1) caps THAT category. Both forms: typed caps govern their category, bare `N` caps every OTHER; a category with no typed spec + no bare-`N` fallback is hidden. The match is never repeated. No `--sibling-category` flag (folded into SPEC). No effect under `-c`.
- **EXCERPT TRUNCATION (defeat it!)**: each hit shows a **~400-char excerpt CENTERED on the match**, elided `… (+N chars)`. **`--full`/`--no-truncate`** = whole record every hit; `--line`/`--uuid`-addressed records always render FULL.
- **FETCH mode** - `--line SPEC` (1-based physical line(s), comma+range `87,495-500`; **needs a SINGLE transcript**) or `--uuid U` (record uuid, comma-repeatable). Emits those records FULL - the in-permission raw-`Read` alternative. A singleton resolving to nothing => `unresolved: …` + exit 0 (a miss inside `--line A-B` is clamped). **`--line <hex>:<spec>`** - prefix the FIRST token with a subagent's bare hex to pin ONE subagent transcript (`--line 7f3c9e21:88,495-500`); else `--line` addresses the top-level transcript (`@<uuid> --no-subagents`). All hex-bearing tokens must name the SAME transcript. (Replaces the removed `--subagent HEX`.)
- `--resolve-persisted` - before matching, inline `<persisted-output>` pointers: a too-large `tool_result` saved to `tool-results/<id>.txt` gets FULL content inlined (via `toolUseResult.persistedOutputPath` or scraped `Full output saved to: <ABS>`) so a regex hits it. Read-failure non-fatal.

```
s1 = 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d

s1·t270  2026-06-21 21:17:24.563+10:00
  ▸ agent  L57738  Batch 3 is fixed and committed (`a1b2c3d4e` components, `5f6e7d8c9` tests)… (+2804 chars)
  · user   L57248  ...sibling (other side of the turn, shown under --siblings)...  [1 image: #265]

matched 1 exchange · 1 session · category=agent
```
The `s<k>·t<n>` header carries the turn index (`t270` => `--turn-range 270..270`). JSON per Exchange (leading `session_header` if `S>0`; row trio): `{…trio…, turn_index, ts_utc, ts_local, hits:[{category, line, uuid, ts_utc, ts_local, tool_name, excerpt, image_ids:[…]}], record_uuids:[…]}` (+`siblings:[…]` same shape under `--siblings`). `image_ids` (`#N`/`L<line>i<n>`) feed `image --id` (see §image). CLOSED UNTAGGED `{matched, sessions, dropped_by_cap, skipped_lines, unresolved:[…]}` (`unresolved` = explicit `--line`/`--uuid` tokens that hit nothing). `-c` JSON = `{"matched":N}` (header-free).

```bash
csift search "panic" @<uuid> -t agent --since 6h
csift search "" @<uuid> --no-subagents --line 46550        # the exact cited message, full
csift search "" @<parent> --line 7f3c9e21:88,495-500       # …in a SUBAGENT transcript
csift search "Full output" --resolve-persisted --format json  # inline persisted-output, then match
```

## `agents` - subagent lifecycle + topology
```
csift agents [PATH|@<uuid>] [--agent HEX] [--kind builtin-task|workflow|teammate].. [--since][--until]
  [--order-by trigger|start|completion] [--with-files] [--returned-message] [--format json]
```
Lists a session's subagents as an ALWAYS-rendered parent->child tree (NO `--tree` flag; there is no flat mode). **Kind = on-disk PATH LOCATION, not `agentType`**: `builtin-task` = `subagents/agent-<hex>.jsonl`; `workflow` = `subagents/workflows/wf_<id>/agent-<hex>.jsonl`; `journal.jsonl` is an EVENT log, not a transcript. **EXCEPTION `teammate`** (the new `in_process_teammate` / FleetView agents): same built-in location as `builtin-task`, distinguished by the meta `taskKind:"in_process_teammate"`. A teammate's id EMBEDS its name (`aVSRepro-68a2a1661c9390c1`), its meta overloads `agentType` with the handle + omits `toolUseId`, and it is messaged bidirectionally via `<teammate-message>`/`SendMessage` — csift recovers its real `agent_type` (the spawning `Agent` tool's `subagent_type`, e.g. `oh-my-claudecode:qa-tester`), spawn linkage, true trigger ts via a NAME-join, and adds JSON `name`/`team_name`. Canonical id joins to files/recover/list/search (re-feed it as `@<id>`). **CONTROLLING a teammate is NOT a csift op** (csift is read-only) — the output carries a pointer to the right tool: a teammate is steered/terminated via the **`SendMessage`** tool addressed BY NAME (`message:{type:"shutdown_request"}` terminates it), NOT `TaskStop` (that only finds `run_in_background` tasks by `task_id` — it rejects EVERY teammate id form) and NOT `pkill` (a teammate is an in-process Agent subagent sharing the orchestrator PID — no separate process). Text prints this as a footer when ≥1 teammate is in scope; JSON puts it on each teammate node's `control_hint`. `agentType` is NOT a reliable discriminator (kinds spread `Explore`/`general-purpose`/`oh-my-claudecode:*`); only `workflow-subagent` is workflow-exclusive, kept as a per-row sub-label (JSON `agent_type`).
- **`--kind` = TRANSCRIPT-SHAPE filter, enum `builtin-task|workflow|teammate` ONLY** (a DIFFERENT axis from the automation-trigger `kind` taxonomy in §Categories - they share only the token `workflow`). `csift agents --kind monitor` is a clap PARSE error, not an empty result. Ignored when `--agent <hex>` pins one. (`--no-subagents` is a hidden no-op that errors pointedly; `--include-subagents`/`--subagents-only` no longer exist - `agents` has no span control.)
- **`--order-by`** (NOT `--by`) sets BOTH the tree sort AND the `--since`/`--until` axis: `trigger` (DEFAULT - the parent tool_use ts, the true spawn instant), `start` (child's first record, LAGS trigger 0.2-4.7s), `completion` (last record). Workflow agents lack `toolUseId` => trigger->start.
- **TOPOLOGY**: FLAT on disk at every depth (CC writes every subagent - even a sub-subagent - under the MAIN session's `subagents/` dir). Nesting is LOGICAL, from the spawning `Task`/`Agent` `tool_use` + child meta's `toolUseId`: a GLOBAL spawn index sets `parent_agent_id` (null => direct child, depth 0) + walks `depth`. All real data is depth 0 today.
- **`--agent HEX`** drills into ONE node (implies `--returned-message`; renders a tree-of-one; BYPASSES `--since`/`--until`/`--order-by`/`--kind`; a non-matching hex HARD-errors). **`--with-files`** attaches files-changed. **`--returned-message`** (3-way; auto for a single `--agent`; JSON `returned_message_source`): (a) sync built-in => parent tool_result text (`sync-tool-result`); (b) async built-in => child transcript tail (`async-child-tail`); (c) workflow => `journal.jsonl` `result` (`workflow-journal`). Status: `completed`/`running`/`unknown`.

```
SESSION 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  WORKFLOW  wf_2d6f1a8c-031  [audit-error-paths]  completed
      agents 18 · duration 10m42s · tokens 1840293 · model claude-opus-4-8[1m]
    c4f07a2b9e1d6358a  workflow  (wf_2d6f1a8c-031)  [workflow-subagent]  completed
      triggered  2026-06-14 18:24:53 AEST (2026-06-14T08:24:53.696Z)
      started    2026-06-14 18:24:53 AEST (2026-06-14T08:24:53.696Z)
      completed  2026-06-14 18:29:10 AEST (2026-06-14T08:29:10.363Z)
      duration   4m17s
```
JSON: header-free; one object PER SESSION `{session_id, workflow_runs:[run], agents:[node]}` (always this nested shape; NO trailer). A single `--agent` grab is the ONE exception - a BARE node object (no session wrapper). Node fields: `{agent_id, agent_type, name, team_name, kind, status, parent_session_id, parent_agent_id, workflow_id, depth, description, spawn_tool, spawn_tool_use_id, trigger_utc/local, started_utc/local, completed_utc/local, duration, skipped_lines, children:[node]}` (`name` = the `Agent` tool's name / teammate handle; `team_name` non-null only for a `kind:"teammate"`; a `kind:"teammate"` node ALSO carries `control_hint` = the SendMessage-not-TaskStop pointer) + on demand `returned_message(_source)`, `files_changed:[{path, op, is_create}]` (`op` HYPHENATED `write`/`edit`/`notebook-edit`/`multi-edit`/`bash`). `agent_id` IS this row's transcript-own id (the `session_id` concept; bare hex; `is_subagent` implied-true + omitted) - re-feed `parent_session_id`, never `agent_id`. RUN object (all snake_case - a camelCase `jq` like `.runId` returns null): `{run_id, task_id, workflow_name, status, agent_count, duration_ms, total_tokens, total_tool_calls, default_model, started_utc, started_local, children:[node]}`.

## `whoami` - identify the calling session (false-positive-safe)
```
csift whoami [@trap:<marker>|@main] [--format json]
```
Reads `$CLAUDE_CODE_SESSION_ID`; alias `$CODEX_COMPANION_SESSION_ID`. NEVER a loose `/session/i` regex (`SECURITYSESSIONID` is a trap). Neither set => **errors** (pass `@<uuid>`); never guesses by mtime. The `path` line is ALWAYS printed (there is NO `--show-path` flag; a `<not found>` note when unresolved). Optional positional **`@trap:<marker>`** (grammar: see §Targeting) = "which SUBAGENT am I?" - returns the full UPSTREAM ancestry CHAIN (self -> … -> top-level root; the walk-UP mirror of `agents`' walk-DOWN), env-INDEPENDENT (works for built-in Task AND workflow subagents). **`@main`/none** = env top-level.
- **SUBAGENT caveat**: the env var names the SUBAGENT's own id in a built-in Task subagent, but the PARENT id in a workflow `agent()` subagent. Without `@trap`, disambiguate: `csift agents --agent <id> --format json` -> a returned node means read `parent_session_id` for the root; `no subagent matched` means the id is ALREADY top-level.

```
session  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
path     ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype/0a1b2c3d-….jsonl
```
JSON (header-free, NO trailer): env form `{session_id, path}`; `@trap` form `{chain:[{session_id, is_subagent, parent_session_id, depth, path}, …]}` (self first, top-level root last - a subagent reads `is_subagent`/`parent_session_id` directly, no `agents` round-trip).

## `files` - what a session changed, and when
```
csift files [PATH|@<uuid>] [--by summary|dir|file|timeline] [--regex RE] [--glob PAT]
  [--no-subagents] [--turn-range A..B] [--since][--until] [--format json]
```
Authoritative for Edit/Write/MultiEdit/NotebookEdit (`input.file_path`/`notebook_path`; create-vs-edit from the paired `toolUseResult.type`). **Bash mutations are HEURISTIC** (lexical parse of `rm`/`mv`/`cp`/`mkdir`/`touch`/`tee`/`sed -i`/`git`/redirection; no path field) - always `(heuristic)`, relative paths VERBATIM. A failed `is_error:true` op is EXCLUDED. Spans subagents by default (OMC fan-out edits live there).
- **`--by <summary|dir|file|timeline>`** (default `summary`, strictly coarsening; NOT 4 booleans): `summary`=coarse top-level-prefix op rollup (whole tree -> one row); `dir`=one row per FULL parent dir + per-op + distinct-file + first/last; `file`=one row per abs path + per-op counts + first/last; `timeline`=full chronological, one line/mutation (HEAVY).
- **`--regex <RE>`** (Rust regex, matches ANYWHERE in the full abs path, no smart-case) + **`--glob <PAT>`** (full-path glob, `**` crosses `/`) - filter by FULL path, ANDed together, applied BEFORE the rollup (so every view + the Edit-before-Read section reflect the filtered set). Invalid pattern => hard error; removes-everything => normal empty output. (No `--subagents-only`; `--no-subagents` is the only span flag.)
- **Edit-before-Read boundaries** section follows the body in EVERY mode (or alone): `(⚠ path, Lnnnn, turn, ts, kind)` - a formatter/husky/git/external-editor changed a file out of band, forcing a fresh Read.

```
SESSION 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  ./: 9 bash (heuristic)
  /Users/testuser/.claude/plans: 2 write, 144 edit
  /Users/testuser/Projects/widget_app_prototype: 49 write, 500 edit
  /tmp: 122 write, 21 edit, 68 bash (heuristic)
```
JSON (leading `session_header` if `S>0`; row trio on EVERY row): **`--by timeline`** one object/mutation `{…trio…, path, op, ts_utc/local, turn_index, is_create, heuristic}` (`op` = UNDERSCORE `json_key` `write`/`edit`/`notebook_edit`/`multi_edit`/`bash` - vs `agents`' hyphenated form; there is NO `op=="create"`, use the SEPARATE `is_create` bool; `heuristic==true` only for bash). **`--by file`** one object/file `{…trio…, file, write, edit, bash, multi_edit, notebook_edit, total, distinct_files, first_utc/local, last_utc/local}`. **`--by dir`/`--by summary`** same per-op count keys + trio under a `dir`/`bucket` key. Then per-boundary `{type:"edit_before_read_boundary", path, line_no, turn_index, kind, ts_utc/local, …trio…}`; UNTAGGED summary `{distinct_files, total_mutations, edit_before_read_boundaries, skipped_lines, detail_level}`. Empty => `no file mutations found`.

## `recover` - reconstruct a file (or plan) from the transcript
```
csift recover @<uuid> --file <ABS_PATH|PATH-SUFFIX|@plan> [--salvage|--patches|--at WHEN|--coverage]
  [--out PATH] [--turn-range A..B|--since/--until] [--line-range A..B]
  [--no-subagents] [--files-from MANIFEST --out-dir DIR [--force]] [--format json]
```
Replays the file's Read/Write/Edit stream in transcript order into a sparse buffer (absent line = explicit gap, NEVER fabricated). `--file` REQUIRED, matched EXACTLY or as a component-aligned trailing SUFFIX (`app.py`/`src/app.py` both match `/abs/src/app.py`; `b.rs` != `/x/ab.rs`). Five **mutually-exclusive modes** (default = restore):
- **restore (no mode flag)** = RAW final bytes to stdout (no banner/line-numbers - clean to `> file`), OR `--out` writes the file. **HARD-FAILS (non-zero, never a holey file)** if the session saw only PART of the file - names covered+missing ranges + a reconcile recipe. JSON `{file, complete:true, lines, content}` (+`path, wrote` with `--out`), NO header/trailer.
- **`--salvage`** = never-fails best-effort FINAL-state fragment: known lines numbered, gaps `??? lines A..B unknown`. **BYTE-IDENTICAL to `--at @latest`**.
- **`--patches`** = segmented unified diffs between integrity boundaries, FULL context; no diff spans a boundary. JSON segment `{type:"segment", segment_index, line_no, line_no_start, line_no_end, turn_start, turn_end, ts_utc/local, pre_state_known, anchor_source('write'|'full-read'|'file-attachment'), unified_diff}` (`jq -r '.unified_diff // empty'`). THREE anti-fabrication anchor checks gate every hunk (any failure => hunk dropped `UnAnchorable`): (1) **neighbourhood-known** (`oldStart`±1 touches ≥1 known line); (2) **region-known** (every removed-range line known when `old_lines>0`); (3) **context-verification** (old-region lines EQUAL the buffer at the anchor). Net-delta total-length fix: post-splice seen-total adjusted by the edit's NET line delta (clamped ≥ max-known-line, ≥0).
- **`--at WHEN`** = point-in-time partial snapshot (line-numbered, gaps explicit). WHEN = ISO8601 | relative `2h` | `@turn:N` | `@line:N` | `@latest`. **`@turn:N` = the MAX jsonl `line_no` among events with `turn_index ≤ N`** (LAST line at-or-below turn N, INCLUSIVE); none => line 0 => EMPTY. **`@line:N` = a JSONL TRANSCRIPT line** (the `Lnnnnn` csift prints), NOT a `--file` line (for a FILE-line span use `--line-range`). `@latest` = no cutoff. JSON `{type:"snapshot", line_no, line_no_cutoff, lines:[{n,text,set_at_line}], gaps:[[a,b]], seen_total_lines}`.
- **`--coverage`** (alias `--dry-run`) = scoping only, no dump. JSON `{recoverable_lines, seen_total_lines, covered_ranges:[[a,b]], fragments, events:{read_full, read_windowed, edit, edit_unanchorable, write, bash, external_edit, history_snapshot, integrity_error}, boundaries:[…], file}` + row trio. Each `boundaries[]` = `{line_no, turn_index, ts_utc, ts_local, kind, confidence, detail}` - `confidence in {"authoritative","heuristic"}` (text `⚠`/`~`).
- **Integrity boundaries** (confidence order): (1) AUTHORITATIVE `modified since read` harness error (the HARD boundary, INVALIDATES the pre-boundary buffer - only content re-read/re-written AFTER survives); (2) AUTHORITATIVE Edit whose `originalFile` disagrees with the buffer at a ≥25% line mismatch; (3) AUTHORITATIVE external `edited_text_file` attachment; (4) HEURISTIC Bash mutation. **`File has not been read yet` is NOT a boundary** (the edit never landed).
- **Subagent/workflow recovery - input-side fallback (load-bearing)**: a SUBAGENT/workflow-agent records a BARE string with NO `toolUseResult` => its WRITTEN file would be invisible; `recover` closes the gap from the tool_use INPUT (`Write.content`, `Edit.{old_string,new_string,replace_all}` replayed old->new, `MultiEdit.edits[]`), DUAL-gated (skip ids that already have a `toolUseResult` + skip FAILED ops). So `recover @<subagent-hex>` works.
- **`--file @plan`** = bash-safe magic VALUE: resolves the session-bound plan (see §plan), reconstructs it under any mode + `--out`/`--format`. Rebuilds even a DELETED plan. ERRORS when no plan bound / target spans different plans.
- `--line-range A..B` (1-based) restricts the reconstructed FILE-line space. **BATCH** `--files-from MANIFEST` (one abs path/line; `#`comments + blanks ignored) requires `--out-dir`, exclusive with `--file`: ONE corpus scan (Aho-Corasick of basenames) reconstructs every file -> `<DIR>/<abs-without-leading-slash>` + a `recovery-report.tsv` (per-file `complete|partial|no-history|skipped-exists`; `--force` overwrites). Honors `--at`/`--since`/`--until`.

```
SESSION 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  file: /Users/testuser/.claude/plans/elegant-scribbling-dream.md
  recoverable: 1419/1419 lines (100%)  fragments: 1
  covered line ranges: [1..1419]
  events: 20 read (0 full, 20 windowed) · 144 edit · 2 write · 413 history-snapshot · 5 integrity-error
  integrity boundaries: (none)

mode=coverage  (reconstruction is partial - unknown lines are explicit, never fabricated)
```
The four non-restore modes emit NDJSON + leading `session_header` (if `S>0`), closed by the NESTED `{"summary":{sessions, file, mode, skipped_lines}}`. Restore is the lone single-object form (NO header/trailer).

```bash
csift recover @<uuid> --file /abs/gone.py --salvage          # survivors, gaps explicit
csift recover @<uuid> --file @plan --out /tmp/plan.md        # rebuild the bound plan (deleted ok)
```

## `plan` - locate the plan bound to a session
```
csift plan [PATH|@<uuid>] [--reverse PLAN_FILE] [--no-subagents] [--format json]
```
LOCATES (doesn't dump - use `recover --file @plan`) the AUTHORITATIVE binding: a `plan_mode` **attachment record** carrying `planFilePath`/`isSubAgent`/`planExists` - NOT a path heuristic (a session may freely Edit OTHER sessions' plans). Plans live flat under `~/.claude/plans/<three-word>.md` (subagent: `-agent-<hex>` suffix), name NOT derivable from id. No target => calling session (via `$CLAUDE_CODE_SESSION_ID`, never an all-projects scan); spans subagents (`--no-subagents` restricts).
- **`--reverse <plan.md>`** inverts: which session(s) bind this plan file (matched by absolute identity). Conflicts with a positional target; an empty result is honest, not an error.

```
session  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
plan     /Users/testuser/.claude/plans/elegant-scribbling-dream.md  [exists]
line     L31160
```
JSON per plan (NDJSON, header-free, NO trailer): `{plan_file, session_id, is_subagent, parent_session_id, plan_exists, line_no}`.

## `turns` - restore the verbatim turns a compaction summary clipped
```
csift turns [PATH|@<uuid>] [--budget N] [--budget-unit chars|tokens] [--round-trip-fraction F]
  [--agent-msgs longest|eot-only|rich|all] [--profile heavy|light] [--max-compactions N]
  [--agent-run-threshold N] [--agent-rich-min-chars N] [--agent-declaration-max-chars N]
  [--keep-first|--no-keep-first] [--include-subagents] [--turn-range A..B|--since/--until]
  [--slice N [--slices N] [--window N]] [--out PATH] [--format json]
```
A CC compaction summary keeps task STATE but loses TURN fidelity (clips ~22 user turns -> ~17 bullets; ~239 assistant turns -> 1 quote). `turns` SUPPLEMENTS it: re-emits verbatim user/assistant turns in order (each line `Lnnnnn`); does NOT re-derive task state. **Selection walks backward from EOF (recency-first), output sorts ascending**; transparent to compaction boundaries (a summary is a turn MEMBER, not a delimiter) => reaches across MANY by default (`--max-compactions N` caps crossings, default 0 = uncapped). Default TOP-LEVEL only; `--include-subagents` opts in (then `--budget` MULTIPLIES per session - the ONLY place that flag is meaningful; `--no-subagents` cancels it, last flag wins).
- **`--budget N` is PER SESSION** (default 40000 chars). `--budget-unit tokens` reads N as tokens (~4 chars/token) - GOTCHA: with the default `--budget` that is ~160000 chars; pass explicit `--budget` when flipping. JSON `budget_chars`/`max_total_chars` are ALWAYS chars (a token budget is pre-multiplied x4).
- **`--round-trip-fraction F`** (default 0.5, open (0,1)) = hard floor: Phase 1 spends `budget·F` only on round-trip-complete turns recency-first; Phase 2 fills the rest user-first. **TWO predicates:** Phase-1 FLOOR uses `is_human_round_trip` (GENUINE-human opener AND ≥1 agent msg - an automation pulse NEVER consumes the human lane); Phase-2 FILL uses `is_round_trip` (looser: ANY opener incl. machine pulse AND ≥1 agent msg).
- **`--agent-msgs`** (master switch; a turn can own a LONG agent run clipped to 1 quote):
  - **`longest` (the real clap DEFAULT)** - per-index keep over a multi-message turn: the LONGEST (by `full_chars`) ALWAYS kept; the FIRST iff `full_chars ≥ --agent-rich-min-chars`; each MIDDLE/LAST iff RICH; a single-message turn keeps its sole message. Tie: LAST max wins. (`--keep-first`/`--no-keep-first` are NO-OPS here - first retention is the length gate.)
  - **`eot-only`** - last message only per turn (pre-feature output). **`rich`** - last always + first by `--keep-first` + each non-droppable middle; only filters a LONG run (> `--agent-run-threshold`, default 6). **`all`** - every message.
  - **"RICH" test** (first-match-wins OR, KEEP-ON-DOUBT): length `≥ --agent-rich-min-chars` (default 280) OR a SIGNAL - number-of-substance, commit-hash-like hex, `file.rs:NNN`/`src/…` ref, backtick code span, or finding/decision lexeme. Only a SHORT (`< --agent-declaration-max-chars`, default 200) signal-less intent-verb opener (`let me…`) is droppable.
  - **Collapsed-run placeholder (`△`)**: a contiguous collapsed run renders as ONE `△ L{first}-L{last}  [X agent message(s), Y tool call(s)[, Z failed]]` (X=collapsed agent msgs, Y=`tool_use` blocks, Z=erroring tool_results) - Y shown even at 0, **Z clause OMITTED when 0**. **FETCH the bodies**: `csift search "" @<uuid> --no-subagents --line {first}-{last}` (`<hex>:` prefix in a subagent - see §search FETCH mode).
- `--keep-first` (default on; `rich` only). `--profile heavy` = 4/200/140; `light` = 8/360/240 (threshold/rich-min/declaration-max); applied BEFORE individual flags (explicit wins); does NOT change `--agent-msgs`.
- **Ellipsis (role-asymmetric)**: a unit over its role cap (`USER_CAP=600`, `ASST_CAP=900`) is middle-truncated keeping head+tail, marked `… [+K chars, L lines elided] …` (UTF-8-safe).
- **Dedup vs the live summary**: a live-region turn (`compactions_before==0`) whose 80-char prefix matches the summary's §6 user bullets / §9 assistant quote is flagged `(also in summary)` + DEMOTED (selected after non-dups), never dropped.
- **SLICING for hook injection** (the ≤10000-char `SessionStart` `additionalContext` cap - see hook recipe). `--slice N` (1-based) prints ONLY the Nth chunk of the DOCUMENT body (turn units + boundary banners, NO chrome), greedily packing lines into `≤ --window`-char chunks (default 10000, hard-splits an over-long line). DETERMINISTIC; `1..K` concatenated reproduces the body byte-for-byte. Out-of-range N => nothing (exit 0). TEXT-ONLY, exclusive with `--out`/`--format json`; `--slice 0`/`--window 0` error. **`--slices N` (FIXED-FLEET)** pins chunk COUNT to N hooks: fills N newest-first slices with WHOLE turns (per-role caps dropped; a turn middle-truncated only when it ALONE exceeds `window-200`), DISCARDS oldest overflow => count never drifts (budget = `N x --window`, `--budget` ignored). REQUIRES `--slice i` (both >0). Else `--slice` is legacy variable-count.

```
SESSION 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  budget 1500 chars · round-trip-fraction 0.50 · spanned 1 compaction boundaries
  selected 3 user (1 automation trigger: 1 agent) + 0 assistant units across 3 turns · 1425 / 1500 chars used
  dedup: 1 units also present in summary L55401 (demoted, flagged)
  ----------------------------------------------------------  (real output uses a box-drawing rule)
▽ L54630  USER  (2026-06-21 00:47:54 AEST (2026-06-20T14:47:54.636Z))
continue - resuming after a short break
══ compaction boundary · summary at L55401 · (turns below predate it) ══
▽ L55793  USER  (2026-06-21 10:28:43 AEST (2026-06-21T00:28:43.263Z))
[agent b1e9d3c75a2f08e64 completed] Agent "Check the empty-state rendering path" completed
```
Text: `▽ Lnnnnn USER` / `[N tool calls]` / `△ Lnnnnn ASSISTANT` (one `△` per kept agent message), `══ compaction boundary … ══` banners, `(also in summary)` flags, collapsed `△ L{a}-L{b}` placeholders. The top `scope` line prints ONLY when >1 session in scope OR a session was budget-skipped - read scope from JSON instead. `--out` writes the full doc (byte-identical to stdout - turns does NOT line-truncate stdout). JSON: a SUPERSET `session_header` `{kind:"session_header", sessions_in_scope, sessions_rendered, top_level_sessions, subagent_sessions, budget_chars, budget_is_per_session, max_total_chars, selected_user, automation_triggers, automation_by_kind:{…5…}, automation_in_scope_by_kind:{…5…}}` (`*_by_kind` = SELECTED vs EVERY-in-scope per-class breakdown - a monitor-heavy session reads `monitor:0` selected yet nonzero in-scope). Then one object/unit `{…trio…, turn_index, line_no, role, ts_utc/local, tool_calls, full_chars, rendered_chars, truncated, elided_chars, elided_lines, also_in_summary, compactions_before, text, is_automation}` (an automation USER unit adds `{trigger_kind, task_id, status, event}`; `event` = Monitor/ScheduleWakeup tag, null otherwise), interleaved `{kind:"compaction_boundary",…}`/`{kind:"collapsed_agents",…}`, CLOSED by `{kind:"skipped_lines", skipped_lines:N}` (the ONLY kind-tagged terminator, always closes).

## `image` - list + extract the images a session carries
```
csift image [PATH|@<uuid>] [--id '#N'|L<line>i<n>].. [--out DIR|FILE.ext]
  [--since][--until][--turn-range A..B][--uuid PREFIX] [--no-subagents] [--format json]
```
Pasted/screenshot images live INLINE as `{type:"image",source:{type:"base64",…}}` blocks. Default = LIST (content-deduped via a `<len>:<head>:<tail>` fingerprint - a re-injected image shows once); `--out <DIR>` => EXTRACT.
- **`#N` handle** = the SAME `[Image #N]` number the model sees (assigned by zipping a record's markers with its image blocks ONLY when counts match; a count-mismatch leaves `seq=None` so a `#N` may resolve to nothing - the image keeps its `L<line>i<n>`). **NOT unique** - CC reuses low N. **`--id #N` naming >1 DISTINCT image => AMBIGUOUS, HARD ERRORS** with the occurrence list (`t<turn>`/`L<line>i<n>`/uuid/time/excerpt). Distinct = by fingerprint (0=>unresolved, 1=>pick, >1=>ambiguous). Disambiguate via the locator or PRE-narrow with `--since`/`--until`/`--turn-range`/`--uuid PREFIX`. Bare `32` == `#32`.
- **`L<line>i<n>` locator** (always unambiguous) = 1-based jsonl line + 1-based ordinal of the image among its blocks (direct OR nested in a `tool_result` array). `--id` is per-transcript => needs a SINGLE transcript.
- **`--out` extension drives format** (`convert in out.jpg` idiom): a DIRECTORY (or any path with no `png`/`jpg`/`jpeg`/`gif`/`webp` ext) writes each image SOURCE-format, auto-named `<session-short>[-img<N>]-L<line>i<n>.<ext>`; a FILE path with an image ext writes the SINGLE selected image, CONVERTING if it differs (->png lossless, ->jpeg/webp q90, ->gif Floyd-Steinberg ≤256-color). >1 image + a file path => error. Animated GIF -> FIRST frame + warning. A `url`-source image (no inline bytes) is reported with its URL.

```
#1    L9797i1   image/png ~356 KB  2026-06-11 14:22:34 AEST (2026-06-11T04:22:34.566Z)
#2    L6812i1   image/png ~440 KB  2026-06-11 10:27:10 AEST (2026-06-11T00:27:10.447Z)
#3    L6812i2   image/png ~252 KB  2026-06-11 10:27:10 AEST (2026-06-11T00:27:10.447Z)
```
JSON list per image (header-free): `{handle, seq, id(=L<line>i<n>), line_no, img_index, …trio…, source_kind, media_type, b64_len, est_bytes, url, record_uuid, ts_utc/local}` + UNTAGGED summary `{images, transcripts, skipped_lines}`. Extract JSON: `{path, bytes, media_type, source_media_type, converted, notes}`. `turns`/`search` surface these ids inline (`[1 image: #265]`).

```bash
csift image @<uuid>                                                   # list (deduped)
csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg   # locator -> convert to jpeg
```

---

## Recipes - shell pipe + jq

```bash
# Every abs path a session touched (authoritative only, drop Bash heuristics):
csift files @<uuid> --by file --format json | jq -r 'select(.file and (.heuristic|not))|.path // .file'
# WHICH sessions matched a pattern (there is no -l):
csift search "panic" --format json | jq -r 'select(.kind!="session_header")|.session_id' | sort -u
```

### SessionStart(compact) hook - install turns as a post-compaction verbatim supplement (the #1 recipe)
N `SessionStart(matcher:"compact")` hooks run `turns --slices N --slice i` (slice mechanics in §turns) to re-inject the recent verbatim User<->Agent turns - a SUPPLEMENT to the summary, orthogonal + window-extending. The text lands as a **`type:"attachment"`** record (csift ignores `attachment` => never a turn). Safe to re-fire each compaction: the old copy is summarized away, re-injected fresh = one (no pile-up); resume won't match.

**The slice race is load-bearing.** CC runs same-event hooks CONCURRENTLY, collecting output in COMPLETION (not registration) order, so they arrive scrambled - a turn split across a chunk glues back wrong. A `$PPID`-namespaced **done-flag barrier** forces order: slice i waits for slice i-1's `.done`. ONE script, registered N times with a different slice arg:

```bash
#!/usr/bin/env bash
set -euo pipefail
slice="${1:?1-based slice index}"; N=4; WINDOW=9000 # N MUST equal the number of registered hooks
SEQ="/tmp/csift-turns-slice-$PPID" # $PPID = the CC process; isolates concurrent sessions
release(){ mkdir -p "$SEQ"; touch "$SEQ/s$slice.done"; [ "$slice" = "$N" ] && rm -rf "$SEQ"; return 0; }
trap release EXIT # release on EVERY exit path so the chain can't stall
if [ "$slice" -le 1 ]; then rm -rf "$SEQ"; mkdir -p "$SEQ" # slice 1 resets stale flags from a prior PID collision
else u=$(($(date +%s)+5)); until [ -f "$SEQ/s$((slice-1)).done" ] || [ "$(date +%s)" -ge "$u" ]; do sleep 0.05; done; fi
command -v jq >/dev/null || exit 0 # ANY failure path injects nothing + exits 0 (never block start)
in=$(cat); [ "$(jq -r '.source//empty' <<<"$in")" = compact ] || exit 0 # backstop the matcher
C=$(command -v csift||true); [ -x "$C" ] || C=$HOME/.cargo/bin/csift; [ -x "$C" ] || exit 0 # resolve csift; hooks get no PATH
sid=$(jq -r '.session_id//empty' <<<"$in"); [ -n "$sid" ] || exit 0 # id from stdin, NOT $CLAUDE_CODE_SESSION_ID
chunk=$("$C" turns "@$sid" --slices $N --window $WINDOW --slice "$slice" 2>/dev/null||true)
[ -n "$chunk" ] || exit 0 # out-of-range slice prints nothing -> self-trims
jq -n --arg c "Verbatim turns the compaction summary clipped (part $slice - a supplement; the summary still owns task state):
$chunk" '{hookSpecificOutput:{hookEventName:"SessionStart",additionalContext:$c}}'
```
Register N times in settings.json: each `{"matcher":"compact","hooks":[{"type":"command","command":"/ABS/csift-turns-slice.sh i"}]}`, i=1..N. ABSOLUTE path for a global `~/.claude/` install (`$CLAUDE_PROJECT_DIR` is unset there). `--window 9000` stays under the 10K cap.

## Practical tips
- `search --format json | jq .session_id` (which sessions) before a full `search` (what); `recover --coverage` before trusting a `--salvage`.
- A subagent's bare-hex `session_id` is NOT re-feedable as `@<uuid>` - always re-feed `parent_session_id` (§JSON conventions).
- Sub-second on 200MB+ files (mmap + SIMD + prefilter + rayon); never fear an unscoped scan.
