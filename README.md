<div align="center">
  <img src="assets/logo.svg" alt="csift logo" width="168" />

  <h1>csift</h1>

  <p><sub>Pronounced <strong>"c-sift"</strong> (<em>see-sift</em>)</sub></p>

  <p><strong>The missing tool for Claude Code session transcripts.</strong></p>

  <p>
    <strong>search</strong>, <strong>recover</strong>, <strong>monitor</strong>, and <strong>audit</strong> any Claude Code session straight from the <code>.jsonl</code> logs.
  </p>

  <p>
    <img alt="Rust 1.89+" src="https://img.shields.io/badge/Rust-1.89%2B-dea584?logo=rust&logoColor=white" />
    <img alt="search: pure regex" src="https://img.shields.io/badge/search-pure%20regex-7c9cff" />
    <img alt="embeddings: none" src="https://img.shields.io/badge/embeddings-none-22d3ee" />
    <img alt="coverage: 95.7%" src="https://img.shields.io/badge/coverage-95.7%25-4ade80" />
    <img alt="mutation score: 91.2%" src="https://img.shields.io/badge/mutation%20score-91.2%25-a3e635" />
    <img alt="verified against Claude Code 2.1.258" src="https://img.shields.io/badge/verified%20against%20Claude%20Code-2.1.258-d97757" />
    <img alt="built for Claude Code" src="https://img.shields.io/badge/built%20for-Claude%20Code-d97757" />
    <img alt="written by Claude Code" src="https://img.shields.io/badge/written%20by-Claude%20Code-d97757" />
    <img alt="license: MIT" src="https://img.shields.io/badge/license-MIT-a78bfa" />
  </p>
</div>

---

