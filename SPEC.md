# SPEC.md — csift

> **Status: AUTHORITATIVE — implemented.** This is the single source of truth for *what csift does and how*. Its design is grounded in real `~/.claude/projects` data (51 sampled sessions across CC 2.1.133–2.1.168, files up to 225 MB / 115 879 records), and the measurements throughout are cited as the evidence for each decision. All nine subcommands (§6) are built; §11 folds in the per-feature design rationale + the empirical measurements behind the deepest three (`recover` / `turns` / `agents`). [`AGENTS.md`](./AGENTS.md) (Claude Code loads it as the `CLAUDE.md` symlink) remains authoritative for *how to work in the repo* (git discipline, gates, conventions); this file is authoritative for *what to build*. An engineer who has never seen a Claude Code (CC) `.jsonl` should be able to implement csift from this document alone.

---

## 0. Mission & non-negotiables

**csift = "ripgrep for Claude Code session transcripts".** A fast Rust CLI to **list** and **regex-search** CC session `.jsonl` files. Nine subcommands: `list`, `search`, `agents` (subagent lifecycle, §6.5), `whoami`, `files` (which files a session changed, §6.6), `recover` (reconstruct a file's content from the transcript, §6.7), `plan` (locate the plan file bound to a session, §6.7.1), `turns` (turn-fidelity reconstruction across compaction, §6.8), `image` (list + extract the images a session carries, §6.9). `list`/`search`/`files`/`recover`/`plan`/`image` span each session's subagent transcripts by default (`--no-subagents` opts out); `turns` is the exception (top-level thread only, `--include-subagents` opts in); `agents` lists subagents as its targets.

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

**Two more rules CC applies that the naive encoding above omits (verified against the cleanroom `sanitizePath` AND the shipping binary `Siq`, 2026-06-16):**

1. **Canonicalization first.** CC encodes the cwd's **`realpath` + Unicode-NFC** form (`canonicalizePath`), not the raw string — so `/tmp`↔`/private/tmp` symlinks and NFC/NFD variants *converge* to one dir. csift's `absolutize` does the realpath (`canonicalize`) but **not** NFC; for an all-ASCII path (the overwhelming case) this is identical, and a non-ASCII byte encodes to `-` either way — the only divergence is the dash COUNT for an NFD-decomposed non-ASCII path (an accepted, documented edge gap).

2. **200-char cap + hash (`MAX_SANITIZED_LENGTH = 200`).** If the encoded string exceeds 200 chars, CC stores the dir as **`<first-200>-<hash>`** — `Bun.hash` on the CLI, a djb2 variant in the SDK (so the SAME path gets DIFFERENT suffixes depending on which wrote it). Because the suffix is not reconstructible, CC's own `findProjectDir` does **not** recompute it — it **prefix-scans** the projects root for a dir starting with `<first-200>-`. `resolve_target` mirrors this exactly (§2.3 step 3). `encode_cwd` itself stays the *raw* full encoding (it also feeds the §2.4 cross-check); the cap lives in the lookup.

**Collision is REAL and CC does NOT disambiguate it (for ≤200-char paths).** The map is many-to-one, so two *different* cwds can encode to the *same* dir — e.g. `/Users/x/Projects/Acme/widget_factory-worktrees/portal` and `/Users/x/Projects-Acme/widget-factory-worktrees-portal` BOTH encode to `-Users-x-Projects-Acme-widget-factory-worktrees-portal` (the first has `/Acme/` + `_` separators, the second `-Acme` + `-` — all collapse to `-`). CC takes no action: both projects' sessions coexist in that one dir, told apart ONLY by session UUID + the in-record `cwd` field (§2.4). csift faithfully mirrors CC (the encoded dir is the lookup key), so a real-path target whose dir is shared by a colliding sibling will surface that sibling's sessions too — `list` shows the true per-session `cwd` (so the collision is visible), but the scoped commands do not filter on it. The `cwd` field is the authoritative origin; never trust the dir name alone to mean one cwd.

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
plus version-specific extras observed: `last-prompt`, and the catch-all must absorb any future addition. **There is NO `type:"summary"` record in CC 2.1.x** — zero `summary` records exist across all 51 sampled sessions. Compaction is a `type:"user"` record + a `type:"system"` `compact_boundary` record (§4.7), so detection keys on `isCompactSummary` and never depends on a `type:"summary"`, while *defensively* still tolerating a legacy `type:"summary"` (older CC) without relying on it.

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
               caller: Option<serde_json::Value> },              // caller:{type:"direct"} observed on real records
    ToolResult { tool_use_id: Option<String>,
                 content: Option<serde_json::Value>,             // string OR array — keep raw, see §4.5
                 is_error: Option<bool> },
    Image    { source: Option<serde_json::Value> },
    #[serde(other)]
    Unknown,                                                     // any future block type — never a parse failure
}
```

Block → category mapping is in §5. Note the `ToolResult` `content` can itself be an array containing `{type:"text",text}`, `{type:"image",…}`, and **`{type:"tool_reference", tool_name}`** (emitted by ToolSearch results); model it as raw `Value` and inspect as needed.

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
3. **`isMeta` is falsey** (excludes system-injected pseudo-turns, §4.2); AND
4. content is a **string**, OR content is a block array containing a `text` block and **no** `tool_result` block.

(`text` and `tool_result` never co-occur in one user record in real data, so "has a `text` block" is a clean genuine signal once carriers and meta are excluded.) `src/model.rs::is_genuine_user` implements (1)–(4); it **additionally** excludes the machine-synthesized markers of §4.2.1–.3 (interrupt strings, `<local-command-stdout>…`, `<command-name>…`) so they never start a turn (codepoint-safe exact `==`/`starts_with`, never a byte-offset slice).

**Boundary predicate `opens_turn(record)` — the single turn delimiter (§6.4).** Turn-delimiting keys on `opens_turn`, NOT on `is_genuine_user` alone. A turn opens on ANY of:
1. `is_genuine_user` (above); OR
2. an **answered AskUserQuestion** carrier (§4.4) — the answer IS the user's message; OR
3. a **tool-use rejection carrying a typed user message** (§4.2.4) — the typed instruction IS the user's message.

Cases 2–3 are genuine user messages, so each opens a turn (§6.4). The reconstruction (`reconstructed_user_text`) renders the genuine-user body for each: the plain text, the full AUQ Q+options+answer unit, or the rejection instruction + a `[plan: <path>]` pointer.

### 4.2 `isMeta:true` pseudo-turns + synthesized markers — TRAP, must be excluded

`isMeta:true` `user` records have string/text content that *looks* human but is **system-injected**, e.g. `"Continue from where you left off."`, `"# Autonomous loop tick"`, `"Stop hook feedback: …"`, `"<local-command-caveat>…"`, `"[Image: source: …]"`. These are **not** genuine human input and must be excluded from the `user` category **and** from turn-delimiting. The `isMeta` exclusion is load-bearing.

**Beyond `isMeta`, four NON-`isMeta` user-record shapes are also machine-synthesized and must NOT open a turn** (verified on real `~/.claude/projects` data; corpus counts in parentheses):

#### 4.2.1 Interrupt markers (116 + 21)
A `type:"user"` `text`-block (or bare-string) record whose content is **EXACTLY** `"[Request interrupted by user]"` or `"[Request interrupted by user for tool use]"` — a CC-synthesized interrupt marker, not a human turn. None carry extra prose (dropping them as boundaries loses zero user content). Match is **exact `==`** (a real message that merely *quotes* the phrase stays genuine).

#### 4.2.2 `<local-command-stdout>…` (52)
String content (NOT `isMeta`) starting with `<local-command-stdout>` — local-command OUTPUT (machine), not the user's prose. (Its sibling `<local-command-caveat>` carries `isMeta` and is already excluded.) Excluded via `starts_with`.

#### 4.2.3 `<command-name>…` slash-command wrapper (54)
String content (NOT `isMeta`) starting with `<command-name>` — the machine-templated EXPANSION of a slash command. The templated wrapper does NOT open a turn; any genuine prose the user typed after the command lives in `<command-args>…</command-args>` and is recovered separately (`slash_command_args`) so it still surfaces under the `user` category.

#### 4.2.4 Tool-use rejection — `is_error:true` tool_result (31 with-message / 36 without)
When the user REJECTS a tool use (ExitPlanMode plan kick-back, or a rejected AskUserQuestion / Edit / …), the result carrier is `is_error:true` with content beginning `"The user doesn't want to proceed with this tool use…"`. TWO sub-shapes:
- **With a typed message** — ends `"To tell you how to proceed, the user said:\n<TEXT>"`. `<TEXT>` IS a genuine user message → **opens a turn** (`plan_rejection_message` extracts the tail codepoint-safely via `split_once`). For an ExitPlanMode rejection, the rejected `tool_use_id` resolves through a `PlanIndex` (`tool_use_id → planFilePath`) to append a `[plan: <path>]` pointer so a consuming LLM can Read the plan.
- **Without a typed message** — ends `"STOP what you are doing and wait for the user to tell you how to proceed."`. The user clicked reject but typed nothing → **NOT a boundary**.

The plan **APPROVAL** carrier (`"User has approved your plan…"`, no `is_error`, no typed message) is the harness greenlight, NOT a user message → never a boundary.

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
and the carrier's top-level `toolUseResult` echoes the full `questions[]` structure + a **structured `answers{question → answer}` map** (the CLEAN source — the inline synthesized string is noisy/truncated). **Implication:** csift never needs to render a "pending question" state — if an AUQ `tool_use` is on disk, its answer is too.

**The COMPLETE user message = QUESTION + OPTIONS + ANSWER as one unit.** The user does not click an option — she answers in prose (often a counter-question or a scope-expanding decision), so the answer is her selection *plus* her reasoning. `auq_exchange()` reconstructs the whole exchange as one genuine-user unit: `[AskUserQuestion · N questions]` then, per question, `Qn (header): question  options: a | b | …` and `An: <answer prose>`. It is built from the structured `toolUseResult.questions[]` zipped with `toolUseResult.answers{}` (clean), falling back to parsing the synthesized `"<q>"="<a>"` string only when `toolUseResult` is absent. Codepoint-safe (no byte-offset slice into a CJK question/answer).

**The answer is a genuine-user TURN BOUNDARY** (`is_auq_answer_boundary` — true iff a non-errored `tool_result` carrier with a non-empty `toolUseResult.answers` OR the synthesized marker; a CANCELLED/rejected/validation-errored AUQ has `is_error:true` and no `answers` → NOT a boundary). An AUQ answer (e.g. one that expanded the session scope) is a genuine-user message, so it opens its own turn (§6.4) and is surfaced on every surface (turns/list/search/recover/budget/topology). The answer is also surfaced under the `user` category (§5).

### 4.5 `tool_result` (tool's response) → category `tool-response`

Lives **inside a `type:"user"` carrier record's** `message.content` array, never on assistant. Fields: `tool_use_id?`, `content` (string OR array of `{type:text,text}`/`{type:image}`/`{type:tool_reference,tool_name}`), `is_error?`. Linkage to the originating `tool_use` is `tool_result.tool_use_id == tool_use.id`; **some legacy carriers omit the block-level `tool_use_id`** — fall back to the carrier's top-level `toolUseResult`/`sourceToolAssistantUUID` to resolve linkage.

### 4.6 Externalised (persisted) large outputs

When a tool output is too large, the inline `tool_result.content` string is a pointer:
```
<persisted-output>
Output too large (NNN KB). Full output saved to: <ABSOLUTE_PATH>

