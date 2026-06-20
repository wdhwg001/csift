# AGENTS.md — csift operating manual

Project-specific operating manual for any AI agent (Claude Code, Codex, Cursor) working in this repo. Read this first when a conversation opens.

> **About this file.** `AGENTS.md` is the canonical, vendor-neutral filename (Codex / Cursor / GPT-tooling convention). `CLAUDE.md` is Claude Code's expected filename and is a **symlink to `AGENTS.md`**, so the same content loads whichever tool is driving. **Edit `AGENTS.md` only; `CLAUDE.md` follows automatically.**
>
> **Companion doc.** [`SPEC.md`](./SPEC.md) is the product/behaviour spec — the record model, the per-subcommand spec, the performance contract, and (§11) the design rationale + empirical grounding for `recover` / `turns` / `agents`. This file is authoritative for _how to work in the repo_; SPEC.md for _what to build_.

---

## 1. What csift is

**csift — "ripgrep for Claude Code session transcripts".** A fast Rust CLI that **lists** and **regex-searches** Claude Code session `.jsonl` files.

- **Primary consumer is an LLM** — a Claude Code agent searching/recovering its own or a peer session. Output must be clean, token-efficient, and regex-driven. Default output is human/LLM-readable with clear session/turn/category/timestamp headers; `--json` is the machine format.
- **Explicitly NO BM25 / embeddings / semantic search.** Pure regex/ripgrep only. Lexical tokenisation across scripts (CJK / multi-byte) is intractable for scoring; regex is the strength and the whole point.
- **Subcommands:** `list`, `search`, `agents`, `whoami`, `files`, `recover`, `plan`, `turns`, `image`. `list`/`search`/`files`/`recover`/`turns`/`image` span each session's subagent transcripts by default (`--no-subagents` opts out); `agents` reports a session's subagent lifecycle (kind / start / completion / status), with `--since`/`--until` + `--by start|completion` window filters. `files` reports which files/dirs a session changed AND detects Edit-before-Read boundaries (files changed outside the tool stream — the discovery signal for risky-to-reconstruct files), every row carrying its `Lnnnn`. `recover` defaults to restoring a file's RAW final content (or failing, never a holey file, when the session saw only part — reach for `--salvage` then, the best-effort line-numbered fragment); `recover --file @plan` reconstructs the session-bound plan file; `plan` locates it (via the `plan_mode` attachment). `image` lists + extracts the inline base64 images a session carries — addressed by the `#N` handle the session uses (or the exact `L<line>i<n>` locator), `--out <dir>` decodes to files; `turns`/`search` surface the ids inline. An ambiguous `#N` (CC reuses them across prompts) errors with the occurrence list — disambiguate via the locator or `--since`/`--turn-range`/`--uuid`; the `--out` PATH extension drives the format (a `.jpg`/`.gif`/`.webp` file converts a single image, a directory keeps source formats). See SPEC §6.5–6.9.

---

## 2. Git & quality gate

No CI service runs here; the pre-commit hook (§5) is the entire quality gate.

---

## 3. Stack

| Layer | Choice | Why |
| --- | --- | --- |
| Language | Rust 2021, `rust-version = 1.89` | Fast byte/IO, strong types over dense jsonl |
| CLI | `clap` (derive) | Subcommands + example-rich `--help` |
| Regex | `regex` | ripgrep-like matching, smart-case |
| Multi-literal | `aho-corasick` | `recover --files-from` batch prefilter — match all manifest basenames against a transcript in one pass (already transitive via `regex`) |
| JSON | `serde` + `serde_json` | Lazy parse only on candidate lines |
| Scan | `memchr` (SIMD newline) + `memmap2` (mmap) | 200MB+ files without full-buffer reads |
| Parallel | `rayon` | Fan-out across many session files |
| Errors | `anyhow` | Error chains surfaced on stderr; no `unwrap` in lib paths |
| Date/TZ | `jiff` | ISO8601 parse + system-local timezone render alongside raw UTC (auto-detected via `TimeZone::system()`) |
| Images | `image` (features `png`/`jpeg`/`gif`/`webp`/`color_quant`) + `webp` (libwebp) + `color_quant` | `image --out <file.ext>` transcoding: decode any of the four Claude-API types via `image`; re-encode png/jpeg(q90)/gif(Floyd-Steinberg dithered, NeuQuant palette) via `image`, and webp(q90 lossy) via libwebp (the `webp` crate). The heavyweight deps — only `image.rs` touches them |
| Hooks | `cargo-husky` (dev-dep, `user-hooks`) | Installs the pre-commit gate |

