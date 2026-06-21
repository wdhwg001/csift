---
name: csift
description: Search, recover, and audit Claude Code (CC) session + subagent transcripts - the `.jsonl` logs under ~/.claude/projects. Reach for csift for ANY task touching a PAST or the CURRENT CC session; regex-search what was said/done across sessions; list sessions to answer "which session is this"; recover a file's content (or a DELETED plan) from the transcript's Read/Write/Edit stream; restore the verbatim user/assistant turns a context-compaction summary clipped (standing directives, an earlier decision); see which files/dirs a session changed and when; inspect a session's subagents (built-in Task + workflow/OMC agents) - lifecycle, status, topology; locate the plan bound to a session; identify the calling session; fetch one exact message in full; extract pasted images. Nine subcommands - list search agents whoami files recover plan turns image. Pure regex; no embeddings/BM25/semantic.
user-invocable: true
---

# csift - ripgrep for Claude Code session transcripts

Rust CLI over CC session `.jsonl` under `~/.claude/projects/<encoded-cwd>/`. LLM-facing: clean token-efficient regex text; `--format json` for machines. **Pure regex - no embeddings/BM25/semantic.** Regex = Rust `regex` 1.12 (RE2-class, linear-time): classes, alternation, groups, quantifiers (+lazy `*?`), anchors, inline flags `(?i)(?m)(?s)(?x)`. NOT supported (fail-to-compile): backrefs, lookaround, atomic/possessive. Default **smart-case** (insensitive unless the pattern has an uppercase).

