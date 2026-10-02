//! WhoamiArgs: the `whoami` subcommand's flags and help page.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "Identify the calling Claude Code session (via CLAUDE_CODE_SESSION_ID, falling \
             back to CODEX_COMPANION_SESSION_ID)",
    long_about = "Identify the calling Claude Code session, with no false positives.\n\n\
Claude Code exports `CLAUDE_CODE_SESSION_ID` into every Bash-tool environment. Its value is\n\
exactly the calling session's own jsonl basename. csift trusts that variable and nothing\n\
else. It is per session, independent of the Claude Code version, survives nested shells and\n\
gives no false positives. When it is absent, csift reads `CODEX_COMPANION_SESSION_ID`, the\n\
alias the Codex companion plugin sets, before giving up. Both names are matched exactly. A\n\
loose /session/i regex would match macOS's SECURITYSESSIONID by mistake.\n\n\
When neither variable is set (an old Claude Code build, or a shell outside Claude Code and\n\
Codex), whoami does not guess. It fails with guidance to pass an explicit `@<uuid>` target.\n\
csift never falls back to the most recently modified transcript or to a process-tree walk.\n\
Many sessions can be live at once, so the newest file is almost always the wrong one, and\n\
an answer of \"ambiguous, pass `@<uuid>`\" is the expected result in that case.\n\n\
SUBAGENT CAVEAT: current Claude Code hands $CLAUDE_CODE_SESSION_ID the TOP-LEVEL session's\n\
id in every lane. An Agent-tool subagent's own id is withheld from its environment, and an\n\
orchestrated workflow subagent (an OMC Workflow `agent()`, for example) holds the parent's\n\
id too. So on current builds, plain `whoami` run from a subagent resolves the root session.\n\
Older builds handed a built-in Task subagent its own id. To tell which one you were given,\n\
feed the id to `csift agents --agent <id> --format json`. A returned node carries the root\n\
in `parent_session_id`, so one call settles it. The error `no subagent matched` means the id is already\n\
top-level, so use it directly. Scanning `csift agents .` or `csift list .` on the project\n\
path also finds the parent uuid. The env form's identity row carries `is_subagent`,\n\
`parent_session_id` and `depth` as null. The `@trap:<marker>` form emits one identity row\n\
per node of the upstream chain instead, self first and the top-level root last, so a\n\
subagent reads `is_subagent` and `parent_session_id` with no `agents` round trip.",
    after_help = "SESSION-ID SOURCE\n  \
csift reads CLAUDE_CODE_SESSION_ID, which Claude Code sets in every Bash-tool process. Its\n  \
value is the calling session's jsonl basename. When it is absent, csift falls back to\n  \
CODEX_COMPANION_SESSION_ID, the Codex companion plugin's alias. When neither is set,\n  \
whoami fails with guidance to pass an `@<uuid>` target, and it never guesses by mtime.\n  \
An env resolution prints the stderr note `resolved via env ($<VAR>)`, which names the\n  \
variable that answered, so it reads $CODEX_COMPANION_SESSION_ID when the alias carried\n  \
the id. `@main` and the lane note of `msg` and `ack` name the variable the same way.\n\n\
WHICH SUBAGENT AM I\n  \
For a definitive answer that does not depend on the environment, run\n  \
`whoami @trap:<marker>`. You embed a one-shot literal marker in that very command, and\n  \
csift maps it to your subagent's hex id and walks the upstream chain to the top-level\n  \
root. The JSON is one `identity` row per node of that chain.\n  \
Timing differs by lane. A subagent's transcript is written per content block, so its\n  \
command is on disk at dispatch and a first try resolves. The main conversation's record lands about 1 to\n  \
3.4 s after dispatch, as an async flush of the completed message. A first use from the top\n  \
level races that flush and normally misses. From the main thread use `@main`, which has no\n  \
race, or re-run the same command with the same marker.\n  \
Without @trap, do not assume which id the environment gave you. Feed it to\n  \
`csift agents --agent <id> --format json`. A returned node carries the root in\n  \
`parent_session_id`, and the error `no subagent matched` means the id is already\n  \
top-level. A plain `whoami` identity row carries `is_subagent` and `parent_session_id` as\n  \
null, because the environment alone cannot know them. Resolving them is what `@trap` is for.\n\n\
FLAG NOTE\n  \
The positional target is optional and takes one of four forms. `@main` is the calling\n  \
top-level session and the default. `@trap:<marker>` asks which subagent you are. A lane id,\n  \
`@<agent-id>` or a teammate's routing form `@<Name>@<Team>`, answers the same sections for\n  \
that lane. A session uuid is refused. To inspect another session, use `list` or `agents`.\n  \
The `path` line is always printed. Every other session-operating subcommand takes a general\n  \
positional `[PATH]...` or `@` token. No command takes a target flag.\n\n\
REACH\n  \
After the identity output come three sections about the resolved lane. `self` gives its id\n  \
(both forms for a teammate), its kind and its depth. `parent` names the lane above it,\n  \
whether that lane is alive, and the channel a reply would take. `topology` lists the live\n  \
child lanes as `id kind state`, then counts the other live lanes. A lane target answers the\n  \
three sections for another lane.\n  \
`--to @<target>` predicts reach without sending anything. It reports the channel, the teams\n  \
and harbor gate verdicts, the configured and the armed delivery slots, a `settings` line\n  \
naming the cascade all of that was read from (scopes read, scopes absent, inputs that leave\n  \
no trace), and what a sender would be told. The sender is the lane the positional resolved,\n  \
so `csift whoami @trap:<marker> --to @<target>` gives a subagent the answer a send from its\n  \
own lane gets. The positional's sections print first and the prediction follows. With no\n  \
positional the sender is the session the environment names, which is the top-level session\n  \
in every lane, and a stderr note says the sender was assumed. An agent target adds the\n  \
sentence naming what the prediction is made of (the transcript tail and a pid probe, never\n  \
the harness's own state) and the fallback carrier.\n  \
`--peers` lists every live lane as `id kind state` and nothing more. It prints no\n  \
description, no agent type and no name read as a role, because that is the material one\n  \
lane uses to claim standing over another.\n  \
Nothing on this path writes. A prediction is a read of disk plus the policy table.\n\n\
OUTSIDE CLAUDE CODE\n  \
With no CLAUDE_CODE_SESSION_ID, whoami prints the not-a-lane answer on stdout. It names the\n  \
one channel out (`csift send`) and what a receiver needs installed (`csift deliver --slot k`\n  \
lines, which `csift deliver --recipe` prints). whoami still exits non-zero, because the\n  \
identity question has no answer and guessing one is the documented trap.\n\n\
EXAMPLES\n  \
csift whoami                    # the calling session's uuid and its jsonl path\n  \
csift whoami @<agent-id>        # the same three sections for another lane\n  \
csift whoami --to @<lane>       # would a message reach it? (nothing is sent)\n  \
csift whoami @trap:<marker> --to @<lane>   # the same, sent from your own lane\n  \
csift whoami --peers            # every live lane: id, kind, state\n  \
csift whoami --format json      # a header, one identity row, a summary\n  \
# Which subagent am I? Prints the upstream chain, self -> ... -> top-level root.\n  \
# The marker is yours to invent fresh each time, never a copied literal.\n  \
csift whoami @trap:<invent-a-fresh-3word-4digit-marker>\n  \
# Fallback without @trap: map the env's id to its root through the agents node.\n  \
id=$(csift whoami --format json | jq -r 'select(.kind==\"identity\").session_id')\n  \
csift agents --agent \"$id\" --format json | jq -r 'select(.kind==\"agent\").parent_session_id'\n  \
# `no subagent matched` above means the env id is the root (use @main).\n  \
# Then scope the whole conversation with that uuid:\n  \
csift search \"<pattern>\" @<that-uuid>\n\n\
JSON SCHEMA (per --format json)\n  \
The stream is a header, then {kind:\"identity\", session_id, is_subagent,\n  \
parent_session_id, depth, path} rows, then a summary. A plain run emits one identity row.\n  \
`@trap:<marker>` emits the full upstream chain, from depth 0 (yourself) up to the\n  \
top-level root. Select the rows with `jq 'select(.kind==\"identity\")'`.\n  \
Each mode closes with its own summary, and the three share no key. An ordinary run closes\n  \
{kind:\"summary\", identities, live_child_lanes, other_live_lanes}. `identities` is 1\n  \
where the caller resolved and 0 where detection failed, which is a different fact from an\n  \
empty scope. `--to` closes {kind:\"summary\", queued, refused}, and `queued` is false by\n  \
construction, since a prediction sends nothing. `--peers` closes {kind:\"summary\", peers},\n  \
the row count. A positional with `--to` prints the positional's rows, then the `reach`\n  \
row, then one summary carrying both key sets.\n\n\
SEE ALSO\n  \
csift send @<lane> \"…\"      act on the prediction: queue a message for that lane\n  \
csift msg                   the calling lane's channel ledger, newest first\n  \
csift ack <ID>              record that this lane read a delivered message\n  \
csift deliver --recipe      the hook block a lane needs installed to be reachable\n  \
csift status @<lane>        the full live verdict for one lane, with its evidence\n  \
csift agents @<uuid>        the lanes below a session (the walk-down mirror)"
)]
pub struct WhoamiArgs {
    /// Optional SELF target: `@main`, `@trap:<marker>`, or a lane id (`@<agent-id>`, or a
    /// teammate's routing form `@<Name>@<Team>`). With no target, whoami identifies the
    /// calling session from `$CLAUDE_CODE_SESSION_ID`, and `@main` is the explicit spelling of
    /// that default. `@trap:<marker>` answers "which subagent am I?". A running subagent,
    /// whose own id Claude Code withholds from its environment, embeds a unique, literal,
    /// one-shot marker in this very csift command. csift maps the marker to the subagent's
    /// bare hex id and walks the upstream ancestry chain up to the top-level root, the walk-up
    /// mirror of `agents`' walk-down. The walk never reads the environment, so it is reliable
    /// for a built-in Task subagent and for a workflow subagent, whose environment holds the
    /// parent's id. A lane id answers the same three sections for that lane. A session uuid
    /// is refused. To inspect a different session, use `list` or `agents`.
    #[arg(value_name = "SELF")]
    pub self_target: Option<String>,