Versions are pinned by `^`-range in `Cargo.toml` + `Cargo.lock`. **Do not bump majors without an explicit reason.**

---

## 4. Conventions — read before changing code

- **No `unwrap`/`expect` in library/hot paths.** Propagate with `anyhow::Result` and `?`. Tests may `unwrap`. The `main` shim is the only place that turns an error into an exit code.
- **No silent truncation.** If a result set is capped (`--max-count`), the output MUST state how many were dropped. A skipped malformed line must be counted, never hidden.
- **Tolerant parsing.** Real jsonl carries far more fields than any doc lists (`attachment`, `file-history-snapshot`, `queue-operation`, `isMeta`, `toolUseResult`, `slug`, …) and some records have no `timestamp`. Deserialize only what's used, ignore the rest, never crash on a new field or block type. The `Block` enum has a `#[serde(other)] Unknown` arm for exactly this.
- **Performance is a contract, not a nicety.** `list`/`search` must stay fast on 200MB+ files: mmap + `memchr` line scan + a cheap byte/regex prefilter, with full `serde_json` only on candidate lines; tail reads SEEK from EOF backward (never parse the whole file); `rayon` parallelizes across files.
- **`PascalCase` types, `snake_case` items, one module per concern** (`cli`, `path`, `model`, `parse`, `session`, `search`, `subagent`, `agents`, `files`, `recover`, `plan`, `turns`, `time_window`, `timez`, `whoami`).
- **Comments capture _why_ / a non-obvious constraint**, not what the code already says.
- **Dead-code allows:** there is no crate-level `#![allow(dead_code)]`. The only `#[allow(dead_code)]` is targeted on `model::Record`/`Block` for SPEC-mandated record-shape fields that are deserialized for tolerance/completeness but not yet read by a handler (justified inline). Do not add a crate-wide allow — it would mask real dead code.

---

## 5. Commands

```bash
cargo build                                  # debug build (GATE: must succeed)
cargo build --release                        # optimised (thin-LTO, 1 cgu) for real scans
cargo run -- list [PATH...]                  # list sessions (+ subagents by default; --no-subagents to skip)
cargo run -- search PATTERN [flags]          # regex search (spans subagents by default)
cargo run -- agents [PATH | --session ID]    # subagent lifecycle (kind/start/completion/status; --since/--until/--by)
cargo run -- whoami [--show-path]            # identify the calling CC session
cargo fmt --all                              # format
cargo fmt --all -- --check                   # format gate
cargo clippy --all-targets -- -D warnings    # lint gate (warnings-as-errors)
cargo test                                   # unit tests (also installs the hook)
```

**Pre-commit gate (cargo-husky).** On the first `cargo test`/`cargo build` after checkout, cargo-husky installs `.git/hooks/pre-commit` from `.cargo-husky/hooks/pre-commit`. It runs, in order: `cargo fmt --all -- --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test`. A failure blocks the commit. Edit the **source** hook (`.cargo-husky/hooks/pre-commit`), not the installed copy, then re-run `cargo test` to reinstall. Genuine-WIP bypass: `git commit --no-verify` (use sparingly).

---

## 6. The Claude Code jsonl knowledge (verified empirically 2026-06-07)

This is the load-bearing domain knowledge. Verified against real `~/.claude/projects/**/*.jsonl`.

### 6.1 Data location

```
~/.claude/projects/<ENCODED_PROJECT_DIR>/<session-uuid>.jsonl                          # a session transcript
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/agent-<hex>.jsonl                # (A) built-in Task/Agent subagent transcript
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/workflows/wf_*/agent-<hex>.jsonl # (B) workflow / OMC subagent transcript
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/workflows/wf_*/journal.jsonl     # (C) workflow EVENT log — NOT a transcript (excluded)
~/.claude/projects/<ENCODED>/<session-uuid>/subagents/**/*.meta.json                   # {agentType, description?, …} companions
~/.claude/projects/<ENCODED>/<session-uuid>/tool-results/<id>.txt                      # externalised tool output
```
Kind = path location (A→builtin-task, B→workflow), NOT `agentType`. Canonical agent id = bare `<hex>` (the record/journal `agentId`). `journal.jsonl` is read only for completion status, never listed/searched. See SPEC §6.5 + `src/subagent.rs`.

### 6.2 Path encoding (verified, deterministic forward / lossy reverse)

