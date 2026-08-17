//! ShowArgs + StatsArgs.

use super::*;

/// `csift show`: fetch specific record(s) of ONE transcript, rendered full (or raw).
#[derive(Args, Debug)]
#[command(
    about = "Fetch specific record(s) of ONE transcript by line / turn index / record uuid \
            , rendered full, or verbatim raw jsonl with `--raw`",
    long_about = "Fetch the record(s) at specific 1-based jsonl line number(s) (the `Lnnnn` \
        every csift surface prints) and/or record uuid(s), from exactly ONE transcript. \
        Default output renders each record FULL (label + timestamp + complete text: the \
        permission-friendly alternative to `Read`-ing the raw jsonl). `--raw` emits the \
        VERBATIM raw jsonl line(s) instead: the escape hatch for inspecting fields csift \
        does not render (usage tokens, stop_reason, model, …).",
    after_help = "EXAMPLES\n  \
          csift show @<uuid> --line 46550                # the record at line 46550, full\n  \
          csift show @<uuid> --line 87,495..500,992      # several lines + ranges\n  \
          csift show @<agent-id> --line 88               # a SUBAGENT transcript (id from `csift agents`)\n  \
          csift show @<uuid> --uuid <record-uuid>        # by record uuid\n  \
          csift show @<uuid> --line 46550 --raw          # the verbatim raw jsonl line\n\n\
        TARGET, exactly ONE transcript\n  \
          `@<uuid>` / `@<uuid-prefix>` → that top-level transcript (never spans subagents); \
        `@<agent-id>` (from `csift agents`) → that subagent transcript; a `*.jsonl` path → \
        that file. A target resolving to more or fewer than one transcript is a hard error; \
        line numbers address one file.\n\n\
        ADDRESSING + EXIT\n  \
          `--line` / `--turn` tokens take the shared range grammar: `N` · `A..B` · `N..` (to \
        the end) · `..N` · `-k` from the end (`--line -20..` = the last 20 lines, `--turn -3..` = \
        the last 3 turns); 1-based for lines, 0-based for turns, inclusive, repeatable / \
        comma-joined. An explicitly named line/uuid that resolves to no record is a HARD \
        ERROR (exit non-zero); a range CLAMPS to the file but erroring if it yields nothing. \
        A pending-elicitation record merged from the sidecar has no physical line; address \
        it by `--uuid` (it renders `(elicitation sidecar)` in place of `Lnnnn`).\n\n\
        RAW MODE\n  \
          `--raw` prints the exact bytes of each addressed jsonl line (even a malformed / \
        torn line: that is the point). It is mutually exclusive with `--format json` (raw \
        IS the file's own JSON) and reads the transcript file only (no sidecar merge).\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: {kind:\"header\", command:\"show\", session_id, is_subagent, \
        parent_session_id, path} → one {kind:\"record\", …} row per fetched record → \
        {kind:\"summary\", …}. Record rows carry {session_id, is_subagent, parent_session_id, \
        turn_index, line (null for a sidecar-merged record), uuid, label, labels:[…], \
        tool_name, from, to, pairing (paired | pending | orphan | null), tool_use_id, \
        source (\"elicitation-sidecar\" | null), ts_utc, ts_local, text (FULL; never \
        clipped), image_ids:[…]}. The summary is {records, dropped_by_cap, refetch_remainder \
        (the ready-to-run continuation command when the cap dropped units, else null), \
        non_record_lines, skipped_lines, with_elicitation_sidecar}."
)]
pub struct ShowArgs {
    /// ONE transcript: `@<uuid>` | `@<uuid-prefix>` | `@<agent-id>` | a `*.jsonl` path.
    // Declared as a Vec DELIBERATELY (the run handler enforces exactly-one): a single
    // non-Vec positional with `allow_hyphen_values` makes clap consume a mistyped
    // long flag AS the target and report the user's VALID target as the surplus
    // "unexpected argument" - the wrong-hypothesis rabbit hole. The Vec absorbs the
    // bad token instead, so `parse_project_target` rejects it BY NAME with the same
    // "did you mistype a flag?" error every sibling command gives.
    #[arg(value_name = "TARGET", value_parser = parse_project_target, allow_hyphen_values = true, required = true, num_args = 1..)]
    pub target: Vec<std::path::PathBuf>,

