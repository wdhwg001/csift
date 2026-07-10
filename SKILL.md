# csift — ripgrep for Claude Code session transcripts

Rust CLI over CC session `.jsonl` under `~/.claude/projects/<encoded-cwd>/`. Built FOR an LLM consumer: token-lean text, uniform JSON, pure regex (RE2-class, linear-time; NO backrefs/lookaround — they fail to compile by design). Smart-case: pattern is case-insensitive unless it has an uppercase; `-i` forces insensitive. `csift <cmd> --help` is the authoritative flag manual.

## Verb map — one intent ⇒ one command

| intent | command |
|---|---|
| FIND text (regex, round-trip exchanges) | `search PATTERN [target…]` |
| FETCH exact record(s) full / raw bytes | `show TARGET --line N\|A-B / --uuid U [--raw]` |
| which session is this / who am I | `list` / `whoami` |
| AGGREGATE: tokens·tools·turns·span | `stats [target…]` |
| what files changed, when | `files [target…]` |
| REBUILD a file (or deleted plan) from history | `recover TARGET --file P` |
| restore turns a compaction clipped | `turns [target…]` |
| subagent tree: lifecycle·status·frozen lanes | `agents [target]` |
| locate a session's bound plan file | `plan [target]` |
| list/extract pasted images | `image [target]` |

## Targeting (positional, every command; `whoami` optional)

`@<uuid>` one session · `@<uuid-prefix>` (4-11 hex, unique else error) · `@main` calling top-level (env) · `@trap:<marker>` calling SUBAGENT (§trap) · `@<agent-id>` a subagent + its subtree (ids from `agents`; bare hex ≥12 OR teammate form `aVSRepro-68a2…`) · `.`/real path/`-Users-…` encoded dir ⇒ project(s) · `*.jsonl` one transcript · 0 targets ⇒ ALL projects (fast — never fear it).
- A BARE id without `@` errors with "did you mean '@…'?" — ids always take `@`.
- `search` is the ONE command whose first positional is the PATTERN, targets after: `csift search P @<uuid>`. A pattern starting `@` errors (escape as `\@…` to match literally); a uuid-shaped pattern prints a stderr note (it searches the uuid AS TEXT).
- `show` targets exactly ONE transcript: `@<uuid>` = that top-level file (never spans), `@<agent-id>` = that subagent's file.

## Four laws (all commands)

