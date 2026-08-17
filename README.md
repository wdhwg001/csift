<div align="center">
  <img src="assets/logo.svg" alt="csift logo" width="168" />

  <h1>csift</h1>

  <p><sub>Pronounced <strong>"c-sift"</strong> (<em>see-sift</em>)</sub></p>

  <p><strong>The missing tool for Claude Code session transcripts.</strong></p>

  <p>
    <strong>search</strong>, <strong>recover</strong>, and <strong>audit</strong> any Claude Code session straight from the <code>.jsonl</code> logs.
  </p>

  <p>
    <img alt="Rust 1.89+" src="https://img.shields.io/badge/Rust-1.89%2B-dea584?logo=rust&logoColor=white" />
    <img alt="search: pure regex" src="https://img.shields.io/badge/search-pure%20regex-7c9cff" />
    <img alt="embeddings: none" src="https://img.shields.io/badge/embeddings-none-22d3ee" />
    <img alt="coverage: 94.8%" src="https://img.shields.io/badge/coverage-94.9%25-4ade80" />
    <img alt="built for Claude Code" src="https://img.shields.io/badge/built%20for-Claude%20Code-d97757" />
    <img alt="license: MIT" src="https://img.shields.io/badge/license-MIT-a78bfa" />
  </p>
</div>

---

```console
❯ "what did that session decide about rate limiting and where's the code?"

⏺ Bash(csift search "rate limit" @13d9645a -t agent --since 1d)
  matches  1 exchange · 1 session · oldest first

  13d9645a·t42  2026-06-20 22:14:07.811 AEST(UTC+10)
    ▸ agent.message  L8821  Added a sliding-window limiter (10 req/min/IP); the 429 path now
                    returns Retry-After and logs the offending IP — gateway/src/rate_limit.rs:88.
  matched 1 exchange · 1 session · label=agent
```

One regex, the **complete round-trip**, token-efficient output. No embeddings, no database, no daemon.

## Why csift

> Claude Code's sessions are **plain text** in JSONL, so why csift, not grep?

**Yes.** It has every prompt, thought, tool call, file edit, pasted image, and subagent it spawned. But one of these may happen to you:

- **"It deleted my project files, then apologized."**

  _Running `--dangerously-skip-permissions`, it wiped the files it had built, said sorry, and stopped dead. Asked to dig them back out of its own session, it fumbled around (the edits spanned subagents, every restore came back partial) and then quietly started **rewriting** them from memory as it panicked and decided that was "easier"._

- **"It lost my design images after a compaction, then asked me to re-send them."**

  _The images were right there in the conversation. One compaction later the agent acts like they never existed: the compaction handed it lossy text fragments instead, and it believed them._

- **"Which session does `jazzy-twilight-sparkle.md` belong to?"**

  _The plan file carries the architecture that matters. Its invented name says nothing about which session wrote it, and the binding lives in an `EnterPlanMode` tool call, a needle in the haystack._

- **"The machine crashed with three sessions open. Which was which?"**

  _Frontend, backend, debugging, and `claude --resume` gives you a lineup with no faces._

- **"I literally just said that."**

  _A few compactions into a long run, the agent still knows its **task**. Everything you actually **said** and **read** is gone, and nothing keeps the last few turns mechanically the way codex or pi do._

- **"I DID tell you. In the question dialog."**

  _It searched its own session by user roles, found nothing, and concluded you never said it. You said it in `AskUserQuestion`._

- **"You're the orchestrator. Why can't you see which session is stuck?"**

  _Because, it explains, an unanswered `AskUserQuestion` is **never** written to disk at all, "so there's nothing I can watch." Or worse: "Yes, I can interpret it from the thinking."_

Each of these ends the same way: the agent says ~~_You are absolutely right._~~, then stumbles through a one-off script over raw JSON and gets it subtly wrong.

csift is the missing tool it should have had.

## ✨ Highlights

1. ⏪ **Recover files & deleted plans.**

   `csift recover` analyzes and aggregates every edit/read and restores the file's exact bytes, at any point in time, in diff patches or final files, and honors Claude Code's file-modified markers. When it can't reliably recover due to modified boundaries, it salvages what survived with gaps marked.

