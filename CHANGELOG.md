# Changelog

All notable changes to csift are documented in this file, newest first — one
entry per released version, written in that version's release commit. Pre-1.0
SemVer: a BREAKING surface change bumps the MINOR version; a non-breaking
surface change bumps the PATCH.

## [0.7.3] - 2026-08-12

- Path encoding is now EXACTLY Claude Code's (evidence extracted from the CC
  2.1.228 binary): the cwd is NFC-normalized, then replaced per UTF-16 code
  unit — an NFD-spelled accented path now encodes identically to its NFC
  spelling (closing a formerly-documented divergence that resolved the wrong
  dir on macOS NFD paths), and an astral char yields two dashes, matching the
  JS regex's view.
- Windows drive-encoded project dirs (`C:\Users\x` → `C--Users-x`,
  letter-led) are first-class targets: both the bare positional token and
  `@C--Users-…` resolve; a drive-shaped token matching no projects dir falls
  through to real-path resolution. A UNC-encoded dir (`--server-…`) is
  targeted via the `@` form (the mistyped-flag guard's error now says so).
- Verified from the same binary, no code change needed: CC's config home is
  `CLAUDE_CONFIG_DIR ?? os.homedir() + "/.claude"` (NFC-normalized) — Windows
  never consults `HOME`, confirming the 0.7.1 per-platform split.

## [0.7.2] - 2026-08-12

- Performance round (behavior-identical — a 28-command byte-exact A/B battery
  pins stdout, stderr, and exit codes unchanged): every per-line byte
  prefilter now uses construct-once memmem finders (the stateless form
  rebuilt its searcher on every call); the parallel line scanner no longer
  runs a serial whole-file newline count (skipped outright for single-chunk
  files, computed in parallel otherwise); `search`'s per-turn match+render
  phase fans out on rayon when the scope is small or the file is 64 MB+ (the
  straggler class), gated so broad scans keep the serial walk. Measured warm
  on the reference corpus: big-session census 1.21x, no-match unscoped
  1.13x, caseless literal 1.09x, verbatim 1.08x; user CPU down 2-4 percent.
- README gains a coverage badge (94.8 percent line coverage,
  cargo-llvm-cov over the full suite).

## [0.7.1] - 2026-08-12

- The default data root resolves per platform, the way Claude Code's own
  `os.homedir()` does (correctness): `$HOME/.claude` on Unix,
  `%USERPROFILE%\.claude` on Windows — `HOME` is never read on Windows, so a
  stray Git-Bash/MSYS `HOME` (often a POSIX-style path a native process cannot
  open) no longer points csift at a `.claude` dir Claude Code never writes.
  Precedence is unchanged: `--claude-home` > `$CLAUDE_CONFIG_DIR` > the OS
  home's `.claude`. The error message and `--claude-home` help name both
  variables.

## [0.7.0] - 2026-08-12

Breaking text-surface release; JSON output is unchanged.

- **Breaking:** `search` exchange headers are self-resolving. Each header opens
  with a STABLE id-prefix token — the first 8 chars of the owning transcript id
  (`<tok>·t<N>`) — instead of a per-invocation `sN` ordinal, and the
  `sN = <id>` session legend block is removed entirely. A token is a valid `@`
  target as-is and identical across invocations; within one output, distinct
  ids sharing their first 8 chars lengthen together (8 → 12 → full id); a
  teammate id (name-embedded, not hex-led) renders whole. A subagent exchange
  carries `(parent <first-8>)` on EVERY header.
- Resolver widening so every emitted token round-trips (all fail-loud on
  ambiguity): the `@`-prefix match domain is the UNION of top-level session
  uuids and subagent agent ids; a literal `8-4-4-4-12`-layout prefix longer
  than 11 chars is a valid uuid-prefix token; a 12+-hex token keeps
  exact-agent-id semantics first, then falls back to a unique literal-prefix
  match.
- Output geometry: a head `matches` banner (true totals · `oldest first` · the
  emitted window · `undated last` when present) follows the scope banner; the
  tail footer repeats the TRUE pre-cap totals beside its drop accounting; the
  stderr zero-match diagnosis discloses the malformed-line count (an absence
  claim is definitive for parseable lines only). The both-ends placement law
  joins SPEC section 0 as a crate-wide design law.
- `--max-count` is SIGNED: `N` keeps the EARLIEST N of the chronological
  stream, `-N` the LATEST N, `0` stays uncapped; the kept exchanges still emit
  oldest-first among themselves. Both ends disclose the window; the footer
  names the dropped side (`N later|earlier dropped by --max-count`).
- Docs: an OUTPUT GEOMETRY section in `search --help` and SKILL; recipes for
  "when did X first happen" (`--max-count 1`), "most recent occurrence"
  (`--max-count -1`), and the header-token follow-up into `show`.

## [0.6.10] - 2026-07-14

- `@trap` retry guidance states the granularity: the retry must be a NEW,
  SEPARATE Bash invocation — two attempts inside one shell script are still one
  in-flight tool_use, so both miss (error text + SKILL + assumption table).
- Documented: EVERY `tool_use`'s matchable text is its name + the re-serialized
  JSON input, so an embedded real newline is the two-character `\n` by match
  time — match the literal `\\n`; `--multiline` is correctly irrelevant there.
- SKILL completeness: verbatim header fields `automation_triggers`,
  `budget_is_per_session`, `sessions_rendered` named; the self-echo trap
  recorded (a nonce used as a search pattern writes itself into your own live
  transcript — scope absence checks away from your own session).

## [0.6.9] - 2026-07-14

- Stage-1 candidate detection is serialization-tolerant (correctness): a
  valid-JSON record whose serialization differs from the compact wire format
  (whitespace around the colon — python `json.dumps` defaults, a jq/editor
  round-trip) used to vanish one layer BEFORE any malformed counter: no match,
  no count, zero disclosure. The role needles now route through shared
  whitespace-tolerant matchers; every other prefilter needle is
  serialization-safe by construction, and the needle law is codified in
  AGENTS.md. Framing is unchanged: one record per line.

## [0.6.8] - 2026-07-14

- `list`/`agents` head+tail scans no longer double-book malformed lines: the
  tail scan floors at the head scan's consumed end, so the two windows are
  disjoint and every malformed line in them is counted exactly once (an
  all-garbage file used to report exactly 2×).
- `list`'s malformed count is a DISCLOSED window census, never a whole-file
  verdict: the note reads `… skipped (among the head/tail lines read — full
  census: csift stats)`, and `stats` is named the full-scan census authority.
- A sidecar marker line the current schema cannot read (a pre-release fossil
  under old field names) is counted as malformed — provably-ours yet
  uninterpretable never buys silence.

## [0.6.7] - 2026-07-14

Doc-only convergence round.

- SKILL names verbatim's two automation header fields: `automation_by_kind`
  (the SELECTED triggers per class) vs `automation_in_scope_by_kind` (every
  in-scope pulse regardless of budget).
- `agents` `returned_message` semantics stated: it is the ORCHESTRATOR's
  record of the child's return, not the agent's own conclusion; the child's
  final words are always `show @<agent-id> --turn -1..`.

## [0.6.6] - 2026-07-14

- Obviously-corrupt lines are COUNTED (correctness): every byte-prefilter
  rejection path runs an O(1) shape check (non-blank but not `{…}`-framed ⇒
  malformed), so free-text garbage and crash-truncation move `skipped_lines`
  on every command. Documented residue: a `{…}`-framed invalid INTERIOR is
  only counted on a parse candidate.
- `verbatim`'s header reads `spanned K of N compaction boundaries in scope`
  (K alone read as a transcript property); its JSON header carries the full
  budget accounting (`round_trip_fraction`, `chars_used`, `boundaries_*`,
  `selected_*`).
- Docs: under `--turn`/time windows every `stats` figure windows EXCEPT
  `lines`; an hours-old `awaiting-execution` lane is overwhelmingly an
  abandoned parent session — weigh `pending_since_utc`; `--siblings` caps
  apply to NON-matching context records only.

## [0.6.5] - 2026-07-13

- Bare ISO datetimes are LOCAL wall-clock time (correctness): `--since
  "2026-07-13T20:00:00"` used to collapse silently to local midnight (the
  civil-Date parser kept only the date part). A civil-DateTime arm now
  precedes the Date arm; a string carrying a malformed offset still bails.
  One fix covers every `--since`/`--until` consumer.
- The id trio (`session_id` / `is_subagent` / `parent_session_id`) rides EVERY
  search hit and sibling object, so bare `.hits[]` flattening keeps real ids.
- Advisory notes fire AFTER target resolution — never a warning about a run
  that was never going to happen.
- SKILL: the missing `plan` / `recover` / `image` JSON row schemas added;
  `@trap` marker uniqueness stated as conversation-wide.

## [0.6.4] - 2026-07-13

- The removed `turns` name gets a tombstone error: a hidden variant always
  bails naming the rename (`verbatim`, same flags) and routes plain turn
  reading to `show <target> --turn -3..`. A wall, never a shim — it never
  runs.
- `agents` text brands a non-completed lane's `returned_message` inline
  (`history — predates the still-open lane, NOT the outcome`); a completed
  lane stays unbranded.
- Docs: a workflow RUN row's `status` is journal-verbatim (an open set, not a
  csift enum); the richest-view dedup rule stated mechanically (`labels[]` is
  richest-first; the rendered view is the first label surviving `-t`/`-T`);
  exit codes de facto (usage errors 2, csift errors 1 — the contract stays
  0-vs-non-zero); the record-level jq pipeline idiom (select in jq, run the
  csift-generated `refetch`).