## Global conventions (every subcommand)
- `--claude-home <DIR>` (global, any position): repoint `~/.claude`. Priority **flag > `$CLAUDE_CONFIG_DIR` > `$HOME/.claude`**. Transcripts: `<DIR>/projects/<enc>/*.jsonl`.
- `--format text|json` (default `text`). JSON = one object per emitted unit, deterministic order.
- Timestamps render local-TZ (via `jiff`) ± offset, e.g. `2026-06-21 10:30:00.811+10:00`; JSON carries `ts_utc`+`ts_local` (`ts_utc/local`).
- **Relative WHEN** (`--since`/`--until`/`recover --at`) = `Ns`/`Nm`/`Nh`/`Nd`/`Nw` = seconds/minutes/hours/days/**weeks** ago (`45s` `90m` `2h` `3d` `1w`). Or absolute ISO8601 (bare date => local midnight); records w/o `timestamp` never match. A datetime bound is **<=-INCLUSIVE** (highest line whose `ts ≤ bound`).
- **`--turn-range A..B`** is inclusive, **0-based** - **turn 0 = the pre-first-user lead** (synthetic opening-context chain folded into the first genuine-user turn), so **`1..N` SKIPS it**; `0..N` re-includes it. Discover indices from the `s·t<n>` header in `search` text or `turn_index` in JSON.
- **Exit codes**: `0` success, **non-zero (=1)** on ANY error -> stderr, prefix `csift: error: `, full `anyhow` chain (levels joined `: `).
- **NOT errors (exit 0)**: `turns --slice` out-of-range (prints nothing); `search --line/--uuid` to no record (`unresolved: …`). **HARD non-zero**: `recover` partial/no-history; `image --id` no-match/ambiguous; exclusive-flag clashes; bad address syntax.
- **No silent truncation**: caps report their drop count; malformed lines are counted, not hidden.
- **Leading `session_header` JSON record (THE jq trap)** is `{"kind":"session_header","sessions_in_scope":N,"top_level_sessions":T,"subagent_sessions":S}`, the FIRST line whenever scope holds >=1 subagent - but **NOT every command emits it**. It has NO `session_id`, so a naive `jq -r '.session_id'` over an emitter hits a null first row; **filter emitters**: `jq -r 'select(.kind!="session_header") | .session_id'`. **EMITS (subagent in scope):** `list`, `files`, `turns`, `recover` **non-restore** (`--salvage`/`--patches`/`--at`/`--coverage`), `search` **DEFAULT** mode only. **HEADER-FREE always:** `whoami`, `agents`, `plan`, `image`, `recover --`**restore** (lone raw single-object form), `search -c` (pure `{"matched":N}`), `search -l` (pure NDJSON). So filtering a `-c`/`-l` stream FOR a header row returns nothing. Suppressed when `S==0` (`--no-subagents`). The shared header (list/search-default/files/recover-non-restore) is the **4-field** `scope_header_json`; **`turns` emits a SUPERSET** (those fields PLUS budget/automation fields) - don't key consumers on field-presence ACROSS commands.
- **Per-command JSON TERMINATORS are non-uniform** - can't key EOF on one shape. `turns` is the ONLY kind-tagged trailer (`{"kind":"skipped_lines",…}`, ALWAYS closes even at 0). `search`/`files`/`image` close with an UNTAGGED summary; `recover` non-restore closes a NESTED untagged `{"summary":{…}}`; `list`/`agents`/`plan` emit NO trailer (exact fields in each command section below).

### Argument quirks - flag order is FREE
A pre-pass REORDERS declared flags + values ahead of leading-dash project-target positionals (single `-` accepted, `--`-leading rejected). Net: put flags anywhere.

## Targeting - the positional `[PATH]...` (every subcommand except `whoami`)
0 targets => scan ALL projects. A bare uuid is NOT a target - prefix `@`.
- **real cwd** (path-encoded for you) | already-encoded `-Users-…` dir | `.` (this project) => scopes to project(s).
- **`@<uuid>`** (8-4-4-4-12) => one top-level session, across all projects if no project path. **`@<uuid-prefix>`** - 4-11 leading hex (`@13d9645a`) => the UNIQUE session it prefixes (else error).
- **`@main`** => calling TOP-LEVEL session (reads `$CLAUDE_CODE_SESSION_ID`). **`@trap:<marker>`** => calling SUBAGENT.
- **`@<agent-hex>`** (>=12 hex, from `csift agents`) => that subagent + descendants (or the agent ALONE under `--no-subagents`). **`*.jsonl`** => that one transcript + subtree.

`search` puts PATTERN first: `csift search <uuid>` searches the LITERAL string, scope via `csift search PATTERN @<uuid>`.

### Subagent span - default ON for list/search/files/recover/plan/image; `--no-subagents` (DOMINANT, any order) restricts to top-level; `files` also `--subagents-only`. `turns` is the EXCEPTION: top-level only by default, `--include-subagents` opts in (then `--budget` is PER session). `agents` lists subagents; no span flag.

### `@trap:<marker>` - "which subagent am I?"
CC sets `$CLAUDE_CODE_SESSION_ID` to the TOP-LEVEL id even inside a subagent, so it can't name itself via env. Fix: INVENT a marker, put it LITERALLY in THIS csift command; csift scans the session's main + subagent transcripts for a **Bash `tool_use`** whose **`input.command`** holds BOTH the marker AND literal `csift`. Matching the tool_use INPUT (not the line / a `tool_result`) avoids a false hit on an ECHOED marker. CC flushes the tool_use BEFORE it runs => resolves first-try. Resolution: 1 subagent => that agent; only main => session; 0 => error (re-run); >1 => ambiguity error.
**Marker grammar (ENFORCED, rejects loudly)**: one-shot, invented NOW, imaginative + CONTEXT-INDEPENDENT; ASCII + **byte length >= 13**; **EXACTLY 3 CamelCase words** (each 1 upper + >=2 lower - no single letters, no ALLCAPS like `HTML`/`USB`) + **exactly 4 trailing digits**, not a trivial run (constant digit-step -2..2 => rejects `0000`/`1234`/`1357` etc.). NEVER script-generate (a generator call itself carries the marker => ambiguity), never from a shell var/concat (must appear verbatim), never reuse. From MAIN use `@main` (its marker flushes only at turn end => may need re-run).

## Categories (`search -t`) - `thinking | user | tool | tool-response | agent`
- `thinking` = assistant thinking. `tool` = `tool_use` (AUQ counts). `tool-response` = `tool_result` (each names its tool). `agent` = visible assistant end-of-turn text.
- `user` = GENUINE human input + **AUQ answers** + machine **automation-trigger openers** (`<task-notification>` -> `[<kind> <id> <status>] <summary>`); NOT tool_result carriers (which ride `role:"user"` too, ~4-5x as many). An UNanswered AUQ never reaches disk; only an ANSWERED one opens a turn.
- **Automation `kind`** (longest leading prefix of `<summary>`, case-insensitive): `background command`->`background-command`, `dynamic workflow`/`workflow`->`workflow`, `monitor`/`scheduled`/`cron`->`monitor`, `agent`->`agent`, else->`task`. **Monitor RECLASSIFICATION (load-bearing)**: a captured monitor loop is often a `&`-detached **`Background command "<name>"`** leading with `background command`, so a pure prefix check buries it all under `background-command` and `monitor` matches zero. Rescue: scan the **QUOTED command NAME** (between the first `"` pair) - a monitor-cadence token there (standalone word `monitor` or `liveness`, or substring `re-arm` / `relaunch monitor`; `tick`/`cadence` EXCLUDED as too broad) re-routes to `monitor`. Quoted-name-only so trailing prose mentioning "monitor" isn't over-captured; no quotes => `background-command`.
- **isMeta wakeup-tick BYPASS**: classification only sees `<task-notification>` COMPLETION pulses. The `ScheduleWakeup` **wakeup-tick prompts** DRIVING a monitor/cron cadence are `isMeta:true` user records, NOT `<task-notification>`s - they bypass automation parsing via the isMeta gate in `is_genuine_user` (neither genuine-user nor classified-automation), so driving ticks don't surface as `-t user`.

---

## `list` - fast "which session is this?" index
```
csift list [PATH...] [--no-subagents] [--format json]
```
Head+tail read only. Per session: first/last genuine-user (+ts), last agent (+ts), decoded `cwd`, `version`, `gitBranch`, skipped count. Excerpts cap `… (+N chars)`. Glyphs `◂`=user `▸`=agent.
JSON per session: `{session_id, is_subagent, parent_session_id, path, cwd, version, git_branch, first_user{…}, last_user{…}, last_agent{…}, skipped_lines}` - NDJSON, NO trailer, leading `session_header` with a subagent in scope. `parent_session_id` is the always-re-feedable owning uuid (== `session_id` on a top-level row); NEVER re-feed a subagent's bare-hex `session_id` as `@<uuid>` - use the parent.

## `search` - regex round-trips + message fetcher
```
csift search [PATTERN] [PATH...] [-t CAT].. [-i] [--multiline] [--since 2h] [--until ISO]
  [--turn-range A..B] [--max-count N] [-c|--count] [-l|--files-with-matches]
  [--siblings] [--sibling-category CAT].. [--full|--no-truncate] [--line SPEC] [--uuid U]
  [--subagent HEX] [--resolve-persisted] [--no-subagents] [--format json]
```
Empty PATTERN (`""`) = pure filter (combine `-t`/`--since`/`--turn-range`). A hit returns the COMPLETE round-trip: a `tool_use` WITH its `tool_result` (paired by `tool_use_id`); a user turn WITH the agent reply; a `tool_result` WITH its `tool_use`; matched thinking/agent text in its turn (an answered AUQ -> Q+options+answer together).

**Turn-boundary predicate (`opens_turn`, shared by turns/search/files/recover so they never drift):** a record opens a turn iff it is a GENUINE user msg OR an AUQ-answer boundary OR a plan-rejection boundary (a tool-use rejection with typed user prose). An **AUQ answer** opens a turn only on a NON-errored `tool_result` with non-empty `toolUseResult.answers`; a cancelled/errored AUQ does NOT. **The `(notes only)` path is load-bearing**: when the answer is the literal `(notes only)` placeholder the real message lives in `toolUseResult.annotations[question].notes` (`note: …`); dropping it swallows the turn. Abandoned esc-cancel / edit-resend DRAFTS are dropped via draft-supersession (NON-null `parentUuid` identity, last opener per parent wins).
- `-t/--category` (repeatable) - default all. `-i` forces insensitive (over smart-case). `--multiline` lets `.` cross newlines.
- `--turn-range A..B` (inclusive, 0-based; turn 0 = pre-first-user lead) - **mutually exclusive** with `--since`/`--until` (`s/m/h/d/w`, <=-inclusive).
- `--max-count N` caps emitted exchanges (GLOBAL, AFTER the cross-scope `started_utc` sort); reports drops.
- `-c/--count` - ONLY the integer total; honors every filter; **adds back the `--max-count`-capped remainder** (true total). Text=`N`, JSON=`{"matched":N}`. Exclusive with `-l`.
- `-l/--files-with-matches` - list ONLY distinct sessions with >=1 match, one id/line, first-match chronological; **from the FULL set BEFORE `--max-count`**. Top-level => bare uuid; subagent => bare hex + `  (parent <uuid>)`. JSON per line `{session_id,is_subagent,parent_session_id}` - **PURE NDJSON, NO leading `session_header`, NO footer** (unlike default-mode search); subagents ARE in scope (with a `parent` annotation). `jq` it raw, do NOT `select(.kind!="session_header")` (a no-op returning everything). **Exclusive with `-c`.**
- `--siblings` - also render each matched turn's NON-matched records. `--sibling-category CAT` (repeatable) **implies `--siblings`**, narrows by REPLACEMENT; `--siblings` alone => all categories EXCEPT the match `-t` set (none => all five). The match is never repeated. No effect under `-c`/`-l`.
- **EXCERPT TRUNCATION (defeat it!)**: each hit shows a **~400-char excerpt CENTERED on the match** (`list`/`agents` previews 200), elided `… (+N chars)`. **`--full`/`--no-truncate`** = whole record every hit; `--line`/`--uuid`-addressed render FULL.
- **FETCH mode** - `--line SPEC` (1-based physical line(s), comma+range `87,495-500`; **needs a SINGLE transcript**: `@<uuid> --no-subagents` or `--subagent HEX`) or `--uuid U` (record uuid, comma-repeatable). Emits those records FULL - the in-permission raw-`Read` alternative. A singleton resolving to nothing => `unresolved: …` + exit 0 (a miss inside `--line A-B` is clamped, not an error).
- `--subagent HEX` - pin `--line` to ONE subagent transcript (fail-CLOSED: unmatched/`[]`/multiple => error); else `--line` addresses the top-level transcript.
- `--resolve-persisted` - before matching, inline `<persisted-output>` pointers: a too-large `tool_result` saved to `tool-results/<id>.txt` gets FULL content inlined (via `toolUseResult.persistedOutputPath` or scraped `Full output saved to: <ABS>`) so a regex hits it. Read-failure is non-fatal.
- JSON per Exchange: `{session_id, is_subagent, parent_session_id, turn_index, started_utc/local, hits:[{category, line, uuid, ts_utc/local, tool_name, excerpt, image_ids:[…]}], record_uuids:[…]}` (+`siblings:[…]` under `--siblings`). `image_ids` (`#N`/`L<line>i<n>`) feed `csift image --id`. Leading `session_header`; CLOSED UNTAGGED `{matched, sessions, dropped_by_cap, skipped_lines, unresolved:[…]}` (`unresolved` = explicit `--line`/`--uuid` tokens hitting nothing). `-c` JSON = `{"matched":N}`.

```bash
csift search "panic" @<uuid> -t agent --since 6h
csift search "" @<uuid> --no-subagents --line 46550 # the exact cited message, full
csift search "Full output" --resolve-persisted --format json # inline persisted-output, then match
```

## `agents` - subagent lifecycle + topology
```
csift agents [PATH|@<uuid>] [--agent HEX] [--kind builtin-task|workflow].. [--since][--until]
  [--by trigger|start|completion] [--tree] [--with-files] [--returned-message] [--format json]
```
Lists a session's subagents. **Kind = on-disk PATH LOCATION, not `agentType`**: `builtin-task` = `subagents/agent-<hex>.jsonl`; `workflow` = `subagents/workflows/wf_<id>/agent-<hex>.jsonl`; `journal.jsonl` is an EVENT log, not a transcript. Canonical id = bare `<hex>` (joins to files/recover/list). `agentType` is NOT a reliable discriminator - both kinds carry the same spread (`Explore`, `general-purpose`, `oh-my-claudecode:*`); only **`workflow-subagent` is workflow-exclusive** (an unlabeled workflow meta defaults to it), kept as a per-row sub-label (JSON `agent_type`).
- **`--kind` is the TRANSCRIPT-SHAPE filter, value-enum `builtin-task | workflow` ONLY** - a DIFFERENT axis from the automation-trigger `kind` taxonomy (`background-command`/`agent`/`monitor`/`task`/`workflow`) that `turns`/`search -t user` classify; they share ONLY the literal token `workflow` (unrelated meaning). So `csift agents --kind monitor` (or `agent`/`task`/`background-command`) is a **clap parse error** (`invalid value 'monitor' for '--kind <KINDS>'`), not an empty result. Ignored when `--agent <hex>` pins one. (Hidden no-op `--include-subagents`/`--no-subagents` exist only to error pointedly vs being swallowed as a bogus PATH - `agents` has no span control.)
- **TOPOLOGY**: on disk FLAT at every depth (CC writes every subagent - even a sub-subagent - under the MAIN session's `subagents/` dir). Nesting is LOGICAL, from the spawning `Task`/`Agent` `tool_use` + child meta's `toolUseId`: a GLOBAL spawn index sets `parent_agent_id` (null => direct child, depth 0) + walks `depth`. All real data is depth 0 today.
- **TRIGGER time** = parent tool_use ts (true spawn instant); the child's `started_utc` LAGS 0.2-4.7s and can't order a sibling fan-out => `--by` defaults to **`trigger`** (`start`=child's first record, `completion`=last). Workflow agents lack `toolUseId` => trigger->start.
- **Returned message - 3-way** (`--returned-message`; auto for a single `--agent`; JSON `returned_message_source`): (a) sync built-in => parent tool_result text (`sync-tool-result`); (b) async built-in (parent `Async agent launched …`) => child transcript tail (`async-child-tail`); (c) workflow => `journal.jsonl` `result` (`workflow-journal`).
- `--agent HEX` drills into one (implies `--returned-message`). `--with-files` attaches files-changed. `--kind` filters. `--tree` renders parent->child (workflow RUN nodes from `workflows/wf_*.json` parent their `wf_<id>` agents); JSON nests `children`. Status: `completed` (journal `result` OR a visible terminal EOT) / `running` / `unknown`.
- JSON node fields: `agent_id, kind, parent_session_id, parent_agent_id, spawn_tool_use_id, spawn_tool, workflow_id, agent_type, description, trigger/started/completed_utc/local, duration, status, depth, skipped_lines` + on demand `returned_message(_source)`, `files_changed:[{path, op, is_create}]`, `children[]`. NO trailer. `files_changed[].op` = HYPHENATED `write`/`edit`/`notebook-edit`/`multi-edit`/`bash` (vs `files --timeline`'s underscore `json_key`). `--tree` JSON: one object/session `{session_id, workflow_runs:[run], agents:[node]}`; each RUN all **snake_case** `{run_id, task_id, workflow_name, status, agent_count, duration_ms, total_tokens, total_tool_calls, default_model, started_utc, started_local, children:[node]}` (a camelCase `jq` like `.runId` returns null).

## `whoami` - identify the calling session (false-positive-safe)
```
csift whoami [@trap:<marker>|@main] [--format json]
```
Reads `$CLAUDE_CODE_SESSION_ID`; alias `$CODEX_COMPANION_SESSION_ID`. NEVER a loose `/session/i` regex (`SECURITYSESSIONID` is a trap). Neither set => **errors** (pass `@<uuid>`); never guesses by mtime. The `path` line is ALWAYS printed (a `<not found>` note when unresolved). Optional positional **`@trap:<marker>`** = "which SUBAGENT am I?" → the full UPSTREAM ancestry CHAIN (self → … → top-level root; the walk-UP mirror of `agents`' walk-DOWN), env-INDEPENDENT (works for built-in Task AND workflow subagents); JSON `{chain:[{session_id,is_subagent,parent_session_id,depth,path}…]}`. **`@main`/none** = env top-level; JSON `{session_id, path}`.

## `files` - what a session changed, and when
```
csift files [PATH|@<uuid>] [--summary|--by-dir|--by-file|--timeline]
  [--subagents-only|--no-subagents] [--turn-range A..B] [--since][--until] [--format json]
```
Authoritative for Edit/Write/MultiEdit/NotebookEdit (`input.file_path`/`notebook_path`; create-vs-edit from the paired `toolUseResult.type`). **Bash mutations are HEURISTIC** (lexical parse of `rm`/`mv`/`cp`/`mkdir`/`touch`/`tee`/`sed -i`/`git`/redirection; no path field) - always `(heuristic)`, relative paths VERBATIM. A failed `is_error:true` op is EXCLUDED.
- Detail (clap group, default **`--summary`**): `--summary`=per-top-level-dir rollup; `--by-dir`=per-dir counts + distinct-file + first/last; `--by-file`=per-abs-path counts + first/last; `--timeline`=full chronological (heavy).
- **Edit-before-Read boundaries** section follows the body in EVERY mode (or alone): `(⚠ path, Lnnnn, turn, ts, kind)` - a formatter/husky/git/external-editor changed a file out of band, forcing a fresh Read.
- JSON (leading `session_header` with a subagent in scope): one object per bucket/dir/file (counts + first/last), or per mutation for `--timeline` `{path, op, ts_utc/local, turn_index, line_no, is_create, heuristic}` (`op` = underscore `json_key` `write`/`edit`/`notebook_edit`/`multi_edit`/`bash`); then per boundary `{type:"edit_before_read_boundary", path, line_no, turn_index, kind, ts_utc/local, session_id, is_subagent, parent_session_id}`; then an UNTAGGED summary `{distinct_files, total_mutations, edit_before_read_boundaries, skipped_lines, detail_level}`. Empty => `no file mutations found`.

## `recover` - reconstruct a file (or plan) from the transcript
```
csift recover @<uuid> --file <ABS_PATH|PATH-SUFFIX|@plan> [--salvage|--patches|--at WHEN|--coverage]
  [--out PATH] [--turn-range A..B|--since/--until] [--line-range A..B]
  [--no-subagents] [--files-from MANIFEST --out-dir DIR [--force]] [--format json]
```
Replays the file's Read/Write/Edit stream in transcript order into a sparse buffer (absent line = explicit gap, NEVER fabricated). `--file` REQUIRED, matched against the transcript path EXACTLY or as a component-aligned trailing SUFFIX (`app.py`/`src/app.py` both match `/abs/src/app.py`; `b.rs` != `/x/ab.rs`). Five **mutually-exclusive modes** (default = restore):
- **restore (no mode flag)** = RAW final bytes to stdout (no banner/line-numbers - clean to `> file`), OR `--out` writes the file. **HARD-FAILS (non-zero, never a holey file)** if the session saw only PART of the file - names covered+missing ranges, boundaries, a reconcile recipe. JSON `{file, complete:true, lines, content}` (+`path, wrote` with `--out`), no header/trailer.
- **`--salvage`** = never-fails best-effort FINAL-state fragment: known lines numbered, gaps `??? lines A..B unknown`. **BYTE-IDENTICAL to `--at @latest`** - both = no cutoff => full final-state replay, gaps explicit.
- **`--patches`** = segmented unified diffs between integrity boundaries, FULL context (CC's Read-before-Edit means every read-covered line was observed); no diff spans a boundary. JSON segment: `{type:"segment", segment_index, line_no, line_no_start, line_no_end, turn_start, turn_end, ts_utc/local, pre_state_known, anchor_source('write'|'full-read'|'file-attachment'), unified_diff}` (`jq -r '.unified_diff // empty'`).
  - **THREE anti-fabrication anchor checks gate every structured-patch hunk** (any failure => hunk dropped `UnAnchorable`, never a fabricated island line): (1) **neighbourhood-known** - `oldStart`±1 must touch >=1 known line; a hunk in an all-unknown gap is position-drifted, refused. (2) **region-known** - when `old_lines>0`, EVERY removed-range line must be known (no re-anchor across a gap). (3) **context-verification** - the patch's old-region lines must EQUAL the buffer at the anchored position; disagreement (buffer drifted) => refuse rather than corrupt known lines.
  - **Net-delta total-length fix**: post-splice, seen-total is adjusted by the edit's **NET line delta** (`added-removed` over hunks), clamped >= max-known-line and >=0 - NOT monotonically maxed. Maxing left a phantom trailing gap after a net deletion (insert to N+1 then delete to N still reported N+1 => spurious `??? line N+1 unknown`).
- **`--at WHEN`** = point-in-time partial snapshot (line-numbered, gaps explicit). WHEN = ISO8601 | relative `2h` | `@turn:N` | `@line:N` | `@latest`. **`@turn:N` cutoff = the MAX jsonl `line_no` among events with `turn_index <= N`** (LAST line at-or-below turn N, INCLUSIVE - the in-code doc-comment "first line strictly after turn N" is STALE); none <= N => line 0 => EMPTY. **`@line:N` = a JSONL TRANSCRIPT line** (the `Lnnnnn` csift prints), **NOT a `--file` line** (for a FILE-line span use `--line-range`). `@latest` = no cutoff; a datetime is <=-INCLUSIVE. JSON: `{type:"snapshot", line_no, line_no_cutoff, lines:[{n,text,set_at_line}], gaps:[[a,b]], seen_total_lines}`.
- **`--coverage`** (alias `--dry-run`) = scoping only. JSON: `{recoverable_lines, seen_total_lines, covered_ranges:[[a,b]], fragments, events:{read_full, read_windowed, edit, edit_unanchorable, write, bash, external_edit, history_snapshot, integrity_error}, boundaries:[…], file}`. Each `boundaries[]` = `{line_no, turn_index, ts_utc, ts_local, kind, confidence, detail}` - `confidence in {"authoritative","heuristic"}` (text `⚠`/`~`); `jq 'select(.confidence=="authoritative")'` keeps HARD only.
- **Integrity boundaries** (confidence order): (1) AUTHORITATIVE `modified since read` harness error (the HARD boundary); (2) AUTHORITATIVE Edit whose `originalFile` disagrees with the replayed buffer at a **>=25% line mismatch** (`mismatches*4 ≥ compared`) - a drift signal csift MINES to SEGMENT (claude-file-recovery DISCARDS it); (3) AUTHORITATIVE external `edited_text_file` attachment; (4) HEURISTIC Bash mutation (redirect / `sed -i` / `tee`). **`File has not been read yet` is NOT a boundary** - the edit never landed. **Edit-before-Read detection** = the `is_error` `File has not been read yet` / `String to replace not found` carriers.
- **Subagent / workflow recovery - input-side fallback (load-bearing)**: a top-level tool_result carries a structured `toolUseResult` echo `recover` replays; a SUBAGENT (built-in Task) / workflow-agent records a BARE string with NO `toolUseResult` => its WRITTEN file is invisible (`no recoverable history`) though `files`/`search` see it. `recover` closes the gap from the tool_use INPUT - `Write.content`, `Edit.{old_string,new_string,replace_all}` (replayed old->new), `MultiEdit.edits[]`. The input-fallback is DUAL-gated: skip ids that ALREADY have a `toolUseResult` (top-level carriers always echo => never double-emits, main recovery byte-identical) AND ids of FAILED ops (`is_error==true`). **WHY both**: a FAILED Edit has NO `toolUseResult`, so the failed-id gate catches it (covers `String to replace not found` AND `File has not been read yet`). So `recover @<subagent-hex>` works, no ghost edit lands.
- **Modified-since-read invalidation (load-bearing)**: a `modified since read` boundary INVALIDATES the pre-boundary buffer (clears known lines + resets seen-total), so only content RE-READ/re-written AFTER survives. Stops restore falsely reporting `complete` and `--salvage`/`@latest` dumping stale lines.
- **`--file @plan`** = bash-safe magic VALUE (not a mode): resolves the bound plan, reconstructs it like any file under any mode + `--out`/`--format`. Rebuilds even a DELETED plan. ERRORS when no plan bound / target spans different plans.
- `--line-range A..B` (1-based) restricts the reconstructed line space; every ref carries `Lnnnnn`.
- **BATCH** `--files-from MANIFEST` (one abs path/line; `#`comments + blanks ignored) requires `--out-dir`, exclusive with `--file`. ONE corpus scan (Aho-Corasick of basenames) reconstructs every file -> `<DIR>/<abs-without-leading-slash>` + a `recovery-report.tsv`. Per-file `complete|partial|no-history|skipped-exists`; partial best-effort + flagged. `--force` overwrites. Honors `--at`/`--since`/`--until`.
- The four non-restore modes emit NDJSON + leading `session_header`, closed by the NESTED `{"summary":{sessions, file, mode, skipped_lines}}`. Restore is the lone single-object form (NO header/trailer).

```bash
csift recover @<uuid> --file /abs/gone.py --salvage # survivors, gaps explicit
csift recover @<uuid> --file @plan --out /tmp/plan.md # rebuild the bound plan (deleted ok)
```

## `plan` - locate the plan bound to a session
```
csift plan [PATH|@<uuid>] [--reverse PLAN_FILE] [--no-subagents] [--format json]
```
LOCATES (doesn't dump - use `recover --file @plan`) the AUTHORITATIVE binding: a `plan_mode` **attachment record** carrying `planFilePath`/`isSubAgent`/`planExists` - NOT a path heuristic (a session may freely Edit OTHER sessions' plans). Plans live flat under `~/.claude/plans/<three-word>.md` (subagent: `-agent-<hex>` suffix), name NOT derivable from id. No target => calling session; spans subagents (`--no-subagents` restricts).
- **`--reverse <plan.md>`** inverts: which session(s) bind this plan file. Conflicts with a positional target; an empty result is honest, not an error.
- JSON per plan (NDJSON, header-free): `{plan_file, session_id, is_subagent, parent_session_id, plan_exists, line_no}`.

## `turns` - restore the verbatim turns a compaction summary clipped
```
csift turns [PATH|@<uuid>] [--budget N] [--budget-unit chars|tokens] [--round-trip-fraction F]
  [--agent-msgs longest|eot-only|rich|all] [--profile heavy|light] [--max-compactions N]
  [--agent-run-threshold N] [--agent-rich-min-chars N] [--agent-declaration-max-chars N]
  [--keep-first|--no-keep-first] [--include-subagents] [--turn-range A..B|--since/--until]
  [--slice N [--slices N] [--window N]] [--out PATH] [--format json]
```
A CC compaction summary keeps task STATE but loses TURN fidelity (clips ~22 user turns -> ~17 bullets; ~239 assistant turns -> 1 quote). `turns` SUPPLEMENTS it: re-emits verbatim user/assistant turns in order (each line `Lnnnnn`); does NOT re-derive task state. **Selection walks backward from EOF (recency-first), output sorts ascending**; transparent to compaction boundaries (a summary is a turn MEMBER, not a delimiter) => reaches across MANY by default (`--max-compactions N` caps crossings, default 0 = uncapped).
- **`--budget N` is PER SESSION** (default 40000). `--budget-unit tokens` reads N as tokens (~4 chars/token) - GOTCHA: default=160000 chars; pass explicit `--budget` when flipping. JSON `budget_chars`/`max_total_chars` are ALWAYS chars.
- **`--round-trip-fraction F`** (default 0.5, open (0,1)) = hard floor: Phase 1 spends `budget·F` only on round-trip-complete turns recency-first; Phase 2 fills the rest user-first (without it an assistant-heavy tail recovers ZERO user turns). **TWO round-trip predicates - don't conflate:** the **Phase-1 FLOOR uses `is_human_round_trip`** (GENUINE-human opener AND >=1 agent msg) - so an automation `<task-notification>` pulse + agent ack, a structural round-trip, NEVER consumes the human-reserved fraction lane. **Phase-2 FILL uses `is_round_trip`** (looser: ANY opener - human OR machine pulse - AND >=1 agent msg). A pulse is floor-excluded but fill-eligible; keeps the floor agreeing with the header's `selected N user units (M automation triggers)` split.
- **`--agent-msgs`** (master switch; a turn can own a LONG agent run clipped to 1 quote):
  - **`longest` (the real clap DEFAULT - any "eot-only is default" doc-string is stale)** - per-index keep predicate over a multi-message turn: the **LONGEST** (by `full_chars`) is ALWAYS kept; the **FIRST** iff `full_chars >= --agent-rich-min-chars` (LENGTH gate); the **LAST** iff RICH (distinct from FIRST's length-only gate); each **MIDDLE** iff RICH; a single-message turn keeps its sole message. **Tie**: the LAST max wins.
 - **`eot-only`** - last message only per turn (= pre-feature single-EOT output).
 - **`rich`** - last always + first by `--keep-first` + each non-droppable middle; only filters a LONG run (> `--agent-run-threshold`, default 6).
 - **`all`** - every message.
 - **"RICH" test** (first-match-wins OR over 6 arms, KEEP-ON-DOUBT): (1) length `≥ --agent-rich-min-chars` (default 280) OR a SIGNAL - (2) number-of-substance, (3) commit-hash-like hex, (4) `file.rs:NNN`/`src/…` ref, (5) backtick code span, (6) finding/decision lexeme. Only a SHORT (`< --agent-declaration-max-chars`, default 200) signal-less intent-verb opener (`let me…`/`now i…`) is droppable; anything uncertain is kept (a wrong-drop is unrecoverable).
 - **Collapsed-run placeholder (`△`)**: a contiguous collapsed run renders as ONE `△ L{first}-L{last}  [X agent message(s), Y tool call(s)[, Z failed]]` (**X** = collapsed agent messages, **Y** = `tool_use` blocks in preceding spans, **Z** = erroring tool_results) - Y shown even at 0, the **Z clause OMITTED when 0**. **FETCHABLE range**: pull the bodies with `csift search "" @<uuid> --no-subagents --line {first}-{last}` (`--subagent <hex>` in a subagent).
- `--keep-first` (default on) keeps a turn's first message by position privilege in `rich`; NO effect in `longest`. `--profile heavy` = 4 / 200 / 140; `light` = 8 / 360 / 240 (threshold / rich-min / declaration-max). Profiles apply BEFORE individual flags (explicit wins); don't change `--agent-msgs` mode.
- **Ellipsis (role-asymmetric)**: a unit over its role cap (`USER_CAP=600`, `ASST_CAP=900`) is middle-truncated keeping head+tail, marked `… [+K chars, L lines elided] …` (UTF-8-safe, nothing fabricated/dropped).
- **Dedup vs the live summary**: a live-region turn (`compactions_before==0`) whose 80-char prefix matches the summary's §6 user bullets / §9 assistant quote is flagged `(also in summary)` and DEMOTED (after non-dups), never dropped. A turn predating an OLDER boundary = pure restoration.
- **Scope banner SUPPRESSED for a single clean session**: the top `scope` line prints ONLY when >1 session in scope OR a session was budget-skipped. Don't script on it; read scope from JSON's `session_header`.
- Text: `SESSION <id>` + budget header, then `▽ Lnnnnn USER (ts)` / `[N tool calls]` / `△ Lnnnnn ASSISTANT (ts)` (one `△` per kept agent message under `rich`/`all`), `══ compaction boundary · summary at Lnnnnn ══` banners, `(also in summary)` flags, collapsed `△ L{a}-L{b}` placeholders. `--out` writes the full doc while the summary prints to stdout. JSON (verbatim, un-truncated `text`): one object/unit `{line_no, role, ts_utc/local, tool_calls, full_chars, rendered_chars, truncated, elided_chars/lines, also_in_summary, compactions_before}`, interleaved `{"kind":"compaction_boundary",…}` + per span `{"kind":"collapsed_agents",…}`, closed by `{"kind":"skipped_lines",…}`.
- **SLICING for hook injection** (the <=10000-char `SessionStart` `additionalContext` cap - see hook recipe). `--slice N` (1-based) prints ONLY the Nth chunk of the DOCUMENT body (turn units + boundary banners, NO chrome), greedily packing lines into `<= --window`-char chunks (default 10000, hard-splits an over-long line). DETERMINISTIC: same session+budget => identical boundaries; `1..K` concatenated reproduces the body byte-for-byte. Out-of-range N => nothing. `--slice` is TEXT-ONLY, exclusive with `--out`/`--format json`; `--slice 0`/`--window 0` error. **`--slices N` (FIXED-FLEET)** pins chunk COUNT to N hooks: fills N newest-first slices with WHOLE turns (per-role caps dropped; a turn middle-truncated ONLY when it ALONE exceeds `window - 200` reserved for the elision notice), DISCARDS oldest overflow => count never drifts (budget = `N * --window`, `--budget` ignored). REQUIRES `--slice i` (both >0). Else `--slice` is legacy variable-count.

## `image` - list + extract the images a session carries
```
csift image [PATH|@<uuid>] [--id '#N'|L<line>i<n>].. [--out DIR|FILE.ext]
  [--since][--until][--turn-range A..B][--uuid PREFIX] [--no-subagents] [--format json]
```
Pasted/screenshot images live INLINE as `{type:"image",source:{type:"base64",…}}` blocks. Default = LIST (content-deduped via a `<len>:<head>:<tail>` fingerprint - a re-injected image shows once); `--out <DIR>` => EXTRACT.
- **`#N` handle** = the SAME `[Image #N]` number the model sees, assigned by zipping a record's markers with its image blocks **ONLY when `markers.len() == out.len()`**; on a count mismatch (a back-ref to a compressed-out image) `seq` is `None` => a `#N` may resolve to nothing (image keeps only its `L<line>i<n>` locator). **NOT unique** - CC reuses low N. **`--id #N` naming >1 DISTINCT image => AMBIGUOUS, HARD ERRORS** with the occurrence list (`t<turn>` / `L<line>i<n>` / uuid / time / excerpt). "Distinct" is by the `<len>:<head>:<tail>` base64 fingerprint (latest kept): 0=>unresolved, 1=>pick, >1=>ambiguous. Disambiguate via the locator or PRE-narrow with `--since`/`--until`/`--turn-range`/`--uuid PREFIX`. Bare `32` == `#32`. **`L<line>i<n>` locator** (always unambiguous) = 1-based jsonl line + 1-based ordinal of the image among its blocks (direct OR nested in a `tool_result` array). `--id` is per-transcript => needs a SINGLE transcript.
- **`--out` extension drives format** (`convert in out.jpg` idiom): a DIRECTORY (or any path with no `png`/`jpg`/`jpeg`/`gif`/`webp` ext) writes each image SOURCE-format, auto-named `<session-short>[-img<N>]-L<line>i<n>.<ext>`; a FILE path with an image ext writes the SINGLE selected image, CONVERTING if it differs (->png lossless, ->jpeg/webp q90, ->gif Floyd-Steinberg <=256-color). >1 image + a file path => error. Animated GIF -> FIRST frame + warning. A `url`-source image (no inline bytes) is reported with its URL.
- JSON list per image: `{handle, seq, id(=L<line>i<n>), line_no, img_index, session_id, is_subagent, parent_session_id, source_kind, media_type, b64_len, est_bytes, url, record_uuid, ts_utc/local}` + summary `{images, transcripts, skipped_lines}`. Extract JSON: `{path, bytes, media_type, source_media_type, converted, notes}`. `turns`/`search` surface these ids inline (`[N image(s): #32…]`).

```bash
csift image @<uuid> # list (deduped)
csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg # locator -> convert to jpeg
```

---

## Recipes - shell pipe + jq

```bash
# Every abs path a session touched (authoritative only, drop Bash heuristics):
csift files @<uuid> --by-file --format json | jq -r 'select(.path and (.heuristic|not))|.path'
```

### SessionStart(compact) hook - install turns as a post-compaction verbatim supplement (the #1 recipe)
N `SessionStart(matcher:"compact")` hooks run `turns --slices N --slice i` (slice mechanics above) to re-inject the recent verbatim User<->Agent turns - a SUPPLEMENT to the summary, orthogonal + window-extending. The text lands as a **`type:"attachment"`** record (csift ignores `attachment` => never a turn). Safe to re-fire each compaction: the old copy is summarized away, re-injected fresh = one (no pile-up); resume won't match.

**The slice race is load-bearing.** CC runs same-event hooks CONCURRENTLY, collecting output in COMPLETION (not registration) order, so they arrive scrambled - a turn split across a chunk glues back wrong. A `$PPID`-namespaced **done-flag barrier** forces order: slice i waits for slice i-1's `.done`. ONE script, registered N times with a different slice arg, reproduces the installed hook:

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
- `search -l` (which sessions) before `search` (what); `recover --coverage` before trusting a `--salvage`.
- Sub-second on 200MB+ files (mmap + SIMD + prefilter + rayon); never fear an unscoped scan.