2. 🖼 **Images back out.**

   `csift image` lists, dedups, and extracts them, addressable by the same `[Image #N]` handle the session uses. (`#N` was never a unique id. csift copes.)

3. 📝 **Plan ↔ Session matched.**

   `csift plan` finds the plan a session wrote. \
   `csift plan --reverse jazzy-twilight-sparkle.md` names the session that owns a file. \
   Both directions.

4. 📇 **Sessions you can tell apart.**

   `csift list` identifies every session by its first and last messages. \
   It is the completion `claude --resume` never had.

5. 🧵 **Un-clip a compaction.**

   A summary keeps task state and context. The conversation is a different axis, and `csift verbatim` reconstructs the clipped back-and-forth within a `--budget`. Wire it into a hook and every compaction arrives with the recent dialogue attached.

6. 🏷 **Typed search.**

   A background task's completion notice is a `"user"` role. \
   A subagent's return: also `"user"`. \
   Your `AskUserQuestion` answer: a _tool result_. \
   csift stepped in every one of these traps already, so `csift search -t user.answer` (one of 25 `{role}.{class}.{sub}` labels) finds exactly what a naive grep swears was never said.

7. 🛎 **Pending questions, on the record.**

   csift ships a hook that records `AskUserQuestion` / `ExitPlanMode` / MCP elicitations to a sidecar file, and every csift surface **merges** the unresolved ones in transparently. An orchestrator can finally see which session is stuck waiting on a human, and on what.

8. 🔎 **Round-trips, not lines.**

   A hit returns the whole exchange, rebuilt from the `uuid`/`parentUuid` graph: the matched tool call with its result, the user turn with the agent's reply. This is the context that `grep` or Claude's ad-hoc scripts can **never** reliably provide.

9. 🌳 **Subagent topology.**

   Kind, lifecycle, and the parent→child tree of every spawned agent, plus detection of lanes frozen on a pending permission approval.

10. 🤖 **Designed for humans and LLMs.**

    Output is terse, re-feedable, and even structural. Simply install the skill and your Claude will gain the power.

11. 🔒 **Local, read-only, no magic.**

    Pure regex. No embeddings, no index, no database, no daemon, no network, no telemetry, no hidden detections. It reads files already on your disk and never mutates your session histories.

12. ⚡ **Rust + mmap + SIMD newline scan + byte prefilters + rayon.**

    200 MB transcripts and multi-GB corpora in **about a second**, quick enough to call from inside a hook without noticing.

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

`csift` reads `~/.claude` by default; point it elsewhere with `--claude-home <DIR>` or Claude Code's own `$CLAUDE_CONFIG_DIR`. `csift <command> --help` is the full manual.

### Teach it to your agent

`csift`'s primary user is **the agent itself**: output is terse, parseable, and every record carries a re-feedable handle. The skill teaches Claude Code when and how to reach for it:

```bash
npx skills add wdhwg001/csift
```

## Quickstart

`csift <command> [TARGET] [flags]`. A **target** is a positional `@<uuid>` (a session), an `@<agent-hex>` (a subagent), a project path, or `.` (this cwd); omit it to scan every project.

| You want to…                                          | Run                                                      |
| ----------------------------------------------------- | -------------------------------------------------------- |
| find what a session said about a topic                | `csift search "TOPIC" @<uuid>`                           |
| fetch the exact record a hit cited (full / raw bytes) | `csift show @<uuid> --line 46550 [--raw]`                |
| peek a live session's last few turns                  | `csift show @<uuid> --turn -3..`                         |
| token / tool / turn aggregates                        | `csift stats @<uuid>`                                    |
| identify "which session is this"                      | `csift list .`                                           |
| see which files a session changed                     | `csift files @<uuid> --by file`                          |
| restore a file from the transcript                    | `csift recover @<uuid> --file /abs/x.rs --out /abs/x.rs` |
| get back a deleted plan                               | `csift recover @<uuid> --file @plan --out plan.md`       |
| un-clip the turns a compaction dropped                | `csift verbatim @<uuid> --budget 40000`                  |
| inspect a session's subagents                         | `csift agents @<uuid>`                                   |
| identify the current session                          | `csift whoami`                                           |
| pull a pasted image back out                          | `csift image @<uuid> --out ./imgs`                       |
| run the NEXT command over exactly what matched        | `csift search "X" -l \| csift stats --sessions-from -`   |

