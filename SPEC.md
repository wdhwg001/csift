# SPEC.md — csift

> **Status: AUTHORITATIVE — implemented.** This is the single source of truth for *what csift does and how*. Its design is grounded in real `~/.claude/projects` data (51 sampled sessions across CC 2.1.133–2.1.168, files up to 225 MB / 115 879 records), and the measurements throughout are cited as the evidence for each decision. All thirteen subcommands (§6) are built; §10 folds in the per-feature design rationale + the empirical measurements behind the deepest three (`recover` / `verbatim` / `agents`). [`AGENTS.md`](./AGENTS.md) (Claude Code loads it as the `CLAUDE.md` symlink) remains authoritative for *how to work in the repo* (git discipline, gates, conventions); this file is authoritative for *what to build*. An engineer who has never seen a Claude Code (CC) `.jsonl` should be able to implement csift from this document alone.

---

## 0. Mission & non-negotiables

**csift = "ripgrep for Claude Code session transcripts".** A fast Rust CLI to **list** and **regex-search** CC session `.jsonl` files. Thirteen subcommands: `list`, `search`, `show` (record FETCH by line / turn / uuid, §6.11), `stats` (one-scan aggregates, §6.12), `agents` (subagent lifecycle, §6.5), `whoami`, `files` (which files a session changed, §6.6), `recover` (reconstruct a file's content from the transcript, §6.7), `plan` (locate the plan file bound to a session, §6.7.1), `verbatim` (turn-fidelity reconstruction across compaction, §6.8), `image` (list + extract the images a session carries, §6.9), `status` (one-shot LIVE verdict, §6.13), `wait` (block until a session condition fires, §6.14). `list`/`search`/`stats`/`files`/`recover`/`plan`/`image`/`status`/`wait` span each session's subagent transcripts by default (`--no-subagents` opts out); `verbatim` is the exception (top-level thread only, `--subagents` opts in, a target is REQUIRED); `agents` lists subagents as its targets (rejects both span flags).

- **Primary consumer is an LLM** (a CC agent searching/recovering its own or a peer session). Output must be clean, token-efficient, and regex-driven. Human/LLM-readable text by default; `--format json` for machine use.
- **Explicitly NO BM25 / embeddings / semantic search.** Pure regex/ripgrep only. Lexical tokenisation across scripts (CJK / multi-byte) is intractable for scoring; regex is the strength and the whole point.
- **Quality gates (hard):** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` all green. No `unwrap`/`expect` in library/hot paths (only `main` maps error→exit code, tests may `unwrap`). **No silent truncation** anywhere.
- **Output geometry (crate-wide design law, v0.7.0).** A truncating consumer (`| head -N` / `| tail -N`) amputates exactly one end of the output. Therefore: (a) anything load-bearing appears at BOTH ends, or inline on the rows themselves; (b) the head must carry enough to interpret what follows (scope, totals, direction); (c) the tail remains the complete ledger (full summary, integrity notes, cautions); (d) the blessed way to limit output is `--max-count`, which keeps every note intact — piping through `head`/`tail` is the discouraged fallback. (Applied to `search` in v0.7.0; auditing the other subcommands against this law is a recorded follow-up.)

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
encoded = nfc(session_start_cwd).replace(/[^a-zA-Z0-9]/g, "-")   // JS semantics: per UTF-16 code unit
```

The cwd is **NFC-normalized**, then every UTF-16 **code unit** not in `[A-Za-z0-9]` becomes a single `-`, **one-for-one, position-preserving, with NO collapsing of consecutive dashes** (extracted verbatim from the 2.1.228 binary: `gT`/`upo` — `e.replace(/[^a-zA-Z0-9]/g,"-")` — with `.normalize("NFC")` riding every projectRoot path). Unit-wise matters only off ASCII: an astral char is two surrogate units → **two** dashes; an NFC-composed accent is one unit → one dash. That is the entire algorithm.

- `/`, `_`, `.`, space, `-`, and every other non-alphanumeric → a single `-` each. No special-casing of dots, dot-dirs, or extensions.
- **No dash collapsing.** Two adjacent special chars emit two dashes. **Proof:** the real dir `-Users-testuser-Projects-Acme-widget-factory--cache-worktrees-sunny-meadow` decodes from `/Users/testuser/Projects/Acme/widget_factory/.cache-worktrees/sunny-meadow`: the `/.` (slash then the hidden-dir dot) emits a literal `--`. A collapse rule can never emit `--`, so its mere existence refutes collapsing.
- **Leading `/`** (Unix absolute paths) → a leading `-`; every Unix-encoded dir begins with `-`. No stripping. **On WINDOWS the shape differs:** `C:\Users\x` → **`C--Users-x`** (the drive letter passes through; `:` and `\` are one dash each — a drive-encoded dir leads with a LETTER), and a UNC `\\server\share` → `--server-share` (leads with `--`). Both shapes are first-class `@`-targets; the bare-positional form covers Unix + drive shapes (a bare `--…` token is reserved for the mistyped-flag guard — target a UNC-encoded dir via `@--server-…`).
- **Trailing separator:** cwds are stored without a trailing slash, so no trailing dash in practice.
- **Case is preserved** (`Users`, `Projects`, `Acme` keep their capitals — NOT lowercased). Letters/digits pass through unchanged.

Worked examples (all verified):

| cwd | encoded dir |
|---|---|
| `/Users/testuser/Projects/widget_app_prototype` | `-Users-testuser-Projects-widget-app-prototype` |
| `/Users/testuser/Projects/Acme/widget_factory-worktrees/main` | `-Users-testuser-Projects-Acme-widget-factory-worktrees-main` |
| `/a/.claude/b` | `-a--claude-b` |

Reference implementation (matches `src/path.rs::encode_cwd`: NFC first, then per UTF-16 code unit — ASCII-alphanumeric pass-through, every other unit → `-`):

```rust
pub fn encode_cwd(cwd: &Path) -> String {
    use unicode_normalization::UnicodeNormalization;
    let s: String = cwd.to_string_lossy().nfc().collect();
    s.encode_utf16()
        .map(|u| match u8::try_from(u) {
            Ok(b) if b.is_ascii_alphanumeric() => char::from(b),
            _ => '-',
        })
        .collect()
}
```

**Two more rules CC applies (verified against the shipping binaries — `Siq` 2026-06-16, re-verified as `gT`/`upo`/`wn` in 2.1.228 on 2026-08-12):**

1. **Canonicalization first.** CC encodes the cwd's **`realpath` + Unicode-NFC** form (`canonicalizePath`), not the raw string — so `/tmp`↔`/private/tmp` symlinks and NFC/NFD variants *converge* to one dir. csift's `absolutize` does the realpath (`canonicalize`) and `encode_cwd` now applies NFC + the unit-wise replacement itself, so the formerly-documented NFD dash-count divergence is **closed** (v0.7.3): an NFD-decomposed accented path encodes identically to its NFC spelling, matching CC.

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

Ignored-but-tolerated extras (do NOT model; serde drops them): `entrypoint`, `permissionMode`, `sourceToolAssistantUUID`, `attachment`, `snapshot`, `messageId`, `leafUuid`, `aiTitle`, `lastPrompt`, `hookInfos`, `durationMs`, `level`, … Unknown top-level keys must never cause a parse error (serde ignores unknown fields by default — do **not** add `#[serde(deny_unknown_fields)]`).

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

Block → label mapping is in §5. Note the `ToolResult` `content` can itself be an array containing `{type:"text",text}`, `{type:"image",…}`, and **`{type:"tool_reference", tool_name}`** (emitted by ToolSearch results); model it as raw `Value` and inspect as needed.

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

A `type:"user"` record is **NOT** always a human turn. `tool_result` blocks ride on `role:"user"` records — such a record is a tool-result *carrier*, not human input. Magnitudes are extreme: in one real session **177 genuine** users (167 string + 10 text-block) vs **8 675 tool_result-carriers** (98% of `user` records); another had 332+61 genuine vs 1 619 carriers. Getting this wrong corrupts both the `user` label and turn-delimiting. Two further `role:"user"` shapes that *look* human but are NOT — and now leave the `user` role under the §5 taxonomy — are an inbound `<teammate-message>`/`<agent-message>` (→ `agent.communication.*`, the GOLD §1 fix) and a `<task-notification>` automation pulse (→ `harness.notification.*`).

**`is_genuine_user(record)` is true iff ALL of:**
1. `type == "user"` AND `message.role == "user"`; AND
2. `isCompactSummary` is falsey (excludes compaction summaries, §4.7); AND
3. **`isMeta` is falsey** (excludes system-injected pseudo-turns, §4.2); AND
4. content is a **string**, OR content is a block array containing a `text` block and **no** `tool_result` block.

(`text` and `tool_result` never co-occur in one user record in real data, so "has a `text` block" is a clean genuine signal once carriers and meta are excluded.) `src/model.rs::is_genuine_user` implements (1)–(4); it **additionally** excludes the machine-synthesized markers of §4.2.1–.3 (interrupt strings, `<local-command-stdout>…`, `<command-name>…`) AND an inbound `<teammate-message>`/`<agent-message>` (`is_peer_message`, the GOLD §1 + FINDING-2 fix — a peer-agent message is never the operator; detected ONLY at a SECTION BOUNDARY per FINDING-1, so a tag merely QUOTED mid-prose stays the human) so none of them start a turn AS the human (codepoint-safe via `trim_end`/`ends_with`/exact `==`/`starts_with`, never a byte-offset slice).

**Boundary predicate `opens_turn(record)` — the single turn delimiter (§6.4).** Turn-delimiting keys on `opens_turn`, NOT on `is_genuine_user` alone. A turn opens on ANY of:
1. `is_genuine_user` (above); OR
2. an **answered AskUserQuestion** carrier (§4.4) — the answer IS the user's message; OR
3. a **tool-use rejection carrying a typed user message** (§4.2.4) — the typed instruction IS the user's message; OR
4. an **inbound peer message** (`is_peer_message_record` — a `<teammate-message>` OR `<agent-message>` from another session, FINDING-2) — a real delivered message, so it stays a turn-opener even though `is_genuine_user` excludes it (segmentation/turn-COUNT stays byte-stable where peers already opened turns; only the §5 label changes, to `agent.communication.*`).

Cases 2–3 are genuine user messages, so each opens a turn (§6.4). **Turn-opening is a SEPARATE axis from the §5 label:** `opens_turn` segments the transcript, `Record::classify` assigns the searchable `role.class.sub` label(s). The reconstruction (`reconstructed_user_text`) renders the opener body for each case: the plain genuine text (`user.message`), the full AUQ Q+options+answer unit (`user.answer`), the rejection instruction + a `[plan: <path>]` pointer (`user.rejection`), or the teammate-message body (`agent.communication.inbox`).

### 4.2 `isMeta:true` pseudo-turns + synthesized markers — TRAP, must be excluded

`isMeta:true` `user` records have string/text content that *looks* human but is **system-injected**, e.g. `"Continue from where you left off."`, `"# Autonomous loop tick"`, `"Stop hook feedback: …"`, `"<local-command-caveat>…"`, `"[Image: source: …]"`. These are **not** genuine human input and must be excluded from `user.message` **and** from turn-delimiting. The `isMeta` exclusion is load-bearing. Under the §5 taxonomy these are not dropped from search — `classify` reparents the recognized ones to `harness.schedule.{wakeup,continuation}` (a fired `ScheduleWakeup`/autonomous-loop timer tick / a `Continue from where you left off.` resume) or `harness.meta.{hook,loop}` (stop-hook / `<local-command-caveat>` / edit-retry feedback; an autonomous-loop driver tick). An `isMeta` record that matches NO harness marker (a novel hook wrapper, the `[Image: source:…]` reference) classifies **empty** (excluded), never `user.message` (the M2b root-fix: a genuine `user.message` is never `isMeta`).

**Beyond `isMeta`, four NON-`isMeta` user-record shapes are also machine-synthesized and must NOT open a turn** (verified on real `~/.claude/projects` data; corpus counts in parentheses):

#### 4.2.1 Interrupt markers (116 + 21)
A `type:"user"` `text`-block (or bare-string) record whose content is **EXACTLY** `"[Request interrupted by user]"` or `"[Request interrupted by user for tool use]"` — a CC-synthesized interrupt marker, not a human turn. None carry extra prose (dropping them as boundaries loses zero user content). Match is **exact `==`** (a real message that merely *quotes* the phrase stays genuine).

#### 4.2.2 `<local-command-stdout>…` (52)
String content (NOT `isMeta`) starting with `<local-command-stdout>` — local-command OUTPUT (machine), not the user's prose. (Its sibling `<local-command-caveat>` carries `isMeta` and classifies `harness.meta.hook`.) Does NOT open a turn (excluded via `starts_with`); classifies **`harness.command.stdout`** (§5).

#### 4.2.3 slash-command wrapper — BOTH tag orders (`<command-name>…` / `<command-message>…`)
The machine-templated EXPANSION of a slash command. **Current CC emits `<command-message>` FIRST, then `<command-name>`** (verified in real corpora: 14 new-order sessions vs 35 older `<command-name>`-first ones), so detection is `is_slash_command_wrapper` — string content (NOT `isMeta`) whose LEADING tag is EITHER `<command-name>` OR `<command-message>` (the new `COMMAND_MESSAGE_PREFIX`). **`is_genuine_user` excludes BOTH orders** (a new-order wrapper no longer masquerades as human prose or opens a turn — a v0.5 correctness fix; turn NUMBERING may shift on a transcript carrying new-order wrappers). The templated wrapper does NOT open a turn. Classification is multi-label: any genuine prose the user typed after the command lives in `<command-args>…</command-args>` and is recovered via `slash_command_name` as `/name args`, so a WITH-ARGS wrapper classifies **`[user.message, harness.command.invocation]`** with `user.message` FIRST (the richest-view law, §5.2) — the UNFILTERED render is the extracted `/name args`, never the wrapper XML; a NO-ARGS wrapper classifies **`harness.command.invocation`** ONLY. An explicit `-t harness.command.invocation` still renders the raw wrapper. (No new Matcher synth needle is needed — the name + args are verbatim raw substrings and the rendered seam is a space, so a whitespace-bearing pattern is never prefilter-eligible.)

#### 4.2.4 Tool-use rejection — `is_error:true` tool_result (31 with-message / 36 without)
When the user REJECTS a tool use (ExitPlanMode plan kick-back, or a rejected AskUserQuestion / Edit / …), the result carrier is `is_error:true` with content beginning `"The user doesn't want to proceed with this tool use…"`. TWO sub-shapes:
- **With a typed message** — ends `"To tell you how to proceed, the user said:\n<TEXT>"`. `<TEXT>` IS a genuine user message → **opens a turn** (`plan_rejection_message` extracts the tail codepoint-safely via `split_once`) and classifies **`user.rejection`** (dual-labeled with `agent.tool.result` — it rides on the rejecting tool_result carrier; deduped to the richer `user.rejection`, §5). Both rejected tools share the marker (ExitPlanMode AND an AskUserQuestion-clarify reject). For an ExitPlanMode rejection, the rejected `tool_use_id` resolves through a `PlanIndex` (`tool_use_id → planFilePath`) to append a `[plan: <path>]` pointer so a consuming LLM can Read the plan (ExitPlanMode-only; an AUQ-clarify reject has no plan path).
- **Without a typed message** — ends `"STOP what you are doing and wait for the user to tell you how to proceed."`. The user clicked reject but typed nothing → **NOT a boundary** (→ only `agent.tool.result`, no `user.rejection`).

The plan **APPROVAL** carrier (`"User has approved your plan…"`, no `is_error`, no typed message) is the harness greenlight, NOT a user message → never a boundary.

### 4.3 `tool_use` (assistant calling a tool)

Assistant `tool_use` blocks → `agent.tool.use` (§5). `name` identifies the tool; `input` is an arbitrary per-tool JSON object. `AskUserQuestion` is a `tool_use` (§4.4). A `caller` field (`{type:"direct"}`) may be present. A `SendMessage` tool_use ALSO carries `agent.communication.{sent,signal}` and a `Task`/`Agent`/`Workflow` spawn ALSO carries `agent.communication.sent` (the dual is deduped to the comm view, §5).

### 4.4 AskUserQuestion

A `tool_use` block with `name == "AskUserQuestion"`. Input shape:
```
input.questions: [ { question, header, multiSelect: bool,
                     options: [ { label, description } ] } ]
```
**HARD-WON (verified across all 51 sessions, 0 counterexamples at the time; RE-MEASURED at CC 2.1.258 for v0.10.1):** a **pending/unanswered single-question** AUQ is **never** flushed to jsonl. Only answered AUQs appear. **The multi-question shape (`input.questions` length >= 2) is the exception since at least 2.1.258:** it is written at question time as an unreturned `tool_use` (measured live on macOS and Windows with the dialog on screen; three single-question trials in the same sessions stayed unwritten), so the `status`/`wait` tail reads it as `waiting-hitl` without the sidecar, while the single-question shape still needs the sidecar (section 6.13). The answer arrives as a `tool_result` whose `content` is a synthesized string:
```
User has answered your questions: "<question>"="<chosen label>". You can now continue…
```
and the carrier's top-level `toolUseResult` echoes the full `questions[]` structure + a **structured `answers{question → answer}` map** (the CLEAN source — the inline synthesized string is noisy/truncated). **Implication:** from the NATIVE transcript alone csift cannot render a "pending question" state — an unanswered AUQ is off-disk, and if an AUQ `tool_use` is on disk its answer is too. The unanswered window is recovered ONLY from the hook-written elicitation sidecar, which csift merges transparently (§6.10).

**The COMPLETE user message = QUESTION + OPTIONS + ANSWER as one unit.** The user does not click an option — she answers in prose (often a counter-question or a scope-expanding decision), so the answer is her selection *plus* her reasoning. `auq_exchange()` reconstructs the whole exchange as one genuine-user unit: `[AskUserQuestion · N questions]` then, per question, `Qn (header): question  options: a | b | …` and `An: <answer prose>`. It is built from the structured `toolUseResult.questions[]` zipped with `toolUseResult.answers{}` (clean), falling back to parsing the synthesized `"<q>"="<a>"` string only when `toolUseResult` is absent. Codepoint-safe (no byte-offset slice into a CJK question/answer).

**The answer is a genuine-user TURN BOUNDARY** (`is_auq_answer_boundary` — true iff a non-errored `tool_result` carrier with a non-empty `toolUseResult.answers` OR the synthesized marker; a CANCELLED/rejected/validation-errored AUQ has `is_error:true` and no `answers` → NOT a boundary). An AUQ answer (e.g. one that expanded the session scope) is a genuine-user message, so it opens its own turn (§6.4) and is surfaced on every surface (verbatim/list/search/recover/budget/topology). The answer classifies **`user.answer`** (dual-labeled with `agent.tool.result`; deduped to the richer `user.answer`, §5).

### 4.5 `tool_result` (tool's response) → `agent.tool.result`

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

Most `system` records carry no §5 label and are skippable noise; the one EXCEPTION is `compact_boundary`, which the classifier labels **`harness.compaction.boundary`** (`Record::classify`, engine-verified). **`search`-surfaced (D7):** `search`'s §7 stage-1 transcript-candidate prefilter — which keeps `"role":"user"`/`"role":"assistant"` lines for the 200 MB-file performance contract — ALSO keeps the rare `compact_boundary` record — but that keep is **`-t`-GATED** (a `needs_compact_boundary` bool, `label_selected(&args.categories, "harness.compaction.boundary")`, derived once and `&&`-gating the extra `memmem`): no-`-t` or a selector reaching `harness.compaction.boundary` keeps it; a `-t user`/`-t agent.*` query pays ZERO (the `memmem` is never reached). The role keeps ALWAYS run (conservative superset); when the boundary `memmem` runs it only touches lines that already failed both role checks, and boundaries are rare — so the perf contract holds. So `search -t harness.compaction.boundary` surfaces it: for a message-less system record `record_raw_text` renders the boundary's top-level `content` plus a readable `compactMetadata` excerpt (`[compaction boundary: trigger=… preTokens=… postTokens=… durationMs=…]`) as the match + excerpt, so compaction points can be enumerated and inspected; `verbatim` still also renders the boundary as its own banner. The compaction SUMMARY (item 1 above) is a `type:"user"` record, so it classifies **`harness.compaction.summary`** AND is surfaced by `search`. All other `system` subtypes still parse cleanly, carry no label, and must never crash time logic.

### 4.8 `attachment` & other event records — skippable noise

`attachment` is the **dominant** record type (54% of all records in the largest file — mostly `{attachment:{type:"hook_success",…}}` SessionStart spam). ~21 attachment subtypes exist (`hook_success`, `hook_additional_context`, `task_reminder`, `date_change`, `plan_mode*`, `deferred_tools_delta`, `ultrathink_effort`, `compact_file_reference`, `edited_text_file`, `queued_command`, `skill_listing`, `nested_memory`, `selected_lines_in_ide`, `hook_cancelled`/`hook_blocking_error`, `file`, …). **None carry a §5 label** — they are dropped pre-JSON by the prefilter (§7) and absorbed by the tolerant model (`classify` returns empty for them). `file-history-snapshot` nests its timestamp inside `snapshot` (not top-level). `queue-operation` has a top-level `timestamp` + `operation ∈ {enqueue,dequeue,remove,popAll}`. Metadata records (`last-prompt`/`ai-title`/`agent-name`/`mode`/`permission-mode`) have **no `timestamp`/`uuid`** — skip in time/turn logic.

### 4.9 The Bash shell cwd + file freshness (verified against CC 2.1.237; full-corpus validated)

The mechanisms `files`/`recover` build on. Extracted from the Claude Code binary and
validated against real transcripts (18,185 commands for the cwd tracker; 577 hint
attributions); the crate's implementation notes live at `bash_mutations::cwd` and
`recover::carriers`.

**The shell cwd.** Claude Code keeps NO long-lived shell: every Bash call spawns a
fresh shell at a TRACKED cwd, and that tracked value is stamped on every jsonl record
as the top-level `cwd` field (subagent transcripts included; stable across
compaction). The tracker updates via a `&& pwd -P >| <tmp>/claude-<id>-cwd` read-back
appended to each command — so a command that exits non-zero, and any backgrounded
command, never advances it. A `cd` into a project SUBDIRECTORY persists silently into
later calls (the record `cwd` follows); a reset to the original directory fires only
when the shell ends OUTSIDE originalCwd + the `/add-dir` set, printing "Shell cwd was
reset to <path>" into the tool result. `CLAUDE_BASH_MAINTAIN_PROJECT_WORKING_DIR`
truthy = reset every call, silently. **ANCHOR LAW: read `cwd` off the TOOL_USE
record, never the result record** (the post-command update is asynchronous; ~0.24% of
result records lag). Consequence: no cross-command state machine is needed — each
record carries its own spawn cwd, and only INTRA-command `cd`s need lexical tracking
(`bash_mutations::cwd`; measured 99.967% ending-cwd accuracy, 99.65% agreement with
CC's own modified-file hints). Corpus shape of mutation targets: 19.75% typed
absolute, 62.79% relative (84.21% of those resolve at the zero-inference `cwd-joined`
class), 17.47% dynamic (`$VAR`/glob — never resolved).

**File freshness (readFileState).** Write/Edit gate on an mtime check against the
last Read, with a CONTENT-HASH absolution available only to FULL reads (a windowed
read has no absolution channel: mtime moved ⇒ rejected). Byte-identical rewrites (an
idempotent formatter) never trip the gate. Bash NEVER invalidates readFileState —
that is exactly how staleness arises — but a bash `cat <file>` SEEDS read-state (CC
lexically recognizes read-shaped commands), so a Write can legally follow one.
Rejections always land as `is_error:true` tool_results (the `behavior:'ask'` field is
inert in the runner; corpus 25/25). Three signals ride the stream and are consumed by
`recover` (v0.8.0):
- `toolUseResult.staleReadFileStateHint` (Bash results): CC stats its whole read set
  after a WRITE_COMMAND_MARKERS-shaped command and NAMES the modified files —
  `[This command modified N file(s) you've previously read: p1, p2 and M more. Call
  Read before editing.]`, paths relative to the shell cwd, list capped at five.
- `toolUseResult.staleRecovered:true` (successful Edit results): disk had drifted
  since the last read, but `old_string` stayed unique and the edit applied; corpus
  carries roughly twice as many of these as hard rejections.
- the `edited_text_file` attachment: idiom-independent external-change capture,
  gated on read-set membership + full-view read + one shot per read-epoch, with a
  16KB budget that degrades to an EMPTY snippet when exceeded.
A change with none of these signals (read-first then changed with no later
Write/Edit attempt) leaves NO transcript trace — absence of a signal is never
evidence of absence, which is why recover's window accounting (§6.7) exists.

---

## 5. Label taxonomy — `role.class.sub` (`search -t/--category`, repeatable)

The `-t` axis is a 3-role, **34-leaf** dotted taxonomy (`model::Class`; `Class::ALL` is the single source of truth, drift-guard-tested at 33). `Record::classify(ctx) -> Vec<Class>` is the engine: **multi-label** (one physical record can carry >1 leaf), pure, tolerant (an unmodeled record → empty `Vec`, never a crash). A selector matches a record iff some selector path is a **dot-SEGMENT prefix** of some label path (so `-t agent` covers the whole role, `-t agent.tool` covers use+result), and the rendered/JSON label is always the full dotted leaf path.

### 5.1 The tree (role → class → sub)
```
user                                       (the human)
├─ user.message            genuine prose (string or text-only blocks; incl. a slash-command's args, rendered `/name args` — §4.2.3)
├─ user.answer             AUQ answer (the Q+options+answer unit; INCLUDES the question)   ALSO agent.tool.result
├─ user.rejection          plan/tool reject + typed instruction (render [plan:<path>] for ExitPlanMode)  ALSO agent.tool.result
├─ user.unsent             a SUPERSEDED turn-opener draft (v0.9.2): sent, esc-recalled into the input box, edited, re-sent — the original stays on disk sharing the resend's parentUuid. Assigned at the SCAN layer (the set needs the LATER sibling; C-18's `superseded_draft_indices` is the one rule), SINGLE label (user.message stays pure), outside turn numbering (header `<tok>·draft`, JSON null turn_index), suppressed under a `--turn` window, always address-reachable. 99% never drew a reply (measured n=317). LIMITS: an abandoned recall (no resend) is structurally undetectable; a queued text edited before dispatch never becomes a user record (its `queue-operation` line is `user.queued`, v0.10.0).
└─ user.queued             the human's text as it sat in the input QUEUE (v0.10.0): a `queue-operation` line carrying `content` — `enqueue` (typed while a turn ran), `popAll` (recalled to the input box), `remove` (consumed; `reason` ∈ {absorbed_mid_turn, delivered_to_agent}, measured the only two values). A content-less `dequeue` carries no label; a queued `<task-notification>` / peer message is a harness RIDER, not the human, and carries no label (the delivered twin already classifies harness.notification.* / agent.communication.inbox). No join key exists on the queue side (measured 4-6 keys, no promptId/uuid) so `dispatched` is NEVER asserted; the hit carries `operation` + `reason` verbatim (label zone `[enqueue]` / `[remove · absorbed_mid_turn]`, JSON `queue_operation`/`queue_reason`). GATED: parsed only under an explicit selector reaching the leaf (§7 gated-leaf law). LLM-invisible (no `message{}`).

agent                                      (the assistant)
├─ agent.message           visible end-of-turn text (assistant `text` block)
├─ agent.thinking          thinking block (incl. `redacted_thinking` → here, renders `[redacted thinking]`)
├─ agent.thinking.narration  narration-TAGGED thinking block (v0.9.2): an API-issued one-sentence summary of the reasoning beside it, in the user's language. The tag hides in the base64 `signature` (protobuf field path 2→1→8, LAST length-delimited field at each level; decoded whole-string, every failure → plain `agent.thinking`; open tag set). Renders the `[narration summary]` LABEL-zone marker (display-only); sibling cap 1; excluded from `verbatim`. The first leaf whose path another leaf prefixes: `-t agent.thinking` selects both, pure reasoning = `-t agent.thinking -T agent.thinking.narration`.
├─ agent.tool.use          tool_use block (incl. a pending elicitation sidecar marker — AUQ/ExitPlanMode/MCP)
├─ agent.tool.result       tool_result block (incl. errored)
└─ agent.communication                     (EVERY comm hit renders  from ⇨ to)
   ├─ inbox                received prose (a peer/teammate message, a subagent spawn prompt, a subagent return)
   ├─ sent                 sent prose (`SendMessage` type message; a Task/Agent/Workflow spawn)
   └─ signal               control/status (idle_notification, shutdown_request/approved, teammate_terminated)

harness                                    (Claude Code machinery — render glyph ⚙)
├─ harness.notification.{workflow, monitor, subagent, background-command, task}   <task-notification> completion pulses
├─ harness.compaction.{summary, boundary}        the isCompactSummary record + the system compact_boundary
├─ harness.command.{invocation, stdout}          <command-name> slash wrapper + <local-command-stdout>
├─ harness.interrupt.{user, tool}                [Request interrupted by user] / …for tool use]
├─ harness.schedule.{wakeup, continuation}       fired ScheduleWakeup/autonomous-loop timer tick / "Continue from where you left off."
├─ harness.meta.{hook, loop}                     hook feedback (stop-hook / <local-command-caveat> / edit-retry) + autonomous-loop driver tick
├─ harness.meta.attachment                       any OTHER attachment payload (v0.8.1; scanned only under --attachments / --count-by attachment / an explicit show address; text = the VERBATIM payload JSON)
├─ harness.meta.turn-duration                    (v0.10.0, GATED, LLM-invisible) the `system`/`turn_duration` end-of-turn telemetry record: renders `[turn duration: 1m 5s · durationMs=… messageCount=… pendingBackgroundAgentCount=… pendingWorkflowCount=…]` (present fields only; the human form rounds to the second, tops out at `Nd Nh`) — the structured body behind the REPL's "Done in Ns" / "Waiting for N agents" lines, which never land on disk. Fires per assistant turn (measured 4,580 vs ~40k user records: not every turn). `pendingBackgroundAgentCount` since CC 2.1.159 (~3% of records), `pendingWorkflowCount` since 2.1.156 (~5%). The pending counts cover background agents and workflows ONLY — a turn that ended with a background shell still running wrote no pending field at all (measured on a live specimen: the REPL's "1 shell still running" had no on-disk counterpart), so an EOT record is never evidence that the session is done; that question belongs to `status`.
├─ harness.meta.away-summary                     (v0.10.0, GATED, LLM-invisible) the `system`/`away_summary` recap: MODEL-GENERATED (a side `generate()` call, config-gated) when the operator returns after a 5+ minute blur; text = the verbatim `content`. Measured 1,196/1,198 never re-appear downstream — "not in the surviving conversation".
├─ harness.meta.stop-hooks                       (v0.10.0, GATED, LLM-invisible) the `system`/`stop_hook_summary` Stop-hook execution ledger: renders `[stop hooks: count=N errors=M prevented=<bool>]` + one `<command> (<ms>ms)` line per `hookInfos[]` entry (the commands are the record's only text; `hookAdditionalContext` was EMPTY on 4,865/4,865 measured instances). Distinct from `harness.meta.hook` (text a hook INJECTED).
├─ harness.meta.snapshot                         (v0.10.0, GATED, LLM-invisible) a `file-history-snapshot` (renders `[file-history snapshot at <snapshot.timestamp>: <path>@vN, …]`, every tracked path sorted — the line has NO top-level timestamp, so the hit's ts is null and the excerpt carries the nested one) or a `file-history-delta` (`[file-history delta at <ts>: <trackingPath>@vN backup=<backupFileName>]`, one path's bump; 1,934 corpus lines that nothing read before). The v0.9.4 recover instrument, now searchable by path and version.
└─ harness.meta.system                           (v0.10.1, GATED, LLM-invisible) every OTHER `type:"system"` subtype the harness writes for its own UI and never sends to the model: `informational` (the Remote Control disconnect warning after an account switch, `level:"warning"`), `api_error`, `model_refusal_fallback` / `model_refusal_no_fallback`, `agents_killed`, `local_command`, `scheduled_task_fire`, and any subtype a later build adds; renders `[<subtype> <level>] <content>` (a string content verbatim, a JSON content compact, the bare head when there is none); the compaction boundary keeps its own leaf. Admitted by the key-only `"subtype"` needle under an explicit selector, synth-marked on the same needle.
```
Notes: `harness.notification.subagent` = the `AutomationKind::Agent` pulse (renamed so it never collides with the `agent` role) AND, since v0.10.0, the harness's plain-text agents-stopped notice (`N background agents were stopped by the user: …` / `Background agent "…" was stopped by the user.` - excluded from `is_genuine_user`, never a turn opener, rendered `[subagent stopped] <notice>`). `harness.notification.monitor` = the Monitor tool's own pulses and termination notices ONLY (summary opens `Monitor`/`Scheduled`/`cron`); a `Background command "…"` pulse is always `.background-command` whatever its quoted name says (the v0.10.0 retirement of the quoted-name heuristic). An `isMeta` record matching no harness marker classifies **empty** (excluded), never `user.message`. `system` records other than `compact_boundary` / `turn_duration` / `away_summary` / `stop_hook_summary` (the unpromoted subtypes: scheduled_task_fire, api_error, model_refusal_fallback, local_command, agents_killed, …), plus the session-state cache records (`last-prompt`/`ai-title`/`agent-name`/`mode`/`permission-mode` — no uuid, no timestamp, no message; `lastPrompt` duplicates `user.message`) and the other bookkeeping types (`atis-latch`, `bridge-session`, `cost-state`, `fork-context-ref`), carry no label; `queue-operation` (with content) and `file-history-snapshot`/`-delta` carry the v0.10.0 leaves above; an `attachment` record classifies `harness.meta.hook` (a hook-context payload) or `harness.meta.attachment` (anything else), both reachable only through their gates (v0.8.1). `harness.compaction.boundary` IS a valid classify label and, since D7, IS surfaced by `search` (the §7 prefilter keeps the `compact_boundary` record; `record_raw_text` renders its content + `compactMetadata` — see §4.7); its sibling `harness.compaction.summary` is a `type:"user"` record and IS searchable.

### 5.2 Multi-label (one record → ≥1 leaf) + Q4 dedup
A physical record can carry several labels; `search` emits it **ONCE** under the **richest** view when a query selects ≥2 of them (Q4 dedup), while JSON `labels[]` still lists the full set.

| record (disk shape) | label A | label B | direction ⇨ | richest (dedup) |
|---|---|---|---|---|
| AUQ answer carrier (non-errored tool_result + answers) | `user.answer` | `agent.tool.result` | — | `user.answer` |
| plan/tool reject w/ typed msg (is_error tool_result + marker) | `user.rejection` | `agent.tool.result` | — | `user.rejection` |
| slash-command wrapper WITH args (either tag order) | `user.message` | `harness.command.invocation` | — | `user.message` (renders `/name args`) |
| `SendMessage` tool_use, `input.type=="message"` | `agent.tool.use` | `agent.communication.sent` | self ⇨ to | `…sent` |
| `SendMessage` tool_use, `input.type=="shutdown_request"` | `agent.tool.use` | `agent.communication.signal` | self ⇨ to | `…signal` |
| `Task`/`Agent`/`Workflow` spawn tool_use | `agent.tool.use` | `agent.communication.sent` | self ⇨ child | `…sent` |
| inbound `<teammate-message>`/`<agent-message>` prose | `agent.communication.inbox` | — | from ⇨ self | `…inbox` |
| inbound peer control payload (`{"type":sig}`) | `agent.communication.signal` | — | from ⇨ self | `…signal` |
| subagent transcript opener (the delivered spawn prompt) | `agent.communication.inbox` | — | parent ⇨ self | `…inbox` |
| subagent return (one-shot Task tool_result) | `agent.tool.result` | `agent.communication.inbox` | child ⇨ self | `…inbox` |
| `harness.notification.{subagent,task,workflow}` carrying a `<result>` (bg-agent report, G1) | `harness.notification.*` | `agent.communication.inbox` | child ⇨ self | (per-section: both emit) |

A batched record (≥1 `<task-notification>` and/or ≥1 inbound peer section concatenated in ONE `type:"user"` record) classifies **PER SECTION** (the labels are the UNION), and `search` renders **one hit per section** (G4/G5); a peer tag merely QUOTED inside a notification span is masked by the notification (precedence). A spawn **launch-ack** (`teammate_spawned` / `Async agent launched successfully…`) is `agent.tool.result` ONLY — it is the launch confirmation, not the child's return.

### 5.3 Direction (`from ⇨ to`) sources (GOLD §4)
Every `agent.communication.*` hit renders a direction (`Record::direction`); the transcript owner's own id renders as the literal `self`.
- `<teammate-message>`/`<agent-message>`: the `teammate_id`/`from` attribute = FROM; owner = TO (`from ⇨ self`).
- `SendMessage`: FROM = self; `input.to`/`recipient` = TO (`self ⇨ to`).
- spawn tool_use: FROM = self; TO = the spawned child (id-join on the spawn `tool_use_id`, else the name-join, else the raw spawn name / `?`).
- subagent opener: `parent ⇨ self`. subagent return / `<result>` pulse: `child ⇨ self` (child via the embedded `<tool-use-id>`).

### 5.4 Render markers + selector grammar
- **Role glyph** leads every hit line: **`◂` user · `▸` agent · `⚙` harness** (gear = machinery). `agent.communication.*` is agent-side, so `▸`.
- **`⇨`** (hollow arrow) = the comm direction `<from> ⇨ <to>`. **`▹`** = ONLY the `agent.tool.use ▹ agent.tool.result` pairing (joined by `tool_use_id`; an unreturned use → `agent.tool.use (no result — pending)`, an orphan result → `agent.tool.result (use not in scope)`).
- **Selector grammar**: a dotted `role.class.sub` path, dot-segment-prefix matched; with no `-t`, EVERY label is eligible (search everything except no-label records). The **old flat values** `thinking`/`tool`/`tool-response` HARD-error (0 back-compat, like `--full`), the error LISTING the valid selectors (derived from `Class::ALL`); `user`/`agent` keep working as ROLE selectors.

---

## 6. Subcommand specifications

> **v0.10.4 CHANGE LEDGER (non-breaking; the live-file reader -- `csift 0.10.4`).**
> ① `wait` mapped the whole transcript on every poll and `tail_shape` mapped it for its 512 KB window. A memory map of a file another process truncates faults with SIGBUS on the first page touched past the new end (measured: a 1 MB map, a truncation to 4 KB by a second writer, one read past the end - signal 10 on macOS, 7 on Linux), and the ledger's MISC-020 trace shows Claude Code rewriting a transcript in place on two paths (a rewind tombstone: truncate plus tail rewrite, ungated since at least 2.1.191; the local GC compaction rewrite, gated). Both repeated readers now use plain positional reads (`parse::read_range`, `parse::read_tail`); a shrink returns fewer bytes. `wait` detects a file shorter than its baseline between two polls, moves the baseline to the new end and reports `transcript shrank N time(s) (lane: -bytes): rewritten in place, baseline moved` in its activity line and JSON `activity.shrinks`; 0.10.0-0.10.3 skipped a shrunk file silently. The one-shot full scans keep the section 7a map (their sub-second life bounds the exposure; the performance contract rests on it), and section 7a now states the hazard as it is.
>
> **v0.10.3 CHANGE LEDGER (non-breaking; the attribution push -- `csift 0.10.3`).**
> ① The freeform AskUserQuestion answer. The 2.1.258 binary synthesizes the answering `tool_result` from `{questions, answers, response?, annotations?, afkTimeoutMs?}` in this order: an idle timeout (`afkTimeoutMs`) renders `No response after ...` (plus `Before going idle the user had selected: ...` when partial answers exist); a non-blank `response` renders `The user responded: <text>`; a non-empty `answers` renders `Your questions have been answered: ...` (every question answered by a listed label, no notes) or `The user answered: ...`; nothing renders `The user did not answer the questions.`. The TUI dialog writes only `answers`/`annotations`/`afkTimeoutMs` (a typed "Other" lands in `answers[question]`; "Chat about this" is a rejection), so a `response`-only carrier comes from another answerer. csift 0.10.0-0.10.2 required a non-empty `answers` and lacked the prefix, so that carrier classified `agent.tool.result` and opened no turn. Now `Record::has_auq_response` (present and non-blank) is a second structured signal beside `has_auq_answers`; `auq_exchange` renders the questions asked and a `response:` line; `AUQ_ANSWER_MARKERS` and the verifiable synth needles gain `The user responded:`. The idle-timeout text is not an answer and never opens a turn.
> ③ Four defects the ledger tracing found in csift, each fixed with its claim: (a) `BgState::from_status` mapped every completion status outside failed/killed/stopped to completed; the harness's remote-agent notifier writes a fifth value, `blocked`, so `BgState` gains `Blocked` and `Other` (an unknown literal, rendered `N with an unknown status`, JSON `blocked`/`other`) - BG-011. (b) `Cond::Notification` matched the main transcript only; csift's own classifier finds 2 of 2906 delivered pulse records in subagent lanes at 2.1.258 (a pulse addressed to the owning agent lands in its lane), so `record_matches` lost its main-only scope and `wait --until notification` fires in every watched lane - BG-015. (c) `push_read_event` never read `truncatedByTokenCap`; on the character-truncation branch `numLines` equals `totalLines`, so a capped read replayed as a full snapshot (1 of 180 flagged corpus reads) - now never full, the cut last line dropped, no anchor when nothing whole remains - REC-047. (d) `record_images` read user records and tool results only; a `queued_command` attachment's `prompt[]` is the third image carrier (52 blocks in 11 transcripts, 45 with no other copy) and is now walked with the same marker zip - IMG-002.
> ② The ledger schema gains the two attribution legs (`producer_trace`, `specimen`), the five-value `attribution` derived from them (`partial-producer` is new: a template or field found in the binary without its trigger and gate), and `open_legs`; a traced claim carries its `producer_chain` (the hops with byte offsets and verbatim excerpts), a negative its `enumeration` (every site of the function or field in the binary, each read with a verdict: the closure of a negative is the enumeration, a bounded sample leaves it partial), and a non-gap note for a later audit its `residue`. A claim whose text a traced hop refuted was rewritten (verdict `refined` or `drifted`), never annotated around. Version floors (`first_seen_claude_code`) were checked against historical native builds fetched by version from the distribution bucket the installer names and bisected on distinctive string literals; a traced claim also records its writer's `writer_literal_floor` (the lowest build carrying the writer literal and the highest lacking it, from a ladder of every eighth build plus the cached bisection builds), a lead for the next audit and never a substitute for the floor, since a literal dates itself and not the behavior. The gate (rules 7 and 8) checks the derivation, refuses `holds` on a by-elimination claim, refuses an end-to-end claim with a non-empty `open_legs`, and requires the README tally table (between the `ledger-tally` markers) to equal the ledger's counts. AGENTS section 7.1 carries the schema, the three-hop definition of a complete producer trace and the closure rule.
>
> **v0.10.2 CHANGE LEDGER (non-breaking; the ledger's first re-read after 0.10.1 -- `csift 0.10.2`).**
> ① `wait --until notification` reads the three pulse carriers (section 6.14): the idle-delivery user record, the `queue-operation` enqueue line and the `queued_command` attachment; a pulse absorbed mid-turn exists only on the latter two (3218 pulse-bearing user records against 5893 enqueue lines in the reference corpus), so 0.10.0-0.10.1 could never fire on roughly every other completion. The wait activity census counts the same deliveries; a queue remove/dequeue counts nothing.
> ② `plan` slug-only binding: the project root is the transcript's FIRST recorded `cwd` (section 6.9). The harness memoizes its plans directory at first access through the cwd getter that also stamps records; 0.10.1 took the slug record's own `cwd`, which follows the tracked shell cwd, so a `cd` before the first slug record joined the plan under a subdirectory and dropped a project-scope `plansDirectory`.
> ③ A `/fork` clone (line 1 `fork-context-ref`) has NO spawn-prompt seed: its first turn-opener is the parent's own human message and classifies `user.message` (42 clones in the reference corpus, 10 with such a record); a genuine child keeps the parent-to-self seed.
> ④ The global spawn index folds FIRST-wins (main, then the subs in discovery order): a clone's copy of a sibling's spawn record never re-parents that sibling.
> ⑤ A superseded draft keeps its single `user.unsent` view whatever its text shape; a hit's `label` is always a member of its own `labels[]`.
> ⑥ Two string literals lost their line continuations (the `wait` timeout-guard message, a `recover` bash-append boundary detail); `show --turn` help names the gated lines a turn fetch omits; `search --help` names the eight invisible leaves. The ledger gate gains rule 6 (every claim cites a code site); AGENTS.md 7.1 carries the ledger schema and procedures.

> **v0.10.1 CHANGE LEDGER (non-breaking; the live layer verified on a REAL Windows session, the registry status law, two HITL instruments, and the introspection ledger gate -- `csift 0.10.1`).**
>
> ① **The registry `shell` status was a running shape in 0.9.0-0.10.0; it is the idle-with-a-background-shell shape.** Extracted from the 2.1.258 binary: the row's status is computed as `waiting` when a dialog blocks the session, else `busy` while loading or delegating, else `idle`, and the `idle` result is relabeled `shell` while a `local_bash` background task is open. Measured on the Windows session: at end of turn with the dev server running the row read `shell`, csift reported `running`, and `wait --until stop` timed out even under `--ignore-background`. Now `busy` is the only registry running signal, `shell` makes the tail idle-shaped (the seventh verdict when the background scan counts the task, `idle-eot` with a note when the lens excludes it), and the same session reads `idle-background-open` / `idle-eot` / `fired stop` in 0s.
> ② **Windows pid probe.** `procStart` in a Windows registry row is a FILETIME tick count (100ns since 1601), not the asctime string unix writes; `probe_pid` now parses both, reads the owner's creation time through PowerShell `Get-Process -Id N` + `StartTime.ToFileTimeUtc()` (exit 3 = no such process; `NOSTART` = alive, guard skipped), falls back to `tasklist` for liveness alone, and returns `Unavailable` only when neither tool exists. `pidDomain` (`darwin` / `linux` / `win32:<host>`) is compared with the local domain first: a foreign row is `ForeignDomain`, never probed, and the verdict names it. Verified live: start-time guard matched on the running session, `stale-dead` after `taskkill /F` on Windows and after SIGKILL on macOS (a SIGHUP lets Claude Code delete its own row, an honest `idle-eot`). The reuse-guard e2e and the reaped-pid unit run on Windows now (`cmd /c exit`).
> ③ **Two HITL instruments beside the sidecar.** Registry `status:"waiting"` => `waiting-hitl` (the binary's `waitingFor` kinds: a queued elicitation, any top dialog including permission prompts and plan approvals, a worker request, a sandbox request); an unreturned `AskUserQuestion`/`ExitPlanMode` at the main tail => `waiting-hitl`, because a MULTI-question ask is written at question time since 2.1.258 while a single-question ask stays buffered (section 4; measured three single-question and two multi-question trials on macOS and Windows). The idle-verdict honesty note now names the registry status it saw. `verdict::assess_full` split into `collect_evidence` + `rank_verdict` (the complexity gate).
> ④ **A child lane whose completion pulse landed is settled.** `children_report` takes the set of agent ids the background scan saw returned; such a lane is `settled` whatever its tail says (a finished subagent used to read `generating` for up to 300s).
> ⑤ **Introspection ledger.** `INTROSPECTION.json` (repo root, schema 1): one claim per Claude Code behavior csift depends on, excavated from SPEC/AGENTS/SKILL/CHANGELOG/code comments/the development corpus and re-verified per claim against the 2.1.258 binary and the live corpus; `tests/cli/ledger.rs` is the gate (badge version == `verified_claude_code` == a check on every claim; claim-specific evidence; verbatim snippets present), run by name in the pre-commit hook. README gains the verified-Claude-Code badge and the mutation-score badge. AGENTS section 7 records the per-release audit as step (0).
> ⑥ Measured Windows record shapes (AGENTS section 3.9): both `Bash` and `PowerShell` tools in one session when an msys2 bash is merely on PATH; `PowerShell` honors `run_in_background` with the same result grammar; task output files under `%LOCALAPPDATA%\Temp\claude\<encoded>\<session>\tasks\`; a `claude -p` session writes a registry row (`entrypoint:"sdk-cli"`, `status:null`); rows also carry `pidDomain`, `bridgeSessionId`, an optional `tmux` locator and a sibling `<pid>.<hash>.key` file.
> ⑦ **The ledger's first catch: `plansDirectory` was resolved against the wrong root.** The v0.9.4 slug-only fallback joined a relative `plansDirectory` to the config home and took an absolute one verbatim; the binary resolves it against the PROJECT ROOT through the merged settings scopes and refuses a value that escapes that root (falling back to `<claude-home>/plans`). `plans_dir(project_root)` now takes the binding record's own `cwd`, reads user, project and project-local settings in that precedence, joins against the root and applies the lexical containment check (the harness's extra symlink walk is the one unmodeled leg). Section 6.9 corrected; the e2e pins the project join, the local-scope override and the escaping fallback.
> ⑧ **Second catch: forked lanes were their own parents (depth 65).** A `/fork` child's transcript is a clone of its parent's, so the spawning tool_use sits in the child's own file and the global spawn index attributed the spawn to the child itself; `assign_depths` then ran to its cycle cap and `agents` printed depth 65 for every forked lane (32 in the reference corpus). `node_for` now prefers the meta's `parentAgentId` (a field CC 2.1.25x writes and csift never read) and drops any parent link that names the node itself. Third catch, wording: the parent's record of a subagent return is a terse sign-off plus an APPENDED continuation footer (`agentId: <id> (use SendMessage …)`), never a harness truncation - `agents --help`, SKILL and the ledger corrected. Fourth, reach: the `toolUseResult` echo that gates recover's Bash READ anchors is absent from WORKFLOW lanes only (0 of 3867 results) while built-in Task/Agent and teammate lanes carry it 97-99% of the time, so the gate already reaches them - the "top-level only" comment was the stale part.
> ⑨ **The 34th leaf, `harness.meta.system`.** The catch-all for every `type:"system"` subtype not modeled elsewhere (`informational`, `api_error`, `model_refusal_fallback` / `model_refusal_no_fallback`, `agents_killed`, `local_command`, `scheduled_task_fire`, and any later addition): `Record::promoted_class` maps any system subtype other than the four modeled ones to it, the render is `[<subtype> <level>] <content>` (`level` is a new tolerant record field), the candidate needle is the key-only `"subtype"` under an explicit selector (`GATED_LEAVES` grows to six, the census note and the `show` miss text name it), and the same needle is a verifiable synth marker for the fabricated head. LLM-invisible by the family's instrument (no `message{}`). Observed live: the Remote Control disconnect warning (`level:"warning"`) written when the signed-in account changed in another session, and a content-less `agents_killed` record. The section 5 tree, SKILL, AGENTS and README move 33 -> 34.
> ⑩ **Ledger audit drifts fixed in code.** (a) `PEER_MESSAGE_PREAMBLES`: Claude Code 2.1.258 relays a peer message under THREE preambles (`Another Claude session sent a message:`, `... while you were working:`, `A peer session sent a message while you were working:`); csift knew one, so 29 of 47 `<agent-message>` records in the reference corpus (the mid-turn form) carried NO label at all - `is_section_boundary` now accepts all three; the corpus-wide `agent.communication.inbox` census moves 8721 -> 8750. (b) `AUQ_ANSWER_MARKERS` gains the third synthesized phrasing `The user answered: ...` (16 carriers at 2.1.258; the retired `User has answered your questions` stays for older transcripts; `The user did not answer the questions.` is the unanswered branch and never opens a turn); the same string joins the verifiable synth markers. (c) The registry reader takes `procStartFt` when present, else `procStart` (the 2.1.258 schema carries both and every binary reader is `procStartFt ?? procStart`; the live Windows row wrote its FILETIME under `procStart`). **Measured corrections recorded in the ledger only (no csift behavior change):** narration-tagged thinking blocks DO occur in subagent transcripts (24 in 5 lanes of one parent session; the spanned census exceeds the top-level one by exactly that); at 2.1.258 `stop_hook_summary` and `turn_duration` are written as a strictly alternating pair (24/24), `turn_duration` naming the summary as parentUuid, and `turn_duration` is suppressed by the `showTurnDuration` setting; the Read truncation banner moved from the tool_result text (<= 2.1.202, pathless) to a `read_truncation_notice` attachment that always names the path (>= ~2.1.206); the `hook_response` system message is emitted only for SessionStart/Setup unless `allHookEventsEnabled` and is never a transcript record; a Monitor armed by a subagent persists its pulses ONLY in the parent main (39 corpus arms, one live); `isSnapshotUpdate` is uninformative (never true without a version change, 1,278 changes carried false, uniformly false since 2026-08); since 2.1.251 a `parts` Read's PDF page bytes DO reach the transcript (a follow-up for `image`).

> **v0.10.0 CHANGE LEDGER (BREAKING: `wait --timeout` is required and `stop` no longer fires over an open background task; plus the promoted-leaf work built as 0.9.5 and never released separately -- `csift 0.10.0`).**
>
> ① BACKGROUND TASKS in `status`/`wait`. A `Bash` launched with `run_in_background` pairs its tool_result within milliseconds, so the tail state machine never sees it; the harness writes NOTHING about a still-running shell at end of turn (measured: 100/100 `turn_duration` records emitted with an open shell carry no shell field; the REPL's "N shell still running" is process memory; `pendingBackgroundAgentCount` covers async agents only, never emitted as 0). `status` now scans the WHOLE main transcript (never-returned launches sit 56-375 MB before EOF; a five-needle byte prefilter keeps it at +0.2-0.4 s worst case) for three launch kinds -- a backgrounded shell (`input.run_in_background:true`, result `Command running in background with ID: <id>. Output is being written to: <path>` + `toolUseResult.backgroundTaskId`), an async agent (`toolUseResult.status:"async_launched"` + `agentId`/`description`/`outputFile`), and a Monitor arm (`Monitor started (task <id>, …)` + `toolUseResult.taskId`; a command Monitor is a `local_bash` task and shares the `b…` id namespace with shells, so only the tool name distinguishes them) -- and for their completions, which are `<task-notification>` records joined by `<tool-use-id>` (== the launching tool_use id, exact; `<task-id>` second) with `<status>` completed | failed | killed | stopped, riding THREE carriers: a `type:"user"` record when the session was idle, or a `queue-operation` enqueue/remove + a `queued_command` attachment when it landed mid-turn (40% of returned shells never produce a user record). Launches are read from every lane (a subagent target also reads its PARENT main), completions from the main file only (607/618 subagent-launched shells complete in the parent; zero notifications exist in any subagent transcript). A Monitor's event pulses (no `<status>`) never close it; its termination notice (`Monitor …`, completed) or a `timed out` event does; a PERSISTENT monitor never returns by design. Claude Code's own orphan summary (one notice with several `<task-id>` + `__orphan_summary__:shell`, status stopped, written at the next session start with `shouldQuery:false`) marks tasks stopped and states the honesty bound csift repeats: "may have been stopped (via the UI, Monitor timeout, or agent teardown -- these leave no transcript marker)". Ctrl+C kills background AGENTS only (never shells or monitors); the REPL hides no running task by age. Each open task renders a `bg` row (kind, id, launch instant + age, description/command, the output file's size and last write via one `stat`); closed ones fold to counts; JSON `background{open, ignored, completed, failed, killed, stopped, timed_out, scanned_files, tasks[], notes[]}`.
> ② THE SEVENTH VERDICT `idle-background-open` (precedence dead > hitl > running > children > background > eot > unknown): the turn ended cleanly but N background task(s) the lens counts have not returned -- neither running nor stopped, by design or not, csift cannot tell. `Cond::Stop` stays = idle-eot | stale-dead, so it NEVER satisfies `--until stop` (a session with a dev server reaches `stop` only through the lens, else its timeout). THE LENS on both commands: `--background-since WHEN` (the shared time grammar, now also `2mo`/`1y`, a tolerated leading `-`, and the token `now` = the command's own start instant) counts only tasks launched at or after WHEN; `--ignore-background RE` (repeatable) excludes tasks whose command or description matches. Every task is still listed; an excluded one is marked with its rule.
> ③ **BREAKING** `wait --timeout` is REQUIRED (a call without it is rejected with the reason: a background task can be designed never to return). On every exit (fired or timeout) `wait` reports `at exit` (in a tool call for N s / generating / idle), `activity` (a census of the records that landed after the baseline: tool calls by name, thinking, messages, prompts, notifications, lanes), the `bg` rows, and the LAST MESSAGES; `status` prints the same `last ◂ / last ▸` excerpts (400 chars, `(+N chars)` marker, refetch `show --turn -1`) under the warning that an excerpt is a partial view of the final state, useful only for judging whether a background task is still meaningful, never a review of the work (a model with a partial context believes it read everything, even past an explicit tool error). JSON: `wait{at_exit, activity{}, background{}, last{}, notes[]}`, `status{background{}, last{}, tail_state}`.
> ④ TWO CLASSIFICATION FIXES. (a) The harness's agents-stopped notice -- `N background agents were stopped by the user: …` / `Background agent "…" was stopped by the user.` (plain text, `origin.kind:"task-notification"`, no isMeta, written by the Ctrl+C handler; 9 corpus specimens read as `user.message` = a kill notice counted as a human turn) -- is `harness.notification.subagent`, excluded from `is_genuine_user` (never opens a turn), rendered `[subagent stopped] <notice>` with its own synth marker. (b) A `Background command "…"` pulse is ALWAYS `harness.notification.background-command`: the quoted-name heuristic (`monitor`/`re-arm`/`liveness` in the name routed to `.monitor`) predated the real Monitor tool and double-booked -- 40 false `monitor` records on one project against zero genuine Monitor pulses; `.monitor` is now the Monitor tool's own pulses and termination notices only. RE-CLASSIFICATION IS INTENDED (historical `--count-by label` figures for those two leaves move).
> ⑤ FIVE PROMOTED LEAVES (taxonomy 28 -> 33): the non-record jsonl line types become searchable, classifiable leaves -- `user.queued` (a `queue-operation` line carrying the human's text), `harness.meta.turn-duration` (`system`/`turn_duration`), `harness.meta.away-summary` (`system`/`away_summary`), `harness.meta.stop-hooks` (`system`/`stop_hook_summary`), `harness.meta.snapshot` (`file-history-snapshot` + `file-history-delta`). Every one is LLM-INVISIBLE by the boundary's instrument: none of the 24 measured non-record types carries a `message{}` field, and Claude Code's own source labels them REPL-render internals (its `is_virtual` "display-only, filtered before API send" concept never lands on disk). NOT instruments: parentUuid threading (later user records DO name a `turn_duration` uuid as parent -- chain continuity) and `preservedMessages` membership (a tail window; system subtypes appear at the same rate as messages). The naming is by FAMILY semantics: every `harness.notification.*` leaf is a delivered user-role pulse, so telemetry goes under `harness.meta`, named by the meta fact it carries. §5.1 carries the per-leaf laws and renders.
> ⑥ THE GATED-LEAF LAW: a promoted leaf is admitted by the §7 byte prefilter ONLY when an EXPLICIT `-t` reaches it (the full path, a glob, or the `harness.meta` prefix) or a `show --line/--uuid` address names the line. A bare scan, a bare role, a `-T`-only filter and `--count-by label` without `-t` never parse those lines -- unlike the compact-boundary keep, which a match-all scan admits -- because 87% of queued content is a duplicate automation pulse and every promoted line is a non-message line; the default surface stays the conversation at zero added cost (measured 1.03x, noise, on an unscoped no-match search). Each gate is its own `&&`-gated memmem needle (`"queue-operation"`, `turn_duration`, `away_summary`, `stop_hook_summary`, `"file-history-`) per the R13 needle law. The three fabricated renders (turn-duration, stop-hooks, snapshot) register their type value as a VERIFIABLE synth marker so the §7f whole-file gate stays sound (pinned: a pattern matching only fabricated text still hits). The zero-match diagnosis states when no selector reached a gated leaf (stderr note + JSON `gated_leaves_unreached`).
> ⑦ QUEUE FACTS, measured over the corpus: `content` rides `enqueue` (100%) / `popAll` (100%) / `remove` (75%) and NEVER `dequeue`; `reason` occurs only on `remove` (`absorbed_mid_turn`, `delivered_to_agent`); the queue line has NO join key (4-6 keys, no promptId/uuid) and a dequeue references nothing, so `dispatched` is never asserted -- the hit carries `operation` + `reason` verbatim (label zone `[enqueue]` / `[popAll]` / `[remove · <reason>]`; JSON `queue_operation` / `queue_reason`, null on every other hit). Riders (a `<task-notification>` or peer message queued by the harness -- 87% of content-bearing queue lines) carry no label; the human/automation discriminator lives on the delivered twin (`origin.kind`, `promptSource`, `queued_command.commandMode`), not the queue line. CORRECTION to the v0.9.2 ledger's "~61% of queued texts never become user records": that rule counted every queue operation (remove lines included) over 3 sessions; enqueue-only over 6 sessions, the human's prose is dispatched 72% (exact) / 81% (prefix) of the time, so 19-28% never becomes a user record.
> ⑧ `show --line/--uuid` renders every promoted line flag-free (the refetch law); the miss error now names what stays `--raw`-only: the session-state cache lines (`last-prompt`/`mode`/`ai-title`/`agent-name`/`permission-mode` -- no uuid, no timestamp, no message, `lastPrompt` a duplicate of `user.message`; deliberately NOT promoted), a content-less dequeue, the unpromoted system subtypes (`scheduled_task_fire` 459 -- a second record per event beside `harness.schedule.wakeup`, `model_refusal_fallback` 50, `api_error` 29, `local_command` 15, `agents_killed` 10, `model_refusal_no_fallback` 2, `informational` 1) and the other bookkeeping types (`atis-latch` 450, `bridge-session` 382, `fork-context-ref` 33, `cost-state` 14). Deferred, with counts, so nobody re-derives them. `stats` continues to census every line type.
> ⑨ Help/doc surface: the `-t` reference lists 33 leaves, the taxonomy help gains the GATED LEAVES rule and four meta rows, the JSON schema names `queue_operation`/`queue_reason`/`gated_leaves_unreached`.

> **v0.9.4 CHANGE LEDGER (non-breaking surface additions + two correctness fixes -- `csift 0.9.4`).**
> ① `recover` BASH CONTENT ANCHORS: the deterministic shell subset replays as first-class content events instead of boundaries. WRITE anchors are SEGMENT-scoped (the dominant real shape is "write the file, then run it" in one compound command): a quoted-delimiter heredoc consumed by `cat`/`tee` (body byte-verbatim in the tool_use input; an unquoted delimiter admits only an expansion-free body), literal `echo`/`printf`, `truncate -s 0`; a compound command additionally demands a CLEAN result echo (empty stderr + not interrupted -- only the last segment owns the exit code, and a failing cat/tee/echo always writes stderr), and a second touch of the same resolved path anywhere in the command refuses the anchor. READ anchors are single-simple-command only (compound stdout is an unattributable concatenation): `cat F`, `head -n N F`, `sed -n 'A[,B|,$]p' F` under the completeness gate (result echo present, empty stderr, not interrupted, not externalized); an EOF-reached window from line 1 promotes to a full anchor. A byte-known `>>` append places only onto a complete newline-terminated buffer, else it discloses as a `bash_append_unplaced` heuristic boundary. An admitted write anchor supersedes its own heuristic bash-mutation boundary. New coverage counts `bash_read_anchor`/`bash_write_anchor`; new anchor-source labels `bash-cat`/`bash-heredoc`/`bash-write`. Deliberate non-anchors, each measured or unplaceable: `tail` (line-keyed buffer cannot place it), `sed -i` (zero literal-subset yield in 1,085 real steered commands), variable targets, interpreter and ssh heredocs. Measured: a real heredoc-then-run python tool went from no recoverable history to 33/33 lines verbatim.
> ② `plan` CORRECTNESS: csift bound ONLY via the `plan_mode` attachment while Claude Code itself binds by the FIRST slug-carrying record -- on a forked clone (attachments stripped, slug records kept) csift answered "no plan" for a session whose plan CC WILL re-inject. Now two binding laws in precedence order: `plan_mode` attachment, else the first VALID slug record (CC's slug validity rule: lowercase alnum head, alnum/dash tail, <=120; `plansDirectory` from settings.json honored, else `<claude-home>/plans`). Rows carry `binding_source` ("plan_mode"|"slug-only") and `minted_at_compaction` (the slug's first carrier is a compaction boundary -- the fork mint site).
> ③ `list` CLONE LINEAGE: a transcript whose FIRST TIMESTAMPED record is a system/`compact_boundary` was minted by COPYING another session at that compaction (background-job forks; measured on a 61-file dir: the one known fork, zero false positives; file-birthtime rules refuted -- copies/migrations move birthtimes). The row annotates `clone forked from SESSION <origin> at compaction boundary <uuid>`; the origin join scans siblings for the boundary uuid and requires the record itself (uuid match + compact_boundary) NOT at the sibling's own head (a prose quote parses to a different uuid; a co-clone's head probe returns the same uuid) -- paid only on detection. JSON: `is_clone`, `clone_of` (null when unjoined), `clone_boundary_uuid`. Corollary documented: a clone DOUBLE-COUNTS its inherited records on every spanning surface until scoped away.
> ④ `status`: child lanes gain a `generating` state (a tail RECORD younger than 300s AND last assistant stop_reason != end_turn; measured separation ~4 orders of magnitude -- intra-lane record gaps p99.9 = 295s vs dead-lane tail age p1 = 31h; the old 15s mtime window misread 1 lane in 17 mid-generation, and stop_reason alone would mark 73% of dead lanes live). The mtime recency leg is retired (`active` state removed; the closed set is `in-flight|generating|settled`). Settled lanes FOLD to a count (text one line; JSON `children[]` = live lanes only + `settled_children`). NEW tasks section: the harness task list under `<claude-home>/tasks/<uuid>/` or `tasks/session-<first8>/` (both real dir-name forms, merged) -- open tasks in_progress-first with `blockedBy`, completed folded to a count; JSON `tasks` (null = no dir, [] = an empty one) + `tasks_completed`. The help schema line also dropped `since_utc`/`since_local`, which the verdict row never emitted.
> ⑦ LLM-VISIBILITY SELECTOR LAW (a correctness fix: `user.unsent` under `-t user` broke a 0.7-era consumer - a superseded draft 12s before the real submit poisoned a last-human-touch hook reading `-t user --max-count -3`). Bare ROLE selectors now expand to LLM-VISIBLE leaves only; a new GLOB form `<prefix>.*` expands to every leaf under the prefix (visibility ignored); intermediate prefixes and full leaf paths keep their full sets (a drill-down names what it wants - `-t harness.compaction` still reaches the boundary). Exactly two leaves are invisible, each instrument-verified: `user.unsent` (CC's own `compactMetadata.preservedMessages` excludes every draft uuid, 0/772 measured; the conversation DAG threads through the resend sibling - wording law: "not in the surviving conversation", never "the model never saw it", 3 drafts drew replies pre-retraction) and `harness.compaction.boundary` (no `message` field; the compaction SUMMARY is visible - the DAG threads THROUGH it, and `isVisibleInTranscriptOnly` turned out to be a summary-only display flag, `isMeta` an authorship flag: neither is a visibility instrument, `Class::llm_visible` is). `-t`/`-T` speak the same three-form rule; the label census keys follow it; `-t user` restores the 0.7 contract (0.9.2..0.9.3 included drafts). Blast radius measured: exactly one live downstream consumer (already self-patched with `-T user.unsent`, now redundant-but-harmless); the no-`-t` pure scan is UNCHANGED (drafts/boundaries stay searchable by default with disclosure).
> ⑥ THE FILE-HISTORY SNAPSHOT INSTRUMENT (settings-family external writes + recover rebasing). Claude Code rewrites its settings files IN-PROCESS (/model, /config, theme, permission-allow, plugin toggles) with no tool record and usually no printed trace (measured: 4 of 5 silent writes print nothing; corpus-wide, HALF of all settings.json mutations are silent). The instrument is CC's own `file-history-snapshot` records: every Edit/Write-tracked file gets a per-prompt `trackedFileBackups[<path>].{version, backupFileName, backupTime}` entry, and `version` bumps only when the bytes changed. Laws, all measured: the counter RESETS mid-session (148 real cases - a decrease is a new GENERATION, never an anomaly, and only same-generation jumps are readable as numbers); the `@vN` store name COLLIDES across a reset, so store content is trusted only when the blob's mtime agrees with the marker's `backupTime` (120s window); a tool write and a silent write in ONE snapshot interval merge into one bump, so the content comparison is the complete detector. `recover`: at each version change with a VERIFIED blob, the replayed buffer is compared to the snapshot content - a disagreement is an authoritative HARD `external_write` boundary and the replay REBASES on the snapshot bytes (before this, the incident file replayed a state that never existed on disk and reported 100% complete); without a blob, a jump with no tool write since the previous marker discloses the same boundary content-less. `files`: a jump with no tool record in the interval emits an `external write` timeline row (op `external_write`, non-heuristic, detail = the version transition + interval) - SCOPE-LIMITED to the settings family (`settings.json`/`settings.local.json` under a `.claude` parent): the tracked set spans 1701 corpus paths vs 11 settings-family ones, and the harness writes bookkeeping constantly, so wider reporting would flood every timeline (operator-ruled). New: `SnapSource::HistorySnapshot` (label `file-history-snapshot`), `EventCounts.snapshot_rebase`, `FileOp::ExternalWrite`, `FileMutation.detail`, the `external_write` hard-boundary kind.
> ⑤ `search` REQUIRED-NEEDLE PREFILTER (perf, byte-identical output): a pattern WITH metacharacters now derives a necessity-only needle set from its parsed HIR -- see the amended §7d law -- so an alternation query (`TodoWrite.*legacy|legacy.*TodoWrite`) drops from a full-regex crawl to near-literal speed (measured 1.70x wall / 1.9x CPU on the motivating query; a no-match alternation reaches the skim floor). Side win: a space-carrying plain pattern (previously prefilter-ineligible) anchors its longest whitespace-free run.

> **v0.9.2 CHANGE LEDGER (non-breaking surface additions + one measurement CORRECTION -- `csift 0.9.2`).**
> ① NEW LEAF `agent.thinking.narration` (taxonomy 26 -> 27). Since at least CC 2.1.170 the API can return a SECOND `thinking` block per assistant message under `thinking.display:"summarized"`: a one-sentence, user-language summary of the reasoning beside it, distinguished ONLY by a tag encoded in the base64 `signature` (protobuf path 2 -> 1 -> 8, LAST field at each level; clients >= 2.1.241 render it dim under the hint `summarized`). Classification is by SIGNATURE alone -- ~4% of narration blocks have no reasoning sibling at all, so adjacency is never consulted; signatures reach 200K+ base64 chars, so the WHOLE string is decoded; every failure path (absent/empty/bad base64/truncation/unknown wire type/non-UTF-8/unknown tag) degrades to plain `agent.thinking`. Render: the `[narration summary]` marker rides the LABEL zone only (the tag word is base64-hidden in raw bytes; matchable marker text would break the §7 prefilter laws). Hot-path cost is gated: a signature containing none of the tag's three base64 alignments cannot decode to `narration` and skips the decode entirely (whitespace inside the base64 conservatively re-opens the gate); the ungated decode cost ~+20% wall on a 685MB transcript, the gated form is noise-level. Sibling cap 1. `verbatim` never replays narration (already true by construction; pinned). RE-CLASSIFICATION IS INTENDED: narration blocks exist on disk from 2026-06-10, so historical `--count-by label` figures move -- `agent.thinking`(0.9.1) == `agent.thinking` + `agent.thinking.narration`(0.9.2).
> ② `stats` TOKEN CORRECTION: Claude Code writes one line per content block and repeats the IDENTICAL `message.usage` on every per-block record of one API message; summing per record over-read 2.2-3.5x (measured per field/model/session). Sums now dedupe PER FILE by the new tolerant `message.id` field, per-field MAX across the id's admitted records (immune to the compaction-replay shape that re-writes an id with ZEROED usage). An id-less record counts on its own. Scope law: the same id recurs across a session's transcripts (the spawn message is copied into each child with its own usage) and each copy is a genuine per-transcript fact, so the TOTAL row sums rows, never dedupes across files. Printed totals DROP accordingly.
> ③ `stats` narration census: `narration_blocks` per model (block counts only -- usage is per MESSAGE and covers the reasoning block and its narration sibling together, so a narration token figure is not derivable and none is invented) + `unknown_thinking_tags` (a tag value that is neither `thinking` nor `narration` surfaces without a csift release).
> ④ NEW LEAF `user.unsent` (taxonomy -> 28; operator-requested mid-round). The C-18 superseded-draft set (esc-recall + edit-resend, shared parentUuid) becomes searchable and censusable under its own leaf instead of collapsed-and-counted-only: a scan emits a matching draft as its own annotated unit (`<tok>·draft`, JSON `superseded_draft:true` + null `turn_index`, hits single-labeled `user.unsent`), the footer names the leaf, `--turn` windows suppress draft units, and `user.message` counts are UNCHANGED (drafts were never censused before, so this is a pure addition). Verified against live specimens: 7 historical drafts in the tool's own dev session surface with the leaf, incl. text that was a definitive absence under 0.9.1. Measured limits documented: an abandoned recall is undetectable (no sibling); ~61% of QUEUED texts never become user records (they persist only in `queue-operation` lines - non-records; a future release may gate them like attachments). [v0.10.0 correction: that figure counted every queue operation over 3 sessions; enqueue-only over 6 sessions the human's prose reaches a user record 72-81% of the time. The lines are now the gated `user.queued` leaf.]
> ⑤ Help staleness swept: the `-t` reference said "25 leaves" and omitted `harness.meta.attachment` since v0.8.1; the hand-kept path oracle in the unit tree was missing the same leaf and gained a length pin against `Class::ALL`.

> **v0.9.1 CHANGE LEDGER (non-breaking; pid-liveness fix on busybox-ps hosts -- `csift 0.9.1`).**
> busybox `ps` (Alpine and friends) rejects `-p`/`lstart` outright, so the `status` pid probe read a LIVE pid as dead and reported a live session `stale-dead`. When the ps form fails the probe now consults `/proc/<pid>` on Linux (present => alive, start time unknown, reuse-guard skip disclosed; absent, or no /proc as on macOS where ps is reliable => the no-such-process verdict stands). Found by the release matrix's musl test lanes; the reuse-guard e2e became platform-aware. (This entry was added retroactively in the v0.9.2 pass -- the 0.9.1 release commit carried only the CHANGELOG digest.)

> **v0.9.0 CHANGE LEDGER (MINOR - a new COMMAND CLASS: the live-truth pair `status` +
> `wait` -- `csift 0.9.0`).** The version signals the contract addition, not a breakage:
> every existing surface is unchanged; the new pair is a deliberate, documented departure
> from the forensic contract (point-in-time, explicitly non-reproducible; everything else
> stays reproducible). Grounded in live measurement on this machine (registry shape,
> flush timing, journal incrementality all verified before design).
> - **`csift status TARGET`**: one verdict on "has this session truly stopped?" from a
>   three-way join - the harness session registry (`<claude-home>/sessions/<pid>.json`:
>   status idle/busy/shell observed, blocked/waiting carried by the binary;
>   transition-writes only, NEVER a heartbeat - staleness is inverted, an old
>   statusUpdatedAt means "unchanged"; the failure mode is a SIGKILLed session's stale
>   entry, guarded by a ps-based pid probe plus a process-start-time check that parses
>   the registry's UTC-rendered procStart against ps's local-rendered lstart as
>   INSTANTS), the transcript tail state machine (an unreturned tool call at the tail =
>   in flight; stop_reason is trustworthy on the main lane and normally null mid-message
>   on subagents; growth alone is never activity - enqueues and attachments grow an idle
>   main), and child lanes (each child transcript's own tail + the INCREMENTAL workflow
>   journal, where started minus result = agents in flight; meta.json has no status
>   field and workflow result files are terminal-only). The elicitation sidecar covers
>   HITL. Verdicts: running | waiting-children | waiting-hitl | idle-eot | stale-dead |
>   unknown - each with named evidence rows; idle verdicts carry the honesty note that a
>   pending PERMISSION prompt is invisible to this instrument (no sidecar exists yet).
>   The registry covers top-level interactive sessions only; a subagent target degrades
>   honestly to tail evidence.
> - **`csift wait TARGET --until COND ...`**: block until a condition fires. Conditions
>   (repeatable, OR, first hit wins): stop | hitl | auq | notification[:RE] (main lane
>   only - notifications never persist in child transcripts) | tool:NAME[:RE] |
>   write:PATH_RE[:LINE_RE] | verdict:V; the same RE2-class regex engine as search.
>   BASELINE SEMANTICS: wait snapshots every watched file's byte length at startup and
>   fires only on events strictly after those offsets - a condition satisfied by history
>   alone never fires (history is `search`'s job); a readiness line on stderr after the
>   snapshot makes scripted waits race-free against their own trigger. Incremental
>   torn-tail-guarded reads (append-atomicity measured); adaptive polling (200ms floor
>   to 2s, `--interval` pins it); new child lanes join the watch mid-wait with baseline
>   zero. EXIT LAW EXCEPTION: timeout exits **124** (the GNU timeout convention) - the
>   ONE documented exception to the 0-vs-non-zero law, because a monitor's timeout is a
>   normal outcome a script must branch on; JSON always carries fired:"timeout".
> - Model: `Message` gains the tolerant `stop_reason` field. Structure gate: the
>   per-folder subfolder cap rises 15 to 16 (src/ grows one directory per command family
>   by design and now sits AT the cap; the next command module must consolidate first).
> - Out of scope (recorded): a permission-prompt sidecar (the elicitations precedent,
>   post-0.9.0); a per-subagent registry (does not exist in the harness - never faked);
>   teams inbox contents (C-09 stands); remote/cross-machine sessions.
> - Suite grows to 939 unit + 474 e2e.

> **v0.8.2 CHANGE LEDGER (non-breaking; field-incident fixes + a soundness correction --
> `csift 0.8.2`).** Every item traces to a measured field incident or a live re-measurement;
> two of the fixes correct csift's OWN documentation and output where they stated a wrong
> mechanism or a fabricated certainty.
> - **Targeting: a bare-basename `*.jsonl` token resolves and classifies.** `x.jsonl` with no
>   separator fed its empty parent into the project scan (a wrong "no session file found"
>   bail) and misclassified a bare `agent-<hex>.jsonl` as a top-level session. The token is
>   absolutized at the top of the `.jsonl` branch; every spelling of one file now resolves
>   identically on every command.
> - **@trap mechanism sentence corrected (soundness).** The documented "the main record is
>   flushed only after the command completes" was refuted by re-measurement: a subagent
>   transcript flushes per content block (on disk at dispatch, first try resolves); the MAIN
>   conversation's record is an async flush of the completed assistant message landing
>   ~1-3.4s after dispatch - a race, not a wait. Both behavior rules survive with corrected
>   reasons; the no-match error is re-ordered route-first (`@main` is the answer, the
>   literal-marker check second, the same-marker retry last as lane confirmation); and a
>   @trap that RESOLVES to the main transcript now prints a stderr lane note (that branch's
>   former silence is what let the wrong doctrine form). Dated ledger entries keep their
>   original text with bracketed corrections.
> - **Lane honesty: `whoami` (env form) + `@main`.** The env names the TOP-LEVEL session in
>   every lane, so bare `whoami` reported three confidently wrong fields from a subagent -
>   on exactly the command an agent runs to CHECK its identity. Env-form `is_subagent` /
>   `parent_session_id` / `depth` are now null (unknown is stated, never fabricated), text
>   carries a lane line, stderr names the resolution path; every `@main` resolution prints
>   an unconditional stderr lane note. A first-try `@trap` HIT is lane proof; a miss proves
>   nothing.
> - **Image discoverability.** A fleet incident published "images are unreadable" without
>   testing extraction. The first image-bearing row per run now carries a paste-ready hint
>   (INPUT id forms: a `#N` handle as the bare number; `L<line>i<n>` as-is) on search and
>   show; the search footer gains a capability note; search and verbatim `--help` gain SEE
>   ALSO sections naming the extraction command.
> - **Errored results are visible.** Hit JSON gains `is_error` (true|false on result hits,
>   never null there); the text render decorates an errored result row `[error]`; a new
>   additive `--count-by result` axis buckets `ok`|`error` (the closed `pairing` enum is
>   untouched: pairing answers "did a result come back", result answers "was it good").
> - **Superseded-draft honesty.** The esc-edit draft collapse in turn reconstruction was
>   silent and an addressed draft failed with a wrong error. The collapse count is now
>   disclosed (search footer + JSON `superseded_drafts`); an explicit `show --line`/`--uuid`
>   fetches a draft as an annotated unit outside turn numbering (JSON `superseded_draft`,
>   null `turn_index`); the address-miss error states the real render domain.
> - **SKILL**: a new "Why not hand-roll this format" section (nine measured traps, each
>   returning a plausible wrong answer with no error, each mapped to a csift move) placed
>   before the routing table; the frontmatter description now carries the cost of skipping
>   csift (a live-session /reload-skills is needed once after upgrading).
> - Suite grows to 935 unit + 466 e2e.

> **v0.8.1 CHANGE LEDGER (non-breaking; the maintenance round: additive surfaces + one
> correctness flip -- `csift 0.8.1`).** Every claim below was verified against real corpora
> (and the live on-disk stores where applicable) before implementation; two brief-proposed
> features were refuted by that verification and replaced with fact-reporting surfaces.
> - **`list`: `version`/`git_branch` report LAST-seen** (a correctness flip with the v0.7.1
>   precedent: the docs promise the session's version, and a session that upgraded Claude
>   Code or switched branches mid-flight reported stale opening samples). The opening
>   samples move to `version_first`/`git_branch_first` (JSON also mirrors `version_last`/
>   `git_branch_last`); text shows a drift arrow (`branch a->b, CC x->y`) when they moved.
>   `cwd` stays FIRST-seen on purpose: the record cwd follows the tracked shell cwd, so
>   last-seen could be a transient subdirectory.
> - **`plan` surfaces the binding record's `slug`** (text `slug` line, JSON `slug`): the
>   slug is minted at Plan-Mode entry and carried by the `plan_mode` bind record itself
>   (absent from every earlier record, including the whole head window).
> - **`agents`: fork provenance + `--agent-type`.** A transcript created by `/fork` opens
>   with a `fork-context-ref` record (`{agentId, parentSessionId, parentLastUuid,
>   contextLength}`, timestampless); nodes carry `fork_parent_last_uuid` +
>   `fork_context_length` (text: `forked-at <uuid> (context N)`), and `--agent-type <T>`
>   (repeatable, exact match) filters nodes by `agent_type` (`--agent-type fork` lists
>   fork children). NOT a `--shape` value: the discriminator is the agent type, not the
>   on-disk location.
> - **`search --attachments` + the `harness.meta.attachment` leaf + `--count-by
>   attachment`.** Attachment records (the bulk of many transcripts' bytes) become
>   searchable behind an explicit gate generalizing the `--additional-context` precedent:
>   the flag is a SUPERSET of it (hook payloads keep `harness.meta.hook`; every other
>   payload classifies the new leaf, `Class::ALL` 25 -> 26), the matchable text is the
>   VERBATIM payload JSON (a byte substring of the raw line, so the prefilter and
>   whole-file-gate laws hold with no synthesized markers), and the census axis implies
>   the gate. A default scan still never parses attachment lines; an explicit `show`
>   address renders any attachment record flag-free.
> - **`search --count-by version`**: a per-record census of the top-level Claude Code
>   `version` stamp (which versions a session ran under, where an upgrade landed);
>   stampless records are excluded and disclosed.
> - **`stats`: whole-file line-type census.** Every physical line counts under its
>   top-level `type` (`line_types` in JSON, a `types` text line, merged scope totals); a
>   file fact like `lines`, never windowed. The non-candidate probe fully validates every
>   line, so a `{...}`-framed line with an invalid interior is now counted malformed even
>   off the candidate path: stats' full-scan census claim is exact.
> - **`recover --list-backups`**: lists Claude Code's OWN file-history checkpoint store
>   (`<claude-home>/file-history/<session>/<hash>@vN`; hash = sha256 of the absolute path,
>   first 16 hex; content = the plain snapshot). Verified store facts bound the surface:
>   tool-layer writes only (bash and manual edits never land there), pruned wholesale, and
>   `@vN` counters reset per session dir and get reused, so only the backup instant (store
>   mtime) orders entries. Listing only: checkpoint content has no transcript anchor and is
>   never merged into a reconstruction; an empty result exits 0 and says absence proves
>   nothing. Rides the same change: four doc sites claimed `backupFileName` is frequently
>   null; measured 83-98% present, reworded.
> - **`show --branch-points` + `logicalParentUuid`.** The brief proposed a live/abandoned
>   rewind-branch classifier; verification REFUTED it (parallel tool fan-out creates
>   sibling childless leaves that false-positive as abandoned branches on most sessions),
>   so csift ships fork FACTS instead: every record with 2+ conversation children (a later
>   `parentUuid` re-attach), children with lines and timestamps, ranked by the widest
>   inter-child gap; tool-result carriers, isMeta records, and compaction summaries never
>   count as children; csift never classifies which side is live. The model also gains the
>   boundary's top-level `logicalParentUuid` (the true predecessor a compaction re-links
>   to), surfaced in the boundary excerpt as `[logicalParent=<uuid>]`.
> - **`plan --audit`**: joins the target scope's structured plan-file mutations against the
>   corpus's `plan_mode` bindings and warns when the mutating session does not bind the
>   file (only the BOUND plan is re-injected in full after a compaction). Plan files are
>   identified by the binding join, never a directory guess (`plansDirectory` is
>   configurable); bash-side edits are outside the audit and the empty form says so.
> - **Out of scope, recorded**: the live team/task coordination files under the config home
>   (mutable, unversioned, mid-write state owned by the running harness) stay unread; the
>   transcript is the durable record (`agents` for topology, `search -t
>   agent.communication` for the messages).
> - Dependency: `sha2` (the store key). Suite grows to 935 unit + 456 e2e.

> **v0.8.0 CHANGE LEDGER (BREAKING; the bash-mutation cwd resolution + honest window
> accounting -- `csift 0.8.0`).** The feature: bash file mutations resolve against the
> shell cwd Claude Code itself records, and every recover output accounts for what the
> replay could not include. Grounded in a full-corpus investigation (22,410 real bash
> commands joined against csift's own output; mechanisms extracted from the CC 2.1.237
> binary and validated on 18,185 commands -- see section 4.9): 19.4% of bash calls
> mutate a persistent file; csift's full-target attribution rises from 27.1% to a
> measured 77.0% ceiling, and recover's relative-vs-absolute join closes from 95.84%
> to 99.65%. Determinism is by construction: every resolved path carries an explicit
> resolution class (`absolute` = typed absolute; `cwd-joined` = the record's own `cwd`
> field, zero inference; `cd-tracked` = literal in-command cds, a validated lexical
> inference; `unresolved` = kept verbatim and disclosed, never guessed).
> - `bash_mutations`: per-segment cwd tracking (`CwdAt` checkpoint per operand;
>   subshell-isolated; `cd -`/pushd/popd modeled; unknowable targets stay verbatim);
>   interpreter write-idiom analysis (python/node/ruby heredoc + inline scripts;
>   literal and one-hop-constant targets emit `interp-write` rows, anything else one
>   `interp:<lang>` marker); `perl -i` (the sed twin, verb `perl-i`); leading-`~`
>   operands kept verbatim (class `unresolved`) instead of dropped; mutating-CLASS
>   markers in the `git:<sub>` style (`fmt:<tool>`, `pkg:<manager>`,
>   `extract:<tool>`), with dry-run forms emitting nothing and named-operand
>   formatter runs emitting real `fmt` rows.
> - `files`: a bash row's `path` is the RESOLVED spelling (relative + absolute
>   spellings of one file share every bucket); JSON timeline rows gain `resolution`,
>   `path_verbatim`, `command_errored`; mutations from a partially failed bash chain
>   are kept and flagged instead of dropped; `git apply --check`/`clean -n` dry runs
>   and rsync remote destinations no longer emit rows.
> - `recover`: the bash arm joins on resolved paths (verbatim kept as a belt); every
>   mode disclosed per window -- boundary sections with a hard/soft split
>   (`boundaries_hard_count` now counts only invalidating kinds), opaque
>   mutating-class + PowerShell command counts and rows, and a ready-to-run
>   time-bounded `csift search` command; restore's success note states a clean
>   window positively or lists what was not replayed ("complete from the tool
>   stream; NOT verified against disk"); Claude Code's own freshness signals are
>   adopted (`staleReadFileStateHint` -> hard `hint_modified` boundary,
>   `staleRecovered` -> `stale_recovered` annotation, the degraded empty-snippet
>   `edited_text_file` form named, the formatter-class clue on external edits);
>   `String to replace not found` and `File does not exist` are counted annotations;
>   soft boundaries no longer disarm the originalFile cross-check.
> - BREAKING details: the batch `recovery-report.tsv` gains three inserted columns
>   (`boundaries`, `bash_file`, `bash_opaque` before `target`); restore's status
>   lines and the partial-failure message are reworded; a fully-invalidated history
>   errors with an invalidation-naming message instead of the false "never
>   Read/Written/Edited"; salvage's empty result names "at the latest state"; the
>   restore JSON error paths now emit their `{kind:"restore", complete:false,
>   reason}` row + summary before the non-zero exit (the envelope is never left
>   half-open); boundary JSON rows gain `source_session_id`/`source_line` and
>   coverage rows gain `hard_boundaries`/`soft_boundaries`/`opaque_commands`/
>   `powershell_commands`/`suggested_search`.

> **v0.7.8 CHANGE LEDGER (non-breaking; windows-path recover fix + the first cross-platform validation -- `csift 0.7.8`).**
> `recover`'s file-level basename prefilter split on `/` only, so a windows-shaped
> target (drive letter, backslashes) became a raw-byte needle that JSON escaping can
> never contain: every recover of such a path silently reported no history. The gate
> now splits on both separators, refuses a basename JSON could rewrite (7d law), the
> batch scan exempts such targets instead of dropping them, and `path_matches`'
> basename-suffix arm accepts a backslash boundary. Found by the first test-suite
> runs on real Windows; the full suite now passes on macOS (arm64, x64), Linux
> (gnu/musl x x64/arm64), and Windows (MSVC arm64/x64).

> **v0.7.7 CHANGE LEDGER (non-breaking; help prose pass -- `csift 0.7.7`).**
> Every `--help` page reworked for plain punctuation (em dashes replaced by commas,
> colons, semicolons, parentheses; one flag-list restructure, zero content loss --
> the drift guards pin flag mentions and example validity). No flag, output, or JSON
> change. Repo-side, .rs comments are ASCII-only (em dash banned, gate-enforced);
> string literals -- output glyphs like the missing-timestamp marker, fixtures,
> quoted program output -- keep their deliberate non-ASCII.

> **v0.7.6 CHANGE LEDGER (non-breaking; opt-in hook-context search — `csift 0.7.6`).**
> `search --additional-context` ALSO scans hook-injected additionalContext — the
> `type:"attachment"` records a SessionStart/UserPromptSubmit/... hook writes into the
> transcript (`attachment.type == "hook_additional_context"`; `content` is a string ARRAY,
> joined with `\n`, bare string tolerated). Off by default: injected context is harness
> machinery and echoes prompts/files wholesale. Hits classify `harness.meta.hook` (a
> record-text class; the record never opens a turn). The stage-1 candidate needle
> (`hook_additional_context`, a bare value substring per the R13 needle law) is `&&`-gated
> like the D7 boundary keep — a default scan pays ZERO; the widening also fires for an
> ADDRESSED fetch, so the `csift show @<id> --line N` refetch a hit prints renders the
> record WITHOUT the flag. The zero-match diagnosis lists the flag among active filters.
> Docs: README restructured scenario-first; SKILL documents the file-mtime semantics of
> CC's `cleanupPeriodDays` retention (verified against the 2.1.233 binary: cutoff =
> now - N days vs `stat.mtime`; deleting a transcript removes its sidecar tree).

> **v0.7.5 CHANGE LEDGER (non-breaking; packaging/publication release — no CLI surface change — `csift 0.7.5`).**
> csift is published on crates.io: `cargo install csift` is now the primary install path.
> Cargo.toml gained the publication metadata (`repository`, `readme`, `keywords`,
> `categories`; the `publish = false` guard is removed; the `CLAUDE.md` symlink is
> excluded from the tarball — it would dereference into a duplicate of `AGENTS.md`).
> README documents the name — pronounced "c-sift", in the `csplit`/`ctags` naming
> tradition: **c** for Claude Code, **sift** for what it does — and the crates.io
> install. No command, flag, output shape, or help text changed.

> **v0.7.4 CHANGE LEDGER (non-breaking; correctness fix — the Windows shell tool, evidence extracted from the CC 2.1.228 binary — `csift 0.7.4`).**
> Binary-forensics round 2 answered "what is the Bash tool on Windows":
> 1. **The Windows shell is a SEPARATE tool named `PowerShell`** (`mi="Bash"; Ls="PowerShell"`;
>    tool registry lists both; same `input.command` field; "Executes a given PowerShell
>    command with optional timeout…"). Enablement: `CLAUDE_CODE_USE_POWERSHELL_TOOL`
>    override → FORCED ON when no Git-for-Windows bash is found (detection:
>    `CLAUDE_CODE_GIT_BASH_PATH` → `C:\Program Files\Git\bin\bash.exe` → the `(x86)`
>    sibling) → else a feature gate. The Windows `Bash` tool runs the REAL Git-for-Windows
>    (MSYS2) bash — POSIX syntax, so the lexical layers stay valid on those records.
> 2. **`@trap` matches BOTH shell tools (§6.3a).** The carrier check accepted only
>    `name:"Bash"`, leaving self-identification blind exactly in the mandatory Windows
>    fallback mode; it now accepts `Bash` and `PowerShell` (both carry `input.command`).
>    Error/retry text says "shell (Bash / PowerShell) invocation".
> 3. **Deliberate non-changes, documented:** the bash-LEXICAL layers (dangerous-rm
>    escalation classification, bash_mutations attribution in files/recover) do NOT run on
>    `PowerShell` records — a pending PowerShell lane classifies `awaiting-execution`, and
>    PS shell-side mutations stay outside the heuristic (structured Read/Write/Edit
>    attribution unaffected). ALSO RECORDED: 2.1.228's dangerous-rm has evolved past the
>    ported 2.1.x generation (fixpoint `$(…)` stripping; tree-sitter bail at >64 command
>    substitutions) — port refresh is a follow-up (bash_danger.rs staleness note).

> **v0.7.3 CHANGE LEDGER (non-breaking; correctness fix + target-grammar widening, evidence extracted from the CC 2.1.228 binary — `csift 0.7.3`).**
> A binary-forensics pass (the bash_danger / plan-namer method) answered both open
> Windows questions from CC's own shipped code, then fixed csift to match:
> 1. **Config-home resolution VERIFIED (no code change).** CC 2.1.228:
>    `wn = (CLAUDE_CONFIG_DIR ?? join(os.homedir(), ".claude")).normalize("NFC")` —
>    `os.homedir()` never consults `HOME` on Windows, confirming v0.7.1's per-platform
>    split. (Divergence noted, csift deliberately saner: CC's `??` would accept an EMPTY
>    `CLAUDE_CONFIG_DIR` as a real value; csift treats empty as unset.)
> 2. **Encoding is now CC-EXACT (§2.1 law updated).** The sanitizer (`gT`/`upo`) runs the
>    JS regex per UTF-16 CODE UNIT on the NFC form. `encode_cwd` now applies NFC first
>    (new `unicode-normalization` dep) and replaces per code unit — closing the
>    formerly-documented NFD dash-count divergence (an NFD accented path now encodes
>    identically to its NFC spelling) and matching the astral-char two-dash behavior.
> 3. **Windows drive-encoded dirs are first-class targets (§2.3).** `C:\Users\x` encodes
>    to `C--Users-x` (letter-led, not `-`-led): `looks_like_encoded_token` accepts the
>    `<letter>--…` shape, the `@`-grammar routes `@C--Users-…`, and a bare drive token
>    that matches no projects dir falls through to the real-path interpretation (it CAN
>    be a real relative path, unlike a `-`-led token). A UNC-encoded dir (`--server-…`)
>    is targetable via the `@` form only (the bare `--…` positional stays reserved for
>    the mistyped-flag guard, which now says so).

> **v0.7.2 CHANGE LEDGER (non-breaking; performance only, zero surface change — `csift 0.7.2`).**
> A real-corpus profiling round (macOS `sample` + hyperfine on a frozen APFS-clone
> snapshot; a 28-command byte-exact A/B battery pins stdout/stderr/exit codes identical):
> 1. **Construct-once memmem finders.** Every per-line byte prefilter (the §7d stage-1
>    role matcher, the turns/files/recover/image/plan needle sets, `show`'s per-uuid
>    needle) built its searcher on every call via the stateless `memmem::find`; all now
>    use cached `LazyLock` finders (memchr's documented pattern).
> 2. **No wasted newline pass.** `scan_lines_parallel_chunked` ran a SERIAL whole-slice
>    newline count before any parallel work; a single-chunk scan (files < 4 MB — nearly
>    the whole corpus) never consumed it. Single-chunk scans skip it; multi-chunk scans
>    count in parallel, never counting the last chunk.
> 3. **Gated per-turn fan-out (§6.4 walk).** `search`'s per-turn match+render phase ran
>    serially per file — a giant transcript sat on one worker while the pool idled. The
>    turn walk now fans out on rayon, DOUBLE-GATED: scope small enough that the
>    across-files fan-out cannot fill the pool (or the file is 64 MB+, the straggler
>    class) AND ≥1024 records. Ungated nested fan-out measurably regressed broad scans
>    (steal churn) and was rejected; serial and parallel walks share one closure, and an
>    ordered collect keeps output byte-identical.
> Measured warm: big-session census 1.21×, no-match unscoped 1.13×, caseless literal
> 1.09×, verbatim 1.08×, phrase 1.06×; user CPU −2–4% across the matrix.

> **v0.7.1 CHANGE LEDGER (non-breaking; correctness fix — per-platform home resolution — `csift 0.7.1`).**
> 1. **The default data root resolves the way Claude Code's own `os.homedir()` does.**
>    `path::home_dir()` consulted `$HOME` first on EVERY platform; CC uses `$HOME` on Unix
>    but ONLY `%USERPROFILE%` on Windows. A bare Windows shell worked by accident (no
>    `HOME`, so the `std::env::home_dir()` fallback already returned the profile dir), but
>    a Git-Bash/MSYS shell exports a stray `HOME` — often a POSIX-style `/c/Users/...` a
>    native process cannot open — and csift resolved a `.claude` dir CC never writes.
>    `home_dir()` is now cfg-split (Unix: `$HOME` non-empty, else the OS fallback; Windows:
>    `%USERPROFILE%` non-empty, else the OS fallback — `HOME` is never read there). The
>    precedence contract is unchanged: `--claude-home` > `$CLAUDE_CONFIG_DIR` > the OS
>    home's `.claude`. The error message and `--claude-home` help name both variables.

> **v0.7.0 BREAKING-CHANGE LEDGER (authoritative — supersedes any conflicting older text; text surface only, JSON unchanged — `csift 0.7.0`).**
> Motivated by field telemetry of agent consumers abandoning the tool mid-task: a broad
> query's per-invocation session table flooded the head of the output before the first
> hit, a table ordinal reused across invocations silently addressed the wrong session,
> and the emission order was a coded contract no doc surfaced — each drove a consumer
> back to hand-parsing raw jsonl.
> 1. **Self-resolving exchange headers; session legend REMOVED (§6.2).** The `s<N> = <id>`
>    table and the `s<N>·t<turn>` ordinal header are gone. Every exchange header now opens
>    with a STABLE id-prefix token: the first 8 chars of the owning transcript id
>    (`<tok>·t<turn>`), derived from the id alone — identical across invocations, directly
>    usable as an `@` target, zero joins. Within one output, DISTINCT ids sharing their
>    first 8 chars lengthen as a group to their first 12 raw chars (a uuid token spans its
>    first dash), then to the full id; a teammate agent id (name-embedded, not hex-led)
>    renders whole. A subagent exchange carries `(parent <first-8-of-owning-uuid>)` on
>    EVERY header so a tail-truncated read still resolves ownership.
> 2. **Resolver widening so every emitted token round-trips (§2.3, fail-loud).** The
>    `@`-prefix match domain is now the UNION of top-level session uuids and subagent
>    agent ids (a unique subagent hit dispatches like a full `@<agent-id>`); a literal
>    `8-4-4-4-12`-layout prefix longer than 11 chars is a valid uuid-prefix token; a
>    12+-hex token keeps exact-agent-id semantics first, then falls back to a unique
>    literal-prefix match. Ambiguity always errors naming the candidates.
> 3. **Output geometry contract (§0 law; `search` text mode).** A head `matches` banner
>    (`matches  N exchanges · M sessions · oldest first[· showing earliest|latest K]
>    [· undated last]`) follows the scope banner whenever ≥1 exchange matched; the tail
>    footer now repeats the TRUE (pre-cap) totals beside its drop accounting. Suppressed
>    in `-c`/`-l`/`--count-by`/`--raw`/`--format json`. The stderr zero-match diagnosis
>    now discloses the malformed-line count when >0 (an absence claim is definitive for
>    parseable lines only). JSON summary shape unchanged (post-cap `matched` +
>    `dropped_by_cap` reconcile to the banner total).
> 4. **Signed `--max-count` (§6.2).** `N` keeps the EARLIEST N of the chronological
>    stream (unchanged), `-N` keeps the LATEST N, `0` stays uncapped. ONE ordering rule:
>    the kept exchanges always emit oldest-first among themselves — the sign only selects
>    which end survives (mirrors the range grammar's `-k` from-end form). Both ends
>    disclose the window; the footer names the dropped side (`N later|earlier dropped by
>    --max-count`). `show`/`list`/`stats` `--max-count` unchanged.

> **v0.6.10 CHANGE LEDGER (non-breaking; help/error-text/doc surface only — `csift 0.6.10`).**
> The fourteenth-audit release (Sonnet 5 on v0.6.9 — the strongest convergence evidence
> yet: the witness independently re-verified the whole R9→R10→refutation saga with
> CORRECT wire-format fixtures from scratch, spawned a live subagent to confirm both
> sides of the @trap timing asymmetry, closed R13's open `--multiline` thread by root
> cause, argued R13's completeness from the JSON grammar itself (RFC 8259: space/tab are
> the only legal intra-line whitespace — the NBSP case is invalid JSON, i.e. the already-
> named §4 residue), and surfaced zero new behavioral defects; every remaining yield is
> text-level).
> 1. **@trap retry granularity stated in the error + docs.** "Re-run the SAME command"
>    invited an agentic caller to batch both attempts into ONE shell script — which is
>    still ONE in-flight Bash tool_use (nothing flushes until the script exits), so both
>    miss. The no-match error, SKILL's §trap, and the assumption table now say the retry
>    must be a NEW, SEPARATE Bash invocation. *(v0.8.2 correction: the mechanism clause
>    here was refuted — the main-lane write is an async flush of the completed assistant
>    message landing ~1-3.4s after dispatch, a race, not a wait; the same-script rule
>    stands because both attempts run inside that unlanded window. The routing behavior
>    shipped at this version is unchanged.)*
> 2. **`--multiline` × re-serialized `tool_use.input` documented** (search --help +
>    SKILL): EVERY tool_use's matchable text is name + re-serialized JSON input (not only
>    AskUserQuestion's), so an embedded real newline is the two-character `\n` by match
>    time — match the literal `\\n`; `--multiline` is correctly irrelevant there.
> 3. **Doc completeness:** verbatim header's `automation_triggers` /
>    `budget_is_per_session` / `sessions_rendered` added to SKILL's field list; the
>    v0.6.9 ledger's "agree line-for-line" tightened to "agree in kind, tail-window
>    width differs by scan target" (a tear on the penultimate line is caught by `list`,
>    not by `agents` — verified at positions 298/299/300); a new SKILL row records the
>    self-echo trap (a nonce used as a search pattern writes itself into your own live
>    transcript — absence checks must scope away from your own session).

> **v0.6.9 CHANGE LEDGER (non-breaking; correctness fix — serialization-tolerant candidate detection — `csift 0.6.9`).**
> The thirteenth-audit release (Sonnet 5 on v0.6.8). The witness's headline (§1: "R12
> made `list` full-coverage but left `agents` unfixed") was REFUTED as an artifact of
> its own second finding — its fixtures used python `json.dumps` DEFAULT separators
> (`"role": "user"`), which blinded every byte prefilter, forcing `list`'s head scan to
> walk whole files (hence "full coverage") while `agents`' unprefiltered lifecycle
> stopped early as designed (hence "unfixed"); on compact (wire-format) fixtures the
> two commands agree IN KIND — window census, same disclosure trigger and wording, R12's
> fix + note live on both — while the tail-window WIDTH differs by scan target (`list`
> needs `last_user` + `last_agent`, two lines under alternating roles; `agents` needs one
> terminal record — so a tear on the penultimate line is caught by `list` and not by
> `agents`; R14's precision correction of this ledger's original "line-for-line"). But the
> witness's §2 IS the gold, and it is the same phenomenon R12's auditor hit and
> under-filed as a "fixture lesson":
> 1. **Stage-1 candidate detection is serialization-tolerant.** A valid-JSON record
>    whose serialization differs from CC's compact wire format (whitespace around the
>    colon: `json.dumps` defaults, a jq/editor round-trip) used to vanish one layer
>    BEFORE any malformed counter — no preview, no record count, no search match,
>    `skipped_lines: 0`, zero disclosure. The six role needles (search/turns/list/
>    stats + the user-only hook in files/recover) now route through shared
>    whitespace-tolerant matchers (`parse::line_has_role_marker` /
>    `line_has_user_role_marker`); a keyless line costs ONE `memmem` scan where the
>    old disjunct cost two, so §7 holds. Every other needle in the tree was
>    inventoried and is serialization-safe by construction (value substrings /
>    key-only forms); the needle law is codified in AGENTS.md §3.3a. The framing law
>    is unchanged: one record per LINE — pretty-printed multi-line JSON still breaks
>    jsonl framing and counts as malformed.

> **v0.6.8 CHANGE LEDGER (non-breaking; correctness fix + disclosed window semantics — `csift 0.6.8`).**
> The twelfth-audit release (Sonnet 5 on v0.6.7 — the round that broke R11's convergence
> declaration by varying a NEW axis: malformed-line POSITION as an independent variable
> from presence, compared ACROSS commands on identical bytes; both findings reproduced
> byte-for-byte).
> 1. **`list`/`agents` head+tail scans no longer double-book malformed lines
>    (correctness).** The two scans each counted whatever they walked, summed without
>    dedup — an all-garbage file reported exactly 2× at every size (both scans, finding
>    no anchor, walked the whole file). The head reader now returns its consumed-end
>    offset and the tail reader floors there: the windows are DISJOINT, every malformed
>    line in the read regions is booked exactly once. Anchor semantics unchanged (the
>    tail still walks below the floor for missing anchors — it just never re-counts).
> 2. **`list`'s window census is DISCLOSED, not silently narrower.** A mid-file tear is
>    outside `list`'s head/tail windows BY DESIGN (§7: full coverage measured ~4× the
>    unscoped runtime — the tradeoff stands, the silence about it does not). The text
>    note now reads `… skipped (among the head/tail lines read — full census: csift
>    stats)`; list/agents/stats `--help`, SKILL law 4, the assumption table, and the
>    schema tables all state the window-vs-census split; `stats` is named the full-scan
>    corruption-census authority. An e2e pins BOTH numbers (list 0 / stats 1 on the same
>    mid-tear bytes) so the divergence stays a contract, never drift. (list --help also
>    gained the row fields `sidecar_present`/`pending_elicitations`/
>    `with_elicitation_sidecar` its schema prose had omitted.)
> 3. **A schema-skewed sidecar marker is COUNTED (the fossil hole).** A sidecar line
>    bearing the `csift:"elicitation-marker-v1"` sentinel whose `csiftPhase` the current
>    schema cannot read (pre-release fossils under old field names: `phase`/`kind`/`key`)
>    was invisible — correctly never merged, but uncounted. The unknown-phase arm now
>    moves `skipped_lines` on every sidecar-merging surface: provably-ours yet
>    uninterpretable is a failure signature; valid-JSON-ness does not buy silence.

> **v0.6.7 CHANGE LEDGER (non-breaking; help/SKILL doc surface only — `csift 0.6.7`).**
> The eleventh-audit release (Sonnet 5 on v0.6.6 — a convergence round: 114 invocations
> across all twelve command surfaces, every prior-round fix independently re-verified
> including the documented residual boundary of the R10 malformed-shape check, ZERO new
> behavioral findings; the round's whole yield is two documentation clauses).
> 1. **SKILL names verbatim's two automation header fields** — `automation_by_kind` (the
>    SELECTED triggers per class) vs `automation_in_scope_by_kind` (every in-scope pulse
>    regardless of budget; the same window-vs-scope pairing as `boundaries_*`). The
>    distinction was already fully documented in `verbatim --help`'s schema prose — the
>    witness's "I had to guess from the name alone" was overbroad (it read SKILL, not
>    --help; the R10 sampled-one-row shape again) — but SKILL's field list really did say
>    only "+ the automation split". Both facts recorded; the list now names both.
> 2. **`returned_message` semantics stated: the ORCHESTRATOR's record, not the agent's
>    conclusion** (agents --help + SKILL). A `sync-tool-result` source faithfully reports
>    the parent's tool_result even when the harness truncated it to a `Done. agentId: …`
>    wrapper (verified live on a real spawned subagent); the child's own final words are
>    always `show @<agent-id> --turn -1..`.

> **v0.6.6 CHANGE LEDGER (non-breaking; correctness fix + header/JSON surface — `csift 0.6.6`).**
> The tenth-audit release (Sonnet 5 on v0.6.5 — the first witness to build a synthetic
> `--claude-home` fixture, closing a gap six prior reports had declined on a mistaken
> ethics premise; its meta-finding stands: "does the reported count match reality" keeps
> paying).
> 1. **Obviously-corrupt lines are COUNTED (correctness — the malformed law had a hole).**
>    A syntactically-invalid line carries no `"role":…` marker, so every §7 byte prefilter
>    routed it to the silent `Ignore` branch: `skipped_lines` reported 0 on a corrupted
>    file across search/list/show/stats/files/recover/verbatim — a partially-corrupt file
>    was indistinguishable from a clean one, the exact signature the law exists to rule
>    out. Every non-candidate path now runs an O(1) SHAPE check
>    (`parse::line_shape_malformed`: non-blank but not `{…}`-framed ⇒ counted): free-text
>    garbage has no leading `{`, crash-truncation loses its trailing `}` — the two
>    realistic corruption shapes. Zero §7 regression (two byte compares on non-candidate
>    lines only); verified zero false positives across a 315-transcript real corpus. The
>    documented residue: a `{…}`-framed line with an invalid INTERIOR is only counted when
>    it is a parse candidate — validating every non-candidate line would repeal §7.
> 2. **`verbatim`'s header reads `spanned K of N compaction boundaries in scope`.** K
>    (budget-window-relative) alone read as a TRANSCRIPT property — `spanned 0` on a
>    4-boundary session under a small `--budget` looked like a bug and cost the witness a
>    real debugging detour. N is the session's true total (same self-disambiguation
>    pattern as the automation `in scope (not all selected)` note). The golden baseline is
>    recaptured.
> 3. **`verbatim --format json`'s header carries the full budget accounting** —
>    `round_trip_fraction, chars_used, boundaries_spanned, boundaries_total,
>    selected_assistant` join the existing fields, so "did the reconstruction consume its
>    budget / cross the compactions" is machine-answerable without regex-parsing the text
>    header. (The witness's "JSON is strictly thinner than text" was OVERBROAD — it only
>    read the trailing summary and missed the leading header row, which already carried
>    budget/selected/automation/sidecar — but the accounting fields really were absent;
>    both facts recorded.) Help fixed alongside: the schema doc said `kind:"session_header"`
>    while the binary emits `kind:"header"` — pre-existing drift.
> 4. **Doc-only:** `stats` help/SKILL note that under `--turn`/time windowing every figure
>    windows EXCEPT `lines` (a file fact); `agents` help/SKILL state the staleness reality
>    of `awaiting-execution` (at corpus scale an hours/days-old pending lane is
>    overwhelmingly an abandoned parent session — 54 such lanes live in the audit corpus,
>    oldest 6 weeks; weigh `pending_since_utc` yourself); `--siblings` caps apply to
>    NON-matching context records only (hits always render in full).

> **v0.6.5 CHANGE LEDGER (non-breaking; correctness fix + additive JSON — `csift 0.6.5`).**
> The ninth-audit release (Sonnet 5 on v0.6.4 — its self-initiated integrity re-audit
> surfaced the round's headline bug; one suggestion was refuted as already-shipped).
> 1. **Bare ISO datetimes are LOCAL wall-clock time — the time-of-day is no longer
>    silently discarded (correctness).** `--since "2026-07-13T20:00:00"` (bare, no
>    offset/`Z`) used to collapse to local MIDNIGHT: jiff's civil-Date parser accepts a
>    full datetime string and keeps only its date part, and `parse_absolute` tried Date
>    before any DateTime arm — so a bounded window looked exactly like a quiet time period
>    (the worst silent-wrong-answer shape a time flag can produce; verified live: three
>    different times of day returned byte-identical results). A civil-DateTime arm now
>    precedes the Date arm — bare datetime ⇒ system-local wall clock, the bare-date
>    convention extended — guarded so a string CARRYING a malformed offset still bails
>    (jiff's civil parsers ignore offsets; it must never be re-read as local). One fix
>    covers every WHEN consumer (`--since`/`--until` on search/list/stats/agents/files/
>    recover/image/verbatim). The bail text + every WHEN doc now name all three forms.
> 2. **The id trio rides EVERY search hit row (additive).** Two independent audits (R6,
>    R9) tripped on bare `.hits[]` flattening yielding `session_id: null` — jq cannot fail
>    loud on a missing key, so the data now matches the natural access pattern:
>    `session_id`/`is_subagent`/`parent_session_id` on each hit (and sibling) object,
>    duplicating the exchange row's copy. The help sentence claiming "per-hit objects
>    carry no session_id" is corrected; `refetch` stays the preferred single-record path.
> 3. **Advisory notes fire AFTER target resolution.** `search "" @abc` used to print the
>    empty-pattern scope warning, then fail resolution — a warning about a run that was
>    never going to happen. Resolution now precedes the uuid-as-text note and the
>    may-emit-a-lot warning.
> 4. **Doc-only:** SKILL's JSON reference gains the missing `plan` / `recover`
>    (coverage|segment|snapshot|boundary) / `image` row schemas (transcribed from live
>    output — the R9 witness burned a round-trip guessing `plan_file` as `path`); @trap
>    marker uniqueness is stated as CONVERSATION-wide (concurrent agents collide →
>    AMBIGUOUS error, verified live by two real subagents); `show`'s help notes a "turn"
>    can be dozens of records on an agentic session (cap-protected). REFUTED, no change:
>    "stats --help lacks the ≈2× cross-reference" — it has carried it since v0.6.2 (the
>    witness never ran `stats --help`; its 5 transcript mentions were its own report
>    prose).

> **v0.6.4 CHANGE LEDGER (non-breaking; error-text + render + doc surface — `csift 0.6.4`).**
> The eighth-audit release (Sonnet 5 on v0.6.3 — the round that verified the v0.5→v0.6.3
> arc against a resurfaced pre-v0.5 critique and found the remaining gaps at the edges).
> 1. **The removed `turns` name gets a tombstone error.** `csift turns …` used to hit
>    clap's teach-nothing "unrecognized subcommand" — the one error left below the tool's
>    water line (the `-t thinking` legacy values got successor pointers in v0.6.3; the
>    LARGER rename taught nothing). A hidden `Turns` variant (bare, `#[command(hide)]`,
>    swallowing every token so a flag-parse error can never preempt the message) now
>    always bails: renamed `verbatim` in v0.5, same flags — and routes plain turn READING
>    to `show <target> --turn -3..`. Still a wall, never a shim: it never runs.
> 2. **`agents` text brands a non-completed lane's `returned_message` inline.** A frozen
>    teammate's newest returned message can READ like a clean finale ("work is complete,
>    confirming shutdown" — a real two-week frozen lane) and nearly misled a real reader
>    despite the schema note. On a lane with no status-gated `completed_utc`, the text
>    render now prints `returned (<source> · history — predates the still-open lane, NOT
>    the outcome) <msg>`; a completed lane stays unbranded. JSON is unchanged (status sits
>    beside it).
> 3. **Doc-only:** (a) an agents RUN row's `status` is the workflow journal's last word
>    VERBATIM — an open set (observed `completed`/`killed`), not a csift enum, distinct
>    from the agent row's computed status (help + SKILL note it); (b) the richest-view
>    dedup rule stated mechanically: `labels[]` is richest-first and the rendered view is
>    the FIRST label surviving `-t`/`-T` — no lookup table needed; (c) exit codes: usage
>    errors exit 2 (clap convention), csift errors exit 1 — informational, the contract
>    stays 0-vs-non-zero; (d) the RECORD-level pipeline idiom canonized in SKILL jq canon
>    (select hits in jq → run their csift-generated `refetch` commands; a primitive was
>    REJECTED — it would break `show`'s single-transcript law); (e) files-vs-search
>    routing: EDITED = `files --glob`, MENTIONED = `search -l`.

> **v0.6.3 CHANGE LEDGER (non-breaking; correctness + consistency fixes — `csift 0.6.3`).**
> The seventh-audit release (Sonnet 5's v0.6.2 review — the first witness to surface a
> CORRECTNESS-class defect; its mechanism hypothesis was wrong, the phenomenon real).
> 1. **Elicitation-sidecar GHOST-PENDING guard (correctness).** Claude Code fires NO
>    `PostToolUse` for a REJECTED AskUserQuestion/ExitPlanMode, so the recipe-3 hook never
>    writes the `resolved` marker on that path — sidecar-internal pairing alone then
>    reported the elicitation pending FOREVER (verified live: 6/6 real ghost keys across
>    4 sessions were all rejections; the ghost also DUPLICATED beside its flushed native
>    record in `search`, and its own `pairing:"paired"` was csift holding the disproof).
>    `elicitation::unresolved_pending` now cross-checks the native transcript: an
>    AUQ/ExitPlanMode pending whose `csiftKey` appears on a native record as an actual
>    `tool_use` block `id` / `tool_result` `tool_use_id` (STRUCTURAL check — a key quoted
>    in prose does not count) is dropped like a resolved pair. The native record outranks
>    the sidecar; MCP markers (no native form, non-unique keys) stay sidecar-paired only.
>    Cost: paid only when ≥1 AUQ/EPM key is sidecar-unresolved. SKILL recipe 3 also
>    subscribes `PostToolUseFailure` (belt-and-suspenders for hook-side pairing).
> 2. **`list` scope banner / JSON header report the PRE-cap range.** The unscoped
>    flood-guard capped the ROWS and then derived the banner from the capped set, so line 1
>    read "scope 50 sessions in scope" over a ~8000-transcript corpus (~160× off) — the
>    only spanning surface whose scope numbers a row cap could shrink. Banner + header
>    `sessions_in_scope` now come from the resolved pre-cap set; the summary's `sessions`
>    stays the emitted-row count and `dropped_by_cap` is unchanged.
> 3. **`--count-by label` keys pass the active `-t`/`-T` predicate.** The census counted
>    each surviving record under its FULL label set, so a dual-labeled record leaked its
>    filtered-out twin into the keys (`-t user -T user.message` censused
>    `agent.tool.result`; a `-t harness` census was dominated by
>    `agent.communication.inbox`). Keys now follow the same include-minus-exclude
>    predicate that admits the record's views; membership and record totals are unchanged,
>    and the zero-match probe still reports FULL label sets (it exists to name what the
>    dropped filter excluded). No filter ⇒ unchanged full-set census.
> 4. **`show` rejects the span pair with the rule (muscle-memory guard).** Ten sibling
>    commands accept `--no-subagents`/`--subagents`; `show` fed them to the TARGET parser's
>    "did you mistype a flag?" guess. Hidden accepted-then-rejected pair (the `agents`
>    precedent) now states: show fetches from exactly ONE transcript, never spans — target
>    a subagent by its own `@<agent-id>`.
> 5. **Legacy flat selector errors name their successor.** `-t thinking`/`tool`/
>    `tool-response` still hard-error, now with a direct pointer (`'thinking' is the
>    pre-v0.5 flat spelling — today that is agent.thinking`) ahead of the 25-value list.
>    Plus doc-only: the `-t` taxonomy help notes a `ScheduleWakeup` CALL is
>    `agent.tool.use` (arming) while `harness.schedule.wakeup` is only the FIRED
>    marker-carrying tick (a custom-prompt tick is isMeta ⇒ excluded); root-help PITFALLS
>    + SKILL document the ghost guard.

> **v0.6.2 CHANGE LEDGER (non-breaking; error-text + help/SKILL surface — `csift 0.6.2`).**
> The sixth-audit release (Sonnet 5's v0.6.1 review — the most honest witness of the
> series: zero refuted findings, all four suggestions doc/diagnostic-level; no
> previously-succeeding invocation changes behavior).
> 1. **`image --id` miss error explains itself.** The bare "`--id matched no image: #1`"
>    was the last error that said WHAT without WHY. `#N` handles are inherited from CC's
>    paste-time `[Image #N]` numbering (positional zip, image.rs), NOT a dense 1..N index —
>    a transcript's handles can start past #1 and carry holes (verified live: a real
>    session's handles run #2..#11 with no `[Image #1]`/`[Image #6]` marker anywhere in the
>    file — a source gap, not a csift drop). The miss error now names the handles PRESENT
>    (cap 24 + explicit `+N more`; unnumbered images pointed at their `L<line>i<n>`
>    locator), states the numbering provenance, and routes to the plain listing.
>    `image --help` + SKILL document the hole semantics.
> 2. **The three count units are cross-referenced where the numbers collide.** `stats`'
>    tool tallies count CALLS; `search --count-by tool` counts RECORDS (tool_use + result
>    carrier ⇒ ≈2× the call figure; an answered AskUserQuestion re-homes its carrier to
>    `user.answer`, so AUQ stays ≈1× — verified: 11/11 tools exactly 2× on a real session,
>    AUQ the sole exception). One sentence each in `stats --help`, the `--count-by` flag
>    doc, and the root-help PITFALLS (`-c` = EXCHANGES · `--count-by` = RECORDS · `stats`
>    tools = CALLS), plus a SKILL wrong-assumption row — the witness nearly filed the 2×
>    gap as a counting bug.
> 3. **Doc-only, two jq-ecosystem traps + one field-semantics note:** (a) the id trio
>    lives on the EXCHANGE row, so bare `.hits[]` flattening yields `session_id: null`
>    silently — root help + SKILL jq canon now show the merge idiom
>    (`. as $e | .hits[] | . + {session_id: $e.session_id}`) and re-route to `refetch`;
>    (b) root help JSON OUTPUT: `select(.kind==…)` BEFORE projecting, or header/summary
>    rows stamp all-null; (c) `agents` `returned_message` is the NEWEST message the child
>    EVER returned — on a frozen/running lane it predates the pending call (read beside
>    `pending_*`, never as the outcome); help + SKILL note it.
> 4. **Every subcommand's `long_about` was DEAD TEXT — now rendered (side-catch while
>    verifying item 2).** The `Command` enum's variant doc comments SHADOWED each `*Args`
>    struct's `about` + `long_about` (clap derive precedence: variant attributes override
>    the augmented struct's), so all 11 struct-level `long_about` prose blocks — the
>    long-form intros written across v0.4–v0.6 — never rendered in any `--help`. Fix:
>    variants are bare (a source comment forbids re-docking docs there); every struct
>    carries `about` (wording identical to the old variant one-liners, so the root
>    subcommand list is byte-stable — except `show`, whose struct about was itself stale,
>    predating `--turn`) + its now-visible `long_about`. The help lints
>    (`help_mentions_only_declared_flags` / examples) now scan the newly-rendered prose.

> **v0.6.1 CHANGE LEDGER (non-breaking; error-text + help/SKILL surface — `csift 0.6.1`).**
> The fifth-audit release (Sonnet 5's v0.6.0 re-review: two real findings, both fixed at
> root; no previously-succeeding invocation changes behavior).
> 1. **Unrecognized `@`-shape = hard error (targeting fail-loud closure).** The shared
>    resolver's `@`-dispatch catch-all treated ANY unmatched token as an encoded
>    project-dir name — so `@a` / `@22` / `@224` (below the 4-char uuid-prefix minimum)
>    silently stripped the `@`, fell through to cwd-relative path resolution, and reported
>    a misleading "no Claude Code project dir for …/a" (a filesystem debugging trail for an
>    ID typo; the one spot the fail-loud targeting law was violated). The encoded-dir arm
>    now requires the `-`-leading encoded shape (`@-Users-…` still works); every other
>    unrecognized `@`-token hard-errors naming the @-grammar, with a dedicated "too short
>    for a session-uuid prefix — needs 4-11 leading (dashless) hex chars" message for a
>    1-3-char hex token. `--sessions-from` already bailed on non-id tokens (unchanged).
> 2. **`@trap` main-thread timing documented + error routes the fix (verified live).** The
>    trap premise "CC records the tool_use before it runs" holds ONLY for a subagent's own
>    transcript (eager flush — first try resolves, re-verified); the MAIN conversation's
>    record is flushed AFTER the current Bash call completes (a 3s in-command sleep still
>    saw 0 on disk), so a top-level FIRST use ALWAYS misses and only re-running the SAME
>    marker (now in the flushed previous record) resolves. *(v0.8.2 correction: refuted —
>    the main-lane write is an async flush landing ~1-3.4s after dispatch; that 3s probe
>    sat exactly at the window's edge and had no resolving power. The observed behaviors
>    and the shipped routing stand.)* The no-match error now states
>    the timing split and routes both paths (`@main` for the main thread — env-based, no
>    race; re-run the SAME command+marker otherwise; a FRESH marker restarts the race).
>    SKILL/`--help` document it; the `whoami` SUBAGENT CAVEAT is corrected to current CC
>    (an Agent-tool subagent's env holds the PARENT id — own id withheld; older builds
>    handed a Task subagent its own id).
> 3. **Doc-only:** `--count-by model` documents the raw `<synthetic>` key (a CC-fabricated
>    stand-in assistant record, e.g. an API-error notice — reported verbatim); a new
>    pitfall row: text-mode excerpts keep LITERAL newlines, so `| head -N` can cut
>    mid-record — the line-safe machine form is `--format json`.

> **v0.6.0 CHANGE LEDGER (authoritative — supersedes any conflicting older text).** The
> fourth-audit release (a Sonnet 5 hands-on round surfaced one real parser trap and two
> field-truthfulness gaps; every fix is root-cause, none is a doc workaround). Preceded by
> v0.5.2, a help-only PATCH (search COUNT section: "integer EXCHANGE total" + `-l` routing)
> that carried no ledger of its own.
> 1. **`show` bad-flag attribution (bug fix, error-text surface).** `show`'s TARGET was the
>    only single non-Vec positional in the tree; with `allow_hyphen_values`, a mistyped or
>    foreign `--flag` was consumed AS the target and clap blamed the user's VALID target as
>    the surplus token ("unexpected argument '@…'") — the wrong-hypothesis rabbit hole.
>    TARGET is now a Vec (`num_args=1..`) with the run handler enforcing exactly-one, so a
>    bad long flag in ANY position falls into `parse_project_target` and is rejected BY
>    NAME with the same "did you mistype a flag?" error every sibling command gives; two
>    real targets get a pointed "show reads exactly ONE transcript per call" arity error
>    (addresses are per-FILE). The old-spelling did-you-mean tips (`--turn-range` → `--turn`)
>    are unchanged.
> 2. **Census counts RECORDS, not section hits (bug fix, §6.2/§8.2 numbers change).** One
>    record can emit several per-section hits (a batched notification, a text+tool_use
>    assistant record); `--count-by` and the zero-match label probe counted each hit, so a
>    leaf tally could drift ABOVE what `-t <leaf>` surfaces (the documented invariant) and
>    comm-heavy scopes inflated hard (a real 8-signal record counted 8×). All censuses now
>    group hits per record (`record_groups`); the axis value is the first `Some` among the
>    record's section hits; `matched record(s)` wording is now literally true.
> 3. **Pairing rides the tool BLOCK through communication views (§6.2 pairing domain
>    widened).** `pairing` is a property of the tool_use/tool_result join, but it was only
>    populated on `agent.tool.*`-classed hits — a SendMessage/spawn whose richest view is
>    `agent.communication.sent`/`.signal` (and a subagent-return `…inbox` on a tool_result)
>    fell OUTSIDE the pairing census. The frozen-SendMessage lane (the dominant stuck shape
>    in a teams session) is now `pending` under ANY selector: "any pending tools?" =
>    `csift search "" T --count-by pairing`, no `-t` needed. Record-text comm units (an
>    inbound teammate-message, an idle signal section) carry no tool_use_id and stay
>    outside the axis, reported as excluded.
> 4. **`agents` `completed_*` is status-gated + new `last_activity_*` pair (BREAKING, agent
>    row).** `completed_utc/_local` (and `duration`) are non-null ONLY when
>    `status:"completed"` — a frozen teammate carried `completed_utc == pending_since_utc`,
>    a false "done" for any name-driven consumer (the TEXT tree had suppressed the
>    misleading line since v0.5.0; JSON now agrees). Every timestamped lane carries the new
>    `last_activity_utc/_local` pair (the tail newest-record instant; == `pending_since_*`
>    on a frozen lane). TEXT prints `completed`+`duration` only on a completed lane and
>    `last-seen` on a running-not-frozen one (a frozen lane keeps its PENDING line as the
>    sole instant carrier). `--order-by completion` and its `--since/--until` window run on
>    the terminal instant (`last_activity`) — numerically identical to the old behavior, so
>    frozen lanes still window on their freeze instant.
> 5. **Help drift fixed in passing:** the `search --help` JSON hit-field list now includes
>    `pairing` (it was always emitted).

> **v0.5.1 CHANGE LEDGER (non-breaking; help-text surface only — `csift 0.5.1`).** The
> `--help` parity release: the FIVE-DOCUMENT CONTRACT is now `SKILL.md` = the LLM manual ·
> `--help` = the human (CLI-proficient) manual, INFORMATION-PARITY with SKILL but written in
> plain prose with stronger structure/layout · `README.md` = promotion · `SPEC.md` = design
> intent · `AGENTS.md` = maintenance guide. Behavior is unchanged; only help text moved:
> 1. Root `csift --help` gains five human-toned sections: THE RULES EVERY COMMAND FOLLOWS
>    (exit codes / ranges / subagents / caps / time — the SKILL "five laws" in prose), JSON
>    OUTPUT (the envelope contract + jq idiom + id trio + `refetch`), PITFALLS WORTH KNOWING
>    UP FRONT (the SKILL wrong-assumptions table in prose), WHAT csift WILL NOT DO, and
>    RETENTION (`cleanupPeriodDays`).
> 2. `search --help` gains THE LABEL TAXONOMY — the full 3-role / 25-leaf tree with per-leaf
>    one-liners, selector grammar, richest-view rule and the glyph legend (it previously
>    existed only in SKILL/SPEC).
> 3. `show`/`stats`/`plan`/`whoami`/`image` `--help` gain JSON SCHEMA sections (fixture-
>    verified row/summary fields), completing per-command schema coverage (list/search/
>    agents/files/recover/verbatim already had one).
> 4. Every `--sessions-from` help (×9) now states that resolved ids follow the command's
>    normal SPAN rules (span-by-default commands expand each session to its subagents; add
>    `--no-subagents` to pin).
> 5. `whoami --help`'s composition example is fixed for the v0.5.0 flat envelopes (it
>    consumed `agents` JSON with bare `jq -r .field`; now `select(.kind=="identity")`).

> **v0.5.0 CHANGE LEDGER (authoritative — supersedes any conflicting older text).** The
> 0.5.0 rework (0 backcompat; `csift 0.5.0`). Items 1–7 + 11 are BREAKING:
> 1. **The per-command turn-window flag is renamed to `--turn`** on every command that had it
>    (`search`/`stats`/`files`/`recover`/`verbatim`/`image`; the old name carried a `-range`
>    suffix) — SAME range grammar (`N`/`A..B`/`N..`/`..N`/`-k`/`..`), same AND-intersection with
>    `--since`/`--until`. `show --turn` is unchanged. The parse-error label is now `--turn`; the
>    `files` text footer fragment now reads `turn=SPEC`.
> 2. **The label census is generalized to `--count-by <AXIS>`** (search terminal mode; the old
>    flag was label-only). Closed axes: `label` | `tool` | `turn` | `session` | `pairing` |
>    `model`. `label` MULTI-counts (a record counts under every leaf it carries); the other axes
>    count each record ONCE, and records outside an axis's domain (no tool / pairing / model) are
>    EXCLUDED with the excluded count reported. `turn` sorts ASCENDING (a histogram; keys `t<N>`,
>    `<full-transcript-id>·t<N>` when >1 transcript in scope); the others sort count-DESC. JSON:
>    the old per-leaf census row kind is REPLACED by kind **`census`** `{axis, key, records}`;
>    summary `{axis, matched_records, distinct_keys, excluded_records, dropped_by_cap,
>    skipped_lines}`. Still conflicts with `-c`/`-l`/`--siblings`/`--raw`. The empty-result
>    diagnosis recommends `--count-by label`.
> 3. **`agents --format json` is FLAT** (envelope v2, NO exceptions — the old nested
>    `{session_id, workflow_runs:[…children…], agents:[…]}` per-session object AND the bare-node
>    `--agent` special case are BOTH gone). Now: header → per session a light
>    `{kind:"session", session_id, runs:N, agents:N}` row → each in-scope workflow run a
>    `{kind:"run", …run fields…, session_id}` row (NO `children`) → its member `{kind:"agent",
>    …node fields…}` rows in tree PRE-ORDER → built-in agents pre-order → `{kind:"summary",
>    sessions, runs, agents}`. Tree NESTING is TEXT-mode only; JSON consumers rebuild it from
>    `parent_agent_id`/`depth`. A node unreachable from any root (a forged parent cycle) is
>    APPENDED, never dropped (the old nested shape silently dropped such nodes).
> 4. **`show --turn` address-miss is a HARD error.** An EXPLICIT `--turn N` or `--turn A..B`
>    resolving to zero records errors naming the domain (`no such turn(s): t99 — the transcript
>    has 2 turn(s) (t0..t1); …`); open/from-end forms (`N..`/`..N`/`-k`/`..`) CLAMP (tail-peek
>    robustness). Both rendered and `--raw` modes.
> 5. **`show` flood guard.** Default cap `DEFAULT_SHOW_CAP = 200` record units (keep-FIRST); the
>    text drop line is `+N more record unit(s) beyond the {cap}-unit cap · continue: csift show
>    @<id> --line A..B  (or --max-count 0 = uncapped)`; JSON summary gains `dropped_by_cap`,
>    `refetch_remainder`, `non_record_lines`. New `show --max-count N` flag; `--raw` caps by LINE
>    with an equivalent stderr note. A line-RANGE covering non-record lines prints `N line(s) in
>    the addressed range are not records (metadata/attachment — inspect with --raw)`.
> 6. **`--max-count 0` = uncapped, uniformly** on `list`/`stats`/`search`/`show` (previously
>    `Some(0)` was a literal zero-cap absurdity).
> 7. **Timestamps (text): ONE canonical form** — `YYYY-MM-DD HH:MM:SS[.mmm] <TZAB>(UTC±offset)`
>    (`2026-07-11 15:33:37 AEST(UTC+10)`). The OLD dual form `… AEST (2026-…Z)` and the bare
>    `…+10:00` form are GONE (the UTC copy invited timezone-math errors). `timez::format_timestamp`
>    (seconds) + `format_local_compact` (ms) share one renderer + `tz_marker`. JSON `ts_utc`/
>    `ts_local` unchanged.
> 8. **`files` JSON summary gains `sessions`** (distinct owning sessions among emitted rows);
>    deliberately NO `dropped_by_cap` (files has no cap).
> 9. **`verbatim` self-diagnosis** — a non-`--slice` run prints, per session with ZERO
>    compaction summaries, the stderr note `csift: note: @<id> has no compaction — nothing was
>    clipped; for plain reading use \`csift show @<id> --turn <N|A..B|-k..>\` (full records, no
>    budget)`.
> 10. **`list` rows gain `sidecar_present: bool`** (the elicitation sidecar FILE exists = hook
>    installed; tri-state: present+pending = blocked / present+none = provably not blocked /
>    absent = cannot conclude).
> 11. **Slash-command wrapper — BOTH tag orders.** Current CC emits `<command-message>` FIRST
>    (verified: 14 new-order sessions vs 35 old-order). Detection is `is_slash_command_wrapper`
>    (either leading tag; new `COMMAND_MESSAGE_PREFIX`). `is_genuine_user` now excludes BOTH
>    orders (a new-order wrapper no longer masquerades as human prose or opens a turn — turn
>    NUMBERING may shift on transcripts with new-order wrappers; a correctness fix). `classify`:
>    a with-args wrapper → `[user.message, harness.command.invocation]` with `user.message` FIRST
>    (richest-view law), so the unfiltered render is the extracted `/name args` (`slash_command_name`),
>    never wrapper XML; a no-args wrapper → `harness.command.invocation` only. An explicit
>    `-t harness.command.invocation` still renders the raw wrapper.
> 12. **`pairing` JSON enum documented:** `paired` | `pending` | `orphan` | `null`.
> 13. **`normalize_argv` fix (P0):** the subcommand is located by SCANNING over declared root
>    flags (+ their value tokens; `--flag=value` spans one token), so `csift --claude-home DIR
>    list @x --max-count 3` works — previously a pre-subcommand global flag disabled normalization
>    and any flag after a positional was swallowed by the PATH positional with a misleading "not a
>    project target" error. "Flag order is free" and "`--claude-home` any position" now hold in
>    combination.

> **v0.4 CHANGE LEDGER (authoritative — supersedes any conflicting older text).** The
> 0.4 rework (0 backcompat; `csift 0.4.0`):
> - **`turns` → `verbatim`** (rename, zero-BC — old `csift turns` is now an UNRECOGNIZED
>   subcommand; the module stays `src/turns.rs`, the handler is `run_verbatim`). `verbatim`
>   is REFRAMED as the compaction-fidelity SPECIALIST: it restores the verbatim turns a
>   COMPACTION SUMMARY clipped — NOT the tail-peek tool. The "read a session's recent turns
>   from the live transcript" intent moved to **`show --turn`** (below). The subcommand
>   COUNT is still eleven.
> - **`show --turn N|A..B|-k`** (§6.11): a THIRD addressing mode (mutually exclusive with
>   `--line`/`--uuid`) that fetches EVERY record of the named turn(s); the turn numbering is
>   IDENTICAL to `search`'s `s1·tN`. `show --turn -3..` = the last 3 turns (the tail-peek /
>   live-monitoring intent).
> - **Range grammar extended** (`text::parse_range_spec` + `RangeSpec::resolve`, replacing
>   `parse_range`). EVERY range flag (`show --line`, the per-command `--turn` window, and
>   `--file-lines`) now
>   accepts `N` · `A..B` · `N..` (to the end) · `..N` (from the start) ·
>   `-k` = the k-th from the end (`-3..` = the last 3, `-1` = the last) — all inclusive. The
>   dash form `A-B` still hard-errors (teaches `..`); a statically reversed closed range
>   (`9..3`) errors. Open / from-end forms resolve PER TARGET (the last 3 turns of EACH
>   session). The space form `--turn -3..` parses (`allow_hyphen_values`).
> - **`search --count-by <AXIS>`** (see the v0.5.0 ledger for the rename):
>   a per-KEY census of the matched records along one closed axis (empty PATTERN =
>   a whole-scope census; on the `label` axis a leaf's count = how many records `-t <leaf>`
>   would surface). JSON emits `census` rows.
> - **Empty-result self-diagnosis** (search): a zero-match run prints a stderr diagnosis —
>   "a DEFINITIVE absence (exit 0), NOT an error", the active filters, and (when `-t`/`-T`
>   was on) an ACTIVE PROBE naming the label(s) the pattern DOES occur under. JSON summary
>   gains `definitive_absence` / `active_filters` / `excluded_by_label`.
> - **Flood guards**: **`list`** DEFAULT-caps an UNSCOPED (all-projects) listing to the 50
>   most-recently-active rows (drop reported: text footer + JSON `dropped_by_cap`;
>   `--max-count` overrides; a scoped query — a target / `--sessions-from` — is uncapped).
>   **`stats`** gained an opt-in `--max-count`. `files`/`agents` stay scope-bounded.
> - **JSON field rename** (search summary): `session_ids` → **`transcript_ids`**,
>   `session_ids_truncated` → **`transcript_ids_truncated`** (named apart from `-l`'s
>   owning-session id stream).