Claude Code encodes a project's absolute cwd into a dir name by replacing **every** non-`[A-Za-z0-9]` byte with a single `-`. **No** consecutive-dash collapsing; `.`, `/`, `_`, space all map to `-`. Confirmed:

- `/Users/testuser/Projects/widget_app_prototype` → `-Users-testuser-Projects-widget-app-prototype` (both `/` and `_` → `-`).
- `/Users/testuser/Projects/Acme/widget_factory-worktrees/main` → `-Users-testuser-Projects-Acme-widget-factory-worktrees-main`.
- A `/.claude/` segment → `--claude-` (a literal `--` double-dash — proves no collapse, and `.` → `-`).

Forward is deterministic; **reverse is lossy** (a `-` could have been `/`, `_`, `.`, …) so we never reverse. The tool ACCEPTS either (a) an actual filesystem path (encode it, locate the matching dir) or (b) a direct `~/.claude/projects/<encoded>` path (use as-is). Detect which by whether the arg resolves under the projects root.

### 6.3 Record model (one JSON object per line)

**Top-level fields used:** `type`, `uuid`, `parentUuid`, `timestamp` (ISO8601 UTC, e.g. `2026-06-07T05:43:00.000Z`), `sessionId`, `cwd`, `version`, `gitBranch`, `isSidechain`, `userType`, `message`, plus `subtype`/`content` on system records and `isCompactSummary` on compaction summaries. **Many more fields exist and are ignored** (`attachment`, `file-history-snapshot`, `queue-operation`, `isMeta`, `toolUseResult`, `sourceToolAssistantUUID`, `slug`, `entrypoint`, `promptId`, …).

**`type` values seen:** `user`, `assistant`, `system`, plus metadata-only records `last-prompt`, `ai-title`, `agent-name`, `mode`, `permission-mode`, `attachment`, `file-history-snapshot`, `queue-operation` (the metadata-only ones often have **no `timestamp`** — skip in time logic, never crash).

**`type:"user"`** — `message.role="user"`; `message.content` is EITHER a string (genuine user text, older format) OR an array of blocks. **CRUCIAL: a "user" record is NOT always a human turn** — `tool_result` blocks are carried on `role:user` records too. In one real session: 332 genuine string-content + 61 text-block users vs **1619 tool_result-carriers**. The genuine-user classification is load-bearing.

**`type:"assistant"`** — `message.role="assistant"`; `message.content` = array of blocks.

**Block types:** `{type:text,text}`, `{type:thinking,thinking,signature?}`, `{type:tool_use,id,name,input}`, `{type:tool_result,tool_use_id,content,is_error?}`, `{type:image,source}`. `tool_result.content` may be a string OR an array of `{type:text,text}` / `{type:image}`.

**AskUserQuestion** = a `tool_use` block with `name="AskUserQuestion"`. **HARD-WON:** a PENDING/unanswered AskUserQuestion is **not** flushed to jsonl — only answered ones appear (the answer returns as a later `tool_result`/user record).

**`type:"system"`** — `{subtype, content?, level?, toolUseID?}`. Subtypes seen: `stop_hook_summary`, `turn_duration`, `away_summary` (a short auto-summary of what the session was doing when it went idle), `compact_boundary`.

**Compaction (verified shape):** the summary is a `type:"user"` record with `isCompactSummary:true` + `isVisibleInTranscriptOnly:true` carrying **string** content (NOT a `type:"summary"` record). A separate `type:"system"` `subtype:"compact_boundary"` record carries `compactMetadata:{trigger,preTokens,postTokens,durationMs}`. **A compaction summary must be excluded from "genuine user".**

**Externalised output:** large tool outputs may be moved to a sibling `tool-results/<id>.txt`, with an inline `<persisted-output>` pointer (carrying an absolute path + a preview). Optionally resolved with `--resolve-persisted`.

### 6.4 Genuine-user vs tool-result-carrier (the filter that everything hinges on)

A GENUINE user turn (for the `user` category and for turn-delimiting):

1. `type:"user"` with `message.role == "user"`, AND
2. `isCompactSummary` is falsey, AND
3. content is a string, OR content blocks contain a `text` block and NO `tool_result` block.

A `tool_result`-carrier record does **not** count as genuine user and does **not** start a turn. See `model::Record::is_genuine_user`.

### 6.5 Categories (`search -t/--category`, repeatable)

- `thinking` = assistant thinking blocks.
- `user` = genuine user input + user answers to AskUserQuestion (NOT tool_result-carriers).
- `tool` = `tool_use` blocks (AskUserQuestion is a tool_use).
- `tool-response` = `tool_result` blocks.
- `agent` = assistant visible end-of-turn text (the agent message; "agent includes AskUserQuestion").