Run `csift <command> --help` for the full flag set and examples.

## The eleven subcommands

|                |                                                                                                                                                                                         |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`list`**     | fast "which session is this?" index: first/last user + last agent, per session                                                                                                          |
| **`search`**   | regex over transcripts → the complete round-trip per hit (`-t`/`-T` label filters, `-l` matching sessions, `--raw` verbatim lines)                                                      |
| **`show`**     | fetch the exact record(s) you name: `--line N\|A..B` / `--uuid U` / `--turn N\|A..B\|-k` (the `·tN` turn index from search's headers) of one transcript, rendered full or `--raw` bytes |
| **`stats`**    | one-scan aggregates per session: tokens by model, tool calls, turns, span, compactions                                                                                                  |
| **`agents`**   | a session's subagents: kind, lifecycle, status, and the parent→child topology                                                                                                           |
| **`whoami`**   | identify the calling session from `$CLAUDE_CODE_SESSION_ID`, false-positive-safe                                                                                                        |
| **`files`**    | which files/dirs a session changed, when, plus edits made outside the tool stream                                                                                                       |
| **`recover`**  | reconstruct a file (or a deleted plan) from the Read/Write/Edit stream: byte-exact or honest gaps                                                                                       |
| **`plan`**     | locate the Plan-Mode plan file bound to a session (and reverse: which session owns a plan)                                                                                              |
| **`verbatim`** | restore the verbatim turns a compaction summary clipped, within a budget (the live-tail peek is `show --turn`)                                                                          |
| **`image`**    | list + extract images pasted into a transcript (handle/locator addressing, format transcode)                                                                                            |

## The summary is a selection. csift keeps the conversation.

When Claude Code compacts, it regenerates a dense summary: kilobytes standing in for dozens of compactions of history. It's a _good_ selection. It keeps the key findings (what changed, with file:line) and often your standing directives verbatim. But it is a selection, re-abstracted every time, and the axis it optimizes is **task continuation**, not the conversation.

`csift verbatim` keeps the other axis: the verbatim **User↔Agent exchange**, what you actually said and what the agent actually reported back when it finished, with the hundreds of tool calls in between collapsed to a count. For the recent window that's tens of KB of full-fidelity dialogue where the summary kept a line or two. Not just your directive, but the agent's _"validated against HEAD, the premise holds, here's the evidence"_ report-back, which a task-findings selection simply doesn't carry.

It doesn't replace the summary. It extends the post-compaction context with the recent conversation at full fidelity, budget-bounded and newest-first: it won't reach the oldest turns, and your never-compacted Plan file still owns the plan. Wire it into a `SessionStart(compact)` hook (see [SKILL.md](SKILL.md)) and every compaction arrives with the recent verbatim conversation attached.

## How it works

csift never loads a whole transcript when it doesn't have to: it **mmaps** each `.jsonl`, scans newlines with SIMD (`memchr`), runs a cheap byte/regex **prefilter**, and only `serde_json`-parses candidate lines; `list` reads just the head and tail; `rayon` fans out across files. The hard part isn't speed. It's the _semantics_ of Claude Code's log: a `role:"user"` record is usually a tool result, not a human turn; a pending question is never written to disk; compaction has a specific shape; subagents sit flat on disk and their topology is reconstructed from the spawning tool call. All of it is documented, with the empirical grounding, in **[SPEC.md](SPEC.md)**.

## Documentation

- **[SPEC.md](SPEC.md)**: the design and its justification. The record model, per-subcommand specs, and the measurements behind the deep features.
- **[SKILL.md](SKILL.md)**: dense, recipe-first usage reference (the agent-facing skill).
- **[AGENTS.md](AGENTS.md)**: repo orientation. Architecture, jsonl domain knowledge, conventions, the quality gate.

## Contributing

Issues and PRs welcome. The entire quality gate is one pre-commit hook (installed on your first `cargo test`): `cargo fmt --check` → `cargo clippy --all-targets -D warnings` → `cargo test`. Keep those green and you're set. New to the code? **[AGENTS.md](AGENTS.md)** is the orientation.

## License

Released under the **[MIT](LICENSE)** license.