> **v0.3 CHANGE LEDGER (authoritative — supersedes any conflicting older text).**
> - **ONE range-token grammar** (extended in v0.4): every range flag (`show --line`/`--turn`,
>   the per-command turn window — v0.5 `--turn` — and `--file-lines`) takes `N` · `A..B` · `N..`
>   (to the end) · `..N` (from the start) · `-k` = k-th from the end (`-3..` = the last 3),
>   resolved per-target; the dash form `A-B` is REMOVED (hard error teaching the `..` spelling),
>   a reversed closed range errors. The turn window + `--since`/`--until`
>   now INTERSECT on EVERY command (`files`/`verbatim` dropped their leftover
>   mutual-exclusion bails).
> - **`-T`/`--label-not`** (search): label EXCLUSION, same selector grammar as `-t` (the
>   rg -t/-T duality); richest-SURVIVING-view dedup; statically-empty combos hard-error.
> - **`--sessions-from <FILE|->`** on every multi-target command (list/search/stats/
>   agents/files/recover/plan/verbatim/image): union an id list into the scope; empty list =
>   empty scope. **`search -l`** emits the matching OWNING session ids (uncapped) to pipe
>   into it. **`search --raw`** emits matched records' verbatim jsonl lines (stdout pure,
>   notes on stderr).
> - **`refetch`**: search JSON hits + verbatim `collapsed_agents` rows carry the ready-to-run
>   `csift show` command addressed at the line-owning transcript.
> - **`verbatim` REQUIRES a target** (budget × every-session flood guard). **`list`** gains
>   `--since`/`--until` (activity-span intersection); **`stats`** gains the turn window (v0.5 `--turn`)
>   (and its positional accepts encoded `-Users-…` dirs — an `allow_hyphen_values`
>   omission fixed).
> - **Teammate ids with dashed NAMES round-trip** (`aP1-engine-9cf2…`): the
>   `is_teammate_agent_id` head accepts `[A-Za-z0-9-]` with an explicit `!is_uuid` guard.

