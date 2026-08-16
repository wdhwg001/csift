<div align="center">
  <img src="assets/logo.svg" alt="csift logo" width="168" />

  <h1>csift</h1>

  <p><strong>ripgrep for Claude Code session transcripts.</strong></p>

  <p>
    Your AI coding agent writes down <em>everything</em> it does.<br/>
    <code>csift</code> is how you read it back — <strong>search</strong>, <strong>recover</strong>, and <strong>audit</strong> any Claude Code session straight from the <code>.jsonl</code> logs.
  </p>

  <p>
    <img alt="Rust 1.89+" src="https://img.shields.io/badge/Rust-1.89%2B-dea584?logo=rust&logoColor=white" />
    <img alt="search: pure regex" src="https://img.shields.io/badge/search-pure%20regex-7c9cff" />
    <img alt="embeddings: none" src="https://img.shields.io/badge/embeddings-none-22d3ee" />
    <img alt="coverage: 94.8%" src="https://img.shields.io/badge/coverage-94.8%25-4ade80" />
    <img alt="built for Claude Code" src="https://img.shields.io/badge/built%20for-Claude%20Code-d97757" />
    <img alt="license: MIT" src="https://img.shields.io/badge/license-MIT-a78bfa" />
  </p>
</div>

---

```console
# "What did that session decide about rate limiting — and where's the code?"
$ csift search "rate limit" @13d9645a -t agent --since 1d
matches  1 exchange · 1 session · oldest first

13d9645a·t42  2026-06-20 22:14:07.811 AEST(UTC+10)
  ▸ agent.message  L8821  Added a sliding-window limiter (10 req/min/IP); the 429 path now
                  returns Retry-After and logs the offending IP — gateway/src/rate_limit.rs:88.
matched 1 exchange · 1 session · label=agent
```

One regex, the **complete round-trip**, token-efficient output — no embeddings, no database, no daemon.

## Why csift

Every Claude Code session is a dense `~/.claude/projects/<cwd>/<uuid>.jsonl` — every prompt,
thought, tool call, file edit, and subagent it spawned. That log is the ground truth of what your
agent actually did… and it's also a 200 MB wall of JSON. When you (or your agent) need to know:

- *what did that session decide about X — and where's the code?*
- *which files did it touch, and did any change behind the tool stream's back?*
- *recover the file it rewrote five times — or the plan it deleted.*
- *what's the verbatim exchange a compaction summary compressed to a single line?*
- *which subagent ran what, and how did the fan-out nest?*

…`csift` answers it in one command. It's `grep` that understands the transcript: it returns the
**complete exchange** (a matched tool call *with* its result; a user turn *with* the agent's reply),
reconstructs files and plans from the Read/Write/Edit stream, and restores the verbatim turns a
context compaction clipped.

## The name

Pronounced **"c-sift"** (*see-sift*), in the `csplit`/`ctags` naming tradition: **c** for Claude
Code, **sift** for what it does — sifting gigabytes of transcript for the few lines that matter.

## ✨ Highlights

- 🔎 **Round-trips, not lines.** A hit returns the whole exchange — the matched tool call with its result, the user turn with the agent's reply — rebuilt from the `uuid`/`parentUuid` graph.
- ⏪ **Recover files & deleted plans.** Replay the Read/Write/Edit stream to restore a file's exact final bytes — or, when the session saw only part of it, fail honestly and salvage what survived with gaps marked.
- 🧵 **Un-clip a compaction.** A summary keeps task *state* but drops *turn* fidelity (~239 assistant turns → one quote). `verbatim` restores the verbatim back-and-forth within a `--budget`.
- 🌳 **Subagent topology.** Every built-in Task and workflow/OMC agent — kind, trigger/start/finish, status, and the parent→child tree (reconstructed even though they sit flat on disk).
- 🗂 **File forensics.** What each session changed and when, with edits made *outside* the tool stream flagged — the risky-to-reconstruct signal.
- ⚡ **Fast on huge logs.** mmap + SIMD newline scan + a regex prefilter, full JSON only on candidate lines; 200 MB transcripts without a full read.
- 🤖 **The user is a model.** Terse, parseable output, re-feedable `Lnnnn`/`@<uuid>` handles, errors instead of silent guesses — a zero-match `search` even *self-diagnoses* (a definitive absence, not a syntax slip, naming the label that hid your hits) so a model never bails to hand-parsing — and `@trap:<marker>`, so a subagent can identify *itself*.
- 🧩 **No magic, no index.** Pure regex — no embeddings, no BM25, no semantic search, no database, no daemon. It reads the files that are already there.

## Built for an AI consumer

Most CLIs are read by a human at a terminal. `csift`'s primary user is **the AI agent itself** — a
Claude Code session searching its own or a peer session's history. That one constraint shapes
everything: output is terse and parseable, every record carries a re-feedable `Lnnnn` locator and
`@<uuid>` handle, ambiguity is an explicit error rather than a silent guess, and a running session
can even ask *"which subagent am I?"* with `whoami @trap:<marker>`. It's the rare tool whose UX is tuned
for a model, not a person.

## The summary is a selection. csift keeps the conversation.

When Claude Code compacts, it regenerates a dense summary — kilobytes standing in for dozens of
compactions of history. It's a *good* selection: it keeps the key findings (what changed, with
file:line) and often your standing directives verbatim. But it is a selection, re-abstracted every
time, and the axis it optimizes is **task continuation** — not the conversation.