    /// 1-based jsonl line(s) in the shared range grammar: `N` · `A..B` · `N..` · `..N` · `-k` from the end (`-20..` = last 20), repeatable / comma-joined
    /// (`--line 87,495..500,992`).
    #[arg(
        long,
        value_name = "SPEC",
        value_delimiter = ',',
        allow_hyphen_values = true
    )]
    pub line: Vec<String>,

    /// Record uuid(s), repeatable / comma-joined.
    #[arg(long, value_name = "UUID", value_delimiter = ',')]
    pub uuid: Vec<String>,

    /// 0-based TURN index/range: the `tN` `search` prints, in the SAME grammar as `--line`
    /// (`N` · `A..B` · `N..` · `..N` · `-k` from the end, so `-3..` = the last 3 turns,
    /// `42..` = turn 42 → the end). Fetches EVERY record of the named turns (reads a turn's
    /// whole back-and-forth), and the turn numbering matches the `·t<N>` in `search`'s
    /// exchange headers exactly.
    /// Mutually exclusive with `--line`/`--uuid` (pick ONE addressing mode).
    #[arg(
        long,
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        conflicts_with_all = ["line", "uuid"]
    )]
    pub turn: Option<String>,

    /// Cap the emitted record units: the CONTEXT-FLOOD guard for open ranges (`--line ..`
    /// / `--turn ..` address a whole transcript). Defaults to 200; the drop is ALWAYS
    /// reported, with the exact continuation command for the remainder. `0` = uncapped
    /// (the crate-wide `--max-count 0` convention).
    #[arg(long, value_name = "N")]
    pub max_count: Option<usize>,

    /// Emit the VERBATIM raw jsonl line(s) instead of the rendered record view
    /// (mutually exclusive with `--format json`).
    #[arg(long)]
    pub raw: bool,

    /// Emit JSON instead of the rendered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// HIDDEN no-op subagent-span flag. `show` has NO span control: it fetches from exactly
    /// ONE transcript, never a set, but ten sibling commands accept the span pair, so a
    /// muscle-memory `--no-subagents` here is a REAL observed slip (R7 §2.3). Accepted only
    /// to emit a pointed error naming the actual rule, instead of the misleading generic
    /// "did you mistype a flag?" the TARGET value-parser would give. See
    /// [`ShowArgs::span_flag_error`].
    #[arg(long = "no-subagents", hide = true)]
    pub no_subagents: bool,

    /// Hidden no-op twin (see `no_subagents`): both span switches are accepted-then-rejected.
    #[arg(long = "subagents", hide = true)]
    pub subagents: bool,
}

impl ShowArgs {
    /// The pointed rejection for the hidden span pair (the `agents` precedent): name the
    /// actual rule and the correct next move, not a typo hypothesis.
    #[must_use]
    pub fn span_flag_error(&self) -> Option<&'static str> {
        if self.no_subagents || self.subagents {
            Some(
                "`show` has no subagent-span flag: it fetches record(s) from exactly ONE \
                 transcript and never spans a session's subagents (line numbers are \
                 per-FILE). Drop the span flag. To read a subagent transcript, target it \
                 directly: `csift show @<agent-id> --line N` (the id `agents`/`search` \
                 print); to search across a session AND its subagents use `csift search`.",
            )
        } else {
            None
        }
    }
}

