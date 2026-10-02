//! VerbatimArgs: the `verbatim` subcommand's flags and help page.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "Restore verbatim user and assistant turns a compaction summary cut, in a char \
             budget",
    long_about = "Restore the verbatim user and assistant back-and-forth that a Claude Code\n\
COMPACTION SUMMARY clipped.\n\n\
A summary preserves TASK STATE in high fidelity (the 9-section synthesis: intent, files\n\
and code, errors and fixes, plan, next step), and it provably loses turn fidelity. Its \"All\n\
user messages\" section clips the person's prose turns to fewer, `...`-truncated bullets,\n\
and the assistant side collapses to a SINGLE verbatim quote. `verbatim` supplements the\n\
summary and never replaces it. It re-emits the clipped user phrasings and the discarded\n\
assistant end-of-turn replies IN ORIGINAL ORDER. Each line carries its jsonl LINE NUMBER,\n\
so a consumer can `Read` the raw transcript at the cited line.\n\n\
For a session's RECENT turns, read straight from the live transcript with no compaction\n\
involved, use `show --turn N..` (`--turn -3..` is the last 3 turns). `verbatim` exists to\n\
RESTORE the turns a compaction summary already CLIPPED, and its budget, round-trip and\n\
richness heuristics serve that job.\n\n\
SELECTION is recency-first. The most recent turns win the budget, because they are what a\n\
resumed agent most needs. The EMITTED document is sorted ascending, so it reads as a\n\
forward transcript. The backward walk passes through compaction boundaries. A summary\n\
record is a turn MEMBER and never a delimiter, so a 40K-char budget reaches back across\n\
several boundaries by default. `--max-compactions N` caps that walk at N boundaries. It is\n\
the one flag that stops the selection. The global `--cross-compactions` decides only whether\n\
a selected unit above a cut reads `live` or `pre-cut`, as the paragraph on crossing a cut\n\
below says.\n\n\
BUDGET (`--budget`, default 40000) bounds EACH session's reconstruction in CHARS, and it\n\
applies PER session in scope. As a sizing rule of thumb, 4 chars make about one token. The\n\
header's `spanned K of N compaction boundaries in scope` is budget-relative on the K side.\n\
K counts the boundaries the SELECTED window crossed, so a small budget on a\n\
compaction-heavy session can legitimately read `0 of 4` when the backward-from-EOF\n\
selection never reached them. N is the session's true total, the same unwindowed count\n\
`stats` reports as `compactions`. BUDGET FAN-OUT below covers what `--subagents` does to\n\
the total.\n\n\
ROUND TRIPS. `--round-trip-fraction` (default 0.5) is a HARD FLOOR. That fraction of the\n\
budget can be spent ONLY on COMPLETE round-trips (user → [N tool calls] → the assistant's\n\
end-of-turn reply, EOT),\n\
never on user-only or assistant-only fragments. Without it, an assistant-heavy tail\n\
recovers ZERO human turns. Over-cap units are MIDDLE-truncated, head and tail kept, with an\n\
explicit `… [+K chars, L lines elided] …` marker. The assistant head is larger than the\n\
user head, because assistant prose front-loads context and back-loads the decision.\n\
Nothing is ever fabricated or silently dropped.\n\n\
PER-ROLE CAPS. The 600-char user and 900-char assistant body caps belong to the DIRECT\n\
path, which is a plain run (with or without `--out`) and the legacy budget-driven\n\
`--slice i` on its own. `--slices N` splits the output across a fixed set of hooks, one\n\
slice each (the `--slices` entry below), and DROPS them for a WINDOW cap. A body renders\n\
whole up to one window less a small headroom, so a turn is ellipsized only when it ALONE\n\
exceeds a window. The oldest turns that do not fit the N\n\
slices are discarded, so no turn is shortened to make room. The same session read two\n\
ways therefore differs on purpose. The direct path keeps MORE turns, each cut to 600/900,\n\
and the `--slices` path keeps FEWER turns whole.\n\n\
THE `L lines elided` FIGURE counts only the newlines the CUT removed. The rendered body is\n\
whitespace-collapsed into one line, so a unit carries the offsets at which its original\n\
newlines landed in that line. The note counts the ones strictly inside the removed span.\n\
The head keeps everything before the cut, and the tail keeps everything after it. An UNCUT\n\
body prints no note, and neither does a cut that falls between two newlines. A WIDER cut\n\
of the same message prints a larger figure. A body csift COMPOSED instead of reading it (an\n\
AskUserQuestion question-and-answer scaffold, an automation attribution label, a\n\
peer-message preview) prints no line note at all, because the record's own newlines sit\n\
nowhere inside it.\n\n\
AGENT MESSAGES (`--agent-msgs`, default `longest`). A single turn can own a LONG run of\n\
agent messages, such as a debugging or build chain the model narrates, which the summary\n\
clips to the single assistant quote it keeps. `longest` keeps the longest message of the\n\
run, plus the first when it is substantive, plus each rich middle. `eot-only` keeps just\n\
the last, which is the wrap-up when the substance sits earlier. `rich` ALSO restores the\n\
first and middle messages that carry important info (a count, a commit hash, a\n\
`file.rs:NNN` ref, backtick code, a finding or decision lexeme) or that are clearly long.\n\
It collapses pure \"let me look into this\" declarations into a placeholder,\n\
`△ L… [X agent messages collapsed, N chars, Y tool calls, Z failed]`, only on runs longer\n\
than the mode's threshold (default 6, `--profile heavy` 4, `light` 8). `all` keeps every\n\
agent message. `--profile heavy|light` is the only way to change those thresholds.\n\n\
A SAME-PREFIX RE-SEND happens when one turn carries two assistant messages and the LATER\n\
one's body CONTAINS the earlier one's whole body, because the model re-sent its message with\n\
an addendum. The earlier one then renders as\n\
`△ L<n>  [superseded by the same-prefix re-send at L<m>, N chars]` in place of a second copy\n\
of the prose, and the survivor prints in full. CONTAINMENT is the test. Two messages\n\
routinely share a long boilerplate head and then say different things, and folding one of\n\
those would drop prose the survivor never carried, so a shared opening alone folds nothing.\n\
A body under 80 normalized chars is never folded this way. `--format json` still emits the\n\
superseded message's own object with its FULL text, plus `superseded_by_line` naming the\n\
survivor, so a machine reader loses nothing.\n\n\
WHAT A FOLD DISCLOSES. The placeholder's `N chars` is the summed length of the bodies it\n\
stands for, so the line says how much prose was folded as well as how many messages. Each\n\
folded message of 210 chars or more is then shown under the marker as an indented\n\
`L<n>  <its first 60 chars>… (+K chars)` line. That is enough to tell a folded finding from\n\
a folded declaration without fetching the range. A message at or above the rich-min\n\
length (280 chars by default, the `--profile` entry gives the others) is KEPT instead of\n\
folded, and a pure declaration is under the declaration-max length (200 chars by default).\n\
So at the defaults a preview appears only for a folded message of 210 to 279 chars. The\n\
fetchable range is still the marker's own `L{first}–L{last}`, and JSON carries `refetch`.\n\n\
THE SURVIVAL AXIS. Only LIVE turns are replayed, meaning the turns Claude Code's own\n\
conversation chain still reaches (the global `--cross-compactions` entry says what the chain\n\
is). A turn the operator rewound past, or a prompt recalled\n\
and re-typed, is COUNTED in a per-session note,\n\
`abandoned  N turn(s) off the surviving conversation, not replayed: L…`, with the exact\n\
`csift show @<id> --line <n>` that fetches one. JSON carries `abandoned_turns` in the\n\
trailing summary. The reason is the compaction summariser's input, the IN-MEMORY message\n\
array, which by then no longer held that turn. Replaying it would put text into a \"what the\n\
compaction clipped\" reconstruction that no model ever read. Turn numbers are the live\n\
ones, the same numbers `search` and `show` print and the ones `--turn` and `--slice` index.\n\n\
ACROSS A COMPACTION CUT, csift keeps going where Claude Code stops. When Claude Code\n\
rebuilds a conversation, its walk stops at a `compact_boundary`, whose parentUuid is null,\n\
so on a resume the history above the cut is gone from the conversation. csift's survival\n\
walk CROSSES a boundary through the record its `logicalParentUuid` names, up to\n\
`--cross-compactions` boundaries (default 3, `all` for no limit, `0` for Claude Code's own\n\
stop). The turns it reaches that way read `live`. The walk stops when the budget runs out,\n\
or when the boundary names a record the file no longer carries or one the walk already\n\
reached. Above that floor csift still reads the transcript in FILE ORDER and replays the\n\
turns, because \"what the model saw at the time\" is the question this command answers.\n\
Such units are flagged and never hidden. JSON `survival` reads `pre-cut` for them and\n\
`live` otherwise, and the closing lines say how many boundaries were crossed and where the\n\
floor sits.\n\n\
DEDUP. A turn the NEWEST summary already quotes verbatim is flagged `(also in summary)` and\n\
DEMOTED, which means it is selected only after the non-duplicate turns. It is never\n\
silently dropped, because a false positive must not lose a real turn.\n\n\
WINDOWING. `--turn (N|A..B|N..|-k)` (inclusive and 0-based, counted by the turn rule the\n\
`--turn` entry below states) intersects (AND) with `--since`/`--until` (ISO8601, or a\n\
relative `N<unit>` whose seven units the `--since` entry lists). `verbatim` TEXT prints\n\
`L<line>` per unit and no turn marker. To pick a value for `--turn`, read the index from\n\
`csift search` text (the `<tok>·t<N>` header) or from this command's own `--format json`\n\
(`turn_index`).\n\n\
OUTPUT. `--out <PATH>` captures the SAME rendered reconstruction that prints to stdout into\n\
a file, byte-identical. verbatim does NOT line-truncate stdout, so `--out` differs only in\n\
going to a file. Over-cap units are middle-truncated with an explicit\n\
`… [+K chars, L lines elided] …` marker in BOTH. For UN-truncated unit bodies use\n\
`--format json`, which emits one VERBATIM object per unit (full `text`, no per-unit cap)\n\
plus interleaved compaction-boundary records.",
    after_help = "SEE ALSO\n  \
verbatim cites image handles inline (`[1 image: #265]`). To extract them to files you can\n  \
then Read, run csift image <target> --id <ID> --out DIR, passing a `#N` handle as the bare\n  \
number.\n\n\
EXAMPLES\n  \
csift verbatim .                                  # default 40K-char rebuild, top-level thread\n  \
csift verbatim @<uuid> --budget 12000             # one thread, at most 12000 chars of turns\n  \
csift verbatim @<uuid> --round-trip-fraction 0.6  # weight harder toward complete round-trips\n  \
csift verbatim @<uuid> --agent-msgs eot-only      # last-message-only per turn\n  \
csift verbatim @<uuid> --profile heavy            # lower thresholds (max fidelity)\n  \
csift verbatim @<uuid> --agent-msgs all           # every agent message, no filtering\n  \
# ALSO span subagents (budget × N), as line-numbered JSON:\n  \
csift verbatim @<uuid> --subagents --format json\n  \
# Full reconstruction to a file:\n  \
csift verbatim . --budget 40000 --out /tmp/verbatim.md\n  \
# The 1st chunk of at most 9000 chars, for a SessionStart hook (fan slices 1..4):\n  \
csift verbatim . --budget 36000 --window 9000 --slice 1\n  \
# Stay within one compaction boundary:\n  \
csift verbatim @<uuid> --budget 8000 --max-compactions 1\n\n\
AUTOMATION TRIGGERS\n  \
A machine `<task-notification>` is a background-command, workflow, spawned-agent or\n  \
monitor-tick COMPLETION pulse that Claude Code injects as a `type:\"user\"` record. It OPENS\n  \
a turn like a human message, so it appears in the reconstruction. It renders as a PARSED\n  \
attribution label, `[<kind> <task-id> <status>] <summary>`, where kind is one of\n  \
background-command | workflow | agent | monitor | task, read from the summary. The raw\n  \
`<task-id>` / `<output-file>` XML never prints. The header reports the human and\n  \
automation split, for example\n  \
`selected 16 user (3 automation triggers) + 58 assistant units`. These pulses are\n  \
EXCLUDED from the `--round-trip-fraction` HARD FLOOR, which is reserved for human\n  \
exchanges, and they can still fill the budget left after that floor.\n  \
LIMITATION. Only `<task-notification>` COMPLETION pulses are segmented and attributed. The\n  \
isMeta ScheduleWakeup WAKEUP-TICK prompts (a monitor or cron tick FIRING, e.g.\n  \
`MONITOR TICK`) bypass this. They are isMeta records that do NOT open a turn, so the agent\n  \
run a tick triggers groups under the PRECEDING turn, with no per-tick segment of its own.\n  \
In an automation-heavy monitor session this lumps the\n  \
dominant tick-driven work onto one human turn.\n\n\
BUDGET FAN-OUT\n  \
`--budget` applies PER session in scope. `verbatim` defaults to the TOP-LEVEL thread only,\n  \
so an `@<uuid>` run is one session at `budget` chars. `--subagents` also spans that\n  \
session's subagents, and the realized output is then budget × (sessions in scope). A\n  \
target that spans S subagents realizes up to `budget × (1 + S)` chars in total. A\n  \
top-of-output `scope` line names the TRUE scope (all top-level and subagent sessions\n  \
discovered), how many rendered within budget, and the realized multiplier. A targeted\n  \
top-level session that does not fit `budget` is reported with an explicit\n  \
`skipped; needs ≥ N chars` note and is never silently dropped.\n\n\
JSON SCHEMA (per --format json)\n  \
The stream opens with a header object, the full budget accounting the text header shows:\n  \
`{kind:\"header\", command, sessions_in_scope, sessions_rendered,\n  \
top_level_sessions, subagent_sessions, budget_chars, budget_is_per_session,\n  \
max_total_chars, round_trip_fraction, chars_used, boundaries_spanned,\n  \
boundaries_total, selected_user, selected_assistant, automation_triggers,\n  \
automation_by_kind, automation_in_scope_by_kind, with_elicitation_sidecar}`\n  \
plus the four duplicate-copy keys the headers of the commands that span sessions carry\n  \
(`duplicate_transcripts`, `duplicate_records_dropped`, `divergent_transcripts`,\n  \
`unreadable_transcripts`).\n  \
`sessions_in_scope` is the TRUE scope (every discovered session), and `sessions_rendered`\n  \
is how many fit the budget. The top_level/subagent split is over ALL in scope.\n  \
`budget_chars` and `max_total_chars` are ALWAYS in CHARS. `boundaries_spanned` is\n  \
relative to the budget window, and `boundaries_total` is the scope's true total.\n  \
`automation_by_kind` breaks the SELECTED `automation_triggers` total down per class\n  \
({background-command,agent,workflow,monitor,task}). `automation_in_scope_by_kind` is the\n  \
SAME breakdown over EVERY in-scope automation pulse, whatever the budget selected, so a\n  \
monitor-heavy session never reads as `monitor:0` just because the recency window selected\n  \
none of its deep pulses. Compare the two to see how much automation exists against how\n  \
much was rendered.\n  \
Then comes one object PER emitted unit:\n  \
{kind:\"turn\", session_id, is_subagent, parent_session_id, turn_index, line,\n  \
source, role, ts_utc, ts_local, tool_calls, full_chars, rendered_chars, truncated,\n  \
elided_chars, elided_lines, also_in_summary, compactions_before, survival,\n  \
superseded_by_line, text, is_automation}\n  \
`is_subagent` flags a bare-hex subagent unit. Re-feed `parent_session_id`, never the bare\n  \
`session_id`. `line` is the unit's jsonl line, the key every command's rows use for it.\n  \
It is null exactly when `source` reads `elicitation-sidecar`, which marks a pending unit\n  \
merged from the session's elicitation file (`elicitations.jsonl` in its sidecar directory,\n  \
the `<session-id>/` folder beside the transcript), which has no physical line. `source` is\n  \
null on every other unit. Such a unit's `role` is `assistant` and its text header reads\n  \
`ASSISTANT`, because the pending line stands for the assistant's AskUserQuestion /\n  \
ExitPlanMode call. It is a question still waiting for the person's answer and never words\n  \
the person said. `survival` is `live`, or `pre-cut` for a unit ABOVE a compaction cut\n  \
where Claude Code's own rebuild of the conversation stops. It is never `abandoned`, since\n  \
those turns are counted and not replayed.\n  \
An automation USER unit also carries {trigger_kind, task_id, status, event}, where\n  \
`event` is the Monitor/ScheduleWakeup outcome tag and null on non-monitor pulses. An\n  \
inbound peer-message USER unit carries {is_inbound_comm, comm_label, comm_from, comm_to}:\n  \
the comm leaf, the sender, and `self`.\n  \
Boundary objects are tagged {kind:\"compaction_boundary\", line, summary_chars, mode} and\n  \
{kind:\"collapsed_agents\", session_id, is_subagent, parent_session_id, turn_index,\n  \
agent_messages, tool_calls, failed, first_line, last_line, collapsed_chars,\n  \
collapsed_previews[{line, excerpt}], compactions_before, refetch}.\n  \
A summarize-mode boundary also names its gesture in the text banner\n  \
(`… summary at L<n> · summarize <direction> · …`). A `/rewind` summarize IS a compaction,\n  \
so the turns it clipped are reconstructed exactly like any other's.\n  \
A trailing {kind:\"summary\", skipped_lines, abandoned_turns} ALWAYS closes the stream,\n  \
even when 0. `abandoned_turns` counts the turn openers off the surviving conversation\n  \
across every session in scope, the machine twin of the per-session note. The summary also\n  \
carries the crossing set {crossing_budget, boundaries_crossed, budget_exhausted,\n  \
boundaries_above_floor, floor_line, floor_cause} whenever `--cross-compactions` is\n  \
non-zero. It says what the survival walk stepped over to decide each unit's `survival`,\n  \
and where it stopped. At `--cross-compactions 0` the keys are absent.\n  \
The stream is therefore `{kind:\"header\",…}` → unit and boundary rows →\n  \
`{kind:\"summary\",…}`, so `tail -1 | jq 'select(.kind==\"summary\")'` reads its summary."
)]
pub struct VerbatimArgs {
    /// Project target(s) (an actual cwd or an encoded dir) whose sessions to reconstruct turns
    /// from, or an `@<uuid>` session token. Repeatable. `@<uuid>` (8-4-4-4-12 hex) scopes to
    /// one session, so `csift verbatim @<uuid>` works as the EXAMPLES show. `@<agent-id>`
    /// reconstructs that ONE subagent's own thread (the ids come from `csift agents`). `@main`
    /// and `@trap:<marker>` resolve the calling session, and a `*.jsonl` file scopes to that
    /// transcript. A target is REQUIRED, here or through `--sessions-from`. A bare
    /// `csift verbatim` fails, because it would realize `--budget` × every session of every
    /// project. `verbatim` defaults to the TOP-LEVEL thread only, the single-thread recovery
    /// use case, so `csift verbatim @<uuid>` reconstructs just that conversation. Add
    /// `--subagents` for the rare cross-fan-out reconstruction, and `--budget` then applies PER
    /// session in scope.
    #[arg(
        value_name = "PATH",
        allow_hyphen_values = true,
        value_parser = parse_project_target,
        verbatim_doc_comment
    )]
    pub paths: Vec<PathBuf>,

    /// Also scope to the session ids in FILE (`-` reads stdin). The ids are whitespace- or
    /// newline-separated uuid, uuid-prefix or agent-id tokens, bare or `@`-prefixed, exactly as
    /// csift emits them (`search -l`, JSON `transcript_ids` / `parent_session_id`). They are
    /// unioned with the positional targets, and each id resolves fail-loud like an `@`
    /// positional. An EMPTY list (an upstream stage that found nothing) scopes to NOTHING. The
    /// result is an honest empty one with exit 0, never a silent widening to every project. The
    /// resolved ids then follow this command's normal span rules, exactly as `@` positionals
    /// would. On the span-by-default commands each session EXPANDS to its subagent transcripts,
    /// and `--no-subagents` pins the scope to exactly the listed ids.
    #[arg(long = "sessions-from", value_name = "FILE|-", verbatim_doc_comment)]
    pub sessions_from: Option<std::path::PathBuf>,

    /// Character budget applied PER session in scope, always in chars (as a sizing rule, about
    /// 4 chars make one token). It bounds each session's reconstruction. `verbatim` defaults to
    /// the top-level thread only, so an `@<uuid>` run realizes just `budget` chars. With
    /// `--subagents` the realized total is `budget × (sessions in scope)`, and a scope banner
    /// surfaces the multiplier. Default 40000.
    #[arg(long, value_name = "N", default_value_t = 40000, verbatim_doc_comment)]
    pub budget: usize,

    /// Fraction of the budget RESERVED for complete round-trips (user → [N tool calls] →
    /// assistant EOT), which user messages alone cannot spend. A hard floor. Default 0.5. It
    /// must lie in the open interval (0.0, 1.0).
    #[arg(
        long = "round-trip-fraction",
        value_name = "F",
        default_value_t = 0.5,
        verbatim_doc_comment
    )]
    pub round_trip_fraction: f64,

    /// Also span each in-scope session's SUBAGENT transcripts under `subagents/**`: built-in
    /// Task/Agent-tool subagents, and workflow agents, whose transcripts sit under
    /// `subagents/workflows/<run>/`. OFF by default for `verbatim`, unlike files
    /// and search. `verbatim` is a SINGLE-THREAD recovery tool and `--budget` MULTIPLIES per
    /// session in scope, so spanning hundreds of unrelated fan-out subagents by default would
    /// bury the thread you asked to restore under megabytes of noise. Opt in with `--subagents`
    /// only for the rare cross-fan-out reconstruction.
    #[arg(
        long = "subagents",
        conflicts_with = "no_subagents",
        default_value_t = false,
        verbatim_doc_comment
    )]
    pub include_subagents: bool,

    /// Exclude subagent transcripts and reconstruct only from the top-level session. This is
    /// already the `verbatim` DEFAULT. The flag is kept so every span command answers the same
    /// two switches, and passing it beside `--subagents` is a usage error, as on every span
    /// command.
    #[arg(long = "no-subagents", verbatim_doc_comment)]
    pub no_subagents: bool,

    /// Stop the selection's backward walk after crossing N compaction boundaries (0 = unlimited,
    /// the default). A guard, never a target. It decides which turns are selected. The global
    /// `--cross-compactions` decides only whether a selected unit reads `live` or `pre-cut`.
    #[arg(
        long = "max-compactions",
        value_name = "N",
        default_value_t = 0,
        verbatim_doc_comment
    )]
    pub max_compactions: usize,

    /// Which agent messages of a turn are kept when the turn holds several. `longest` (DEFAULT)
    /// keeps the LONGEST agent message. That is often a MIDDLE message, while the last is often a
    /// wrap-up of about 50 chars. It also keeps the first message when it reaches the rich-min
    /// length (the `--profile` entry gives the values), and each RICH middle message, meaning one
    /// carrying a number, commit hash, file:line reference, backtick code or finding or decision
    /// word, or one at least the rich-min length. The rest collapse into a placeholder.
    /// `eot-only` keeps only each turn's LAST agent message, which takes the wrap-up whenever the
    /// substance of the run sits earlier. `rich` keeps the last message, the first message and
    /// every middle message except a pure declaration, which is a message under the
    /// declaration-max length that opens with an intent verb ("let me look into this") and
    /// carries none of those signals. `rich` filters only a run longer than the run threshold.
    /// `all` keeps every agent message.
    #[arg(long = "agent-msgs", value_enum, default_value_t = crate::commands::turns::AgentMsgMode::Longest, verbatim_doc_comment)]
    pub agent_msgs: crate::commands::turns::AgentMsgMode,

    /// Threshold bundle for `--agent-msgs`. Three thresholds: the run threshold (how many agent
    /// messages a turn needs before `rich` filters it, default 6), rich-min (the length in chars
    /// at which a message counts as rich on length alone, default 280) and declaration-max (the
    /// length under which a signal-free message can be a pure declaration, default 200). `heavy`
    /// is maximal fidelity (threshold 4, rich-min 200, declaration-max 140). `light` is lean
    /// (threshold 8, rich-min 360, declaration-max 240). A profile never changes the master
    /// `--agent-msgs` mode. With no `--agent-msgs` the mode stays the default `longest`, run with
    /// the profile's thresholds, and `--agent-msgs rich` also switches the keep-set.
    #[arg(long = "profile", value_enum, verbatim_doc_comment)]
    pub profile: Option<crate::commands::turns::Profile>,

    /// Inclusive turn-index range in the shared grammar: `N` (one turn) · `A..B` · `N..` (to
    /// the end) · `..N` · `-k` from the end (`-3..` = the last 3). It is 0-BASED. Turn 0 is the
    /// first opener's turn together with every record above it (the session's opening context),
    /// so `1..N` drops the first human turn as well as that context. A turn opens on a genuine
    /// typed message, an answered AskUserQuestion, a tool rejection carrying typed text, an
    /// inbound peer message, or a `<task-notification>` pulse. A slash command with arguments
    /// carries `user.message` and still opens no turn. Find turn indices in the `<tok>·t<N>`
    /// exchange header of `csift search` text output, or in the `turn_index` field of any
    /// `--format json` record. Intersects (AND) with `--since` / `--until`.
    #[arg(
        long = "turn",
        value_name = "N|A..B|N..|-k",
        allow_hyphen_values = true,
        verbatim_doc_comment
    )]
    pub turn_range: Option<String>,

    /// Lower time bound, in the WHEN grammar (system-local tz). A relative `N<unit>` is N units
    /// AGO, with seven units: `s` seconds, `m` minutes, `h` hours, `d` days, `w` weeks, `mo`
    /// months (30 days) and `y` years (365 days), as in `45s`, `90m`, `2h`, `3d`, `1w`, `2mo`,
    /// `1y`. A leading `-` is tolerated. An ISO8601 instant (`2026-06-01T05:00:00Z` / `…+10:00`)
    /// is that instant. A BARE datetime (`2026-06-01T05:00:00`) is that LOCAL wall-clock time,
    /// and a bare date (`2026-06-01`) is LOCAL MIDNIGHT that day. Intersects (AND) with --turn,
    /// so both filters apply.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub since: Option<String>,

    /// Upper time bound (same WHEN grammar as --since). Intersects (AND) with --turn, so both
    /// filters apply.
    #[arg(long, value_name = "WHEN", verbatim_doc_comment)]
    pub until: Option<String>,

    /// Capture the SAME rendered reconstruction that prints to stdout into this file,
    /// byte-identical. verbatim does NOT truncate stdout, and over-cap units are
    /// middle-truncated with a `… [+K chars, L lines elided] …` marker in BOTH. The summary
    /// still prints to stdout. For UN-truncated unit bodies use `--format json`, which carries
    /// the full verbatim `text`.
    #[arg(long, value_name = "PATH", verbatim_doc_comment)]
    pub out: Option<PathBuf>,

    /// Emit JSON instead of the headered text format.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text, verbatim_doc_comment)]
    pub format: OutputFormat,

    /// CHUNKED OUTPUT. Paginate the recovered DOCUMENT (the verbatim turns, the same content
    /// `--out` writes) into chunks of at most `--window` CHARACTERS, and print ONLY the Nth chunk
    /// (1-based) to stdout, with NO operational chrome (scope banner, SESSION header, footer). It
    /// is built for fanning a >10K reconstruction across several SessionStart hooks. Claude Code
    /// caps EACH hook's `additionalContext` at 10,000 UTF-16 code units, and an over-cap string
    /// is replaced by a file path and a short preview, so the body is effectively LOST to the
    /// model. One hook per slice keeps every injected chunk under that cap. Slicing is
    /// DETERMINISTIC (same session and budget, identical chunk boundaries), so N independent
    /// hooks can each request their own slice. csift takes no lock and does not order the hooks,
    /// so a hook script that needs its chunks in order arranges that itself. An out-of-range N
    /// prints nothing (exit 0), so surplus hooks inject nothing. Text format only, and not
    /// combinable with `--out`.
    #[arg(long, value_name = "N", verbatim_doc_comment)]
    pub slice: Option<usize>,

    /// Chunk size for `--slice`, in CHARACTERS counted as Unicode scalars, so a CJK-heavy
    /// document is NOT over-counted 3× the way a byte budget would count it. Claude Code's
    /// 10,000 `additionalContext` cap counts UTF-16 code units. That is the same number for BMP
    /// text, CJK included, while an emoji or a CJK ext-B character counts 1 toward the window
    /// and 2 toward the cap. The default 10000 equals the cap. Pass a little under (e.g.
    /// `--window 9000`) to leave headroom for any wrapper text the hook adds around the chunk,
    /// and more for a document carrying astral characters. Lines are packed greedily up to the
    /// window at LINE boundaries. A single line longer than the window is hard-split on a char
    /// boundary, so no chunk ever exceeds it. Ignored without `--slice`.
    #[arg(long, value_name = "N", default_value_t = 10000, verbatim_doc_comment)]
    pub window: usize,

    /// FIXED-FLEET mode. Pin the reconstruction to AT MOST N slices of `--window` chars each,
    /// instead of letting `--budget` decide a VARIABLE number of chunks. A hook fleet is a
    /// fixed set of registered `SessionStart` hooks, so the slice COUNT is the hard constraint,
    /// and it must NOT drift to 5/6/7 as the turns grow. With `--slices N`, csift fills the N
    /// newest-first slices with WHOLE turns. The per-role 600/900 caps are dropped, and a turn
    /// is ellipsized ONLY if it alone exceeds one window. csift DISCARDS the oldest turns that
    /// don't fit, so the emitted count is ALWAYS ≤N however big the turns are. It requires
    /// `--slice i` to pick which chunk. The budget becomes N×`--window`, so `--budget` is
    /// ignored. Without `--slices`, `--slice` keeps its legacy budget-driven,
    /// variable-chunk-count behavior.
    #[arg(long, value_name = "N", verbatim_doc_comment)]
    pub slices: Option<usize>,
}

