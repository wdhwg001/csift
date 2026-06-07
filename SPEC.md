# SPEC.md — csift

> **Status: AUTHORITATIVE (build-ready).** This is the single source of truth for *what csift does and how*. It merges the original project brief with empirically-verified research (real `~/.claude/projects` data, 51 sampled sessions across CC 2.1.133–2.1.168, files up to 225 MB / 115 879 records). **Where the research contradicted the brief, the research wins — each correction is called out inline as `[CORRECTION]`.** [`CLAUDE.md`](./CLAUDE.md) remains authoritative for *how to work in the repo* (git discipline, gates, conventions); this file is authoritative for *what to build*. An engineer who has never seen a Claude Code (CC) `.jsonl` should be able to implement csift from this document alone.

---

## 0. Mission & non-negotiables

**csift = "ripgrep for Claude Code session transcripts".** A fast Rust CLI to **list** and **regex-search** CC session `.jsonl` files. Subcommands: `list`, `search`, `agents` (subagent lifecycle, §6.5), `whoami`, `files` (which files a session changed, §6.6), `recover` (reconstruct a file's content / restore a plan from the transcript, §6.7). `list`/`search`/`files`/`recover` span each session's subagent transcripts by default (`--no-subagents` opts out).

- **Primary consumer is an LLM** (a CC agent searching/recovering its own or a peer session). Output must be clean, token-efficient, and regex-driven. Human/LLM-readable text by default; `--json` for machine use.
- **Explicitly NO BM25 / embeddings / semantic search.** Pure regex/ripgrep only. Lexical tokenisation across scripts (CJK / multi-byte) is intractable for scoring; regex is the strength and the whole point.
- **Quality gates (hard):** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` all green. No `unwrap`/`expect` in library/hot paths (only `main` maps error→exit code, tests may `unwrap`). **No silent truncation** anywhere.

---

## 1. Data location & on-disk layout

Each session lives under the CC projects root (`~/.claude/projects`, honoring `$HOME`):

```
~/.claude/projects/<ENCODED>/
  <session-uuid>.jsonl                                  # the main session transcript (one JSON object per line)
  <session-uuid>/                                       # sibling per-session sidecar dir
    tool-results/<id>.txt                               # externalised large tool outputs (see §4.6)
    subagents/agent-<hex>.jsonl                         # (A) built-in Task/Agent-tool subagent transcript — isSidechain:true
    subagents/agent-<hex>.meta.json                     #     companion {agentType, description, name?, toolUseId}
    subagents/workflows/wf_<id>/agent-<hex>.jsonl       # (B) workflow / OMC workflow-subagent transcript (dominant kind)
    subagents/workflows/wf_<id>/agent-<hex>.meta.json   #     companion {agentType}
    subagents/workflows/wf_<id>/journal.jsonl           # (C) workflow EVENT log {agentId,key,type∈{started,result}} — NOT a transcript
    workflows/wf_*.json  workflows/scripts/*.js         # top-level workflow DEFINITIONS (NOT transcripts — ignore)
```

The three subagent transcript/journal shapes (A)/(B)/(C) and the `agents` subcommand are specified in **§6.5**. The bare `<hex>` (== the record/journal `agentId`) is the canonical agent id; the filename stem is `agent-<hex>`. Kind is the on-disk **path location**, not `agentType`.

`<ENCODED>` is the session's **start cwd**, path-encoded (§2).

**Structural facts the implementation must respect (verified):**

1. **A projects dir is keyed by the session's START cwd, not by every cwd the session visits.** Every top-level `*.jsonl` in a given `<ENCODED>` dir carries a `cwd` field that encodes char-for-char to that dir name (0 anomalies across 11 ground-truth dirs). Deep in-session `cd`s (e.g. `…/beacon/src/beacon/turn_engine`) and subagent cwds appear *inside* the data but do **not** spawn their own top-level project dir. So: `<ENCODED> dir ⇒ its files' start cwd` holds 1:1; the inverse (`any cwd seen ⇒ a dir exists`) does **not**. Never assume it.
2. **Some `<ENCODED>` dirs contain NO top-level `*.jsonl`** — only a nested `<uuid>/` sidecar (subagent transcripts) or a `memory/` dir. `list` must tolerate childless/jsonl-less project dirs (skip them) — never assume every projects dir contains a session file, never crash on one that doesn't. Top-level session enumeration is **non-recursive** (it never descends into the `<uuid>/` sidecar), so it cannot mistakenly pick up a subagent file as a session.
3. **Subagent transcripts use the identical record model** as main transcripts, distinguished by `isSidechain:true`, with a companion `agent-<hex>.meta.json`. csift **discovers + spans subagent transcripts by DEFAULT** on `list`/`search` (built-in + workflow / OMC agents under `subagents/**`); `--no-subagents` restricts to main transcripts. The workflow `journal.jsonl` is an event log, never a transcript, and is always excluded. The dedicated `agents` subcommand (§6.5) reports subagent lifecycle.

---

## 2. Path encoding (the cwd ↔ dir name rule)

### 2.1 Forward encoding (cwd → projects dir name) — DEFINITIVE

```
encoded = re.sub(r'[^A-Za-z0-9]', '-', session_start_cwd)
```

Every byte **not** in `[A-Za-z0-9]` becomes a single `-`, **one-for-one, position-preserving, with NO collapsing of consecutive dashes**. That is the entire algorithm.

- `/`, `_`, `.`, space, `-`, and every other non-alphanumeric → a single `-` each. No special-casing of dots, dot-dirs, or extensions.
- **No dash collapsing.** Two adjacent special chars emit two dashes. **Proof:** the real dir `-Users-testuser-Projects-Acme-widget-factory--cache-worktrees-sunny-meadow` decodes from `/Users/testuser/Projects/Acme/widget_factory/.cache-worktrees/sunny-meadow`: the `/.` (slash then the hidden-dir dot) emits a literal `--`. A collapse rule can never emit `--`, so its mere existence refutes collapsing.
- **Leading `/`** (absolute paths always start with `/`) → a leading `-`. Every encoded dir begins with `-`. No stripping.
- **Trailing separator:** cwds are stored without a trailing slash, so no trailing dash in practice.
- **Case is preserved** (`Users`, `Projects`, `Acme` keep their capitals — NOT lowercased). Letters/digits pass through unchanged.

Worked examples (all verified):

| cwd | encoded dir |
|---|---|
| `/Users/testuser/Projects/widget_app_prototype` | `-Users-testuser-Projects-widget-app-prototype` |
| `/Users/testuser/Projects/Acme/widget_factory-worktrees/main` | `-Users-testuser-Projects-Acme-widget-factory-worktrees-main` |
| `/a/.claude/b` | `-a--claude-b` |

Reference implementation (matches `src/path.rs::encode_cwd`, operates on the char stream, ASCII-alphanumeric pass-through, everything else → `-`):

```rust
pub fn encode_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}
```

### 2.2 Reverse (dir → cwd) is LOSSY — never attempt it

The forward map is many-to-one: `/`, `_`, `.`, space, `-`, … all collapse to `-`. Given a `-` you cannot recover the original byte. Concrete collisions in the real tree: `feature-session-2` (literal `-`) vs `widget_factory` (`_`→`-`) vs `/` separators — all `-`. So `-A-B` could be `/A/B`, `/A-B`, `/A_B`, `/A.B`, … **Do not reverse the encoding.** When the true cwd is needed for an `<ENCODED>` dir, **read the `cwd` field out of the first record of any contained `*.jsonl`** (§2.4) — that is the only reliable recovery.

### 2.3 Target detection — "is this arg a real path, or an already-encoded dir?"

A `list`/`search` target argument is EITHER (a) an actual filesystem cwd or (b) a pre-encoded `<ENCODED>` dir (optionally under `~/.claude/projects/`). Resolve as follows, in order:

1. **Pre-encoded projects-dir token** if: after stripping any leading `~/.claude/projects/` (expand `~`/`$HOME` first), the remainder contains **no `/`**, matches `^-[A-Za-z0-9-]*$` (starts with `-`, only `[A-Za-z0-9-]`), AND `<projects_root>/<arg>` `is_dir()`. → use that dir as-is.
2. **Otherwise treat as a real filesystem path:** canonicalize/absolutize it, apply the §2.1 forward encoding, and look up `<projects_root>/<encoded>`.
3. **Disambiguation belt-and-suspenders:** if an arg could be read both ways, prefer the filesystem-path interpretation **only when** the arg contains a `/` or is absolute. A bare `-…` token with no slashes that exists under the projects root is the encoded form. (This is unambiguous in practice: a real *absolute* cwd always starts with `/` → encodes to a leading `-`, so encoded dirs start with `-` precisely *because* real paths can't be passed as a bare leading-`-` token without a slash.)
4. If neither a real path nor an encoded dir resolves, error with the attempted projects-root path (no silent empty result).

### 2.4 cwd cross-check (recommended integrity step)

Every top-level session record carries `"cwd":"<absolute path>"`. After resolving a target dir, csift MAY re-encode the `cwd` from the first record of a contained file and assert it equals the dir basename — the authoritative confirmation it located the right dir, and the canonical way to recover the human-readable cwd for display.

---

## 3. The jsonl record model (one JSON object per line)

> **Parsing philosophy (load-bearing):** real records carry **far** more fields and `type`s than any fixed list. The model must **tolerate the unknown**: deserialize only what csift uses, ignore everything else, and never crash on a new field, block type, record type, or a missing `timestamp`. Field presence is **ragged** across record types, so nearly every field is `Option<T>`.

### 3.1 Complete enumeration of top-level `type` values (CC 2.1.x, verified)

```
user  assistant  system  attachment  file-history-snapshot  queue-operation
last-prompt  ai-title  agent-name  mode  permission-mode
```
plus version-specific extras observed: `last-prompt`, and the catch-all must absorb any future addition. **`[CORRECTION] There is NO `type:"summary"` record in CC 2.1.x.** The brief's `{type:"summary", leafUuid}` compaction shape is **stale** — zero `summary` records exist across all 51 sampled sessions. Compaction is a `type:"user"` record + a `type:"system"` `compact_boundary` record (§4.7). Detection must key on `isCompactSummary`, and *defensively* still tolerate a legacy `type:"summary"` (older CC) without relying on it.

### 3.2 Common envelope

Present on message-bearing + most event records; **ragged** — model every field `Option`:

| field | rename | type | notes |
|---|---|---|---|
| `type` | — | `Option<String>` | discriminator; keep as String (open set), never enum-panic |
| `uuid` | — | `Option<String>` | record identity; used for round-trip stitching |
| `parentUuid` | `parent_uuid` | `Option<String>` | parent link; `null`/absent at thread roots & metadata records |
| `timestamp` | — | `Option<String>` | ISO8601 UTC `2026-06-07T05:43:00.000Z`; **absent on metadata-only records** |
| `sessionId` | `session_id` | `Option<String>` | the owning session uuid |
| `cwd` | — | `Option<String>` | absolute start cwd (re-encodes to the dir name) |
| `version` | — | `Option<String>` | CC version string (e.g. `2.1.159`) |
| `gitBranch` | `git_branch` | `Option<String>` | |
| `isSidechain` | `is_sidechain` | `Option<bool>` | `true` ⇒ subagent transcript record |
| `userType` | — | `Option<String>` | typically `"external"` |
| `isMeta` | `is_meta` | `Option<bool>` | **TRAP — see §4.2:** system-injected pseudo-turn marker |
| `isCompactSummary` | `is_compact_summary` | `Option<bool>` | compaction-summary marker (§4.7) |
| `isVisibleInTranscriptOnly` | `is_visible_in_transcript_only` | `Option<bool>` | co-set on compaction summaries |
| `subtype` | — | `Option<String>` | on `system` records (§4.7) |
| `content` | — | `Option<serde_json::Value>` | on `system` records (free-form) — keep raw |
| `message` | — | `Option<Message>` | role-bearing payload on user/assistant records |
| `toolUseResult` | `tool_use_result` | `Option<serde_json::Value>` | structured echo on tool-result carriers (§4.6) — keep raw |
| `requestId` | `request_id` | `Option<String>` | on assistant records |
| `promptId` | `prompt_id` | `Option<String>` | on user/assistant records |
| `logicalParentUuid` | `logical_parent_uuid` | `Option<String>` | on `compact_boundary` |

Ignored-but-tolerated extras (do NOT model; serde drops them): `entrypoint`, `permissionMode`, `slug`, `sourceToolAssistantUUID`, `attachment`, `snapshot`, `messageId`, `leafUuid`, `aiTitle`, `lastPrompt`, `hookInfos`, `durationMs`, `level`, … Unknown top-level keys must never cause a parse error (serde ignores unknown fields by default — do **not** add `#[serde(deny_unknown_fields)]`).

### 3.3 `Message`

```rust
pub struct Message {
    pub role: Option<String>,        // "user" | "assistant"
    pub content: Option<Content>,    // string OR block array (§3.4)
}
```
On `assistant` records the raw `message` is the full Anthropic API message object (`model`, `id`, `stop_reason`, `usage`, `diagnostics`, …); csift models only `role` + `content` and ignores the rest.

### 3.4 `Content` — polymorphic string-or-array

```rust
#[serde(untagged)]
pub enum Content {
    Text(String),          // bare string: older format / genuine-user text / compaction summary
    Blocks(Vec<Block>),    // array of typed blocks
}
```

### 3.5 `Block` — typed content blocks (internally tagged on `type`, with Unknown catch-all)

```rust
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text     { text: String },
    Thinking { thinking: String, signature: Option<String> },   // signature = opaque base64, may be absent
    ToolUse  { id: Option<String>, name: Option<String>,
               input: Option<serde_json::Value>,
               caller: Option<serde_json::Value> },              // [RESEARCH-EXTEND] caller:{type:"direct"} seen
    ToolResult { tool_use_id: Option<String>,
                 content: Option<serde_json::Value>,             // string OR array — keep raw, see §4.5
                 is_error: Option<bool> },
    Image    { source: Option<serde_json::Value> },
    #[serde(other)]
    Unknown,                                                     // any future block type — never a parse failure
}
```

Block → category mapping is in §5. Note the `ToolResult` `content` can itself be an array containing `{type:"text",text}`, `{type:"image",…}`, and **`{type:"tool_reference", tool_name}`** ([RESEARCH-EXTEND] — emitted by ToolSearch results); model it as raw `Value` and inspect as needed.

### 3.6 serde shape requirements (summary for the implementer)

- Top-level `Record`: a struct with `#[serde(default)]` on every field (ragged presence) and `rename` for camelCase→snake_case. **Do not** model it as an externally-tagged enum on `type` — too many open-set variants; keep `type` as `Option<String>` and branch in logic. (The existing `src/model.rs` does exactly this; extend it, don't restructure.)
- `Content`: `#[serde(untagged)]` string|array.
- `Block`: internally `#[serde(tag="type", rename_all="snake_case")]` + `#[serde(other)] Unknown`.
- `tool_result.content` and `system.content`: raw `serde_json::Value` (string|array|object).
- Metadata records (`last-prompt`/`ai-title`/`agent-name`/`mode`/`permission-mode`/`file-history-snapshot`) parse into `Record` with `timestamp == None` — **never `unwrap()` a timestamp**; skip such records in all time logic.
- **Zero-copy where it pays:** for the full-scan path, prefer `serde_json::from_slice` deserializing borrowed `&str`/`Cow<str>` out of the mmap slice (`#[serde(borrow)]`) to avoid per-field allocation. (See §7.)

---

## 4. Record semantics (the rules that hinge everything)

### 4.1 Genuine-user vs tool_result-carrier (THE load-bearing distinction)

A `type:"user"` record is **NOT** always a human turn. `tool_result` blocks ride on `role:"user"` records — such a record is a tool-result *carrier*, not human input. Magnitudes are extreme: in one real session **177 genuine** users (167 string + 10 text-block) vs **8 675 tool_result-carriers** (98% of `user` records); another had 332+61 genuine vs 1 619 carriers. Getting this wrong corrupts both the `user` category and turn-delimiting.

**`is_genuine_user(record)` is true iff ALL of:**
1. `type == "user"` AND `message.role == "user"`; AND
2. `isCompactSummary` is falsey (excludes compaction summaries, §4.7); AND
3. **`isMeta` is falsey** ([RESEARCH-EXTEND] — excludes system-injected pseudo-turns, §4.2); AND
4. content is a **string**, OR content is a block array containing a `text` block and **no** `tool_result` block.

(`text` and `tool_result` never co-occur in one user record in real data, so "has a `text` block" is a clean genuine signal once carriers and meta are excluded.) The current `src/model.rs::is_genuine_user` implements (1),(2),(4); **Phase 2 must add (3) the `isMeta` guard.**

**Documented exception — AskUserQuestion answers count as "user":** the answer to an AUQ arrives as a `tool_result` (§4.4). For the **`user` category** that answer is reported as a user turn (it *is* the user's input). For **turn-delimiting**, it does **not** start a new turn — it belongs to the turn whose assistant `tool_use` posed the question. (I.e. the `user` *category* membership and the *turn boundary* are decided by different predicates — see §5 and §6.4.)

### 4.2 `isMeta:true` pseudo-turns — TRAP, must be excluded ([CORRECTION/EXTEND])

`isMeta:true` `user` records have string/text content that *looks* human but is **system-injected**, e.g. `"Continue from where you left off."`, `"# Autonomous loop tick"`, `"Stop hook feedback: …"`, `"<local-command-caveat>…"`, `"[Image: source: …]"`. These are **not** genuine human input and must be excluded from the `user` category **and** from turn-delimiting. The brief did not mention `isMeta`; it is load-bearing.

### 4.3 `tool_use` (assistant calling a tool)

Assistant `tool_use` blocks → category `tool`. `name` identifies the tool; `input` is an arbitrary per-tool JSON object. `AskUserQuestion` is a `tool_use` (§4.4). A `caller` field (`{type:"direct"}`) may be present.

### 4.4 AskUserQuestion

A `tool_use` block with `name == "AskUserQuestion"`. Input shape:
```
input.questions: [ { question, header, multiSelect: bool,
                     options: [ { label, description } ] } ]
```
**HARD-WON (verified across all 51 sessions, 0 counterexamples):** a **pending/unanswered** AUQ is **never** flushed to jsonl. Only answered AUQs appear. The answer arrives as a `tool_result` whose `content` is a synthesized string:
```
User has answered your questions: "<question>"="<chosen label>". You can now continue…
```
and the carrier's top-level `toolUseResult` echoes the full `questions[]` structure + the selection. **Implication:** csift never needs to render a "pending question" state — if an AUQ `tool_use` is on disk, its answer is too. For round-trip reconstruction (§6.4), an AUQ `tool_use` and its answering `tool_result` are a complete pair like any other tool round-trip; additionally the answer is surfaced under the `user` category (§4.1 exception).

### 4.5 `tool_result` (tool's response) → category `tool-response`

Lives **inside a `type:"user"` carrier record's** `message.content` array, never on assistant. Fields: `tool_use_id?`, `content` (string OR array of `{type:text,text}`/`{type:image}`/`{type:tool_reference,tool_name}`), `is_error?`. Linkage to the originating `tool_use` is `tool_result.tool_use_id == tool_use.id`; **some legacy carriers omit the block-level `tool_use_id`** — fall back to the carrier's top-level `toolUseResult`/`sourceToolAssistantUUID` to resolve linkage.

### 4.6 Externalised (persisted) large outputs ([CORRECTION/EXTEND] — cleaner pointer than the brief implied)

When a tool output is too large, the inline `tool_result.content` string is a pointer:
```
<persisted-output>
Output too large (NNN KB). Full output saved to: <ABSOLUTE_PATH>

Preview (first 2KB):
…
</persisted-output>
```
`<ABSOLUTE_PATH>` = `<ENCODED>/<session-uuid>/tool-results/<id>.txt` (`<id>` is a tool short-id like `b070yh2rb` or a hook form `hook-<uuid>-<n>-additionalContext`). **In addition**, the carrier's top-level `toolUseResult` object carries **structured** `persistedOutputPath` + `persistedOutputSize` (bytes) fields. **Resolution rule:** when `search --resolve-persisted` is set, read the file at `toolUseResult.persistedOutputPath` (exact — no regex on the inline marker needed); only fall back to regex-scraping the inline `Full output saved to:` path if the structured field is absent. Default (flag off): leave the inline pointer as-is (token economy). Resolution failures (missing file) are reported, never fatal.

### 4.7 `type:"system"` records & compaction ([CORRECTION])

`system` records carry the common envelope + `subtype` (+ `level?` ∈ {`info`,`error`}). Subtypes (complete, verified):

| subtype | key fields |
|---|---|
| `stop_hook_summary` | `hookCount`, `hookInfos:[{command,durationMs}]`, `hookErrors`, `preventedContinuation`, `stopReason`, `hasOutput`, `toolUseID` |
| `turn_duration` | `durationMs`, `messageCount` |
| `away_summary` | `content` (short auto-summary of what the session was doing when it went idle; free text, often non-English) |
| **`compact_boundary`** (= compaction metrics) | `content:"Conversation compacted"`, `logicalParentUuid`, `compactMetadata:{ trigger:"auto"\|"manual", preTokens, durationMs, preCompactDiscoveredTools[], preservedSegment:{headUuid,anchorUuid,tailUuid}, preservedMessages:{anchorUuid,uuids[],allUuids[]} }` |
| `scheduled_task_fire` | `content` (e.g. `"Claude resuming /loop wakeup …"`), `slug` |
| `local_command` | `content` (`<command-name>…</command-name>` wrapper), `level` |
| `api_error` | `level:"error"`, `error:{status, headers{…}, …}` |

**Compaction = TWO records, NOT a `type:"summary"`:**
1. The **summary text** is a `type:"user"` record with `isCompactSummary:true` + `isVisibleInTranscriptOnly:true`, **string** content (commonly starting `"This session is being continued from a previous conversation…"`). → **excluded from genuine-user** (§4.1 rule 2).
2. The **metrics** are the `type:"system"` `subtype:"compact_boundary"` record above.

`system` records are not a default search category (they have no category in §5); they are skippable noise unless a future flag opts in. They still parse cleanly and must never crash time logic.

### 4.8 `attachment` & other event records — skippable noise

`attachment` is the **dominant** record type (54% of all records in the largest file — mostly `{attachment:{type:"hook_success",…}}` SessionStart spam). ~21 attachment subtypes exist (`hook_success`, `hook_additional_context`, `task_reminder`, `date_change`, `plan_mode*`, `deferred_tools_delta`, `ultrathink_effort`, `compact_file_reference`, `edited_text_file`, `queued_command`, `skill_listing`, `nested_memory`, `selected_lines_in_ide`, `hook_cancelled`/`hook_blocking_error`, `file`, …). **None map to a search category** — they are dropped pre-JSON by the prefilter (§7) and absorbed by the tolerant model. `file-history-snapshot` nests its timestamp inside `snapshot` (not top-level). `queue-operation` has a top-level `timestamp` + `operation ∈ {enqueue,dequeue,remove,popAll}`. Metadata records (`last-prompt`/`ai-title`/`agent-name`/`mode`/`permission-mode`) have **no `timestamp`/`uuid`** — skip in time/turn logic.

---

## 5. Category mapping (`search -t/--category`, repeatable)

| Category (CLI value) | Source | Rule |
|---|---|---|
| `thinking` | assistant `thinking` block | always |
| `user` | genuine human turn **+** AUQ answers | `is_genuine_user(record)` per §4.1 (string or text-only, non-meta, non-carrier, non-compaction); PLUS `tool_result` blocks that are AUQ answers (§4.4) |
| `tool` | assistant `tool_use` block | includes `AskUserQuestion` |
| `tool-response` | `tool_result` block (on user-carrier records) | excludes AUQ answers *as a category member*? **No** — an AUQ answer IS a `tool_result`, so it belongs to both `tool-response` (it is a tool_result) and `user` (the §4.1 exception). A category filter that names either will surface it; do not double-count within a single emitted exchange. |
| `agent` | assistant visible `text` block | the agent's end-of-turn message ("agent includes AskUserQuestion" framing — an AUQ `tool_use` is reachable via both `tool` and the `agent` turn it sits in) |

When no `--category` is given, **all five** categories are eligible (search everything except pure-noise records: `system`, `attachment`, `file-history-snapshot`, `queue-operation`, metadata records are never category matches).

---

## 6. Subcommand specifications

> **Common conventions.** All timestamps display as **system-local timezone + raw UTC** side-by-side (e.g. `2026-06-07 15:43:00 AEST (2026-06-07T05:43:00.000Z)` on a machine in Sydney; the `<TZ>` abbrev and offset auto-track the detected local zone) via `jiff` (`TimeZone::system()`). All subcommands accept `--format text|json` (default `text`). Text output is headered and LLM-friendly; JSON is one object per emitted unit, deterministic order. Errors go to stderr with the full `anyhow` chain; exit code 0 on success, non-zero on error. **No silent truncation** — any cap reports the drop count.

### 6.1 `list` — "which session is this?" fast index

**Purpose.** For each session under the target(s), emit the quick-identity tuple without parsing the whole file.

**Args (matches `cli::ListArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | a real cwd OR an encoded dir (§2.3); 0 args ⇒ every dir under projects root |
| `--format text\|json` | enum | `text` | output format |
| `--include-subagents` | bool | `true` | also list each session's subagent transcripts (built-in `subagents/agent-<hex>.jsonl` + workflow `subagents/workflows/wf_*/agent-<hex>.jsonl`); **default ON**. Workflow `journal.jsonl` is excluded (not a transcript). |
| `--no-subagents` | bool | — | restrict to top-level `<uuid>.jsonl` sessions only (overrides `--include-subagents`) |
| `--limit N` | usize | none | *(Phase-2 add)* cap rows; reports dropped count |

**Per-session fields emitted:** `session-id`, **first** genuine-user message (+ts), **last** genuine-user message (+ts), **last** agent message (+ts), the session `cwd` (decoded from data, §2.4), `version`, `gitBranch`. Each message is a one-line excerpt (truncated with an explicit `… (+N chars)` marker — never silent).

**Algorithm (NON-FUNCTIONAL: must be fast on 200 MB+):**
1. Resolve target dir(s) (§2.3); enumerate `*.jsonl` directly under each (skip childless dirs, §1).
2. `rayon` `par_iter` across all session files (across-file parallelism, §7e).
3. Per file: **head-read** forward in 64 KiB chunks from offset 0, lazy-parsing only candidate lines, until the **first** `is_genuine_user` record is found (typically within the first few KB). **Tail-read** by seeking from EOF backward (§7b) until **both** the last genuine-user and last assistant-`text` (agent) records are found.
4. Collect into an order-stable `Vec`, render.

**Text output example:**
```
SESSION  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  cwd      /Users/testuser/Projects/widget_app_prototype   (branch main, CC 2.1.159)
  first ◂  2026-06-07 14:01:09 AEST (2026-06-07T04:01:09.123Z)
           Write the AUTHORITATIVE SPEC.md for csift…
  last ◂   2026-06-07 15:48:22 AEST (2026-06-07T05:48:22.880Z)
           also add an --include-subagents flag please
  last ▸   2026-06-07 15:49:10 AEST (2026-06-07T05:49:10.004Z)
           Done — SPEC.md rewritten; summary below.
```
(`◂` = user, `▸` = agent.)

**Example invocations:**
```bash
csift list                                              # every session, all projects
csift list .                                            # sessions for the current cwd's project
csift list /Users/testuser/Projects/widget_app_prototype
csift list -Users-testuser-Projects-widget-app-prototype   # pre-encoded dir
csift list ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype
csift list --format json .                              # machine-readable index
```

### 6.2 `search` — regex over transcripts, returns complete round-trip exchanges

**Purpose.** ripgrep-like regex search returning the **complete round-trip** containing each hit, never a bare fragment.

**Args (matches `cli::SearchArgs`):**
| flag | short | type | default | meaning |
|---|---|---|---|---|
| `[PATTERN]` | — | positional string | `""` (empty ⇒ pure filter) | regex, ripgrep-like, default **smart-case** |
| `--path PATH` | — | repeatable | all projects | target(s): real cwd or encoded dir (§2.3) |
| `--session ID` | — | string | none | restrict to one session uuid |
| `--category C` | `-t` | repeatable enum | all | one of `thinking\|user\|tool\|tool-response\|agent` (§5) |
| `--ignore-case` | `-i` | bool | smart-case | force case-insensitive |
| `--multiline` | — | bool | false | let `.` cross newlines / multiline mode |
| `--turn-range START..END` | — | string | none | inclusive 0-based turn index range; **mutually exclusive** with `--since`/`--until` |
| `--since WHEN` | — | string | none | lower time bound (ISO8601 or relative, e.g. `2h`, `2026-06-01`) |
| `--until WHEN` | — | string | none | upper time bound |
| `--max-count N` | — | usize | none | cap emitted exchanges; **reports dropped count** |
| `--resolve-persisted` | — | bool | false | resolve `<persisted-output>` pointers (§4.6) |
| `--include-subagents` | — | bool | `true` | also search each in-scope session's subagent transcripts (built-in + workflow / OMC agents under `subagents/**`); **default ON**. Workflow `journal.jsonl` is never searched (not a transcript). |
| `--no-subagents` | — | bool | — | search only top-level `<uuid>.jsonl` sessions (overrides `--include-subagents`) |
| `--format text\|json` | — | enum | `text` | output format |
| `--context N` | `-C` | usize | full exchange | *(Phase-2 add)* optionally trim the exchange to ±N surrounding records |

**Smart-case rule:** pattern is case-insensitive iff it contains no uppercase letter; `-i` forces insensitive regardless; the two never conflict (`-i` wins). Compile via `regex::bytes::RegexBuilder` (match on raw line bytes pre-JSON in the prefilter, then on decoded text for excerpting). `--multiline` sets `.dot_matches_new_line(true)` + multiline mode.

**Regex dialect — linear-time (RE2-class).** The pattern is the Rust `regex` 1.12 crate (`regex::bytes`), which **guarantees linear-time matching** in the input length: **no catastrophic backtracking, ever**. *Supported:* literals; character classes `[...]` / `[^...]` / `\d \w \s` + Unicode classes `\p{...}`; alternation `|`; groups `(...)` / non-capturing `(?:...)`; quantifiers `* + ? {m,n}` (greedy + lazy `*?`); anchors `^ $ \b \B`; dot `.` (`--multiline` lets it cross newlines); inline flags `(?i)(?m)(?s)(?x)`; Unicode-aware by default. ***Deliberately NOT supported*** (they require a non-linear engine): backreferences `\1`; lookahead/lookbehind `(?=) (?!) (?<=) (?<!)`; atomic groups / possessive quantifiers `(?>...)` / `a*+`. A pattern using these **fails to compile** with a clear error — by design, not a bug. This boundary is documented identically in `--help` (`search`'s `after_help`) and `SKILL.md`.

**Validation:** `--turn-range` together with `--since`/`--until` is an error (mutually exclusive). An empty `PATTERN` with no other filter is allowed (matches every category-eligible record) but should warn that it will emit a lot.

**Filter application order per session:** `--session`/`--path` (target selection) → category eligibility (§5) → time/turn window → regex match → round-trip reconstruction (§6.4) → `--max-count` cap (with drop accounting).

**Time window:** `--since`/`--until` compare against each record's `timestamp` (UTC). Records with no timestamp (metadata/noise) are never time-matched. Relative forms (`2h`,`3d`,`90m`) are resolved against *now* in the system-local timezone then converted to UTC for comparison.

**Output — complete round-trip:** see §6.4. Each emitted unit is one **Exchange** with a session header, turn index, and the matched hit(s) shown in context of their full round-trip.

**Text output example:**
```
═══ SESSION 0a1b2c3d… · TURN 47 ═══
◂ user  2026-06-07 14:32:01 AEST (…T04:32:01Z)
   why is the tail-read carry needed?
▸ thinking  …T04:32:05Z
   The carry holds an incomplete line straddling a chunk boundary…   ◀ match: "carry"
▸ agent  …T04:32:40Z
   The carry is the partial line at the low-offset edge of each chunk…
matched 1 exchange (category=thinking)  ·  0 dropped
```

**Example invocations:**
```bash
csift search "carry"                                   # all projects, smart-case
csift search -i "askuserquestion" -t tool             # tool_use blocks naming AUQ
csift search "" -t user --since 2h --path .            # pure filter: genuine user turns, last 2h, this project
csift search "tail.read" --multiline --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
csift search "panic" -t agent -t thinking --turn-range 10..20 --max-count 50
csift search "persisted-output" --resolve-persisted --format json
```

### 6.3 `whoami` — identify the calling CC session (false-positive-safe)

**Strategy ([CORRECTION] — the env var EXISTS; the brief left this conditional):**

1. **Primary — read `CLAUDE_CODE_SESSION_ID`.** Verified definitive: CC exports it into every Bash-tool environment and its value equals the calling session's own jsonl basename exactly (e.g. `0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d` → `…/<ENCODED>/0a1b2c3d-….jsonl`). Per-session, version-independent, survives arbitrary bash nesting, **zero false positives**. If set and a non-empty UUID, that **is** the answer. Match the **exact** name `CLAUDE_CODE_SESSION_ID` — never a loose `/session/i` regex (`SECURITYSESSIONID`, the macOS login session, is a false-positive trap; `CODEX_COMPANION_SESSION_ID` mirrors the value but is Codex-plugin-specific — accept it only as a secondary alias, prefer the canonical var).
2. Resolve its transcript: encode `$PWD` (or an explicit `--path`) to the `<ENCODED>` dir and open `<ENCODED>/$CLAUDE_CODE_SESSION_ID.jsonl`. If `$PWD` doesn't resolve, scan projects-root dirs for the one containing `<id>.jsonl`.
3. **Fallback when the var is absent/empty — ERROR with actionable guidance, never guess.** Message: your session id was not found (`CLAUDE_CODE_SESSION_ID` unset — old CC build or running outside CC); pass `--session <uuid>`; your id is the basename of your own transcript, or grep a unique recent line you wrote to disambiguate; **do NOT trust most-recent-mtime** (many CC sessions may be live concurrently). It is acceptable for `whoami` to often say "ambiguous, pass --session".
4. **FORBIDDEN as a whoami source:** process-tree walk and most-recent-mtime. (Evidence: 83 concurrent `claude` processes + 6 installed CC versions on one machine ⇒ ~83-way ambiguity and cross-version argv brittleness; the UUID isn't even on the process command line. mtime with 83 live sessions is almost always wrong.)

**Args (matches `cli::WhoamiArgs`):** `--path` (also print the resolved jsonl path) — *Phase-2: rename/extend to accept an explicit cwd `--cwd PATH` for transcript resolution*; `--format text|json`.

**Text output:**
```
session  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
path     ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl
```
**Error output (var absent):** non-zero exit + the guidance string from `whoami::AMBIGUOUS_GUIDANCE`.

### 6.4 Round-trip (turn / exchange) reconstruction algorithm

**A "turn" is delimited by a GENUINE user message** (§4.1) — a `tool_result`-carrier and an `isMeta` pseudo-turn never start a turn; a compaction summary never starts a turn. Turn index is 0-based in genuine-user order within a session.

**Records form a tree via `uuid`/`parentUuid`** (each record's `parentUuid` points at the record it follows). A single turn typically expands to a chain: `genuine-user → assistant(thinking…/tool_use…/text) → user-carrier(tool_result) → assistant(…) → …` until the next genuine-user.

**Reconstruction (per session, single forward pass after collecting records):**
1. Build `by_uuid: HashMap<uuid, &Record>` and a `children` adjacency (parentUuid → [child uuids]) over the session's records.
2. Walk records in file order; each `is_genuine_user` record opens a new **Exchange** (turn index ++). All subsequent records up to (but excluding) the next genuine-user belong to that exchange.
3. **Round-trip completeness rules when a hit lands:**
   - A matched **`tool_use`** is returned **with its `tool_result`** — pair by `tool_result.tool_use_id == tool_use.id` (fallback via `toolUseResult`/`sourceToolAssistantUUID`, §4.5). Both blocks appear in the emitted exchange even if only one matched.
   - A matched **genuine-user turn** is returned **with the agent response** — i.e. the assistant `text`/`thinking`/`tool_use` records chained under it in the same turn.
   - A matched **`tool_result`** is returned **with the `tool_use` that produced it** (reverse of the above pairing).
   - A matched **`thinking`/`agent` text** is returned within its full turn (the opening genuine-user + sibling assistant records).
4. The emitted Exchange carries: `session_id`, `turn_index`, the list of `Hit`s (category + excerpt + UTC timestamp), and `record_uuids` (every record stitched in, for traceability) — matching `search::Exchange`.
5. **AUQ pairing:** an `AskUserQuestion` `tool_use` and its answering `tool_result` (§4.4) are one pair; the answer is also surfaced under `user` per §4.1 but is **not** a turn boundary.
6. **Compaction continuity:** a `compact_boundary`'s `logicalParentUuid`/`preservedSegment` may be used (best-effort) to keep turn indices monotonic across a compaction, but turn delimiting still keys on genuine-user records; never crash if these fields are absent.

### 6.5 `agents` — a session's subagent lifecycle (kind, start/completion, status)

**Purpose.** List the subagent transcripts a session spawned, with per-subagent identity + lifecycle, and an optional time-window filter (which subagents started/completed within a bound). Complements `--include-subagents` on `list`/`search` (which fold subagent *content* into those views); `agents` is the *lifecycle index* of those same subagents.

**Subagent on-disk layout (empirically mapped against `~/.claude/projects`, 0 linkage mismatches across 600+ nested transcripts). Three shapes under a top-level session's sidecar `<ENCODED>/<session-uuid>/`:**

| kind (csift) | path | meta.json | record markers |
|---|---|---|---|
| `builtin-task` | `subagents/agent-<hex>.jsonl` | `{agentType, description, name?, toolUseId}` | `isSidechain:true`, `agentId == <hex>`, `sessionId == <session-uuid>` |
| `workflow` | `subagents/workflows/wf_<id>/agent-<hex>.jsonl` | `{agentType}` | same; `cwd` is often a DEEPER in-session path — never re-encode a subagent cwd |
| *(excluded)* | `subagents/workflows/wf_<id>/journal.jsonl` | — | **NOT a transcript**: `{agentId, key, type∈{started,result}}`, no `message`/role |

**Kind is the on-disk PATH LOCATION, not `agentType`.** Both `builtin-task` and `workflow` carry the same spread of `agentType` values (`Explore`, `general-purpose`, `oh-my-claudecode:*`); only `workflow-subagent` is workflow-exclusive. So `agentType` is a descriptive sub-label, not the discriminator. **The canonical agent id is the bare `<hex>`** (the record/journal `agentId`), not the `agent-<hex>` filename stem.

**Linkage to the parent session** is FILESYSTEM-primary (the enclosing `<session-uuid>` dir name) corroborated by the record `sessionId` (600/0 verified). Top-level dir enumeration + `whoami` are safe from matching subagents because subagent files are named `agent-<hex>.jsonl` (never `<uuid>.jsonl`) and sit in a nested dir the non-recursive top-level scan never descends.

**Args (matches `cli::AgentsArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3) whose sessions' subagents to list; optional when `--session` is given |
| `--session ID` | string | none | restrict to one parent session uuid |
| `--kind builtin-task\|workflow` | repeatable enum | all | filter to a subagent kind |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`/…, system-local), filters by the `--by` axis |
| `--by start\|completion` | enum | `start` | which lifecycle timestamp `--since`/`--until` filter on |
| `--format text\|json` | enum | `text` | output format |

**Per-subagent fields emitted:** `agent_id` (bare hex), `kind`, `workflow_id` (workflow only), `agent_type` (meta `agentType`), `description` (built-in meta), `started_utc`/`started_local` (first transcript record ts), `completed_utc`/`completed_local` (last transcript record ts), `duration`, `status`, `parent_session_id`.

**Status resolution (honest — never over-claims "failed"):** `completed` when a workflow `journal.jsonl` carries a `result` event for the agent OR the transcript terminates with a visible assistant end-of-turn message; `running` when records exist with a start but no completion signal; `unknown` when no timestamps are determinable.

**Time window** is the same semantics as `search` (§6.2): records/rows with no timestamp on the chosen axis are never admitted by a *bounded* window; an unbounded window admits all.

**Example invocations:**
```bash
csift agents --session 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d   # one session's subagents
csift agents .                                                # every session under this project
csift agents . --kind workflow                                # only workflow agents
csift agents --session <uuid> --since 2h                      # subagents STARTED in the last 2h
csift agents --session <uuid> --since 09:00 --by completion   # COMPLETED since a bound
csift agents . --format json                                  # machine-readable lifecycle rows
```

### 6.6 `files` — which files/dirs a session modified, when

**Purpose.** Report the files + directories a session changed, attributed to genuine-user turns, with a context-blow-up guard (a compact per-dir **summary** is the default; the full chronological list is opt-in). Answers the acid-test question "how many distinct gap docs did this session touch, and how many `/tmp` docs did it create?".

**Extraction (verified against the live `~/.claude/projects` corpus, 2026-06-08) — authoritative vs heuristic:**

| source | how | create-vs-edit | label |
|---|---|---|---|
| `Edit` / `Write` / `MultiEdit` | `input.file_path` | from paired `toolUseResult.type` (`create` ⇒ new file; `update`/`file_unchanged` ⇒ edit), joined by `tool_use_id` within the turn | authoritative |
| `NotebookEdit` | `input.notebook_path` | same join | authoritative |
| `Bash` | LEXICAL parse of `input.command` (`rm`/`mv`/`cp`/`mkdir`/`touch`/`tee`/`sed -i`/`git`/redirection) | not knowable lexically (heuristic guess) | **HEURISTIC** — always labelled `(heuristic)` |

Bash's `toolUseResult` is `{stdout, stderr, interrupted, isImage, noOutputExpected}` — **no path field** — so Bash mutations are a best-effort lexical (NOT shell) parse and are flagged heuristic everywhere they surface (text, JSON, help, SKILL). The `file_path` lives on the **tool_use** record while `toolUseResult.type` (`create`/`update`) lives on the **paired tool_result carrier**; the joiner pairs them by `tool_use_id` within the turn so `is_create` is accurate. Relative Bash paths are reported VERBATIM (the session's cwd at command time is not reliably known — absolutizing would fabricate a path).

**Args (matches `cli::FilesArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); optional when `--session` is given |
| `--session ID` | string | none | restrict to one parent session uuid |
| `--include-subagents` / `--no-subagents` | bool | `true` | attribute SUBAGENT mutations under the session (default ON — OMC fan-out edits happen in subagents) |
| `--summary` / `--by-dir` / `--by-file` / `--timeline` | mutually-exclusive enum (clap group) | `--summary` | detail level (below) |
| `--turn-range START..END` | string | none | inclusive 0-based turn range; **mutually exclusive** with `--since`/`--until` |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`/…, system-local) |
| `--format text\|json` | enum | `text` | output format |

**Detail levels (the context-blow-up guard — exactly one is active, default `--summary`):**
- **`--summary` (DEFAULT)** — compact per-top-level-dir rollup with op counts (e.g. `"/tmp: 12 write, 3 edit; spec/gaps: 4 edit"`); the smallest output, answers the acid test directly. Bucket = the mutation path's parent directory; a bare relative filename buckets under `./`.
- **`--by-dir`** — one row per distinct directory (full path) with per-op counts + distinct-file count + first/last timestamp.
- **`--by-file`** — one row per distinct absolute path with per-op counts + first/last timestamp (where "how many distinct gap docs touched" is exactly answerable).
- **`--timeline`** — full chronological list, one line per mutation `(timestamp, turn index, op, path)`. The verbose mode; never the default.

**Filtering** is per-mutation: `--turn-range` (turn index assigned by the §6.4 genuine-user delimiter, shared with `search`) and `--since`/`--until` (a mutation with no timestamp never falls inside a *bounded* window — same rule as §6.2). **No silent truncation:** skipped malformed lines are counted and surfaced.

**Output.** Text groups under a `SESSION <id>` header, then the level-appropriate body, then a footer: distinct files + total mutations, the active detail level, the turn/time filter context, the Bash-heuristic caveat, and the skipped-line count. Empty result prints `no file mutations found`. JSON is one object per emitted unit (bucket / dir / file with the counts + first/last; or per mutation for `--timeline` with `{path, op, ts_utc, ts_local, turn_index, is_create, heuristic}`), then a trailing summary object `{distinct_files, total_mutations, skipped_lines, detail_level}` (mirrors §6.2's trailing-summary convention).

**Perf shape** is `search`'s: a single forward pass per file (mmap + SIMD newline scan + a pre-JSON mutation byte-prefilter, full parse only on candidate lines), no large-blob retention (extract small `FileMutation` strings, drop the record — never hold `originalFile`/`content`/`structuredPatch`), rayon across files on the default pool (= CPU count).

**Example invocations:**
```bash
csift files <uuid>                          # default summary: per-top-level-dir op rollup
csift files <uuid> --by-file                # per-file op counts + first/last touch
csift files <uuid> --timeline --since 2h    # full chronological, last 2h (heavy)
csift files . --format json --by-dir        # machine-readable per-dir rollup
```

### 6.7 `recover` — reconstruct a file's content (or a plan) from the transcript

**Purpose.** Where `files` (§6.6) only reports THAT a file was touched, `recover` rebuilds the file's **content** by replaying its Read / Write / Edit stream in transcript order. Four mutually-exclusive modes (clap group `mode`, default `--patches`): segmented diff-patch history (`--patches`), a point-in-time partial snapshot (`--at`), coverage/scoping (`--coverage`, alias `--dry-run`), or plan restoration (`--plan`). The motivating use is restoring a deleted plan or a file lost in a bad-recovery. **Every output reference carries the JSONL line number** (`Lnnnnn`) so a consumer can `Read` the raw jsonl directly — the one genuinely-new capability over the other subcommands (added via a local line counter threaded through `scan_lines_bytes`; the shared signature is untouched).

**Extraction (verified against the live corpus, 2026-06-08).** Per `--file`, in transcript order:

| event | source | result |
|---|---|---|
| full Read | `toolUseResult.file` with `startLine==1 && numLines==totalLines` (content is RAW, no gutter) | a **full-snapshot anchor** |
| windowed Read | `toolUseResult.file` with `startLine>1` or `numLines<totalLines` | a **partial splice** (gaps stay gaps — never padded) |
| Write | `toolUseResult.{type:create\|update, content}` | a full-snapshot anchor |
| Edit / MultiEdit | `toolUseResult.{oldString,newString,structuredPatch,originalFile}` (no `type`) | applied to the running buffer by `structuredPatch` line position (string-replace fallback) |
| integrity error | a `tool_result` with `is_error` whose body is `File has been modified since read` / `File has not been read yet` (no inline path → attributed via the `tool_use_id`↔tool_use join) | a HARD boundary (modified-since-read) / a non-boundary (not-read-yet) |
| Bash mutation | `input.command` lexically parsed (§6.6) | a HEURISTIC (soft) boundary |
| `edited_text_file` attachment | `attachment.{filename,snippet}` (TAB **or** U+2192 gutter) | a HARD boundary (external edit) |
| `file-history-snapshot` | `snapshot.trackedFileBackups[<path>]` | a coverage ANNOTATION only — the on-disk blob name is NOT derivable (the real `backupFileName` is frequently `null`), so it is **never** used to fabricate content |

**Reconstruction model (the "in the LLM's eyes" sparse buffer).** A `BTreeMap<file_line, cell>`; a line absent from the map is an **explicit gap** (unknown — never fabricated). A full snapshot resets the map; a windowed read splices without padding; an edit applies by structured-patch position. **Anti-fabrication guards (load-bearing):** an edit whose old region falls in an unknown gap, whose anchor neighbourhood is entirely unknown, or whose patch context disagrees with the buffer is an **un-anchorable coverage hole** (counted, never asserted as a known line). This is what keeps the reconstructed-vs-disk guarantee honest: the contiguous-from-line-1 prefix matches disk byte-for-byte even on a heavily-edited file with no clean anchor.

**Integrity boundaries (a point where reconstruction across it is invalid), in confidence order:** (1) AUTHORITATIVE — a `modified since read` harness error; (2) AUTHORITATIVE — an Edit whose `originalFile` disagrees with the replayed buffer (the signal claude-file-recovery *discards*, csift mines); (3) AUTHORITATIVE — an external `edited_text_file`; (4) HEURISTIC — a Bash mutation (always flagged best-effort). `--patches` emits one unified diff per segment between hard boundaries, each segment + boundary carrying its jsonl line / turn / timestamp; **no diff spans a boundary**.

**Args (matches `cli::RecoverArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); optional with `--session` |
| `--session ID` | string | none | restrict to one parent session uuid |
| `--file ABS_PATH` | string | none | the file to reconstruct (exact raw-string match + basename-suffix fallback); **REQUIRED** for `--patches`/`--at`/`--coverage`, OPTIONAL for `--plan` |
| `--include-subagents` / `--no-subagents` | bool | `true` | span subagent transcripts (OMC fan-out edits happen there) |
| `--patches` / `--at WHEN` / `--coverage` (alias `--dry-run`) / `--plan` | clap group `mode` | `--patches` | the reconstruction mode |
| `--turn-range START..END` | string | none | inclusive 0-based turn range; mutually exclusive with `--since`/`--until` |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 / relative) |
| `--line-range START..END` | string | none | 1-based inclusive file-line span to restrict the reconstructed line space |
| `--out PATH` | path | none | write the reconstructed artifact (snapshot / plan / concatenated patches) verbatim; the summary still prints to stdout |
| `--format text\|json` | enum | `text` | output format |

`--at <WHEN>` accepts ISO8601, relative (`2h`), `@turn:<N>`, or `@line:<N>` (state as of jsonl line N) and doubles as the snapshot cutoff. `--plan` collects every `ExitPlanMode` `input.plan` + plan-ish Writes in range, restores the latest (by timestamp, then line), and cites its `Lnnn`/turn/ts provenance; `--dry-run` lists candidates without dumping bodies.

**Output.** Text groups under `SESSION <id>`. `--coverage`: recoverable-line fraction, covered ranges, per-op counts, the integrity-boundary list (`⚠` authoritative / `~` heuristic), fragment count (= boundaries + 1). `--patches`: interleaved segment headers (`─ SEGMENT n  L..L  turns..  ts..  (pre-state…)`) and boundary dividers, with the unified diffs. `--at`: line-numbered known lines + explicit `??? lines A..B unknown` gap markers. JSON is NDJSON (one object per segment/boundary/snapshot/plan-candidate, `line_no` + `ts_utc`/`ts_local` on every object, `set_at_line` provenance per reconstructed line) + a trailing summary. **No silent truncation** — long inline content uses the `… (+N chars)` marker; JSON + `--out` are verbatim; skipped malformed lines are counted.

**Perf shape** mirrors `search`/`files`: a single forward `scan_lines_bytes` pass per file (mmap + SIMD newline scan + a broad pre-JSON byte prefilter), rayon across files. The forward path is mandatory (NOT head/tail) — it visits every line including blanks, so the local counter equals the true jsonl line 1:1.

**Example invocations:**
```bash
csift recover . --file /abs/PLAN.md --coverage          # scope first: covered ranges + boundaries
csift recover <uuid> --file /abs/app.py --patches       # segmented unified diffs over the session
csift recover <uuid> --file /abs/app.py --at @turn:42   # partial snapshot as the LLM saw it at turn 42
csift recover . --plan --out /tmp/restored-plan.md      # restore the latest plan verbatim
```

### 6.8 `turns` — turn-fidelity reconstruction (restore the back-and-forth a compaction summary clipped)

**Purpose.** A Claude Code **compaction summary** preserves TASK STATE (its 9-section synthesis: primary request, key concepts, file ledger, errors+fixes, plan, next step) in high fidelity, but provably **loses turn fidelity**: its "All user messages" section clips real prose turns to `...`-truncated bullets (measured: ~22 real user turns → ~17 bullets), and the assistant side collapses to a SINGLE verbatim quote (the last pre-compaction message). `turns` **supplements** the summary — it re-emits the verbatim user/assistant TURNS, in original order, each line carrying the JSONL line number (`Lnnnnn`) so a consumer can `Read` the raw transcript at the cited line. It does **not** re-derive task state (the summary owns that; duplicating it wastes budget and risks contradiction). The split of labor is the summary's own design — its trailer says "read the full transcript at `<path>`" for the exact content it generated; `turns` automates that pointer.

**Reuse, no re-parse.** `turns` sits on the §6.7 `recover` extraction layer verbatim: the same `scan_one_file` forward line-numbered `scan_lines_bytes` pass (the 1:1 jsonl line map), the same `group_turn_indices` (§6.4) turn delimiter, the same `Record` helpers (`is_genuine_user` / `genuine_user_text` / `agent_text` / `blocks` / `is_compact_summary`), the same `resolve_session_files` / `TimeWindow` / `timez` rendering. The byte prefilter is a SUPERSET of recover's, broadened with `"role":"assistant"` / `"type":"assistant"` probes so a pure-text assistant turn (carrying none of Edit/Write/Read/Bash) is never missed. The `Record`/`Block` model needs no change.

**Selection vs render order.** Selection walks **backward from EOF** (recency-first) so the budget is spent on what a resumed agent most needs; the emitted document is sorted **ascending** so it reads as a forward transcript. The backward walk is **transparent to `isCompactSummary` boundaries** (a summary is a turn MEMBER, never a delimiter — §6.4 / model.rs), so it reaches back across multiple compaction boundaries by default (verified on real transcripts: a 40K-char ellipsized budget spans 26 boundaries on a 35-summary session; `--max-compactions N` caps the crossing count).

**Budget allocation (two-phase).** `--budget <N>` (default 40000) bounds the whole reconstruction in chars (or tokens via `--budget-unit tokens`, ≈4 chars/token). `--round-trip-fraction <F>` (default 0.5) is a **hard floor**: Phase 1 spends `budget·F` ONLY on round-trip-complete turns (user && assistant-EOT), walking recency-first; Phase 2 fills the rest with whichever single sides remain, user-first (the user wording is the scarcer, higher-signal loss). Without the floor an assistant-heavy tail recovers ZERO user turns (measured on a real pulse-shaped tail). The `[N tool calls]` marker cost is charged per turn (omitted when 0). Determinism: recency = descending line_no, ties by descending turn_index.

**Ellipsis (role-asymmetric middle-truncation).** A unit over its role cap (`USER_CAP=600`, `ASST_CAP=900`, sized from measured medians) is **middle-truncated**, keeping head+tail, with an explicit `… [+K chars, L lines elided] …` marker (the line count uses the pre-normalization text; omitted for single-line user messages). The assistant head is both absolutely larger (900 vs 600) and a larger fraction (0.66 vs 0.60 → head 594/tail 306 vs user 360/240), because EOT prose front-loads context and back-loads the decision. Cuts are on `char` boundaries (UTF-8 safe). No content is fabricated; nothing is silently dropped.

**Dedup against the live summary.** The newest in-range summary is already in the resumed model's context (it IS the seed). A live-region turn (compactions_before == 0) whose 80-char normalized prefix matches the summary's §6 user bullets or §9 assistant quote is flagged `(also in summary)` and **demoted** (selected only after non-dup turns) — never silently dropped (a false positive must not lose a real turn). Turns predating an OLDER boundary are genuinely gone from context, so they are pure restoration, never deduped.

**Output.** Text groups under `SESSION <id>`: a budget-accounting header, then turn-by-turn `▽ Lnnnnn USER (ts)` / `[N tool calls]` / `△ Lnnnnn ASSISTANT (ts)`, with `══ compaction boundary · summary at Lnnnnn ══` banners at crossings and `(also in summary)` flags on demoted units. `--out` writes the full (un-terminal-truncated) reconstruction to a file while the summary prints to stdout. JSON (`--format json`) emits one VERBATIM (un-truncated `text`) object per unit (`line_no`, `role`, `ts_utc`/`ts_local`, `tool_calls`, `full_chars`, `rendered_chars`, `truncated`, `elided_chars`/`elided_lines`, `also_in_summary`, `compactions_before`) plus interleaved `{"kind":"compaction_boundary","line_no":…,"summary_chars":…}` records. **No silent truncation** — skipped malformed lines are counted and surfaced.

**Windowing** matches §6.7: `--turn-range START..END` (inclusive, 0-based genuine-user order) is mutually exclusive with `--since`/`--until` (ISO8601 / relative).

**Example invocations:**
```bash
csift turns .                                   # default 40K-char reconstruction, this project's session
csift turns <uuid> --budget 12000               # a 200K-context-sized recovery (~10-15K)
csift turns <uuid> --budget 40000 --format json # machine-readable, line-numbered
csift turns <uuid> --round-trip-fraction 0.6    # weight harder toward complete round-trips
csift turns . --budget 40000 --out /tmp/turns.md  # full reconstruction to a file
```

---

## 7. Performance design (NON-FUNCTIONAL contract)

Measured corpus reality (real `~/.claude/projects`, 2 437 jsonl files): largest file **225 MB / 115 879 lines**; **line-length skew is the dominant fact** — min 82 B, p50 813 B, p90 2.6 KB, p99 14.9 KB, **max 400 KB single line**. 98% of `user` records are tool_result-carriers; 54% of all records are `attachment` noise. ⇒ never materialize whole-line `String`s for non-candidates; prefilter aggressively before any JSON parse.

### 7a. mmap + memchr line scan (full-scan path) — use `memmap2::Mmap`, NOT BufReader

Files are 75–225 MB, read-only, scanned once front-to-back. `memmap2::Mmap` (immutable; **never** `MmapMut`) gives the kernel page-cache + readahead and a single `&[u8]` for zero-copy `memchr::memchr_iter(b'\n', &mmap)` line splitting and zero-copy `&str` slices into matched lines. BufReader would copy **every** line (incl. the 98% non-candidates) into a line buffer. **Truncation race:** a live session can grow/shrink under mmap — open + mmap once, treat length as fixed-at-open, tolerate a torn final `\n`-less fragment (skip it, like the tail carry). For files < ~256 KB, skip mmap and `fs::read` (setup cost not worth it) — but all perf-relevant files are huge, so mmap is the default.

### 7b. Tail-read — seek-from-EOF backward chunk scan (for `list`)

Measured: finding last-assistant + last-genuine-user scanned only **1.64 MB (0.695%)** of a 225 MB file. Worst case (long tool tail before the last human turn) is larger, so loop until both anchors found or BOF.

```
CHUNK = 64 KiB (grow to 256 KiB if a chunk yields no newline — guards the 400 KB max line)
pos = file_len;  carry = []           // bytes of an incomplete line straddling the LOW-offset edge
need = {last_user, last_assistant}
while pos > 0 and need not empty:
    n = min(CHUNK, pos); pos -= n; seek(pos); chunk = read(n)
    buf = chunk ++ carry              // re-attach the fragment carried from the previous (higher) chunk
    parts = split(buf, b'\n')
    carry = parts[0]                  // buf[0] is provisional until a LOWER-offset chunk is read
    for line in parts[1..].reverse(): // newest-first within the chunk
        if line.is_empty(): continue
        if not byte_prefilter(line, wanted): continue     // §7d, BEFORE serde
        rec = serde_json::from_slice(line)?               // parse only candidates
        classify → fill last_user / last_assistant
        if both filled: break outer
if pos == 0 and carry not empty: process carry as line 0
```
The **carry** (incomplete-line head straddling a chunk boundary) is the only subtle part — a 400 KB record can straddle any boundary; each chunk's lowest fragment is provisional until the next-lower chunk is read. Head-read (first genuine-user) is the symmetric forward version from offset 0, stopping at the first match.

### 7c. Lazy parse — byte prefilter, then `serde_json` on candidates only

Measured payoff (225 MB file): parse-every-line **1.39 s** vs prefilter-then-parse **0.65 s** (2.1×); raw `memchr` newline floor **0.28 s**. 98% of lines are skipped before any allocation.

### 7d. Two-stage prefilter on raw line bytes (no UTF-8 validation, no JSON parse)

1. **Category prefilter** (cheap `memchr::memmem::Finder`, built once per needle, reused across all lines): a line that could match a requested category must contain a marker substring — `"thinking"`, `"tool_use"`, `"tool_result"`, `"role":"assistant"`, etc. A line lacking **all** active markers is dropped pre-JSON. (Genuine-`user` is the exception: a `"type":"user"` line passing the substring gate still needs structural disambiguation from carriers, so it is parsed — but that's only ~8.8 K of 115 K lines, and only when the `user` category is active.)
2. **Keyword prefilter** (the user regex): if the regex has a required literal substring (extract via `regex-syntax` HIR literal analysis, or use the `regex` crate's own prefilter), `memmem` that literal first. Empty pattern ⇒ pure filter (skip stage 2). Non-anchorable regex ⇒ run `regex::bytes::Regex` directly on raw bytes (still far cheaper than JSON parse).

Only lines passing **both** gates reach `serde_json::from_slice` → typed `Record` → exact field-level match + round-trip stitching. **`simd-json`/`sonic-rs` are NOT used** (and not a default dep): once the prefilter drops 98% of lines, total serde time is a few ms even on 225 MB; simd-json needs an owned, padded, mutable buffer (fights the zero-copy mmap `&[u8]`) and sonic-rs adds unsafe-heavy deps for a speedup on a tiny byte fraction. Capture most of the win instead with `serde_json::from_slice` + `#[serde(borrow)]` zero-copy fields. (Keep simd-json as a possible future `--feature`, not default.)

### 7e. Parallelism — rayon ACROSS files, not within (by default)

- **Across files/sessions:** `rayon` `par_iter` over the file list — the big win for `list` (2 437 files) and multi-target `search`. Each file is an independent mmap + scan; embarrassingly parallel; near-linear to core count. **This is the only parallelism csift needs by default.** rayon's default pool caps concurrency at core count, so at most N files are mmapped at once (virtual address space, not resident until touched).
- **Within a single file:** **do NOT** parallelize by default — one 225 MB full-scan parses in well under a second after prefilter; splitting one file across threads needs newline-aligned chunk boundaries (snap each split to the next `\n` via `memchr`) and adds contention for little gain when N files already saturate cores. Make it a future opt-in (`--threads-per-file`), not the default.
- **Determinism:** collect per-file results into a `Vec` indexed by input order; sort/merge after the parallel section so `text` and `--json` output are deterministic regardless of completion order.
- **`--max-count`:** cap per-file then globally, and **report the dropped count** — never silent truncation.

---

## 8. Output formats

### 8.1 Text (default) — LLM/human-readable

- Clear, scannable **session / turn / category / timestamp** headers (examples in §6.1, §6.2).
- Timestamps: `YYYY-MM-DD HH:MM:SS <TZ> (RAW_UTC_ISO8601)` — system-local timezone + raw UTC, via `jiff` (`TimeZone::system()`; `<TZ>` auto-tracks the detected zone).
- Excerpts truncated only with an explicit `… (+N chars)` marker (never silent).
- Footers state match counts and **dropped counts** (`N dropped` when `--max-count` capped).

### 8.2 `--json` — machine-readable

One JSON object per emitted unit, deterministic order. Shapes mirror the in-memory types:

- **`list`** → one object per session: `{ session_id, path, cwd, version, git_branch, first_user:{ts_utc,ts_local,excerpt}|null, last_user:{…}|null, last_agent:{…}|null }` (mirrors `session::SessionSummary` + `MessagePreview`).
- **`search`** → one object per Exchange: `{ session_id, turn_index, hits:[{category, excerpt, ts_utc, ts_local}], record_uuids:[…] }` (mirrors `search::Exchange` + `Hit`), followed by a trailing summary object `{ matched, dropped_by_cap }` (mirrors `search::SearchOutcome`).
- **`whoami`** → `{ session_id, path? }`.

JSON must be valid UTF-8; non-UTF-8 bytes in excerpts are lossily replaced (`String::from_utf8_lossy`), never panicked on.

---

## 9. Non-functional gates & invariants (checklist)

- [ ] **Speed:** `list` and `search` stay fast on 200 MB+ files — mmap + memchr + two-stage prefilter + lazy serde; tail/head reads never parse the whole file; rayon across files. (§7)
- [ ] **No silent truncation:** every cap (`--max-count`, excerpt length) reports its drop count; skipped malformed lines are **counted** and surfaced, never hidden. (§0, §7e, §8)
- [ ] **No `unwrap`/`expect`** in library/hot paths; `anyhow::Result` + `?`; `main` is the only error→exit-code site; a malformed line is skipped+counted, never fatal. (§0)
- [ ] **Tolerant parsing:** unknown top-level fields, unknown `type`s, unknown `Block` types (`#[serde(other)] Unknown`), and missing `timestamp` never crash. (§3)
- [ ] **Genuine-user correctness** (the load-bearing rule): excludes tool_result-carriers, `isCompactSummary`, and `isMeta` pseudo-turns; AUQ answers handled per §4.4. Unit-tested against fixtures incl. the `isMeta` case. (§4.1–§4.2)
- [ ] **Path encoding:** `[^A-Za-z0-9]→'-'`, no collapse, case-preserved; reverse never attempted; target detection real-path-vs-encoded per §2.3. (§2)
- [ ] **whoami:** env-var-first (`CLAUDE_CODE_SESSION_ID`, exact name), error-with-guidance on absence, **never** mtime/process-walk. (§6.3)
- [ ] **Timestamps:** system-local timezone + raw UTC everywhere (auto-detected via `TimeZone::system()`). (§8.1)
- [ ] **Example-rich `--help`** for every subcommand (the invocations in §6.1–§6.3 are the baseline). (clap `long_about`/`after_help`)
- [ ] **Gates green:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. No crate-level `#![allow(dead_code)]`; the only `#[allow(dead_code)]` is targeted on `model::Record`/`Block` for SPEC-mandated record-shape fields retained for tolerance/completeness (justified inline).

---

## 10. Brief-vs-research corrections (consolidated)

| # | Brief said | Verified reality (research wins) | Where |
|---|---|---|---|
| 1 | Compaction may be `{type:"summary", summary/leafUuid}` | **No `type:"summary"` exists in CC 2.1.x.** Compaction = `type:"user"` + `isCompactSummary:true` + `isVisibleInTranscriptOnly:true` (string summary) **and** a `type:"system"` `subtype:"compact_boundary"` (metrics in `compactMetadata`). Detect on `isCompactSummary`; tolerate legacy `summary` defensively but don't rely on it. | §3.1, §4.7 |
| 2 | Path encoding "verify: collapse? dots?" | `[^A-Za-z0-9]→'-'`, **NO collapse**, `.`/`/`/`_`/space all →`-`, case preserved (proof: literal `--` from `/.`). Forward deterministic, reverse lossy. | §2 |
| 3 | whoami env var "IF Claude Code exposes one" | It **does**: `CLAUDE_CODE_SESSION_ID` is definitive (== own jsonl basename). Build env-var-first; error-with-guidance only on absence. | §6.3 |
| 4 | Genuine-user = string or text blocks (not tool_result-carrier) | Correct **but incomplete** — must **also** exclude `isMeta:true` pseudo-turns (`"Continue from where you left off."`, loop ticks, stop-hook feedback) and `isCompactSummary`. `isMeta` not in brief. | §4.1–§4.2 |
| 5 | Persisted output = `<persisted-output>` inline pointer | True **plus** a structured `toolUseResult.persistedOutputPath` + `persistedOutputSize`; resolve via the structured field (no regex needed), inline marker is the fallback. | §4.6 |
| 6 | `type` set = user/assistant/system + a few metadata | Full set incl. `attachment` (54% of records — dominant noise), `file-history-snapshot`, `queue-operation`; block extras `caller` (on tool_use), `tool_reference` (in tool_result content). Model must absorb all via tolerant parsing. | §3.1, §3.5, §4.8 |
| 7 | Generic "mmap + memchr + rayon + lazy parse" | Quantified: 400 KB max single line ⇒ chunk-with-carry tail read; prefilter is two-stage (category memmem + keyword literal); rayon **across** files only (within-file is future opt-in); simd-json explicitly rejected for the default. | §7 |