## [0.6.3] - 2026-07-13

- Elicitation-sidecar GHOST-PENDING guard (correctness): Claude Code fires no
  PostToolUse for a REJECTED AskUserQuestion/ExitPlanMode, so the hook can
  never write `resolved` there. A pending whose key appears on a native record
  as an actual `tool_use` block id / `tool_result` id (structural check) is
  dropped like a resolved pair — the native transcript outranks the sidecar.
- `list`'s scope banner / JSON `sessions_in_scope` report the PRE-cap resolved
  range; the flood guard caps only the rows.
- `--count-by label` census keys pass the active `-t`/`-T` predicate — a
  dual-labeled record no longer leaks its filtered-out twin into the keys.
- `show` rejects the span pair with the single-transcript rule; legacy flat
  `-t` values (`thinking`/`tool`/`tool-response`) name their successor path.

## [0.6.2] - 2026-07-12

- `image --id` miss error explains itself: it names the handles PRESENT,
  states that `#N` is inherited from paste-time numbering (holes and non-1
  starts are source gaps, not csift drops), and routes to the plain listing.
- The three count units are cross-referenced where the numbers collide:
  `-c` counts EXCHANGES · `--count-by` counts RECORDS (a tool call + its
  result carrier ⇒ ≈2× the call figure) · `stats` tools count CALLS.
