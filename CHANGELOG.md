# Changelog

All notable changes to csift are documented in this file, newest first — one
entry per released version, written in that version's release commit. Pre-1.0
SemVer: a BREAKING surface change bumps the MINOR version; a non-breaking
surface change bumps the PATCH.

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
