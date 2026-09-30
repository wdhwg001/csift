//! The body of `search --help` down to the family regions: the examples, the output
//! geometry, the selector law, the leaf list and the JSON schema. Its closing notes live
//! in [`super::outro`] and follow it directly, so the schema block reads as one piece.
//!
//! No test pins this text as a literal. The page grows by one column-0 region per family
//! that has written something, so a literal would go stale the first time a family wrote a
//! sentence; the frame test checks the ORDER, the two seams and the region shape instead.
//! A family edits its OWN section file, never this one.

/// See the module doc.
pub(crate) const INTRO: &str = "EXAMPLES\n  \
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
        @<lane> -t harness.hook.context` proves the text reached that lane's context. \
        `csift msg <ID>` runs that same join and adds csift's own ledger beside it. The \
        delivery itself needs no flag: `agent.communication.channel` is one of THREE \
        attachment leaves a DEFAULT scan sees, the other two being the person's own \
        mid-turn steer (`user.message`, a `queued_command` payload whose own \
        `origin.kind` reads `human`) and the model's own final answer \
        (`agent.message.structured`, a `structured_output` payload). Each has its own \
        value needle, so naming it costs one memmem rather than a parse of every \
        attachment line, and only a `-t` that EXCLUDES the leaf turns its needle off. \
        Every OTHER attachment leaf still needs `-t` or `--attachments`.\n\n\
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
        `[<kind> <task-id>[, <task-id>…] <status>] <summary>` (kind = background-command | \
        workflow | subagent | monitor | task, read from the summary); never the raw XML. ONE \
        pulse can close SEVERAL tasks - at the next session start Claude Code reconciles what \
        the previous one left open - and the label then names EVERY task id, comma-joined, plus \
        a `(orphan reconciliation: <shell|agent|workflow>)` marker for the internal scan marker \
        that names the kind. JSON hit rows carry the same as `task_ids` and `orphan_kind`. Match \
        it like any text, e.g. `csift search 'background-command' -t harness.notification`. The \
        `<kind>` prefix distinguishes a machine opener from a genuine human message.\n\n\
        EMPTY RESULTS ARE AN ANSWER, NOT A FAILURE\n  \
          With NO `-t`/`--label`, a scan reaches every leaf the tree below tags [scan] or \
        [scan/needle]. A GATED leaf - tagged [-t] or [-t/att]: the whole `harness.bookkeeping` \
        category and the gated attachment, system and turn leaves - is reached only by naming \
        it with `-t` (or, for [-t/att], by `--attachments`), and the zero-match note lists the \
        gated leaves an absence does not cover. A ZERO-match result is a DEFINITIVE \
        absence (exit 0), never an error, and it SELF-DIAGNOSES on stderr: it echoes the active \
        filters and, when a `-t`/`-T` was on, an active probe NAMES the label(s) the pattern DOES \
        occur under (so an empty `-t user.message` that hid tool-name hits under `agent.tool.use` \
        tells you exactly that). Read the diagnosis and adjust the filter; do NOT assume a syntax \
        error or fall back to hand-parsing jsonl. To SEE a scope's record-types BEFORE you guess a \
        filter, run `--count-by label` (a per-leaf census; empty pattern = whole-scope census; a \
        leaf's count is exactly how many records `-t <leaf>` would surface; JSON `census` \
        rows).\n\n\
        THE LABEL TAXONOMY (-t / -T select by dot-SEGMENT prefix): 3 roles, 17 categories,\n  \
        73 leaves\n  \
          A selector is a LEAF path, a CATEGORY (`agent.message`, `harness.turn`), a ROLE\n  \
        (`user`), or any of those plus `.*`. A bare ROLE or CATEGORY selects only the\n  \
        leaves the model RECEIVES - the coarse \"show me the conversation\" ask; a GLOB or\n  \
        an exact LEAF reaches every record under all three narrowing axes below. No -t\n  \
        means every leaf a default scan reaches (the GATE words below). `-T` excludes with\n  \
        the same grammar, and ONLY a -T that cancels your -t is an error (a selector\n  \
        typo is a parse error naming the valid set).\n  \
          `harness.bookkeeping` is the one category whose whole face is invisible: the\n  \
        bare form is ACCEPTED, matches nothing by the law, and says so on stderr with the\n  \
        remedy (`-t harness.bookkeeping.*`); JSON carries `empty_visible_face`.\n  \
          RETIRED, each a hard error naming its successors: `harness.meta` and every\n  \
        `harness.meta.*` path, the flat `thinking`/`tool`/`tool-response`, and\n  \
        `harness.schedule.continuation`. `--additional-context` is retired too - the leaf\n  \
        it gated has a name, `-t harness.hook.context`. There is no alias anywhere.\n  \
          GATE words below: [scan] a line a default scan already parses ·\n  \
        [-t] you must name the leaf, its category or a glob reaching it · [-t/att]\n  \
        either that or the wide `--attachments` flag, which admits every attachment line ·\n  \
        [scan/needle] an attachment leaf a DEFAULT scan reaches through its own value\n  \
        needle, which only a `-t` excluding that leaf turns off: the three are words\n  \
        someone owned - a csift-channel delivery, the person's mid-turn steer, the\n  \
        model's structured answer.\n  \
        A gated leaf costs a query that cannot want it NOTHING (one gated memmem each).\n  \
          user        .message [scan] the person\'s own prose (a slash command with typed\n                                   \
        args renders `/name args`) · .answer [scan] an answered\n                                   \
        AskUserQuestion as one Q+options+answer unit · .rejection\n                                   \
        [scan] a tool rejection carrying the typed instruction (+ a\n                                   \
        `[plan: …]` pointer) · .unsent [scan] a SUPERSEDED draft:\n                                   \
        sent, esc-recalled, edited, re-sent, the original left on\n                                   \
        disk OUTSIDE turn numbering, with its distance from the\n                                   \
        message that replaced it · .rewound [scan] a turn that WAS\n                                   \
        answered and the conversation was rewound past · .queued\n                                   \
        [-t] the human\'s text while it sat in the input QUEUE\n  \
          agent       .message.plain [scan] the assistant\'s own text ·\n                                   \
        .message.structured [scan/needle] a schema-bound subagent\'s\n                                   \
        final structured answer · .thinking.summary [scan] the\n                                   \
        API\'s summary of the reasoning, never the reasoning ·\n                                   \
        .thinking.narration [scan] its one-sentence progress\n                                   \
        update between tool calls · .thinking.omitted [scan] a\n                                   \
        thinking block the API returned with NO summary text ·\n                                   \
        .thinking.redacted [scan] its safety-redacted ciphertext ·\n                                   \
        .tool.{use,result} [scan] tool traffic, paired by\n                                   \
        tool_use_id (the `▹` join) ·\n                                   \
        .communication.{inbox,sent,signal} [scan] peer messages,\n                                   \
        rendered `from ⇨ to` - a COORDINATOR relay reads\n                                   \
        `coordinator ⇨ self`, the literal word, because its\n                                   \
        origin object names a kind and no sender ·\n                                   \
        .communication.channel [scan/needle] a\n                                   \
        csift-channel delivery, rendered VERBATIM from its\n                                   \
        `[csift-channel v1 …]` envelope - one of the three\n                                   \
        [scan/needle] attachment leaves\n  \
          harness     .notification.{workflow,monitor,subagent,background-command,task}\n                                   \
        [scan] `<task-notification>` completion pulses, rendered\n                                   \
        `[<kind> <task-id>… <status>] <summary>` ·\n                                   \
        .compaction.summary [scan] / .compaction.boundary [scan]\n                                   \
        the summary plus its metrics record, both carrying `mode`\n                                   \
        (compact | summarize-from-here | summarize-up-to-here) ·\n                                   \
        .command.{invocation,stdout} [scan] the slash wrapper and\n                                   \
        its output · .command.caveat [scan] the caveat the command\n                                   \
        EXECUTOR writes after one · .interrupt.{user,tool} [scan] ·\n                                   \
        .schedule.wakeup [scan] a fired timer tick ·\n                                   \
        .schedule.event [-t] the scheduler\'s own fire RECORD ·\n                                   \
        .schedule.fire [scan] the PROMPT it submits, recognised by\n                                   \
        its parent link to that record, with the instant in the\n                                   \
        label zone · .schedule.tick [scan] the autonomous-loop\n                                   \
        driver tick · .resume.prompt [scan] the loader\'s repair\n                                   \
        prompt · .resume.placeholder [scan] the stand-in reply it\n                                   \
        splices after one ([paired]/[unpaired]) ·\n                                   \
        .hook.context [-t/att] context a hook injected ·\n                                   \
        .hook.outcome [-t/att] a hook RUN\'s own record ·\n                                   \
        .hook.feedback [scan] a blocking hook\'s feedback, over the four \n                                   \
        names one formatter writes (Stop, TeammateIdle, TaskCreated, \n                                   \
        TaskCompleted) ·\n                                   \
        .context.{reminder,listing,prompt,file,mode,ide,task}\n                                   \
        [-t/att] what the harness attached to the turn ·\n                                   \
        .context.{image,skill} [scan] its isMeta image references\n                                   \
        and a skill body the harness injected - preloaded into\n                                   \
        a subagent\'s opening, or expanded into any transcript\n                                   \
        where the skill was invoked ·\n                                   \
        .enforce.{structured-output,brief,handback,retry} [scan]\n                                   \
        the redo nudges the harness sends back to the model ·\n                                   \
        .turn.{duration,stop-hooks,recap} [-t] the end-of-turn\n                                   \
        telemetry records · .system.{refusal,api-error,notice,\n                                   \
        kill,command} [-t] the `type:system` event kinds, of which\n                                   \
        only `.command` (a slash command\'s echo and stdout) is\n                                   \
        re-minted for the model · .bookkeeping.{last-prompt,mode,\n                                   \
        name,atis,bridge,suppression,pr-link,artifact,file-history,\n                                   \
        cost,fork,continued,queue} [-t] the 18 non-transcript line\n                                   \
        types: no uuid, no message, never in the model\'s context\n  \
          A RENDERED DRAFT carries its distance from the message that replaced it:\n  \
        `differs from the sent message in N chars (P% of the final)` - insertions plus\n  \
        deletions of a shortest CHARACTER edit script, never a length difference, so P\n  \
        can exceed 100. JSON: `superseding_line`, `superseding_uuid` and\n  \
        `diff_chars`, `diff_pct`, `diff_exact`. A `.rewound` opener leads the same line\n  \
        with\n  \
        `rewound: the conversation continued from L<n> instead`.\n  \
          THREE NARROWING AXES, and only the FIRST is per-leaf. AXIS 1, LEAF VISIBILITY:\n  \
        28 of the 73 leaves are outside a bare role or category - the two scan-layer user\n  \
        leaves, `user.queued` and all 13 bookkeeping leaves (non-transcript lines with no\n  \
        message field), the two textless thinking leaves, the message-less system records\n  \
        (the compaction boundary, the fire record, the three turn leaves, four of the five\n  \
        system event kinds), and `harness.hook.outcome`. AXIS 2, RECORD-LEVEL DELIVERY:\n  \
        Claude Code\'s request assembler runs one drop predicate, and where it disagrees\n  \
        with the leaf default csift mirrors it per record - a `system`/`local_command`\n  \
        record IS sent, an `isVirtual` record and the `<synthetic>` API-error placeholder\n  \
        are NOT, an attachment whose payload type the converter drops is NOT, and a\n  \
        `hook_success` IS exactly when it carries a rendered body. A record the model\n  \
        never receives - by its leaf (axis 1) or by this predicate - renders\n  \
        `[not delivered]`, exactly where the JSON `delivered` every hit carries reads\n  \
        false. ONE leaf is exempt from axis 2, the single stated exception to the\n  \
        visible-face law: `agent.message.structured` rides a converter-dropped payload, so\n  \
        the axis would take it out of `-t agent` and `-t agent.message` and leave that\n  \
        category showing only its plain half. The axis asks what the model RECEIVED and\n  \
        this is the leaf the model WROTE, so a bare `agent.message` gives you both - the\n  \
        way the API gives a turn free text or a schema-bound answer and never both. The\n  \
        record was still not delivered, so its `delivered` field and its marker are\n  \
        unchanged. AXIS 3, SURVIVAL: Claude Code rebuilds a session by walking `parentUuid`\n  \
        from ONE leaf, and a record that walk no longer reaches is off the conversation\n  \
        however it was delivered - marked `[abandoned]` / `[rewound]` /\n  \
        `[replay copy of L<n>]`, outside turn numbering, reached by a glob or an exact\n  \
        leaf. A PRE-CUT record (above a compaction cut) is never narrowed away: what the\n  \
        model saw AT THE TIME is the question an archive answers. JSON carries `survival`,\n  \
        `abandoned_root_line` and `replay_copy_of` per hit and the totals in the summary;\n  \
        the text footer states every one of them.\n  \
          THE 1:1 LAW: `csift show --line N` never hard-misses a PARSEABLE line. A shape\n  \
        csift models no leaf for renders `? unmodeled: <type[/subtype|/attachment.type]>`\n  \
        with the line\'s own top-level keys (JSON `label`:null, `labels`:[], `unmodeled`),\n  \
        so an unknown line is visibly unknown rather than absent. In that excerpt a\n  \
        `headers` member is ELIDED at every depth and prints as `<elided>`; `--raw` is\n  \
        the one path that prints an HTTP response header set. A SCAN never emits one,\n  \
        so no census, count or -t result moves. A line that is not a JSON object at all is\n  \
        still a hard miss pointing at `--raw`.\n  \
          A record carrying several labels prints ONCE, under its richest view (an AUQ\n  \
        answer is `user.answer`, not `agent.tool.result`); `labels` is ordered\n  \
        richest-first and the rendered view is the first entry your -t/-T leaves alive.\n  \
        Glyphs: ◂ user · ▸ agent · ⚙ harness · ? an unmodeled unit · ▹ tool use↔result\n  \
        pairing · ⇨ message direction · · sibling.\n\n\
        JSON SCHEMA (per --format json)\n  \
          One ENVELOPE object PER matched exchange (NOT one bare record per line): \
        {session_id, is_subagent, parent_session_id, turn_index, ts_utc, ts_local, \
        record_uuids:[…], hits:[{session_id, is_subagent, parent_session_id, label, \
        labels:[…], line, uuid, excerpt, body, tool_name, tool_use_id, pairing, \
        from, to, ts_utc, ts_local, source (\"elicitation-sidecar\" | null), \
        queue_operation, queue_reason, resume_paired, \
        denial_kind, image_ids, \
        survival, abandoned_root_line, replay_copy_of, mode, compact_metadata, \
        for:[{kind,basis,target:{line,uuid,session,note},reason}], \
        refetch, refetch_uuid}, …]}: \
        `label` is the matched dotted path, `labels` \
        the record's full label set, `excerpt` the match-centered fragment (ONE line: \
        whitespace runs collapse to single spaces, and it is clipped with the explicit \
        `… (+N chars)` marker unless `--no-truncate` lifts the cap), `body` the SAME \
        rendered text with its NEWLINES intact - non-null only under `--no-truncate`, so a \
        default scan carries one fragment per hit rather than a fragment plus a full copy of \
        every record, and it is the RENDERED form, so a user message the opener renderer \
        already flattened carries no newline there either (`--raw` is the path to the \
        bytes), `pairing` the tool_use↔tool_result join state \
        (paired | pending | orphan; null off the tool axis), `from`/`to` the comm direction \
        when the hit is `agent.communication.*`, `denial_kind` the harness's own reason a \
        rejected call never ran (null off a rejection's tool_result), `image_ids` the \
        extractable `#N` / `L<line>i<n>` ids the record carries (an empty array when \
        none), `for` the record's PROVENANCE edges - one array however many relations a \
        unit carries, each `{kind, basis, target, reason}`, present only on a unit that \
        carries one - and `for_root` the message the unit corresponds to: \
        `{line, uuid, hops, note}` on EVERY hit, naming the record that opened its turn \
        and the parent hops up to it, with `note: \"turn opener\"` when the unit IS that \
        record, `note: \"not reached by the parent chain\"` when the turn grouping \
        places it but the parent links do not carry it there, and \
        `note: \"no opener above this unit\"` when there is no opener above it at all \
        (the synthetic lead a transcript opens with, the head of an abandoned branch), \
        `refetch` is the ready-to-run `csift show` \
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
        skipped_lines, with_elicitation_sidecar, excerpts_truncated, superseded_drafts, \
        abandoned_records, rewound_turns, replay_copies, boundary_cut_line, leaf_source} \
        closes the stream. It ALSO carries the crossing set {crossing_budget, \
        boundaries_crossed, budget_exhausted, boundaries_above_floor, floor_line, \
        floor_cause} whenever `--cross-compactions` is non-zero - how many compaction \
        boundaries the survival walk stepped over, how many it did not, and where the chain \
        floor sits and why; under `--cross-compactions 0` csift crossed nothing and the keys \
        are absent. The `--count-by` census summary carries the same six (its two text \
        lines ride stderr, beside the census's own accounting note, so the table stays \
        parseable). Plus \
        {definitive_absence, active_filters, excluded_by_label, outside_turn_numbering, \
        gated_leaves_unreached, attachment_leaves_unread} on a \
        ZERO-match run (`outside_turn_numbering` counts the records a `--turn` window left \
        out because they sit outside turn numbering, null without `--turn`; `queue_operation`/`queue_reason` are the `user.queued` facts and \
        `resume_paired` the `harness.resume.placeholder` one, null \
        elsewhere; `gated_leaves_unreached` is true when no selector reached a gated leaf, \
        and `attachment_leaves_unread` lists the attachment leaves the scan did not read - \
        every one on a run with neither `--attachments` nor a `-t` reaching one, else \
        empty - which the stderr note names beside the flag).\
";