- Docs: the jq merge idiom for flattening hits with their exchange-row ids;
  `select(.kind==…)` before projecting; `returned_message` is the NEWEST
  message the child ever returned (on a frozen lane it predates the pending
  call).
- Every subcommand's `long_about` was dead text — now rendered.

## [0.6.1] - 2026-07-12

- An unrecognized `@`-token is a HARD error naming the @-grammar — it never
  falls through to path resolution (a stripped `@a` used to become a
  cwd-relative path with a misleading project-dir error); a 1-3-char hex token
  gets the dedicated too-short-for-a-prefix message.
- `@trap` main-thread timing documented and routed by the error: a subagent's
  transcript records the launching tool_use eagerly, but the MAIN
  conversation's record flushes only after the current Bash call completes —
  a top-level first use always misses; `@main` for the main thread, re-run
  the SAME marker otherwise.
- Docs: `--count-by model` reports the raw `<synthetic>` key verbatim; text
  excerpts keep literal newlines (`| head -N` can cut mid-record — the
  line-safe machine form is `--format json`).

## [0.6.0] - 2026-07-12

- **Breaking (agents JSON):** `completed_utc/_local` (+ `duration`) are
  non-null ONLY when `status == "completed"` (a frozen lane is never "done");
  every timestamped lane gains the `last_activity_utc/_local` pair (the tail
  newest-record instant).
