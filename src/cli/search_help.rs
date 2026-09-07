//! The `search --help` after-help body: the examples, the output geometry, the label
//! taxonomy and the JSON schema. Extracted from [`SearchArgs`]'s `#[command(...)]`
//! attribute so the args file keeps room for the per-flag documentation; the string is
//! byte-identical to what the attribute carried, so `--help` is unchanged by the move.

/// The `after_help` body of `csift search --help` (see the module doc).
pub(crate) const SEARCH_AFTER_HELP: &str = "EXAMPLES\n  \
          csift search \"carry\"                                  # all projects, smart-case\n  \
          csift search \"carry\" .                                # this project (positional PATH, like every sibling)\n  \
          csift search -i \"askuserquestion\" -t agent.tool.use  # tool_use blocks naming AUQ\n  \
          csift search \"\" -t user --since 2h .                  # user turns, last 2h, this project\n  \
          csift search \"tail.read\" --multiline @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\n  \
          csift search \"panic\" -t agent.message -t agent.thinking --turn 10..20 --max-count 50\n  \
          csift search \"persisted-output\" --resolve-persisted --format json\n  \
          csift search \"refactor\" -c                            # COUNT matches only (ripgrep -c idiom)\n  \
          csift search \"refactor\" -l                            # WHICH sessions matched, one id per line (rg -l idiom)\n  \
          csift search \"refactor\" -l | csift files --sessions-from -  # …then scope the NEXT command to them\n  \
          csift search \"\" @<uuid> -t agent -T agent.thinking    # the agent role MINUS its thinking (-T excludes)\n  \
          csift search \"\" @<uuid> -t agent.message --raw | jq -r '.message.model'  # raw lines: any unrendered field\n  \
          csift search \"let's chat\" -t user --siblings              # the match WITH the turn's other side\n  \
          csift search \"let's chat\" -t user --siblings --no-truncate # …and READ the reply end-to-end\n  \
          csift search \"X\" --max-count 1                        # when did X FIRST happen? (earliest exchange)\n  \
          csift search \"X\" --max-count -1                       # most recent occurrence of X\n  \
          csift show @<tok> --line <n>                            # follow up a hit: paste its header token + L<n>\n\n\
        OUTPUT GEOMETRY (text mode)\n  \
          Exchanges emit oldest-first (stable chronological across every transcript in \
        scope; undated exchanges last). Each exchange header opens with a STABLE id-prefix \
        token: the first 8 chars of the owning transcript id (a within-output collision \
        lengthens the colliding group to 12, then the full id; a teammate id renders whole), \
        directly usable as an `@` target, identical across invocations. A subagent \
        exchange carries `(parent <first-8>)` on every header. The head carries scope + \
        match totals + direction; the tail repeats the totals and adds integrity notes and \
        refetch guidance; each over-long fragment marks its own truncation inline \
        (`(+N chars)`). To limit output, prefer `--max-count N` (earliest N) or \
        `--max-count -N` (latest N) over piping into `head`/`tail`: a capped run keeps \
        every note; a pipe amputates one end of the ledger.\n\n\
        SIBLINGS (`--siblings`)\n  \
          A match renders only the records that MATCHED. `--siblings` additionally renders \
        the OTHER records of the same turn (the back-and-forth around the hit) under a `·` marker, \
        so a matched user question surfaces WITH the agent's reply. Fixed policy: message units \
        always render (user.*, agent.message, agent.communication.*); chattier machinery is \
        capped per leaf (thinking ≤2, tool.use ≤3, tool.result ≤3, harness ≤2); the capped-away \
        remainder surfaces as an explicit `(+N more · csift show @<id> --line A..B)` pointer. A \
        record that itself matched is never duplicated as a sibling.\n\n\
        SEE ALSO\n  \
          search is the surface that PRINTS image handles (`[N image(s): #7]` / `L123i1`), so: \
        csift image <target> --id <ID> --out DIR extracts them to files you can then Read - pass a \
        `#N` handle as the bare number. An image-bearing row is evidence, not a blank.\n  \
          search is also the FACT half of the csift channel: a delivered message lands in the \
        receiving lane as an attachment carrying its envelope, so `csift search '<message id>' \
        @<lane> --additional-context` proves the text reached that lane's context. \
        `csift msg <ID>` runs that same join and adds csift's own ledger beside it. The \
        delivery itself needs no flag: `agent.communication.channel` is the one attachment \
        leaf a DEFAULT scan sees.\n\n\
        COUNT (`-c` / `--count-only`)\n  \
          `-c`/`--count-only` prints just the integer EXCHANGE total: matched round-trips, \
        the ripgrep `-c` idiom, honoring every filter (per-RECORD counts are `--count-by`). \
        That total is ALSO always in the normal output's footer (alongside the \
        distinct-session total); `--count-only` just isolates that ONE integer for a pipe. To \
        list WHICH sessions matched, use `-l` (one owning uuid per line; it pipes straight \
        into `--sessions-from -`).\n\n\
        REGEX DIALECT: linear-time (RE2-class)\n  \
          The pattern is the Rust `regex` crate (regex::bytes), which GUARANTEES \
        linear-time matching in the input length: NO catastrophic backtracking, ever.\n  \
          Supported: literals; character classes [...] / [^...] / \\d \\w \\s and \
        Unicode classes \\p{...}; alternation |; groups (...) and non-capturing \
        (?:...); quantifiers * + ? {m,n} (greedy + lazy *?); anchors ^ $ \\b \\B; \
        dot . (use --multiline to let it cross newlines); inline flags (?i)(?m)(?s)(?x); \
        Unicode-aware by default.\n  \
          NOT supported (these need non-linear engines): backreferences \\1; \
        lookahead/lookbehind (?=) (?!) (?<=) (?<!); atomic groups / possessive \
        quantifiers (?>...) / a*+. A pattern using these fails to COMPILE with a clear \
        error (by design, not a bug).\n  \
          Case: smart-case by default (insensitive unless the pattern has an uppercase \
        letter); -i forces insensitive. --multiline lives in the SAME dialect (it sets \
        the (?s)(?m) flags). CAVEAT: tool_use.input is matched RE-SERIALIZED: every \
        tool_use's matchable text is its name + the re-serialized JSON input (not just \
        AskUserQuestion's), so a real newline inside e.g. a Bash `input.command` is \
        already the two-character sequence \\n by match time; match the literal `\\\\n`; \
        --multiline is correctly irrelevant there (it helps only where the RENDERED text \
        keeps real newlines: message text, thinking, tool_result bodies).\n\n\
        AUTOMATION TRIGGERS (`harness.notification.*`)\n  \
          A machine `<task-notification>` (a background-command / workflow / spawned-agent / \
        monitor-tick COMPLETION pulse) OPENS a turn but classifies under \
        `harness.notification.<kind>` (NOT `user`). It renders as a PARSED attribution label \
        `[<kind> <task-id> <status>] <summary>` (kind = background-command | workflow | subagent | \
        monitor | task, read from the summary); never the raw XML. Match it like any text, e.g. \
        `csift search 'background-command' -t harness.notification`. The `<kind>` prefix \
        distinguishes a machine opener from a genuine human message.\n\n\
        EMPTY RESULTS ARE AN ANSWER, NOT A FAILURE\n  \
          With NO `-t`/`--label`, EVERY label is searched. A ZERO-match result is a DEFINITIVE \
        absence (exit 0), never an error, and it SELF-DIAGNOSES on stderr: it echoes the active \
        filters and, when a `-t`/`-T` was on, an active probe NAMES the label(s) the pattern DOES \
        occur under (so an empty `-t user.message` that hid tool-name hits under `agent.tool.use` \
        tells you exactly that). Read the diagnosis and adjust the filter; do NOT assume a syntax \
        error or fall back to hand-parsing jsonl. To SEE a scope's record-types BEFORE you guess a \
        filter, run `--count-by label` (a per-leaf census; empty pattern = whole-scope census; a \
        leaf's count is exactly how many records `-t <leaf>` would surface; JSON `census` \
        rows).\n\n\
        THE LABEL TAXONOMY (-t / -T select by dot-segment prefix): 3 roles, 35 leaves\n  \
          LLM-VISIBILITY (v0.9.4): a bare ROLE selector (`-t user`) selects only the\n  \
        role's DELIVERED leaves - the conversation as the model receives/produces\n  \
        it. Eight leaves are invisible and need naming or a glob: `user.unsent`\n  \
        (a superseded draft is not in the surviving conversation - CC's own\n  \
        preservedMessages accounting excludes every draft), `harness.compaction.boundary`\n  \
        (a metrics-only system record), and the six gated leaves below. The glob form\n  \
        `-t 'user.*'` selects EVERY leaf under the prefix, visibility ignored; an\n  \
        intermediate prefix (`-t harness.compaction`) or an exact leaf path is a\n  \
        deliberate drill-down and keeps its full set. (`-t user` restores the 0.7\n  \
        contract: 0.9.2..0.9.3 briefly included drafts, which poisoned a real\n  \
        last-human-touch consumer.)\n  \
          RECORD-LEVEL DELIVERY: a bare role decides per RECORD, not per leaf, because\n  \
        Claude Code's request assembler runs one drop predicate and it disagrees with\n  \
        the leaf table in exactly three cases - a `system`/`local_command` record (a\n  \
        slash command's own echo and stdout) IS re-minted as a user message and sent,\n  \
        so `-t harness` shows it although `harness.meta.system` is invisible; an\n  \
        `isVirtual` record and the `<synthetic>` API-error placeholder are NOT sent, so\n  \
        `-t agent` / `-t user` hide them although their leaves are visible. A glob, an\n  \
        intermediate prefix and an exact leaf are unaffected and reach every record;\n  \
        an undelivered one renders a `[not delivered]` marker in the label zone, and\n  \
        JSON carries `delivered` on every hit.\n  \
          GATED LEAVES (v0.10.0, +1 in v0.10.1): the six promoted non-record leaves -\n  \
        `user.queued` and `harness.meta.{turn-duration,away-summary,stop-hooks,snapshot,\n  \
        system}` - are scanned ONLY when an explicit -t reaches them (the full path, a\n  \
        glob such as `-t 'user.*'` / `-t 'harness.*'`, or the `harness.meta` prefix), or\n  \
        when a `csift show --line/--uuid` address names the line. A bare scan with no\n  \
        -t, a bare role, and `--count-by label` without -t never parse those lines\n  \
        (every one is a non-message line, and most queued content is a duplicate\n  \
        automation pulse) - with ONE exception: a bare `-t harness` admits the system\n  \
        lines so the delivered `local_command` records among them can be reached, and\n  \
        the per-record rule then drops the rest. All six leaf DEFAULTS are\n  \
        LLM-invisible: none carries a message field.\n  \
        Their raw form is still `show --line N --raw`.\n  \
          user     .message                genuine human prose (a slash command with typed\n                                   \
        prose renders as `/name args`)\n           \
        .answer                 an answered AskUserQuestion: question, options and\n                                   \
        the picked answer as one unit\n           \
        .rejection              a plan/tool rejection carrying the user's typed\n                                   \
        instruction (+ a `[plan: …]` pointer when resolvable)\n           \
        .unsent                 a SUPERSEDED draft: sent, esc-recalled, edited and\n                                   \
        re-sent - the original stays on disk OUTSIDE turn\n                                   \
        numbering (never counted as user.message; a queued\n                                   \
        text edited before dispatch never becomes a record). A\n                                   \
        rendered draft states its distance from the message that\n                                   \
        replaced it - `differs from the sent message in N chars\n                                   \
        (P% of the final)`: insertions plus deletions of a shortest\n                                   \
        CHARACTER edit script, never a length difference, so P can\n                                   \
        exceed 100 (JSON superseding_line, superseding_uuid,\n                                   \
        diff_chars, diff_pct, diff_exact)\n           \
        .queued                 the human's text as it sat in the input QUEUE (a\n                                   \
        queue-operation line with content: enqueue, a popAll\n                                   \
        recall, or a remove with its reason); the label zone\n                                   \
        shows the operation. A queued automation pulse or\n                                   \
        peer message is not the human and carries no label;\n                                   \
        the queue line has no join key, so `dispatched` is\n                                   \
        never asserted (GATED - see above)\n  \
          agent    .message · .thinking    assistant prose · reasoning (a redacted block\n                                   \
        renders \"[redacted thinking]\")\n           \
        .thinking.narration     an API-issued one-sentence SUMMARY of the reasoning\n                                   \
        beside it, never the reasoning (renders \"[narration\n                                   \
        summary]\"; pure reasoning = -t agent.thinking\n                                   \
        -T agent.thinking.narration)\n           \
        .tool.use · .tool.result  tool traffic, paired by tool_use_id (the `▹` join)\n           \
        .communication.{inbox,sent,signal}  peer messages, rendered `from ⇨ to`\n           \
        .communication.channel  a csift-channel delivery: a message another lane (or a\n                                   \
        sender outside Claude Code) had a hook inject into this\n                                   \
        one. Rendered VERBATIM from its `[csift-channel v1 …]`\n                                   \
        envelope, direction from the header's `from=`. The one\n                                   \
        attachment leaf a DEFAULT scan sees\n  \
          harness  .notification.{workflow,monitor,subagent,background-command,task}\n           \
        .compaction.{summary,boundary} · .command.{invocation,stdout}\n           \
        .interrupt.{user,tool} · .schedule.{wakeup,continuation} · .meta.{hook,loop,attachment}\n           \
        .meta.turn-duration     the end-of-turn telemetry record (durationMs,\n                                   \
        messageCount, pendingBackgroundAgentCount,\n                                   \
        pendingWorkflowCount) behind the REPL's \"Done in Ns\" /\n                                   \
        \"Waiting for N agents\" lines (GATED)\n           \
        .meta.away-summary      the model-generated recap shown on return after 5+\n                                   \
        minutes away; never in the surviving conversation (GATED)\n           \
        .meta.stop-hooks        the Stop-hook execution ledger: each hook command and\n                                   \
        its duration, errors, preventedContinuation (GATED)\n           \
        .meta.snapshot          a file-history snapshot (every tracked path@version) or\n                                   \
        delta (one path's bump) - the v0.9.4 recover instrument,\n                                   \
        now searchable by path and version (GATED)\n           \
        .meta.system            every OTHER `type:system` subtype the harness writes for\n                                   \
        its own UI (informational such as the Remote Control\n                                   \
        disconnect warning, api_error, model_refusal_fallback,\n                                   \
        agents_killed, local_command, scheduled_task_fire); renders\n                                   \
        `[<subtype> <level>] <content>` (GATED, v0.10.1)\n  \
          `-t agent` selects the whole role, `-t agent.tool` both tool leaves, a full path\n  \
        just that leaf; `-T` excludes with the same grammar (a combination that excludes\n  \
        everything it includes is a parse error, as is a selector typo, with suggestions).\n  \
        A record carrying several labels prints ONCE, under its richest view (an AUQ answer\n  \
        is `user.answer`, not `agent.tool.result`). Glyphs: ◂ user · ▸ agent · ⚙ harness ·\n  \
        ▹ tool use↔result pairing · ⇨ message direction · · sibling.\n\n\
        JSON SCHEMA (per --format json)\n  \
          One ENVELOPE object PER matched exchange (NOT one bare record per line): \
        {session_id, is_subagent, parent_session_id, turn_index, ts_utc, ts_local, \
        record_uuids:[…], hits:[{session_id, is_subagent, parent_session_id, label, \
        labels:[…], line, uuid, excerpt, tool_name, pairing, \
        from, to, ts_utc, ts_local, queue_operation, queue_reason, refetch, refetch_uuid}, …]}: \
        `label` is the matched dotted path, `labels` \
        the record's full label set, `pairing` the tool_use↔tool_result join state \
        (paired | pending | orphan; null off the tool axis), `from`/`to` the comm direction \
        when the hit is `agent.communication.*`, `refetch` is the ready-to-run `csift show` \
        command addressed at the RIGHT id (run it verbatim), and `refetch_uuid` its \
        uuid-addressed twin: a line number is a durable address only while the transcript \
        is append-only, and Claude Code rewrites a live transcript in place on a stream \
        tombstone, a local compaction rewrite and a remote-ingress resume, shifting the lines \
        above the cut so a kept `--line` pointer resolves silently to another record; a \
        pointer held across a live session refetches by uuid. With `--count-by <axis>` the rows are `census` \
        objects instead. The \
        id trio rides EVERY hit object too (so bare `.hits[]` flattening keeps real ids); \
        `refetch` stays the preferred single-record path. With `--siblings`, the \
        envelope also carries a `siblings:[…]` array (same per-hit shape) for the turn's \
        non-matched records. Envelopes stream in \
        a COMBINED STABLE CHRONOLOGICAL order (subagent exchanges interleaved with top-level \
        by `ts_utc`, the turn-opening timestamp; timestamp-less exchanges sort last); the \
        per-hit `ts_utc` may be later than the envelope's for a deep tool_use match. \
        `session_id` is the transcript's own id: a re-feedable top-level uuid, OR a bare \
        SUBAGENT hex when `is_subagent` is true (that hex is NOT a re-feedable `@<uuid>` target; \
        re-feed `parent_session_id`, which is always the owning top-level uuid). \
        `record_uuids` lists every record stitched into the round-trip (§6.4 completeness \
        evidence). A trailing footer object {matched, sessions, transcript_ids, dropped_by_cap, \
        skipped_lines, with_elicitation_sidecar, excerpts_truncated} closes the stream, plus \
        {definitive_absence, active_filters, excluded_by_label, gated_leaves_unreached} on a \
        ZERO-match run (`queue_operation`/`queue_reason` are the `user.queued` facts, null \
        elsewhere; `gated_leaves_unreached` is true when no selector reached a gated leaf). \
        (`transcript_ids` is the per-TRANSCRIPT matching-id set, named apart from `-l`'s \
        owning-session ids.) (Whole-document `json.load` fails; parse line-by-line as JSONL: N \
        envelopes then the footer.)";