1. **Exit**: address-miss (a NAMED line/uuid/id/file that doesn't resolve) = hard error, stderr `csift: error: …`, exit≠0. Filter-empty (a search/window matching nothing) = honest empty, exit 0. `turns --slice` out-of-range prints nothing, exit 0 (hook affordance).
2. **Line domains**: `line`/`Lnnnn` = TRANSCRIPT jsonl physical line, everywhere. `--file-lines` (recover) = the reconstructed FILE's lines. `turn`/`t<n>` = turn index (0-based; turn k = k-th genuine turn; the synthetic pre-first-user lead folds into turn 0). Read turn indices from output (`s1·t270` ⇒ `--turn-range 270..270`) — don't compute them.
3. **Span**: subagents included by default; `--no-subagents` restricts; `turns` is the one opt-IN (`--subagents`; budget multiplies per session). Both switches exist everywhere (contradictory pair = parse error); `agents` rejects both (it LISTS subagents).
4. **No silent truncation**: every cap reports its drop; malformed lines are counted (`N malformed line(s) skipped`), never hidden.

## JSON — envelope v2, ONE shape for every command

`--format json` always emits exactly: `{"kind":"header","command":…, …}` first line (span commands add `sessions_in_scope/top_level_sessions/subagent_sessions`) → kind-tagged rows → `{"kind":"summary", …}` last line (even all-zero). Universal idiom: `jq 'select(.kind=="<row>")'`; summary = `tail -1 | jq`.

Row kinds: list→`session` · search→`exchange` · show→`record` · stats→`session` · files→`mutation|file|dir|bucket|boundary` · agents→`session` · turns→`turn|compaction_boundary|collapsed_agents` · plan→`plan` · image→`image|extract` · whoami→`identity` · recover→`coverage|segment|snapshot|restore|boundary`.

Shared row fields: **id trio** `session_id` (display; top-level uuid OR subagent bare hex) + `is_subagent` + `parent_session_id` (ALWAYS re-feedable as `@…`; = session_id on top-level rows). Re-feed `@<session_id>` works for BOTH forms (agent ids round-trip); across surfaces prefer `parent_session_id` for the owning session. `ts_utc`+`ts_local` travel as a pair. `kind` is the envelope's word; a transcript's SHAPE is `shape` (agents), a boundary's WHAT-changed is `cause`, an automation pulse's class is its `trigger`.

## Labels (`-t/--label`) — dotted `role.class.sub`, 3 roles, 25 leaves

Selector = dot-SEGMENT prefix: `-t agent` (role) · `-t agent.tool` (use+result) · full leaf = just it. No `-t` ⇒ all. Multi-label records emit ONCE under the richest view (JSON `label` = matched leaf, `labels[]` = full set).

```
user     .message   genuine human prose (incl. slash <command-args>)
         .answer    AskUserQuestion answer (Q+options+answer unit)
         .rejection plan/tool reject + typed instruction
agent    .message · .thinking (redacted → "[redacted thinking]") · .tool.use · .tool.result
         .communication.{inbox,sent,signal}   peer msgs — render `from ⇨ to` (self = owner)
harness  .notification.{workflow,monitor,subagent,background-command,task}  ← <task-notification> pulses
         .compaction.{summary,boundary}   boundary renders its compactMetadata (trigger=… preTokens=…)
         .command.{invocation,stdout} · .interrupt.{user,tool}
         .schedule.{wakeup,continuation} · .meta.{hook,loop}
```
Glyphs: `◂` user · `▸` agent · `⚙` harness · `·` sibling · `▹` tool use↔result pair (unreturned → `(no result — pending)`; orphan → `(use not in scope)`) · `⇨` comm direction.
- Inbound `<teammate-message>`/`<agent-message>` = a PEER ⇒ `agent.communication.inbox` (not user) — still opens a turn. `<task-notification>` ⇒ `harness.notification.<trigger>`, rendered `[<trigger> <task-id> <status>] <summary>` (never raw XML); trigger from the summary's lead (`monitor` also rescued from a quoted command NAME containing monitor/liveness/re-arm tokens); a pulse carrying `<result>` is ALSO `…inbox` (child ⇨ self).
- Turn boundary = genuine user ∨ AUQ answer ∨ typed rejection ∨ inbound peer msg — shared by search/turns/files/recover.

---

## search — find round-trips

```
csift search PATTERN [target…] [-t SEL]… [-i] [--multiline] [--since W] [--until W]
  [--turn-range A..B] [--max-count N] [-c] [--siblings] [--no-truncate]
  [--resolve-persisted] [--no-subagents] [--format json]
```
Empty `""` pattern = pure filter (combine `-t`/time/turn-range). A hit returns the COMPLETE round-trip (tool_use with its result; user turn with reply; answered AUQ as one Q+A unit).
- Excerpts: ~400 chars CENTERED ON THE MATCH — a fragment, NOT a summary; never conclude from a clipped head. When anything clipped, text prints a caution + JSON summary `excerpts_truncated:true` → refetch via `--no-truncate` or `csift show`.
- `--siblings` (zero-arg): also render the turn's other records — messages always, thinking≤2 · tool.use≤3 · tool.result≤3 · harness≤2 per leaf; overflow prints `(+N more · csift show @<id> --line A-B)` (run it verbatim). JSON adds `siblings[]`, `siblings_hidden`, `turn_lines`.
- `-c`: just the integer total (adds back `--max-count` drops). "WHICH sessions matched" = JSON summary `session_ids` (sorted, ≤100 + `session_ids_truncated`): `--format json | tail -1 | jq .session_ids`.
- `--resolve-persisted`: inline `tool-results/<id>.txt` persisted-output files before matching (regex reaches externalized output).
- `--turn-range` ∧ `--since/--until` intersect (AND).
- Exchange row: `{kind:"exchange", …trio…, turn_index, ts_utc/local, hits:[{label,labels,line,uuid,ts_utc/local,tool_name,from,to,pairing,tool_use_id,source,excerpt,image_ids}], record_uuids}`; summary `{matched,sessions,session_ids,…,dropped_by_cap,skipped_lines,with_elicitation_sidecar,excerpts_truncated}`.

```bash
csift search "panic" @<uuid> -t agent --since 6h
csift search "" @<uuid> -t user.rejection            # every typed rejection
csift search "todo" . -c                             # count in this project
```

## show — fetch records (the reader; also the RAW escape hatch)

```
csift show TARGET (--line N|A-B,…)…|(--uuid U,…)… [--raw] [--format json]
```
- TARGET = ONE transcript (see Targeting). No selector ⇒ error (csift never dumps a whole transcript into context; the error names the file path).
- Default render: FULL records through the same pipeline as search hits (labels, plan pointers, pairing, sidecar merge). Rendered "records" are role:user/assistant message lines; a metadata/attachment line is NOT renderable — the miss error points at `--raw`.
- `--raw`: the VERBATIM jsonl line bytes — for fields csift doesn't render (usage, stop_reason, model, any new field) and for torn/malformed lines. Excludes `--format json` (raw IS json); reads the file only (no sidecar).
- Misses: explicit line/uuid → error; range clamps to EOF but errors on zero yield.
- Row: `{kind:"record", …trio…, turn_index, line(null=sidecar), uuid, label, labels, tool_name, from, to, pairing, tool_use_id, source, ts_utc/local, text, image_ids}`.

```bash
csift show @<uuid> --line 46550             # the record search cited as L46550
csift show @<agent-id> --line 88,495-500    # from a subagent transcript
csift show @<uuid> --line 46550 --raw       # exact bytes (all fields)
```

## stats — aggregates (tokens · tools · turns · span)

```
csift stats [target…] [--since W] [--until W] [--no-subagents] [--format json]
```
Per session: lines, user/assistant record counts, turns, compactions, first→last span + duration, tokens per model (`input/output/cache_read/cache_creation` from message.usage), tool calls by name. Scope TOTAL block when >1 session. Row kind `session`; summary carries scope totals (`tail -1 | jq .tokens`).

## list — which session is this?

```
csift list [target…] [--no-subagents] [--format json]
```
Head+tail read only (fast at any size). Per session: cwd (+branch, CC version), first ◂ / last ◂ / last ▸ excerpts (200 chars) + timestamps; subagent rows branded `SUBAGENT <hex> · parent SESSION <uuid>`; pending elicitations annotate the row (`with elicitation sidecar`). Row: trio + `path,cwd,version,git_branch,first_user,last_user,last_agent` (`{excerpt,ts_utc,ts_local}`|null) + `skipped_lines,pending_elicitations,with_elicitation_sidecar`.

## whoami — identify the caller (false-positive-safe)

```
csift whoami [@main|@trap:<marker>] [--format json]
```
Reads `$CLAUDE_CODE_SESSION_ID` (alias `$CODEX_COMPANION_SESSION_ID`). Neither set ⇒ ERRORS with guidance — never guesses by mtime. Prints session + path. Rows kind `identity` `{…trio…, depth, path}`; `@trap` returns the full UPSTREAM chain (self depth 0 → top-level root).
- SUBAGENT caveat: env var = own id in a built-in Task subagent but the PARENT's id in a workflow `agent()` subagent — you can't tell which from env. `@trap` solves it env-independently.

### @trap:<marker> — "which subagent am I?"
Invent a fresh marker, put it literally IN the csift command; csift finds the transcript whose Bash `tool_use` input carries it (input-side ⇒ no echo false-positive; flushed pre-run ⇒ resolves first try). Grammar (enforced): ASCII ≥13 bytes, EXACTLY 3 CamelCase words (1 upper + ≥2 lower each; no ALLCAPS) + EXACTLY 4 trailing digits, non-trivial (no constant step: 0000/1234/9876/2468 rejected). NEVER script-generate/reuse/copy the doc example (`@trap:JollyShinyBrook4283` is hard-rejected). From MAIN just use `@main`. 0 matches ⇒ rerun; >1 ⇒ ambiguity error.

## files — what changed, when

```
csift files [target…] [--by summary|dir|file|timeline] [--regex RE] [--glob PAT]
  [--turn-range A..B] [--since W] [--until W] [--no-subagents] [--format json]
```
Authoritative for Edit/Write/MultiEdit/NotebookEdit (create-vs-edit from the paired toolUseResult); Bash mutations HEURISTIC (lexical rm/mv/cp/mkdir/touch/tee/sed -i/git/redirect; relative paths verbatim; always `(heuristic)`); failed ops excluded.
- `--by`: summary=top-prefix rollup (default) · dir · file · timeline (one line/mutation, heavy). `--regex` (full abs path, case-exact) ∧ `--glob` (`**` crosses `/`) filter BEFORE rollup.
- Edit-before-Read boundaries section always follows: an out-of-band change (formatter/git/editor) that forced a re-Read — the "risky to recover" discovery signal (then `recover --coverage`).
- Rows: timeline `{kind:"mutation", …trio…, path, op(write|edit|notebook_edit|multi_edit|bash), turn_index, line, is_create, heuristic, ts_utc/local}`; grouped `{kind:"file"|"dir"|"bucket", …trio…, path, write, edit, bash, multi_edit, notebook_edit, total, distinct_files, first/last_utc/local}`; boundaries `{kind:"boundary", …trio…, path, line, turn_index, cause, ts_utc/local}`; summary `{distinct_files,total_mutations,edit_before_read_boundaries,skipped_lines,detail_level}`.

```bash
csift files @<uuid> --by file --format json | jq -r 'select(.kind=="file" and (.heuristic|not)).path'
csift files --glob '**/foo.rs' --by file      # which sessions touched foo.rs (all projects)
```

## recover — rebuild a file (or a deleted plan) from the transcript

```
csift recover TARGET --file <ABS|SUFFIX|@plan> [--salvage|--patches|--at WHEN|--coverage]
  [--out PATH] [--file-lines A..B] [--turn-range A..B] [--since/--until]
  [--files-from MANIFEST --out-dir DIR [--force]] [--no-subagents] [--format json]
```
Replays the file's Read/Write/Edit stream into a sparse buffer — absent lines are explicit gaps, NEVER fabricated. `--file` matches exact or component-aligned trailing suffix (`app.py`≡`src/app.py`; `b.rs`≠`ab.rs`). Five exclusive modes:
- **restore** (default): raw final bytes to stdout/`--out`; HARD-FAILS if partial (names covered+missing ranges + recipe) — never a holey file.
- **--salvage**: never-fails final-state fragment; gaps `??? lines A..B unknown` (≡ `--at @latest`).
- **--patches**: unified diffs segmented at integrity boundaries; 3 anti-fabrication anchor checks gate every hunk (neighbourhood-known, region-known, context-verified; failures drop the hunk as UnAnchorable). `jq -r 'select(.kind=="segment").unified_diff'`.
- **--at WHEN**: point-in-time snapshot. WHEN = ISO | `2h` | `@turn:N` (last line with turn_index ≤ N) | `@line:N` (a TRANSCRIPT line — the Lnnnn csift prints) | `@latest`.
- **--coverage** (`--dry-run`): scoping only — `{kind:"coverage", recoverable_lines, seen_total_lines, covered_ranges, fragments, events{...}, boundaries:[{line,turn_index,cause,confidence,detail,ts}]}`. Run before trusting a salvage.
- Integrity boundaries (confidence `authoritative`|`heuristic`, text `⚠`/`~`): modified-since-read error (hard: invalidates prior buffer) > mismatching Edit originalFile > external `edited_text_file` attachment > heuristic Bash mutation. "File has not been read yet" is NOT a boundary.
- Subagent WRITE gap closed from tool_use INPUT (Write.content, Edit old→new, MultiEdit) — `recover @<agent-id>` works.
- `--file @plan` resolves the session-BOUND plan (rebuilds even a deleted one; errors if no binding / targets disagree). Batch: `--files-from` manifest (+`--out-dir`), one corpus scan, per-file report tsv.

```bash
csift recover @<uuid> --file /abs/gone.py --salvage
csift recover @<uuid> --file @plan --out /tmp/plan.md
```

## plan — locate the bound plan file

```
csift plan [target] [--reverse PLAN.md] [--no-subagents] [--format json]
```
The AUTHORITATIVE binding is the `plan_mode` attachment (`planFilePath`) — NOT path heuristics (sessions edit each other's plans). No target ⇒ calling session (env). `--reverse <file>` inverts: which session(s) bind this plan (empty = honest, exit 0). Locates only — dump via `recover --file @plan`. Row: `{kind:"plan", plan_file, …trio…, plan_exists, line}`.

## turns — restore what compaction clipped

```
csift turns [target…] [--budget N] [--round-trip-fraction F] [--agent-msgs longest|eot-only|rich|all]
  [--profile heavy|light] [--max-compactions N] [--subagents] [--turn-range A..B] [--since/--until]
  [--slice N [--slices N] [--window N]] [--out PATH] [--format json]
```
A compaction summary keeps task STATE but loses TURN fidelity; `turns` re-emits verbatim user/assistant turns (each `Lnnnnn`), selection backward from EOF (recency-first), output ascending, transparent across MANY boundaries (`--max-compactions` caps crossings; 0 = uncapped).
- `--budget N` = CHARS, PER session (default 40000; ≈4 chars/token as sizing rule). `--round-trip-fraction F` (default .5): floor reserved for HUMAN round-trips (machine pulses never consume the human lane).
- `--agent-msgs`: `longest` (default — keep each turn's longest + substantive first + rich middles) · `eot-only` (last only) · `rich` · `all`. `--profile heavy|light` is the whole tuning surface (thresholds 4/200/140 vs 8/360/240; default 6/280/200).
- Collapsed runs render `△ L{a}-L{b} [X agent message(s), Y tool call(s)[, Z failed]]` → fetch via `csift show @<id> --line a-b`. Units over the role cap (user 600 / assistant 900 chars) middle-truncate `… [+K chars, L lines elided] …` (JSON `text` is always FULL). A turn already quoted by the newest summary is flagged `(also in summary)` + demoted (never dropped).
- Rows: `{kind:"turn", …trio…, turn_index, line(null=sidecar), source, role, ts, tool_calls, full_chars, rendered_chars, truncated, elided_*, also_in_summary, compactions_before, text, is_automation(+trigger_kind, task_id, status, event)}` · `{kind:"compaction_boundary", line, summary_chars}` · `{kind:"collapsed_agents", …, first_line, last_line}`; header adds budget/automation split (`automation_by_kind` selected vs `automation_in_scope_by_kind`).
- SLICING (hook injection, ≤10000-char cap): `--slices N --slice i --window W` = fixed-fleet N chunks, whole turns, oldest discarded; `--slice i` alone = legacy variable chunks (1..K concat == body byte-exact). Out-of-range ⇒ nothing, exit 0. Text-only.

## agents — subagent lifecycle + topology

```
csift agents [target] [--agent ID] [--shape builtin-task|workflow|teammate]…
  [--since/--until] [--order-by trigger|start|completion] [--with-files] [--returned-message] [--format json]
```
Always a parent→child TREE (workflow runs as parents; nesting = logical, from spawn tool_use links; disk is flat). **shape** = on-disk location: `builtin-task` `subagents/agent-<hex>.jsonl` · `workflow` `…/workflows/wf_<id>/…` (journal.jsonl = event log, not a transcript) · `teammate` = built-in location + meta `taskKind:"in_process_teammate"` (name-embedded id, agentType overloaded with the handle; csift recovers the REAL type + spawn via name-join; node adds `name`/`team_name`).
- `--order-by` sets tree sort AND the --since/--until axis: `trigger` (default; parent tool_use ts = true spawn) · `start` (child head, lags 0.2-4.7s) · `completion`.
- `--agent ID` = one node (implies returned-message; bypasses filters; miss = error). `--returned-message` 3-way source: sync tool_result / async child tail / workflow journal.
- **Frozen lane**: newest record an UNRETURNED tool_use ⇒ `status:"running"` (never completed) + `pending_classification`: `escalation-blocked` (a dangerous-rm Bash CC hoists for approval — waiting for a human Yes, the ONE positively confirmable state) | `awaiting-execution` (slow OR wedged — jsonl can't distinguish; weigh `pending_since_utc`). Fields: `pending_tool_use_id/tool_name/classification/since_*`.
- **Teammate control** (csift is read-only — this is a pointer): steer/terminate via `SendMessage` BY NAME (`message:{type:"shutdown_request"}`), NOT TaskStop (only knows run_in_background task_ids — rejects every teammate id form), NOT pkill (in-process, shares orchestrator PID). Text footer + node `control_hint`.
- Node: `{agent_id, agent_type, name, team_name, shape, status, parent_session_id, parent_agent_id, workflow_id, depth, description, spawn_tool(_use_id), trigger/started/completed_utc/local, duration, pending_*, skipped_lines, children[], +returned_message(_source), +files_changed[{path,op,is_create}]}`. Run: `{run_id, task_id, workflow_name, status, agent_count, duration_ms, total_tokens, total_tool_calls, default_model, started_*, children}` (all snake_case). `agent_id` re-feeds as `@<agent_id>`; the owning session is `parent_session_id`.

## image — pasted images

```
csift image [target] [--id N|L<line>i<n>]… [--out DIR|FILE.ext]
  [--since/--until] [--turn-range] [--uuid PREFIX] [--no-subagents] [--format json]
```
Default LIST (content-deduped by fingerprint; re-injections show once). `--id` input = BARE digits (the `[Image #N]` number the model saw; display shows `#N`, input drops the `#` — shell would eat it) or the always-unique locator `L<line>i<n>`; `#N` naming >1 DISTINCT image hard-errors with the occurrence list (disambiguate by locator or pre-narrow via time/turn/uuid). `--out`: dir ⇒ source-format files auto-named; `file.ext` ⇒ single image, converted by extension (png lossless · jpeg/webp q90 · gif dithered; animated gif → first frame + warning); url-source images reported, never fabricated. Row: `{kind:"image", handle, seq, id, line, img_index, …trio…, source_kind, media_type, b64_len, est_bytes, url, record_uuid, ts}`; extract `{kind:"extract", path, bytes, media_type, source_media_type, converted, notes}`. search/turns cite ids inline (`[1 image: #265]`).

```bash
csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg
```

---

## Elicitation sidecar — pending AUQ / ExitPlanMode / MCP (transparent merge)

While PENDING these three leave NO live trace in native jsonl (whole-turn buffered / in-memory) — a session blocked on a human looks stalled. A CC hook (recipe below) appends native-shaped markers to `<uuid>/elicitations.jsonl` (a SIDECAR beside `subagents/`, never the transcript). csift then merges UNRESOLVED pendings automatically wherever it reads a session: they classify `agent.tool.use` (searchable by text or `-t agent.tool.use`), turns appends each as its own newest turn unit, list annotates rows. Once answered, CC writes the real record and the pending pairs off — no duplicates. A merged record has no physical line: renders `(elicitation sidecar)`, JSON `source:"elicitation-sidecar"` + null `line`; surfaces print `with elicitation sidecar` / `with_elicitation_sidecar:true`. Sidecar is keyed by the TOP-LEVEL session and cannot be targeted directly (error). `show --uuid` can fetch a pending marker.

## Time + misc conventions

- WHEN grammar (`--since/--until/--at`): relative `45s 90m 2h 3d 1w` = that long ago (system-local) or ISO8601 (bare date ⇒ local midnight). Bounds are ≤-inclusive; records without timestamps never match a bounded window.
- `--claude-home DIR` (global, any position) repoints `~/.claude`; precedence flag > `$CLAUDE_CONFIG_DIR` > `$HOME/.claude`.
- Flag order is free (a pre-pass reorders flags around leading-dash encoded-dir positionals).
- Timestamps render local (+offset); JSON carries `ts_utc`+`ts_local`.
- Path filters (`files --regex/--glob`) are case-exact (paths); search PATTERN is smart-case (text).
- **Retention**: CC deletes transcripts older than `cleanupPeriodDays` (default 30!). Check `jq '.cleanupPeriodDays // 30' ~/.claude/settings.json`; recommend 180/365 — csift can only read what survives.

## Recipes

```bash
# which sessions mention X (no grep -l needed):
csift search "X" --format json | tail -1 | jq .session_ids
# every abs path a session touched (authoritative only):
csift files @U --by file --format json | jq -r 'select(.kind=="file" and (.heuristic|not)).path'
# scope a search to sessions found by a previous query:
csift search P $(… | jq -r '.session_ids[]' | sed 's/^/@/' | tr '\n' ' ')
# token burn of the current session:
csift stats @main --no-subagents --format json | tail -1 | jq .tokens
# read what search found:  search prints L<n>  →  csift show @U --line <n>
```

### Hook 1 — SessionStart(compact): re-inject verbatim turns after compaction (#1 recipe)
N hooks run `turns --slices N --slice i` to supplement the summary with the recent verbatim turns (lands as an `attachment` record — csift ignores it, no feedback loop; safe to re-fire, old copies get summarized away). **Load-bearing race fix**: CC runs same-event hooks CONCURRENTLY and concatenates in completion order — a `$PPID`-namespaced done-flag barrier forces slice order. ONE script registered N times:

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
chunk=$("$C" turns "@$sid" --slices $N --window $WINDOW --slice "$slice" 2>/dev/null||true)
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
Fires when AUQ/ExitPlanMode/MCP elicitation OPENS and CLOSES; appends pending/resolved markers to the sidecar. MUST print nothing (observe only). Verified live: the pending marker lands the instant the picker appears.

```bash
#!/usr/bin/env bash
set -uo pipefail
in=$(cat 2>/dev/null) || exit 0
command -v jq >/dev/null 2>&1 || exit 0
ev=$(jq -r '.hook_event_name//empty' <<<"$in" 2>/dev/null); tool=$(jq -r '.tool_name//empty' <<<"$in" 2>/dev/null)
kind=""; phase=""
case "$ev" in
  PreToolUse)  case "$tool" in AskUserQuestion|ExitPlanMode) kind="$tool"; phase="pending";; esac ;;
  PostToolUse) case "$tool" in AskUserQuestion|ExitPlanMode) kind="$tool"; phase="resolved";; esac ;;
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
Register 4 events (one script, absolute path): `PreToolUse`+`PostToolUse` matcher `"AskUserQuestion|ExitPlanMode"`; `Elicitation`+`ElicitationResult` (no matcher).