impl VerbatimArgs {
    /// Resolve the include/exclude flags into a single decision. UNLIKE the other
    /// subcommands, `verbatim` defaults to TOP-LEVEL-ONLY (`include_subagents` defaults false):
    /// spanning is opt-in via `--subagents`, `--no-subagents` restates the default, and clap
    /// refuses the two together. So a `csift verbatim @<uuid>` reconstructs just that one thread.
    #[must_use]
    pub fn want_subagents(&self) -> bool {
        self.include_subagents && !self.no_subagents
    }

    /// Resolve the agent-message policy into a [`crate::commands::turns::RichnessCfg`]. The
    /// per-knob tuning flags are GONE (0-backcompat surface diet): `--profile` bundles are
    /// the only tuning: `heavy` (4/200/140) / `light` (8/360/240) / none (6/280/200).
    #[must_use]
    pub fn richness_cfg(&self) -> crate::commands::turns::RichnessCfg {
        use crate::commands::turns::{Profile, RichnessCfg};
        let (run_threshold, rich_min_chars, declaration_max_chars) = match self.profile {
            Some(Profile::Heavy) => (4, 200, 140),
            Some(Profile::Light) => (8, 360, 240),
            None => (6, 280, 200),
        };
        RichnessCfg {
            mode: self.agent_msgs,
            run_threshold,
            rich_min_chars,
            declaration_max_chars,
            keep_first: true,
        }
    }
}