- `show`'s TARGET is a Vec so a mistyped or foreign `--flag` is rejected BY
  NAME instead of being consumed as the target; two real targets get a
  pointed one-transcript arity error.
- Censuses count RECORDS, not per-section hits — a leaf tally now equals
  exactly what `-t <leaf>` surfaces.
- `pairing` rides the tool BLOCK through the communication views: a frozen
  SendMessage is `pending` under ANY selector ("any pending tools?" needs no
  `-t`).

## [0.5.2] - 2026-07-11

- `search --help`'s COUNT section says the `-c` integer is the EXCHANGE total
  and routes session listing to `-l`.

## [0.5.1] - 2026-07-11

Help-parity release; behavior unchanged.

- The five-document contract: `SKILL.md` = the LLM manual · `--help` = the
  human (CLI-proficient) manual, information-parity with SKILL · `README.md` =
  promotion · `SPEC.md` = design intent · `AGENTS.md` = maintenance.
- Root `--help` gains the human-toned sections (the rules every command
  follows, JSON output, pitfalls, non-goals, retention); `search --help`
  gains the full 3-role / 25-leaf label taxonomy; `show`/`stats`/`plan`/
  `whoami`/`image` gain JSON SCHEMA sections; every `--sessions-from` help
  states the span rules; `whoami`'s composition example matches the flat
  envelopes.

## [0.5.0] - 2026-07-11

Breaking rework, zero backcompat.

- The per-command turn-window flag is `--turn` everywhere (was `--turn-range`);
  same range grammar, same AND-intersection with `--since`/`--until`.
- The label census generalizes to `--count-by <AXIS>` with six closed axes:
  `label` · `tool` · `turn` · `session` · `pairing` · `model`; JSON row kind
  is `census`; records outside an axis's domain are excluded and reported.
- `agents --format json` is FLAT (envelope v2, no exceptions): session → run →
  agent rows in tree pre-order; nesting is text-only, rebuilt from
  `parent_agent_id`/`depth`; an unreachable node is appended, never dropped.
- `show`: an EXPLICIT `--turn` miss is a hard error naming the domain
  (open/from-end forms clamp); a 200-record-unit flood guard with the exact
  continuation command; `--max-count 0` = uncapped uniformly on
  list/stats/search/show.
- Timestamps (text) take the ONE canonical local form
  `YYYY-MM-DD HH:MM:SS[.mmm] TZAB(UTC±offset)` — the second UTC copy is gone.
- Slash-command wrappers detected in BOTH tag orders (`<command-message>`
  first is current CC); a new-order wrapper no longer masquerades as human
  prose or opens a turn.
- `list` rows gain `sidecar_present` (tri-state elicitation evidence); `files`
  JSON summary gains `sessions`; `verbatim` prints a per-session
  no-compaction note routing to `show --turn`; `normalize_argv` locates the
  subcommand by scanning past root flags — flag order is free in combination
  with `--claude-home`.

## [0.4.1] - 2026-07-11