### 6.6 Complete round-trip (exchange)

On a match, return the COMPLETE exchange, not a fragment: a matched `tool_use` WITH its `tool_result`; a matched user turn WITH the agent response. Reconstruct via `uuid`/`parentUuid` linking. A **turn** is delimited by genuine-user messages.

### 6.7 whoami detection (verified)

Claude Code exports **`CLAUDE_CODE_SESSION_ID`** into its Bash tool env, equal to the session's own jsonl basename (verified: env value `0a1b2c3d-…` matched `…/0a1b2c3d-….jsonl` in this project dir). This is definitive — per-session, version-independent, survives bash nesting, zero false positives. Use it and nothing else. When absent/empty, **DO NOT GUESS** (concurrent sessions, different binaries; most-recent-mtime is a false-positive trap) — error with guidance to pass `--session`. It is acceptable for whoami to often say "ambiguous". (`CODEX_COMPANION_SESSION_ID` mirrors it but is Codex-plugin-specific; prefer the canonical var.)

---

## 7. Module map

```
src/main.rs      # binary entrypoint: parse args, dispatch, error→exit code
src/cli.rs       # clap derive: Cli/Command + ListArgs/SearchArgs/WhoamiArgs + Category
src/path.rs      # encode_cwd + projects-root + target resolution (real-path vs encoded; >200-char prefix-scan)
src/model.rs     # serde Record/Message/Content/Block + is_genuine_user
src/parse.rs     # mmap + memchr head/tail/stream readers + lazy parse_line
src/session.rs   # `list`: head+tail read → SessionSummary (+ spans subagents by default)
src/search.rs    # `search`: regex + filters → complete round-trip Exchange (+ spans subagents)
src/subagent.rs  # subagent discovery/classification (3 on-disk shapes, journal excluded) + lifecycle/status + agent→agent topology (flat on disk; nesting reconstructed via a GLOBAL spawn index → parent_agent_id/depth/tree)
src/agents.rs    # `agents`: per-subagent lifecycle rows + --kind/--since/--until/--by filters
src/time_window.rs # `--since`/`--until` parsing (absolute + relative, system-local); shared by search + agents
src/timez.rs     # shared system-local timestamp rendering (format_timestamp / local_iso / local_tz)
src/whoami.rs    # `whoami`: CLAUDE_CODE_SESSION_ID detection, false-positive-safe
src/recover.rs   # `recover`: file-content reconstruction (default restore = raw final content, or a SMART fail-if-partial that lists every external-change boundary + recommends the pre-change dump + patches-since + reconcile; --salvage/--patches/--at/--coverage) + the `--file @plan` sigil; modified-since-read invalidates the final-state buffer; --patches uses FULL context (all read lines, Read-before-Edit-guaranteed); basename prefilter + `--files-from`/`--out-dir` batch (parse each transcript once for many files)
src/plan.rs      # `plan`: plan-file binding resolver (the `plan_mode` attachment) + shared @plan resolution
src/turns.rs     # `turns`: turn-fidelity reconstruction of a compaction-clipped exchange
src/image.rs     # `image`: list + extract inline base64 images (#N handle + L<line>i<n> locator; ambiguous-#N error + --since/--turn-range/--uuid disambiguators; --out extension-driven transcode via image + libwebp)
```
The CLI entrypoint is `cli::parse_argv` (NOT `Cli::parse`): it runs an argv-normalization pass (`cli::normalize_argv`) so a `--format`/`--kind`/… flag works in ANY position relative to a leading-`-` encoded project target — fixes clap's `allow_hyphen_values` greedy-absorb bug (#3880) with zero-drift flag discovery via clap introspection.

---

## 8. What NOT to do

- **Don't introduce BM25 / embeddings / semantic search.** Regex only (§1).
- **Don't `unwrap`/`expect` in library paths**, and **don't silently truncate** (§4).
- **Don't parse a whole 200MB file** when a head or tail read answers the question (§4, §6).
- **Don't trust most-recent-mtime for `whoami`** (§6.7).
- **Don't blindly trust this doc's field list** — real jsonl evolves; re-verify against `~/.claude/projects` and EXTEND the model tolerantly rather than tightening it.
- **Don't bump a dependency major** without an explicit reason.
- **Don't edit `.git/hooks/pre-commit` directly** — edit `.cargo-husky/hooks/pre-commit` and re-run `cargo test`.
