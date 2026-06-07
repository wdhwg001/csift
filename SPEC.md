# SPEC.md — csift

> **Status: PLACEHOLDER (Phase 1 scaffold).** This document is seeded **verbatim** from the original project brief. Phase 2 finalizes it (concrete CLI grammar, output formats, edge-case rules, fixtures). Treat the brief below as the source of intent; where the empirically-verified jsonl knowledge in [`CLAUDE.md`](./CLAUDE.md) § 6 is more specific, that takes precedence and Phase 2 reconciles the two.

---

## PROJECT BRIEF (verbatim)

csift — "ripgrep for Claude Code session transcripts". A fast Rust CLI to LIST and SEARCH Claude Code session .jsonl files.
Repo: /Users/testuser/Projects/csift (LOCAL git only — git init + commits, NEVER add a remote, NEVER push). Primary consumer = an LLM (Claude Code agents searching/recovering their own or a peer session), so output must be clean, token-efficient, regex-driven.
EXPLICITLY NO BM25 / embeddings / semantic search — pure regex/ripgrep (Chinese tokenisation is intractable and regex is the strength).

DATA LOCATION: each session = ~/.claude/projects/<ENCODED_PROJECT_DIR>/<session-uuid>.jsonl . Subagent transcripts live under ~/.claude/projects/<ENCODED>/<session-uuid>/subagents/*.jsonl (a sibling <uuid>/ dir may also hold tool-results/).

PATH ENCODING (MUST verify empirically vs real dirs in ~/.claude/projects): the project absolute cwd is encoded by replacing every non-alphanumeric char with a dash. e.g. /Users/testuser/Projects/widget_app_prototype -> -Users-testuser-Projects-widget-app-prototype (BOTH "/" and "_" become "-"). VERIFY the exact rule ([^a-zA-Z0-9] -> "-"? does it collapse consecutive dashes? how is "." handled?) by listing ~/.claude/projects and reverse-checking against known cwds (e.g. /Users/testuser/Projects/Acme/widget_factory-worktrees/main -> -Users-testuser-Projects-Acme-widget-factory-worktrees-main). Forward (actual path -> encoded dir) is deterministic; reverse is LOSSY. So the tool ACCEPTS either (a) an actual filesystem path (encode it, locate the matching projects dir) or (b) a direct ~/.claude/projects/<encoded> path (use as-is). Detect which by whether the arg resolves under ~/.claude/projects.

JSONL RECORD MODEL (one JSON object per line; VERIFY + EXTEND vs real data, do not blindly trust this list):
- top-level fields seen: type, uuid, parentUuid, timestamp (ISO8601 UTC e.g. 2026-06-07T05:43:00.000Z), sessionId, cwd, version (the Claude Code version), gitBranch, isSidechain, userType, message.
- type "user": message.role="user"; message.content is EITHER a string (genuine user text, older format) OR an array of blocks. CRUCIAL: tool_result blocks are ALSO carried on role:user records, so a "user" record is NOT always a genuine human turn — it may be a tool-response carrier. Distinguish GENUINE user input (string, or text blocks) from tool-result-carrier (content is/contains tool_result).
- type "assistant": message.role="assistant"; message.content = array of blocks.
- block types: {type:"text",text}, {type:"thinking",thinking,signature?}, {type:"tool_use",id,name,input}, {type:"tool_result",tool_use_id,content,is_error?}. tool_result.content may be a string OR an array of {type:text,text}/{type:image}.
- AskUserQuestion = a tool_use block with name="AskUserQuestion". HARD-WON: a PENDING/unanswered AskUserQuestion is NOT flushed to jsonl — only answered ones appear (the answer returns as a later tool_result / user record).
- type "system": {subtype:"stop_hook_summary"|"turn_duration"|"away_summary"|...,content?,level?,toolUseID?}. away_summary = a short auto summary of what the session was doing when it went idle.
- type "summary" (compaction): carries a compaction summary — VERIFY exact shape (may be {type:"summary",summary} and/or a leafUuid pointer).
- metadata-only records WITHOUT a timestamp seen: last-prompt, ai-title, agent-name, mode, permission-mode — handle gracefully (skip in time logic, never crash).
- large tool outputs may be EXTERNALISED to a sibling tool-results/<id>.txt with a <persisted-output> pointer inline — be aware (optionally resolve with a flag).

CATEGORIES (the -t/--category filter; repeatable):
- thinking      = assistant thinking blocks
- user          = GENUINE user input (human turns) + user answers to AskUserQuestion (NOT tool_result-carrier records)
- tool          = tool_use blocks (agent calling a tool); AskUserQuestion is a tool_use
- tool-response = tool_result blocks
- agent         = assistant visible end-of-turn text (agent message; "agent includes AskUserQuestion")
COMPLETE x: when a match is found, return the COMPLETE exchange/round-trip, not a fragment — a matched tool_use returned WITH its tool_result; a matched user turn WITH the agent response. Reconstruct exchanges via uuid/parentUuid linking.

FEATURES:
1. list — given path(s), list every session with: session-id, FIRST genuine-user message, LAST genuine-user message, LAST agent message, and each timestamp (for fast "which session is this?"). MUST be fast on 200MB+ files: head-read first K records for the first user msg; read the TAIL by SEEKING from EOF and reading backward for the last user/agent msg — do NOT parse the whole file.
2. search — regex (ripgrep-like; default smart-case, -i, multiline opts), keyword MAY be empty (pure filter), filters: --category/-t (repeatable), --turn-range (by turn index range) OR --since/--until (time range), --session; returns complete x exchanges. A "turn" is delimited by GENUINE user messages (a tool_result-carrier does NOT start a turn).
3. multi-target — repeatable --path (search all sessions across all), all sessions within one path, or a single --session.
4. whoami (OPTIONAL, false-positive-safe) — detect the CALLING Claude Code session. SAFE: read a session-id env var IF Claude Code exposes one — EMPIRICALLY check what CC sets for its Bash tool (run `env | grep -iE "claude|session|anthropic|codex"` inside this very CC session). If a definitive env var exists -> use it (zero false positives, survives bash nesting, per-session, version-independent). If absent/ambiguous -> DO NOT GUESS (multiple CC sessions run concurrently, possibly different versions/binaries) -> ERROR with guidance ("pass --session; your id is in your own context / the jsonl path; or grep a unique recent line to identify yourself"). NEVER use most-recent-mtime as the answer. It is acceptable for whoami to often say "ambiguous, pass --session".

NON-FUNCTIONAL: handle 200MB+ jsonl FAST — mmap + memchr line scan + rayon parallel across files + lazy parse (fast regex/byte prefilter, full serde_json only on candidate lines). NO silent truncation — if capped (--max-count), state how many were dropped. Timestamps displayed in Australia/Sydney local + raw UTC. Complete, example-rich --help for every subcommand. Must pass: cargo fmt --check, cargo clippy --all-targets -- -D warnings, cargo test. Output default = human/LLM-readable with clear session/turn/category/timestamp headers; provide --json for machine use.

---

## EMPIRICAL ADDENDUM (verified 2026-06-07 during Phase-1 scaffold)

Findings from real `~/.claude/projects` data that EXTEND/CONFIRM the brief. Phase 2 folds these into the finalized spec body. Full detail lives in [`CLAUDE.md`](./CLAUDE.md) § 6.

1. **Path encoding rule confirmed:** every non-`[A-Za-z0-9]` byte → a single `-`, with **NO consecutive-dash collapsing**. `.` → `-`, `/` → `-`, `_` → `-`, space → `-`. Evidence: a source `/.claude/` segment encodes to a literal `--claude-` double-dash. Reverse is therefore lossy (confirmed); the real-path-vs-encoded acceptance strategy in the brief stands.

2. **Compaction shape (brief left this "VERIFY"):** the summary is NOT a `type:"summary"` record. It is a `type:"user"` record carrying `isCompactSummary:true` + `isVisibleInTranscriptOnly:true` with **string** content. A separate `type:"system"` `subtype:"compact_boundary"` record holds `compactMetadata:{trigger,preTokens,postTokens,durationMs}`. A compaction summary MUST be excluded from the `user` (genuine-human) category.

3. **Genuine-user is heavily outnumbered:** one real session had 332 genuine string-content + 61 text-block users vs **1619** tool_result-carrier `role:user` records. The classification is the single most load-bearing rule; `model::Record::is_genuine_user` encodes it and is unit-tested.

4. **Real top-level fields far exceed the brief's list** — also present: `attachment`, `file-history-snapshot`, `queue-operation`, `isMeta`, `toolUseResult`, `sourceToolAssistantUUID`, `slug`, `entrypoint`, `promptId`, `permissionMode`, `leafUuid`, `messageId`, `snapshot`, `lastPrompt`, `aiTitle`, `requestId`, `hookInfos`, `durationMs`, `compactMetadata`. The model deserializes only what it uses and ignores the rest. An `image` block type also appears in content arrays.

5. **whoami env var confirmed definitive:** `CLAUDE_CODE_SESSION_ID` (value matched this session's own jsonl basename exactly). `CODEX_COMPANION_SESSION_ID` mirrors it (Codex-plugin-specific). The canonical `CLAUDE_CODE_SESSION_ID` is the one to read; absence ⇒ error-with-guidance, never a guess.

6. **Externalised output pointer format:** inline text `<persisted-output>\nOutput too large (NN KB). Full output saved to: <ABS_PATH>/tool-results/<id>.txt\n\nPreview (first 2KB):\n…`. `--resolve-persisted` will read the pointed-to file.
