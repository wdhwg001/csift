//! WhoamiArgs: the `whoami` subcommand's flags and help page.

use super::*;

#[derive(Debug, Args)]
#[command(
    about = "Identify the calling Claude Code session (via CLAUDE_CODE_SESSION_ID, falling \
             back to CODEX_COMPANION_SESSION_ID)",
    long_about = "Identify the CALLING Claude Code session, false-positive-safe.\n\n\
        Claude Code exports `CLAUDE_CODE_SESSION_ID` into every Bash-tool environment, \
        and its value equals the calling session's own jsonl basename exactly. That \
        canonical var is the signal csift trusts: per-session, version-independent, \
        survives bash nesting, zero false positives. When it is absent, csift falls back \
        to `CODEX_COMPANION_SESSION_ID` (the alias the Codex companion plugin sets) before \
        giving up. (The exact var names are matched; never a loose /session/i regex, which \
        would false-positive on macOS's SECURITYSESSIONID.)\n\n\
        When NEITHER var is set (an old CC build, or running outside CC/Codex), whoami \
        does NOT guess; it errors with guidance to pass an explicit `@<uuid>` target. \
        Most-recent-mtime and process-tree walking are FORBIDDEN: many CC sessions \
        may be live at once, so mtime is almost always wrong. It is acceptable for \
        whoami to often say \"ambiguous, pass `@<uuid>`\".\n\n\
        SUBAGENT CAVEAT: current Claude Code hands $CLAUDE_CODE_SESSION_ID the TOP-LEVEL \
        session's id in every lane - an Agent-tool subagent's OWN id is withheld from its \
        env, and an ORCHESTRATED/workflow subagent (e.g. an OMC Workflow `agent()`) holds \
        the PARENT's too - so from a subagent plain `whoami` resolves the ROOT, not you; \
        older builds handed a built-in Task subagent its OWN id. Do NOT assume which; \
        disambiguate via the recovery: feed the resolved id to `csift agents --agent <id> \
        --format json`; if it returns the node, read `parent_session_id` for the ROOT (one \
        call); if it errors `no subagent matched`, the id is ALREADY top-level; use it \
        directly. (Or scan `csift agents .` / `csift list .` on the project PATH and find the \
        parent uuid.) The ENV form's JSON carries `{session_id, path}`; the `@trap:<marker>` form \
        instead returns the UPSTREAM CHAIN `{chain:[{session_id, is_subagent, parent_session_id, \
        depth, path}, …]}` (self first, top-level root last), so a subagent reads is_subagent / \
        parent_session_id directly, no `agents` round-trip.\n\n\
        REACH: after the identity output, whoami prints three sections about the lane it \
        resolved - `self` (its id, both forms for a teammate, its kind and its depth), `parent` \
        (the lane above it, whether that lane is alive, and which channel a reply would take) \
        and `topology` (the live child lanes under it, then how many other live lanes exist). \
        `--to @<target>` asks the reach question about ANOTHER lane, predicting the channel and \
        the verdict of a send from the lane the positional resolved WITHOUT sending anything; \
        with no positional the sender is the session the environment names. `--peers` lists every live lane as \
        `id kind state` and nothing else. Nothing on this path writes: a prediction is a read of \
        disk plus the policy table.\n\n\
        FLAG NOTE: whoami's positional is the optional SELF token `@trap:<marker>` / `@main` / a \
        LANE id (`@<agent-id>`, or a teammate's `@<Name>@<Team>`); a session uuid is refused. \
        The `path` line is ALWAYS printed. Every OTHER session-operating subcommand takes \
        a general POSITIONAL `[PATH]...` / `@`-token; there is no target FLAG.",
    after_help = "SESSION-ID SOURCE\n  \
          The canonical env var CLAUDE_CODE_SESSION_ID (CC sets it per Bash-tool process; \
        its value IS the calling session's jsonl basename). If absent, csift falls back to \
        CODEX_COMPANION_SESSION_ID (the Codex companion plugin's alias). If NEITHER is set, \
        whoami errors with guidance to pass an `@<uuid>` target; it never guesses by mtime. \
        The stderr note an env resolution prints (`resolved via env ($<VAR>)` here, the same \
        on `@main` and on `msg`/`ack`'s lane note) names the variable that ANSWERED, so it reads \
        $CODEX_COMPANION_SESSION_ID when that alias carried the id.\n\n\
        SUBAGENT CAVEAT\n  \
          What the env var names depends on HOW the subagent was spawned and has CHANGED \
        across CC versions: current CC hands an Agent-tool subagent the PARENT session's id \
        (the subagent's OWN id is withheld from its env), and an \
        ORCHESTRATED/workflow subagent (e.g. an OMC Workflow `agent()`) likewise holds the \
        PARENT id; older builds handed a built-in Task subagent its OWN id. So from a \
        subagent, plain `whoami` usually resolves the ROOT, not you. For a DEFINITIVE, \
        env-INDEPENDENT answer use `whoami @trap:<marker>`: you embed a one-shot literal marker in \
        that very command and csift maps it to your subagent hex and walks the UPSTREAM CHAIN up to \
        the top-level root (the @trap form's JSON is `{chain:[{session_id, is_subagent, \
        parent_session_id, depth, path}, …]}`). @trap TIMING: a subagent's transcript flushes per \
        content block (its command is on disk at dispatch, so a first try resolves); the MAIN \
        conversation's record lands ~1-3.4s after dispatch as an async flush of the completed \
        message, so a top-level FIRST use races it and normally misses; from the main thread \
        use `@main` (no race), or re-run the SAME command with the SAME marker. \
        WITHOUT @trap, don't assume which id the env gave; feed it to `csift agents --agent <id> \
        --format json`; a returned node → read `parent_session_id` for the ROOT; `no subagent \
        matched` → the id is ALREADY top-level. Plain `whoami` JSON carries only {session_id, \
        path}; from the env alone it canNOT know is_subagent / parent_session_id (that is exactly \
        what `@trap` resolves).\n\n\
        FLAG NOTE\n  \
          whoami takes an OPTIONAL positional target in one of FOUR forms: `@main` (the \
        calling top-level session, the default), `@trap:<marker>` (which subagent am I), or a \
        LANE id - `@<agent-id>`, or a teammate's routing form `@<Name>@<Team>` - which answers \
        the same sections for that lane. A session uuid is refused (use `list` / `agents` for \
        another session). The `path` line is ALWAYS printed. Every OTHER session-operating \
        subcommand takes a general \
        POSITIONAL [PATH]... / `@`-token. There is no target flag anywhere.\n\n\
        REACH\n  \
          After the identity output come three sections about the resolved lane: `self` (id,\n  \
          both forms for a teammate, kind, depth), `parent` (the lane above, alive or not, and\n  \
          the channel a reply would take) and `topology` (live child lanes as `id kind state`,\n  \
          then how many other live lanes exist). A LANE target answers them for another lane.\n  \
          `--to @<target>` predicts reach WITHOUT sending: the channel, the teams and harbor\n  \
          gate verdicts, the configured and armed delivery slots, a `settings` line naming the\n  \
          cascade all of that was read from (scopes read, scopes absent, inputs that leave no\n  \
          trace), and what a sender would be\n  \
          told. The sender is the lane the positional resolved, so `csift whoami\n  \
          @trap:<marker> --to @<target>` gives a subagent the answer a send from its own lane\n  \
          gets. The positional's sections print first and the prediction follows. With no\n  \
          positional the sender is the session the environment names, which is the top-level\n  \
          session in every lane, and a stderr note says the sender was assumed.\n  \
          An AGENT target adds the sentence naming what the prediction is made of - the\n  \
          transcript tail and a pid probe, not the harness's own state - and the fallback\n  \
          carrier. `--peers` lists every live lane as `id kind state` ONLY: no description, no\n  \
          agent type, no name-as-role, because that is the material one lane uses to claim\n  \
          standing over another. OUTSIDE Claude Code (no CLAUDE_CODE_SESSION_ID) whoami prints\n  \
          the not-a-lane answer on stdout - the one channel out (`csift send`) and what a\n  \
          receiver needs installed (`csift deliver --slot k` lines; print them with `csift\n  \
          deliver --recipe`) - and still exits non-zero, because the identity question has no\n  \
          answer and guessing one is the documented trap.\n\n\
        EXAMPLES\n  \
          csift whoami                  # the calling session's uuid + its jsonl path\n  \
          csift whoami @<agent-id>      # the same three sections for another lane\n  \
          csift whoami --to @<lane>     # would a message reach it? (nothing is sent)\n  \
          csift whoami @trap:<marker> --to @<lane>   # the same, sent from your own lane\n  \
          csift whoami --peers          # every live lane: id, kind, state\n  \
          csift whoami --format json    # {\"session_id\":\"…\",\"path\":\"…\"}\n  \
          csift whoami @trap:<invent-a-fresh-3word-4digit-marker>   # which SUBAGENT am I? -> upstream chain (self -> ... -> top-level root); the marker is YOURS to invent, never a copied literal\n  \
          # FALLBACK (no @trap): map the env's id to its ROOT - the agents node carries parent_session_id:\n  \
          csift agents --agent \"$(csift whoami --format json | jq -r 'select(.kind==\"identity\").session_id')\" --format json | jq -r 'select(.kind==\"agent\").parent_session_id'\n  \
          # …then scope the whole conversation with that uuid (`no subagent matched` above = the env id IS the root: use @main):\n  \
          csift search \"<pattern>\" @<that-uuid>\n\n\
        JSON SCHEMA (per --format json)\n  \
          Envelope: header → {kind:\"identity\", session_id, is_subagent, parent_session_id, \
        depth, path} rows → a summary terminator. One identity row for \
        a plain run; `@trap:<marker>` emits the full upstream chain, depth 0 (yourself) up \
        to the top-level root. Select with `jq 'select(.kind==\"identity\")'`.\n  \
          THREE terminators, one per mode, and they share no key: an ordinary run closes\n  \
        {kind:\"summary\", identities, live_child_lanes, other_live_lanes} - `identities` is 1\n  \
        where the caller resolved and 0 where detection failed, which is a different fact\n  \
        from an empty scope; `--to` closes {kind:\"summary\", queued, refused} with `queued`\n  \
        false by construction, since a prediction sends nothing; `--peers` closes\n  \
        {kind:\"summary\", peers}, the row count. A positional with `--to` prints the\n  \
        positional's rows, then the `reach` row, then ONE summary carrying both key sets.\n\n\
        SEE ALSO\n  \
          csift send @<lane> \"…\"      act on the prediction: queue a message for that lane\n  \
          csift msg                   the calling lane's channel ledger, newest first\n  \
          csift ack <ID>              record that this lane read a delivered message\n  \
          csift deliver --recipe      the hook block a lane needs installed to be reachable\n  \
          csift status @<lane>        the full live verdict for one lane, with its evidence\n  \
          csift agents @<uuid>        the lanes BELOW a session (the walk-DOWN mirror)"
)]
pub struct WhoamiArgs {
    /// Optional SELF target: `@trap:<marker>`, `@main`, or a LANE id (`@<agent-id>` /
    /// `@<Name>@<Team>`). With NO target, identify
    /// the calling session from `$CLAUDE_CODE_SESSION_ID` (the default; `@main` is its
    /// explicit spelling). `@trap:<marker>` answers "which SUBAGENT am I?": a running subagent
    /// (whose own id CC withholds from the env) embeds a unique, literal, one-shot marker in THIS
    /// very csift command and csift maps it to the subagent's bare hex and walks the UPSTREAM
    /// ancestry CHAIN up to the top-level root (the walk-UP mirror of `agents`' walk-DOWN);
    /// env-INDEPENDENT, so it is reliable for a built-in Task AND a workflow subagent (whose env id
    /// is the PARENT). A LANE id (`@<agent-id>`, or a teammate's routing form `@<Name>@<Team>`)
    /// answers the same three sections for THAT lane. A session uuid is still refused: use
    /// `list`/`agents` to inspect a different SESSION.
    #[arg(value_name = "SELF")]
    pub self_target: Option<String>,