```console
──────────────────────────────────────────────────────────────────────────────
 ❯ what that session decide about rate limiting and wheres the code?
──────────────────────────────────────────────────────────────────────────────

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

- **"You said it stopped but it's waiting for a subagent!"**

  _It apologized, found a "root cause" by searching all other jsonl files. When you have another session to orchestration in the same project, it wouldn't even know whether it's stopped. Or worse, it found a turn duration metadata and believed it must be the true stop. It is not._

Each of these ends the same way: the agent says ~~_You are absolutely right._~~, then stumbles through a one-off script over raw JSON and gets it subtly wrong.

csift is the missing tool it should have had.

## ✨ Highlights

1. ⏪ **Recover files & deleted plans.**

   `csift recover` analyzes and aggregates every edit/read and restores the file's exact bytes, at any point in time, in diff patches or final files, and honors Claude Code's file-modified markers. Shell traffic counts too: a heredoc or literal echo written through Bash, and a clean `cat`/`head`/`sed -n` read, replay as real content under strict admission gates. When it can't reliably recover due to modified boundaries, it salvages what survived with gaps marked.

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
   csift stepped in every one of these traps already, so `csift search -t user.answer` (one of 34 `{role}.{class}.{sub}` labels) finds exactly what a naive grep swears was never said. \
   Some labels exist because the format hides things: `user.unsent` finds the message you esc-recalled and never actually sent, `user.queued` finds what you typed into the queue while a turn was running, `harness.meta.turn-duration` is the record behind the "Done in 1m 5s" line, and `agent.thinking.narration` separates the API's one-line summaries from the reasoning they summarize.

7. 🛎 **Pending questions, on the record.**

   csift ships a hook that records `AskUserQuestion` / `ExitPlanMode` / MCP elicitations to a sidecar file, and every csift surface **merges** the unresolved ones in transparently. An orchestrator can finally see which session is stuck waiting on a human, and on what.

8. 🩺 **Is it actually stopped?**

   An end-of-turn record is not a stop. \
   "Crunched for 46m 26s · done 11:00 am · 1 shell still running" wrote a `turn_duration` line with a duration, a message count, and nothing about the shell. \
   `csift status` joins the session registry, the transcript tail, every child lane, the task list and a process probe into one verdict, with the evidence named. \
   Every background shell, async agent and Monitor is listed with its age and whether it came back. Long sessions carry dozens of zombies. \
   `csift wait --until stop --timeout 300` blocks until it really stops, exits 124 on timeout with a report of what happened meanwhile, and takes a lens (`--ignore-background 'npm run dev'`) so a dev server never holds the wait. \
   The timeout is required, on purpose. Point-in-time by design, on macOS, Linux and Windows.

9. 🔎 **Round-trips, not lines.**

   A hit returns the whole exchange, rebuilt from the `uuid`/`parentUuid` graph: the matched tool call with its result, the user turn with the agent's reply. This is the context that `grep` or Claude's ad-hoc scripts can **never** reliably provide.

10. 🌳 **Subagent topology.**

    Kind, lifecycle, and the parent→child tree of every spawned agent, plus detection of lanes frozen on a pending permission approval.

11. 🤖 **Designed for humans and LLMs.**

    Output is terse, re-feedable, and even structural. Simply install the skill and your Claude will gain the power.

12. 🔒 **Local, read-only, no magic.**

    Pure regex. No embeddings, no index, no database, no daemon, no network, no telemetry, no hidden detections. It reads files already on your disk and never mutates your session histories.

13. ⚡ **Rust + mmap + SIMD newline scan + byte prefilters + rayon.**

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
| is that session still running, and on what            | `csift status @<uuid>`                                   |
| block until a session stops or asks                   | `csift wait @<uuid> --until stop --timeout 300`          |
| find the message you esc-recalled and never sent      | `csift search "" @<uuid> -t user.unsent`                 |
| see what record-types fill a session                  | `csift search "" @<uuid> --count-by label`               |
| run the NEXT command over exactly what matched        | `csift search "X" -l \| csift stats --sessions-from -`   |

Run `csift <command> --help` for the full flag set and examples.

## The thirteen subcommands

|                |                                                                                                                                                                                         |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`list`**     | fast "which session is this?" index: first/last user + last agent, per session; flags a background-job clone and names the session it forked from                                        |
| **`search`**   | regex over transcripts → the complete round-trip per hit (`-t`/`-T` label filters, `--count-by` censuses, `--attachments`, `-l` matching sessions, `--raw` verbatim lines)                                                      |
| **`show`**     | fetch the exact record(s) you name: `--line N\|A..B` / `--uuid U` / `--turn N\|A..B\|-k` (the `·tN` turn index from search's headers) of one transcript, rendered full or `--raw` bytes; `--branch-points` maps where the conversation forked |
| **`stats`**    | one-scan aggregates per session: tokens by model (counted once per API message), tool calls, turns, span, compactions, narration blocks, a whole-file line-type census                                                                                                  |
| **`agents`**   | a session's subagents: kind, lifecycle, status, and the parent→child topology                                                                                                           |
| **`whoami`**   | identify the calling session from `$CLAUDE_CODE_SESSION_ID`, false-positive-safe                                                                                                        |
| **`files`**    | which files/dirs a session changed, when, plus edits made outside the tool stream                                                                                                       |
| **`recover`**  | reconstruct a file (or a deleted plan) from the Read/Write/Edit stream: byte-exact or honest gaps; `--list-backups` reads Claude Code's own file-history checkpoints                                                                                       |
| **`plan`**     | locate the Plan-Mode plan file bound to a session (reverse: which session owns a plan; `--audit` flags edits to plans the session does not own)                                                                                              |
| **`verbatim`** | restore the verbatim turns a compaction summary clipped, within a budget (the live-tail peek is `show --turn`)                                                                          |
| **`image`**    | list + extract images pasted into a transcript (handle/locator addressing, format transcode)                                                                                            |
| **`status`**   | one-shot LIVE verdict on a session: running / waiting-children / waiting-hitl / idle-background-open / idle-eot / stale-dead / unknown, from the registry + tail + process probe + every background task it ever launched |
| **`wait`**     | block until a session condition fires (`stop` / `hitl` / `auq` / `notification[:RE]` / `tool:NAME` / `write:PATH` / `verdict:V`); `--timeout` required, exit 124 with a report of what happened meanwhile |

## The summary is a selection. csift keeps the conversation.

When Claude Code compacts, it regenerates a dense summary: kilobytes standing in for dozens of compactions of history. It's a _good_ selection. It keeps the key findings (what changed, with file:line) and often your standing directives verbatim. But it is a selection, re-abstracted every time, and the axis it optimizes is **task continuation**, not the conversation.

`csift verbatim` keeps the other axis: the verbatim **User↔Agent exchange**, what you actually said and what the agent actually reported back when it finished, with the hundreds of tool calls in between collapsed to a count. For the recent window that's tens of KB of full-fidelity dialogue where the summary kept a line or two. Not just your directive, but the agent's _"validated against HEAD, the premise holds, here's the evidence"_ report-back, which a task-findings selection simply doesn't carry.

It doesn't replace the summary. It extends the post-compaction context with the recent conversation at full fidelity, budget-bounded and newest-first: it won't reach the oldest turns, and your never-compacted Plan file still owns the plan. Wire it into a `SessionStart(compact)` hook (see [SKILL.md](SKILL.md)) and every compaction arrives with the recent verbatim conversation attached.

## How it works

csift never loads a whole transcript when it doesn't have to: it **mmaps** each `.jsonl`, scans newlines with SIMD (`memchr`), runs a cheap byte/regex **prefilter**, and only `serde_json`-parses candidate lines; `list` reads just the head and tail; `rayon` fans out across files. The hard part isn't speed. It's the _semantics_ of Claude Code's log: a `role:"user"` record is usually a tool result, not a human turn; a pending question is never written to disk; compaction has a specific shape; subagents sit flat on disk and their topology is reconstructed from the spawning tool call. All of it is documented, with the empirical grounding, in **[SPEC.md](SPEC.md)**.

## How much of this is verified

csift rests on hundreds of small facts about what Claude Code writes. Each one is a claim in [`INTROSPECTION.json`](./INTROSPECTION.json), and each claim records how completely it is attributed: whether the code that writes the fact was traced in the shipped Claude Code binary, and whether a specimen of the fact was observed. The table is regenerated from the ledger and checked by the pre-commit gate; the numbers are what they are.

<!-- ledger-tally:begin -->
| attribution | claims | share | meaning |
|---|---:|---:|---|
| end-to-end | 418 | 80.1% | the writer, its gate and its trigger read in the shipped binary, and a specimen observed on disk or live |
| &nbsp;&nbsp;of which chain traced and adversarially re-read | 205 | 39.3% | the three hops quoted at byte offsets in `producer_chain`, every excerpt re-read by an independent verifier |
| &nbsp;&nbsp;of which audit-graded, chain not yet traced | 213 | 40.8% | graded end-to-end from the release-audit checks (writer offsets cited there) before the three-hop tracing existed; the next audit traces them |
| specimen-only | 70 | 13.4% | observed on disk or live; the writer not traced (or traced only in part) (57 of them with a partly traced writer) |
| producer-only | 32 | 6.1% | the writer traced in full; no specimen exists in the corpus or could be produced here |
| partial-producer | 2 | 0.4% | a template or field located without its gate and trigger; no specimen |
| by-elimination | 0 | 0.0% | neither leg; attributed by exclusion or from csift's own design |
| total | 522 | 100.0% | one claim per Claude Code behavior csift depends on, verified at Claude Code 2.1.258 |
<!-- ledger-tally:end -->

A claim that is not end-to-end lists the exact instrument that would close it, and a claim attributed by elimination can never be marked as holding. The ledger is re-audited against the Claude Code version the badge names before every release.

## Documentation

There is none, in the human sense. This repository is written and maintained entirely by Claude Code, and the three reference files are the corpus the maintaining agent works from:

- **[SKILL.md](SKILL.md)**: the usage reference an agent loads to operate csift.
- **[SPEC.md](SPEC.md)**: design intent, record-model semantics, and the measurements behind the deep features.
- **[AGENTS.md](AGENTS.md)**: the repo operating manual. Architecture, jsonl domain knowledge, the quality gate.

They are dense, cross-referenced, and written for a model's attention, not yours. The practical way to understand this codebase as a human is to hand the repo to your own agent and ask for the tour: it reads these files well.

## Contributing

An honest note first: csift's potholes are Claude's own, and I don't think fixing them should be a human's job. The intended path is to have Claude clone the repo and clean up after itself, then open the PR. From the PR onward, though, I expect a human who has read the code; if reviewing your PR would mean chatting with an AI, it should have been an Issue instead. Issues are genuinely welcome: describe the problem clearly and I'm happy to point my Claude at it.

The entire quality gate is one pre-commit hook (installed on your first `cargo test`): the structure gate (file/folder limits) → `cargo fmt --check` → `cargo clippy --all-targets -D warnings` → `cargo test`. Keep those green and you're set.

## License

Released under the **[MIT](LICENSE)** license.