    /// Predict reach to one target lane. No message is sent and nothing is written. The
    /// prediction names the channel that would carry a message (from the same policy table
    /// `csift send` runs), the two gate verdicts, the delivery slots configured in the
    /// receiver's settings cascade, the slots that have actually run there, and the failure a
    /// sender would be told. For an agent target the answer also says what it is made of.
    /// csift reads the lane's transcript tail and probes a pid, never the harness's own
    /// memory, so the prediction is an inference, and the line names the carrier that remains
    /// when it is wrong. A `settings` line names the cascade the gates and the slot census
    /// were read from. It lists the scopes that contributed, the ones tried and absent, and the
    /// inputs that leave nothing on disk (a settings file or inline JSON handed to Claude Code
    /// on its command line, a restricted source set, the trust dialog, MDM). TARGET takes any
    /// `@` form csift accepts (`@<uuid>`, a uuid prefix, `@<agent-id>`, `@<Name>@<Team>`,
    /// `@main`, `@trap:<marker>`). The sender is the lane the SELF positional resolved, so a
    /// subagent passes `@trap:<marker>` to get the answer a send from its own lane would get.
    /// With no positional the sender is the session the environment names, which is the
    /// top-level session in every lane, and a stderr note says the sender was assumed.
    #[arg(
        long = "to",
        value_name = "TARGET",
        conflicts_with = "peers",
        value_parser = parse_lane_target
    )]
    pub to: Option<String>,

    /// List every live lane under the projects root as `id kind state`, and nothing else. A
    /// description, an agent type or a name read as a role is the material one lane would use
    /// to claim standing over another, so the census publishes none of it. It answers who is
    /// alive and says nothing about who should be obeyed. A lane is live when its owning
    /// session has a registry row whose process answers a pid probe and, for a child lane,
    /// when its own tail is not settled. A child of a dead session is never live. A top-level
    /// lane's state is the registry's own word (`busy`, `shell`, `idle` or `waiting`). A child
    /// lane's state is `in-flight` or `generating`.
    #[arg(long = "peers", conflicts_with = "self_target")]
    pub peers: bool,

    /// Emit JSON instead of text.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}