    /// REACH PREDICTION for one target lane, with NO message sent and nothing written: which
    /// channel would carry a message (the same policy table `csift send` runs), the two gate
    /// verdicts, the delivery slots configured in the receiver's settings cascade and the ones
    /// that have actually run there, and the failure a sender would be told. For an AGENT target
    /// the answer also names what it is made of: csift reads the lane's transcript tail and
    /// probes a pid, never the harness's own memory, so the prediction is an inference and the
    /// line names the carrier that remains when it is wrong. A `settings` line discloses the
    /// cascade the gates and the slot census were read from: the scopes that contributed, the
    /// ones tried and absent, and the inputs that leave nothing on disk (a settings file or
    /// inline JSON handed to Claude Code on its command line, a restricted source set, the
    /// trust dialog, MDM). Takes any `@`-form csift accepts
    /// (`@<uuid>`, a uuid prefix, `@<agent-id>`, `@<Name>@<Team>`, `@main`, `@trap:<marker>`).
    /// The sender is the lane the SELF positional resolved, so a subagent passes `@trap:<marker>`
    /// for the answer a send from its own lane would get. With no positional the sender is the
    /// session the environment names, the top-level session in every lane, and a stderr note
    /// says the sender was assumed.
    #[arg(
        long = "to",
        value_name = "TARGET",
        conflicts_with = "peers",
        value_parser = parse_lane_target
    )]
    pub to: Option<String>,

    /// List every LIVE lane under the projects root as `id kind state` and nothing else. A
    /// description, an agent type or a name read as a role is exactly the material one lane
    /// would use to claim standing over another, so the census publishes none of it: it answers
    /// who is alive, not who should be obeyed. Live means the owning session has a registry row
    /// whose process answers a pid probe, and, for a child lane, its own tail is not settled (a
    /// child of a dead session cannot be live). A top-level lane's state is the registry's own
    /// word (`busy`/`shell`/`idle`/`waiting`); a child lane's is `in-flight`/`generating`.
    #[arg(long = "peers", conflicts_with = "self_target")]
    pub peers: bool,

    /// Emit JSON instead of text.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,
}
