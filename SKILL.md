---
name: csift
description: Search and audit Claude Code session + subagent jsonl transcripts. Use when you need to find what was said/done in a past (or the current) Claude Code session — regex-search the transcript corpus under ~/.claude/projects, list sessions to identify "which session is this", inspect a session's subagent lifecycle (built-in Task agents + OMC/workflow agents), identify the calling session, or recover standing directives a context-compaction dropped. ripgrep-for-transcripts: pure regex, no embeddings/semantic search.
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

- **"What did session X say / decide / do about Y?"** → `csift search "Y" --session <uuid>` (or
  `--path <project>`). Returns the *complete round-trip exchange* (the matched record plus its
  paired request/response), never a bare fragment.
- **"Which session is this transcript / which session was working on Z?"** → `csift list <path>`
  emits a fast identity tuple per session (first user msg, last user msg, last agent msg, cwd,
  branch, CC version) without parsing whole files.
- **"What subagents did this session spawn, and did they finish?"** → `csift agents --session <uuid>`
  reports each subagent's kind / start / completion / duration / status.
- **"Who am I (the calling session)?"** → `csift whoami` resolves the current session id from
  `$CLAUDE_CODE_SESSION_ID`.
- **Post-compaction recovery** → after a context compaction, diff the compaction summary against the
  lossless jsonl to surface STANDING DIRECTIVES the summary dropped or inverted (the motivating
  use-case — see "Integration recipes → (A)").

Reach for csift instead of hand-grepping `~/.claude/projects/**/*.jsonl`: it understands the
record model (genuine-user vs tool_result-carrier, thinking/tool/agent categories), spans subagent
transcripts, and reconstructs whole turns rather than emitting line fragments.

---

## Command surface

Four subcommands: `list`, `search`, `agents`, `whoami`. `list`/`search` span each session's
subagent transcripts **by default** (`--no-subagents` opts out). Every subcommand takes
`--format text|json` (default `text`).

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
csift search [PATTERN] [--path PATH...] [--session ID] [--no-subagents]
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
- On a hit, csift returns the **whole turn** (a turn is delimited by genuine-user messages): a
  matched `tool_use` comes WITH its `tool_result`; a matched user turn WITH the agent's response.
- **`--category`/`-t`** (repeatable; with none given, all five are eligible) — see the category
  model below.
- **Windowing**: `--turn-range START..END` (inclusive, 0-based on genuine-user order) is mutually
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
csift search -i "askuserquestion" -t tool             # tool_use blocks naming AUQ
csift search "" -t user --since 2h --path .            # user turns, last 2h, this project
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

### `agents` — a session's subagent lifecycle

```
csift agents [PATH...] [--session ID] [--kind builtin-task|workflow]...
             [--since WHEN] [--until WHEN] [--by start|completion] [--format text|json]
```

Lists every subagent transcript a session spawned, with id, kind, start + completion timestamps,
duration, and a determinable status (`completed` / `running` / `unknown`). The TARGET selects the
parent session: `--session <uuid>` for one session, or a project PATH/encoded-dir to cover every
session under it (subagents grouped under their parent session). `--since`/`--until` filter by
START time by default; `--by completion` switches the axis to completion time.

Examples:

```bash
csift agents --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d      # one session's subagents
csift agents .                                                   # every session under this project
csift agents . --kind workflow                                   # only workflow agents
csift agents --session <uuid> --since 2h                         # subagents STARTED in the last 2h
csift agents --session <uuid> --since 09:00 --by completion      # filtered on COMPLETION time
csift agents . --format json                                     # machine-readable lifecycle rows
```

Text output shape (`(wf_…)` = workflow id, `[…]` = agentType sub-label):

```
SESSION  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  ae24045bd6d4bdaff  workflow  (wf_35cd8400-f04)  [Explore]  completed
    started    2026-05-29 19:53:16 AEST (2026-05-29T09:53:16.201Z)
    completed  2026-05-29 19:54:34 AEST (2026-05-29T09:54:34.593Z)
    duration   1m18s

1 subagent(s)  ·  kind=all  ·  window-axis=start
```

JSON: one object per subagent (`agent_id`, `kind`, `parent_session_id`, `workflow_id`,
`agent_type`, `description`, `started_utc`, `started_local`, `completed_utc`, `completed_local`,
`duration`, `status`, `skipped_lines`).

### `whoami` — identify the calling session

```
csift whoami [--path] [--format text|json]
```

Resolves the calling Claude Code session from **`$CLAUDE_CODE_SESSION_ID`** (Claude Code exports it
into every Bash-tool environment; its value equals the calling session's own jsonl basename
exactly). That is the ONLY signal csift trusts — per-session, version-independent, zero
false-positives. `CODEX_COMPANION_SESSION_ID` is accepted only as a fallback. When the var is
absent/empty (an old CC build, or running outside CC) whoami **does NOT guess** — most-recent-mtime
is a false-positive trap with concurrent sessions — it exits non-zero with guidance to pass
`--session <uuid>`. `--path` prints the resolved jsonl path; `--format json` emits
`{"session_id":"…","path":"…"}`.

```bash
csift whoami                  # print the calling session's uuid (+ its jsonl path if found)
csift whoami --path           # always show the resolved jsonl path (or a not-found note)
csift whoami --format json
```

If `whoami` errors that `CLAUDE_CODE_SESSION_ID is not set`, install the remediation hook in
"Integration recipes → (B)".

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
The genuine-user classification is load-bearing and excludes tool_result-carriers, `isMeta`
pseudo-turns, and compaction summaries.

- **`thinking`** — assistant thinking blocks.
- **`user`** — genuine human input + user answers to AskUserQuestion (NOT tool_result-carriers).
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
# Find every time a directive was given in this session (current session id from whoami):
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