/// `csift stats`: one-scan aggregates per session (and a scope total).
#[derive(Args, Debug)]
#[command(
    about = "One-scan aggregates per session: records, turns, tool calls by name, tokens \
             by model, time span, compactions",
    long_about = "One scan, one fixed rich shape: the aggregation questions that \
        otherwise force hand-rolled jsonl parsing: token burn per model \
        (message.usage sums), tool-call counts by name (per CALL: one per invocation; \
        `search --count-by tool` counts RECORDS, use + result carrier, so it reads ≈2× \
        these tallies: a unit difference, not a discrepancy), turn count, first/last \
        timestamps + duration, compaction count, malformed-line count. Spans subagents \
        by default (each transcript is its own row; the scope TOTAL block sums them). \
        `--since`/`--until` bound the counted records by timestamp. Under `--turn`/time \
        windowing every figure windows EXCEPT `lines`, which stays the file's physical \
        line count (a file fact, not a window fact).",
    after_help = "EXAMPLES\n  \
          csift stats @<uuid>                    # one session + its subagents\n  \
          csift stats @<uuid> --no-subagents     # just the top-level thread\n  \
          csift stats . --since 1d               # this project, last 24h\n  \
          csift stats @<uuid> --format json | tail -1 | jq .tokens   # scope token totals\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: header → one {kind:\"session\", …} row per session → summary. Session \
        rows carry {session_id, is_subagent, parent_session_id, lines, user_records, \
        assistant_records, turns, compactions, first_utc, first_local, last_utc, last_local, \
        tokens:{<model>:{input, output, cache_read, cache_creation}}, tools:{<name>:count}, \
        skipped_lines}. The summary adds the scope totals ({sessions, tokens, tools, turns, \
        dropped_by_cap, skipped_lines}): `tail -1 | jq .tokens` is the one-liner for total \
        burn. `skipped_lines` here is a FULL-SCAN census (stats parses every line): the \
        corruption-census authority for \"does this transcript carry a torn/corrupt line \
        anywhere\"; `list`'s same-named field covers only the head/tail lines list reads."
)]
pub struct StatsArgs {
    /// Project path(s) / `@<uuid>` / `@<agent-id>` / `*.jsonl`; same targeting grammar as
    /// every subcommand. Empty ⇒ every project.
    #[arg(
        value_name = "PATH",
        value_parser = parse_project_target,
        allow_hyphen_values = true
    )]
    pub paths: Vec<std::path::PathBuf>,

    /// Scope ALSO to the session ids in FILE (`-` = stdin): whitespace/newline-separated
    /// uuid / uuid-prefix / agent-id tokens, bare or `@`-prefixed, exactly the ids csift
    /// emits (`search -l`, JSON `transcript_ids` / `parent_session_id`). UNION with positional
    /// targets; each id resolves fail-loud like an `@` positional. An EMPTY list (an upstream
    /// stage that found nothing) scopes to NOTHING: honest empty, exit 0, never a silent
    /// widening to every project. The resolved ids then follow this command's normal
    /// span rules, exactly as if they were `@` positionals: on the span-by-default
    /// commands each session EXPANDS to its subagent transcripts (add `--no-subagents`
    /// to pin to exactly the listed ids).
    #[arg(long = "sessions-from", value_name = "FILE|-")]
    pub sessions_from: Option<std::path::PathBuf>,

    /// Lower time bound (ISO8601 / relative `2h`/`3d`/…): records outside are not counted.
    #[arg(long, value_name = "WHEN")]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since).
    #[arg(long, value_name = "WHEN")]
    pub until: Option<String>,

    /// Count ONLY records in this inclusive turn-index window: the shared range grammar
    /// (`N` · `A..B` · `N..` · `..N` · `-k` from the end), 0-based on the transcript's genuine-turn
    /// order (the same axis `search`/`files` use; discover indices from their output). Intersects
    /// (AND) with `--since`/`--until`, e.g. token burn of the last N turns is simply
    /// `csift stats @main --turn -N..` (no need to look up the current turn index).
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true
    )]
    pub turn_range: Option<String>,

    /// Cap the emitted per-session rows (default: unlimited); bound an unscoped run's output.
    /// The kept rows are the most recently active; the drop is reported, never silent (the
    /// scope TOTAL block then covers the shown subset). Pass a target / `--since` to choose
    /// WHICH sessions instead.
    #[arg(long, value_name = "N")]
    pub max_count: Option<usize>,

    /// Exclude subagent transcripts: count only the top-level session(s).
    #[arg(long = "no-subagents")]
    pub no_subagents: bool,

    /// Span subagent transcripts: the DEFAULT here; the explicit flag exists so every
    /// span command answers the same two switches (`--subagents` / `--no-subagents`).
    #[arg(long = "subagents", conflicts_with = "no_subagents")]
    pub subagents: bool,

    /// Emit JSON instead of the text blocks.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}

impl StatsArgs {
    /// Whether subagent transcripts are spanned (the default).
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.subagents || !self.no_subagents
    }
}
