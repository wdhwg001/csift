# csift

**ripgrep for Claude Code session transcripts.** A fast Rust CLI that reads, searches, and
reconstructs the JSONL session logs Claude Code writes under `~/.claude/projects/**/*.jsonl` — without
the brittleness of hand-grepping raw JSON. It understands the record model (genuine-user vs
tool_result-carrier, thinking / tool / agent categories), spans subagent transcripts, and reconstructs
whole turns rather than emitting line fragments.

The headline subcommand is [`turns`](#turns), which restores the verbatim user/assistant back-and-forth
that a Claude Code **compaction summary** clips — supplementing the summary (which owns task state) with
the turn fidelity it loses.

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
content or a dropped plan (`recover`), and a session's subagent topology (`agents`).

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

Every **session-operating** subcommand (`list`, `search`, `agents`, `files`, `recover`, `turns`) takes
optional `PATH...` targets and an optional `--session <uuid>` (a bare session-UUID in the positional
slot is routed to `--session`; a bare **subagent hex** is NOT accepted there — inspect one with `csift
agents --agent <hex>`). For `search` the first positional is the **PATTERN**, so a lone-uuid `search
<uuid>` is routed to `--session` (a one-line note reports it); pass a scope target to keep a literal
search (`csift search <uuid> .`). The subagent-span flags `--include-subagents` / `--no-subagents`
apply to the **four** transcript-spanning subcommands `list`/`search`/`files`/`recover` (default **ON**)
and `turns` (default **OFF** — a single-thread recovery tool with a per-session budget that multiplies,
so it spans subagents only on explicit `--include-subagents`). On the four default-ON subcommands
`--include-subagents` is a **no-op** (the default already spans) and `--no-subagents` is **DOMINANT** —
present, it always wins regardless of flag order; on `turns` `--include-subagents` is the load-bearing
opt-in (last-flag-wins). A `--subagents-only` flag (a `files`-only scope flag) mistyped onto a sibling
gives a pointed "that's a `files`-only flag" error. Every spanning subcommand discloses a subagent
fan-out the SAME way: a leading `SCOPE  N sessions in scope (X top-level + Y subagent)` text banner and
a leading `{kind:"session_header", sessions_in_scope, top_level_sessions, subagent_sessions}` JSON
record (both suppressed under `--no-subagents` / a single-transcript scope). `agents` has **no**
subagent-span flag: it *discovers* a session's subagents as its primary output, so there is nothing to
span over (passing `--no-subagents`/`--include-subagents` errors with that note). The argv pre-pass routes declared flags
(long and the search short flags `-t`/`-i`) away from the positional, so a flag works in any position,
including trailing. `whoami` is the
exception: it takes NO target (it reads `$CLAUDE_CODE_SESSION_ID`, falling back to
`CODEX_COMPANION_SESSION_ID`) and its only flags are `--show-path` / `--format`. All support
`--format text|json`.

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
THAT a file changed; `recover` rebuilds its content. Bash mutations are parsed lexically and flagged
`(heuristic)`: the verb allowlist (incl. `ln`/`install`/`rsync`, GNU `-t DIR`, and `tar -c` → its `-f`
archive) plus fd-qualified redirects (`2>`/`1>`/`&>`, and the noclobber-override `>|`), `curl`/`wget`
output flags, and allowlisted flag outputs (`--junit-xml=`/`--report-path`/`dd of=`/`zip`) — only
concrete, resolvable paths (an unexpandable `$VAR`/`~`, a `/dev/null` sink with a glued substitution
`)`, a `>(…)` process substitution and its body args, and a quote-severed fragment are all dropped,
never fabricated). The parser is **quote/backtick/procsub/arith-aware**: a `>`/`<` inside a quoted
echo/printf or regex (`echo "idle >8min"`), inside a backtick command substitution (`` `date >f` ``),
or inside an arithmetic/test comparison (`(( a > b ))` / `[[ a > b ]]`) is masked before redirect
detection, so it never fabricates a file. A trailing **`#` shell comment** (an unquoted `#` at a word
boundary → end-of-line) is likewise masked, so `cp src dst  # x` reports `dst` (not the comment word),
an in-comment `> /x` fabricates nothing, and a real cp/mv/ln destination is never displaced by the
comment; an in-path `#` (`/tmp/a#b`) is preserved. A write inside an embedded-language
body (heredoc / `python -c`) is out of scope and missed — but **never mis-reported** (heredoc body
lines are lexically skipped and quoted/procsub spans masked before scanning). The four detail levels
**strictly coarsen**: `--summary` is a top-level-PREFIX rollup (a whole project tree → one row;
smallest output) < `--by-dir` (full parent dir) < `--by-file` < `--timeline`. Subagent scope is
mutually exclusive: default spans subagents, `--no-subagents` is the top-level session only, and
`--subagents-only` is its complement (only the files the session's subagents touched).

```bash
csift files --session <uuid>
csift files . --no-subagents
csift files <uuid> --subagents-only --by-file   # only what the session's subagents touched
```

### `recover` — reconstruct a file's content (or restore a plan)

Rebuilds a file's content by replaying its Read / Write / Edit stream in transcript order. Four
mutually-exclusive modes: `--patches` (DEFAULT, segmented unified-diff history split at integrity
boundaries), `--at WHEN` (the partial line-numbered snapshot as the LLM saw it; gaps are explicit, never
fabricated), `--coverage`/`--dry-run` (scope without dumping), and `--plan` (restore a dropped plan).
`--plan` matches plan-files by a **component-scoped** heuristic — a `plans/` directory component or a
filename stem being/carrying the token `plan` (delimited), so `sample.md` / a `widget-app` ancestor
dir do **not** match; pass `--file <abs>` to pin an exact file (its latest `Write` is then restored).
`--out` writes the artifact verbatim and is **data-safe on an empty result** (leaves the destination
UNTOUCHED, prints no false `(wrote …)` line — a stderr `note: … left untouched` fires instead); it is a
no-op in `--coverage` mode. `--line-range` (a 1-based FILE-line span of `--file`) applies in
`--patches`/`--at`/`--coverage` but is a **no-op in `--plan` mode** (a restored plan is verbatim Write
content; a stderr note flags it).

```bash
csift recover . --file /abs/PLAN.md --coverage              # scope first: covered ranges + boundaries
csift recover <uuid> --file /abs/app.py --patches           # segmented unified diffs
csift recover <uuid> --file /abs/app.py --at @turn:42       # partial snapshot as of turn 42
csift recover . --plan --out /tmp/restored-plan.md          # list plan candidates; write the latest to a file
csift recover . --plan --file /abs/PLAN.md --out /tmp/p.md  # restore THAT plan file's latest Write content
```

### `turns`

Reconstruct the verbatim user/assistant back-and-forth a compaction summary clipped — the headline
command. In automation-heavy sessions, machine-injected `<task-notification>` triggers open turns;
`turns` (and `search -t user`) label them as `[<kind> <id> <status>] <summary>` (kind =
`background-command` / `workflow` / `agent` / `monitor` / `task`, read from the summary — never the
raw XML; `monitor` matches a `<task-notification>` whose summary opens `Monitor`/`scheduled`/`cron`
OR a `Background command "…"` whose quoted command NAME carries a monitor-cadence token
(`monitor`/`re-arm`/`relaunch monitor`/`liveness`) — so a monitor loop implemented as `&`-detached
background commands is attributed to `monitor`, not disguised as generic `background-command`) and the
header reports the
human/automation split WITH a per-class breakdown (`N user (M automation triggers: 2 background-command,
1 agent)`). A monitor pulse's real outcome lives in `<event>` (often with no `<status>`), so its label
surfaces the event (`[monitor … STAGE2_OUTPUT_READY]`) rather than fabricating `completed`. **Note:**
the `monitor` class covers only these `<task-notification>` pulses; the **`ScheduleWakeup` wakeup-tick
prompts** that drive a monitor/cron *cadence* arrive as `isMeta:true` user records (not
`<task-notification>`s) and are **not yet segmented or attributed** — they currently group under the
preceding genuine-user turn. In
`--format json` the attribution is structural (`is_automation` / `trigger_kind` / `task_id` / `status` /
`event`) on the user-segment object, with a leading `{kind:"session_header",…}` object carrying the
lumped `automation_triggers` + per-class `automation_by_kind` + `sessions_in_scope` vs
`sessions_rendered`. These pulses are
excluded from the `--round-trip-fraction` human-reserved floor. See the [budget model](#budget-model)
and [richness model](#richness-model) below.

```bash
csift turns .                                               # default 40K-char recon (longest agent msg + rich members/turn)
csift turns <uuid> --budget 12000                           # a 200K-context-sized recovery
csift turns <uuid> --agent-msgs eot-only                    # force the old single-EOT (last-message-only) output
csift turns <uuid> --format json                            # machine-readable, line-numbered
csift turns . --budget 40000 --out /tmp/turns.md            # full reconstruction to a file
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
            [--out PATH] [--format text|json]
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
- a **finding/decision lexeme** (`found` / `confirmed` / `verified` / `root cause` / `DEFER` / `fix` /
  `x` / `x` / `x` / `x` / `x` / …).

**KEEP-ON-DOUBT is the spine.** A message is COLLAPSED only when it is a *proven* pure declaration: short
(`< --agent-declaration-max-chars`, default 200), signal-less, AND opening with an intent verb
(`let me …` / `now I …` / `x…` / `x…`). Anything uncertain is kept — a wrongly-kept declaration
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
- **`--out PATH`** writes the full (un-terminal-truncated) reconstruction verbatim to a file while the
  summary still prints to stdout. **Data-safe on an empty result:** when nothing renders within budget
  the destination is left UNTOUCHED (never truncated to 0 bytes) and no false `(wrote …)` line is
  printed — a stderr `note: … left untouched` fires instead (the same guard `recover --patches`/`--at`
  apply on a no-history result).

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
  recover.rs       # file-content + plan reconstruction
  turns.rs         # turn-fidelity reconstruction (budget + richness model)
  turns/tests.rs   # turns unit tests
tests/
  cli_integration.rs  # end-to-end tests against the compiled binary
```

Design docs: `SPEC.md` (the full record model + subcommand specs), `TURN_FIDELITY_DESIGN.md` (the
`turns` budget + richness design), `SKILL.md` (the agent-facing usage skill).

## License

MIT.
