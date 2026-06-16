# csift

**ripgrep for Claude Code session transcripts.** A fast Rust CLI that reads, searches, and
reconstructs the JSONL session logs Claude Code writes under `~/.claude/projects/**/*.jsonl` — without
the brittleness of hand-grepping raw JSON. It understands the record model (genuine-user vs
tool_result-carrier, thinking / tool / agent categories), spans subagent transcripts, and reconstructs
whole turns rather than emitting line fragments.

The headline subcommand is [`turns`](#turns), which restores the verbatim user/assistant back-and-forth
that a Claude Code **compaction summary** clips — supplementing the summary (which owns task state) with
the turn fidelity it loses.

The default corpus root is `~/.claude`, but a relocated config dir is honored on **every** subcommand:
the global `--claude-home <DIR>` flag and Claude Code's own `$CLAUDE_CONFIG_DIR` env var both repoint it
(precedence: flag → `$CLAUDE_CONFIG_DIR` → `$HOME/.claude`). `<DIR>` is the `.claude` equivalent, so
transcripts are read from `<DIR>/projects/<encoded>/*.jsonl`.

---

## Why

A Claude Code compaction summary preserves task STATE (intent, file ledger, errors+fixes, plan, next
step) in high fidelity, but provably LOSES turn fidelity: its "All user messages" section truncates real
prose turns to `...`-clipped bullets, and the assistant side collapses to a single quote. When you
resume a compacted session, the verbatim back-and-forth is gone. csift reads the lossless JSONL the
summary was derived from and gives it back — every line carrying the jsonl line number so you can `Read`
the raw record.

It also does the bread-and-butter transcript work: which session is this (`whoami` / `list`), find every
exchange matching a regex (`search`), what files a session touched (`files`), reconstruct a file's
content — or a dropped plan, via `recover --file @plan` (`recover`), locate the plan file bound to a
session (`plan`), and a session's subagent topology (`agents`).

---

## Install / build

Requires Rust **1.89+**.

```bash
git clone <repo> csift && cd csift
cargo build --release          # optimized binary at target/release/csift
./target/release/csift --help

# or install into ~/.cargo/bin
cargo install --path .
```

On the first `cargo test` / `cargo build` a git pre-commit hook is installed (via `cargo-husky`) that
runs `cargo fmt --check` → `cargo clippy -D warnings` → `cargo test` before each commit.

```bash
cargo test --release           # full unit + integration suite
cargo clippy --release --all-targets -- -D warnings
cargo fmt --all -- --check
```

---

## Concepts (the data it reads)

```
~/.claude/projects/<ENCODED_CWD>/<session-uuid>.jsonl                                   # a session transcript
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/agent-<hex>.jsonl                 # (A) built-in Task/Agent subagent
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/workflows/wf_*/agent-<hex>.jsonl  # (B) workflow / OMC subagent
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/workflows/wf_*/journal.jsonl      # (C) workflow EVENT log (NOT a transcript)
```

- **`<ENCODED_CWD>`** — the project directory name is the cwd with `/` → `-` (and a few other rules).
  Most commands accept either a real path (encoded for you) or an already-encoded `-Users-…` token; with
  no PATH, every project is scanned.
- **genuine-user** — a real human message, as opposed to a `tool_result` carrier, an `isMeta`
  pseudo-turn (`"Continue from where you left off."`), a compaction summary, or a machine-synthesized
  marker (an interrupt `[Request interrupted by user]`, a `<local-command-stdout>…` output, a
  `<command-name>…` slash-command wrapper). A turn boundary ALSO opens on two genuine-user messages
  that ride INSIDE a tool round-trip: an **answered AskUserQuestion** (the reconstructed
  Q+options+answer unit IS the user's message) and a **tool-use rejection carrying a typed message**
  (an ExitPlanMode plan kick-back + a `[plan: …]` pointer) — both were previously MISSED. This
  distinction is load-bearing for turn segmentation.
- **subagents** — discovered by on-disk path location (directly under `subagents/` ⇒ built-in; under
  `subagents/workflows/wf_*/` ⇒ workflow). The `journal.jsonl` is an event log, never a transcript.

Performance: mmap + a `memchr` byte prefilter scan (full scans), seek-from-EOF tail reads (fast `list`
identity), lazy `serde_json` only on candidate lines, and rayon parallelism ACROSS files.

---

## Subcommands

A few conventions are shared across subcommands:

- **Targets.** Every session-operating subcommand (`list`, `search`, `agents`, `files`, `recover`,
  `plan`, `turns`) takes optional `PATH...` targets (a real cwd or an encoded `-Users-…` dir) and an
  optional `--session <uuid>`; a bare session-UUID in the positional slot routes to `--session`. A bare
  **subagent hex** is not a valid target — inspect one with `csift agents --agent <hex>`. `whoami` takes
  no target (it reads `$CLAUDE_CODE_SESSION_ID`); `plan` falls back to that same env var when given none.
- **`search` puts PATTERN first**, so a lone `search <uuid>` routes the uuid to `--session` (a note
  reports it); add a scope target to keep it a literal search: `csift search <uuid> .`.
- **Subagent span.** `list` / `search` / `files` / `recover` span each session's subagent transcripts by
  **default**; `--no-subagents` is dominant (wins in any flag position) and `--include-subagents` is then
  a no-op. `turns` instead defaults to the **top-level thread only** (its per-session budget multiplies
  across fan-out), so there `--include-subagents` is the opt-in. `files` also accepts `--subagents-only`
  (the complement); `agents` has no span flag — discovering a session's subagents *is* its job.
- **Flags work in any position** — a pre-pass routes declared flags (long, plus search's `-t`/`-i`) away
  from the leading-`-` encoded target, so a trailing flag is never swallowed. All support
  `--format text|json`.
- **Fan-out is disclosed.** A run that spans more than one transcript prints a leading
  `scope  N sessions in scope (X top-level + Y subagent)` banner (and a `{kind:"session_header", …}` JSON
  record), suppressed under `--no-subagents` or a single-transcript scope.

### `list` — session identity index

A fast quick-identity tuple per session WITHOUT parsing the whole file: session id, the first + last
genuine-user message (+ times), the last agent message (+ time), decoded cwd / git branch / CC version.
Forward HEAD read finds the first user; backward TAIL read finds the last — neither parses the full
file, so it stays fast on 200 MB+ transcripts.

```bash
csift list                                                  # every session, all projects
csift list .                                                # sessions for the current cwd's project
csift list /Users/me/Projects/foo                           # a real path (gets encoded)
csift list <uuid>                                           # identify ONE session (bare-uuid positional, like its siblings)
csift list --session <uuid>                                 # same, via the explicit flag
```

### `search` — regex over transcripts, complete round-trip per hit

Linear-time (RE2-class) regex over transcript content. On a hit it returns the WHOLE turn (a turn opens
on a genuine user message, an answered AskUserQuestion, or a plan-rejection-with-message), not a line
fragment. `-t/--category` (repeatable) narrows to `thinking` / `user` / `tool` / `tool-response` /
`agent`. The `user` category reconstructs the full AskUserQuestion Q+options+answer unit and surfaces a
plan-rejection's typed message with a `[plan: …]` pointer.

```bash
csift search "carry logic" .                                # this project (positional PATH, like every sibling)
csift search "" -t user --since 2h .                        # user turns in the last 2h
csift search "panic" --session <uuid> --max-count 5
```

### `whoami` — identify the calling session (false-positive-safe)

Resolves the current Claude Code session id from `$CLAUDE_CODE_SESSION_ID` (the primary trusted signal
— per-session, version-independent, survives bash), falling back to `CODEX_COMPANION_SESSION_ID` (the
Codex companion plugin's alias) when the canonical var is absent. Use it to anchor the other
subcommands to "this session". The resolved `path` line is ALREADY printed by default whenever it
resolves; `--show-path` (boolean; legacy alias `--path`) only FORCES a `path` line in the unresolved
case (printing `path <not found …>` instead of omitting it).

> **Subagent caveat:** inside a Task/Agent subagent, `$CLAUDE_CODE_SESSION_ID` is the SUBAGENT's own
> id, not the parent/root session — run `agents`/`list` on the project path to find the parent uuid.
> whoami JSON intentionally carries only `{session_id, path}` — it does NOT include
> `is_subagent`/`parent_session_id` (unlike the other surfaces), so to learn whether the resolved id is
> a subagent + find its parent, feed it to `csift agents --agent <id> --format json` and read
> `parent_session_id`.

```bash
csift whoami
csift whoami --show-path
```

### `agents` — a session's subagent TOPOLOGY

Builds the **toolUseId-linked topology** of the subagents a session spawned — not a flat list of
detached transcript files. Each built-in subagent joins back to the parent `Agent`/`Task` `tool_use`
that triggered it (`meta.json`'s `toolUseId` → the parent transcript's spawning `tool_use`), so each
node carries: kind (built-in / workflow), the **TRUE trigger time** (the parent tool_use ts — not the
child-head ts, which lags it by 0.2–4.7 s), start + completion, duration, a resolved status
(completed / running / unknown — never over-claiming "failed"), and — on demand — its returned message
and files-changed.

The returned message is resolved **three ways**: a **sync** built-in → the parent `tool_result` text;
an **async** built-in (the parent result is the `Async agent launched …` sentinel) → the child
transcript tail; a **workflow** agent → its `journal.jsonl` `result` payload. `--tree` renders the
parent→child tree with **WorkflowRun** nodes (read from the top-level `workflows/wf_*.json` manifests)
as the parents of their workflow agents. A subagent transcript's bare-hex id is NOT a re-feedable
`--session` target, so every JSON surface that emits a per-transcript identity (`list` / `search` /
`files` / `recover` / `turns`) carries the discriminators `is_subagent` + the re-feedable
`parent_session_id` (the owning uuid; `agents` itself keys on `agent_id` + `parent_session_id`). So a
file mutation — or any per-transcript record — joins back to its node by structured field on every
surface, and ALL the matching text views (`list` / `search` / `files` grouped / `turns`) brand a
subagent block uniformly `SUBAGENT <hex> · parent SESSION <uuid>` — the bare subagent hex is never
tokened `SESSION`.

```bash
csift agents --session <uuid>                         # the topology (flat list; trigger-ordered)
csift agents --session <uuid> --since 2h --by trigger # subagents TRIGGERED in the last 2h (default axis)
csift agents --session <uuid> --tree                  # parent→child tree (workflow runs as parents)
csift agents --session <uuid> --agent <hex> --with-files   # grab one node: its returned msg + files
csift agents --session <uuid> --returned-message --format json   # every node's returned message
```

### `files` — which files/dirs a session modified, when

The set of files a session Read / Wrote / Edited (+ Bash mutations), with timestamps. `files` reports
THAT a file changed; `recover` rebuilds its content. `Edit` / `Write` / `MultiEdit` mutations are
**authoritative** (create-vs-edit comes from the tool result); **Bash** mutations are a best-effort
lexical parse — quote/backtick/procsub/arith/comment-aware, so a `>` inside a quoted string, a comment,
or an arithmetic test never fabricates a path — and are always flagged `(heuristic)`. A write buried in
a heredoc or `python -c` body is out of scope (missed, never mis-reported). The full Bash verb/redirect
allowlist lives in [`SPEC.md`](./SPEC.md) §6.6.

The four detail levels **strictly coarsen**: `--summary` (the default — a top-level-prefix rollup, so a
whole project tree collapses to one row; smallest output) < `--by-dir` < `--by-file` < `--timeline`.
Subagent scope is mutually exclusive: the default spans subagents, `--no-subagents` is the top-level
session only, and `--subagents-only` is its complement (only what the session's subagents touched).

```bash
csift files --session <uuid>
csift files . --no-subagents
csift files <uuid> --subagents-only --by-file   # only what the session's subagents touched
```

### `recover` — reconstruct a file's content (and `plan` — locate a session's plan)

Rebuilds a file's content by replaying its Read / Write / Edit stream in transcript order. Three
mutually-exclusive modes: `--patches` (DEFAULT, segmented unified-diff history split at integrity
boundaries), `--at WHEN` (the partial line-numbered snapshot as the LLM saw it; gaps are explicit, never
fabricated), and `--coverage`/`--dry-run` (scope without dumping). `--file` is **required** for all
three. An Edit/Write whose result ERRORED (`is_error:true`) never mutated the file, so reconstruction
does not apply it and coverage does not count it — this covers both `String to replace not found in file`
and the Edit-before-Read wall (`File has not been read yet`, e.g. a file created with Bash then directly
Edited, since Bash/Grep don't satisfy CC's Read-before-Edit gate); a not-read-yet case is surfaced as an
integrity annotation, not an edit.

**`--file @plan`** is a magic `--file` VALUE (not a mode): bash-safe (no shell metacharacters, like
`--at`'s `@line:`/`@turn:` sigils), it resolves the session-BOUND plan file (see `plan` below) and
reconstructs THAT file exactly like any other — its FULL Write+Edit history, edit-aware (not just the
latest Write). It composes with every mode and with `--out`/`--format`, so it is how you DUMP a plan's
content — including a DELETED plan rebuilt from the transcript alone. It prefers the top-level session's
own plan and ERRORS clearly (never guesses) when no plan is bound to the target, or when the target spans
sessions bound to DIFFERENT plans (asks for `--session`).

`--out` writes the artifact verbatim and is **data-safe on an empty result** (leaves the destination
UNTOUCHED, prints no false `(wrote …)` line — a stderr `note: … left untouched` fires instead); it is a
no-op in `--coverage` mode. `--line-range` (a 1-based FILE-line span of `--file`) applies in all three
modes. A SUBAGENT transcript surfaces as `SUBAGENT <hex> · parent SESSION <uuid>` in recover text (never
a bare-hex `SESSION`).

```bash
csift recover . --file /abs/PLAN.md --coverage                 # scope first: covered ranges + boundaries
csift recover <uuid> --file /abs/app.py --patches              # segmented unified diffs
csift recover <uuid> --file /abs/app.py --at @turn:42          # partial snapshot as of turn 42
csift recover <uuid> --file @plan --out /tmp/restored-plan.md  # rebuild the session's bound plan (DELETED ok)
```

The companion **`plan`** subcommand LOCATES (does not dump) the plan file a session is bound to. The
binding is AUTHORITATIVE: Claude Code writes a `plan_mode` attachment record
(`{"type":"attachment","attachment":{"type":"plan_mode","planFilePath":"…","isSubAgent":…,"planExists":…}}`)
when a session enters Plan Mode, and that `planFilePath` IS the bound plan. It is NOT a path heuristic — a
session may Edit/Write OTHER sessions' plan files (ordinary tool calls on a `~/.claude/plans/…` path), and
those are not its own. Plans live flat under `~/.claude/plans/` with a random three-word name
(`nested-prancing-popcorn.md`; a subagent's gets an `-agent-<hex>` suffix) — the name is NOT derivable
from the session id, only the attachment binds them. Target a project PATH / encoded-dir / bare
session-UUID (positional) or `--session <uuid>`; with no target it resolves the CALLING session from
`CLAUDE_CODE_SESSION_ID` (like `whoami`). It spans subagents by default (their own plans surface, flagged
subagent with the re-feedable parent uuid); `--no-subagents` restricts to the top-level session. Per
resolved session it emits `session_id`, `is_subagent`, `parent_session_id`, `plan_file`, `plan_exists`
(on disk), `line_no`; text or `--format json` (NDJSON, one object per plan).

```bash
csift plan                                  # the calling session's bound plan (resolves CLAUDE_CODE_SESSION_ID)
csift plan <uuid>                           # a specific session's bound plan
csift plan . --no-subagents --format json   # this project's top-level sessions, machine-readable
csift recover --session <uuid> --file @plan # DUMP the plan's content (plan only LOCATES it)
```

### `turns`

Reconstruct the verbatim user/assistant back-and-forth a compaction summary clipped — the headline
command. In automation-heavy sessions, machine-injected `<task-notification>` triggers also open turns;
`turns` (and `search -t user`) label them `[<kind> <id> <status>] <summary>` (kind =
`background-command` / `workflow` / `agent` / `monitor` / `task`, read from the notification summary,
never the raw XML), and the header reports the human-vs-automation split with a per-class breakdown
(`N user (M automation triggers: 2 background-command, 1 agent)`). A monitor pulse's outcome is taken
from its `<event>` rather than fabricating a `completed` status. Automation triggers are excluded from
the `--round-trip-fraction` human floor, and `--format json` carries the attribution structurally
(`is_automation` / `trigger_kind` / `task_id` / `status` / `event`). One gap worth knowing: the
`ScheduleWakeup` wakeup-tick prompts that drive a monitor/cron *cadence* arrive as `isMeta:true` records
(not `<task-notification>`s) and are **not yet** segmented — they group under the preceding genuine-user
turn. See the [budget model](#budget-model) and [richness model](#richness-model) below.

```bash
csift turns .                                               # default 40K-char recon (longest agent msg + rich members/turn)
csift turns <uuid> --budget 12000                           # a 200K-context-sized recovery
csift turns <uuid> --agent-msgs eot-only                    # force the old single-EOT (last-message-only) output
csift turns <uuid> --format json                            # machine-readable, line-numbered
csift turns . --budget 40000 --out /tmp/turns.md            # full reconstruction to a file
csift turns . --budget 36000 --window 9000 --slice 1        # 1st ≤9000-char chunk for a SessionStart hook (slices 1–4 fan 36K)
```

---

## `turns` in depth

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

### Budget model

- **`--budget`** (default 40000) bounds each session's reconstruction in chars, or tokens via
  `--budget-unit tokens` (≈4 chars/token). NOTE: the default `40000` is read as 40000 TOKENS ≈ 160000
  chars the moment you pass `--budget-unit tokens` without also lowering `--budget` (a 4× larger
  output) — pass an explicit `--budget` when flipping to tokens. The JSON `budget_chars`/`max_total_chars`
  are ALWAYS in CHARS (a token budget is pre-multiplied ×4). It is applied **PER session in scope**. UNLIKE
  `files`/`search`, `turns` defaults to the **top-level thread only** — a single-thread recovery tool
  whose per-session budget MULTIPLIES, so spanning hundreds of fan-out subagents by default would bury
  the thread you asked to restore. A bare `turns <uuid>` reconstructs just that conversation at `budget`
  chars; add `--include-subagents` for the rare cross-fan-out reconstruction, where the realized total
  is `budget × (sessions in scope)` and a top-of-output `SCOPE` banner names the TRUE scope (all
  top-level + subagent sessions discovered), how many rendered within budget, and the multiplier. A
  targeted top-level session that does not fit the budget is reported with an explicit `SESSION <uuid>
  skipped — its first round-trip needs ≥ N chars` note, never silently dropped. The summed cost of every
  emitted line equals the real emitted length — the per-session budget is never overshot.
- **Selection is recency-first** (most-recent turns win the budget, what a resumed agent most needs);
  the emitted document is sorted ascending so it reads as a forward transcript.
- **`--round-trip-fraction`** (default 0.5) is a HARD FLOOR: that fraction of the budget can only be
  spent on COMPLETE round-trips (user → `[N tool calls]` → assistant), never on user-only /
  assistant-only fragments. Without it an assistant-heavy tail recovers zero human turns.
- **Over-cap units are middle-truncated** (head+tail kept) with an explicit
  `… [+K chars, L lines elided] …` marker; the assistant head is larger than the user head (its prose
  front-loads context, back-loads the decision). Nothing is ever fabricated or silently dropped.
- **Spanning** — the backward walk is transparent to compaction boundaries (a summary is a turn member,
  never a delimiter), so a 40K budget reaches across multiple boundaries by default;
  `--max-compactions N` caps the reach. Each crossed boundary renders a
  `══ compaction boundary · summary at L… ══` banner.
- **Dedup** — a turn the NEWEST summary already quotes verbatim is flagged `(also in summary)` and
  DEMOTED (selected after non-dup turns), never silently dropped.

### Richness model

A single genuine-user turn can own a LONG run of agent messages — a debugging/build chain the model
narrates step by step — that the summary clips to its single quote. `--agent-msgs` decides how much of
that run to restore.

**Why the default keeps the LONGEST, not the LAST.** The summary's single assistant quote is the turn's
LAST message, which is frequently a ~50-char throwaway wrap-up ("Done.", "Let me know if you want X")
while the SUBSTANTIVE Rich Response — the actual finding, the committed answer — sits in a MIDDLE
message. The pre-feature default kept `agents.last()` and so silently DROPPED the substance of exactly
those turns. The default now keeps the LONGEST agent message (by char count, the best one-message proxy
for "where the substance is"), and because more than one message often matters, ALSO a substantive first
and the rich middles.

| Mode | Behavior |
| --- | --- |
| `longest` | **DEFAULT.** Keep the LONGEST agent message (the substantive Rich Response, often a middle) + the FIRST when substantive (`≥ --agent-rich-min-chars`) + each RICH middle; collapse the rest — including a short non-rich throwaway last — into a placeholder. Applies to every multi-message turn. |
| `eot-only` | **Force last-only.** Keep ONLY each turn's last agent message — byte-identical to the pre-feature single-EOT output. |
| `rich` | Keep the last always + the first by position privilege + each non-droppable middle, collapsing pure declarations into a placeholder. Only fires on a run longer than `--agent-run-threshold` (default 6); shorter runs keep every message. |
| `all` | Keep every agent message — maximal fidelity, no collapse. |

**The `longest` keep-set.** The LONGEST message is always kept (on a tie the LAST maximum wins, so an
all-equal run matches the old `agents.last()` pick); the FIRST is kept when `≥ --agent-rich-min-chars`
(a short "let me look" opener collapses); each RICH middle is kept; the LAST is kept only when itself
rich/substantive (a throwaway wrap-up collapses); everything else collapses. `--agent-rich-min-chars`
(default 280) is the tuning knob — in `longest` it gates both the substantive-first decision and the
rich length arm.

In `rich` (and the `longest` rich-middle test) a message is **"rich"** — a cheap single-pass OR of a
length gate (kept on length alone when ≥ `--agent-rich-min-chars`, default 280) and a signal test:

- a **number-of-substance** — a count adjacent to a substance noun (`12 passed 3 failed`), or an `N/M` /
  `N of M` ratio;
- a **commit-hash-like hex** (a 7–40 char `[0-9a-f]` run with at least one a–f letter);
- a **file-and-line ref** (`turns.rs:402`, a `src/…` path);
- a **backtick code path** (`` `agents` ``);
- a **finding/decision lexeme** (`found` / `confirmed` / `verified` / `root cause` / `DEFER` / `fix` / …).

**KEEP-ON-DOUBT is the spine.** A message is COLLAPSED only when it is a *proven* pure declaration: short
(`< --agent-declaration-max-chars`, default 200), signal-less, AND opening with an intent verb
(`let me …` / `now I …`). Anything uncertain is kept — a wrongly-kept declaration
costs at most one capped body, while a wrongly-dropped finding is unrecoverable. A FUSED
finding+declaration body trips a signal → kept WHOLE; its trailing declaration is shed only by the
within-message char-ellipsis, never by whole-message drop.

A contiguous collapsed run renders as one placeholder line carrying the fetchable jsonl range + the
per-message attribution:

```
△ L412–L437  [4 agent messages, 9 tool calls, 1 failed]
```

(`X` collapsed agent messages, `Y` tool calls owned by the span, `Z` erroring tool results — `Z` omitted
when 0, `Y` always shown). A single-message span renders `L412` (no dash).

**Flags.** `--keep-first` (default) keeps a turn's first agent message by position privilege in `rich`
mode (the opening message often states the plan); `--no-keep-first` decides it as a middle. In the
default `longest` mode `--keep-first` has no effect — the first is gated on length (`≥
--agent-rich-min-chars`) instead. `--profile heavy` (threshold 4, rich-min 200, declaration-max 140) and
`--profile light` (threshold 8, rich-min 360, declaration-max 240) bundle the thresholds — applied
before the individual flags, so an explicit flag overrides the profile. A profile keeps the master
`--agent-msgs` mode as-is (so `--profile heavy` alone runs `longest` with heavier thresholds).

**Picking heavy vs light:** read the compaction summary you are supplementing. Pick `heavy` when its
errors/decisions narrative is THIN (you need the debugging back-and-forth restored); pick `light` when
it is already rich (restore the user phrasings + the substance, skip the intermediate chatter); use
`--agent-msgs eot-only` for the smallest, last-message-only supplement.

Subagent transcripts (spanned only on explicit `--include-subagents` for `turns`) get the SAME richness
treatment via the shared code path when spanned.

### Output

- **Text** (default) — a `SESSION` header block (budget / spanned boundaries / selected counts / dedup),
  then the ascending turn-by-turn document: `▽ L… USER (ts)` / `△ L… ASSISTANT (ts)` headers, bodies
  (verbatim or middle-truncated), `[N tool calls]` markers, boundary banners, and collapsed-agent
  placeholders.
- **JSON** (`--format json`) — one VERBATIM (un-truncated) object per emitted unit, interleaved
  `compaction_boundary` records, and a `collapsed_agents` placeholder record (`agent_messages` /
  `tool_calls` / `failed` / `first_line` / `last_line`) per collapsed span.
- **`--out PATH`** captures the **same** rendered reconstruction that prints to stdout into a file
  (byte-identical — `turns` does not line-truncate stdout, so over-cap units are middle-truncated with a
  `… [+K chars, L lines elided] …` marker in BOTH; for **un**-truncated unit bodies use `--format json`,
  whose `text` is verbatim). The summary still prints to stdout. **Data-safe on an empty result:** when
  nothing renders within budget
  the destination is left UNTOUCHED (never truncated to 0 bytes) and no false `(wrote …)` line is
  printed — a stderr `note: … left untouched` fires instead (the same guard `recover --patches`/`--at`
  apply on a no-history result).

---

### `image` — get a sent image back out of a transcript

A pasted/attached image (or a tool screenshot) is stored INLINE on a record as a base64 image block, so
it is recoverable straight from the JSONL — no hand-parsing. `image` lists them by a stable id
`L<line>i<n>` (the carrying record's JSONL line + the 1-based ordinal of the image within it, the same
`Lnnnnn` line refs `turns`/`search` print), and `--out <dir>` decodes each one back to a real file
(`<session-short>-L<line>i<n>.<ext>`, extension from the media type). Default is to LIST; spans subagents
by default (a screenshot may be in one). A `url`-source image is reported, never fabricated.

```bash
csift image <uuid>                                # list every image: id · media-type · ~size · time
csift image <uuid> --out /tmp/imgs                # extract ALL images to a dir (real bytes)
csift image <uuid> --no-subagents --id L6812i2 --out /tmp/imgs   # extract one by id
csift image . --format json                       # one object per image + a trailing summary
```

---

## Project layout

```
src/
  main.rs          # entry point
  cli.rs           # clap surface (all subcommand args)
  parse.rs         # mmap + line-numbered scan, head/tail reads, lazy serde_json
  model.rs         # Record / Block / Content model + turn segmentation
  path.rs          # cwd ↔ encoded-dir-name rules, session resolution
  search.rs        # regex search → round-trip exchanges
  session.rs       # list (fast identity index)
  whoami.rs        # calling-session id resolution
  subagent.rs      # subagent discovery + lifecycle
  files.rs         # files/dirs a session modified
  recover.rs       # file-content reconstruction (+ @plan resolution lives in plan.rs)
  plan.rs          # plan-file binding resolver + `plan` subcommand
  turns.rs         # turn-fidelity reconstruction (budget + richness model)
  turns/tests.rs   # turns unit tests
  image.rs         # list + extract inline base64 images (stable L<line>i<n> id)
tests/
  cli_integration.rs  # end-to-end tests against the compiled binary
```

Design docs: [`SPEC.md`](./SPEC.md) — the full record model, the per-subcommand spec, the performance
contract, and (§11) the design rationale + empirical grounding for `recover` / `turns` / `agents` (the
former `RECOVERY_DESIGN` / `TURN_FIDELITY_DESIGN` / `TOPOLOGY_DESIGN` docs are folded in there).
[`SKILL.md`](./SKILL.md) is the agent-facing usage skill; [`AGENTS.md`](./AGENTS.md) is how to work in
the repo.

## License

MIT.