- Version + tag discipline codified: `Cargo.toml` ≡ SKILL surface header ≡
  `csift --version` move together in the same commit; every release gets an
  annotated `vX.Y.Z` tag; `--help` text is release surface. This release
  bumps for the v0.4 round's `--help` corrections.

## [0.4.0] - 2026-07-11

Breaking rework, zero backcompat.

- `turns` is renamed `verbatim` and reframed as the compaction-fidelity
  specialist (restore the verbatim turns a compaction summary clipped);
  tail-peek reading moves to `show --turn` — a third addressing mode that
  fetches EVERY record of the named turn(s) (`-3..` = the last 3).
- ONE range grammar everywhere: `N` · `A..B` · `N..` · `..N` · `-k` from the
  end — all inclusive, resolved per target; the dash form `A-B` hard-errors
  teaching the `..` spelling.
- `search --count-by-label`: a per-leaf label census terminal mode (empty
  pattern = whole-scope census; a leaf's count = what `-t <leaf>` would
  surface); JSON `label_count` rows.
- Empty-result self-diagnosis: a zero-match run prints a stderr diagnosis —
  "a DEFINITIVE absence (exit 0), NOT an error", the active filters, and an
  active probe naming the label(s) the pattern DOES occur under; JSON summary
  gains `definitive_absence` / `active_filters` / `excluded_by_label`.
- Flood guards: an unscoped all-projects `list` caps at the 50
  most-recently-active rows (drop reported; `--max-count` overrides); `stats`
  gains an opt-in `--max-count`.
- JSON rename (search summary): `session_ids` → `transcript_ids`
  (+ `transcript_ids_truncated`) — named apart from `-l`'s owning-session ids.

## [0.3.0] - 2026-07-11

- `-T`/`--label-not` (search): label EXCLUSION with the same selector grammar
  as `-t`; richest-SURVIVING-view dedup; statically-empty combos hard-error.
- `--sessions-from <FILE|->` on every multi-target command (union an id list
  into the scope; an explicit empty list = an empty scope); `search -l` emits
  the matching owning-session ids to pipe into it; `search --raw` emits
  matched records' verbatim jsonl lines (stdout pure, notes on stderr).
- Search JSON hits + verbatim collapsed-agent rows carry `refetch` — the
  ready-to-run `csift show` command addressed at the line-owning transcript.
- The turn window and `--since`/`--until` INTERSECT on every command;
  `verbatim` REQUIRES a target (budget × every-session flood guard); `list`
  gains `--since`/`--until`.
- Teammate ids with dashed NAMES round-trip as `@<agent-id>` targets.

## [0.2.0] - 2026-07-10

Breaking ergonomics rework, zero backcompat — one way per intent.

- New `show` (§ record FETCH by `--line`/`--uuid`, rendered full or `--raw`
  verbatim jsonl bytes) owns fetching; `search --line/--uuid` are REMOVED.
  New `stats`: one-scan per-session aggregates (tokens by model, tool counts,
  turns, span, compactions).
- Envelope v2: EVERY `--format json` stream is ONE header line + kind-tagged
  rows + ONE summary line, no exceptions; the jsonl-line key is `line`
  everywhere.
- Flag surface: `-t`'s long form is `--label`; `agents --kind` → `--shape`;
  `recover --line-range` → `--file-lines`; the uniform span pair
  `--subagents`/`--no-subagents`; verbatim's five tuning knobs collapse into
  `--profile heavy|light`; `--siblings` is a zero-arg fixed policy;
  `image --id` takes bare digits or `L<line>i<n>`.
- Guardrails: a bare id target errors "did you mean '@<id>'?"; a search
  PATTERN starting `@` errors; a uuid-shaped pattern notes on stderr.

## [0.1.0] - 2026-06-07

- Initial scaffold of csift — "ripgrep for Claude Code session transcripts":
  a fast Rust CLI to list and regex-search the Claude Code session `.jsonl`
  transcripts under `~/.claude/projects/`.