`csift verbatim` keeps the other axis: the verbatim **User↔Agent exchange** — what you actually said,
and what the agent actually reported back when it finished — with the hundreds of tool calls in
between collapsed to a count. For the recent window that's tens of KB of full-fidelity dialogue the
summary compressed to a line or two: not just your directive, but the agent's *"validated against
HEAD — the premise holds, here's the evidence"* report-back, the kind of thing a task-findings
selection simply doesn't carry.

So it doesn't replace the summary — it's **orthogonal** to it, and it **extends** the
post-compaction context with the recent conversation at full fidelity. It's budget-bounded and
newest-first (it won't reach the oldest turns, and your never-compacted Plan file still owns the
plan), but filling the dialogue the summary abstracted away is exactly the point: *better, not
complete.* Wire it into a `SessionStart(compact)` hook — see [SKILL.md](SKILL.md) — and every
compaction arrives with the recent verbatim conversation attached.

## Install

```bash
cargo install csift
```

Requires **Rust 1.89+**. Or from source:

```bash
git clone https://github.com/wdhwg001/csift.git
cd csift
cargo install --path .        # builds the optimized binary and puts `csift` on your PATH
# …or just `cargo build --release` → ./target/release/csift
```

`csift` reads `~/.claude` by default; point it elsewhere with `--claude-home <DIR>` or Claude
Code's own `$CLAUDE_CONFIG_DIR`. Then:

```bash
csift --help
```

## Quickstart

`csift <command> [TARGET] [flags]`. A **target** is a positional `@<uuid>` (a session), an
`@<agent-hex>` (a subagent), a project path, or `.` (this cwd) — omit it to scan every project.

| You want to… | Run |
|---|---|
| find what a session said about a topic | `csift search "TOPIC" @<uuid>` |
| fetch the exact record a hit cited (full / raw bytes) | `csift show @<uuid> --line 46550 [--raw]` |
| peek a live session's last few turns | `csift show @<uuid> --turn -3..` |
| token / tool / turn aggregates | `csift stats @<uuid>` |
| identify "which session is this" | `csift list .` |
| see which files a session changed | `csift files @<uuid> --by file` |
| restore a file from the transcript | `csift recover @<uuid> --file /abs/x.rs --out /abs/x.rs` |
| get back a deleted plan | `csift recover @<uuid> --file @plan --out plan.md` |
| un-clip the turns a compaction dropped | `csift verbatim @<uuid> --budget 40000` |
| inspect a session's subagents | `csift agents @<uuid>` |
| identify the current session | `csift whoami` |
| pull a pasted image back out | `csift image @<uuid> --out ./imgs` |
| run the NEXT command over exactly what matched | `csift search "X" -l \| csift stats --sessions-from -` |

Run `csift <command> --help` for the full flag set and examples.

## The eleven subcommands

| | |
|---|---|
| **`list`** | fast "which session is this?" index — first/last user + last agent, per session |
| **`search`** | regex over transcripts → the complete round-trip per hit (`-t`/`-T` label filters, `-l` matching sessions, `--raw` verbatim lines) |
| **`show`** | fetch the exact record(s) you name — `--line N\|A..B` / `--uuid U` / `--turn N\|A..B\|-k` (the `·tN` turn index from search's headers) of one transcript, rendered full or `--raw` bytes |
| **`stats`** | one-scan aggregates per session: tokens by model, tool calls, turns, span, compactions |
| **`agents`** | a session's subagents: kind, lifecycle, status, and the parent→child topology |
| **`whoami`** | identify the calling session from `$CLAUDE_CODE_SESSION_ID`, false-positive-safe |
| **`files`** | which files/dirs a session changed, when — plus edits made outside the tool stream |
| **`recover`** | reconstruct a file (or a deleted plan) from the Read/Write/Edit stream — byte-exact or honest gaps |
| **`plan`** | locate the Plan-Mode plan file bound to a session (and reverse: which session owns a plan) |
| **`verbatim`** | restore the verbatim turns a compaction summary clipped, within a budget (the live-tail peek is `show --turn`) |
| **`image`** | list + extract images pasted into a transcript (handle/locator addressing, format transcode) |

## How it works

csift never loads a whole transcript when it doesn't have to: it **mmaps** each `.jsonl`, scans
newlines with SIMD (`memchr`), runs a cheap byte/regex **prefilter**, and only `serde_json`-parses
candidate lines; `list` reads just the head and tail; `rayon` fans out across files. The hard part
isn't speed — it's the *semantics* of Claude Code's log: a `role:"user"` record is usually a tool
result, not a human turn; a pending question is never written to disk; compaction has a specific
shape; subagents sit flat on disk and their topology is reconstructed from the spawning tool call.
All of it is documented, with the empirical grounding, in **[SPEC.md](SPEC.md)**.

## Documentation

- **[SPEC.md](SPEC.md)** — the design + the justification: the record model, per-subcommand spec, and the measurements behind the deep features.
- **[SKILL.md](SKILL.md)** — dense, recipe-first usage reference (the agent-facing skill).
- **[AGENTS.md](AGENTS.md)** — repo orientation: architecture, the jsonl domain knowledge, conventions, and the quality gate.

## Contributing

Issues and PRs welcome. The entire quality gate is one pre-commit hook (installed on your first
`cargo test`): `cargo fmt --check` → `cargo clippy --all-targets -D warnings` → `cargo test`. Keep
those green and you're set. New to the code? **[AGENTS.md](AGENTS.md)** is the orientation.

## License

Released under the **[MIT](LICENSE)** license.