Preview (first 2KB):
…
</persisted-output>
```
`<ABSOLUTE_PATH>` = `<ENCODED>/<session-uuid>/tool-results/<id>.txt` (`<id>` is a tool short-id like `b070yh2rb` or a hook form `hook-<uuid>-<n>-additionalContext`). **In addition**, the carrier's top-level `toolUseResult` object carries **structured** `persistedOutputPath` + `persistedOutputSize` (bytes) fields. **Resolution rule:** when `search --resolve-persisted` is set, read the file at `toolUseResult.persistedOutputPath` (exact — no regex on the inline marker needed); only fall back to regex-scraping the inline `Full output saved to:` path if the structured field is absent. Default (flag off): leave the inline pointer as-is (token economy). Resolution failures (missing file) are reported, never fatal.

### 4.7 `type:"system"` records & compaction

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
| `user` | genuine human turn **+** AUQ answers **+** plan-rejection messages | `reconstructed_user_text(record)`: `is_genuine_user` per §4.1 (string or text-only, non-meta, non-carrier, non-compaction, non-synthetic-marker), OR the reconstructed AUQ Q+options+answer unit (§4.4), OR a tool-use rejection's typed message + `[plan: …]` pointer (§4.2.4), OR recovered `<command-args>` prose (§4.2.3). The AUQ/rejection cases surface the CLEAN answer (from `toolUseResult.answers` / the `said:` tail), not the noisy synthesized string. |
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
| `--no-subagents` | bool | — | restrict to top-level `<uuid>.jsonl` sessions only; **DOMINANT** (always wins when present, any flag order) |

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
| `[PATTERN]` | — | positional string | `""` (empty ⇒ pure filter) | regex, ripgrep-like, default **smart-case**. A bare uuid is a LITERAL pattern (no special routing); to scope to one session, pass `@<uuid>` as a PATH positional (`search PATTERN @<uuid>`). |
| `[PATH]…` | — | repeatable positional | all projects | scope target(s): real cwd / encoded dir (§2.3) / `@<uuid>` (one top-level session) / `@<uuid-prefix>` (a 4–11-hex leading run like `@13d9645a` → the UNIQUE session it prefixes, else an ambiguity error listing the candidates) / `@main` (calling **top-level** session, env-resolved) / `@trap:<marker>` (the calling **SUBAGENT**, found by a unique literal marker the caller embeds in this csift command — §6.3a) / `@<agent-hex>` (a SUBAGENT + its topological descendants — or the agent alone under `--no-subagents`) / `*.jsonl` (one transcript — a subagent transcript scopes to that agent's subtree) — the same positional surface every sibling uses |
| `--category C` | `-t` | repeatable enum | all | one of `thinking\|user\|tool\|tool-response\|agent` (§5) |
| `--ignore-case` | `-i` | bool | smart-case | force case-insensitive |
| `--multiline` | — | bool | false | let `.` cross newlines / multiline mode |
| `--turn-range START..END` | — | string | none | inclusive 0-based turn index range; **mutually exclusive** with `--since`/`--until` |
| `--since WHEN` / `--until WHEN` | — | string | none | time bounds (ISO8601 or relative `2h`/`3d`/…, system-local; bare date ⇒ local midnight) |
| `--max-count N` | — | usize | none | cap emitted exchanges; **reports dropped count** |
| `--count` | `-c` | bool | off | print ONLY the match total (ripgrep `-c` idiom); honors every filter; mutually exclusive with `-l` |
| `--files-with-matches` | `-l` | bool | off | list ONLY the distinct sessions containing ≥1 match (ripgrep `-l`); mutually exclusive with `-c` |
| `--siblings` | — | bool | off | also render each matched turn's NON-matched records (the surrounding back-and-forth, e.g. a matched user question + the agent reply) |
| `--sibling-category C` | — | repeatable enum | off | restrict `--siblings` rendering to these categories (implies `--siblings`) |
| `--full` | — | bool | off | emit each record's FULL text instead of the centered ~400-char excerpt (alias `--no-truncate`) |
| `--line SPEC` | — | repeatable + comma | none | ADDRESS by 1-based physical line(s)/ranges (`--line 87,495-500`) instead of/with pattern; needs a single-transcript scope; addressed records render FULL; an explicit miss is reported `unresolved` |
| `--uuid U` | — | repeatable + comma | none | ADDRESS by record uuid(s) (globally unique); addressed records render FULL; a miss is `unresolved` |
| `--subagent HEX` | — | string | none | pin `--line` addressing to one subagent transcript (bare hex from `agents`) |
| `--resolve-persisted` | — | bool | false | resolve `<persisted-output>` pointers (§4.6) |
| `--include-subagents` | — | bool | `true` | also search each in-scope session's subagent transcripts (built-in + workflow / OMC agents under `subagents/**`); **default ON** (passing it is a no-op for explicitness). Workflow `journal.jsonl` is never searched (not a transcript). |
| `--no-subagents` | — | bool | — | search only top-level `<uuid>.jsonl` sessions; **DOMINANT** — always wins when present, any flag order |
| `--format text\|json` | — | enum | `text` | output format |

**Addressing (record fetch) — `--line` / `--uuid`.** Beyond regex matching, a record can be selected by **physical line** (`--line`, per-file, needs a single-transcript scope) or **uuid** (`--uuid`, global). Addressed records emit at FULL length (you asked for *this* message, not a teaser); a pattern, if also given, further narrows within the addressed set. This is the permission-friendly alternative to `Read`-ing the raw jsonl. An explicitly-requested address that resolves to nothing is reported in an `unresolved:` footer line — never silently dropped.

**Smart-case rule:** pattern is case-insensitive iff it contains no uppercase letter; `-i` forces insensitive regardless; the two never conflict (`-i` wins). Compile via `regex::bytes::RegexBuilder` (match on raw line bytes pre-JSON in the prefilter, then on decoded text for excerpting). `--multiline` sets `.dot_matches_new_line(true)` + multiline mode.

**Regex dialect — linear-time (RE2-class).** The pattern is the Rust `regex` 1.12 crate (`regex::bytes`), which **guarantees linear-time matching** in the input length: **no catastrophic backtracking, ever**. *Supported:* literals; character classes `[...]` / `[^...]` / `\d \w \s` + Unicode classes `\p{...}`; alternation `|`; groups `(...)` / non-capturing `(?:...)`; quantifiers `* + ? {m,n}` (greedy + lazy `*?`); anchors `^ $ \b \B`; dot `.` (`--multiline` lets it cross newlines); inline flags `(?i)(?m)(?s)(?x)`; Unicode-aware by default. ***Deliberately NOT supported*** (they require a non-linear engine): backreferences `\1`; lookahead/lookbehind `(?=) (?!) (?<=) (?<!)`; atomic groups / possessive quantifiers `(?>...)` / `a*+`. A pattern using these **fails to compile** with a clear error — by design, not a bug. This boundary is documented identically in `--help` (`search`'s `after_help`) and `SKILL.md`.

**Validation:** `--turn-range` together with `--since`/`--until` is an error (mutually exclusive). An empty `PATTERN` with no other filter is allowed (matches every category-eligible record) but should warn that it will emit a lot.

**Filter application order per session:** target selection (positional PATH/`@<uuid>`/`*.jsonl`) → category eligibility (§5) → time/turn window → regex match → round-trip reconstruction (§6.4) → `--max-count` cap (with drop accounting).

**Time window:** `--since`/`--until` compare against each record's `timestamp` (UTC). Records with no timestamp (metadata/noise) are never time-matched. Relative forms (`2h`,`3d`,`90m`) are resolved against *now* in the system-local timezone then converted to UTC for comparison.

**Output — complete round-trip:** see §6.4. Each emitted unit is one **Exchange** with a session header, turn index, and the matched hit(s) shown in context of their full round-trip.

**Text output example.** Each distinct session is declared ONCE in a label table (`s1 = <uuid>`), then every exchange header is the cheap `s<N>·t<turn>` reference + a single compact local instant (the offset already pins it — no second UTC copy); hit lines are `  <glyph> <category>[ <tool>]  L<line>  <excerpt>` with `◂` = user, `▸` = everything else:
```
s1 = 0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d

s1·t47  2026-06-07 14:32:05.478+10:00
  ◂ user  L990  why is the tail-read carry needed?
  ▸ thinking  L994  The carry holds an incomplete line straddling a chunk boundary…
  ▸ agent  L1003  The carry is the partial line at the low-offset edge of each chunk…

matched 1 exchange · 1 session · category=all
```
(A subagent session's table row reads `s2 = <hex> (subagent · parent s1)`; a `tool-response` hit names its tool, `▸ tool-response Edit  L2128  …`.)

**Example invocations:**
```bash
csift search "carry"                                   # all projects, smart-case
csift search -i "askuserquestion" -t tool             # tool_use blocks naming AUQ
csift search "" -t user --since 2h .                   # pure filter: genuine user turns, last 2h, this project
csift search "tail.read" --multiline @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
csift search "panic" -t agent -t thinking --turn-range 10..20 --max-count 50
csift search "" @0a1b2c3d-… --no-subagents --line 992,1374   # fetch records by line (FULL)
csift search "" --uuid 7f3c9e21-…                      # fetch a record by uuid, anywhere in scope
csift search -l "deadline"                             # WHICH sessions mention it (ripgrep -l)
csift search "persisted-output" --resolve-persisted --format json
```

### 6.3 `whoami` — identify the calling CC session (false-positive-safe)

**Strategy (env-var-first — the calling session id is read directly from the environment):**

1. **Primary — read `CLAUDE_CODE_SESSION_ID`.** Verified definitive: CC exports it into every Bash-tool environment and its value equals the calling session's own jsonl basename exactly (e.g. `0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d` → `…/<ENCODED>/0a1b2c3d-….jsonl`). Per-session, version-independent, survives arbitrary bash nesting, **zero false positives**. If set and a non-empty UUID, that **is** the answer. Match the **exact** name `CLAUDE_CODE_SESSION_ID` — never a loose `/session/i` regex (`SECURITYSESSIONID`, the macOS login session, is a false-positive trap; `CODEX_COMPANION_SESSION_ID` mirrors the value but is Codex-plugin-specific — accept it only as a secondary alias, prefer the canonical var).
2. Resolve its transcript: encode `$PWD` to the `<ENCODED>` dir and open `<ENCODED>/$CLAUDE_CODE_SESSION_ID.jsonl`. If `$PWD` doesn't resolve, scan projects-root dirs for the one containing `<id>.jsonl`.
3. **Fallback when the var is absent/empty — ERROR with actionable guidance, never guess.** Message: your session id was not found (`CLAUDE_CODE_SESSION_ID` unset — old CC build or running outside CC); pass an explicit `@<uuid>` target; your id is the basename of your own transcript, or grep a unique recent line you wrote to disambiguate; **do NOT trust most-recent-mtime** (many CC sessions may be live concurrently). It is acceptable for `whoami` to often say "ambiguous, pass an explicit `@<uuid>`".
4. **FORBIDDEN as a whoami source:** process-tree walk and most-recent-mtime. (Evidence: 83 concurrent `claude` processes + 6 installed CC versions on one machine ⇒ ~83-way ambiguity and cross-version argv brittleness; the UUID isn't even on the process command line. mtime with 83 live sessions is almost always wrong.)

**Args (matches `cli::WhoamiArgs`):** `--show-path` and `--format text|json`. The resolved `path` line is ALREADY printed by default whenever the id resolves; `--show-path` only FORCES a `path <not found …>` line in the unresolved case (instead of omitting it). `whoami` takes **no** target — it reads the env var, never a path argument.

**Text output:**
```
session  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
path     ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl
```
**Error output (var absent):** non-zero exit + the guidance string from `whoami::AMBIGUOUS_GUIDANCE`.

### 6.3a `@trap:<marker>` — let a SUBAGENT identify itself

`whoami` and `@main` both read `CLAUDE_CODE_SESSION_ID`, which CC sets to the **top-level** session id in EVERY Bash environment — including inside an in-process subagent. A subagent therefore cannot name *itself* by env (CC withholds the per-subagent id from the Bash env; it is given only to hooks). `@trap:<marker>` recovers it from the transcript instead.

**Mechanism.** The caller INVENTS a unique marker and embeds it **literally** in the very csift command (`csift agents @trap:JollyShinyBrook4283`). CC records an assistant message — including its `tool_use` — to the transcript BEFORE the tool runs (a subagent's sidechain transcript is flushed via `recordSidechainTranscript` ahead of execution; verified empirically — a subagent's Bash can already grep its own marker mid-run). csift resolves `CLAUDE_CODE_SESSION_ID`, then scans that session's main transcript **and** its subagent transcripts for a **Bash** `tool_use` whose `command` contains BOTH the marker AND the literal `csift` (so an unrelated command that merely echoed the token cannot satisfy it). Resolution:
- exactly one **subagent** carries it → that agent (then its subtree, per `--no-subagents`);
- only the **main** transcript carries it → the session itself;
- **zero** → error (marker not literal / mistyped, the command did not run `csift`, or the transcript has not flushed — re-run);
- **>1 subagent** → ambiguity error (use a fresher marker).

A subagent resolves **first-try** (its tool_use is flushed before it runs); the main thread flushes at turn end, so a main-thread `@trap` may need a re-run — from the main thread just use `@main`.

**Marker grammar (ENFORCED — the discipline is the point).** The marker must be a fresh, one-shot, imaginative token the model invents on the spot: **≥3 CamelCase words** (each one uppercase letter + ≥2 lowercase — no single letters, no ALLCAPS acronyms like `HTML`) followed by **exactly 4 digits** that do not form a trivial run (all-equal / consecutive / simple odd / simple even — `0000` / `1234` / `9876` / `1357` / `2468`). It must NOT be script-generated (a generator would itself be a `csift`-ish Bash command carrying the marker → ambiguity), built from a shell variable / concatenation (it must appear verbatim in the recorded command), or reused. csift rejects every violation loudly with guidance. This strictness exists precisely to make a hand-invented literary token the path of least resistance and kill the over-engineered shortcuts at the source.

### 6.4 Round-trip (turn / exchange) reconstruction algorithm

**A "turn" is delimited by a boundary record** — `opens_turn(record)` (§4.1): a genuine human message, an answered AskUserQuestion (§4.4), or a tool-use rejection-with-message (§4.2.4). A non-boundary `tool_result`-carrier, an `isMeta` pseudo-turn, an interrupt / `<local-command-stdout>` / `<command-name>` synthesized marker (§4.2.1–.3), and a compaction summary never start a turn. Turn index is 0-based in boundary order within a session.

**Boundary set (load-bearing for turn indexing).** An AUQ answer and a plan-rejection-with-message are genuine user messages, so each opens its own turn (e.g. an AUQ answer that expanded the session scope is its own turn). The interrupt / local-command / slash-wrapper markers are machine-synthesized, so they never open a turn. Getting either side wrong — a missed genuine boundary or a spurious synthetic one — corrupts the turn index, so both rules are part of the same `opens_turn` contract.

**Records form a tree via `uuid`/`parentUuid`** (each record's `parentUuid` points at the record it follows). A single turn typically expands to a chain: `genuine-user → assistant(thinking…/tool_use…/text) → user-carrier(tool_result) → assistant(…) → …` until the next genuine-user.

**Reconstruction (per session, single forward pass after collecting records):**
1. Build `by_uuid: HashMap<uuid, &Record>` and a `children` adjacency (parentUuid → [child uuids]) over the session's records.
2. Walk records in file order; each `opens_turn` record opens a new **Exchange** (turn index ++). All subsequent records up to (but excluding) the next boundary belong to that exchange.
3. **Round-trip completeness rules when a hit lands:**
   - A matched **`tool_use`** is returned **with its `tool_result`** — pair by `tool_result.tool_use_id == tool_use.id` (fallback via `toolUseResult`/`sourceToolAssistantUUID`, §4.5). Both blocks appear in the emitted exchange even if only one matched.
   - A matched **genuine-user turn** is returned **with the agent response** — i.e. the assistant `text`/`thinking`/`tool_use` records chained under it in the same turn.
   - A matched **`tool_result`** is returned **with the `tool_use` that produced it** (reverse of the above pairing).
   - A matched **`thinking`/`agent` text** is returned within its full turn (the opening genuine-user + sibling assistant records).
4. The emitted Exchange carries: `session_id`, `is_subagent` + the always-re-feedable `parent_session_id` (the id-domain discriminators; `parent_session_id == session_id` for a top-level hit), `turn_index`, `started_utc` (the turn-opening timestamp = the exchange's chronological position; falls back to the earliest hit's ts, `None` only when neither exists), the list of `Hit`s (category + excerpt + UTC timestamp + `tool_name`), and `record_uuids` (every record stitched in — the §6.4 round-trip-completeness evidence) — matching `search::Exchange`.
5. **AUQ pairing:** an `AskUserQuestion` `tool_use` and its answering `tool_result` (§4.4) are one pair; the answer is surfaced under `user` AND — per the §4.4 boundary rule — **opens its own turn** (`opens_turn`). The reconstructed turn opener is the full Q+options+answer unit (`auq_exchange`).
6. **Compaction continuity:** a `compact_boundary`'s `logicalParentUuid`/`preservedSegment` may be used (best-effort) to keep turn indices monotonic across a compaction, but turn delimiting still keys on genuine-user records; never crash if these fields are absent.
7. **Combined stable chronological timeline.** Across the WHOLE scope, emitted Exchanges are sorted ASCENDING by `started_utc` (ISO-8601 UTC sorts lexicographically as text), so subagent exchanges INTERLEAVE with top-level ones by absolute time rather than grouping per file; timestamp-less exchanges sort LAST, with a deterministic tie-break (sorted file order → turn order, preserved by a stable sort — same shape as `files --timeline`). The GLOBAL `--max-count` cap is applied AFTER the sort (keeping the earliest N and reporting the dropped remainder — never silent).

### 6.5 `agents` — a session's subagent TOPOLOGY (kind, trigger/start/completion, returned message, files)

**Purpose.** Build the toolUseId-LINKED topology of the subagents a session spawned: each subagent joined back to the parent `Task`/`Agent`/`Workflow` `tool_use` that triggered it, carrying its identity + lifecycle, the TRUE trigger time, the returned message (3-way resolved), and (on demand) its files-changed. `--tree` shows workflow RUN nodes (from the top-level `workflows/wf_*.json` manifests) as parents of their agents. Complements `--include-subagents` on `list`/`search` (which fold subagent *content* into those views); `agents` is the *topology + lifecycle index* of those same subagents. (Design rationale + verified corpus counts: **§11.3**.)

**Topology linkage (the spawn join).** A built-in subagent's `meta.json` carries `toolUseId` — the id of the parent `Task`/`Agent` `tool_use` that spawned it. csift builds a per-session `ParentSpawnIndex` (one forward scan of the parent transcript) mapping `tool_use_id → {spawn tool name, trigger ts, description, subagent_type}` and `tool_use_id → paired tool_result text`. Each subagent joins on its `spawn_tool_use_id`, recovering:

- **TRUE trigger time** = the parent tool_use ts (the real "when was it triggered" instant). The subagent's own first-record ts (`started_utc`) LAGS this by 0.2–4.7 s and cannot order a sibling fan-out, so trigger is the **default** time axis. `started_utc` is retained as a secondary timestamp. (Workflow agents carry no `toolUseId`, so their trigger falls back to the start ts.)
- **Returned message — resolved 3 ways:** (a) **sync built-in** → the parent tool_result text; (b) **async built-in** (the parent tool_result is the `Async agent launched …` sentinel) → the child transcript's tail assistant text; (c) **workflow** → the `journal.jsonl` `result` event payload (not just the completion bool — the full message). The source is reported (`sync-tool-result` / `async-child-tail` / `workflow-journal`).
- **WorkflowRun nodes** are read from the UNSCANNED top-level `<session>/workflows/wf_*.json` manifests (NOT `subagents/workflows/`): `{runId, taskId, workflowName, status, agentCount, durationMs, totalTokens, totalToolCalls, defaultModel}`. In `--tree` each run is the parent of its `wf_<id>` agents (joined on `workflow_id == runId`).
- **Nested subagents (agent→agent).** On disk the layout is FLAT at every depth: CC writes a subagent's transcript under `getSessionId()` = the MAIN session, regardless of who spawned it (verified vs the cleanroom — `getAgentTranscriptPath`/`Shell.ts` both key off the process-global main id; no `setSessionId` in agent code), so a sub-subagent lands flat in the SAME `<main>/subagents/` dir, never `subagents/.../subagents/`. Nesting is therefore LOGICAL, not structural: the child's spawning `Task`/`Agent` tool_use is recorded in its SPAWNING agent's transcript (not the main one), and the child's `meta.json` `toolUseId` points at it. `build_topology` recovers the tree with a GLOBAL spawn index (the main transcript + EVERY subagent transcript, each spawn id tagged with its issuing agent), then sets `parent_agent_id` (the issuer; `null` ⇒ a direct child of the session, depth 0) and walks that chain for `depth`. `--tree` (text + JSON) nests a sub-subagent UNDER its spawning agent; the flat list still emits every agent (so disk coverage and topology are both complete). All real data today is depth 0 (CC currently provisions most subagents without an agent-spawn tool — 0 sub-sub-agents across 2348 transcripts), but the linkage is reconstructed correctly the moment nesting occurs, validated against a contract-faithful synthetic fixture.

**Id-form unification:** a subagent transcript's printed `session_id` is the **bare `<hex>`** everywhere (`agents`, `files`, `recover`, `list`) — the `agent-` filename prefix is stripped — so a file mutation or recovered event is joinable back to its `agents` node id.

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
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3) whose sessions' subagents to list; a `@<uuid>` positional restricts to one top-level parent session |
| `--kind builtin-task\|workflow` | repeatable enum | all | filter to a subagent kind |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`/…, system-local), filters by the `--by` axis |
| `--by trigger\|start\|completion` | enum | **`trigger`** | which timestamp `--since`/`--until` filter on — `trigger` is the true spawn instant (the **default**), `start` the child's first-record ts, `completion` its last |
| `--tree` | bool | off | render the parent→child topology tree (workflow runs as parents of their agents; a nested sub-subagent under its spawning agent) instead of a flat list; JSON nests `children` |
| `--agent HEX` | string | none | grab ONE subagent by bare-hex id; prints its full node incl. the returned message (implies `--returned-message`) and, with `--with-files`, its files-changed |
| `--returned-message` | bool | off | include each subagent's 3-way-resolved returned message (omitted by default — can be large; always on for a single `--agent` grab) |
| `--with-files` | bool | off | attach each node's files-changed list (reuses the `files` extractors over the subagent's own transcript) |
| `--format text\|json` | enum | `text` | output format |

**Per-subagent (node) fields emitted:** `agent_id` (bare hex), `kind`, `parent_session_id`, `parent_agent_id` (the spawning agent for a nested subagent; `null` ⇒ a direct child of the session, depth 0), `spawn_tool_use_id`, `spawn_tool` (`Agent`/`Task`/`Workflow`), `workflow_id` (workflow only), `agent_type` (meta `agentType`, fallback the spawn's `subagent_type`), `description` (built-in meta, fallback the spawn's), `trigger_utc`/`trigger_local` (the true spawn instant), `started_utc`/`started_local` (first transcript record ts), `completed_utc`/`completed_local` (last transcript record ts), `duration` (trigger→completion), `status`, `depth`, `skipped_lines`; plus on demand `returned_message` + `returned_message_source`, `files_changed[]`, and `children[]`. The `--tree` JSON emits one object per session: `{session_id, workflow_runs:[{…manifest fields…, children:[node]}], agents:[node]}`.

**Status resolution (honest — never over-claims "failed"):** `completed` when a workflow `journal.jsonl` carries a `result` event for the agent OR the transcript terminates with a visible assistant end-of-turn message; `running` when records exist with a start but no completion signal; `unknown` when no timestamps are determinable.

**Time window** is the same semantics as `search` (§6.2): records/rows with no timestamp on the chosen axis are never admitted by a *bounded* window; an unbounded window admits all. **Default axis:** `--since`/`--until` filter on the **trigger** axis (the true spawn instant) by default — the sharpest, most accurate bound; `--by start` (the child's first-record ts) or `--by completion` opts to a different axis.

**Example invocations:**
```bash
csift agents @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d            # one session's subagent topology
csift agents . --kind workflow                                # only workflow agents
csift agents @<uuid> --since 2h                               # subagents TRIGGERED in the last 2h
csift agents @<uuid> --since 09:00 --by completion            # COMPLETED since a bound
csift agents @<uuid> --tree                                   # parent→child topology (runs as parents)
csift agents @<uuid> --agent <hex> --with-files               # grab one subagent: its returned msg + files
csift agents @<uuid> --returned-message --format json         # every node's returned message
```

### 6.6 `files` — which files/dirs a session modified, when

**Purpose.** Report the files + directories a session changed, attributed to genuine-user turns, with a context-blow-up guard (a compact per-dir **summary** is the default; the full chronological list is opt-in). Answers the acid-test question "how many distinct gap docs did this session touch, and how many `/tmp` docs did it create?".

**Extraction (verified against the live `~/.claude/projects` corpus, 2026-06-08) — authoritative vs heuristic:**

| source | how | create-vs-edit | label |
|---|---|---|---|
| `Edit` / `Write` / `MultiEdit` | `input.file_path` | from paired `toolUseResult.type` (`create` ⇒ new file; `update`/`file_unchanged` ⇒ edit), joined by `tool_use_id` within the turn | authoritative |
| `NotebookEdit` | `input.notebook_path` | same join | authoritative |
| `Bash` | LEXICAL parse of `input.command` (`rm`/`mv`/`cp`/`mkdir`/`touch`/`tee`/`sed -i`/`git`/redirection) | not knowable lexically (heuristic guess) | **HEURISTIC** — always labelled `(heuristic)` |

Bash's `toolUseResult` is `{stdout, stderr, interrupted, isImage, noOutputExpected}` — **no path field** — so Bash mutations are a best-effort lexical (NOT shell) parse and are flagged heuristic everywhere they surface (text, JSON, help, SKILL). The `file_path` lives on the **tool_use** record while `toolUseResult.type` (`create`/`update`) lives on the **paired tool_result carrier**; the joiner pairs them by `tool_use_id` within the turn so `is_create` is accurate. **An op whose tool_result is `is_error:true` is EXCLUDED** — a failed Edit, or a Write `Cancelled: parallel tool call … errored` when a sibling op in the same batch failed, never landed, so counting it would be a forensic FALSE POSITIVE ("did this session write X?") and would contradict `recover` (which correctly reconstructs nothing). Same `failed_ids` gate `recover::extract` applies. Relative Bash paths are reported VERBATIM (the session's cwd at command time is not reliably known — absolutizing would fabricate a path).

**Edit-before-Read boundaries (file changed outside the tool stream).** Beyond mutations, `files` also DETECTS the `File has been modified since read` integrity errors and attributes each to its file (the rejected op's `tool_use_id` ↔ that op's `file_path`, even though the op never landed). These are the points where a formatter / linter / husky-or-pre-commit hook / git / external editor changed the file out from under the harness and a fresh Read was forced — the same authoritative boundary `recover` segments on. Surfacing them in `files` is the DISCOVERY signal: "which files in this session are risky to reconstruct?" → then `recover --file <path> --coverage` for the precise per-boundary breakdown. The error-carrier line is kept by the prefilter (an `is_error` hook) so the detection survives. (csift does NOT hunt for HIDDEN boundaries — a change with no downstream `modified since read` error, e.g. a Read-first-then-windowed-edit, leaves no transcript signal and is undetectable; verified against the CC cleanroom + binary. What `files` surfaces is the detectable subset, honestly bounded.)

Every `files` row + boundary also carries the **JSONL line number** (`Lnnnn`) of its record — the same join-back-to-the-transcript locator `recover`/`search`/`turns` provide (the parallel scan already produces it).

**Args (matches `cli::FilesArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); a `@<uuid>` positional restricts to one top-level parent session |
| `--include-subagents` / `--no-subagents` / `--subagents-only` | mutually-exclusive (clap group `subagent_scope`) | `--include-subagents` | subagent scope: default attributes SUBAGENT mutations under the session (OMC fan-out edits happen there); `--no-subagents` = top-level only; `--subagents-only` = the complement (only what the session's subagents touched) |
| `--summary` / `--by-dir` / `--by-file` / `--timeline` | mutually-exclusive enum (clap group `detail`) | `--summary` | detail level (below) |
| `--turn-range START..END` | string | none | inclusive 0-based turn range; **mutually exclusive** with `--since`/`--until` |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`/…, system-local) |
| `--format text\|json` | enum | `text` | output format |

**Detail levels (the context-blow-up guard — exactly one is active, default `--summary`):**
- **`--summary` (DEFAULT)** — compact per-top-level-dir rollup with op counts (e.g. `"/tmp: 12 write, 3 edit; spec/gaps: 4 edit"`); the smallest output, answers the acid test directly. Bucket = the mutation path's parent directory; a bare relative filename buckets under `./`.
- **`--by-dir`** — one row per distinct directory (full path) with per-op counts + distinct-file count + first/last timestamp.
- **`--by-file`** — one row per distinct absolute path with per-op counts + first/last timestamp (where "how many distinct gap docs touched" is exactly answerable).
- **`--timeline`** — full chronological list, one line per mutation `(Lnnnn, timestamp, turn index, op, path)`. The verbose mode; never the default.

Regardless of detail level, an **Edit-before-Read boundaries** section follows the mutation body (and shows on its own when a session ONLY hit boundaries, no mutations), one row per boundary `(⚠ path, Lnnnn, turn, ts, kind)`.

**Filtering** is per-mutation: `--turn-range` (turn index assigned by the §6.4 genuine-user delimiter, shared with `search`) and `--since`/`--until` (a mutation with no timestamp never falls inside a *bounded* window — same rule as §6.2). **No silent truncation:** skipped malformed lines are counted and surfaced.

**Output.** Text groups under a `SESSION <id>` header, then the level-appropriate body, then the Edit-before-Read boundary section (if any), then a footer: distinct files + total mutations + boundary count, the active detail level, the turn/time filter context, the Bash-heuristic caveat, and the skipped-line count. Empty result (no mutations AND no boundaries) prints `no file mutations found`. JSON is one object per emitted unit (bucket / dir / file with the counts + first/last; or per mutation for `--timeline` with `{path, op, ts_utc, ts_local, turn_index, line_no, is_create, heuristic}`), then one `{type:"edit_before_read_boundary", path, line_no, turn_index, kind, ts_utc, ts_local, session_id, is_subagent, parent_session_id}` per boundary (in every detail mode), then a trailing summary object `{distinct_files, total_mutations, edit_before_read_boundaries, skipped_lines, detail_level}` (mirrors §6.2's trailing-summary convention).

**Perf shape** is `search`'s: a single forward pass per file (mmap + SIMD newline scan + a pre-JSON mutation byte-prefilter, full parse only on candidate lines), no large-blob retention (extract small `FileMutation` strings, drop the record — never hold `originalFile`/`content`/`structuredPatch`), rayon across files on the default pool (= CPU count).

**Example invocations:**
```bash
csift files @<uuid>                         # default summary: per-top-level-dir op rollup
csift files @<uuid> --by-file               # per-file op counts + first/last touch
csift files @<uuid> --timeline --since 2h   # full chronological, last 2h (heavy)
csift files . --format json --by-dir        # machine-readable per-dir rollup
csift files @<uuid> --format json | jq 'select(.type=="edit_before_read_boundary")'  # which files changed outside the tool stream
```

### 6.7 `recover` — reconstruct a file's content (or a plan) from the transcript

**Purpose.** Where `files` (§6.6) only reports THAT a file was touched, `recover` rebuilds the file's **content** by replaying its Read / Write / Edit stream in transcript order. Five mutually-exclusive modes (clap group `mode`, **default = restore**): **restore** (no mode flag) hands back the file's FINAL content as RAW restorable bytes (`recover --file X > X`), but ONLY when the session saw the WHOLE file — if it observed just PART (a windowed read + a few edits) restore FAILS LOUDLY rather than emit a holey file. The failure is a SMART diagnostic: covered + missing ranges, EVERY external-change boundary (Edit-before-Read / external edit), and — when a richer state survived BEFORE the first such change (the session-authored "complete as of 5pm" case, or a fuller partial otherwise) — a `dump-the-pre-change-version (--at @line:<before>) + dump-the-changes-since (--patches --since) + reconcile-by-hand` recipe; it ALWAYS closes with the caveat that csift cannot see changes made OUTSIDE the Read/Write/Edit stream (a formatter, husky/pre-commit, git, bash) and does NOT hunt for hidden boundaries (escalated when a bash mutation may have touched the file); **`--salvage`** is restore's never-fails sibling — the best-effort line-numbered FINAL-state fragment (known lines numbered, the rest left as explicit `??? lines A..B unknown` gaps), for a file that is gone, only-partially-read, and barely-edited, where salvaging the surviving proportion beats rewinding (identical output to `--at @latest`); **`--patches`** is the segmented diff-patch CHANGES/rewind view, rendered with FULL context — every read-covered line is shown, not a 3-line window (CC's strict Read-before-Edit guarantees those lines were genuinely observed, so a fully-read, one-line-edited file reproduces in full); **`--at WHEN`** is a point-in-time partial snapshot; **`--coverage`** (alias `--dry-run`) is coverage/scoping only (its per-boundary JSON — `boundaries[]` with `line_no`/`ts`/`kind` — drives the boundary-by-boundary recover/salvage recipe in SKILL.md). `--file` is REQUIRED for all five. A magic `--file @plan` VALUE (not a mode) resolves the session-bound plan file and reconstructs THAT file exactly like any other — its full Write+Edit history, edit-aware — so it composes with every mode and with `--out`/`--format`; this is how you DUMP a plan's content, including a DELETED plan rebuilt from the transcript alone (§6.7.1 covers `@plan` + the sibling `plan` subcommand that LOCATES a session's plan). The motivating use is restoring a file (or a dropped plan) lost in a bad-recovery. (Design rationale — the reference-tool survey + the `originalFile` boundary inversion: **§11.1**.) **Every output reference carries the JSONL line number** (`Lnnnnn`) so a consumer can `Read` the raw jsonl directly — the one genuinely-new capability over the other subcommands (added via a local line counter threaded through `scan_lines_bytes`; the shared signature is untouched).

**Extraction (verified against the live corpus, 2026-06-08).** Per `--file`, in transcript order:

| event | source | result |
|---|---|---|
| full Read | `toolUseResult.file` with `startLine==1 && numLines==totalLines` (content is RAW, no gutter) | a **full-snapshot anchor** |
| windowed Read | `toolUseResult.file` with `startLine>1` or `numLines<totalLines` | a **partial splice** (gaps stay gaps — never padded) |
| Write | `toolUseResult.{type:create\|update, content}` | a full-snapshot anchor |
| Edit / MultiEdit | `toolUseResult.{oldString,newString,structuredPatch,originalFile}` (no `type`) | applied to the running buffer by `structuredPatch` line position (string-replace fallback) — **only when the paired result did NOT error.** An Edit/Write whose result was `is_error:true` (`String to replace not found in file`, or the Edit-before-Read wall `File has not been read yet`) never mutated the file, so it is NOT applied |
| integrity error | a `tool_result` with `is_error` whose body is `File has been modified since read` / `File has not been read yet` / `String to replace not found in file` (no inline path → attributed via the `tool_use_id`↔tool_use join) | a HARD boundary (modified-since-read); the `not-read-yet` and `string-not-found` cases are a non-boundary integrity ANNOTATION — the edit is skipped, not counted as a recoverable edit, never fabricated |
| Bash mutation | `input.command` lexically parsed (§6.6) | a HEURISTIC (soft) boundary |
| `edited_text_file` attachment | `attachment.{filename,snippet}` (TAB **or** U+2192 gutter) | a HARD boundary (external edit) |
| `file-history-snapshot` | `snapshot.trackedFileBackups[<path>]` | a coverage ANNOTATION only — the on-disk blob name is NOT derivable (the real `backupFileName` is frequently `null`), so it is **never** used to fabricate content |

**Reconstruction model (the "in the LLM's eyes" sparse buffer).** A `BTreeMap<file_line, cell>`; a line absent from the map is an **explicit gap** (unknown — never fabricated). A full snapshot resets the map; a windowed read splices without padding; an edit applies by structured-patch position. **Anti-fabrication guards (load-bearing):** an edit whose old region falls in an unknown gap, whose anchor neighbourhood is entirely unknown, or whose patch context disagrees with the buffer is an **un-anchorable coverage hole** (counted, never asserted as a known line). This is what keeps the reconstructed-vs-disk guarantee honest: the contiguous-from-line-1 prefix matches disk byte-for-byte even on a heavily-edited file with no clean anchor.

**Integrity boundaries (a point where reconstruction across it is invalid), in confidence order:** (1) AUTHORITATIVE — a `modified since read` harness error; (2) AUTHORITATIVE — an Edit whose `originalFile` disagrees with the replayed buffer (the signal claude-file-recovery *discards*, csift mines); (3) AUTHORITATIVE — an external `edited_text_file`; (4) HEURISTIC — a Bash mutation (always flagged best-effort). `--patches` emits one unified diff per segment between hard boundaries, each segment + boundary carrying its jsonl line / turn / timestamp; **no diff spans a boundary**. **Final-state invalidation (load-bearing for honesty):** a `modified since read` boundary means the file changed out from under the harness (a `prettier`/linter rewrite, etc.) and a fresh Read was demanded — so every line known BEFORE that boundary is now SUSPECT. The final-state reconstruction therefore INVALIDATES the pre-boundary buffer (clears the known lines + the seen-total); only content RE-READ / re-written after the boundary survives into the final state. Pre-boundary lines become explicit gaps, never silently-stale lines presented as "current". This is what stops restore from falsely reporting `complete` (and `--salvage`/`--at @latest` from dumping stale content) when a session read a file, watched it change, and only re-read part of it.

**Args (matches `cli::RecoverArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); a `@<uuid>` positional restricts to one top-level parent session |
| `--file ABS_PATH` \| `@plan` | string | none | the file to reconstruct (exact raw-string match + basename-suffix fallback); **REQUIRED** for every mode. The magic value `@plan` resolves the session-bound plan file (§6.7.1) and reconstructs it like any other file |
| `--include-subagents` / `--no-subagents` | bool | `true` | span subagent transcripts (OMC fan-out edits happen there) |
| `--salvage` / `--patches` / `--at WHEN` / `--coverage` (alias `--dry-run`) | clap group `mode` | restore (none set) | the reconstruction mode (mutually exclusive; with NONE set, the default restore mode applies) |
| `--turn-range START..END` | string | none | inclusive 0-based turn range; mutually exclusive with `--since`/`--until` |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 / relative) |
| `--line-range START..END` | string | none | 1-based inclusive file-line span to restrict the reconstructed line space |
| `--out PATH` | path | none | write the reconstructed artifact (snapshot / plan / concatenated patches) verbatim; the summary still prints to stdout |
| `--format text\|json` | enum | `text` | output format |
| `--files-from MANIFEST` | path | none | **BATCH MODE**: reconstruct EVERY absolute path listed in MANIFEST (one per line; blank lines + `#` comments ignored) in a SINGLE corpus scan. Requires `--out-dir`; mutually exclusive with `--file`. Honors `--at`/`--since`/`--until` (default = each file's final state) |
| `--out-dir DIR` | path | none | batch output dir — each recovered file is written to `<DIR>/<abs-path-without-leading-slash>` (mirrored, dirs created), plus a `recovery-report.tsv` (status · known · total · target · written_to) |
| `--force` | bool | `false` | batch: overwrite an already-present output file (default: skip it + report `skipped-exists`) |

`--at <WHEN>` accepts ISO8601, relative (`2h`), `@turn:<N>`, `@line:<N>` (state as of jsonl line N), or `@latest` (the file's FINAL reconstructed state — no cutoff; the clean way to ask for "its last form" without guessing a timestamp past the last write) and doubles as the snapshot cutoff. A datetime bound is **INCLUSIVE** of events at that instant. **`--file @plan`** is a bash-safe magic value (no shell metacharacters, no escaping in mixed scripts — consistent with `--at`'s `@line:`/`@turn:` sigils): it resolves the target session's authoritatively-bound plan file (§6.7.1) and substitutes that path, so `recover` then rebuilds the plan's FULL Write+Edit history (edit-aware, not just the latest Write) under whatever mode + window + `--out`/`--format` were given — the way to dump a plan's content, a DELETED plan included. It prefers the top-level session's own plan and **ERRORS clearly (never guesses)** when no plan is bound to the target session(s), or when the target spans sessions bound to DIFFERENT plans (asks for an explicit `@<uuid>` target).

**Output.** **restore** (default) is the exception: it writes the RAW final content to stdout (no `SESSION` banner, no line numbers — clean to pipe `> file`) with a one-line stderr note, OR `--out` writes the raw file + a stderr note while stdout stays empty; a partial file is a hard ERROR (stderr message naming covered/missing ranges + non-zero exit), and `--format json` emits a SINGLE `{file, complete:true, lines, content}` object (with `--out`: `{file, complete, lines, path, wrote}`) and NO trailer. The other four modes group text under `SESSION <id>`: `--coverage`: recoverable-line fraction, covered ranges, per-op counts, the integrity-boundary list (`⚠` authoritative / `~` heuristic), fragment count (= boundaries + 1). `--patches`: interleaved segment headers (`─ SEGMENT n  L..L  turns..  ts..  (pre-state…)`) and boundary dividers, with the unified diffs. `--at` / `--salvage`: line-numbered known lines + explicit `??? lines A..B unknown` gap markers (`--salvage` ≡ `--at @latest`). For those four, JSON is NDJSON (one object per segment/boundary/snapshot, `line_no` + `ts_utc`/`ts_local` on every object, `set_at_line` provenance per reconstructed line) + a trailing summary. **No silent truncation** — long inline content uses the `… (+N chars)` marker; JSON + `--out` are verbatim; skipped malformed lines are counted.

**Perf shape** mirrors `search`/`files`: a single forward `scan_lines_bytes` pass per file (mmap + SIMD newline scan + a broad pre-JSON byte prefilter), rayon across files. The forward path is mandatory (NOT head/tail) — it visits every line including blanks, so the local counter equals the true jsonl line 1:1. **File-level prefilter:** a transcript whose mmap never contains the target's BASENAME holds no events for it (every op `extract` replays carries the path literal), so one SIMD `memmem` skips PARSING it entirely — an unscoped single-file recover only fully parses the transcripts that touched the file. **Batch mode (`--files-from`)** turns the quadratic "N files × M transcripts re-parsed" into one pass: an Aho-Corasick of all manifest basenames gates each transcript, and a matched transcript is parsed + turn-grouped ONCE and every manifest file it touched is extracted from that shared grouping (so a session that wrote — or merely mentions — hundreds of files is parsed a single time, not once per file). Cross-session writes are merged per top-level group; when unrelated sessions each hold a version, the freshest (latest-write) wins. Output is the RAW reconstructed file (restorable bytes, not the line-numbered diff view); a partial reconstruction (a line never seen) is written best-effort and flagged `partial` in the report. (Batch is intentionally best-effort — recovering 1000+ nuked files, you want every survivable byte written + a per-file status, not the single-file restore's all-or-fail; the report's `partial` flag is the batch analogue of restore's "use `--salvage`".)

**Example invocations:**
```bash
csift recover @<uuid> --file /abs/app.py                # DEFAULT restore: raw final content (or FAIL if only partial)
csift recover @<uuid> --file /abs/app.py --out /abs/app.py  # restore straight back onto disk (raw bytes)
csift recover @<uuid> --file /abs/gone.py --salvage     # gone + only partly seen: dump what survived, gaps explicit
csift recover . --file /abs/PLAN.md --coverage          # scope first: covered ranges + boundaries
csift recover @<uuid> --file /abs/app.py --patches      # segmented unified diffs over the session
csift recover @<uuid> --file /abs/app.py --at @turn:42  # partial snapshot as the LLM saw it at turn 42
csift recover @<uuid> --file @plan --out /tmp/restored-plan.md  # rebuild the session's bound plan (DELETED ok), write it out
csift recover --files-from nuked-files.txt --out-dir /restore   # BATCH: reconstruct every listed file in ONE corpus scan
csift plan @<uuid>                                      # LOCATE the plan file bound to a session (does not dump it)
```

### 6.7.1 `plan` — locate the plan file BOUND to a session (and the `@plan` recover sigil)

**Purpose.** `plan` LOCATES (does not dump) the plan file a session is bound to. To DUMP a plan's content — including a deleted one rebuilt from the transcript alone — use `csift recover @<uuid> --file @plan` (§6.7); `@plan` resolves through the SAME binding this subcommand reports. **`--reverse <PLAN_FILE>`** inverts the direction: given a plan file, it scans the resolved scope (default every project; narrow with a PATH target) for the `plan_mode` binding that names it (absolute-path identity) and reports the bound session/subagent id(s) + the binding's jsonl line — "which conversation owns this plan?". Conflicts with a positional target; an empty result (nobody bound) is honest, not an error. JSON: `{plan_file, session_id, is_subagent, parent_session_id, line_no}` per bound session.

**The binding is AUTHORITATIVE, not a path heuristic.** When a session enters Plan Mode, Claude Code writes a `plan_mode` **attachment record**:
```
{"type":"attachment","attachment":{"type":"plan_mode","planFilePath":"…","isSubAgent":…,"planExists":…}}
```
The session's bound plan is exactly that `planFilePath`. This matters because a session may freely Edit/Write OTHER sessions' plan files (ordinary tool calls on a `~/.claude/plans/…` path) — those are NOT its own plan, and a path-shaped guess would mis-attribute them. Only the `plan_mode` attachment binds a plan to a session.

**Plan-file storage facts.** Claude Code stores plans flat under `~/.claude/plans/` with a random three-word name like `nested-prancing-popcorn.md`; a subagent's plan gets an `-agent-<hex>` suffix. The name is **NOT derivable from the session id** — only the `plan_mode` attachment binds them.

**Targeting.** A project PATH / encoded-dir / `@<uuid>` positional. With **no target** it resolves the CALLING session from `CLAUDE_CODE_SESSION_ID` (like `whoami`, §6.3). It spans subagents by default — their own plans surface, flagged as a subagent with the re-feedable parent uuid; `--no-subagents` restricts to the top-level session.

**`@plan` resolution (the recover sigil).** `recover --file @plan` prefers the top-level session's own bound plan, then reconstructs it like any other file. It **ERRORS clearly (never guesses)** when no plan is bound to the target session(s), or when the target spans sessions bound to DIFFERENT plans (the error asks for an explicit `@<uuid>` target).

**Args (matches `cli::PlanArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH-or-session]` | positional | calling session | a project PATH / encoded dir / `@<uuid>`; absent ⇒ resolve `CLAUDE_CODE_SESSION_ID` |
| `--no-subagents` | bool | — | restrict to the top-level session (default spans subagents, surfacing their own plans) |
| `--format text\|json` | enum | `text` | output format |

**Per-resolved-session fields emitted:** `session_id`, `is_subagent`, `parent_session_id`, `plan_file`, `plan_exists` (on disk), `line_no` (the jsonl line of the binding `plan_mode` attachment). Text format + `--format json` (NDJSON, one object per plan).

**Example invocations:**
```bash
csift plan                                  # the calling session's bound plan (resolves CLAUDE_CODE_SESSION_ID)
csift plan @<uuid>                          # a specific session's bound plan
csift plan . --no-subagents                 # this project's top-level sessions only
csift plan @<uuid> --format json            # machine-readable, one object per plan
csift recover @<uuid> --file @plan          # DUMP the plan's content (this subcommand only LOCATES it)
```

### 6.8 `turns` — turn-fidelity reconstruction (restore the back-and-forth a compaction summary clipped)

**Purpose.** A Claude Code **compaction summary** preserves TASK STATE (its 9-section synthesis: primary request, key concepts, file ledger, errors+fixes, plan, next step) in high fidelity, but provably **loses turn fidelity**: its "All user messages" section clips real prose turns to `...`-truncated bullets (measured: ~22 real user turns → ~17 bullets), and the assistant side collapses to a SINGLE verbatim quote (the last pre-compaction message). `turns` **supplements** the summary — it re-emits the verbatim user/assistant TURNS, in original order, each line carrying the JSONL line number (`Lnnnnn`) so a consumer can `Read` the raw transcript at the cited line. It does **not** re-derive task state (the summary owns that; duplicating it wastes budget and risks contradiction). The split of labor is the summary's own design — its trailer says "read the full transcript at `<path>`" for the exact content it generated; `turns` automates that pointer. (The measured basis for every default below, and the proof that the budget really reaches back across compaction boundaries: **§11.2**.)

**Reuse, no re-parse.** `turns` sits on the §6.7 `recover` extraction layer verbatim: the same `scan_one_file` forward line-numbered `scan_lines_bytes` pass (the 1:1 jsonl line map), the same `group_turn_indices` (§6.4) turn delimiter, the same `Record` helpers (`is_genuine_user` / `genuine_user_text` / `agent_text` / `blocks` / `is_compact_summary`), the same `resolve_session_files` / `TimeWindow` / `timez` rendering. The byte prefilter is a SUPERSET of recover's, broadened with `"role":"assistant"` / `"type":"assistant"` probes so a pure-text assistant turn (carrying none of Edit/Write/Read/Bash) is never missed. The `Record`/`Block` model needs no change.

**Selection vs render order.** Selection walks **backward from EOF** (recency-first) so the budget is spent on what a resumed agent most needs; the emitted document is sorted **ascending** so it reads as a forward transcript. The backward walk is **transparent to `isCompactSummary` boundaries** (a summary is a turn MEMBER, never a delimiter — §6.4 / model.rs), so it reaches back across multiple compaction boundaries by default (verified on real transcripts: a 40K-char ellipsized budget spans 26 boundaries on a 35-summary session; `--max-compactions N` caps the crossing count).

**Budget allocation (two-phase).** `--budget <N>` (default 40000) bounds the whole reconstruction in chars (or tokens via `--budget-unit tokens`, ≈4 chars/token). `--round-trip-fraction <F>` (default 0.5) is a **hard floor**: Phase 1 spends `budget·F` ONLY on round-trip-complete turns (user && assistant-EOT), walking recency-first; Phase 2 fills the rest with whichever single sides remain, user-first (the user wording is the scarcer, higher-signal loss). Without the floor an assistant-heavy tail recovers ZERO user turns (measured on a real pulse-shaped tail). The `[N tool calls]` marker cost is charged per turn (omitted when 0). Determinism: recency = descending line_no, ties by descending turn_index.

**Multi-agent-message richness (`--agent-msgs`).** A single user turn can own a LONG run of agent messages (a debugging/build chain the model narrates step by step) that the summary clips to its single §9 quote. Each `TurnSlice` carries EVERY agent-text record (`agents: Vec<AgentMsg>`); a derived `assistant_eot()` (== `agents.last()`) keeps the EOT anchor for dedup/round-trip/render. `--agent-msgs` decides how much of the run to restore — four modes: **`longest`** (DEFAULT) keeps the LONGEST agent message (the best one-message proxy for "where the substance is" — the summary's single quote is the turn's LAST message, often a ~50-char throwaway wrap-up, while the real finding sits in a MIDDLE one) **+** the first-when-substantive **+** every rich middle, collapsing the rest (including a short non-rich last) into a placeholder; **`eot-only`** forces last-message-only — byte-identical to the pre-expansion single-EOT output (the escape hatch); **`rich`** keeps the last always + the first by position privilege + each non-droppable middle; **`all`** keeps every message. `longest` applies to every multi-message turn; `rich` only filters a LONG run (`agents.len() > --agent-run-threshold`, default 6). A message is **kept when "rich"** — a cheap single-pass OR of a length gate (`>= --agent-rich-min-chars`, default 280) and a signal test (a number-of-substance, a commit-hash-like hex, a `file.rs:NNN`/`src/…` ref, a backtick code span, or a finding/decision lexeme). **KEEP-ON-DOUBT** is the spine: only a short (`< --agent-declaration-max-chars`, default 200) signal-less intent-verb opener (`let me …`/`now i …`) is collapsed; anything uncertain is kept (a wrongly-kept declaration costs ≤ one capped body; a wrongly-dropped finding is unrecoverable). A FUSED finding+declaration body trips a signal → kept WHOLE, its trailing declaration shed only by the char-ellipsis. A contiguous collapsed run renders as one `△ L{first}–L{last}  [X agent message(s), Y tool call(s)[, Z failed]]` placeholder carrying the fetchable line range + per-message attribution. `--keep-first` (default) keeps a turn's first message by position privilege **in `rich` mode** (`--no-keep-first` decides it as a middle); it has NO effect in `longest`, where the first is gated on length instead. `--profile heavy|light` bundles the thresholds (applied before the individual flags, explicit flag wins; the master `--agent-msgs` mode is unchanged, so `--profile heavy` alone still runs `longest`). Subagent transcripts get the SAME treatment via the shared selection path. The summed-cost == summed-emitted invariant holds with placeholders (the dropped bodies contribute zero cost; the placeholder line is charged like any emitted line).

**Ellipsis (role-asymmetric middle-truncation).** A unit over its role cap (`USER_CAP=600`, `ASST_CAP=900`, sized from measured medians) is **middle-truncated**, keeping head+tail, with an explicit `… [+K chars, L lines elided] …` marker (the line count uses the pre-normalization text; omitted for single-line user messages). The assistant head is both absolutely larger (900 vs 600) and a larger fraction (0.66 vs 0.60 → head 594/tail 306 vs user 360/240), because EOT prose front-loads context and back-loads the decision. Cuts are on `char` boundaries (UTF-8 safe). No content is fabricated; nothing is silently dropped.

**Dedup against the live summary.** The newest in-range summary is already in the resumed model's context (it IS the seed). A live-region turn (compactions_before == 0) whose 80-char normalized prefix matches the summary's §6 user bullets or §9 assistant quote is flagged `(also in summary)` and **demoted** (selected only after non-dup turns) — never silently dropped (a false positive must not lose a real turn). Turns predating an OLDER boundary are genuinely gone from context, so they are pure restoration, never deduped.

**Output.** Text groups under `SESSION <id>`: a budget-accounting header, then turn-by-turn `▽ Lnnnnn USER (ts)` / `[N tool calls]` / `△ Lnnnnn ASSISTANT (ts)` (one `△` line per KEPT agent message under `--agent-msgs rich`/`all`), with `══ compaction boundary · summary at Lnnnnn ══` banners at crossings, `(also in summary)` flags on demoted units, and `△ L{a}–L{b}  [X agent messages, Y tool calls, Z failed]` placeholders for collapsed agent-message runs. `--out` writes the full (un-terminal-truncated) reconstruction to a file while the summary prints to stdout. JSON (`--format json`) emits one VERBATIM (un-truncated `text`) object per unit (`line_no`, `role`, `ts_utc`/`ts_local`, `tool_calls`, `full_chars`, `rendered_chars`, `truncated`, `elided_chars`/`elided_lines`, `also_in_summary`, `compactions_before`) plus interleaved `{"kind":"compaction_boundary","line_no":…,"summary_chars":…}` records and, per collapsed agent-message span, a `{"kind":"collapsed_agents","agent_messages":…,"tool_calls":…,"failed":…,"first_line":…,"last_line":…}` record. **No silent truncation** — skipped malformed lines are counted and surfaced.

**Chunked output for hook injection (`--slice <N>` / `--window <N>`).** A Claude Code `SessionStart` hook can inject at most **10,000 CHARACTERS** of `additionalContext` (over-cap output is replaced by a file-path + preview, so the body is lost), so a >10K reconstruction must be fanned across several hooks. `--slice <N>` prints ONLY the Nth (1-based) chunk of the verbatim DOCUMENT (the `--out` body — turn units + boundary banners, NO scope/header/footer chrome) after packing its lines greedily into `≤ --window`-CHARACTER chunks. `--window` defaults to 10000 and counts CHARACTERS (Unicode scalars — the unit the cap itself counts, so CJK prose is not 3× over-charged the way bytes would be), hard-splitting any single line longer than the window on a char boundary so no chunk ever exceeds it. Slicing is **deterministic** (same session + `--budget` ⇒ identical chunk boundaries; concatenating slices `1..K` reproduces the document byte-for-byte), so N independent `SessionStart(compact)` hooks each request their own slice and the lock/ordering lives in the hook shell. An out-of-range `N` prints nothing (exit 0); `--slice` is **text-only** and **mutually exclusive with `--out`** and with `--format json`; `--slice 0` or `--window 0` errors. **`--slices <N>` (FIXED-FLEET mode)** pins the chunk COUNT to match a fixed set of N registered `SessionStart` hooks: csift fills the N newest-first slices with WHOLE turns (the per-role 600/900 caps are dropped — a turn is ellipsized only if it ALONE exceeds one window) and DISCARDS the oldest overflow, so the count never drifts to 5/6/7 as the conversation grows (the hook count never needs re-tuning); the realized budget becomes `N × --window` and `--budget` is ignored, with `--slice i` still picking which chunk to print. Without `--slices`, `--slice` keeps its legacy budget-driven, variable-chunk-count behavior. Safe to fire on every compaction — the drop-and-re-inject cycle (the old injection is summarized away pre-boundary while `SessionStart('compact')` re-injects fresh) prevents context pile-up.

**Windowing** matches §6.7: `--turn-range START..END` (inclusive, 0-based genuine-user order) is mutually exclusive with `--since`/`--until` (ISO8601 / relative).

**Example invocations:**
```bash
csift turns .                                   # default 40K recon (longest agent msg + rich members per turn)
csift turns @<uuid> --budget 12000              # a 200K-context-sized recovery (~10-15K)
csift turns @<uuid> --budget 40000 --format json # machine-readable, line-numbered
csift turns @<uuid> --round-trip-fraction 0.6    # weight harder toward complete round-trips
csift turns @<uuid> --agent-msgs eot-only        # force the old single-EOT (last-message-only) output
csift turns @<uuid> --profile heavy              # longest mode, lower thresholds (max fidelity)
csift turns @<uuid> --agent-msgs all             # every agent message, no filtering
csift turns . --budget 40000 --out /tmp/turns.md  # full reconstruction to a file
csift turns . --window 9000 --slices 4          # fixed 4-chunk fan-out for 4 SessionStart hooks
csift turns . --window 9000 --slices 4 --slice 1  # print the 1st of those 4 chunks
```

### 6.9 `image` — list + extract the images a session carries

**Purpose.** A pasted/attached image (and a tool-result screenshot) is stored **inline** on a record as an `{type:"image", source:{type:"base64", media_type:"image/png", data:"<base64>"}}` block — verified on real `~/.claude/projects` data (2026-06-16): a single user record commonly carries several. The bytes are in the jsonl, so `image` lists them and decodes them straight back to files; nothing was externalised. This is the way to get a sent image back out of a transcript without hand-parsing base64.

**Two addresses, by design.**

- **`#N` — the session handle** (preferred). This is the SAME `[Image #N]` number the model sees and refers to ("re-share #32") — Claude Code renders pasted images as `[Image #N]` text markers, and `image` recovers `N` by positionally zipping a record's markers with its image blocks. So a consumer reading a `[Image #32]` reference (in `turns`/`search` output, or in its own context) addresses it directly: `--id #32`. **`#N` is NOT unique across a session** — CC numbers per-prompt and **reuses** low numbers across prompts (`history.ts`: "unique within a single prompt but not across prompts"). When a `#N` names **>1 distinct image**, `--id #N` is **AMBIGUOUS and ERRORS** with the occurrence list — each occurrence's `t<turn>` / `L<line>i<n>` locator / uuid / time / excerpt-around-`[Image #N]` — rather than silently guessing. Disambiguate by the exact locator, or by narrowing scope with `--since`/`--until` (a time window), `--turn-range`, or `--uuid` (each can be **pre-applied** so `#N` is already unique — e.g. `--since 1h` for the last hour). When a record's marker count doesn't match its image count, `#N` is left unset for that record (only the locator addresses it) — never mis-attributed.
- **`L<line>i<n>` — the exact locator** (always unambiguous). The 1-based JSONL line of the carrying record + the 1-based ordinal of the image among that record's image blocks (a direct `Block::Image`, OR an `{type:"image"}` element nested in a `tool_result` content array, counted in document order). Stable because the transcript is append-only, and consistent with the `Lnnnnn` line refs `recover`/`turns`/`search` already emit. Use it to pin one specific occurrence regardless of `#N` reuse.

`turns` and `search` both surface these ids inline (`[N image(s): #32, #33, …]` under a user turn / as a hit suffix), so an id seen there feeds straight back into `--id`. Default action is to **LIST** (deduped — see Behaviour); `--out <DIR>` switches to **EXTRACT**.

**Args (matches `cli::ImageArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); a `@<uuid>` positional restricts to one top-level session |
| `--id ID` | repeatable + comma | none | address images by the `#N` handle (`--id #32,#33`; bare `32` == `#32`) or the exact `L<line>i<n>` locator (`--id L6812i1,L6812i2`); without `--out` filters the LISTING, with `--out` selects what to extract. Both forms are per-transcript, so `--id` needs a single transcript in scope (pin with `@<uuid> --no-subagents`). An ambiguous `#N` errors (above) |
| `--since` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`, system-local) — narrows the image set, a `#N` disambiguator |
| `--turn-range A..B` | string | none | 0-based inclusive turn window — a per-transcript `#N` disambiguator (turn indices come from the shared §6.4 delimiter) |
| `--uuid PREFIX` | string | none | restrict to the record whose uuid starts with this — a `#N` disambiguator (the uuid is shown in the ambiguity list / JSON) |
| `--out PATH` | path | none | EXTRACT. The path's EXTENSION drives the format (the `convert in out.jpg` idiom): a **directory** (or any path with no `png`/`jpg`/`jpeg`/`gif`/`webp` extension) writes each image auto-named `<session-short>[-img<N>]-L<line>i<n>.<ext>` in its SOURCE format; a path WITH one of those extensions writes the **single** selected image to exactly that file, CONVERTING if the format differs (see below). >1 image with a file path is an error. Without `--out`, only LIST |
| `--include-subagents` / `--no-subagents` | bool | `true` | span subagent transcripts (a tool screenshot may live there); `--no-subagents` dominant |
| `--format text\|json` | enum | `text` | output format |

**Behaviour.** Same scan shape as `recover`/`files` (mmap + a pre-JSON image byte prefilter + parse only candidate lines, line-numbered 1:1). The media type (and thus the default extension) is read **per image** from its `source.media_type` — never assumed PNG; real sessions carry a mix (png, jpeg, …), and the `~/.claude/image-cache/*.png` mirror is a lossy `.png` rename that the inline bytes do not share. The **LISTING is content-deduped**: the same image re-injected across context windows (every prompt re-sends attached images + compaction re-includes them) is shown **once**, keeping its latest occurrence, via a cheap `<len>:<head>:<tail>` base64 fingerprint; two DISTINCT-content images sharing a `#N` both survive (so the reuse is visible — and `--id #N` errors on it). Decoded size is **estimated** for the listing (4 b64 chars → 3 bytes) without decoding the large payload; extraction decodes in full and reports the exact byte count. A `source.type == "url"` image has no inline bytes — it is reported (with its URL), never fabricated into a file. **No silent truncation / no silent miss:** an explicitly-requested `--id` that matches nothing is an error; a base64 that fails to decode is an error (never a wrong file); skipped malformed lines are counted. Base64 decoding is a small in-crate standard-alphabet decoder; format transcoding uses the `image` crate + libwebp (the heavyweight dependencies — only `image.rs` touches them).

**Format conversion (output extension).** A directory output (no image extension) writes the original bytes under the source-inferred extension (the written path + extension is echoed, so a caller that omitted an extension sees what it got). A file output whose extension **differs** from the source is **converted, never rejected**: →png is lossless, →jpeg is **quality-90 lossy** (brief note), →gif is **Floyd-Steinberg dithered** into a ≤256-color NeuQuant palette (brief note; a sub-2px image skips the dither), →webp is **quality-90 lossy** via libwebp (the `webp` crate; brief note). A file output whose extension **equals** the source writes the original bytes unchanged (so an animated GIF stays animated). An **animated GIF** converted to a still format yields its **first frame**, with a warning carrying the frame count + total duration (`animated GIF (3 frames, 1.5s) — extracted the first frame only`). A decode/encode failure is surfaced (never a wrong file).

**Text output example** (leads with the `#N` handle when known, else the bare locator):
```
#1    L6802i1  image/png ~440 KB  2026-06-11 10:25:50 AEST (2026-06-11T00:25:50.942Z)
#2    L6812i1  image/png ~440 KB  2026-06-11 10:27:10 AEST (2026-06-11T00:27:10.447Z)
#3    L6812i2  image/png ~252 KB  2026-06-11 10:27:10 AEST (2026-06-11T00:27:10.447Z)
55 image(s) · 3 transcript(s)
```
(A subagent image row is suffixed `  SUBAGENT <hex> · parent <uuid>`.) The **listing/JSON** is one object per image (`handle`, `seq`, `id` [the `L<line>i<n>` locator], `line_no`, `img_index`, `session_id`, `is_subagent`, `parent_session_id`, `source_kind`, `media_type`, `b64_len`, `est_bytes`, `url`, `record_uuid`, `ts_utc`/`ts_local`) + a trailing `{images, transcripts, skipped_lines}` summary. **Extract JSON** carries `path`, `bytes`, `media_type` (output), `source_media_type`, `converted`, and `notes` (the conversion/first-frame warnings).

**Example invocations:**
```bash
csift image @<uuid>                             # list every image (deduped), id · type · ~size · time
csift image . --format json                     # machine-readable listing
csift image @<uuid> --out /tmp/imgs             # extract ALL to a DIR (source formats, auto-named)
csift image @<uuid> --no-subagents --id '#32,#33,#34,#36' --out /tmp/imgs   # re-share by handle
csift image @<uuid> --no-subagents --id '#1' --since 1h --out /tmp/imgs     # disambiguate a reused #1 by time
csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg         # one image to a FILE → convert to jpeg
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
- **`search`** → an optional leading `{kind:"session_header",…}` scope record (only when the scope spans ≥1 subagent), then one object per Exchange **in combined chronological order** (subagents interleaved by `ts_utc`): `{ session_id, is_subagent, parent_session_id, turn_index, ts_utc, ts_local, hits:[{category, excerpt, ts_utc, ts_local, tool_name}], record_uuids:[…] }` (mirrors `search::Exchange` + `Hit`; the envelope `ts_utc`/`ts_local` = the turn-opening `started_utc`, the key the timeline is sorted on — a per-hit `ts_utc` may be later for a deep tool_use match), followed by a trailing summary object `{ matched, dropped_by_cap, skipped_lines }` (mirrors `search::SearchOutcome`).
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

## 11. Design notes & empirical grounding

> The normative spec is §0–§9; this section records the **design rationale and the measurements** behind the three deepest features (`recover`, `turns`, `agents`) — *why* the algorithm is shaped as it is and *why* the magic numbers are what they are. All counts below are empirical, captured against the live `~/.claude/projects` corpus on the dates noted; treat them as representative magnitudes, not invariants (a live session's subagent counts drift upward as it spawns more).

### 11.1 `recover` — reference-tool survey + the `originalFile` boundary inversion

Four prior tools were studied (ccdiag, claude-file-recovery, coding-agent-session-search, florian-gist). **None reconstructs file content across integrity boundaries, and none segments at out-of-band-edit boundaries** — that is `recover`'s original territory (§6.7). Only four peripheral primitives were harvested from them:

1. the `tool_use_id ↔ tool_result` join (used everywhere a result must be attributed to its intent);
2. the Read `→` (U+2192) gutter-strip regex (recovers raw line text from a visible `cat -n`-style Read);
3. the `file-history-snapshot` on-disk backup channel (which claude-file-recovery parses and ccdiag discards) — used only as a coverage annotation, never to fabricate content, because the real `backupFileName` is frequently `null`;
4. `(timestamp, session_id, line_number)` as the ordering key.

**The load-bearing inversion** is over claude-file-recovery: where it uses an Edit's `originalFile` to *paper over* drift (assume the file was whatever `originalFile` claims), `recover` uses **replayed-buffer ≠ the next op's `originalFile`** to *detect and segment at* the drift — that disagreement is exactly the AUTHORITATIVE integrity boundary #2 of §6.7. The same "in the LLM's eyes" sparse-buffer model (a `BTreeMap<file_line, cell>`; an absent line is an explicit gap, never fabricated; an un-anchorable edit becomes a counted coverage hole) is what keeps the reconstructed-vs-disk guarantee honest — the contiguous-from-line-1 prefix matches disk byte-for-byte even on a heavily-edited file with no clean anchor. The one genuinely-new capability under all of this is the per-line `Lnnnnn` counter (§6.7), threaded locally so the shared `scan_lines_bytes` signature is untouched.

### 11.2 `turns` — the measured basis for every default

**What the summary loses (the anatomy that motivates the feature).** Measured over three real sessions (246 MB, 130 MB, and 80 MB): a Claude Code compaction summary preserves task STATE in high fidelity but provably loses TURN fidelity — its §6 "All user messages" clips ~**22 real user prose turns → ~17 `...`-truncated bullets**, and the assistant side collapses ~**239 assistant turns → exactly 1 verbatim quote** (the last pre-compaction message). `turns` supplements (never re-derives) the summary by restoring those verbatim turns in order, each carrying its `Lnnnnn`.

**Every default is sized from those measurements**, not guessed:

| parameter | default | empirical basis |
|---|---|---|
| `--budget` | 40000 chars | a 1M-context session at 40–50% compaction comfortably recovers ~40K; a 200K context → ~10–15K |
| `--budget-unit` | chars | tokens via ≈4 chars/token (a ~17K-char summary measured ≈ ~3.5–4.5K tokens) |
| `--round-trip-fraction` | 0.5 | ~50% reservation guarantees the back-and-forth; without it an assistant-heavy tail recovers ZERO user turns (the 10K-verbatim case → `users=0`, below) |
| `USER_CAP` | 600 | user text median 410 / p90 2,574 → 600 keeps the median whole, ellipsizes only the tail |
| `ASST_CAP` | 900 | assistant-EOT natural-stop median 608–1,710 (1.45–2.16× the user side), more newlines → a larger cap |
| assistant head fraction | 0.66 | EOT prose front-loads context, back-loads the decision → keep head ≈ ⅔ |
| user head fraction | 0.60 | the user front-loads the ask → slightly less tail needed |
| `--agent-run-threshold` | 6 | a run > 6 messages fires on ~52–55% of multi-message turns (the rich-filter trigger) |
| `--agent-rich-min-chars` | 280 | ≈ 1.5× the measured 184-char median middle message |
| `--max-compactions` | 0 (∞) | reach across multiple boundaries by default; a guard, not a target |

**The spanning is proven, not asserted.** A backward char-budget walk on the two real multi-compaction transcripts, with vs without the §6.8 ellipsis cost model (USER_CAP 600 / ASST_CAP 900):

| sample | compactions in file | budget | mode | boundaries spanned | users / asst recovered |
|---|---|---|---|---|---|
| alpha | 35 | 40K | verbatim | 1 | 5 / 15 |
| alpha | 35 | 40K | **ellipsis** | **3** | **22 / 49** |
| beta | 54 | 40K | verbatim | 2 | 2 / 127 |
| beta | 54 | 40K | **ellipsis** | 2 | 2 / **160** |
| beta | 54 | 10K | verbatim | 0 | **0** / 26 |
| beta | 54 | 10K | ellipsis | 1 | 0 / 35 |

Three conclusions this *proves*: (1) a 40K ellipsized budget spans ≥2 boundaries on both samples (alpha 3, beta 2) — and **the ellipsis compression is what makes it reach back** (alpha 40K: 1 boundary → 3, recovery 5/15 → 22/49); (2) a naive recency walk starves the round-trip guarantee (beta 10K verbatim → 0 users), which is the empirical justification for the 50% floor forcing user inclusion; (3) 200K-context sizing (~10–15K) spans ~1 boundary, 1M-context (~40K) spans 2–3 — matching the budget→reach guidance.

### 11.3 `agents` — the topology fix + verified corpus

**Why the topology layer matters.** Discovering subagent transcripts as a flat list — each nested transcript a detached session, with no LINKAGE back to the parent `tool_use` that spawned it — answers only three of six real queries and answers two more lossily (the "when" would be the lagging child-head ts; there would be no workflow-run grouping). `agents` therefore builds a topology on top of discovery: a `ParentSpawnIndex` (one forward scan of the parent transcript joining `tool_use_id → {spawn tool, trigger ts, description, subagent_type}`) joins each subagent to its spawning `tool_use`, recovering the returned message, files-changed, and the topology tree. Two consequences of the linkage are specified in §6.5: `--by` filters on the true `trigger` axis (the accurate spawn instant, sharper than the child-head `start` ts), and a subagent row's printed id is the bare `<hex>` (joinable to an `agents` node), not the un-joinable `agent-<hex>` stem.

**Verified on-disk (a representative live session, 2026-06-08).** 152 built-in (`subagents/agent-<hex>.jsonl`) + 404 workflow (`subagents/workflows/wf_*/agent-<hex>.jsonl`) = 556 subagents, **0 nested** (depth uniformly 1 — Claude Code provisions subagents without an agent-spawn tool, so there are zero sub-sub-agents; confirmed across 2348 transcripts). The parent transcript carried 151 `Agent` + 22 `Workflow` tool_uses (this corpus spells the built-in spawn tool `Agent`, not `Task`; `Task` is matched defensively for other corpora), and 19 top-level `workflows/wf_*.json` run manifests. The **3-way returned-message resolve** broke down 147 sync-tool-result, 6 async-child-tail (the parent result was the `Async agent launched …` run-in-background sentinel → fall back to the child transcript tail), 403 workflow-journal (the `journal.jsonl` `result` event payload), 1 unresolved — 0 linkage mismatches.