> **v0.2 BREAKING-CHANGE LEDGER (authoritative — supersedes any conflicting older text
> in this section).** The 0.2 ergonomics rework (0 backcompat, one-way-per-intent):
> - **`show` (new, §6.11)** owns record FETCHING; `search --line/--uuid` (and the
>   `<hex>:<spec>` pin) are REMOVED. `show --raw` emits verbatim jsonl line bytes.
> - **`stats` (new, §6.12)**: one-scan per-session aggregates (tokens by model, tool
>   counts, turns, span, compactions).
> - **envelope v2 (§8.2)**: every `--format json` stream = ONE header line +
>   kind-tagged rows + ONE summary line, no exceptions; the jsonl-line key is `line`
>   everywhere; a boundary's classifier is `cause`; files' grouping key is always
>   `path`; agents' bare-node `--agent` JSON special case is gone; whoami emits
>   `identity` rows; verbatim's `skipped_lines` trailer folded into `summary`.
> - **Flags**: `-t` long form is `--label` (category/selector vocabulary retired);
>   `agents --kind` → `--shape` (+ node field `shape`); `recover --line-range` →
>   `--file-lines`; span switches are the uniform pair `--subagents`/`--no-subagents`
>   (verbatim's `--include-subagents` gone; agents rejects both); `verbatim --budget-unit` +
>   the five tuning knobs (`--agent-run-threshold`, `--agent-rich-min-chars`,
>   `--agent-declaration-max-chars`, `--keep-first`, `--no-keep-first`) are REMOVED
>   (`--budget` is chars; `--profile heavy|light` is the whole tuning surface);
>   `--siblings` is a zero-arg fixed policy (message units always; thinking≤2,
>   tool.use≤3, tool.result≤3, harness≤2; overflow surfaces an explicit
>   `(+N more · csift show …)` pointer + JSON `siblings_hidden`/`turn_lines`);
>   `image --id` takes bare digits or `L<line>i<n>` (the `#N` input form errors).
> - **Guardrails**: a bare id target errors "did you mean '@<id>'?"; a search PATTERN
>   starting `@` errors; a uuid-shaped PATTERN notes on stderr; the turn window (v0.5 `--turn`)
>   now INTERSECTS `--since`/`--until`.

> **Common conventions.** Every TEXT timestamp is a SINGLE canonical local form — `YYYY-MM-DD HH:MM:SS[.mmm] <TZAB>(UTC±offset)` (e.g. `2026-07-11 15:33:37 AEST(UTC+10)` on a machine in Sydney; January Sydney = `AEDT(UTC+11)`; India = `IST(UTC+05:30)`), via `jiff` (`TimeZone::system()`). The marker is a FORMAT not a value: the abbreviation + offset are derived from the system zone at that instant (DST-correct), so the only mental step is "shift by the given offset"; a whole-hour offset renders compact (`UTC+10`), a fractional one zero-padded (`UTC+05:30`), and a zone with no abbreviation renders `(UTC±offset)` alone. **There is NO raw-UTC parenthetical in text** (the former `… AEST (…Z)` dual form and the bare-offset `…+10:00` form are GONE — the UTC copy invited LLM timezone-conversion errors); machine UTC lives ONLY in JSON (`ts_utc`, §8.2). All subcommands accept `--format text|json` (default `text`). Text output is headered and LLM-friendly; JSON is one object per emitted unit, deterministic order. Errors go to stderr with the full `anyhow` chain; exit code 0 on success, non-zero on error. **No silent truncation** — any cap reports the drop count.

### 6.1 `list` — "which session is this?" fast index

**Purpose.** For each session under the target(s), emit the quick-identity tuple without parsing the whole file.

**Clone lineage (v0.9.4).** A background-job fork copies a whole session file at a compaction point (record uuids preserved, timestamps predating the file, slug stripped). Detection law, measured with zero false positives on a real 61-file dir: a transcript whose FIRST TIMESTAMPED record is a system/`compact_boundary` is a clone (birthtime rules were refuted — filesystem copies and migrations move birthtimes). The row annotates `clone    forked from SESSION <origin> at compaction boundary <uuid>`; the ORIGIN join memmem-scans the project dir's siblings for the boundary uuid and demands the actual RECORD (uuid match + compact_boundary) that is NOT the sibling's own head (a prose quote parses to a different uuid; a co-clone's head probe returns the same boundary) — paid only on detection, near-free per row otherwise (the probe early-exits at the first timestamped record). JSON: `is_clone`, `clone_of` (null when the origin left the dir), `clone_boundary_uuid`. Corollary on every spanning surface: a clone DOUBLE-COUNTS its inherited records until scoped away (`search -l` lists both ids; `stats` folds the copied turns/tokens) — detection is the disclosure; a dedup engine is deliberately out of scope this release.

**Args (matches `cli::ListArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | a real cwd OR an encoded dir (§2.3); 0 args ⇒ every dir under projects root (default-capped — see `--max-count`) |
| `--since WHEN` / `--until WHEN` | string | none | keep a session iff its [first-activity, last-activity] SPAN (the head/tail timestamps this index already reads — never a full scan) INTERSECTS the window, so a long-running session straddling the window still lists; a session with no readable timestamp never matches a bounded window |
| `--sessions-from F` | path or `-` | none | union an id list into the scope (the shared §6.2 semantics) |
| `--max-count N` | usize | 50 for an UNSCOPED run, else unlimited | cap the emitted rows. An UNSCOPED (all-projects, no target / `--sessions-from`) listing floods the reader's context, so it DEFAULT-caps to the **50 most-recently-active** sessions (kept by max first/last-user + last-agent ts, then restored to path order); the drop is REPORTED (text footer + JSON `dropped_by_cap`), never silent. A scoped query (any target or `--sessions-from`) is UNCAPPED unless `--max-count` is passed; an explicit `--max-count` overrides the default in both cases. **`--max-count 0` = UNCAPPED** (the crate-wide convention on `list`/`stats`/`search`/`show`), NOT a zero-row cap. |
| `--format text\|json` | enum | `text` | output format |
| `--no-subagents` | bool | — | restrict to top-level `<uuid>.jsonl` sessions only. Subagent transcripts (built-in `subagents/agent-<hex>.jsonl` + workflow `subagents/workflows/wf_*/agent-<hex>.jsonl`) are listed by default; the uniform `--subagents` twin affirms the default. Workflow `journal.jsonl` is excluded (not a transcript). |

**Per-session fields emitted:** `session-id`, **first** genuine-user message (+ts), **last** genuine-user message (+ts), **last** agent message (+ts), the session `cwd` (decoded from data, §2.4), `version`, `gitBranch`, and `sidecar_present` (JSON `bool`) — whether the elicitation-sidecar FILE exists for the session (i.e. the CC hook is installed). It is a TRI-STATE for "is this session blocked on a human?": `sidecar_present:true` + ≥1 pending elicitation = blocked; `true` + none = provably NOT blocked on an elicitation; `false` = the hook is not installed, so "nothing pending" is NOT concludable. Each message is a one-line excerpt (truncated with an explicit `… (+N chars)` marker — never silent).

**Algorithm (NON-FUNCTIONAL: must be fast on 200 MB+):**
1. Resolve target dir(s) (§2.3); enumerate `*.jsonl` directly under each (skip childless dirs, §1).
2. `rayon` `par_iter` across all session files (across-file parallelism, §7e).
3. Per file: **head-read** forward in 64 KiB chunks from offset 0, lazy-parsing only candidate lines, until the **first** `is_genuine_user` record is found (typically within the first few KB). **Tail-read** by seeking from EOF backward (§7b) until **both** the last genuine-user and last assistant-`text` (agent) records are found.
4. Collect into an order-stable `Vec`, render.

**Text output example:**
```
SESSION  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
  cwd      /Users/testuser/Projects/widget_app_prototype   (branch main, CC 2.1.159)
  first ◂  2026-06-07 14:01:09 AEST(UTC+10)
           Write the AUTHORITATIVE SPEC.md for csift…
  last ◂   2026-06-07 15:48:22 AEST(UTC+10)
           also add a --no-subagents flag please
  last ▸   2026-06-07 15:49:10 AEST(UTC+10)
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
| `--label SEL` | `-t` | repeatable selector | all labels | a dotted `role.class.sub` selector (§5), dot-segment-prefix matched (`-t agent` = the role, `-t agent.tool` = use+result, `-t harness.notification` = all five pulse kinds). The old flat values `thinking`/`tool`/`tool-response` HARD-error (0 back-compat). |
| `--label-not SEL` | `-T` | repeatable selector | none | EXCLUDE labels matching the selector (same grammar/validation as `-t`; the rg `-t`/`-T` duality). Effective set = (`-t` selectors, or ALL) minus (`-T` selectors); a multi-label record renders under its richest SURVIVING label; a statically-empty combination hard-errors. Filters HITS only — `--siblings` rendering is unaffected. |
| `--ignore-case` | `-i` | bool | smart-case | force case-insensitive |
| `--multiline` | — | bool | false | let `.` cross newlines / multiline mode |
| `--turn N\|A..B\|N..\|-k` | — | string | none | inclusive 0-based turn window in the shared §6 range-token grammar (`N` ≡ `N..N`; `N..` to the end; `..N` from the start; `-k` = k-th from the end, so `-3..` = the last 3 turns); INTERSECTS (AND) `--since`/`--until`. |
| `--since WHEN` / `--until WHEN` | — | string | none | time bounds (ISO8601 or relative `2h`/`3d`/…, system-local; bare date ⇒ local midnight) |
| `--max-count N` | — | usize | none (**unlimited — no cap**) | cap emitted exchanges; **reports dropped count**. `--max-count 0` = uncapped (the crate-wide convention). |
| `--count-only` | `-c` | bool | off | print ONLY the match total — one integer counting EXCHANGES, not lines (ripgrep `-c` idiom); honors every filter. The total is ALSO in the normal footer, so `--count-only` just isolates it for a pipe. |
| `--sessions-with-matches` | `-l` | bool | off | print ONLY the distinct matching OWNING sessions (`parent_session_id`), one per line, sorted, UNCAPPED (a `--max-count` drop notes on stderr). Pipes into `--sessions-from -`. (Per-transcript detail = the JSON summary's `transcript_ids`, ≤100 + `transcript_ids_truncated` — named apart from this owning-session stream.) |
| `--count-by AXIS` | — | value-enum | off | instead of exchanges, print a per-KEY CENSUS of the matched RECORDS along ONE closed axis (a record whose several sections match still counts ONCE — `record_groups` groups per-section hits back into records) — `<count>  <key>` per line (stderr carries the record accounting). The axis is a CLOSED value-enum, NOT a query language: `label` (per role.class.sub leaf — a record counts under EVERY leaf it carries, so a leaf's count = how many records `-t <leaf>` would surface; the exploration on-ramp) · `tool` (per tool name) · `turn` (per turn, ASCENDING turn order — a histogram; keys `t<N>`, or `<full-transcript-id>·t<N>` when >1 transcript is in scope) · `session` (per transcript) · `pairing` (`paired`\|`pending`\|`orphan`, joined by `tool_use_id`; the join rides the tool BLOCK through the communication views — a SendMessage/spawn whose richest view is `agent.communication.sent`/`.signal`, and a subagent-return `…inbox` on a tool_result, are IN the domain, so a frozen SendMessage is `pending` with no `-t`; record-text comm units carry no tool_use_id and are excluded+reported — the one-command "any pending tools?") · `model` (per assistant model) · `attachment` (per attachment payload type; IMPLIES the `--attachments` gate) · `version` (per CC version stamp) · `result` (tool results `ok`|`error` - pairing answers "did a result come back", result answers "was it good"). `label` MULTI-counts; every OTHER axis counts each record ONCE and EXCLUDES records outside its domain (no tool/pairing/model), REPORTING the excluded count (never silent). `label` sorts richest-first; the others count-DESC except `turn` (ascending). An EMPTY PATTERN = a whole-scope census. Honors `-t`/`-T`/time/turn/scope; a THIRD terminal mode (conflicts with `-c`/`-l`/`--siblings`/`--raw`). JSON: a `census` row per key (`{axis, key, records}`) + a `{axis, matched_records, distinct_keys, excluded_records, dropped_by_cap, skipped_lines}` summary. |
| `--raw` | — | bool | off | emit each matched record's VERBATIM jsonl line (deduped per record) instead of the rendered exchange — `show --raw`'s escape hatch on search's whole filter surface. stdout = pure jsonl; scope/drop/malformed notes → stderr; sidecar-merged records (no physical line) are omitted with a stderr note. Excludes `--format json`/`--siblings`/`-c`/`-l`/`--no-truncate`. |
| `--siblings` | — | bool | off | render each matched turn's NON-matched records (the surrounding back-and-forth) under a FIXED zero-arg policy: message units always (user.*, agent.message, agent.communication.*); thinking≤2, thinking.narration≤1, tool.use≤3, tool.result≤3, harness≤2 per leaf; overflow surfaces an explicit `(+N more · csift show @<id> --line A..B)` pointer + JSON `siblings_hidden`/`turn_lines` |
| `--no-truncate` | — | bool | off | emit each record's FULL text instead of the centered ~400-char excerpt (no `--full` alias — that spelling was removed as ambiguous). When NOT set and ≥1 excerpt is clipped, a trailing reader-caution prints (text) / `excerpts_truncated:true` (JSON): an excerpt is a match-centered FRAGMENT, not a summary, and can misrepresent the record's full intent — re-fetch via `--no-truncate` or `csift show` (§6.11; every JSON hit's `refetch` is the ready-to-run command) |
| `--sessions-from F` | — | path or `-` | none | UNION the scope with an id LIST (whitespace-separated uuid/prefix/agent-id tokens, bare or `@`-prefixed — what `-l` emits); per-id fail-loud; an explicit EMPTY list = an empty scope (honest empty), never all-projects. Available on every multi-target subcommand. |
| `--resolve-persisted` | — | bool | false | resolve `<persisted-output>` pointers (§4.6); under `--raw` it affects MATCHING only (the emitted line is the original) |
| `--no-subagents` | — | bool | — | search only top-level `<uuid>.jsonl` sessions. Each in-scope session's subagent transcripts (built-in + workflow / OMC agents under `subagents/**`) are searched by default; this is the only span flag. Workflow `journal.jsonl` is never searched (not a transcript). |
| `--format text\|json` | — | enum | `text` | output format |

**Addressing (record fetch) moved to `show` (§6.11).** `search` FINDS (match-centered excerpts, round-trip exchanges); `csift show TARGET --line N|A..B / --uuid U` FETCHES the records you name, full or `--raw`. Every JSON hit carries `refetch` — the ready-to-run `show` command already addressed at the transcript that OWNS the hit's line number (its `session_id`; a parent uuid + a subagent line number would silently fetch the wrong record).

**Smart-case rule:** pattern is case-insensitive iff it contains no uppercase letter; `-i` forces insensitive regardless; the two never conflict (`-i` wins). Compile via `regex::bytes::RegexBuilder` (match on raw line bytes pre-JSON in the prefilter, then on decoded text for excerpting). `--multiline` sets `.dot_matches_new_line(true)` + multiline mode.

**Regex dialect — linear-time (RE2-class).** The pattern is the Rust `regex` 1.12 crate (`regex::bytes`), which **guarantees linear-time matching** in the input length: **no catastrophic backtracking, ever**. *Supported:* literals; character classes `[...]` / `[^...]` / `\d \w \s` + Unicode classes `\p{...}`; alternation `|`; groups `(...)` / non-capturing `(?:...)`; quantifiers `* + ? {m,n}` (greedy + lazy `*?`); anchors `^ $ \b \B`; dot `.` (`--multiline` lets it cross newlines); inline flags `(?i)(?m)(?s)(?x)`; Unicode-aware by default. ***Deliberately NOT supported*** (they require a non-linear engine): backreferences `\1`; lookahead/lookbehind `(?=) (?!) (?<=) (?<!)`; atomic groups / possessive quantifiers `(?>...)` / `a*+`. A pattern using these **fails to compile** with a clear error — by design, not a bug. This boundary is documented identically in `--help` (`search`'s `after_help`) and `SKILL.md`.

**Validation:** `--turn` and `--since`/`--until` INTERSECT (AND) — the one windowing rule shared by every subcommand. A `-t`/`-T` combination whose effective label set is statically empty is a hard error. An empty `PATTERN` with no other filter is allowed (matches every label-eligible record) but warns (`empty pattern with no category/time/turn/session filter …`) that it will emit a lot.

**Zero-match self-diagnosis (the anti-slippage keystone).** A zero-result search is an ANSWER — a DEFINITIVE absence — not a syntax error, and must never be misread as one. A run that emits nothing prints a diagnosis to **stderr** (stdout stays a pure/empty stream): *"csift: 0 matches — a DEFINITIVE absence (exit 0), NOT an error. Scope: N session(s). Active filters: &lt;the `-t`/`-T`/`--since`/`--until`/`--turn` echo, or `none`&gt;."* Then, when a `-t`/`-T` filter was active AND the pattern DOES occur under OTHER labels, an **active probe** names them: *"⚠ but "&lt;pattern&gt;" DOES occur — R record(s) under: &lt;leaf ×n · …&gt;. Your -t/-T excluded them; drop -t/-T or select one of those labels."* (the exact `-t user.message` searching-a-tool-name trap). When the label filter was active but the pattern is genuinely absent even unfiltered, it says so; with no label filter it points at `csift search "" <target> --count-by label`. JSON echoes this in the summary: `definitive_absence:true`, `active_filters`, and `excluded_by_label` (the per-leaf rows + record total, or null).

**Filter application order per session:** target selection (positional PATH/`@<uuid>`/`*.jsonl`) → label eligibility (§5) → time/turn window → regex match → round-trip reconstruction (§6.4) → `--max-count` cap (with drop accounting).

**Time window:** `--since`/`--until` compare against each record's `timestamp` (UTC). Records with no timestamp (metadata/noise) are never time-matched. Relative forms (`2h`,`3d`,`90m`) are resolved against *now* in the system-local timezone then converted to UTC for comparison.

**Output — complete round-trip:** see §6.4. Each emitted unit is one **Exchange** with a session header, turn index, and the matched hit(s) shown in context of their full round-trip.

**Text output example.** Every exchange header opens with a STABLE id-prefix token (`<tok>·t<turn>`, `<tok>` = the first 8 chars of the owning transcript id — an `@` target as-is; within-output collisions lengthen the group 8 → 12 → full; a subagent header adds `(parent <first-8>)`) + a single compact local instant (the offset already pins it — no second UTC copy); hit lines are `  <glyph> <label>[ <tool>]  L<line>  <excerpt>` where `<glyph>` is the ROLE glyph (`◂` user · `▸` agent · `⚙` harness) and `<label>` is the full dotted leaf path, DECORATED with `from ⇨ to` on a comm hit and the `▹` pairing on a tool hit:
```
matches  1 exchange · 1 session · oldest first

0a1b2c3d·t47  2026-06-07 14:32:05.478 AEST(UTC+10)
  ◂ user.message  L990  why is the tail-read carry needed?
  ▸ agent.thinking  L994  The carry holds an incomplete line straddling a chunk boundary…
  ▸ agent.message  L1003  The carry is the partial line at the low-offset edge of each chunk…
  ▸ agent.tool.use ▹ agent.tool.result  Bash  L1005  rg -n carry src/parse.rs…
  ▸ agent.communication.inbox  VSMultiRegion ⇨ self  L1018  please double-check the chunk math
  ⚙ harness.notification.background-command  L1020  [background-command wf12abc completed] echo done

matched 1 exchange · 1 session · label=all
```
(A subagent exchange header carries its parent on EVERY row: `<tok>·t<N> (parent <first-8>)`; an `agent.tool.result` hit names its tool, `▸ agent.tool.result Edit  L2128  …`. The footer keyword is `label=<joined selectors or all>` (+ `label-not=<…>` when `-T` is active).)

**Example invocations:**
```bash
csift search "carry"                                   # all projects, smart-case
csift search -i "askuserquestion" -t agent.tool.use   # tool_use blocks naming AUQ
csift search "" -t user --since 2h .                   # pure filter: genuine user turns, last 2h, this project
csift search "tail.read" --multiline @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
csift search "panic" -t agent.message -t agent.thinking --turn 10..20 --max-count 50
csift search "" -t agent.communication.inbox @<uuid>   # inbound peer/teammate messages (from ⇨ self)
csift search "" -t harness.notification @<uuid>        # subagent/workflow/bg-command completion pulses
csift search "" @<uuid> -t agent -T agent.thinking     # the agent role MINUS thinking (-T excludes)
csift search "deadline" -l                             # WHICH sessions mention it (one id per line)
csift search "deadline" -l | csift stats --sessions-from -   # …then aggregate exactly those
csift search "" @<uuid> -t agent.message --raw | jq -r '.message.model'  # raw lines: unrendered fields
csift search "let's chat" -t user --siblings           # the match WITH the turn's other side (fixed policy)
csift search "" @<uuid> --count-by label               # whole-scope per-leaf census (what a scope holds, before filtering)
csift search "AskUserQuestion" @<uuid> --count-by label # which labels a term occurs under (before guessing a -t)
csift search "" @<uuid> --count-by pairing                    # any pending tools? (paired|pending|orphan)
csift search "persisted-output" --resolve-persisted --format json
```

### 6.3 `whoami` — identify the calling CC session (false-positive-safe)

**Strategy (env-var-first — the calling session id is read directly from the environment):**

1. **Primary — read `CLAUDE_CODE_SESSION_ID`.** Verified definitive: CC exports it into every Bash-tool environment and its value equals the calling session's own jsonl basename exactly (e.g. `0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d` → `…/<ENCODED>/0a1b2c3d-….jsonl`). Per-session, version-independent, survives arbitrary bash nesting, **zero false positives**. If set and a non-empty UUID, that **is** the answer. Match the **exact** name `CLAUDE_CODE_SESSION_ID` — never a loose `/session/i` regex (`SECURITYSESSIONID`, the macOS login session, is a false-positive trap; `CODEX_COMPANION_SESSION_ID` mirrors the value but is Codex-plugin-specific — accept it only as a secondary alias, prefer the canonical var).
2. Resolve its transcript: encode `$PWD` to the `<ENCODED>` dir and open `<ENCODED>/$CLAUDE_CODE_SESSION_ID.jsonl`. If `$PWD` doesn't resolve, scan projects-root dirs for the one containing `<id>.jsonl`.
3. **Fallback when the var is absent/empty — ERROR with actionable guidance, never guess.** Message: your session id was not found (`CLAUDE_CODE_SESSION_ID` unset — old CC build or running outside CC); pass an explicit `@<uuid>` target; your id is the basename of your own transcript, or grep a unique recent line you wrote to disambiguate; **do NOT trust most-recent-mtime** (many CC sessions may be live concurrently). It is acceptable for `whoami` to often say "ambiguous, pass an explicit `@<uuid>`".
4. **FORBIDDEN as a whoami source:** process-tree walk and most-recent-mtime. (Evidence: 83 concurrent `claude` processes + 6 installed CC versions on one machine ⇒ ~83-way ambiguity and cross-version argv brittleness; the UUID isn't even on the process command line. mtime with 83 live sessions is almost always wrong.)

**Args (matches `cli::WhoamiArgs`):** an OPTIONAL positional SELF token (`@trap:<marker>` | `@main`) and `--format text|json`. The `path` line is ALWAYS printed (a `<not found …>` note when the id resolves to no file). **Self-token:** with NO token (or `@main`) `whoami` is the env path above; `@trap:<marker>` answers "which SUBAGENT am I?" (§6.3a) — it walks the full **UPSTREAM ancestry chain** (self → … → top-level root; the walk-UP mirror of `agents`' walk-DOWN, via `agents::topology_for_session` + `parent_agent_id`) **env-INDEPENDENTLY** (reliable for a built-in Task AND a workflow subagent, whose env id is the PARENT). Its JSON is the standard envelope (§8.2): a `{kind:"header",command:"whoami"}` line, then one `{kind:"identity", session_id, is_subagent, parent_session_id, depth, path}` row per ancestry link (self first at `depth:0` → top-level root last), then `{kind:"summary", identities:N}`. The env (non-`@trap`) form emits a SINGLE identity row carrying the FULL id trio + `depth:0` (NOT just `{session_id, path}`). `whoami` accepts NO other target (no project path / bare `@<uuid>`) — to inspect a DIFFERENT session use `list`/`agents`.

**Text output:**
```
session  0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d
path     ~/.claude/projects/-Users-testuser-Projects-widget-app-prototype/0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d.jsonl
```
**Error output (var absent):** non-zero exit + the guidance string from `whoami::AMBIGUOUS_GUIDANCE`.

### 6.3a `@trap:<marker>` — let a SUBAGENT identify itself

`whoami` and `@main` both read `CLAUDE_CODE_SESSION_ID`, which CC sets to the **top-level** session id in EVERY Bash environment — including inside an in-process subagent. A subagent therefore cannot name *itself* by env (CC withholds the per-subagent id from the Bash env; it is given only to hooks). `@trap:<marker>` recovers it from the transcript instead.

**Mechanism.** The caller INVENTS a unique marker and embeds it **literally** in the very csift command (shaped like `csift agents @trap:JollyShinyBrook4283` — though that exact marker literal is RESERVED and always refused: as a doc example it co-occurs with `csift` and gets copied by many agents, so it can never resolve to one transcript; callers must invent their own). A SUBAGENT's transcript is flushed per content block as it closes, so its `tool_use` is on disk at dispatch (verified empirically — a subagent's Bash can already grep its own marker mid-run). The MAIN transcript is written differently: an async flush of the COMPLETED assistant message, landing ~1-3.4s after the tool was dispatched (re-measured 2026-08-29) — a race, not a wait. csift resolves `CLAUDE_CODE_SESSION_ID`, then scans that session's main transcript **and** its subagent transcripts for a **shell** `tool_use` — `Bash`, or Windows' separate `PowerShell` tool (same `input.command` field; v0.7.4) — whose `command` contains BOTH the marker AND the literal `csift` (so an unrelated command that merely echoed the token cannot satisfy it). Resolution:
- exactly one **subagent** carries it → that agent (then its subtree, per `--no-subagents`);
- only the **main** transcript carries it → the session itself;
- **zero** → error (marker not literal / mistyped, the command did not run `csift`, or the record has not landed yet on the main lane — the error routes `@main` first, then the literal check, then a same-marker re-run as lane confirmation);
- **>1 subagent** → ambiguity error (use a fresher marker).

A subagent resolves **first-try** (its tool_use is on disk at dispatch). From the main thread the record lands ~1-3.4s after dispatch, so a first `@trap` normally loses that race and a re-run resolves — but the main thread should simply use `@main`, and a @trap that does resolve to the main transcript says so on stderr (v0.8.2).

**Marker grammar (ENFORCED — the discipline is the point).** The marker must be a fresh, one-shot, imaginative, CONTEXT-INDEPENDENT token the model invents on the spot: **EXACTLY 3 CamelCase words** (each one uppercase letter + ≥2 lowercase — no single letters, no ALLCAPS acronyms like `HTML` / `USB`) followed by **exactly 4 digits** that do not form a trivial run (all-equal / consecutive / simple odd / simple even — `0000` / `1234` / `9876` / `1357` / `2468`). It must NOT be script-generated (a generator would itself be a `csift`-ish Bash command carrying the marker → ambiguity), built from a shell variable / concatenation (it must appear verbatim in the recorded command), or reused. csift rejects every violation loudly with guidance. This strictness exists precisely to make a hand-invented literary token the path of least resistance and kill the over-engineered shortcuts at the source.

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
   - A matched **`agent.thinking`/`agent.message` text** is returned within its full turn (the opening genuine-user + sibling assistant records).
4. The emitted Exchange carries: `session_id`, `is_subagent` + the always-re-feedable `parent_session_id` (the id-domain discriminators; `parent_session_id == session_id` for a top-level hit), `turn_index`, `started_utc` (the turn-opening timestamp = the exchange's chronological position; falls back to the earliest hit's ts, `None` only when neither exists), the list of `Hit`s (the matched leaf `label` + the full `labels[]` set + excerpt + UTC timestamp + `tool_name` + comm `from`/`to` + tool `pairing`/`tool_use_id`), and `record_uuids` (every record stitched in — the §6.4 round-trip-completeness evidence) — matching `search::Exchange`.
5. **AUQ pairing:** an `AskUserQuestion` `tool_use` and its answering `tool_result` (§4.4) are one pair; the answer is surfaced under `user` AND — per the §4.4 boundary rule — **opens its own turn** (`opens_turn`). The reconstructed turn opener is the full Q+options+answer unit (`auq_exchange`).
6. **Compaction continuity:** a `compact_boundary`'s `logicalParentUuid`/`preservedSegment` may be used (best-effort) to keep turn indices monotonic across a compaction, but turn delimiting still keys on genuine-user records; never crash if these fields are absent.
7. **Combined stable chronological timeline.** Across the WHOLE scope, emitted Exchanges are sorted ASCENDING by `started_utc` (ISO-8601 UTC sorts lexicographically as text), so subagent exchanges INTERLEAVE with top-level ones by absolute time rather than grouping per file; timestamp-less exchanges sort LAST, with a deterministic tie-break (sorted file order → turn order, preserved by a stable sort — same shape as `files --by timeline`). The GLOBAL `--max-count` cap is applied AFTER the sort (keeping the earliest N and reporting the dropped remainder — never silent).

### 6.5 `agents` — a session's subagent TOPOLOGY (kind, trigger/start/completion, returned message, files)

**Purpose.** Build the toolUseId-LINKED topology of the subagents a session spawned: each subagent joined back to the parent `Task`/`Agent`/`Workflow` `tool_use` that triggered it, carrying its identity + lifecycle, the TRUE trigger time, the returned message (3-way resolved), and (on demand) its files-changed. The output is ALWAYS the parent→child tree: workflow RUN nodes (from the top-level `workflows/wf_*.json` manifests) parent their agents, and a nested sub-subagent renders under its spawning agent (there is no flat mode). Complements the default subagent span on `list`/`search` (which fold subagent *content* into those views); `agents` is the *topology + lifecycle index* of those same subagents. (Design rationale + verified corpus counts: **§10.3**.)

**Topology linkage (the spawn join).** A built-in subagent's `meta.json` carries `toolUseId` — the id of the parent `Task`/`Agent` `tool_use` that spawned it. csift builds a per-session `ParentSpawnIndex` (one forward scan of the parent transcript) mapping `tool_use_id → {spawn tool name, trigger ts, description, subagent_type}` and `tool_use_id → paired tool_result text`. Each subagent joins on its `spawn_tool_use_id`, recovering:

- **TRUE trigger time** = the parent tool_use ts (the real "when was it triggered" instant). The subagent's own first-record ts (`started_utc`) LAGS this by 0.2–4.7 s and cannot order a sibling fan-out, so trigger is the **default** time axis. `started_utc` is retained as a secondary timestamp. (Workflow agents carry no `toolUseId`, so their trigger falls back to the start ts.)
- **Returned message — resolved 3 ways:** (a) **sync built-in** → the parent tool_result text; (b) **async built-in** (the parent tool_result is the `Async agent launched …` sentinel) → the child transcript's tail assistant text; (c) **workflow** → the `journal.jsonl` `result` event payload (not just the completion bool — the full message). The source is reported (`sync-tool-result` / `async-child-tail` / `workflow-journal`).
- **WorkflowRun nodes** are read from the UNSCANNED top-level `<session>/workflows/wf_*.json` manifests (NOT `subagents/workflows/`): `{runId, taskId, workflowName, status, agentCount, durationMs, totalTokens, totalToolCalls, defaultModel}`. In the tree each run is the parent of its `wf_<id>` agents (joined on `workflow_id == runId`).
- **Nested subagents (agent→agent).** On disk the layout is FLAT at every depth: CC writes a subagent's transcript under `getSessionId()` = the MAIN session, regardless of who spawned it (verified against the shipping binary: the agent-transcript path and the shell both key off the process-global main id, and agent code never resets it), so a sub-subagent lands flat in the SAME `<main>/subagents/` dir, never `subagents/.../subagents/`. Nesting is therefore LOGICAL, not structural: the child's spawning `Task`/`Agent` tool_use is recorded in its SPAWNING agent's transcript (not the main one), and the child's `meta.json` `toolUseId` points at it. `build_topology` recovers the tree with a GLOBAL spawn index (the main transcript + EVERY subagent transcript, each spawn id tagged with its issuing agent), then sets `parent_agent_id` (the issuer; `null` ⇒ a direct child of the session, depth 0) and walks that chain for `depth`. The always-on tree (text + JSON) nests a sub-subagent UNDER its spawning agent, and every agent appears exactly once in it (so disk coverage and topology are both complete). Real multi-level nesting exists now (re-measured for v0.10.1 over 7501 subagent transcripts: 190 nodes at depth 1-3 with a distinct parent; 117 subagent transcripts carry a spawning `Task`/`Agent`/`Workflow` tool_use, all at the flat built-in location). Two v0.10.1 rules keep the walk honest: the meta's `parentAgentId` (written by CC 2.1.25x) is preferred over the tool_use-graph join, and a join that names the node ITSELF is dropped — a `/fork` child is a clone of its parent's transcript, so its own file carries the spawning tool_use and the graph join used to make every forked lane its own parent (32 nodes sat at the depth-65 cycle cap before the fix).

**Id-form unification:** a subagent transcript's printed `session_id` is the **bare `<hex>`** everywhere (`agents`, `files`, `recover`, `list`) — the `agent-` filename prefix is stripped — so a file mutation or recovered event is joinable back to its `agents` node id.

**Subagent on-disk layout (empirically mapped against `~/.claude/projects`, 0 linkage mismatches across 600+ nested transcripts). Three shapes under a top-level session's sidecar `<ENCODED>/<session-uuid>/`:**

| kind (csift) | path | meta.json | record markers |
|---|---|---|---|
| `builtin-task` | `subagents/agent-<hex>.jsonl` | `{agentType, description, name?, toolUseId}` | `isSidechain:true`, `agentId == <hex>`, `sessionId == <session-uuid>` |
| `workflow` | `subagents/workflows/wf_<id>/agent-<hex>.jsonl` | `{agentType}` | same; `cwd` is often a DEEPER in-session path — never re-encode a subagent cwd |
| `teammate` | `subagents/agent-a<Name>-<hex>.jsonl` | `{agentType(=handle), description, name, taskKind:"in_process_teammate", teamName, color, model, …}` (NO `toolUseId`) | `isSidechain:true`, `agentId == a<Name>-<hex>`; inbound `<teammate-message teammate_id="…">` blocks |
| *(excluded)* | `subagents/workflows/wf_<id>/journal.jsonl` | — | **NOT a transcript**: `{agentId, key, type∈{started,result}}`, no `message`/role |

**Kind is the on-disk PATH LOCATION, not `agentType` — with ONE meta-driven exception, `teammate`.** Both `builtin-task` and `workflow` carry the same spread of `agentType` values (`Explore`, `general-purpose`, `oh-my-claudecode:*`); only `workflow-subagent` is workflow-exclusive. So `agentType` is a descriptive sub-label, not the discriminator. A **teammate** (Claude Code's `in_process_teammate` / FleetView feature) sits at the SAME location as `builtin-task`, so location can't classify it — the meta `taskKind:"in_process_teammate"` upgrades it to `teammate`. Its id EMBEDS the teammate name (`aVSRepro-68a2a1661c9390c1` = `a<Name>-<16hex>`, NOT a bare hex), its meta overloads `agentType` with the handle and omits `toolUseId`, and it is spawned by an `Agent` tool_use whose `input.name`==the handle + `input.subagent_type`==the REAL type. csift recovers the real `agent_type`, spawn `tool_use_id`, true trigger ts, and parent via a NAME-join (`meta.name` ⇆ `Agent.input.name`), and surfaces `name`/`team_name`. **`agents` also emits a CONTROL pointer** (read-only csift can't act, but it names the tool that can): a text footer when ≥1 teammate is in scope + each teammate node's JSON `control_hint` — a teammate is steered/terminated via `SendMessage` BY NAME (`message:{type:"shutdown_request"}`), NOT `TaskStop` (a `task_id`-only background-task tool that rejects every teammate id) or `pkill` (in-process; shares the orchestrator PID). **The canonical agent id** (bare `<hex>` for built-in/workflow, `a<Name>-<hex>` for a teammate) is what every surface prints AND re-accepts as `@<id>` (`path::is_subagent_id`), not the `agent-…` filename stem.

**Linkage to the parent session** is FILESYSTEM-primary (the enclosing `<session-uuid>` dir name) corroborated by the record `sessionId` (600/0 verified). Top-level dir enumeration + `whoami` are safe from matching subagents because subagent files are named `agent-<hex>.jsonl` (never `<uuid>.jsonl`) and sit in a nested dir the non-recursive top-level scan never descends.

**Args (matches `cli::AgentsArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3) whose sessions' subagents to list; a `@<uuid>` positional restricts to one top-level parent session |
| `--shape builtin-task\|workflow\|teammate` | repeatable enum | all | filter to a subagent transcript SHAPE (`teammate` = `in_process_teammate`/FleetView agents) |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`/…, system-local), filters by the `--order-by` axis |
| `--order-by trigger\|start\|completion` | enum | **`trigger`** | the ORDERING axis — which timestamp sorts the tree AND bounds `--since`/`--until`. `trigger` is the true spawn instant (the **default**), `start` the child's first-record ts, `completion` its last. (Named `--order-by`, not `--by`, because it names the sort axis; `files` uses `--by` for a PROJECTION — a different meaning on a different subcommand.) |
| `--agent HEX` | string | none | grab ONE subagent by bare-hex id; prints its full node incl. the returned message (implies `--returned-message`) and, with `--with-files`, its files-changed. Renders just that node (a tree of one) — JSON emits the bare node, not the per-session envelope |
| `--returned-message` | bool | off | include each subagent's 3-way-resolved returned message (omitted by default — can be large; always on for a single `--agent` grab) |
| `--with-files` | bool | off | attach each node's files-changed list (reuses the `files` extractors over the subagent's own transcript) |
| `--format text\|json` | enum | `text` | output format |

**Per-subagent (`kind:"agent"`) node fields emitted:** `agent_id` (bare hex, or a teammate `aName-<hex>` id — the name may itself carry dashes), `shape` (the transcript shape — `builtin-task`\|`workflow`\|`teammate`; JSON key is `shape`, not `kind` — `kind` is the envelope's row tag), `parent_session_id`, `parent_agent_id` (the spawning agent for a nested subagent; `null` ⇒ a direct child of the session, depth 0), `spawn_tool_use_id`, `spawn_tool` (`Agent`/`Task`/`Workflow`), `workflow_id` (workflow only), `agent_type` (meta `agentType`, fallback the spawn's `subagent_type`), `name`/`team_name` (teammate only, else null), `description` (built-in meta, fallback the spawn's), `trigger_utc`/`trigger_local` (the true spawn instant), `started_utc`/`started_local` (first transcript record ts), `completed_utc`/`completed_local` (the completion instant — non-null ONLY when `status` is `completed`; a frozen/running/unknown lane carries `null`), `last_activity_utc`/`last_activity_local` (the tail newest-record instant, present on every timestamped lane regardless of status; == `pending_since_utc` on a frozen lane), `duration` (trigger→completion; null unless completed), `status`, the FROZEN-LANE fields `pending_tool_use_id`/`pending_tool_name`/`pending_classification`/`pending_since_utc`(+`_local`) (all `null` on a normal lane), `depth`, `skipped_lines`, `control_hint` (teammate only); plus on demand `returned_message` + `returned_message_source` and `files_changed[]`. **Frozen-lane detection:** when a subagent's NEWEST meaningful record is an UNRETURNED `tool_use` (no following `tool_result` for its id — a permission escalation leaves NO jsonl trace, the lane just freezes there), `status` is forced `running` (NEVER `completed` — overriding the end-of-turn-text walk-back, which would otherwise read the assistant text PRECEDING the frozen tool_use as a clean finish) and `pending_classification` ∈ {`escalation-blocked`, `awaiting-execution`}: `escalation-blocked` iff the pending Bash command is one CC's `dangerous-rm` classifier ([`bash_danger`], a 1:1 lexical port of the 2.1.x binary's `Ywa`/`egp`/`Zhp`) would HOIST for human approval even under bypass (≈ waiting for a Yes); else `awaiting-execution` (slow tool OR wedged — indistinguishable in jsonl; weigh elapsed-since-`pending_since_utc`). A pending **`PowerShell`** lane (Windows' separate shell tool, v0.7.4) always classifies `awaiting-execution`: the lexical-bash classifier deliberately does not run on PowerShell commands.

**`--format json` is FLAT (envelope v2, NO exceptions — v0.5).** The stream is: `{kind:"header",command:"agents"}` → per in-scope session a light `{kind:"session", session_id, runs:N, agents:N}` count row → per in-scope workflow RUN a `{kind:"run", run_id, task_id, workflow_name, status, agent_count, duration_ms, total_tokens, total_tool_calls, default_model, started_utc/local, session_id}` row (NO `children`) followed by its member agent rows, then the session's built-in agents — every agent as its OWN `{kind:"agent", …node fields above…}` row in tree PRE-ORDER (a nested sub-subagent immediately after its parent) → `{kind:"summary", sessions, runs, agents}`. **Tree NESTING is TEXT-mode ONLY** (the text output is still a parent→child tree); JSON consumers rebuild it from `parent_agent_id`/`depth` (`jq 'select(.kind=="agent")'` reaches every node). A node UNREACHABLE from any root (a forged parent cycle) is APPENDED, never dropped — the pre-order guarantees full coverage where the old nested `{session_id, workflow_runs:[…children…], agents:[…]}` shape silently dropped such nodes. A single `--agent <hex>` grab is NOT a special JSON shape — it emits the same envelope (header → session → the one agent, with `returned_message`/`files_changed` → summary); the old bare-node exception is gone.

**Status resolution (honest — never over-claims "failed"):** `completed` when a workflow `journal.jsonl` carries a `result` event for the agent OR the transcript terminates with a visible assistant end-of-turn message; `running` when records exist with a start but no completion signal; `unknown` when no timestamps are determinable.

**Time window** is the same semantics as `search` (§6.2): records/rows with no timestamp on the chosen axis are never admitted by a *bounded* window; an unbounded window admits all. **Default axis:** `--since`/`--until` filter on the **trigger** axis (the true spawn instant) by default — the sharpest, most accurate bound; `--order-by start` (the child's first-record ts) or `--order-by completion` opts to a different axis.

**Example invocations:**
```bash
csift agents @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d            # one session's subagent tree (always)
csift agents . --shape workflow                               # only workflow agents
csift agents @<uuid> --since 2h                               # subagents TRIGGERED in the last 2h
csift agents @<uuid> --since 09:00 --order-by completion      # COMPLETED since a bound
csift agents @<uuid> --order-by completion                    # order/window on the completion axis
csift agents @<uuid> --agent <hex> --with-files               # grab one subagent: its returned msg + files
csift agents @<uuid> --returned-message --format json         # every node's returned message
```

### 6.6 `files` — which files/dirs a session modified, when

**Purpose.** Report the files + directories a session changed, attributed to genuine-user turns, with a context-blow-up guard (a compact per-dir **summary** is the default via `--by summary`; the full chronological list is opt-in via `--by timeline`). Answers the acid-test question "how many distinct gap docs did this session touch, and how many `/tmp` docs did it create?". Optional `--regex` / `--glob` full-path filters narrow the reported set.

**Extraction (verified against the live `~/.claude/projects` corpus, 2026-06-08) — authoritative vs heuristic:**

| source | how | create-vs-edit | label |
|---|---|---|---|
| `Edit` / `Write` / `MultiEdit` | `input.file_path` | from paired `toolUseResult.type` (`create` ⇒ new file; `update`/`file_unchanged` ⇒ edit), joined by `tool_use_id` within the turn | authoritative |
| `NotebookEdit` | `input.notebook_path` | same join | authoritative |
| `Bash` | LEXICAL parse of `input.command` (`rm`/`mv`/`cp`/`mkdir`/`touch`/`tee`/`sed -i`/`perl -i`/`git`/redirection/interpreter write idioms), operands RESOLVED against the record's own `cwd` (§4.9) | not knowable lexically (heuristic guess) | **HEURISTIC** — always labelled `(heuristic)` |

Bash's `toolUseResult` is `{stdout, stderr, interrupted, isImage, noOutputExpected}` — **no path field** — so Bash mutations are a best-effort lexical (NOT shell) parse and are flagged heuristic everywhere they surface (text, JSON, help, SKILL). The `file_path` lives on the **tool_use** record while `toolUseResult.type` (`create`/`update`) lives on the **paired tool_result carrier**; the joiner pairs them by `tool_use_id` within the turn so `is_create` is accurate. **A STRUCTURED op whose tool_result is `is_error:true` is EXCLUDED** — a failed Edit, or a Write `Cancelled: parallel tool call … errored` when a sibling op in the same batch failed, never landed, so counting it would be a forensic FALSE POSITIVE ("did this session write X?") and would contradict `recover` (which correctly reconstructs nothing). Same `failed_ids` gate `recover`'s extraction applies. A BASH mutation from a chain whose result errored is KEPT and flagged `command_errored` instead (v0.8.0): which arms of a compound command ran is unknowable, so the mutation is disclosed, never silently dropped. **A bash operand is RESOLVED against the recording shell's cwd** — the record's own top-level `cwd` field (§4.9) — with an explicit per-row resolution class (`absolute` / `cwd-joined` / `cd-tracked` / `unresolved`); relative and absolute spellings of one file therefore share a bucket in every rollup, and only a genuinely unresolvable operand (`$VAR`, a leading `~`, an unknowable in-command cwd) stays verbatim, classed `unresolved`. Commands that mutate files they never name emit a CLASS-MARKER pseudo-path in the `git:<sub>` style — `fmt:<tool>` (a formatter run), `interp:<lang>` (an interpreter write whose target the guards cannot extract), `pkg:<manager>` (installs/updates), `extract:<tool>` (archive/patch extraction) — never a fabricated file path.

**Edit-before-Read boundaries (file changed outside the tool stream).** Beyond mutations, `files` also DETECTS the `File has been modified since read` integrity errors and attributes each to its file (the rejected op's `tool_use_id` ↔ that op's `file_path`, even though the op never landed). These are the points where a formatter / linter / husky-or-pre-commit hook / git / external editor changed the file out from under the harness and a fresh Read was forced — the same authoritative boundary `recover` segments on. Surfacing them in `files` is the DISCOVERY signal: "which files in this session are risky to reconstruct?" → then `recover --file <path> --coverage` for the precise per-boundary breakdown. The error-carrier line is kept by the prefilter (an `is_error` hook) so the detection survives. (csift does NOT hunt for HIDDEN boundaries — a change with no downstream `modified since read` error, e.g. a Read-first-then-windowed-edit, leaves no transcript signal and is undetectable; verified against the CC binary. What `files` surfaces is the detectable subset, honestly bounded.)

Every `files` row + boundary also carries the **JSONL line number** (`Lnnnn`) of its record — the same join-back-to-the-transcript locator `recover`/`search`/`verbatim` provide (the parallel scan already produces it).

**Args (matches `cli::FilesArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); a `@<uuid>` positional restricts to one top-level parent session |
| `--no-subagents` | bool | spans subagents | subagent scope: by default attributes SUBAGENT mutations under the session (OMC fan-out edits happen there); `--no-subagents` = top-level only. `files` matches every other default-on command (`want_subagents()` → `SubagentScope::from(bool)`); the uniform pair is `--subagents`/`--no-subagents` (no `--subagents-only` mode). |
| `--by <summary\|dir\|file\|timeline>` | value-enum | `summary` | detail level (below) |
| `--regex RE` | string | none | keep only mutated paths whose **full absolute path** matches the Rust `regex` pattern ANYWHERE (used as-is; invalid pattern = hard error) |
| `--glob PAT` | string | none | keep only mutated paths whose **full absolute path** matches the glob (`**` crosses `/`, via `globset`; invalid pattern = hard error) |
| `--turn N\|A..B\|N..\|-k` | string | none | inclusive 0-based turn window (shared range-token grammar: `N`/`A..B`/`N..`/`..N`/`-k` from the end); INTERSECTS (AND) `--since`/`--until`. |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`/…, system-local) |
| `--format text\|json` | enum | `text` | output format |

**Detail levels — `--by <value>` (the context-blow-up guard — exactly one is active, default `summary`):**
- **`--by summary` (DEFAULT)** — compact per-top-level-dir rollup with op counts (e.g. `"/tmp: 12 write, 3 edit; spec/gaps: 4 edit"`); the smallest output, answers the acid test directly. Bucket = the mutation path's parent directory; a bare relative filename buckets under `./`.
- **`--by dir`** — one row per distinct directory (full path) with per-op counts + distinct-file count + first/last timestamp.
- **`--by file`** — one row per distinct absolute path with per-op counts + first/last timestamp (where "how many distinct gap docs touched" is exactly answerable).
- **`--by timeline`** — full chronological list, one line per mutation `(Lnnnn, timestamp, turn index, op, path)`. The verbose mode; never the default.

Regardless of detail level, an **Edit-before-Read boundaries** section follows the mutation body (and shows on its own when a session ONLY hit boundaries, no mutations), one row per boundary `(⚠ path, Lnnnn, turn, ts, kind)`.

**Filtering** is per-mutation. **Path filters** `--regex` / `--glob` are OPTIONAL, combinable with each other and with `--by`, and **ANDed** (a path must satisfy every supplied filter); both match against the **full absolute path** and are applied to mutations AND boundaries **BEFORE** the `--by` rollup, so summary/dir/file/timeline and the boundary section all reflect the filtered set (a filter that removes everything yields the normal `no file mutations found`). **Time/turn filters** `--turn` (turn index assigned by the §6.4 genuine-user delimiter, shared with `search`) and `--since`/`--until` (a mutation with no timestamp never falls inside a *bounded* window — same rule as §6.2). **No silent truncation:** skipped malformed lines are counted and surfaced.

**Output.** Text groups under a `SESSION <id>` header, then the level-appropriate body, then the Edit-before-Read boundary section (if any), then a footer: distinct files + total mutations + boundary count, the active detail level (`detail=…`), the turn/time filter context (`turn=<SPEC>` when `--turn` is set), the Bash-heuristic caveat, and the skipped-line count. Empty result (no mutations AND no boundaries) prints `no file mutations found`. JSON is the §8.2 envelope: the `{kind:"header",…}` line, then one object per emitted unit (bucket / dir / file with the counts + first/last; or per mutation for `--by timeline` with `{kind:"mutation", session_id, is_subagent, parent_session_id, path, op, ts_utc, ts_local, turn_index, line, is_create, heuristic, resolution, path_verbatim, command_errored}` — `resolution`/`path_verbatim` are the v0.8.0 bash-resolution provenance, null on structured tools and class markers), then one `{kind:"boundary", session_id, is_subagent, parent_session_id, path, line, turn_index, cause, ts_utc, ts_local}` per boundary (in every detail mode; `cause` names what changed the file out of band — `kind` stays the envelope discriminator exclusively), then the trailing `{kind:"summary", sessions, distinct_files, total_mutations, edit_before_read_boundaries, skipped_lines, detail_level}` (`sessions` = the distinct owning sessions among emitted rows — v0.5). `files` has NO row cap, so the summary deliberately carries NO `dropped_by_cap`.

**Perf shape** is `search`'s: a single forward pass per file (mmap + SIMD newline scan + a pre-JSON mutation byte-prefilter, full parse only on candidate lines), no large-blob retention (extract small `FileMutation` strings, drop the record — never hold `originalFile`/`content`/`structuredPatch`), rayon across files on the default pool (= CPU count).

**Example invocations:**
```bash
csift files @<uuid>                         # default summary: per-top-level-dir op rollup
csift files @<uuid> --by file               # per-file op counts + first/last touch
csift files @<uuid> --by file --regex '\.rs$'         # only .rs paths, per-file
csift files @<uuid> --by timeline --glob '**/src/**'  # mutations anywhere under a src/ dir
csift files @<uuid> --by timeline --since 2h          # full chronological, last 2h (heavy)
csift files . --format json --by dir        # machine-readable per-dir rollup
csift files @<uuid> --format json | jq 'select(.kind=="boundary")'  # which files changed outside the tool stream
```

### 6.7 `recover` — reconstruct a file's content (or a plan) from the transcript

**Purpose.** Where `files` (§6.6) only reports THAT a file was touched, `recover` rebuilds the file's **content** by replaying its Read / Write / Edit stream in transcript order. Five mutually-exclusive modes (clap group `mode`, **default = restore**): **restore** (no mode flag) hands back the file's FINAL content as RAW restorable bytes (`recover --file X > X`), but ONLY when the session saw the WHOLE file — if it observed just PART (a windowed read + a few edits) restore FAILS LOUDLY rather than emit a holey file. The failure is a SMART diagnostic: covered + missing ranges, EVERY external-change boundary (Edit-before-Read / external edit), and — when a richer state survived BEFORE the first such change (the session-authored "complete as of 5pm" case, or a fuller partial otherwise) — a `dump-the-pre-change-version (--at @line:<before>) + dump-the-changes-since (--patches --since) + reconcile-by-hand` recipe; it ALWAYS closes with the caveat that csift cannot see changes made OUTSIDE the Read/Write/Edit stream (a formatter, husky/pre-commit, git, bash) and does NOT hunt for hidden boundaries (escalated when a bash mutation may have touched the file); **`--salvage`** is restore's never-fails sibling — the best-effort line-numbered FINAL-state fragment (known lines numbered, the rest left as explicit `??? lines A..B unknown` gaps), for a file that is gone, only-partially-read, and barely-edited, where salvaging the surviving proportion beats rewinding (identical output to `--at @latest`); **`--patches`** is the segmented diff-patch CHANGES/rewind view, rendered with FULL context — every read-covered line is shown, not a 3-line window (CC's strict Read-before-Edit guarantees those lines were genuinely observed, so a fully-read, one-line-edited file reproduces in full); **`--at WHEN`** is a point-in-time partial snapshot; **`--coverage`** (alias `--dry-run`) is coverage/scoping only (its per-boundary JSON — `boundaries[]` with `line`/`source_line`/`ts`/`cause` — drives the boundary-by-boundary recover/salvage recipe in SKILL.md). `--file` is REQUIRED for all five. A magic `--file @plan` VALUE (not a mode) resolves the session-bound plan file and reconstructs THAT file exactly like any other — its full Write+Edit history, edit-aware — so it composes with every mode and with `--out`/`--format`; this is how you DUMP a plan's content, including a DELETED plan rebuilt from the transcript alone (§6.7.1 covers `@plan` + the sibling `plan` subcommand that LOCATES a session's plan). The motivating use is restoring a file (or a dropped plan) lost in a bad-recovery. (Design rationale — the reference-tool survey + the `originalFile` boundary inversion: **§10.1**.) **Every output reference carries the JSONL line number** (`Lnnnnn`) so a consumer can `Read` the raw jsonl directly — the one genuinely-new capability over the other subcommands (added via a local line counter threaded through `scan_lines_bytes`; the shared signature is untouched).

**Bash content anchors (v0.9.4).** The deterministic shell subset joins the replay as first-class content events — full laws in the v0.9.4 ledger at the top of §6: segment-scoped quoted-heredoc/`echo`/`printf`/`truncate -s 0` writes (compound commands demand a clean result echo; same-path second touches refuse), single-command `cat`/`head -n N`/`sed -n 'A,Bp'` reads under the completeness gate, `>>` appends placed only onto a complete newline-terminated buffer (else the `bash_append_unplaced` boundary). New counts `bash_read_anchor`/`bash_write_anchor`; anchor sources `bash-cat`/`bash-heredoc`/`bash-write`. An admitted write anchor supersedes its own heuristic bash-mutation boundary row.

**Extraction (verified against the live corpus, 2026-06-08).** Per `--file`, in transcript order:

| event | source | result |
|---|---|---|
| full Read | `toolUseResult.file` with `startLine==1 && numLines==totalLines` (content is RAW, no gutter) | a **full-snapshot anchor** |
| windowed Read | `toolUseResult.file` with `startLine>1` or `numLines<totalLines` | a **partial splice** (gaps stay gaps — never padded) |
| Write | `toolUseResult.{type:create\|update, content}` | a full-snapshot anchor |
| Edit / MultiEdit | `toolUseResult.{oldString,newString,structuredPatch,originalFile}` (no `type`) | applied to the running buffer by `structuredPatch` line position (string-replace fallback) — **only when the paired result did NOT error.** An Edit/Write whose result was `is_error:true` (`String to replace not found in file`, or the Edit-before-Read wall `File has not been read yet`) never mutated the file, so it is NOT applied |
| integrity error | a `tool_result` with `is_error` whose body is `File has been modified since read` / `File has not been read yet` / `String to replace not found in file` / `File does not exist` (no inline path → attributed via the `tool_use_id`↔tool_use join) | a HARD boundary (modified-since-read); the other cases are non-boundary integrity ANNOTATIONS, counted in the event ledger — the edit is skipped, not counted as a recoverable edit, never fabricated |
| Bash mutation | `input.command` lexically parsed (§6.6), operands RESOLVED against the record's own `cwd` (§4.9) and matched resolved-first with the verbatim spelling as a belt | a HEURISTIC (soft) boundary naming the resolved path + resolution class |
| `staleReadFileStateHint` | `toolUseResult.staleReadFileStateHint` on a Bash result — CC's own modified-file attribution (§4.9); named paths (relative to the shell cwd) resolved against the carrying record's `cwd` | an AUTHORITATIVE HARD boundary (`hint_modified`) — the one signal that attributes a formatter-class rewrite to concrete files. A truncated list (`… and N more`) can only match its named first five |
| `staleRecovered` | `toolUseResult.staleRecovered:true` on a SUCCESSFUL Edit result (§4.9) | an AUTHORITATIVE non-invalidating ANNOTATION boundary (`stale_recovered`) — the edit applied; other disk changes exist outside the stream |
| `edited_text_file` attachment | `attachment.{filename,snippet}` (TAB **or** U+2192 gutter); an EMPTY snippet is the over-budget degraded form, named in the boundary detail | a HARD boundary (external edit); when a formatter/interpreter class command ran in the same window, the detail names it with its real transcript line (the formatter-class clue) |
| opaque commands | every mutating-CLASS marker (`fmt:`/`interp:`/`pkg:`/`extract:`/worktree-rewriting `git:` — index-only `git:add`/`git:commit` excluded) + every `PowerShell` tool call | SCOPE ACCOUNTING, never events: counted + listed per window with a ready-to-run time-bounded `csift search` command |
| `file-history-snapshot` | `snapshot.trackedFileBackups[<path>]` | a coverage ANNOTATION only — `backupFileName` is usually present (measured 83–98%), but the store it names is PRUNED and checkpoint content has no transcript anchor, so it is **never** used to fabricate content (`--list-backups` lists the store) |

**Reconstruction model (the "in the LLM's eyes" sparse buffer).** A `BTreeMap<file_line, cell>`; a line absent from the map is an **explicit gap** (unknown — never fabricated). A full snapshot resets the map; a windowed read splices without padding; an edit applies by structured-patch position. **Anti-fabrication guards (load-bearing):** an edit whose old region falls in an unknown gap, whose anchor neighbourhood is entirely unknown, or whose patch context disagrees with the buffer is an **un-anchorable coverage hole** (counted, never asserted as a known line). This is what keeps the reconstructed-vs-disk guarantee honest: the contiguous-from-line-1 prefix matches disk byte-for-byte even on a heavily-edited file with no clean anchor.

**Integrity boundaries (a point where reconstruction across it is invalid), in confidence order:** (1) AUTHORITATIVE — a `modified since read` harness error; (2) AUTHORITATIVE — Claude Code's own `staleReadFileStateHint` naming the file (`hint_modified`); (3) AUTHORITATIVE — an Edit whose `originalFile` disagrees with the replayed buffer (the signal claude-file-recovery *discards*, csift mines — a soft bash boundary does NOT disarm this check, v0.8.0: catching post-bash drift is its job); (4) AUTHORITATIVE — an external `edited_text_file`; (5) AUTHORITATIVE annotation — `stale_recovered` (nothing invalidated); (6) HEURISTIC — a Bash mutation (always flagged best-effort, naming the resolved path + class). **The HARD/soft split (v0.8.0):** hard = the INVALIDATING kinds (`modified_since_read`, `external_edit`, `hint_modified`); soft = annotations + heuristics (`original_file_disagreement`, `stale_recovered`, `bash_mutation`). `fragments = hard + 1`, so soft signals never inflate it; coverage prints `N (h hard · s soft)` and JSON carries `hard_boundaries`/`soft_boundaries`. `--patches` emits one unified diff per segment between segment-splitting boundaries, each segment + boundary carrying its jsonl line / turn / timestamp; **no diff spans a boundary**. Boundary rows also carry `source_session_id` + `source_line` — the REAL transcript location — while `line` stays the replay/cutoff coordinate (`--at @line:<N>`); the two differ only after a cross-transcript merge. **Final-state invalidation (load-bearing for honesty):** a `modified since read` boundary means the file changed out from under the harness (a `prettier`/linter rewrite, etc.) and a fresh Read was demanded — so every line known BEFORE that boundary is now SUSPECT. The final-state reconstruction therefore INVALIDATES the pre-boundary buffer (clears the known lines + the seen-total); only content RE-READ / re-written after the boundary survives into the final state. Pre-boundary lines become explicit gaps, never silently-stale lines presented as "current". This is what stops restore from falsely reporting `complete` (and `--salvage`/`--at @latest` from dumping stale content) when a session read a file, watched it change, and only re-read part of it.

**Args (matches `cli::RecoverArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); a `@<uuid>` positional restricts to one top-level parent session |
| `--file ABS_PATH` \| `@plan` | string | none | the file to reconstruct (exact raw-string match + basename-suffix fallback); **REQUIRED** for every mode. The magic value `@plan` resolves the session-bound plan file (§6.7.1) and reconstructs it like any other file |
| `--no-subagents` | bool | spans subagents | restrict to the top-level session; subagent transcripts are spanned by default (OMC fan-out edits happen there). The only span flag. |
| `--salvage` / `--patches` / `--at WHEN` / `--coverage` (alias `--dry-run`) | clap group `mode` | restore (none set) | the reconstruction mode (mutually exclusive; with NONE set, the default restore mode applies) |
| `--turn N\|A..B\|N..\|-k` | string | none | inclusive 0-based turn window (shared range-token grammar: `N`/`A..B`/`N..`/`..N`/`-k` from the end); INTERSECTS (AND) `--since`/`--until`. |
| `--since WHEN` / `--until WHEN` | string | none | time bound (ISO8601 / relative) |
| `--file-lines N\|A..B\|N..\|-k` | string | none | 1-based inclusive FILE-line span to restrict the reconstructed line space (shared range-token grammar: `N`/`A..B`/`N..`/`..N`/`-k` from the end) |
| `--out PATH` | path | none | write the reconstructed artifact (snapshot / plan / concatenated patches) verbatim; the summary still prints to stdout |
| `--format text\|json` | enum | `text` | output format |
| `--files-from MANIFEST` | path | none | **BATCH MODE**: reconstruct EVERY absolute path listed in MANIFEST (one per line; blank lines + `#` comments ignored) in a SINGLE corpus scan. Requires `--out-dir`; mutually exclusive with `--file`. Honors `--at`/`--since`/`--until` (default = each file's final state) |
| `--out-dir DIR` | path | none | batch output dir — each recovered file is written to `<DIR>/<abs-path-without-leading-slash>` (mirrored, dirs created), plus a `recovery-report.tsv` (status · known_lines · total_lines · boundaries · bash_file · bash_opaque · target · written_to) |
| `--force` | bool | `false` | batch: overwrite an already-present output file (default: skip it + report `skipped-exists`) |

`--at <WHEN>` accepts ISO8601, relative (`2h`), `@turn:<N>`, `@line:<N>` (state as of jsonl line N), or `@latest` (the file's FINAL reconstructed state — no cutoff; the clean way to ask for "its last form" without guessing a timestamp past the last write) and doubles as the snapshot cutoff. A datetime bound is **INCLUSIVE** of events at that instant. **`--file @plan`** is a bash-safe magic value (no shell metacharacters, no escaping in mixed scripts — consistent with `--at`'s `@line:`/`@turn:` sigils): it resolves the target session's authoritatively-bound plan file (§6.7.1) and substitutes that path, so `recover` then rebuilds the plan's FULL Write+Edit history (edit-aware, not just the latest Write) under whatever mode + window + `--out`/`--format` were given — the way to dump a plan's content, a DELETED plan included. It prefers the top-level session's own plan and **ERRORS clearly (never guesses)** when no plan is bound to the target session(s), or when the target spans sessions bound to DIFFERENT plans (asks for an explicit `@<uuid>` target).

**Output.** **restore** (default) is the exception: it writes the RAW final content to stdout (no `SESSION` banner, no line numbers — clean to pipe `> file`), with the WINDOW-ACCOUNTING status on stderr (v0.8.0): a clean window says so positively (`…complete; no bash mutation of this file and no opaque mutating-class command detected in the window`); a window with boundaries or opaque commands says `complete from the tool stream; NOT verified against disk` and lists the boundaries, the opaque-command note, and a ready-to-run time-bounded `csift search` for the window. `--out` writes the raw file + the same stderr status while stdout stays empty. A partial file is a hard ERROR (stderr diagnostic naming covered/missing ranges, boundaries, opaque counts, and the inspection search + non-zero exit) — and a fully-INVALIDATED history (events exist but no line content survives the replay) errors with a message naming the invalidating boundary kinds, never the false "never Read/Written/Edited". `--format json` ALWAYS emits the full three-part envelope (§8.2): the `{kind:"header",…}` line, one `{kind:"restore", file, complete, lines, boundaries, bash_events, opaque_commands, powershell_commands, suggested_search}` row carrying `content` (or `path`+`wrote` with `--out`), and the `{kind:"summary", mode:"restore", …}` trailer — the ERROR paths included, which emit their `{kind:"restore", complete:false, reason:"partial"|"no-history"|"invalidated"}` row + summary BEFORE the non-zero exit. The `restore` record's `content` carries RAW newlines, so that single record is NOT per-line NDJSON — a deliberate verbatim-bytes choice. The other four modes group text under `SESSION <id>`: `--coverage`: recoverable-line fraction, covered ranges, per-op counts (incl. `modified-file-hint` / `stale-recovered`), the integrity-boundary list (`⚠` authoritative / `~` heuristic) with the hard/soft split, the fragment count (= hard boundaries + 1), and the per-window opaque disclosure (counts, the first five command rows with real transcript lines + an explicit remainder, the inspection search). `--patches`: interleaved segment headers (`─ SEGMENT n  L..L  turns..  ts..  (pre-state…)`) and boundary dividers, with the unified diffs, then the same disclosure. `--at` / `--salvage`: line-numbered known lines + explicit `??? lines A..B unknown` gap markers, then the boundary section + disclosure (`--salvage` ≡ `--at @latest`; an empty salvage names `at the latest state`). For those four, JSON is NDJSON — coverage rows add `hard_boundaries`/`soft_boundaries`/`opaque_commands`/`powershell_commands`/`suggested_search`, snapshot rows add `boundaries` + the same opaque fields, and every boundary object is `{line, source_session_id, source_line, turn_index, ts_utc, ts_local, cause, confidence, detail}` — + the trailing summary. A `--turn`/`--since`/`--until` window that excludes integrity-relevant events prints a stderr note naming how many fell outside. **Batch (`--files-from`)**: the `recovery-report.tsv` columns are `status · known_lines · total_lines · boundaries · bash_file · bash_opaque · target · written_to`, and the summary flags how many files carried boundaries or unparsed mutating bash in their window. **No silent truncation** — long inline content uses the `… (+N chars)` marker; JSON + `--out` are verbatim; skipped malformed lines are counted.

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

**TWO binding laws, in precedence order (v0.9.4) — never a path heuristic.** First law: the `plan_mode` attachment (below) when one exists. Second law, matching Claude Code's own resolution: when NO `plan_mode` attachment exists, the binding is the FIRST record carrying a VALID `slug` (CC's validity rule: lowercase alphanumeric head, alphanumeric/dash tail, <=120 chars), the plan path derived as `<plansDirectory or <claude-home>/plans>/<slug>.md` (v0.10.1, matching the 2.1.258 binary: `plansDirectory` is read from the MERGED settings — project `.claude/settings.local.json` over `.claude/settings.json` over `<claude-home>/settings.json` — resolved against the PROJECT ROOT - the transcript's FIRST recorded `cwd`, since v0.10.2: the harness memoizes the directory at its first access through the same cwd getter that stamps records, and the slug record's own `cwd` may already have followed a shell `cd` into a subdirectory - and accepted only when the result stays inside that root; an escaping value is refused and the default `<claude-home>/plans` applies. The pre-0.10.1 reading joined a relative value to the config home, which named a directory Claude Code never uses). This is how a forked clone — attachments stripped, slug records kept — still gets its plan re-injected by CC, and how csift now answers it correctly instead of "no plan". Rows carry `binding_source` (`plan_mode` | `slug-only`) and `minted_at_compaction` (true when the slug's first carrier is a compaction boundary — the fork mint site; an ordinary session's first carrier is a normal record). When a session enters Plan Mode, Claude Code writes a `plan_mode` **attachment record**:
```
{"type":"attachment","attachment":{"type":"plan_mode","planFilePath":"…","isSubAgent":…,"planExists":…}}
```
The session's bound plan is exactly that `planFilePath`. This matters because a session may freely Edit/Write OTHER sessions' plan files (ordinary tool calls on a `~/.claude/plans/…` path) — those are NOT its own plan, and a path-shaped guess would mis-attribute them. Only the `plan_mode` attachment binds a plan to a session.

**Plan-file storage facts.** Claude Code stores plans flat under `~/.claude/plans/` with a random three-word name like `nested-prancing-popcorn.md`; a subagent's plan gets an `-agent-<hex>` suffix. The name is **NOT derivable from the session id** — only the `plan_mode` attachment binds them.

**Why not derivable, and the pre-file window (binary-verified, CC `2.1.227`).** The three-word name is minted from a **CSPRNG with no seed** — decompiled generator (`Oyo`): `KCy(e)=randomBytes(4).readUInt32BE(0)%e` → `oin(a)=a[KCy(a.length)]` → `` `${oin(ADJ)}-${oin(GERUND)}-${oin(SURN)}` `` where `ADJ=["abundant","ancient",…]` (adjectives), `GERUND=["baking","beaming","foraging",…]` (gerunds), `SURN=[…"shamir","clarke","sphinx","waterfall"…]` (a mixed CS-pioneer-surname + noun list). No `Math.random`, no `mulberry32`/`seedrandom`/`cyrb53`, and no session id feeds the three `oin` calls — so the name carries **zero** information about the session and is **unknowable before `EnterPlanMode`**. (A sibling worktree namer `${adj}-${fox|owl|elm}-${Math.random().toString(36)}` uses the weak `Math.random` — do NOT confuse the two.) TIMING: the name is generated **at plan-mode entry** and written into the transcript as the `plan_mode` attachment **at that instant** — this WRITE precedes the on-disk `.md` by an arbitrary gap (the file lands only when content is first written). So there is a real **bound-but-not-created window** in which `plan`/`@plan` already resolve the full name while the file is absent; `plan_exists:false` (text `[missing]`) is the accurate state of that window, not an error. csift's knowledge point = the name's birth = the binding line, strictly **earlier** than the file. Verified live: a freshly-bound session reported `drifting-sniffing-waterfall.md [missing]` at its binding line with `plan_exists:false`, no such file on disk.

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

### 6.8 `verbatim` — turn-fidelity reconstruction (restore the back-and-forth a compaction summary clipped)

**Purpose.** A Claude Code **compaction summary** preserves TASK STATE (its 9-section synthesis: primary request, key concepts, file ledger, errors+fixes, plan, next step) in high fidelity, but provably **loses turn fidelity**: its "All user messages" section clips real prose turns to `...`-truncated bullets (measured: ~22 real user turns → ~17 bullets), and the assistant side collapses to a SINGLE verbatim quote (the last pre-compaction message). `verbatim` **supplements** the summary — it re-emits the verbatim user/assistant TURNS, in original order, each line carrying the JSONL line number (`Lnnnnn`) so a consumer can `Read` the raw transcript at the cited line. It does **not** re-derive task state (the summary owns that; duplicating it wastes budget and risks contradiction). The split of labor is the summary's own design — its trailer says "read the full transcript at `<path>`" for the exact content it generated; `verbatim` automates that pointer. (The measured basis for every default below, and the proof that the budget really reaches back across compaction boundaries: **§10.2**.)

**NOT the tail-peek tool.** `verbatim` is the compaction-fidelity SPECIALIST — its budget / round-trip-floor / richness heuristics all exist for the one job of RESTORING the turns a compaction summary already CLIPPED. To read a session's RECENT turns straight from the live transcript (no compaction involved), use **`show --turn N..`** (§6.11) — e.g. `show @<uuid> --turn -3..` fetches the last 3 turns verbatim. Reach for `verbatim` only when you need to un-clip a summary.

**Reuse, no re-parse.** `verbatim` sits on the §6.7 `recover` extraction layer verbatim: the same `scan_one_file` forward line-numbered `scan_lines_bytes` pass (the 1:1 jsonl line map), the same `group_turn_indices` (§6.4) turn delimiter, the same `Record` helpers (`is_genuine_user` / `genuine_user_text` / `agent_text` / `blocks` / `is_compact_summary`), the same `resolve_session_files` / `TimeWindow` / `timez` rendering. The byte prefilter is a SUPERSET of recover's, broadened with `"role":"assistant"` / `"type":"assistant"` probes so a pure-text assistant turn (carrying none of Edit/Write/Read/Bash) is never missed. The `Record`/`Block` model needs no change.

**Selection vs render order.** Selection walks **backward from EOF** (recency-first) so the budget is spent on what a resumed agent most needs; the emitted document is sorted **ascending** so it reads as a forward transcript. The backward walk is **transparent to `isCompactSummary` boundaries** (a summary is a turn MEMBER, never a delimiter — §6.4 / model.rs), so it reaches back across multiple compaction boundaries by default (verified on real transcripts: a 40K-char ellipsized budget spans 26 boundaries on a 35-summary session; `--max-compactions N` caps the crossing count).

**Budget allocation (two-phase).** `--budget <N>` (default 40000) bounds the whole reconstruction in chars, always (sizing rule ≈4 chars/token; the former `--budget-unit tokens` mode is removed). `--round-trip-fraction <F>` (default 0.5) is a **hard floor**: Phase 1 spends `budget·F` ONLY on round-trip-complete turns (user && assistant-EOT), walking recency-first; Phase 2 fills the rest with whichever single sides remain, user-first (the user wording is the scarcer, higher-signal loss). Without the floor an assistant-heavy tail recovers ZERO user turns (measured on a real pulse-shaped tail). The `[N tool calls]` marker cost is charged per turn (omitted when 0). Determinism: recency = descending line_no, ties by descending turn_index.

**Multi-agent-message richness (`--agent-msgs`).** A single user turn can own a LONG run of agent messages (a debugging/build chain the model narrates step by step) that the summary clips to its single §9 quote. Each `TurnSlice` carries EVERY agent-text record (`agents: Vec<AgentMsg>`); a derived `assistant_eot()` (== `agents.last()`) keeps the EOT anchor for dedup/round-trip/render. `--agent-msgs` decides how much of the run to restore — four modes: **`longest`** (DEFAULT) keeps the LONGEST agent message (the best one-message proxy for "where the substance is" — the summary's single quote is the turn's LAST message, often a ~50-char throwaway wrap-up, while the real finding sits in a MIDDLE one) **+** the first-when-substantive **+** every rich middle, collapsing the rest (including a short non-rich last) into a placeholder; **`eot-only`** forces last-message-only — byte-identical to the pre-expansion single-EOT output (the escape hatch); **`rich`** keeps the last always + the first by position privilege + each non-droppable middle; **`all`** keeps every message. `longest` applies to every multi-message turn; `rich` only filters a LONG run (`agents.len() > --agent-run-threshold`, default 6). A message is **kept when "rich"** — a cheap single-pass OR of a length gate (`>= --agent-rich-min-chars`, default 280) and a signal test (a number-of-substance, a commit-hash-like hex, a `file.rs:NNN`/`src/…` ref, a backtick code span, or a finding/decision lexeme). **KEEP-ON-DOUBT** is the spine: only a short (`< --agent-declaration-max-chars`, default 200) signal-less intent-verb opener (`let me …`/`now i …`) is collapsed; anything uncertain is kept (a wrongly-kept declaration costs ≤ one capped body; a wrongly-dropped finding is unrecoverable). A FUSED finding+declaration body trips a signal → kept WHOLE, its trailing declaration shed only by the char-ellipsis. A contiguous collapsed run renders as one `△ L{first}–L{last}  [X agent message(s), Y tool call(s)[, Z failed]]` placeholder carrying the fetchable line range + per-message attribution. `--keep-first` (default) keeps a turn's first message by position privilege **in `rich` mode** (`--no-keep-first` decides it as a middle); it has NO effect in `longest`, where the first is gated on length instead. `--profile heavy|light` bundles the thresholds (applied before the individual flags, explicit flag wins; the master `--agent-msgs` mode is unchanged, so `--profile heavy` alone still runs `longest`). Subagent transcripts get the SAME treatment via the shared selection path. The summed-cost == summed-emitted invariant holds with placeholders (the dropped bodies contribute zero cost; the placeholder line is charged like any emitted line).

**Ellipsis (role-asymmetric middle-truncation).** A unit over its role cap (`USER_CAP=600`, `ASST_CAP=900`, sized from measured medians) is **middle-truncated**, keeping head+tail, with an explicit `… [+K chars, L lines elided] …` marker (the line count uses the pre-normalization text; omitted for single-line user messages). The assistant head is both absolutely larger (900 vs 600) and a larger fraction (0.66 vs 0.60 → head 594/tail 306 vs user 360/240), because EOT prose front-loads context and back-loads the decision. Cuts are on `char` boundaries (UTF-8 safe). No content is fabricated; nothing is silently dropped.

**Dedup against the live summary.** The newest in-range summary is already in the resumed model's context (it IS the seed). A live-region turn (compactions_before == 0) whose 80-char normalized prefix matches the summary's §6 user bullets or §9 assistant quote is flagged `(also in summary)` and **demoted** (selected only after non-dup turns) — never silently dropped (a false positive must not lose a real turn). Turns predating an OLDER boundary are genuinely gone from context, so they are pure restoration, never deduped.

**Output.** Text groups under `SESSION <id>`: a budget-accounting header, then turn-by-turn `▽ Lnnnnn USER (ts)` / `[N tool calls]` / `△ Lnnnnn ASSISTANT (ts)` (one `△` line per KEPT agent message under `--agent-msgs rich`/`all`), with `══ compaction boundary · summary at Lnnnnn ══` banners at crossings, `(also in summary)` flags on demoted units, and `△ L{a}–L{b}  [X agent messages, Y tool calls, Z failed]` placeholders for collapsed agent-message runs. `--out` writes the full (un-terminal-truncated) reconstruction to a file while the summary prints to stdout. JSON (`--format json`) emits one VERBATIM (un-truncated `text`) object per unit (`line_no`, `role`, `ts_utc`/`ts_local`, `tool_calls`, `full_chars`, `rendered_chars`, `truncated`, `elided_chars`/`elided_lines`, `also_in_summary`, `compactions_before`) plus interleaved `{"kind":"compaction_boundary","line_no":…,"summary_chars":…}` records and, per collapsed agent-message span, a `{"kind":"collapsed_agents","agent_messages":…,"tool_calls":…,"failed":…,"first_line":…,"last_line":…}` record. **No silent truncation** — skipped malformed lines are counted and surfaced.

**Chunked output for hook injection (`--slice <N>` / `--window <N>`).** A Claude Code `SessionStart` hook can inject at most **10,000 CHARACTERS** of `additionalContext` (over-cap output is replaced by a file-path + preview, so the body is lost), so a >10K reconstruction must be fanned across several hooks. `--slice <N>` prints ONLY the Nth (1-based) chunk of the verbatim DOCUMENT (the `--out` body — turn units + boundary banners, NO scope/header/footer chrome) after packing its lines greedily into `≤ --window`-CHARACTER chunks. `--window` defaults to 10000 and counts CHARACTERS (Unicode scalars — the unit the cap itself counts, so CJK prose is not 3× over-charged the way bytes would be), hard-splitting any single line longer than the window on a char boundary so no chunk ever exceeds it. Slicing is **deterministic** (same session + `--budget` ⇒ identical chunk boundaries; concatenating slices `1..K` reproduces the document byte-for-byte), so N independent `SessionStart(compact)` hooks each request their own slice and the lock/ordering lives in the hook shell. An out-of-range `N` prints nothing (exit 0); `--slice` is **text-only** and **mutually exclusive with `--out`** and with `--format json`; `--slice 0` or `--window 0` errors. **`--slices <N>` (FIXED-FLEET mode)** pins the chunk COUNT to match a fixed set of N registered `SessionStart` hooks: csift fills the N newest-first slices with WHOLE turns (the per-role 600/900 caps are dropped — a turn is ellipsized only if it ALONE exceeds one window) and DISCARDS the oldest overflow, so the count never drifts to 5/6/7 as the conversation grows (the hook count never needs re-tuning); the realized budget becomes `N × --window` and `--budget` is ignored, with `--slice i` still picking which chunk to print. Without `--slices`, `--slice` keeps its legacy budget-driven, variable-chunk-count behavior. Safe to fire on every compaction — the drop-and-re-inject cycle (the old injection is summarized away pre-boundary while `SessionStart('compact')` re-injects fresh) prevents context pile-up.

**No-compaction self-diagnosis (v0.5).** `verbatim` restores what a COMPACTION clipped — so on a session with ZERO compaction summaries there was nothing to clip. A non-`--slice` run prints, per such session, a stderr note steering to the right tool: `csift: note: @<id> has no compaction — nothing was clipped; for plain reading use `csift show @<id> --turn <N|A..B|-k..>` (full records, no budget)`. (stdout still carries whatever verbatim reconstructed; the note is advisory.)

**Windowing** matches every sibling: `--turn N|A..B|N..|-k` (inclusive, 0-based genuine-user order, the shared range-token grammar — `-3..` = the last 3 turns) INTERSECTS (AND) `--since`/`--until` (ISO8601 / relative). **A target is REQUIRED** — `--budget` applies PER session, so a bare `csift verbatim` (0 targets ⇒ ALL projects everywhere else — `list` alone still SCANS every project but DEFAULT-caps its emitted rows to 50, a non-silent `dropped_by_cap`) would realize budget × every session of every project; it hard-errors with the valid target forms (`@<uuid>`/`@main`/`@<agent-id>`/project path/`--sessions-from`).

**Example invocations:**
```bash
csift verbatim .                                   # default 40K recon (longest agent msg + rich members per turn)
csift verbatim @<uuid> --budget 12000              # a 200K-context-sized recovery (~10-15K)
csift verbatim @<uuid> --budget 40000 --format json # machine-readable, line-numbered
csift verbatim @<uuid> --round-trip-fraction 0.6    # weight harder toward complete round-trips
csift verbatim @<uuid> --agent-msgs eot-only        # force the old single-EOT (last-message-only) output
csift verbatim @<uuid> --profile heavy              # longest mode, lower thresholds (max fidelity)
csift verbatim @<uuid> --agent-msgs all             # every agent message, no filtering
csift verbatim . --budget 40000 --out /tmp/verbatim.md  # full reconstruction to a file
csift verbatim . --window 9000 --slices 4          # fixed 4-chunk fan-out for 4 SessionStart hooks
csift verbatim . --window 9000 --slices 4 --slice 1  # print the 1st of those 4 chunks
```

### 6.9 `image` — list + extract the images a session carries

**Purpose.** A pasted/attached image (and a tool-result screenshot) is stored **inline** on a record as an `{type:"image", source:{type:"base64", media_type:"image/png", data:"<base64>"}}` block — verified on real `~/.claude/projects` data (2026-06-16): a single user record commonly carries several. The bytes are in the jsonl, so `image` lists them and decodes them straight back to files; nothing was externalised. This is the way to get a sent image back out of a transcript without hand-parsing base64.

**Two addresses, by design.**

- **`#N` — the session handle** (preferred). This is the SAME `[Image #N]` number the model sees and refers to ("re-share #32") — Claude Code renders pasted images as `[Image #N]` text markers, and `image` recovers `N` by positionally zipping a record's markers with its image blocks. So a consumer reading a `[Image #32]` reference (in `verbatim`/`search` output, or in its own context) addresses it directly: `--id 32` (bare digits — a `#`-prefixed `--id` is REJECTED, since unquoted `#` is a shell comment). **`#N` is NOT unique across a session** — CC numbers per-prompt and **reuses** low numbers across prompts (`history.ts`: "unique within a single prompt but not across prompts"). When a `#N` names **>1 distinct image**, `--id N` is **AMBIGUOUS and ERRORS** with the occurrence list — each occurrence's `t<turn>` / `L<line>i<n>` locator / uuid / time / excerpt-around-`[Image #N]` — rather than silently guessing. Disambiguate by the exact locator, or by narrowing scope with `--since`/`--until` (a time window), `--turn`, or `--uuid` (each can be **pre-applied** so `#N` is already unique — e.g. `--since 1h` for the last hour). When a record's marker count doesn't match its image count, `#N` is left unset for that record (only the locator addresses it) — never mis-attributed.
- **`L<line>i<n>` — the exact locator** (always unambiguous). The 1-based JSONL line of the carrying record + the 1-based ordinal of the image among that record's image blocks (a direct `Block::Image`, OR an `{type:"image"}` element nested in a `tool_result` content array, counted in document order). Stable because the transcript is append-only, and consistent with the `Lnnnnn` line refs `recover`/`verbatim`/`search` already emit. Use it to pin one specific occurrence regardless of `#N` reuse.

`verbatim` and `search` both surface these ids inline (`[N image(s): #32, #33, …]` under a user turn / as a hit suffix), so an id seen there feeds straight back into `--id`. Default action is to **LIST** (deduped — see Behaviour); `--out <DIR>` switches to **EXTRACT**.

**Args (matches `cli::ImageArgs`):**
| flag / positional | type | default | meaning |
|---|---|---|---|
| `[PATH]…` | repeatable positional | all projects | project target(s) (§2.3); a `@<uuid>` positional restricts to one top-level session |
| `--id ID` | repeatable + comma | none | address images by the handle NUMBER as bare digits (`--id 32,33` — the `#N` the model saw; a `#`-prefixed input errors with "drop the #", because unquoted `#` is a shell comment) or the exact `L<line>i<n>` locator (`--id L6812i1,L6812i2`); without `--out` filters the LISTING, with `--out` selects what to extract. Both forms are per-transcript, so `--id` needs a single transcript in scope (pin with `@<uuid> --no-subagents`). An ambiguous handle errors (above) |
| `--since` / `--until WHEN` | string | none | time bound (ISO8601 or relative `2h`/`3d`, system-local) — narrows the image set, a `#N` disambiguator |
| `--turn N\|A..B\|N..\|-k` | string | none | 0-based inclusive turn window — a per-transcript `#N` disambiguator (shared range-token grammar; turn indices come from the shared §6.4 delimiter). |
| `--uuid PREFIX` | string | none | restrict to the record whose uuid starts with this — a `#N` disambiguator (the uuid is shown in the ambiguity list / JSON) |
| `--out PATH` | path | none | EXTRACT. The path's EXTENSION drives the format (the `convert in out.jpg` idiom): a **directory** (or any path with no `png`/`jpg`/`jpeg`/`gif`/`webp` extension) writes each image auto-named `<session-short>[-img<N>]-L<line>i<n>.<ext>` in its SOURCE format; a path WITH one of those extensions writes the **single** selected image to exactly that file, CONVERTING if the format differs (see below). >1 image with a file path is an error. Without `--out`, only LIST |
| `--no-subagents` | bool | spans subagents | restrict to the top-level session; subagent transcripts are scanned by default (a tool screenshot may live there). The only span flag. |
| `--format text\|json` | enum | `text` | output format |

**Behaviour.** Same scan shape as `recover`/`files` (mmap + a pre-JSON image byte prefilter + parse only candidate lines, line-numbered 1:1). The media type (and thus the default extension) is read **per image** from its `source.media_type` — never assumed PNG; real sessions carry a mix (png, jpeg, …), and the `~/.claude/image-cache/*.png` mirror is a lossy `.png` rename that the inline bytes do not share. The **LISTING is content-deduped**: the same image re-injected across context windows (every prompt re-sends attached images + compaction re-includes them) is shown **once**, keeping its latest occurrence, via a cheap `<len>:<head>:<tail>` base64 fingerprint; two DISTINCT-content images sharing a `#N` both survive (so the reuse is visible — and `--id N` errors on it). Decoded size is **estimated** for the listing (4 b64 chars → 3 bytes) without decoding the large payload; extraction decodes in full and reports the exact byte count. A `source.type == "url"` image has no inline bytes — it is reported (with its URL), never fabricated into a file. **No silent truncation / no silent miss:** an explicitly-requested `--id` that matches nothing is an error; a base64 that fails to decode is an error (never a wrong file); skipped malformed lines are counted. Base64 decoding is a small in-crate standard-alphabet decoder; format transcoding uses the `image` crate + libwebp (the heavyweight dependencies — only `image.rs` touches them).

**Format conversion (output extension).** A directory output (no image extension) writes the original bytes under the source-inferred extension (the written path + extension is echoed, so a caller that omitted an extension sees what it got). A file output whose extension **differs** from the source is **converted, never rejected**: →png is lossless, →jpeg is **quality-90 lossy** (brief note), →gif is **Floyd-Steinberg dithered** into a ≤256-color NeuQuant palette (brief note; a sub-2px image skips the dither), →webp is **quality-90 lossy** via libwebp (the `webp` crate; brief note). A file output whose extension **equals** the source writes the original bytes unchanged (so an animated GIF stays animated). An **animated GIF** converted to a still format yields its **first frame**, with a warning carrying the frame count + total duration (`animated GIF (3 frames, 1.5s) — extracted the first frame only`). A decode/encode failure is surfaced (never a wrong file).

**Text output example** (leads with the `#N` handle when known, else the bare locator):
```
#1    L6802i1  image/png ~440 KB  2026-06-11 10:25:50 AEST(UTC+10)
#2    L6812i1  image/png ~440 KB  2026-06-11 10:27:10 AEST(UTC+10)
#3    L6812i2  image/png ~252 KB  2026-06-11 10:27:10 AEST(UTC+10)
55 image(s) · 3 transcript(s)
```
(A subagent image row is suffixed `  SUBAGENT <hex> · parent <uuid>`.) The **listing/JSON** is one object per image (`handle`, `seq`, `id` [the `L<line>i<n>` locator], `line_no`, `img_index`, `session_id`, `is_subagent`, `parent_session_id`, `source_kind`, `media_type`, `b64_len`, `est_bytes`, `url`, `record_uuid`, `ts_utc`/`ts_local`) + a trailing `{images, transcripts, skipped_lines}` summary. **Extract JSON** carries `path`, `bytes`, `media_type` (output), `source_media_type`, `converted`, and `notes` (the conversion/first-frame warnings).

**Example invocations:**
```bash
csift image @<uuid>                             # list every image (deduped), id · type · ~size · time
csift image . --format json                     # machine-readable listing
csift image @<uuid> --out /tmp/imgs             # extract ALL to a DIR (source formats, auto-named)
csift image @<uuid> --no-subagents --id '32,33,34,36' --out /tmp/imgs       # re-share by handle
csift image @<uuid> --no-subagents --id 1 --since 1h --out /tmp/imgs        # disambiguate a reused #1 by time
csift image @<uuid> --no-subagents --id L6812i2 --out /tmp/shot.jpg         # one image to a FILE → convert to jpeg
```

### 6.10 Elicitation sidecar — the TRANSPARENT merge (AskUserQuestion / ExitPlanMode / MCP)

There is NO `pending` subcommand. Three elicitations BLOCK a session on a human yet leave NO usable live trace in the native transcript: **AskUserQuestion** + **ExitPlanMode** (CC buffers the whole assistant turn until answered — see §4: the assistant tool_use is committed to the persisted message array only after the interactive tool resolves, so nothing is on disk during the pending window) and **MCP Elicitation** (the inner `elicitation/create` request is an in-memory MCP-client callback, never a transcript record). Reading the jsonl cannot detect these; csift instead reads a SIDECAR and MERGES its unresolved-pending records TRANSPARENTLY wherever it reads a session.

**Sidecar:** `<sidecar_dir>/elicitations.jsonl` (the `<uuid>/` dir beside `subagents/`, via `crate::subagent::sidecar_dir_for_session` / `crate::elicitation::sidecar_path`), written by the `csift-elicitation-marker` CC hook (SKILL.md recipe) on the `PreToolUse`/`PostToolUse` (AskUserQuestion|ExitPlanMode) and `Elicitation`/`ElicitationResult` events. NEVER the native transcript (no pollution). Append-only; every line carries `csift:"elicitation-marker-v1"` + `csiftPhase`∈{`pending`,`resolved`} + `csiftKind`∈{`AskUserQuestion`,`ExitPlanMode`,`mcp-elicitation`} + `csiftKey` (pair id) + `timestamp`. A `pending` line is shaped like the NATIVE record CC will eventually write (an AUQ/ExitPlanMode `tool_use` on a `type:"assistant"` record; an MCP `type:"system"` record), so it classifies naturally; a `resolved` line is a lightweight `type:"csift-elicitation-resolved"` close marker. **Keyed by the TOP-LEVEL session** — the hook's `session_id`/`transcript_path` is always the top-level/leader uuid (process-global, never per-subagent); AskUserQuestion/ExitPlanMode are main-thread-only, and a subagent's MCP elicitation surfaces under its top-level session (agent-blind — the Elicitation hook carries no `agent_id`).

**Merge semantics (`crate::elicitation::unresolved_pending`).** Group the sidecar by `csiftKey`; a key with a `pending` and NO `resolved` is UNRESOLVED → its pending record is emitted (exactly the one MISSING from the native transcript — once resolved CC wrote the real record, so the pending is paired off and DROPPED ⇒ no duplicates, the auto-dedup is the whole point). **search / verbatim / list** read the sidecar when they read a TOP-LEVEL session (subagent transcripts have none — the sidecar is keyed by the top-level uuid) and merge the unresolved-pending records as native: `classify` labels a pending marker **`agent.tool.use`**, so `search` matches all three kinds under `-t agent.tool.use` (an AUQ/ExitPlanMode `tool_use` via its block; the MCP `system` record — which has no `tool_use` block — via a guarded `collect_record_hits` arm that matches its top-level `content` string and tags it `agent.tool.use` named by `csiftKind`, so `search MCPPROBE` / `-t agent.tool.use` find it; gated to a no-tool_use marker so AUQ/ExitPlanMode never double-emit), `verbatim` appends each as its own pending turn unit (ranked most-recent), `list` annotates the row + surfaces the pending kind. **files / recover** carry no file ops for these records (no output), so they deliberately do not read the sidecar. A merged record has NO physical line: it renders **`(elicitation sidecar)`** in place of `Lnnnn` (never a fabricated L) and carries JSON `source:"elicitation-sidecar"` + null `line`/`line_no`; every surface that merged ≥1 record emits the EXACT note **`with elicitation sidecar`** (JSON `with_elicitation_sidecar:true`). Malformed sidecar lines are skipped + COUNTED into that surface's `skipped_lines` (never silent); a missing sidecar dir/file ⇒ no merge (not an error); near-free when nothing is pending. **Targeting rejection:** `resolve_session_files` `bail!`s when a `*.jsonl` target `is_sidecar_path` (basename `elicitations.jsonl` OR a content-sniff where every line is a `csift`-marked record) — the sidecar is read automatically, never searched directly. Verified live on 2.1.191: the `pending` marker is written the instant the picker appears and persists through the entire wait while the native record stays buffered.

---

### 6.11 `show` — fetch specific record(s) of ONE transcript

`csift show TARGET (--line N|A..B,…)…|(--uuid U,…)…|(--turn N|A..B|-k) [--raw] [--max-count N] [--format json]`

The reader companion to `search` (search FINDS with match-centered excerpts; show
FETCHES the records you NAME, full). TARGET resolves to exactly ONE transcript:
`@<uuid>`/`@<uuid-prefix>` → that top-level file (never spans), `@<agent-id>` → that
subagent's file, a `*.jsonl` path → itself; ≠1 file is a hard error. No selector is a
hard error naming the resolved path (csift never dumps a whole transcript). Exactly ONE
addressing mode: `--line` (jsonl line), `--uuid` (record uuid), or `--turn` (turn index)
— they are mutually exclusive (clap `conflicts_with`).

- Addressing modes (pick ONE): `--line N|A..B|N..|-k` (1-based jsonl line, repeatable /
  comma-joined), `--uuid U` (record uuid, repeatable / comma-joined), or `--turn
  N|A..B|-k` — a 0-based TURN index/range in the SAME range-token grammar, whose numbering
  is IDENTICAL to the `·tN` in `search`'s exchange headers. `--turn` fetches EVERY record of the named
  turn(s) (the whole back-and-forth), so `show @<uuid> --turn -3..` reads the last 3 turns
  straight from the live transcript — the tail-peek / monitoring path `verbatim` (§6.8) is
  deliberately NOT.
- Rendered default: each addressed record FULL through search's per-record pipeline
  (classify → labels, plan pointers, tool pairing, elicitation-sidecar merge; a record
  yields one row per rendered unit). Only `role:user`/`role:assistant` message lines
  are renderable — a single-line miss on a metadata/attachment line is an error that
  points at `--raw`; a line-RANGE covering some non-record lines prints the count note
  `N line(s) in the addressed range are not records (metadata/attachment — inspect with
  --raw)` and renders the rest (JSON summary `non_record_lines`).
- `--raw`: the VERBATIM jsonl line bytes (malformed/torn lines included — that is the
  point; the escape hatch for fields csift does not render). Mutually exclusive with
  `--format json`; reads the transcript file only (no sidecar merge). Caps by LINE with an
  equivalent stderr continuation note.
- Flood guard (v0.5): a default cap of `DEFAULT_SHOW_CAP = 200` record units (open ranges
  — `--line ..` / `--turn ..` — address a whole transcript) — keep-FIRST; the drop ALWAYS
  reports the exact continuation: `+N more record unit(s) beyond the 200-unit cap ·
  continue: csift show @<id> --line A..B  (or --max-count 0 = uncapped)`. `--max-count N`
  overrides; `--max-count 0` = UNCAPPED (the crate-wide convention).
- Exit law (v0.5): an explicit `--line N` / `--uuid U` / `--turn N` / `--turn A..B` that
  resolves to zero records = HARD error naming the domain (a turn miss: `no such turn(s):
  t99 — the transcript has 2 turn(s) (t0..t1); …`). Open / from-end forms (`N..`, `..N`,
  `-k`, `..`) CLAMP to the file (tail-peek robustness — `--turn -9..` on a 2-turn session
  is fine), erroring only when the clamped range still yields nothing. Applies in both
  rendered and `--raw` modes.
- JSON: header + `{"kind":"record", …trio…, turn_index, line(null for a sidecar
  record), uuid, label, labels, tool_name, from, to, pairing, tool_use_id, source,
  ts_utc/local, text, image_ids}` rows + summary
  `{records, dropped_by_cap, refetch_remainder, non_record_lines, skipped_lines,
  with_elicitation_sidecar}` (`refetch_remainder` = the `csift show` continuation command
  when the cap dropped rows, else null).

### 6.12 `stats` — one-scan aggregates per session

`csift stats [PATH|@…]… [--since W] [--until W] [--turn N|A..B|-k] [--max-count N] [--sessions-from F] [--subagents|--no-subagents] [--format json]`

Per session (spans subagents by default; each transcript is its own row): physical
`lines`; a whole-file `line_types` census (every physical line by its top-level `type`;
a FILE fact, never windowed - v0.8.1); `user_records`/`assistant_records`; `turns`
(the shared §6.4 grouping); `compactions` (isCompactSummary count); first/last
timestamps + duration; `tokens` per model (`message.usage`
input/output/cache_read/cache_creation, counted ONCE per API message: CC repeats the
identical usage object on every per-block record, so sums dedupe PER FILE by
`message.id` with a per-field MAX - v0.9.2; an id-less record counts on its own; the
scope TOTAL sums rows and never dedupes across files); `narration_blocks` per model +
`unknown_thinking_tags` (v0.9.2, §5.1 `agent.thinking.narration`; block counts only -
the token split is not derivable from the jsonl); `tools` call counts by tool_use name. `--since`/`--until` bound the COUNTED records (turn
grouping runs over the windowed set); `--turn`
windows on the genuine-turn axis — indices computed over the FULL transcript (stable),
then INTERSECTED (AND) with the time window. Text prints per-session blocks + a scope
TOTAL block when >1 session; JSON is header + `kind:"session"` rows + a summary carrying
scope totals (`sessions`, `turns`, merged `tools`/`tokens`, `skipped_lines`). One
fixed shape — no view modes, no tuning flags. `--max-count N` (opt-in, default
unlimited) bounds an unscoped run's emitted per-session rows to the N most-recently-active,
reporting the drop (never silent); the scope TOTAL block then covers the shown subset.
`--max-count 0` = UNCAPPED (the crate-wide convention).

### 6.13 `status` — one-shot LIVE verdict (the live-truth pair, v0.9.0)

`csift status <TARGET> [--background-since WHEN] [--ignore-background RE]… [--subagents|--no-subagents] [--format json]` — exactly ONE session per call.

A deliberate, documented departure from the forensic contract: `status` (and `wait`)
answer "NOW", are point-in-time, and are explicitly NON-reproducible; every other
command stays reproducible. The verdict is a THREE-WAY JOIN, never a single-surface
inference: (1) the harness session registry (`<claude-home>/sessions/<pid>.json` -
status transitions land sub-second but the file is TRANSITION-written only, never a
heartbeat: an hours-old `statusUpdatedAt` just means the state has not changed; covers
TOP-LEVEL interactive sessions only, a subagent has no row and csift never fabricates
one); (2) the transcript tail state machine (a bounded 512KB tail window, complete
lines only: an unreturned `tool_use` at the tail = a tool in flight; the last assistant
`stop_reason` is trustworthy on the MAIN lane and normally null mid-message on
subagents); (3) owner-process liveness (unix: a `ps`-based probe with a `/proc/<pid>`
fallback where ps lacks the flags, e.g. busybox - v0.9.1; Windows, v0.10.1: PowerShell
`Get-Process -Id` with `StartTime.ToFileTimeUtc()`, `tasklist` for liveness alone when
PowerShell cannot run; guarded against pid reuse by the process start time: the
registry renders `procStart` in UTC asctime on unix and as a FILETIME tick count on
Windows, while `ps lstart` renders local - every form parses to an instant, compared
with a 2s tolerance; a missing/unparseable start time degrades to a pid-only probe
AND says so; a row whose `pidDomain` names another domain -- `darwin`, `linux`,
`win32:<host>` -- is never probed and the verdict says so). The registry `status` is
the binary's closed set `busy | shell | idle | waiting`: `busy` = loading or
delegating, `waiting` = blocked on a dialog (a question, a permission prompt, a plan
approval, a sandbox or worker request), `shell` = `idle` relabeled while a background
shell task is open, so `shell` is NEVER a running shape (v0.10.1 corrected the
0.9.0-0.10.0 reading that made an idle session with a dev server report `running`);
a `claude -p` session writes a row with `status: null` that never transitions. Child
liveness = each child transcript's own tail (v0.9.4: the closed lane-state set is
`in-flight` [unreturned call] | `generating` [tail RECORD younger than 300s AND last
assistant stop_reason != end_turn -- record-tail time, mtime is not consulted; the
conjunction is load-bearing: recency alone misread mid-generation lanes, stop_reason
alone would mark 73% of dead lanes live] | `settled` [also, v0.10.1, whenever the main
transcript already carries that agent id's completion notification: the harness's
own word outranks the tail]) + the incremental workflow journal (`started` minus
`result` = agents in flight). Settled lanes FOLD: text prints one count line, JSON
`children[]` carries live lanes only beside `settled_children`. A tasks section reads
the harness task list (`<claude-home>/tasks/<uuid>/` or `tasks/session-<first8>/`,
both real dir-name forms, merged): open tasks render in_progress-first with
`blockedBy`, completed ones fold to a count; JSON `tasks` is null with no dir, `[]`
for an empty one, beside `tasks_completed`. Human-in-the-loop blocks come from three
instruments: the elicitation sidecar (the only one that sees a pending
single-question AskUserQuestion), the registry `waiting` status, and an unreturned
`AskUserQuestion`/`ExitPlanMode` at the main tail (a multi-question ask is written at
question time since CC 2.1.258, section 4).

BACKGROUND TASKS (v0.10.0; the full mechanism + measurements in the v0.10.0 ledger
①): a whole-main-file scan behind a five-needle prefilter lists every OPEN
backgrounded shell, async agent and Monitor arm as a `bg` row (kind, id, launch
instant + age, description or command, the output file's bytes + last write) and
folds closed ones to counts (completed / failed / killed / stopped / timed out, plus `blocked`, the remote-agent notifier's fifth value, and `with an unknown status` for any literal csift does not know - v0.10.3, never booked as completed);
launches come from every lane, completions normally from the main file, and from the owning agent's lane when the pulse was addressed to it (v0.10.3; `wait --until notification` reads every watched lane), joined by the
launching tool_use id across the three carriers (user record / queue-operation /
queued_command attachment). NOT RETURNED IS NOT PROOF OF RUNNING (CC's own orphan
summary: a UI stop, a Monitor timeout or agent teardown leaves no transcript marker);
the section says so. THE LENS (`--background-since WHEN`, `--ignore-background RE`)
decides which open tasks COUNT toward the verdict; every task is still listed, an
excluded one marked with its rule. An agents-stopped notice names no id, so csift
notes it but cannot mark which agents it stopped.

LAST MESSAGES: the newest human prompt (an automation pulse as its label) and the
newest assistant message as 400-char excerpts (`(+N chars)` when clipped; refetch =
`show @<id> --turn -1`) under the one-line warning that an excerpt is a partial view
of the final state, never a review of the work. `tail_state` names the main lane's
instant in words (`in a Bash call for 34s` / `generating (…)` / `idle (…)`).

VERDICTS (closed set of SEVEN since v0.10.0, precedence dead > hitl > running >
children > background > eot > unknown): `running` | `waiting-children` |
`waiting-hitl` | `idle-background-open` (a clean end_turn with N counted background
tasks not returned - neither running nor stopped) | `idle-eot` | `stale-dead` |
`unknown`. Every verdict ships its evidence rows (surface + value + age; the
`background` row summarizes the counts), and every degradation is STATED in the
output: a skipped reuse guard, a missing registry row, a foreign pid domain, and the
honesty note that a pending PERMISSION prompt leaves no transcript trace and would
masquerade as idle - only a CURRENT registry row shows it (status `waiting`), so the
note names the status the row carries (v0.10.1). Growth alone is never activity (a main transcript grows while idle - enqueues,
attachments); what grew must be classified. JSON: header (session trio) → one
`{kind:"verdict"}` row (verdict, evidence[], children[], settled_children, tasks[],
tasks_completed, pending[], background{}, last{}, tail_state, notes[]) → summary.

### 6.14 `wait` — block until a session condition fires (v0.9.0)

`csift wait <TARGET> --until COND… --timeout S [--interval MS] [--background-since WHEN] [--ignore-background RE]… [--subagents|--no-subagents] [--format json]`

Conditions (closed grammar, OR semantics, first hit wins; an unknown head is a hard
error naming the set): `stop` (verdict becomes idle-eot or stale-dead - a TRUE stop;
`idle-background-open` never satisfies it, v0.10.0) · `hitl` ·
`auq` (a sidecar pending ask, or a native AskUserQuestion tool_use landing) ·
`notification[:REGEX]` (a `<task-notification>` in the MAIN transcript - they never
land in children) · `tool:NAME[:REGEX]` (any watched lane) · `write:PATH_RE[:LINE_RE]`
(Write/Edit/MultiEdit/NotebookEdit) · `verdict:V`.

BASELINE SEMANTICS (the monitor/query boundary): every watched file's byte length is
snapshotted at startup; ONLY bytes appended after those offsets are events - a
condition already satisfied by history NEVER fires (history is `search`'s job). After
the snapshot a READINESS line prints to stderr (`csift: watching N file(s)…`), so a
scripted caller orders its own trigger after it and the race is gone; the same line
names the timeout and the lens. Reads are incremental (byte offsets, complete
lines only - a torn tail is held and re-read next poll); polling is adaptive (200ms
floor to a 2s ceiling; `--interval` pins it). Child lanes and the elicitation sidecar
born AFTER the watch starts join it automatically with a ZERO baseline (their whole
content is post-start; a sidecar's dir is typically created by the first pending ask -
v0.9.2). Verdict-class conditions re-run the §6.13 join each poll.

`--timeout` IS REQUIRED (v0.10.0, BREAKING): a background task can be designed
never to return (a dev server, a watcher), so an unbounded wait on `stop` is a bug; a
call without it is rejected with that reason. On EVERY exit the report carries
`at exit` (the main lane's instant in words), `activity` (a census of the records
that landed after the baseline, classified through the search engine: tool calls by
name, thinking, messages, prompts, notifications, lanes), the `bg` rows and the last
messages (§6.13). HOW TO WAIT: `status` first and read the `bg` rows; pick the lens
(`--background-since now` for "only what launches after I start", `--ignore-background`
for the services known never to return); always bound; on 124 read the report.

EXIT CODES: 0 = a condition fired (the output names which, plus the closing
assessment); **124** = `--timeout` expired (the GNU timeout convention - the ONE
documented exception to the crate's 0-vs-non-zero exit law, §4; JSON still emits
`fired:"timeout"`); any other non-zero = error (a missing `--timeout` is one). JSON: a
single `{kind:"wait"}` object (fired, verdict, waited_secs, at_exit, activity{},
evidence[], background{}, last{}, notes[]).

## 7. Performance design (NON-FUNCTIONAL contract)

Measured corpus reality (real `~/.claude/projects`, 2 437 jsonl files): largest file **225 MB / 115 879 lines**; **line-length skew is the dominant fact** — min 82 B, p50 813 B, p90 2.6 KB, p99 14.9 KB, **max 400 KB single line**. 98% of `user` records are tool_result-carriers; 54% of all records are `attachment` noise. ⇒ never materialize whole-line `String`s for non-candidates; prefilter aggressively before any JSON parse.

### 7a. mmap + memchr line scan (full-scan path) — use `memmap2::Mmap`, NOT BufReader

Files are 75–225 MB, read-only, scanned once front-to-back. `memmap2::Mmap` (immutable; **never** `MmapMut`) gives the kernel page-cache + readahead and a single `&[u8]` for zero-copy `memchr::memchr_iter(b'\n', &mmap)` line splitting and zero-copy `&str` slices into matched lines. BufReader would copy **every** line (incl. the 98% non-candidates) into a line buffer. **Truncation race:** a live session can grow under mmap — open + mmap once, treat length as fixed-at-open, tolerate a torn final `\n`-less fragment (skip it, like the tail carry). A SHRINK is the one case the map cannot contain: a page touched past the new end faults with SIGBUS, and no Result catches it (measured 2026-09-04: a 1 MB file mapped, truncated to 4 KB by another writer, read past the new end - the process dies with signal 10 on macOS, 7 on Linux). Claude Code does rewrite a transcript in place (a rewind tombstone truncates and rewrites the tail; an armed local GC rewrites on compaction; ledger MISC-020), so since v0.10.4 the readers that run REPEATEDLY against a live file - `wait`'s per-poll increment and the `status`/`wait` tail window - use plain positional reads (`parse::read_range` / `read_tail`: a shrink returns fewer bytes), and `wait` reports a shrink (the baseline moves to the new end). The one-shot full scans keep the map: their sub-second life bounds the exposure, and the performance contract rests on it. For files < ~256 KB, skip mmap and `fs::read` (setup cost not worth it) — but all perf-relevant files are huge, so mmap is the default.

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

1. **Label prefilter** (cheap `memchr::memmem::Finder`, built once per needle, reused across all lines): a line that could match a requested `-t` selector must contain a marker substring — `"thinking"`, `"tool_use"`, `"tool_result"`, `"role":"assistant"`, etc. A line lacking **all** active markers is dropped pre-JSON. (Genuine-`user` is the exception: a `"type":"user"` line passing the substring gate still needs structural disambiguation from carriers, so it is parsed — but that's only ~8.8 K of 115 K lines, and only when the `user` role is active.) The marker→leaf wiring widened with the taxonomy but the mechanics are unchanged.
2. **Keyword prefilter** (the user regex): a REQUIRED-NEEDLE set is derived from the pattern (`required_needles`, v0.9.4): a whole-pattern plain literal is the fast path; a pattern WITH metacharacters goes through a NECESSITY-ONLY walk of its parsed HIR (`hir_needles`) — a literal node contributes its longest SAFE run (whitespace and JSON-escaped chars SPLIT runs, since a sub-run of a required literal is itself required), a concatenation contributes its strongest single part, an alternation gates only when EVERY branch contributes (result = the union, capped at 8 needles), and min-0 repetitions / classes / anchors contribute nothing. The safety predicate applies PER NEEDLE — whitespace-free, JSON-escape-free (`json_escapes_in_string`), ≥3 bytes — because JSON encoding + render-time whitespace normalization rewrite exactly those bytes; a needle passing it survives verbatim in the raw line whenever the decoded text matches, so the historical "no HIR extraction" clause is retired with its RATIONALE intact (it guarded against unsafe or non-required extraction, and this walk emits neither). Prefilter = ANY needle present against the raw bytes: per-needle `memmem` when case-sensitive (`Prefilter::Literal`/`AnyLiteral`), one `(?i)`-escaped-literal alternation bytes regex when smart-case-insensitive (`CaselessLiteral`, Teddy-class). This is what turns `search "TodoWrite.*legacy|legacy.*TodoWrite"` from a full-regex crawl of every candidate line into a near-literal-speed scan (both branches require `TodoWrite`, so the union dedups to one needle and the §7f whole-file gate applies). A raw SYNTHESIZED-TEXT marker set (per-needle `memmem::Finder`s — deliberately NOT one Aho-Corasick automaton; task-notifications, AUQ, rejections, `-t`-gated compact boundaries, flag-gated persisted pointers) exempts the records whose matchable text is fabricated rather than verbatim. Empty pattern ⇒ pure filter (skip stage 2). Non-anchorable pattern ⇒ run `regex::bytes::Regex` directly on raw bytes (still far cheaper than JSON parse).

Only lines passing **both** gates reach `serde_json::from_slice` → typed `Record` → exact field-level match + round-trip stitching. **`simd-json`/`sonic-rs` are NOT used** (and not a default dep): once the prefilter drops 98% of lines, total serde time is a few ms even on 225 MB; simd-json needs an owned, padded, mutable buffer (fights the zero-copy mmap `&[u8]`) and sonic-rs adds unsafe-heavy deps for a speedup on a tiny byte fraction. Capture most of the win instead with `serde_json::from_slice` + `#[serde(borrow)]` zero-copy fields. (Keep simd-json as a possible future `--feature`, not default.)

### 7e. Parallelism — rayon ACROSS files, not within (by default)

- **Across files/sessions:** `rayon` `par_iter` over the file list — the big win for `list` (2 437 files) and multi-target `search`. Each file is an independent mmap + scan; embarrassingly parallel; near-linear to core count. **This is the only parallelism csift needs by default.** rayon's default pool caps concurrency at core count, so at most N files are mmapped at once (virtual address space, not resident until touched).
- **Within a single file:** ALSO parallelized (`parse::scan_lines_parallel` — newline-aligned chunks, exact 1-based line numbers, byte-identical results to the serial scan). This is what rescues the SINGLE-session target (`search`/`verbatim`/`files`/`recover` on one 200MB+ transcript), where the across-files fan-out degenerates to one core. Below 4 MiB it runs as a single chunk (split overhead isn't worth it).
- **Determinism:** collect per-file results into a `Vec` indexed by input order; sort/merge after the parallel section so `text` and `--format json` output are deterministic regardless of completion order.
- **`--max-count`:** cap per-file then globally, and **report the dropped count** — never silent truncation.

### 7f. Whole-file match gate (`search`)

When stage-2 anchors a prefilter (7d) and the invocation is not an addressing fetch (`--line`/`--uuid`), a PARALLEL pre-scan (the same newline-aligned rayon chunking as the full scan — never a serial whole-mmap pass) can prove no candidate line matches: per line, no literal occurrence and no synthesized-text marker. Every emitted exchange requires ≥1 regex hit, so the file is skipped without building a single `Record`; a relaxed atomic flag short-circuits the pre-scan the moment a literal (or a conservative marker) hits, so a file WITH matches pays only a skim before its normal full scan. VERIFIABLE marker lines (task-notifications, AUQ answers, compact boundaries) do not force the full scan: stage 2 parses just those lines, re-renders their synthesized texts through the shared engines, and regex-checks them — only a synthesized match reopens the full scan. Two obligations survive the skip: a gated file's candidate lines were each syntax-validated (`parse::validate_line_syntax`, allocation-free) so the malformed-line count stays exact (§0 no-silent-truncation), and a top-level session's pending elicitation-sidecar records (which live OUTSIDE the file) force the normal scan when present. This is the difference between "parse 5 GB" and "skim 5 GB" for the common needle-not-here case.

---

## 8. Output formats

### 8.1 Text (default) — LLM/human-readable

- Clear, scannable **session / turn / label / timestamp** headers (examples in §6.1, §6.2).
- **Timestamps — one canonical LOCAL form (v0.5):** `YYYY-MM-DD HH:MM:SS[.mmm] <TZAB>(UTC±offset)`, e.g. `2026-07-11 15:33:37 AEST(UTC+10)`, `IST(UTC+05:30)`. Derived from the system zone at that instant (DST-correct) via `jiff` (`TimeZone::system()`): the abbreviation + offset are a FORMAT, not a stored value; whole-hour offsets compact (`UTC+10`), fractional zero-padded (`UTC+05:30`), no-abbreviation zones render `(UTC±offset)` alone. `timez::format_timestamp` (seconds) and `format_local_compact` (milliseconds) share the one renderer. **No raw-UTC parenthetical in text** — the machine UTC lives ONLY in JSON (`ts_utc`), per the PAIR LAW below. (The former `… AEST (…Z)` dual form and the bare `…+10:00` form are removed — the UTC copy invited timezone-conversion errors.)
- Excerpts truncated only with an explicit `… (+N chars)` marker (never silent).
- Footers state match counts and **dropped counts** (`N dropped` when `--max-count` capped).

### 8.2 `--format json` — envelope v2 (one shape for every command)

Every JSON stream is EXACTLY three parts, no exceptions (S==0, `-c`, restore included):

1. `{"kind":"header","command":"<cmd>", …}` — the FIRST line, always. Span commands add
   `sessions_in_scope`/`top_level_sessions`/`subagent_sessions`; `verbatim` adds its
   budget/automation fields.
2. Kind-tagged rows — list→`session`, search→`exchange` | `census`, show→`record`,
   stats→`session`, files→`mutation|file|dir|bucket|boundary`,
   agents→`session|run|agent` (the v0.5 FLAT shape — one light `session` count row, each
   workflow `run`, and every subagent as its own `agent` row in tree pre-order; NESTING is
   text-mode only, rebuilt in JSON from `parent_agent_id`/`depth`; there is NO nested
   `workflow_runs`/`children` row field, and `--agent` is no exception), verbatim→`turn|
   compaction_boundary|collapsed_agents`, plan→`plan|binding|plan-edit`,
   image→`image|extract`, whoami→`identity`,
   recover→`coverage|segment|snapshot|restore|boundary|backup`,
   show→ also `branch-point` (under `--branch-points`), status→`verdict`, wait→`wait`.
3. `{"kind":"summary", …}` — the LAST line, always (even all-zero counts). Notable
   summaries: search `census` mode → `{axis, matched_records, distinct_keys,
   excluded_records, dropped_by_cap, skipped_lines}`; files → adds `sessions` (distinct
   owning sessions) but deliberately NO `dropped_by_cap` (files has no cap); show →
   `{records, dropped_by_cap, refetch_remainder, non_record_lines, skipped_lines,
   with_elicitation_sidecar}`; agents → `{sessions, runs, agents}`.

One reading idiom serves everything: `jq 'select(.kind=="<row>")'`; the summary is
`tail -1 | jq`. Vocabulary: `kind` belongs to the envelope EXCLUSIVELY; the jsonl-line
key is `line` (everywhere; `--file-lines` is the one FILE-line flag); a transcript's
shape is `shape`; a boundary's what-changed classifier is `cause`; an automation
pulse's class is its `trigger`; a tool hit's pairing state is the closed enum `pairing`
∈ `paired` | `pending` | `orphan` | `null` (non-tool records). Every spanning row carries
the id trio (`session_id`/`is_subagent`/`parent_session_id`). **Timestamp PAIR LAW:** every
machine instant is a `_utc`+`_local` PAIR — a record's OWN instant is `ts_utc`+`ts_local`;
a NAMED instant is `<name>_utc`+`<name>_local` (`first_*`, `last_*`, `trigger_*`,
`started_*`, `completed_*`, `pending_since_*`). `_utc` is the raw ISO-8601 Z; `_local` is
the same instant with the system offset. (The text form derives from these but drops the
UTC copy — §8.1.) Whole-stream `json.load` fails by design — parse per line (NDJSON).


## 9. Non-functional gates & invariants (checklist)

- [ ] **Speed:** `list` and `search` stay fast on 200 MB+ files — mmap + memchr + two-stage prefilter + lazy serde; tail/head reads never parse the whole file; rayon across files. (§7)
- [ ] **No silent truncation:** every cap (`--max-count`, excerpt length) reports its drop count; skipped malformed lines are **counted** and surfaced, never hidden. (§0, §7e, §8)
- [ ] **No `unwrap`/`expect`** in library/hot paths; `anyhow::Result` + `?`; `main` is the only error→exit-code site; a malformed line is skipped+counted, never fatal. (§0)
- [ ] **Tolerant parsing:** unknown top-level fields, unknown `type`s, unknown `Block` types (`#[serde(other)] Unknown`), and missing `timestamp` never crash. (§3)
- [ ] **Genuine-user correctness** (the load-bearing rule): excludes tool_result-carriers, `isCompactSummary`, and `isMeta` pseudo-turns; AUQ answers handled per §4.4. Unit-tested against fixtures incl. the `isMeta` case. (§4.1–§4.2)
- [ ] **Path encoding:** `[^A-Za-z0-9]→'-'`, no collapse, case-preserved; reverse never attempted; target detection real-path-vs-encoded per §2.3. (§2)
- [ ] **whoami:** env-var-first (`CLAUDE_CODE_SESSION_ID`, exact name), error-with-guidance on absence, **never** mtime/process-walk. (§6.3)
- [ ] **Timestamps:** one canonical local text form `<TZAB>(UTC±offset)` (no raw-UTC copy; auto-detected via `TimeZone::system()`); machine UTC only in JSON, paired `_utc`+`_local`. (§8.1, §8.2)
- [ ] **Example-rich `--help`** for every subcommand (the invocations in §6.1–§6.3 are the baseline). (clap `long_about`/`after_help`)
- [ ] **Gates green:** `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. No crate-level `#![allow(dead_code)]`; the only `#[allow(dead_code)]` is targeted on `model::Record`/`Block` for SPEC-mandated record-shape fields retained for tolerance/completeness (justified inline).

---

## 10. Design notes & empirical grounding

> The normative spec is §0–§9; this section records the **design rationale and the measurements** behind the three deepest features (`recover`, `verbatim`, `agents`) — *why* the algorithm is shaped as it is and *why* the magic numbers are what they are. All counts below are empirical, captured against the live `~/.claude/projects` corpus on the dates noted; treat them as representative magnitudes, not invariants (a live session's subagent counts drift upward as it spawns more).

### 10.1 `recover` — reference-tool survey + the `originalFile` boundary inversion

Four prior tools were studied (ccdiag, claude-file-recovery, coding-agent-session-search, florian-gist). **None reconstructs file content across integrity boundaries, and none segments at out-of-band-edit boundaries** — that is `recover`'s original territory (§6.7). Only four peripheral primitives were harvested from them:

1. the `tool_use_id ↔ tool_result` join (used everywhere a result must be attributed to its intent);
2. the Read `→` (U+2192) gutter-strip regex (recovers raw line text from a visible `cat -n`-style Read);
3. the `file-history-snapshot` on-disk backup channel (which claude-file-recovery parses and ccdiag discards) — used only as a coverage annotation, never to fabricate content: the blob name is usually present but the store is pruned and its content carries no transcript anchor (`--list-backups` lists it);
4. `(timestamp, session_id, line_number)` as the ordering key.

**The load-bearing inversion** is over claude-file-recovery: where it uses an Edit's `originalFile` to *paper over* drift (assume the file was whatever `originalFile` claims), `recover` uses **replayed-buffer ≠ the next op's `originalFile`** to *detect and segment at* the drift — that disagreement is exactly the AUTHORITATIVE integrity boundary #2 of §6.7. The same "in the LLM's eyes" sparse-buffer model (a `BTreeMap<file_line, cell>`; an absent line is an explicit gap, never fabricated; an un-anchorable edit becomes a counted coverage hole) is what keeps the reconstructed-vs-disk guarantee honest — the contiguous-from-line-1 prefix matches disk byte-for-byte even on a heavily-edited file with no clean anchor. The one genuinely-new capability under all of this is the per-line `Lnnnnn` counter (§6.7), threaded locally so the shared `scan_lines_bytes` signature is untouched.

### 10.2 `verbatim` — the measured basis for every default

**What the summary loses (the anatomy that motivates the feature).** Measured over three real sessions (246 MB, 130 MB, and 80 MB): a Claude Code compaction summary preserves task STATE in high fidelity but provably loses TURN fidelity — its §6 "All user messages" clips ~**22 real user prose turns → ~17 `...`-truncated bullets**, and the assistant side collapses ~**239 assistant turns → exactly 1 verbatim quote** (the last pre-compaction message). `verbatim` supplements (never re-derives) the summary by restoring those verbatim turns in order, each carrying its `Lnnnnn`.

**Every default is sized from those measurements**, not guessed:

| parameter | default | empirical basis |
|---|---|---|
| `--budget` | 40000 chars | a 1M-context session at 40–50% compaction comfortably recovers ~40K; a 200K context → ~10–15K |
| `--budget-unit` | REMOVED (v0.2) — `--budget` is chars, always | historical: tokens via ≈4 chars/token (a ~17K-char summary measured ≈ ~3.5–4.5K tokens) |
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

### 10.3 `agents` — the topology fix + verified corpus

**Why the topology layer matters.** Discovering subagent transcripts as a flat list — each nested transcript a detached session, with no LINKAGE back to the parent `tool_use` that spawned it — answers only three of six real queries and answers two more lossily (the "when" would be the lagging child-head ts; there would be no workflow-run grouping). `agents` therefore builds a topology on top of discovery: a `ParentSpawnIndex` (one forward scan of the parent transcript joining `tool_use_id → {spawn tool, trigger ts, description, subagent_type}`) joins each subagent to its spawning `tool_use`, recovering the returned message, files-changed, and the topology tree. Two consequences of the linkage are specified in §6.5: `--order-by` orders/filters on the true `trigger` axis by default (the accurate spawn instant, sharper than the child-head `start` ts), and a subagent row's printed id is the bare `<hex>` (joinable to an `agents` node), not the un-joinable `agent-<hex>` stem.

**Verified on-disk (a representative live session, 2026-06-08).** 152 built-in (`subagents/agent-<hex>.jsonl`) + 404 workflow (`subagents/workflows/wf_*/agent-<hex>.jsonl`) = 556 subagents, **0 nested** (depth uniformly 1 — Claude Code provisions subagents without an agent-spawn tool, so there are zero sub-sub-agents; confirmed across 2348 transcripts). The parent transcript carried 151 `Agent` + 22 `Workflow` tool_uses (this corpus spells the built-in spawn tool `Agent`, not `Task`; `Task` is matched defensively for other corpora), and 19 top-level `workflows/wf_*.json` run manifests. The **3-way returned-message resolve** broke down 147 sync-tool-result, 6 async-child-tail (the parent result was the `Async agent launched …` run-in-background sentinel → fall back to the child transcript tail), 403 workflow-journal (the `journal.jsonl` `result` event payload), 1 unresolved — 0 linkage mismatches.
