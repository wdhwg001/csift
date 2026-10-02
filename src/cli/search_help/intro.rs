//! The body of `search --help` down to the family regions: the examples, the output
//! geometry, the selector law, the leaf list and the JSON schema. Its closing notes live
//! in [`super::outro`] and follow it directly, so the schema block reads as one piece.
//!
//! No test pins this text as a literal. The page grows by one column-0 region per family
//! that has written something, so a literal would go stale the first time a family wrote a
//! sentence; the frame test checks the ORDER, the two seams and the region shape instead.
//! A family edits its OWN section file, never this one.
//!
//! Every line carries its own break: clap prints an after-help verbatim and wraps nothing,
//! so a rendered line is exactly as wide as the source makes it.

/// See the module doc.
pub(crate) const INTRO: &str = "EXAMPLES\n  \
          csift search \"carry\"                                  # all projects, smart-case\n  \
          csift search \"carry\" .                                # this project (positional PATH)\n  \
          csift search -i \"askuserquestion\" -t agent.tool.use   # tool_use blocks naming AUQ\n  \
          csift search \"\" -t user --since 2h .                  # user turns, last 2h, this project\n  \
          csift search \"tail.read\" --multiline @0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d\n  \
          csift search \"panic\" -t agent.message -t agent.thinking --turn 10..20 --max-count 50\n  \
          csift search \"persisted-output\" --resolve-persisted --format json\n  \
          csift search \"refactor\" -c                            # COUNT matches only (rg -c)\n  \
          csift search \"refactor\" -l                            # WHICH sessions, one id a line (rg -l)\n  \
          # …then scope the NEXT command to the sessions that matched\n  \
          csift search \"refactor\" -l | csift files --sessions-from -\n  \
          csift search \"\" @<uuid> -t agent -T agent.thinking    # the agent role MINUS its thinking\n  \
          # raw lines, for any field csift does not render\n  \
          csift search \"\" @<uuid> -t agent.message --raw | jq -r '.message.model'\n  \
          csift search \"let's chat\" -t user --siblings          # the match WITH the turn's other side\n  \
          # …and READ the reply end to end\n  \
          csift search \"let's chat\" -t user --siblings --no-truncate\n  \
          csift search \"X\" --max-count 1                        # when did X FIRST happen?\n  \
          csift search \"X\" --max-count -1                       # the most recent occurrence of X\n  \
          csift show @<tok> --line <n>                          # follow up a hit by token and L<n>\n\n\
        OUTPUT GEOMETRY (text mode)\n  \
          Exchanges emit oldest-first, in a stable chronological order across every transcript in\n  \
          scope, and undated exchanges come last. Each exchange header opens with a STABLE\n  \
          id-prefix token, the first 8 characters of the owning transcript id. A collision within\n  \
          one output lengthens the colliding group to 12 characters and then to the full id, and a\n  \
          teammate id renders whole. The token is directly usable as an `@` target and is\n  \
          identical across invocations. A subagent exchange carries `(parent <first-8>)` on every\n  \
          header.\n\n  \
          The head carries the scope, the match totals and the direction. The tail repeats the\n  \
          totals and adds integrity notes and refetch guidance. Each over-long fragment marks its\n  \
          own truncation inline (`(+N chars)`). To limit output, prefer `--max-count N` (the\n  \
          earliest N) or `--max-count -N` (the latest N) over piping into `head`/`tail`. A capped\n  \
          run keeps every note, while a pipe amputates one end of the ledger.\n\n\
        SIBLINGS (`--siblings`)\n  \
          A match renders only the records that MATCHED. `--siblings` also renders the OTHER\n  \
          records of the same turn, the back-and-forth around the hit, under a `·` marker. A\n  \
          matched user question then surfaces WITH the agent's reply. The policy is fixed.\n  \
          Message units always render (user.*, agent.message, agent.communication.*). Chattier\n  \
          machinery is capped per leaf (thinking ≤2, tool.use ≤3, tool.result ≤3, harness ≤2), and\n  \
          the capped-away remainder surfaces as an explicit\n  \
          `(+N more · csift show @<id> --line A..B)` pointer. A record that itself matched is never\n  \
          duplicated as a sibling.\n\n\
        SEE ALSO\n  \
          Images. search is the surface that PRINTS image handles (`[N image(s): #7]` /\n  \
          `L123i1`). `csift image <target> --id <ID> --out DIR` extracts them to files you can\n  \
          then Read. Pass a `#N` handle as the bare number. An image-bearing row is evidence and\n  \
          never a blank.\n\n  \
          The csift channel. search is the FACT half of the channel. A delivered message lands in\n  \
          the receiving lane as an attachment carrying its envelope, so\n  \
          `csift search '<message id>' @<lane> -t harness.hook.context` proves the text reached\n  \
          that lane's context. `csift msg <ID>` runs that same join and adds csift's own ledger\n  \
          beside it. The delivery itself needs no flag. `agent.communication.channel` is one of\n  \
          THREE attachment leaves a DEFAULT scan sees. The other two are the person's own\n  \
          mid-turn steer (`user.message`, a `queued_command` payload whose own `origin.kind`\n  \
          reads `human`) and the model's own final answer (`agent.message.structured`, a\n  \
          `structured_output` payload). Each has its own value needle, so naming it costs one\n  \
          memmem and no parse of every attachment line. Only a `-t` that EXCLUDES the leaf turns\n  \
          its needle off. Every OTHER attachment leaf still needs `-t` or `--attachments`.\n\n\
        COUNT (`-c` / `--count-only`)\n  \
          `-c`/`--count-only` prints just the integer EXCHANGE total, the matched round-trips, in\n  \
          the ripgrep `-c` idiom. It honors every filter, and per-RECORD counts are `--count-by`.\n  \
          The same total is ALSO always in the normal output's footer beside the distinct-session\n  \
          total, and `--count-only` isolates that ONE integer for a pipe. To list WHICH sessions\n  \
          matched, use `-l`. It prints one owning uuid per line and pipes straight into\n  \
          `--sessions-from -`.\n\n\
        REGEX DIALECT (linear-time, RE2-class)\n  \
          The pattern is the Rust `regex` crate (regex::bytes), which GUARANTEES linear-time\n  \
          matching in the input length. Catastrophic backtracking never happens.\n\n  \
          Supported      literals, character classes [...] / [^...] / \\d \\w \\s and Unicode\n  \
          \x20              classes \\p{...}, alternation |, groups (...) and non-capturing (?:...),\n  \
          \x20              quantifiers * + ? {m,n} (greedy and lazy *?), anchors ^ $ \\b \\B,\n  \
          \x20              dot . (--multiline lets it cross newlines), inline flags\n  \
          \x20              (?i)(?m)(?s)(?x). Unicode-aware by default.\n  \
          NOT supported  backreferences \\1, lookahead and lookbehind (?=) (?!) (?<=) (?<!),\n  \
          \x20              atomic groups and possessive quantifiers (?>...) / a*+. These need\n  \
          \x20              non-linear engines. A pattern using one fails to COMPILE with a clear\n  \
          \x20              error, by design.\n\n  \
          Case follows smart-case by default, insensitive unless the pattern has an uppercase\n  \
          letter, and -i forces insensitive matching. --multiline lives in the SAME dialect and\n  \
          sets the (?s)(?m) flags.\n\n  \
          One CAVEAT concerns tool_use.input, which is matched RE-SERIALIZED. Every tool_use's\n  \
          matchable text is its name plus its re-serialized JSON input, AskUserQuestion's and\n  \
          every other tool's alike. A real newline inside a Bash `input.command`, for instance,\n  \
          is already the two-character sequence \\n by match time. Match the literal `\\\\n` there.\n  \
          --multiline is correctly irrelevant there. It helps only where the RENDERED text keeps\n  \
          real newlines, which is message text, thinking and tool_result bodies.\n\n\
        AUTOMATION TRIGGERS (`harness.notification.*`)\n  \
          A machine `<task-notification>` is a COMPLETION pulse for a background command, a\n  \
          workflow, a spawned agent or a monitor tick. Claude Code injects it as a `type:\"user\"`\n  \
          record, and it OPENS a turn the way a human message does. It classifies under\n  \
          `harness.notification.<kind>`, NOT under `user`. It renders as a PARSED attribution\n  \
          label, `[<kind> <task-id>[, <task-id>…] <status>] <summary>`, where the kind is\n  \
          background-command, workflow, subagent, monitor or task, read from the summary. The\n  \
          raw `<task-id>`/`<output-file>` XML never prints.\n\n  \
          ONE pulse can close SEVERAL tasks, because at the next session start Claude Code\n  \
          reconciles what the previous one left open. The label then names EVERY task id,\n  \
          comma-joined. The internal scan marker that names the kind never prints either, and\n  \
          becomes a trailing `(orphan reconciliation: <shell|agent|workflow>)` marker. JSON hit\n  \
          rows carry the same as `task_ids` and `orphan_kind`. Match a pulse like any text.\n  \
          \x20 csift search 'background-command' -t harness.notification\n  \
          \x20 csift search 'background-command' -t harness.notification.background-command\n  \
          The `<kind>` prefix tells a machine opener from a genuine human message.\n\n\
        EMPTY RESULTS ARE AN ANSWER, NOT A FAILURE\n  \
          With NO `-t`/`--label`, a scan reaches every leaf the tree below tags [scan] or\n  \
          [scan/needle]. A GATED leaf is one tagged [-t] or [-t/att], which covers the whole\n  \
          `harness.bookkeeping` category and the gated attachment, system and turn leaves. A\n  \
          gated leaf is reached only by naming it with `-t`, or, for [-t/att], by `--attachments`.\n  \
          The zero-match note lists the gated leaves an absence does not cover. A ZERO-match\n  \
          result is a DEFINITIVE absence (exit 0), never an error, and it SELF-DIAGNOSES on\n  \
          stderr. It echoes the active filters, and when a `-t`/`-T` was on, an active probe NAMES\n  \
          the label(s) the pattern DOES occur under. An empty `-t user.message` that hid tool-name\n  \
          hits under `agent.tool.use` tells you exactly that. Read the diagnosis and adjust the\n  \
          filter. Do NOT assume a syntax error or fall back to hand-parsing jsonl. To SEE a\n  \
          scope's record types BEFORE you guess a filter, run `--count-by label`. It is a per-leaf\n  \
          census, an empty pattern makes it a whole-scope census, and a leaf's count is exactly\n  \
          how many records `-t <leaf>` would surface (JSON `census` rows).\n\n\
        THE LABEL TAXONOMY (-t / -T select by dot-SEGMENT prefix)\n  \
          3 roles, 17 categories, 73 leaves. A selector is a LEAF path, a CATEGORY\n  \
        (`agent.message`, `harness.turn`), a ROLE (`user`), or any of those plus `.*`.\n\n  \
          A bare ROLE or CATEGORY selects only the leaves the model RECEIVES, the coarse \"show me\n  \
        the conversation\" ask. A GLOB or an exact LEAF reaches every record under all three\n  \
        narrowing axes below. With no -t, every leaf a default scan reaches is eligible (the GATE\n  \
        words below). `-T` excludes with the same grammar, and ONLY a -T that cancels your -t is\n  \
        an error. A selector typo is a parse error naming the valid set.\n\n  \
          `harness.bookkeeping` is the one category whose whole face is invisible. Its bare form\n  \
        is ACCEPTED, matches nothing by the law, and says so on stderr with the remedy\n  \
        (`-t harness.bookkeeping.*`). JSON carries `empty_visible_face`.\n\n  \
          RETIRED spellings are each a hard error naming their successors. They are `harness.meta`\n  \
        and every `harness.meta.*` path, the flat `thinking`/`tool`/`tool-response`, and\n  \
        `harness.schedule.continuation`. `--additional-context` is retired too, and the leaf it\n  \
        gated has a name, `-t harness.hook.context`. There is no alias anywhere.\n\n  \
          GATE words in the tree below.\n  \
          \x20 [scan]         a line a default scan already parses\n  \
          \x20 [-t]           you must name the leaf, its category or a glob reaching it\n  \
          \x20 [-t/att]       either that, or the wide `--attachments` flag, which admits every\n  \
          \x20                attachment line\n  \
          \x20 [scan/needle]  an attachment leaf a DEFAULT scan reaches through its own value\n  \
          \x20                needle, which only a `-t` excluding that leaf turns off. The three\n  \
          \x20                such leaves are words someone owned, a csift-channel delivery, the\n  \
          \x20                person's mid-turn steer and the model's structured answer.\n  \
          A gated leaf costs a query that cannot want it NOTHING (one gated memmem each).\n\n  \
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
        .hook.feedback [scan] a blocking hook\'s feedback, over the four\n                                   \
        names one formatter writes (Stop, TeammateIdle, TaskCreated,\n                                   \
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
        types, which carry no uuid and no message and never reach\n                                   \
        the model\'s context\n\n  \
          A RENDERED DRAFT carries its distance from the message that replaced it, as the line\n  \
        `differs from the sent message in N chars (P% of the final)`. N counts the insertions\n  \
        plus deletions of a shortest CHARACTER edit script, never a length difference, so P can\n  \
        exceed 100. JSON carries `superseding_line`, `superseding_uuid` and the counts\n  \
        `diff_chars`, `diff_pct`, `diff_exact`. A `.rewound` opener leads the same line with\n  \
        `rewound: the conversation continued from L<n> instead`.\n\n  \
          THREE NARROWING AXES shape a bare role or category, and only the FIRST is per-leaf.\n\n  \
          AXIS 1, LEAF VISIBILITY. 28 of the 73 leaves are outside a bare role or category. They\n  \
        are the two scan-layer user leaves, `user.queued` and all 13 bookkeeping leaves\n  \
        (non-transcript lines with no message field), the two textless thinking leaves, the\n  \
        message-less system records (the compaction boundary, the fire record, the three turn\n  \
        leaves, four of the five system event kinds), and `harness.hook.outcome`.\n\n  \
          AXIS 2, RECORD-LEVEL DELIVERY. Claude Code\'s request assembler runs one drop predicate,\n  \
        and where it disagrees with the leaf default csift mirrors it per record. A\n  \
        `system`/`local_command` record IS sent. An `isVirtual` record and the `<synthetic>`\n  \
        API-error placeholder are NOT, and neither is an attachment whose payload type the\n  \
        converter drops. A `hook_success` IS sent exactly when it carries a rendered body. A\n  \
        record the model never receives, by its leaf (axis 1) or by this predicate, renders\n  \
        `[not delivered]`, exactly where the JSON `delivered` every hit carries reads false.\n  \
        ONE leaf is exempt from axis 2, the single stated exception to the visible-face law.\n  \
        `agent.message.structured` rides a converter-dropped payload, so the axis would take it\n  \
        out of `-t agent` and `-t agent.message` and leave that category showing only its plain\n  \
        half. The axis asks what the model RECEIVED, and this is the leaf the model WROTE, so a\n  \
        bare `agent.message` gives you both leaves. That matches the API, which gives a turn\n  \
        free text or a schema-bound answer and never both. Its `delivered` field and its\n  \
        marker stay unchanged, because the record was still not delivered.\n\n  \
          AXIS 3, SURVIVAL. Claude Code rebuilds a session by walking `parentUuid` from ONE leaf.\n  \
        A record that walk no longer reaches is off the conversation however it was delivered.\n  \
        It is marked `[abandoned]`, `[rewound]` or `[replay copy of L<n>]`, sits outside turn\n  \
        numbering, and is reached by a glob or an exact leaf. A PRE-CUT record (above a\n  \
        compaction cut) is never narrowed away, because what the model saw AT THE TIME is the\n  \
        question an archive answers. JSON carries `survival`, `abandoned_root_line` and\n  \
        `replay_copy_of` per hit and the totals in the summary, and the text footer states\n  \
        every one of them.\n\n  \
          THE 1:1 LAW. `csift show --line N` never hard-misses a PARSEABLE line. A shape csift\n  \
        models no leaf for renders `? unmodeled: <type[/subtype|/attachment.type]>` with the\n  \
        line\'s own top-level keys (JSON `label`:null, `labels`:[], `unmodeled`), so an unknown\n  \
        line is visibly unknown and never absent. In that excerpt two members are ELIDED at\n  \
        every depth and print as `<elided>`. A `headers` member holds an HTTP response header\n  \
        set, and `--raw` is the one path that prints one. A `data` member holds a base64 image\n  \
        or document source, or a redacted thinking block's ciphertext. Its head identifies\n  \
        nothing, and at the 80-character cut it would crowd out the sibling keys that do say\n  \
        what the shape is. A SCAN never emits an unmodeled unit, so no census, count or -t\n  \
        result moves. A line that is not a JSON object at all is still a hard miss pointing at\n  \
        `--raw`.\n\n  \
          A record carrying several labels prints ONCE, under its richest view (an AUQ answer is\n  \
        `user.answer`, not `agent.tool.result`). `labels` is ordered richest-first, and the\n  \
        rendered view is the first entry your -t/-T leaves alive.\n\n  \
          Glyphs.\n  \
          \x20 ◂ user    ▸ agent    ⚙ harness    ? an unmodeled unit    · sibling\n  \
          \x20 ▹ tool use↔result pairing    ⇨ message direction\n\n\
        JSON SCHEMA (per --format json)\n  \
          One ENVELOPE object PER matched exchange, never one bare record per line.\n  \
          \x20 {session_id, is_subagent, parent_session_id, turn_index, ts_utc, ts_local,\n  \
          \x20  record_uuids:[…], hits:[{session_id, is_subagent, parent_session_id, label,\n  \
          \x20  labels:[…], line, uuid, excerpt, body, tool_name, tool_use_id, pairing, from, to,\n  \
          \x20  ts_utc, ts_local, source (\"elicitation-sidecar\" | null), queue_operation,\n  \
          \x20  queue_reason, resume_paired, denial_kind, image_ids, survival,\n  \
          \x20  abandoned_root_line, replay_copy_of, mode, compact_metadata,\n  \
          \x20  for:[{kind,basis,target:{line,uuid,session,note},reason}],\n  \
          \x20  refetch, refetch_uuid}, …]}\n\n  \
          The per-hit keys that need a word.\n  \
          \x20 label            the matched dotted path\n  \
          \x20 labels           the record's full label set\n  \
          \x20 excerpt          the match-centered fragment, ONE line. Whitespace runs collapse to\n  \
          \x20                  single spaces, and it is clipped with the explicit\n  \
          \x20                  `… (+N chars)` marker unless `--no-truncate` lifts the cap.\n  \
          \x20 body             the SAME rendered text with its NEWLINES intact. It is non-null\n  \
          \x20                  only under `--no-truncate`, so a default scan carries one fragment\n  \
          \x20                  per hit and no full copy of every record. It is the RENDERED form,\n  \
          \x20                  so a user message the opener renderer already flattened carries no\n  \
          \x20                  newline there either (`--raw` is the path to the bytes).\n  \
          \x20 pairing          the tool_use↔tool_result join state (paired | pending | orphan,\n  \
          \x20                  null off the tool axis)\n  \
          \x20 from, to         the comm direction, when the hit is `agent.communication.*`\n  \
          \x20 denial_kind      the harness's own reason a rejected call never ran (null off a\n  \
          \x20                  rejection's tool_result)\n  \
          \x20 image_ids        the extractable `#N` / `L<line>i<n>` ids the record carries (an\n  \
          \x20                  empty array when none)\n  \
          \x20 queue_operation, queue_reason\n  \
          \x20                  the `user.queued` facts (null elsewhere)\n  \
          \x20 resume_paired    the `harness.resume.placeholder` fact (null elsewhere)\n  \
          \x20 for              the record's PROVENANCE edges, one array however many relations a\n  \
          \x20                  unit carries, each `{kind, basis, target, reason}`. It is present\n  \
          \x20                  only on a unit that carries one.\n  \
          \x20 for_root         the message the unit corresponds to, `{line, uuid, hops, note}` on\n  \
          \x20                  EVERY hit. It names the record that opened the unit's turn and the\n  \
          \x20                  parent hops up to it. Its `note` takes three values.\n  \
          \x20                    `note: \"turn opener\"`  the unit IS that record\n  \
          \x20                    `note: \"not reached by the parent chain\"`  the turn grouping\n  \
          \x20                      places it, but the parent links do not carry it there\n  \
          \x20                    `note: \"no opener above this unit\"`  there is no opener above\n  \
          \x20                      it at all (the synthetic lead a transcript opens with, the\n  \
          \x20                      head of an abandoned branch)\n  \
          \x20 refetch          the ready-to-run `csift show` command addressed at the RIGHT id.\n  \
          \x20                  Run it verbatim. It stays the preferred single-record path.\n  \
          \x20 refetch_uuid     its uuid-addressed twin. A line number is a durable address only\n  \
          \x20                  while the transcript is append-only. Claude Code rewrites a live\n  \
          \x20                  transcript in place on a stream tombstone, a local compaction\n  \
          \x20                  rewrite and a remote-ingress resume, which shifts the lines above\n  \
          \x20                  the cut, so a kept `--line` pointer resolves silently to another\n  \
          \x20                  record. A pointer held across a live session refetches by uuid.\n\n  \
          The id trio rides EVERY hit object too, so bare `.hits[]` flattening keeps real ids.\n  \
          With `--siblings` the envelope also carries a `siblings:[…]` array, in the same\n  \
          per-hit shape, for the turn's non-matched records. With `--count-by <axis>` the rows\n  \
          are `census` objects instead.\n\n  \
          Envelopes stream in a COMBINED STABLE CHRONOLOGICAL order. Subagent exchanges\n  \
          interleave with top-level ones by `ts_utc`, the turn-opening timestamp, and\n  \
          timestamp-less exchanges sort last. A hit's own `ts_utc` may be later than its\n  \
          envelope's for a deep tool_use match. `session_id` is the transcript's own id. It is a\n  \
          re-feedable top-level uuid, OR a bare SUBAGENT hex when `is_subagent` is true. That\n  \
          hex is NOT a re-feedable `@<uuid>` target, so re-feed `parent_session_id`, which is\n  \
          always the owning top-level uuid. `record_uuids` lists every record stitched into the\n  \
          round-trip, as completeness evidence.\n\n  \
          A trailing footer object closes the stream.\n  \
          \x20 {matched, sessions, transcript_ids, dropped_by_cap, skipped_lines,\n  \
          \x20  with_elicitation_sidecar, excerpts_truncated, superseded_drafts,\n  \
          \x20  abandoned_records, rewound_turns, replay_copies, boundary_cut_line, leaf_source}\n  \
          It ALSO carries the crossing set whenever `--cross-compactions` is non-zero.\n  \
          \x20 {crossing_budget, boundaries_crossed, budget_exhausted, boundaries_above_floor,\n  \
          \x20  floor_line, floor_cause}\n  \
          These say how many compaction boundaries the survival walk stepped over, how many it\n  \
          did not, and where the chain floor sits and why. Under `--cross-compactions 0` csift\n  \
          crossed nothing and the keys are absent. The `--count-by` census summary carries\n  \
          the same six, and its two text lines ride stderr beside the census's own accounting\n  \
          note, which keeps the table parseable.\n\n  \
          A ZERO-match run adds six more.\n  \
          \x20 {definitive_absence, active_filters, excluded_by_label, outside_turn_numbering,\n  \
          \x20  gated_leaves_unreached, attachment_leaves_unread}\n  \
          `outside_turn_numbering` counts the records a `--turn` window left out because they sit\n  \
          outside turn numbering, and it is null without `--turn`. `gated_leaves_unreached` is\n  \
          true when no selector reached a gated leaf. `attachment_leaves_unread` lists the\n  \
          attachment leaves the scan did not read. That is every one on a run with neither\n  \
          `--attachments` nor a `-t` reaching one, and empty otherwise, and the stderr note names\n  \
          them beside the flag.\
";
