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
    <img alt="coverage: 97.0%" src="https://img.shields.io/badge/coverage-97.0%25-4ade80" />
    <img alt="mutation score: 93.4%" src="https://img.shields.io/badge/mutation%20score-93.4%25-a3e635" />
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

⏺ Bash(csift search "rate limit" @1a2b3c4d -t agent --since 1d)
  matches  1 exchange · 1 session · oldest first

  1a2b3c4d·t42  2026-06-20 22:14:07.811 AEST(UTC+10)
    ▸ agent.message  L8821  Added a sliding-window limiter (10 req/min/IP); the 429 path now
                    returns Retry-After and logs the offending IP — gateway/src/rate_limit.rs:88.
  matched 1 exchange · 1 session · label=agent
```

That is one regex. The hit comes back as the **complete round-trip** it belongs to, in output short enough for an agent to read cheaply. No embeddings, database or daemon sit behind it.

## Why csift

> Claude Code's sessions are **plain text** in JSONL, so why csift, not grep?

They are plain text. The log holds every prompt, thought, tool call, file edit and pasted image, and every subagent the session spawned. Reading it back is where an agent goes wrong.

- **"It deleted my project files, then apologized."**

  _It was running with `--dangerously-skip-permissions`. It wiped the files it had built, said sorry and stopped dead. Asked to dig them back out of its own session, it fumbled around, because the edits spanned subagents and every restore came back partial. Then it quietly started **rewriting** them from memory, having decided in its panic that this was "easier"._

- **"It lost my design images after a compaction, then asked me to re-send them."**

  _The images were right there in the conversation. One compaction later the agent acted as if they had never existed. The compaction had handed it lossy text fragments in their place, and it believed them._

- **"Which session does `jazzy-twilight-sparkle.md` belong to?"**

  _The plan file carries the architecture that matters. Its invented name says nothing about which session wrote it. The only record of that is a `plan_mode` attachment the session wrote when it entered Plan Mode, one line among many in its transcript._

- **"The machine crashed with three sessions open. Which was which?"**

  _One was frontend, one backend, one debugging, and `claude --resume` gives you a lineup with no faces._

- **"I literally just said that."**

  _A few compactions into a long run, the agent still knows its **task**. Everything you actually **said** and **read** is gone. Claude Code does not carry the last few turns across a compaction word for word, the way the coding agents Codex and pi do._

- **"I DID tell you. In the question dialog."**

  _It searched its own session by user roles, found nothing, and concluded you never said it. You had said it in `AskUserQuestion`._

- **"You're the orchestrator. Why can't you see which session is stuck?"**

  _Because, it explains, an unanswered `AskUserQuestion` is **never** written to disk at all, "so there's nothing I can watch." Or worse, it offers to "interpret it from the thinking."_

- **"You said it stopped, but it's waiting for a subagent!"**

  _It apologized, searched every other jsonl file and came back with a "root cause". Asked to orchestrate another session in the same project, it could not even tell whether that session had stopped. Another time it took a turn-duration record for the stop, though that record only times a single turn._

Each of these ends the same way. The agent says ~~_You are absolutely right._~~, then stumbles through a one-off script over raw JSON and gets it subtly wrong.

## ✨ Highlights

1. ⏪ **Recover files & deleted plans.**

   `csift recover` replays every edit and read to restore a file's exact bytes at any point in time, as diff patches or as the final file. \
   Shell traffic counts too. A heredoc or literal `echo` written through Bash, and a clean `cat`, `head` or `sed -n` read, replay as real content when the command's shape leaves no doubt about the bytes. \
   Claude Code notes when a file changed outside its own edits, and csift stops at that point instead of guessing across it. \
   Where such a change rules out an exact restore, it salvages what survived and marks the gaps.

2. 🖼 **Images back out.**

   `csift image` lists, dedups and extracts them by the same `[Image #N]` handle the session uses. (`#N` was never a unique id. csift copes.) \
   A PDF read page by page comes back the same way, each image labelled with the page it actually is.

3. 📝 **Plan ↔ Session, both directions.**

   `csift plan` finds the plan a session wrote. \
   `csift plan --reverse ~/.claude/plans/jazzy-twilight-sparkle.md` names the session that owns it.

4. 📇 **Sessions you can tell apart.**

   `csift list` names every session by its first and last messages, which the `claude --resume` picker never shows. \
   A renamed or copied project directory can leave one session in two places. \
   csift decides by comparing the two files' bytes, never their timestamps. \
   A copy that matches the other or stops short of it merges into one, and copies that genuinely differ are both kept and both named.

5. 🧵 **Un-clip a compaction.**

   A compaction summary keeps the task state and drops most of the conversation. \
   `csift verbatim` rebuilds the clipped back-and-forth within a `--budget`. \
   Wired into a hook, it attaches the recent dialogue to every compaction.

   When Claude Code resumes a session, it reads back only as far as the latest compaction. \
   `--cross-compactions` lets csift step back over a bounded number of them, so it can still tell which earlier turns the conversation kept and which it abandoned. \
   Every command says how many it crossed and where it stopped.

6. 🏷 **Typed search.**

   A background task's completion notice is a `"user"` role. \
   A subagent's return is also `"user"`. \
   Your `AskUserQuestion` answer is a _tool result_. \
   csift has stepped in each of these traps already. `csift search -t user.answer` finds exactly what a naive grep swears was never said, and it is one of 73 `{role}.{category}.{leaf}` labels.

   Some labels name what the format hides.

   | label | finds |
   | --- | --- |
   | `user.unsent` | the message you esc-recalled and never actually sent |
   | `user.queued` | what you typed into the queue while a turn was running |
   | `harness.turn.duration` | the record behind the "Done in 1m 5s" line |
   | `agent.thinking.narration` | the API's one-line summaries, kept apart from the reasoning they summarize |

7. 🛎 **Pending questions, on the record.**

   A hook csift ships records each `AskUserQuestion`, `ExitPlanMode` and MCP elicitation to a sidecar file. \
   Every csift surface **merges** the unresolved ones in, transparently. \
   An orchestrator can finally see which session is stuck waiting on a human, and on what.

8. 🩺 **Is it actually stopped?**

   A session can end its turn and still be running.

   - `csift status` gives one verdict and names its evidence (the session registry, the transcript tail, every child lane, the task list, a live process probe). A lane is one agent's thread of work, the main session or one of its subagents.
   - Every background shell, async agent and Monitor is listed with its age and whether it ever came back. Long sessions carry dozens of zombies.
   - `csift wait --until stop --timeout 300` blocks until it really stops.
   - `--ignore-background 'npm run dev'` leaves out the tasks that never return by design.
   - On timeout it exits 124 and reports what happened while you waited.

   The timeout is required because some background tasks, a dev server for one, never return. \
   Both commands read the live state at the moment they run, on macOS, Linux and Windows.

9. 📣 **Message an agent Claude Code gives you no way to reach.**

   Claude Code's own messaging tools reach some lanes and not others. \
   An agent running inside a workflow script has no official channel. \
   Neither does your own parent subagent, an idle session with no messaging socket open, or anything at all when you are outside Claude Code.

   - `csift send @<lane> "stop after the current file"` queues one message and says what will carry it.
   - Each send gets one verdict out of OK, FULL, MAY-FAIL, UNPREDICTABLE and REFUSED.
   - Delivery is a hook line you paste. `csift deliver --recipe` prints the block, and csift writes no settings file.
   - `csift msg <id>` compares what csift meant to deliver with what the receiver's own transcript shows arrived.
   - `csift whoami --to @<lane>` predicts reach and sends nothing.

   Where an official transport does exist, csift prints the exact call for you to make. It still queues its own copy, so the message survives even if that call is never made.

10. 🔎 **Round-trips, not lines.**

    A hit returns the whole exchange, rebuilt from the `uuid` / `parentUuid` graph. \
    A matched tool call comes back with its result, and a user turn with the agent's reply. \
    Neither `grep` nor Claude's ad-hoc scripts can reliably give you that context.

11. 🧭 **What is this record even about?**

    Every hit names the message it belongs to, and how many hops away that message sits. \
    Where a record states its link to another one (a tool result to its call, a summary to the compaction it closes), the hit names that link with one of 27 fixed relation words. \
    `csift search "" @<uuid> --for L42` follows those links backward and returns every record that message led to, transitively.

12. 🌳 **Subagent topology.**

    The parent→child tree of every spawned agent, with each one's kind and lifecycle. \
    Lanes frozen on a pending permission approval are detected.

13. 🤖 **Designed for humans and LLMs.**

    Output is terse, re-feedable and structured. \
    csift ships a Claude Code skill, a set of instructions the agent loads when a task calls for it. \
    With the skill installed, your Claude knows when and how to reach for csift.

14. 🔒 **Local, plain, and it writes in two places only.**

    Pure regex. No embeddings, no index, no database, no daemon, no network, no telemetry. \
    Nothing is inferred without the output saying so. \
    It reads files already on your disk and never touches your session histories. \
    `send`, `deliver` and `ack` write into a `csift-channel` directory csift creates beside the session. \
    `recover`, `verbatim` and `image` write only the path you name with an output flag. Nothing else is written.

15. ⚡ **Rust + mmap + SIMD newline scan + byte prefilters + rayon.**

    A search that matches nothing in a 397 MiB transcript runs in **0.63 to 0.73 s**. \
    Those are medians of 21 runs on an idle host, recorded in [SPEC.md](SPEC.md) section 7h. \
    Quick enough to call from inside a hook without noticing.

## Install

```bash
cargo install csift
```

csift needs **Rust 1.89+**. To build from source instead:

```bash
git clone https://github.com/wdhwg001/csift.git
cd csift
cargo install --path .        # builds the optimized binary and puts `csift` on your PATH
# …or just `cargo build --release` → ./target/release/csift
```

By default `csift` reads `~/.claude`. It reads another directory when given `--claude-home <DIR>` or Claude Code's own `$CLAUDE_CONFIG_DIR`. `csift <command> --help` is the full manual.

### Teach it to your agent

The primary user of `csift` is **the agent itself**. Its output is terse and parseable, and every record carries a handle the agent can feed straight back in. The skill teaches Claude Code when and how to reach for it.

```bash
npx skills add wdhwg001/csift
```

From a clone, link the skill directory instead.

```bash
ln -s "$PWD/skills/csift" ~/.claude/skills/csift
```

## Quickstart

Every command takes the shape `csift <command> [TARGET] [flags]`. A **target** is positional. It is `@<uuid>` for a session, `@<agent-hex>` for a subagent, a project path, or `.` for the current directory. With no target, csift scans every project.

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
| see every record one message led to, transitively     | `csift search "" @<uuid> --for L42`                      |
| run the NEXT command over exactly what matched        | `csift search "X" -l \| csift stats --sessions-from -`   |
| message a lane the official tool cannot reach         | `csift send @<lane> "TEXT"`                              |
| check whether that message actually arrived           | `csift msg <id>`                                         |
| ask whether a lane is reachable, sending nothing      | `csift whoami --to @<lane>`                              |

`csift <command> --help` lists the full flag set, with examples.

## The seventeen subcommands

|                |                                                                                                                                                                                         |
| -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **`list`**     | indexes every session by its first and last user message and its last agent message, to answer "which session is this?". It flags a background-job clone and names the session it forked from |
| **`search`**   | runs a regex over transcripts and returns the complete round-trip per hit (`-t`/`-T` label filters, `--count-by` censuses, `--for` for the reverse provenance closure, `--attachments`, `-l` for matching sessions, `--raw` for verbatim lines) |
| **`show`**     | fetches the exact record(s) you name by `--line N\|A..B`, `--uuid U` or `--turn N\|A..B\|-k` (the `·tN` turn index in search's headers) from one transcript, rendered in full or as `--raw` bytes. `--branch-points` maps where the conversation forked |
| **`stats`**    | computes one-scan aggregates per session (tokens by model counted once per API message, tool calls, turns, span, compactions, narration blocks, a whole-file line-type census) |
| **`agents`**   | lists a session's subagents with their kind, lifecycle, status and parent→child topology |
| **`whoami`**   | identifies the calling session from `$CLAUDE_CODE_SESSION_ID` with no false positives, then the lane it sits in (itself, its parent, the live lanes under it). `--to <lane>` says whether a message would reach one, and `--peers` lists every live lane by id and state only |
| **`files`**    | shows which files and directories a session changed, and when, including edits made outside the tool stream |
| **`recover`**  | reconstructs a file, or a deleted plan, from the Read/Write/Edit stream, byte-exact or with honest gaps. `--list-backups` reads Claude Code's own file-history checkpoints |
| **`plan`**     | locates the Plan-Mode plan file bound to a session. `--reverse` finds the session that owns a plan, and `--audit` flags edits to plans the session does not own |
| **`verbatim`** | restores the verbatim turns a compaction summary clipped, within a budget (the live-tail peek is `show --turn`) |
| **`image`**    | lists and extracts images pasted into a transcript, with handle or locator addressing and format transcode |
| **`status`**   | gives a one-shot LIVE verdict on a session (running / waiting-children / waiting-hitl / idle-background-open / idle-eot / stale-dead / unknown) from the registry, the tail, a process probe and every background task it ever launched |
| **`wait`**     | blocks until a session condition fires (`stop` / `hitl` / `auq` / `notification[:RE]` / `tool:NAME` / `write:PATH` / `verdict:V`). `--timeout` is required, and a timeout exits 124 with a report of what happened meanwhile |
| **`send`**     | queues a message for one lane (a session, a subagent, a teammate, a workflow lane) from another lane or from any process outside Claude Code. Each send gets one verdict, and an official transport is printed for you to call, never performed |
| **`msg`**      | answers whether that message arrived, with one verdict joining the channel ledger's intent to the receiver transcript's proof. With no id, it lists the lane's whole ledger |
| **`ack`**      | records the receiver's own word that this lane read that message. Once a message is acked, csift stops delivering it again after a compaction |
| **`deliver`**  | is the hook entry that carries the channel into a lane. Claude Code runs it, and you never do. `csift deliver --recipe` prints the settings block for you to paste, and csift writes no settings file |

## What a compaction summary leaves out

When Claude Code compacts, it regenerates a dense summary, kilobytes standing in for the whole history before it. The summary is a good one. It keeps the key findings (what changed, with file:line) and often your standing directives verbatim. It is rewritten at every compaction, and what it is built to carry forward is **the task**.

`csift verbatim` carries the conversation instead, the verbatim **User↔Agent exchange**. That is what you actually said and what the agent actually reported back when it finished, with the hundreds of tool calls in between collapsed to a count. For the recent window it comes to tens of KB of full-fidelity dialogue where the summary kept a line or two. Your directive is in it, and so is the agent's _"validated against HEAD, the premise holds, here's the evidence"_ report-back, which a selection of task findings does not carry.

The summary stays where it is. `verbatim` extends the post-compaction context with the recent conversation at full fidelity, budget-bounded and newest-first. It does not reach the oldest turns, and your never-compacted Plan file still owns the plan. The skill ships a `SessionStart(compact)` hook for this, [`compact-slice.sh`](skills/csift/scripts/compact-slice.sh), with its settings fragment in [hooks.md](skills/csift/references/hooks.md). With the hook wired in, every compaction arrives with the recent verbatim conversation attached.

## How it works

csift avoids loading a whole transcript wherever it can. It **mmaps** each `.jsonl`, scans newlines with SIMD (`memchr`), runs a cheap byte or regex **prefilter**, and parses only the candidate lines with `serde_json`. `list` reads just the head and tail of each file, and `rayon` fans the work out across files.

Most of the work is in reading Claude Code's log correctly. Most `role:"user"` records carry tool results, and only some are human turns. A pending question can stay off disk until it is answered. Compaction writes a specific shape. Subagents sit flat on disk, and their topology is rebuilt from the tool call that spawned each one. **[SPEC.md](SPEC.md)** documents all of it, with the empirical grounding.

## How much of this is verified

csift rests on hundreds of small facts about what Claude Code writes. Each one is a claim in [`INTROSPECTION.json`](./INTROSPECTION.json). A claim carries the Claude Code version range it holds over and how completely it is attributed. Attribution asks whether the code that writes the fact was traced in the shipped Claude Code binary, and whether a specimen of the fact was observed. A claim keeps its current reading and at most one superseded one, and the table below counts the current reading of each.

Every range carries one receipt, and a receipt has to have measured something outside csift's own tree, such as a build, a byte offset in one, a corpus of transcripts or a live trial. The table is regenerated from the ledger, and the pre-commit gate checks it.

<!-- ledger-tally:begin -->
| attribution | claims | share | meaning |
|---|---:|---:|---|
| end-to-end | 591 | 78.7% | the writer, its gate and its trigger read in the shipped binary, and a specimen observed on disk or live |
| &nbsp;&nbsp;of which chain traced | 591 | 78.7% | the three hops quoted at byte offsets in `producer_chain`; the anchors line below is the measured state of those offsets |
| specimen-only | 51 | 6.8% | observed on disk or live; the writer not traced (or traced only in part) (25 of them with a partly traced writer) |
| producer-only | 79 | 10.5% | the writer traced in full; no specimen exists in the corpus or could be produced here |
| partial-producer | 13 | 1.7% | a template or field located without its gate and trigger; no specimen |
| by-elimination | 0 | 0.0% | neither leg; attributed by exclusion or from csift's own design |
| upstream | 17 | 2.3% | the producer lies outside the shipped binary by construction (the model or API side, the operating system, a native runtime binding); the client-side treatment is traced and a specimen observed |
| total | 751 | 100.0% | one claim per Claude Code behavior csift depends on, verified at Claude Code 2.1.258 |

A traced claim pins each hop at a byte offset in that build, and the gate re-reads every one of them:

`anchors byte-exact at Claude Code 2.1.258: 7,317 of 7,717 (elided 69, absent 30, prefix-only 80, read in another build 221), closed-state anchors 0, not checked`

The five counts partition the total, and the gate asserts that they do. BYTE-EXACT is read over two encodings: a few excerpts were captured from regions that are not valid UTF-8 and stored one code point per byte, and 14 of these anchors match only under that second reading. An ELIDED excerpt carries an ellipsis somewhere, which is not the same as having no byte-exact form: 1,481 such excerpts ARE byte-exact at their own offsets and are counted with the rest, because minified JavaScript's spread operator wears the same three dots. What is left in the elided count really did elide something, and the gate holds each of those to the run BEFORE its first ellipsis at its own offset. An absent or prefix-only anchor is marked as such in the ledger, with the residue line that says how to find its producer again; those are the first thing the next pass re-locates. A hop read in an artifact the badge does not name carries that build and is compared against THAT one, which is why it is disclosed as a count and never as a verdict: whether a given machine holds those artifacts is a property of the machine. A closed state's anchors were exact at the build its own range ended on, so the gate counts them and does not re-read them against a later one.
<!-- ledger-tally:end -->

A claim that is not end-to-end names the exact instrument that would close it. A claim attributed by elimination can never carry `holds`, the ledger's verdict for a claim re-observed exactly as written.

The project keeps private the method it used to take Claude Code apart. It does not publish a mechanical diff of the ledger either, since such a diff would expose the same method. It will not publish a corpus of transcripts. No level of redaction means anything for this format, because a redacted real transcript cannot be told apart from a hand-made one. A released corpus would prove nothing and could still leak.

Claude Code is conservative with its own jsonl, and the format rarely breaks across versions. So the ledger is re-verified periodically, and not in full for every Claude Code release. The sessions csift is developed in always run the current Claude Code, so a format change shows up in daily use between re-verifications.

## Documentation

`csift <command> --help` is the manual written for people, and it carries the same information as the skill. Claude Code writes and maintains this whole repository, and its other documents are written for the agent that does that work.

- [skills/csift/](skills/csift/SKILL.md) is the skill an agent loads to operate csift. A short `SKILL.md` routes, one reference file covers each topic, and three hook scripts sit beside them.
- [SPEC.md](SPEC.md) holds the design intent, the record-model semantics and the measurements behind the deep features.
- [AGENTS.md](AGENTS.md) is the repository's operating manual, covering the architecture, the jsonl domain knowledge and the quality gate.

They are dense and cross-referenced, and they are written for a model's attention. A person who wants to understand the codebase does best to hand the repository to their own agent and ask for the tour. It reads these files well.

## Contributing

Claude wrote csift, so its bugs are Claude's own, and I don't think fixing them should be a human's job. The intended path is to have Claude clone the repo, clean up after itself and open the PR. From the PR onward, though, I expect a human who has read the code. If reviewing your PR would mean chatting with an AI, it should have been an Issue instead. Issues are genuinely welcome, and a clearly described problem is all I need to point my Claude at it.

The whole quality gate is one pre-commit hook, installed on your first `cargo test`. It runs the structure gate (file and folder limits), then `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, the introspection ledger gate and `cargo test`. A failure at any stage blocks the commit.

## License

Released under the **[MIT](LICENSE)** license.
